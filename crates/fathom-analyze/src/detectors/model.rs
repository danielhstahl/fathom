//! `model.rs` — everything a detector is allowed to know, and nothing else.
//!
//! The ticket's phrase is "a pure function over the delta model", and this type is that model read by a
//! detector rather than by a surface. It is the same facts [`crate::insight::Input`] carries — the path
//! rows, their classes, their churn, the declared scope — plus the one thing the document deliberately
//! leaves out: **the lines**. ADR-001 §6.3 keeps file contents out of the delta payload, which is right
//! for a document that goes into an artefact and over MCP, and wrong for a detector whose whole subject
//! is the text of a hunk. So the lines arrive here, bounded, and nowhere else;
//! [`crate::insight::Supplied::findings`] carries a detector's *conclusions* into the document, never
//! its evidence text.
//!
//! # Why this is a separate type and not [`crate::insight::Input`]
//!
//! Two reasons, both practical. `Input` is a *published* document: adding a `Vec<String>` of hunk lines
//! to it would put content into every artefact and every MCP result whether or not anything asked for it,
//! and `check_delta_compat.py` would rightly call that a version bump. And `Input` describes one episode
//! in isolation, while half of this catalogue is about what happened *before* — a churn loop and a rework
//! loop are properties of a sequence, not of a frame. [`Corpus`] is the sequence.
//!
//! # Three states for content, and why the third matters
//!
//! [`Content::Full`] means every non-binary path's hunks were read. [`Content::Partial`] means some were
//! skipped (over the size cap, binary, a blob the object store would not give back) and the file rows say
//! which. [`Content::Absent`] means no content was read at all — a store written by an older build, or a
//! caller that only has the spine rollup. A content detector that reads `Absent` as "found nothing" is
//! the failure ADR-001 §7.7 refuses: an unknown printed as a zero. [`crate::detectors::Report`] turns
//! `Absent` into a stated gap naming the verb that would fill it, and the affected detectors report
//! `not examined` instead of silence.
//!
//! # Determinism
//!
//! No clock, no randomness, no `HashMap` iteration: the same corpus yields the same findings in the same
//! order, per ADR-001 §7.4. Ordering is the caller's job ([`crate::detectors::scan`] sorts) and every
//! collection here is a `Vec` in the order it was built.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::risk::{Surface, classify};

/// The change kinds, in the delta model's own spelling (`FileDelta::change`).
pub mod change {
    /// The path did not exist before this episode.
    pub const ADDED: &str = "added";
    /// The path existed and its bytes changed.
    pub const MODIFIED: &str = "modified";
    /// The path is gone after this episode.
    pub const DELETED: &str = "deleted";
    /// The path moved, with or without a change.
    pub const RENAMED: &str = "renamed";
    /// The path moved and a copy stayed behind.
    pub const COPIED: &str = "copied";
    /// Only the mode changed.
    pub const MODE_ONLY: &str = "mode_only";
    /// The index recorded the path and no change kind — a rollup read, not a per-commit one.
    pub const UNRECORDED: &str = "unrecorded";
}

/// How much of an episode's text this read saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Content {
    /// No hunks were read: the caller has rows, not text. Content detectors must say so, not hush.
    #[default]
    Absent,
    /// Every non-binary path's hunks were read.
    Full,
    /// Some paths' hunks were skipped; the file rows name which, in [`FileChange::note`].
    Partial,
}

impl Content {
    /// The published spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Content::Absent => "absent",
            Content::Full => "full",
            Content::Partial => "partial",
        }
    }

    /// Whether a detector that reads lines may claim to have looked.
    #[must_use]
    pub const fn examined(self) -> bool {
        !matches!(self, Content::Absent)
    }
}

/// One `@@` hunk, with only the lines a detector argues from.
///
/// [`Hunk::header`] is the stored `@@` line verbatim — git's own region identity, and the reason
/// [`Hunk::region`] can name a region without a parser. `added` and `removed` are the `+` and `-` lines
/// with their markers stripped, in file order, capped by the caller that read them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@ context` line, as git wrote it.
    #[serde(default)]
    pub header: String,
    /// The `+` lines, markers stripped, in file order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<String>,
    /// The `-` lines, markers stripped, in file order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
}

impl Hunk {
    /// Build a hunk from a header and its two sides.
    #[must_use]
    pub fn new(
        header: impl Into<String>,
        added: impl IntoIterator<Item = impl Into<String>>,
        removed: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Hunk {
            header: header.into(),
            added: added.into_iter().map(Into::into).collect(),
            removed: removed.into_iter().map(Into::into).collect(),
        }
    }

    /// The region this hunk touched, as a string two episodes can compare.
    ///
    /// git's own anchor when it has one — the function context after the second `@@` — because line
    /// numbers drift the moment anything above moves and a churn loop that loses its regions every time
    /// a comment is added is a churn loop that never fires. Without a context, the **new-side** start
    /// rounded down to a [`Self::REGION_BLOCK`]-line block: the old side is where the *previous* edit
    /// landed, and a region is a claim about where this one is. A block, stated as a block, and never a
    /// symbol the detector did not read.
    #[must_use]
    pub fn region(&self) -> String {
        if let Some(context) = self.context() {
            return context.to_owned();
        }
        let start = self
            .header
            .split_whitespace()
            .nth(2)
            .and_then(|side| side.trim_start_matches(['+', '-']).split(',').next())
            .and_then(|text| text.parse::<i64>().ok())
            .unwrap_or(0);
        format!("L{}", start / Self::REGION_BLOCK * Self::REGION_BLOCK)
    }

    /// The block size a context-free region is rounded to, in lines.
    pub const REGION_BLOCK: i64 = 40;

    /// The function context git printed after the `@@` pair, if it printed one.
    #[must_use]
    pub fn context(&self) -> Option<&str> {
        let (_, after) = self.header.rsplit_once("@@")?;
        let context = after.trim();
        if context.is_empty() {
            None
        } else {
            Some(context)
        }
    }

    /// Every line this hunk added, in file order.
    #[must_use]
    pub fn added(&self) -> &[String] {
        &self.added
    }

    /// Every line this hunk removed, in file order.
    #[must_use]
    pub fn removed(&self) -> &[String] {
        &self.removed
    }
}

/// One path's change within one episode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// The path, repo-relative and slash-separated, as the store spells it.
    pub path: String,
    /// The path it came from, when the index asserted a move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    /// One of [`change`]'s constants.
    #[serde(default = "default_change")]
    pub change: String,
    /// Lines added, as the rollup recorded them (the hunks may hold fewer, when they were capped).
    #[serde(default)]
    pub add: i64,
    /// Lines removed.
    #[serde(default)]
    pub del: i64,
    /// Whether any touch was binary.
    #[serde(default)]
    pub binary: bool,
    /// `e0.9`'s path class as stored, or `unknown`.
    #[serde(default)]
    pub path_class: String,
    /// `e0.9`'s noise level as stored, or `unknown`.
    #[serde(default)]
    pub noise_class: String,
    /// The hunks, when content was read. Empty with [`Content::Absent`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hunks: Vec<Hunk>,
    /// Why content is missing for this path (`oversize`, `binary`, `blob unavailable …`), if it is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

fn default_change() -> String {
    change::UNRECORDED.to_owned()
}

impl FileChange {
    /// A file row with no content — the shape a spine-only read produces.
    #[must_use]
    pub fn row(path: impl Into<String>, add: i64, del: i64) -> Self {
        FileChange {
            path: path.into(),
            change: change::MODIFIED.to_owned(),
            add,
            del,
            ..Default::default()
        }
    }

    /// Churn on this one path.
    #[must_use]
    pub fn churn(&self) -> i64 {
        self.add.saturating_add(self.del)
    }

    /// The reviewer-facing surface, from the shipped vocabulary — never a fourth one.
    #[must_use]
    pub fn surface(&self) -> Surface {
        classify(&self.path, &self.path_class, &self.noise_class)
    }

    /// Every added line across every hunk, in path order.
    #[must_use]
    pub fn added(&self) -> Vec<&str> {
        self.hunks
            .iter()
            .flat_map(|hunk| hunk.added())
            .map(String::as_str)
            .collect()
    }

    /// Every removed line across every hunk.
    #[must_use]
    pub fn removed(&self) -> Vec<&str> {
        self.hunks
            .iter()
            .flat_map(|hunk| hunk.removed())
            .map(String::as_str)
            .collect()
    }

    /// The regions this path was touched in, deduplicated and sorted.
    #[must_use]
    pub fn regions(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .hunks
            .iter()
            .map(Hunk::region)
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        out.sort();
        out
    }

    /// Whether the path moved (a rename or a copy).
    #[must_use]
    pub fn is_move(&self) -> bool {
        self.change == change::RENAMED || self.change == change::COPIED || self.old_path.is_some()
    }

    /// Whether the path was created inside the episode.
    #[must_use]
    pub fn is_new(&self) -> bool {
        self.change == change::ADDED
    }

    /// Whether the path is gone after the episode.
    #[must_use]
    pub fn is_delete(&self) -> bool {
        self.change == change::DELETED
    }

    /// Whether the surface is the test surface.
    #[must_use]
    pub fn is_test(&self) -> bool {
        self.surface() == Surface::Test
    }

    /// Whether the churn here is churn a review does not read: generated, lockfile, vendored, a
    /// reformat, or a binary. Discounted everywhere in this crate, discounted here too.
    #[must_use]
    pub fn noisy(&self) -> bool {
        self.binary
            || matches!(
                self.noise_class.as_str(),
                "generated" | "lockfile" | "vendored" | "reformat"
            )
            || matches!(
                self.surface(),
                Surface::Generated | Surface::Vendored | Surface::Dependency | Surface::Binary
            )
    }

    /// Whether this path's text was actually read. A `binary` path with no hunks is not "read and clean".
    #[must_use]
    pub fn examined(&self) -> bool {
        !self.binary && !self.hunks.is_empty()
    }

    /// The taxonomy label that best describes this path, for a message that has to name a source.
    #[must_use]
    pub fn taxonomy_label(&self) -> &str {
        for candidate in [&self.path_class, &self.noise_class] {
            if !candidate.is_empty() && candidate != "unknown" {
                return candidate.as_str();
            }
        }
        "generated by its path"
    }

    /// A clause saying how the path was touched, for a finding's message.
    #[must_use]
    pub fn change_kind_sentence(&self) -> &'static str {
        match self.change.as_str() {
            change::ADDED => "the file is new in this episode",
            change::DELETED => "it was deleted here",
            change::RENAMED => "it arrived by a move",
            change::COPIED => "it arrived as a copy",
            change::MODE_ONLY => "only its mode changed",
            _ => "it was modified here",
        }
    }

    /// The name of the file, without its directories.
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(self.path.as_str())
    }

    /// The file's extension, lower-cased and without the dot; `None` when it has none.
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.name()
            .rsplit_once('.')
            .map(|(_, ext)| ext)
            .filter(|ext| !ext.is_empty())
    }

    /// The directory segments, outermost first, excluding the file itself.
    #[must_use]
    pub fn dirs(&self) -> Vec<&str> {
        self.path
            .rsplit_once('/')
            .map(|(dir, _)| {
                dir.split('/')
                    .filter(|segment| !segment.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// One episode, as a detector sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    /// The derived id ADR-003 §2.1 gives the episode — the review's key, and what a finding is about.
    pub id: String,
    /// The one-line summary the rail prints.
    #[serde(default)]
    pub summary: String,
    /// `agent`, `human` or `unknown`, as the spine attributed it.
    #[serde(default)]
    pub author_kind: String,
    /// When the episode began, unix millis. `0` means the caller had no date, which is a stated
    /// unknown rather than 1970: [`Episode::dated`] says which.
    #[serde(default)]
    pub when_utc: i64,
    /// The ref the episode is ordered within.
    #[serde(default)]
    pub lineage_ref: String,
    /// Its ordinal on that ref.
    #[serde(default)]
    pub lineage_seq: i64,
    /// The harness session, when a trailer named one — two episodes in one session is a fact two of the
    /// loop detectors use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Member commits, counted.
    #[serde(default)]
    pub n_commits: i64,
    /// The scope somebody *declared* (`e4.2`), as globs. Empty means nothing was declared, which is not
    /// "nothing was allowed" — see [`crate::scope::Deviation::Unknown`].
    #[serde(default)]
    pub declared_scope: Vec<String>,
    /// The paths it touched, in path order.
    #[serde(default)]
    pub files: Vec<FileChange>,
    /// How much text came with the rows.
    #[serde(default)]
    pub content: Content,
    /// Why the text is partial or absent, in one clause, for the gap line.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_note: String,
    /// What the read had to skip, one clause each. Not a silent truncation: a scan that could not reach a
    /// blob says so where the findings are, and [`crate::detectors::collect::Collected`] lifts these into
    /// its own list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<String>,
}

impl Episode {
    /// An episode with the boilerplate filled in and no content — the shape a spine-only read builds.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Episode {
            id: id.into(),
            author_kind: "unknown".to_owned(),
            ..Default::default()
        }
    }

    /// An episode whose rows carry hunks.
    #[must_use]
    pub fn with_content(id: impl Into<String>, files: Vec<FileChange>) -> Self {
        Episode {
            id: id.into(),
            author_kind: "unknown".to_owned(),
            files,
            content: Content::Full,
            ..Default::default()
        }
    }

    /// The paths touched, in the order the rows hold them.
    #[must_use]
    pub fn paths(&self) -> Vec<&str> {
        self.files.iter().map(|file| file.path.as_str()).collect()
    }

    /// The rows on one surface, in path order.
    #[must_use]
    pub fn on(&self, surface: Surface) -> Vec<&FileChange> {
        self.files
            .iter()
            .filter(|file| file.surface() == surface)
            .collect()
    }

    /// Whether the episode touched the test surface at all — created, edited, renamed or deleted.
    #[must_use]
    pub fn test_activity(&self) -> bool {
        self.files.iter().any(FileChange::is_test)
    }

    /// Total churn.
    #[must_use]
    pub fn churn(&self) -> i64 {
        self.files
            .iter()
            .map(FileChange::churn)
            .fold(0_i64, i64::saturating_add)
    }

    /// Churn that is not noise — the part a reviewer was expected to read.
    #[must_use]
    pub fn informative_churn(&self) -> i64 {
        self.files
            .iter()
            .filter(|file| !file.noisy())
            .map(FileChange::churn)
            .fold(0_i64, i64::saturating_add)
    }

    /// Whether the read has a date to order by. `0` is "no date", not January 1970.
    #[must_use]
    pub const fn dated(&self) -> bool {
        self.when_utc != 0
    }

    /// Whether the change was authored by something other than a person. Unknown counts as *not*
    /// agent — a detector may not invent an attribution the spine did not earn.
    #[must_use]
    pub fn is_agent(&self) -> bool {
        self.author_kind == "agent"
    }

    /// Every added line in the episode, keyed by path, for the line-level detectors.
    #[must_use]
    pub fn added_lines(&self) -> Vec<(&str, &str, usize)> {
        let mut out = Vec::new();
        for file in &self.files {
            for (index, line) in file.added().into_iter().enumerate() {
                out.push((file.path.as_str(), line, index));
            }
        }
        out
    }

    /// Whether any row's text was read. `false` for [`Content::Absent`] and for a read where every path
    /// came back binary or oversized.
    #[must_use]
    pub fn content_examined(&self) -> bool {
        self.content.examined() && self.files.iter().any(FileChange::examined)
    }

    /// The declared scope's answer about this episode, in one clause, or `None` when nothing was
    /// declared. A convenience for surfaces that print a finding's reason without re-running
    /// [`crate::scope::deviation`].
    #[must_use]
    pub fn scope_clause(&self) -> Option<String> {
        crate::scope::deviation(&self.declared_scope, &self.paths()).summary(self.files.len())
    }
}

/// The episodes a detector may look back through.
///
/// A churn loop and a rework loop are properties of a *sequence*, which is the one thing a per-episode
/// document cannot answer. The corpus is borrowed, never owned: the caller already holds the read, and
/// the ordering is the caller's to decide (lineage ordinal, then date, then id) so that the same index
/// walked twice gives the same look-back.
#[derive(Debug, Clone, Copy, Default)]
pub struct Corpus<'a> {
    /// Every episode the caller has, oldest first.
    pub episodes: &'a [Episode],
}

impl Corpus<'_> {
    /// A corpus over a slice of episodes.
    #[must_use]
    pub const fn of(episodes: &[Episode]) -> Corpus<'_> {
        Corpus { episodes }
    }

    /// The episodes that came before `episode`, newest first, capped at `limit`.
    ///
    /// "Before" is ordinal when the lineage is known and date otherwise; an episode that has neither is
    /// compared by id, which is stable and says nothing about time — the same refusal ADR-003 §6 makes
    /// about ordering by array position when a ref is unknown.
    ///
    /// `episode` itself is never in the result, and neither is any episode with its id.
    #[must_use]
    pub fn prior(&self, episode: &Episode, limit: usize) -> Vec<&Episode> {
        let mut prior: Vec<&Episode> = self
            .episodes
            .iter()
            .filter(|other| {
                other.id != episode.id
                    && (other.lineage_ref != episode.lineage_ref || {
                        if episode.lineage_ref.is_empty() {
                            other.when_utc < episode.when_utc
                        } else {
                            other.lineage_seq < episode.lineage_seq
                        }
                    })
            })
            .collect();
        prior.sort_by(|a, b| {
            b.lineage_seq
                .cmp(&a.lineage_seq)
                .then(b.when_utc.cmp(&a.when_utc))
                .then(b.id.cmp(&a.id))
        });
        prior.truncate(limit);
        prior
    }

    /// Every other episode in the corpus, in corpus order, excluding `episode`.
    #[must_use]
    pub fn others(&self, episode: &Episode) -> Vec<&Episode> {
        self.episodes
            .iter()
            .filter(|other| other.id != episode.id)
            .collect()
    }

    /// How many episodes the corpus holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.episodes.len()
    }

    /// Is it empty? (`len` without `is_empty` is a clippy refusal, not a design decision.)
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.episodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn file(path: &str, add: i64, del: i64) -> FileChange {
        FileChange::row(path, add, del)
    }

    #[test]
    fn a_hunk_with_a_function_context_is_that_context() {
        let hunk = Hunk::new(
            "@@ -12,7 +14,9 @@ fn parse_token(",
            ["a"],
            Vec::<String>::new(),
        );
        assert_eq!(hunk.region(), "fn parse_token(");
        assert_eq!(hunk.context(), Some("fn parse_token("));
    }

    #[test]
    fn a_hunk_without_one_is_a_rounded_block_and_not_a_symbol() {
        let hunk = Hunk::new("@@ -12,7 +140,9 @@", ["a"], Vec::<String>::new());
        assert_eq!(hunk.region(), "L120");
        assert_eq!(hunk.context(), None);
    }

    #[test]
    fn the_two_anchorings_agree_that_a_moved_hunk_is_a_different_region_only_when_it_moved_far() {
        let before = Hunk::new(
            "@@ -10,3 +100,3 @@",
            Vec::<String>::new(),
            Vec::<String>::new(),
        );
        let nudged = Hunk::new(
            "@@ -10,3 +115,3 @@",
            Vec::<String>::new(),
            Vec::<String>::new(),
        );
        let moved = Hunk::new(
            "@@ -10,3 +400,3 @@",
            Vec::<String>::new(),
            Vec::<String>::new(),
        );
        assert_eq!(
            before.region(),
            nudged.region(),
            "a 15-line nudge is the same block"
        );
        assert_ne!(before.region(), moved.region());
    }

    #[test]
    fn a_binary_row_is_never_examined() {
        let mut binary = file("assets/logo.png", 0, 0);
        binary.binary = true;
        assert!(
            !binary.examined(),
            "a row with no text is not a row that was read and found clean"
        );
        let text = FileChange {
            path: "src/a.rs".to_owned(),
            hunks: vec![Hunk::new(
                "@@ -1,2 +1,3 @@",
                ["fn a() {}"],
                Vec::<String>::new(),
            )],
            ..Default::default()
        };
        assert!(text.examined());
    }

    #[test]
    fn surface_and_noise_come_from_the_shipped_vocabulary() {
        let test = file("tests/api.rs", 10, 0);
        assert!(test.is_test());
        let lock = FileChange {
            path: "Cargo.lock".to_owned(),
            noise_class: "lockfile".to_owned(),
            ..Default::default()
        };
        assert!(lock.noisy());
        assert!(!test.noisy());
    }

    #[test]
    fn an_absent_content_read_says_it_was_not_examined() {
        let mut episode = Episode::new("ep-1");
        episode.files = vec![file("src/a.rs", 4, 1)];
        assert!(!episode.content_examined(), "rows are not text");
        episode.content = Content::Full;
        assert!(
            !episode.content_examined(),
            "and full-content rows with no hunks on any path are still no text — the detectors say \
             `not examined` rather than `clean`"
        );
    }

    #[test]
    fn a_zero_date_is_an_unknown_date() {
        let undated = Episode::new("ep-1");
        let dated = Episode {
            when_utc: 1_770_000_000_000,
            ..Episode::new("ep-2")
        };
        assert!(!undated.dated());
        assert!(dated.dated());
    }

    #[test]
    fn look_back_is_by_ordinal_and_excludes_the_episode_itself() {
        let episodes: Vec<Episode> = (1..=5)
            .map(|n| Episode {
                id: format!("ep-{n}"),
                lineage_ref: "refs/heads/main".to_owned(),
                lineage_seq: n,
                when_utc: 1_000 * n,
                ..Episode::new(format!("ep-{n}"))
            })
            .collect();
        let corpus = Corpus::of(&episodes);
        let prior = corpus.prior(&episodes[3], 10);
        assert_eq!(
            prior
                .iter()
                .map(|episode| episode.id.as_str())
                .collect::<Vec<_>>(),
            ["ep-3", "ep-2", "ep-1"]
        );
        assert_eq!(
            corpus.prior(&episodes[3], 2).len(),
            2,
            "the window is a cap, not a suggestion"
        );
        assert_eq!(corpus.others(&episodes[0]).len(), 4);
    }

    #[test]
    fn look_back_falls_back_to_the_date_when_no_lineage_is_known() {
        let episodes = vec![
            Episode {
                id: "a".to_owned(),
                when_utc: 10,
                ..Episode::new("a")
            },
            Episode {
                id: "b".to_owned(),
                when_utc: 30,
                ..Episode::new("b")
            },
            Episode {
                id: "c".to_owned(),
                when_utc: 20,
                ..Episode::new("c")
            },
        ];
        let corpus = Corpus::of(&episodes);
        let prior = corpus.prior(&episodes[1], 10);
        assert_eq!(
            prior
                .iter()
                .map(|episode| episode.id.as_str())
                .collect::<Vec<_>>(),
            ["c", "a"]
        );
    }

    #[test]
    fn a_fixture_deserialises_from_json_because_that_is_what_a_fixture_should_be() {
        let json = r#"{
            "id": "ep-1",
            "summary": "add the parser",
            "author_kind": "agent",
            "declared_scope": ["src/parser/**"],
            "files": [
                {"path": "src/parser/mod.rs", "change": "modified", "add": 30, "del": 2,
                 "hunks": [{"header": "@@ -1,2 +1,30 @@ fn parse(", "added": ["fn parse() {}"]}]}
            ]
        }"#;
        let episode: Episode = serde_json::from_str(json).expect("the fixture shape parses");
        assert_eq!(episode.id, "ep-1");
        assert_eq!(episode.declared_scope, ["src/parser/**"]);
        assert_eq!(episode.files[0].change, change::MODIFIED);
        assert_eq!(episode.files[0].regions(), ["fn parse("]);
        assert_eq!(
            episode.content,
            Content::Absent,
            "content is a claim, not an assumption"
        );
    }
}

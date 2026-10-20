//! `collect.rs` — building [`Episode`]s from the index and the object store (`e4.3`).
//!
//! The detectors are pure, and the analysis layer's rule is that it opens no files of its own. The way
//! those two rules coexist is that this module is the only place in `detectors` holding a [`Store`] and a
//! [`Repository`], and it produces plain data. It is a thin reader on purpose: it reads what the index
//! already knows (`git_episode`, `git_episode_file`, `git_commit_file`) and renders the one thing those
//! tables do not store — the added and removed lines — from stored blob ids through
//! [`render_blobs`], the same rendering `fathom-cli`'s patch provider uses, so a detector and the diff
//! pane are arguing about identical bytes.
//!
//! # One row per path, net across the episode
//!
//! An episode is one to five commits and a review is of the *net* change, so the member commit rows are
//! folded the way [`fathom_cli`'s patch provider](../../../fathom-cli/src/patches.rs) folds them: the
//! before side comes from the first touch, the after side from the last, a rename moves the row to its
//! new name, and a path created and deleted inside the episode is dropped — there is nothing for a
//! detector to read there. The `add`/`del` figures, by contrast, come from `git_episode_file`, which
//! sums every commit: a file edited three times in one episode added lines three times, and a detector
//! pricing "20 new lines of public API" should count them all. Where the two disagree, the hunks hold
//! the net and the counts hold the sum, and [`Episode::content_note`] says which came from where.
//!
//! # The content state is set honestly
//!
//! [`Episode::content`] is [`Content::Full`] only when every path that could be read was read. A missing
//! blob, a blob over the cap, a `.gitattributes` driver that owns the file's text, a binary, or a path
//! list cut short by [`Reader::max_paths`] drops it to [`Content::Partial`] and names the path in the
//! note. Detectors price that state in — [`crate::detectors::Need::Content`] detectors report
//! `not examined` on [`Content::Absent`] rather than reporting clean — which is the difference between a
//! skipped blob and a clean repository (ADR-001 §7.7).

use std::collections::BTreeMap;

use fathom_ingest::diff::{Rendered, render_blobs};
use fathom_ingest::oid::Oid;
use fathom_ingest::record::{Limits, NotExamined};
use fathom_ingest::repo::Repository;
use fathom_store::episode::{EpisodeQuery, EpisodeRow};
use fathom_store::spine::{CommitRow, FileRow, Store};

use crate::detectors::model::{Content, Episode, FileChange, Hunk, change};

/// How many paths' content one episode read is allowed to render. A pathological episode touches ten
/// thousand paths; a scan that materialised all of them would be a scan that ran out of memory for a
/// finding nobody reads.
pub const DEFAULT_MAX_PATHS: usize = 400;

/// The reader: an index, its object store, and the limits both are allowed to work under.
#[derive(Debug)]
pub struct Reader<'a> {
    store: &'a Store,
    repo: &'a Repository,
    limits: Limits,
    max_paths: usize,
}

/// What a collection pass produced.
#[derive(Debug, Default)]
pub struct Collected {
    /// One [`Episode`] per index row, in the query's order.
    pub episodes: Vec<Episode>,
    /// One line per episode whose read was not complete, prefixed with the episode id. A surface prints
    /// these; it does not fold them away.
    pub notices: Vec<String>,
    /// Rows the query selected. Equal to [`Collected::episodes`] unless a read failed mid-way.
    pub rows: usize,
}

impl<'a> Reader<'a> {
    /// A reader over an index and its object store, with the ingest limits the run was configured with.
    #[must_use]
    pub fn new(store: &'a Store, repo: &'a Repository, limits: Limits) -> Self {
        Reader {
            store,
            repo,
            limits,
            max_paths: DEFAULT_MAX_PATHS,
        }
    }

    /// Override [`DEFAULT_MAX_PATHS`].
    #[must_use]
    pub fn with_max_paths(mut self, max_paths: usize) -> Self {
        self.max_paths = max_paths.max(1);
        self
    }

    /// One episode by id, or `None` when the index has no such episode.
    ///
    /// The index's per-path rollup is folded in exactly as [`Self::collect`] folds it in, so that the
    /// episode a verb reads alone — `fathom why`, `fathom export`, the queue's drill-down — is measured
    /// on the same per-path `add`/`del` the page read gives the rail, the queue and the Guardrail 3
    /// harness. [`Self::build`] fills paths, hunks and classes and nothing else: line counts come from the
    /// rollup, because the rollup is what the indexer reconciled against the diff. Read one episode
    /// without it and every per-path axis is measured against zero — `size` calls a lockfile regeneration
    /// "all of it hand-written", `noise` says nothing in the change is locked about the episode whose
    /// dependency axis is naming two lockfiles, and `test_ratio` reports that neither logic nor tests
    /// arrived about a diff that changed both.
    ///
    /// # Errors
    ///
    /// The store's own error when a read fails.
    pub fn one(&self, id: &str) -> Result<Option<Episode>, fathom_store::error::Error> {
        let Some(row) = self.store.episode(id)? else {
            return Ok(None);
        };
        let mut episode = self.build(&row)?;
        let rollup = self.store.episode_files(std::slice::from_ref(&row.id))?;
        if let Some(files) = rollup.by_episode.get(&row.id) {
            apply_rollup(&mut episode.files, files);
            if rollup.truncated {
                note(
                    &mut episode,
                    "the index's per-path rollup was capped by the store's read cap",
                );
                if episode.content == Content::Full {
                    episode.content = Content::Partial;
                }
            }
        }
        Ok(Some(episode))
    }

    /// Every episode a query selects.
    ///
    /// # Errors
    ///
    /// The store's error when a read fails. A failure on one episode's members is recorded as a notice
    /// and a partial episode rather than abandoning the pass: a scan of two hundred episodes that stops
    /// at the first missing object is a scan that never finishes.
    pub fn collect(&self, query: &EpisodeQuery) -> Result<Collected, fathom_store::error::Error> {
        let rows = self.store.episodes(query)?;
        let rollup = self
            .store
            .episode_files(&rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>())?;
        let mut out = Collected {
            episodes: Vec::with_capacity(rows.len()),
            notices: Vec::new(),
            rows: rows.len(),
        };
        for row in &rows {
            let mut episode = self.build(row)?;
            if let Some(files) = rollup.by_episode.get(&row.id) {
                apply_rollup(&mut episode.files, files);
                if rollup.truncated {
                    note(
                        &mut episode,
                        "the index's per-path rollup was capped by the store's read cap",
                    );
                    if episode.content == Content::Full {
                        episode.content = Content::Partial;
                    }
                }
            }
            for notice in std::mem::take(&mut episode.notices) {
                out.notices.push(format!("{}: {notice}", row.id));
            }
            out.episodes.push(episode);
        }
        Ok(out)
    }

    /// One episode's headline, its scope, and its content.
    fn build(&self, row: &EpisodeRow) -> Result<Episode, fathom_store::error::Error> {
        let mut episode = Episode {
            id: row.id.clone(),
            summary: row.summary.clone(),
            author_kind: row.author_kind.clone(),
            when_utc: row.first_utc,
            lineage_ref: row.lineage_ref.clone(),
            lineage_seq: row.lineage_seq,
            session_id: row.session_id.clone(),
            n_commits: row.n_commits(),
            declared_scope: declared_scope(&row.intent),
            content: Content::Absent,
            ..Episode::new(row.id.clone())
        };

        let mut missing: Vec<String> = Vec::new();
        let mut commits: Vec<CommitRow> = Vec::new();
        for member in &row.members {
            match self.store.read_commit(member) {
                Ok(commit) => commits.push(commit),
                Err(_) => missing.push(member.clone()),
            }
        }
        // Read every commit first, then borrow their rows: a `&FileRow` cannot outlive the `CommitRow` it
        // came from, and folding per path needs all of them at once.
        let mut rows: Vec<&FileRow> = Vec::new();
        for commit in &commits {
            rows.extend(commit.files.iter());
        }
        if !missing.is_empty() {
            note(
                &mut episode,
                &format!(
                    "{} member commit(s) have no file rows in the index ({}), so their hunks are missing",
                    missing.len(),
                    crate::detectors::named(
                        &missing.iter().map(String::as_str).collect::<Vec<_>>(),
                        3
                    )
                ),
            );
        }

        let mut paths: Vec<PathRow<'_>> = Vec::new();
        let mut index: BTreeMap<&str, usize> = BTreeMap::new();
        for file in rows {
            let slot = known(&mut index, file);
            let slot = match slot {
                Some(slot) => slot,
                None => {
                    paths.push(PathRow {
                        path: &file.new_path,
                        old_path: file.old_path.clone(),
                        added: file.change_kind == "add",
                        deleted: file.change_kind == "delete",
                        blob_old: file.blob_old.clone(),
                        blob_new: file.blob_new.clone(),
                        binary: file.is_binary,
                        path_class: file.path_class.clone(),
                        noise_class: file.noise_class.clone(),
                        moved: !matches!(file.change_kind.as_str(), "add" | "modify" | "delete"),
                        copied: file.change_kind == "copy",
                    });
                    let slot = paths.len() - 1;
                    index.insert(&file.new_path, slot);
                    slot
                }
            };
            let agg = &mut paths[slot];
            if agg.added {
                // A path created inside the episode has no before side, whatever a later row says.
                agg.blob_old = None;
            }
            if file.change_kind == "delete" {
                agg.deleted = true;
                agg.blob_new = None;
            } else {
                agg.deleted = false;
                agg.blob_new = file.blob_new.clone();
            }
            agg.binary |= file.is_binary;
            agg.path_class = file.path_class.clone();
            agg.noise_class = file.noise_class.clone();
            if let Some(old) = &file.old_path {
                agg.old_path = Some(agg.old_path.take().unwrap_or_else(|| old.clone()));
                agg.moved = true;
            }
        }

        paths.sort_by(|a, b| a.path.cmp(b.path));
        let mut read = 0_usize;
        let mut skipped = 0_usize;
        for (position, agg) in paths.iter().enumerate() {
            if agg.added && agg.deleted {
                // Created and gone inside one episode: no reviewer will ever find this path in either
                // tree, and a detector reading it would invent a file that does not exist.
                continue;
            }
            let mut file = FileChange {
                path: agg.path.to_owned(),
                old_path: agg.old_path.clone(),
                change: if agg.copied {
                    change::COPIED.to_owned()
                } else if agg.moved {
                    change::RENAMED.to_owned()
                } else if agg.added {
                    change::ADDED.to_owned()
                } else if agg.deleted {
                    change::DELETED.to_owned()
                } else {
                    change::MODIFIED.to_owned()
                },
                binary: agg.binary,
                path_class: agg.path_class.clone(),
                noise_class: agg.noise_class.clone(),
                ..Default::default()
            };
            if position >= self.max_paths {
                skipped += 1;
                file.note = format!(
                    "not read: over the {0}-path content cap for one episode",
                    self.max_paths
                );
                episode.files.push(file);
                continue;
            }
            if agg.binary {
                file.note = "binary: there is no line text to read".to_owned();
                episode.files.push(file);
                continue;
            }
            match self.hunks(agg.blob_old.as_deref(), agg.blob_new.as_deref()) {
                Ok(hunks) if hunks.is_empty() => {
                    // Same lines on both sides: a mode-only change, or a rename with no edit. Nothing to
                    // read, and nothing missing.
                    file.note = "no line change (a mode change or a move)".to_owned();
                    file.change = change::MODE_ONLY.to_owned();
                    episode.files.push(file);
                }
                Ok(hunks) => {
                    read += 1;
                    file.hunks = hunks;
                    episode.files.push(file);
                }
                Err(why) => {
                    skipped += 1;
                    file.note = why;
                    episode.files.push(file);
                }
            }
        }
        if skipped > 0 {
            note(
                &mut episode,
                &format!("{skipped} of {} path(s) had no readable text", paths.len()),
            );
        }
        episode.content = match (read, skipped) {
            (0, _) => Content::Absent,
            (_, 0) => Content::Full,
            _ => Content::Partial,
        };
        Ok(episode)
    }

    /// Render one (before, after) blob pair into hunks.
    fn hunks(&self, old_oid: Option<&str>, new_oid: Option<&str>) -> Result<Vec<Hunk>, String> {
        let old = parse_oid(old_oid)?;
        let new = parse_oid(new_oid)?;
        if old.is_none() && new.is_none() {
            return Err("the index stored no blob id for either side".to_owned());
        }
        match render_blobs(self.repo, old, new, &self.limits) {
            Ok(Rendered::Text { hunks }) => Ok(parse_hunks(&String::from_utf8_lossy(&hunks))),
            Ok(Rendered::Binary) => Err("binary by git's rule".to_owned()),
            Ok(Rendered::NotExamined { reason }) => Err(match reason {
                NotExamined::Oversized { size } => {
                    format!(
                        "over the {}-byte blob cap ({} bytes)",
                        self.limits.blob_memory_cap, size
                    )
                }
                NotExamined::Unavailable { why } => format!("the blob is {why}"),
                NotExamined::ExternalDiffDriver { driver } => {
                    format!("a `.gitattributes` diff driver (`{driver}`) owns this file's text")
                }
                NotExamined::NotABlob { found } => {
                    format!("the stored object is a {found}, not a file")
                }
            }),
            Err(error) => Err(format!("the object store refused the read ({error})")),
        }
    }
}

/// A path folded across the episode's commits: before side from the first touch, after side from the last.
#[derive(Debug, Clone)]
struct PathRow<'a> {
    path: &'a str,
    old_path: Option<String>,
    added: bool,
    deleted: bool,
    blob_old: Option<String>,
    blob_new: Option<String>,
    binary: bool,
    path_class: String,
    noise_class: String,
    moved: bool,
    copied: bool,
}

/// Which aggregated row this file row belongs to, rename-aware: a rename row moves the row to its new
/// name, so a path moved twice in one episode stays one row.
fn known<'a>(index: &mut BTreeMap<&'a str, usize>, file: &'a FileRow) -> Option<usize> {
    if let Some(slot) = index.get(file.new_path.as_str()).copied() {
        return Some(slot);
    }
    let old = file.old_path.as_deref()?;
    let slot = index.remove(old)?;
    index.insert(file.new_path.as_str(), slot);
    Some(slot)
}

/// Copy the index's summed per-path counts onto the folded rows.
fn apply_rollup(files: &mut [FileChange], rows: &[fathom_store::episode::EpisodeFile]) {
    for file in files.iter_mut() {
        if let Some(row) = rows.iter().find(|row| row.path == file.path) {
            file.add = row.add;
            file.del = row.del;
            file.binary |= row.binary;
            if file.path_class == "unknown" || file.path_class.is_empty() {
                file.path_class = row.path_class.clone();
            }
            if file.noise_class == "unknown" || file.noise_class.is_empty() {
                file.noise_class = row.noise_class.clone();
            }
        }
    }
}

/// Add a notice. They are gathered on the episode and lifted into [`Collected::notices`] by the caller, so
/// a surface that only has one episode still sees them.
fn note(episode: &mut Episode, text: &str) {
    episode.notices.push(text.to_owned());
}

/// The declared scope out of the intent document.
///
/// The column holds the whole `fathom_ingest::intent::Intent` as JSON. It is parsed into that type first,
/// because then a scope glob is read by the same code that normalised it, and only falls back to a loose
/// field lookup when the document is from a newer build than this one. Empty means *undeclared*, which is
/// [`crate::scope::Deviation::Unknown`] and not an empty allow-list.
fn declared_scope(intent: &str) -> Vec<String> {
    if intent.trim().is_empty() {
        return Vec::new();
    }
    if let Ok(parsed) = serde_json::from_str::<fathom_ingest::intent::Intent>(intent) {
        return parsed.scope;
    }
    serde_json::from_str::<serde_json::Value>(intent)
        .ok()
        .and_then(|value| {
            value.get("scope")?.as_array().map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::to_owned)
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// An index blob column into an oid. An unparseable column is a stated absence, never a crash: a store
/// written by a build that stored something else should produce fewer findings, not none.
fn parse_oid(hex: Option<&str>) -> Result<Option<Oid>, String> {
    match hex {
        None => Ok(None),
        Some(hex) => hex
            .parse::<Oid>()
            .map(Some)
            .map_err(|_| format!("the index names blob `{hex}`, which is not an oid")),
    }
}

/// Parse rendered unified-diff text into hunks. The renderer emits `@@` sections with no `diff`/`---`/
/// `+++` preamble, so a section boundary is all the structure there is; anything before the first `@@` is
/// dropped rather than guessed at.
fn parse_hunks(text: &str) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::new();
    for line in text.lines() {
        if line.starts_with("@@ ") || line.starts_with("@@-") {
            out.push(Hunk {
                header: line.to_owned(),
                added: Vec::new(),
                removed: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = out.last_mut() else { continue };
        if let Some(added) = line.strip_prefix('+') {
            hunk.added.push(added.to_owned());
        } else if let Some(removed) = line.strip_prefix('-') {
            hunk.removed.push(removed.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_hunk_body_splits_into_two_sides() {
        let text =
            "@@ -1,3 +1,4 @@ fn parse(\n let a = 1;\n-let b = 2;\n+let b = 3;\n+let c = 4;\n";
        let hunks = parse_hunks(text);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].header, "@@ -1,3 +1,4 @@ fn parse(");
        assert_eq!(hunks[0].region(), "fn parse(");
        assert_eq!(hunks[0].added, ["let b = 3;", "let c = 4;"]);
        assert_eq!(hunks[0].removed, ["let b = 2;"]);
    }

    #[test]
    fn text_before_the_first_header_is_not_invented_into_a_hunk() {
        assert!(parse_hunks("commit abc\n\ndiff --git a/x b/x\n").is_empty());
    }

    #[test]
    fn an_unparseable_blob_column_is_a_stated_absence() {
        assert!(
            parse_oid(Some("nothex"))
                .unwrap_err()
                .contains("not an oid")
        );
        assert_eq!(parse_oid(None).unwrap(), None);
    }

    #[test]
    fn an_empty_or_rubbish_intent_document_declares_no_scope() {
        assert!(declared_scope("").is_empty());
        assert!(declared_scope("{").is_empty());
        assert!(declared_scope(r#"{"state":"none"}"#).is_empty());
        assert_eq!(
            declared_scope(r#"{"scope":["src/parser/**"]}"#),
            ["src/parser/**"]
        );
    }

    #[test]
    fn a_rename_row_moves_the_aggregation() {
        let mut index: BTreeMap<&str, usize> = BTreeMap::new();
        let first = FileRow {
            path_hash: String::new(),
            new_path: "src/a.rs".to_owned(),
            old_path: None,
            change_kind: "modify".to_owned(),
            mode_old: None,
            mode_new: None,
            is_binary: false,
            n_add: 1,
            n_del: 0,
            hunk_headers: Vec::new(),
            blob_old: None,
            blob_new: None,
            path_class: "unknown".to_owned(),
            taxonomy_version: 0,
            noise_class: "unknown".to_owned(),
        };
        index.insert(first.new_path.as_str(), 0);
        let moved = FileRow {
            new_path: "src/b.rs".to_owned(),
            old_path: Some("src/a.rs".to_owned()),
            ..first.clone()
        };
        assert_eq!(known(&mut index, &moved), Some(0));
        assert!(index.contains_key("src/b.rs"));
        assert!(
            !index.contains_key("src/a.rs"),
            "one path, one row, under its last name"
        );
    }
}

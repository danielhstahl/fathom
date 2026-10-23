//! `export::record` — the one structure every artefact format renders (`e5.7`).
//!
//! The ticket names six things — the summary card, the dependency and architecture deltas, findings with
//! evidence, the operator-action checklist, the sign-off block, and links to commits — and the tempting
//! way to build an artefact is to print each of them on the way past. That is how a PR comment, an audit
//! page and a JSON payload end up disagreeing, which is the bug ADR-001 §7.10 exists to name. So this file
//! is the artefact and the renderers are its projections: [`Record`] is built once, checked once, and then
//! printed three ways by modules that are allowed to differ in layout and in nothing else.
//!
//! # Byte-stability is a structural property, not a cleanup pass
//!
//! A review artefact is committed, diffed and blessed. Two exports of the same range must be the same
//! bytes, so the things that make bytes jitter are excluded by construction rather than scrubbed after:
//!
//! * **no clock.** The only instants in a record are instants the repository already recorded — a commit's
//!   date, a sign-off's stamp. Nothing asks the wall clock what time it is, which is also why a record
//!   cannot say "signed 3 days ago": that sentence changes every day and would make the artefact a
//!   screenshot of the afternoon it was written.
//! * **no absolute paths.** [`Record::provenance`] carries the redacted store path the caller supplied, and
//!   [`Record::verify`] fails a document whose provenance fields hold one.
//! * **no hash-order iteration and no randomness.** Every collection is sorted by [`build`], and
//!   [`Record::verify`] re-checks the order it promises. Keys are printed by serde in field-declaration
//!   order; the maps that matter are [`BTreeMap`]s.
//!
//! # What the record does *not* contain
//!
//! **No second analysis.** Every number is a copy of something the owning analysis already produced: the
//! episode sections are [`crate::insight`]’s delta documents, verbatim; the dependency section is
//! `e2.6`’s headline and counts, handed over by the caller; the findings are `e4.3`’s, with their own
//! evidence. The two arithmetic acts in this module — summing the per-episode totals into a range roll-up,
//! and bucketing the file rows by directory — add the document's own figures and cite the rows they came
//! from. A renderer that recomputed a figure would be a second opinion, which is the thing the narrow waist
//! was built to prevent.
//!
//! **No verdict about staleness.** The ledger's relink ladder answers "is what was signed still what is
//! here?" and it is `fathom review`'s to print; the record copies that answer when the caller supplies it
//! ([`SignOffRow::content_matches`]) and does not re-derive it, because a staleness verdict computed
//! against "now" is exactly the clock dependency this module refuses.

#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::insight::{self, EpisodeDelta, Gap, STATE_MEASURED};
use crate::narrative;

/// The contract id of the artefact's structure.
///
/// Independent of [`crate::insight::CONTRACT`] on purpose: the delta is the evidence, the record is a
/// review document, and a change to the layout of a document is not a change to the model behind it.
pub const CONTRACT: &str = "fathom-review-record/1";

/// The version of [`CONTRACT`].
pub const VERSION: u32 = 1;

/// Who writes records, printed in the artefact itself.
///
/// The narrative has its own stamp ([`crate::narrative::GENERATOR`], `fathom-template/1`) and it stays
/// inside the narrative: a reader has to be able to tell which sentences the record laid out and which the
/// composer wrote, and one watermark over both would blur the line `e4.7` drew on purpose.
pub const GENERATOR: &str = "fathom-review-record/1";

/// The order episodes are printed in, said out loud because the artefact claims it.
pub const EPISODE_ORDER: &str = "oldest episode first, by first member commit, then by episode id";

/// The order the checklists' open items are printed in.
pub const CHECKLIST_ORDER: &str = "open items first, then by kind, then by subject";

/// The largest number of rows a rendered list prints before the remainder is counted, not listed.
///
/// An artefact that runs to nine hundred lines is not read, and an artefact that silently truncates is a
/// lie, so every capped list in the renderers prints `… and N more` beside the count that produced it.
pub const LIST_CAP: usize = 40;

/// A commit the artefact links to.
///
/// `url` is the only link the artefact invents, and it is not invented: it is the repository's own remote
/// plus the commit's own id, assembled by the caller and left `None` when there is no remote to
/// derive it from. The subject is carried so the link text is the commit's, never a guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRef {
    /// The full commit id, lowercase hex.
    pub oid: String,
    /// The seven-character spelling the prose prints.
    pub short: String,
    /// The first line of the commit message, as committed.
    pub subject: String,
    /// The author as committed.
    pub author: String,
    /// The author's address as committed.
    pub email: String,
    /// Committer date, unix milliseconds.
    pub utc: i64,
    /// The web link, when the repository's remote makes one derivable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The episode this commit belongs to, in this record.
    pub episode: String,
}

impl CommitRef {
    /// The stamp to print beside a date, from the milliseconds the repository recorded.
    #[must_use]
    pub fn stamp(&self) -> String {
        crate::ledger::entry::iso_utc(self.utc)
    }
}

/// One finding, copied out of the model that raised it.
///
/// This is `e4.3`'s finding with the fields an artefact has room for; the evidence list is the detector's
/// own locators, and [`FindingRow::suppress`] is the key that silences it, so the reader who disagrees has
/// something to type rather than something to accept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingRow {
    /// The episode it is about.
    pub episode: String,
    /// The catalogue id, e.g. `scope_creep`.
    pub kind: String,
    /// The path, region or test it is about.
    pub subject: String,
    /// The claim, in one sentence a person wrote.
    pub message: String,
    /// `high`, `medium` or `low` — the detector's own vocabulary.
    pub severity: String,
    /// `0..=1`, or `None` when the finding is a measured fact rather than an inferred one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// `locator — detail` lines, in the detector's order, strongest first.
    pub evidence: Vec<String>,
    /// The ticket that owns the rule.
    pub owner: String,
    /// The key that suppresses this finding, and the verb that writes it.
    pub suppress: String,
    /// The view the surface opens to look at it, e.g. `diff:ep-…#src/x.rs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<String>,
}

/// One sign-off, as the ledger wrote it, plus the two things only the ledger can say.
///
/// The AC asks for reviewer, time, scope and dismissals with reasons. `scope` is not one field: a sign-off
/// covers an episode identity, four digests, a member set and a path set, and every one of them is a thing
/// a later reader may want to disagree with. `reason_label` is the dismiss vocabulary's own words for the
/// code the reviewer chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignOffRow {
    /// The entry id (`sg-…`).
    pub id: String,
    /// The ledger file it was read from, repo-relative.
    pub file: String,
    /// The episode it covers.
    pub episode: String,
    /// `accepted`, `flagged`, `dismissed`, `snoozed`.
    pub decision: String,
    /// Whether this decision is the one the north-star metric counts (`accepted` only).
    pub counts_as_review: bool,
    /// Who signed.
    pub reviewer: String,
    /// `human` or `agent`, as the ledger recorded it.
    pub reviewer_kind: String,
    /// When, unix milliseconds — the instant the *ledger* holds, never the clock of the export.
    pub signed_utc: i64,
    /// The dismiss/snooze code, when the decision used one.
    pub reason_code: String,
    /// The shipped vocabulary's words for [`SignOffRow::reason_code`].
    pub reason_label: String,
    /// The reviewer's own words.
    pub note: String,
    /// The member commits the sign-off covered.
    pub members: Vec<String>,
    /// How many members the entry did not print.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_remainder: Option<u64>,
    /// Files and content digests the entry covered.
    pub paths: usize,
    /// Commits, files and churn covered — the `scale:` line, split out.
    pub scale: SignOffScale,
    /// The episode identity digests, labelled, in the order the ledger writes them.
    pub digests: Vec<(String, String)>,
    /// The entry's own tamper digest.
    pub digest: String,
    /// Whether the content signed is the content here now, when the caller could answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_matches: Option<bool>,
    /// The reason for [`SignOffRow::content_matches`], in the ledger's own words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_note: Option<String>,
    /// The entry's own body lines, as the file holds them.
    pub body: Vec<(String, String)>,
}

/// The scale a sign-off covered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignOffScale {
    /// Commits covered.
    pub commits: i64,
    /// Files covered.
    pub files: i64,
    /// Lines added.
    pub add: i64,
    /// Lines removed.
    pub del: i64,
}

/// One recorded note, as the review memory wrote it (`e6.5`, `reco-552`).
///
/// The sign-off says a person accepted a change; this says what they wrote down while doing it — the
/// decision and its reason, the question nobody has answered, the follow-up somebody owes. Both halves are
/// in the artefact because a reviewer asked to look at a range needs both: the gate and the memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRow {
    /// The note id (`nt-…`).
    pub id: String,
    /// The memory file it was read from, repo-relative.
    pub file: String,
    /// The episode it is about *now*, after the re-link ladder — empty when it is anchored to a path.
    pub episode: String,
    /// `decision`, `question`, `followup`, `annotation`.
    pub kind: String,
    /// `open`, `answered`, `withdrawn`.
    pub status: String,
    /// Who wrote it.
    pub author: String,
    /// `human` or `agent`, as the file recorded it.
    pub author_kind: String,
    /// When, unix milliseconds — the instant the memory holds, never the clock of the export.
    pub written_utc: i64,
    /// The words, as written.
    pub text: String,
    /// The note this one answers, when it answers one.
    pub about: String,
    /// The notes that answer this one.
    pub answers: Vec<String>,
    /// `fresh`, `stale`, `orphan` or `anchored` — the ladder's verdict.
    pub link_state: String,
    /// Which rung of the ladder attached it.
    pub link_method: String,
    /// The episode it was written against, when a rewrite moved it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relinked_from: Option<String>,
    /// Why it is stale or orphaned, in the ladder's own words.
    pub why: String,
    /// The path and line it points at, as written.
    pub target: String,
    /// Which door wrote it.
    pub via: String,
    /// The episode identity digests the note carries, labelled, in the order the file writes them.
    pub digests: Vec<(String, String)>,
    /// The note's own tamper digest.
    pub digest: String,
    /// Whether its text still hashes to [`MemoryRow::digest`].
    pub tampered: bool,
}

/// One row of the committed technical-debt ledger, as the artefact shows it (`e3.10`, `reco-5ht`).
///
/// **There is no clock in here, and that is deliberate.** Whether an allowance "expires soon" is a question
/// about today, and a review artefact is a committed, diffed, blessed document: a field that changed value
/// every midnight would make two exports of the same range differ for a reason nobody reviewed. So the row
/// carries the *dates the ledger itself holds* — class M state, in the file, stable until someone edits it
/// — and the record compares them against the newest commit it carries, which is also recorded state. That
/// answer is dated to the change under review rather than to the afternoon the artefact was printed, which
/// is the answer a review can actually act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowanceRow {
    /// `cycle` or `layer` — which engine's debt this is.
    pub kind: String,
    /// The key, as the ledger spells it.
    pub key: String,
    /// The named human or team who owes the payoff.
    pub owner: String,
    /// The sentence that justified the exception.
    pub reason: String,
    /// The day it stops forgiving, as the file spelled it; `None` is a permanent allowance and is printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    /// The day it was granted, when the file knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted: Option<String>,
    /// Who wrote the row.
    pub granted_by: String,
}

impl AllowanceRow {
    /// The one-line rendering every format uses.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}/{} — owner {}, runs out {}, granted {} by {}: {}",
            self.kind,
            self.key,
            if self.owner.is_empty() {
                "nobody"
            } else {
                &self.owner
            },
            self.expires.clone().unwrap_or_else(|| "never".to_owned()),
            self.granted
                .clone()
                .unwrap_or_else(|| "unstated".to_owned()),
            if self.granted_by.is_empty() {
                "unstated"
            } else {
                &self.granted_by
            },
            self.reason
        )
    }

    /// Whether the row's own day had gone by at `horizon` (unix milliseconds), compared as the ISO dates the
    /// file itself uses. A row with no expiry has none to run out.
    #[must_use]
    pub fn lapsed_by(&self, horizon: Option<i64>) -> bool {
        let (Some(expires), Some(horizon)) = (self.expires.as_ref(), horizon) else {
            return false;
        };
        // `iso_utc` prints `YYYY-MM-DD…`, and ISO dates order correctly as strings, which is the whole of the
        // arithmetic this needs — no calendar, no time zone, no clock.
        expires.as_str() <= crate::ledger::entry::iso_utc(horizon).as_str()
    }
}

/// The committed ledger and the artefact's own date, handed to the checklist as the pair they are.
///
/// "Has this allowance run out?" is meaningless without a *by when*, and inside a record the only honest *by
/// when* is the newest commit the record carries (`Record::horizon`) rather than the wall. Bundling the two
/// also keeps [`checklist_of`] under the argument ceiling clippy names — which is a sign, not the reason.
#[derive(Debug, Clone, Default)]
pub struct Debts<'a> {
    /// The rows, in `(kind, key)` order.
    pub rows: &'a [AllowanceRow],
    /// The artefact's date: the newest commit's timestamp, or `None` when it carries no commits.
    pub horizon: Option<i64>,
}

impl Debts<'_> {
    /// The rows whose own day had gone by by the horizon — the ones the checklist asks a human about.
    pub fn lapsed(&self) -> impl Iterator<Item = &AllowanceRow> {
        self.rows
            .iter()
            .filter(move |row| row.lapsed_by(self.horizon))
    }
}

/// One card row: a label and the value the model printed for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardRow {
    /// The section id the row came from.
    pub id: String,
    /// `measured`, `partial` or `unavailable`.
    pub state: String,
    /// The `x` / `~` / `?` marker, so the state survives a plain-text rendering.
    pub glyph: char,
    /// The value, already worded by the model.
    pub value: String,
    /// The plan key that owns the answer, printed for the `?` rows.
    pub owner: String,
}

impl CardRow {
    /// `~ what` — the marker and the id, the two columns that never change width.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} {}", self.glyph, self.id)
    }
}

/// One episode's half of the artefact.
///
/// The whole delta document travels, because the JSON artefact is the record *itself* and a machine that
/// reads it should get the evidence and not a paraphrase of it. The markdown and HTML renderers print a
/// chosen subset of the same document; they do not hold a copy of any of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeView {
    /// The versioned delta document, verbatim.
    pub delta: EpisodeDelta,
    /// The `e4.7` narrative — the exact bytes `fathom narrative --markdown` prints.
    pub narrative_markdown: String,
    /// The summary card: one row per section, in the model's own order.
    pub card: Vec<CardRow>,
    /// The findings raised on this episode.
    pub findings: Vec<FindingRow>,
    /// The commits that make it up, in the episode's own order.
    pub commits: Vec<CommitRef>,
    /// The entry that accepted it, if one is in scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_by: Option<String>,
    /// The newest decision recorded for it, in scope: `accepted`, `flagged`, `dismissed`, `snoozed`, or
    /// `unsigned` when the ledger holds no decision at all.
    pub decision: String,
}

impl EpisodeView {
    /// The episode's risk line, or the honest absence.
    #[must_use]
    pub fn risk_line(&self) -> String {
        self.delta.risk.as_ref().map_or_else(
            || "unranked".to_owned(),
            |risk| {
                if risk.rank.is_some() {
                    format!("{} · {}", risk.headline(), risk.place())
                } else {
                    risk.headline()
                }
            },
        )
    }

    /// The lines the summary card's row prints for this episode.
    #[must_use]
    pub fn headline(&self) -> String {
        let totals = &self.delta.totals;
        format!(
            "+{} −{} · {} file(s)",
            totals.n_add, totals.n_del, totals.n_files
        )
    }
}

/// What was exported.
///
/// The range is echoed as it was asked *and* as it resolved, because "the artefact covers less than I
/// asked for" is the failure a reviewer cannot see after the fact, and `resolved` is where an export that
/// quietly ran out of index says so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    /// The range as the operator typed it.
    pub asked: String,
    /// The resolved start, short form, when it resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// The resolved end, short form, when it resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Whether both ends resolved to commits this repository has.
    pub resolved: bool,
    /// Member commits counted.
    pub commits: usize,
    /// Episodes counted.
    pub episodes: usize,
    /// The episodes' ids, in printing order.
    pub episode_ids: Vec<String>,
    /// The contract of the delta documents this record was built from.
    pub delta_contract: String,
    /// Their version.
    pub delta_version: u32,
    /// The order episodes are printed in ([`EPISODE_ORDER`]).
    pub episode_order: String,
    /// The state of each of the seven sections across these episodes, as `measured/partial/unavailable`
    /// counts — the artefact's own honest table of what it can and cannot answer.
    pub section_states: Vec<SectionState>,
}

/// How much of one section this record actually answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionState {
    /// The section id.
    pub id: String,
    /// The question it answers, in the model's words.
    pub question: String,
    /// The ticket that owns a full answer.
    pub owner: String,
    /// How many episodes answered it fully, partly, not at all.
    pub measured: usize,
    /// See [`SectionState::measured`].
    pub partial: usize,
    /// See [`SectionState::measured`].
    pub unavailable: usize,
}

/// The dependency half of the delta, for the whole range.
///
/// `e2.6`'s answer, handed over by the caller and reduced to the rows a reviewer reads. `None` in the
/// record means the caller could not run the analysis (no tree to compare against, no index), and the
/// artefact prints that refusal instead of an empty section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependencies {
    /// The owning analysis' own one-sentence answer.
    pub headline: String,
    /// What the answer was computed from.
    pub basis: String,
    /// The plan key that owns it.
    pub owner: String,
    /// The counts that matter, in the analysis' own vocabulary.
    pub counts: BTreeMap<String, i64>,
    /// The changed packages: `ecosystem name kind before → after`.
    pub changes: Vec<String>,
    /// How many changes the list above did not print.
    pub change_remainder: usize,
    /// New sources of packages — a new registry, a git URL, a vendored tree.
    pub new_sources: Vec<String>,
    /// Sources that left.
    pub sources_gone: Vec<String>,
    /// Peer-dependency conflicts introduced.
    pub peer_conflicts: Vec<String>,
    /// Licences new to the graph, and licences gone.
    pub licenses_added: Vec<String>,
    /// See [`Dependencies::licenses_added`].
    pub licenses_removed: Vec<String>,
    /// Whether both sides of the comparison were read completely.
    pub complete: bool,
    /// What could not be determined, in the analysis' own words.
    pub notes: Vec<String>,
    /// Dependency files folded into the comparison.
    pub files: usize,
    /// The two trees the comparison ran between, as short commit ids.
    pub between: String,
}

/// The architecture half of the delta, for the whole range.
///
/// Built from the file rows the delta documents already carry: which directories the range landed in, what
/// moved, what disappeared, and which operator surfaces it touched. Every figure here is a sum or a bucket
/// of rows printed elsewhere in the same document, and [`Architecture::basis`] says which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Architecture {
    /// The plan key that owns the folder-shape answer.
    pub owner: String,
    /// What these rows are a roll-up of.
    pub basis: String,
    /// Directories touched: `path`, files, add, del, biggest single file.
    pub directories: Vec<DirectoryRow>,
    /// Moves the index recorded: `(old, new, episode)`.
    pub renames: Vec<RenameRow>,
    /// Deleted paths.
    pub deleted: Vec<String>,
    /// Binary paths.
    pub binary: Vec<String>,
    /// The operator surfaces the range touched, most consequential first.
    pub surfaces: Vec<String>,
    /// How many rows were path-classed as generated, vendored or lockfile noise.
    pub noise_rows: usize,
    /// Whether any episode's file read was capped by the store, which bounds every number above.
    pub truncated: bool,
    /// What this roll-up cannot see, in the model's own words.
    pub blind_spots: Vec<String>,
}

/// One directory's share of the range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryRow {
    /// The directory, `.` for the repository root.
    pub path: String,
    /// File rows under it.
    pub files: usize,
    /// Lines added under it.
    pub add: i64,
    /// Lines removed under it.
    pub del: i64,
    /// The largest row under it, `path (+n −n)`.
    pub largest: String,
}

/// One move the index asserted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameRow {
    /// Where it was.
    pub from: String,
    /// Where it is.
    pub to: String,
    /// The episode that moved it.
    pub episode: String,
    /// `full` or `rollup`, which is the difference between a recorded move and an inferred one.
    pub basis: String,
}

/// One line of the operator-action checklist.
///
/// The checklist is the artefact's only imperative mood, so every item names who owns the rest of it and
/// what to type. An item with no `how` says so instead of inventing a command; an item already `done` says
/// which sign-off closed it, so the list doubles as the audit of what was decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistItem {
    /// A stable id, derived from the item's kind and subject rather than from a counter.
    pub id: String,
    /// `review`, `finding`, `sign-off`, `ledger` or `surface` — what kind of thing this asks for.
    pub kind: String,
    /// `open` or `done`.
    pub state: String,
    /// The ask, in one line.
    pub text: String,
    /// The detail that makes it checkable, when the ask is not enough on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// What to type, when there is a command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub how: Option<String>,
    /// The plan key that owns what is missing.
    pub owner: String,
    /// The episode this is about, when it is about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<String>,
    /// The entry that closed it, when it is done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_by: Option<String>,
    /// Where the evidence is.
    pub evidence: Vec<String>,
}

impl ChecklistItem {
    /// Is this item still asking for something?
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.state == STATE_OPEN
    }

    /// `[x]` / `[ ]` — the markdown checkbox, in the plain-text renderings too.
    #[must_use]
    pub fn box_label(&self) -> &'static str {
        match self.state.as_str() {
            STATE_OPEN => "[ ]",
            _ => "[x]",
        }
    }
}

/// `open`: still asking for a human.
pub const STATE_OPEN: &str = "open";
/// `done`: closed by a decision the ledger holds.
pub const STATE_DONE: &str = "done";

/// The roll-up the summary card prints.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Episodes, commits.
    pub episodes: usize,
    /// See [`Summary::episodes`].
    pub commits: usize,
    /// Files touched across the range (the sum of the episode totals).
    pub files: i64,
    /// Lines added.
    pub add: i64,
    /// Lines removed.
    pub del: i64,
    /// Churn.
    pub churn: i64,
    /// Churn a reviewer is expected to read, after the noise discount.
    pub reviewable_churn: i64,
    /// How many episodes have an accepted sign-off in scope.
    pub signed_off: usize,
    /// How many have no decision at all.
    pub unsigned: usize,
    /// How many were dismissed, flagged or snoozed.
    pub dismissed: usize,
    /// See [`Summary::dismissed`].
    pub flagged: usize,
    /// See [`Summary::dismissed`].
    pub snoozed: usize,
    /// Findings, and how many of them are `high`.
    pub findings: usize,
    /// See [`Summary::findings`].
    pub high_findings: usize,
    /// Checklist items still open.
    pub open_actions: usize,
    /// The highest-risk episode, as `ep-… · spine-1 88 = …`, when anything was ranked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_risk: Option<String>,
    /// The one-line verdict a reader acts on.
    pub verdict: String,
}

/// Where the record's rows came from — the artefact's own provenance, with no clock in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordProvenance {
    /// [`GENERATOR`].
    pub generator: String,
    /// `fathom <version>`, so an artefact says which build wrote it.
    pub produced_by: String,
    /// The redacted store path the rows came from, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    /// The index epoch the rows came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<i64>,
    /// The verb that was typed, echoed rather than reconstructed.
    pub asked_with: String,
    /// The contracts this record is built out of, so a reader can pin every one of them.
    pub contracts: BTreeMap<String, String>,
}

/// The record's verdict on itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Integrity {
    /// Whether [`Record::verify`] found nothing.
    pub verified: bool,
    /// One line per problem, empty when `verified`.
    pub problems: Vec<String>,
    /// What was checked, so `verified` is never a bare boolean.
    pub checks: Vec<String>,
}

/// What was checked, printed with every artefact (`e2.11`'s rule that a self-check names itself).
pub const CHECKS: [&str; 8] = [
    "every episode's delta document verifies, and every episode is in the stated scope",
    "episodes, findings, sign-offs and checklist items are printed in the order the record claims",
    "every narrative is the bytes the composer produces for the same document",
    "every finding belongs to an episode in scope and carries at least one piece of evidence",
    "every sign-off belongs to an episode in scope and carries its tamper digest",
    "every closed checklist item names the entry that closed it",
    "no absolute path, no generated timestamp and no control character is in the record",
    "the summary's counts are the counts of the rows it summarises",
];

/// The review artefact: one document, six sections, three renderings.
///
/// Build it with [`build`] and print it with [`super::render`]. Nothing in here is a rendering decision:
/// every field is a fact the caller supplied or an arithmetic consequence of what it supplied, and the
/// renderers are allowed to differ from each other only in layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// [`CONTRACT`].
    pub contract: String,
    /// [`VERSION`].
    pub schema_version: u32,
    /// [`GENERATOR`].
    pub generator: String,
    /// What was exported, and how completely it resolved.
    pub scope: Scope,
    /// The roll-up a reader acts on.
    pub summary: Summary,
    /// One entry per episode, oldest first.
    pub episodes: Vec<EpisodeView>,
    /// The dependency delta for the range, or the refusal that stood in for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Dependencies>,
    /// The architecture delta for the range.
    pub architecture: Architecture,
    /// Every finding in scope, in (severity, kind, subject) order.
    pub findings: Vec<FindingRow>,
    /// The operator-action checklist: open items first.
    pub checklist: Vec<ChecklistItem>,
    /// The sign-off block, oldest stamp first.
    pub signoffs: Vec<SignOffRow>,
    /// What the ledger reported about itself: a torn line, a digest that does not match.
    pub ledger_problems: Vec<String>,
    /// The review memory in scope — decisions, questions, follow-ups, annotations — oldest first.
    pub memory: Vec<MemoryRow>,
    /// What the memory files reported about themselves (`e6.5`).
    pub memory_problems: Vec<String>,
    /// The committed technical-debt allowances, in `(kind, key)` order (`e3.10`, `reco-5ht`).
    pub debts: Vec<AllowanceRow>,
    /// Every commit the artefact links to, oldest first.
    pub commits: Vec<CommitRef>,
    /// Every gap in the delta documents, in episode then section order.
    pub gaps: Vec<Gap>,
    /// Where the rows came from.
    pub provenance: RecordProvenance,
    /// The self-check.
    pub integrity: Integrity,
}

/// One episode's supplied half of [`build`].
#[derive(Debug, Clone)]
pub struct EpisodeInput {
    /// The delta document, already composed by [`crate::insight`].
    pub delta: EpisodeDelta,
    /// The findings raised on it, from the detector report.
    pub findings: Vec<FindingRow>,
    /// Its member commits, in the episode's own order.
    pub commits: Vec<CommitRef>,
}

/// Everything [`build`] is allowed to know.
///
/// The caller — `fathom-cli`, which is the only thing in the workspace with a store handle, a git
/// repository and a clock it is allowed to look at — supplies the rows; this module supplies the shape,
/// the order and the arithmetic, and never asks when it is.
#[derive(Debug, Clone)]
pub struct Input {
    /// The range as typed.
    pub asked: String,
    /// The resolved endpoints, short form.
    pub from: Option<String>,
    /// See [`Input::from`].
    pub to: Option<String>,
    /// Whether both endpoints resolved.
    pub resolved: bool,
    /// The episodes in scope, in any order — [`build`] sorts them.
    pub episodes: Vec<EpisodeInput>,
    /// The commits in scope, in any order — [`build`] sorts them.
    pub commits: Vec<CommitRef>,
    /// The dependency delta, or `None` with the refusal printed in its place.
    pub dependencies: Option<Dependencies>,
    /// The sign-offs in scope, as ledger entries.
    pub signoffs: Vec<SignOffRow>,
    /// What the ledger reported about its own files.
    pub ledger_problems: Vec<String>,
    /// The notes the review memory holds for the range, in any order — [`build`] sorts them (`e6.5`).
    pub memory: Vec<MemoryRow>,
    /// What the memory files reported about their own contents.
    pub memory_problems: Vec<String>,
    /// The committed allowance ledger, as the ratchet reads it — in any order, [`build`] sorts it
    /// (`e3.10`, `reco-5ht`).
    pub debts: Vec<AllowanceRow>,
    /// `fathom <version>`.
    pub produced_by: String,
    /// The redacted store path.
    pub store: Option<String>,
    /// The index epoch.
    pub epoch: Option<i64>,
    /// The command line that asked, echoed rather than reconstructed.
    pub asked_with: String,
    /// Gaps the *export* itself owes — a capped findings pass, a delta that could not be computed — in
    /// addition to the ones the delta documents carry. An artefact that left out a third of its own
    /// findings and said nothing would be reporting zero where nobody looked.
    pub extra_gaps: Vec<Gap>,
}

/// Compose the review record.
///
/// This is the whole product surface: no store, no git, no clock, no config. Deterministic by
/// construction — every collection it returns is sorted by a comparator that names no wall time.
#[must_use]
pub fn build(input: &Input) -> Record {
    let mut episodes: Vec<EpisodeView> = input
        .episodes
        .iter()
        .map(|given| EpisodeView {
            narrative_markdown: narrative_markdown(&given.delta),
            card: card_of(&given.delta),
            decision: decision_of(&given.delta.episode.id, &input.signoffs).to_owned(),
            accepted_by: accepted_by(&given.delta.episode.id, &input.signoffs).map(str::to_owned),
            delta: given.delta.clone(),
            findings: given.findings.clone(),
            commits: given.commits.clone(),
        })
        .collect();
    episodes.sort_by_key(episode_key);

    let mut commits = input.commits.clone();
    commits.sort_by(|left, right| (left.utc, &left.oid).cmp(&(right.utc, &right.oid)));

    let mut findings: Vec<FindingRow> = episodes
        .iter()
        .flat_map(|episode| episode.findings.iter().cloned())
        .collect();
    findings.sort_by_key(finding_key);

    let mut signoffs = input.signoffs.clone();
    signoffs.sort_by(|left, right| (left.signed_utc, &left.id).cmp(&(right.signed_utc, &right.id)));
    // Oldest note first, the order the memory file appends in: an artefact that re-ordered the memory would
    // read like a narrative the reviewer never wrote.
    let mut memory = input.memory.clone();
    memory.sort_by(|left, right| (left.written_utc, &left.id).cmp(&(right.written_utc, &right.id)));
    // The committed allowance ledger, in the order the ratchet's own file lists rows in once they are sorted.
    // This is a copy of class-M state, not a second opinion: the record never decides what is forgiven.
    let mut debts = input.debts.clone();
    debts.sort_by(|left, right| (&left.kind, &left.key).cmp(&(&right.kind, &right.key)));
    // The artefact's own date, taken from a commit rather than from the wall: "had this allowance run out by
    // the time of the change under review?" is answerable from what the record already holds, and a question
    // about today is not, without making the bytes of a blessed document change at midnight.
    let horizon = input.commits.iter().map(|commit| commit.utc).max();
    let architecture = architecture_of(&episodes);
    let checklist = checklist_of(
        &episodes,
        &signoffs,
        &findings,
        &memory,
        &input.ledger_problems,
        &input.memory_problems,
        &Debts {
            rows: &debts,
            horizon,
        },
    );
    let mut gaps: Vec<Gap> = episodes
        .iter()
        .flat_map(|episode| episode.delta.gaps.iter().cloned())
        .chain(input.extra_gaps.iter().cloned())
        .collect();
    // One row per *kind* of gap, not per episode that hit it: seven honest gaps become fourteen
    // duplicated lines on a two-episode range, and a list that repeats itself is a list that gets
    // skimmed past. The first occurrence keeps its place, in episode then section order.
    {
        let mut seen: Vec<(String, String, String)> = Vec::with_capacity(gaps.len());
        gaps.retain(|gap| {
            let key = (gap.section.clone(), gap.short.clone(), gap.reason.clone());
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        });
    }
    let section_states = section_states_of(&episodes);
    let scope = Scope {
        asked: input.asked.clone(),
        from: input.from.clone(),
        to: input.to.clone(),
        resolved: input.resolved,
        commits: commits.len(),
        episodes: episodes.len(),
        episode_ids: episodes
            .iter()
            .map(|episode| episode.delta.episode.id.clone())
            .collect(),
        delta_contract: episodes.first().map_or_else(
            || insight::CONTRACT.to_owned(),
            |episode| episode.delta.contract.clone(),
        ),
        delta_version: episodes.first().map_or(insight::SCHEMA_VERSION, |episode| {
            episode.delta.schema_version
        }),
        episode_order: EPISODE_ORDER.to_owned(),
        section_states,
    };

    let mut record = Record {
        contract: CONTRACT.to_owned(),
        schema_version: VERSION,
        generator: GENERATOR.to_owned(),
        summary: Summary::default(),
        scope,
        episodes,
        dependencies: input.dependencies.clone(),
        architecture,
        findings,
        checklist,
        signoffs,
        ledger_problems: input.ledger_problems.clone(),
        memory,
        memory_problems: input.memory_problems.clone(),
        debts,
        commits,
        gaps,
        provenance: RecordProvenance {
            generator: GENERATOR.to_owned(),
            produced_by: input.produced_by.clone(),
            store: input.store.clone(),
            epoch: input.epoch,
            asked_with: input.asked_with.clone(),
            contracts: contracts(),
        },
        integrity: Integrity {
            verified: false,
            problems: Vec::new(),
            checks: CHECKS.iter().map(|check| (*check).to_owned()).collect(),
        },
    };
    record.summary = summary_of(&record);
    let problems = record.verify();
    record.integrity = Integrity {
        verified: problems.is_empty(),
        problems,
        checks: CHECKS.iter().map(|check| (*check).to_owned()).collect(),
    };
    record
}

/// The narrative for one episode — the exact bytes `fathom narrative --markdown` prints.
///
/// One function, two callers, so `e4.7`'s last acceptance criterion ("the narrative is the text used by
/// the export artefact") is a call graph rather than an intention.
#[must_use]
pub fn narrative_markdown(delta: &EpisodeDelta) -> String {
    narrative::compose(delta, &narrative::Options::new()).to_markdown()
}

/// The card rows: the model's own [`insight::brief`], re-typed so the record owns its serialisation.
#[must_use]
pub fn card_of(delta: &EpisodeDelta) -> Vec<CardRow> {
    insight::brief(delta)
        .into_iter()
        .map(|row| CardRow {
            id: row.id,
            state: row.state,
            glyph: row.glyph,
            value: row.value,
            owner: row.owner,
        })
        .collect()
}

/// The order episodes print in.
fn episode_key(episode: &EpisodeView) -> (i64, String) {
    (
        episode.delta.episode.first_utc,
        episode.delta.episode.id.clone(),
    )
}

/// The order findings print in: worst first, then alphabetically, so a re-run cannot shuffle them.
fn finding_key(finding: &FindingRow) -> (u8, String, String, String) {
    (
        match finding.severity.as_str() {
            "high" => 0,
            "medium" => 1,
            "low" => 2,
            _ => 3,
        },
        finding.kind.clone(),
        finding.subject.clone(),
        finding.episode.clone(),
    )
}

/// The order checklist items print in ([`CHECKLIST_ORDER`]).
fn checklist_key(item: &ChecklistItem) -> (u8, String, String) {
    (
        if item.is_open() { 0 } else { 1 },
        item.kind.clone(),
        item.id.clone(),
    )
}

/// The newest decision the ledger holds for one episode, in scope.
fn decision_of<'a>(episode: &str, signoffs: &'a [SignOffRow]) -> &'a str {
    signoffs
        .iter()
        .filter(|row| row.episode == episode)
        .max_by(|left, right| (left.signed_utc, &left.id).cmp(&(right.signed_utc, &right.id)))
        .map_or("unsigned", |row| row.decision.as_str())
}

/// The entry that accepted one episode, in scope, newest first.
fn accepted_by<'a>(episode: &str, signoffs: &'a [SignOffRow]) -> Option<&'a str> {
    signoffs
        .iter()
        .filter(|row| row.episode == episode && row.counts_as_review)
        .max_by(|left, right| (left.signed_utc, &left.id).cmp(&(right.signed_utc, &right.id)))
        .map(|row| row.id.as_str())
}

/// The seven questions, counted across the episodes in scope.
fn section_states_of(episodes: &[EpisodeView]) -> Vec<SectionState> {
    insight::SectionId::ALL
        .iter()
        .map(|id| {
            let mut state = SectionState {
                id: id.as_str().to_owned(),
                question: id.question().to_owned(),
                owner: id.owner().to_owned(),
                measured: 0,
                partial: 0,
                unavailable: 0,
            };
            for episode in episodes {
                let Some(section) = episode.delta.section(*id) else {
                    state.unavailable += 1;
                    continue;
                };
                match section.state.as_str() {
                    STATE_MEASURED => state.measured += 1,
                    "partial" => state.partial += 1,
                    _ => state.unavailable += 1,
                }
            }
            state
        })
        .collect()
}

/// Bucket the file rows the delta documents carry into the architecture half of the artefact.
///
/// Nothing is measured here: the directories are the rows' own first path segments, the numbers are sums of
/// the rows' own add/del, and the surfaces are the ranking's own ordering. [`Architecture::basis`] says
/// exactly that, because a roll-up that does not say what it rolled up is a second opinion.
fn architecture_of(episodes: &[EpisodeView]) -> Architecture {
    let mut directories: BTreeMap<String, (usize, i64, i64, i64, String)> = BTreeMap::new();
    let mut renames: Vec<RenameRow> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut binary: Vec<String> = Vec::new();
    let mut surfaces: Vec<String> = Vec::new();
    let mut blind_spots: Vec<String> = Vec::new();
    let mut noise_rows = 0_usize;
    let mut truncated = false;

    for episode in episodes {
        let id = &episode.delta.episode.id;
        truncated |= episode.delta.totals.files_truncated;
        for file in &episode.delta.files {
            let directory = directory_of(&file.path);
            let entry = directories
                .entry(directory)
                .or_insert((0, 0, 0, 0, String::new()));
            entry.0 += 1;
            entry.1 += file.add;
            entry.2 += file.del;
            if file.churn() >= entry.3 {
                entry.3 = file.churn();
                entry.4 = format!("{} (+{} −{})", file.path, file.add, file.del);
            }
            if let Some(old) = &file.old_path {
                renames.push(RenameRow {
                    from: old.clone(),
                    to: file.path.clone(),
                    episode: id.clone(),
                    basis: file.basis.clone(),
                });
            }
            if file.change == "deleted" {
                deleted.push(format!("{id} {}", file.path));
            }
            if file.binary {
                binary.push(format!("{id} {}", file.path));
            }
            if !matches!(
                file.noise_class.as_str(),
                "" | "clean" | "none" | "unknown" | "source"
            ) {
                noise_rows += 1;
            }
        }
        if let Some(risk) = &episode.delta.risk {
            for surface in &risk.surfaces {
                if !surfaces.contains(surface) {
                    surfaces.push(surface.clone());
                }
            }
            for spot in &risk.blind_spots {
                if !blind_spots.contains(spot) {
                    blind_spots.push(spot.clone());
                }
            }
        }
    }

    let directories = directories
        .into_iter()
        .map(
            |(path, (files, add, del, _largest_churn, largest))| DirectoryRow {
                path,
                files,
                add,
                del,
                largest,
            },
        )
        .collect();

    Architecture {
        owner: "e2.3".to_owned(),
        basis: "the per-file rows the delta documents carry, bucketed by first path segment and summed \
                from the rows' own add/del"
            .to_owned(),
        directories,
        renames,
        deleted,
        binary,
        surfaces,
        noise_rows,
        truncated,
        blind_spots,
    }
}

/// The first path segment, or `.` for a file at the repository root.
fn directory_of(path: &str) -> String {
    match path.split_once('/') {
        Some((first, _)) if !first.is_empty() => first.to_owned(),
        _ => ".".to_owned(),
    }
}

/// The operator-action checklist: what this artefact asks of a human, and what has already been answered.
///
/// Six sources, in the order the ticket names them: an episode with no decision, a finding with no
/// disposition, a surface that needs doing outside the review, a sign-off whose content has moved, a
/// ledger that reported a problem, and — as a closed item — the decision that already closed an episode.
/// Every closed item names the entry that closed it, which is what lets the same list serve as the audit.
fn checklist_of(
    episodes: &[EpisodeView],
    signoffs: &[SignOffRow],
    findings: &[FindingRow],
    memory: &[MemoryRow],
    ledger_problems: &[String],
    memory_problems: &[String],
    debts: &Debts<'_>,
) -> Vec<ChecklistItem> {
    let mut items: Vec<ChecklistItem> = Vec::new();

    for episode in episodes {
        let id = episode.delta.episode.id.as_str();
        let entry = episode
            .accepted_by
            .clone()
            .or_else(|| newest_entry(id, signoffs).map(|row| row.id.clone()));
        let closed = episode.decision != "unsigned";
        items.push(ChecklistItem {
            id: format!("review:{id}"),
            kind: "review".to_owned(),
            state: if closed { STATE_DONE } else { STATE_OPEN }.to_owned(),
            text: if closed {
                format!("{} — decided `{}`", id, episode.decision)
            } else {
                format!("Decide {}: {}", id, episode.delta.episode.summary)
            },
            detail: Some(format!(
                "{} · {} · risk {}",
                episode.delta.episode.author_kind,
                episode.headline(),
                episode.risk_line()
            )),
            how: if closed {
                None
            } else {
                Some(format!("fathom review sign {id}"))
            },
            owner: "e4.6".to_owned(),
            episode: Some(id.to_owned()),
            closed_by: if closed { entry } else { None },
            evidence: vec![format!("episode {id}")],
        });
    }

    for finding in findings {
        let closed_by = episodes
            .iter()
            .find(|episode| episode.delta.episode.id == finding.episode)
            .and_then(|episode| episode.accepted_by.clone());
        let closed = closed_by.is_some();
        items.push(ChecklistItem {
            id: format!("finding:{}:{}", finding.kind, finding.subject),
            kind: "finding".to_owned(),
            state: if closed { STATE_DONE } else { STATE_OPEN }.to_owned(),
            text: format!("{} on `{}`: {}", finding.kind, finding.subject, finding.message),
            detail: Some(format!(
                "{} · {} · confidence {} · the rule is owned by {}; the key that silences it is `{}`",
                finding.episode,
                finding.severity,
                finding
                    .confidence
                    .map_or_else(|| "not stated".to_owned(), |value| format!("{value:.2}")),
                finding.owner,
                finding.suppress
            )),
            how: finding
                .jump
                .as_ref()
                .map(|jump| format!("open the `{jump}` view")),
            owner: finding.owner.clone(),
            episode: Some(finding.episode.clone()),
            closed_by,
            evidence: finding.evidence.clone(),
        });
    }

    for episode in episodes {
        let Some(section) = episode.delta.section(insight::SectionId::Surface) else {
            continue;
        };
        for figure in &section.figures {
            if figure.label != "operator surfaces" || !figure.known {
                continue;
            }
            if figure.value.starts_with("none") {
                continue;
            }
            items.push(ChecklistItem {
                id: format!("surface:{}:{}", episode.delta.episode.id, figure.value),
                kind: "surface".to_owned(),
                state: STATE_OPEN.to_owned(),
                text: format!(
                    "Someone outside this review has to act: {} ({})",
                    figure.value, episode.delta.episode.id
                ),
                detail: Some(figure.label.clone()),
                how: figure.evidence.run.clone(),
                owner: section
                    .gap
                    .as_ref()
                    .map_or_else(|| section.owner.clone(), |gap| gap.owner.clone()),
                episode: Some(episode.delta.episode.id.clone()),
                closed_by: None,
                evidence: evidence_lines(&figure.evidence),
            });
        }
    }

    for row in signoffs {
        if row.content_matches == Some(true) || row.content_matches.is_none() {
            continue;
        }
        items.push(ChecklistItem {
            id: format!("sign-off:{}", row.id),
            kind: "sign-off".to_owned(),
            state: STATE_OPEN.to_owned(),
            text: format!(
                "Re-sign {}: the content moved after {} signed it on {}",
                row.episode,
                row.reviewer,
                crate::ledger::entry::iso_utc(row.signed_utc)
            ),
            detail: row.content_note.clone(),
            how: Some(format!("fathom review sign {}", row.episode)),
            owner: "e4.6".to_owned(),
            episode: Some(row.episode.clone()),
            closed_by: None,
            evidence: vec![row.id.clone(), row.file.clone()],
        });
    }

    for problem in ledger_problems {
        items.push(ChecklistItem {
            id: format!("ledger:{problem}"),
            kind: "ledger".to_owned(),
            state: STATE_OPEN.to_owned(),
            text: format!("The ledger needs a fix: {problem}"),
            detail: None,
            how: Some("fathom review verify".to_owned()),
            owner: "e4.6".to_owned(),
            episode: None,
            closed_by: None,
            evidence: vec![problem.clone()],
        });
    }
    // An unanswered question or follow-up is an open item, in the same sense an unsigned episode is: the
    // artefact is not finished while somebody's question is still sitting in the memory file (`e6.5`).
    for row in memory {
        if row.status != "open" || !matches!(row.kind.as_str(), "question" | "followup") {
            continue;
        }
        items.push(ChecklistItem {
            id: format!("memory:{}", row.id),
            kind: "memory".to_owned(),
            state: STATE_OPEN.to_owned(),
            text: format!("Answer the {} {}: {}", row.kind, row.id, row.text),
            detail: Some(format!(
                "{} asked {} · about {} · recorded in {}",
                row.author,
                crate::ledger::entry::iso_utc(row.written_utc),
                if row.episode.is_empty() {
                    row.target.as_str()
                } else {
                    row.episode.as_str()
                },
                row.file
            )),
            how: Some(format!(
                "fathom note answer {} \"the answer, in one line\"",
                row.id
            )),
            owner: "e6.5".to_owned(),
            episode: (!row.episode.is_empty()).then(|| row.episode.clone()),
            closed_by: None,
            evidence: vec![row.id.clone(), row.file.clone()],
        });
    }

    for problem in memory_problems {
        items.push(ChecklistItem {
            id: format!("memory-problem:{problem}"),
            kind: "memory".to_owned(),
            state: STATE_OPEN.to_owned(),
            text: format!("The review memory needs a fix: {problem}"),
            detail: None,
            how: Some("fathom note verify".to_owned()),
            owner: "e6.5".to_owned(),
            episode: None,
            closed_by: None,
            evidence: vec![problem.clone()],
        });
    }

    // An allowance whose own day had gone by before the change under review is an open item: the exception
    // written down has stopped covering the violation, and whether anybody noticed is not something the
    // ledger knows (`e3.10`, `reco-5ht`). The date is compared to the newest commit in the record and never
    // to today, because an artefact that changed which items it listed at midnight is not a document anyone
    // would bless.
    for row in debts.lapsed() {
        items.push(ChecklistItem {
            id: format!("debt:{}:{}", row.kind, row.key),
            kind: "debt".to_owned(),
            state: STATE_OPEN.to_owned(),
            text: format!(
                "The allowance for `{}` ran out on {}; {} still owes it: {}",
                row.key,
                row.expires.clone().unwrap_or_default(),
                if row.owner.is_empty() {
                    "nobody"
                } else {
                    &row.owner
                },
                row.reason
            ),
            detail: Some(row.render()),
            how: Some(
                "`fathom ratchet --update-baseline --reason \"…\"` re-grants it with a new date; paying the \
                 violation off instead takes the row out on the next run"
                    .to_owned(),
            ),
            owner: "e3.10".to_owned(),
            episode: None,
            closed_by: None,
            evidence: vec![format!("{}:{}", row.kind, row.key)],
        });
    }

    items.sort_by_key(checklist_key);
    items
}

/// The newest entry recorded for one episode, whatever it decided.
fn newest_entry<'a>(episode: &str, signoffs: &'a [SignOffRow]) -> Option<&'a SignOffRow> {
    signoffs
        .iter()
        .filter(|row| row.episode == episode)
        .max_by(|left, right| (left.signed_utc, &left.id).cmp(&(right.signed_utc, &right.id)))
}

/// The four spellings one piece of evidence takes in the delta model, as lines an artefact can print.
fn evidence_lines(evidence: &insight::Evidence) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(pointer) = &evidence.pointer {
        out.push(format!("pointer {pointer}"));
    }
    if let Some(run) = &evidence.run {
        out.push(format!("run `{run}`"));
    }
    if let Some(owner) = &evidence.owner {
        out.push(format!("owned by {owner}"));
    }
    out
}

/// The roll-up, computed from the rows it summarises and nowhere else.
fn summary_of(record: &Record) -> Summary {
    let mut summary = Summary {
        episodes: record.episodes.len(),
        commits: record.scope.commits,
        ..Summary::default()
    };
    let mut best_score: Option<i64> = None;
    for episode in &record.episodes {
        let totals = &episode.delta.totals;
        summary.files += totals.n_files;
        summary.add += totals.n_add;
        summary.del += totals.n_del;
        summary.churn += totals.churn;
        summary.reviewable_churn += totals.reviewable_churn;
        match episode.decision.as_str() {
            "accepted" => summary.signed_off += 1,
            "unsigned" => summary.unsigned += 1,
            "dismissed" => summary.dismissed += 1,
            "flagged" => summary.flagged += 1,
            "snoozed" => summary.snoozed += 1,
            _ => summary.unsigned += 1,
        }
        if let Some(risk) = &episode.delta.risk
            && best_score.is_none_or(|best| risk.score > best)
        {
            best_score = Some(risk.score);
            summary.top_risk = Some(format!(
                "{} · {}",
                episode.delta.episode.id,
                risk.headline()
            ));
        }
    }
    summary.findings = record.findings.len();
    summary.high_findings = record
        .findings
        .iter()
        .filter(|finding| finding.severity == "high")
        .count();
    summary.open_actions = record
        .checklist
        .iter()
        .filter(|item| item.is_open())
        .count();
    summary.verdict = verdict_of(&summary);
    summary
}

/// The one sentence the summary card ends with.
fn verdict_of(summary: &Summary) -> String {
    if summary.episodes == 0 {
        return "Nothing in this range reached the index, so there is nothing to review."
            .to_owned();
    }
    let mut parts = vec![format!(
        "{} of {} episode(s) carry an accepted sign-off",
        summary.signed_off, summary.episodes
    )];
    if summary.unsigned > 0 {
        parts.push(format!("{} undecided", summary.unsigned));
    }
    let refusals = summary.dismissed + summary.flagged + summary.snoozed;
    if refusals > 0 {
        parts.push(format!("{} refused or deferred", refusals));
    }
    if summary.high_findings > 0 {
        parts.push(format!("{} high finding(s)", summary.high_findings));
    }
    parts.push(if summary.open_actions == 0 {
        "no actions open".to_owned()
    } else {
        format!("{} action(s) open", summary.open_actions)
    });
    parts.join(" · ")
}

/// Every contract this record is built out of, so a reader can pin them one by one.
#[must_use]
pub fn contracts() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("record".to_owned(), format!("{CONTRACT}/v{VERSION}")),
        (
            "delta".to_owned(),
            format!("{}/v{}", insight::CONTRACT, insight::SCHEMA_VERSION),
        ),
        (
            "narrative".to_owned(),
            format!("{}/v{}", narrative::CONTRACT, narrative::VERSION),
        ),
        (
            "ledger".to_owned(),
            crate::ledger::entry::REVIEW_SCHEMA.to_owned(),
        ),
    ])
}

impl Record {
    /// The artefact's own date: the newest commit it carries, in unix milliseconds (`e3.10`, `reco-5ht`).
    ///
    /// This is the only "when" a record is allowed to answer with, and it is answered from committed state
    /// rather than from the wall — which is what lets a dated question like *had this allowance run out by the
    /// time of this change?* be asked of a blessed artefact without the answer changing overnight.
    #[must_use]
    pub fn horizon(&self) -> Option<i64> {
        self.commits.iter().map(|commit| commit.utc).max()
    }

    /// The serialised form, keys in field order, byte-stable.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }

    /// The serialised form, indented — the shape `--format json` writes, because an artefact a machine
    /// reads is also an artefact a human diffs in a PR.
    #[must_use]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| String::from("{}"))
    }

    /// Read a record back. The JSON artefact is the record, so this is the round trip's other half.
    ///
    /// # Errors
    ///
    /// `serde_json::Error` when the bytes are not a record.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The episodes in scope, by id — the set every other section is checked against.
    #[must_use]
    pub fn episodes_in(&self) -> Vec<&str> {
        self.episodes
            .iter()
            .map(|episode| episode.delta.episode.id.as_str())
            .collect()
    }

    /// The artefact's verdict on itself: one line per thing that does not hold, empty when it all does.
    ///
    /// The checks are [`CHECKS`], and they are the reason the three renderings can be believed to be the
    /// same document: a record that shuffled its own order, lost a finding's evidence, carried a delta that
    /// does not verify, or leaked a host path says so in the artefact itself, on the same line as the prose
    /// that would otherwise have been trusted.
    #[must_use]
    pub fn verify(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.contract != CONTRACT {
            problems.push(format!(
                "the record claims contract `{}`, and this build writes `{CONTRACT}`",
                self.contract
            ));
        }
        if self.schema_version != VERSION {
            problems.push(format!(
                "the record claims version {}, and this build writes {VERSION}",
                self.schema_version
            ));
        }

        let in_scope = self.episodes_in();
        let scope_ids: Vec<&str> = self.scope.episode_ids.iter().map(String::as_str).collect();
        if scope_ids != in_scope {
            problems
                .push("the stated scope does not list the episodes the record holds".to_owned());
        }
        if self.scope.episodes != self.episodes.len() {
            problems.push(format!(
                "the scope says {} episode(s) and the record holds {}",
                self.scope.episodes,
                self.episodes.len()
            ));
        }
        if self.scope.commits != self.commits.len() {
            problems.push(format!(
                "the scope says {} commit(s) and the record links {}",
                self.scope.commits,
                self.commits.len()
            ));
        }

        // Episodes: their own self-checks, their order, and the prose's identity with the composer.
        let keys: Vec<(i64, String)> = self.episodes.iter().map(episode_key).collect();
        if !keys.windows(2).all(|pair| pair[0] <= pair[1]) {
            problems.push(format!(
                "episodes are not printed in the order claimed: {EPISODE_ORDER}"
            ));
        }
        let mut ids: Vec<&str> = Vec::with_capacity(self.episodes.len());
        for episode in &self.episodes {
            if ids.contains(&episode.delta.episode.id.as_str()) {
                problems.push(format!(
                    "episode {} appears twice",
                    episode.delta.episode.id
                ));
            }
            ids.push(&episode.delta.episode.id);
            for problem in episode.delta.verify() {
                problems.push(format!("{}: {}", episode.delta.episode.id, problem));
            }
            if episode.narrative_markdown != narrative_markdown(&episode.delta) {
                problems.push(format!(
                    "{}: the narrative in the artefact is not the bytes the composer produces",
                    episode.delta.episode.id
                ));
            }
            if episode.card != card_of(&episode.delta) {
                problems.push(format!(
                    "{}: the card rows are not the rows the model's brief produces",
                    episode.delta.episode.id
                ));
            }
            if episode.decision == "accepted" && episode.accepted_by.is_none() {
                problems.push(format!(
                    "{}: the decision is `accepted` and no entry is named as the acceptance",
                    episode.delta.episode.id
                ));
            }
        }

        // Findings: owned, evidenced, ordered.
        let finding_keys: Vec<_> = self.findings.iter().map(finding_key).collect();
        if !finding_keys.windows(2).all(|pair| pair[0] <= pair[1]) {
            problems
                .push("findings are not printed worst-first, then by kind and subject".to_owned());
        }
        for finding in &self.findings {
            if !in_scope.contains(&finding.episode.as_str()) {
                problems.push(format!(
                    "finding {} on {} is held against {}, which is no episode in scope",
                    finding.kind, finding.subject, finding.episode
                ));
            }
            if finding.evidence.is_empty() {
                problems.push(format!(
                    "finding {} on {} carries no evidence",
                    finding.kind, finding.subject
                ));
            }
        }

        // Sign-offs: owned, hashed, ordered.
        let signoff_keys: Vec<(i64, &String)> = self
            .signoffs
            .iter()
            .map(|row| (row.signed_utc, &row.id))
            .collect();
        if !signoff_keys.windows(2).all(|pair| pair[0] <= pair[1]) {
            problems.push("sign-offs are not printed oldest stamp first".to_owned());
        }
        let oids: Vec<&str> = self
            .commits
            .iter()
            .map(|commit| commit.oid.as_str())
            .collect();
        for row in &self.signoffs {
            let owned = in_scope.contains(&row.episode.as_str())
                || row.members.iter().any(|oid| oids.contains(&oid.as_str()));
            if !owned {
                problems.push(format!(
                    "sign-off {} covers {}, which is neither in scope nor shares a commit with it",
                    row.id, row.episode
                ));
            }
            if row.digest.is_empty() {
                problems.push(format!("sign-off {} carries no tamper digest", row.id));
            }
        }

        // Memory: owned, hashed, ordered, and vocabulary-bound (`e6.5`).
        let memory_keys: Vec<(i64, &String)> = self
            .memory
            .iter()
            .map(|row| (row.written_utc, &row.id))
            .collect();
        if !memory_keys.windows(2).all(|pair| pair[0] <= pair[1]) {
            problems.push("the review memory is not printed oldest note first".to_owned());
        }
        for row in &self.memory {
            // A note is in scope when it is about an episode in scope, when it is anchored to a path
            // instead of an episode, or when the ladder said honestly that it lost its episode — an
            // orphaned note is still a record the reader has to see, which is the whole point of the file.
            let owned = row.episode.is_empty()
                || row.link_state == "orphan"
                || in_scope.contains(&row.episode.as_str())
                || row
                    .relinked_from
                    .as_ref()
                    .is_some_and(|from| in_scope.contains(&from.as_str()));
            if !owned {
                problems.push(format!(
                    "note {} is about {}, which is no episode in scope",
                    row.id, row.episode
                ));
            }
            if row.digest.is_empty() {
                problems.push(format!("note {} carries no tamper digest", row.id));
            }
            if !matches!(
                row.kind.as_str(),
                "decision" | "question" | "followup" | "annotation"
            ) {
                problems.push(format!("note {} has kind `{}`", row.id, row.kind));
            }
            if !matches!(row.status.as_str(), "open" | "answered" | "withdrawn") {
                problems.push(format!("note {} has status `{}`", row.id, row.status));
            }
        }

        // The allowance ledger: ordered, and every row names a kind of debt there is an engine for
        // (`e3.10`, `reco-5ht`). The record does not decide what is forgiven — the ledger does — so the only
        // things worth checking here are the order a reader diff's against and the vocabulary.
        let debt_keys: Vec<(&String, &String)> =
            self.debts.iter().map(|row| (&row.kind, &row.key)).collect();
        if !debt_keys.windows(2).all(|pair| pair[0] <= pair[1]) {
            problems.push("the allowance ledger is not printed by kind, then by key".to_owned());
        }
        for row in &self.debts {
            if row.kind != "cycle" && row.kind != "layer" {
                problems.push(format!(
                    "allowance {} has kind `{}`, and only `cycle` and `layer` have engines",
                    row.key, row.kind
                ));
            }
            if row.key.is_empty() {
                problems.push("an allowance row carries no key".to_owned());
            }
        }

        // Checklist: closed items pay for their closure.
        let checklist_keys: Vec<_> = self.checklist.iter().map(checklist_key).collect();
        if !checklist_keys.windows(2).all(|pair| pair[0] <= pair[1]) {
            problems.push(format!(
                "checklist items are not printed in the order claimed: {CHECKLIST_ORDER}"
            ));
        }
        let mut ids_seen: Vec<&str> = Vec::with_capacity(self.checklist.len());
        for item in &self.checklist {
            if !matches!(item.state.as_str(), STATE_OPEN | STATE_DONE) {
                problems.push(format!(
                    "checklist item {} has state `{}`",
                    item.id, item.state
                ));
            }
            if ids_seen.contains(&item.id.as_str()) {
                problems.push(format!("checklist item {} appears twice", item.id));
            }
            ids_seen.push(&item.id);
            if item.state == STATE_DONE && item.closed_by.is_none() {
                problems.push(format!(
                    "checklist item {} is closed by nothing, which is not a closure",
                    item.id
                ));
            }
            if let Some(episode) = &item.episode
                && !in_scope.contains(&episode.as_str())
            {
                problems.push(format!(
                    "checklist item {} is about {}, which is not in scope",
                    item.id, episode
                ));
            }
        }

        // Leaks: a host path in a field this module owns, or a control byte anywhere in the bytes.
        for (field, value) in [
            ("provenance.store", self.provenance.store.as_deref()),
            ("scope.from", self.scope.from.as_deref()),
            ("scope.to", self.scope.to.as_deref()),
        ] {
            if value.is_some_and(absolute_looking) {
                problems.push(format!(
                    "{field} holds an absolute path, which is a host leak"
                ));
            }
        }
        for row in &self.signoffs {
            if absolute_looking(&row.file) {
                problems.push(format!(
                    "sign-off {} names {}, and a ledger file is repo-relative",
                    row.id, row.file
                ));
            }
        }
        for commit in &self.commits {
            if let Some(url) = &commit.url {
                if !url.starts_with("https://") && !url.starts_with("http://") {
                    problems.push(format!(
                        "commit {} has a link that is not http(s)",
                        commit.short
                    ));
                }
                if url.split('/').nth(2).is_some_and(|host| host.contains('@')) {
                    problems.push(format!(
                        "commit {}'s link carries credentials, which is a leak",
                        commit.short
                    ));
                }
            }
        }
        let bytes = self.to_json();
        if let Some(bad) = bytes
            .bytes()
            .find(|byte| *byte < 0x20 && !matches!(byte, b'\n' | b'\t' | b'\r'))
        {
            problems.push(format!(
                "the serialised record holds control byte {bad:#04x}"
            ));
        }

        // The roll-up has to be the rows it claims to be.
        let expected = summary_of(self);
        if expected != self.summary {
            problems.push("the summary does not match the rows it summarises".to_owned());
        }

        problems
    }
}

/// Does this string read like a path that names somebody's machine?
fn absolute_looking(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value
            .as_bytes()
            .first()
            .is_some_and(|_| value.get(1..3) == Some(":/") || value.get(1..3) == Some(":\\"))
}

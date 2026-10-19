//! `episode.rs` — migration 3: the episodes `e1.3` derives and the review state a human leaves.
//!
//! The DDL is [`fathom_ingest`]'s and ADR-004's, not a new invention: the `episode`, `episode_member`
//! and `episode_rewrite` tables are ADR-004 Appendix A §4.3 reproduced with the columns this epic's
//! screens actually draw added, and `session_state` is §4.7's verbatim. Two copies of the *shape* would
//! be the drift this repository's whole tooling exists to prevent, so the column names are the reference
//! schema's and the additions are listed at the bottom of each table with the ticket that needed them.
//!
//! # Why identity is a digest and not a row
//!
//! A rebase, an amend or a force-push gives every commit in the rewritten range a fresh oid, and a review
//! decision attached to an oid is a review decision that silently evaporates — ADR-001 §7.8's data-loss
//! machine, run annually. So an episode's id is derived from the facts a rewrite preserves:
//!
//! ```text
//! id = "ep-" + sha256("fathom-episode/1" \0 author \0 normalized-subject \0 sorted-path-set)[0..20]
//! ```
//!
//! Rewriting does not change who authored the change, what its subject line says, or which paths it
//! touches, so the id is stable across `--amend`, `rebase -i`, `commit --fixup` and a force-push, and the
//! review row attached to it is found again without a relink step existing at all.
//!
//! Two rewrites do break that: a **reword** (subject changed) and a **squash or split** (the path set
//! moves). For those there are two weaker digests — author+subject, which survives files moving, and
//! author+paths, which survives the subject changing — and a relink pass that re-attaches review state from
//! a dead episode to a live one when either digest matches or their commit sets overlap. Every relink is
//! written to `episode_rewrite`, which is what makes "sign-off survived the rebase" a queryable fact rather
//! than a hope — ADR-003's stability clause needs the rewrites recorded, and this is where they get
//! recorded.
//!
//! # What is deliberately not here
//!
//! The durable, committable review ledger (`ledger_entry` with its signatures and anchors) is `e4.6`;
//! `review_state` is the epic-1 materialisation of "which of these have I read and what did I think",
//! keyed by the same fingerprint so that the ledger can be built on top of it rather than beside it.
//! Risk scores, findings and the delta model are `e2.x`; nothing here computes one.

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension as _, params};

use crate::error::Error;
use crate::spine::Store;
use std::fmt;

use crate::spine::corrupt;

/// The episode DDL, as migration 3.
///
/// The reference columns and their CHECK constraints come from ADR-004 Appendix A §4.3 unchanged; the
/// columns after the `-- e1.3 additions` marker are what the timeline rail and the episode card draw and
/// are derived, never authoritative (they are rewritten on every index run, unlike the ones above, which
/// are the identity and the boundary decision).
pub const MIGRATION_3: &str = "
CREATE TABLE IF NOT EXISTS episode (
  id            TEXT PRIMARY KEY,          -- ADR-003 identity
  kind          TEXT NOT NULL CHECK (kind IN ('committed','provisional')),
  fingerprint   TEXT NOT NULL,             -- content digest that survives a rewrite
  lineage_ref   TEXT NOT NULL,             -- ref/branch the episode is ordered within
  lineage_seq   INTEGER NOT NULL,          -- ordinal on that lineage
  created_utc   INTEGER NOT NULL,
  created_source TEXT NOT NULL CHECK (created_source IN ('hook','commit','index')),
  created_confidence REAL NOT NULL,        -- 1.0 hook, 0.7 commit, 0.3 index (e1.3)
  session_id    TEXT,
  turn_id       TEXT,
  boundary_rule TEXT NOT NULL,             -- ADR-003 App. B, closed: override|hook|trailer|branch|author|time-gap|message-shape|root
  boundary_confidence REAL NOT NULL,
  author_kind   TEXT NOT NULL CHECK (author_kind IN ('agent','human','unknown')),
  author_confidence REAL NOT NULL,
  intent        TEXT,                      -- JSON: prompt sha, plan items, declared scope
  noise_share   REAL NOT NULL DEFAULT 0,
  epoch         INTEGER NOT NULL,
  -- e1.3 additions: the derived columns the timeline rail, the episode card and the diff tree read.
  fallback_fingerprint TEXT NOT NULL,      -- author+subject only: the relink key after files move
  shape_fingerprint TEXT NOT NULL DEFAULT '', -- author+paths: the relink key after a reword
  first_oid     TEXT NOT NULL,
  last_oid      TEXT NOT NULL,
  n_commits     INTEGER NOT NULL DEFAULT 1,
  n_files       INTEGER NOT NULL DEFAULT 0,
  n_add         INTEGER NOT NULL DEFAULT 0,
  n_del         INTEGER NOT NULL DEFAULT 0,
  n_binary      INTEGER NOT NULL DEFAULT 0,
  first_utc     INTEGER NOT NULL DEFAULT 0,
  last_utc      INTEGER NOT NULL DEFAULT 0,
  author_name   TEXT NOT NULL DEFAULT '',
  author_email  TEXT NOT NULL DEFAULT '',
  summary       TEXT NOT NULL DEFAULT '',
  overridden    INTEGER NOT NULL DEFAULT 0, -- a fathom.toml force-split/force-merge moved this edge
  live          INTEGER NOT NULL DEFAULT 1, -- 0 once no current commit belongs to it
  seen_utc      INTEGER NOT NULL DEFAULT 0  -- stamped by the run that last derived this row
) STRICT;
CREATE INDEX IF NOT EXISTS episode_fingerprint ON episode(fingerprint);
CREATE INDEX IF NOT EXISTS episode_lineage ON episode(lineage_ref, lineage_seq);
CREATE INDEX IF NOT EXISTS episode_created ON episode(created_utc);
CREATE INDEX IF NOT EXISTS episode_epoch ON episode(epoch);
CREATE INDEX IF NOT EXISTS episode_fallback ON episode(fallback_fingerprint);
CREATE INDEX IF NOT EXISTS episode_shape ON episode(shape_fingerprint);
CREATE INDEX IF NOT EXISTS episode_live ON episode(live);

CREATE TABLE IF NOT EXISTS episode_member (
  episode_id TEXT NOT NULL REFERENCES episode(id) ON DELETE CASCADE,
  ordinal    INTEGER NOT NULL,
  commit_oid TEXT NOT NULL REFERENCES git_commit(oid) ON DELETE CASCADE,
  PRIMARY KEY (episode_id, ordinal)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS episode_member_commit ON episode_member(commit_oid);

CREATE TABLE IF NOT EXISTS episode_rewrite (
  old_id   TEXT NOT NULL,
  new_id   TEXT NOT NULL,
  relation TEXT NOT NULL CHECK (relation IN ('amend','rebase','squash','split','cherry','merge','other')),
  method   TEXT NOT NULL CHECK (method IN ('exact','fingerprint','shape','member-set','manual')),
  score    REAL NOT NULL,
  at_utc   INTEGER NOT NULL,
  PRIMARY KEY (old_id, new_id)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS episode_rewrite_new ON episode_rewrite(new_id);

CREATE TABLE IF NOT EXISTS review_state (
  episode_id  TEXT PRIMARY KEY REFERENCES episode(id) ON DELETE CASCADE,
  state       TEXT NOT NULL CHECK (state IN ('unread','read','accepted','flagged','dismissed','snoozed')),
  fingerprint TEXT NOT NULL,
  actor       TEXT NOT NULL,
  note        TEXT,
  updated_utc INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS review_state_state ON review_state(state);

CREATE TABLE IF NOT EXISTS session_state (
  key TEXT PRIMARY KEY,                    -- 'tui.pane_sizes', 'queue.filter', 'unread:<id>'
  value TEXT NOT NULL,
  updated_utc INTEGER NOT NULL
) STRICT;
";

/// The attribution DDL, as migration 4 (`e4.1`, `reco-w7q`).
///
/// Three groups, one migration, because they are one decision: an episode now carries *why* it is labelled
/// the way it is, a commit now carries the committer identity the committer rung reads, and a human
/// correction is stored so it applies to the next commit with the same signature rather than being typed
/// again.
///
/// Only the new table lives here. SQLite has no `ADD COLUMN IF NOT EXISTS`, and §9.5 requires every
/// migration in [`crate::spine::MIGRATIONS`] to be re-runnable against a store that already has it — so the
/// six column additions are [`ADDED_COLUMNS`], applied by a probe-then-add step that is idempotent for
/// exactly the reason a `CREATE TABLE IF NOT EXISTS` is.
///
/// `author_kind` / `author_confidence` stay exactly as they are: they are ADR-003's columns and the
/// segmentation still writes them from the same verdict. The `attribution_*` columns are the extra detail
/// the card shows — the agent's name, the evidence list as JSON, the rung that decided, and the reason an
/// answer was refused. Like the other derived columns, they are rewritten on every index run.
pub const MIGRATION_4: &str = "
CREATE TABLE IF NOT EXISTS attribution_correction (
  identity    TEXT PRIMARY KEY,           -- 'identity:<address>' or 'identity:<name>' (attribute::identity_key)
  kind        TEXT NOT NULL CHECK (kind IN ('agent','human','unknown')),
  agent       TEXT NOT NULL DEFAULT '',   -- named agent, when kind is 'agent'
  note        TEXT NOT NULL DEFAULT '',   -- why the human overrode it, for the next reader
  created_utc INTEGER NOT NULL
) WITHOUT ROWID, STRICT;
";

/// Column additions made to tables that already exist, as `(table, column, definition)` (`e4.1`).
///
/// A migration string cannot hold these without breaking §9.5's rule that a migration may be applied to a
/// store that already has it, so they are data, and the step that applies them reads `PRAGMA table_info`
/// first — which makes applying it twice the no-op it has to be.
pub const ADDED_COLUMNS: &[(&str, &str, &str)] = &[
    // The committer identity, which the committer rung of attribution reads.
    ("git_commit", "committer_name", "TEXT NOT NULL DEFAULT ''"),
    ("git_commit", "committer_email", "TEXT NOT NULL DEFAULT ''"),
    // The case behind the label, so the card can show its work.
    ("episode", "attribution_source", "TEXT NOT NULL DEFAULT ''"),
    ("episode", "attribution_agent", "TEXT NOT NULL DEFAULT ''"),
    (
        "episode",
        "attribution_evidence",
        "TEXT NOT NULL DEFAULT '[]'",
    ),
    ("episode", "attribution_reason", "TEXT NOT NULL DEFAULT ''"),
    // The intent, and the four facts about it a surface reads without parsing JSON (`e4.2`, `reco-c54`).
    //
    // The ADR's `episode` table already reserved an `intent` column ("JSON: prompt sha, plan items,
    // declared scope"), and that column is what this writes — the *lists* live there, because nothing
    // queries on a list. These five are the scalars something does query on: which state, which rung, how
    // sure, the text itself, and the reason there is no text. A row written before this migration has
    // `intent_state = ''`, which every reader treats as "this build never looked", not as "absent" — the
    // two are different claims and only one of them can be checked afterwards.
    ("episode", "intent_state", "TEXT NOT NULL DEFAULT ''"),
    ("episode", "intent_source", "TEXT NOT NULL DEFAULT ''"),
    ("episode", "intent_confidence", "REAL NOT NULL DEFAULT 0"),
    ("episode", "intent_text", "TEXT NOT NULL DEFAULT ''"),
    ("episode", "intent_reason", "TEXT NOT NULL DEFAULT ''"),
];

/// Migration 5: the search document `e4.2` fills and `e5.2` reads (`reco-c54`).
///
/// ADR-004 §4.8's `episode_doc` — the plain content table under the `episode_fts` index, reproduced with
/// its DDL unchanged (that schema is the reference's, not a new invention), and the same durability class:
/// **R**, rebuildable from the rows it indexes, and never authoritative. Writing it is what makes the
/// ticket's fourth acceptance criterion a fact rather than a promise: intent text goes into the body
/// column, so the search surface — `e5.2`'s overlay and ADR-005 §4.1's `search_history` tool, neither of
/// which exists yet — has the text to search the day either is built, and does not have to be retrofitted
/// with a second capture path.
///
/// The FTS5 half lives in [`DDL_FTS`] rather than here, because §4.8 makes it conditional on a compile
/// option and a migration may not be conditional: a migration that created the table on one machine and
/// not on another would make `user_version` mean different schemas on two hosts. So the table is created at
/// open when `sqlite_compileoption_used('SQLITE_ENABLE_FTS5')` says so, and the absence is recorded.
pub const MIGRATION_5: &str = "
CREATE TABLE IF NOT EXISTS episode_doc (
  episode_id TEXT PRIMARY KEY,
  title  TEXT NOT NULL,
  body   TEXT NOT NULL,
  tokens INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX IF NOT EXISTS episode_doc_title ON episode_doc(title);
";

/// The external-content FTS5 index over [`MIGRATION_5`]'s table, with the triggers that keep it in step
/// (ADR-004 §4.8: "rebuildable independently of the content").
pub const DDL_FTS: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS episode_fts USING fts5(
  episode_id UNINDEXED, title, body,
  content='episode_doc', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS episode_doc_ai AFTER INSERT ON episode_doc BEGIN
  INSERT INTO episode_fts(rowid, episode_id, title, body)
  VALUES (new.rowid, new.episode_id, new.title, new.body);
END;
CREATE TRIGGER IF NOT EXISTS episode_doc_ad AFTER DELETE ON episode_doc BEGIN
  INSERT INTO episode_fts(episode_fts, rowid, episode_id, title, body)
  VALUES ('delete', old.rowid, old.episode_id, old.title, old.body);
END;
CREATE TRIGGER IF NOT EXISTS episode_doc_au AFTER UPDATE ON episode_doc BEGIN
  INSERT INTO episode_fts(episode_fts, rowid, episode_id, title, body)
  VALUES ('delete', old.rowid, old.episode_id, old.title, old.body);
  INSERT INTO episode_fts(rowid, episode_id, title, body)
  VALUES (new.rowid, new.episode_id, new.title, new.body);
END;
";

/// Migration 6: the triage ledger — every act the attention queue takes (`e4.5`, `reco-wsj`).
///
/// One row per **act**, append-only, and that shape is the whole point. `review_state` keeps one row per
/// episode: the *current* marker, which is what the rail's badge and the queue's counts read, and which a
/// second `x` on the same episode overwrites. The ledger must not overwrite, because the criterion is
/// "every triage action records who, when, why" and a record that a later keystroke can erase records
/// nothing. It is also the only place the retrospective can come from: "`not-my-area`, twelve times this
/// week, by both reviewers, last at 14:22" is a query over acts, not over states.
///
/// # Why this is not `ledger_entry`, and what `e4.6` does with it
///
/// ADR-004 §4.7 defines `ledger_entry` as the **mirror of committed markdown** under `.fathom/reviews/`,
/// class M, with the file as the authoritative copy. That file is `e4.6`'s to write, and `e4.5` sits in
/// front of it in the plan: the acts have to exist before there is a sign-off format to render them into.
/// So this table is the act log, and `e4.6` renders *from* it — re-linking with the same `fingerprint`
/// column, which is why the fingerprint is stored per row rather than derived at read time. The markdown
/// file then becomes the committable copy, `ledger_entry` its mirror, and this table the record of what
/// actually happened at the keyboard, including the dismissals and snoozes that are not sign-offs and
/// never become ledger files.
///
/// # No foreign key, on purpose
///
/// `review_state` references `episode(id) ON DELETE CASCADE` because a marker on an episode that is gone
/// is meaningless. A triage act is a *historical fact*: "this reviewer dismissed this fingerprint" stays
/// true after a rebase retires the episode row, and a cascade would erase the audit trail exactly when it
/// is most interesting — which is §8.7's rule that a prune may never lose a review, applied to
/// migrations. The `fingerprint` is the durable link; `episode_id` is a convenience join that is allowed
/// to dangle.
pub const MIGRATION_6: &str = "
CREATE TABLE IF NOT EXISTS triage_event (
  seq          INTEGER PRIMARY KEY AUTOINCREMENT,
  episode_id   TEXT NOT NULL,
  fingerprint  TEXT NOT NULL,               -- what the act was taken against (e4.6 re-links with it)
  disposition  TEXT NOT NULL CHECK (disposition IN
                   ('unread','read','accepted','flagged','dismissed','snoozed')),
  reason_code  TEXT NOT NULL DEFAULT '',    -- the dismiss vocabulary; empty when not a dismissal
  note         TEXT NOT NULL DEFAULT '',    -- the reviewer's own words, when the act carries any
  until        TEXT NOT NULL DEFAULT '',    -- the snooze condition, stamped with its baseline
  actor        TEXT NOT NULL,               -- who: 'name <address>', or 'unknown', never invented
  risk         INTEGER,                     -- the score at the time of the act; NULL = unscored then
  risk_model   TEXT NOT NULL DEFAULT '',    -- which ranking produced that number
  utc          INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS triage_event_episode ON triage_event(episode_id, seq);
CREATE INDEX IF NOT EXISTS triage_event_utc ON triage_event(utc);
CREATE INDEX IF NOT EXISTS triage_event_reason ON triage_event(reason_code);
";

/// One stored human correction (`e4.1`, `reco-w7q`): "whoever answers to this identity is an agent / a
/// person", learned once and applied to every later commit carrying that signature.
///
/// `identity` is an `attribute::identity_key` string (`identity:<address>`, or `identity:<name>` when
/// there is no address). The store keeps it, and the kind, as opaque text so it does not depend on the
/// analysis crates — exactly as it keeps `author_kind` as text rather than as an enum. The CHECK on the
/// column is what stops a typo being stored, and `fathom_ingest::attribute::AuthorKind::parse` is what
/// reads it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionCorrection {
    /// The identity key the user corrected.
    pub identity: String,
    /// `agent`, `human` or `unknown`.
    pub kind: String,
    /// The agent to name, when `kind` is `agent`. Empty otherwise.
    pub agent: String,
    /// Why the human overrode the classifier, for the next reader.
    pub note: String,
    /// When it was stored, unix millis.
    pub created_utc: i64,
}

/// What a human's review marker can be.
///
/// `Unread` is the default and is stored, not inferred from absence: "this row is missing" and "nobody has
/// looked at this episode" are different claims, and only one of them is checkable after a relink.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    /// Nobody has opened it.
    #[default]
    Unread,
    /// Opened, no verdict given.
    Read,
    /// Signed off (`x` in the TUI).
    Accepted,
    /// Called out for someone else (`f`).
    Flagged,
    /// Refused with a reason (`-`).
    Dismissed,
    /// Deferred (`z`).
    Snoozed,
}

impl ReviewState {
    /// The store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ReviewState::Unread => "unread",
            ReviewState::Read => "read",
            ReviewState::Accepted => "accepted",
            ReviewState::Flagged => "flagged",
            ReviewState::Dismissed => "dismissed",
            ReviewState::Snoozed => "snoozed",
        }
    }

    /// Parse the store's spelling; unknown values come back as `Unread` rather than an error, because a
    /// row written by a newer fathom must not make this one refuse to start (§9.5's forward-compat rule).
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "read" => ReviewState::Read,
            "accepted" => ReviewState::Accepted,
            "flagged" => ReviewState::Flagged,
            "dismissed" => ReviewState::Dismissed,
            "snoozed" => ReviewState::Snoozed,
            _ => ReviewState::Unread,
        }
    }

    /// Whether a row with this state is a decision someone made.
    #[must_use]
    pub const fn is_verdict(self) -> bool {
        !matches!(self, ReviewState::Unread | ReviewState::Read)
    }
}

impl fmt::Display for ReviewState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One triage act, as the ledger stores it (`e4.5`, `reco-wsj`).
///
/// The store spells the disposition as [`ReviewState`] — the same closed word list, because "what the act
/// was" and "what the marker now is" are the same word in every surface that prints either. The reasons,
/// notes and snooze conditions are opaque text here for the same reason `attribution_correction.kind` is:
/// the vocabulary belongs to the analysis layer, and a store that parsed it would have to be updated every
/// time a reason word was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageEvent {
    /// The episode the act was taken on.
    pub episode: String,
    /// The digest it was taken against. Read back from the row by [`Store::record_triage`] rather than
    /// trusted from the caller, so a stale in-memory read cannot write a stale link into the durable copy.
    pub fingerprint: String,
    /// What was decided.
    pub disposition: ReviewState,
    /// The dismiss code, empty unless `disposition` is `Dismissed`.
    pub reason: String,
    /// The reviewer's own words, when the act carries any.
    pub note: String,
    /// The snooze condition with its baseline, empty unless `disposition` is `Snoozed`.
    pub until: String,
    /// Who: `name <address>`, or `unknown`. Never invented (`ADR-001 §7.7`).
    pub actor: String,
    /// The risk score at the time of the act, and which model scored it. `None` when nothing had scored
    /// the episode then, which the retrospective prints as its own caveat rather than as a zero.
    pub risk: Option<i64>,
    /// See [`TriageEvent::risk`].
    pub risk_model: String,
    /// When, unix millis.
    pub utc: i64,
}

/// The `review_state.note` a triage act writes.
///
/// The act's own words if it has any, and otherwise what the act amounted to, so a marker read without the
/// ledger beside it still says something true. `None` for a bare accept, which has nothing to say beyond
/// the state itself — and a `NULL` note is how "no note was written" differs from "an empty note was".
///
/// No `dismissed:` or `snoozed:` prefix, because the note is always drawn beside the state that says exactly
/// that (`⚑ dismissed: not-my-area`), and a note reading `dismissed: dismissed: not-my-area` is the sound
/// of two writers assuming the other one would name the act.
fn note_text(note: &str, reason: &str, until: &str) -> Option<String> {
    if !note.is_empty() {
        return Some(note.to_owned());
    }
    if !reason.is_empty() {
        return Some(reason.to_owned());
    }
    if !until.is_empty() {
        return Some(format!("until {until}"));
    }
    None
}

/// One episode as `e1.3` computed it, in the store's own vocabulary (hex ids, plain strings).
#[derive(Debug, Clone, Default)]
pub struct EpisodeRow {
    /// The derived identity.
    pub id: String,
    /// The strong digest (author, subject, path set).
    pub fingerprint: String,
    /// The relink digest (author, subject).
    pub fallback_fingerprint: String,
    /// The relink digest (author, path set): what a reword rides across on.
    pub shape_fingerprint: String,
    /// `committed` for a closed episode, `provisional` for the still-growing tail of a session.
    pub kind: String,
    /// The ref this episode is ordered within, and its ordinal on it.
    pub lineage_ref: String,
    /// See [`EpisodeRow::lineage_ref`].
    pub lineage_seq: i64,
    /// When the episode began, unix millis.
    pub created_utc: i64,
    /// `hook`, `commit` or `index`: which evidence opened it.
    pub created_source: String,
    /// See [`EpisodeRow::created_source`].
    pub created_confidence: f64,
    /// The harness session, when a trailer named one.
    pub session_id: Option<String>,
    /// The harness turn, when a trailer named one.
    pub turn_id: Option<String>,
    /// Which rule opened this episode, and how much to trust it.
    pub boundary_rule: String,
    /// See [`EpisodeRow::boundary_rule`].
    pub boundary_confidence: f64,
    /// `agent`, `human` or `unknown`.
    pub author_kind: String,
    /// See [`EpisodeRow::author_kind`].
    pub author_confidence: f64,
    /// Which class of evidence decided (`e4.1`): `correction`, `harness`, `trailer`, `declared`,
    /// `author`, `committer`, `message`, `identity`, `absent`, or empty on a row stored before
    /// migration 4.
    pub attribution_source: String,
    /// The agent the evidence named, when it named exactly one.
    pub attribution_agent: String,
    /// The evidence list as a JSON array of `{source, detail, confidence}`, strongest first.
    pub attribution_evidence: String,
    /// Why the verdict is `unknown`, or why a label was refused. Empty when the answer was the answer.
    pub attribution_reason: String,
    /// Which intent state the last index recorded: `recorded`, `digest-only`, `none`, or `''` for a row
    /// written before migration 5 — which means "this build never looked", not "nothing was recorded".
    pub intent_state: String,
    /// The rung that decided, in `fathom_ingest::intent::Source`'s spelling. Empty with no state.
    pub intent_source: String,
    /// Confidence in the intent text; `0.0` when there is none.
    pub intent_confidence: f64,
    /// The intent text itself, which is what the search document quotes and the card prints. Empty when
    /// [`EpisodeRow::intent_state`] is not `recorded`.
    pub intent_text: String,
    /// Why there is no intent — what was looked for and not found. Empty when there is one.
    pub intent_reason: String,
    /// The whole intent document as JSON: the plan list, the declared scope globs, the prompt's digests,
    /// the evidence list. The ADR's reserved `intent` column, finally written.
    pub intent: String,
    /// The author as committed.
    pub author_name: String,
    /// The author's address as committed.
    pub author_email: String,
    /// The commit range, first-parent-first order.
    pub first_oid: String,
    /// See [`EpisodeRow::first_oid`].
    pub last_oid: String,
    /// Members in order; the durable half of the range.
    pub members: Vec<String>,
    /// The rollups the rail draws.
    pub n_files: i64,
    /// See [`EpisodeRow::n_files`].
    pub n_add: i64,
    /// See [`EpisodeRow::n_files`].
    pub n_del: i64,
    /// See [`EpisodeRow::n_files`].
    pub n_binary: i64,
    /// The first/last member's author date.
    pub first_utc: i64,
    /// See [`EpisodeRow::first_utc`].
    pub last_utc: i64,
    /// The one-line summary the rail prints.
    pub summary: String,
    /// Whether a `fathom.toml` override moved one of this episode's boundaries.
    pub overridden: bool,
    /// Whether any current commit still belongs to this episode.
    pub live: bool,
    /// What share of the episode's churn the noise policy explains (`e2.2`; 0 until then).
    pub noise_share: f64,
    /// Which epoch wrote this row.
    pub epoch: i64,
}

impl EpisodeRow {
    /// Number of members — the "4 commits" the card prints.
    #[must_use]
    pub fn n_commits(&self) -> i64 {
        self.members.len().try_into().unwrap_or(i64::MAX)
    }
}

/// One path's summed change inside one episode.
///
/// The per-episode rollups on [`EpisodeRow`] (`n_add`, `n_del`, `n_files`) are the numbers the rail
/// prints, and they are not enough to say *what* an episode did: a turn that adds 520 lines of source
/// and deletes 380 lines of tests has the same `n_del` as one that deletes 380 lines of a lockfile,
/// and only one of those is a review problem. This is the row that distinguishes them, summed per
/// `(episode, path)` over `episode_member ⋈ git_commit_file` — so a four-commit turn that edited the
/// same file four times is one row, which is what "what did this episode do to this file" asks.
///
/// The two taxonomy columns are read back rather than re-derived (docs/concepts/paths.md §1: "the
/// store records the taxonomy decision, nothing downstream re-derives it"). They are `unknown` in
/// every index built before the e0.9 table is wired into the Rust indexer, and a consumer that reads
/// them must handle that as "no answer", not as a class.
#[derive(Debug, Clone)]
pub struct EpisodeFile {
    /// The path as the file row recorded it (the new path, after a rename).
    pub path: String,
    /// Lines added across the episode's commits.
    pub add: i64,
    /// Lines removed across the episode's commits.
    pub del: i64,
    /// How many of the episode's commits touched it — more than one is a file the turn kept returning to.
    pub commits: i64,
    /// Whether any of those touches was binary.
    pub binary: bool,
    /// The e0.9 class the indexer stamped, or `unknown` when it stamped none.
    pub path_class: String,
    /// The e0.9 noise level the indexer stamped, or `unknown` when it stamped none.
    pub noise_class: String,
}

/// What [`Store::episode_files`] read, and how much of the index it had to leave out.
#[derive(Debug, Clone)]
pub struct EpisodeFiles {
    /// The summed file rows, as `episode id → files`, sorted by path.
    pub by_episode: std::collections::BTreeMap<String, Vec<EpisodeFile>>,
    /// `(episode, path)` rows read.
    pub rows: u64,
    /// Whether [`Store::EPISODE_FILE_CAP`] cut the read short. Truncation is a fact the reader has to
    /// print, not a detail it gets to forget (ADR-001 §7.2): a ranking that saw 50 000 of 90 000 file
    /// rows is a ranking that says so.
    pub truncated: bool,
}

/// The composable filter `e1.6`'s rail asks for (author AND path AND unread must compose).
#[derive(Debug, Clone, Default)]
pub struct EpisodeQuery {
    /// Exact author names; empty means no constraint.
    pub authors: Vec<String>,
    /// `agent`/`human`/`unknown`; empty means no constraint.
    pub kinds: Vec<String>,
    /// `Some(ReviewState::Unread)` is "unread only"; `Some(ReviewState::Read)` is "anything opened".
    pub state: Option<ReviewState>,
    /// A path fragment; an episode matches when any member touched a path containing it.
    pub path: Option<String>,
    /// Inclusive lower bound on `last_utc`.
    pub since_utc: Option<i64>,
    /// Exclusive upper bound on `last_utc`.
    pub until_utc: Option<i64>,
    /// Skip this many rows after ordering.
    pub offset: i64,
    /// At most this many rows; `0` (the default) means "every row the filter selects".
    pub limit: i64,
    /// Include episodes no current commit belongs to.
    ///
    /// Off by default because every surface that draws a list wants the live set; `e4.6`'s ledger turns it on
    /// because a sign-off that points only at a dead episode is a different, more useful sentence than a
    /// sign-off that points at nothing — "the history that carried your review was rewritten away" is
    /// advice, and "no match" is a shrug (ADR-003 §6.5).
    pub include_dead: bool,
}

/// What a write of episodes did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EpisodeWrite {
    /// Episodes this run created.
    pub inserted: usize,
    /// Episodes whose derived columns were refreshed.
    pub updated: usize,
    /// Review-state rows carried across a detected rewrite.
    pub relinked: usize,
    /// `episode_member` rows written.
    pub members: usize,
    /// Dead episodes garbage-collected because nothing referenced them.
    pub retired: usize,
}

impl Store {
    /// Write the derived episodes, relink review state across rewrites, and garbage-collect dead rows.
    ///
    /// Three passes, one transaction:
    ///
    /// 1. **Write.** `INSERT … ON CONFLICT(id) DO UPDATE` on the derived columns only. The identity and
    ///    the boundary decision are rewritten too because they are a pure function of the commits, but
    ///    nothing a human wrote is touched here. Every row written stamps `seen_utc = now`, which is what
    ///    makes "produced by the current segmentation" a queryable property rather than a memory.
    /// 2. **Retire the stale.** Anything this run did not write is `live = 0`. This is the half that keeps a
    ///    force-push from leaving a ghost episode beside its replacement: the store is append-only, so the
    ///    rebased-away commits are still in `git_commit`, and an episode that only those commits belong to
    ///    must stop being a reviewable thing rather than linger as a duplicate of its own rewrite.
    /// 3. **Relink.** For each *live* episode with no review row, look for a *dead* episode that has one and
    ///    either shares the fallback digest or shares a commit member. Copy the review row forward and
    ///    record the rewrite. This is the half that survives a reword or a squash — the case a rebase
    ///    changes the *id* of, and so the one a rewrite would otherwise orphan.
    /// 4. **Prune.** Delete dead episodes nobody has a decision about. An episode somebody signed off on
    ///    is kept, flagged `live = 0`, so the reviewer can still see "you accepted this; its commits are
    ///    gone" instead of watching the row vanish.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if any statement fails; the whole write is one transaction, so a
    /// failure leaves the previous episode set intact.
    pub fn write_episodes(&mut self, now: i64, rows: &[EpisodeRow]) -> Result<EpisodeWrite, Error> {
        let epoch = self.epoch()?;
        let mut stats = EpisodeWrite::default();
        {
            let tx = self.immediate_transaction()?;
            // Which of these episodes existed before this run. `changes()` cannot tell an insert from an
            // upsert whose values happened to be identical — SQLite reports one row changed either way —
            // so "new since the last index" is answered by asking which ids were already in the table.
            // One query, not one per episode.
            let mut statement = tx
                .prepare("SELECT id FROM episode")
                .map_err(|err| corrupt("reading the episode set", &err))?;
            let before: std::collections::BTreeSet<String> = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|err| corrupt("reading the episode set", &err))?
                .collect::<Result<_, _>>()
                .map_err(|err| corrupt("reading the episode set", &err))?;
            drop(statement);
            for row in rows {
                let existed = before.contains(&row.id);
                tx
                    .execute(
                        "INSERT INTO episode(id, kind, fingerprint, lineage_ref, lineage_seq,
                                created_utc, created_source, created_confidence, session_id, turn_id,
                                boundary_rule, boundary_confidence, author_kind, author_confidence,
                                noise_share, epoch, fallback_fingerprint, shape_fingerprint,
                                first_oid, last_oid,
                                n_commits, n_files, n_add, n_del, n_binary, first_utc, last_utc,
                                author_name, author_email, summary, overridden, live, seen_utc,
                                attribution_source, attribution_agent, attribution_evidence,
                                attribution_reason,
                                intent, intent_state, intent_source, intent_confidence, intent_text,
                                intent_reason)
                         VALUES (?1,'committed',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,0,?14,
                                 ?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,1,?30,
                                 ?31,?32,?33,?34,
                                 ?35,?36,?37,?38,?39,?40)
                         ON CONFLICT(id) DO UPDATE SET
                            boundary_rule=excluded.boundary_rule,
                            boundary_confidence=excluded.boundary_confidence,
                            author_kind=excluded.author_kind,
                            author_confidence=excluded.author_confidence,
                            attribution_source=excluded.attribution_source,
                            attribution_agent=excluded.attribution_agent,
                            attribution_evidence=excluded.attribution_evidence,
                            attribution_reason=excluded.attribution_reason,
                            intent=excluded.intent,
                            intent_state=excluded.intent_state,
                            intent_source=excluded.intent_source,
                            intent_confidence=excluded.intent_confidence,
                            intent_text=excluded.intent_text,
                            intent_reason=excluded.intent_reason,
                            session_id=excluded.session_id,
                            turn_id=excluded.turn_id,
                            lineage_ref=excluded.lineage_ref,
                            lineage_seq=excluded.lineage_seq,
                            created_utc=excluded.created_utc,
                            first_oid=excluded.first_oid,
                            last_oid=excluded.last_oid,
                            n_commits=excluded.n_commits,
                            n_files=excluded.n_files,
                            n_add=excluded.n_add,
                            n_del=excluded.n_del,
                            n_binary=excluded.n_binary,
                            first_utc=excluded.first_utc,
                            last_utc=excluded.last_utc,
                            author_name=excluded.author_name,
                            author_email=excluded.author_email,
                            summary=excluded.summary,
                            overridden=excluded.overridden,
                            epoch=excluded.epoch,
                            live=1,
                            seen_utc=excluded.seen_utc",
                        params![
                            row.id,
                            row.fingerprint,
                            row.lineage_ref,
                            row.lineage_seq,
                            row.created_utc,
                            row.created_source,
                            row.created_confidence,
                            row.session_id,
                            row.turn_id,
                            row.boundary_rule,
                            row.boundary_confidence,
                            row.author_kind,
                            row.author_confidence,
                            epoch,
                            row.fallback_fingerprint,
                            row.shape_fingerprint,
                            row.first_oid,
                            row.last_oid,
                            row.n_commits(),
                            row.n_files,
                            row.n_add,
                            row.n_del,
                            row.n_binary,
                            row.first_utc,
                            row.last_utc,
                            row.author_name,
                            row.author_email,
                            row.summary,
                            i64::from(row.overridden),
                            now,
                            row.attribution_source,
                            row.attribution_agent,
                            row.attribution_evidence,
                            row.attribution_reason,
                            row.intent,
                            row.intent_state,
                            row.intent_source,
                            row.intent_confidence,
                            row.intent_text,
                            row.intent_reason
                        ],
                    )
                    .map_err(|err| corrupt("writing episode rows", &err))?;
                if existed {
                    stats.updated += 1;
                } else {
                    stats.inserted += 1;
                }
                // The search document (ADR-004 §4.8), written from the row and never read back to build
                // anything: `episode_doc` is class R, the triggers on it keep the FTS index in step, and
                // deleting the whole table is a supported repair. This is where intent text becomes
                // searchable — the write path is the only place that has it, which is why `e4.2` owns this
                // row and `e5.2` does not have to retrofit a capture pass.
                let body = document_body(&row.summary, &row.intent_text, &row.intent);
                let tokens = i64::try_from(body.split_whitespace().count()).unwrap_or(i64::MAX);
                tx.execute(
                    "INSERT INTO episode_doc(episode_id, title, body, tokens) VALUES (?1,?2,?3,?4)
                     ON CONFLICT(episode_id) DO UPDATE SET
                        title=excluded.title, body=excluded.body, tokens=excluded.tokens",
                    params![row.id, row.summary, body, tokens],
                )
                .map_err(|err| corrupt("writing the search document", &err))?;
                // Membership is derived from the current commit set, so it is replaced rather than merged:
                // a commit that moved to another episode must not stay pointed at this one.
                tx.execute(
                    "DELETE FROM episode_member WHERE episode_id = ?1",
                    params![row.id],
                )
                .map_err(|err| corrupt("replacing episode membership", &err))?;
                for (ordinal, oid) in row.members.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO episode_member(episode_id, ordinal, commit_oid) VALUES (?1,?2,?3)",
                        params![row.id, i64::try_from(ordinal).unwrap_or(i64::MAX), oid],
                    )
                    .map_err(|err| corrupt("writing episode members", &err))?;
                    stats.members += 1;
                }
            }
            // 2. Everything this run did not stamp is a rewrite's ghost: mark it dead *before* the relink
            // pass, which is written in terms of live and dead episodes and would otherwise find nothing to
            // relink from.
            tx.execute(
                "UPDATE episode SET live = 0 WHERE live = 1 AND seen_utc != ?1",
                params![now],
            )
            .map_err(|err| corrupt("retiring stale episodes", &err))?;
            stats.relinked = relink_review_state(&tx, now)
                .map_err(|err| corrupt("relinking review state across a rewrite", &err))?;
            stats.retired =
                retire_dead_episodes(&tx).map_err(|err| corrupt("retiring dead episodes", &err))?;
            tx.commit()
                .map_err(|err| corrupt("committing an episode write", &err))?;
        }
        Ok(stats)
    }

    /// The episodes a query selects, newest first.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn episodes(&self, query: &EpisodeQuery) -> Result<Vec<EpisodeRow>, Error> {
        let (sql, args) = episode_sql(query, false);
        let mut statement = self
            .connection()
            .prepare(&sql)
            .map_err(|err| corrupt("reading episodes", &err))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(args.iter()), row_to_episode)
            .map_err(|err| corrupt("reading episodes", &err))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| corrupt("reading episodes", &err))?);
        }
        for episode in &mut out {
            episode.members = self.episode_members(&episode.id)?;
        }
        Ok(out)
    }

    /// How many episodes the query would select without paging — the rail's "3 412 eps".
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn count_episodes(&self, query: &EpisodeQuery) -> Result<i64, Error> {
        let (sql, args) = episode_sql(query, true);
        self.connection()
            .query_row(&sql, rusqlite::params_from_iter(args.iter()), |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|err| corrupt("counting episodes", &err))
    }

    /// The members of one episode, in order.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn episode_members(&self, id: &str) -> Result<Vec<String>, Error> {
        let mut statement = self
            .connection()
            .prepare("SELECT commit_oid FROM episode_member WHERE episode_id = ?1 ORDER BY ordinal")
            .map_err(|err| corrupt("reading episode members", &err))?;
        let rows = statement
            .query_map(params![id], |row| row.get::<_, String>(0))
            .map_err(|err| corrupt("reading episode members", &err))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("reading episode members", &err))
    }

    /// One episode by id, if it exists.
    ///
    /// Members are loaded: every caller of this wants the whole card, and a card that prints "0 commits"
    /// for a four-commit episode is worse than no card at all.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn episode(&self, id: &str) -> Result<Option<EpisodeRow>, Error> {
        let mut statement = self
            .connection()
            .prepare(&episode_sql_for_id())
            .map_err(|err| corrupt("reading an episode", &err))?;
        let found = statement
            .query_map(params![id], row_to_episode)
            .map_err(|err| corrupt("reading an episode", &err))?
            .next()
            .transpose()
            .map_err(|err| corrupt("reading an episode", &err))?;
        Self::with_members(self, found)
    }

    /// Which episode contains a commit, if any.
    ///
    /// This is what `fathom show <rev>` asks, so it carries the members like [`Store::episode`] does.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn episode_for_commit(&self, oid: &str) -> Result<Option<EpisodeRow>, Error> {
        let mut statement = self
            .connection()
            .prepare(&format!(
                "SELECT {EPISODE_COLUMNS} FROM episode \
                 WHERE id IN (SELECT episode_id FROM episode_member WHERE commit_oid = ?1)"
            ))
            .map_err(|err| corrupt("reading an episode by commit", &err))?;
        let found = statement
            .query_map(params![oid], row_to_episode)
            .map_err(|err| corrupt("reading an episode by commit", &err))?
            .next()
            .transpose()
            .map_err(|err| corrupt("reading an episode by commit", &err))?;
        Self::with_members(self, found)
    }

    /// Fill in one row's member list, leaving `None` alone. The single-row reads all want this: the rows
    /// themselves come from one `SELECT`, and the members are one cheap indexed lookup apiece.
    fn with_members(&self, found: Option<EpisodeRow>) -> Result<Option<EpisodeRow>, Error> {
        match found {
            Some(mut row) => {
                let id = row.id.clone();
                row.members = self.episode_members(&id)?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }

    /// The distinct authors in the episode set, with counts — the filter chooser's list.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn episode_authors(&self) -> Result<Vec<(String, i64)>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT author_name, COUNT(*) FROM episode WHERE live = 1
                 GROUP BY author_name ORDER BY COUNT(*) DESC, author_name",
            )
            .map_err(|err| corrupt("reading episode authors", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|err| corrupt("reading episode authors", &err))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("reading episode authors", &err))
    }

    /// The paths the given episodes touched, as `episode id → paths`, sorted and deduplicated.
    ///
    /// This is a *join*, not a stored column: the store records paths per commit (`git_commit_file`)
    /// and an episode is a set of commits, so an episode's paths are their union. It is read as one
    /// statement over `episode_member ⋈ git_commit_file` for the ids asked for rather than one per
    /// episode, for the reason [`Store::episode_inputs`] gives for its own single pass — a read whose
    /// cost grows with the rows it describes is a read a first frame cannot afford, and `reco-wzt`
    /// fills a whole page of the rail before that frame is drawn.
    ///
    /// An episode with no file rows is *absent* from the map rather than present and empty, because
    /// "the index recorded nothing for it" and "this episode touches nothing" are different claims and
    /// the card prints the first as `paths unknown` (ADR-001 §7.7).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if any read fails.
    pub fn episode_paths(
        &self,
        ids: &[String],
    ) -> Result<std::collections::BTreeMap<String, Vec<String>>, Error> {
        // Bound on the placeholder list, not a guess at SQLite's `SQLITE_LIMIT_VARIABLE_NUMBER`: 999 is
        // the historical floor, and half of it leaves room for whatever else a statement grows.
        const CHUNK: usize = 500;
        let mut out: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for chunk in ids.chunks(CHUNK) {
            let marks = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT DISTINCT m.episode_id, f.new_path \
                 FROM episode_member m JOIN git_commit_file f ON f.commit_oid = m.commit_oid \
                 WHERE m.episode_id IN ({marks}) ORDER BY m.episode_id, f.new_path"
            );
            let mut statement = self
                .connection()
                .prepare(&sql)
                .map_err(|err| corrupt("reading episode paths", &err))?;
            let rows = statement
                .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|err| corrupt("reading episode paths", &err))?;
            for row in rows {
                let (id, path) = row.map_err(|err| corrupt("reading episode paths", &err))?;
                out.entry(id).or_default().push(path);
            }
        }
        Ok(out)
    }

    /// The summed per-file change of each given episode, as [`EpisodeFiles`].
    ///
    /// [`Store::episode_paths`] answers "which files"; this answers "what happened to each of them",
    /// which is the difference between an episode that added 900 lines and one that added 520 and
    /// deleted 380 of the test suite. It is one grouped statement over
    /// `episode_member ⋈ git_commit_file` for the ids asked for, for the same reason
    /// [`Store::episode_paths`] is one statement: a read whose cost grows with the episodes it
    /// describes is a read a first frame cannot afford.
    ///
    /// A `SUM`, not a raw dump: an episode whose four commits all edited `pricing.py` is one row with
    /// four commits on it. A path whose class the indexer actually decided wins over a sibling commit
    /// where it wrote `unknown`, so a partially-classified index is not read as an unclassified one.
    ///
    /// The read is capped at [`Store::EPISODE_FILE_CAP`] grouped rows and says whether it hit the cap,
    /// because a 10 000-commit repository's page of episodes can hold more file rows than a frame
    /// should be holding (ADR-004 §7).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if any read fails.
    pub fn episode_files(&self, ids: &[String]) -> Result<EpisodeFiles, Error> {
        const CHUNK: usize = 500;
        let mut out = EpisodeFiles {
            by_episode: std::collections::BTreeMap::new(),
            rows: 0,
            truncated: false,
        };
        'chunks: for chunk in ids.chunks(CHUNK) {
            let marks = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT m.episode_id, f.new_path, SUM(f.n_add), SUM(f.n_del), COUNT(*), \
                 MAX(f.is_binary), \
                 CASE WHEN SUM(CASE WHEN f.path_class <> 'unknown' THEN 1 ELSE 0 END) > 0 \
                      THEN MAX(CASE WHEN f.path_class <> 'unknown' THEN f.path_class END) \
                      ELSE 'unknown' END, \
                 CASE WHEN SUM(CASE WHEN f.noise_class <> 'unknown' THEN 1 ELSE 0 END) > 0 \
                      THEN MAX(CASE WHEN f.noise_class <> 'unknown' THEN f.noise_class END) \
                      ELSE 'unknown' END \
                 FROM episode_member m JOIN git_commit_file f ON f.commit_oid = m.commit_oid \
                 WHERE m.episode_id IN ({marks}) \
                 GROUP BY m.episode_id, f.new_path \
                 ORDER BY m.episode_id, f.new_path"
            );
            let mut statement = self
                .connection()
                .prepare(&sql)
                .map_err(|err| corrupt("reading episode file stats", &err))?;
            let rows = statement
                .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        EpisodeFile {
                            path: row.get::<_, String>(1)?,
                            add: row.get::<_, i64>(2)?,
                            del: row.get::<_, i64>(3)?,
                            commits: row.get::<_, i64>(4)?,
                            binary: row.get::<_, i64>(5)? != 0,
                            path_class: row.get::<_, String>(6)?,
                            noise_class: row.get::<_, String>(7)?,
                        },
                    ))
                })
                .map_err(|err| corrupt("reading episode file stats", &err))?;
            for row in rows {
                let (id, file) = row.map_err(|err| corrupt("reading episode file stats", &err))?;
                if out.rows >= Self::EPISODE_FILE_CAP {
                    out.truncated = true;
                    break 'chunks;
                }
                out.rows += 1;
                out.by_episode.entry(id).or_default().push(file);
            }
        }
        Ok(out)
    }

    /// The grouped `(episode, path)` rows [`Store::episode_files`] will read before it stops.
    ///
    /// Sized so a page of episodes at the façade's own page ceiling fits in a frame's worth of memory
    /// even in a repository where every commit touches a hundred files; the count is printed when it
    /// binds rather than the read silently coming up short.
    pub const EPISODE_FILE_CAP: u64 = 50_000;

    /// The review marker on one episode, defaulting to `Unread`.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn review_of(&self, id: &str) -> Result<ReviewState, Error> {
        let state: Option<String> = self
            .connection()
            .query_row(
                "SELECT state FROM review_state WHERE episode_id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|err| corrupt("reading review state", &err))?;
        Ok(state
            .map(|value| ReviewState::parse(&value))
            .unwrap_or_default())
    }

    /// Every review marker, as `episode id → state` — the unread set the rail paints.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn review_states(&self) -> Result<std::collections::BTreeMap<String, ReviewState>, Error> {
        let mut statement = self
            .connection()
            .prepare("SELECT episode_id, state FROM review_state")
            .map_err(|err| corrupt("reading review state", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| corrupt("reading review state", &err))?;
        let mut out = std::collections::BTreeMap::new();
        for row in rows {
            let (id, state) = row.map_err(|err| corrupt("reading review state", &err))?;
            out.insert(id, ReviewState::parse(&state));
        }
        Ok(out)
    }

    /// How many episodes are in each review state — the status line's `queue 7 open`.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn review_counts(&self) -> Result<std::collections::BTreeMap<String, i64>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT COALESCE(r.state,'unread'), COUNT(*) FROM episode e
                 LEFT JOIN review_state r ON r.episode_id = e.id
                 WHERE e.live = 1 GROUP BY 1",
            )
            .map_err(|err| corrupt("counting review states", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|err| corrupt("counting review states", &err))?;
        let mut out = std::collections::BTreeMap::new();
        for row in rows {
            let (state, n) = row.map_err(|err| corrupt("counting review states", &err))?;
            out.insert(state, n);
        }
        Ok(out)
    }

    /// Store a correction, replacing any correction already held for that identity (`e4.1`, `reco-w7q`).
    ///
    /// This is what `fathom attribution correct` writes, and what the next index run reads back into
    /// `fathom_ingest::attribute::Policy::corrections` so the label moves for the commits that match it.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn set_attribution_correction(
        &mut self,
        correction: &AttributionCorrection,
        now: i64,
    ) -> Result<(), Error> {
        self.conn_mut()
            .execute(
                "INSERT INTO attribution_correction(identity, kind, agent, note, created_utc)
                 VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(identity) DO UPDATE SET kind=excluded.kind, agent=excluded.agent,
                        note=excluded.note, created_utc=excluded.created_utc",
                params![
                    correction.identity,
                    correction.kind,
                    correction.agent,
                    correction.note,
                    now
                ],
            )
            .map_err(|err| corrupt("writing an attribution correction", &err))?;
        Ok(())
    }

    /// Every stored correction, ordered by identity so a run's policy is reproducible.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn attribution_corrections(&self) -> Result<Vec<AttributionCorrection>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT identity, kind, agent, note, created_utc FROM attribution_correction
                 ORDER BY identity",
            )
            .map_err(|err| corrupt("reading attribution corrections", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok(AttributionCorrection {
                    identity: row.get(0)?,
                    kind: row.get(1)?,
                    agent: row.get(2)?,
                    note: row.get(3)?,
                    created_utc: row.get(4)?,
                })
            })
            .map_err(|err| corrupt("reading attribution corrections", &err))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| corrupt("reading attribution corrections", &err))?);
        }
        Ok(out)
    }

    /// Drop a correction. Returns whether one was there.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn forget_attribution_correction(&mut self, identity: &str) -> Result<bool, Error> {
        let removed = self
            .conn_mut()
            .execute(
                "DELETE FROM attribution_correction WHERE identity = ?1",
                params![identity],
            )
            .map_err(|err| corrupt("forgetting an attribution correction", &err))?;
        Ok(removed > 0)
    }

    /// Stamp a human's review marker on an episode.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn set_review(
        &mut self,
        id: &str,
        state: ReviewState,
        actor: &str,
        note: Option<&str>,
        now: i64,
    ) -> Result<(), Error> {
        let fingerprint: String = self
            .connection()
            .query_row(
                "SELECT fingerprint FROM episode WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|err| corrupt("reading an episode fingerprint", &err))?;
        self.conn_mut()
            .execute(
                "INSERT INTO review_state(episode_id, state, fingerprint, actor, note, updated_utc)
                 VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(episode_id) DO UPDATE SET state=excluded.state, note=excluded.note,
                        actor=excluded.actor, updated_utc=excluded.updated_utc",
                params![id, state.as_str(), fingerprint, actor, note, now],
            )
            .map_err(|err| corrupt("writing review state", &err))?;
        Ok(())
    }

    /// Record one triage act, and move the episode's current marker to match, in one transaction.
    ///
    /// Two writes, never one without the other. The act log is what the retrospective and an audit read;
    /// `review_state` is what the rail's badge and the queue's counts read. Recording only the first
    /// leaves the queue lying about what it holds; recording only the second is the criterion this table
    /// exists to satisfy, so a caller cannot get it wrong: there is no method that writes one alone.
    ///
    /// The `fingerprint` is read here rather than passed in, for the same reason `set_review` reads it: a
    /// caller cannot sign off an episode against a digest it has not been shown, and a stale read in the
    /// caller must not be able to write a stale link into the durable record.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if either write fails — and then neither row landed, which is the only
    /// outcome that lets the shell say "recorded" truthfully.
    pub fn record_triage(&mut self, act: &TriageEvent) -> Result<i64, Error> {
        let fingerprint: String = self
            .connection()
            .query_row(
                "SELECT fingerprint FROM episode WHERE id = ?1",
                params![act.episode],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| corrupt("reading an episode fingerprint", &err))?
            .unwrap_or_else(|| act.fingerprint.clone());
        let tx = self
            .conn_mut()
            .transaction()
            .map_err(|err| corrupt("opening a triage transaction", &err))?;
        let seq: i64 = tx
            .query_row(
                "INSERT INTO triage_event(episode_id, fingerprint, disposition, reason_code, note,
                                          until, actor, risk, risk_model, utc)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) RETURNING seq",
                params![
                    act.episode,
                    fingerprint,
                    act.disposition.as_str(),
                    act.reason,
                    act.note,
                    act.until,
                    act.actor,
                    act.risk,
                    act.risk_model,
                    act.utc
                ],
                |row| row.get(0),
            )
            .map_err(|err| corrupt("writing a triage act", &err))?;
        tx.execute(
            "INSERT INTO review_state(episode_id, state, fingerprint, actor, note, updated_utc)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(episode_id) DO UPDATE SET state=excluded.state, note=excluded.note,
                    actor=excluded.actor, updated_utc=excluded.updated_utc",
            params![
                act.episode,
                act.disposition.as_str(),
                fingerprint,
                act.actor,
                note_text(&act.note, &act.reason, &act.until),
                act.utc
            ],
        )
        .map_err(|err| corrupt("writing review state for a triage act", &err))?;
        tx.commit()
            .map_err(|err| corrupt("committing a triage act", &err))?;
        Ok(seq)
    }

    /// The act log, newest first, capped so a huge ledger cannot outrun a frame budget.
    ///
    /// The cap is reported by [`Store::triage_event_count`] rather than applied silently, and the
    /// retrospective prints it as a caveat: "the last 5 000 acts" and "every act" are different claims.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn triage_events(&self, limit: i64) -> Result<Vec<TriageEvent>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT episode_id, fingerprint, disposition, reason_code, note, until, actor,
                        risk, risk_model, utc
                 FROM triage_event ORDER BY utc DESC, seq DESC LIMIT ?1",
            )
            .map_err(|err| corrupt("reading the triage ledger", &err))?;
        let rows = statement
            .query_map(params![limit], |row| {
                Ok(TriageEvent {
                    episode: row.get(0)?,
                    fingerprint: row.get(1)?,
                    disposition: ReviewState::parse(&row.get::<_, String>(2)?),
                    reason: row.get(3)?,
                    note: row.get(4)?,
                    until: row.get(5)?,
                    actor: row.get(6)?,
                    risk: row.get::<_, Option<i64>>(7)?,
                    risk_model: row.get(8)?,
                    utc: row.get(9)?,
                })
            })
            .map_err(|err| corrupt("reading the triage ledger", &err))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| corrupt("reading the triage ledger", &err))?);
        }
        Ok(out)
    }

    /// How many acts the ledger holds, which is how a caller knows [`Store::triage_events`] cut it short.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn triage_event_count(&self) -> Result<i64, Error> {
        self.connection()
            .query_row("SELECT count(*) FROM triage_event", [], |row| row.get(0))
            .map_err(|err| corrupt("counting the triage ledger", &err))
    }

    /// The last act recorded for one episode, or `None` when nothing has been done with it.
    ///
    /// This is what makes a snooze checkable: the stored `until` carries the epoch or session count it was
    /// stamped against, and a released snooze is an open row again.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn last_triage(&self, id: &str) -> Result<Option<TriageEvent>, Error> {
        self.connection()
            .query_row(
                "SELECT episode_id, fingerprint, disposition, reason_code, note, until, actor,
                        risk, risk_model, utc
                 FROM triage_event WHERE episode_id = ?1 ORDER BY utc DESC, seq DESC LIMIT 1",
                params![id],
                |row| {
                    Ok(TriageEvent {
                        episode: row.get(0)?,
                        fingerprint: row.get(1)?,
                        disposition: ReviewState::parse(&row.get::<_, String>(2)?),
                        reason: row.get(3)?,
                        note: row.get(4)?,
                        until: row.get(5)?,
                        actor: row.get(6)?,
                        risk: row.get::<_, Option<i64>>(7)?,
                        risk_model: row.get(8)?,
                        utc: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(|err| corrupt("reading an episode's last triage act", &err))
    }

    /// How often each dismiss code has been used — the "learnable" half of the vocabulary, and the
    /// numbers that make the retrospective's rollup a query rather than a scan.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn dismiss_usage(&self) -> Result<std::collections::BTreeMap<String, i64>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT reason_code, count(*) FROM triage_event
                 WHERE disposition = 'dismissed' AND reason_code <> ''
                 GROUP BY 1 ORDER BY 2 DESC, 1 ASC",
            )
            .map_err(|err| corrupt("counting dismiss reasons", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|err| corrupt("counting dismiss reasons", &err))?;
        let mut out = std::collections::BTreeMap::new();
        for row in rows {
            let (code, count) = row.map_err(|err| corrupt("counting dismiss reasons", &err))?;
            out.insert(code, count);
        }
        Ok(out)
    }

    /// Read one `session_state` value.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn ui_state(&self, key: &str) -> Result<Option<String>, Error> {
        self.connection()
            .query_row(
                "SELECT value FROM session_state WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|err| corrupt("reading session state", &err))
    }

    /// Every `session_state` key starting with a prefix, as `key → value`.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn ui_state_all(&self, prefix: &str) -> Result<Vec<(String, String)>, Error> {
        Ok(self
            .ui_state_rows(prefix)?
            .into_iter()
            .map(|(key, value, _)| (key, value))
            .collect())
    }

    /// Every `session_state` key starting with a prefix, as `(key, value, updated_utc)`.
    ///
    /// The stamp is what makes a resumed session able to say *when* a value was written, which is the
    /// difference between "your pane sizes came back" and "another fathom session was here 4 s ago and
    /// its answer is the one on screen" (`e1.11`).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn ui_state_rows(&self, prefix: &str) -> Result<Vec<(String, String, i64)>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT key, value, updated_utc FROM session_state WHERE key LIKE ?1 ORDER BY key",
            )
            .map_err(|err| corrupt("reading session state", &err))?;
        let rows = statement
            .query_map(params![format!("{prefix}%")], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|err| corrupt("reading session state", &err))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("reading session state", &err))
    }

    /// Write one `session_state` value, and delete it when the value is `None` (idempotent either way).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn set_ui_state(&mut self, key: &str, value: Option<&str>, now: i64) -> Result<(), Error> {
        match value {
            Some(value) => self.conn_mut().execute(
                "INSERT INTO session_state(key, value, updated_utc) VALUES (?1,?2,?3)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_utc=excluded.updated_utc",
                params![key, value, now],
            ),
            None => self
                .conn_mut()
                .execute("DELETE FROM session_state WHERE key = ?1", params![key]),
        }
        .map(|_| ())
        .map_err(|err| corrupt("writing session state", &err))
    }

    /// Write one `session_state` row **only if it still holds what the caller read** — the compare half
    /// of a compare-and-set — and say whether the row was written.
    ///
    /// This is `e1.11`'s concurrent-TUI rule, pushed down to the one place that can enforce it. Two
    /// fathom sessions on one repository both read the same `session_state`, and the one that quits
    /// last must not be able to overwrite what the other one wrote in between: `fathom.toml`-style
    /// "last writer wins on the whole blob" loses a filter, a cursor and a read set with no trace, and
    /// a reviewer who signed nothing off finds their queue rewritten. So the writer states the value it
    /// believes is there (`expected`, `None` for "there was no row"), and a row that moved in between is
    /// left alone and reported — the loser keeps its own screen and is told its write did not land,
    /// which is a fact it can act on, instead of a silent clobber.
    ///
    /// Every case is one statement, so the check and the write cannot be split by another writer:
    ///
    /// | `expected` | `value` | Statement | `Ok(true)` means |
    /// |---|---|---|---|
    /// | `Some(e)` | `Some(v)` | `UPDATE … WHERE key AND value = e` | the row still held `e`, and now holds `v` |
    /// | `Some(e)` | `None` | `DELETE … WHERE key AND value = e` | the row still held `e`, and is gone |
    /// | `None` | `Some(v)` | `INSERT … ON CONFLICT DO NOTHING` | there was no row, and now there is |
    /// | `None` | `None` | nothing | nothing to do |
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn set_ui_state_if(
        &mut self,
        key: &str,
        expected: Option<&str>,
        value: Option<&str>,
        now: i64,
    ) -> Result<bool, Error> {
        let changed = match (expected, value) {
            (None, None) => return Ok(true),
            (None, Some(value)) => self.conn_mut().execute(
                "INSERT INTO session_state(key, value, updated_utc) VALUES (?1,?2,?3)
                 ON CONFLICT(key) DO NOTHING",
                params![key, value, now],
            ),
            (Some(expected), Some(value)) => self.conn_mut().execute(
                "UPDATE session_state SET value = ?2, updated_utc = ?3
                 WHERE key = ?1 AND value = ?4",
                params![key, value, now, expected],
            ),
            (Some(expected), None) => self.conn_mut().execute(
                "DELETE FROM session_state WHERE key = ?1 AND value = ?2",
                params![key, expected],
            ),
        }
        .map_err(|err| corrupt("writing session state", &err))?;
        Ok(changed > 0)
    }

    /// The rewrite ledger, newest first: `(old, new, relation, method)`.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn episode_rewrites(&self) -> Result<Vec<(String, String, String, String)>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT old_id, new_id, relation, method FROM episode_rewrite
                 ORDER BY at_utc DESC, old_id LIMIT 200",
            )
            .map_err(|err| corrupt("reading episode rewrites", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|err| corrupt("reading episode rewrites", &err))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("reading episode rewrites", &err))
    }
}

/// The per-commit facts episode segmentation needs, in the store's own vocabulary.
#[derive(Debug, Clone)]
pub struct EpisodeInput {
    /// The commit id.
    pub oid: String,
    /// Parents, first parent first.
    pub parents: Vec<String>,
    /// The author name and address as committed.
    pub author_name: String,
    /// See [`EpisodeInput::author_name`].
    pub author_email: String,
    /// The author date, unix millis.
    pub author_utc: i64,
    /// The committer name as committed — an attribution input (`e4.1`), empty on rows stored before
    /// migration 4.
    pub committer_name: String,
    /// See [`EpisodeInput::committer_name`].
    pub committer_email: String,
    /// The first line of the message.
    pub subject: String,
    /// The trailer block, lower-cased keys.
    pub trailers: std::collections::BTreeMap<String, String>,
    /// The paths this commit touched, in the store's quoted form.
    pub paths: Vec<String>,
    /// Files touched.
    pub n_files: i64,
    /// Lines added.
    pub n_add: i64,
    /// Lines removed.
    pub n_del: i64,
    /// Binary files touched.
    pub n_binary: i64,
}

impl Store {
    /// Every commit row, topologically ordered, with the paths each touched — the input segmentation reads.
    ///
    /// Only commits the current refs can reach are returned. The spine is append-only: a rebase, an amend or
    /// a force-push leaves the commits it replaced sitting in `git_commit` (they are the audit trail of what
    /// was indexed, and `fathom stats` counts them), and a segmentation that read every row would present a
    /// reviewer with the same change twice — the one they are reviewing and the ghost its rewrite replaced.
    /// Reachability is computed from the stored parent pointers rather than from git, so a commit that is
    /// stored without its parents (a shallow store, or a corrupt one) is kept rather than dropped.
    ///
    /// The order is a real topological order over `parent_oids` (parents before children), with author date
    /// and oid as the tie-break so that two runs over the same store produce byte-identical episodes. A
    /// cycle in the stored parent pointers — which git forbids and a corrupt store can do — cannot be
    /// ordered; the remainder is appended in date order rather than dropped, because a segmentation that
    /// silently skips commits is worse than one that reviews them in a strange order.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn episode_inputs(&self) -> Result<Vec<EpisodeInput>, Error> {
        let mut statement = self.connection().prepare(
            "SELECT oid, parent_oids, author_name, author_email, author_utc, message_head, trailers,
                    n_files, n_add, n_del, n_binary, committer_name, committer_email FROM git_commit
             ORDER BY author_utc, oid",
        ).map_err(|err| corrupt("reading episode inputs", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                ))
            })
            .map_err(|err| corrupt("reading episode inputs", &err))?;
        let mut inputs: Vec<EpisodeInput> = Vec::new();
        for row in rows {
            let (
                oid,
                parents,
                author_name,
                author_email,
                author_utc,
                subject,
                trailers,
                n_files,
                n_add,
                n_del,
                n_binary,
                committer_name,
                committer_email,
            ) = row.map_err(|err| corrupt("reading episode inputs", &err))?;
            inputs.push(EpisodeInput {
                oid,
                parents: serde_json::from_str(&parents).unwrap_or_default(),
                author_name,
                author_email,
                author_utc,
                committer_name,
                committer_email,
                subject,
                trailers: serde_json::from_str(&trailers).unwrap_or_default(),
                paths: Vec::new(),
                n_files,
                n_add,
                n_del,
                n_binary,
            });
        }
        drop(statement);
        // One pass for the paths, keyed by commit, rather than one query per commit.
        let mut statement = self
            .connection()
            .prepare(
                "SELECT commit_oid, new_path FROM git_commit_file ORDER BY commit_oid, new_path",
            )
            .map_err(|err| corrupt("reading episode paths", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| corrupt("reading episode paths", &err))?;
        let mut by_oid: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for row in rows {
            let (oid, path) = row.map_err(|err| corrupt("reading episode paths", &err))?;
            by_oid.entry(oid).or_default().push(path);
        }
        for input in &mut inputs {
            if let Some(paths) = by_oid.get(&input.oid) {
                input.paths.clone_from(paths);
            }
        }
        retain_reachable(&mut inputs, &self.ref_tip_oids()?);
        topological_order(&mut inputs);
        Ok(inputs)
    }

    /// The oids the refs currently name — the roots the reachability walk starts from.
    ///
    /// Tags are not included: a tag pinned to a rebased-away commit is a fact about the release that commit
    /// was, not a request to keep showing it in the review queue.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn ref_tip_oids(&self) -> Result<std::collections::BTreeSet<String>, Error> {
        let mut statement = self
            .connection()
            .prepare("SELECT oid FROM ref_tip WHERE kind IN ('head','branch')")
            .map_err(|err| corrupt("reading ref tips", &err))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|err| corrupt("reading ref tips", &err))?;
        rows.collect::<Result<_, _>>()
            .map_err(|err| corrupt("reading ref tips", &err))
    }

    /// The ref the current walk ordered its episodes on, for `episode.lineage_ref`.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn primary_lineage(&self) -> Result<String, Error> {
        let head: Option<String> = self
            .connection()
            .query_row(
                "SELECT name FROM ref_tip WHERE kind = 'head' ORDER BY name LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| corrupt("reading the head ref", &err))?;
        if let Some(head) = head {
            return Ok(head);
        }
        Ok(self
            .connection()
            .query_row(
                "SELECT name FROM ref_tip WHERE kind = 'branch' ORDER BY name LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| corrupt("reading the branch refs", &err))?
            .unwrap_or_else(|| "HEAD".to_owned()))
    }
}

/// Keep only the commits the current refs can reach, walking the stored parent pointers backwards.
///
/// No refs at all means no opinion: a store with commit rows and no `ref_tip` rows (a test that seeded
/// commits directly, an index whose walk found a ref it could not name) keeps every commit rather than
/// silently segmenting nothing. Nor does a ref whose own commit row is missing: see the body, where a tip
/// that resolves to nothing is treated as no opinion rather than an empty history.
fn retain_reachable(inputs: &mut Vec<EpisodeInput>, tips: &std::collections::BTreeSet<String>) {
    use std::collections::{BTreeMap, BTreeSet};
    if tips.is_empty() {
        return;
    }
    // A tip this store has no row for is not a tip with an empty history behind it — it is a tip the walk
    // could not read, which is what a corrupt or filter-omitted object leaves behind (`reco-7f4`). Treating
    // it as "reachable: nothing" blanks the review surface for every *readable* commit in the repository,
    // which is the opposite of what a partial read should mean. So: no tip resolves, no opinion, and the
    // inputs are kept whole, exactly as when there were no tips at all.
    let known: std::collections::HashSet<&str> =
        inputs.iter().map(|input| input.oid.as_str()).collect();
    if !tips.iter().any(|tip| known.contains(tip.as_str())) {
        return;
    }
    let seen: BTreeSet<String> = {
        let by_oid: BTreeMap<&str, &EpisodeInput> = inputs
            .iter()
            .map(|input| (input.oid.as_str(), input))
            .collect();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut queue: Vec<String> = tips
            .iter()
            .filter(|tip| by_oid.contains_key(tip.as_str()))
            .cloned()
            .collect();
        while let Some(oid) = queue.pop() {
            if !seen.insert(oid.clone()) {
                continue;
            }
            let Some(input) = by_oid.get(oid.as_str()) else {
                continue;
            };
            queue.extend(
                input
                    .parents
                    .iter()
                    .filter(|parent| {
                        by_oid.contains_key(parent.as_str()) && !seen.contains(*parent)
                    })
                    .cloned(),
            );
        }
        seen
    };
    inputs.retain(|input| seen.contains(&input.oid));
}

/// Kahn's algorithm over the stored parent pointers, with (author date, oid) as the tie-break.
fn topological_order(inputs: &mut Vec<EpisodeInput>) {
    use std::collections::BTreeMap;
    let index: BTreeMap<&str, usize> = inputs
        .iter()
        .enumerate()
        .map(|(position, input)| (input.oid.as_str(), position))
        .collect();
    let mut indegree: Vec<usize> = vec![0; inputs.len()];
    for (position, input) in inputs.iter().enumerate() {
        for parent in &input.parents {
            if index
                .get(parent.as_str())
                .copied()
                .is_some_and(|parent| parent != position)
            {
                indegree[position] += 1;
            }
        }
    }
    let mut ready: Vec<usize> = (0..inputs.len())
        .filter(|position| indegree[*position] == 0)
        .collect();
    let mut ordered: Vec<EpisodeInput> = Vec::with_capacity(inputs.len());
    let mut emitted = vec![false; inputs.len()];
    while !ready.is_empty() {
        ready.sort_by(|a, b| {
            let left = &inputs[*a];
            let right = &inputs[*b];
            (left.author_utc, &left.oid).cmp(&(right.author_utc, &right.oid))
        });
        let position = ready.remove(0);
        emitted[position] = true;
        ordered.push(inputs[position].clone());
        for (child, input) in inputs.iter().enumerate() {
            if !emitted[child]
                && input
                    .parents
                    .iter()
                    .any(|parent| index.get(parent.as_str()) == Some(&position))
            {
                indegree[child] = indegree[child].saturating_sub(1);
                if indegree[child] == 0 {
                    ready.push(child);
                }
            }
        }
    }
    // The cycle remainder, if the parent pointers lie: date order, appended, never dropped.
    for (position, input) in inputs.iter().enumerate() {
        if !emitted[position] {
            ordered.push(input.clone());
        }
    }
    *inputs = ordered;
}

/// The columns [`EpisodeRow`] reads, in order, shared by every episode read so a projection change is one
/// edit rather than four.
const EPISODE_COLUMNS: &str = "id, fingerprint, lineage_ref, lineage_seq, created_utc, created_source, \
     session_id, turn_id, boundary_rule, boundary_confidence, author_kind, author_confidence, \
     fallback_fingerprint, shape_fingerprint, first_oid, last_oid, n_files, n_add, n_del, n_binary, first_utc, last_utc, \
     author_name, author_email, summary, overridden, live, noise_share, kind,
     created_confidence, attribution_source, attribution_agent, attribution_evidence, attribution_reason,
     intent, intent_state, intent_source, intent_confidence, intent_text, intent_reason";

/// The `episode_doc.body` for one episode: the summary, the intent text, then the plan items and the
/// declared globs out of the intent document.
///
/// The plan list is in here because that is where a harness usually *puts* the intent — a search that
/// indexed only the winning line would miss the item that lost the ladder, and the whole point of the
/// document is that a person can find the episode by any phrase anybody wrote down about it.
#[must_use]
pub fn document_body(summary: &str, intent_text: &str, intent_json: &str) -> String {
    let mut out = String::new();
    out.push_str(summary);
    out.push('\n');
    if !intent_text.is_empty() {
        out.push_str(intent_text);
        out.push('\n');
    }
    if let Ok(doc) = serde_json::from_str::<serde_json::Value>(intent_json) {
        if let Some(items) = doc.get("plan").and_then(serde_json::Value::as_array) {
            for item in items.iter().filter_map(serde_json::Value::as_object) {
                if let Some(text) = item.get("text").and_then(serde_json::Value::as_str) {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
        if let Some(items) = doc.get("scope").and_then(serde_json::Value::as_array) {
            for glob in items.iter().filter_map(serde_json::Value::as_str) {
                out.push_str(glob);
                out.push('\n');
            }
        }
    }
    out
}

fn episode_sql_for_id() -> String {
    format!("SELECT {EPISODE_COLUMNS} FROM episode WHERE id = ?1")
}

/// Build the filter's SQL. Placeholders only — the filter values are user text and never spliced in.
fn episode_sql(query: &EpisodeQuery, count: bool) -> (String, Vec<SqlValue>) {
    let mut where_ = vec![if query.include_dead {
        "1 = 1".to_owned()
    } else {
        "live = 1".to_owned()
    }];
    let mut args: Vec<SqlValue> = Vec::new();
    if !query.authors.is_empty() {
        let marks = vec!["?"; query.authors.len()].join(",");
        where_.push(format!("author_name IN ({marks})"));
        args.extend(query.authors.iter().map(|a| SqlValue::Text(a.clone())));
    }
    if !query.kinds.is_empty() {
        let marks = vec!["?"; query.kinds.len()].join(",");
        where_.push(format!("author_kind IN ({marks})"));
        args.extend(query.kinds.iter().map(|k| SqlValue::Text(k.clone())));
    }
    if let Some(state) = query.state {
        if state == ReviewState::Unread {
            where_.push(
                "id NOT IN (SELECT episode_id FROM review_state WHERE state != 'unread')"
                    .to_owned(),
            );
        } else {
            where_.push("id IN (SELECT episode_id FROM review_state WHERE state = ?)".to_owned());
            args.push(SqlValue::Text(state.as_str().to_owned()));
        }
    }
    if let Some(path) = &query.path {
        where_.push(
            "id IN (SELECT m.episode_id FROM episode_member m
                    JOIN git_commit_file f ON f.commit_oid = m.commit_oid
                    WHERE f.new_path LIKE ?)"
                .to_owned(),
        );
        args.push(SqlValue::Text(format!("%{path}")));
    }
    if let Some(since) = query.since_utc {
        where_.push("last_utc >= ?".to_owned());
        args.push(SqlValue::Integer(since));
    }
    if let Some(until) = query.until_utc {
        where_.push("last_utc < ?".to_owned());
        args.push(SqlValue::Integer(until));
    }
    let predicate = where_.join(" AND ");
    if count {
        return (
            format!("SELECT COUNT(*) FROM episode WHERE {predicate}"),
            args,
        );
    }
    let ordering = "ORDER BY last_utc DESC, id";
    // A non-positive limit means "the rest", which is what a caller who did not think about paging means;
    // paging is explicit (`offset` with a positive `limit`), never an accident of a default-constructed
    // query silently returning one row.
    if query.limit > 0 || query.offset > 0 {
        args.push(SqlValue::Integer(query.limit.max(-1)));
        args.push(SqlValue::Integer(query.offset));
        return (
            format!(
                "SELECT {EPISODE_COLUMNS} FROM episode WHERE {predicate} {ordering} LIMIT ? OFFSET ?"
            ),
            args,
        );
    }
    (
        format!("SELECT {EPISODE_COLUMNS} FROM episode WHERE {predicate} {ordering}"),
        args,
    )
}

fn row_to_episode(row: &rusqlite::Row<'_>) -> rusqlite::Result<EpisodeRow> {
    Ok(EpisodeRow {
        id: row.get(0)?,
        fingerprint: row.get(1)?,
        lineage_ref: row.get(2)?,
        lineage_seq: row.get(3)?,
        created_utc: row.get(4)?,
        created_source: row.get(5)?,
        session_id: row.get(6)?,
        turn_id: row.get(7)?,
        boundary_rule: row.get(8)?,
        boundary_confidence: row.get(9)?,
        author_kind: row.get(10)?,
        author_confidence: row.get(11)?,
        fallback_fingerprint: row.get(12)?,
        shape_fingerprint: row.get(13)?,
        first_oid: row.get(14)?,
        last_oid: row.get(15)?,
        n_files: row.get(16)?,
        n_add: row.get(17)?,
        n_del: row.get(18)?,
        n_binary: row.get(19)?,
        first_utc: row.get(20)?,
        last_utc: row.get(21)?,
        author_name: row.get(22)?,
        author_email: row.get(23)?,
        summary: row.get(24)?,
        overridden: row.get::<_, i64>(25)? != 0,
        live: row.get::<_, i64>(26)? != 0,
        // Read but not stored on the row: the noise share is `e2.2`'s, and until then it is 0.
        noise_share: row.get(27)?,
        kind: row.get(28)?,
        created_confidence: row.get(29)?,
        members: Vec::new(),
        epoch: 0,
        attribution_source: row.get(30)?,
        attribution_agent: row.get(31)?,
        attribution_evidence: row.get(32)?,
        attribution_reason: row.get(33)?,
        intent: row.get(34)?,
        intent_state: row.get(35)?,
        intent_source: row.get(36)?,
        intent_confidence: row.get(37)?,
        intent_text: row.get(38)?,
        intent_reason: row.get(39)?,
    })
}

/// Carry review state from dead episodes to the live episodes that replaced them.
///
/// Three methods, tried in order of trust, each one only consulted for episodes the stronger ones did not
/// already place:
///
/// 1. a shared `fallback_fingerprint` — same author, same subject, different paths: an amend that added a
///    file, or a squash that kept the subject line.
/// 2. a shared `shape_fingerprint` — same author, same paths, different subject: a reword, including one
///    performed in the middle of an interactive rebase.
/// 3. a shared member commit — the digest moved entirely, but a commit is still in both episodes: a split,
///    or a rebase that rewrote the subject *and* moved files.
///
/// A live episode is claimed by the first tier that finds a candidate, and a dead episode's decision is
/// copied at most once, so the weaker evidence can never overwrite the stronger. "Closest in time" is the
/// tie-break within a tier because it is the only ordering a rewrite leaves us with.
fn relink_review_state(conn: &Connection, now: i64) -> Result<usize, rusqlite::Error> {
    let mut relinked = 0_usize;
    let mut claimed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // (1) and (2): one query per digest, because a JOIN the optimizer can use is worth more than a clever
    // single statement that it cannot.
    for (digest, relation, method, score) in [
        ("fallback_fingerprint", "rebase", "fingerprint", 0.9),
        ("shape_fingerprint", "rebase", "shape", 0.7),
    ] {
        let rows: Vec<(String, String)> = conn
            .prepare(&format!(
                "SELECT n.id, o.id FROM episode n
                   JOIN episode o
                     ON o.{digest} = n.{digest}
                    AND o.id != n.id AND o.live = 0 AND o.{digest} != ''
                   JOIN review_state r ON r.episode_id = o.id
                  WHERE n.live = 1
                    AND NOT EXISTS (SELECT 1 FROM review_state q WHERE q.episode_id = n.id)
                  ORDER BY n.id, o.last_utc DESC, o.created_utc DESC"
            ))?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<_, _>>()?;
        for (new_id, old_id) in rows {
            if !claimed.insert(new_id.clone()) {
                continue;
            }
            carry(conn, &old_id, &new_id, relation, method, score, now)?;
            relinked += 1;
        }
    }
    // (3) squash/split: a live episode whose members were dead episodes' members.
    let mut statement = conn.prepare(
        "SELECT m.commit_oid, o.id, n.id FROM episode_member m
           JOIN episode o ON o.id = m.episode_id AND o.live = 0
           JOIN review_state r ON r.episode_id = o.id
           JOIN episode_member m2 ON m2.commit_oid = m.commit_oid
           JOIN episode n ON n.id = m2.episode_id AND n.live = 1
          WHERE o.id != n.id",
    )?;
    let rows: Vec<(String, String, String)> = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    drop(statement);
    let mut done = std::collections::BTreeSet::new();
    for (_oid, old_id, new_id) in rows {
        if !claimed.insert(new_id.clone()) || !done.insert((old_id.clone(), new_id.clone())) {
            continue;
        }
        let fresh: i64 = conn.query_row(
            "SELECT COUNT(*) FROM review_state WHERE episode_id = ?1",
            params![new_id],
            |row| row.get(0),
        )?;
        if fresh == 0 {
            carry(conn, &old_id, &new_id, "squash", "member-set", 0.6, now)?;
            relinked += 1;
        }
    }
    Ok(relinked)
}

/// Copy one review row forward and record the rewrite that justified it.
fn carry(
    conn: &Connection,
    old_id: &str,
    new_id: &str,
    relation: &str,
    method: &str,
    score: f64,
    now: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO review_state(episode_id, state, fingerprint, actor, note, updated_utc)
         SELECT ?1, state, (SELECT fingerprint FROM episode WHERE id = ?1), actor, note, ?3
           FROM review_state WHERE episode_id = ?2
         ON CONFLICT(episode_id) DO NOTHING",
        params![new_id, old_id, now],
    )?;
    conn.execute(
        "INSERT INTO episode_rewrite(old_id, new_id, relation, method, score, at_utc)
         VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(old_id, new_id) DO NOTHING",
        params![old_id, new_id, relation, method, score, now],
    )?;
    Ok(())
}

/// Drop dead episodes nobody has a decision about, and the rewrites that pointed at them.
fn retire_dead_episodes(conn: &Connection) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "DELETE FROM episode WHERE live = 0
           AND id NOT IN (SELECT episode_id FROM review_state)
           AND id NOT IN (SELECT new_id FROM episode_rewrite)",
        [],
    )
}

#[cfg(test)]
mod tests {
    use super::note_text;

    /// The marker's note is the act's claim, in the act's own words when it had any.
    ///
    /// A unit test rather than an integration one because this is a pure choice about wording a stored
    /// column, and the difference it encodes — `NULL` for "nothing was written" against `""` for "an empty
    /// note was" — is exactly the kind of corner an end-to-end test reads as a passing row.
    #[test]
    fn the_markers_note_says_what_the_act_said_and_null_when_it_said_nothing() {
        assert_eq!(
            note_text("", "not-my-area", "").as_deref(),
            Some("not-my-area"),
            "a dismissal's reason is its claim, and a marker read without the ledger should still state it"
        );
        assert_eq!(
            note_text("the pool is being drained", "", "").as_deref(),
            Some("the pool is being drained"),
            "a flag's note is the whole act"
        );
        assert_eq!(
            note_text("", "", "next-index:41").as_deref(),
            Some("until next-index:41"),
            "a snooze says what it is waiting for, baseline included"
        );
        assert_eq!(
            note_text("", "", ""),
            None,
            "a bare accept has nothing to say, and `NULL` is how 'nothing was written' is stored — \
             distinct from an empty note, which would be a note"
        );
    }
}

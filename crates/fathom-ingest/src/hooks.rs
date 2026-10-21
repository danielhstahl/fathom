//! `hooks.rs` — `e6.1` (`reco-afh`): the read side of the hooks v1 contract
//! ([ADR-005](../../../docs/adr/0005-harness-contract.md)).
//!
//! A coding harness appends one JSON object per line to `.fathom/events.jsonl`. This module is what
//! *reads* that file: it tolerates a line caught mid-write, survives two writers at once, remembers how
//! far it got so a restart does not reprocess, refuses records that fail the schema instead of trusting
//! them, quarantines the ones that fail later with the diagnostics attached, and can rebuild everything
//! from byte 0 for a `--rebuild`-style reindex.
//!
//! # The four properties the ticket is named for
//!
//! | Property | Where it lives | Refused failure |
//! |---|---|---|
//! | a torn or mid-write line is neither lost nor fatal | [`Reader::read_after`] holding a [`Scanned`] tail incomplete | a reader that decodes a half-written object, or that dies on one |
//! | malformed events are quarantined and counted while the stream keeps flowing | [`Rejection`], [`Counters::rejections`], [`Quarantine`] | a pipeline that stops at the first bad line, or that drops it silently |
//! | a durable offset means a restart does not reprocess | [`Cursor`], persisted by [`Reader::save_cursor`] | double-counted evidence after every crash |
//! | replay from offset 0 reproduces *identical* episode linkage | [`Ledger`], [`attribute`], [`Linkage::digest`] | a rebuild that quietly renumbers turns, orphaning every review keyed to them |
//!
//! # The two facts this module exists to produce
//!
//! Everything else is plumbing for these two:
//!
//! 1. **turns** — [`Ledger`] allocates `turn_id = "<session_id>:<turn_index>"` **at ingest** (ADR-005
//!    §3.3, ADR-003 §9.1), never from a writer's own counter, and closes a turn that never ended with the
//!    next `turn_start` (`closed_by: "next-turn"`) so a crash cannot orphan one. The id is
//!    **session-qualified on purpose**: two sessions that both reach turn 3 are two turns, and an id
//!    spelled `3` reads as one (ADR-003 §18.2 **D4** — the defect is closed by the qualification here and
//!    by nothing else).
//! 2. **commit ↔ turn linkage** — [`attribute`] maps commits onto turns by the turn's time window plus
//!    the paths it touched, and [`fill_turn_ids`] writes the result into
//!    [`CommitFacts::turn_id`](crate::segment::CommitFacts::turn_id) so ADR-003 §4.2's rung 2 (`hook`,
//!    confidence 1.0) finally has something to fire on. Spike `s2` measured that rung at 0.000 recall on
//!    real history as shipped (ADR-003 §18.2 **D3**): this is the missing input, which is why this ticket
//!    is P1 rather than P2.
//!
//! # The trailer this module writes (ADR-003 §18.2 **D5**)
//!
//! [`trailer_block`] emits the `Agent-Session:` / `Agent-Turn:` pair, with the **session-qualified** turn
//! id, and [`crate::segment`]'s rung 3 reads [`TRAILER_TURN_KEY`] — the same constant, not a copy of the
//! same string. `Fathom-Episode:` ([`TRAILER_EPISODE_KEY`]) is *reserved*: the day Fathom itself declares
//! an episode start is when it gets written, and adding a third spelling is an amendment to ADR-003 §4.2,
//! not a new enum value.
//!
//! # What this module never does
//!
//! * **Reads no clock.** Every timestamp is passed in (`now_ms`), per ADR-002 §4.4 — only `fathom-cli`
//!   reads time, so "how far behind is the stream" is answerable without two clocks disagreeing.
//! * **Reads no config.** [`Limits`] and the path arrive as values (ADR-002 §4.5).
//! * **Runs nothing it was told to.** ADR-005 §6 rule 2: `test_run.command` is stored as a string and only
//!   ever displayed. Nothing here `eval`s, imports, shell-parses or follows a path out of the session
//!   directory ([`contain_path`]).
//! * **Rewrites the events file.** Damage is counted and copied to a quarantine sidecar; the file itself
//!   is append-only forever (ADR-005 §3.6).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::io::SeekFrom;
use std::io::{Read as _, Seek as _};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::segment::CommitFacts;

/// The wire protocol this reader speaks (ADR-005 §5.1). `fathom hooks --json` prints it so a reader can
/// tell which contract a report was produced under.
pub const PROTOCOL: &str = "fathom-hooks/1";

/// The canonical event schema version (ADR-005 §5.1: `fathom_event_schema = 1`).
pub const EVENT_SCHEMA: u64 = 1;

/// The line cap: a longer line is refused at read time, never truncated (ADR-005 §3.6). The writer's
/// `MAX_LINE_BYTES` in `src/recommender/harness/harness.py` is the same number, and
/// `tests/test_hooks_contract.py` fails if the two ever disagree.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// The per-session ingest budget (ADR-005 §6 rule 3, `harness.max_bytes_per_session_mib`). Over quota the
/// session is quarantined, not trusted.
pub const DEFAULT_MAX_BYTES_PER_SESSION: u64 = 64 * 1024 * 1024;

/// How many quarantined records to keep before the oldest are dropped (with the drop counted, never
/// silently): the quarantine is a diagnostic, and a diagnostic that fills the disk is a second outage.
pub const DEFAULT_QUARANTINE_RECORDS: usize = 512;

/// How many turns a cursor carries across a restart. Attribution only ever needs recent turns — a commit
/// is written during or after its turn — and an unbounded cursor would grow with the events file.
pub const DEFAULT_TURNS_KEPT: usize = 4096;

// The trailer keys, defined once in [`crate::segment`] next to the rule that reads them and re-exported
// here because this is the module that *writes* them. One definition on each side of the boundary is what
// makes ADR-003 §18.2 **D5** ("the writer and rung 3 use the same key") a fact instead of a grep.
pub use crate::segment::{
    TRAILER_EPISODE, TRAILER_EPISODE_KEY, TRAILER_SESSION, TRAILER_SESSION_KEY, TRAILER_TURN,
    TRAILER_TURN_KEY,
};

/// The namespace of the linkage digest, so a change to the attribution rules cannot collide with an old
/// digest and pretend a rebuild reproduced itself.
pub const LINKAGE_SCHEMA: &str = "fathom-hook-linkage/1";

// ---------------------------------------------------------------------------
// The shape of a v1 record, as a table
// ---------------------------------------------------------------------------

/// A field's declared JSON type. The JSON Schema spelling of each name is the obvious one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A JSON string.
    Str,
    /// A JSON integer (non-negative: `seq` and `schema_version` are both counters).
    Int,
    /// A JSON object.
    Object,
    /// A JSON boolean.
    Bool,
    /// A JSON array.
    Array,
    /// Any JSON value, including `null`.
    Any,
}

impl FieldKind {
    /// The JSON Schema type name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            FieldKind::Str => "string",
            FieldKind::Int => "integer",
            FieldKind::Object => "object",
            FieldKind::Bool => "boolean",
            FieldKind::Array => "array",
            FieldKind::Any => "any",
        }
    }

    /// Does this value have this type?
    #[must_use]
    pub fn matches(self, value: &Value) -> bool {
        match self {
            FieldKind::Str => value.is_string(),
            FieldKind::Int => value.is_u64() || value.is_i64(),
            FieldKind::Object => value.is_object(),
            FieldKind::Bool => value.is_boolean(),
            FieldKind::Array => value.is_array(),
            FieldKind::Any => true,
        }
    }
}

/// One declared field: its name and its type.
pub type Field = (&'static str, FieldKind);

/// The fields every v1 record must carry, and their types.
///
/// **This table is the ADR's record shape written down a second time, on purpose.** [ADR-005 §3.3] shows the
/// canonical record and §6 names the validations; prose cannot be dispatched on, and ADR-002 §6 rule 1
/// refuses a JSON-Schema dependency in this crate, so a reader that wants to check a line has to carry the
/// field list itself. [`SCHEMA_FILE`] is where a *vendor* reads the same list. The pair is a duplication, so
/// it is tested: `tests/test_hooks_contract.py::test_the_required_field_table_is_the_adr_s_and_payload_is_not_in_it`
/// diffs the table against the ADR's example record and fails if they part company — the only thing that
/// makes a duplication safe is a test that notices.
pub const REQUIRED_FIELDS: &[Field] = &[
    ("seq", FieldKind::Int),
    ("schema_version", FieldKind::Int),
    ("harness", FieldKind::Str),
    ("source", FieldKind::Str),
    ("ts", FieldKind::Str),
];

/// The fields a v1 record may carry, and their types. Anything else on the line is preserved verbatim in
/// [`Event::extra`] (ADR-005 §5.1: additive evolution must not require a change here).
///
/// **`payload` is optional here, not required, and that is a deliberate reading of two sentences that
/// disagree.** ADR-005 §3.2 lists `payload` under "Required on every event … (may be `{}`)", but §3.3's
/// canonical stored record — "the example every adapter is tested against", and the one `tests/hooks_v1.rs`
/// reproduces — carries no `payload` key on `turn_start` and `session_end`, and the reference reader
/// defaults a missing or non-object payload to `{}` (`src/recommender/harness/harness.py`, `validate_wire`:
/// `rec.get("payload") if isinstance(...) else {}`). A reader that refused the absent key would quarantine
/// the ADR's own example and every line the reference writer emits without one. So an absent `payload`
/// means an empty payload here; a *present* one still has to be an object, and [`Event::payload`] is then
/// `{}` either way — the distinction is not load-bearing for anything downstream.
pub const OPTIONAL_FIELDS: &[Field] = &[
    ("payload", FieldKind::Object),
    ("event_id", FieldKind::Str),
    ("session_id", FieldKind::Str),
    ("cwd", FieldKind::Str),
    ("git_branch", FieldKind::Str),
    ("path", FieldKind::Str),
    ("tool_input_sha256", FieldKind::Str),
    ("redacted", FieldKind::Array),
    ("fathom_event", FieldKind::Any),
];

/// The document the table above is the executable form of, named in a rejection's detail so an operator
/// reading `hooks --check` output is sent to the page rather than to a guess.
///
/// It is deliberately an ADR section and not a `schemas/*.schema.json` path. ADR-005 §5.1's additive rule —
/// unknown fields preserved verbatim, unknown event types projected to `null` and counted — is what keeps a
/// record schema from being a straitjacket, and the `schemas/` directory is one file per CLI verb
/// (`crates/fathom-cli/tests/schemas.rs::the_schema_directory_holds_one_file_per_verb` enumerates it and
/// fails on an unclaimed file), and ADR-002 §6 rule 1 forbids this crate from validating against a schema
/// even if one existed. So the contract here is the prose section, and the sentence that says so is this one.
pub const SCHEMA_FILE: &str = "docs/adr/0005-harness-contract.md §3.3 and §6";

/// The event types that must carry a `session_id` (ADR-005 §3.6: the fallback attribution rule needs it,
/// so a turn-scoped event without one is not evidence of anything).
pub const SESSION_REQUIRED_SOURCES: &[&str] = &[
    "UserPromptSubmit",
    "Stop",
    "StopFailure",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "PermissionDenied",
    "Edit",
    "Write",
    "MultiEdit",
    "file.change",
    "file.edited",
    "prompt.submit",
    "turn.stop",
    "tool.start",
    "tool.end",
    "permission.request",
];

/// The shape of a rejection reason as it appears in every report, and as the key of the rejection
/// histogram. Stable strings: these are a published vocabulary, and a CI log that greps for
/// `path_escape` must keep matching next month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RejectReason {
    /// The line was blank. Counted in [`Counters::bad_lines`] like damage, because a blank line in a file
    /// two writers append to is not a normal thing (ADR-005 §3.6).
    Blank,
    /// The line was not one JSON object.
    Undecodable,
    /// The line decoded as JSON but was not an object (an array, a bare string).
    NotAnObject,
    /// No `seq`, or a `seq` that is not an integer.
    MissingSeq,
    /// `schema_version` was absent, not an integer, or not [`EVENT_SCHEMA`]. ADR-005 §6 rule 6: checked before
    /// dispatch and never defaulted to the newest parser.
    SchemaVersion,
    /// A required field from [`REQUIRED_FIELDS`] was absent.
    MissingField,
    /// A field was present with the wrong JSON type.
    FieldType,
    /// Neither `source` nor `fathom_event` named anything, so the record cannot be projected.
    MissingType,
    /// A turn-scoped event arrived without a `session_id`.
    MissingSessionId,
    /// A path in the record escaped the session's working directory (ADR-005 §6 rule 1).
    PathEscape,
    /// The line was longer than [`MAX_LINE_BYTES`]; refused whole, nothing truncated (ADR-005 §3.6).
    Oversized,
    /// `seq` was not greater than the last one seen: a duplicate, or a rewrite of a file already read.
    SequenceDuplicate,
    /// `seq` jumped: records between the last one read and this one were lost. ADR-005 §3.3 makes a gap a
    /// hard error in `--check`, not a thing to scroll past.
    SequenceGap,
    /// The session had already passed its byte budget, so this record was refused too.
    OverQuota,
    /// The session was quarantined, so its records are skipped until an operator clears it.
    SessionQuarantined,
}

impl RejectReason {
    /// The stable key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RejectReason::Blank => "blank",
            RejectReason::Undecodable => "undecodable",
            RejectReason::NotAnObject => "not_an_object",
            RejectReason::MissingSeq => "missing_seq",
            RejectReason::SchemaVersion => "schema_version",
            RejectReason::MissingField => "missing_field",
            RejectReason::FieldType => "field_type",
            RejectReason::MissingType => "missing_type",
            RejectReason::MissingSessionId => "missing_session_id",
            RejectReason::PathEscape => "path_escape",
            RejectReason::Oversized => "oversized",
            RejectReason::SequenceDuplicate => "sequence_duplicate",
            RejectReason::SequenceGap => "sequence_gap",
            RejectReason::OverQuota => "over_quota",
            RejectReason::SessionQuarantined => "session_quarantined",
        }
    }

    /// Whether the reason means *this record* was refused (as opposed to the file being out of order,
    /// which is a statement about the stream and not about the line).
    #[must_use]
    pub const fn quarantines(self) -> bool {
        !matches!(self, RejectReason::SequenceGap | RejectReason::Blank)
    }
}

impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A refused line, with the diagnostics an operator needs to say *why* without opening the events file in
/// an editor and squinting at byte offsets.
#[derive(Debug, Clone)]
pub struct Rejection {
    /// Byte offset the line started at.
    pub offset: u64,
    /// Reason, as the histogram key.
    pub reason: RejectReason,
    /// One human sentence, already sanitized: never a copy of the payload, which is untrusted.
    pub detail: String,
    /// The byte length of the whole line, including the newline.
    pub line_bytes: u64,
    /// `seq` when the line carried one, so a gap can be named by its numbers.
    pub seq: Option<u64>,
    /// sha256 of the raw line: an evidence handle that survives the operator deleting the preview.
    pub digest: String,
    /// The first bytes of the line, redacted and capped. Enough to recognise the writer, not enough to
    /// become a prompt-injection vector by being printed back.
    pub preview: String,
}

impl Rejection {
    /// The JSON form written to the quarantine sidecar.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "offset": self.offset,
            "reason": self.reason.as_str(),
            "detail": self.detail,
            "line_bytes": self.line_bytes,
            "seq": self.seq,
            "sha256": self.digest,
            "preview": self.preview,
        })
    }
}

/// A record refused *before* it reached the events file at write time, kept for the write-side contract in
/// ADR-005 §3.6 (the read path uses [`Rejection`]).
#[derive(Debug, Clone)]
pub struct WriteRejection {
    /// The reason.
    pub reason: RejectReason,
    /// Why, in one line.
    pub detail: String,
}

#[cfg(test)]
mod module_notes {
    //! A marker so the module doc's promises are attached to something greppable: every clause of this
    //! module doc has a test in `crates/fathom-ingest/tests/hooks_v1.rs` or in
    //! `tests/test_hooks_contract.py`.

    #[test]
    fn the_quarantine_reason_vocabulary_is_stable() {
        // Renaming one of these is a contract change: CI logs and `hooks --check` consumers grep them.
        let spelled: Vec<&str> = [
            super::RejectReason::Blank,
            super::RejectReason::Undecodable,
            super::RejectReason::NotAnObject,
            super::RejectReason::MissingSeq,
            super::RejectReason::SchemaVersion,
            super::RejectReason::MissingField,
            super::RejectReason::FieldType,
            super::RejectReason::MissingType,
            super::RejectReason::MissingSessionId,
            super::RejectReason::PathEscape,
            super::RejectReason::Oversized,
            super::RejectReason::SequenceDuplicate,
            super::RejectReason::SequenceGap,
            super::RejectReason::OverQuota,
            super::RejectReason::SessionQuarantined,
        ]
        .iter()
        .map(|reason| reason.as_str())
        .collect();
        assert_eq!(
            spelled,
            vec![
                "blank",
                "undecodable",
                "not_an_object",
                "missing_seq",
                "schema_version",
                "missing_field",
                "field_type",
                "missing_type",
                "missing_session_id",
                "path_escape",
                "oversized",
                "sequence_duplicate",
                "sequence_gap",
                "over_quota",
                "session_quarantined",
            ]
        );
    }
}

// ---------------------------------------------------------------------------
// The canonical projection (ADR-005 §2)
// ---------------------------------------------------------------------------

/// Fathom's own event union (ADR-005 §2.1).
///
/// The projection is **total**: a wire event whose `source` means nothing to Fathom is
/// [`FathomEvent::None`], and it is *counted*, never dropped (ADR-005 §2). That counting is the difference
/// between a coverage gap somebody knows about and one discovered in a customer's incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FathomEvent {
    /// A session started.
    SessionStart,
    /// A session ended.
    SessionEnd,
    /// A turn started (`turn_start`).
    TurnStart,
    /// A turn ended (`turn_end`).
    TurnEnd,
    /// A tool was called.
    ToolCall,
    /// A tool returned.
    ToolResult,
    /// A file was edited.
    FileEdit,
    /// A test run, derived (ADR-005 §2.2).
    TestRun,
    /// An approval was granted or refused.
    Approval,
    /// An error.
    Error,
    /// A subagent started.
    SubagentStart,
    /// A subagent ended.
    SubagentEnd,
    /// Nothing in Fathom's vocabulary: stored, counted, never dropped.
    None,
}

impl FathomEvent {
    /// The canonical spelling, and `null` in JSON for [`FathomEvent::None`] (ADR-005 §2.1: an
    /// unprojectable event carries `fathom_event: null`).
    #[must_use]
    pub const fn as_str(self) -> Option<&'static str> {
        match self {
            FathomEvent::SessionStart => Some("session_start"),
            FathomEvent::SessionEnd => Some("session_end"),
            FathomEvent::TurnStart => Some("turn_start"),
            FathomEvent::TurnEnd => Some("turn_end"),
            FathomEvent::ToolCall => Some("tool_call"),
            FathomEvent::ToolResult => Some("tool_result"),
            FathomEvent::FileEdit => Some("file_edit"),
            FathomEvent::TestRun => Some("test_run"),
            FathomEvent::Approval => Some("approval"),
            FathomEvent::Error => Some("error"),
            FathomEvent::SubagentStart => Some("subagent_start"),
            FathomEvent::SubagentEnd => Some("subagent_end"),
            FathomEvent::None => None,
        }
    }

    /// A canonical event name, as `fathom_event` spells it, or as a *stored* line's `source` sometimes
    /// spells it (see [`FathomEvent::from_source`]).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "session_start" => FathomEvent::SessionStart,
            "session_end" => FathomEvent::SessionEnd,
            "turn_start" => FathomEvent::TurnStart,
            "turn_end" => FathomEvent::TurnEnd,
            "tool_call" => FathomEvent::ToolCall,
            "tool_result" => FathomEvent::ToolResult,
            "file_edit" => FathomEvent::FileEdit,
            "test_run" => FathomEvent::TestRun,
            "approval" => FathomEvent::Approval,
            "error" => FathomEvent::Error,
            "subagent_start" => FathomEvent::SubagentStart,
            "subagent_end" => FathomEvent::SubagentEnd,
            _ => return None,
        })
    }

    /// Project a `source` name onto Fathom's union.
    ///
    /// The vendor half of the table is the ADR-005 §2.1 table and is deliberately the same mapping as
    /// `project()` in `src/recommender/harness/harness.py`, which is the contract fixture the ADR points at;
    /// `tests/test_hooks_contract.py` diffs the two tables against the ADR.
    ///
    /// **The canonical names are accepted too, and that is not the reference's behaviour.** ADR-005 §3.3's
    /// canonical *stored* record is `{"source": "turn_start", ...}` — the reference's `project()` table has no
    /// `turn_start` key, so feeding the ADR's own example back through the reference counts it unprojectable.
    /// Reading `events.jsonl` is exactly that operation: the file holds stored lines, whose `source` may
    /// already be a canonical name (which is also what happens when one build's output is replayed into
    /// another). A reader that could not read its own canonical example would be wrong about the file, so a
    /// canonical name projects to itself here. The vendor table is untouched, so a line the reference
    /// projects still projects identically.
    #[must_use]
    pub fn from_source(source: &str) -> Self {
        if let Some(canonical) = Self::from_name(source) {
            return canonical;
        }
        match source {
            "SessionStart" | "session-start" | "session.created" => FathomEvent::SessionStart,
            "SessionEnd" | "session-end" => FathomEvent::SessionEnd,
            "UserPromptSubmit" | "prompt.submit" => FathomEvent::TurnStart,
            "Stop" | "turn.stop" => FathomEvent::TurnEnd,
            "PreToolUse" | "tool.start" | "tool.execute.before" => FathomEvent::ToolCall,
            "PostToolUse" | "tool.end" | "tool.execute.after" => FathomEvent::ToolResult,
            "PostToolUseFailure" => FathomEvent::Error,
            "PermissionRequest" | "PermissionDenied" | "permission.request" => {
                FathomEvent::Approval
            }
            "SubagentStart" => FathomEvent::SubagentStart,
            "SubagentStop" => FathomEvent::SubagentEnd,
            "StopFailure" | "PreCompact" | "error" => FathomEvent::Error,
            // The path-bearing spellings: `Edit`/`Write`/`MultiEdit` are tool names a vendor sends as the
            // event name, and `file.change`/`file.edited` are the native file events.
            "Edit" | "Write" | "MultiEdit" | "file_change" | "file.change" | "file.edited" => {
                FathomEvent::FileEdit
            }
            _ => FathomEvent::None,
        }
    }

    /// Whether the event belongs to a turn (as opposed to a session or a subagent).
    #[must_use]
    pub const fn is_turn_scoped(self) -> bool {
        matches!(
            self,
            FathomEvent::TurnStart
                | FathomEvent::TurnEnd
                | FathomEvent::ToolCall
                | FathomEvent::ToolResult
                | FathomEvent::FileEdit
                | FathomEvent::Approval
                | FathomEvent::Error
        )
    }
}

impl fmt::Display for FathomEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_str() {
            Some(name) => f.write_str(name),
            None => f.write_str("null"),
        }
    }
}

/// How a `test_run` was recognised (ADR-005 §2.2).
///
/// The spellings are §2.2's own words — `name`, `pattern`, `heuristic`, `none` — because they are a
/// published value of a published field, and the Python reference reader publishes the same words. An
/// internal name for an enum variant is free; what a status payload prints is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DetectedBy {
    /// The command's first word is a known test runner (`pytest`, `jest`, `rspec`).
    Name,
    /// A known runner was not first, but the command names a test target (`cargo test`, `make test`).
    Pattern,
    /// Something that looks like a test invocation without matching a known runner or target — the
    /// heuristic in §2.2's `(name, pattern, heuristic)`.
    Heuristic,
    /// Nothing recognised it; it is still reported, because a detector that misses half the test runs
    /// flatters the loop it is measuring (ADR-005 §2.2).
    None,
}

impl DetectedBy {
    /// The stable spelling, which is also the value in every published `detected_by` and every
    /// `test_run` histogram key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            DetectedBy::Name => "name",
            DetectedBy::Pattern => "pattern",
            DetectedBy::Heuristic => "heuristic",
            DetectedBy::None => "none",
        }
    }

    /// Whether this counts as a *recognised* test run: `name`, `pattern` and `heuristic` do, `none` does
    /// not (§2.2, and see the note on [`derived_test_run`]).
    #[must_use]
    pub const fn is_recognised(self) -> bool {
        !matches!(self, DetectedBy::None)
    }
}

/// Whether a command is a *derived* `test_run` in §2.2's sense, and how it was recognised.
///
/// `DetectedBy::None` means "a command, not recognised as a runner", and §2.2 derives a `test_run` from a
/// command that **matches a known test runner pattern** — so the unrecognised case is a plain
/// `tool_result`. The distinction is not pedantry: counting every shell call as a test run makes
/// `events.test_run` a restatement of `events.by_event.tool_result`, and the loop metric built on it
/// ("how many tests did the agent run") silently becomes "how many commands did it run".
fn derived_test_run(command: &str) -> Option<DetectedBy> {
    detect_test_run(command).filter(|by| by.is_recognised())
}

/// A shell command, classified as a test run or not.
#[must_use]
pub fn detect_test_run(command: &str) -> Option<DetectedBy> {
    let text = command.trim();
    if text.is_empty() {
        return None;
    }
    let words: Vec<String> = text
        .split(|c: char| c == '|' || c == ';' || c == '&' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .map(|word| word.rsplit('/').next().unwrap_or(word).to_ascii_lowercase())
        .collect();
    // The first meaningful word, skipping `sudo`, `time`, env assignments and `npx`-style wrappers.
    let head = words
        .iter()
        .find(|word| {
            !matches!(
                word.as_str(),
                "sudo" | "time" | "env" | "npx" | "pnpm" | "yarn" | "bash" | "sh" | "zsh"
            ) && !word.contains('=')
        })
        .cloned()
        .unwrap_or_default();
    const RUNNERS: [&str; 12] = [
        "pytest", "py.test", "nose2", "jest", "vitest", "mocha", "rspec", "prove", "ctest", "go",
        "dotnet", "mix",
    ];
    if RUNNERS.contains(&head.as_str()) {
        // `go run`, `mix compile` and `dotnet build` are not test runs; only the test verb is.
        let second = words.get(words.iter().position(|w| w == &head).unwrap_or(0) + 1);
        if head == "go" || head == "mix" || head == "dotnet" {
            return second.and_then(|verb| classify_verb(verb));
        }
        return Some(DetectedBy::Name);
    }
    if head == "cargo" || head == "make" || head == "npm" || head == "deno" {
        return words
            .get(words.iter().position(|w| w == &head).unwrap_or(0) + 1)
            .and_then(|verb| classify_verb(verb));
    }
    // Anything else that ran a command is a command: reported with `detected_by: none` rather than
    // vanished, which is the half of §2.2 that a table of known runners would quietly break.
    Some(DetectedBy::None)
}

/// Does this sub-command mean "run the tests"?
fn classify_verb(verb: &str) -> Option<DetectedBy> {
    let verb = verb.to_ascii_lowercase();
    if verb == "test" {
        return Some(DetectedBy::Pattern);
    }
    if verb.contains("test") || verb.contains("spec") {
        // `nextest`, `rstest`, `--spec`: recognisable as a test invocation, but not a runner's name or
        // the plain `test` verb, so it is the heuristic tier and is labelled as one.
        return Some(DetectedBy::Heuristic);
    }
    None
}

// ---------------------------------------------------------------------------
// Redaction (ADR-005 §6 rule 4, §4.3)
// ---------------------------------------------------------------------------

/// One redaction rule: a name and a hand-rolled matcher (the workspace takes no `regex` dependency, and
/// ADR-002 §6 rule 1 makes every dependency a decision rather than a default).
#[derive(Debug, Clone, Copy)]
pub struct RedactRule {
    /// The stable rule name, printed as `[REDACTED:<rule>]`.
    pub name: &'static str,
    /// What it looks for, as a substring to find a candidate and a check to confirm it.
    pub matcher: fn(&str) -> Option<(usize, usize)>,
}

/// The rules, in the order `src/recommender/harness/harness.py`'s `REDACT_RULES` lists them, so the two
/// implementations agree on which name a redaction carries.
pub const REDACT_RULES: &[RedactRule] = &[
    RedactRule {
        name: "aws-access-key",
        matcher: find_aws_key,
    },
    RedactRule {
        name: "generic-token",
        matcher: find_generic_token,
    },
    RedactRule {
        name: "private-key",
        matcher: find_private_key,
    },
    RedactRule {
        name: "bearer",
        matcher: find_bearer,
    },
    RedactRule {
        name: "high-entropy",
        matcher: find_high_entropy,
    },
];

/// The marker a redaction leaves behind.
pub fn redaction_marker(rule: &str) -> String {
    format!("[REDACTED:{rule}]")
}

fn find_aws_key(text: &str) -> Option<(usize, usize)> {
    let mut start = 0;
    while let Some(relative) =
        find_from(text, "AKIA", start).or_else(|| find_from(text, "ASIA", start))
    {
        let end = (relative + 20).min(text.len());
        let candidate = &text[relative..end];
        if candidate.len() == 20
            && candidate
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Some((relative, end));
        }
        start = relative + 4;
    }
    None
}

fn find_generic_token(text: &str) -> Option<(usize, usize)> {
    const KEYS: [&str; 6] = [
        "api_key",
        "api-key",
        "apikey",
        "access_token",
        "secret",
        "password",
    ];
    let lower = text.to_ascii_lowercase();
    for key in KEYS {
        let mut from = 0;
        while let Some(at) = find_from(&lower, key, from) {
            let rest = &text[at + key.len()..];
            let trimmed = rest.trim_start();
            let gap = rest.len() - trimmed.len();
            if trimmed.starts_with(':') || trimmed.starts_with('=') {
                let value_start = at + key.len() + gap + 1;
                let value = text[value_start.min(text.len())..].trim_start();
                let value_end = value
                    .find(['"', '\'', ',', ' ', '\n'])
                    .unwrap_or(value.len());
                if !value[..value_end].is_empty() {
                    return Some((at, value_start + value_end));
                }
            }
            from = at + key.len();
        }
    }
    None
}

fn find_private_key(text: &str) -> Option<(usize, usize)> {
    let at = text.find("-----BEGIN ")?;
    let rest = text[at..].find("PRIVATE KEY-----")?;
    Some((at, at + rest + "PRIVATE KEY-----".len()))
}

fn find_bearer(text: &str) -> Option<(usize, usize)> {
    let lower = text.to_ascii_lowercase();
    let at = find_from(&lower, "bearer", 0)?;
    let rest = &text[at + "bearer".len()..];
    let gap = rest.len() - rest.trim_start().len();
    let value = rest.trim_start();
    let value_end = value
        .find(['"', '\'', ',', ' ', '\n'])
        .unwrap_or(value.len());
    if value[..value_end].chars().count() < 20 {
        return None;
    }
    Some((at, at + "bearer".len() + gap + value_end))
}

/// A long run of base64-ish characters: the shape of a token a vendor echoed back verbatim.
fn find_high_entropy(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut run = 0usize;
    let mut start = 0usize;
    for index in 0..=bytes.len() {
        let alnum = index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'+' | b'/' | b'='));
        if alnum {
            if run == 0 {
                start = index;
            }
            run += 1;
            if run >= 40 {
                let mut end = index + 1;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric()
                        || matches!(bytes[end], b'+' | b'/' | b'='))
                {
                    end += 1;
                }
                // A run of 40 base64 characters that is *all* lowercase prose is a long identifier, not a
                // token; require the mixed case or symbol content that random bytes actually have.
                let candidate = &text[start..end];
                let distinct = candidate
                    .chars()
                    .filter(|c| {
                        c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '+' | '/' | '=')
                    })
                    .count();
                if distinct >= 8 {
                    return Some((start, end));
                }
                return None;
            }
        } else {
            run = 0;
        }
    }
    None
}

fn find_from(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..].find(needle).map(|at| at + from)
}

/// Redact every rule out of one string, reporting which rules fired.
#[must_use]
pub fn redact(text: &str) -> (String, Vec<&'static str>) {
    let mut out = text.to_owned();
    let mut hits: Vec<&'static str> = Vec::new();
    for rule in REDACT_RULES {
        // A rule can fire more than once; keep going until it stops matching, bounded so a pathological
        // overlap cannot spin forever.
        for _ in 0..64 {
            let Some((start, end)) = (rule.matcher)(&out) else {
                break;
            };
            if !hits.contains(&rule.name) {
                hits.push(rule.name);
            }
            out.replace_range(start..end, &redaction_marker(rule.name));
        }
    }
    if !hits.is_empty() {
        hits.sort_unstable();
    }
    (out, hits)
}

/// Redact a payload in place: only string leaves, and only the fields a harness would echo back
/// (ADR-005 §6 rule 4 redacts at ingest *and* again at MCP egress; this is the ingest half).
fn redact_value(value: &mut Value, hits: &mut Vec<&'static str>) {
    match value {
        Value::String(text) => {
            let (clean, fired) = redact(text);
            if !fired.is_empty() {
                *text = clean;
                hits.extend(fired);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_value(item, hits);
            }
        }
        Value::Object(map) => {
            for (_key, item) in map.iter_mut() {
                redact_value(item, hits);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------

/// Parse an RFC 3339 / ISO 8601 timestamp into unix milliseconds.
///
/// `ts` is the *vendor's* timestamp when it has one (ADR-005 §3.3), and the vendor's format is not ours to
/// choose, so the parser accepts what vendors actually send: `2026-02-11T10:00:00Z`, with or without
/// fractional seconds, and with a `+hh:mm` / `-hh:mm` offset or none (an offset-less stamp is read as
/// UTC). Ordering never depends on this: ordering is by `seq` (§3.3), and this parse exists for the
/// attribution window and for the lag number.
#[must_use]
pub fn parse_timestamp_millis(text: &str) -> Option<i64> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() < 20 || (bytes[4] != b'-' || bytes[7] != b'-') {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        if part.len() != 2 || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        part.parse::<i64>().ok()
    };
    let year = text.get(0..4)?.parse::<i64>().ok()?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    if !(bytes[10] == b'T' || bytes[10] == b' ') || number(11..13).is_none() {
        return None;
    }
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut index = 19;
    let mut millis = 0i64;
    if text.get(index..index + 1) == Some(".") {
        let digits = text[index + 1..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        let fraction = text[index + 1..index + 1 + digits].parse::<i64>().ok()?;
        millis = match digits {
            1 => fraction * 100,
            2 => fraction * 10,
            _ => fraction,
        };
        index += 1 + digits;
    }
    let mut offset_seconds = 0i64;
    match text.get(index..) {
        None | Some("") | Some("Z") | Some("z") => {}
        Some(rest) => {
            let sign = match rest.as_bytes().first() {
                Some(b'+') => 1,
                Some(b'-') => -1,
                _ => return None,
            };
            let (hh, mm) = (rest.get(1..3)?, rest.get(4..6)?);
            let hours: i64 = hh.parse().ok()?;
            let minutes: i64 = mm.parse().ok()?;
            offset_seconds = sign * (hours * 3600 + minutes * 60);
        }
    }
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds;
    seconds.checked_mul(1000)?.checked_add(millis)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's `days_from_civil`, which is
/// short, exact and free of a date dependency).
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

// ---------------------------------------------------------------------------
// The accepted record
// ---------------------------------------------------------------------------

/// The limits the reader enforces. Values, not config (ADR-002 §4.5).
#[derive(Debug, Clone)]
pub struct Limits {
    /// The per-line cap ([`MAX_LINE_BYTES`]).
    pub max_line_bytes: usize,
    /// The per-session byte budget over which a session is quarantined.
    pub max_bytes_per_session: u64,
    /// How many quarantined records to keep.
    pub quarantine_records: usize,
    /// The cap on any one piece of intent text read out of a payload (`e4.2`).
    ///
    /// The same budget as [`crate::intent::Policy::max_text_bytes`] — one default, named once — and applied
    /// here as well as there because this is where the payload is: a harness that echoed a megabyte of
    /// prompt per turn would otherwise grow the cursor, which holds [`Limits::turns_kept`] turns.
    pub max_intent_text_bytes: usize,
    /// How many turns the cursor carries across a restart.
    pub turns_kept: usize,
    /// Whether refused lines are written to the sidecar.
    ///
    /// True for the reader that owns the queue. `fathom hooks --check` sets it false: a diagnostic that
    /// quarantines would re-record the same damage on every run, and the sidecar's bound means a monitoring
    /// loop polling `--check` would push the *real* records out with its own duplicates. The refusals are
    /// still returned and counted either way — a probe that could not say what it would refuse would be
    /// useless — they are simply not written down twice.
    pub write_quarantine: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_line_bytes: MAX_LINE_BYTES,
            max_bytes_per_session: DEFAULT_MAX_BYTES_PER_SESSION,
            quarantine_records: DEFAULT_QUARANTINE_RECORDS,
            max_intent_text_bytes: crate::intent::Policy::default().max_text_bytes,
            turns_kept: DEFAULT_TURNS_KEPT,
            write_quarantine: true,
        }
    }
}

impl Limits {
    /// Limits from a `harness.max_bytes_per_session_mib` value.
    #[must_use]
    pub fn with_session_budget(bytes: u64) -> Self {
        Limits {
            max_bytes_per_session: bytes,
            ..Limits::default()
        }
    }
}

/// A path-shaped value in a record, contained or refused (ADR-005 §6 rule 1).
///
/// Absolute paths are accepted only when they are already inside `cwd`; `..`, NUL bytes, control
/// characters and a `~` prefix are refused outright. No filesystem access happens here — the check is on
/// the *name*, because resolving a name an untrusted writer chose is the act this clause exists to refuse.
pub fn contain_path(cwd: &str, path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("empty path".to_owned());
    }
    if path
        .chars()
        .any(|c| c == '\0' || (c.is_control() && c != '\n' && c != '\t'))
        || path.starts_with('~')
    {
        return Err("control character or home prefix".to_owned());
    }
    let clean = normalize_path(path);
    if std::path::Path::new(&clean)
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("path climbs out of the session directory".to_owned());
    }
    if std::path::Path::new(path).is_absolute() {
        let base = normalize_path(if cwd.is_empty() { "." } else { cwd });
        let target = normalize_path(path);
        if starts_with_component(&target, &base) {
            return Ok(target);
        }
        return Err(format!("{target} is outside {base}"));
    }
    Ok(clean)
}

/// Lexical normalization: `a/./b` becomes `a/b`, and a trailing slash goes. Nothing touches the disk.
fn normalize_path(path: &str) -> String {
    let parsed = std::path::Path::new(path);
    let mut out = String::new();
    for component in parsed.components() {
        match component {
            std::path::Component::CurDir => {}
            other => {
                if !out.is_empty() && !out.ends_with(std::path::MAIN_SEPARATOR_STR) {
                    out.push(std::path::MAIN_SEPARATOR);
                }
                out.push_str(&other.as_os_str().to_string_lossy());
            }
        }
    }
    if out.is_empty() {
        return ".".to_owned();
    }
    out
}

/// Is `target` inside `base`, on a component boundary?
fn starts_with_component(target: &str, base: &str) -> bool {
    let base = base.trim_end_matches(std::path::MAIN_SEPARATOR);
    if base.is_empty() {
        return true;
    }
    target == base
        || (target.starts_with(base)
            && target
                .as_bytes()
                .get(base.len())
                .is_some_and(|byte| *byte == b'/' || *byte == std::path::MAIN_SEPARATOR as u8))
}

/// An accepted event: the v1 envelope, the projection, and the two derived handles the pipeline needs.
#[derive(Debug, Clone)]
pub struct Event {
    /// The sequence number, allocated by the file (ADR-005 §3.3).
    pub seq: u64,
    /// `event_id`, which is `"e" + seq` by the same clause.
    pub event_id: String,
    /// The canonical event, or [`FathomEvent::None`] when nothing in Fathom's vocabulary fits.
    pub fathom_event: FathomEvent,
    /// The vendor's event name, verbatim.
    pub source: String,
    /// The adapter's name (`claude_code`, `pi_json`, …).
    pub harness: String,
    /// The harness session, when the record carried one.
    pub session_id: Option<String>,
    /// The session's working directory, when it carried one.
    pub cwd: Option<String>,
    /// The branch, when it carried one.
    pub git_branch: Option<String>,
    /// The vendor's timestamp, verbatim.
    pub ts: String,
    /// The vendor payload, redacted and otherwise verbatim (ADR-005 §2: never reshaped).
    pub payload: Value,
    /// Fields on the line this version of Fathom does not know, preserved (ADR-005 §5.1).
    pub extra: serde_json::Map<String, Value>,
    /// The contained path a file-editing event named, if it named one.
    pub path: Option<String>,
    /// The redaction rule names that fired on this record, if any.
    pub redacted: Vec<&'static str>,
    /// `tool_input_sha256` as sent, or as computed here over the redacted payload.
    pub tool_input_sha256: Option<String>,
    /// The turn this event was allocated to, if it was turn-scoped.
    pub turn: Option<TurnRef>,
    /// How far into the file this line started, for the quarantine report.
    pub byte_offset: u64,
    /// The line's length including its newline.
    pub byte_len: u64,
}

impl Event {
    /// The canonical name, or the source for an unprojectable event (the histogram key: an unprojectable
    /// event is counted under its *source*, ADR-005 §2).
    #[must_use]
    pub fn bucket(&self) -> String {
        if matches!(self.fathom_event, FathomEvent::ToolResult)
            && let Some(command) = self.command()
            && derived_test_run(&command).is_some()
        {
            return "test_run".to_owned();
        }
        self.fathom_event
            .as_str()
            .unwrap_or(self.source.as_str())
            .to_owned()
    }

    /// `pass` / `fail` / `none` when this event is a derived test run (ADR-005 §11.1 `events.test_run`).
    ///
    /// A *separate* histogram from `by_event`, and keyed by outcome rather than by how the runner was
    /// recognised: §11.1 publishes `{"pass": n, "fail": n, "none": n}`, and a reader that published the
    /// detection method there instead would be publishing a different contract than the one it cites.
    #[must_use]
    pub fn test_run_status(&self) -> Option<&'static str> {
        if !matches!(self.fathom_event, FathomEvent::ToolResult) {
            return None;
        }
        let command = self.command()?;
        derived_test_run(&command)?;
        Some(match self.exit_code() {
            Some(0) => "pass",
            Some(_) => "fail",
            None => "none",
        })
    }

    /// The exit code the event reported, when it reported one (`ok: false` counts as a failure with no
    /// number rather than inventing one).
    #[must_use]
    pub fn exit_code(&self) -> Option<i64> {
        if let Some(value) = self.payload.get("exit_code").and_then(Value::as_i64) {
            return Some(value);
        }
        if let Some(true) = self.payload.get("is_error").and_then(Value::as_bool) {
            return Some(-1);
        }
        if let Some(false) = self.payload.get("ok").and_then(Value::as_bool) {
            return Some(-1);
        }
        None
    }

    /// The shell command the event carried, when it carried one. Stored as a string and displayed, never
    /// run (ADR-005 §6 rule 3).
    #[must_use]
    pub fn command(&self) -> Option<String> {
        for key in ["command", "cmd"] {
            if let Some(Value::String(text)) = self.payload.get(key) {
                return Some(text.clone());
            }
        }
        for key in ["tool_input", "input"] {
            if let Some(Value::Object(inner)) = self.payload.get(key)
                && let Some(Value::String(text)) = inner.get("command")
            {
                return Some(text.clone());
            }
        }
        None
    }

    /// The JSON form `fathom hooks --tail` prints: the envelope and the derived turn, never the payload.
    ///
    /// The payload stays out on purpose: ADR-005 §4.3 keeps content inside the model, and a tail that
    /// echoed vendor payloads would put untrusted text on a terminal unsolicited.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "seq": self.seq,
            "event_id": self.event_id,
            "ts": self.ts,
            "harness": self.harness,
            "source": self.source,
            "fathom_event": self.fathom_event.as_str(),
            "session_id": self.session_id,
            "turn_id": self.turn.as_ref().map(|turn| turn.turn_id.clone()),
            "path": self.path,
            "offset": self.byte_offset,
        })
    }
}

/// A turn reference: the qualified id plus its two parts, because a card prints the parts and a join uses
/// the id (ADR-003 §9.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRef {
    /// The harness session.
    pub session_id: String,
    /// The per-session index allocated at ingest, starting at 1.
    pub turn_index: u64,
    /// `"<session_id>:<turn_index>"`.
    pub turn_id: String,
}

/// Allocate a session-qualified turn id (ADR-005 §3.3, ADR-003 §9.1, §18.2 **D4**).
///
/// The qualification is not decoration: an unqualified `3` makes two sessions' turn 3 one turn, and the
/// `hook` rung then declines to cut between them — two agent turns become one episode, which is the
/// defect ADR-003 §18.2 **D4** names and this function closes.
#[must_use]
pub fn qualify_turn_id(session_id: &str, turn_index: u64) -> String {
    format!("{session_id}:{turn_index}")
}

/// Split a session-qualified turn id back into its parts.
#[must_use]
pub fn parse_turn_id(turn_id: &str) -> Option<(String, u64)> {
    let (session, index) = turn_id.rsplit_once(':')?;
    if session.is_empty() {
        return None;
    }
    let index: u64 = index.parse().ok()?;
    if index == 0 {
        return None;
    }
    Some((session.to_owned(), index))
}

/// The trailer block this module writes for a turn (ADR-003 §18.2 **D5**: this ticket is the writer).
///
/// Two trailer lines, `Agent-Session:` and `Agent-Turn:`, the latter session-qualified. Whatever stamps a
/// commit uses this function, so the key rung 3 reads and the key written here cannot drift: rung 3 reads
/// [`TRAILER_TURN_KEY`] and this writes [`TRAILER_TURN`], which are the same string modulo the case rule
/// `crate::record::Trailers` applies.
#[must_use]
pub fn trailer_block(turn: &TurnRef) -> String {
    format!(
        "{TRAILER_SESSION}: {}\n{TRAILER_TURN}: {}\n",
        turn.session_id, turn.turn_id
    )
}

/// The same, from parts, for a caller that has a session and an index rather than a [`TurnRef`].
#[must_use]
pub fn trailer_block_for(session_id: &str, turn_index: u64) -> String {
    trailer_block(&TurnRef {
        session_id: session_id.to_owned(),
        turn_index,
        turn_id: qualify_turn_id(session_id, turn_index),
    })
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// A record that failed validation, before anybody decided what to do about it.
#[derive(Debug, Clone)]
pub struct Reject {
    /// The vocabulary entry.
    pub reason: RejectReason,
    /// One sentence about this record.
    pub detail: String,
    /// The `seq` the record carried, when it carried one.
    pub seq: Option<u64>,
}

/// The canonical bytes a digest is taken over: compact JSON with keys sorted.
///
/// This is deliberately the same shape as `canonical_sha256` in `src/recommender/harness/harness.py`
/// (`json.dumps(..., sort_keys=True, separators=(",", ":"))`, non-ASCII left literal), so a digest an
/// adapter computed and stored survives being checked here. A digest is evidence: if the two
/// implementations disagreed about the bytes, the chain the thesis rests on would be spelled differently
/// on each side of the boundary.
#[must_use]
pub fn canonical_bytes(value: &Value) -> Vec<u8> {
    // `serde_json`'s map is key-sorted and this is the compact form: `{`/`,`/`:` with no spaces.
    serde_json::to_vec(value).unwrap_or_default()
}

/// The hex sha256 of the canonical bytes.
#[must_use]
pub fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// sha256 of one raw line, which is what a quarantine record carries as its handle.
#[must_use]
pub fn digest_line(line: &[u8]) -> String {
    digest_bytes(line)
}

/// Validate one decoded JSON value as a v1 record.
///
/// The order is the ADR's: shape, then `seq`, then `schema_version` **before** any dispatch (§6 rule 6), then
/// the declared field table, then the semantic rules (session id, path containment), then redaction and
/// the digest. Errors name the field that failed rather than the whole record.
///
/// # Errors
///
/// A [`Reject`] with the vocabulary entry and one sentence of detail. Never a panic: the whole point of
/// the read side is that a hostile or broken line is a *count*, not an outage.
pub fn validate_record(record: &Value, byte_offset: u64, byte_len: u64) -> Result<Event, Reject> {
    let reject = |reason: RejectReason, detail: String, seq: Option<u64>| {
        Err(Reject {
            reason,
            detail,
            seq,
        })
    };
    let object = match record {
        Value::Object(object) => object,
        other => {
            return reject(
                RejectReason::NotAnObject,
                format!("the line is a JSON {}, not an object", kind_name(other)),
                None,
            );
        }
    };
    let seq_value = object.get("seq");
    let seq = match seq_value {
        Some(Value::Number(number)) => number.as_u64(),
        _ => None,
    };
    let Some(seq) = seq else {
        return reject(
            RejectReason::MissingSeq,
            "`seq` is absent or is not an integer; a record with no sequence number cannot be placed on \
             the index"
                .to_owned(),
            None,
        );
    };
    // §6 rule 6: the version gate is before dispatch, and an absent version is not defaulted to the newest
    // parser. A record that will not say which contract it speaks does not get to pick one.
    match object.get("schema_version") {
        None => {
            return reject(
                RejectReason::SchemaVersion,
                format!(
                    "`schema_version` is absent; {SCHEMA_FILE} requires it and §6 rule 6 forbids guessing"
                ),
                Some(seq),
            );
        }
        Some(Value::Number(version)) if version.as_u64() == Some(EVENT_SCHEMA) => {}
        Some(other) => {
            return reject(
                RejectReason::SchemaVersion,
                format!(
                    "`schema_version` is {other}, and this build speaks event schema {EVENT_SCHEMA}; \
                     a session may not mix schema majors"
                ),
                Some(seq),
            );
        }
    }
    let mut missing = Vec::new();
    let mut wrong_type = Vec::new();
    for (name, kind) in REQUIRED_FIELDS {
        match object.get(*name) {
            None => missing.push(*name),
            Some(value) if !kind.matches(value) => {
                wrong_type.push(format!("{name}:{}", kind.as_str()))
            }
            _ => {}
        }
    }
    if !missing.is_empty() {
        return reject(
            RejectReason::MissingField,
            format!("required field(s) {} are absent", missing.join(", ")),
            Some(seq),
        );
    }
    if !wrong_type.is_empty() {
        return reject(
            RejectReason::FieldType,
            format!("field(s) {} are the wrong JSON type", wrong_type.join(", ")),
            Some(seq),
        );
    }
    for (name, kind) in OPTIONAL_FIELDS {
        if let Some(value) = object.get(*name)
            && !kind.matches(value)
        {
            return reject(
                RejectReason::FieldType,
                format!(
                    "optional field `{name}` is {}, not {}",
                    kind_name(value),
                    kind.as_str()
                ),
                Some(seq),
            );
        }
    }

    let text = |key: &str| -> Option<String> {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(|value| value.to_owned())
            .filter(|value| !value.is_empty())
    };
    let source = text("source").unwrap_or_default();
    let projected = FathomEvent::from_source(&source);
    // A record that names nothing at all cannot even be counted under a source, so it is a rejection;
    // a record that names something *unknown* is the additive-safe case and gets through as `None`.
    if source.is_empty() && object.get("fathom_event").is_none() {
        return reject(
            RejectReason::MissingType,
            "neither `source` nor `fathom_event` names an event type".to_owned(),
            Some(seq),
        );
    }
    // A record that states its own `fathom_event` is stating a fact about itself, and the reference honours
    // it the same way (`project()` returns `{"fathom_event": fe, **rec}`, and `**rec` wins). The difference
    // here is that an *unknown* claim is discarded rather than stored: a writer may not put a type this build
    // has never heard of into the field the index groups on, because that is how a vocabulary stops being
    // closed. A claim that merely disagrees with `source` is kept silently — the claim is the writer's, the
    // source is the transport's, and the writer is the authority on the event.
    let fathom_event = match object.get("fathom_event") {
        Some(Value::String(name)) => FathomEvent::from_name(name).unwrap_or(projected),
        _ => projected,
    };

    let session_id = text("session_id");
    if session_id.is_none() && SESSION_REQUIRED_SOURCES.contains(&source.as_str()) {
        return reject(
            RejectReason::MissingSessionId,
            format!(
                "`{source}` is turn-scoped and carries no `session_id`, so it could not be attributed to \
                 anyone's turn"
            ),
            Some(seq),
        );
    }
    let cwd = text("cwd");

    // The payload is redacted *before* it is kept (ADR-005 §6 rule 4), and the containment check looks inside
    // the shapes vendors actually use: `tool_input.file_path` for Claude Code, `path` for the others.
    let mut payload = object
        .get("payload")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    let mut hits: Vec<&'static str> = Vec::new();
    redact_value(&mut payload, &mut hits);
    for key in ["tool_input", "tool_response", "input", "output"] {
        if let Some(value) = object.get(key) {
            let mut value = value.clone();
            redact_value(&mut value, &mut hits);
            if let Value::Object(map) = &mut payload {
                map.insert(key.to_owned(), value);
            }
        }
    }
    if !hits.is_empty() {
        hits.sort_unstable();
        hits.dedup();
    }

    let mut path = text("path");
    if path.is_none() {
        path = payload_path(&payload).map(|found| found.to_owned());
    }
    let path = match path {
        None => None,
        Some(raw) => Some(
            contain_path(cwd.as_deref().unwrap_or("."), &raw).map_err(|why| Reject {
                reason: RejectReason::PathEscape,
                detail: why,
                seq: Some(seq),
            })?,
        ),
    };

    let mut extra = serde_json::Map::new();
    if let Value::Object(map) = record {
        for (key, value) in map {
            let declared = REQUIRED_FIELDS
                .iter()
                .chain(OPTIONAL_FIELDS.iter())
                .any(|(name, _kind)| name == key)
                || key == "seq";
            if !declared {
                extra.insert(key.clone(), value.clone());
            }
        }
    }

    let tool_input_sha256 = object
        .get("tool_input_sha256")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            payload
                .get("tool_input")
                .map(|input| digest_bytes(&canonical_bytes(input)))
        });

    Ok(Event {
        seq,
        event_id: format!("e{seq}"),
        fathom_event,
        source,
        harness: text("harness").unwrap_or_else(|| "unknown".to_owned()),
        session_id,
        cwd,
        git_branch: text("git_branch"),
        ts: object
            .get("ts")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        payload,
        extra,
        path,
        redacted: hits,
        tool_input_sha256,
        turn: None,
        byte_offset,
        byte_len,
    })
}

/// The path-shaped field inside a payload, wherever the vendor put it.
fn payload_path(payload: &Value) -> Option<&str> {
    for key in ["file_path", "path"] {
        if let Some(Value::String(text)) = payload.get(key) {
            return Some(text.as_str());
        }
    }
    for key in ["tool_input", "input", "edit"] {
        if let Some(inner) = payload.get(key)
            && let Some(found) = payload_path(inner)
        {
            return Some(found);
        }
    }
    None
}

const fn kind_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ---------------------------------------------------------------------------
// Turns: the thing this ingest exists to produce
// ---------------------------------------------------------------------------

/// How a turn was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedBy {
    /// A `turn_end` event said so.
    TurnEnd,
    /// The next `turn_start` arrived before this one ended — ADR-005 §3.3's crash rule, so a turn that
    /// never ends still has a right edge.
    NextTurn,
    /// The session ended.
    SessionEnd,
    /// Still open.
    Open,
}

impl ClosedBy {
    /// The stable spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ClosedBy::TurnEnd => "turn-end",
            ClosedBy::NextTurn => "next-turn",
            ClosedBy::SessionEnd => "session-end",
            ClosedBy::Open => "open",
        }
    }
}

/// One turn of one session: a time window, the paths it touched, and the id segmentation keys on.
#[derive(Debug, Clone)]
pub struct Turn {
    /// The session the turn belongs to.
    pub session_id: String,
    /// The index allocated at ingest, from 1.
    pub turn_index: u64,
    /// `"<session_id>:<turn_index>"`.
    pub turn_id: String,
    /// The `seq` that opened it.
    pub started_seq: u64,
    /// The vendor timestamp of the opening event.
    pub started_ts: String,
    /// The `seq` that closed it, if it is closed.
    pub ended_seq: Option<u64>,
    /// The timestamp of the closing event.
    pub ended_ts: Option<String>,
    /// What closed it.
    pub closed_by: ClosedBy,
    /// Whether the turn was inferred from a turn-scoped event with no `turn_start` (see [`Ledger`]).
    pub inferred: bool,
    /// The paths the turn touched, sorted for the digest.
    pub paths: BTreeSet<String>,
    /// Tool calls in the turn.
    pub tool_calls: u64,
    /// File edits in the turn.
    pub edits: u64,
    /// Test runs observed in the turn, keyed by how they were detected.
    pub test_runs: BTreeMap<String, u64>,
    /// The session's working directory at the time, when the events said.
    pub cwd: Option<String>,
    /// The branch at the time, when the events said.
    pub git_branch: Option<String>,
    /// The prompt **text**, when the harness sent it.
    ///
    /// ADR-005 §2.1's `turn_start` row names `prompt_sha256` and `prompt_chars`, and the reference writer
    /// and every corpus stream send exactly those two — so `None` here is the normal case, not a gap
    /// ([`crate::intent::State::Digest`]). A vendor that echoes the prompt into its `UserPromptSubmit`
    /// payload is answered in full, because the intent of the turn is the one piece of payload content the
    /// product is for.
    pub prompt: Option<String>,
    /// `prompt_sha256`, as sent.
    pub prompt_sha256: Option<String>,
    /// `prompt_chars`, as sent, or the length of the text when only the text arrived.
    pub prompt_chars: Option<u64>,
    /// A declared intent line (`intent` / `declared_intent` in the payload).
    pub intent: Option<String>,
    /// The plan items the harness held for the turn.
    pub plan: Vec<crate::intent::PlanItem>,
    /// The declared scope globs, seeded from the session and overridden by the turn.
    pub scope: Vec<String>,
}

impl Turn {
    /// The turn's canonical projection for [`Cursor::ledger_digest`], as `(key, value)` pairs in a fixed
    /// order — the same style as [`Linkage::digest`]'s framing, because both are digests a rebuild must reproduce.
    ///
    /// Everything here is a fact the reader derived from the stream in `seq` order: identity, window, and
    /// *counts* of what the turn contained. The paths are folded as their sorted join, so a turn that
    /// touched three files digests identically however the writes arrived (ADR-005 §3.6). The open-ended
    /// end is written as the empty string rather than an unbounded sentinel, so the digest does not encode
    /// an implementation detail about how an unfinished turn is modelled.
    ///
    /// The `e4.2` intent fields are deliberately **not** in here. The digest is the *linkage*'s — the claim
    /// that a rebuild maps these commits onto these turns again — and intent text is content, not linkage:
    /// a harness that reworded a prompt, or a vendor that started sending `payload.prompt` where it had
    /// sent only a digest, would otherwise invalidate every stored linkage in the repository for a change
    /// that moves no commit and reorders no turn.
    #[must_use]
    pub fn digest_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("turn_id", self.turn_id.clone()),
            ("session_id", self.session_id.clone()),
            ("turn_index", self.turn_index.to_string()),
            ("started_seq", self.started_seq.to_string()),
            (
                "ended_seq",
                self.ended_seq.map_or(String::new(), |seq| seq.to_string()),
            ),
            ("ended_ts", self.ended_ts.clone().unwrap_or_default()),
            ("closed_by", self.closed_by.as_str().to_owned()),
            ("inferred", (self.inferred as u8).to_string()),
            (
                "paths",
                self.paths.iter().cloned().collect::<Vec<_>>().join(","),
            ),
            ("tool_calls", self.tool_calls.to_string()),
            ("edits", self.edits.to_string()),
            (
                "test_runs",
                self.test_runs
                    .iter()
                    .map(|(how, count)| format!("{how}={count}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        ]
    }

    /// The turn as the pipeline refers to it.
    #[must_use]
    pub fn reference(&self) -> TurnRef {
        TurnRef {
            session_id: self.session_id.clone(),
            turn_index: self.turn_index,
            turn_id: self.turn_id.clone(),
        }
    }

    /// The turn's window in unix milliseconds, when both ends parse. An open turn runs to
    /// [`i64::MAX`] for the purposes of attribution: an unfinished turn still owns what it is editing.
    #[must_use]
    pub fn window_millis(&self) -> Option<(i64, i64)> {
        let start = parse_timestamp_millis(&self.started_ts)?;
        let end = match &self.ended_ts {
            Some(text) => parse_timestamp_millis(text)?,
            None => i64::MAX,
        };
        Some((start, end))
    }

    /// The JSON form for `--json` and for the cursor file.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "session_id": self.session_id,
            "turn_index": self.turn_index,
            "turn_id": self.turn_id,
            "started_seq": self.started_seq,
            "started_ts": self.started_ts,
            "ended_seq": self.ended_seq,
            "ended_ts": self.ended_ts,
            "closed_by": self.closed_by.as_str(),
            "inferred": self.inferred,
            "paths": self.paths.iter().cloned().collect::<Vec<_>>(),
            "tool_calls": self.tool_calls,
            "edits": self.edits,
            "test_runs": &self.test_runs,
            "cwd": self.cwd,
            "git_branch": self.git_branch,
            "prompt": self.prompt,
            "prompt_sha256": self.prompt_sha256,
            "prompt_chars": self.prompt_chars,
            "intent": self.intent,
            "plan": self
                .plan
                .iter()
                .filter_map(|item| serde_json::to_value(item).ok())
                .collect::<Vec<_>>(),
            "scope": self.scope,
        })
    }

    /// What the stream said about this turn's *intent*, in the shape [`crate::intent::capture`] reads.
    ///
    /// A projection, not a decision: this says what was stated, [`crate::intent`] decides what it means.
    /// A turn that stated nothing still returns a value with empty fields, because "the harness had a turn
    /// and it stated no intent" is the evidence the card's `no recorded intent` line is made of — it is a
    /// checked absence, and an unchecked one would print the same words.
    #[must_use]
    pub fn intent(&self) -> crate::intent::TurnEvidence {
        crate::intent::TurnEvidence {
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
            prompt: self.prompt.clone(),
            prompt_sha256: self.prompt_sha256.clone(),
            prompt_chars: self.prompt_chars,
            intent: self.intent.clone(),
            plan: self.plan.clone(),
            scope: self.scope.clone(),
        }
    }
}

/// Per-session state the ledger needs to allocate turn ids and honour a quarantine.
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    /// The next turn index to allocate (turns are numbered from 1).
    pub next_turn_index: u64,
    /// The index of the currently open turn, if one is open.
    pub open_turn: Option<u64>,
    /// Events accepted from this session.
    pub events: u64,
    /// Bytes of events accepted from this session.
    pub bytes: u64,
    /// Whether the session is quarantined (over quota, ADR-005 §6 rule 3), and why.
    pub quarantined: Option<String>,
    /// The scope globs the session declared, which every turn of the session inherits unless the turn
    /// states its own (`e4.2`: one `scope:` on a session start is how a harness usually declares scope).
    pub scope: Vec<String>,
    /// The intent line the session declared, inherited the same way.
    pub intent: Option<String>,
}

impl SessionState {
    /// The JSON form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "next_turn_index": self.next_turn_index,
            "open_turn": self.open_turn,
            "events": self.events,
            "bytes": self.bytes,
            "quarantined": self.quarantined,
            "scope": self.scope,
            "intent": self.intent,
        })
    }
}

/// The counters a run accumulates, and the durable record of what has already been read.
///
/// Both live on [`Cursor`] rather than being separate because they must be written *together*: counters
/// that advanced past the offset would double-count on a restart, and an offset past the counters would
/// drop evidence. One file, one rename, one truth.
#[derive(Debug, Clone, Default)]
pub struct Counters {
    /// Records accepted and projected.
    pub accepted: u64,
    /// Lines that were blank, undecodable, or not objects (ADR-005 §3.6's `bad_lines`).
    pub bad_lines: u64,
    /// Of those, how many were merely blank.
    pub blank_lines: u64,
    /// Lines that exceeded [`MAX_LINE_BYTES`].
    pub oversized: u64,
    /// Rejections by reason.
    pub rejections: BTreeMap<String, u64>,
    /// Unprojectable events by vendor source name (ADR-005 §2).
    pub unprojectable: BTreeMap<String, u64>,
    /// Accepted events by canonical type. These values sum to [`Counters::accepted`]: a derived
    /// `test_run` is counted as `test_run` and *not* also as `tool_result`, per ADR-005 §2.2's "presented
    /// as a `test_run`". A histogram whose buckets overlap cannot be checked against the total it explains.
    pub by_event: BTreeMap<String, u64>,
    /// Recognised test runs by outcome (ADR-005 §2.2's amendment): `pass`, `fail`, `none`.
    pub test_run: BTreeMap<String, u64>,
    /// Shell commands that ran and were **not** recognised as a test runner — ADR-005 §2.2's "surfaces
    /// with `detected_by: none` rather than vanish", made countable. Without this key the honest detector
    /// and the optimistic one print the same report, which is the exact failure §2.2 was written against.
    pub unrecognised_commands: u64,
    /// Accepted events by vendor source name.
    pub by_source: BTreeMap<String, u64>,
    /// Accepted events by adapter name.
    pub by_harness: BTreeMap<String, u64>,
    /// Records whose payload was redacted, and the rules that fired.
    pub redacted: BTreeMap<String, u64>,
    /// Sessions that passed their byte budget and were quarantined.
    pub sessions_quarantined: u64,
    /// `seq` gaps detected, and the number of records they swallowed.
    pub sequence_gaps: u64,
    /// Records swallowed by those gaps.
    pub sequence_gap_records: u64,
    /// Duplicate `seq` values seen.
    pub sequence_duplicates: u64,
    /// Torn tails seen — a line with no newline at the end of the file, i.e. a writer mid-write.
    pub torn_tails: u64,
}

impl Counters {
    /// Count one rejection.
    pub fn reject(&mut self, reason: RejectReason) {
        *self
            .rejections
            .entry(reason.as_str().to_owned())
            .or_insert(0) += 1;
    }

    /// The total number of rejections of every kind.
    #[must_use]
    pub fn rejected_total(&self) -> u64 {
        self.rejections.values().sum()
    }

    /// The total number of unprojectable events.
    #[must_use]
    pub fn unprojectable_total(&self) -> u64 {
        self.unprojectable.values().sum()
    }

    /// The JSON form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "accepted": self.accepted,
            "bad_lines": self.bad_lines,
            "blank_lines": self.blank_lines,
            "oversized": self.oversized,
            "rejected_total": self.rejected_total(),
            "rejections": &self.rejections,
            "unprojectable_total": self.unprojectable_total(),
            "unprojectable_by_source": &self.unprojectable,
            "by_event": &self.by_event,
            "test_run": &self.test_run,
            "unrecognised_commands": &self.unrecognised_commands,
            "by_source": &self.by_source,
            "by_harness": &self.by_harness,
            "redacted": &self.redacted,
            "sessions_quarantined": self.sessions_quarantined,
            "sequence_gaps": self.sequence_gaps,
            "sequence_gap_records": self.sequence_gap_records,
            "sequence_duplicates": self.sequence_duplicates,
            "torn_tails": self.torn_tails,
        })
    }
}

/// Something the reader noticed that is neither an event nor a rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// The stable kind: `rotated`, `torn-tail`, `cursor-reset`, `quarantine-trimmed`.
    pub kind: String,
    /// One sentence.
    pub detail: String,
}

impl Notice {
    /// The JSON form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({"kind": self.kind, "detail": self.detail})
    }
}

/// The turn ledger: the turns this stream has produced, in allocation order.
///
/// Two rules make it deterministic, and determinism is the acceptance criterion:
///
/// * **`turn_index` is allocated here**, per session, in `seq` order — never from a field in the record.
///   Two concurrent adapters that each counted their own turns would otherwise produce the same id for
///   two turns and the same episode for both.
/// * **A turn-scoped event with no open turn opens one**, marked `inferred`. A harness that never emits a
///   prompt-submit event is a real harness; dropping its evidence would be the 0.000-recall failure this
///   ticket exists to fix. Inference is counted (`turns_inferred`) so the gap stays a number on the status
///   page rather than a quiet guess.
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    /// Per-session allocation and accounting state.
    pub sessions: BTreeMap<String, SessionState>,
    /// The turns, oldest first. Bounded by [`Limits::turns_kept`]: the trim is counted in
    /// `Counters` minus [`Ledger::turns`], and reported as a notice.
    pub turns: Vec<Turn>,
    /// How many turns the ledger has trimmed to stay inside `turns_kept`.
    pub trimmed: u64,
    /// Turns allocated, ever — including the ones since trimmed.
    pub opened: u64,
    /// Turns allocated implicitly because a turn-scoped event arrived with no `turn_start`.
    pub inferred: u64,
    /// Turns closed by the next `turn_start` rather than by a `turn_end` (ADR-005 §3.3's crash rule).
    pub closed_by_next: u64,
}

impl Ledger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Ledger::default()
    }

    /// The turn a reference points at, when the ledger still carries it.
    #[must_use]
    pub fn turn(&self, turn_id: &str) -> Option<&Turn> {
        self.turns.iter().find(|turn| turn.turn_id == turn_id)
    }

    /// The open turn of a session, if any.
    #[must_use]
    pub fn open_turn(&self, session_id: &str) -> Option<&Turn> {
        let index = self.sessions.get(session_id)?.open_turn?;
        self.turns
            .iter()
            .find(|turn| turn.session_id == session_id && turn.turn_index == index)
    }

    /// Apply one accepted event, returning the turn it was allocated to.
    ///
    /// Session-scoped events (`session_start`, `session_end`, subagent events) do not open a turn;
    /// `session_end` closes the open one.
    pub fn apply(&mut self, event: &mut Event, limits: &Limits) -> Option<TurnRef> {
        let Some(session_id) = event.session_id.clone() else {
            // Nothing can be attributed without a session; ADR-005 §3.6's fallback needs it too. A record
            // without one is still counted by type — it is evidence of *something* — it is just not
            // evidence of a turn.
            return None;
        };
        {
            let state = self.sessions.entry(session_id.clone()).or_default();
            state.events += 1;
            state.bytes += event.byte_len;
        }

        match event.fathom_event {
            FathomEvent::SessionEnd => {
                self.close_session(&session_id, event);
                return None;
            }
            FathomEvent::SessionStart => {
                // A session that declares a scope or an intent declares it for every turn in it — that is
                // how a harness usually says "this whole session is inside `src/parser/**`" (`e4.2`). The
                // turn may still state its own, and does when it does.
                let stated = crate::intent::TurnEvidence::from_payload(
                    &event.payload,
                    limits.max_intent_text_bytes,
                );
                if let Some(state) = self.sessions.get_mut(&session_id) {
                    if !stated.scope.is_empty() {
                        state.scope = stated.scope;
                    }
                    if stated.intent.is_some() {
                        state.intent = stated.intent;
                    }
                }
                return None;
            }
            FathomEvent::SubagentStart | FathomEvent::SubagentEnd => {
                return None;
            }
            _ => {}
        }
        if !event.fathom_event.is_turn_scoped() {
            return None;
        }

        let open = self
            .sessions
            .get(&session_id)
            .and_then(|state| state.open_turn);
        let (inferred, reference) = match event.fathom_event {
            FathomEvent::TurnStart => (false, self.allocate(&session_id, event, limits)),
            _ => match open {
                Some(index) => (
                    false,
                    TurnRef {
                        session_id: session_id.clone(),
                        turn_index: index,
                        turn_id: qualify_turn_id(&session_id, index),
                    },
                ),
                None => (true, self.allocate(&session_id, event, limits)),
            },
        };

        if let Some(turn) = self
            .turns
            .iter_mut()
            .find(|turn| turn.turn_id == reference.turn_id)
        {
            if inferred {
                turn.inferred = true;
            }
            if turn.cwd.is_none() {
                turn.cwd = event.cwd.clone();
            }
            if turn.git_branch.is_none() {
                turn.git_branch = event.git_branch.clone();
            }
            if matches!(event.fathom_event, FathomEvent::TurnStart) {
                // The `e4.2` capture: everything the prompt-submit payload states about *why* this turn is
                // happening, read at the boundary that already redacted the payload (ADR-005 §6 rule 4) and
                // capped to `Limits::max_intent_text_bytes`. Deciding what any of it *means* is
                // [`crate::intent`]'s job, not this reader's.
                let stated = crate::intent::TurnEvidence::from_payload(
                    &event.payload,
                    limits.max_intent_text_bytes,
                );
                turn.prompt = stated.prompt;
                turn.prompt_sha256 = stated.prompt_sha256;
                turn.prompt_chars = stated
                    .prompt_chars
                    .or_else(|| turn.prompt.as_ref().map(|text| text.chars().count() as u64));
                if stated.intent.is_some() {
                    turn.intent = stated.intent;
                }
                if !stated.plan.is_empty() {
                    turn.plan = stated.plan;
                }
                if !stated.scope.is_empty() {
                    turn.scope = stated.scope;
                }
            }
            match event.fathom_event {
                FathomEvent::ToolCall => turn.tool_calls += 1,
                FathomEvent::FileEdit => {
                    turn.edits += 1;
                    if let Some(path) = &event.path {
                        turn.paths.insert(path.clone());
                    }
                }
                FathomEvent::Approval => {}
                // An error inside a turn does not close it: a retry is the same turn's work.
                FathomEvent::Error => {}
                _ => {}
            }
            if let Some(command) = event.command()
                && let Some(detected) = detect_test_run(&command)
            {
                *turn
                    .test_runs
                    .entry(detected.as_str().to_owned())
                    .or_insert(0) += 1;
            }
        }
        if matches!(event.fathom_event, FathomEvent::TurnEnd) {
            if let Some(state) = self.sessions.get_mut(&session_id) {
                state.open_turn = None;
            }
            if let Some(turn) = self
                .turns
                .iter_mut()
                .find(|turn| turn.turn_id == reference.turn_id)
            {
                turn.ended_seq = Some(event.seq);
                turn.ended_ts = Some(event.ts.clone());
                turn.closed_by = ClosedBy::TurnEnd;
            }
        }
        if inferred {
            self.inferred += 1;
        }
        event.turn = Some(reference.clone());
        Some(reference)
    }

    /// Close a session's open turn because the session ended.
    fn close_session(&mut self, session_id: &str, event: &Event) {
        let Some(state) = self.sessions.get_mut(session_id) else {
            return;
        };
        let Some(index) = state.open_turn.take() else {
            return;
        };
        if let Some(turn) = self
            .turns
            .iter_mut()
            .find(|turn| turn.session_id == session_id && turn.turn_index == index)
        {
            turn.ended_seq = Some(event.seq);
            turn.ended_ts = Some(event.ts.clone());
            turn.closed_by = ClosedBy::SessionEnd;
        }
    }

    /// Allocate the next turn of a session, closing whatever was open.
    fn allocate(&mut self, session_id: &str, event: &Event, limits: &Limits) -> TurnRef {
        let state = self.sessions.entry(session_id.to_owned()).or_default();
        if let Some(index) = state.open_turn.take()
            && let Some(turn) = self
                .turns
                .iter_mut()
                .find(|turn| turn.session_id == session_id && turn.turn_index == index)
        {
            // ADR-005 §3.3: a turn that never ends is closed by the *next* `turn_start`, so a crash
            // mid-turn leaves no unresolvable episode. An inferred turn gets the same treatment —
            // otherwise a harness that never sends a prompt submit would hold a turn open forever
            // and every later commit of its session would land inside it.
            turn.ended_seq = Some(event.seq);
            turn.ended_ts = Some(event.ts.clone());
            turn.closed_by = ClosedBy::NextTurn;
            self.closed_by_next += 1;
        }
        state.next_turn_index += 1;
        self.opened += 1;
        let turn_index = state.next_turn_index;
        state.open_turn = Some(turn_index);
        self.turns.push(Turn {
            session_id: session_id.to_owned(),
            turn_index,
            turn_id: qualify_turn_id(session_id, turn_index),
            started_seq: event.seq,
            started_ts: event.ts.clone(),
            ended_seq: None,
            ended_ts: None,
            closed_by: ClosedBy::Open,
            inferred: false,
            paths: BTreeSet::new(),
            tool_calls: 0,
            edits: 0,
            test_runs: BTreeMap::new(),
            cwd: event.cwd.clone(),
            git_branch: event.git_branch.clone(),
            prompt: None,
            prompt_sha256: None,
            prompt_chars: None,
            // A session that declared a scope declares it for every turn in it; a turn that states its own
            // wins (`e4.2`). Same for an intent line stated at session start.
            intent: state.intent.clone(),
            plan: Vec::new(),
            scope: state.scope.clone(),
        });
        if self.turns.len() > limits.turns_kept.max(1) {
            let drop = self.turns.len() - limits.turns_kept.max(1);
            self.turns.drain(0..drop);
            self.trimmed += u64::try_from(drop).unwrap_or(u64::MAX);
        }
        TurnRef {
            session_id: session_id.to_owned(),
            turn_index,
            turn_id: qualify_turn_id(session_id, turn_index),
        }
    }

    /// Load a ledger back from the cursor's JSON, so a restart resumes mid-stream with its turns intact.
    #[must_use]
    pub fn from_json(value: &Value) -> Ledger {
        let mut ledger = Ledger::new();
        if let Some(sessions) = value.get("sessions").and_then(Value::as_object) {
            for (session, state) in sessions {
                ledger.sessions.insert(
                    session.clone(),
                    SessionState {
                        next_turn_index: state
                            .get("next_turn_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                        open_turn: state.get("open_turn").and_then(Value::as_u64),
                        events: state.get("events").and_then(Value::as_u64).unwrap_or(0),
                        bytes: state.get("bytes").and_then(Value::as_u64).unwrap_or(0),
                        quarantined: state
                            .get("quarantined")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        scope: string_list(state.get("scope")),
                        intent: state
                            .get("intent")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    },
                );
            }
        }
        if let Some(turns) = value.get("turns").and_then(Value::as_array) {
            for turn in turns {
                let (Some(session_id), Some(turn_id)) = (
                    turn.get("session_id").and_then(Value::as_str),
                    turn.get("turn_id").and_then(Value::as_str),
                ) else {
                    continue;
                };
                ledger.turns.push(Turn {
                    session_id: session_id.to_owned(),
                    turn_index: turn.get("turn_index").and_then(Value::as_u64).unwrap_or(0),
                    turn_id: turn_id.to_owned(),
                    started_seq: turn.get("started_seq").and_then(Value::as_u64).unwrap_or(0),
                    started_ts: turn
                        .get("started_ts")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    ended_seq: turn.get("ended_seq").and_then(Value::as_u64),
                    ended_ts: turn
                        .get("ended_ts")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    closed_by: match turn.get("closed_by").and_then(Value::as_str) {
                        Some("turn-end") => ClosedBy::TurnEnd,
                        Some("next-turn") => ClosedBy::NextTurn,
                        Some("session-end") => ClosedBy::SessionEnd,
                        _ => ClosedBy::Open,
                    },
                    inferred: turn
                        .get("inferred")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    paths: turn
                        .get("paths")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default(),
                    tool_calls: turn.get("tool_calls").and_then(Value::as_u64).unwrap_or(0),
                    edits: turn.get("edits").and_then(Value::as_u64).unwrap_or(0),
                    test_runs: turn
                        .get("test_runs")
                        .and_then(Value::as_object)
                        .map(|map| {
                            map.iter()
                                .filter_map(|(key, value)| {
                                    value.as_u64().map(|count| (key.clone(), count))
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    cwd: turn.get("cwd").and_then(Value::as_str).map(str::to_owned),
                    git_branch: turn
                        .get("git_branch")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    // The `e4.2` intent fields, read back so a restart resumes with the intent it already
                    // read rather than reporting `no recorded intent` for a turn it read last run.
                    prompt: turn
                        .get("prompt")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    prompt_sha256: turn
                        .get("prompt_sha256")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    prompt_chars: turn.get("prompt_chars").and_then(Value::as_u64),
                    intent: turn
                        .get("intent")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    plan: turn
                        .get("plan")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| {
                                    serde_json::from_value::<crate::intent::PlanItem>(item.clone())
                                        .ok()
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    scope: string_list(turn.get("scope")),
                });
            }
        }
        ledger.trimmed = value.get("trimmed").and_then(Value::as_u64).unwrap_or(0);
        ledger.opened = value.get("opened").and_then(Value::as_u64).unwrap_or(0);
        ledger.inferred = value.get("inferred").and_then(Value::as_u64).unwrap_or(0);
        ledger.closed_by_next = value
            .get("closed_by_next")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        ledger
    }

    /// The cursor's JSON form of the whole ledger — the state a restart needs to resume allocation
    /// exactly where it stopped.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "sessions": self
                .sessions
                .iter()
                .map(|(session, state)| (session.clone(), state.to_json()))
                .collect::<serde_json::Map<_, _>>(),
            "turns": self.turns.iter().map(Turn::to_json).collect::<Vec<_>>(),
            "trimmed": self.trimmed,
            "opened": self.opened,
            "inferred": self.inferred,
            "closed_by_next": self.closed_by_next,
        })
    }

    /// How many turns are currently open across every session.
    #[must_use]
    pub fn open_turns(&self) -> usize {
        self.sessions
            .values()
            .filter(|state| state.open_turn.is_some())
            .count()
    }

    /// The sessions under quarantine.
    #[must_use]
    pub fn quarantined_sessions(&self) -> Vec<String> {
        self.sessions
            .iter()
            .filter(|(_id, state)| state.quarantined.is_some())
            .map(|(id, _state)| id.clone())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// The durable cursor
// ---------------------------------------------------------------------------

/// The reader's durable state: how far the file has been read, what was seen, and what the turn ledger
/// needs to keep allocating ids in the same order.
///
/// One file, written with a temp-and-rename, because offset and counters must advance **together**: an
/// offset ahead of the counters reprocesses records, and counters ahead of the offset silently drops
/// evidence. A corrupt cursor file is not fatal either — the reader starts from byte 0, which is exactly a
/// replay and reproduces the same state by determinism rather than by luck (see [`Reader::open_cursor`]).
#[derive(Debug, Clone)]
pub struct Cursor {
    /// The cursor format version, distinct from the event schema version.
    pub version: u64,
    /// The events file this cursor belongs to, as the CLI printed it.
    pub events_file: String,
    /// Bytes consumed. The next read starts here.
    pub offset: u64,
    /// The file's size at the end of the last read, so a shrink is noticed rather than assumed.
    pub size_at_read: u64,
    /// The highest `seq` accepted.
    pub last_seq: u64,
    /// The last accepted record's timestamp, verbatim.
    pub last_ts: Option<String>,
    /// When this cursor was written, unix millis — passed in, never read from a clock here.
    pub read_at_ms: i64,
    /// How many read passes produced this state.
    pub reads: u64,
    /// How many times the file shrank under the cursor (rotation, truncation, a `git clean`).
    pub rotations: u64,
    /// Everything counted so far.
    pub counters: Counters,
    /// The turn ledger, with its per-session allocation state.
    pub ledger: Ledger,
    /// Notices worth printing again after a restart.
    pub notices: Vec<Notice>,
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor {
            version: 1,
            events_file: String::new(),
            offset: 0,
            size_at_read: 0,
            last_seq: 0,
            last_ts: None,
            read_at_ms: 0,
            reads: 0,
            rotations: 0,
            counters: Counters::default(),
            ledger: Ledger::new(),
            notices: Vec::new(),
        }
    }
}

impl Cursor {
    /// The digest of the turn ledger this cursor carries, as `0x…`.
    ///
    /// This is the reader's half of "replay is deterministic": a rebuild that allocated the same turns from
    /// the same bytes prints the same digest, so a reviewer watching it change knows the *reader* moved
    /// rather than the repository. It is deliberately **not** [`Linkage::digest`], which additionally commits
    /// to which commits hung off those turns and so needs git in hand — the index pass computes that one.
    ///
    /// The framing is canonical for the same reasons [`Linkage::digest`] states: length-prefixed fields and
    /// the schema name, so no turn id can shift a boundary into another field.
    #[must_use]
    pub fn ledger_digest(&self) -> String {
        let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
        sha2::Digest::update(&mut hasher, LINKAGE_SCHEMA.as_bytes());
        sha2::Digest::update(&mut hasher, [0x1f]);
        for turn in &self.ledger.turns {
            for (key, value) in turn.digest_fields() {
                for field in [key, &value] {
                    sha2::Digest::update(&mut hasher, field.as_bytes());
                    sha2::Digest::update(&mut hasher, [0x1f]);
                }
            }
            sha2::Digest::update(&mut hasher, [0x1e]);
        }
        format!(
            "0x{}",
            sha2::Digest::finalize(hasher)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    }

    /// Every turn id this ledger allocated, in allocation order.
    #[must_use]
    pub fn turn_ids(&self) -> Vec<String> {
        self.ledger
            .turns
            .iter()
            .map(|turn| turn.turn_id.clone())
            .collect()
    }

    /// A cursor positioned at the start of `events_file`.
    #[must_use]
    pub fn fresh(events_file: &str) -> Self {
        Cursor {
            events_file: events_file.to_owned(),
            ..Cursor::default()
        }
    }

    /// The cursor file's JSON.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "version": self.version,
            "protocol": PROTOCOL,
            "events_file": self.events_file,
            "offset": self.offset,
            "size_at_read": self.size_at_read,
            "last_seq": self.last_seq,
            "last_ts": self.last_ts,
            "read_at_ms": self.read_at_ms,
            "reads": self.reads,
            "rotations": self.rotations,
            "counters": self.counters.to_json(),
            "ledger": self.ledger.to_json(),
            "notices": self.notices.iter().map(Notice::to_json).collect::<Vec<_>>(),
        })
    }

    /// Parse a cursor file's JSON. A document that is not a cursor at all is an error, so the caller can
    /// say so instead of quietly starting over.
    pub fn from_json(value: &Value) -> Result<Cursor, String> {
        if value.get("version").and_then(Value::as_u64) != Some(1) {
            return Err(format!(
                "cursor version is {:?}, and this build reads version 1",
                value.get("version")
            ));
        }
        let mut counters = Counters::default();
        if let Some(map) = value.get("counters").and_then(Value::as_object) {
            let number = |key: &str| -> u64 { map.get(key).and_then(Value::as_u64).unwrap_or(0) };
            let histogram = |key: &str| -> BTreeMap<String, u64> {
                map.get(key)
                    .and_then(Value::as_object)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|(name, count)| {
                                count.as_u64().map(|count| (name.clone(), count))
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            counters = Counters {
                accepted: number("accepted"),
                bad_lines: number("bad_lines"),
                blank_lines: number("blank_lines"),
                oversized: number("oversized"),
                rejections: histogram("rejections"),
                unprojectable: histogram("unprojectable_by_source"),
                by_event: histogram("by_event"),
                test_run: histogram("test_run"),
                unrecognised_commands: number("unrecognised_commands"),
                by_source: histogram("by_source"),
                by_harness: histogram("by_harness"),
                redacted: histogram("redacted"),
                sessions_quarantined: number("sessions_quarantined"),
                sequence_gaps: number("sequence_gaps"),
                sequence_gap_records: number("sequence_gap_records"),
                sequence_duplicates: number("sequence_duplicates"),
                torn_tails: number("torn_tails"),
            };
        }
        Ok(Cursor {
            version: 1,
            events_file: value
                .get("events_file")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            offset: value.get("offset").and_then(Value::as_u64).unwrap_or(0),
            size_at_read: value
                .get("size_at_read")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            last_seq: value.get("last_seq").and_then(Value::as_u64).unwrap_or(0),
            last_ts: value
                .get("last_ts")
                .and_then(Value::as_str)
                .map(str::to_owned),
            read_at_ms: value.get("read_at_ms").and_then(Value::as_i64).unwrap_or(0),
            reads: value.get("reads").and_then(Value::as_u64).unwrap_or(0),
            rotations: value.get("rotations").and_then(Value::as_u64).unwrap_or(0),
            counters,
            ledger: Ledger::from_json(value.get("ledger").unwrap_or(&Value::Null)),
            notices: value
                .get("notices")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            Some(Notice {
                                kind: item.get("kind")?.as_str()?.to_owned(),
                                detail: item.get("detail")?.as_str()?.to_owned(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// The quarantine sidecar: refused lines with their diagnostics, next to the file they came from.
///
/// The events file is never rewritten to remove damage (ADR-005 §3.6), so the *diagnostics* have to live
/// somewhere an operator can read without a hex editor. This is that place. It is bounded, and the number
/// of records it dropped is counted rather than forgotten.
#[derive(Debug, Clone)]
pub struct Quarantine {
    /// Where the records go.
    pub path: PathBuf,
    /// How many to keep before dropping the oldest.
    pub keep: usize,
    /// Records appended by this run.
    pub appended: u64,
    /// Records dropped by the bound, across the file's life.
    pub dropped: u64,
}

impl Quarantine {
    /// Open the sidecar at `path`.
    #[must_use]
    pub fn new(path: PathBuf, keep: usize) -> Self {
        Quarantine {
            path,
            keep: keep.max(16),
            appended: 0,
            dropped: 0,
        }
    }

    /// Append one rejection. An append failure is returned, not swallowed: a quarantine that cannot be
    /// written means the operator cannot see the damage, which is worth a line on stderr.
    pub fn append(&mut self, rejection: &Rejection) -> Result<(), String> {
        use std::io::Write as _;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|err| format!("{}: {err}", self.path.display()))?;
        let mut line = serde_json::to_vec(&rejection.to_json()).unwrap_or_default();
        line.push(b'\n');
        file.write_all(&line)
            .map_err(|err| format!("{}: {err}", self.path.display()))?;
        self.appended += 1;
        Ok(())
    }

    /// How many records are in the file.
    #[must_use]
    pub fn records(&self) -> usize {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => text.lines().filter(|line| !line.trim().is_empty()).count(),
            Err(_) => 0,
        }
    }

    /// Bound the file, keeping the newest `self.keep` records.
    ///
    /// Returns how many were dropped. The drop is reported because a quarantine that silently rotates is a
    /// quarantine whose oldest — and usually most interesting — evidence is the first to go.
    pub fn trim(&mut self) -> Result<u64, String> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Ok(0);
        };
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        if lines.len() <= self.keep * 2 {
            return Ok(0);
        }
        let keep = self.keep;
        let mut kept = lines[lines.len() - keep..].join("\n");
        kept.push('\n');
        let dropped = u64::try_from(lines.len() - keep).unwrap_or(u64::MAX);
        std::fs::write(&self.path, kept)
            .map_err(|err| format!("{}: {err}", self.path.display()))?;
        self.dropped += dropped;
        Ok(dropped)
    }
}

// ---------------------------------------------------------------------------
// The reader
// ---------------------------------------------------------------------------

/// What one line was, as the scanner saw it.
#[derive(Debug, Clone)]
pub enum Scanned {
    /// A complete line (no newline included).
    Complete {
        /// Byte offset of the line's first byte.
        offset: u64,
        /// The bytes, without the newline.
        bytes: Vec<u8>,
        /// The line's length including the newline.
        len: u64,
        /// sha256 of the line's bytes.
        digest: String,
    },
    /// A complete line longer than the cap: the prefix is kept for the diagnostic, the whole line is
    /// hashed, and nothing is truncated into an event (ADR-005 §3.6).
    Oversized {
        /// Byte offset of the line's first byte.
        offset: u64,
        /// The retained prefix, capped at the reader's line limit.
        preview: Vec<u8>,
        /// The whole line's length including the newline.
        len: u64,
        /// sha256 over the whole line, prefix included.
        digest: String,
    },
}

impl Scanned {
    /// The line's offset.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        match self {
            Scanned::Complete { offset, .. } | Scanned::Oversized { offset, .. } => *offset,
        }
    }

    /// Whether the scan consumed nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The line's total length.
    #[must_use]
    pub const fn len(&self) -> u64 {
        match self {
            Scanned::Complete { len, .. } | Scanned::Oversized { len, .. } => *len,
        }
    }

    /// The digest of the whole line.
    #[must_use]
    pub fn digest(&self) -> &str {
        match self {
            Scanned::Complete { digest, .. } | Scanned::Oversized { digest, .. } => digest,
        }
    }

    /// Whether the line was empty after trimming.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        match self {
            Scanned::Complete { bytes, .. } => bytes.iter().all(u8::is_ascii_whitespace),
            Scanned::Oversized { .. } => false,
        }
    }

    /// The first bytes of the line, redacted, for a quarantine preview.
    #[must_use]
    pub fn preview(&self) -> String {
        const PREVIEW_BYTES: usize = 400;
        let raw: &[u8] = match self {
            Scanned::Complete { bytes, .. } => bytes,
            Scanned::Oversized { preview, .. } => preview,
        };
        let mut end = raw.len().min(PREVIEW_BYTES);
        while end > 0 && !std::str::from_utf8(&raw[..end]).is_ok() {
            end -= 1;
        }
        let text = String::from_utf8_lossy(&raw[..end]);
        let (clean, _hits) = redact(&text);
        if raw.len() > end {
            format!("{clean}\u{2026}{} more bytes", raw.len() - end)
        } else {
            clean
        }
    }
}

/// What a scan pass found, from a starting offset to the last *complete* line.
#[derive(Debug, Default)]
pub struct Scan {
    /// The file's size in bytes.
    pub size_bytes: u64,
    /// The offset a caller may safely advance to: past every complete line, before any torn tail.
    pub consumed_to: u64,
    /// Complete lines seen.
    pub lines: u64,
    /// A trailing line with no newline: a writer caught mid-write. Its bytes are **not** consumed.
    pub torn_tail: Option<(u64, u64)>,
}

/// The events file, the reader's state directory, and the limits it enforces.
#[derive(Debug, Clone)]
pub struct Reader {
    /// The events file.
    pub events: PathBuf,
    /// Where the cursor and the quarantine live: a `hooks/` directory beside the events file.
    pub state_dir: PathBuf,
    /// What the reader enforces.
    pub limits: Limits,
}

impl Reader {
    /// Point a reader at an events file with the default limits.
    #[must_use]
    pub fn new(events: impl Into<PathBuf>) -> Self {
        Reader::with_limits(events, Limits::default())
    }

    /// Point a reader at an events file with given limits. The state directory is `<events dir>/hooks`,
    /// so moving the events file moves the reader's whole state with it.
    #[must_use]
    pub fn with_limits(events: impl Into<PathBuf>, limits: Limits) -> Self {
        let events: PathBuf = events.into();
        let dir = events
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), |parent| parent.to_path_buf());
        Reader {
            events,
            state_dir: dir.join("hooks"),
            limits,
        }
    }

    /// The cursor file.
    #[must_use]
    pub fn cursor_path(&self) -> PathBuf {
        self.state_dir.join("cursor.json")
    }

    /// The quarantine file.
    #[must_use]
    pub fn quarantine_path(&self) -> PathBuf {
        self.state_dir.join("quarantine.jsonl")
    }

    /// Load the cursor, or start a fresh one.
    ///
    /// A cursor that does not parse is moved aside (never deleted: it is evidence about the failure) and
    /// the read restarts from byte 0. That is not a lossy fallback: reading from 0 *is* a replay, and a
    /// replay is defined to reproduce the same state, so the worst a corrupt cursor costs is the time it
    /// takes to rebuild it. The notice says so, because a silent reset is how a corrupted file becomes a
    /// mystery.
    #[must_use]
    pub fn open_cursor(&self, now_ms: i64) -> (Cursor, Option<Notice>) {
        let path = self.cursor_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) => match Cursor::from_json(&value) {
                    Ok(cursor) => (cursor, None),
                    Err(why) => (self.fresh_cursor(&why, now_ms), None),
                },
                Err(_) => {
                    let aside = path.with_extension(format!("corrupt.{now_ms}"));
                    let _ = std::fs::rename(&path, &aside);
                    let cursor = self.fresh_cursor(
                        &format!(
                            "{} did not parse and was moved to {}; the stream was replayed from byte 0",
                            path.display(),
                            aside.display()
                        ),
                        now_ms,
                    );
                    let notice = cursor.notices.last().cloned();
                    (cursor, notice)
                }
            },
            Err(_) => (Cursor::fresh(&self.display_events()), None),
        }
    }

    fn fresh_cursor(&self, why: &str, now_ms: i64) -> Cursor {
        let mut cursor = Cursor::fresh(&self.display_events());
        cursor.notices.push(Notice {
            kind: "cursor-reset".to_owned(),
            detail: why.to_owned(),
        });
        cursor.read_at_ms = now_ms;
        cursor
    }

    /// The events path as this report prints it.
    #[must_use]
    pub fn display_events(&self) -> String {
        self.events.display().to_string()
    }

    /// Whether the events file exists at all. Absent is a normal state, not an error: ADR-003 §9.2 makes
    /// "no events" an absence of evidence rather than a failure.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.events.is_file()
    }

    /// The file's size, or 0 when it is absent.
    #[must_use]
    pub fn size(&self) -> u64 {
        std::fs::metadata(&self.events)
            .map(|meta| meta.len())
            .unwrap_or(0)
    }

    /// Read complete lines from `from` to the end of the file, calling `on_line` for each.
    ///
    /// The scanner never hands out a line it cannot see the end of, which is the whole torn-write rule: a
    /// harness that is mid-append leaves bytes after the last newline, and those bytes stay unread until
    /// the next pass writes the rest of them. Memory is bounded by the line cap even when a line is not:
    /// past the cap the bytes are hashed and dropped rather than buffered.
    ///
    /// # Errors
    ///
    /// An I/O failure reading the file. A *malformed* line is never an error here; it is a line.
    pub fn scan<F: FnMut(&Scanned)>(&self, from: u64, on_line: F) -> Result<Scan, String> {
        self.scan_within(from, None, on_line)
    }

    /// [`scan`](Self::scan), with a cap on how many bytes one pass may consume.
    ///
    /// The cap is the daemon's back-pressure (`e6.3`): a harness that appends faster than the daemon may
    /// read makes the *lag* grow instead of making the daemon's cycle time grow, and the lag is a number
    /// on a status page rather than a surprise in a budget. Stopping happens only at a line boundary, so
    /// a capped pass still leaves the cursor on a byte the next pass can pick up, and a torn tail is
    /// reported exactly as it would be by an uncapped pass that reached it.
    ///
    /// # Errors
    ///
    /// As [`scan`](Self::scan).
    pub fn scan_within<F: FnMut(&Scanned)>(
        &self,
        from: u64,
        within: Option<u64>,
        mut on_line: F,
    ) -> Result<Scan, String> {
        let mut scan = Scan {
            size_bytes: self.size(),
            consumed_to: from.min(self.size()),
            ..Scan::default()
        };
        if !self.exists() {
            scan.consumed_to = 0;
            return Ok(scan);
        }
        let file = std::fs::File::open(&self.events)
            .map_err(|err| format!("{}: {err}", self.events.display()))?;
        let len = file
            .metadata()
            .map(|meta| meta.len())
            .map_err(|err| format!("{}: {err}", self.events.display()))?;
        scan.size_bytes = len;
        if from > len {
            // The file shrank (rotation, truncation, a checkout). The caller decides what that means; this
            // only promises not to seek past the end and pretend the read succeeded.
            scan.consumed_to = 0;
            return Ok(scan);
        }
        let mut file = file;
        file.seek(SeekFrom::Start(from))
            .map_err(|err| format!("{}: {err}", self.events.display()))?;
        scan.consumed_to = from;

        let cap = self.limits.max_line_bytes.max(1);
        let mut buffer = vec![0u8; 64 * 1024];
        let mut line: Vec<u8> = Vec::new();
        let mut hasher = Sha256::new();
        let mut position = from;
        let mut line_start = from;
        let mut line_bytes = 0u64;
        let mut overflowed = false;
        'pass: loop {
            let read = file
                .read(&mut buffer)
                .map_err(|err| format!("{}: {err}", self.events.display()))?;
            if read == 0 {
                break;
            }
            let mut rest = &buffer[..read];
            while !rest.is_empty() {
                match rest.iter().position(|byte| *byte == b'\n') {
                    Some(at) => {
                        let (chunk, after) = rest.split_at(at);
                        hasher.update(chunk);
                        hasher.update(b"\n");
                        line_bytes += u64::try_from(chunk.len()).unwrap_or(u64::MAX) + 1;
                        if !overflowed && !buffer_into(&mut line, chunk, cap) {
                            overflowed = true;
                        }
                        let digest = digest_finalize(&mut hasher);
                        if overflowed {
                            on_line(&Scanned::Oversized {
                                offset: line_start,
                                preview: std::mem::take(&mut line),
                                len: line_bytes,
                                digest,
                            });
                        } else {
                            on_line(&Scanned::Complete {
                                offset: line_start,
                                bytes: std::mem::take(&mut line),
                                len: line_bytes,
                                digest,
                            });
                        }
                        scan.lines += 1;
                        position += line_bytes;
                        scan.consumed_to = position;
                        line_start = position;
                        line_bytes = 0;
                        overflowed = false;
                        // The cap is checked here and only here: a full line has just been consumed, so
                        // stopping is always resumable. Stopping in the middle of a line would leave the
                        // cursor somewhere the next pass cannot pick up from without buffering.
                        if within.is_some_and(|cap| position - from >= cap) {
                            break 'pass;
                        }
                        rest = &after[1.min(after.len())..];
                    }
                    None => {
                        hasher.update(rest);
                        line_bytes += u64::try_from(rest.len()).unwrap_or(u64::MAX);
                        if !overflowed && !buffer_into(&mut line, rest, cap) {
                            overflowed = true;
                        }
                        position += u64::try_from(rest.len()).unwrap_or(u64::MAX);
                        rest = &[];
                    }
                }
            }
        }
        if line_bytes > 0 {
            scan.torn_tail = Some((line_start, line_bytes));
        }
        Ok(scan)
    }

    /// Read everything after the cursor's offset, advancing it, and quarantine what it refused.
    ///
    /// This is the function the pipeline runs in a loop. It never returns an error because a record was
    /// bad — bad records are counted, quarantined and stepped over, which is the acceptance criterion
    /// ("malformed events are quarantined and counted; the stream keeps flowing"). It errors only when the
    /// file itself cannot be read.
    ///
    /// # Errors
    ///
    /// A [`String`] describing the I/O failure, when the events file or the quarantine cannot be read or
    /// written.
    pub fn read_after(&self, cursor: &mut Cursor, now_ms: i64) -> Result<Batch, String> {
        self.read_after_within(cursor, now_ms, None)
    }

    /// [`read_after`](Self::read_after), capped at `within` bytes of complete lines.
    ///
    /// This is the form the daemon runs: the cursor advances by at most `within` bytes per pass, so one
    /// cycle of a long-lived process is bounded no matter what the harness wrote while it was busy. What
    /// was not read is not lost — it is the lag, and `fathom status` prints it.
    ///
    /// # Errors
    ///
    /// As [`read_after`](Self::read_after).
    pub fn read_after_within(
        &self,
        cursor: &mut Cursor,
        now_ms: i64,
        within: Option<u64>,
    ) -> Result<Batch, String> {
        let mut batch = Batch::default();
        let mut quarantine =
            Quarantine::new(self.quarantine_path(), self.limits.quarantine_records);
        if !self.exists() {
            batch.notices.push(Notice {
                kind: "no-events-file".to_owned(),
                detail: format!(
                    "{} does not exist: the harness has written nothing yet, which is an absence of \
                     evidence and not an error (ADR-003 §9.2)",
                    self.events.display()
                ),
            });
            return Ok(batch);
        }
        let size = self.size();
        if cursor.offset > size {
            cursor.rotations += 1;
            cursor.offset = 0;
            cursor.last_seq = 0;
            cursor.counters = Counters::default();
            cursor.ledger = Ledger::new();
            let notice = Notice {
                kind: "rotated".to_owned(),
                detail: format!(
                    "{} is {size} bytes, below the cursor's offset: the file was truncated or rotated, so \
                     the read restarted from byte 0 and the counters were rebuilt",
                    self.events.display()
                ),
            };
            cursor.notices.push(notice.clone());
            batch.notices.push(notice);
        }
        let mut rejected: Vec<Rejection> = Vec::new();
        // The accepted high-water mark *before* this pass: the sequence-gap test below measures from here,
        // because the loop moves `cursor.last_seq` as it goes.
        let seq_before = cursor.last_seq;
        let scan = self.scan_within(cursor.offset, within, |scanned| {
            self.consume(scanned, cursor, &mut batch, &mut rejected);
        })?;
        batch.lines = scan.lines;
        batch.bytes = scan.consumed_to.saturating_sub(cursor.offset);
        batch.last_seq_in_file = batch.last_seq_in_file.max(cursor.last_seq);
        if let Some((offset, bytes)) = scan.torn_tail {
            cursor.counters.torn_tails += 1;
            batch.torn_tail = Some(TornTail { offset, bytes });
            batch.notices.push(Notice {
                kind: "torn-tail".to_owned(),
                detail: format!(
                    "{bytes} bytes at offset {offset} have no newline yet: a writer is mid-append. They \
                     stay unread and uncounted until the newline lands, which is why the offset did not \
                     move past them"
                ),
            });
        }
        cursor.offset = scan.consumed_to;
        cursor.size_at_read = scan.size_bytes;
        cursor.reads += 1;
        cursor.read_at_ms = now_ms;
        if let Some(notice) = batch
            .notices
            .iter()
            .find(|notice| notice.kind == "torn-tail")
        {
            // Kept on the cursor as well: a stream that is *always* caught mid-write is a fact about the
            // writer, and it should still be on the report after the tail completed.
            cursor.notices.push(notice.clone());
        }
        // ADR-005 §3.3: `seq` is allocated by the file, so a jump means the records between are not in the
        // file at all. The test runs once, after the pass, and against every seq the pass *saw* — accepted
        // and refused alike. That distinction is the whole point: a line refused for a bad path or an unknown
        // schema still tells us the file advanced past its seq, so it is quarantined rather than lost, and a
        // reader that measured gaps between accepted seqs alone would announce a gap for every damaged line
        // it stepped over. That noise is exactly what would hide a real hole.
        {
            let mut seen: Vec<u64> = batch
                .events
                .iter()
                .map(|event| event.seq)
                .chain(rejected.iter().filter_map(|rejection| rejection.seq))
                .filter(|seq| *seq > seq_before)
                .collect();
            seen.sort_unstable();
            seen.dedup();
            let mut position = seq_before;
            for seq in seen {
                if position > 0 && seq > position + 1 {
                    let missing = seq - position - 1;
                    cursor.counters.sequence_gaps += 1;
                    cursor.counters.sequence_gap_records += missing;
                    let notice = Notice {
                        kind: "sequence-gap".to_owned(),
                        detail: format!(
                            "{missing} record(s) between seq {position} and seq {seq} are not in the file"
                        ),
                    };
                    cursor.notices.push(notice.clone());
                    batch.notices.push(notice);
                }
                position = position.max(seq);
            }
            batch.last_seq_in_file = batch.last_seq_in_file.max(position);
        }

        for rejection in &rejected {
            if self.limits.write_quarantine {
                quarantine.append(rejection)?;
            }
        }
        batch.rejections = rejected;
        batch.quarantine_records = quarantine.records();
        if self.limits.write_quarantine {
            batch.quarantine_dropped = quarantine.trim()?;
        }
        Ok(batch)
    }

    /// One line, accounted for.
    fn consume(
        &self,
        scanned: &Scanned,
        cursor: &mut Cursor,
        batch: &mut Batch,
        rejected: &mut Vec<Rejection>,
    ) {
        let record: Option<Value> = if scanned.is_blank() {
            cursor.counters.bad_lines += 1;
            cursor.counters.blank_lines += 1;
            rejected.push(Rejection {
                offset: scanned.offset(),
                reason: RejectReason::Blank,
                detail: "the line is blank".to_owned(),
                line_bytes: scanned.len(),
                seq: None,
                digest: scanned.digest().to_owned(),
                preview: scanned.preview(),
            });
            return;
        } else {
            match scanned {
                Scanned::Oversized { len, .. } => {
                    cursor.counters.bad_lines += 1;
                    cursor.counters.oversized += 1;
                    cursor.counters.reject(RejectReason::Oversized);
                    rejected.push(Rejection {
                        offset: scanned.offset(),
                        reason: RejectReason::Oversized,
                        detail: format!(
                            "the line is {len} bytes, over the {} byte cap; it was refused whole and not \
                             truncated",
                            self.limits.max_line_bytes
                        ),
                        line_bytes: *len,
                        seq: None,
                        digest: scanned.digest().to_owned(),
                        preview: scanned.preview(),
                    });
                    return;
                }
                Scanned::Complete { bytes, .. } => {
                    match serde_json::from_slice::<Value>(bytes) {
                        Ok(value) => Some(value),
                        Err(err) => {
                            // A line that will not decode is damage in ADR-005 §3.6's sense: counted,
                            // quarantined, and stepped over.
                            cursor.counters.bad_lines += 1;
                            cursor.counters.reject(RejectReason::Undecodable);
                            rejected.push(Rejection {
                                offset: scanned.offset(),
                                reason: RejectReason::Undecodable,
                                detail: format!("the line is not one JSON object: {err}"),
                                line_bytes: scanned.len(),
                                seq: None,
                                digest: scanned.digest().to_owned(),
                                preview: scanned.preview(),
                            });
                            return;
                        }
                    }
                }
            }
        };

        let value = match record {
            Some(value) => value,
            None => {
                cursor.counters.bad_lines += 1;
                cursor.counters.reject(RejectReason::NotAnObject);
                rejected.push(Rejection {
                    offset: scanned.offset(),
                    reason: RejectReason::NotAnObject,
                    detail: "the line is JSON but not an object".to_owned(),
                    line_bytes: scanned.len(),
                    seq: None,
                    digest: scanned.digest().to_owned(),
                    preview: scanned.preview(),
                });
                return;
            }
        };

        let mut event = match validate_record(&value, scanned.offset(), scanned.len()) {
            Ok(event) => event,
            Err(reject) => {
                // A line that is not JSON, or is JSON but not an object, is a *bad line* in ADR-005 §3.6's
                // sense: the file has damage in it, and `--check` counts it as such whoever noticed.
                if matches!(
                    reject.reason,
                    RejectReason::Undecodable | RejectReason::NotAnObject
                ) {
                    cursor.counters.bad_lines += 1;
                }
                cursor.counters.reject(reject.reason);
                rejected.push(Rejection {
                    offset: scanned.offset(),
                    reason: reject.reason,
                    detail: reject.detail,
                    line_bytes: scanned.len(),
                    seq: reject.seq,
                    digest: scanned.digest().to_owned(),
                    preview: scanned.preview(),
                });
                return;
            }
        };

        // Sequence: `seq` is allocated by the file, so a repeat means either a rewrite of a region already
        // read or two writers that did not take the lock, and a gap means records were lost. Neither is a
        // reason to stop: the duplicate is refused and the gap is counted (ADR-005 §3.3).
        if event.seq <= cursor.last_seq {
            cursor.counters.sequence_duplicates += 1;
            cursor.counters.reject(RejectReason::SequenceDuplicate);
            rejected.push(Rejection {
                offset: scanned.offset(),
                reason: RejectReason::SequenceDuplicate,
                detail: format!(
                    "`seq` {} was already read (the last accepted was {}): a duplicate would double-count \
                     its evidence",
                    event.seq, cursor.last_seq
                ),
                line_bytes: scanned.len(),
                seq: Some(event.seq),
                digest: scanned.digest().to_owned(),
                preview: scanned.preview(),
            });
            return;
        }
        // (The sequence-gap test lives with the seq tracking above, where refused lines count as seen.)

        // Per-session budget: over quota the session is quarantined and its records are skipped until an
        // operator says otherwise (ADR-005 §6 rule 3).
        if let Some(session) = event.session_id.clone() {
            let state = cursor.ledger.sessions.entry(session.clone()).or_default();
            if let Some(why) = state.quarantined.clone() {
                cursor.counters.reject(RejectReason::SessionQuarantined);
                rejected.push(Rejection {
                    offset: scanned.offset(),
                    reason: RejectReason::SessionQuarantined,
                    detail: format!("session {session} is quarantined: {why}"),
                    line_bytes: scanned.len(),
                    seq: Some(event.seq),
                    digest: scanned.digest().to_owned(),
                    preview: scanned.preview(),
                });
                return;
            }
            if state.bytes >= self.limits.max_bytes_per_session {
                let why = format!(
                    "it passed the {} byte per-session budget",
                    self.limits.max_bytes_per_session
                );
                state.quarantined = Some(why.clone());
                cursor.counters.sessions_quarantined += 1;
                cursor.counters.reject(RejectReason::OverQuota);
                rejected.push(Rejection {
                    offset: scanned.offset(),
                    reason: RejectReason::OverQuota,
                    detail: format!("session {session}: {why}"),
                    line_bytes: scanned.len(),
                    seq: Some(event.seq),
                    digest: scanned.digest().to_owned(),
                    preview: scanned.preview(),
                });
                return;
            }
        }

        // The turn allocation happens here, on the cursor's own ledger, so the durable state and the
        // event's turn reference advance together.
        cursor.ledger.apply(&mut event, &self.limits);
        cursor.counters.accepted += 1;
        cursor.last_seq = event.seq;
        cursor.last_ts = Some(event.ts.clone());
        *cursor.counters.by_event.entry(event.bucket()).or_insert(0) += 1;
        *cursor
            .counters
            .by_source
            .entry(event.source.clone())
            .or_insert(0) += 1;
        *cursor
            .counters
            .by_harness
            .entry(event.harness.clone())
            .or_insert(0) += 1;
        if matches!(event.fathom_event, FathomEvent::None) {
            *cursor
                .counters
                .unprojectable
                .entry(event.source.clone())
                .or_insert(0) += 1;
        }
        for rule in event.redacted.iter() {
            *cursor
                .counters
                .redacted
                .entry((*rule).to_owned())
                .or_insert(0) += 1;
        }
        if let Some(status) = event.test_run_status() {
            *cursor
                .counters
                .test_run
                .entry(status.to_owned())
                .or_insert(0) += 1;
        } else if matches!(event.fathom_event, FathomEvent::ToolResult)
            && event
                .command()
                .is_some_and(|command| matches!(detect_test_run(&command), Some(DetectedBy::None)))
        {
            cursor.counters.unrecognised_commands += 1;
        }
        batch.events.push(event);
    }
}

/// Extend a line buffer, reporting whether it still fits under `cap`.
///
/// Past the cap the bytes are dropped (they are still hashed, so the quarantine's digest covers the whole
/// line) — a reader that buffered an unbounded line would let one bad writer exhaust the process's memory,
/// which is the failure ADR-005 §6 rule 3's size limits exist to close.
fn buffer_into(line: &mut Vec<u8>, chunk: &[u8], cap: usize) -> bool {
    let room = cap.saturating_sub(line.len());
    if chunk.len() > room {
        line.extend_from_slice(&chunk[..room]);
        false
    } else {
        line.extend_from_slice(chunk);
        true
    }
}

/// Finalize a hasher, leaving a fresh one in its place.
fn digest_finalize(hasher: &mut Sha256) -> String {
    let taken = std::mem::replace(hasher, Sha256::new());
    taken
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A line caught mid-write.
#[derive(Debug, Clone, Copy)]
pub struct TornTail {
    /// Where the partial line starts.
    pub offset: u64,
    /// How many bytes are there so far.
    pub bytes: u64,
}

/// One read pass.
#[derive(Debug, Default)]
pub struct Batch {
    /// The events accepted, in `seq` order.
    pub events: Vec<Event>,
    /// The lines refused, with their diagnostics (already written to the quarantine too).
    pub rejections: Vec<Rejection>,
    /// Complete lines scanned.
    pub lines: u64,
    /// Bytes consumed by this pass.
    pub bytes: u64,
    /// A trailing partial line, if one was found.
    pub torn_tail: Option<TornTail>,
    /// Notices raised by this pass.
    pub notices: Vec<Notice>,
    /// Quarantine records in the sidecar after this pass.
    pub quarantine_records: usize,
    /// Records the sidecar's bound dropped.
    pub quarantine_dropped: u64,
    /// The highest `seq` seen in the lines this pass read, accepted or not.
    pub last_seq_in_file: u64,
}

/// What a read *would* have produced, computed without moving the durable offset.
///
/// `fathom hooks --check` is the command an operator runs when something looks wrong, and the honest answer
/// to "what is in this file" is not "nothing, I have not read it yet". So a check does a throwaway read of
/// the unread region and reports it here: the offset in the report stays where the cursor says it is, and
/// the probe says what the next real read will do. The two are reported separately rather than merged
/// because conflating them would make a diagnostic look like consumption.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    /// Complete lines ahead of the offset.
    pub lines: u64,
    /// Bytes ahead of the offset.
    pub bytes: u64,
    /// Events that read cleanly.
    pub events: u64,
    /// Lines that would be refused, by reason.
    pub refusals: BTreeMap<String, u64>,
    /// Bytes at the tail that are mid-write.
    pub torn_bytes: u64,
    /// Turns this read would open.
    pub turns_opened: u64,
    /// Turns left open by this read — expected for a session still running.
    pub turns_open: u64,
}

impl Probe {
    /// Build a probe from a read whose cursor was a copy.
    #[must_use]
    pub fn of(batch: &Batch, before: &Ledger, after: &Ledger) -> Self {
        let mut refusals: BTreeMap<String, u64> = BTreeMap::new();
        for rejection in &batch.rejections {
            *refusals
                .entry(rejection.reason.as_str().to_owned())
                .or_insert(0) += 1;
        }
        Probe {
            lines: batch.lines,
            bytes: batch.bytes,
            events: batch.events.len() as u64,
            refusals,
            torn_bytes: batch.torn_tail.map_or(0, |torn| torn.bytes),
            turns_opened: after.opened.saturating_sub(before.opened),
            turns_open: u64::try_from(after.open_turns().saturating_sub(before.open_turns()))
                .unwrap_or(u64::MAX),
        }
    }

    /// How many lines would be refused.
    #[must_use]
    pub fn refused_total(&self) -> u64 {
        self.refusals.values().sum()
    }

    /// The probe as JSON.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "lines": self.lines,
            "bytes": self.bytes,
            "events": self.events,
            "refusals": &self.refusals,
            "refused_total": self.refused_total(),
            "torn_bytes": self.torn_bytes,
            "turns_opened": self.turns_opened,
            "turns_open": self.turns_open,
        })
    }
}

impl Reader {
    /// Write the cursor: a temp file in the same directory, then a rename.
    ///
    /// The rename is the whole point. A cursor written in place would be half-written by exactly the crash
    /// this module is built to survive, and a half cursor means re-reading the whole stream.
    ///
    /// # Errors
    ///
    /// The I/O failure, as a sentence naming the file.
    pub fn save_cursor(&self, cursor: &Cursor) -> Result<(), String> {
        std::fs::create_dir_all(&self.state_dir)
            .map_err(|err| format!("{}: {err}", self.state_dir.display()))?;
        let path = self.cursor_path();
        let temp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&cursor.to_json())
            .map_err(|err| format!("{}: {err}", path.display()))?;
        std::fs::write(&temp, text).map_err(|err| format!("{}: {err}", temp.display()))?;
        std::fs::rename(&temp, &path).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Rebuild everything from byte 0.
    ///
    /// This is what `--replay` runs and what an index rebuild calls: the same file, read again from the
    /// start, must produce the same turns and the same commit linkage — that is what makes it safe to key a
    /// review to a turn id across a rebuild (ADR-003 I1's argument, applied to the harness side).
    ///
    /// # Errors
    ///
    /// Whatever [`Reader::read_after`] reports.
    pub fn replay(&self, now_ms: i64) -> Result<(Cursor, Batch), String> {
        let mut cursor = Cursor::fresh(&self.display_events());
        cursor.notices.push(Notice {
            kind: "replay".to_owned(),
            detail: "the stream was rebuilt from byte 0".to_owned(),
        });
        let batch = self.read_after(&mut cursor, now_ms)?;
        Ok((cursor, batch))
    }
}

// ---------------------------------------------------------------------------
// Commit ↔ turn linkage
// ---------------------------------------------------------------------------

/// How a commit learned which turn it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionBy {
    /// The event stream: the turn's time window contains the commit and its paths intersect.
    Event,
    /// The turn window contained the commit but the turn recorded no paths, so time is the whole claim.
    TimeOnly,
    /// A trailer on the commit named it (the writer's own statement).
    Trailer,
    /// Nothing said. Segmentation falls through to the git heuristics (ADR-003 §9.2, row 1).
    Nothing,
}

impl AttributionBy {
    /// The stable spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            AttributionBy::Event => "event",
            AttributionBy::TimeOnly => "time-only",
            AttributionBy::Trailer => "trailer",
            AttributionBy::Nothing => "none",
        }
    }

    /// What the attribution is worth, priced the way ADR-003 §9.3 prices its provenance: a statement
    /// (trailer) is worth more than an inference (window), and an inference that had no paths to match is
    /// worth least.
    #[must_use]
    pub const fn confidence(self) -> f64 {
        match self {
            AttributionBy::Event => 1.0,
            AttributionBy::Trailer => 0.9,
            AttributionBy::TimeOnly => 0.6,
            AttributionBy::Nothing => 0.0,
        }
    }
}

/// One commit's turn.
#[derive(Debug, Clone)]
pub struct Link {
    /// The commit.
    pub oid: String,
    /// The turn it belongs to, when something said.
    pub turn_id: Option<String>,
    /// The session that owns the turn.
    pub session_id: Option<String>,
    /// How the link was established.
    pub by: AttributionBy,
    /// Two sources disagreed, and the event won (ADR-003 §9.2's last row).
    pub conflict: bool,
}

/// The whole mapping, plus the digest that makes "the same linkage" a checkable phrase.
#[derive(Debug, Clone)]
pub struct Linkage {
    /// One link per commit, in the input order.
    pub links: Vec<Link>,
    /// sha256 over the linkage ([`LINKAGE_SCHEMA`]).
    pub digest: String,
    /// Links established from events.
    pub by_event: u64,
    /// Links established from time alone.
    pub by_time: u64,
    /// Links established from trailers.
    pub by_trailer: u64,
    /// Commits nothing mapped.
    pub unmapped: u64,
    /// Commits where a trailer and an event disagreed.
    pub conflicts: u64,
}

impl Linkage {
    /// The link for one commit.
    #[must_use]
    pub fn link(&self, oid: &str) -> Option<&Link> {
        self.links.iter().find(|link| link.oid == oid)
    }

    /// The report form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "schema": LINKAGE_SCHEMA,
            "digest": self.digest,
            "commits": self.links.len(),
            "by_event": self.by_event,
            "by_time_only": self.by_time,
            "by_trailer": self.by_trailer,
            "unmapped": self.unmapped,
            "conflicts": self.conflicts,
            "all_qualified": self.links.iter().all(|link| {
                link.turn_id
                    .as_deref()
                    .is_none_or(|id| parse_turn_id(id).is_some())
            }),
        })
    }
}

/// Map commits onto turns.
///
/// The rule, in the order it is applied per commit:
///
/// 1. **event evidence**: the commit's author date falls inside a turn's window *and* the commit touches a
///    path that turn touched. Where several turns qualify, the one that started last wins — that is the
///    turn that was actually in progress — and a tie is broken by `seq`, so the answer is a function of the
///    input and nothing else.
/// 2. **time alone**: the window contains the commit but the turn recorded no paths. Reported with a lower
///    confidence rather than hidden, because "we only knew when" changes how much a reviewer trusts the
///    grouping.
/// 3. **trailer**: the commit already carries a turn id, which is the writer's own statement.
///
/// When an event and a trailer disagree the event wins (ADR-003 §9.2's last row) and the disagreement is
/// counted, never resolved silently.
#[must_use]
pub fn attribute(commits: &[CommitFacts], ledger: &Ledger) -> Linkage {
    let mut links: Vec<Link> = Vec::with_capacity(commits.len());
    let (mut by_event, mut by_time, mut by_trailer, mut unmapped, mut conflicts) = (0, 0, 0, 0, 0);
    for commit in commits {
        let window = ledger
            .turns
            .iter()
            .filter_map(|turn| Some((turn, turn.window_millis()?)))
            .filter(|(turn, (start, end))| {
                *start <= commit.author_utc && commit.author_utc <= *end && turn.turn_index > 0
            })
            .collect::<Vec<_>>();
        let path_match = window
            .iter()
            .filter(|(turn, _)| {
                !turn.paths.is_empty()
                    && commit
                        .paths
                        .iter()
                        .any(|path| turn.paths.contains(path.as_str()))
            })
            .map(|(turn, _)| *turn)
            .max_by_key(|turn| (turn.started_seq, turn.turn_id.as_str()));
        let time_only = path_match.is_none().then(|| {
            window
                .iter()
                .filter(|(turn, _)| turn.paths.is_empty())
                .map(|(turn, _)| *turn)
                .max_by_key(|turn| (turn.started_seq, turn.turn_id.as_str()))
        });
        let trailer = commit
            .trailers
            .get(TRAILER_TURN_KEY)
            .cloned()
            .or_else(|| commit.turn_id.clone());
        let (turn_id, session_id, by, conflict) = match (path_match, time_only.flatten()) {
            (Some(turn), _) => {
                let conflict = trailer.is_some_and(|value| value != turn.turn_id);
                (
                    Some(turn.turn_id.clone()),
                    Some(turn.session_id.clone()),
                    AttributionBy::Event,
                    conflict,
                )
            }
            (None, Some(turn)) => {
                let conflict = trailer.is_some_and(|value| value != turn.turn_id);
                (
                    Some(turn.turn_id.clone()),
                    Some(turn.session_id.clone()),
                    AttributionBy::TimeOnly,
                    conflict,
                )
            }
            (None, None) => match trailer {
                Some(value) => {
                    let session = commit
                        .trailers
                        .get(TRAILER_SESSION_KEY)
                        .cloned()
                        .or_else(|| parse_turn_id(&value).map(|(session, _)| session))
                        .or_else(|| commit.session_id.clone());
                    (Some(value), session, AttributionBy::Trailer, false)
                }
                None => (
                    commit.turn_id.clone(),
                    commit.session_id.clone(),
                    if commit.turn_id.is_some() {
                        AttributionBy::Event
                    } else {
                        AttributionBy::Nothing
                    },
                    false,
                ),
            },
        };
        match by {
            AttributionBy::Event => by_event += 1,
            AttributionBy::TimeOnly => by_time += 1,
            AttributionBy::Trailer => by_trailer += 1,
            AttributionBy::Nothing => unmapped += 1,
        }
        if conflict {
            conflicts += 1;
        }
        links.push(Link {
            oid: commit.oid.clone(),
            turn_id,
            session_id,
            by,
            conflict,
        });
    }
    let digest = linkage_digest(&links);
    Linkage {
        links,
        digest,
        by_event,
        by_time,
        by_trailer,
        unmapped,
        conflicts,
    }
}

/// The digest that says "this is the same linkage": one line per commit, sorted by oid, so the answer does
/// not depend on the order the commits were walked in.
#[must_use]
pub fn linkage_digest(links: &[Link]) -> String {
    let mut lines: Vec<String> = links
        .iter()
        .map(|link| {
            format!(
                "{}\t{}\t{}",
                link.oid,
                link.turn_id.clone().unwrap_or_default(),
                link.by.as_str()
            )
        })
        .collect();
    lines.sort_unstable();
    let mut body = format!("{LINKAGE_SCHEMA}\n");
    body.push_str(&lines.join("\n"));
    digest_bytes(body.as_bytes())
}

/// Write a linkage back onto the commit facts, so [`crate::segment::segment`] sees harness turn ids where
/// the stream had evidence and nothing where it did not (ADR-003 §9.2: absence of events is absence of
/// evidence, never a guess).
///
/// Returns how many commits gained a turn id.
pub fn fill_turn_ids(commits: &mut [CommitFacts], linkage: &Linkage) -> usize {
    let mut filled = 0;
    for commit in commits {
        let Some(link) = linkage.link(&commit.oid) else {
            continue;
        };
        if commit.turn_id.is_none()
            && let Some(turn_id) = &link.turn_id
        {
            commit.turn_id = Some(turn_id.clone());
            filled += 1;
        }
        if commit.session_id.is_none() {
            commit.session_id = link.session_id.clone();
        }
    }
    filled
}

// ---------------------------------------------------------------------------
// Health: what `fathom hooks --check` answers
// ---------------------------------------------------------------------------

/// The stream's health, measured from a cursor and the file as it stands right now.
///
/// Measuring is read-only: a check must never move the offset, or reading the dashboard would consume the
/// queue.
#[derive(Debug, Clone)]
pub struct Health {
    /// The events file, shown relative to the repository where it is inside one.
    pub events_file: String,
    /// The cursor file.
    pub cursor_file: String,
    /// The quarantine file.
    pub quarantine_file: String,
    /// Whether the events file exists.
    pub exists: bool,
    /// The file's size.
    pub size_bytes: u64,
    /// The durable offset.
    pub offset: u64,
    /// Bytes the reader has not consumed.
    pub lag_bytes: u64,
    /// Complete lines the reader has not consumed.
    pub lag_lines: u64,
    /// Bytes sitting after the last newline: a writer mid-append.
    pub torn_bytes: u64,
    /// Lines counted as damage (blank, undecodable, not-an-object) so far.
    pub bad_lines: u64,
    /// The highest `seq` consumed.
    pub last_seq_read: u64,
    /// The highest `seq` in the file, consumed or not.
    pub last_seq_in_file: u64,
    /// Records lost to sequence gaps.
    pub sequence_gap_records: u64,
    /// The last event's timestamp, verbatim.
    pub last_event_ts: Option<String>,
    /// How old that is, in whole seconds, or `None` when it cannot be computed (no clock read here).
    pub age_seconds: Option<u64>,
    /// How many read passes produced the cursor.
    pub reads: u64,
    /// How many rotations the cursor survived.
    pub rotations: u64,
    /// The counters, verbatim.
    pub counters: Counters,
    /// Turns, sessions and the state of both.
    pub ledger: Ledger,
    /// [`Cursor::ledger_digest`] at the moment the report was built, so a report alone can show that two
    /// reads of the same file agreed.
    pub ledger_digest: String,
    /// How many turns the ledger no longer carries (bounded state).
    pub trimmed: u64,
    /// The quarantine's size in records.
    pub quarantine_records: usize,
    /// What a throwaway read of the unread region would produce, when the caller did one (see [`Probe`]).
    pub probe: Option<Probe>,
    /// Notices the cursor carries.
    pub notices: Vec<Notice>,
    /// Whether the stream is healthy, with the reasons either way.
    pub ok: bool,
    /// What makes it not healthy.
    pub reasons: Vec<String>,
    /// What is unusual but not unhealthy.
    pub warnings: Vec<String>,
    /// What this report cannot say.
    pub unknowns: Vec<String>,
}

impl Health {
    /// Measure the stream as `cursor` last saw it.
    ///
    /// `now_ms` is the caller's clock (ADR-002 §4.4); `root`, when given, is what paths are printed
    /// relative to, so a machine-readable report does not name someone's home directory.
    ///
    /// # Errors
    ///
    /// Whatever scanning the unread tail costs when the file cannot be read.
    pub fn measure(
        reader: &Reader,
        cursor: &Cursor,
        now_ms: i64,
        root: Option<&Path>,
    ) -> Result<Health, String> {
        let mut lag_lines = 0u64;
        let mut last_seq_in_file: Option<u64> = None;
        let mut lag_bytes = 0u64;
        let mut torn_bytes = 0u64;
        if reader.exists() {
            let scan = reader.scan(cursor.offset, |scanned| {
                lag_lines += 1;
                if let Ok(value) = serde_json::from_slice::<Value>(match scanned {
                    Scanned::Complete { bytes, .. } => bytes,
                    Scanned::Oversized { .. } => &[],
                }) && let Some(seq) = value.get("seq").and_then(Value::as_u64)
                {
                    last_seq_in_file =
                        Some(last_seq_in_file.map_or(seq, |best: u64| best.max(seq)));
                }
            })?;
            torn_bytes = scan.torn_tail.map_or(0, |(_offset, bytes)| bytes);
            lag_bytes = scan.size_bytes.saturating_sub(cursor.offset);
        }
        let age_seconds = cursor
            .last_ts
            .as_deref()
            .and_then(parse_timestamp_millis)
            .and_then(|then| now_ms.checked_sub(then))
            .and_then(|delta| u64::try_from(delta).ok())
            .map(|millis| millis / 1000);
        let counters = cursor.counters.clone();
        let mut reasons: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        if counters.sequence_gaps > 0 {
            reasons.push(format!(
                "{} record(s) are missing from the sequence: {} gap(s) — ADR-005 §3.3 makes a gap a hard \
                 error, not something to scroll past",
                counters.sequence_gap_records, counters.sequence_gaps
            ));
        }
        if counters.sequence_duplicates > 0 {
            reasons.push(format!(
                "{} record(s) reused a `seq`: two writers allocated the same number, so one of them did \
                 not take the append lock",
                counters.sequence_duplicates
            ));
        }
        if counters.oversized > 0 {
            reasons.push(format!(
                "{} line(s) exceeded the {} byte cap and were refused whole",
                counters.oversized, reader.limits.max_line_bytes
            ));
        }
        let escaped = *counters
            .rejections
            .get(RejectReason::PathEscape.as_str())
            .unwrap_or(&0);
        if escaped > 0 {
            reasons.push(format!(
                "{} record(s) named a path outside the session directory and were refused (ADR-005 §6 rule 1)",
                escaped
            ));
        }
        let versions = *counters
            .rejections
            .get(RejectReason::SchemaVersion.as_str())
            .unwrap_or(&0);
        if versions > 0 {
            reasons.push(format!(
                "{} record(s) did not declare event schema {EVENT_SCHEMA}: a session may not mix schema \
                 majors (ADR-005 §5.1)",
                versions
            ));
        }
        let quarantined = cursor.ledger.quarantined_sessions();
        if !quarantined.is_empty() {
            reasons.push(format!(
                "{} session(s) are quarantined ({}): their events are skipped until an operator clears \
                 them",
                quarantined.len(),
                quarantined.join(", ")
            ));
        }
        if counters.blank_lines > 0 || counters.rejected_total() > 0 {
            warnings.push(format!(
                "{} line(s) were refused in total; the quarantine file names every one with its offset",
                counters.rejected_total()
            ));
        }
        if torn_bytes > 0 {
            warnings.push(format!(
                "{} byte(s) sit after the last newline: a writer is mid-append, and they are not counted \
                 as anything until the newline lands",
                torn_bytes
            ));
        }
        if !reader.exists() {
            warnings.push(
                "there is no events file yet: every count below is by absence, which is an absence of \
                 evidence and not a failure (ADR-003 §9.2)"
                    .to_owned(),
            );
        }
        if cursor.ledger.inferred > 0 {
            warnings.push(format!(
                "{} turn(s) were inferred from turn-scoped events with no turn_start: the adapter never \
                 said where a turn began",
                cursor.ledger.inferred
            ));
        }
        let mut unknowns = vec![
            // Written for a human reading a terminal: no intra-doc links, no folded newline leaving a
            // run of spaces behind, and no claim this report cannot support.
            concat!(
                "which commits belong to which turn is not measured here: that needs git in hand, so the ",
                "index pass is what maps commits onto turns. What this report can prove is that the turns ",
                "themselves rebuild identically, which is what `turns.digest` is."
            )
            .to_owned(),
        ];
        if cursor.ledger.trimmed > 0 {
            unknowns.push(format!(
                "the cursor carries the last {} turn(s) only ({} trimmed): attribution of much older \
                 commits is not possible from the cursor alone",
                reader.limits.turns_kept, cursor.ledger.trimmed
            ));
        }
        Ok(Health {
            events_file: show(&reader.events, root),
            cursor_file: show(&reader.cursor_path(), root),
            quarantine_file: show(&reader.quarantine_path(), root),
            exists: reader.exists(),
            size_bytes: if reader.exists() { reader.size() } else { 0 },
            offset: cursor.offset,
            lag_bytes,
            lag_lines,
            torn_bytes,
            bad_lines: counters.bad_lines,
            last_seq_read: cursor.last_seq,
            last_seq_in_file: last_seq_in_file.unwrap_or(cursor.last_seq),
            sequence_gap_records: counters.sequence_gap_records,
            last_event_ts: cursor.last_ts.clone(),
            age_seconds,
            reads: cursor.reads,
            rotations: cursor.rotations,
            counters,
            ledger: cursor.ledger.clone(),
            ledger_digest: cursor.ledger_digest(),
            trimmed: cursor.ledger.trimmed,
            probe: None,
            quarantine_records: Quarantine::new(reader.quarantine_path(), 1).records(),
            notices: cursor.notices.clone(),
            ok: reasons.is_empty(),
            reasons,
            warnings,
            unknowns,
        })
    }

    /// Fold a [`Probe`] into the report, and say so in `reasons` when the unread region is damaged.
    ///
    /// Damage that has not been read yet is still damage: a `--check` that returned `ok: true` while three
    /// lines ahead of the offset were about to be quarantined would be a health report that lies about the
    /// one thing it was asked about. The offset is *not* advanced by the probe — that is the difference
    /// between reporting and consuming, and it is why `fathom hooks` can run from a cron job or a bug-report
    /// script without stealing work from the watcher.
    pub fn note_probe(&mut self, probe: Probe) {
        let refused = probe.refused_total();
        if refused > 0 {
            let mut detail = probe
                .refusals
                .iter()
                .map(|(reason, count)| format!("{reason} {count}"))
                .collect::<Vec<_>>()
                .join(", ");
            if detail.is_empty() {
                detail = "see the quarantine".to_owned();
            }
            self.reasons.push(format!(
                "{refused} line(s) ahead of the offset would be refused: {detail}"
            ));
            self.ok = false;
        }
        if probe.lines > 0 && self.last_seq_in_file < self.last_seq_read {
            self.reasons.push(format!(
                "the file no longer contains the sequence the cursor stopped at (file at {}, cursor at {}):                  it was truncated or rotated under us",
                self.last_seq_in_file, self.last_seq_read
            ));
            self.ok = false;
        }
        self.probe = Some(probe);
    }

    /// The `--json` payload.
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "exists": self.exists,
            "events_file": self.events_file,
            "cursor_file": self.cursor_file,
            "quarantine_file": self.quarantine_file,
            "size_bytes": self.size_bytes,
            "offset": self.offset,
            "lag_bytes": self.lag_bytes,
            "lag_lines": self.lag_lines,
            "torn_bytes": self.torn_bytes,
            "bad_lines": self.bad_lines,
            "last_seq_read": self.last_seq_read,
            "last_seq_in_file": self.last_seq_in_file,
            "sequence_gap_records": self.sequence_gap_records,
            "last_event_ts": self.last_event_ts,
            "age_seconds": self.age_seconds,
            "reads": self.reads,
            "rotations": self.rotations,
            "counts": self.counters.to_json(),
            "by_event": &self.counters.by_event,
            "by_harness": &self.counters.by_harness,
            "rejections": &self.counters.rejections,
            "unprojectable": {
                "total": self.counters.unprojectable_total(),
                "by_source": &self.counters.unprojectable,
            },
            "turns": {
                "opened": self.ledger.opened,
                "carried": self.ledger.turns.len(),
                "inferred": self.ledger.inferred,
                "closed_by_next": self.ledger.closed_by_next,
                "open": self.ledger.open_turns(),
                "trimmed": self.trimmed,
                "digest": self.ledger_digest,
                "all_session_qualified": self.ledger.turns.iter().all(|turn| {
                    parse_turn_id(&turn.turn_id)
                        .is_some_and(|(session, index)| session == turn.session_id && index == turn.turn_index)
                }),
            },
            "sessions": {
                "count": self.ledger.sessions.len(),
                "quarantined": self.ledger.quarantined_sessions(),
                "detail": self
                    .ledger
                    .sessions
                    .iter()
                    .map(|(session, state)| {
                        serde_json::json!({"session_id": session, "events": state.events, "bytes": state.bytes,
                            "turns": state.next_turn_index, "quarantined": state.quarantined})
                    })
                    .collect::<Vec<_>>(),
            },
            "quarantine": { "file": self.quarantine_file, "records": self.quarantine_records },
            "probe": self.probe.as_ref().map(Probe::to_json),
            "notices": self.notices.iter().map(Notice::to_json).collect::<Vec<_>>(),
            "ok": self.ok,
            "reasons": self.reasons,
            "warnings": self.warnings,
            "unknowns": self.unknowns,
        })
    }

    /// The lines `fathom hooks --check` prints when it is not asked for JSON.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!(
            "{} — offset {} of {} bytes ({} byte{}, {} line{} behind)",
            self.events_file,
            self.offset,
            self.size_bytes,
            self.lag_bytes,
            if self.lag_bytes == 1 { "" } else { "s" },
            self.lag_lines,
            if self.lag_lines == 1 { "" } else { "s" },
        )];
        out.push(format!(
            "  events read: {}   last seq {} (file at {})   types: {}",
            self.counters.accepted,
            self.last_seq_read,
            self.last_seq_in_file,
            if self.counters.by_event.is_empty() {
                "none yet".to_owned()
            } else {
                self.counters
                    .by_event
                    .iter()
                    .map(|(kind, count)| format!("{kind} {count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        out.push(format!(
            "  turns: {} opened, {} open, {} inferred, {} closed by the next turn — all session-qualified",
            self.ledger.opened,
            self.ledger.open_turns(),
            self.ledger.inferred,
            self.ledger.closed_by_next
        ));
        if !self.counters.rejections.is_empty() {
            out.push(format!(
                "  rejections: {}",
                self.counters
                    .rejections
                    .iter()
                    .map(|(reason, count)| format!("{reason} {count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            out.push("  rejections: none".to_owned());
        }
        if self.counters.bad_lines > 0 || self.quarantine_records > 0 {
            out.push(format!(
                "  bad lines: {}   quarantined records: {} in {}",
                self.bad_lines, self.quarantine_records, self.quarantine_file
            ));
        }
        if let Some(probe) = &self.probe {
            out.push(format!(
                "  unread ahead: {} event(s) of {} line(s), {} byte(s); {} would be refused; {} turn(s) \
                 opened, {} still open (a check reads nothing: run `fathom hooks --tail` to advance)",
                probe.events,
                probe.lines,
                probe.bytes,
                probe.refused_total(),
                probe.turns_opened,
                probe.turns_open
            ));
        }
        for reason in &self.reasons {
            out.push(format!("  NOT OK: {reason}"));
        }
        for warning in &self.warnings {
            out.push(format!("  note: {warning}"));
        }
        out
    }
}

/// Print a path relative to the repository when it is inside one.
#[must_use]
fn show(path: &Path, root: Option<&Path>) -> String {
    if let Some(root) = root
        && let Ok(relative) = path.strip_prefix(root)
    {
        return relative.display().to_string();
    }
    path.display().to_string()
}

/// A JSON array of strings as a `Vec<String>`; anything else is the empty list.
///
/// Used by the cursor load path for the `e4.2` lists. A field of the wrong shape is no data rather than a
/// failed restart — the same rule [`Turn::from_json`] applies to every field it reads, because a cursor
/// written by a newer fathom must not make this one refuse to start (ADR-004 §9.5).
fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

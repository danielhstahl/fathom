//! The door the tool surface reads through: a trait, not a store handle (ADR-002 §5.2, §5.3).
//!
//! [`Source`] is the whole of what the server may ask the world for, and it is deliberately not
//! `&Store`. Two rules make that shape necessary rather than stylistic:
//!
//! * **One reality** (ADR-001 §7.10, lint `M02`/`M03`). The answers a tool returns have to be the answers
//!   the TUI draws, which means they come from `fathom_analyze::insight` and friends — and a crate at this
//!   layer may not open a store, may not run an analysis, and may not name the crate that does. So the
//!   composition root reads, composes and ranks, and hands the finished document across this trait.
//! * **No IO below the surfaces** (lint `W1`). `fathom-mcp` may not name `std::fs`, so it cannot open the
//!   store *or* write an audit line. The read side is this trait; the write side is [`Source::audit`] and
//!   the two write methods, all of which the composition root implements against real files.
//!
//! The [`Args`] a method receives have already been validated by the dispatcher: required keys are present,
//! unknown keys have been refused, and the window has been clamped. A method may therefore be a
//! straight-line read rather than a parser, and the only failures it can return are the ones the caller
//! needs told about ([`Failure`], ADR-005 §4.1).

use serde_json::Value;

/// A clamped request window: the first index to return and how many to return.
///
/// Built by the dispatcher from the tool's own bound ([`crate::tools::ToolSpec::default_items`] and
/// [`crate::tools::ToolSpec::max_items`]), never from the caller directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// The index of the first item the source should return.
    pub offset: usize,
    /// How many items the source should return, at most.
    pub limit: usize,
}

impl Window {
    /// One past the last index in the window, saturating so a hostile offset cannot wrap to zero.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.offset.saturating_add(self.limit)
    }
}

/// One page of a tool's answer: the items, the denominator, and what the read could not answer.
///
/// `total` is the denominator of the *whole* answer, not of the page: "3 of 5 000" and "there are only 3"
/// have to be distinguishable by a reader who cannot see the repository (ADR-004 §7). A source that cannot
/// count the whole sets `total` to what it returned and says so in [`Batch::unknowns`].
#[derive(Debug, Clone, Default)]
pub struct Batch {
    /// The page's rows, each a JSON object in the tool's own published shape.
    pub items: Vec<Value>,
    /// How many rows exist under the same query, whether or not this page holds them.
    pub total: usize,
    /// Scalar fields to merge into the envelope: endpoints, model identity, the counts that are not lists.
    pub fields: Vec<(String, Value)>,
    /// Which signals were missing, whether the change was ever reviewed, whether the corpus is old
    /// (ADR-005 §4.1: "confidence 0.9 with empty `unknowns`" must be impossible).
    pub unknowns: Vec<String>,
}

impl Batch {
    /// An empty page, to push onto.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one row.
    #[must_use]
    pub fn item(mut self, item: Value) -> Self {
        self.items.push(item);
        self
    }

    /// The page's rows, all at once.
    #[must_use]
    pub fn items(mut self, items: Vec<Value>) -> Self {
        self.items = items;
        self
    }

    /// The number the reader divides by.
    #[must_use]
    pub fn total(mut self, total: usize) -> Self {
        self.total = total;
        self
    }

    /// Merge one scalar field into the envelope.
    #[must_use]
    pub fn field(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.fields.push((key.to_owned(), value.into()));
        self
    }

    /// Record one gap, in the reader's words rather than the code's.
    #[must_use]
    pub fn unknown(mut self, note: impl Into<String>) -> Self {
        self.unknowns.push(note.into());
        self
    }

    /// A batch whose items came from a document rather than a collection: the document's fields are merged
    /// wholesale and the item list is the document's own paginated collection, if it has one.
    #[must_use]
    pub fn document(fields: Vec<(String, Value)>) -> Self {
        Self {
            fields,
            ..Self::default()
        }
    }
}

/// What an argument is, as the tool sees it: already a JSON value, already checked for presence and
/// spelling.
///
/// The accessors return `Option` rather than panicking because the dispatcher's checks are about the keys a
/// tool declares; a tool that reaches for a key it did not declare gets `None` and says so, rather than
/// ending the agent's turn (ADR-005 §4.1: "nothing throws at the caller").
#[derive(Debug, Clone, Default)]
pub struct Args(pub Vec<(String, Value)>);

impl Args {
    /// The value under `key`, if it was sent.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(name, _)| name == key).map(|(_, v)| v)
    }

    /// A string argument, trimmed of surrounding space; `None` if absent or not a string.
    #[must_use]
    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str).map(str::trim)
    }

    /// An integer argument; `None` if absent, not an integer, or negative.
    #[must_use]
    pub fn usize(&self, key: &str) -> Option<usize> {
        self.get(key).and_then(|value| match value {
            Value::Number(number) => number.as_i64().and_then(|n| usize::try_from(n).ok()),
            _ => None,
        })
    }

    /// A boolean argument; `None` if absent or not a boolean.
    #[must_use]
    pub fn flag(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Value::as_bool)
    }

    /// The keys that were sent, in the order they arrived — for an audit line.
    #[must_use]
    pub fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(name, _)| name.as_str()).collect()
    }
}

/// A refusal, in the vocabulary ADR-005 §4.1 publishes.
///
/// The codes are a closed set, and a tool that raises ends the agent's turn while a tool that returns one
/// of these lets the agent decide what to do next. `Internal` is the only code that means "the server
/// failed"; every other one means the caller can change something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The tool is not in the session's tool list.
    UnknownTool(String),
    /// A required argument was missing, or an unexpected one arrived, or a value has the wrong shape.
    BadArgs(String),
    /// An argument was well-formed and out of the bounds this build will answer (a `k` above the cap).
    OutOfBounds(String),
    /// The session was not granted the capability the tool needs (ADR-005 §4.3): the write half, or an
    /// approval token the caller does not hold.
    NoCapability(String),
    /// The question is answerable but the model behind it is not there (no risk model, no index).
    NoModel(String),
    /// The thing named exists but cannot be reviewed or annotated (provisional, empty, already closed).
    NotReviewable(String),
    /// The server itself failed, with the sentence the operator needs and no more.
    Internal(String),
}

impl Failure {
    /// The wire code, one of ADR-005 §4.1's six.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownTool(_) => "unknown_tool",
            Self::NoCapability(_) => "no_capability",
            Self::BadArgs(_) => "bad_args",
            Self::OutOfBounds(_) => "out_of_bounds",
            Self::NoModel(_) => "no_model",
            Self::NotReviewable(_) => "not_reviewable",
            Self::Internal(_) => "internal",
        }
    }

    /// What the caller can do about it (§4.1's `remedy`), phrased as an action.
    ///
    /// A refusal without a remedy is a dead end, and an agent at the end of a dead end tends to invent a
    /// door. Every code except `internal` has something to try.
    #[must_use]
    pub fn remedy(&self) -> String {
        match self {
            Self::UnknownTool(_) => "call `tools/list` and use a name from it".to_owned(),
            Self::BadArgs(_) => "re-read the tool's `inputSchema` and send only its own arguments".to_owned(),
            Self::OutOfBounds(_) => {
                "ask for fewer rows, or page with the `next_cursor` the truncated answer returned"
                    .to_owned()
            }
            Self::NoCapability(_) => "an operator sets `mcp.allow_writes = true`, and `mark_reviewed`                                       additionally needs a token minted by `fathom mcp approval-token`"
                .to_owned(),
            Self::NoModel(_) => "run `fathom index`, or name the endpoints the index can resolve".to_owned(),
            Self::NotReviewable(_) => {
                "pick a change the queue offers; `fathom episodes` lists them".to_owned()
            }
            Self::Internal(_) => "this is a Fathom bug: `fathom doctor` prints the report to file".to_owned(),
        }
    }

    /// The sentence to return. Redaction runs on it like on anything else on the way out.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::UnknownTool(name) => name,
            Self::BadArgs(why)
            | Self::NoCapability(why)
            | Self::OutOfBounds(why)
            | Self::NoModel(why)
            | Self::NotReviewable(why)
            | Self::Internal(why) => why,
        }
    }
}

/// One write the server was asked to make, already digested.
///
/// The server formats this and hands it to [`Source::audit`]; it carries no argument bodies, only a digest
/// of them (`e6.4`'s audit line is about *which* call happened, not about copying the note text
/// twice).
#[derive(Debug, Clone)]
pub struct AuditLine {
    /// The tool that was called.
    pub tool: String,
    /// The capability the call needed: `read` or `write`.
    pub capability: &'static str,
    /// `sha256:` over the canonical JSON of the arguments, so a later reader can prove which call this was
    /// without this line holding the arguments.
    pub args_digest: String,
    /// `ok`, or the refusal's code.
    pub outcome: String,
    /// How long the call took.
    pub cost_ms: u128,
    /// How many bytes the answer was.
    pub bytes_returned: usize,
    /// The identity the write was made under, when the call named one.
    pub reviewer: Option<String>,
}

/// Everything the server may ask of the process that started it.
///
/// Implemented by `fathom-cli`, which owns the store handle, the clock and the files. Each read method
/// answers for one tool and returns a [`Batch`]; each write method answers for one gated tool and returns
/// the object the envelope reports under `result`.
///
/// # Errors
///
/// Each method returns [`Failure`], and only the codes its tool can honestly produce. The dispatcher turns
/// one into a structured refusal; it never turns one into a panic or a JSON-RPC error.
#[allow(unused_variables)]
pub trait Source: Send + Sync + 'static {
    /// `explain_episode` — one episode as the versioned delta document.
    fn explain_episode(&self, args: &Args, window: Window) -> Result<Batch, Failure>;

    /// `changes_between` — what one range did, as change rows with evidence ids.
    fn changes_between(&self, args: &Args, window: Window) -> Result<Batch, Failure>;

    /// `architecture_delta` — what a range did to the graph: packages, cycles, boundary crossings.
    fn architecture_delta(&self, args: &Args, window: Window) -> Result<Batch, Failure>;

    /// `dependency_delta` — what a range did to the declared and locked dependencies.
    fn dependency_delta(&self, args: &Args, window: Window) -> Result<Batch, Failure>;

    /// `risk_report` — the ranked changes with their factors and the model's own identity.
    fn risk_report(&self, args: &Args, window: Window) -> Result<Batch, Failure>;

    /// `search_history` — pointer-shaped hits over the indexed history, never content.
    fn search_history(&self, args: &Args, window: Window) -> Result<Batch, Failure>;

    /// `mark_reviewed` — record that a named human accepted a change.
    ///
    /// The `Batch` here is a document, not a page: the ledger entry's identity, whether it was already
    /// there, and what could not be settled go in [`Batch::fields`].
    fn mark_reviewed(&self, args: &Args) -> Result<Batch, Failure>;

    /// `add_review_note` — attach a note to a known target.
    fn add_review_note(&self, args: &Args) -> Result<Batch, Failure>;

    /// Append one audit line (`e6.4`: every gated call is logged, refusals included).
    ///
    /// The server calls this for **every** gated call, including the ones it refused, and never fails a
    /// tool call because the log could not be written — it reports the failure in the response's
    /// `unknowns` instead, so a read-only `.fathom/` degrades the audit rather than the answer.
    fn audit(&self, line: &AuditLine) -> Result<(), String>;
}

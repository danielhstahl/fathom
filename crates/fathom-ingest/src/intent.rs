//! `intent.rs` — why this episode exists: the prompt, the plan and the declared scope
//! (`e4.2`, `reco-c54`).
//!
//! Attribution (`e4.1`, [`crate::attribute`]) answers *who*. This answers *what for*, and the two are
//! deliberately not interchangeable: [`schemas/fathom.schema.json`]'s `agent_counts_as_intent` says so
//! already ("who executed the change is not who asked for it"), and the reason it says so is that a
//! detector which conflates them can never report the thing worth reporting — an agent doing something
//! nobody asked for.
//!
//! # The ladder
//!
//! One shape as the boundary cascade ([`crate::segment`]) and the attribution ladder: each rung is one
//! class of evidence, one price, and one thing it can assert; the first rung with an opinion decides the
//! text; nothing is blended; every rung that fired stays in [`Intent::evidence`] so the card can show its
//! work.
//!
//! | # | `source` | Class | Fires when | Asserts | Confidence |
//! |---|---|---|---|---|---|
//! | 1 | `hook-prompt` | stated | the harness recorded the turn's prompt **text** | prose | 0.95 |
//! | 2 | `hook-intent` | stated | the harness recorded a declared intent line | prose | 0.95 |
//! | 3 | `intent-trailer` | stated | an `Intent:` / `Agent-Intent:` trailer | prose | 0.90 |
//! | 4 | `hook-plan` | named | no prose anywhere, but the harness recorded plan items | the open plan item | 0.70 |
//! | 5 | `task-trailer` | named | a `Task:` / `Agent-Task:` trailer names the task | the task line | 0.70 |
//! | 6 | `plan-trailer` | named | a `Plan:` trailer lists plan items and nothing above fired | the first open item | 0.65 |
//! | 7 | `prompt-digest` | digest | hooks recorded a prompt and **not** its text | *a request happened* | 0.30 |
//! | — | `none` | — | nothing anywhere | nothing | 0.00 |
//!
//! Rows 1–3 are somebody *saying* what the work was for, which is why they are priced above rows 4–6,
//! where Fathom reads a name — a task id, a plan item — rather than a sentence. Row 7 is the interesting
//! one and the reason this ladder is not just "text or nothing": ADR-005 §2.1's `turn_start` row carries
//! `prompt_sha256` and `prompt_chars` and **not** the prompt, and every fixture in
//! `fathom-fixtures/corpus` is written that way on purpose (§2.1's own words: a prompt is stored as a
//! digest so that rewording a subject line cannot move a corpus digest). A digest proves a request
//! happened; it does not say what it was. That is exactly the state
//! `schemas/fathom.schema.json`'s `intent_not_captured` already describes ("its intent is not, **or only
//! a prompt digest is**"), so it gets its own [`Intent::state`] rather than being folded into either end.
//!
//! # Missing intent is a state, not an error
//!
//! The ticket's rule. Row — is a value: [`Intent::state`] is `none`, [`Intent::confidence`] is `0.0`, and
//! [`Intent::reason`] names what was looked for and not found ("no `Intent:`/`Task:` trailer and no
//! harness turn"), because "nothing was recorded" and "nothing was checked" are different claims and only
//! one of them is checkable afterwards. Nothing here guesses from the subject line, the author, or the
//! files that arrived: an intent inferred from the change it is supposed to judge is a tautology, and the
//! scope-creep detector built on it would fire on nothing ever.
//!
//! # Declared scope, and why nothing infers it
//!
//! [`Intent::scope`] holds the globs somebody *declared* — from a hook payload or a `Scope:` trailer —
//! normalised ([`normalise_scope`]) and never inferred from the files that arrived. Deriving scope from
//! the diff would make the deviation check (`Deviation` in `fathom_analyze::scope`, which reads these
//! globs) structurally unable to report the only case it exists for. An empty scope means **no declared
//! scope**, which the detector reads as "no opinion", not as "nothing was allowed";
//! `Deviation::Unknown` is that state, spelled.
//!
//! # What is deliberately not here
//!
//! * **No clock, no file, no config.** The hook evidence arrives as [`TurnEvidence`] and the knobs as
//!   [`Policy`] (ADR-002 §4.5). The reader that produces [`TurnEvidence`] is [`crate::hooks`].
//! * **No redaction.** Hook payloads are redacted at the boundary that reads them
//!   ([`crate::hooks`], ADR-005 §6 rule 4) and the text kept here is what survived that. Truncation is
//!   applied here, at [`Policy::max_text_bytes`], and is *said*: [`Intent::truncated`].
//! * **No second copy of the prompt.** The text is kept because the ticket asks for the intent text; the
//!   digest is kept alongside it because the digest is what survives a policy change that stops capturing
//!   text, and it is the evidence that a request existed at all.

use serde::{Deserialize, Serialize};

use crate::segment::CommitFacts;

/// The namespace of [`Intent::to_json`], so a stored document says which shape it is.
pub const INTENT_SCHEMA: &str = "fathom-intent/1";

/// The prompt text a harness stated. Above every named thing: it is the request itself.
pub const CONFIDENCE_PROMPT: f64 = 0.95;
/// A declared intent line — in a hook payload or in a trailer.
pub const CONFIDENCE_INTENT: f64 = 0.95;
/// An `Intent:` / `Agent-Intent:` trailer, which is a statement about one commit rather than the turn.
pub const CONFIDENCE_INTENT_TRAILER: f64 = 0.90;
/// A plan item or a task line standing in for prose.
pub const CONFIDENCE_NAMED: f64 = 0.70;
/// A `Plan:` trailer with nothing else: the weakest *stated* thing there is.
pub const CONFIDENCE_PLAN_TRAILER: f64 = 0.65;
/// A prompt digest and no text: proof a request happened, not evidence of what it said.
pub const CONFIDENCE_DIGEST: f64 = 0.30;
/// Nothing anywhere.
pub const CONFIDENCE_NONE: f64 = 0.0;

/// The trailer keys read for intent, in ladder order.
///
/// `agent-intent` is the spelling the fixture corpus already writes
/// (`fathom-fixtures/src/scenarios.rs`, and the `Agent-Intent:` lines the `scope_creep` scenario's ground
/// truth quotes), so the corpus's history is the trailer-fallback test rather than a new fixture. `task`
/// is the ticket's other spelling, and the one other teams use.
pub const INTENT_TRAILERS: [&str; 2] = ["intent", "agent-intent"];
/// See [`INTENT_TRAILERS`].
pub const TASK_TRAILERS: [&str; 2] = ["task", "agent-task"];
/// A plan list carried in a trailer, `;`- or newline-separated (git folds a continued trailer value into
/// one line, so a multi-line plan arrives as one value with the items joined).
pub const PLAN_TRAILERS: [&str; 2] = ["plan", "agent-plan"];
/// Declared scope: the path globs the work was supposed to stay inside.
pub const SCOPE_TRAILERS: [&str; 2] = ["scope", "agent-scope"];

/// Which rung decided the intent text.
///
/// The spellings are published: they are what `episode.intent.source` prints, what the store's
/// `intent_source` column holds, and — for the two that overlap it — what
/// `schemas/fathom.schema.json`'s `[deps.provenance] intent_sources` already lists (`agent-intent-trailer`
/// is that list's spelling of rung 3, `plan-item` of rungs 4 and 6). Reusing them is the point: one
/// vocabulary for "where did this intent come from", two config keys for two different questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// The turn's prompt text, from a hook event.
    HookPrompt,
    /// A declared intent line in a hook event.
    HookIntent,
    /// An `Intent:` / `Agent-Intent:` trailer.
    IntentTrailer,
    /// The open plan item of a hook event's plan list.
    HookPlan,
    /// A `Task:` / `Agent-Task:` trailer.
    TaskTrailer,
    /// A `Plan:` trailer's first open item.
    PlanTrailer,
    /// A prompt digest with no text.
    PromptDigest,
    /// Nothing was recorded.
    None,
}

impl Source {
    /// The published spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Source::HookPrompt => "hook-prompt",
            Source::HookIntent => "hook-intent",
            Source::IntentTrailer => "agent-intent-trailer",
            Source::HookPlan => "plan-item",
            Source::TaskTrailer => "task-trailer",
            Source::PlanTrailer => "plan-item",
            Source::PromptDigest => "prompt-digest",
            Source::None => "none",
        }
    }

    /// Parse a published spelling; anything else is [`Source::None`] rather than an error, because a row
    /// written by a newer fathom must not make this one refuse to read it (ADR-004 §9.5).
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "hook-prompt" => Source::HookPrompt,
            "hook-intent" => Source::HookIntent,
            "agent-intent-trailer" => Source::IntentTrailer,
            "plan-item" => Source::HookPlan,
            "task-trailer" => Source::TaskTrailer,
            "prompt-digest" => Source::PromptDigest,
            _ => Source::None,
        }
    }

    /// What this rung alone prices the verdict at.
    #[must_use]
    pub const fn confidence(self) -> f64 {
        match self {
            Source::HookPrompt => CONFIDENCE_PROMPT,
            Source::HookIntent => CONFIDENCE_INTENT,
            Source::IntentTrailer => CONFIDENCE_INTENT_TRAILER,
            Source::HookPlan => CONFIDENCE_NAMED,
            Source::TaskTrailer => CONFIDENCE_NAMED,
            Source::PlanTrailer => CONFIDENCE_PLAN_TRAILER,
            Source::PromptDigest => CONFIDENCE_DIGEST,
            Source::None => CONFIDENCE_NONE,
        }
    }

    /// Whether this rung states *what was asked* in prose, as opposed to naming it or proving it happened.
    #[must_use]
    pub const fn is_stated(self) -> bool {
        matches!(
            self,
            Source::HookPrompt | Source::HookIntent | Source::IntentTrailer
        )
    }
}

impl core::fmt::Display for Source {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether an episode has an intent, has only proof that one existed, or has nothing recorded.
///
/// Three states, not two, because the middle one is the common case in a repository whose harness follows
/// ADR-005 §2.1 and digests prompts instead of copying them. Collapsing it into `Recorded` would be the
/// product claiming it knows what was asked when it knows only that something was asked; collapsing it
/// into `Absent` would throw away real evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// There is text: a prompt, an intent line, a task, a plan item.
    Recorded,
    /// A prompt was recorded and its text was not: the digest is the whole of the evidence.
    Digest,
    /// Nothing was recorded, anywhere. The normal state of a repository with no hooks and no trailers.
    Absent,
}

impl State {
    /// The published spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            State::Recorded => "recorded",
            State::Digest => "digest-only",
            State::Absent => "none",
        }
    }

    /// Parse a published spelling; anything else is [`State::Absent`].
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "recorded" => State::Recorded,
            "digest-only" => State::Digest,
            _ => State::Absent,
        }
    }
}

impl core::fmt::Display for State {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the harness said about one turn, before this module decides what it means.
///
/// Built by [`crate::hooks`] out of the events stream (see `hooks::Turn::intent`) and handed to
/// [`capture`] as data. Every field is optional because every field comes from a vendor that may or may
/// not send it: ADR-005 §5.1's rule is that unknown payload fields are preserved and missing ones are
/// normal, and an adapter for a harness that states nothing is a working adapter, not a broken one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnEvidence {
    /// The session the turn belongs to.
    pub session_id: String,
    /// The session-qualified turn id (`ADR-005 §3.3`).
    pub turn_id: String,
    /// The prompt **text**, when the harness sent it.
    pub prompt: Option<String>,
    /// `prompt_sha256`, as sent or as computed over the redacted payload.
    pub prompt_sha256: Option<String>,
    /// `prompt_chars`.
    pub prompt_chars: Option<u64>,
    /// A declared intent line, distinct from the prompt: some harnesses summarise rather than echo.
    pub intent: Option<String>,
    /// The plan items the harness had for the turn.
    pub plan: Vec<PlanItem>,
    /// The declared scope globs.
    pub scope: Vec<String>,
}

/// One plan item, as the harness or a trailer stated it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    /// The item's text, already truncated to [`Policy::max_text_bytes`].
    pub text: String,
    /// Its state in the harness's own words (`pending`, `in_progress`, `completed`, …), when it had any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl PlanItem {
    /// An item with no declared status.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        PlanItem {
            text: text.into(),
            status: None,
        }
    }

    /// Whether the harness says this item is finished.
    ///
    /// Only an explicit done-word counts: an item with no status is *open*, because an unclaimed
    /// "still to do" is the honest reading of a bare plan line, and the fallback intent text is picked
    /// from the open items.
    #[must_use]
    pub fn is_done(&self) -> bool {
        matches!(
            self.status
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "done" | "complete" | "completed" | "closed" | "skipped" | "cancelled" | "canceled"
        )
    }
}

/// The prompt's digests, kept whether or not the text was captured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptRecord {
    /// `prompt_sha256`, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// `prompt_chars`, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chars: Option<u64>,
    /// Whether the text itself was captured. `false` is ADR-005 §2.1's default and every fixture's shape.
    pub text_captured: bool,
}

/// One line of the evidence list: what fired, in whose words, priced on its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// The rung that fired, in [`Source`]'s spelling.
    pub source: String,
    /// The literal, in the source's own spelling, truncated like everything else here.
    pub detail: String,
    /// What this item alone would have priced the intent at.
    pub confidence: f64,
}

/// The intent of one episode, as [`capture`] decided it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    /// The document's namespace.
    #[serde(default = "schema_default")]
    pub schema: String,
    /// Recorded / digest-only / none.
    pub state: StateText,
    /// The intent text — the answer to "what was this for". Absent when [`Intent::state`] is not
    /// `recorded`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The rung that decided.
    pub source: String,
    /// Confidence in [`Intent::text`]; `0.0` is "nothing was recorded", not "nothing matters".
    pub confidence: f64,
    /// The harness session, when an event or an `Agent-Session:` trailer named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The harness turn, ditto (ADR-005 §3.3's session-qualified id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// The prompt's digests, and whether its text came with them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptRecord>,
    /// The plan items, in the order they were stated.
    #[serde(default)]
    pub plan: Vec<PlanItem>,
    /// The declared scope, as normalised globs, sorted and deduplicated.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Every rung that fired, strongest first.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    /// Why there is no answer: what was looked for and not found. Absent when there is an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether [`Intent::text`] was truncated to stay inside [`Policy::max_text_bytes`]. Said out loud,
    /// because a quote that silently stops mid-sentence is a quote that changed its meaning.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    /// Globs that were *rejected* rather than normalised (an absolute path, `..`, a NUL). Counted and
    /// named rather than dropped, ADR-005 §6's argument applied to intent rather than to ingest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope_rejected: Vec<String>,
}

/// The `state` field's JSON spelling, kept as a string so a newer build's state is readable rather than
/// fatal (ADR-004 §9.5's forward-compatibility rule, applied to a JSON document).
pub type StateText = String;

fn schema_default() -> String {
    INTENT_SCHEMA.to_owned()
}

impl Intent {
    /// The empty intent: nothing was recorded, and the reason says what was looked for.
    #[must_use]
    pub fn absent(reason: impl Into<String>) -> Self {
        Intent {
            schema: INTENT_SCHEMA.to_owned(),
            state: State::Absent.as_str().to_owned(),
            text: None,
            source: Source::None.as_str().to_owned(),
            confidence: CONFIDENCE_NONE,
            session_id: None,
            turn_id: None,
            prompt: None,
            plan: Vec::new(),
            scope: Vec::new(),
            evidence: Vec::new(),
            reason: Some(reason.into()),
            truncated: false,
            scope_rejected: Vec::new(),
        }
    }

    /// Whether anything was recorded at all — text or a digest.
    #[must_use]
    pub fn is_recorded(&self) -> bool {
        self.state != State::Absent.as_str()
    }

    /// The [`State`] this document holds.
    #[must_use]
    pub fn state_of(&self) -> State {
        State::parse(&self.state)
    }

    /// The [`Source`] this document holds.
    #[must_use]
    pub fn source_of(&self) -> Source {
        Source::parse(&self.source)
    }

    /// Round-trip through the store's `intent` JSON column.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Read the store's `intent` column back. An empty or unparseable value is `None`, which the caller
    /// prints as "no recorded intent" — the same answer an absent row gives, and honest either way.
    #[must_use]
    pub fn from_json(text: &str) -> Option<Intent> {
        if text.trim().is_empty() {
            return None;
        }
        serde_json::from_str(text).ok()
    }

    /// The intent text plus the plan items, as one searchable body.
    ///
    /// This is what goes into `episode_doc`'s body column (ADR-004 §4.8), and it is the whole reason
    /// `e5.2`'s search and ADR-005 §4.1's `search_history` find intent text without either of them
    /// knowing this module exists: the plan list is where a harness usually *puts* the intent, and a
    /// search that indexed only the winning text would miss the items that lost the ladder.
    #[must_use]
    pub fn search_body(&self) -> String {
        let mut out = String::new();
        if let Some(text) = &self.text {
            out.push_str(text);
            out.push('\n');
        }
        for item in &self.plan {
            out.push_str(&item.text);
            out.push('\n');
        }
        for glob in &self.scope {
            out.push_str(glob);
            out.push('\n');
        }
        out
    }
}

/// The knobs, as values (ADR-002 §4.5: a crate below `fathom-cli` never opens `fathom.toml`).
#[derive(Debug, Clone)]
pub struct Policy {
    /// Cap on any one piece of intent text (a prompt, a plan item, a trailer value).
    pub max_text_bytes: usize,
    /// Cap on the plan list kept per episode.
    pub plan_cap: usize,
    /// Cap on the declared scope list kept per episode.
    pub scope_cap: usize,
    /// Which sources may be used, in [`Source`] spelling. A source not listed is **reported as existing
    /// and not used**, exactly as `[deps.provenance] intent_sources` does for dependency provenance.
    pub sources: Vec<String>,
    /// Whether a prompt digest with no text counts as an intent at all. On by default, because
    /// "a request was made and its text was not captured" is a fact worth printing; a repository that
    /// would rather see `no recorded intent` than `digest-only` turns this off and gets its way.
    pub digest_counts: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            // 4 KiB per piece of text. A prompt is a paragraph, not a file, and ADR-005 §6.3's rule is that
            // no file contents cross the boundary; a harness that echoed a 2 MB prompt into a
            // `UserPromptSubmit` payload would otherwise grow the store by the prompt's size forever.
            max_text_bytes: 4096,
            plan_cap: 64,
            scope_cap: 64,
            sources: default_sources(),
            digest_counts: true,
        }
    }
}

impl Policy {
    /// Whether `source` is allowed to decide the text.
    #[must_use]
    pub fn allows(&self, source: Source) -> bool {
        self.sources.iter().any(|entry| entry == source.as_str())
    }
}

/// The default source list: every rung, in ladder order.
///
/// `review-note` and `manual-note` are the two spellings `[deps.provenance] intent_sources` lists that
/// this ladder has no rung for — the review ledger (`e4.6`) and a human's own note (`e6.5`) are the tickets
/// that produce them — and they are absent here rather than refused: an entry naming no rung is a notice,
/// not an error, so a config written against one list keeps working when read by the other.
fn default_sources() -> Vec<String> {
    [
        Source::HookPrompt,
        Source::HookIntent,
        Source::IntentTrailer,
        Source::HookPlan,
        Source::TaskTrailer,
        Source::PlanTrailer,
        Source::PromptDigest,
    ]
    .iter()
    .map(|source| source.as_str().to_owned())
    .collect()
}

/// One episode's commit-side evidence, read out of [`CommitFacts`].
#[derive(Debug, Clone, Default)]
struct TrailerEvidence {
    /// Every distinct `Intent:` / `Agent-Intent:` value in commit order, each with the commit that stated
    /// it. The first is the request; the rest are the same episode saying something else later, which the
    /// evidence list shows and the deciding rung ignores.
    intent: Vec<(String, String)>,
    /// See [`TrailerEvidence::intent`], for `Task:` / `Agent-Task:`.
    task: Vec<(String, String)>,
    plan: Vec<PlanItem>,
    scope: Vec<String>,
    session: Option<String>,
    turn: Option<String>,
}

/// One rung that fired: what it was, how to say it, and the text it can assert (if any).
#[derive(Clone)]
struct Rung {
    source: Source,
    detail: String,
    text: Option<String>,
}

impl Rung {
    fn stated(source: Source, detail: String, text: String) -> Self {
        Rung {
            source,
            detail,
            text: Some(text),
        }
    }
    fn named(source: Source, detail: String, text: String) -> Self {
        Rung {
            source,
            detail,
            text: Some(text),
        }
    }
    fn digest(detail: String) -> Self {
        Rung {
            source: Source::PromptDigest,
            detail,
            text: None,
        }
    }
}

/// Decide the intent of one episode.
///
/// `commits` are the episode's members, in order; `hook` is what the stream said about the turn those
/// commits belong to, when anything did (see [`crate::hooks::Turn::intent`] and
/// [`crate::hooks::attribute`]).
///
/// The episode is the unit rather than the commit because intent is a property of a *turn*: an episode
/// whose three commits each carry the same `Agent-Intent:` line has one intent, not three, and an episode
/// whose commits disagree takes the first one that stated anything — first stated wins, so a trailer added
/// by a later "address review" commit does not rename the request that opened the episode. Every commit
/// that stated an intent is still in [`Intent::evidence`].
#[must_use]
pub fn capture(commits: &[CommitFacts], hook: Option<&TurnEvidence>, policy: &Policy) -> Intent {
    let cap = policy.max_text_bytes.max(16);
    let trailers = read_trailers(commits, policy);

    let mut intent = Intent {
        schema: INTENT_SCHEMA.to_owned(),
        state: State::Absent.as_str().to_owned(),
        source: Source::None.as_str().to_owned(),
        confidence: CONFIDENCE_NONE,
        session_id: hook
            .filter(|turn| !turn.session_id.is_empty())
            .map(|turn| turn.session_id.to_owned())
            .or_else(|| trailers.session.clone()),
        turn_id: hook
            .filter(|turn| !turn.turn_id.is_empty())
            .map(|turn| turn.turn_id.to_owned())
            .or_else(|| trailers.turn.clone()),
        ..Intent::absent(String::new())
    };

    // The plan list. The harness's copy wins where it has one: it is the live list, and a `Plan:` trailer
    // is a transcription of it. The cap is applied here and the fact of the cut is said.
    let stated_plan = hook.filter(|turn| !turn.plan.is_empty());
    let mut plan: Vec<PlanItem> = stated_plan
        .map(|turn| turn.plan.clone())
        .unwrap_or_else(|| trailers.plan.clone());
    if plan.len() > policy.plan_cap {
        plan.truncate(policy.plan_cap);
        intent
            .scope_rejected
            .push(format!("plan list truncated to {} items", policy.plan_cap));
    }
    intent.plan = plan
        .iter()
        .map(|item| PlanItem {
            text: clip(&item.text, cap),
            status: item.status.clone(),
        })
        .collect();

    // The declared scope, from whichever side stated one, normalised and refused-in-public.
    let declared = hook
        .filter(|turn| !turn.scope.is_empty())
        .map_or_else(|| trailers.scope.clone(), |turn| turn.scope.clone());
    let (scope, rejected) = normalise_scope(&declared, policy.scope_cap);
    intent.scope = scope;
    intent.scope_rejected.extend(rejected);

    // The prompt, and whether its text arrived with it.
    if let Some(turn) = hook {
        let text = turn
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        if text.is_some() || turn.prompt_sha256.is_some() || turn.prompt_chars.is_some() {
            intent.prompt = Some(PromptRecord {
                sha256: turn.prompt_sha256.clone(),
                chars: turn
                    .prompt_chars
                    .or_else(|| text.map(|value| value.chars().count() as u64)),
                text_captured: text.is_some(),
            });
        }
    }

    // Every rung that fired, in ladder order. A rung that fired and was not allowed is still recorded in
    // `evidence`: `[deps.provenance] intent_sources`' rule is that evidence from a source nobody listed is
    // "existing-but-not-allowed", not invisible.
    let mut rungs: Vec<Rung> = Vec::new();
    if let Some(text) = hook
        .and_then(|turn| turn.prompt.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        rungs.push(Rung::stated(
            Source::HookPrompt,
            format!("prompt from the harness: {text}"),
            text.to_owned(),
        ));
    }
    if let Some(text) = hook
        .and_then(|turn| turn.intent.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        rungs.push(Rung::stated(
            Source::HookIntent,
            format!("intent declared by the harness: {text}"),
            text.to_owned(),
        ));
    }
    for (value, oid) in &trailers.intent {
        rungs.push(Rung::stated(
            Source::IntentTrailer,
            format!("Intent: {value} (on {})", short(oid)),
            value.clone(),
        ));
    }
    // Row 4: the harness's own plan list, when it had one.
    if stated_plan.is_some()
        && let Some(item) = open_plan_item(&intent.plan)
    {
        rungs.push(Rung::named(
            Source::HookPlan,
            format!("plan item from the harness: {}", item.text),
            item.text.clone(),
        ));
    }
    for (value, oid) in &trailers.task {
        rungs.push(Rung::named(
            Source::TaskTrailer,
            format!("Task: {value} (on {})", short(oid)),
            value.clone(),
        ));
    }
    // Row 6, and deliberately after row 5: a `Plan:` trailer is a transcription of a list somebody else
    // kept, priced at 0.65 against a `Task:` line's 0.70, because a task names the work and a plan item
    // names one step of it. The published spelling is the same (`plan-item`) because a reader of the card
    // is being told "a plan item decided", not which copy of the list it came from.
    if stated_plan.is_none()
        && let Some(item) = open_plan_item(&intent.plan)
    {
        rungs.push(Rung::named(
            Source::PlanTrailer,
            format!("plan item from the trailer: {}", item.text),
            item.text.clone(),
        ));
    }
    if let Some(prompt) = &intent.prompt
        && !prompt.text_captured
    {
        let chars = prompt
            .chars
            .map_or_else(String::new, |count| format!("{count} chars, "));
        let sha = prompt.sha256.as_deref().unwrap_or("no digest");
        rungs.push(Rung::digest(format!(
            "a prompt of {chars}sha {} was recorded; its text was not",
            short(sha)
        )));
    }

    // The deciding rung: the first in ladder order that both fired and is allowed.
    let decision = rungs
        .iter()
        .find(|rung| policy.allows(rung.source))
        .cloned();

    intent.evidence = rungs
        .iter()
        .map(|rung| {
            // A rung that fired and was refused by `[intent] sources` says so in its own evidence line.
            // The alternative is a card that prints `no recorded intent` next to a harness that plainly
            // stated one, and "the config refused to quote it" is a different finding from "nobody said
            // anything" — the same distinction `[deps.provenance] intent_sources` draws.
            let detail = if policy.allows(rung.source) {
                rung.detail.clone()
            } else {
                format!("{} — present, not used (`intent.sources`)", rung.detail)
            };
            Evidence {
                source: rung.source.as_str().to_owned(),
                detail: clip(&detail, cap),
                confidence: rung.source.confidence(),
            }
        })
        .collect();

    match decision {
        Some(rung) => match rung.text {
            Some(text) => {
                let (text, cut) = truncate(text.trim(), cap);
                intent.state = State::Recorded.as_str().to_owned();
                intent.source = rung.source.as_str().to_owned();
                intent.confidence = rung.source.confidence();
                intent.text = Some(text);
                intent.truncated = cut;
                intent.reason = None;
            }
            // A digest proves a request happened and does not say what it was. It is its own state and it
            // never becomes text.
            None => {
                intent.state = State::Digest.as_str().to_owned();
                intent.source = rung.source.as_str().to_owned();
                intent.confidence = rung.source.confidence();
                intent.reason = Some(rung.detail.clone());
            }
        },
        None => {
            intent.state = State::Absent.as_str().to_owned();
            intent.source = Source::None.as_str().to_owned();
            intent.confidence = CONFIDENCE_NONE;
            // "Nothing said anything" and "something said something this build is configured not to
            // quote" are the two reasons the tally can be empty, and the second one is a configuration
            // the reader can change.
            intent.reason = Some(
                match rungs.iter().find(|rung| !policy.allows(rung.source)) {
                    Some(rung) => format!(
                        "{}`[intent] sources` does not let {} decide the text, and nothing else stated one",
                        rung.detail,
                        rung.source.as_str()
                    ),
                    None => absent_reason(hook, &trailers).to_owned(),
                },
            );
        }
    }
    if intent.state == State::Digest.as_str() && !policy.digest_counts {
        let mut absent = Intent::absent(absent_reason(hook, &trailers));
        absent.session_id = intent.session_id.clone();
        absent.turn_id = intent.turn_id.clone();
        absent.plan = intent.plan.clone();
        absent.scope = intent.scope.clone();
        absent.scope_rejected = intent.scope_rejected.clone();
        absent.prompt = intent.prompt.clone();
        absent.evidence = intent.evidence.clone();
        return absent;
    }
    intent
}

/// The plan item that stands for the plan: the first the harness does *not* call finished, or, when every
/// item is finished, the first one. A plan whose every item is done still says what the turn was for, and
/// an empty plan has no item to offer.
fn open_plan_item(plan: &[PlanItem]) -> Option<&PlanItem> {
    plan.iter().find(|item| !item.is_done()).or(plan.first())
}

/// What to say when nothing was recorded: the channels looked for, and which of them existed at all.
fn absent_reason(hook: Option<&TurnEvidence>, trailers: &TrailerEvidence) -> &'static str {
    if hook.is_some() {
        "the harness recorded no prompt text, no intent line and no plan for this turn, and no `Intent:` or `Task:` trailer states one"
    } else if trailers.session.is_some() || trailers.turn.is_some() {
        "the commit names a harness turn that the event stream does not describe, and states no intent of its own"
    } else {
        "no harness events for this repository, and no `Intent:` or `Task:` trailer on the commit"
    }
}

/// Read the commit-side evidence out of the episode's commits, in commit order.
fn read_trailers(commits: &[CommitFacts], policy: &Policy) -> TrailerEvidence {
    let cap = policy.max_text_bytes.max(16);
    let mut out = TrailerEvidence::default();
    for commit in commits {
        for key in INTENT_TRAILERS {
            if let Some(value) = trailer(commit, key) {
                stated(&mut out.intent, clip(&value, cap), &commit.oid);
            }
        }
        for key in TASK_TRAILERS {
            if let Some(value) = trailer(commit, key) {
                stated(&mut out.task, clip(&value, cap), &commit.oid);
            }
        }
        for key in SCOPE_TRAILERS {
            if let Some(value) = trailer(commit, key) {
                out.scope.extend(split_list(&value));
            }
        }
        for key in PLAN_TRAILERS {
            if let Some(value) = trailer(commit, key) {
                out.plan
                    .extend(split_list(&value).into_iter().map(PlanItem::new));
            }
        }
        if out.session.is_none() {
            out.session = trailer(commit, "agent-session").or_else(|| trailer(commit, "session"));
        }
        if out.turn.is_none() {
            out.turn = trailer(commit, "agent-turn").or_else(|| trailer(commit, "turn"));
        }
    }
    out
}

fn trailer(commit: &CommitFacts, key: &str) -> Option<String> {
    commit
        .trailers
        .get(key)
        .cloned()
        .filter(|value| !value.trim().is_empty())
}

/// Record one stated trailer value against the first commit that said it.
///
/// Three commits carrying the same `Agent-Intent:` line are one intent, not three rungs — that is the whole
/// reason the episode and not the commit is the unit here. Two commits carrying *different* lines are two
/// rungs and still one answer, the first's, with the second visible on the evidence list as the later thing
/// somebody asked for.
fn stated(out: &mut Vec<(String, String)>, value: String, oid: &str) {
    if !out.iter().any(|(seen, _)| seen == &value) {
        out.push((value, oid.to_owned()));
    }
}

/// Split a trailer value into items: `;`, `,` and the newline a folded continuation becomes.
///
/// Deliberately not whitespace: a scope list is a sentence-ish list and a glob may hold spaces inside
/// braces. `,,` and `;;` produce no empty item rather than one that reads as nothing.
fn split_list(value: &str) -> Vec<String> {
    value
        .split([';', ',', '\n'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Normalise declared-scope globs: trim, drop a `./`, reject what could escape the repository, cap, sort,
/// dedupe.
///
/// The rejection list is returned rather than swallowed. A declared scope of `/` or `../secrets` is either
/// a mistake or a probe, and in both cases the reviewer wants the row that says it was refused, not a scope
/// list that quietly came back empty — an empty scope means "nothing was declared", and folding a refusal
/// into that would be the same category of bug as a quarantine that looks like an empty inbox.
#[must_use]
pub fn normalise_scope(values: &[String], cap: usize) -> (Vec<String>, Vec<String>) {
    let mut kept: Vec<String> = Vec::new();
    let mut rejected: Vec<String> = Vec::new();
    for value in values {
        let trimmed = value.trim().trim_matches('"').trim();
        let glob = trimmed.strip_prefix("./").unwrap_or(trimmed);
        if glob.is_empty()
            || glob.starts_with('/')
            || glob.contains('\\')
            || glob.contains('\0')
            || glob
                .split('/')
                .any(|segment| segment == ".." || segment == "~")
        {
            if !trimmed.is_empty() {
                rejected.push(trimmed.to_owned());
            }
            continue;
        }
        if !kept.contains(&glob.to_owned()) {
            kept.push(glob.to_owned());
        }
    }
    kept.sort();
    let mut overflow = if kept.len() > cap.max(1) {
        kept.split_off(cap.max(1))
    } else {
        Vec::new()
    };
    rejected.append(&mut overflow);
    (kept, rejected)
}

/// Truncate to `cap` bytes at a character boundary, saying whether anything was cut.
fn truncate(text: &str, cap: usize) -> (String, bool) {
    if text.len() <= cap {
        return (text.to_owned(), false);
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

fn clip(text: &str, cap: usize) -> String {
    truncate(text, cap).0
}

/// A display-short form for a digest or an id in an evidence line.
fn short(value: &str) -> String {
    value.chars().take(8).collect()
}

// ---------------------------------------------------------------------------
// Reading the vocabulary out of a payload
// ---------------------------------------------------------------------------

/// The payload keys that carry the prompt **text**, in priority order.
///
/// `prompt` is what a Claude-Code-shaped `UserPromptSubmit` payload sends; the others are the shapes the
/// other two adapters were written against. ADR-005 §2 keeps the payload verbatim, so all of them are
/// readable without a vendor branch, and an unknown key is additive-safe (§5.1) rather than a breakage.
pub const PROMPT_KEYS: [&str; 3] = ["prompt", "prompt_text", "user_prompt"];
/// The payload keys that carry a declared intent line.
///
/// `declared_intent` is the key `fathom-fixtures` already writes on a `turn_start`
/// (`fathom-fixtures/src/events.rs`), so the corpus's vocabulary and this reader agree by construction.
pub const INTENT_KEYS: [&str; 3] = ["intent", "declared_intent", "agent_intent"];
/// The payload keys that carry the plan list.
pub const PLAN_KEYS: [&str; 2] = ["plan", "plan_items"];
/// The payload keys that carry the declared scope globs.
pub const SCOPE_KEYS: [&str; 3] = ["scope", "declared_scope", "intent_scope"];

impl TurnEvidence {
    /// Read what one event's payload states about the turn's intent.
    ///
    /// Text is truncated to `cap` bytes at the boundary it is read at, not later: the cursor holds
    /// [`Limits::turns_kept`](crate::hooks::Limits::turns_kept) turns across a restart, so an uncapped
    /// prompt is an uncapped cursor (ADR-005 §6 rule 3, applied to intent rather than to bytes on disk).
    ///
    /// Nothing here trusts shape: a `plan` that is a string rather than a list is read as one item, a
    /// `scope` that holds numbers is skipped, and an object item takes the first of
    /// `text`/`content`/`item`/`title`/`description` that holds a string. A malformed field is no intent,
    /// which is a state the ladder already models.
    #[must_use]
    pub fn from_payload(payload: &serde_json::Value, cap: usize) -> Self {
        let text = |value: &serde_json::Value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(|text| clip(text, cap.max(16)))
        };
        let list = |keys: &[&'static str]| -> Option<Vec<String>> {
            for key in keys {
                match payload.get(key) {
                    Some(serde_json::Value::Array(items)) => {
                        let out: Vec<String> = items
                            .iter()
                            .filter_map(|item| match item {
                                serde_json::Value::String(_) => text(item),
                                serde_json::Value::Object(_) => {
                                    ["text", "content", "item", "title", "description"]
                                        .iter()
                                        .find_map(|field| item.get(*field).and_then(text))
                                }
                                _ => None,
                            })
                            .collect();
                        return Some(out);
                    }
                    Some(value @ serde_json::Value::String(_)) => {
                        // A folded `Scope: a, b` or a harness that sends `plan: "one; two"` states its list
                        // in one string; the same split the trailer reader uses answers it.
                        return Some(split_list(&text(value).unwrap_or_default()));
                    }
                    _ => {}
                }
            }
            None
        };
        let prompt = PROMPT_KEYS
            .iter()
            .find_map(|key| payload.get(*key).and_then(text));
        let intent = INTENT_KEYS
            .iter()
            .find_map(|key| payload.get(*key).and_then(text));
        let plan = list(&PLAN_KEYS)
            .unwrap_or_default()
            .into_iter()
            .map(|item| PlanItem {
                text: item,
                status: None,
            })
            .collect::<Vec<_>>();
        // A plan whose items arrived as objects may carry a status per item; re-read with the status.
        let plan: Vec<PlanItem> = match PLAN_KEYS
            .iter()
            .find_map(|key| payload.get(*key).and_then(serde_json::Value::as_array))
        {
            Some(items) if !items.is_empty() && plan.len() == items.len() => plan
                .into_iter()
                .zip(items.iter())
                .map(|(mut item, raw)| {
                    item.status = ["status", "state", "state_type"].iter().find_map(|field| {
                        raw.get(*field).and_then(|v| v.as_str()).map(str::to_owned)
                    });
                    item
                })
                .collect(),
            _ => plan,
        };
        let scope = list(&SCOPE_KEYS).unwrap_or_default();
        TurnEvidence {
            session_id: String::new(),
            turn_id: String::new(),
            prompt,
            prompt_sha256: payload
                .get("prompt_sha256")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            prompt_chars: payload
                .get("prompt_chars")
                .and_then(serde_json::Value::as_u64),
            intent,
            plan,
            scope,
        }
    }
}

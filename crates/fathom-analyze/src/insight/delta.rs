//! The one versioned delta model (`e2.11` / `reco-tt4`), published as `fathom_analyze::insight`.
//!
//! This is the structure the TUI card, `fathom analyze --json`, the review artefact (`e5.7`) and the MCP
//! tools (`e6.4`) all read. ADR-001 §7.10 is the reason it exists: four surfaces that each assemble their
//! own idea of "what this episode did" is four realities, and every one of them is plausible right up until
//! a reviewer notices the export says 6 files and the card says 4. So the analysis model is the source of
//! truth and the surfaces are renderings of it — which is only enforceable if the model says everything a
//! surface needs, including what it does *not* know.
//!
//! # What a delta is
//!
//! A [`EpisodeDelta`] is one episode: its identity, its rows, its derived totals, seven analysis sections
//! in a fixed order, the ranking, the gaps, and a self-check. Three rules hold it together, and each one is
//! a type rather than a habit:
//!
//! * **every section is always present.** The seven ids are [`SectionId::ALL`], and a section that could not
//!   be answered is `unavailable` with a [`Gap`] — not missing. A consumer that has to handle "the section
//!   is absent" and "the section says no" differently is a consumer that will get them confused. This is
//!   `e2.10`'s card rule (`docs/concepts/summary-card.md`), inherited deliberately.
//! * **every figure carries its evidence.** [`Figure::new`] is the only way to make one and it takes the
//!   [`Evidence`] as an argument, so a figure with no pointer cannot be constructed by accident. The pointer
//!   is a JSON pointer *into this same document* wherever the evidence is internal, which makes the document
//!   self-auditing, and a `run` command or an owning ticket wherever it is not.
//! * **the document checks itself.** [`EpisodeDelta::verify`] re-reads the built document the way a reviewer
//!   would: does every figure's pointer resolve, do the file rows sum to the totals, is every `unavailable`
//!   section explained, are the seven sections in order. The result travels *inside* the document as
//!   `integrity`, so an export that disagrees with itself says so in the artifact rather than in a bug
//!   report three weeks later.
//!
//! # What it is not
//!
//! It is not a bag of every analysis' internals. The sections carry headline figures with their absolutes and
//! their basis — the same three things `e2.10`'s card prints — and the pointer says where the detail lives.
//! Nor is it a database dump: it is built from rows that were already read ([`Input`]), so a rail page of
//! five hundred deltas costs no more queries than a rail page of five hundred rows.
//!
//! # Versioning
//!
//! `contract` is [`CONTRACT`] and `schema_version` is [`VERSION`]. Additive changes — a new optional field, a
//! new section appended, a new figure — keep the version and are ignored by older readers. Breaking changes
//! (a removed or renamed field, a narrowed enum, a reordered section list) bump the version, and
//! `tools/check_delta_compat.py` fails CI when a schema changes in a breaking way without that bump. The
//! schema itself is generated from this file's types by [`super::schema`], so the checked-in
//! `schemas/episode-delta.schema.json` cannot drift from the struct it describes.

#![deny(missing_docs)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::risk::{self, Surface};

/// The contract id of this structure: one string, printed in every payload, versioned like a schema.
///
/// Same spelling as `e2.10`'s `fathom-card/1` for the same reason — a consumer switches on the shape it got
/// rather than the shape it hoped for.
pub const CONTRACT: &str = "fathom-delta/2";

/// The version of this structure. Bump it in the same commit as a breaking schema change, and no earlier.
///
/// `e10.3` (`reco-ydek.3`) is v2: the queue's placement figures — `risk.rank` and `risk.ranked`, the
/// `rank N of M` a ranked set produced — left the document with the pane that printed them. A removal is
/// breaking under the policy above, however few readers used it, so the number moves and the baseline is
/// re-blessed in the same commit (`python3 tools/check_delta_compat.py bless`).
pub const VERSION: u32 = 2;

/// How many figures one section may carry before the remainder is named.
///
/// A cap with a stated remainder, not a truncation: `e2.10`'s card rule that a cut which is not counted is
/// a lost fact. Ten is what the `what` section needs now that `e8.5` has put a complexity figure in it —
/// the four size figures, the complexity figure, the binary row, the capped-read row, the two taxonomy
/// rows and the noise fold — because a cap that dropped the noise fold to make room for a new figure would
/// have traded one honest number for another. Raising it is additive: `maxItems` in the published schema
/// only ever accepts more.
pub const FIGURE_CAP: usize = 10;

/// How many rows of a rollup (surfaces, directories, classes) one figure's value may list.
pub const ROLLUP_CAP: usize = 5;

/// How many member commits the `episode.members` row lists before the remainder is counted.
pub const MEMBER_CAP: usize = 8;

/// How many plan items the text rendering lists before it counts the rest (`e4.2`, `reco-c54`). Six is
/// not a threshold, only a legibility limit: the document carries every item, and a card that printed
/// forty would hide the four findings under them.
const PLAN_LINES: usize = 6;

/// The section states, as serialized. Three, because they are the three claims a reviewer acts on
/// differently: this was measured, this was measured with a hole in it, this was not measured at all.
///
/// Module-level constants with a `STATE_` prefix rather than a nested `pub mod state`: `M02`'s sibling
/// rule `W4` keeps a crate's public module list in one file (`src/lib.rs`), and a submodule of a
/// submodule is exactly the kind of surface nobody finds by looking at the crate root.
/// Something answered the question, with the basis printed.
pub const STATE_MEASURED: &str = "measured";
/// Something answered part of it; the hole is in `gap`.
pub const STATE_PARTIAL: &str = "partial";
/// Nothing answered it. `gap` says who will and what to run meanwhile.
pub const STATE_UNAVAILABLE: &str = "unavailable";

/// Every state, in the order the schema lists them.
pub const STATE_ALL: [(&str, &str); 3] = [
    (STATE_MEASURED, "the analysis answered, with its basis"),
    (
        STATE_PARTIAL,
        "the analysis answered part of it, and the hole is named",
    ),
    (
        STATE_UNAVAILABLE,
        "nothing answered it, and the owner is named",
    ),
];

/// The seven questions, in the order a reviewer reads them.
///
/// The list is a constant, not a consequence of what happened to be measured — `e2.10`'s argument, and the
/// reason it survives here: a card whose sections depend on what arrived cannot distinguish "not measured"
/// from "not asked". The ids and owners are the card's, so the Python card and this model name one set of
/// questions rather than two overlapping ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SectionId {
    /// What changed, once the noise is folded away.
    What,
    /// Where it landed, and whether the tree moved.
    Where,
    /// What the tree now depends on that it did not.
    Dependencies,
    /// Which non-code surfaces moved, and who has to act.
    Surface,
    /// What is tested after this change.
    Tests,
    /// How far the change reaches.
    Blast,
    /// Why this episode is in front of a human.
    Attention,
}

impl SectionId {
    /// All seven, in reading order. The order is part of the contract (see `verify`).
    pub const ALL: [SectionId; 7] = [
        SectionId::What,
        SectionId::Where,
        SectionId::Dependencies,
        SectionId::Surface,
        SectionId::Tests,
        SectionId::Blast,
        SectionId::Attention,
    ];

    /// The serialized id.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SectionId::What => "what",
            SectionId::Where => "where",
            SectionId::Dependencies => "dependencies",
            SectionId::Surface => "surface",
            SectionId::Tests => "tests",
            SectionId::Blast => "blast",
            SectionId::Attention => "attention",
        }
    }

    /// Parse a section id; anything unknown comes back as `None` so a newer document's extra section is
    /// skipped rather than fatal (the forward-compatibility rule in the module header).
    #[must_use]
    pub fn parse(value: &str) -> Option<SectionId> {
        SectionId::ALL.into_iter().find(|id| id.as_str() == value)
    }

    /// The row label a surface prints.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            SectionId::What => "what changed",
            SectionId::Where => "where it landed",
            SectionId::Dependencies => "what it depends on",
            SectionId::Surface => "what surface it touches",
            SectionId::Tests => "what is tested",
            SectionId::Blast => "what it reaches",
            SectionId::Attention => "why it is here",
        }
    }

    /// The question the section answers, printed by `fathom analyze --schema --describe` and by the docs.
    #[must_use]
    pub const fn question(self) -> &'static str {
        match self {
            SectionId::What => "What changed, once the noise is folded away?",
            SectionId::Where => "Where did the code land, and did the tree move?",
            SectionId::Dependencies => "What does the tree now depend on that it did not?",
            SectionId::Surface => "Which non-code surfaces moved, and who has to do something?",
            SectionId::Tests => "What is tested after this change?",
            SectionId::Blast => "How far does this change reach?",
            SectionId::Attention => "Why is this episode in front of a human?",
        }
    }

    /// The ticket whose analysis owns a full answer.
    ///
    /// This is a *plan key*, and `verify` requires every gap to name one, so "we have not done that" is
    /// always traceable to the ticket that does it (`ADR-001 §7.2`: a refusal is a pointer).
    #[must_use]
    pub const fn owner(self) -> &'static str {
        match self {
            SectionId::What => "e2.2",
            SectionId::Where => "e2.3",
            SectionId::Dependencies => "e2.6",
            SectionId::Surface => "e2.8",
            SectionId::Tests => "e2.9",
            SectionId::Blast => "e3.2",
            SectionId::Attention => "e4.4",
        }
    }

    /// What this build can say about the section, when nothing better has been handed over.
    ///
    /// The honest default. Each of these is a *statement about this build*, not a claim the analysis is
    /// impossible: `e2.2`'s noise fold, `e2.3`'s shape sentence, `e2.8`'s findings and `e2.9`'s test names
    /// all exist in the Python analysis surface and reach the Rust model as [`Supplied`] input when a
    /// caller lends them.
    #[must_use]
    pub const fn default_state(self) -> &'static str {
        match self {
            // Path-class discounting is real and auditable; token-level noise folding is not in this build.
            SectionId::What => STATE_PARTIAL,
            SectionId::Where => STATE_PARTIAL,
            SectionId::Dependencies => STATE_UNAVAILABLE,
            SectionId::Surface => STATE_PARTIAL,
            SectionId::Tests => STATE_PARTIAL,
            SectionId::Blast => STATE_UNAVAILABLE,
            SectionId::Attention => STATE_MEASURED,
        }
    }
}

// ---------------------------------------------------------------------------
// the shapes
// ---------------------------------------------------------------------------

/// Where a figure's evidence lives.
///
/// Exactly one of the three is normally set, and [`Figure::new`] refuses a figure with none of them:
/// `e2.10`'s "evidence is a constructor, not a convention", expressed as a function signature.
///
/// * [`Evidence::inside`] points at a JSON pointer *within the delta document itself* — the file rows
///   behind a count, the components behind a score. It is the strongest form, because [`EpisodeDelta::verify`]
///   can walk it and fail the document when the pointer resolves to nothing.
/// * [`Evidence::external`] names a command the reviewer can actually type, for evidence that lives outside
///   the document (another verb, another pane).
/// * [`Evidence::owner`] names the ticket that owns the answer, for a figure that is standing in for a
///   measurement it cannot make.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// A JSON pointer into this document, e.g. `#/files` or `#/risk/components`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// A command the reviewer can run to see the evidence, e.g. `fathom show HEAD~3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// The plan key that owns what is missing, e.g. `e3.2`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

impl Evidence {
    /// Evidence inside this document, addressed by JSON pointer.
    #[must_use]
    pub fn inside(pointer: impl Into<String>) -> Self {
        Evidence {
            pointer: Some(pointer.into()),
            run: None,
            owner: None,
        }
    }

    /// Evidence the reviewer can go and look at with a command this build answers to.
    #[must_use]
    pub fn external(run: impl Into<String>) -> Self {
        Evidence {
            pointer: None,
            run: Some(run.into()),
            owner: None,
        }
    }

    /// Evidence that does not exist yet, attributed to the ticket that will produce it.
    #[must_use]
    pub fn owner(ticket: impl Into<String>) -> Self {
        Evidence {
            pointer: None,
            run: None,
            owner: Some(ticket.into()),
        }
    }

    /// Is anything set? `Figure::new` is the only caller that cares, and it is the reason a figure cannot
    /// be born without a pointer.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pointer.is_none() && self.run.is_none() && self.owner.is_none()
    }
}

/// One printed claim, with the raw numbers behind it and the pointer to its evidence.
///
/// `value` is the sentence fragment a surface prints verbatim; `raw` is what a machine compares. Both are
/// required, because the two consumers need different things and a figure that only carries one of them
/// forces the other to re-derive it — which is exactly the second opinion this model exists to forbid.
/// A ratio is never carried alone: the absolutes are in `raw` with it (`e2.9`'s rule that a denominator has
/// to travel with its fraction).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Figure {
    /// The short label the surface prints before the value.
    pub label: String,
    /// The value as the product words it, already grouped and unit-suffixed. Empty means `unknown`, and
    /// `reason` then says why.
    pub value: String,
    /// Whether anything was measured. `false` is printed as `?` and never as a zero (`ADR-001 §7.7`).
    pub known: bool,
    /// Why it is unknown, when it is. An unknown without a reason is a blank, and a blank is a lie.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The integers behind `value`, keyed and sorted, for a consumer that compares rather than reads.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub raw: BTreeMap<String, i64>,
    /// A confidence in 0..=1 when the figure is an estimate, and only then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// Where the evidence is. Required at construction; see [`Evidence`].
    pub evidence: Evidence,
}

impl Figure {
    /// A measured figure. `raw` may be empty when the value is a list of names rather than a number.
    #[must_use]
    pub fn new(label: impl Into<String>, value: impl Into<String>, evidence: Evidence) -> Self {
        Figure {
            label: label.into(),
            value: value.into(),
            known: true,
            reason: None,
            raw: BTreeMap::new(),
            confidence: None,
            evidence,
        }
    }

    /// A figure that says nothing was measured, and why. It still carries its evidence: the reason *this*
    /// build cannot answer is itself a fact, and it points at the ticket that owns the answer.
    #[must_use]
    pub fn unknown(
        label: impl Into<String>,
        reason: impl Into<String>,
        evidence: Evidence,
    ) -> Self {
        Figure {
            label: label.into(),
            value: String::new(),
            known: false,
            reason: Some(reason.into()),
            raw: BTreeMap::new(),
            confidence: None,
            evidence,
        }
    }

    /// Attach the raw integers a machine will compare against.
    #[must_use]
    pub fn with_raw(mut self, raw: &[(&str, i64)]) -> Self {
        self.raw = raw
            .iter()
            .map(|(key, value)| ((*key).to_owned(), *value))
            .collect();
        self
    }

    /// Attach a confidence, for an estimate.
    #[must_use]
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence);
        self
    }
}

/// Why a section is not a full answer, who owns the rest, and what to do meanwhile.
///
/// The three fields are the difference between a gap and a shrug. `ADR-006 §10.3`: "a shrug is not a
/// remedy", and a remedy that does not exist is worse — so `remedy` is only filled with a command this
/// binary answers to, and otherwise left empty with `owner` carrying the weight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    /// The section this gap belongs to, by its serialized id.
    pub section: String,
    /// The one-line version, for the brief: a card has thirty-five columns and a reason does not fit in
    /// them. Required, because "a gap that is too long to print is a gap that is not printed" is the
    /// failure `e2.10` names, and the short form is what keeps the one-screen card honest about what it
    /// left out.
    pub short: String,
    /// What is missing, stated as a fact about this build rather than as an apology.
    pub reason: String,
    /// The plan key that owns the answer.
    pub owner: String,
    /// A command the reviewer can run instead, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
}

/// One of the seven sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// One of [`SectionId::ALL`], or a newer document's addition (ignored, never fatal).
    pub id: String,
    /// The row label.
    pub title: String,
    /// The question it answers, printed in `fathom analyze --schema --describe` and in the docs.
    pub question: String,
    /// The ticket whose analysis owns a full answer.
    pub owner: String,
    /// One of [`state`]'s three.
    pub state: String,
    /// The figures, capped at [`FIGURE_CAP`], in the order they are printed.
    pub figures: Vec<Figure>,
    /// How many figures were dropped by the cap — counted, never silently cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remainder: Option<u64>,
    /// Why the state is what it is, when it is not `measured`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Gap>,
}

impl Section {
    /// Build one section from a [`SectionId`] and the figures this build managed to produce.
    fn build(id: SectionId, state: &str, figures: Vec<Figure>, gap: Option<Gap>) -> Section {
        let (kept, remainder) = cap(figures, FIGURE_CAP);
        Section {
            id: id.as_str().to_owned(),
            title: id.title().to_owned(),
            question: id.question().to_owned(),
            owner: id.owner().to_owned(),
            state: state.to_owned(),
            figures: kept,
            remainder: (remainder > 0).then_some(remainder),
            gap,
        }
    }
}

/// One file the episode touched, as the index recorded it.
///
/// This is the evidence layer of the document: most figure pointers land here, so the rows have to carry the
/// taxonomy decisions the figures were derived from and not just the path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDelta {
    /// The path, as the store spells it (slash-separated, repo-relative).
    pub path: String,
    /// The path it came from, when the index asserted a move. `None` is "no move was recorded", which is
    /// *not* the same claim as `change: unrecorded` — see [`FileDelta::basis`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    /// `added`, `modified`, `deleted`, `renamed`, `mode-only`, or `unrecorded`.
    pub change: String,
    /// Lines added.
    pub add: i64,
    /// Lines removed.
    pub del: i64,
    /// Whether the change was binary.
    pub binary: bool,
    /// Member commits that touched this path.
    pub commits: i64,
    /// `e0.9`'s path class as stored, or `unknown`.
    pub path_class: String,
    /// `e0.9`'s noise class as stored, or `unknown`.
    pub noise_class: String,
    /// The reviewer-facing surface, from [`crate::risk::classify`] — the shipped vocabulary, not a second one.
    pub surface: String,
    /// How much of this row the read actually saw: `full` (per-commit file rows) or `rollup` (the summed
    /// per-path read a rail page uses, which has no change kind and no move).
    pub basis: String,
}

impl FileDelta {
    /// Churn on this one file.
    #[must_use]
    pub fn churn(&self) -> i64 {
        self.add.saturating_add(self.del)
    }
}

/// The attribution an episode carries, as the delta document prints it (`e4.1`, `reco-w7q`).
///
/// This is the label *plus the case for it*: which class of evidence decided, which agent it named, the
/// whole evidence list, and the reason when the answer is `unknown`. The ticket's rule is that an ambiguous
/// commit is never silently bucketed, and a reviewer who cannot see why a commit reads `agent` cannot
/// disagree with it — the same argument ADR-003 §4 makes about boundary rules, applied to authorship.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributionRef {
    /// `agent`, `human` or `unknown` — the same vocabulary as [`EpisodeRef::author_kind`].
    pub kind: String,
    /// The rung that decided: `correction`, `harness`, `trailer`, `declared`, `author`, `committer`,
    /// `message`, `identity`, `absent`.
    pub source: String,
    /// The agent the evidence named, when it named exactly one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Confidence in `kind`; `0.0` means nothing was known, not that nothing matters.
    pub confidence: f64,
    /// Why this is `unknown`, or why a label was refused. Absent when the answer was simply the answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Everything that fired, strongest first. Empty on a row that stored no evidence list.
    #[serde(default)]
    pub evidence: Vec<AttributionEvidence>,
}

/// One line of the evidence list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributionEvidence {
    /// Which class of evidence this is, in the same vocabulary as [`AttributionRef::source`].
    pub source: String,
    /// The literal, in the commit's own spelling.
    pub detail: String,
    /// What this item alone would have priced the verdict at.
    pub confidence: f64,
}

/// The published spellings of the intent states, as the delta document writes them (`e4.2`, `reco-c54`).
///
/// A view crate reads these instead of naming `fathom-ingest` itself: ADR-002 §5.2 gives `fathom-tui` no
/// ingest edge, and a pane that could not spell the states it prints would be a pane re-deriving them.
/// They are the ingest crate's own strings *by construction*: each one is `State::as_str` called in a
/// constant expression, so there is no second copy that can drift — the same shape as [`STATE_MEASURED`]
/// and its siblings above.
pub const INTENT_RECORDED: &str = fathom_ingest::intent::State::Recorded.as_str();
/// A prompt was recorded, its text was not.
pub const INTENT_DIGEST_ONLY: &str = fathom_ingest::intent::State::Digest.as_str();
/// Nothing stated the episode's purpose, and that was checked.
pub const INTENT_NONE: &str = fathom_ingest::intent::State::Absent.as_str();

/// The intent block of the text rendering, as lines (`e4.2`, `reco-c54`).
///
/// Three states get three different sentences, because the ticket's rule is that "nothing was
/// recorded" is a state and not a blank: a reader who cannot tell *"nothing asked"* from *"something
/// asked and we did not capture it"* has to go ask a human, which is the thing this tool exists to
/// avoid. Scope and plan get their own lines rather than being folded into the quote, because they are
/// checked as well as read — scope is what `e4.3`'s scope-creep detector measures a change against
/// ([`crate::scope::deviation`]), and a plan is read item by
/// item or not at all.
///
/// Public because the card pane and `fathom show` draw these same lines: one rendering of an intent for
/// every surface, not three that could drift (ADR-001 §7.10).
#[must_use]
pub fn intent_block(intent: &fathom_ingest::intent::Intent) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    use fathom_ingest::intent::State;
    match State::parse(&intent.state) {
        State::Recorded => out.push(format!(
            "intent    {} · via {} · {:.2}",
            intent.text.clone().unwrap_or_default(),
            intent.source,
            intent.confidence
        )),
        State::Digest => {
            out.push(format!(
                "intent    no text captured · prompt {} · via {} · {:.2}",
                intent
                    .prompt
                    .as_ref()
                    .and_then(|prompt| prompt.sha256.as_deref())
                    .and_then(|sha| sha.get(..12))
                    .unwrap_or("—"),
                intent.source,
                intent.confidence
            ));
            // The digest state explains itself the way the absence does: "no text" invites "then go and
            // find it", and the reason line is what tells the reviewer there is nothing to find.
            if let Some(reason) = intent.reason.as_deref() {
                out.push(format!("reason    {reason}"));
            }
        }
        State::Absent => out.push(format!(
            "intent    no recorded intent · {}",
            intent
                .reason
                .clone()
                .unwrap_or_else(|| "nothing stated it, in the hooks or in the trailers".to_owned())
        )),
    }
    if intent.truncated {
        out.push("intent    (the quote above was cut at the stored byte cap)".to_owned());
    }
    if !intent.scope.is_empty() {
        out.push(format!(
            "scope     {} (declared){}",
            intent.scope.join(", "),
            if intent.scope_rejected.is_empty() {
                String::new()
            } else {
                format!(
                    " · {} refused: {}",
                    intent.scope_rejected.len(),
                    intent.scope_rejected.join(", ")
                )
            }
        ));
    }
    for (index, item) in intent.plan.iter().take(PLAN_LINES).enumerate() {
        out.push(format!(
            "plan      {}. {}{}",
            index + 1,
            item.text,
            match &item.status {
                Some(status) => format!(" [{status}]"),
                None => String::new(),
            }
        ));
    }
    if intent.plan.len() > PLAN_LINES {
        out.push(format!(
            "plan      … {} more item(s)",
            intent.plan.len() - PLAN_LINES
        ));
    }
    out
}

/// The episode's own identity, carried verbatim from the spine.
///
/// The digests are here so that a consumer of `--json` can relink after a rebase without asking the store
/// again, and so an exported artefact names the identity it was exported for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeRef {
    /// ADR-003 §2.1's derived id — what a review is keyed to.
    pub id: String,
    /// The strong digest.
    pub fingerprint: String,
    /// Relink tier 1.
    pub fallback_fingerprint: String,
    /// Relink tier 2.
    pub shape_fingerprint: String,
    /// Member commit SHAs, capped at [`MEMBER_CAP`]; `n_commits` is the truth.
    pub members: Vec<String>,
    /// How many were dropped from `members`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_remainder: Option<u64>,
    /// Member commits, counted.
    pub n_commits: i64,
    /// The summary line.
    pub summary: String,
    /// `agent`, `human` or `unknown`.
    pub author_kind: String,
    /// The author as committed.
    pub author_name: String,
    /// The author's email as committed.
    pub author_email: String,
    /// How far to trust the attribution; `None` prints `?`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_confidence: Option<f64>,
    /// The attribution behind [`EpisodeRef::author_kind`], with the evidence that produced it (`e4.1`).
    /// `None` on a row written before migration 4, which recorded the label and nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<AttributionRef>,
    /// What the episode was *for*, with the evidence for that (`e4.2`, `reco-c54`): the text, the session
    /// and turn it arrived on, the plan items, the declared scope globs, and the rung that decided. This
    /// is the store's intent document, unchanged, because the store column and the document field must not
    /// be two answers to one question (ADR-001 §7.10). `None` means the row was written before migration
    /// 5 and no intent was ever read for it — which is a different fact from `state: "none"`, the state a
    /// row carries when intent *was* looked for and nothing was there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<fathom_ingest::intent::Intent>,
    /// Which boundary rule opened the episode, as the store recorded it; how far to trust it is
    /// [`EpisodeRef::boundary_confidence`] (`ADR-003 §4.7`).
    pub boundary_rule: String,
    /// See [`EpisodeRef::boundary_rule`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_confidence: Option<f64>,
    /// The harness session and turn, when a trailer or an event named one (`ADR-005`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// See [`EpisodeRef::session`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    /// First member timestamp, **milliseconds** since the epoch — the store's unit, not
    /// `fathom-core`'s microseconds. A surface that wants a `fathom_core::time::Timestamp` multiplies
    /// by 1 000 on the way in, which is the same conversion `reco-wzt`'s rail read makes.
    pub first_utc: i64,
    /// See [`EpisodeRef::first_utc`].
    pub last_utc: i64,
    /// The review marker as stored: `unread`, `read`, `accepted`, `flagged`, `dismissed`, `snoozed`.
    pub review: String,
    /// Whether a current commit still belongs to it (`ADR-003 §6.3`).
    pub live: bool,
    /// The branch or lineage the episode sits on, and its position.
    pub lineage_ref: String,
    /// See [`EpisodeRef::lineage_ref`].
    pub lineage_seq: i64,
}

/// The episode's scale, and what of it a review actually reads.
///
/// The caller fills the six spine counts; [`compose`] fills the four derived ones, which is why they are
/// plain fields rather than methods: they travel in the document, so a consumer can check the arithmetic
/// without running the code that made it. The discount rule is [`crate::risk`]'s — generated, vendored,
/// lockfile, binary churn counts a quarter — restated here rather than invented twice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    /// Files touched, as the episode rollup records them.
    pub n_files: i64,
    /// Lines added.
    pub n_add: i64,
    /// Lines removed.
    pub n_del: i64,
    /// Binary files in the change.
    pub n_binary: i64,
    /// Churn, `n_add + n_del`. Filled by [`compose`].
    pub churn: i64,
    /// Churn discounted as noise, by the same rule `spine-3` prices size with. Filled by [`compose`].
    pub discounted_churn: i64,
    /// Churn a reviewer is expected to read: `churn - discounted_churn`. Filled by [`compose`].
    pub reviewable_churn: i64,
    /// Whether the file rows the model was built from were capped by the read.
    pub files_truncated: bool,
    /// Grouped file rows the read returned. May be fewer than `n_files` when the read capped them.
    pub files_read: i64,
    /// Whether the rows seen sum to the episode counts. Filled by [`compose`]; `None` when the cap bound and
    /// the question cannot be answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciles: Option<bool>,
}

impl Totals {
    /// The discount rule's name, printed beside any figure that used it.
    ///
    /// It names `spine-1` and keeps naming it after `spine-2` arrived (`reco-l9bz.7`), on purpose. The
    /// token is *data*, not a version stamp: it is written into every delta document this build has
    /// already emitted, into the gap sentence beside it and into the goldens that recorded it, and the
    /// rule it cites — three quarters of noisy churn discounted, by path class — did not change when a
    /// sixth component joined the ranking. Renaming it would make two documents describe one discount
    /// under two names, and a reader could not tell that apart from the discount changing.
    pub const DISCOUNT_RULE: &'static str = "spine-1/noisy-path";

    /// The fraction of noisy churn still counted, as a percent — printed so the discount is checkable.
    pub const DISCOUNT_KEEP_PERCENT: u64 = 25;
}

// ---------------------------------------------------------------------------
// complexity: the figure that lives beside the line counts
// ---------------------------------------------------------------------------

/// How many columns the narrowest card has, which is how long the one-line form of a gap may be.
///
/// ADR-006 §5.1 gives the timeline card thirty-five columns, and `e2.10`'s rule — inherited by this model
/// along with the rest of the card's discipline — is that the line a card prints is written where the fact
/// is written. So the width limit belongs to the model that composes the sentence and not to whichever pane
/// is drawing today, and [`EpisodeDelta::verify`] enforces it on the one line in this document whose whole
/// sentence has to fit: the complexity gap.
pub const CARD_COLUMNS: usize = 35;

/// The label of the complexity figure, spelled once (ADR-007 §10.1: one word, in full, lowercase).
///
/// The card draws its row under this name, `fathom analyze` prints it under this name and the export
/// links it under this name. A second spelling in a second surface is exactly the collision §10.2 refuses,
/// and a pane that matches on a literal copy of the word is a pane that silently stops drawing the row the
/// day the model rewords it.
pub const COMPLEXITY_LABEL: &str = "complexity";

/// What one episode did to the complexity of the files it touched, as the document carries it
/// (`e8.5`, ADR-007 §5.2–§5.4).
///
/// This is the same *kind* of thing [`Totals`] is: a roll-up over stored rows, arithmetic rather than a
/// computation, sitting beside the `+add`/`−del` counters it qualifies so that a surface never has to
/// decide for itself what "harder to follow" means. The store answers it as a join
/// ([`fathom_store::spine::Store::episode_complexity`] in the store's words); the document then words
/// it, because the three things a figure has to travel with — its absolutes, its coverage and the reason
/// codes for what was never counted — are the model's business, not the read's.
///
/// * [`ComplexityView::delta`] is the review number: `head − base` summed over the paths whose two sides
///   were counted under one basis. It is signed and never bare — `+7`, `−3`, `±0` (ADR-007 §10.1).
/// * [`ComplexityView::absolute`] is a property of the touched paths at head and nothing wider
///   (ADR-007 §8.1 refuses the repository-wide total this would otherwise become).
/// * [`ComplexityView::coverage`] is part of the figure, never a footnote under it (ADR-007 §5.4): a floor
///   printed without its denominator is a number whose reliability a reader cannot see.
/// * [`ComplexityView::reasons`] is the §7.1 vocabulary, per path: a file that was never parsed has no
///   figure, and the code says which kind of never that was.
///
/// `measured: false` is the shape ADR-007 §5.4 insists on — "zero coverage is not a zero delta" — and it is
/// why this block carries its own [`ComplexityView::gap`]: the `what` section has one gap slot and it is
/// already spent on the noise discount, so the card's thirty-five-column line about *this* silence lives
/// beside the figure it describes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ComplexityView {
    /// The basis tokens the figures were counted under (`mccabe-1` for anything this build writes).
    /// More than one is ADR-007 §9.5's `mixed_basis` episode: two rule sets, one episode, no sum.
    pub basis: Vec<String>,
    /// Σ(`head − base`) over the paths whose two sides were both counted under one basis. `None` means
    /// *no* path could be subtracted, which prints `?` and never `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<i64>,
    /// Σ of the head figures over the paths this episode touched. Not a repository figure, and never one
    /// that can be added down a rail (ADR-007 §8.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute: Option<i64>,
    /// The largest head figure over the episode's paths — a max, never a sum (ADR-007 §5.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    /// Paths in scope: the denominator of every coverage number in this block.
    pub touched: i64,
    /// Paths whose two sides could be subtracted, and so are inside `delta`.
    pub measured_paths: i64,
    /// Paths that could not, each tallied in [`ComplexityView::reasons`].
    pub unmeasurable_paths: i64,
    /// Why each unmeasurable path could not be, by ADR-007 §7.1 code, counted once per path.
    pub reasons: BTreeMap<String, i64>,
    /// Whether [`ComplexityView::delta`] holds a number. `false` is printed `?` with its reason and never
    /// as a zero — ADR-007 §5.4's sentence, in a field.
    pub measured: bool,
    /// One of the three states, in the same three spellings every section uses.
    pub state: String,
    /// `measured_paths / touched`, absent when there is no denominator (ADR-007 §5.4: `None` and `0 %`
    /// are different claims).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<f64>,
    /// The gap for a figure nothing measured: the thirty-five-column line a card prints, the plan key that
    /// owns the answer, and the command that fills it when one exists. `None` whenever there is a figure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Gap>,
}

impl ComplexityView {
    /// The dominant refusal: the code that refused the most paths, ties broken by name so two runs over
    /// one store write one string.
    #[must_use]
    pub fn dominant_reason(&self) -> Option<&str> {
        let mut rows: Vec<(&String, &i64)> = self.reasons.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        rows.first().map(|(code, _)| code.as_str())
    }

    /// The sentence a surface prints instead of the figure: the tally, the dominant code's own wording,
    /// and the command that would fill it. Built here because every surface that prints `?` owes the same
    /// sentence (ADR-006 §10.3: a shrug is not a remedy).
    #[must_use]
    pub fn unknown_sentence(&self) -> String {
        if self.reasons.is_empty() {
            return if self.touched == 0 {
                "this episode touched no path the complexity pass could see, so there was nothing to \
                 count"
                    .to_owned()
            } else {
                format!(
                    "{} path(s) were in scope, none produced a figure, and no reason was recorded for \
                     any of them",
                    grouped(self.touched)
                )
            };
        }
        let mut text = format!(
            "{} of {} path(s) in this episode had no figure ({}) — {}",
            grouped(self.unmeasurable_paths),
            grouped(self.touched),
            reasons_phrase(&self.reasons),
            self.dominant_reason()
                .map_or_else(String::new, |code| refusal_long(code).to_owned())
        );
        if self.basis.len() > 1 {
            text.push_str(&format!(
                "; this episode carries {} basis tokens ({}), which are not comparable with each other",
                self.basis.len(),
                self.basis.join(", ")
            ));
        }
        if let Some(run) = self.gap.as_ref().and_then(|gap| gap.remedy.as_deref()) {
            text.push_str(&format!("; `{run}` is what fills it"));
        }
        text
    }
}

/// Every ADR-007 §7.1 code this table words, in one list a test can walk.
///
/// The point of naming them is that the vocabulary is closed and shared: `tests/complexity_delta.rs` fails
/// unless this list and [`crate::structure::UnavailableReason::vocabulary`] hold exactly the
/// same codes, which is what stops a code from arriving here without a card line of its own.
pub const REFUSAL_CODES: &[&str] = &[
    "absent_revision",
    "binary",
    "body_not_visible",
    "column_not_filled",
    "contents_not_kept",
    "extractor_not_shipped",
    "grammar_not_compiled",
    "language_unknown",
    "mixed_basis",
    "no_bodies",
    "node_budget",
    "not_utf8",
    "parse_failed",
    "parser",
    "rename_unpaired",
    "scan_budget",
    "too_large",
];

/// The §7.1 table, in the four columns a surface needs: the phrase that fits beside the word
/// `complexity` on a card, the phrase that explains it, the plan key that owns the answer, and the
/// command that fills it when a command exists.
///
/// The codes are the strings `structure::UnavailableReason` writes and `fathom_store::complexity` stores —
/// one vocabulary for "this has no figure", as ADR-007 §7.1 demands, not a second one invented here. The
/// short column is written to a budget: `complexity: <short> (<owner>)` has to fit the narrowest card
/// ([`CARD_COLUMNS`] columns), and [`EpisodeDelta::verify`] refuses anything longer, so a new code cannot
/// arrive with a sentence that gets cut.
fn refusal(
    code: &str,
) -> (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
) {
    match code {
        "column_not_filled" => (
            "not measured yet",
            "this index has not filled the complexity column for these rows yet",
            "e8.3",
            Some("fathom index"),
        ),
        "scan_budget" => (
            "over scan budget",
            "the per-run complexity budget was spent before these paths were reached; the key that ran out \
             is named by `fathom doctor`",
            "e8.4",
            Some("fathom index"),
        ),
        "too_large" => (
            "file too large",
            "the file is over the byte cap the complexity pass will read",
            "e8.4",
            Some("fathom index"),
        ),
        "contents_not_kept" => (
            "bytes not kept",
            "the object database kept no contents for one side of the comparison, so nothing could be \
             counted on it",
            "e8.4",
            Some("fathom index --refresh"),
        ),
        "mixed_basis" => (
            "two rule sets",
            "the two sides were counted under different rule sets, so subtracting them would be arithmetic \
             on unlike units",
            "e8.5",
            Some("fathom index --rebuild"),
        ),
        "rename_unpaired" => (
            "rename unpaired",
            "a rename was guessed rather than asserted, so its two sides were not treated as one file",
            "e2.4",
            None,
        ),
        "binary" => (
            "git says binary",
            "git reports the file as binary, so its contents were never read",
            "e8.5",
            None,
        ),
        "not_utf8" => (
            "not text",
            "the file is not text in the part the extractor would read",
            "e8.5",
            None,
        ),
        "parse_failed" => (
            "parse failed",
            "the parse reported more error than code and returned no structure",
            "e8.2",
            None,
        ),
        "language_unknown" => (
            "no grammar",
            "the file is not one of the grammars fathom reads",
            "e8.5",
            None,
        ),
        "grammar_not_compiled" => (
            "no grammar built",
            "this build was compiled without the grammar that file needs",
            "e3.1",
            None,
        ),
        "extractor_not_shipped" => (
            "no extractor yet",
            "that language's complexity extractor has not shipped yet",
            "e3.1",
            None,
        ),
        "parser" => (
            "no parser",
            "the grammar was there and the parser could not be handed to it",
            "e8.2",
            None,
        ),
        "body_not_visible" => (
            "body elsewhere",
            "the unit is a declaration with no body here, so its branches are in another file",
            "e8.2",
            None,
        ),
        "node_budget" => (
            "count cut short",
            "the per-unit node budget ran out mid-body, so the figure is a floor and a floor does not \
             print bare",
            "e8.2",
            None,
        ),
        "no_bodies" => (
            "nothing callable",
            "the file has no callable unit in it at all, so there was nothing to count",
            "e8.2",
            None,
        ),
        "absent_revision" => (
            "new path",
            "the path did not exist on the other side of the comparison, so there was nothing to subtract",
            "e8.3",
            None,
        ),
        _ => (
            "nothing counted",
            "the pass recorded neither a figure nor a reason for these paths",
            "e8.5",
            None,
        ),
    }
}

/// The explaining phrase for a reason code.
fn refusal_long(code: &str) -> &'static str {
    refusal(code).1
}

/// The reason tally as one value fragment: `too_large 2 · binary 1`, capped, with the cut counted.
fn reasons_phrase(reasons: &BTreeMap<String, i64>) -> String {
    if reasons.is_empty() {
        return "no reason was recorded".to_owned();
    }
    let mut rows: Vec<(&String, &i64)> = reasons.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let listed: Vec<String> = rows
        .iter()
        .take(ROLLUP_CAP)
        .map(|(code, count)| format!("{code} {count}"))
        .collect();
    let rest = rows.len().saturating_sub(ROLLUP_CAP);
    if rest > 0 {
        format!("{} · {rest} more", listed.join(" · "))
    } else {
        listed.join(" · ")
    }
}

/// The signed spelling ADR-007 §10.1 fixes for a complexity delta: `+7`, `−3`, `±0`.
///
/// The sign is a glyph rather than a hue because ADR-006 §10.2 refuses to let colour carry a fact alone,
/// and `±0` is a different claim from `?` in a monochrome terminal as surely as it is in JSON.
#[must_use]
pub fn complexity_signed(value: i64) -> String {
    match value {
        0 => "±0".to_owned(),
        value if value > 0 => format!("+{}", grouped(value)),
        value => format!(
            "−{}",
            grouped(i64::try_from(value.unsigned_abs()).unwrap_or(i64::MAX))
        ),
    }
}

/// The one string every surface prints for a measured complexity figure (ADR-007 §10.1's three states).
///
/// `?` is deliberately not produced here: an unmeasured episode is an *unknown figure*, whose `?` and its
/// reason arrive together through [`Figure::unknown`], and a surface that pasted its own `?` over this
/// function's empty string would be the second opinion this model exists to forbid.
#[must_use]
pub fn complexity_headline(view: &ComplexityView) -> String {
    let Some(delta) = view.delta else {
        return String::new();
    };
    let signed = complexity_signed(delta);
    let mut marks: Vec<String> = Vec::new();
    if view.unmeasurable_paths > 0 {
        // A floor says what it is a floor of: the mark, the coverage the sum was taken over, and the codes
        // for the paths that are not in it (ADR-007 §5.4).
        marks.push(format!(
            "{} of {} measured · {} unmeasured: {}",
            grouped(view.measured_paths),
            grouped(view.touched),
            grouped(view.unmeasurable_paths),
            reasons_phrase(&view.reasons),
        ));
    }
    if view.basis.len() > 1 {
        // §9.5: the two tokens are not comparable with each other, and the sum carries that warning rather
        // than pretending to be one number.
        marks.push(format!("mixed basis: {}", view.basis.join(" vs ")));
    }
    if marks.is_empty() {
        signed
    } else {
        format!("~{signed} ({})", marks.join(" · "))
    }
}

/// The complexity figure for the `what` section: the one `complexity`-labelled claim every surface prints.
///
/// It goes through the evidence-carrying constructor like every other figure in this document, and its
/// pointer resolves into the block beside it, so `verify` can — and does — fail the document when the
/// printed number and the roll-up disagree.
fn complexity_figure(view: &ComplexityView) -> Figure {
    if let (true, Some(delta)) = (view.measured, view.delta) {
        let mut raw: Vec<(&str, i64)> = vec![
            ("delta", delta),
            ("measured", view.measured_paths),
            ("unmeasurable", view.unmeasurable_paths),
            ("coverage_measured", view.measured_paths),
            ("coverage_total", view.touched),
        ];
        if let Some(absolute) = view.absolute {
            raw.push(("absolute", absolute));
        }
        if let Some(max) = view.max {
            raw.push(("max", max));
        }
        return Figure::new(
            COMPLEXITY_LABEL,
            complexity_headline(view),
            Evidence::inside("#/complexity"),
        )
        .with_raw(&raw);
    }
    // The unknown shape, with its owner and its remedy attached to the evidence as well as to the gap, so
    // a consumer that reads figures and not gaps still gets pointed at the verb that answers.
    let owner = view.gap.as_ref().map_or_else(
        || SectionId::What.owner().to_owned(),
        |gap| gap.owner.clone(),
    );
    let evidence = Evidence {
        pointer: Some("#/complexity".to_owned()),
        // Only a command that *fills* the figure belongs here. The verb that prints this document is not
        // evidence of anything, and a `run` the reviewer can run without learning more is a decoration.
        run: view.gap.as_ref().and_then(|gap| gap.remedy.clone()),
        owner: Some(owner),
    };
    Figure::unknown(COMPLEXITY_LABEL, view.unknown_sentence(), evidence)
}

/// The gap for a figure nothing measured: the thirty-five-column line, its owner and its remedy.
fn complexity_gap(view: &ComplexityView) -> Gap {
    let code = view.dominant_reason().unwrap_or_default();
    let (short, _, owner, remedy) = refusal(code);
    Gap {
        section: SectionId::What.as_str().to_owned(),
        short: format!("{COMPLEXITY_LABEL}: {short}"),
        reason: view.unknown_sentence(),
        owner: owner.to_owned(),
        remedy: remedy.map(str::to_owned),
    }
}

/// Which analysis produced the complexity figures, in the words `provenance.models` uses.
///
/// The basis token is part of the answer, not a detail of it: `mccabe-1` and `cc-shannon-1` count different
/// things and a figure is only comparable with a figure counted under the same token (ADR-007 §9.5).
fn complexity_model_name(view: &ComplexityView) -> String {
    let basis = if view.basis.is_empty() {
        "no basis recorded".to_owned()
    } else {
        view.basis.join("+")
    };
    format!("structure::complexity ({basis})")
}

/// The state a complexity roll-up is in — one rule, used both when the document is built and when it is
/// checked, because two copies of "when is this a partial answer" is two answers (ADR-001 §7.10).
///
/// A mixed basis caps the state at `partial` even when every path was counted: two tokens in one sum is
/// arithmetic on unlike units (ADR-007 §9.5), and a figure that has to be read with a warning is not a
/// measured one.
fn complexity_state(view: &ComplexityView) -> &'static str {
    if view.measured_paths > 0 && view.unmeasurable_paths == 0 && view.basis.len() <= 1 {
        STATE_MEASURED
    } else if view.measured_paths > 0 || view.basis.len() > 1 {
        STATE_PARTIAL
    } else {
        STATE_UNAVAILABLE
    }
}

/// Fill the fields this model derives from the counted ones: the state, the coverage and the gap.
///
/// Split out of [`compose`] because a surface that has a roll-up and no document — `fathom show`, which is
/// a commit card rather than a delta — has to reach the same three conclusions, and two sets of rules
/// about when a figure is a floor would be two answers (ADR-001 §7.10).
fn settle_complexity(view: &mut ComplexityView) {
    view.measured = view.delta.is_some();
    view.state = complexity_state(view).to_owned();
    view.coverage = if view.touched > 0 {
        Some(
            f64::from(u32::try_from(view.measured_paths).unwrap_or(u32::MAX))
                / f64::from(u32::try_from(view.touched).unwrap_or(u32::MAX)),
        )
    } else {
        None
    };
    view.gap = if view.measured {
        None
    } else {
        // The gap's own sentence is the same sentence the figure prints, so it is taken *after* the gap
        // exists: the remedy clause reads the gap's remedy, and a gap written before it would silently
        // drop the one part of the answer a reviewer can act on (ADR-006 §10.3).
        let mut gap = complexity_gap(view);
        gap.reason = view.unknown_sentence();
        Some(gap)
    };
}

/// Hand the document's complexity figure to the ranking (`reco-l9bz.7`).
///
/// A translation, not a recomputation: every field comes from the same roll-up the card prints, because
/// ADR-001 §7.10 allows one figure of an episode's complexity and not one per surface. The ranking's
/// [`risk::Complexity`] is the same numbers with the ranking's own field names, and the reason codes
/// arrive tally-ordered so the printed sentence is stable.
fn risk_complexity(view: &ComplexityView) -> risk::Complexity {
    let mut reasons: Vec<(String, usize)> = view
        .reasons
        .iter()
        .map(|(code, paths)| (code.clone(), usize::try_from(*paths).unwrap_or(usize::MAX)))
        .collect();
    reasons.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    risk::Complexity {
        delta: view.delta,
        absolute: view.absolute,
        max: view.max,
        measured: usize::try_from(view.measured_paths.max(0)).unwrap_or(usize::MAX),
        touched: usize::try_from(view.touched.max(0)).unwrap_or(usize::MAX),
        reasons,
        mixed_basis: view.basis.len() > 1,
    }
}

/// One term of the ranking, with the sentence that justifies it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskComponent {
    /// The component name (`surfaces`, `complexity`, `size`, `deletions`, `breadth`).
    pub name: String,
    /// What it contributed.
    pub points: i64,
    /// What it could have contributed.
    pub cap: i64,
    /// The checkable claim: sizes, paths, counts — never an adjective.
    pub detail: String,
}

/// The ranking, in the form every surface prints.
///
/// `reco-c05` computes the number; this is the same feature vector with its blind spots attached, so the
/// card, the rail, the artefact and the MCP tool print one ranking rather than four close approximations
/// of it.
///
/// It carries no *place* in a ranking. `risk.rank`/`risk.ranked` — the `rank N of M` a card row used to
/// print — went with the ranked set in `e10.3` (ADR-006 §5.1.2 §5): the CLI scores one episode and a pane
/// that no longer ranks a set cannot cite a place in one. That removal is why the contract is v2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskView {
    /// Which ranking produced the number (`spine-3` today; `e4.4`'s scorer replaces the inputs, not this).
    pub model: String,
    /// The score: `0..=92` under `spine-3`, whose caps sum to `risk::SCORE_CEILING`.
    pub score: i64,
    /// `spine-3 88 = surfaces 50 + size 12 + deletions 10 + breadth 8 + complexity 8`.
    pub vector: String,
    /// The components that contributed, largest first — the feature vector, in order.
    pub components: Vec<RiskComponent>,
    /// The surfaces touched, most consequential first.
    pub surfaces: Vec<String>,
    /// The caveat when the file list was capped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caveat: Option<String>,
    /// What the ranking cannot see, in the product's own words — printed with every ranking.
    pub blind_spots: Vec<String>,
}

impl RiskView {
    /// The ranking [`crate::risk`] computed, in the shape every surface prints.
    #[must_use]
    pub fn of(ranking: &risk::Risk) -> Self {
        RiskView {
            model: risk::MODEL.to_owned(),
            score: i64::from(ranking.score),
            vector: ranking.vector(),
            components: ranking
                .components
                .iter()
                .map(|component| RiskComponent {
                    name: component.name.to_owned(),
                    points: i64::try_from(component.points).unwrap_or(i64::MAX),
                    cap: i64::try_from(component.cap).unwrap_or(i64::MAX),
                    detail: component.detail.clone(),
                })
                .collect(),
            surfaces: ranking
                .surfaces
                .iter()
                .map(|surface| surface.as_str().to_owned())
                .collect(),
            caveat: ranking.caveat(),
            blind_spots: risk::BLIND_SPOTS
                .iter()
                .map(|spot| (*spot).to_owned())
                .collect(),
        }
    }
}

/// Where the rows came from, so a figure can be traced to a read rather than to a guess.
///
/// `store` is the *redacted* path — the composition root runs it through `fathom-cli`'s redactor before it
/// gets here, because an absolute path in an exported artefact is a host leak (`ADR-001 §7.5`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// `store`, `fixture`, `synthetic` — the kind of source, not a path.
    pub source: String,
    /// The redacted store path, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    /// The index epoch the rows came from, so a stale artefact can be recognised as stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<i64>,
    /// The handle the user typed, echoed beside what it resolved to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<String>,
    /// `fathom <version>`, so a payload says which build wrote it.
    pub produced_by: String,
    /// Whether the per-commit file rows were read (`full`) or the summed rollup was used (`rollup`).
    pub files_detail: String,
    /// Which models produced which numbers, so a printed name is never the only place a model is recorded.
    pub models: BTreeMap<String, String>,
}

impl Provenance {
    /// The `files_detail` spelling for a per-commit file read.
    pub const DETAIL_FULL: &'static str = "full";
    /// The `files_detail` spelling for the summed per-path rollup a rail page uses.
    pub const DETAIL_ROLLUP: &'static str = "rollup";

    /// The [`Provenance::produced_by`] string this build prints.
    #[must_use]
    pub fn produced_by() -> String {
        format!("fathom {}", env!("CARGO_PKG_VERSION"))
    }

    /// The default model map: what produced which number in this build.
    #[must_use]
    pub fn models() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("risk".to_owned(), risk::MODEL.to_owned()),
            ("surface".to_owned(), "risk::classify".to_owned()),
            ("discount".to_owned(), Totals::DISCOUNT_RULE.to_owned()),
            ("taxonomy".to_owned(), "stored-path-class".to_owned()),
        ])
    }
}

/// An analysis the caller lends, because it is computed somewhere this read cannot reach.
///
/// The delta model does not recompute `e2.6`'s dependency delta or `e2.9`'s test names; a caller that has
/// them hands them over and the section is `measured` instead of `unavailable`. Every field is optional, and
/// a section built from a partial hand-over is `partial` with the missing keys named — never padded.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Supplied {
    /// `e2.6`'s dependency headline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<SuppliedBlock>,
    /// `e2.9`'s test-surface headline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<SuppliedBlock>,
    /// `e3.2`'s blast radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blast: Option<SuppliedBlock>,
    /// `e2.8`'s non-code findings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<SuppliedBlock>,
    /// `e2.3`'s structure sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure: Option<SuppliedBlock>,
    /// `e4.3`'s detector findings, for the attention section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
}

/// A hand-over from an analysis this build cannot run: a headline, the basis it was computed on, and the
/// figures it is claiming, already formatted by the analysis that owns them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SuppliedBlock {
    /// The one-sentence answer, in the owning analysis' own words.
    pub headline: String,
    /// What the answer was computed from (`module graph`, `resolved imports`, `token comparison`, …).
    pub basis: String,
    /// The figures it claims, keyed and sorted. The delta copies them and points its own evidence at
    /// this block, so a supplied figure is distinguishable from a measured one.
    #[serde(default)]
    pub figures: BTreeMap<String, String>,
    /// What the analysis is confident in, when it has a number for that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// The owning ticket, for the pointer.
    pub owner: String,
    /// A command the reviewer can run and get the same answer from, when one exists.
    ///
    /// A hand-over is the one case where a figure's evidence is not inside this document, so this is the
    /// difference between "the card trusts another analysis" and "the card trusts another analysis and
    /// says where to check it" (ADR-001 §7.9). A block without it is still a measured section; it just
    /// points at its owner and nowhere further.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

impl Supplied {
    /// The hand-over with `e2.6`'s dependency delta lent to it.
    ///
    /// The one construction of a [`SuppliedBlock`] this crate offers, because the alternative is every
    /// surface inventing its own sentence for the same delta — which is what `e2.11` exists to stop.
    #[must_use]
    pub fn with_dependencies(mut self, block: SuppliedBlock) -> Self {
        self.dependencies = Some(block);
        self
    }
}

/// Move `e2.6`'s dependency delta for one episode into this model's hand-over shape (`e2.11` §4).
///
/// Nothing is recomputed. The sentence is `e2.6`'s, the counts are `e2.6`'s, and so is the refusal to call
/// a file it could not read a file that changed — which is the whole point: the card's dependency figure and
/// `fathom check --only dependencies` then report one measurement instead of two opinions of it. What this
/// adds is the *basis* — which two revisions were compared, which files were read, and what could not be
/// read — because a figure whose evidence is another analysis has to name what it is talking about or
/// nobody can go and check it (ADR-001 §7.9).
///
/// `from` and `to` are how the caller spells the two revisions, so the printed basis matches the handles the
/// reviewer typed; the run command is built from the same pair, so clicking the number re-runs the thing
/// that made it.
#[must_use]
pub fn dependency_block(
    episode: &crate::deps::DependencyEpisode,
    from: &str,
    to: &str,
) -> SuppliedBlock {
    let model = &episode.model;
    let counts = &model.counts;
    let spell = |list: &mut Vec<String>, count: usize, word: &str| {
        if count > 0 {
            list.push(format!("{count} {word}"));
        }
    };
    let mut figures: BTreeMap<String, String> = BTreeMap::new();
    // The command that reproduces this answer, from the same two handles the basis prints: a figure whose
    // evidence is another analysis has to leave the reviewer somewhere to check it (ADR-001 §7.9).
    let run = format!("fathom check --only dependencies --base {from} --head {to}");
    if episode.paths.totals.files == 0 {
        // Nothing dependency-shaped changed, and the warm path reads the changed paths only — so the two
        // states it compares are empty because they were never populated, not because the repository
        // depends on nothing. Printing "0 → 0 packages" here would be a number whose evidence is an empty
        // read, which is the one way this section can lie (ADR-001 §7.7).
        figures.insert(
            "dependency files".to_owned(),
            "none — no manifest or lockfile moved".to_owned(),
        );
        figures.insert(
            "graph".to_owned(),
            "not read: nothing dependency-shaped changed in this episode".to_owned(),
        );
        return SuppliedBlock {
            headline: model.headline.clone(),
            basis: format!(
                "read between {from} and {to}; no manifest or lockfile was touched, so the graph \
                 was not opened"
            ),
            figures,
            confidence: None,
            owner: SectionId::Dependencies.owner().to_owned(),
            run: Some(run),
        };
    }

    // Direct first: the question a reviewer asks is "what did this commit *declare*", and the transitive
    // movement that follows from it is a separate answer (`e2.6`'s own ordering, kept here).
    let mut direct: Vec<String> = Vec::new();
    spell(&mut direct, counts.direct_added, "added");
    spell(&mut direct, counts.direct_removed, "removed");
    spell(&mut direct, counts.direct_upgraded, "upgraded");
    spell(&mut direct, counts.direct_downgraded, "downgraded");
    spell(
        &mut direct,
        counts.direct_unordered + counts.direct_requalified,
        "moved",
    );
    spell(&mut direct, counts.unattributed, "not attributable");
    figures.insert(
        "direct".to_owned(),
        if direct.is_empty() {
            "nothing declared moved".to_owned()
        } else {
            direct.join(" · ")
        },
    );

    let mut transitive: Vec<String> = Vec::new();
    spell(&mut transitive, counts.transitive_added, "added");
    spell(&mut transitive, counts.transitive_removed, "removed");
    spell(&mut transitive, counts.transitive_upgraded, "upgraded");
    spell(&mut transitive, counts.transitive_downgraded, "downgraded");
    spell(&mut transitive, counts.transitive_unordered, "moved");
    if !transitive.is_empty() && counts.unexplained_transitive > 0 {
        transitive.push(format!(
            "{} with no changed direct dependency behind them",
            counts.unexplained_transitive
        ));
    }
    figures.insert(
        "transitive".to_owned(),
        if transitive.is_empty() {
            "the resolved graph did not move".to_owned()
        } else {
            transitive.join(" · ")
        },
    );

    let size = &model.graph.total;
    let mut graph = format!(
        "{} → {} resolved package(s), net {:+}",
        grouped(i64::try_from(size.before).unwrap_or(i64::MAX)),
        grouped(i64::try_from(size.after).unwrap_or(i64::MAX)),
        size.net
    );
    if model.graph.per_ecosystem.len() > 1 {
        let ecologies: Vec<String> = model
            .graph
            .per_ecosystem
            .iter()
            .take(ROLLUP_CAP)
            .map(|(ecosystem, one)| format!("{ecosystem} {}→{}", one.before, one.after))
            .collect();
        graph.push_str(&format!(" ({})", ecologies.join(", ")));
    }
    figures.insert("graph".to_owned(), graph);

    if !model.sources.is_empty() || !model.sources_gone.is_empty() {
        let named: Vec<String> = model
            .sources
            .iter()
            .take(ROLLUP_CAP)
            .map(|source| match &source.via {
                Some(package) => format!("{} ({package})", source.identity),
                None => source.identity.clone(),
            })
            .collect();
        let mut sources = format!(
            "{} new source(s){}",
            model.sources.len(),
            if named.is_empty() {
                String::new()
            } else {
                format!(": {}", named.join(", "))
            }
        );
        if !model.sources_gone.is_empty() {
            sources.push_str(&format!(", {} gone", model.sources_gone.len()));
        }
        figures.insert("sources".to_owned(), sources);
    }
    if counts.license_changes > 0
        || !model.licenses.added.is_empty()
        || !model.licenses.removed.is_empty()
    {
        let mut licence = Vec::new();
        if counts.license_changes > 0 {
            licence.push(format!(
                "{} changed on an unchanged version",
                counts.license_changes
            ));
        }
        if !model.licenses.added.is_empty() {
            licence.push(format!("{} new to the graph", model.licenses.added.len()));
        }
        if !model.licenses.removed.is_empty() {
            licence.push(format!("{} gone", model.licenses.removed.len()));
        }
        figures.insert("licence".to_owned(), licence.join(" · "));
    }

    // Never silently hidden: a lockfile this build could not parse is a hole in the answer above, and the
    // hole is printed with the parser's own advice rather than folded into a tidy count.
    if let Some(unread) = episode.paths.unparsed.first() {
        let advice = match &unread.body {
            crate::deps::Body::Unparsed(unparsed) => unparsed.advice().to_owned(),
            _ => "it parsed, and the delta still did not cover it".to_owned(),
        };
        figures.insert(
            "not read".to_owned(),
            format!(
                "{} file(s) could not be parsed — {} ({}: {})",
                episode.paths.unparsed.len(),
                advice,
                unread.path,
                unread.ecosystem.as_str()
            ),
        );
    } else if !model.complete || !episode.paths.complete {
        figures.insert(
            "not read".to_owned(),
            "one side of the comparison was read only partially".to_owned(),
        );
    }

    let files: Vec<String> = episode
        .paths
        .paths
        .iter()
        .take(ROLLUP_CAP)
        .map(|row| format!("{} ({})", row.path, row.ecosystem.as_str()))
        .collect();
    let remainder = episode.paths.paths.len().saturating_sub(files.len());
    let mut basis = format!(
        "{} dependency file(s) read between {from} and {to}{}",
        episode.paths.totals.files,
        if files.is_empty() {
            " — no manifest or lockfile changed".to_owned()
        } else {
            format!(
                ": {}{}",
                files.join(", "),
                if remainder > 0 {
                    format!(" +{remainder} more")
                } else {
                    String::new()
                }
            )
        }
    );
    if !model.complete {
        basis.push_str("; one side was read only partially");
    }
    if let Some(note) = model.notes.first() {
        basis.push_str(&format!("; {note}"));
    }

    SuppliedBlock {
        headline: model.headline.clone(),
        basis,
        figures,
        confidence: None,
        owner: SectionId::Dependencies.owner().to_owned(),
        run: Some(run),
    }
}

/// One finding, in the shape every surface prints it: kind, subject, message, severity, evidence.
///
/// `e4.3` owns the detector API and the finding schema; this is the subset the delta carries so that the
/// attention section can be filled by a caller that has findings without waiting for that ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Namespaced kind, e.g. `env-renamed`, `test-deleted-without-coverage`.
    pub kind: String,
    /// The path or symbol it is about.
    pub subject: String,
    /// The claim, in one sentence.
    pub message: String,
    /// `high`, `notable` or `note` — the sub-analyses' own spellings, not a fourth one.
    pub severity: String,
    /// Where the evidence for it is, in this document or outside it.
    pub evidence: Evidence,
    /// The analysis that raised it.
    pub owner: String,
}

/// The document's own verdict on itself.
///
/// Travels inside the payload: an artefact that disagrees with itself says so where the disagreement is,
/// rather than three weeks later in a bug report. `e2.10` audits its renders the same way and then tampers
/// with them to prove the auditor is awake (`tests/test_delta_contract.py` does the same for this one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Integrity {
    /// Whether [`EpisodeDelta::verify`] found nothing.
    pub verified: bool,
    /// One line per problem found, empty when `verified`.
    pub problems: Vec<String>,
    /// What was checked, so the claim is not a bare boolean.
    pub checks: Vec<String>,
}

/// The versioned delta model: one episode, as one document, for every surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeDelta {
    /// [`CONTRACT`].
    pub contract: String,
    /// [`VERSION`].
    pub schema_version: u32,
    /// Whose episode this is.
    pub episode: EpisodeRef,
    /// The scale, measured and derived.
    pub totals: Totals,
    /// What the episode did to the complexity of the files it touched, beside the line counts it belongs
    /// with (`e8.5`, ADR-007 §5.2). `None` means no roll-up was read for this document, which is a
    /// different claim from a figure of zero and prints differently: no row, rather than a `?`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<ComplexityView>,
    /// The rows the figures are derived from, in path order.
    pub files: Vec<FileDelta>,
    /// The seven sections, in [`SectionId::ALL`]'s order. A newer producer's extra sections are ignored on
    /// read, never fatal.
    pub sections: Vec<Section>,
    /// The ranking, or `None` when nothing ranked this episode — which is a different claim from rank 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskView>,
    /// Every gap in one list, section order. Redundant with [`Section::gap`] on purpose: the artefact and the
    /// MCP tools want "what is missing" without walking the sections, and one list means one answer.
    pub gaps: Vec<Gap>,
    /// Where the rows came from.
    pub provenance: Provenance,
    /// The self-check.
    pub integrity: Integrity,
}

/// Everything [`compose`] is allowed to know: rows already read, plus whatever the caller can lend.
///
/// It is rows and not a store handle because the façade owns the read and the surfaces own nothing: a rail
/// page's five hundred deltas are built from the four queries `fathom-cli` already runs, and a test's delta
/// is built from a table. Deserialize is on it so a fixture is a JSON file and not a Rust literal: the
/// fixture corpus is then input-shaped, and the blessed output next to it is what the model produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Input {
    /// The identity, as the spine recorded it.
    pub episode: EpisodeRef,
    /// The file rows. Empty means the index recorded none, which is a stated unknown, not a clean episode.
    pub files: Vec<FileDelta>,
    /// The spine counts and the read's own caps. The derived fields are recomputed by [`compose`].
    pub totals: Totals,
    /// The complexity roll-up, when the read had one. `compose` derives its state and its gap; a reader
    /// that hands over `None` gets no complexity figure, which is the difference between "not read here"
    /// and "nothing could be counted".
    pub complexity: Option<ComplexityView>,
    /// The ranking, if anything ranked this episode.
    pub risk: Option<RiskView>,
    /// Where the rows came from.
    pub provenance: Provenance,
    /// Analyses the caller lends.
    pub supplied: Supplied,
}

impl Input {
    /// An input with the boilerplate filled in: this build's provenance, no rows, nothing lent.
    #[must_use]
    pub fn new(episode: EpisodeRef) -> Self {
        Input {
            episode,
            files: Vec::new(),
            totals: Totals::default(),
            complexity: None,
            risk: None,
            provenance: Provenance {
                source: "synthetic".to_owned(),
                store: None,
                epoch: None,
                asked: None,
                produced_by: Provenance::produced_by(),
                files_detail: Provenance::DETAIL_ROLLUP.to_owned(),
                models: Provenance::models(),
            },
            supplied: Supplied::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// composing a delta
// ---------------------------------------------------------------------------

/// One row of a rollup: how many files, and what they did.
#[derive(Debug, Clone, Default)]
struct Rollup {
    files: i64,
    add: i64,
    del: i64,
}

impl Rollup {
    fn churn(&self) -> i64 {
        self.add.saturating_add(self.del)
    }
}

/// Group the file rows by a key, then order by churn descending and name ascending.
///
/// `BTreeMap` first so the *pre-sort* order is the key order and not a hash's: two runs over the same rows
/// must produce the same value string, or a golden is a coin flip.
fn rollup_by(files: &[FileDelta], key: impl Fn(&FileDelta) -> String) -> Vec<(String, Rollup)> {
    let mut map: BTreeMap<String, Rollup> = BTreeMap::new();
    for file in files {
        let entry = map.entry(key(file)).or_default();
        entry.files += 1;
        entry.add = entry.add.saturating_add(file.add);
        entry.del = entry.del.saturating_add(file.del);
    }
    let mut rows: Vec<(String, Rollup)> = map.into_iter().collect();
    rows.sort_by(|a, b| b.1.churn().cmp(&a.1.churn()).then(a.0.cmp(&b.0)));
    rows
}

/// Render a rollup as one value string, capping the rows and counting what was cut.
///
/// Returns `(value, remainder)`. A cut that is not counted is a lost fact, so the remainder travels with the
/// value rather than being mentioned and then dropped by the formatter.
fn render_rollup(rows: &[(String, Rollup)], cap: usize, show_churn: bool) -> (String, u64) {
    let shown: Vec<String> = rows
        .iter()
        .take(cap)
        .map(|(name, roll)| {
            if show_churn {
                format!("{name} {}f +{} −{}", roll.files, roll.add, roll.del)
            } else {
                format!("{name} {}", roll.files)
            }
        })
        .collect();
    let remainder = u64::try_from(rows.len().saturating_sub(cap)).unwrap_or(u64::MAX);
    (shown.join(" · "), remainder)
}

/// The directory a path landed in, as the `where` section groups it.
fn directory_of(path: &str) -> String {
    match path.rfind('/') {
        Some(index) if index > 0 => path[..index].to_owned(),
        _ => ".".to_owned(),
    }
}

/// Thousands-grouped, so a figure's value reads the same in the card, the CLI and the artefact.
#[must_use]
pub fn grouped(value: i64) -> String {
    let digits = value.abs().to_string();
    let mut out = String::new();
    for (index, byte) in digits.bytes().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(byte as char);
    }
    if value < 0 {
        out.insert(0, '-');
    }
    out
}

/// The `+120 −30` spelling the spine uses everywhere: an ASCII plus, a real minus.
#[must_use]
pub fn plus_minus(add: i64, del: i64) -> String {
    format!("+{} −{}", grouped(add), grouped(del))
}

/// Percent of `part` in `whole`, rounded, and `None` when there is no denominator to speak of.
fn percent(part: i64, whole: i64) -> Option<u64> {
    if whole <= 0 {
        return None;
    }
    let scaled = part.saturating_mul(100);
    Some(((scaled + whole / 2) / whole).max(0) as u64)
}

/// Build the delta from rows that were already read.
///
/// Pure: no clock, no environment, no read, no default that depends on the machine — the same input yields
/// byte-identical output, which is what makes the goldens goldens and the cache keys honest (`ADR-001 §7.4`).
#[must_use]
pub fn compose(input: &Input) -> EpisodeDelta {
    let files = &input.files;
    let mut totals = input.totals.clone();
    totals.churn = input.totals.n_add.saturating_add(input.totals.n_del);

    // The discount is `spine-3`'s, applied the same way, so "reviewable churn" means one thing in the score
    // and in the card. A noisy path still counts a quarter of its churn: noise is a view, never a deletion.
    let noisy: i64 = files
        .iter()
        .filter(|file| is_noisy(file))
        .map(FileDelta::churn)
        .fold(0_i64, i64::saturating_add);
    totals.discounted_churn = noisy.saturating_mul(3) / 4;
    totals.reviewable_churn = totals.churn.saturating_sub(totals.discounted_churn);
    totals.files_read = i64::try_from(files.len()).unwrap_or(i64::MAX);
    totals.reconciles = if totals.files_truncated || files.is_empty() {
        None
    } else {
        let add: i64 = files
            .iter()
            .map(|file| file.add)
            .fold(0, i64::saturating_add);
        let del: i64 = files
            .iter()
            .map(|file| file.del)
            .fold(0, i64::saturating_add);
        Some(add == input.totals.n_add && del == input.totals.n_del)
    };

    let full = input.provenance.files_detail == Provenance::DETAIL_FULL;
    // The complexity roll-up arrives counted and leaves worded: the state, the coverage and the gap are
    // derived here so that the document, not the read, decides what a floor and an absence mean.
    let complexity = input.complexity.as_ref().map(|view| {
        let mut view = view.clone();
        settle_complexity(&mut view);
        view
    });
    let sections = vec![
        what_section(files, &totals, full, complexity.as_ref()),
        where_section(files, &totals, full, &input.supplied),
        dependencies_section(&input.supplied, &input.provenance.files_detail),
        surface_section(files, &input.supplied),
        tests_section(files, &input.supplied),
        blast_section(&input.supplied),
        attention_section(input.risk.as_ref(), &input.supplied),
    ];
    let gaps: Vec<Gap> = sections
        .iter()
        .filter_map(|section| section.gap.clone())
        .collect();
    let mut provenance = input.provenance.clone();
    if let Some(view) = complexity.as_ref() {
        // The model map is where a figure's yardstick is named, and a complexity figure's yardstick is its
        // basis token: the same file counted under a different token is a different number, so the token
        // belongs with the number rather than in a footnote (ADR-007 §9.5). Written here rather than in the
        // store bridge so that a document composed from rows, from an artefact, or from a test says it too.
        provenance
            .models
            .insert("complexity".to_owned(), complexity_model_name(view));
    }
    let mut delta = EpisodeDelta {
        contract: CONTRACT.to_owned(),
        schema_version: VERSION,
        episode: input.episode.clone(),
        totals,
        complexity,
        files: files.clone(),
        sections,
        risk: input.risk.clone(),
        gaps,
        provenance,
        integrity: Integrity {
            verified: false,
            problems: Vec::new(),
            checks: Vec::new(),
        },
    };
    let problems = delta.verify();
    delta.integrity = Integrity {
        verified: problems.is_empty(),
        problems,
        checks: CHECKS.iter().map(|check| (*check).to_owned()).collect(),
    };
    delta
}

/// Is this row's churn one a review does not read line by line? The stored noise class wins when the index
/// decided it; otherwise the surface's own rule does. Same precedence `risk::classify` uses.
fn is_noisy(file: &FileDelta) -> bool {
    if file.binary || file.noise_class == "generated" || file.noise_class == "vendored" {
        return true;
    }
    surface_of(file).noisy()
}

/// The reviewer-facing surface of one row, from the shipped classifier.
fn surface_of(file: &FileDelta) -> Surface {
    risk::classify(&file.path, &file.path_class, &file.noise_class)
}

/// The gap for a section nobody answered: a short label for the one-screen card, the full reason for the
/// document, and the ticket that owns the rest.
fn gap_for(
    id: SectionId,
    short: impl Into<String>,
    reason: impl Into<String>,
    remedy: Option<String>,
) -> Gap {
    Gap {
        section: id.as_str().to_owned(),
        short: short.into(),
        reason: reason.into(),
        owner: id.owner().to_owned(),
        remedy,
    }
}

/// The `what` section: scale, and what of the scale a review reads.
///
/// The complexity figure rides here, beside the line counts, because that is where a reader decides how
/// much of a change is work: `+552 −105` and `±0` say a large edit made the code no harder to follow, and
/// `+552 −105` and `+41 (4/5 measured)` say it did (`e8.5`, ADR-007 §5.3).
fn what_section(
    files: &[FileDelta],
    totals: &Totals,
    full: bool,
    complexity: Option<&ComplexityView>,
) -> Section {
    let inside = |pointer: &str| Evidence::inside(format!("#{pointer}"));
    let mut figures = vec![
        Figure::new(
            "lines",
            plus_minus(totals.n_add, totals.n_del),
            inside("/totals"),
        )
        .with_raw(&[("add", totals.n_add), ("del", totals.n_del)]),
        Figure::new(
            "files",
            format!("{} file(s)", grouped(totals.n_files)),
            inside("/totals/n_files"),
        )
        .with_raw(&[("files", totals.n_files)]),
        Figure::new(
            "churn",
            format!("{} lines changed", grouped(totals.churn)),
            inside("/totals/churn"),
        )
        .with_raw(&[("churn", totals.churn)]),
        Figure::new(
            "reviewable churn",
            format!(
                "{} of {} ({}, keeps {}%)",
                grouped(totals.reviewable_churn),
                grouped(totals.churn),
                Totals::DISCOUNT_RULE,
                Totals::DISCOUNT_KEEP_PERCENT
            ),
            inside("/totals/reviewable_churn"),
        )
        .with_raw(&[
            ("reviewable", totals.reviewable_churn),
            ("discounted", totals.discounted_churn),
        ]),
    ];
    if let Some(view) = complexity {
        figures.push(complexity_figure(view));
    }
    if totals.n_binary > 0 {
        figures.push(
            Figure::new(
                "binary",
                format!("{} binary file(s)", grouped(totals.n_binary)),
                inside("/totals/n_binary"),
            )
            .with_raw(&[("binary", totals.n_binary)]),
        );
    }
    if totals.files_truncated {
        figures.push(Figure::new(
            "file rows",
            format!(
                "{} of {} paths read — the read capped, so the mix below is a partial census",
                grouped(totals.files_read),
                grouped(totals.n_files)
            ),
            inside("/totals/files_truncated"),
        ));
    }
    if files.is_empty() {
        figures.push(Figure::unknown(
            "path mix",
            "the index recorded no per-file rows for this episode, so nothing was classified",
            Evidence::owner("e1.2"),
        ));
    } else {
        let classes = rollup_by(files, |file| file.path_class.clone());
        let (value, _) = render_rollup(&classes, ROLLUP_CAP, false);
        figures.push(Figure::new("path classes", value, inside("/files")));
        if full {
            let kinds = rollup_by(files, |file| file.change.clone());
            let (value, _) = render_rollup(&kinds, ROLLUP_CAP, false);
            figures.push(Figure::new("change kinds", value, inside("/files")));
        }
    }
    // Noise is a view, so the view says which files it folded and which rule folded them. A reader who is
    // shown `5 of 9` and not the folded paths cannot tell a lockfile from a formatter pass, and
    // "suppressed by default" is only honest while it is also "named by default" (ADR-001 §7.2).
    let folded: Vec<&FileDelta> = files.iter().filter(|file| is_noisy(file)).collect();
    if !folded.is_empty() {
        // Named by the noise class when the index recorded one, and by the surface when it did not: the
        // taxonomy columns are `unknown` in a build whose policy engine has not answered, and an
        // unclassified folded file is still a folded file whose discount has to be printable.
        let mut by_class: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        for file in &folded {
            let class = match file.noise_class.as_str() {
                "" | "unknown" => surface_of(file).as_str().to_owned(),
                other => other.to_owned(),
            };
            let entry = by_class.entry(class).or_default();
            entry.0 += 1;
            entry.1 = entry.1.saturating_add(file.churn());
        }
        let listed: Vec<String> = by_class
            .iter()
            .take(ROLLUP_CAP)
            .map(|(class, (count, churn))| format!("{class} {count}f · {churn} lines"))
            .collect();
        figures.push(
            Figure::new(
                "noise folded",
                format!(
                    "{} of {} file(s) discounted ({})",
                    grouped(i64::try_from(folded.len()).unwrap_or(i64::MAX)),
                    grouped(totals.files_read),
                    listed.join(" · ")
                ),
                inside("/totals/discounted_churn"),
            )
            .with_raw(&[
                ("files", i64::try_from(folded.len()).unwrap_or(i64::MAX)),
                ("discounted", totals.discounted_churn),
            ]),
        );
    }
    let (state, gap) = if files.is_empty() {
        (
            STATE_PARTIAL,
            Some(gap_for(
                SectionId::What,
                "no per-file rows",
                "the path mix and the noise discount need per-file rows, and this episode has none; \
                 the counts above are the episode rollup, which is stored independently",
                None,
            )),
        )
    } else {
        (
            STATE_PARTIAL,
            Some(gap_for(
                SectionId::What,
                "path-rule discount only",
                "the discount is a path rule (`spine-1/noisy-path`); the token-level fold that \
                 separates a reformat from a rewrite is `e2.2`'s and reaches this model as supplied \
                 figures, not as a recomputation",
                None,
            )),
        )
    };
    Section::build(SectionId::What, state, figures, gap)
}

/// The `where` section: directories touched, surfaces touched, and what moved.
fn where_section(files: &[FileDelta], totals: &Totals, full: bool, supplied: &Supplied) -> Section {
    let id = SectionId::Where;
    let inside = |pointer: &str| Evidence::inside(format!("#{pointer}"));
    if files.is_empty() {
        return Section::build(
            id,
            STATE_UNAVAILABLE,
            vec![Figure::unknown(
                "directories",
                "no per-file rows were recorded, so nothing has a location",
                Evidence::owner("e1.2"),
            )],
            Some(gap_for(
                id,
                "no location recorded",
                "with no file rows there is no directory, no move and no shape to report",
                None,
            )),
        );
    }
    let mut figures = Vec::new();
    if let Some(block) = &supplied.structure {
        figures.push(
            Figure::new(
                "shape",
                block.headline.clone(),
                Evidence::owner(&block.owner),
            )
            .with_confidence_or_none(block.confidence),
        );
    }
    let dirs = rollup_by(files, |file| directory_of(&file.path));
    let (value, remainder) = render_rollup(&dirs, ROLLUP_CAP, true);
    figures.push(Figure::new(
        if dirs.len() == 1 {
            "directory"
        } else {
            "directories"
        },
        value,
        inside("/files"),
    ));
    if remainder > 0 {
        figures.push(Figure::new(
            "more directories",
            format!("{remainder} not listed"),
            inside("/files"),
        ));
    }
    let surfaces = rollup_by(files, |file| surface_of(file).as_str().to_owned());
    let (value, _) = render_rollup(&surfaces, ROLLUP_CAP, true);
    figures.push(Figure::new("surfaces", value, inside("/files")));
    if full {
        let moves: Vec<&FileDelta> = files
            .iter()
            .filter(|file| file.change == "renamed" || file.old_path.is_some())
            .collect();
        let value = if moves.is_empty() {
            "no path moves were recorded".to_owned()
        } else {
            let listed: Vec<String> = moves
                .iter()
                .take(ROLLUP_CAP)
                .map(|file| {
                    format!(
                        "{} → {}",
                        file.old_path.clone().unwrap_or_else(|| "?".to_owned()),
                        file.path
                    )
                })
                .collect();
            listed.join(" · ")
        };
        // The pointer names the rows the claim was read from, not the field: RFC 6901 has no "the
        // `old_path` of every element", and a pointer that resolves to nothing is a figure whose evidence
        // is a decoration (`verify` fails it, which is the point of the check).
        figures.push(Figure::new("moves", value, inside("/files")));
    } else {
        figures.push(Figure::unknown(
            "moves",
            "the rail's per-path rollup carries no move information; the per-commit read does",
            Evidence::external("fathom analyze <episode> --json"),
        ));
    }
    figures.push(
        Figure::new(
            "basis",
            format!(
                "{} of {} paths, ranked by churn; the census is the episode's own paths",
                grouped(totals.files_read),
                grouped(totals.n_files)
            ),
            inside("/totals"),
        )
        .with_raw(&[("files_read", totals.files_read)]),
    );
    Section::build(
        id,
        STATE_PARTIAL,
        figures,
        Some(gap_for(
            id,
            "shape sentence is e2.3's",
            "the shape sentence — a module split, a flatten, a subtree move, with its churn band — \
             is `e2.3`'s and is not recomputed here; it arrives as a supplied figure",
            None,
        )),
    )
}

/// The `dependencies` section, or the stated refusal.
///
/// The refusal is worded by the read that failed to answer, because the two reasons are different facts: a
/// rail page reads one summed row per path and has no blobs to parse, while a per-episode read over an
/// unopenable clone has the paths and not the trees. Each names the verb that does answer.
fn dependencies_section(supplied: &Supplied, detail: &str) -> Section {
    let id = SectionId::Dependencies;
    match &supplied.dependencies {
        Some(block) => {
            // The hand-over's evidence is the owning analysis plus the command that re-runs it: the
            // figures themselves live in another module, and a pointer into this document would point at
            // a copy. A reviewer with the command has the hunk-level proof (ADR-001 §7.9).
            let evidence = || Evidence {
                pointer: None,
                run: block.run.clone(),
                owner: Some(block.owner.clone()),
            };
            let mut figures = vec![
                Figure::new("headline", block.headline.clone(), evidence())
                    .with_confidence_or_none(block.confidence),
            ];
            if !block.basis.is_empty() {
                figures.push(Figure::new("basis", block.basis.clone(), evidence()));
            }
            for (label, value) in &block.figures {
                figures.push(Figure::new(label.clone(), value.clone(), evidence()));
            }
            Section::build(id, STATE_MEASURED, figures, None)
        }
        None => Section::build(
            id,
            STATE_UNAVAILABLE,
            vec![Figure::unknown(
                "dependencies",
                if detail == Provenance::DETAIL_ROLLUP {
                    "the rail reads one summed row per path; the dependency delta is computed from \
                     manifest and lockfile contents, which a rollup read does not carry"
                } else {
                    "the dependency delta is computed from lockfile and manifest *contents*, and this \
                     read was given rows, not blobs"
                },
                Evidence::owner(id.owner()),
            )],
            Some(gap_for(
                id,
                "no dependency delta here",
                if detail == Provenance::DETAIL_ROLLUP {
                    "`e2.6`'s delta needs both trees' dependency files; the rail's per-path rollup has \
                     neither, and inventing a count from path names is the lie this section exists to \
                     prevent"
                } else {
                    "`fathom_analyze::deps` computes this from the two trees of the episode; this read \
                     had no clone to read them from — run the verb inside the repository, and inventing \
                     a count from path names is the lie this section exists to prevent"
                },
                (detail == Provenance::DETAIL_ROLLUP)
                    .then(|| "fathom analyze <episode> --json".to_owned()),
            )),
        ),
    }
}

/// The `surface` section: which non-code surfaces moved, and how many of them someone outside the review
/// has to act on.
fn surface_section(files: &[FileDelta], supplied: &Supplied) -> Section {
    let id = SectionId::Surface;
    let operator: Vec<&FileDelta> = files
        .iter()
        .filter(|file| surface_of(file).operator())
        .collect();
    let mut figures = Vec::new();
    if files.is_empty() {
        return Section::build(
            id,
            STATE_UNAVAILABLE,
            vec![Figure::unknown(
                "surfaces",
                "no per-file rows were recorded, so no surface was seen",
                Evidence::owner("e1.2"),
            )],
            Some(gap_for(
                id,
                "no file rows",
                "no per-file rows were recorded",
                None,
            )),
        );
    }
    let surfaces = rollup_by(files, |file| surface_of(file).as_str().to_owned());
    let (value, _) = render_rollup(&surfaces, ROLLUP_CAP, true);
    figures.push(Figure::new("surfaces", value, Evidence::inside("#/files")));
    figures.push(
        Figure::new(
            "operator surfaces",
            if operator.is_empty() {
                "none — nothing here needs an action outside the review".to_owned()
            } else {
                let mut names: Vec<String> = operator
                    .iter()
                    .map(|file| surface_of(file).as_str().to_owned())
                    .collect();
                names.sort();
                names.dedup();
                format!("{} ({})", names.len(), names.join("+"))
            },
            Evidence::inside("#/files"),
        )
        .with_raw(&[("operator_files", operator.len() as i64)]),
    );
    if let Some(block) = &supplied.surface {
        for (label, value) in &block.figures {
            figures.push(Figure::new(
                label.clone(),
                value.clone(),
                Evidence::owner(&block.owner),
            ));
        }
        figures.push(Figure::new(
            "findings",
            block.headline.clone(),
            Evidence::owner(&block.owner),
        ));
    }
    let state = if supplied.surface.is_some() {
        STATE_MEASURED
    } else {
        STATE_PARTIAL
    };
    Section::build(
        id,
        state,
        figures,
        Some(gap_for(
            id,
            "findings are e2.8's",
            "which surfaces moved is answered from the stored path classes; what an operator has to \
             *do* — the migration that cannot be reversed, the environment variable nothing reads, \
             the workflow that now signs artefacts — is `e2.8`'s finding set and reaches this model \
             as a supplied block",
            None,
        )),
    )
}

/// The `tests` section: the testing contract, in counts, never a percentage.
fn tests_section(files: &[FileDelta], supplied: &Supplied) -> Section {
    let id = SectionId::Tests;
    if files.is_empty() {
        return Section::build(
            id,
            STATE_UNAVAILABLE,
            vec![Figure::unknown(
                "tests",
                "no per-file rows were recorded, so the test side is unknown — which is not the same \
                 claim as zero",
                Evidence::owner("e1.2"),
            )],
            Some(gap_for(
                id,
                "no file rows",
                "no per-file rows were recorded",
                None,
            )),
        );
    }
    let test: Vec<&FileDelta> = files
        .iter()
        .filter(|file| surface_of(file) == Surface::Test)
        .collect();
    let source: Vec<&FileDelta> = files
        .iter()
        .filter(|file| surface_of(file) == Surface::Source)
        .collect();
    let sum = |rows: &[&FileDelta]| -> (i64, i64) {
        rows.iter().fold((0_i64, 0_i64), |acc, file| {
            (
                acc.0.saturating_add(file.add),
                acc.1.saturating_add(file.del),
            )
        })
    };
    let (test_add, test_del) = sum(&test);
    let (source_add, source_del) = sum(&source);
    let mut figures = vec![
        Figure::new(
            "test side",
            format!(
                "{} file(s) · {}",
                grouped(test.len() as i64),
                plus_minus(test_add, test_del)
            ),
            Evidence::inside("#/files"),
        )
        .with_raw(&[
            ("test_files", test.len() as i64),
            ("test_add", test_add),
            ("test_del", test_del),
        ]),
        Figure::new(
            "source side",
            format!(
                "{} file(s) · {}",
                grouped(source.len() as i64),
                plus_minus(source_add, source_del)
            ),
            Evidence::inside("#/files"),
        )
        .with_raw(&[
            ("source_files", source.len() as i64),
            ("source_add", source_add),
            ("source_del", source_del),
        ]),
    ];
    figures.push(
        match percent(test_add.saturating_add(test_del), totals_churn_of(files)) {
            Some(part) => Figure::new(
                "test share of churn",
                format!("{part}% of the episode's churn is test-side"),
                Evidence::inside("#/files"),
            )
            .with_raw(&[
                ("test_churn", test_add.saturating_add(test_del)),
                ("churn", totals_churn_of(files)),
            ]),
            None => Figure::unknown(
                "test share of churn",
                "no churn to take a share of",
                Evidence::inside("#/totals/churn"),
            ),
        },
    );
    // The one finding the spine can assert on its own, and the one `reco-c05` ranks on: it is stated here so
    // the card and the ranking cannot drift into disagreeing about it.
    if test_del > 0 && source_add > 0 {
        figures.push(
            Figure::new(
                "watch",
                format!(
                    "source grew by {} while {} test lines were removed — the deleted cover is not \
                     re-established by anything in these rows",
                    grouped(source_add),
                    grouped(test_del)
                ),
                Evidence::inside("#/files"),
            )
            .with_raw(&[("source_add", source_add), ("test_del", test_del)]),
        );
    }
    if let Some(block) = &supplied.tests {
        for (label, value) in &block.figures {
            figures.push(Figure::new(
                label.clone(),
                value.clone(),
                Evidence::owner(&block.owner),
            ));
        }
    }
    let state = if supplied.tests.is_some() {
        STATE_MEASURED
    } else {
        STATE_PARTIAL
    };
    Section::build(
        id,
        state,
        figures,
        Some(gap_for(
            id,
            "case names are e2.9's",
            "counts of the test side come from the path taxonomy; the case-level answer — which \
             named cases were added, removed, skipped or weakened, and the pairing between a case and \
             the behaviour it covers — is `e2.9`'s and is supplied, not guessed",
            None,
        )),
    )
}

/// Churn over the rows seen — the denominator every ratio in this document names out loud.
fn totals_churn_of(files: &[FileDelta]) -> i64 {
    files
        .iter()
        .map(FileDelta::churn)
        .fold(0_i64, i64::saturating_add)
}

/// The `blast` section: what the change reaches.
fn blast_section(supplied: &Supplied) -> Section {
    let id = SectionId::Blast;
    match &supplied.blast {
        Some(block) => {
            let mut figures = vec![
                Figure::new(
                    "reach",
                    block.headline.clone(),
                    Evidence::owner(&block.owner),
                )
                .with_confidence_or_none(block.confidence),
            ];
            figures.push(Figure::new(
                "basis",
                block.basis.clone(),
                Evidence::owner(&block.owner),
            ));
            for (label, value) in &block.figures {
                figures.push(Figure::new(
                    label.clone(),
                    value.clone(),
                    Evidence::owner(&block.owner),
                ));
            }
            Section::build(id, STATE_MEASURED, figures, None)
        }
        None => Section::build(
            id,
            STATE_UNAVAILABLE,
            vec![Figure::unknown(
                "reach",
                "nothing in the spine says what depends on a changed path; a reach estimate built \
                 from path prefixes would be a guess wearing a number",
                Evidence::owner(id.owner()),
            )],
            Some(gap_for(
                id,
                "no supplied reach",
                "how far a change reaches is a question for the module graph, and this document carries \
                 no supplied block for it: the count costs a chain walk this pass did not take, and a \
                 reach estimate built from path prefixes would be a guess wearing a number — `fathom \
                 architecture` ranks modules by their fan-in on that graph today",
                None,
            )),
        ),
    }
}

/// The `attention` section: the ranking, its arithmetic, its blind spots, and any findings lent to it.
fn attention_section(risk_view: Option<&RiskView>, supplied: &Supplied) -> Section {
    let id = SectionId::Attention;
    let mut figures = Vec::new();
    if let Some(risk_view) = risk_view {
        figures.push(
            Figure::new("risk", risk_view.vector.clone(), Evidence::inside("#/risk"))
                .with_raw(&[("score", risk_view.score)]),
        );
        for component in &risk_view.components {
            figures.push(Figure::new(
                format!(
                    "{} ({}/{})",
                    component.name, component.points, component.cap
                ),
                component.detail.clone(),
                Evidence::inside("#/risk/components"),
            ));
        }
        if let Some(caveat) = &risk_view.caveat {
            figures.push(Figure::new(
                "caveat",
                caveat.clone(),
                Evidence::inside("#/risk/caveat"),
            ));
        }
        figures.push(Figure::new(
            "cannot see",
            risk_view.blind_spots.join(" · "),
            Evidence::inside("#/risk/blind_spots"),
        ));
    }
    for finding in &supplied.findings {
        figures.push(Figure::new(
            format!("[{}] {}", finding.severity, finding.kind),
            format!("{} — {}", finding.subject, finding.message),
            finding.evidence.clone(),
        ));
    }
    if risk_view.is_none() && supplied.findings.is_empty() {
        return Section::build(
            id,
            STATE_UNAVAILABLE,
            vec![Figure::unknown(
                "risk",
                "nothing ranked this episode, and unranked is not rank 0",
                Evidence::owner(id.owner()),
            )],
            Some(gap_for(
                id,
                "nothing ranked this episode",
                "no ranking was handed to this delta; `reco-c05` ranks the spine and `e4.4` ranks \
                 findings",
                None,
            )),
        );
    }
    let state = if risk_view.is_some() && !supplied.findings.is_empty() {
        STATE_MEASURED
    } else {
        STATE_PARTIAL
    };
    Section::build(
        id,
        state,
        figures,
        Some(gap_for(
            id,
            "spine-only ranking",
            "the ranking is the spine-only one: size, surfaces, complexity, deletions and breadth. \
             Findings, staleness and blast radius enter with `e4.4`'s scorer, and until then the \
             blind-spot list above is part of the answer rather than a footnote to it",
            None,
        )),
    )
}

/// A figure that is only confident when the analysis said so: an absent confidence stays absent rather than
/// becoming 1.0, which would read as certainty.
trait WithConfidenceOrNone {
    fn with_confidence_or_none(self, confidence: Option<f64>) -> Self;
}

impl WithConfidenceOrNone for Figure {
    fn with_confidence_or_none(self, confidence: Option<f64>) -> Self {
        match confidence {
            Some(value) => self.with_confidence(value),
            None => self,
        }
    }
}

/// Split a list at a cap and report what did not fit.
fn cap<T>(rows: Vec<T>, limit: usize) -> (Vec<T>, u64) {
    let remainder = u64::try_from(rows.len().saturating_sub(limit)).unwrap_or(u64::MAX);
    let mut kept = rows;
    kept.truncate(limit);
    (kept, remainder)
}

// ---------------------------------------------------------------------------
// the document checking itself
// ---------------------------------------------------------------------------

/// What [`EpisodeDelta::verify`] checks, in order, in the words the document prints.
///
/// Printed inside the payload (`integrity.checks`) so that `verified: true` is a claim with a definition
/// rather than a boolean a reader has to trust.
pub const CHECKS: [&str; 10] = [
    "contract and schema_version are this build's",
    "the seven sections are present, in order, once each",
    "every figure carries evidence, and every evidence pointer resolves inside the document",
    "an unknown figure carries the reason it is unknown",
    "an unavailable section states no measured figure and names its gap; a measured one shows its work",
    "every gap has a short form, names its section, and blames a plan key that exists",
    "the flat gap list is the section gaps, same order, nothing extra",
    "the derived totals are arithmetic, not decoration",
    "no section exceeds the figure cap",
    "the complexity figure is the roll-up beside it, not a second opinion of it",
];

/// Every plan key shape the documents use: `e2.11`, `s4`, `m1`, or a bead id.
fn looks_like_plan_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 2 {
        return false;
    }
    let head = bytes[0];
    let rest = &value[1..];
    match head {
        b'e' | b's' | b'd' | b'm' => rest.chars().all(|ch| ch.is_ascii_digit() || ch == '.'),
        _ => false,
    }
}

/// Walk a JSON pointer (`#/files`, `#/risk/components/0/detail`) through a document.
///
/// RFC 6901 minus the escapes nobody uses here: `~` and `/` do not appear in any key this model emits, and
/// saying so is better than half-implementing the escaping and being wrong about it.
fn resolve_pointer(document: &serde_json::Value, pointer: &str) -> bool {
    let Some(rest) = pointer.strip_prefix('#') else {
        return false;
    };
    let mut current = document;
    for segment in rest.split('/').skip(1) {
        match current {
            serde_json::Value::Object(map) => {
                let next = map.get(segment);
                match next {
                    Some(value) => current = value,
                    None => return false,
                }
            }
            serde_json::Value::Array(items) => {
                let Ok(index) = segment.parse::<usize>() else {
                    return false;
                };
                match items.get(index) {
                    Some(value) => current = value,
                    None => return false,
                }
            }
            _ => return false,
        }
    }
    true
}

impl EpisodeDelta {
    /// The document as a `serde_json::Value`, keys sorted, for a consumer that wants to edit it.
    #[must_use]
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// The document as one compact line — the `--json` contract's single-value-on-stdout rule.
    ///
    /// Byte-stable: `serde_json`'s map is a `BTreeMap`, so key order is alphabetical and two runs of the
    /// same program over the same episode produce the same bytes (`e5.7`'s artefact depends on it).
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.to_value()).unwrap_or_else(|_| String::from("{}"))
    }

    /// The document pretty-printed, for a human who asked for the JSON to read.
    #[must_use]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.to_value()).unwrap_or_else(|_| String::from("{}"))
    }

    /// Read a document back. Unknown fields are ignored, which is the forward-compatibility half of the
    /// compatibility policy: a v1 reader must survive a v2 producer's additions.
    ///
    /// # Errors
    ///
    /// `serde_json::Error` when the bytes are not a delta document at all.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The section with this id, when the document has one.
    #[must_use]
    pub fn section(&self, id: SectionId) -> Option<&Section> {
        self.sections
            .iter()
            .find(|section| section.id == id.as_str())
    }

    /// Does a JSON pointer address something in this document?
    ///
    /// Published because [`crate::narrative`] cites these pointers rather than re-addressing them: one
    /// resolver for both self-checks means one definition of "the evidence is really there", which is the
    /// whole difference between a citation and a decoration.
    #[must_use]
    pub fn resolves(&self, pointer: &str) -> bool {
        resolve_pointer(&self.to_value(), pointer)
    }

    /// The state of one section, or `unavailable` when the section is absent — absent means unanswered.
    #[must_use]
    pub fn state_of(&self, id: SectionId) -> &'static str {
        match self.section(id) {
            Some(section) => match section.state.as_str() {
                STATE_MEASURED => STATE_MEASURED,
                STATE_PARTIAL => STATE_PARTIAL,
                _ => STATE_UNAVAILABLE,
            },
            None => STATE_UNAVAILABLE,
        }
    }

    /// Re-read the document the way a reviewer would, and return one line per thing that does not hold.
    ///
    /// This is the machine-checkable version of "the model is the source of truth": a figure whose pointer
    /// goes nowhere, a section with no answer and no gap, a total that does not sum, a gap that blames
    /// nobody. Empty means it held up; the checks themselves are [`CHECKS`], and they travel with the
    /// document.
    #[must_use]
    pub fn verify(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let document = self.to_value();

        // 1 — the version marker.
        if self.contract != CONTRACT {
            problems.push(format!(
                "contract `{}` is not this build's `{CONTRACT}`",
                self.contract
            ));
        }
        if self.schema_version != VERSION {
            problems.push(format!(
                "schema_version {} is not this build's {VERSION}",
                self.schema_version
            ));
        }

        // 2 — the seven questions, once each, in order.
        let expected: Vec<&str> = SectionId::ALL
            .iter()
            .copied()
            .map(SectionId::as_str)
            .collect();
        let known: Vec<&str> = self
            .sections
            .iter()
            .filter(|section| SectionId::parse(&section.id).is_some())
            .map(|section| section.id.as_str())
            .collect();
        if known != expected {
            problems.push(format!(
                "sections are [{}, …], not the seven in order [{}]",
                known.join(", "),
                expected.join(", ")
            ));
        }

        for section in &self.sections {
            let where_ = format!("section `{}`", section.id);
            // 3 and 4 — evidence, and pointers that go somewhere.
            for figure in &section.figures {
                if figure.evidence.is_empty() {
                    problems.push(format!(
                        "{where_}/`{}`: a figure with no evidence",
                        figure.label
                    ));
                }
                if let Some(pointer) = &figure.evidence.pointer
                    && !resolve_pointer(&document, pointer)
                {
                    problems.push(format!(
                        "{where_}/`{}`: evidence pointer `{pointer}` resolves to nothing",
                        figure.label
                    ));
                }
                if !figure.known && figure.reason.as_deref().unwrap_or("").is_empty() {
                    problems.push(format!(
                        "{where_}/`{}`: an unknown figure with no reason is a blank, and a blank is a \
                         lie",
                        figure.label
                    ));
                }
            }
            // 5 — state and figures have to agree. An unavailable section may carry figures, but only
            // unknown ones: the row that says *why* nothing was measured is part of the answer, and it is
            // what `fathom analyze` prints where the card prints the gap's short form.
            match section.state.as_str() {
                STATE_UNAVAILABLE => {
                    if let Some(figure) = section.figures.iter().find(|figure| figure.known) {
                        problems.push(format!(
                            "{where_}: unavailable, but `{}` claims a measurement",
                            figure.label
                        ));
                    }
                    if section.gap.is_none() {
                        problems.push(format!("{where_}: unavailable with no gap to explain it"));
                    }
                }
                STATE_MEASURED if section.figures.is_empty() => {
                    problems.push(format!("{where_}: measured with nothing to show"));
                }
                STATE_MEASURED | STATE_PARTIAL => {}
                other => {
                    problems.push(format!("{where_}: state `{other}` is not one of the three"))
                }
            }
            // 9 — the cap, with its counted remainder.
            if section.figures.len() > FIGURE_CAP {
                problems.push(format!(
                    "{where_}: {} figures, over the cap of {FIGURE_CAP}",
                    section.figures.len()
                ));
            }
            // 6 — a gap that names its section and its owner.
            if let Some(gap) = &section.gap {
                if gap.short.trim().is_empty() {
                    problems.push(format!(
                        "{where_}: its gap has no short form, so the one-screen card cannot say what \
                         it left out"
                    ));
                }
                if gap.section != section.id {
                    problems.push(format!(
                        "{where_}: its gap is filed against `{}`",
                        gap.section
                    ));
                }
                if !looks_like_plan_key(&gap.owner) {
                    problems.push(format!(
                        "{where_}: gap owner `{}` is not a plan key",
                        gap.owner
                    ));
                }
            }
        }

        // 7 — the flat gap list is the section gaps, same order, nothing extra.
        let flat: Vec<&str> = self.gaps.iter().map(|gap| gap.section.as_str()).collect();
        let from_sections: Vec<&str> = self
            .sections
            .iter()
            .filter_map(|section| section.gap.as_ref().map(|gap| gap.section.as_str()))
            .collect();
        if flat != from_sections {
            problems.push(format!(
                "the gap list {:?} is not the section gaps {:?}",
                flat, from_sections
            ));
        }

        // 8 — the derived totals.
        if self.totals.churn != self.totals.n_add.saturating_add(self.totals.n_del) {
            problems.push(format!(
                "totals: churn {} is not n_add + n_del",
                self.totals.churn
            ));
        }
        if self.totals.reviewable_churn
            != self
                .totals
                .churn
                .saturating_sub(self.totals.discounted_churn)
        {
            problems.push("totals: reviewable_churn is not churn − discounted_churn".to_owned());
        }
        if self.totals.discounted_churn > self.totals.churn {
            problems.push("totals: more churn was discounted than there was".to_owned());
        }
        let add: i64 = self
            .files
            .iter()
            .map(|file| file.add)
            .fold(0, i64::saturating_add);
        let del: i64 = self
            .files
            .iter()
            .map(|file| file.del)
            .fold(0, i64::saturating_add);
        if !self.totals.files_truncated && !self.files.is_empty() {
            let agrees = add == self.totals.n_add && del == self.totals.n_del;
            if agrees != self.totals.reconciles.unwrap_or(false) {
                problems.push(format!(
                    "totals: the file rows sum to +{add} −{del} against the episode's +{} −{}, and \
                     `reconciles` says {:?}",
                    self.totals.n_add, self.totals.n_del, self.totals.reconciles
                ));
            }
        }
        if self.totals.files_read != i64::try_from(self.files.len()).unwrap_or(i64::MAX) {
            problems.push(format!(
                "totals: files_read {} but {} rows are present",
                self.totals.files_read,
                self.files.len()
            ));
        }

        // 10 — the complexity figure and the roll-up beside it are one answer (`e8.5`). The figure is what
        // every surface prints, the block is what it was read from, and the two disagreeing is the same
        // bug ADR-001 §7.10 was written to end — so it is checked here rather than left to a reviewer who
        // happens to notice that the card's number is not the export's.
        let figure = self.section(SectionId::What).and_then(|section| {
            section
                .figures
                .iter()
                .find(|figure| figure.label == COMPLEXITY_LABEL)
        });
        match (&self.complexity, figure) {
            (None, None) => {}
            (None, Some(_)) => problems.push(
                "the `what` section prints a `complexity` figure with no `complexity` block to read \
                 it from"
                    .to_owned(),
            ),
            (Some(view), None) => problems.push(format!(
                "the document carries a complexity roll-up over {} path(s) and no `{COMPLEXITY_LABEL}` \
                 figure to print it",
                grouped(view.touched)
            )),
            (Some(view), Some(figure)) => {
                if figure.known != view.measured {
                    problems.push(format!(
                        "complexity: the figure says {} and the roll-up says measured={}",
                        if figure.known { "a measurement" } else { "nothing" },
                        view.measured
                    ));
                }
                if figure.raw.get("delta") != view.delta.as_ref() {
                    problems.push(format!(
                        "complexity: the figure's `delta` is {:?} and the roll-up's is {:?}",
                        figure.raw.get("delta"),
                        view.delta
                    ));
                }
                if view.measured && figure.value != complexity_headline(view) {
                    problems.push(format!(
                        "complexity: the figure prints `{}` and this roll-up prints `{}` — one surface \
                         has formatted the number itself",
                        figure.value,
                        complexity_headline(view)
                    ));
                }
                // The block's own arithmetic, re-done rather than trusted.
                if view.measured_paths + view.unmeasurable_paths != view.touched {
                    problems.push(format!(
                        "complexity: {} measured + {} unmeasurable is not the {} paths in scope",
                        grouped(view.measured_paths),
                        grouped(view.unmeasurable_paths),
                        grouped(view.touched)
                    ));
                }
                let expected_state = complexity_state(view);
                if view.state != expected_state {
                    problems.push(format!(
                        "complexity: state `{}` is not what the counts make it (`{expected_state}`)",
                        view.state
                    ));
                }
                let expected_coverage = if view.touched > 0 {
                    Some(view.measured_paths as f64 / view.touched as f64)
                } else {
                    None
                };
                let agrees = match (view.coverage, expected_coverage) {
                    (Some(got), Some(want)) => (got - want).abs() < 1e-9,
                    (None, None) => true,
                    _ => false,
                };
                if !agrees {
                    problems.push(format!(
                        "complexity: coverage {:?} is not measured_paths/touched ({:?})",
                        view.coverage, expected_coverage
                    ));
                }
                // The gap contract: no figure means a short line that a 35-column card can print, naming
                // its owner — never the silence a zero would have been.
                match (&view.gap, view.measured) {
                    (Some(gap), false) => {
                        if gap.short.trim().is_empty() {
                            problems.push(
                                "complexity: an unmeasured figure with an unprintable gap is an \
                                 unmeasured figure with no gap"
                                    .to_owned(),
                            );
                        }
                        if gap.section != SectionId::What.as_str() {
                            problems.push(format!(
                                "complexity: its gap is filed against `{}`",
                                gap.section
                            ));
                        }
                        if !looks_like_plan_key(&gap.owner) {
                            problems.push(format!(
                                "complexity: gap owner `{}` is not a plan key",
                                gap.owner
                            ));
                        }
                        let printed = format!("{} ({})", gap.short, gap.owner);
                        if printed.chars().count() > CARD_COLUMNS {
                            problems.push(format!(
                                "complexity: the card line `{printed}` is {} columns and the \
                                 narrowest card has {CARD_COLUMNS}",
                                printed.chars().count()
                            ));
                        }
                    }
                    (None, false) => problems.push(
                        "complexity: an unmeasured figure with no gap is the silent zero ADR-007 §5.4 \
                         refuses"
                            .to_owned(),
                    ),
                    (Some(_), true) => problems.push(
                        "complexity: a measured figure has no gap to close".to_owned(),
                    ),
                    (None, true) => {}
                }
            }
        }
        // `integrity` itself is not checked here: `compose` runs this before the field can hold its
        // result, and a self-reference that reads as a failure during the only call that matters is a
        // check with a bug in it. The field is filled from this function's return, so it cannot disagree.
        problems
    }
}

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

/// The document as plain lines, one per figure, section by section.
///
/// This is what `fathom analyze` prints without `--json`. It is deliberately thin — label, value, gap —
/// because the layout is the caller's business (the TUI has faces, widths and a pane budget) and a
/// renderer that also decides what the facts *are* is how two surfaces stop agreeing. Every string here is
/// the same string the card prints for the same figure, which is the property worth having.
#[must_use]
pub fn render(delta: &EpisodeDelta) -> Vec<String> {
    let episode = &delta.episode;
    let mut out = vec![format!(
        "{} — {}  [{} v{}]",
        short_id(&episode.id),
        episode.summary,
        delta.contract,
        delta.schema_version
    )];
    out.push(format!(
        "author {} · {} commit(s) · review {}{}",
        episode.author_kind,
        episode.n_commits,
        episode.review,
        if episode.live { "" } else { " · gone" }
    ));
    // The attribution line, shown whenever there is a case to show (`e4.1`): the rung, the agent, and the
    // reason when the label was refused rather than earned.
    if let Some(attribution) = &episode.attribution {
        let agent = attribution.agent.as_deref().unwrap_or("—");
        let mut line = format!(
            "attribution {} · {agent} · {:.2} via {}",
            attribution.kind, attribution.confidence, attribution.source
        );
        if let Some(reason) = &attribution.reason {
            line.push_str(" · ");
            line.push_str(reason);
        }
        out.push(line);
        if let Some(first) = attribution.evidence.first() {
            out.push(format!(
                "evidence  {} {:.2} — {}",
                first.source, first.confidence, first.detail
            ));
        }
    }
    if let Some(intent) = &episode.intent {
        out.extend(intent_block(intent));
    }
    for section in &delta.sections {
        out.push(format!(
            "{} [{}] {}",
            section.title, section.state, section.question
        ));
        for figure in &section.figures {
            let shown = if figure.known {
                figure.value.clone()
            } else {
                format!("? — {}", figure.reason.clone().unwrap_or_default())
            };
            out.push(format!("  {} — {}", figure.label, shown));
        }
        if let Some(remainder) = section.remainder {
            out.push(format!("  … {remainder} more not listed"));
        }
        if let Some(gap) = &section.gap {
            let remedy = gap
                .remedy
                .as_ref()
                .map_or_else(String::new, |run| format!(" — instead: {run}"));
            out.push(format!("  gap ({}): {}{remedy}", gap.owner, gap.reason));
        }
    }
    if !delta.integrity.verified {
        out.push(format!(
            "self-check FAILED: {}",
            delta.integrity.problems.join(" | ")
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// what a document costs
// ---------------------------------------------------------------------------

/// Bytes a string keeps alive: its contents. (A `&str` cannot report its allocation's capacity, so this
/// counts the contents and adds a slot for the header everywhere a `Vec` holds them — the same convention
/// `fathom_tui::timeline::Episode::retained_bytes` uses, so the two numbers are comparable.)
fn bytes(text: &str) -> usize {
    text.len()
}

fn opt_bytes(text: &Option<String>) -> usize {
    text.as_ref().map_or(0, |value| bytes(value))
}

fn vec_bytes(rows: &[String]) -> usize {
    rows.iter().map(|row| bytes(row)).sum::<usize>() + std::mem::size_of_val(rows)
}

impl EpisodeRef {
    /// The heap this identity keeps alive.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        bytes(&self.id)
            + bytes(&self.fingerprint)
            + bytes(&self.fallback_fingerprint)
            + bytes(&self.shape_fingerprint)
            + bytes(&self.summary)
            + bytes(&self.author_name)
            + bytes(&self.author_email)
            + bytes(&self.boundary_rule)
            + bytes(&self.lineage_ref)
            + vec_bytes(&self.members)
            + opt_bytes(&self.session)
            + opt_bytes(&self.turn)
    }
}

impl EpisodeDelta {
    /// The heap this document keeps alive, contents and capacities.
    ///
    /// A rail page holds one of these per row (`e2.11`), so the number is not academic: `e1.6`'s
    /// constant-memory criterion is measured over the rows the rail actually holds, and a figure that
    /// ignored the delta would be the criterion lying to itself. The sections' own vocabulary (ids,
    /// questions, titles) is shared per section *name* but re-allocated per document, and it is counted
    /// here rather than amortised away — 500 rows paying for the same seven strings is a real cost and
    /// the honest place to discover it is the budget test, not a review.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        let mut total = self.episode.retained_bytes() + bytes(&self.contract);
        for file in &self.files {
            total += bytes(&file.path)
                + opt_bytes(&file.old_path)
                + bytes(&file.change)
                + bytes(&file.path_class)
                + bytes(&file.noise_class)
                + bytes(&file.surface)
                + bytes(&file.basis);
        }
        total += self.files.capacity() * std::mem::size_of::<FileDelta>();
        if let Some(complexity) = &self.complexity {
            total += complexity
                .basis
                .iter()
                .map(|token| bytes(token))
                .sum::<usize>();
            total += complexity
                .reasons
                .keys()
                .map(|code| bytes(code))
                .sum::<usize>();
            if let Some(gap) = &complexity.gap {
                total += bytes(&gap.section)
                    + bytes(&gap.short)
                    + bytes(&gap.reason)
                    + bytes(&gap.owner)
                    + opt_bytes(&gap.remedy);
            }
        }
        for section in &self.sections {
            total += bytes(&section.id)
                + bytes(&section.title)
                + bytes(&section.question)
                + bytes(&section.owner)
                + bytes(&section.state);
            for figure in &section.figures {
                total += bytes(&figure.label)
                    + bytes(&figure.value)
                    + opt_bytes(&figure.reason)
                    + opt_bytes(&figure.evidence.pointer)
                    + opt_bytes(&figure.evidence.run)
                    + opt_bytes(&figure.evidence.owner);
                total += figure.raw.len() * (std::mem::size_of::<String>() + 8);
            }
            total += section.figures.capacity() * std::mem::size_of::<Figure>();
            if let Some(gap) = &section.gap {
                total += bytes(&gap.section)
                    + bytes(&gap.reason)
                    + bytes(&gap.owner)
                    + opt_bytes(&gap.remedy);
            }
        }
        total += self.sections.capacity() * std::mem::size_of::<Section>();
        for gap in &self.gaps {
            total += bytes(&gap.section)
                + bytes(&gap.reason)
                + bytes(&gap.owner)
                + opt_bytes(&gap.remedy);
        }
        if let Some(risk) = &self.risk {
            total += bytes(&risk.model)
                + bytes(&risk.vector)
                + vec_bytes(&risk.surfaces)
                + vec_bytes(&risk.blind_spots)
                + opt_bytes(&risk.caveat);
            for component in &risk.components {
                total += bytes(&component.name) + bytes(&component.detail);
            }
        }
        total += bytes(&self.provenance.source)
            + opt_bytes(&self.provenance.store)
            + opt_bytes(&self.provenance.asked)
            + bytes(&self.provenance.produced_by)
            + bytes(&self.provenance.files_detail);
        for (key, value) in &self.provenance.models {
            total += bytes(key) + bytes(value);
        }
        total += vec_bytes(&self.integrity.checks) + vec_bytes(&self.integrity.problems);
        total
    }
}

/// Seven characters of the episode id, which is what every surface prints first.
#[must_use]
pub fn short_id(id: &str) -> String {
    id.chars().take(7).collect()
}

// ---------------------------------------------------------------------------
// the store bridge: rows in the index become rows in the model
// ---------------------------------------------------------------------------
//
// This is the only place in the model that names `fathom-store`, and it is gated the same way `index` and
// `sink` are: a build with no SQLite backend has no rows to bridge. The read is *five* statements at most
// (episode, members, per-file rows, review state, complexity roll-up) and the rail path does each of them
// for a whole page at once, because a read whose cost grows with the rows it describes is a read a first
// frame cannot afford.

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
mod store_bridge {
    use std::collections::BTreeMap;

    use fathom_store::complexity::EpisodeComplexity;
    use fathom_store::episode::{EpisodeFile, EpisodeRow, ReviewState};
    use fathom_store::spine::Store;

    use super::{
        AttributionEvidence, AttributionRef, ComplexityView, EpisodeDelta, EpisodeRef, FileDelta,
        Input, MEMBER_CAP, Provenance, RiskView, Supplied, SuppliedBlock, Totals, compose,
        dependency_block, settle_complexity, short_id,
    };
    use crate::error::Error;
    use crate::risk::{self, FileChange};

    /// Build the model for one episode by id, reading the per-commit file rows (the `full` detail: change
    /// kinds and recorded moves included).
    ///
    /// # Errors
    ///
    /// [`Error::Store`] when the store refuses; `Ok(None)` when this store holds no such episode, which is
    /// the caller's decision about exit code and remedy, not this function's.
    pub fn for_episode(store: &Store, id: &str) -> Result<Option<EpisodeDelta>, Error> {
        let Some(row) = store.episode(id)? else {
            return Ok(None);
        };
        Ok(Some(build_one(store, &row, None)?))
    }

    /// Build the model for the episode a commit belongs to.
    ///
    /// # Errors
    ///
    /// As [`for_episode`].
    pub fn for_commit(store: &Store, oid: &str) -> Result<Option<EpisodeDelta>, Error> {
        let Some(row) = store.episode_for_commit(oid)? else {
            return Ok(None);
        };
        Ok(Some(build_one(store, &row, None)?))
    }

    /// [`for_episode`], with the analyses the store cannot reach read out of an open clone.
    ///
    /// This is the seam `e2.11` §4 describes, with a caller on the other side of it: the dependency delta
    /// (`e2.6`) is computed from the *contents* of manifests and lockfiles, which no row of the spine
    /// carries, so the façade asks `e2.6` itself and lends the answer to the composition. A caller with no
    /// clone keeps today's document — the section is `unavailable` and names the reason — which is why the
    /// clone is an argument and not an assumption.
    ///
    /// # Errors
    ///
    /// As [`for_episode`]. A clone that will not serve the two trees is not an error: the section states
    /// the gap, because "the objects are not in this clone" is a fact about the repository and not a
    /// failure of the read.
    pub fn for_episode_with_repo(
        store: &Store,
        id: &str,
        repo: &fathom_ingest::repo::Repository,
    ) -> Result<Option<EpisodeDelta>, Error> {
        let Some(row) = store.episode(id)? else {
            return Ok(None);
        };
        Ok(Some(build_one(store, &row, Some(repo))?))
    }

    /// [`for_commit`], with the analyses the store cannot reach read out of an open clone.
    ///
    /// # Errors
    ///
    /// As [`for_episode`].
    pub fn for_commit_with_repo(
        store: &Store,
        oid: &str,
        repo: &fathom_ingest::repo::Repository,
    ) -> Result<Option<EpisodeDelta>, Error> {
        let Some(row) = store.episode_for_commit(oid)? else {
            return Ok(None);
        };
        Ok(Some(build_one(store, &row, Some(repo))?))
    }

    /// One episode, fully read, plus whatever an open clone could lend.
    fn build_one(
        store: &Store,
        row: &EpisodeRow,
        repo: Option<&fathom_ingest::repo::Repository>,
    ) -> Result<EpisodeDelta, Error> {
        let members = store.episode_members(&row.id)?;
        let mut files: BTreeMap<String, Aggregated> = BTreeMap::new();
        for oid in &members {
            for file in store.read_files(oid)? {
                files.entry(file.new_path.clone()).or_default().absorb(
                    &file.new_path,
                    &file.old_path,
                    &file.change_kind,
                    &file,
                );
            }
        }
        let review = store.review_of(&row.id)?;
        let supplied = repo.map_or_else(Supplied::default, |repo| lend(repo, row));
        // The complexity roll-up is one join over the episode's own file rows — the read ADR-007 §9.2 put
        // beside `n_add`/`n_del` precisely so that this composition never walks a file a second time.
        let rollups = store.episode_complexity(std::slice::from_ref(&row.id))?;
        let read = Read {
            row,
            review: review.as_str(),
            files: files
                .into_values()
                .map(|aggregated| aggregated.into_file())
                .collect(),
            detail: Provenance::DETAIL_FULL,
            truncated: false,
            supplied,
            complexity: rollups.get(&row.id).map(view),
        };
        Ok(compose(&read.input()))
    }

    /// One episode's complexity roll-up, in the shape the document carries it (`e8.5`).
    ///
    /// Published for the surface that has an episode, a store and no delta document — `fathom show` prints
    /// this figure beside the line counts off its own row — and settled by the same rules the composition
    /// uses, so a card and a document cannot call the same roll-up a floor and a measurement.
    ///
    /// # Errors
    ///
    /// [`Error::Store`] when the store refuses.
    pub fn episode_complexity(store: &Store, id: &str) -> Result<Option<ComplexityView>, Error> {
        Ok(store
            .episode_complexity(std::slice::from_ref(&id.to_owned()))?
            .get(id)
            .map(view))
    }

    /// The store's roll-up, worded: the counted numbers across, then the three this model derives.
    ///
    /// The store's own `state` is not copied, because the document's `state` has to be re-derivable from
    /// the fields the document publishes — a state that depended on a row a reader never sees is a state
    /// [`super::EpisodeDelta::verify`] could not check.
    fn view(roll: &EpisodeComplexity) -> ComplexityView {
        let count = |value: usize| i64::try_from(value).unwrap_or(i64::MAX);
        let mut view = ComplexityView {
            basis: roll.bases.iter().cloned().collect(),
            delta: roll.delta,
            absolute: roll.absolute,
            max: roll.max,
            touched: count(roll.paths_total()),
            measured_paths: count(roll.measured),
            unmeasurable_paths: count(roll.unmeasured),
            reasons: roll
                .reasons
                .iter()
                .map(|(code, paths)| (code.clone(), count(*paths)))
                .collect(),
            ..ComplexityView::default()
        };
        settle_complexity(&mut view);
        view
    }

    /// The analyses a store read cannot answer, read from the clone that owns them.
    ///
    /// One function, so that `fathom analyze`, `fathom narrative`, `fathom export` and the MCP tool cannot
    /// drift into four different answers to "what does this now depend on" (ADR-001 §7.10).
    fn lend(repo: &fathom_ingest::repo::Repository, row: &EpisodeRow) -> Supplied {
        let Some(block) = dependency_handover(repo, row) else {
            return Supplied::default();
        };
        Supplied::default().with_dependencies(block)
    }

    /// The `e2.6` answer for one episode, as a hand-over.
    ///
    /// The pair of trees is the episode's own: the first member's first parent against the last member's
    /// tree, which is the same pair `fathom export` and `fathom check` diff, so the three surfaces cannot
    /// disagree about what the episode made the tree depend on. `None` when the clone cannot serve those
    /// commits — and the section then says so, rather than printing an empty answer that looks like "no
    /// dependency changed".
    fn dependency_handover(
        repo: &fathom_ingest::repo::Repository,
        row: &EpisodeRow,
    ) -> Option<SuppliedBlock> {
        use fathom_ingest::oid::Oid;

        let (first, last) = (row.members.first()?, row.members.last()?);
        let head = repo.commit(last.parse::<Oid>().ok()?).ok().flatten()?;
        let (base, base_label) = match repo
            .commit(first.parse::<Oid>().ok()?)
            .ok()
            .flatten()
            .and_then(|base| base.parents.first().copied())
        {
            Some(parent) => (
                Some(repo.commit(parent).ok().flatten().map(|base| base.tree)?),
                short_id(&parent.hex()),
            ),
            // The episode opens on a root commit: the tree before it is the empty one, and the honest
            // label for that is "nothing", not a zero-length oid.
            None => (None, "(empty tree)".to_owned()),
        };
        let episode = crate::deps::dependency_episode_repository(
            repo,
            base,
            Some(head.tree),
            &crate::deps::DependencyOptions::default(),
        )
        .ok()?;
        Some(dependency_block(&episode, &base_label, &short_id(last)))
    }
    ///
    /// An empty `attribution_source` **and** an empty evidence list mean the row was written before
    /// migration 4, or by a build that stored the label without the case for it. The label is still true
    /// in `author_kind`, so this returns `None` rather than inventing evidence to go with it.
    fn attribution_ref(row: &fathom_store::episode::EpisodeRow) -> Option<AttributionRef> {
        if row.attribution_source.is_empty() && row.attribution_evidence.is_empty() {
            return None;
        }
        let evidence: Vec<AttributionEvidence> =
            serde_json::from_str(&row.attribution_evidence).unwrap_or_default();
        Some(AttributionRef {
            kind: row.author_kind.clone(),
            source: row.attribution_source.clone(),
            agent: (!row.attribution_agent.is_empty()).then(|| row.attribution_agent.clone()),
            confidence: row.author_confidence,
            reason: (!row.attribution_reason.is_empty()).then(|| row.attribution_reason.clone()),
            evidence,
        })
    }

    /// The store row's intent columns → the delta's intent block (`e4.2`, `reco-c54`).
    ///
    /// Three states, and the middle one is the reason this is a function rather than a `?`:
    ///
    /// * no `intent_state` at all — a row from before migration 5, or one whose delta was built from a
    ///   spine row that predates intent. `None`: nobody looked, which the reader prints as a gap rather
    ///   than as an absence.
    /// * a state and a document — the normal case, parsed and handed over whole.
    /// * a state and an unreadable document — a truncated or foreign JSON blob. The *scalars* are still
    ///   true, so they are what gets shown, with the parse failure named as the reason. A delta document
    ///   that refuses to build because one column holds junk is a document that cannot be read at all,
    ///   and the junk is the finding, not the blocker.
    fn intent_ref(
        row: &fathom_store::episode::EpisodeRow,
    ) -> Option<fathom_ingest::intent::Intent> {
        use fathom_ingest::intent::Intent;
        if row.intent.is_empty() {
            return None;
        }
        match Intent::from_json(&row.intent) {
            Some(intent) => Some(intent),
            None => Some(Intent {
                state: fathom_ingest::intent::State::parse(&row.intent_state)
                    .as_str()
                    .to_owned(),
                source: fathom_ingest::intent::Source::parse(&row.intent_source)
                    .as_str()
                    .to_owned(),
                confidence: row.intent_confidence,
                text: (!row.intent_text.is_empty()).then(|| row.intent_text.clone()),
                reason: Some(
                    "the stored intent document could not be read; these fields are the scalars \
                     beside it"
                        .to_owned(),
                ),
                ..Intent::default()
            }),
        }
    }

    /// Build the model for an episode from rows the caller already read — the rail's path, which reads the
    /// summed per-path rollup for a whole page in one statement and so cannot afford the per-commit read.
    ///
    /// The resulting document says so: `provenance.files_detail` is `rollup`, and the `where` section's move
    /// row is an unknown that names the verb that does have moves. One model, two bases, both stated.
    ///
    /// `complexity` is that episode's row from [`Store::episode_complexity`], which is a batched read the
    /// caller makes once for the whole page (ADR-007 §9.2: the roll-up is a join, and a rail that asked per
    /// episode would be a rail paying per row for one statement). `None` is "this read did not look": the
    /// document then carries no complexity figure at all, which is a different claim from the `?` a roll-up
    /// that could not be counted prints.
    pub fn for_rows(
        row: &EpisodeRow,
        files: &[EpisodeFile],
        review: ReviewState,
        truncated: bool,
        complexity: Option<&EpisodeComplexity>,
    ) -> EpisodeDelta {
        let read = Read {
            row,
            review: review.as_str(),
            files: files
                .iter()
                .map(|file| FileDelta {
                    path: file.path.clone(),
                    old_path: None,
                    change: "unrecorded".to_owned(),
                    add: file.add,
                    del: file.del,
                    binary: file.binary,
                    commits: file.commits,
                    path_class: file.path_class.clone(),
                    noise_class: file.noise_class.clone(),
                    surface: risk::classify(&file.path, &file.path_class, &file.noise_class)
                        .as_str()
                        .to_owned(),
                    basis: Provenance::DETAIL_ROLLUP.to_owned(),
                })
                .collect(),
            detail: Provenance::DETAIL_ROLLUP,
            truncated,
            supplied: Supplied::default(),
            complexity: complexity.map(view),
        };
        compose(&read.input())
    }

    /// What [`for_rows`] and [`build_one`] have in common, gathered so `input()` is written once.
    struct Read<'a> {
        row: &'a EpisodeRow,
        review: &'static str,
        files: Vec<FileDelta>,
        detail: &'static str,
        truncated: bool,
        supplied: Supplied,
        complexity: Option<ComplexityView>,
    }

    impl Read<'_> {
        fn input(&self) -> Input {
            let row = self.row;
            let members: Vec<String> = row.members.iter().take(MEMBER_CAP).cloned().collect();
            let remainder = row.members.len().saturating_sub(members.len());
            let episode = EpisodeRef {
                id: row.id.clone(),
                fingerprint: row.fingerprint.clone(),
                fallback_fingerprint: row.fallback_fingerprint.clone(),
                shape_fingerprint: row.shape_fingerprint.clone(),
                members,
                member_remainder: (remainder > 0)
                    .then(|| u64::try_from(remainder).unwrap_or(u64::MAX)),
                n_commits: row.n_commits(),
                summary: row.summary.clone(),
                author_kind: row.author_kind.clone(),
                author_name: row.author_name.clone(),
                author_email: row.author_email.clone(),
                author_confidence: Some(row.author_confidence),
                attribution: attribution_ref(row),
                intent: intent_ref(row),
                boundary_rule: row.boundary_rule.clone(),
                boundary_confidence: Some(row.boundary_confidence),
                session: row.session_id.clone(),
                turn: row.turn_id.clone(),
                first_utc: row.first_utc,
                last_utc: row.last_utc,
                review: self.review.to_owned(),
                live: row.live,
                lineage_ref: row.lineage_ref.clone(),
                lineage_seq: row.lineage_seq,
            };
            let totals = Totals {
                n_files: row.n_files,
                n_add: row.n_add,
                n_del: row.n_del,
                n_binary: row.n_binary,
                churn: 0,
                discounted_churn: 0,
                reviewable_churn: 0,
                files_truncated: self.truncated,
                files_read: 0,
                reconciles: None,
            };
            let changes: Vec<FileChange<'_>> = self
                .files
                .iter()
                .map(|file| FileChange {
                    path: &file.path,
                    add: u64::try_from(file.add.max(0)).unwrap_or(u64::MAX),
                    del: u64::try_from(file.del.max(0)).unwrap_or(u64::MAX),
                    binary: file.binary,
                    path_class: &file.path_class,
                    noise_class: &file.noise_class,
                })
                .collect();
            let ranking = risk::rank(&risk::Signals {
                files: &changes,
                n_add: u64::try_from(row.n_add.max(0)).unwrap_or(u64::MAX),
                n_del: u64::try_from(row.n_del.max(0)).unwrap_or(u64::MAX),
                n_files: u64::try_from(row.n_files.max(0)).unwrap_or(u64::MAX),
                n_binary: u64::try_from(row.n_binary.max(0)).unwrap_or(u64::MAX),
                n_commits: u64::try_from(row.n_commits()).unwrap_or(u64::MAX),
                partial_files: self.truncated,
                complexity: self.complexity.as_ref().map(super::risk_complexity),
            });
            Input {
                episode,
                files: self.files.clone(),
                totals,
                complexity: self.complexity.clone(),
                risk: Some(RiskView::of(&ranking)),
                provenance: Provenance {
                    source: "store".to_owned(),
                    store: None,
                    epoch: Some(row.epoch),
                    asked: None,
                    produced_by: Provenance::produced_by(),
                    files_detail: self.detail.to_owned(),
                    models: models(&self.supplied),
                },
                supplied: self.supplied.clone(),
            }
        }
    }

    /// Which analysis produced which number in this document.
    ///
    /// [`Provenance::models`] is the static answer; a lent section adds its own, because "the card says 12
    /// transitive packages" has to be traceable to the module that measured it (ADR-004 §5.1). The
    /// complexity entry is not here on purpose: its basis token is derived from the roll-up itself, so
    /// [`compose`] writes it and every way of composing a document says the same thing.
    fn models(supplied: &Supplied) -> BTreeMap<String, String> {
        let mut models = Provenance::models();
        if supplied.dependencies.is_some() {
            models.insert(
                "dependencies".to_owned(),
                "deps::episode_repository (e2.6)".to_owned(),
            );
        }
        models
    }

    /// One path across several member commits, folded.
    ///
    /// The two taxonomy fields start empty rather than `unknown` so [`Aggregated::absorb`] can tell "no row
    /// yet" from "a row that said `unknown`"; every path the fold sees has at least one row by the time a
    /// `FileDelta` is built out of it.
    #[derive(Default)]
    struct Aggregated {
        path: String,
        old_path: Option<String>,
        kinds: Vec<String>,
        add: i64,
        del: i64,
        commits: i64,
        binary: bool,
        path_class: String,
        noise_class: String,
    }

    impl Aggregated {
        fn absorb(
            &mut self,
            path: &str,
            old_path: &Option<String>,
            kind: &str,
            file: &fathom_store::spine::FileRow,
        ) {
            self.path = path.to_owned();
            if old_path.is_some() && self.old_path.is_none() {
                self.old_path = old_path.clone();
            }
            self.kinds.push(kind.to_owned());
            self.add = self.add.saturating_add(file.n_add);
            self.del = self.del.saturating_add(file.n_del);
            // The taxonomy travels with the row, and a fold may not drop it: ADR-004 §6.6 R3 forbids
            // re-deriving a stored class downstream, which only means something if the stored class
            // survives the per-commit rows into the per-path one the sections read. Empty is a fresh fold,
            // and the *first* commit's answer wins; a later commit that classified the path keeps it.
            if self.commits == 0 || self.path_class == "unknown" {
                self.path_class = file.path_class.clone();
            }
            if self.commits == 0 || self.noise_class == "unknown" {
                self.noise_class = file.noise_class.clone();
            }
            self.commits += 1;
            self.binary |= file.is_binary;
        }

        fn into_file(self) -> FileDelta {
            // Git's rename assertion outranks everything else the path did inside the episode: a file that
            // moved and then changed is one move, which is what a reviewer needs to know.
            let change = ["renamed", "deleted", "added", "mode-only", "modified"]
                .into_iter()
                .find(|wanted| self.kinds.iter().any(|kind| kind == *wanted))
                .unwrap_or("modified")
                .to_owned();
            let surface = risk::classify(&self.path, &self.path_class, &self.noise_class)
                .as_str()
                .to_owned();
            FileDelta {
                path: self.path,
                old_path: self.old_path,
                change,
                add: self.add,
                del: self.del,
                binary: self.binary,
                commits: self.commits,
                path_class: self.path_class,
                noise_class: self.noise_class,
                surface,
                basis: Provenance::DETAIL_FULL.to_owned(),
            }
        }
    }
}

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub use store_bridge::{
    episode_complexity, for_commit, for_commit_with_repo, for_episode, for_episode_with_repo,
    for_rows,
};

/// The smallest identity a caller can name.
///
/// Every unstated fact is `unknown` rather than a plausible default — an `agent` author, a `read` review,
/// a boundary rule from the last episode in the file. A fixture that does not know is a fixture that says
/// it does not know, which is the rule the rest of the model follows.
impl EpisodeRef {
    /// An identity with an id, a summary, and nothing else claimed.
    #[must_use]
    pub fn sparse(id: impl Into<String>, summary: impl Into<String>) -> Self {
        EpisodeRef {
            id: id.into(),
            fingerprint: String::new(),
            fallback_fingerprint: String::new(),
            shape_fingerprint: String::new(),
            members: Vec::new(),
            member_remainder: None,
            n_commits: 1,
            summary: summary.into(),
            author_kind: "unknown".to_owned(),
            author_name: String::new(),
            author_email: String::new(),
            author_confidence: None,
            attribution: None,
            intent: None,
            boundary_rule: String::new(),
            boundary_confidence: None,
            session: None,
            turn: None,
            first_utc: 0,
            last_utc: 0,
            review: "unread".to_owned(),
            live: true,
            lineage_ref: String::new(),
            lineage_seq: 0,
        }
    }
}

impl EpisodeDelta {
    /// The smallest honest document: a sparse identity, no rows, and the ranking the caller happens to
    /// have. What a test or a fixture builds when it has a score and nothing else — and the sections say
    /// *no rows were given*, which is the truth, rather than the zero a default would print.
    #[must_use]
    pub fn sparse(id: &str, summary: &str, risk: Option<RiskView>) -> Self {
        let mut input = Input::new(EpisodeRef::sparse(id, summary));
        input.risk = risk;
        compose(&input)
    }
}

// ---------------------------------------------------------------------------
// the brief: one row per section
// ---------------------------------------------------------------------------

/// One section, as one row.
///
/// The card has fourteen rows and seven questions, so the card is the brief and `fathom analyze` is the
/// document. `e2.10`'s Python card has the same rule for the same reason, and the glyphs are the same
/// because a reviewer who reads both must not have to learn two legends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefRow {
    /// The section id.
    pub id: String,
    /// One of [`state`]'s three.
    pub state: String,
    /// `x` measured, `~` partial, `?` unavailable — in the text channel, never only in a colour
    /// (`ADR-006 §10.2`).
    pub glyph: char,
    /// The headline value: the first figure, plus a counted remainder, or the reason nothing was measured.
    pub value: String,
    /// How many figures the row did not show.
    pub more: u64,
    /// The plan key that owns the answer, for the `?` rows.
    pub owner: String,
}

impl BriefRow {
    /// `~ what` — the glyph and the id, which is what a row's first cell prints.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} {}", self.glyph, self.id)
    }
}

/// The state's glyph: `x` measured, `~` partial, `?` unavailable.
#[must_use]
pub fn glyph_of(state: &str) -> char {
    match state {
        STATE_MEASURED => 'x',
        STATE_PARTIAL => '~',
        _ => '?',
    }
}

/// One row per section: the first thing it can say, and how much it did not say.
///
/// Shared by the TUI card and the CLI's text mode so that the two cannot word the same section
/// differently. Neither adds a fact: the row is the section's own first figure, or its own gap.
#[must_use]
pub fn brief(delta: &EpisodeDelta) -> Vec<BriefRow> {
    delta
        .sections
        .iter()
        .map(|section| {
            let first = section
                .figures
                .iter()
                .find(|figure| figure.known)
                .or(section.figures.first());
            // A row's value is the section's first *measured* figure; when there is none, it is the gap's
            // short form — never its long reason, which is a paragraph and does not fit in a card. The full
            // reason is still in the document, and `fathom analyze` prints it.
            let mut value = match first {
                Some(figure) if figure.known => figure.value.clone(),
                _ => section.gap.as_ref().map_or_else(
                    || {
                        first
                            .and_then(|figure| figure.reason.clone())
                            .unwrap_or_else(|| "nothing to report".to_owned())
                    },
                    |gap| format!("{} ({})", gap.short, gap.owner),
                ),
            };
            let more = u64::try_from(section.figures.len().saturating_sub(1)).unwrap_or(u64::MAX)
                + section.remainder.unwrap_or(0);
            if more > 0 {
                value.push_str(&format!(" · +{more} more"));
            }
            BriefRow {
                id: section.id.clone(),
                state: section.state.clone(),
                glyph: glyph_of(&section.state),
                value,
                more,
                owner: section
                    .gap
                    .as_ref()
                    .map_or_else(|| section.owner.clone(), |gap| gap.owner.clone()),
            }
        })
        .collect()
}

impl RiskView {
    /// A ranked row with only a score — what a fixture or a test hands the rail when it wants the column
    /// filled and nothing else. Printed as an unexplained number rather than with invented reasons.
    #[must_use]
    pub fn scored(score: u64) -> Self {
        RiskView {
            model: String::new(),
            score: i64::try_from(score).unwrap_or(i64::MAX),
            vector: String::new(),
            components: Vec::new(),
            surfaces: Vec::new(),
            caveat: None,
            blind_spots: Vec::new(),
        }
    }

    /// The ceiling, counted: what this ranking cannot see, in one line a card has room for.
    ///
    /// `None` when the ranking came with no blind spots stated, which is a ranking nobody has written the
    /// honest ceiling for yet.
    #[must_use]
    pub fn blind_spot_note(&self) -> Option<String> {
        let hidden = self.blind_spots.len();
        (hidden > 0).then(|| {
            format!(
                "a spine cannot see {hidden} thing{} — `w` lists them all",
                if hidden == 1 { "" } else { "s" }
            )
        })
    }

    /// What to do with the ceiling, in the model's own voice: the line a surface prints *after* it has
    /// listed what the ranking cannot see.
    ///
    /// [`blind_spot_note`] counts the ceiling for a pane that has room for one row; this is what the
    /// counted ceiling means, and it belongs beside it rather than in whichever pane expands the list
    /// (`reco-5js3.3`, ADR-006 §5.1.1 rule 2). `None` when there is no ceiling to explain.
    ///
    /// [`blind_spot_note`]: Self::blind_spot_note
    #[must_use]
    pub fn ceiling_note(&self) -> Option<String> {
        (!self.blind_spots.is_empty()).then(|| risk::CEILING_NOTE.to_owned())
    }

    /// The surfaces as a row's badge: `migration+ci`, capped at three with a counted remainder.
    ///
    /// Moved here from `fathom-cli` in `e2.11`: the badge is how the model words its own output, and a
    /// second copy in the composition root is how a rail and a card stop agreeing about the same episode.
    #[must_use]
    pub fn badge(&self) -> String {
        if self.surfaces.is_empty() {
            return String::new();
        }
        let shown: Vec<&str> = self.surfaces.iter().map(String::as_str).take(3).collect();
        let rest = self.surfaces.len().saturating_sub(shown.len());
        if rest > 0 {
            format!("{} +{rest}", shown.join("+"))
        } else {
            shown.join("+")
        }
    }

    /// One line per component that contributed, largest first, plus the caveat if the read was capped.
    #[must_use]
    pub fn reasons(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .components
            .iter()
            .map(|component| {
                format!(
                    "{} {}/{} — {}",
                    component.name, component.points, component.cap, component.detail
                )
            })
            .collect();
        if let Some(caveat) = &self.caveat {
            out.push(format!("caveat — {caveat}"));
        }
        out
    }

    /// The arithmetic line, or a bare number when there is no arithmetic to show.
    #[must_use]
    pub fn headline(&self) -> String {
        if self.vector.is_empty() {
            format!("risk {}", self.score)
        } else {
            self.vector.clone()
        }
    }
}

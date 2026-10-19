//! `fathom_analyze::drift` — plan versus execution: what the agent said it would do, measured against
//! what the episodes did (`e4.8`, `reco-cpr`).
//!
//! The delta model answers what an episode did. [`crate::insight`]'s intent block answers what was asked
//! for, in the asker's own words ([`crate::scope`] answers whether the change stayed inside the declared
//! scope, and [`crate::detectors`] turns individual pathologies into findings). What none of them answers is
//! the sentence a reviewer actually forms while reading an agent's work:
//!
//! > *It said it would do four things. I can find two of them.*
//!
//! That is a **session** question, not an episode question: a plan item deferred to the next turn is not
//! drift, a dependency added two turns after the plan that named it is not drift, and a "refactor the
//! parser" that touched only the parser's tests is. So this module reads a session — the episodes a harness
//! recorded in order, each with its recorded intent, its touched paths and any sign-off the ledger holds for
//! it — and reports the six contradictions the ticket named, each with **both sides linked**: the plan text
//! that says one thing, and the diff, scope or ledger entry that says another.
//!
//! # The six kinds, and what each one means operationally
//!
//! The closed vocabulary is [`KINDS`]; each is one function in [`detect`] and each is listed in
//! [`docs/concepts/plan-drift.md`](../../../docs/concepts/plan-drift.md) with the threshold it uses.
//!
//! * **`plan-unimplemented`** — an open plan item with nothing resembling it in this episode or the next
//!   `policy.window` episodes of the same session. "Resembling" is token overlap against the corpus of
//!   subjects, paths and dependency names in that window, and the threshold is published rather than
//!   tuned per case.
//! * **`out-of-scope-edit`** — a touched path outside the declared scope. This is [`crate::scope`]'s answer,
//!   re-raised at the session level with the glob quoted beside the path, because an episode-scoped finding
//!   and a session-level contradiction are different review acts.
//! * **`unplanned-dependency`** — a dependency manifest was touched and neither the plan nor the scope
//!   accounts for it. The dependency *delta* is `e2.6`'s; this build can see the manifest and not the
//!   resolved graph, and every finding says so instead of pretending otherwise.
//! * **`test-promised-not-written`** — a plan item about tests, still open, with no test-surface path
//!   touched in the window.
//! * **`refactor-claimed-not-performed`** — a plan item that claims a structural change (refactor, rename,
//!   move, split, extract) to a named target, with nothing in the window touching that target and no path
//!   move recorded: the surface is unchanged.
//! * **`contradicts-signoff`** — a later plan item re-opens work an earlier episode of the same session was
//!   **signed off** as doing. This is the one kind that involves `e4.6`: the sign-off is durable, so the
//!   contradiction is too, and both entries are cited.
//!
//! # What this module refuses
//!
//! **No reading of the world.** No store, no clock, no config: [`detect::analyse`] is a pure function of the
//! sessions it is handed, and `fathom-cli` reads all three and passes them over (ADR-002 §4.4, §4.5). That
//! is what makes every finding here a fixture.
//!
//! **No finding about an episode that stated nothing.** An episode with no recorded intent has no plan, and
//! a detector that inferred one from the diff would be reporting its own guess. Those episodes are counted
//! in [`Report::unchecked`] with the reason, which is the difference between a clean report and an empty
//! one (ADR-001 §7.2: a refusal is a pointer).
//!
//! **No re-reporting of a dismissed contradiction.** Every finding has a suppression key, and a key the
//! caller has dismissed ([`Policy::dismissed`]) produces no finding and one line in
//! [`Report::suppressed`] — counted, not silently dropped. Dismissal has to survive the rewrite that a
//! *finding* would not, so the key is built from the plan text and the target, not from an episode id.
//!
//! **No confidence overreach.** A plan that arrived as a hook prompt is a stronger claim than a plan that
//! arrived as an `Intent:` trailer, and the intent ladder already prices that ([`fathom_ingest::intent`]).
//! Every finding's confidence is that rung's, scaled by the detector's own base, and the message says which
//! rung decided. A trailer-only repository still gets findings — the ticket's fourth criterion — at prices
//! that admit it.
//!
//! # Bounded by construction
//!
//! The window is finite, the findings per kind and per report are capped, cross-session comparison is off
//! unless the caller turns it on and stays bounded when it does, and a plan item is reported at most once
//! per session whatever fired it. A drift report that grows with the square of a session is a report nobody
//! reads.

#![deny(missing_docs)]

mod detect;

use serde::Deserialize;
use serde::Serialize;

pub use crate::scope::Deviation;
pub use detect::{analyse, overlap, tokens};

/// The contract id of a drift report, so a payload says which rules produced it.
pub const CONTRACT: &str = "fathom-drift/1";

/// The version of [`CONTRACT`]'s shape.
pub const VERSION: u32 = 1;

/// The plan key that owns this analysis, printed on every finding.
pub const OWNER: &str = "e4.8";

/// The default look-ahead, in episodes, for "was this ever done?".
pub const DEFAULT_WINDOW: usize = 4;

/// The default cap on findings of one kind in one report.
pub const DEFAULT_MAX_PER_KIND: usize = 6;

/// The default cap on the whole report.
pub const DEFAULT_MAX_FINDINGS: usize = 20;

/// The token overlap at which a plan item counts as *addressed* by a change.
///
/// Published because the whole module hangs off it. Overlap is `|A∩B| / min(|A|,|B|)` over content tokens —
/// stop-words removed, paths split on punctuation — which makes a short item ("bump the parser") and a long
/// corpus a fair comparison. `0.34` came from the corpus in `crates/fathom-analyze/tests/drift.rs`: below
/// it, the corpus's "not done" items started matching their way out of a finding; above `0.5`, the ones
/// that were genuinely done started failing to be recognised.
pub const DEFAULT_MIN_OVERLAP: f64 = 0.34;

/// The six kinds, in the order a reviewer reads them: what was not done, what was done outside the plan,
/// and what contradicts a decision a human already made.
///
/// `(id, plain name, the question it answers, default severity)`.
pub const KINDS: [(&str, &str, &str, &str); 6] = [
    (
        "plan-unimplemented",
        "planned, never done",
        "Which open plan items have no corresponding change in the window?",
        "notable",
    ),
    (
        "test-promised-not-written",
        "tests promised, none written",
        "Which plan items about tests left the test surface untouched?",
        "high",
    ),
    (
        "refactor-claimed-not-performed",
        "refactor claimed, surface unchanged",
        "Which claimed structural changes touched nothing they named?",
        "notable",
    ),
    (
        "out-of-scope-edit",
        "edited outside the declared scope",
        "Which touched paths are outside the globs the work was meant to stay inside?",
        "notable",
    ),
    (
        "unplanned-dependency",
        "dependency outside the plan",
        "Which dependency manifests were touched without the plan or the scope accounting for them?",
        "notable",
    ),
    (
        "contradicts-signoff",
        "contradicts a sign-off",
        "Which plan items re-open work an earlier signed-off episode of this session already covered?",
        "high",
    ),
];

/// The default severity a kind ships with, or `note` for an unknown kind (a newer build's addition).
#[must_use]
pub fn severity_of(kind: &str) -> &'static str {
    KINDS
        .iter()
        .find(|(id, _, _, _)| *id == kind)
        .map_or("note", |(_, _, _, severity)| *severity)
}

/// The plain name of a kind, for a surface that prints a heading rather than an id.
#[must_use]
pub fn name_of(kind: &str) -> String {
    KINDS
        .iter()
        .find(|(id, _, _, _)| *id == kind)
        .map_or_else(|| kind.to_owned(), |(_, name, _, _)| (*name).to_owned())
}

/// The question a kind answers, printed with the findings so the reader knows what was asked.
#[must_use]
pub fn question_of(kind: &str) -> &'static str {
    KINDS
        .iter()
        .find(|(id, _, _, _)| *id == kind)
        .map_or("", |(_, _, question, _)| *question)
}

/// Which side of a contradiction a piece of evidence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SideKind {
    /// The plan, in the asker's or the harness's words.
    Plan,
    /// A path or a diff the change actually made.
    Diff,
    /// A declared scope glob.
    Scope,
    /// A dependency manifest that was touched.
    Dependency,
    /// A ledger sign-off (`e4.6`).
    Ledger,
    /// The absence of something: the window's corpus, with the overlap that failed.
    Absence,
}

impl SideKind {
    /// The published spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SideKind::Plan => "plan",
            SideKind::Diff => "diff",
            SideKind::Scope => "scope",
            SideKind::Dependency => "dependency",
            SideKind::Ledger => "ledger",
            SideKind::Absence => "absence",
        }
    }
}

/// One side of a finding: what was said, or what was done, with the address a reviewer opens.
///
/// A finding is a *pair* of these. A contradiction with one side is an opinion; with both, it is something
/// you can go and check, which is the whole reason `e4.8`'s acceptance criteria ask for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Side {
    /// Which kind of side this is.
    pub kind: SideKind,
    /// The episode it belongs to — an id for a plan or a diff, an entry id for a ledger row.
    pub episode: String,
    /// The quotation: the plan item's text, the path, the glob, the sign-off's note. Never a paraphrase.
    pub quote: String,
    /// Where to look. A `fathom` verb is preferred; a path with a line range is acceptable; a
    /// `#/…` pointer is what an in-document side carries.
    pub address: String,
}

impl Side {
    /// The plan side: the item, quoted, in the episode that stated it.
    #[must_use]
    pub fn plan(episode: impl Into<String>, quote: impl Into<String>) -> Self {
        let episode = episode.into();
        Side {
            kind: SideKind::Plan,
            address: format!("fathom narrative {episode}"),
            episode,
            quote: quote.into(),
        }
    }

    /// The evidence side: a path, or a path with a range, in the episode that touched it.
    #[must_use]
    pub fn diff(episode: impl Into<String>, path: impl Into<String>) -> Self {
        let episode = episode.into();
        let path = path.into();
        Side {
            kind: SideKind::Diff,
            address: format!("fathom show {episode} # {path}"),
            episode,
            quote: path,
        }
    }

    /// The absence side: the window that was searched, and nothing found in it.
    #[must_use]
    pub fn absence(episode: impl Into<String>, quote: impl Into<String>) -> Self {
        let episode = episode.into();
        Side {
            kind: SideKind::Absence,
            address: format!("fathom show {episode}"),
            episode,
            quote: quote.into(),
        }
    }

    /// A ledger side: the sign-off the contradiction runs into.
    #[must_use]
    pub fn ledger(episode: impl Into<String>, quote: impl Into<String>) -> Self {
        let episode = episode.into();
        Side {
            kind: SideKind::Ledger,
            address: format!("fathom review list #{episode}"),
            episode,
            quote: quote.into(),
        }
    }

    /// A declared-scope side.
    #[must_use]
    pub fn scope(episode: impl Into<String>, glob: impl Into<String>) -> Self {
        let episode = episode.into();
        Side {
            kind: SideKind::Scope,
            address: format!("fathom show {episode}"),
            episode,
            quote: glob.into(),
        }
    }
}

/// One contradiction: what was said, what was done, and how sure this build is that they disagree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// One of [`KINDS`]' ids.
    pub kind: String,
    /// The version of the rule that raised it, so a re-run after a threshold change is distinguishable.
    pub rule_version: u32,
    /// The episode the finding is about — the one whose plan or change is in question.
    pub episode: String,
    /// The session it was found in, when a session was known. `None` is a lineage-scoped comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// What the finding is about within the episode: the plan item, the path, the dependency.
    pub subject: String,
    /// The claim, one sentence, both sides named.
    pub message: String,
    /// How much attention it asks for: [`severity_of`]'s vocabulary, unchanged.
    pub severity: String,
    /// `0..=1`. The intent rung's price scaled by the rule's own base — never a bare 1.0.
    pub confidence: f64,
    /// How the intent was recorded, in the ladder's own spelling, because it is what the confidence means.
    pub intent_source: String,
    /// Side A: what was said.
    pub claim: Side,
    /// Side B: what the history shows instead.
    pub evidence: Side,
    /// The second piece of evidence, when the rule had one — the ledger entry, the scope glob.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corroborating: Option<Side>,
    /// The one key that silences this finding, and the only thing a dismissal is stored against.
    pub suppression_key: String,
    /// The owning plan key.
    pub owner: String,
}

impl Finding {
    /// The severity's published spelling for the delta model, which uses `high`/`notable`/`note` — the same
    /// words this module uses, so no fourth vocabulary appears.
    #[must_use]
    pub fn to_insight(&self) -> crate::insight::Finding {
        // Both sides go in the message: the delta document's `Evidence` is a pointer and this finding has
        // two pointers, so the second travels in the text rather than being dropped.
        let message = format!(
            "{} — said: “{}” ({}); history: “{}” ({})",
            self.message,
            self.claim.quote,
            self.claim.address,
            self.evidence.quote,
            self.evidence.address
        );
        crate::insight::Finding {
            kind: self.kind.clone(),
            subject: self.subject.clone(),
            message,
            severity: self.severity.clone(),
            evidence: crate::insight::Evidence::external(self.evidence.address.clone()),
            owner: self.owner.clone(),
        }
    }
}

/// How one intent was recorded, as the report needs to say it.
///
/// The rung and its price come from the intent document itself ([`fathom_ingest::intent`]); this is the
/// pair of them in the shape this module carries, so a finding can print "the plan was a trailer" without
/// reaching back into the store.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rung {
    /// The rung's spelling: `hook-prompt`, `agent-intent-trailer`, `plan-item`, …
    #[serde(default)]
    pub source: String,
    /// What that rung alone prices the claim at, `0..=1`.
    #[serde(default)]
    pub confidence: f64,
    /// Whether the rung states what was asked in prose, as opposed to naming it or proving it happened.
    #[serde(default)]
    pub stated: bool,
}

/// One episode, as drift sees it.
///
/// Deliberately small: the identity, what was asked for, what was touched, and whether a human signed it
/// off. Anything else a detector wanted would be a second engine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    /// The episode identity.
    pub id: String,
    /// The subject line, used as the corpus a plan item is matched against.
    pub summary: String,
    /// Position within its session, 0-based. Ordering is the caller's job — this module never reads a clock.
    pub ordinal: usize,
    /// The recorded plan, or `None` when nothing stated one. [`Report::unchecked`] is where the `None`s go.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,
    /// The paths this episode touched, as the store spells them.
    #[serde(default)]
    pub touched: Vec<String>,
    /// Whether any touched path is a test-surface path. `None` is "the caller could not tell", which is
    /// reported rather than guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests_touched: Option<bool>,
    /// Whether the index recorded a path move (rename) for this episode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_paths: Option<bool>,
    /// The dependency manifests this episode touched, as the paths themselves.
    #[serde(default)]
    pub manifests: Vec<String>,
    /// A sign-off the ledger holds for this episode, quoted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signoff: Option<String>,
}

/// What was asked for, in one bundle: the plan items, the declared scope, and the rung that said so.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// The items, in the order they were stated. Each carries its own done/not-done state as the harness or
    /// the trailer spelled it.
    #[serde(default)]
    pub items: Vec<PlanItem>,
    /// The declared scope globs, already normalised.
    #[serde(default)]
    pub scope: Vec<String>,
    /// The intent text, when there was one — the "what was this for" line, quoted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// How all of this was recorded.
    pub rung: Rung,
}

/// One plan item, as stated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    /// The item's text, in the asker's words.
    pub text: String,
    /// Whether the harness (or the trailer) says it is finished. An item with no status is **open**: an
    /// unclaimed "still to do" is the honest reading of a bare plan line.
    pub done: bool,
}

impl PlanItem {
    /// An open item.
    #[must_use]
    pub fn open(text: impl Into<String>) -> Self {
        PlanItem {
            text: text.into(),
            done: false,
        }
    }

    /// An item the harness marked finished.
    #[must_use]
    pub fn done(text: impl Into<String>) -> Self {
        PlanItem {
            text: text.into(),
            done: true,
        }
    }
}

/// One session: the episodes a harness recorded, in order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// The harness session id, or the lineage ref when no session was named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The episodes, oldest first. The caller orders them; this module never sorts by time.
    pub episodes: Vec<Episode>,
}

/// What to compare, how far to look, and what has already been dismissed.
///
/// Every knob is a bound. None of them changes what is true; they change how much of it arrives at once.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// How many episodes past the item's own episode to look for the change that did it.
    pub window: usize,
    /// The token overlap at which an item counts as addressed. See [`DEFAULT_MIN_OVERLAP`].
    pub min_overlap: f64,
    /// The cap per kind, per report.
    pub max_per_kind: usize,
    /// The cap on the report.
    pub max_findings: usize,
    /// Compare across sessions when an episode names no session. Off by default: a plan from Monday's
    /// harness run is a different conversation than today's, and a cross-session report that reuses one
    /// session's plan is the false positive this ticket's third criterion is about.
    pub cross_session: bool,
    /// Suppression keys a reviewer has already dismissed. A finding whose key is here is counted in
    /// [`Report::suppressed`] and is not reported.
    pub dismissed: std::collections::BTreeSet<String>,
    /// Kinds the caller has switched off, in [`KINDS`]' spelling.
    pub off: std::collections::BTreeSet<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            window: DEFAULT_WINDOW,
            min_overlap: DEFAULT_MIN_OVERLAP,
            max_per_kind: DEFAULT_MAX_PER_KIND,
            max_findings: DEFAULT_MAX_FINDINGS,
            cross_session: false,
            dismissed: std::collections::BTreeSet::new(),
            off: std::collections::BTreeSet::new(),
        }
    }
}

impl Policy {
    /// A policy from the values a caller read out of `fathom.toml`, clamped to what this build will honour.
    ///
    /// Clamping is the point: a window of 0 would mean "never look", and a window of 10 000 would mean "the
    /// whole repository", neither of which is what a reviewer who mistyped a knob meant.
    #[must_use]
    pub fn from_values(
        window: Option<usize>,
        min_overlap: Option<f64>,
        max_per_kind: Option<usize>,
        max_findings: Option<usize>,
        cross_session: Option<bool>,
        dismissed: &[String],
        off: &[String],
    ) -> Self {
        Policy {
            window: window.unwrap_or(DEFAULT_WINDOW).clamp(1, 24),
            min_overlap: min_overlap.unwrap_or(DEFAULT_MIN_OVERLAP).clamp(0.05, 1.0),
            max_per_kind: max_per_kind.unwrap_or(DEFAULT_MAX_PER_KIND).clamp(1, 50),
            max_findings: max_findings.unwrap_or(DEFAULT_MAX_FINDINGS).clamp(1, 200),
            cross_session: cross_session.unwrap_or(false),
            dismissed: dismissed.iter().cloned().collect(),
            off: off.iter().cloned().collect(),
        }
    }

    /// The knobs, as the report prints them, so a reader can tell which policy produced which findings.
    #[must_use]
    pub fn as_rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("window", self.window.to_string()),
            ("min_overlap", format!("{:.2}", self.min_overlap)),
            ("max_per_kind", self.max_per_kind.to_string()),
            ("max_findings", self.max_findings.to_string()),
            ("cross_session", self.cross_session.to_string()),
            ("dismissed", self.dismissed.len().to_string()),
            (
                "kinds off",
                if self.off.is_empty() {
                    "none".to_owned()
                } else {
                    self.off.iter().cloned().collect::<Vec<_>>().join(",")
                },
            ),
        ]
    }
}

/// An episode or session that could not be compared, and why.
///
/// The report's other half. A drift report with no findings and no unchecked list claims the history was
/// clean; this is what keeps the honest version honest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unchecked {
    /// What was skipped — an episode id, or a session's description.
    pub subject: String,
    /// Why. The published spellings are in the module docs; the reason text is theirs.
    pub reason: String,
    /// The plan key that owns making this checkable, when one owns it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

/// One kind's tally: how many fired, how many were dismissed, how many the cap cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// The kind.
    pub kind: String,
    /// Findings reported.
    pub reported: usize,
    /// Findings silenced by a dismissal the reviewer already made.
    pub dismissed: usize,
    /// Findings the per-kind cap cut, counted rather than dropped.
    pub capped: usize,
    /// Whether the kind is switched off by policy at all.
    pub enabled: bool,
}

/// A drift report: the findings, the bounds that produced them, and what could not be checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// [`CONTRACT`].
    pub contract: String,
    /// [`VERSION`].
    pub schema_version: u32,
    /// The policy that ran, printed so a report can be reproduced.
    pub policy: Vec<(String, String)>,
    /// Sessions examined.
    pub sessions: usize,
    /// Episodes examined.
    pub episodes: usize,
    /// Episodes that stated a plan — the denominator for every "no findings" sentence.
    pub planned: usize,
    /// The findings, in the order a reviewer should read them: severity, then session order.
    pub findings: Vec<Finding>,
    /// Per-kind tallies, in [`KINDS`]' order.
    pub tallies: Vec<Tally>,
    /// What was dismissed, by key. Not findings: history.
    pub suppressed: Vec<String>,
    /// What could not be compared, and who owns making it comparable.
    pub unchecked: Vec<Unchecked>,
    /// Whether any finding was cut by a cap. Printed loudly, because a clean-looking report that was
    /// truncated is the report that hides the thing you were looking for.
    pub truncated: bool,
}

impl Report {
    /// The empty report, in the shape a surface can print without special-casing nothing.
    #[must_use]
    pub fn empty(policy: &Policy) -> Self {
        Report {
            contract: CONTRACT.to_owned(),
            schema_version: VERSION,
            policy: policy
                .as_rows()
                .into_iter()
                .map(|(key, value)| ((*key).to_owned(), value))
                .collect(),
            sessions: 0,
            episodes: 0,
            planned: 0,
            findings: Vec::new(),
            tallies: Vec::new(),
            suppressed: Vec::new(),
            unchecked: Vec::new(),
            truncated: false,
        }
    }

    /// The one-line verdict a shell prints, and a gate reads.
    ///
    /// "No findings" is never printed as "clean" unless the report actually compared something: `nothing to
    /// compare` is a different fact and the reviewer needs to hear which one it is.
    #[must_use]
    pub fn verdict(&self) -> String {
        if self.planned == 0 {
            return format!(
                "nothing to compare: {} episode(s) examined and none stated a plan",
                self.episodes
            );
        }
        let findings = self.findings.len();
        let base = if findings == 0 {
            format!(
                "no contradictions across {} planned episode(s) in {} session(s)",
                self.planned, self.sessions
            )
        } else {
            format!(
                "{findings} contradiction(s) across {} planned episode(s) in {} session(s)",
                self.planned, self.sessions
            )
        };
        if self.truncated {
            format!("{base} — the caps cut some; raise --max or narrow --window")
        } else if self.suppressed.is_empty() {
            base
        } else {
            format!(
                "{base} · {} dismissed contradiction(s) not re-reported",
                self.suppressed.len()
            )
        }
    }

    /// The report as one line, for `--json` — keys sorted, byte-stable.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }

    /// The findings as the delta model's findings, so a caller can lend them to
    /// [`crate::insight::Supplied::findings`] and have them appear in the episode's narrative and card
    /// through the door every other finding already uses.
    #[must_use]
    pub fn to_insight(&self) -> Vec<crate::insight::Finding> {
        self.findings.iter().map(Finding::to_insight).collect()
    }

    /// The report as lines, for a terminal: the verdict, the policy, then one block per kind with both
    /// sides printed under the claim.
    #[must_use]
    pub fn render(&self) -> Vec<String> {
        let mut out = vec![format!("{} [{}]", self.verdict(), self.contract)];
        out.push(format!(
            "  {} episode(s) in {} session(s); {} stated a plan",
            self.episodes, self.sessions, self.planned
        ));
        out.push("  policy:".to_owned());
        for (key, value) in &self.policy {
            out.push(format!("    {key} = {value}"));
        }
        if self.findings.is_empty() {
            out.push("  no findings".to_owned());
        }
        let mut current: Option<&str> = None;
        for finding in &self.findings {
            if current != Some(finding.kind.as_str()) {
                out.push(format!("{}:", name_of(&finding.kind)));
                current = Some(finding.kind.as_str());
            }
            out.push(format!(
                "  [{}] {} — {}",
                finding.severity, finding.subject, finding.message
            ));
            out.push(format!(
                "      said     “{}” ({})",
                finding.claim.quote, finding.claim.address
            ));
            out.push(format!(
                "      history  “{}” ({})",
                finding.evidence.quote, finding.evidence.address
            ));
            if let Some(extra) = &finding.corroborating {
                out.push(format!(
                    "      and      “{}” ({})",
                    extra.quote, extra.address
                ));
            }
            out.push(format!(
                "      confidence {:.2} via {}; dismiss with `{}`",
                finding.confidence, finding.intent_source, finding.suppression_key
            ));
        }
        if !self.unchecked.is_empty() {
            out.push("not compared:".to_owned());
            for skip in &self.unchecked {
                let owner = skip
                    .owner
                    .as_ref()
                    .map_or_else(String::new, |key| format!(" (owner {key})"));
                out.push(format!("  {} — {}{owner}", skip.subject, skip.reason));
            }
        }
        if !self.suppressed.is_empty() {
            out.push(format!(
                "dismissed, not re-reported: {}",
                self.suppressed.join(", ")
            ));
        }
        out
    }
}

//! One finding, in the shape every output format needs (`e6.6`, `reco-ngk`).
//!
//! The gate has five audiences and one fact: a human reading a CI log, a machine reading `--json`,
//! GitHub's code-scanning upload (SARIF), GitHub's step annotations, and the markdown job summary. If
//! each of them built its own finding shape the five would disagree about what was found, which is the
//! exact failure ADR-006 §5.1 refuses — one fact, one wording. So [`Finding`] is the only place a
//! finding is assembled, and every renderer in [`super::render`] and [`super::sarif`] is a projection of
//! it.
//!
//! # The four fields that are not decoration
//!
//! * **`rule`** — a stable id, not a sentence. SARIF requires one (a result must name a rule the driver
//!   declares), `check.ignore` matches on it, and a reviewer who has seen this failure before has to be
//!   able to grep for it. A finding whose only identity is its prose cannot be silenced without
//!   switching its whole check off.
//! * **`message`** — one sentence, with the names in it. It is the SARIF `message.text`, the step
//!   annotation body and the line in the log, and the acceptance criterion ("the *why did CI fail*
//!   message is one sentence long and points at the evidence") is written against it.
//! * **`evidence`** — where the claim came from, and how to check it. A finding with no evidence is an
//!   opinion; a finding whose evidence is `Cargo.lock` and a hunk header is a claim a skeptic can verify
//!   in one `git show` (ADR-001 §7.10).
//! * **`fingerprint`** — a stable hash of (check, rule, message, primary location). SARIF's
//!   `partialFingerprints` wants one, and it is what lets the same finding on the same line across two
//!   pushes be the *same* finding in GitHub's UI instead of a new alert each time.
//!
//! # What a finding is *not*
//!
//! **Not a violation of a rule that already existed.** Every check here reports what the range did; the
//! ratchet ledger (`.fathom/ratchet.toml`, [`crate::ratchet`]) decides what the repository has already
//! agreed to forgive, and a baselined violation never reaches this type. That is why no field here says
//! "severity of the underlying problem" without also having a `check` that was told to look: the engines
//! [`crate::layering`] and [`crate::cycles`] still hold the full picture, and a gate that re-derived it
//! would be a second, disagreeing answer.
//!
//! **Not suppressed by silence.** A finding matched by `check.ignore` is not dropped — [`super::gate`]
//! counts it into [`super::gate::Gate::silenced`] so a green run can say *"3 silenced"* and a reader can
//! ask which three.

use serde::Serialize;

use std::fmt::Write as _;

/// The check a finding belongs to.
///
/// Six families, one per clause of the ticket's sentence, and each one's evidence lives in a different
/// engine — which is the whole reason this is an enum rather than a free-text `rule` prefix: the gate has
/// to be able to say *which* family a run could not judge, and "some string starting with deps" cannot be
/// checked against a config key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Check {
    /// A declared layering rule broke: `[layering]` in `fathom.toml`, judged by [`crate::layering`].
    Layering,
    /// A new cycle in the module graph, judged by [`crate::cycles`] against the ratchet ledger.
    Cycles,
    /// The public API surface moved, judged by [`crate::api`] — breaking, and the indeterminate class
    /// that a scope which cannot be enumeration forces.
    Api,
    /// The dependency set moved in a way `[check.dependencies]` refuses, judged by [`crate::deps`].
    Dependencies,
    /// New logic arrived without tests beside it, above `[check.tests] min_new_lines`.
    Tests,
    /// A high-risk episode in the range has no human sign-off.
    SignOff,
}

impl Check {
    /// Every check, in the order a report lists them — the order of the ticket's sentence.
    pub const ALL: [Check; 6] = [
        Check::Layering,
        Check::Cycles,
        Check::Api,
        Check::Dependencies,
        Check::Tests,
        Check::SignOff,
    ];

    /// The `fathom.toml` and JSON spelling, and the `--only`/`--skip` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Check::Layering => "layering",
            Check::Cycles => "cycles",
            Check::Api => "api",
            Check::Dependencies => "dependencies",
            Check::Tests => "tests",
            Check::SignOff => "sign-off",
        }
    }

    /// Parse the six spellings. `None` is a stated refusal at load, never a check that quietly vanished.
    pub fn parse(text: &str) -> Option<Check> {
        match text {
            "layering" => Some(Check::Layering),
            "cycles" => Some(Check::Cycles),
            "api" => Some(Check::Api),
            "dependencies" | "deps" => Some(Check::Dependencies),
            "tests" => Some(Check::Tests),
            // `sign-off` is the spelling; `sign_off`, `signoff` and `review` are what a human types from
            // memory, and a config key that refuses the second is a key they will then guess wrong twice.
            "sign-off" | "sign_off" | "signoff" | "review" => Some(Check::SignOff),
            _ => None,
        }
    }

    /// What this check looks at, in one clause — the SARIF `shortDescription` and the summary's column.
    pub fn title(self) -> &'static str {
        match self {
            Check::Layering => "declared layering rules",
            Check::Cycles => "new cycles in the module graph",
            Check::Api => "breaking changes to the public API",
            Check::Dependencies => "dependency policy",
            Check::Tests => "new logic without tests",
            Check::SignOff => "high-risk episodes without a sign-off",
        }
    }

    /// The page that explains how this check decides, for SARIF's `help` and the log's remedy line.
    pub fn doc_anchor(self) -> &'static str {
        match self {
            Check::Layering | Check::Cycles => "docs/concepts/layering.md",
            Check::Api => "docs/concepts/api-surface.md",
            Check::Dependencies => "docs/concepts/dependency-risk.md",
            Check::Tests => "docs/concepts/ci-gate.md",
            Check::SignOff => "docs/concepts/review-ledger.md",
        }
    }

    /// What one unit of `examined` is, for the row that says how much was looked at.
    ///
    /// The number is meaningless without its noun, and the noun is different per check because the six
    /// families count different things: an edge, a module, a compared item, a dependency row, an episode.
    pub fn unit(self) -> &'static str {
        match self {
            Check::Layering => "dependency edges judged",
            Check::Cycles => "modules inside cycles",
            Check::Api => "surface items compared",
            Check::Dependencies => "dependency changes",
            Check::Tests => "episodes measured",
            Check::SignOff => "episodes scored",
        }
    }
}

/// How loudly a finding speaks, and — through `[check] fail_on` — whether it stops the pipeline.
///
/// Three levels and not five, because these are the three SARIF can carry
/// (`note`/`warning`/`error`, plus `none`, which means "not reported" and is therefore [`super::policy`]
/// `off` rather than a level). A scale nobody can distinguish is a scale that gets flattened in practice;
/// the gate's whole job is the one line under them, *does this fail the run*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Worth reading, not worth stopping for.
    Note,
    /// Something a human should look at before merging.
    Warning,
    /// The pipeline stops.
    Error,
}

impl Severity {
    /// The stable spelling, which is also the `fathom.toml`, JSON and SARIF spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Note => "note",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }

    /// Parse the three spellings, plus `off` reported as `None` — one knob, [`super::policy`]'s
    /// `[check.severity]`, switches a check off as well as turning it down.
    pub fn parse(text: &str) -> Option<Severity> {
        match text {
            "note" => Some(Severity::Note),
            "warning" => Some(Severity::Warning),
            "error" => Some(Severity::Error),
            _ => None,
        }
    }

    /// Every level, lowest first.
    pub const ALL: [Severity; 3] = [Severity::Note, Severity::Warning, Severity::Error];
}

/// Where a finding came from — the half a reviewer checks before they fix anything.
///
/// `path` is repo-relative and only ever names a file the *head* revision is supposed to have; a
/// location that does not exist is dropped by [`super::sarif`] rather than uploaded as a dead link. The
/// other half is `source`: how the claim was established, which is what separates "the graph says" from
/// "line 41 of this lockfile says".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    /// The file the finding points at, repo-relative, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The 1-based line within it, when one is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// How the claim was established, in a short clause: `graph:layering`, `hunk:Cargo.lock`,
    /// `episode:<id>`, `fathom.toml:[check.tests]`.
    pub source: String,
}

impl Evidence {
    /// A file, with a line where one is known.
    #[must_use]
    pub fn file(path: impl Into<String>, line: Option<u32>, source: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            line,
            source: source.into(),
        }
    }

    /// A claim with no file behind it — an episode, a config key, a revision range.
    #[must_use]
    pub fn pointer(source: impl Into<String>) -> Self {
        Self {
            path: None,
            line: None,
            source: source.into(),
        }
    }

    /// The one-line form a log and a summary print: `src/x.rs:12 (graph:layering)`.
    #[must_use]
    pub fn text(&self) -> String {
        match (&self.path, self.line) {
            (Some(path), Some(line)) => format!("{path}:{line} ({})", self.source),
            (Some(path), None) => format!("{path} ({})", self.source),
            (None, _) => self.source.clone(),
        }
    }
}

/// One objection to this range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// The family it belongs to — the config key that sets its volume and the SARIF rule's tag.
    pub check: Check,
    /// The stable rule id within the family, e.g. `new-direct`, `no-upward`, `untested-logic`.
    pub rule: String,
    /// The volume, already resolved against `[check.severity]` and the family's own knobs.
    pub severity: Severity,
    /// One sentence, with the names in it. Every renderer prints this; none rewrites it.
    pub message: String,
    /// The lines under the message: why it is wrong and the usual fix, in the engine's own words.
    pub detail: Vec<String>,
    /// What this claim rests on, strongest first.
    pub evidence: Vec<Evidence>,
    /// How many identical findings this collapses. A range that adds the same dependency in four
    /// manifests is one finding four times over, and a list of four identical sentences is a list that
    /// gets scrolled past.
    pub count: usize,
    /// The other locations, when the finding repeats: kept out of `evidence` so the primary one stays
    /// the SARIF location.
    pub also: Vec<String>,
}

impl Finding {
    /// A finding with its identity and its sentence, and nothing else yet.
    #[must_use]
    pub fn new(
        check: Check,
        rule: impl Into<String>,
        severity: Severity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            check,
            rule: rule.into(),
            severity,
            message: message.into(),
            detail: Vec::new(),
            evidence: Vec::new(),
            count: 1,
            also: Vec::new(),
        }
    }

    /// Add a piece of evidence (builder style, so a construction reads top to bottom).
    #[must_use]
    pub fn with_evidence(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    /// Add a detail line (`why:` / `fix:` / `note:`).
    #[must_use]
    pub fn with_detail(mut self, line: impl Into<String>) -> Self {
        self.detail.push(line.into());
        self
    }

    /// The `check/rule` key `check.ignore` matches against.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}/{}", self.check.as_str(), self.rule)
    }

    /// The primary location, if the finding has one.
    #[must_use]
    pub fn primary(&self) -> Option<&Evidence> {
        self.evidence.iter().find(|item| item.path.is_some())
    }

    /// A stable hash of what a reader would call "the same problem".
    ///
    /// Deliberately *not* the line number: a finding that moves because an unrelated function was added
    /// above it is the same finding, and a code-scanning alert that re-opens on every rebase trains
    /// people to close alerts. The check, the rule, the sentence and the file are what make it the one it
    /// is.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        hasher.update(self.check.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(self.rule.as_bytes());
        hasher.update([0]);
        hasher.update(self.message.as_bytes());
        if let Some(primary) = self.primary()
            && let Some(path) = &primary.path
        {
            hasher.update([0]);
            hasher.update(path.as_bytes());
        }
        let digest = hasher.finalize();
        let mut out = String::with_capacity(16);
        for byte in digest.iter().take(8) {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    /// The finding's own line, for a log: severity, rule, sentence, primary evidence.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut out = format!(
            "{} [{}] {}",
            self.severity.as_str(),
            self.key(),
            self.message
        );
        if let Some(primary) = self.primary() {
            out.push_str(&format!(" — {}", primary.text()));
        }
        if self.count > 1 {
            let _ = write!(out, " ({} occurrences)", self.count);
        }
        out
    }
}

//! `finding.rs` — what a detector says, in the shape every surface prints.
//!
//! Four things make a finding usable rather than a warning, and the ticket names all four: a
//! **human-readable reason**, a **jump target**, a **confidence**, and a **one-key suppression**. Every
//! constructor here builds all four or fails a debug assertion, because a finding that can be born
//! without a jump target is a finding whose UI grows one later, badly, three times.
//!
//! # The vocabularies, and why these ones
//!
//! * **Severity** is `high | medium | low`, which is ADR-004 Appendix A's `finding.severity CHECK`, the
//!   only severity vocabulary in the product with a constraint behind it. The delta document's comment
//!   floats `high | notable | note`; nothing writes those strings today, and a second vocabulary with no
//!   constraint is the "fourth spelling" its own comment warns about. The moment findings are persisted
//!   (`e4.4`/`e4.6` territory) the store's CHECK is what they will have to satisfy, so this is that
//!   spelling now.
//! * **Evidence grade** is `observed | inferred | asserted | absent`, the same ADR-004 column:
//!   `observed` means the lines are quoted in the finding, `inferred` means the detector derived the
//!   claim from the rows and cannot show the exact line, `asserted` means the repository itself stated
//!   it (a `Scope:` trailer, a `DO NOT EDIT` header) and `absent` names a thing that was looked for and
//!   not found, which is how a gap travels.
//! * **Jump** is `view` + `target`, ADR-004's `finding.jump`. The view names are ADR-006 §4's nine
//!   registry ids, repeated here as constants because `fathom-analyze` may not name `fathom-tui`
//!   (ADR-002 §5, lint `M01`) — `tests/test_pathology_contract.py` fails if the two lists ever differ.
//!
//! # The suppression key
//!
//! One key, one keystroke: `suppression_key` is a short, stable, human-typable string a reviewer puts in
//! `detectors.suppress` to silence *this* finding and its family — `scope_creep:src/parser.rs` silences
//! scope creep on that path, `secret_string` silences the detector outright, and glob characters work in
//! the path half, matched with the same `globset` semantics as every other pattern in the product.
//!
//! Two rules make the key worth typing at all. It contains **no episode id and no timestamp**: a key that
//! moved when the history was rewritten would have to be retyped after every rebase, and ADR-003 §2.7 is
//! explicit that a review must not move when the history does. And it is **derived from the subject**,
//! not from the message — messages are prose and get reworded, and a reword must not invalidate a
//! suppression any more than a rebase may.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::insight;

/// The ticket every finding in this module is attributed to.
pub const OWNER: &str = "e4.3";

/// How much attention a finding asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Somebody must look before this ships.
    High,
    /// Worth a review, not worth a block.
    Medium,
    /// A note for the person who authored it.
    Low,
}

impl Severity {
    /// The stored and printed spelling (ADR-004 Appendix A's CHECK).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
        }
    }

    /// Parse a stored or configured value. Unknown spellings are the caller's problem, not a silent
    /// [`Severity::Low`]: a typo in a config must not quietly demote everything.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "high" => Some(Severity::High),
            "medium" => Some(Severity::Medium),
            "low" => Some(Severity::Low),
            _ => None,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the detector knows. ADR-004's `evidence.grade`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    /// The quoted lines are in [`Evidence::detail`]; the reviewer can check the claim from the finding.
    Observed,
    /// The claim is derived from the rows. The locator resolves, the claim is not a quotation.
    Inferred,
    /// The repository stated it — a trailer, a generated-file header, a declaration.
    Asserted,
    /// Something was looked for and is not there. This is how a detector admits it could not look.
    Absent,
}

impl Grade {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Grade::Observed => "observed",
            Grade::Inferred => "inferred",
            Grade::Asserted => "asserted",
            Grade::Absent => "absent",
        }
    }
}

/// What kind of thing the evidence is.
///
/// `range`, `hunk`, `test` and `config_key` are ADR-004's `evidence.kind` values. `path` and `episode`
/// are two this catalogue needs and the ADR does not list yet: a finding about a *file* (a generated
/// file hand-edited) has no range to point at, and a rework finding's second half is another episode.
/// Both are named here rather than fudged into `range` so that the ADR amendment `e4.4` makes when it
/// persists findings has a concrete list to amend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    /// `path:start-end`, or `path` alone.
    Range,
    /// One `@@` hunk.
    Hunk,
    /// A named test case.
    Test,
    /// A configuration key.
    ConfigKey,
    /// A whole file, for a claim about the file rather than about a line in it.
    Path,
    /// Another episode, for the loop detectors' second half.
    Episode,
}

impl EvidenceKind {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EvidenceKind::Range => "range",
            EvidenceKind::Hunk => "hunk",
            EvidenceKind::Test => "test",
            EvidenceKind::ConfigKey => "config_key",
            EvidenceKind::Path => "path",
            EvidenceKind::Episode => "episode",
        }
    }
}

/// One piece of evidence: where to look, and what is there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// What kind of locator this is.
    pub kind: EvidenceKind,
    /// A resolvable pointer: `src/parser.rs:240-262`, a `#/files/3` document pointer, an episode id.
    pub locator: String,
    /// What is there, quoted or stated. Bounded by the caller — the detector caps the lines it quotes.
    pub detail: String,
    /// How the detector knows.
    pub grade: Grade,
}

impl Evidence {
    /// A quoted hunk or line range the reviewer can open.
    #[must_use]
    pub fn observed(
        kind: EvidenceKind,
        locator: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Evidence {
            kind,
            locator: locator.into(),
            detail: detail.into(),
            grade: Grade::Observed,
        }
    }

    /// A claim derived from the rows rather than quoted from the text.
    #[must_use]
    pub fn inferred(
        kind: EvidenceKind,
        locator: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Evidence {
            kind,
            locator: locator.into(),
            detail: detail.into(),
            grade: Grade::Inferred,
        }
    }

    /// A claim the repository itself made.
    #[must_use]
    pub fn asserted(
        kind: EvidenceKind,
        locator: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Evidence {
            kind,
            locator: locator.into(),
            detail: detail.into(),
            grade: Grade::Asserted,
        }
    }

    /// A stated absence, which is a fact and not a zero.
    #[must_use]
    pub fn absent(
        kind: EvidenceKind,
        locator: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Evidence {
            kind,
            locator: locator.into(),
            detail: detail.into(),
            grade: Grade::Absent,
        }
    }
}

/// Where the surface goes when the reviewer accepts the finding.
///
/// `view` is one of ADR-006 §4's nine registry ids; `target` is `<episode>` or `<episode>#<path-or-
/// region>`, the shape the TUI's scope and the diff pane's path selector already speak.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jump {
    /// The view name, from [`view`].
    pub view: &'static str,
    /// What to open in it.
    pub target: String,
}

impl Jump {
    /// The one the catalogue uses: open this episode's diff at this path.
    #[must_use]
    pub fn diff(episode_id: &str, path: &str) -> Self {
        Jump {
            view: view::DIFF,
            target: format!("{episode_id}#{path}"),
        }
    }

    /// Open the episode itself — for a claim about the change as a whole, where no one path is the
    /// culprit (a shotgun edit, a rework against an earlier episode).
    #[must_use]
    pub fn episode(episode_id: &str) -> Self {
        Jump {
            view: view::TIMELINE,
            target: episode_id.to_owned(),
        }
    }

    /// The printed form: `diff ep-1#src/parser.rs`.
    #[must_use]
    pub fn display(&self) -> String {
        format!("{} {}", self.view, self.target)
    }
}

/// The view registry ids (ADR-006 §4). Duplicated from `fathom-tui` on purpose — see [`Jump`].
pub mod view {
    /// The ranked attention worklist.
    pub const QUEUE: &str = "queue";
    /// The episode rail and card.
    pub const TIMELINE: &str = "timeline";
    /// The file tree and hunks — where almost every finding's evidence lives.
    pub const DIFF: &str = "diff";
    /// The directory treemap, for the findings that are about spread rather than about a line.
    pub const STRUCTURE: &str = "structure";
    /// The module graph.
    pub const GRAPH: &str = "graph";

    /// All seven this catalogue may name, for the contract test that keeps them ADR-006's list.
    pub const ALL: [&str; 5] = [QUEUE, TIMELINE, DIFF, STRUCTURE, GRAPH];
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// The catalogue id, e.g. `scope_creep`.
    pub detector_id: String,
    /// The detector's own version, so a re-run after a rule change is distinguishable from the last.
    pub detector_version: u32,
    /// The episode it is about.
    pub episode_id: String,
    /// What it is about *within* the episode — a path, a region, a test name, or the episode itself.
    pub subject: String,
    /// The claim, in one sentence a person wrote.
    pub message: String,
    /// How much attention it asks for.
    pub severity: Severity,
    /// `0..=1`. Never `1.0` for an inferred claim, and the price list is in the module docs.
    pub confidence: f64,
    /// Why the detector thinks so. Never empty.
    pub evidence: Vec<Evidence>,
    /// Where the surface opens.
    pub jump: Jump,
    /// The one key that silences this finding. [`Finding::suppression_key`].
    pub suppression_key: String,
    /// The owning ticket.
    pub owner: &'static str,
}

impl Finding {
    /// Start one. The four mandatory parts come from the detector; the rest are defaults that a detector
    /// overrides on the builder, so a detector that forgets to set them gets a *usable* finding rather
    /// than a panic in production.
    #[must_use]
    pub fn build(
        detector_id: &str,
        detector_version: u32,
        episode_id: &str,
        subject: impl Into<String>,
        message: impl Into<String>,
    ) -> FindingBuilder {
        FindingBuilder {
            finding: Finding {
                detector_id: detector_id.to_owned(),
                detector_version,
                episode_id: episode_id.to_owned(),
                subject: subject.into(),
                message: message.into(),
                severity: Severity::Medium,
                confidence: 0.5,
                evidence: Vec::new(),
                jump: Jump::episode(episode_id),
                suppression_key: String::new(),
                owner: OWNER,
            },
        }
    }

    /// Turn this into the delta document's shape, so a finding reaches every surface through
    /// [`crate::insight::Supplied::findings`] and not through a second rendering path.
    #[must_use]
    pub fn to_insight(&self) -> insight::Finding {
        // The delta's `Evidence` is a *pointer*, not a quotation: content stays out of the document
        // (ADR-001 §6.3). The first piece of evidence decides which pointer to print, and the rest are
        // counted in the message so nothing is silently dropped.
        let evidence = match self.evidence.first() {
            Some(first) if first.locator.starts_with('#') => {
                insight::Evidence::inside(first.locator.clone())
            }
            Some(first) => insight::Evidence::external(first.locator.clone()),
            None => insight::Evidence::owner(self.owner),
        };
        let mut message = self.message.clone();
        if self.evidence.len() > 1 {
            message.push_str(&format!(" ({} pieces of evidence)", self.evidence.len()));
        }
        insight::Finding {
            kind: self.detector_id.clone(),
            subject: self.subject.clone(),
            message,
            severity: self.severity.as_str().to_owned(),
            evidence,
            owner: self.owner.to_owned(),
        }
    }

    /// The `detector_id`/`detector_version` pair, as one printed string.
    #[must_use]
    pub fn detector(&self) -> String {
        format!("{} v{}", self.detector_id, self.detector_version)
    }
}

/// The builder behind [`Finding::build`].
#[derive(Debug, Clone)]
pub struct FindingBuilder {
    finding: Finding,
}

impl FindingBuilder {
    /// Override the severity.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.finding.severity = severity;
        self
    }

    /// Set the confidence, clamped into `0..=1` — a confidence above 1 is a bug, not a claim.
    #[must_use]
    pub fn confidence(mut self, confidence: f64) -> Self {
        self.finding.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Add evidence. Detectors cap what they quote; nothing here re-expands it.
    #[must_use]
    pub fn evidence(mut self, evidence: Evidence) -> Self {
        self.finding.evidence.push(evidence);
        self
    }

    /// Point the finding at a specific path rather than at the episode.
    #[must_use]
    pub fn jump_path(mut self, path: &str) -> Self {
        self.finding.jump = Jump::diff(&self.finding.episode_id, path);
        self
    }

    /// Point it somewhere else entirely.
    #[must_use]
    pub fn jump(mut self, view: &'static str, target: impl Into<String>) -> Self {
        self.finding.jump = Jump {
            view,
            target: target.into(),
        };
        self
    }

    /// Override the derived suppression key. Only for a finding whose subject is prose a reword would
    /// move; every path-shaped subject derives its own.
    #[must_use]
    pub fn suppression(mut self, key: impl Into<String>) -> Self {
        self.finding.suppression_key = key.into();
        self
    }

    /// Finish. The suppression key is derived from the detector and the subject when the detector did
    /// not name one, and a finding with no evidence is a bug in the detector. It is *not* dropped: the
    /// finding carries a stated absence saying the detector attached nothing, because a dropped finding
    /// is a missed finding nobody can audit, and a panic in a debug build is a product that stops
    /// working over a defect that had a sentence to spare.
    #[must_use]
    pub fn finish(self) -> Finding {
        let mut finding = self.finding;
        if finding.suppression_key.is_empty() {
            finding.suppression_key = suppression_key(&finding.detector_id, &finding.subject);
        }
        if finding.evidence.is_empty() {
            finding.evidence.push(Evidence::absent(
                EvidenceKind::Path,
                finding.subject.clone(),
                "the detector attached no evidence, which is a defect in the detector",
            ));
        }
        finding
    }
}

/// Derive the one-key suppression for a detector and a subject.
///
/// Stable, typable, and free of anything a history rewrite moves: `detector:subject`, lower-cased, with
/// anything that is not path-safe folded to `-`. A subject that is already a path is left alone, because
/// `scope_creep:src/parser.rs` is the key a reviewer will actually type.
#[must_use]
pub fn suppression_key(detector_id: &str, subject: &str) -> String {
    let folded: String = subject
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '/' | '-' | ':' | '*') {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut collapsed: String = String::with_capacity(folded.len());
    for ch in folded.chars() {
        if ch == '-' && collapsed.ends_with('-') {
            continue;
        }
        collapsed.push(ch);
    }
    let folded = collapsed.trim_matches('-');
    if folded.is_empty() {
        return detector_id.to_owned();
    }
    format!("{detector_id}:{folded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finding_carries_all_four_things_the_ticket_asks_for() {
        let finding = Finding::build(
            "scope_creep",
            1,
            "ep-1",
            "src/parser.rs",
            "outside the scope",
        )
        .severity(Severity::High)
        .confidence(0.9)
        .evidence(Evidence::asserted(
            EvidenceKind::Range,
            "src/parser.rs:1-20",
            "fn parse()",
        ))
        .jump_path("src/parser.rs")
        .finish();
        assert_eq!(finding.severity, Severity::High);
        assert!((finding.confidence - 0.9).abs() < f64::EPSILON);
        assert_eq!(finding.jump.display(), "diff ep-1#src/parser.rs");
        assert_eq!(finding.suppression_key, "scope_creep:src/parser.rs");
        assert_eq!(finding.evidence.len(), 1);
        assert_eq!(finding.owner, OWNER);
    }

    #[test]
    fn a_builder_that_forgets_still_produces_a_finding_with_evidence() {
        let finding = Finding::build("todo_debt", 1, "ep-1", "src/a.rs", "one TODO added").finish();
        assert_eq!(finding.evidence.len(), 1);
        assert_eq!(
            finding.evidence[0].grade,
            Grade::Absent,
            "and it says so in the finding"
        );
        assert_eq!(finding.suppression_key, "todo_debt:src/a.rs");
    }

    #[test]
    fn a_confidence_above_one_is_clamped_not_printed() {
        let finding = Finding::build("x", 1, "ep", "s", "m")
            .confidence(1.7)
            .finish();
        assert!((finding.confidence - 1.0).abs() < f64::EPSILON);
        assert!(
            Finding::build("x", 1, "ep", "s", "m")
                .confidence(-2.0)
                .finish()
                .confidence
                == 0.0
        );
    }

    #[test]
    fn a_suppression_key_survives_a_reword_and_a_rebase() {
        // Same subject, two different messages and two different episodes: one key. The rebase half of
        // the claim is that no episode id and no timestamp is in the string at all.
        let a = Finding::build("churn_loop", 1, "ep-1", "src/a.rs", "edited five times")
            .confidence(0.5)
            .evidence(Evidence::inferred(
                EvidenceKind::Path,
                "src/a.rs",
                "five episodes",
            ))
            .finish();
        let b = Finding::build("churn_loop", 1, "ep-99", "src/a.rs", "reworked again")
            .confidence(0.2)
            .evidence(Evidence::inferred(EvidenceKind::Path, "src/a.rs", "again"))
            .finish();
        assert_eq!(a.suppression_key, b.suppression_key);
        assert!(
            !a.suppression_key.contains("ep-"),
            "a key that a rebase invalidates is not a suppression"
        );
    }

    #[test]
    fn an_odd_subject_is_folded_into_something_typable() {
        assert_eq!(
            suppression_key("rework_loop", "src/a.rs ⇄ src/b.rs"),
            "rework_loop:src/a.rs-src/b.rs"
        );
        assert_eq!(suppression_key("shotgun_edit", ""), "shotgun_edit");
    }

    #[test]
    fn the_insight_shape_points_and_does_not_quote() {
        let finding = Finding::build(
            "secret_string",
            1,
            "ep-1",
            "src/config.rs",
            "a secret-shaped value",
        )
        .severity(Severity::High)
        .evidence(Evidence::observed(
            EvidenceKind::Range,
            "src/config.rs:44",
            "AKIAABCDEFGHIJKLMNOP",
        ))
        .finish();
        let document = finding.to_insight();
        assert_eq!(document.kind, "secret_string");
        assert_eq!(document.severity, "high");
        assert_eq!(document.evidence.run.as_deref(), Some("src/config.rs:44"));
        assert_eq!(
            document.evidence.pointer, None,
            "content does not go into the document"
        );

        let in_document = Finding::build("lint_escape", 1, "ep-1", "src/a.rs", "an allow")
            .evidence(Evidence::observed(
                EvidenceKind::Hunk,
                "#/files/3/hunks/0",
                "#[allow(unsafe_code)]",
            ))
            .finish()
            .to_insight();
        assert_eq!(
            in_document.evidence.pointer.as_deref(),
            Some("#/files/3/hunks/0")
        );
    }

    #[test]
    fn the_view_names_are_the_registrys() {
        // ADR-006 §4's ids, kept honest from the Python side too; here the cheap check is that the
        // constants are the strings the ADR prints.
        for name in view::ALL {
            assert!(!name.is_empty() && name == name.to_lowercase());
        }
        assert_eq!(view::DIFF, "diff");
        assert_eq!(view::QUEUE, "queue");
    }
}

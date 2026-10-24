//! `acme-detectors` — two Fathom detectors written by somebody who does not work here
//! (`e6.7`, `reco-raw`).
//!
//! This crate is not a member of the workspace at the repository root. It depends on `fathom-analyze` as a
//! dependency, and the only things it can name are the things the plugin API publishes: a trait, a
//! versioned finding, and the delta document it is handed. That restriction is the exhibit. If this crate
//! compiles, the API is enough; if it ever stops compiling, the breakage is a real breakage of a real
//! third party and not a refactor with a same-PR fix.
//!
//! # The two rules, and why a team would want them
//!
//! Neither is a linter rule, and neither is one of the built-in catalogue's. Both are *house rules about
//! how this particular team works with agents*, which is exactly the category the built-in catalogue
//! refuses to guess at for everyone:
//!
//! * [`UntestedAgentLogic`] — an episode attributed to an agent added a block of source and not one test
//!   file arrived with it. `fathom`'s own `untested_new_logic` asks a weaker question (does *this* new
//!   public surface owe a test, whatever its author was) and ships it on; this one is keyed to authorship,
//!   which is a policy choice with a cost — a human-written episode of the same shape passes.
//! * [`UnseenReach`] — the change is big enough that its blast radius matters and the blast section came
//!   back `unavailable`. The built-in gap already says "no answer, and `e3.2` owns it"; what a plugin can
//!   add is the *review action* the project wants attached to it, with its own suppression key so a team
//!   that has decided to live with the gap can mute the row without muting the gap.
//!
//! # Running them
//!
//! ```text
//! fathom analyze ep-7090b66 --json | acme-detectors --text
//! ```
//!
//! The binary in `src/main.rs` is the whole story: read a delta document, run the registry, print the
//! findings. Everything else is the same two hundred lines of argument the built-in catalogue is made of.

use fathom_analyze::detectors::{Evidence, EvidenceKind, Severity};
use fathom_analyze::insight::{FileDelta, STATE_MEASURED, SectionId};
use fathom_analyze::plugin::{
    Detector, Finding, FindingType, Input, Limits, Meta, RegisterError, Registry,
};

/// The namespace this publisher owns. One word, one publisher, no negotiation.
pub const NAMESPACE: &str = "acme";

/// How many lines of new source count as "a block" for [`UntestedAgentLogic`].
///
/// The same floor `detectors.untested.min_added_lines` uses by default, chosen deliberately: two thresholds
/// for one idea is two answers, and one of them will be wrong.
pub const DEFAULT_MIN_SOURCE_LINES: i64 = 20;

/// How much churn counts as "big enough that the blast radius matters" for [`UnseenReach`].
pub const DEFAULT_MIN_CHURN: i64 = 40;

/// The name of the finding type [`UntestedAgentLogic`] raises.
pub const UNTESTED_AGENT_LOGIC: &str = "acme.untested_agent_logic";

/// The name of the finding type [`UnseenReach`] raises.
pub const UNSEEN_REACH: &str = "acme.unseen_reach";

/// Whether a path is test-side, by this publisher's opinion and not the index's.
///
/// A plugin is allowed to disagree with the taxonomy — that is the point of a plugin — but it has to say
/// which opinion it is running, so the disagreement is written down here rather than being inferred from
/// a regex three functions away.
fn is_test_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("tests/")
        || path.contains("/tests/")
        || path.starts_with("spec/")
        || name.starts_with("test_")
        || name.ends_with("_test.rs")
        || name.ends_with("_spec.rs")
        || name.ends_with(".test.ts")
        || name.ends_with(".test.js")
}

/// An agent episode that added a block of source and no test file.
#[derive(Debug, Clone)]
pub struct UntestedAgentLogic {
    /// The floor below which the episode is too small to owe a test.
    pub min_source_lines: i64,
}

impl Default for UntestedAgentLogic {
    fn default() -> Self {
        UntestedAgentLogic {
            min_source_lines: DEFAULT_MIN_SOURCE_LINES,
        }
    }
}

impl UntestedAgentLogic {
    /// A detector with a chosen floor.
    #[must_use]
    pub fn new(min_source_lines: i64) -> Self {
        UntestedAgentLogic { min_source_lines }
    }
}

impl Detector for UntestedAgentLogic {
    fn meta(&self) -> Meta {
        Meta::new(NAMESPACE, "untested_agent_logic")
            .expect("`acme` and this id are names this crate owns")
            .version(1)
            .title("an agent added source and no test file")
            .asks("did an agent-authored episode add a block of source without adding a test file to it?")
            .default_on(false)
            .declares([UNTESTED_AGENT_LOGIC])
    }

    fn detect(&self, input: &Input<'_>) -> Vec<Finding> {
        // The document may say nothing at all about the episode's author. An unknown author is not an
        // agent, and refusing to guess is the whole rule — a finding that fired on `unknown` would be a
        // finding about a label, not about a change.
        if input.delta.episode.author_kind != "agent" {
            return Vec::new();
        }
        let mut source: Vec<&FileDelta> = Vec::new();
        let mut tests = 0_usize;
        for file in input.files() {
            if is_test_path(&file.path) {
                tests += 1;
            } else if file.add > 0 && !file.binary {
                source.push(file);
            }
        }
        if tests > 0 || source.is_empty() {
            return Vec::new();
        }
        let added: i64 = source.iter().map(|file| file.add).sum();
        if added < self.min_source_lines {
            return Vec::new();
        }

        // The subject is the file the reviewer should open, which is the biggest of the lot.
        let biggest = source
            .iter()
            .max_by_key(|file| (file.add, file.path.as_str()))
            .copied()
            .unwrap_or_else(|| source[0]);
        let severity = if added >= self.min_source_lines.saturating_mul(3) {
            Severity::High
        } else {
            Severity::Medium
        };
        // Rule R4 is what makes a `high` cite evidence, and rule R5 is what prices an inferred claim:
        // this detector reads rows, not lines, so it says so in the grade it quotes.
        let mut finding = Finding::new(
            FindingType::parse(UNTESTED_AGENT_LOGIC).expect("declared in `meta`"),
            "acme-detectors",
            1,
            input.episode_id(),
            &biggest.path,
            format!(
                "{added} lines of source were added by an agent turn across {} file(s) and no test file \
                 was added to this episode; the largest is `{}` (+{} −{})",
                source.len(),
                biggest.path,
                biggest.add,
                biggest.del
            ),
        )
        .with_severity(severity)
        .with_confidence(0.55)
        .with_evidence(Evidence::inferred(
            EvidenceKind::Path,
            biggest.path.clone(),
            format!(
                "{} source file(s) added {} lines; 0 test files; author `{}`, decided by `{}`",
                source.len(),
                added,
                input.delta.episode.author_kind,
                input
                    .delta
                    .episode
                    .attribution
                    .as_ref()
                    .map_or("none", |attribution| attribution.source.as_str())
            ),
        ));
        finding.jump =
            fathom_analyze::plugin::finding::Jump::diff(input.episode_id(), &biggest.path);
        vec![finding]
    }
}

/// A change big enough to owe a blast radius, in a build that could not produce one.
#[derive(Debug, Clone)]
pub struct UnseenReach {
    /// The churn below which an unseen blast radius is not worth a row.
    pub min_churn: i64,
}

impl Default for UnseenReach {
    fn default() -> Self {
        UnseenReach {
            min_churn: DEFAULT_MIN_CHURN,
        }
    }
}

impl UnseenReach {
    /// A detector with a chosen floor.
    #[must_use]
    pub fn new(min_churn: i64) -> Self {
        UnseenReach { min_churn }
    }
}

impl Detector for UnseenReach {
    fn meta(&self) -> Meta {
        Meta::new(NAMESPACE, "unseen_reach")
            .expect("`acme` and this id are names this crate owns")
            .version(1)
            .title("a change this size with no blast radius")
            .asks("is this episode big enough that what depends on it matters, with the graph unread?")
            .default_on(false)
            .declares([UNSEEN_REACH])
    }

    fn detect(&self, input: &Input<'_>) -> Vec<Finding> {
        let churn = input.totals().churn;
        if churn < self.min_churn {
            return Vec::new();
        }
        let Some(blast) = input.section(SectionId::Blast) else {
            return Vec::new();
        };
        if blast.state == STATE_MEASURED {
            return Vec::new();
        }
        let gap = blast
            .gap
            .as_ref()
            .map(|gap| format!("{} — {}", gap.short, gap.reason))
            .unwrap_or_else(|| "the blast section reported nothing".to_owned());
        let subject = input
            .files()
            .iter()
            .max_by_key(|file| (file.churn(), file.path.as_str()))
            .map(|file| file.path.clone())
            .unwrap_or_else(|| input.episode_id().to_owned());
        vec![
            Finding::new(
                FindingType::parse(UNSEEN_REACH).expect("declared in `meta`"),
                "acme-detectors",
                1,
                input.episode_id(),
                subject,
                format!(
                    "{churn} lines changed and the dependency graph was not read, so what depends on \
                     this is not in the review: {gap}"
                ),
            )
            .with_severity(Severity::Low)
            .with_confidence(0.5)
            // An `absent` grade is the vocabulary this product uses for "looked, not there", which is the
            // claim being made here — not `inferred`, which would be a claim about what is present.
            .with_evidence(Evidence::absent(
                EvidenceKind::Range,
                format!("#/sections/{}", blast.id),
                gap,
            )),
        ]
    }
}

/// Register this crate's detectors into a registry someone else owns.
///
/// A host binary calls this and keeps its own registry; returning the results rather than panicking on the
/// first refusal is what lets a host print one line per plugin and carry on.
pub fn register(registry: &mut Registry) -> Vec<Result<Meta, RegisterError>> {
    let detectors: Vec<std::sync::Arc<dyn Detector>> = vec![
        std::sync::Arc::new(UntestedAgentLogic::default()),
        std::sync::Arc::new(UnseenReach::default()),
    ];
    registry.register_all(detectors)
}

/// A registry with this crate's detectors in it, ready to run.
#[must_use]
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    register(&mut registry);
    registry
}

/// The row cap a host should pass when it has no opinion.
#[must_use]
pub fn limits() -> Limits {
    Limits::default()
}

//! `fathom_analyze::detectors` — the catalogue of things coding agents characteristically do wrong
//! (`e4.3`, `reco-2ox`).
//!
//! A detector is a pure function from (one episode, the episodes before it, the repository's policy) to
//! zero or more [`Finding`]s. Nothing in this module opens a store, runs `git`, reads a clock, or talks
//! to a surface: the read happens in [`collect`] (or in a test's fixture), and the finding goes back into
//! the delta document through [`crate::insight::Supplied::findings`], which is the only door the TUI,
//! `fathom analyze`, the export and an MCP result read answers through (ADR-002 §5.3, lint `M02`).
//!
//! # The catalogue
//!
//! Twelve detectors, in the order the ticket lists them. `default on` is the answer to "may this run in a
//! repository that has never seen fathom before", and the argument for each answer is in the detector's
//! own file header, next to the code that makes it true or false.
//!
//! | id | asks | default |
//! |---|---|---|
//! | [`scope_creep`] | did the change stay inside what was declared? | on |
//! | [`untested`] | is there new logic here with nothing that runs it? | on |
//! | [`test_weakening`] | did assertions, tests or coverage disappear rather than move? | on |
//! | [`debug_leftover`] | did a debug print survive into the commit? | on |
//! | [`todo_debt`] | did new unfinished work get a marker? | on |
//! | [`generated_edit`] | is this a hand-edit to a file a program owns? | on |
//! | [`secret_string`] | does a committed string have the shape of a credential? | on |
//! | [`lint_escape`] | was a lint or type check switched off, and which one? | on |
//! | [`churn_loop`] | is the same region being edited without converging? | **off** |
//! | [`rework_loop`] | is this episode a revert, or the same hunk landing twice? | **off** |
//! | [`shotgun_edit`] | does one episode touch unrelated clusters? | **off** |
//! | [`copy_paste`] | did new text arrive as a copy of other new text? | **off** |
//!
//! # Why four of them ship off
//!
//! Guardrail 3 (ADR-001 §5.4) is a *false-positive* budget — `G3 ≤ 0.30` at alpha, `≤ 0.15` at 1.0, with
//! `miss_rate ≤ 0.20` — and a budget nobody has measured is not a budget. The measurement is `e4.3a`
//! (`reco-mkl.1`): three pinned OSS repositories, hand-authored dispositions, per-detector attribution,
//! blessed and re-checked in CI. Until that harness says a detector is inside the gate, the ticket's own
//! rule applies: **a detector whose own false positives could push `G3` past the alpha gate ships
//! disabled by default, fixture and all, and the ticket that re-enables it names the measurement that
//! justifies it** — which is what [`Descriptor::reenable`] is for, and it is printed with the finding
//! rather than left in a comment.
//!
//! The four that ship off share one property: each of them has to decide that a *legitimate* activity is
//! suspicious. Three episodes touching one function may be thrash or may be a hard bug; a cross-cutting
//! change may be a shotgun or may be a rename; twelve copied lines may be growth or may be a table that
//! was always a table. The eight that ship on do not have to make that judgement: they read a declaration
//! the repository made (`scope_creep`, `generated_edit`), a directive that is a fact (`lint_escape`,
//! `test_weakening`, `debug_leftover`, `todo_debt`), a shape with a vendor's prefix on it
//! (`secret_string`), or an absence the reviewer can see for themselves (`untested`).
//!
//! ```text
//! $ fathom detect --episode ep-7f2a --json | head -2
//! {"detector_id":"lint_escape","episode_id":"ep-7f2a","subject":"src/parse/lexer.rs",
//!  "message":"a lint directive was added: #[allow(clippy::unwrap_used)] …",
//!  "severity":"high","confidence":0.9,"suppression_key":"lint_escape:src/parse/lexer.rs"}
//! ```
//!
//! Then, in that repository's `fathom.toml`, the one key the finding printed:
//!
//! ```toml
//! [detectors]
//! suppress = ["lint_escape:src/parse/lexer.rs"]
//! ```
//!
//! # Writing a new one
//!
//! [`docs/concepts/pathology-detectors.md`](../../../docs/concepts/pathology-detectors.md) §4 is the
//! template, and it is a four-line diff plus one file:
//!
//! 1. a `pub fn detect(episode: &Episode, corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding>` in a new
//!    file, with a header comment saying what it claims, what would make it wrong, and what its
//!    confidence price is;
//! 2. one row in [`CATALOGUE`];
//! 3. one fixture under `tests/fixtures/detectors/` that it must fire on;
//! 4. the clean corpus in that same directory, which every detector must stay silent on.
//!
//! Nothing else. No registry call at runtime, no `main` to touch, no surface to teach: `fathom detect`,
//! the delta document and the attention section pick a new row up because they iterate over this table.

#![deny(missing_docs)]

use serde::Serialize;

use crate::detectors::model::{Corpus, Episode};
use crate::detectors::policy::Policy;

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod collect;
pub mod model;
pub mod policy;

mod churn_loop;
mod copy_paste;
mod debug_leftover;
mod finding;
mod generated_edit;
mod lint_escape;
mod rework_loop;
mod scope_creep;
mod secret_string;
mod shotgun_edit;
mod test_weakening;
mod todo_debt;
mod untested;

/// The line folding the text detectors share (`e4.3`): `normalise`, `substantive`, the comment dialects.
///
/// `pub(crate)` rather than private because `e3.8`'s duplication sweep ([`crate::hotspots::deadcode`])
/// folds added lines by exactly these rules — the same four refusals, the same blank-line drop — and a
/// second copy of the fold is a pair of detectors that can disagree about what a duplicate is.
#[doc(hidden)]
pub(crate) mod text;

/// The view names a finding's jump may name, public because a surface matches on them.
pub use finding::view;
pub use finding::{
    Evidence, EvidenceKind, Finding, FindingBuilder, Grade, Jump, OWNER, Severity,
    Severity as FindingSeverity,
};
pub use model::{
    Content as ContentState, Corpus as EpisodeCorpus, Episode as DetectorEpisode, FileChange, Hunk,
};
pub use policy::{Policy as DetectorPolicy, Resolved, Suppressions, policy_from_toml};

/// What a detector has to be handed before it can answer at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// The path rows and their classes are enough.
    Rows,
    /// The hunks are needed: this detector argues about lines.
    Content,
    /// The hunks *and* the episodes before this one.
    History,
}

impl Need {
    /// Whether a read with [`Content::Absent`] can use it.
    #[must_use]
    pub const fn needs_content(self) -> bool {
        !matches!(self, Need::Rows)
    }

    /// Whether a corpus of one episode can use it.
    #[must_use]
    pub const fn needs_history(self) -> bool {
        matches!(self, Need::History)
    }
}

/// The function shape every detector has.
pub type Detect = fn(&Episode, &Corpus<'_>, &Policy) -> Vec<Finding>;

/// One row of the catalogue.
#[derive(Debug, Clone, Copy)]
pub struct Descriptor {
    /// The id, and the `detectors.enabled` / suppression spelling. `snake_case`, matching the ids
    /// `detectors.enabled` already shipped with (`untested_new_logic`, `debug_leftover`).
    pub id: &'static str,
    /// Bumped when the rule changes in a way that would move an existing episode's verdict — the version
    /// ADR-004's `finding.detector_version` column stores, so a stored finding can be told from the one
    /// this build would produce.
    pub version: u32,
    /// The one-line name a surface prints.
    pub title: &'static str,
    /// The question it asks of a change, in one sentence — printed beside every finding it raises.
    pub asks: &'static str,
    /// Whether a repository that configures nothing gets it.
    pub default_on: bool,
    /// What the detector has to be handed.
    pub needs: Need,
    /// For a `default_on = false` row: the measurement that turns it on, named. Empty for a row that
    /// ships on. Never a promise — a ticket key and the thing it has to show.
    pub reenable: &'static str,
    /// The detector itself.
    pub detect: Detect,
}

/// The catalogue. Order is the printed order and is part of the output's determinism.
pub const CATALOGUE: &[Descriptor] = &[
    Descriptor {
        id: "scope_creep",
        version: 1,
        title: "change outside the declared scope",
        asks: "did every touched path fall inside the scope somebody declared?",
        default_on: true,
        needs: Need::Rows,
        reenable: "",
        detect: scope_creep::detect,
    },
    Descriptor {
        id: "untested_new_logic",
        version: 1,
        title: "new logic with no test surface",
        asks: "is there new logic here that nothing in this episode runs?",
        default_on: true,
        needs: Need::Content,
        reenable: "",
        detect: untested::detect,
    },
    Descriptor {
        id: "test_weakening",
        version: 1,
        title: "assertions or tests removed, skipped or focused",
        asks: "did the test surface get smaller, softer, or narrower in this episode?",
        default_on: true,
        needs: Need::Content,
        reenable: "",
        detect: test_weakening::detect,
    },
    Descriptor {
        id: "debug_leftover",
        version: 1,
        title: "debug output committed",
        asks: "did something written to look at the program survive into the commit?",
        default_on: true,
        needs: Need::Content,
        reenable: "",
        detect: debug_leftover::detect,
    },
    Descriptor {
        id: "todo_debt",
        version: 1,
        title: "new TODO/FIXME debt",
        asks: "how much explicitly unfinished work did this episode add?",
        default_on: true,
        needs: Need::Content,
        reenable: "",
        detect: todo_debt::detect,
    },
    Descriptor {
        id: "generated_edit",
        version: 1,
        title: "hand-edit to a generated file",
        asks: "is a file a program owns being edited by hand?",
        default_on: true,
        needs: Need::Content,
        reenable: "",
        detect: generated_edit::detect,
    },
    Descriptor {
        id: "secret_string",
        version: 1,
        title: "secret-shaped string",
        asks: "does a committed string have the shape of a credential?",
        default_on: true,
        needs: Need::Content,
        reenable: "",
        detect: secret_string::detect,
    },
    Descriptor {
        id: "lint_escape",
        version: 1,
        title: "disabled lint or type check",
        asks: "was a check switched off here, and which one?",
        default_on: true,
        needs: Need::Content,
        reenable: "",
        detect: lint_escape::detect,
    },
    Descriptor {
        id: "churn_loop",
        version: 1,
        title: "churn loop: the same region without converging",
        asks: "has this region been edited across N episodes and put back again?",
        default_on: false,
        needs: Need::History,
        reenable: "e4.3a (reco-mkl.1) must show G3 ≤ 0.30 with miss_rate ≤ 0.20 on the three pinned \
                   OSS repositories; the threshold is ADR-001 §5.4's and the harness is not built yet",
        detect: churn_loop::detect,
    },
    Descriptor {
        id: "rework_loop",
        version: 1,
        title: "rework loop: revert or repeated hunk",
        asks: "is this episode undoing, or re-landing, something that already happened?",
        default_on: false,
        needs: Need::History,
        reenable: "e4.3a (reco-mkl.1): a rebase, a squash-merge and a legitimate re-entry are all \
                   indistinguishable from a rework at line level, and only the three-repository \
                   disposition set says how often",
        detect: rework_loop::detect,
    },
    Descriptor {
        id: "shotgun_edit",
        version: 1,
        title: "shotgun edit across unrelated clusters",
        asks: "does one episode reach into several places that have nothing to do with each other?",
        default_on: false,
        needs: Need::Rows,
        reenable: "e4.3a (reco-mkl.1) for the false-positive rate, and `e3.1`/`e3.2`'s module graph to \
                   replace directory clusters with real coupling — until then this is a path heuristic \
                   with a 0.45 ceiling",
        detect: shotgun_edit::detect,
    },
    Descriptor {
        id: "copy_paste_growth",
        version: 1,
        title: "copy-paste growth",
        asks: "did new text arrive as a copy of other new text?",
        default_on: false,
        needs: Need::Content,
        reenable: "e4.3a (reco-mkl.1): tables, generated tests and parallel implementations of one \
                   interface all read as copied; the block-length knob is what the measurement tunes",
        detect: copy_paste::detect,
    },
];

/// Every detector id, in catalogue order.
#[must_use]
pub fn ids() -> Vec<&'static str> {
    CATALOGUE.iter().map(|descriptor| descriptor.id).collect()
}

/// Look one up by id.
#[must_use]
pub fn by_id(id: &str) -> Option<&'static Descriptor> {
    CATALOGUE.iter().find(|descriptor| descriptor.id == id)
}

/// The ids that run when a repository configures nothing.
#[must_use]
pub fn default_enabled() -> std::collections::BTreeSet<String> {
    CATALOGUE
        .iter()
        .filter(|descriptor| descriptor.default_on)
        .map(|descriptor| descriptor.id.to_owned())
        .collect()
}

/// What one detector did during one scan, including the outcomes that print nothing.
///
/// A report that lists only findings cannot answer "did `churn_loop` look at this episode?", which is the
/// first question a person reviewing a miss asks. Silence is a state here, with a reason attached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectorRun {
    /// The detector's id.
    pub detector: &'static str,
    /// What happened to it.
    pub outcome: Outcome,
    /// Findings that survived suppression and the row cap.
    pub findings: usize,
    /// Findings the row cap hid, so a cap is a stated fact and not a shorter list.
    pub hidden: usize,
    /// Suppressed by `detectors.suppress`.
    pub suppressed: usize,
}

/// What happened to one detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// It ran and answered.
    Ran,
    /// Not in `detectors.enabled` and not on by default.
    Disabled,
    /// It has nothing to read in this episode (see [`Need`]).
    NotExamined,
    /// Every finding it raised was suppressed by key.
    Suppressed,
}

/// One episode's scan: the findings, and what every detector did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// The episode that was scanned.
    pub episode_id: String,
    /// Every finding, in catalogue order and then by severity and subject.
    pub findings: Vec<Finding>,
    /// One row per detector, always all twelve.
    pub runs: Vec<DetectorRun>,
    /// The policy knobs in force, printed with the report so a verdict can be reproduced.
    pub knobs: Vec<(String, String)>,
    /// Anything the read could not do and the reviewer should know about.
    pub gaps: Vec<String>,
}

impl Report {
    /// The findings for one detector.
    #[must_use]
    pub fn for_detector(&self, id: &str) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|finding| finding.detector_id == id)
            .collect()
    }

    /// How a detector answered, if it is in the catalogue.
    #[must_use]
    pub fn run(&self, id: &str) -> Option<&DetectorRun> {
        self.runs.iter().find(|run| run.detector == id)
    }

    /// The highest severity present, for a surface that prints one badge.
    #[must_use]
    pub fn ceiling(&self) -> Option<Severity> {
        self.findings
            .iter()
            .map(|finding| finding.severity)
            .max_by_key(|severity| match severity {
                Severity::Low => 0,
                Severity::Medium => 1,
                Severity::High => 2,
            })
    }
}

/// Run the catalogue over one episode.
///
/// The whole table is walked, including the detectors that do not run, so the [`Report`] can account for
/// every one of them. Ordering is catalogue order, then severity descending, then subject: a golden and a
/// re-run agree because of this and nothing else.
#[must_use]
pub fn scan(episode: &Episode, corpus: &Corpus<'_>, policy: &Policy) -> Report {
    let suppressions = policy.suppressions();
    let mut findings: Vec<Finding> = Vec::new();
    let mut runs = Vec::with_capacity(CATALOGUE.len());
    let mut gaps: Vec<String> = Vec::new();
    let invalid = &suppressions.invalid;

    for descriptor in CATALOGUE {
        if !policy.enabled(descriptor.id) {
            runs.push(DetectorRun {
                detector: descriptor.id,
                outcome: Outcome::Disabled,
                findings: 0,
                hidden: 0,
                suppressed: 0,
            });
            continue;
        }
        if descriptor.needs.needs_content() && !episode.content_examined() {
            gaps.push(format!(
                "{} did not look: this read carried no content for episode {} ({}), so it has no \
                 answer rather than a clean one — `fathom detect` reads hunks from the object store",
                descriptor.id, episode.id, episode.content.as_str()
            ));
            runs.push(DetectorRun {
                detector: descriptor.id,
                outcome: Outcome::NotExamined,
                findings: 0,
                hidden: 0,
                suppressed: 0,
            });
            continue;
        }
        if descriptor.needs.needs_history() && corpus.len() < 2 {
            gaps.push(format!(
                "{} did not look: one episode is not a sequence, and a loop needs two points",
                descriptor.id
            ));
            runs.push(DetectorRun {
                detector: descriptor.id,
                outcome: Outcome::NotExamined,
                findings: 0,
                hidden: 0,
                suppressed: 0,
            });
            continue;
        }

        let raised = (descriptor.detect)(episode, corpus, policy);
        let mut kept: Vec<Finding> = Vec::new();
        let mut suppressed = 0_usize;
        for finding in raised {
            if suppressions.silences(descriptor.id, &finding.suppression_key) {
                suppressed += 1;
                continue;
            }
            kept.push(finding);
        }
        let hidden = kept.len().saturating_sub(policy.max_findings);
        kept.sort_by(|a, b| {
            // Severity first (a reviewer reads the worst thing first), then subject so two runs of the
            // same input are byte-identical, then message: never by the order the detector happened to
            // iterate in, which is a hash's opinion in some other language.
            severity_rank(b.severity)
                .cmp(&severity_rank(a.severity))
                .then(a.subject.cmp(&b.subject))
                .then(a.message.cmp(&b.message))
        });
        kept.truncate(policy.max_findings);
        let outcome = if kept.is_empty() && suppressed > 0 {
            Outcome::Suppressed
        } else {
            Outcome::Ran
        };
        runs.push(DetectorRun {
            detector: descriptor.id,
            outcome,
            findings: kept.len(),
            hidden,
            suppressed,
        });
        findings.extend(kept);
    }

    if !invalid.is_empty() {
        gaps.push(format!(
            "detectors.suppress holds patterns that do not compile as globs and matched nothing: {}",
            invalid.join(", ")
        ));
    }

    Report {
        episode_id: episode.id.clone(),
        findings,
        runs,
        knobs: policy
            .knobs()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        gaps,
    }
}

/// Run the catalogue over every episode in `episodes`, each with the whole corpus visible.
#[must_use]
pub fn scan_all(episodes: &[Episode], policy: &Policy) -> Vec<Report> {
    let corpus = Corpus::of(episodes);
    episodes
        .iter()
        .map(|episode| scan(episode, &corpus, policy))
        .collect()
}

/// Quote up to `cap` lines into one evidence detail, marking the cut.
pub(crate) fn quote(lines: &[&str], cap: usize) -> String {
    let mut kept: Vec<&str> = Vec::with_capacity(cap.min(lines.len()));
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if kept.len() >= cap {
            break;
        }
        kept.push(trimmed);
    }
    let mut out = kept.join(" ⏎ ");
    if lines.iter().filter(|line| !line.trim().is_empty()).count() > kept.len() {
        out.push_str(" …");
    }
    out
}

/// Name up to `cap` items and count the rest, so a message never silently shortens a list.
pub(crate) fn named(items: &[&str], cap: usize) -> String {
    if items.is_empty() {
        return "none".to_owned();
    }
    if items.len() <= cap {
        return items.join(", ");
    }
    format!(
        "{} (and {} more)",
        items[..cap].join(", "),
        items.len() - cap
    )
}

const fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::High => 2,
        Severity::Medium => 1,
        Severity::Low => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::model::Content;

    #[test]
    fn the_catalogue_is_unique_and_named_as_the_config_spells_it() {
        let mut seen: Vec<&str> = Vec::new();
        for descriptor in CATALOGUE {
            assert!(
                !seen.contains(&descriptor.id),
                "{} appears twice",
                descriptor.id
            );
            seen.push(descriptor.id);
            assert!(
                descriptor
                    .id
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch == '_'),
                "{} is not snake_case, which is how `detectors.enabled` spells ids",
                descriptor.id
            );
            assert!(!descriptor.title.is_empty() && !descriptor.asks.is_empty());
            assert!(descriptor.version > 0);
        }
        // The three ids `detectors.enabled` shipped with, before this ticket existed, must still exist —
        // a repository that already lists them must not get a notice for nothing.
        for legacy in ["untested_new_logic", "debug_leftover"] {
            assert!(by_id(legacy).is_some(), "{legacy} was renamed away");
        }
    }

    #[test]
    fn every_default_off_detector_names_the_measurement_that_turns_it_on() {
        for descriptor in CATALOGUE.iter().filter(|row| !row.default_on) {
            assert!(
                !descriptor.reenable.is_empty(),
                "{} ships disabled and must name what re-enables it",
                descriptor.id
            );
            assert!(
                descriptor.reenable.contains("e4.3a") || descriptor.reenable.contains("reco-mkl"),
                "{} names a measurement, not a wish: {}",
                descriptor.id,
                descriptor.reenable
            );
        }
        for descriptor in CATALOGUE.iter().filter(|row| row.default_on) {
            assert!(
                descriptor.reenable.is_empty(),
                "a row that ships on has nothing to re-enable"
            );
        }
    }

    #[test]
    fn the_default_set_is_the_eight_that_ship_on() {
        let defaults = default_enabled();
        assert_eq!(defaults.len(), 8, "{defaults:?}");
        assert!(defaults.contains("scope_creep") && defaults.contains("secret_string"));
        assert!(!defaults.contains("churn_loop"));
        assert_eq!(Policy::default().enabled, defaults);
        assert_eq!(Policy::all_enabled().enabled.len(), CATALOGUE.len());
    }

    #[test]
    fn a_detector_that_was_not_asked_reports_disabled_rather_than_silent() {
        let episode = Episode::with_content("ep-1", vec![content_file("src/a.rs", &["todo!();"])]);
        let corpus = Corpus::of(std::slice::from_ref(&episode));
        let policy = Policy::only(&["todo_debt"]);
        let report = scan(&episode, &corpus, &policy);
        assert_eq!(
            report.run("todo_debt").map(|run| run.outcome),
            Some(Outcome::Ran)
        );
        assert_eq!(
            report.run("secret_string").map(|run| run.outcome),
            Some(Outcome::Disabled)
        );
        assert_eq!(report.findings.len(), 1);
        assert_eq!(
            report.runs.len(),
            CATALOGUE.len(),
            "every detector accounts for itself"
        );
    }

    #[test]
    fn an_unexamined_read_says_so_instead_of_reporting_a_clean_episode() {
        let mut episode = Episode::new("ep-1");
        episode.files = vec![FileChange::row("src/a.rs", 200, 0)];
        let corpus = Corpus::of(std::slice::from_ref(&episode));
        let report = scan(&episode, &corpus, &Policy::all_enabled());
        let examined = report
            .runs
            .iter()
            .filter(|run| run.outcome == Outcome::NotExamined)
            .count();
        assert!(
            examined >= 8,
            "every content detector must say it did not look: {examined}"
        );
        assert!(
            report.gaps.iter().any(|gap| gap.contains("no content")),
            "{:?}",
            report.gaps
        );
        // `scope_creep` and `shotgun_edit` need only rows, and they ran: a gap for one of them would be
        // a detector refusing work it can do.
        assert_eq!(
            report.run("scope_creep").map(|run| run.outcome),
            Some(Outcome::Ran)
        );
    }

    #[test]
    fn a_single_episode_is_not_a_sequence() {
        let episode = Episode::with_content("ep-1", vec![content_file("src/a.rs", &["fn a() {}"])]);
        let corpus = Corpus::of(std::slice::from_ref(&episode));
        let report = scan(&episode, &corpus, &Policy::all_enabled());
        assert_eq!(
            report.run("churn_loop").map(|run| run.outcome),
            Some(Outcome::NotExamined)
        );
        assert!(
            report
                .gaps
                .iter()
                .any(|gap| gap.contains("one episode is not a sequence"))
        );
    }

    #[test]
    fn a_suppressed_detector_reports_suppressed_and_no_finding() {
        let episode =
            Episode::with_content("ep-1", vec![content_file("src/a.rs", &["TODO: later"])]);
        let corpus = Corpus::of(std::slice::from_ref(&episode));
        let policy = Policy {
            enabled: ["todo_debt".to_owned()].into_iter().collect(),
            suppress: vec!["todo_debt".to_owned()],
            ..Default::default()
        };
        let report = scan(&episode, &corpus, &policy);
        assert!(report.findings.is_empty());
        assert_eq!(
            report.run("todo_debt").map(|run| run.outcome),
            Some(Outcome::Suppressed)
        );
        assert_eq!(report.run("todo_debt").map(|run| run.suppressed), Some(1));
    }

    #[test]
    fn findings_are_ordered_by_severity_then_subject() {
        let episode = Episode {
            id: "ep-1".to_owned(),
            declared_scope: vec!["src/one/**".to_owned()],
            files: vec![
                FileChange::row("src/two/b.rs", 4, 0),
                FileChange::row("src/a/c.rs", 4, 0),
            ],
            content: Content::Full,
            ..Episode::new("ep-1")
        };
        let corpus = Corpus::of(std::slice::from_ref(&episode));
        let report = scan(&episode, &corpus, &Policy::only(&["scope_creep"]));
        let subjects: Vec<&str> = report
            .findings
            .iter()
            .map(|finding| finding.subject.as_str())
            .collect();
        assert_eq!(
            subjects,
            ["src/a/c.rs", "src/two/b.rs"],
            "sorted, not in row order"
        );
    }

    #[test]
    fn the_row_cap_is_a_stated_number_and_not_a_shorter_list() {
        let files: Vec<FileChange> = (0..20)
            .map(|n| FileChange {
                path: format!("src/outside/{n}.rs"),
                ..FileChange::row(format!("src/outside/{n}.rs"), 1, 0)
            })
            .collect();
        let episode = Episode {
            id: "ep-1".to_owned(),
            declared_scope: vec!["src/inside/**".to_owned()],
            files,
            content: Content::Full,
            ..Episode::new("ep-1")
        };
        let corpus = Corpus::of(std::slice::from_ref(&episode));
        let policy = Policy {
            max_findings: 5,
            ..Policy::only(&["scope_creep"])
        };
        let report = scan(&episode, &corpus, &policy);
        assert_eq!(report.findings.len(), 5);
        assert_eq!(report.run("scope_creep").map(|run| run.hidden), Some(15));
    }

    #[test]
    fn quoting_and_naming_cap_what_they_print() {
        let every: Vec<String> = (0..10)
            .map(|n| format!("        println!(\"{n}\");"))
            .collect();
        let lines: Vec<&str> = every.iter().map(String::as_str).collect();
        let quoted = quote(&lines, 3);
        assert!(quoted.starts_with("println!(\"0\");"), "{quoted}");
        assert!(quoted.ends_with('…'), "and it says it cut: {quoted}");
        assert_eq!(quote(&["a"], 4), "a");
        let names: Vec<&str> = (0..8).map(|_| "x").collect();
        assert_eq!(named(&names, 3), "x, x, x (and 5 more)");
        assert_eq!(named(&[], 3), "none");
    }

    fn content_file(path: &str, added: &[&str]) -> FileChange {
        FileChange {
            path: path.to_owned(),
            change: model::change::ADDED.to_owned(),
            add: added.len() as i64,
            hunks: vec![Hunk::new(
                "@@ -0,0 +1,{} @@ +1,0 @@",
                added.to_vec(),
                Vec::<String>::new(),
            )],
            ..Default::default()
        }
    }
}

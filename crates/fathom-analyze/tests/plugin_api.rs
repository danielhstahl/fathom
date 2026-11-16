//! The plugin API as a contract (`e6.7`, `reco-raw`): what a plugin may say, what the registry refuses,
//! and what happens to a refusal.
//!
//! The acceptance criteria this file is the executable form of:
//!
//! * the finding schema is **versioned**, and an **unknown finding type is ignored rather than fatal**
//!   (`an_unknown_finding_type_is_ignored_and_the_run_carries_on`,
//!   `a_finding_from_a_schema_this_build_cannot_read_is_ignored_by_name`);
//! * a finding is **namespaced**, so a plugin cannot speak for a built-in detector
//!   (`a_namespace_that_belongs_to_somebody_else_is_refused_at_registration`,
//!   `a_suppression_key_outside_the_plugin_namespace_is_rewritten`);
//! * severity has **rules**, not just a vocabulary (`a_high_severity_with_no_evidence_is_demoted_and_says_so`,
//!   `confidence_is_clamped_and_certainty_on_an_inferred_claim_is_priced`);
//! * enabling and disabling is **one grammar** for a plugin and a built-in alike
//!   (`a_plugin_is_selected_by_the_same_grammar_as_a_builtin`,
//!   `a_builtin_can_be_turned_off_in_the_file_and_at_the_prompt`);
//! * the **input is the immutable delta model** and nothing else
//!   (`a_plugin_reads_the_delta_document_a_surface_would_print`, which deserialises a document from JSON
//!   exactly as `plugins/acme-detectors` does out-of-tree).
#![allow(clippy::unwrap_used, clippy::expect_used)]
//
// The eight containment rules are numbered R1–R8 in
// [`docs/concepts/extensibility.md`](../../../docs/concepts/extensibility.md) and in
// [`plugin::finding`], and this file is where each one is proved. The map, because a rule with no name in
// the test that enforces it is a rule that quietly stops being enforced:
//
// | rule | proved by |
// |---|---|
// | R1 — a finding schema version this build cannot read | `a_finding_from_a_schema_this_build_cannot_read_is_ignored_by_name` |
// | R2 — a finding type the detector did not declare | `an_unknown_finding_type_is_ignored_and_the_run_carries_on` |
// | R3 — an empty subject or message | `a_finding_whose_words_are_empty_is_not_a_finding` |
// | R4 — `high` severity with no evidence | `a_high_severity_with_no_evidence_is_demoted_and_says_so` |
// | R5 — confidence clamped, certainty priced | `confidence_is_clamped_and_certainty_on_an_inferred_claim_is_priced` |
// | R6 — a suppression key inside the plugin's own namespace | `a_suppression_key_outside_the_plugin_namespace_is_rewritten` |
// | R7 — a jump to a view this build has not | `a_jump_to_a_view_this_build_has_not_goes_to_the_episode_instead` |
// | R8 — a row cap that counts what it hides | `the_row_cap_counts_what_it_hides` |

use std::sync::Arc;

use fathom_analyze::detectors::Severity;
use fathom_analyze::detectors::policy::{Policy, Selection, policy_from_toml, resolve};
use fathom_analyze::insight::{self, EpisodeDelta, EpisodeRef, FileDelta, Input, Provenance};
use fathom_analyze::plugin::schema;
use fathom_analyze::plugin::{
    API_VERSION, Detector, Finding, FindingType, Input as PluginInput, Limits, Meta, Registry, Run,
};

/// What a scripted detector's body is. Named because it is used twice (here and in `scripted`'s bound), and
/// because clippy's `type_complexity` is right that a `Box<dyn Fn … + Send + Sync>` inline in a field list is
/// hard to read — the fix it suggests, not the flattening it implies.
type ScriptedBody = Box<dyn Fn(&PluginInput<'_>) -> Vec<Finding> + Send + Sync>;

/// A detector whose body is whatever a test wants it to say.
struct Scripted {
    meta: Meta,
    raises: ScriptedBody,
}

impl Detector for Scripted {
    fn meta(&self) -> Meta {
        self.meta.clone()
    }

    fn detect(&self, input: &PluginInput<'_>) -> Vec<Finding> {
        (self.raises)(input)
    }
}

fn scripted<F>(meta: Meta, raises: F) -> Arc<dyn Detector>
where
    F: Fn(&PluginInput<'_>) -> Vec<Finding> + Send + Sync + 'static,
{
    Arc::new(Scripted {
        meta,
        raises: Box::new(raises),
    })
}

/// A usable, honest plugin declaration: `acme.orphan_test`, one declared finding type, off by default.
fn meta() -> Meta {
    Meta::new("acme", "orphan_test")
        .expect("a namespace this test owns")
        .version(2)
        .title("a test that asserts nothing")
        .asks("did this episode grow a test file whose assertions are all gone?")
        .declares(["acme.orphan_test"])
}

/// One finding, as a well-behaved plugin would raise it.
fn finding(input: &PluginInput<'_>) -> Finding {
    Finding::new(
        FindingType::parse("acme.orphan_test").expect("declared by `meta`"),
        "acme-orphan-test",
        2,
        input.episode_id(),
        "tests/store/epoch.rs",
        "grew 41 lines and not one assertion",
    )
    .with_severity(Severity::Medium)
    .with_confidence(0.6)
}

fn delta(id: &str) -> EpisodeDelta {
    let mut input = Input::new(EpisodeRef::sparse(id, "teach the plugin API to answer"));
    input.totals.n_files = 2;
    input.totals.n_add = 120;
    input.totals.n_del = 4;
    input.episode.n_commits = 2;
    input.files = [
        ("tests/store/epoch.rs", 41, 0, "test", "none"),
        ("src/store/epoch.rs", 79, 4, "source", "none"),
    ]
    .into_iter()
    .map(|(path, add, del, path_class, noise_class)| FileDelta {
        path: path.to_owned(),
        old_path: None,
        change: "modified".to_owned(),
        add,
        del,
        binary: false,
        commits: 1,
        path_class: path_class.to_owned(),
        noise_class: noise_class.to_owned(),
        surface: "unclassified".to_owned(),
        basis: Provenance::DETAIL_FULL.to_owned(),
    })
    .collect();
    insight::compose(&input)
}

fn run_with(
    raises: impl Fn(&PluginInput<'_>) -> Vec<Finding> + Send + Sync + 'static,
) -> fathom_analyze::plugin::Report {
    let mut registry = Registry::new();
    registry
        .register(scripted(meta(), raises))
        .expect("the scripted plugin registers");
    let document = delta("ep-plugin");
    registry.run(
        &PluginInput::new(&document),
        &Selection::parse("+acme.orphan_test"),
        &Limits::default(),
    )
}

// --- registration: the last moment a plugin can be refused before it can speak --------------------

#[test]
fn a_honest_plugin_registers_and_appears_in_the_manifest() {
    let mut registry = Registry::new();
    let registered = registry
        .register(scripted(meta(), |_| Vec::new()))
        .expect("registers");
    assert_eq!(registered.key(), "acme.orphan_test");
    assert_eq!(registered.version, 2);
    assert_eq!(registered.api_version, API_VERSION);
    assert!(
        !registered.default_on,
        "a plugin nobody has measured ships off"
    );
    assert_eq!(registry.keys(), vec!["acme.orphan_test".to_owned()]);
    assert_eq!(registry.manifest().len(), 1);
    assert!(registry.meta("acme.orphan_test").is_some());
    // The row a surface prints, so a plugin is in the same list as a built-in and not in a side table.
    let row: Vec<String> = registered
        .row()
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    assert!(row.contains(&"declares=acme.orphan_test".to_owned()));
}

#[test]
fn a_namespace_that_belongs_to_somebody_else_is_refused_at_registration() {
    for namespace in ["fathom", "core", "builtin", "analyze"] {
        let error = Meta::new(namespace, "mine").expect_err("reserved");
        assert!(
            error.to_string().contains("reserved"),
            "{namespace}: {error}"
        );
    }
    // And a namespace that is a built-in detector's id, which is the same theft by another route: the
    // suppression family.
    let error = Meta::new("lint_escape", "mine").expect_err("a built-in's name");
    assert!(
        error.to_string().contains("built-in detector id"),
        "{error}"
    );
}

#[test]
fn a_name_that_is_not_a_name_is_refused_with_the_rule_that_refused_it() {
    for (namespace, id, needle) in [
        ("Acme", "orphan", "lowercase"),
        ("acme", "orphan-test", "underscore"),
        ("acme", "", "empty"),
        ("acme", &"x".repeat(40), "40 characters"),
    ] {
        let error = Meta::new(namespace, id).expect_err("not a name");
        assert!(
            error.to_string().contains(needle),
            "{namespace}.{id}: expected `{needle}`, got `{error}`"
        );
    }
}

#[test]
fn a_second_copy_of_a_plugin_is_refused_because_two_copies_print_everything_twice() {
    let mut registry = Registry::new();
    registry
        .register(scripted(meta(), |_| Vec::new()))
        .expect("first");
    let error = registry
        .register(scripted(meta(), |_| Vec::new()))
        .expect_err("second");
    assert!(error.to_string().contains("twice"), "{error}");
    assert_eq!(registry.len(), 1);
}

#[test]
fn a_plugin_that_declares_nothing_cannot_be_checked_so_cannot_register() {
    let bare = Meta::new("acme", "quiet").expect("a name");
    let error = registry_with(bare).expect_err("nothing declared");
    assert!(error.to_string().contains("no finding types"), "{error}");
}

#[test]
fn a_plugin_that_declares_another_publishers_type_is_refused() {
    let foreign = Meta::new("acme", "quiet")
        .expect("a name")
        .declares(["globin.something"]);
    let error = registry_with(foreign).expect_err("foreign type");
    assert!(error.to_string().contains("own namespace"), "{error}");
}

fn registry_with(meta: Meta) -> Result<Meta, fathom_analyze::plugin::RegisterError> {
    Registry::new().register(scripted(meta, |_| Vec::new()))
}

#[test]
fn a_plugin_written_against_another_api_version_is_refused_with_both_numbers() {
    let stale = meta().api_version(API_VERSION + 1);
    let error = registry_with(stale).expect_err("future API");
    let text = error.to_string();
    assert!(text.contains(&format!("API {}", API_VERSION + 1)), "{text}");
    assert!(text.contains(&format!("API {API_VERSION}")), "{text}");
}

#[test]
fn a_batch_registration_keeps_the_good_ones_and_reports_the_refusal() {
    let mut registry = Registry::new();
    let results = registry.register_all([
        scripted(meta(), |_| Vec::new()),
        registry_stale(),
        scripted(
            Meta::new("acme", "second")
                .expect("a name")
                .declares(["acme.second"]),
            |_| Vec::new(),
        ),
    ]);
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_ok());
    assert_eq!(registry.len(), 2, "the two that were usable still run");
}

fn registry_stale() -> Arc<dyn Detector> {
    scripted(meta().api_version(99), |_| Vec::new())
}

// --- the finding rules: refuse the finding, never the run ---------------------------------------

#[test]
fn a_well_behaved_finding_survives_the_gate_untouched() {
    let report = run_with(|input| vec![finding(input)]);
    assert_eq!(report.findings.len(), 1);
    assert!(report.notices.is_empty(), "{:?}", report.notices);
    let kept = &report.findings[0];
    assert_eq!(kept.severity, Severity::Medium);
    assert_eq!(
        kept.suppression_key,
        "acme.orphan_test:tests/store/epoch.rs"
    );
    assert_eq!(
        kept.schema_version,
        fathom_analyze::plugin::FINDING_SCHEMA_VERSION
    );
    let run = report.run("acme.orphan_test").expect("a row per detector");
    assert_eq!(run.outcome, fathom_analyze::plugin::Outcome::Ran);
    assert_eq!(run.findings, 1);
    assert_eq!(run.rejected, 0);
}

#[test]
fn an_unknown_finding_type_is_ignored_and_the_run_carries_on() {
    // The ticket's own acceptance criterion, at the front door: a type the plugin never declared.
    let report = run_with(|input| {
        let declared = finding(input);
        let invented = Finding::new(
            FindingType::parse("acme.wholesale_refactor").expect("a name is a name"),
            "acme-orphan-test",
            2,
            input.episode_id(),
            "src/store/epoch.rs",
            "this type was never declared",
        );
        vec![declared, invented]
    });
    assert_eq!(report.findings.len(), 1, "the declared one");
    let run = report.run("acme.orphan_test").expect("a row");
    assert_eq!(run.rejected, 1);
    assert_eq!(
        report.notices.len(),
        1,
        "the refusal is said, not swallowed: {:?}",
        report.notices
    );
    assert!(report.notices[0].contains("acme.wholesale_refactor"));
    assert!(report.notices[0].contains("not a finding type"));
}

#[test]
fn a_finding_from_a_schema_this_build_cannot_read_is_ignored_by_name() {
    let report = run_with(|input| {
        let mut later = finding(input);
        later.schema_version = fathom_analyze::plugin::FINDING_SCHEMA_VERSION + 3;
        vec![later]
    });
    assert!(report.findings.is_empty());
    assert!(
        report.notices[0].contains("schema_version"),
        "{:?}",
        report.notices
    );
    assert_eq!(
        report.run("acme.orphan_test").expect("a row").outcome,
        fathom_analyze::plugin::Outcome::Rejected,
        "a detector that ran and was refused is not a detector that was silent"
    );
}

#[test]
fn a_finding_whose_words_are_empty_is_not_a_finding() {
    for (subject, message, needle) in [("", "a claim", "subject"), ("src/a.rs", "   ", "message")] {
        let report = run_with(move |input| {
            vec![Finding::new(
                FindingType::parse("acme.orphan_test").expect("declared"),
                "acme-orphan-test",
                2,
                input.episode_id(),
                subject,
                message,
            )]
        });
        assert!(report.findings.is_empty(), "{subject:?} / {message:?}");
        assert!(
            report.notices[0].contains(needle),
            "{subject:?} / {message:?}: {:?}",
            report.notices
        );
    }
}

#[test]
fn a_high_severity_with_no_evidence_is_demoted_and_says_so() {
    let report = run_with(|input| {
        vec![finding(input).with_severity(Severity::High)] // and no `.with_evidence`
    });
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].severity, Severity::Medium);
    assert!(
        report.notices[0].contains("no evidence"),
        "{:?}",
        report.notices
    );
}

#[test]
fn a_high_severity_with_evidence_stays_high() {
    let report = run_with(|input| {
        use fathom_analyze::detectors::{Evidence, EvidenceKind};
        vec![
            finding(input)
                .with_severity(Severity::High)
                .with_evidence(Evidence::observed(
                    EvidenceKind::Hunk,
                    "tests/store/epoch.rs:10-51",
                    "assert_eq!(epoch.len(), 4) was removed",
                )),
        ]
    });
    assert_eq!(report.findings[0].severity, Severity::High);
    assert!(report.notices.is_empty(), "{:?}", report.notices);
}

#[test]
fn confidence_is_clamped_and_certainty_on_an_inferred_claim_is_priced() {
    use fathom_analyze::detectors::{Evidence, EvidenceKind};

    let wild = run_with(|input| vec![finding(input).with_confidence(4.0)]);
    assert_eq!(wild.findings.len(), 1);
    assert_eq!(wild.findings[0].confidence, 1.0);
    assert!(wild.notices[0].contains("0–1"), "{:?}", wild.notices);

    let certain = run_with(|input| {
        vec![
            finding(input)
                .with_confidence(1.0)
                .with_evidence(Evidence::inferred(
                    EvidenceKind::Path,
                    "tests/store/epoch.rs",
                    "no assertion token seen",
                )),
        ]
    });
    assert_eq!(certain.findings[0].confidence, 0.95);
    assert!(
        certain.notices[0].contains("inferred"),
        "{:?}",
        certain.notices
    );
}

#[test]
fn a_suppression_key_outside_the_plugin_namespace_is_rewritten() {
    // The theft this refuses: a plugin that mutes a built-in's family by writing its key.
    let report = run_with(|input| {
        vec![finding(input).with_suppression_key("lint_escape:src/store/epoch.rs")]
    });
    assert_eq!(report.findings.len(), 1);
    assert_eq!(
        report.findings[0].suppression_key,
        "acme.orphan_test:tests/store/epoch.rs"
    );
    assert!(
        report.notices[0].contains("outside its own namespace"),
        "{:?}",
        report.notices
    );
}

#[test]
fn a_jump_to_a_view_this_build_has_not_goes_to_the_episode_instead() {
    let report = run_with(|input| {
        vec![
            finding(input).with_jump(fathom_analyze::plugin::finding::Jump {
                view: "calendar".to_owned(),
                target: "somewhere".to_owned(),
            }),
        ]
    });
    assert_eq!(report.findings[0].jump.view, "timeline");
    assert!(
        report.notices[0].contains("calendar"),
        "{:?}",
        report.notices
    );
    let known = run_with(|input| {
        vec![
            finding(input).with_jump(fathom_analyze::plugin::finding::Jump::diff(
                input.episode_id(),
                "tests/store/epoch.rs",
            )),
        ]
    });
    assert_eq!(known.findings[0].jump.view, "diff");
    assert!(known.notices.is_empty(), "{:?}", known.notices);
}

#[test]
fn the_row_cap_counts_what_it_hides() {
    let mut registry = Registry::new();
    registry
        .register(scripted(meta(), |input| {
            (0..6)
                .map(|index| {
                    Finding::new(
                        FindingType::parse("acme.orphan_test").expect("declared"),
                        "acme-orphan-test",
                        2,
                        input.episode_id(),
                        format!("tests/store/epoch{index}.rs"),
                        "grew and asserted nothing",
                    )
                })
                .collect()
        }))
        .expect("registers");
    let document = delta("ep-cap");
    let limits = Limits {
        max_findings: 2,
        ..Default::default()
    };
    let report = registry.run(
        &PluginInput::new(&document),
        &Selection::parse("+acme.orphan_test"),
        &limits,
    );
    assert_eq!(report.findings.len(), 2);
    let run = report.run("acme.orphan_test").expect("a row");
    assert_eq!(run.hidden, 4, "four rows hidden, and said");
}

#[test]
fn detectors_suppress_silences_a_plugin_by_the_key_the_finding_prints() {
    let mut registry = Registry::new();
    registry
        .register(scripted(meta(), |input| vec![finding(input)]))
        .expect("registers");
    let document = delta("ep-muted");
    let limits = Limits {
        suppress: vec!["acme.orphan_test".to_owned()],
        ..Default::default()
    };
    let report = registry.run(
        &PluginInput::new(&document),
        &Selection::parse("+acme.orphan_test"),
        &limits,
    );
    assert!(report.findings.is_empty());
    let run = report.run("acme.orphan_test").expect("a row");
    assert_eq!(run.suppressed, 1);
    assert_eq!(run.outcome, fathom_analyze::plugin::Outcome::Suppressed);
}

// --- the same switch for a plugin and for a built-in --------------------------------------------

#[test]
fn a_plugin_is_selected_by_the_same_grammar_as_a_builtin() {
    let default_off = meta();
    assert!(
        !Selection::default().allows(&default_off.key(), default_off.default_on),
        "a plugin that ships off does not run for a repository that configures nothing"
    );
    assert!(
        !Selection::parse("all").allows(&default_off.key(), default_off.default_on),
        "`all` is about the built-in catalogue; a plugin is opted into by name"
    );
    assert!(Selection::parse("+acme.orphan_test").allows("acme.orphan_test", false));
    assert!(
        !Selection::parse("none,+acme.orphan_test,-acme.orphan_test")
            .allows("acme.orphan_test", false)
    );
    assert!(!Selection::parse("none").disable.iter().any(|id| id == "x"));
}

#[test]
fn a_builtin_can_be_turned_off_in_the_file_and_at_the_prompt() {
    // In the file.
    let resolved = policy_from_toml("[detectors]\ndisable = [\"secret_string\"]\n");
    assert!(!resolved.policy.enabled("secret_string"));
    assert!(resolved.policy.disabled("secret_string"));
    assert!(
        resolved
            .notices
            .iter()
            .any(|notice| notice.contains("secret_string") && notice.contains("off")),
        "{:?}",
        resolved.notices
    );
    // The catalogue still accounts for it: `scan` reports `disabled`, which is the answer to "did it look?".
    assert!(fathom_analyze::detectors::by_id("secret_string").is_some());

    // At the prompt, three ways, one answer.
    for selection in [
        Selection::parse("-secret_string"),
        Selection::from_flags::<String>(&[], &[], &["secret_string".to_owned()]),
        Selection::from_flags::<String>(&["all,-secret_string".to_owned()], &[], &[]),
    ] {
        let applied = resolve("", &selection);
        assert!(
            !applied.policy.enabled("secret_string"),
            "{}",
            selection.describe()
        );
        assert!(
            applied.policy.enabled("debug_leftover"),
            "turning one off turns no other off: {}",
            selection.describe()
        );
    }

    // `none` plus one is one; `all` minus one is eleven.
    let only = resolve("", &Selection::parse("none,+churn_loop"));
    assert!(only.policy.enabled("churn_loop"));
    assert!(!only.policy.enabled("lint_escape"));
    let almost = resolve("", &Selection::parse("all,-secret_string"));
    assert_eq!(almost.policy.enabled.len(), 11);
    assert!(!almost.policy.enabled("secret_string"));

    // A name that is nobody's is a notice, never a silent pass.
    let typo = resolve("", &Selection::parse("-secrt_string"));
    assert!(
        typo.notices
            .iter()
            .any(|notice| notice.contains("secrt_string")),
        "{:?}",
        typo.notices
    );
    assert!(
        Policy::default().enabled("secret_string"),
        "and the default is unchanged"
    );
}

#[test]
fn a_selection_prints_what_it_did() {
    assert_eq!(Selection::default().describe(), "(file defaults)");
    assert_eq!(
        Selection::parse("all,-secret_string").describe(),
        "all(12) -secret_string"
    );
    assert_eq!(
        Selection::parse("none,+churn_loop").describe(),
        "none +churn_loop"
    );
}

// --- the wire, and the document a plugin is handed ----------------------------------------------

#[test]
fn the_wire_format_reads_the_lines_it_can_and_keeps_a_saying_of_the_rest() {
    let good = Finding::new(
        FindingType::parse("acme.orphan_test").expect("declared"),
        "acme-orphan-test",
        2,
        "ep-1",
        "tests/a.rs",
        "grew and asserted nothing",
    );
    let text = format!(
        "{}\nnot json\n\n{}\n",
        good.to_line().expect("serialisable"),
        r#"{"finding_type":"acme.orphan_test"}"#
    );
    let stream = fathom_analyze::plugin::read_finding_stream(&text);
    assert_eq!(stream.findings.len(), 1);
    assert_eq!(stream.ignored.len(), 2, "{:?}", stream.ignored);
    assert!(stream.ignored[0].contains("not JSON"));
    // The second line is JSON and describes an unknown *shape*: a partial object with no severity, no
    // message, no jump. It is refused by name and the run continues with the one that read.
    assert!(
        stream.ignored[1].contains("missing field"),
        "{:?}",
        stream.ignored[1]
    );
    assert_eq!(stream.findings[0].subject, "tests/a.rs");
}

#[test]
fn a_plugin_finding_lands_in_the_delta_shape_with_its_four_parts() {
    let report = run_with(|input| vec![finding(input)]);
    let translated = report.delta_findings();
    assert_eq!(translated.len(), 1);
    let delta = &translated[0];
    assert_eq!(delta.kind, "acme.orphan_test", "namespaced on the way in");
    assert_eq!(
        delta.severity, "medium",
        "the ADR-004 vocabulary, not a fourth one"
    );
    assert_eq!(delta.subject, "tests/store/epoch.rs");
    assert_eq!(delta.owner, "acme-orphan-test 2");
    assert!(
        delta
            .evidence
            .run
            .as_deref()
            .is_some_and(|run| run.starts_with("fathom analyze")),
        "a supplied finding names the command that reproduces it"
    );
}

#[test]
fn a_plugin_reads_the_delta_document_a_surface_would_print() {
    // The out-of-tree claim, checked in-tree: the input is a document that survives JSON, so a plugin in
    // another process and a plugin linked in see the same thing.
    let original = delta("ep-round-trip");
    let text = serde_json::to_string(&original).expect("a delta serialises");
    let read: EpisodeDelta = serde_json::from_str(&text).expect("and deserialises");
    let mut registry = Registry::new();
    registry
        .register(scripted(meta(), |input| {
            // A detector that argues from the rows it was handed, which is all a detector gets.
            input
                .files()
                .iter()
                .filter(|file| file.path.starts_with("tests/") && file.add > 0)
                .map(|file| {
                    Finding::new(
                        FindingType::parse("acme.orphan_test").expect("declared"),
                        "acme-orphan-test",
                        2,
                        input.episode_id(),
                        &file.path,
                        format!("{} grew {} lines", file.path, file.add),
                    )
                })
                .collect()
        }))
        .expect("registers");
    let report = registry.run(
        &PluginInput::new(&read),
        &Selection::parse("+acme.orphan_test"),
        &Limits::default(),
    );
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].subject, "tests/store/epoch.rs");
    assert_eq!(report.episode_id, "ep-round-trip");
}

// --- the schema file ----------------------------------------------------------------------------

#[test]
fn the_checked_in_finding_schema_is_the_generated_one() {
    let path = workspace_root().join(schema::SCHEMA_FILE);
    let generated = schema::to_text();
    if std::env::var("FATHOM_BLESS").is_ok_and(|value| value == "1") {
        std::fs::write(&path, &generated).expect("writable");
    }
    let checked_in = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} is generated by `fathom_analyze::plugin::schema::to_text` and must be committed; run with FATHOM_BLESS=1", schema::SCHEMA_FILE));
    assert_eq!(
        checked_in,
        generated,
        "{} is stale; re-run `FATHOM_BLESS=1 cargo test -p fathom-analyze --test plugin_api`",
        schema::SCHEMA_FILE
    );
}

/// The workspace root, two levels above this crate — the same walk `fathom-cli`'s schema tests make.
fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/fathom-analyze is two levels below the workspace root")
        .to_path_buf()
}

#[test]
fn the_schema_names_the_rules_the_code_enforces() {
    let document = schema::document();
    assert_eq!(document["x-fathom"]["contract"], schema::CONTRACT);
    assert_eq!(document["x-fathom"]["owner"], schema::OWNER);
    assert_eq!(
        document["properties"]["severity"]["enum"]
            .as_array()
            .expect("enum")
            .len(),
        3,
        "high, medium, low — there is no fourth severity"
    );
    for key in [
        "schema_version",
        "finding_type",
        "plugin",
        "plugin_version",
        "episode_id",
        "subject",
        "message",
        "severity",
        "confidence",
        "jump",
        "suppression_key",
    ] {
        assert!(
            document["required"]
                .as_array()
                .expect("required list")
                .iter()
                .any(|item| item == key),
            "{key} is required in the schema because the type requires it"
        );
    }
    // And a real finding validates against it — the cheapest form of "the schema describes the code".
    let report = run_with(|input| vec![finding(input)]);
    let value = report.findings[0].to_value().expect("serialisable");
    assert_eq!(value["finding_type"], "acme.orphan_test");
    assert_eq!(value["schema_version"], 1);
}

#[test]
fn a_run_leaves_a_row_for_every_registered_detector_even_the_ones_that_did_not_run() {
    let mut registry = Registry::new();
    registry
        .register(scripted(meta(), |_| Vec::new()))
        .expect("registers");
    registry
        .register(scripted(
            Meta::new("acme", "other")
                .expect("a name")
                .declares(["acme.other"]),
            |_| Vec::new(),
        ))
        .expect("registers");
    let document = delta("ep-quiet");
    let report = registry.run(
        &PluginInput::new(&document),
        &Selection::parse("+acme.orphan_test"),
        &Limits::default(),
    );
    let rows: Vec<&Run> = report.runs.iter().collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        report.run("acme.other").expect("a row").outcome,
        fathom_analyze::plugin::Outcome::Disabled,
        "not running is a state with a reason, not silence"
    );
}

//! `features-1`: the explainable score, its arithmetic, its tunable weights and its calibration set
//! (`e4.4`, `reco-884`).
//!
//! The acceptance criteria this file is built around:
//!
//! * **every feature prints its weight and its contribution** — asserted on the printed `Score`, field
//!   by field, and again end-to-end in `crates/fathom-cli/tests/risk_why.rs`;
//! * **stable and reproducible** — the same vector and the same weights are scored twice and compared as
//!   bytes, which is the only claim a reviewer can act on;
//! * **tunable, deterministic and monotonic** — `[risk.weights]` parses, clamps and refuses, and
//!   `proptest` proves the ranking consequences ([`monotonic`]);
//! * **a calibration set that shows precision and recall before and after retuning** —
//!   [`calibration_meets_its_bar`] measures the shipped fixture, and
//!   [`the_blessed_calibration_report_is_current`] pins the numbers the docs quote.
//!
//! Nothing here opens a store: `risk::collect` is tested by the CLI's fixture repositories, and a pure
//! scorer is a scorer a fixture can drive.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::collections::BTreeMap;

use fathom_analyze::risk::calibrate::{self, DEFAULT_FRACTION, EMBEDDED};
use fathom_analyze::risk::feature::{Feature, MODEL, MODEL_VERSION, Measurement, SPECS, Vector};
use fathom_analyze::risk::score::{self, Points, Score};
use fathom_analyze::risk::weights::{self, DEFAULT_TAU, Origin, Weights};

/// A case the way the collector would hand it over: a destructive migration with a high-severity
/// finding, an agent author, no tests, and no graph to say what depends on it.
fn destructive_migration() -> Vector {
    Vector::new(
        "ep-0038",
        vec![
            Measurement::measured(
                Feature::Findings,
                3.0,
                "e4.3 catalogue",
                "1 finding: 1 high · generated_edit on db/migrations/0038.sql",
            ),
            Measurement::measured(
                Feature::Surface,
                50.0,
                "path class",
                "db/migrations/0038.sql — a schema migration whose reversal is not in the spine",
            ),
            Measurement::measured(
                Feature::Migration,
                1.0,
                "the added lines",
                "drop column: `ALTER TABLE users DROP COLUMN email_backup` in 0038.sql",
            ),
            Measurement::measured(
                Feature::Size,
                340.0,
                "episode rollup",
                "+340 −0 = 340 churn, all of it hand-written",
            ),
            Measurement::measured(
                Feature::Noise,
                0.0,
                "path class",
                "nothing generated, locked, vendored or binary",
            ),
            Measurement::measured(
                Feature::Breadth,
                0.15,
                "episode rollup",
                "2 surfaces over 3 files in 1 commit",
            ),
            Measurement::measured(
                Feature::Deletions,
                0.05,
                "per-path add/del",
                "2% of the churn is removal",
            ),
            Measurement::measured(
                Feature::Attribution,
                0.95,
                "author_confidence",
                "credited to agent by trailer at confidence 0.95",
            ),
            Measurement::measured(
                Feature::Authorship,
                0.7,
                "author_kind",
                "claude wrote this (ADR-001 §7.3)",
            ),
        ],
    )
}

/// A quiet one: prose, by a named human, with nothing to execute.
fn documentation_edit() -> Vector {
    Vector::new(
        "ep-docs",
        vec![
            Measurement::measured(
                Feature::Findings,
                0.0,
                "e4.3 catalogue",
                "the catalogue ran and raised nothing",
            ),
            Measurement::measured(
                Feature::Surface,
                3.0,
                "path class",
                "docs/onboarding.md — prose: read, not executed",
            ),
            Measurement::measured(
                Feature::Migration,
                0.0,
                "path class",
                "no schema history in this change",
            ),
            Measurement::measured(
                Feature::Size,
                400.0,
                "episode rollup",
                "+400 −0 = 400 churn",
            ),
            Measurement::measured(Feature::Noise, 0.0, "path class", "nothing generated"),
            Measurement::measured(
                Feature::Breadth,
                0.0,
                "episode rollup",
                "1 surface over 1 file in 1 commit",
            ),
            Measurement::measured(
                Feature::Deletions,
                0.02,
                "per-path add/del",
                "2% of the churn is removal",
            ),
            Measurement::measured(
                Feature::Attribution,
                0.99,
                "author_confidence",
                "credited to human by author at confidence 0.99",
            ),
            Measurement::measured(
                Feature::Authorship,
                0.0,
                "author_kind",
                "a person wrote this",
            ),
        ],
    )
}

/// The eight identity-direction features — the ones whose ruler is the unit — as a full vector.
fn identity_vector(values: [(Feature, f64); 8]) -> Vector {
    Vector::new(
        "ep-prop",
        values
            .iter()
            .map(|(feature, value)| {
                Measurement::measured(
                    *feature,
                    *value,
                    "property fixture",
                    "a value the ruler passes through",
                )
            })
            .collect(),
    )
}

/// The features whose ruler passes the raw value through, so a property stated in raw values is a
/// property stated in values. The two inverted axes (`test_ratio`, `attribution`) and the two whose unit
/// is a count with its own denominator (`size`, `blast`) are covered by the `normalise` property instead —
/// the composition of the two is the whole path, and each half is checked on its own terms.
const IDENTITY: [Feature; 8] = [
    Feature::Migration,
    Feature::Dependency,
    Feature::Deletions,
    Feature::Noise,
    Feature::Breadth,
    Feature::Findings,
    Feature::Surface,
    Feature::Authorship,
];

// --- criterion 1: every feature prints its weight and its contribution -------------------------

#[test]
fn a_score_prints_every_feature_its_weight_and_its_contribution() {
    let scored = score::score(&destructive_migration(), &Weights::defaults());
    assert_eq!(scored.model, MODEL);
    assert_eq!(scored.model_version, MODEL_VERSION);
    assert!(
        scored.scored,
        "a fully-attributed episode gets a score: {:?}",
        scored.unscored
    );
    assert_eq!(
        scored.terms.len() + scored.absent.len(),
        Feature::ALL.len(),
        "every feature is either a contribution or a stated absence"
    );
    for term in &scored.terms {
        let spec = term.feature.spec();
        assert!(
            term.weight > 0.0,
            "{} contributed without a weight",
            term.feature.id()
        );
        assert!(
            !term.reason.is_empty(),
            "{} contributed with no sentence",
            term.feature.id()
        );
        assert!(
            !term.basis.is_empty(),
            "{} contributed with no basis",
            term.feature.id()
        );
        assert!(
            term.value >= 0.0 && term.value <= 1.0,
            "{} normalised outside the ruler: {}",
            term.feature.id(),
            term.value
        );
        assert!(
            term.points <= term.cap,
            "{} spent more than its ceiling",
            term.feature.id()
        );
        assert_eq!(term.origin, "default", "a default weight prints its origin");
        assert!(!spec.ruler.is_empty());
    }
    for absent in &scored.absent {
        assert!(
            !absent.why.is_empty(),
            "{absent:?}: an absence with no reason"
        );
        assert!(
            !absent.owner.is_empty(),
            "{absent:?}: an absence with no owner"
        );
    }
    // The value itself is a number a reviewer can act on, and the flag is a routing decision on top of
    // it (`thesis.md`: "Flagging is a routing decision, never a judgement").
    assert!(
        scored.score > 40,
        "a destructive migration with a high finding has to sit above the calibration corpus's \
         top-decile floor: {scored:?}"
    );
    // …but it is not *flagged*, and that is a finding about the knob rather than a bug here: `risk.tau`
    // ships at 70 because it is shared with `spine-1`, and nothing measured by `features-1` in the
    // reference corpus reaches 70. `fathom why --calibrate` prints that in numbers and proposes the
    // corpus's own decile floor; a reviewer who wants the flag set `risk.tau`.
    assert_eq!(scored.tau, 70, "the key spec's default, not a private one");
    assert!(!scored.flagged, "49 against a threshold of 70 is below it");
    let mut lowered = Weights::defaults();
    lowered.tau = 45;
    let flagged = score::score(&destructive_migration(), &lowered);
    assert!(
        flagged.flagged,
        "the same score at τ 45 routes differently: {} vs {}",
        flagged.score, scored.score
    );
}

#[test]
fn the_printed_parts_add_up_to_the_printed_total_exactly() {
    for vector in [destructive_migration(), documentation_edit()] {
        let scored = score::score(&vector, &Weights::defaults());
        let sum: i64 = scored.terms.iter().map(|term| term.points.0).sum();
        assert_eq!(
            sum, scored.total.0,
            "the printed terms do not sum to the printed total"
        );
        assert_eq!(
            i64::from(scored.score),
            ((scored.total.0 + 50) / 100).clamp(0, 100),
            "the integer score is not the printed sum rounded"
        );
        // And the vector line carries every term, so a reader with a calculator can redo it.
        let line = scored.vector();
        assert!(
            line.starts_with(&format!("{MODEL} {} = ", scored.score)),
            "{line}"
        );
        for term in &scored.terms {
            assert!(
                line.contains(&format!("{} {}", term.feature.id(), term.points)),
                "{line}"
            );
        }
    }
}

#[test]
fn the_reasons_are_ordered_by_contribution_and_quote_something() {
    let scored = score::score(&destructive_migration(), &Weights::defaults());
    let points: Vec<i64> = scored.reasons().iter().map(|term| term.points.0).collect();
    let mut sorted = points.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(points, sorted, "the reasons are not printed worst-first");
    for line in score::render(&scored) {
        // Every printed reason names something: a digit, a path, or an explicit refusal. The point is
        // that no reason is an adjective (`risk-ranking.md` §2).
        if line.starts_with(" 1. ") || line.starts_with(" 2. ") {
            assert!(
                line.chars().any(char::is_numeric) || line.contains("no "),
                "a reason with nothing checkable in it: {line}"
            );
        }
    }
}

#[test]
fn a_prose_edit_scores_below_the_change_it_would_otherwise_outweigh() {
    // The 400-line documentation edit is the fixture's own "must not be mistaken for risk" case, and it
    // is the size axis's favourite episode. The vector still has to rank it below the migration.
    let docs = score::score(&documentation_edit(), &Weights::defaults());
    let migration = score::score(&destructive_migration(), &Weights::defaults());
    assert!(
        docs.score < migration.score,
        "a 400-line prose edit ({}) beat a destructive migration ({})",
        docs.score,
        migration.score
    );
}

// --- criterion 2: stable and reproducible -------------------------------------------------------

#[test]
fn identical_input_yields_identical_output() {
    let vector = destructive_migration();
    let first = score::score(&vector, &Weights::defaults());
    let second = score::score(&destructive_migration(), &Weights::defaults());
    assert_eq!(first, second, "two scorings of the same rows disagreed");
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap(),
        "the JSON is not byte-stable"
    );
    assert_eq!(first.vector(), second.vector());
    assert_eq!(score::render(&first), score::render(&second));
    // Including through a fresh `Weights`, so the order the defaults were built in cannot matter.
    let third = score::score(&destructive_migration(), &Weights::default());
    assert_eq!(first, third);
}

#[test]
fn a_feature_with_no_collector_is_absent_rather_than_zero() {
    let scored = score::score(&destructive_migration(), &Weights::defaults());
    let blast = scored
        .absent
        .iter()
        .find(|absent| absent.name == "blast")
        .expect("blast is not measurable in this build and must say so, not score 0");
    assert!(blast.why.contains("e3.1"), "{blast:?}");
    assert_eq!(blast.weight, Feature::Blast.default_weight());
    assert_eq!(
        scored.measured + scored.absent.len(),
        Feature::ALL.len(),
        "measured plus absent is the whole vector, always"
    );
    assert_eq!(
        scored.measured, 9,
        "this fixture measures nine of the twelve"
    );
    // The denominator is what was measured, and the print says so: a hidden renormalisation is a second
    // model.
    let expected: f64 = scored
        .terms
        .iter()
        .map(|term| term.weight)
        .fold(0.0_f64, |total, weight| total + weight);
    assert!(
        (scored.denominator - expected).abs() < 1e-9,
        "denominator {} should be the measured weights' sum {expected}",
        scored.denominator
    );
    assert!(
        scored.denominator < 1.0,
        "three features are unmeasured, so the denominator is below the shipped sum"
    );
}

#[test]
fn a_score_that_cannot_be_computed_refuses_instead_of_printing_zero() {
    let empty = Vector::new("ep-nothing", Vec::new());
    let scored = score::score(&empty, &Weights::defaults());
    assert!(!scored.scored);
    assert!(
        scored
            .unscored
            .as_deref()
            .unwrap()
            .contains("nothing was measured")
    );
    assert!(scored.vector().contains("no score"));

    let mut silent = Weights::defaults();
    for feature in Feature::ALL {
        silent
            .set(feature.id(), 0.0, Origin::Flag)
            .expect("a zero weight is legal");
    }
    let scored = score::score(&destructive_migration(), &silent);
    assert!(
        !scored.scored,
        "every weight is zero, so there is no denominator"
    );
    assert!(
        scored
            .unscored
            .as_deref()
            .unwrap()
            .contains("no denominator")
    );
    assert!(!scored.flagged, "an unscorable episode is never flagged");
}

#[test]
fn the_threshold_is_a_routing_decision_and_prints_its_number() {
    let scored = score::score(&destructive_migration(), &Weights::defaults());
    assert_eq!(scored.tau, DEFAULT_TAU);
    let mut raised = Weights::defaults();
    raised.tau = 100;
    let strict = score::score(&destructive_migration(), &raised);
    assert_eq!(strict.tau, 100);
    assert!(!strict.flagged, "τ 100 flags only a perfect episode");
    assert_eq!(
        strict.score, scored.score,
        "τ moves the flag, never the score"
    );
}

// --- criterion 3: tunable, deterministic, monotonic ---------------------------------------------

#[test]
fn weights_come_from_fathom_toml_with_their_origin_printed() {
    let resolved = weights::weights_from_toml(
        "[risk]\ntau = 85\n\n[risk.weights]\nfindings = 0.30\nsize = 0.01\n",
    );
    assert_eq!(
        resolved.notices,
        Vec::<String>::new(),
        "{:?}",
        resolved.notices
    );
    assert_eq!(resolved.weights.tau, 85);
    let findings = resolved
        .weights
        .weights
        .iter()
        .find(|weight| weight.feature == Feature::Findings)
        .unwrap();
    assert_eq!(findings.value, 0.30);
    assert_eq!(findings.origin, Origin::Repo);
    let untouched = resolved
        .weights
        .weights
        .iter()
        .find(|weight| weight.feature == Feature::Noise)
        .unwrap();
    assert_eq!(untouched.origin, Origin::Default);
    assert_eq!(untouched.value, Feature::Noise.default_weight());
    assert_eq!(resolved.weights.untouched(), Feature::ALL.len() - 2);
}

#[test]
fn an_inline_weights_table_reads_the_same_as_a_real_one() {
    let resolved =
        weights::weights_from_toml("[risk]\nweights = { findings = 0.3, size = 0.01 }\n");
    assert_eq!(resolved.weights.weight(Feature::Findings), 0.30);
    assert_eq!(resolved.weights.weight(Feature::Size), 0.01);
    assert!(resolved.notices.is_empty(), "{:?}", resolved.notices);
}

#[test]
fn a_weight_is_clamped_and_told_about() {
    let resolved = weights::weights_from_toml("[risk.weights]\nfindings = 3.4\nsurface = -1\n");
    assert_eq!(resolved.weights.weight(Feature::Findings), 1.0);
    assert_eq!(resolved.weights.weight(Feature::Surface), 0.0);
    assert_eq!(resolved.notices.len(), 2, "{:?}", resolved.notices);
    assert!(resolved.notices[0].contains("outside 0.0..=1"));
    let scored = score::score(&destructive_migration(), &resolved.weights);
    for term in scored
        .terms
        .iter()
        .filter(|term| term.feature == Feature::Findings || term.feature == Feature::Surface)
    {
        assert_eq!(term.origin, "fathom.toml");
    }
    assert!(
        scored
            .notices
            .iter()
            .any(|notice| notice.contains("clamped"))
    );
}

#[test]
fn an_unknown_weight_is_refused_naming_the_valid_names() {
    let resolved = weights::weights_from_toml("[risk.weights]\ncomms = 0.2\nsize = 0.1\n");
    assert_eq!(resolved.notices.len(), 1, "{:?}", resolved.notices);
    assert!(
        resolved.notices[0].contains("comms"),
        "{:?}",
        resolved.notices
    );
    assert!(resolved.notices[0].contains("not a feature"));
    for feature in Feature::ALL {
        assert!(resolved.notices[0].contains(feature.id()));
    }
    // And the rest of the table still applied: one bad key is a notice, not a discarded file.
    assert_eq!(resolved.weights.weight(Feature::Size), 0.1);
}

#[test]
fn a_file_that_does_not_parse_leaves_the_defaults_and_says_so() {
    let resolved = weights::weights_from_toml("[risk.weights\nfindings = ");
    assert_eq!(
        resolved.weights.weight(Feature::Findings),
        Feature::Findings.default_weight()
    );
    assert_eq!(resolved.notices.len(), 1);
    assert!(resolved.notices[0].contains("does not parse"));

    let nonsense = weights::weights_from_toml("[risk]\ntau = \"high\"\nweights = 4\n");
    assert_eq!(nonsense.weights.tau, DEFAULT_TAU);
    assert_eq!(nonsense.notices.len(), 2, "{:?}", nonsense.notices);
}

#[test]
fn a_flag_overrides_a_file_and_both_are_attributed() {
    let resolved = weights::weights_from_toml("[risk.weights]\nfindings = 0.3\n");
    let mut effective = resolved.weights;
    let (name, value) = weights::parse_weight_flag("findings=0.05").unwrap();
    effective.set(&name, value, Origin::Flag).unwrap();
    assert_eq!(effective.weight(Feature::Findings), 0.05);
    let scored = score::score(&destructive_migration(), &effective);
    let term = scored
        .terms
        .iter()
        .find(|term| term.feature == Feature::Findings)
        .unwrap();
    assert_eq!(term.origin, "--weight");
    assert!(
        term.points < Points(600),
        "the flag did not take: {}",
        term.points
    );

    let bad = weights::parse_weight_flag("findings:0.05");
    assert!(bad.is_err());
    assert!(bad.unwrap_err().contains("name=value"));
    assert!(weights::parse_weight_flag("size=wide").is_err());
    assert!(
        effective
            .set("nonsense", 0.5, Origin::Flag)
            .unwrap_err()
            .contains("not a feature")
    );
}

/// Two episodes that differ in exactly one feature, so the only thing that can move their gap is that
/// feature's weight.
fn pair_differing_only_in(findings_a: f64, findings_b: f64) -> (Vector, Vector) {
    let build = |points: f64| {
        Vector::new(
            "ep-pair",
            [
                (Feature::Findings, points),
                (Feature::Surface, 10.0),
                (Feature::Migration, 0.0),
                (Feature::Size, 100.0),
                (Feature::Noise, 0.0),
                (Feature::Breadth, 0.0),
                (Feature::Deletions, 0.0),
                (Feature::Dependency, 0.0),
                (Feature::TestRatio, 0.5),
                (Feature::Attribution, 0.95),
                (Feature::Authorship, 0.7),
            ]
            .into_iter()
            .map(|(feature, raw)| Measurement::measured(feature, raw, "fixture", "one line each"))
            .collect(),
        )
    };
    (build(findings_a), build(findings_b))
}

#[test]
fn raising_a_weight_raises_what_it_favours_and_nothing_else() {
    // The precise claim, stated precisely. Raising one weight does three things, and the fourth thing it
    // does *not* do is the one a reader is likely to assume:
    //
    // 1. the feature's own contribution never gets smaller;
    // 2. between two episodes that differ only in that feature, the gap in its favour never shrinks;
    // 3. an episode that is at least as much of *every* feature stays ahead ([`monotonic`]'s property);
    // 4. and it is *not* true that raising one weight pushes its favourite episode above every other
    //    episode in the corpus — the denominator is the weight sum, so a heavier findings term dilutes
    //    the others. Anyone who expects a lever to move a ranking by one number should read the
    //    calibration report before writing one.
    let (loud, quiet) = pair_differing_only_in(4.0, 0.0);
    let mut light = Weights::defaults();
    light
        .set(Feature::Findings.id(), 0.01, Origin::Flag)
        .expect("a legal weight");
    let mut heavy = Weights::defaults();
    heavy
        .set(Feature::Findings.id(), 0.5, Origin::Flag)
        .expect("a legal weight");

    let findings_points = |vector: &Vector, weights: &Weights| {
        score::score(vector, weights)
            .terms
            .iter()
            .find(|term| term.feature == Feature::Findings)
            .expect("findings is measured")
            .points
    };
    assert!(
        findings_points(&loud, &heavy) >= findings_points(&loud, &light),
        "a heavier weight shrank its own contribution: {} → {}",
        findings_points(&loud, &light),
        findings_points(&loud, &heavy)
    );
    let before = gap(&loud, &quiet, &light);
    let after = gap(&loud, &quiet, &heavy);
    assert!(
        after > before,
        "the gap in findings' favour did not grow when findings' weight did: {before} → {after}"
    );
    // And the ordering itself: more findings, same everywhere else, ranks higher under any weights.
    assert!(score::score(&loud, &light).total > score::score(&quiet, &light).total);
    assert!(score::score(&loud, &heavy).total > score::score(&quiet, &heavy).total);
}

/// The score gap between two episodes, in points.
fn gap(a: &Vector, b: &Vector, weights: &Weights) -> i64 {
    score::score(a, weights).total.0 - score::score(b, weights).total.0
}

#[test]
fn a_retuned_vector_is_printed_as_the_block_to_paste() {
    let mut weights = Weights::defaults();
    weights.set("findings", 0.22, Origin::Repo).unwrap();
    let toml = weights.to_toml();
    assert!(toml.starts_with("[risk.weights]"));
    assert!(toml.contains("findings = 0.22"), "{toml}");
    assert!(
        !toml.contains("noise"),
        "an untouched default should not be written out: {toml}"
    );
    // And what it prints parses back to the same weights, which is the loop a reviewer actually walks.
    let round = weights::weights_from_toml(&toml);
    assert_eq!(round.weights.weight(Feature::Findings), 0.22);
    assert_eq!(round.weights.tau, weights.tau);
    assert_eq!(Weights::defaults().to_toml().matches('=').count(), 0);
}

mod monotonic {
    //! The monotonicity properties, as `proptest`.
    //!
    //! Two claims, both of which the ticket's "deterministic, monotonic effect on ranking" reduces to:
    //!
    //! 1. **per feature**: `normalise` is monotone in its raw value, in the direction the feature's
    //!    ruler states — up for every feature except the two that risk on a complement (`test_ratio`,
    //!    `attribution`), and the print says which;
    //! 2. **per score**: more of any feature never means fewer points, and an episode that is at least as
    //!    much of *every* feature stays at or above another under *any* non-negative weights. That second
    //!    one is what makes a retune safe to ship: it says no weight change can demote a dominating
    //!    episode, which is the only ranking guarantee a scored queue needs.

    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use proptest::prelude::*;

    /// The features whose ruler runs the other way, and which the print must therefore name.
    const INVERTED: [Feature; 2] = [Feature::TestRatio, Feature::Attribution];

    proptest! {
        #[test]
        fn normalise_is_monotone_in_its_own_direction(
            first in 0.0f64..40.0,
            second in 0.0f64..40.0,
        ) {
            for feature in Feature::ALL {
                let (a, b) = if first <= second { (first, second) } else { (second, first) };
                let low = feature.normalise(a);
                let high = feature.normalise(b);
                if INVERTED.contains(&feature) {
                    prop_assert!(low >= high - 1e-12, "{:?} inverted unexpectedly: {} → {}", feature, low, high);
                } else {
                    prop_assert!(low <= high + 1e-12, "{:?} went backwards: {} → {}", feature, low, high);
                }
                prop_assert!((0.0..=1.0).contains(&high), "{:?} left the ruler: {}", feature, high);
            }
        }

        #[test]
        fn adding_to_a_feature_never_lowers_the_score(
            base in prop::array::uniform8(0.0f64..=1.0),
            bump in 0.0f64..=1.0,
            index in 0usize..8,
            weight in 0.05f64..=1.0,
        ) {
            let low: Vec<(Feature, f64)> = IDENTITY.iter().copied().zip(base.into_iter()).collect();
            let mut high = low.clone();
            let raised = (low[index].1 + bump).min(1.0);
            high[index].1 = raised.max(low[index].1);
            let weights = Weights::defaults();
            let before = score::score(&identity_vector(array_of(&low)), &weights);
            let after = score::score(&identity_vector(array_of(&high)), &weights);
            prop_assert!(after.total >= before.total, "raising {} cost points: {} → {}", low[index].0.id(), before.total, after.total);

            // And the same with the bumped feature weighted heavily: the effect has to grow, not flip.
            let mut tuned = Weights::defaults();
            tuned.set(low[index].0.id(), weight, Origin::Flag).unwrap();
            let tuned_before = score::score(&identity_vector(array_of(&low)), &tuned);
            let tuned_after = score::score(&identity_vector(array_of(&high)), &tuned);
            prop_assert!(tuned_after.total >= tuned_before.total);

            // A weight of zero removes the feature's influence entirely, which is the only way
            // "switch this axis off" can be honoured.
            let mut off = Weights::defaults();
            off.set(low[index].0.id(), 0.0, Origin::Flag).unwrap();
            let muted_low = score::score(&identity_vector(array_of(&low)), &off).total;
            let muted_high = score::score(&identity_vector(array_of(&high)), &off).total;
            if low[index].0.default_weight() > 0.0 {
                prop_assert!(muted_low <= muted_high);
            }
        }

        #[test]
        fn a_dominating_episode_stays_ahead_under_any_weights(
            low in prop::array::uniform8(0.0f64..=1.0),
            deltas in prop::array::uniform8(0.0f64..=1.0),
            weights in prop::array::uniform8(0.0f64..=1.0),
        ) {
            let a: Vec<(Feature, f64)> = IDENTITY.iter().copied().zip(low.into_iter()).collect();
            let b: Vec<(Feature, f64)> = IDENTITY
                .iter()
                .copied()
                .enumerate()
                .map(|(index, feature)| (feature, (low[index] + deltas[index]).min(1.0)))
                .collect();
            // `b` dominates `a` by construction. Any non-negative weights must keep it ahead.
            let mut custom = Weights::defaults();
            for (feature, value) in IDENTITY.iter().copied().zip(weights.into_iter()) {
                custom.set(feature.id(), value, Origin::Flag).unwrap();
            }
            let before = score::score(&identity_vector(array_of(&a)), &custom).total;
            let after = score::score(&identity_vector(array_of(&b)), &custom).total;
            prop_assert!(after >= before, "a dominating episode lost under {:?}", IDENTITY);
        }

        #[test]
        fn the_score_is_byte_stable(values in prop::array::uniform8(0.0f64..=1.0)) {
            let vector = identity_vector(array_of(&values.iter().copied().enumerate().map(|(index, value)| (IDENTITY[index], value)).collect::<Vec<_>>()));
            let first = score::score(&vector, &Weights::defaults());
            let second = score::score(&vector, &Weights::defaults());
            prop_assert_eq!(serde_json::to_string(&first).unwrap(), serde_json::to_string(&second).unwrap());
            prop_assert_eq!(first.vector(), second.vector());
        }
    }

    /// Put a list back into the fixed-length array [`identity_vector`] wants.
    fn array_of(values: &[(Feature, f64)]) -> [(Feature, f64); 8] {
        let mut out = [(Feature::Noise, 0.0); 8];
        for (index, pair) in values.iter().enumerate().take(8) {
            out[index] = *pair;
        }
        out
    }
}

// --- criterion 4: the calibration set, before and after -----------------------------------------

#[test]
fn the_calibration_set_reads_and_labels_its_cases() {
    let set = calibrate::parse(EMBEDDED).expect("the shipped calibration set parses");
    assert!(set.cases.len() >= 30, "a cut of ten percent needs a corpus");
    assert!(
        set.notices.is_empty(),
        "the fixture names a feature this vector does not have: {:?}",
        set.notices
    );
    let positives = set.cases.iter().filter(|case| case.label).count();
    assert!(
        positives >= 3,
        "the cut has nothing to aim at: {positives} positives"
    );
    assert!(
        positives * 3 <= set.cases.len(),
        "half the set cannot be must-read: {positives}/{}",
        set.cases.len()
    );
    for case in &set.cases {
        assert!(
            !case.summary.is_empty(),
            "{} has no summary, so its label cannot be argued with",
            case.id
        );
        assert!(!case.features.is_empty(), "{} measures nothing", case.id);
        for (key, value) in &case.features {
            let feature = Feature::parse(key).unwrap_or_else(|| panic!("{} names {key}", case.id));
            let normalised = feature.normalise(*value);
            assert!(
                normalised.is_finite() && (0.0..=1.0).contains(&normalised),
                "{} names {key}={value}, which the ruler cannot place",
                case.id
            );
        }
    }
    assert!(
        !set.labelled_by.is_empty(),
        "labels without a provenance are an opinion with no author"
    );
}

#[test]
fn calibration_meets_its_bar() {
    let set = calibrate::parse(EMBEDDED).unwrap();
    let comparison = calibrate::calibrate(&set, &Weights::defaults(), DEFAULT_FRACTION);
    println!(
        "calibration report:\n{}",
        calibrate::render(&comparison).join("\n")
    );
    let before = &comparison.before;
    let after = &comparison.after;
    // The threshold is part of the proposal, and the corpus has a finding about it: nothing in
    // `features-1`'s range reaches the shipped `risk.tau` of 70, so as shipped the flag routes nothing.
    // A retune that left the knob alone would have proposed a queue in which nothing is flagged.
    assert_eq!(
        comparison.tau_shipped, 70,
        "the key spec's value, shared with `spine-1`"
    );
    assert_eq!(
        comparison.tau_shipped_flags, 0,
        "and this corpus says what that value does: nothing"
    );
    assert!(
        comparison.tau_proposed >= 1 && comparison.tau_proposed <= 100,
        "a proposed threshold is a score: {}",
        comparison.tau_proposed
    );
    assert!(
        comparison.tau_proposed_flags >= after.flagged,
        "the proposed threshold flags at least the decile it was taken from: {} vs {}",
        comparison.tau_proposed_flags,
        after.flagged
    );
    assert!(
        calibrate::render(&comparison)
            .iter()
            .any(|line| line.contains("threshold") && line.contains("risk.tau"))
    );
    assert_eq!(before.cases, set.cases.len());
    assert_eq!(
        before.flagged,
        (((set.cases.len() as f64) * DEFAULT_FRACTION).round() as usize),
        "the cut is ten percent of the set, no more and no less"
    );
    assert!(
        after.f1 > before.f1,
        "the retune did not improve the cut: F1 {:.3} → {:.3}",
        before.f1,
        after.f1
    );
    assert!(
        after.precision >= before.precision,
        "precision fell: {:.3} → {:.3}",
        before.precision,
        after.precision
    );
    assert!(
        after.recall >= before.recall,
        "recall fell: {:.3} → {:.3}",
        before.recall,
        after.recall
    );
    assert!(
        !comparison.moves.is_empty(),
        "a retune that moves nothing has not been evaluated, only agreed with"
    );
    if let Some(notice) = comparison.notices.first() {
        panic!("the calibration set refused something: {notice}");
    }
}

#[test]
fn the_retune_is_the_same_weights_on_every_run() {
    let set = calibrate::parse(EMBEDDED).unwrap();
    let first = calibrate::retune(&set, &Weights::defaults(), DEFAULT_FRACTION);
    let second = calibrate::retune(&set, &Weights::defaults(), DEFAULT_FRACTION);
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap(),
        "the retune is not deterministic, which makes every number in its report meaningless"
    );
    assert_eq!(
        first.tau,
        Weights::defaults().tau,
        "a retune moves weights, never the threshold"
    );
    for line in first.to_toml().lines() {
        assert!(
            line.starts_with("findings")
                || line.starts_with('[')
                || line.starts_with('#')
                || line.contains('=')
        );
    }
}

#[test]
fn the_blessed_calibration_report_is_current() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/risk/calibration.report.json"
    );
    let set = calibrate::parse(EMBEDDED).unwrap();
    let comparison = calibrate::calibrate(&set, &Weights::defaults(), DEFAULT_FRACTION);
    let generated = serde_json::to_string_pretty(&calibrate::to_json(&comparison)).unwrap();
    let blessed = std::fs::read_to_string(path)
        .unwrap_or_else(|_| format!("no blessed report yet; bless it:\n{generated}"));
    assert_eq!(
        blessed.trim(),
        generated.trim(),
        "the calibration report this build produces is not the one checked in — bless it with \
         `cargo test risk_score -- --ignored bless` and read the diff"
    );
}

/// Writes the blessed calibration report. Deliberately `#[ignore]`d: a blessed file is a diff a human
/// reads, and the command that writes it prints what it changed.
#[test]
#[ignore]
fn bless_calibration_report() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/risk/calibration.report.json"
    );
    let set = calibrate::parse(EMBEDDED).unwrap();
    let comparison = calibrate::calibrate(&set, &Weights::defaults(), DEFAULT_FRACTION);
    let generated = serde_json::to_string_pretty(&calibrate::to_json(&comparison)).unwrap();
    std::fs::write(path, format!("{generated}\n")).unwrap();
    println!("wrote {path}");
}

// --- the table itself ---------------------------------------------------------------------------

#[test]
fn every_feature_is_documented_and_weighted_once() {
    assert_eq!(SPECS.len(), Feature::ALL.len());
    let ids: Vec<&str> = Feature::ALL.iter().map(|feature| feature.id()).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(ids.len(), unique.len(), "a feature id is repeated: {ids:?}");
    for spec in SPECS.iter() {
        assert!(
            Feature::parse(spec.id).is_some(),
            "`{}` is in the spec table and not in the vector",
            spec.id
        );
    }
    let total: f64 = Feature::ALL
        .iter()
        .map(|feature| feature.default_weight())
        .fold(0.0_f64, |total, weight| total + weight);
    assert!(
        (total - 1.0).abs() < 1e-9,
        "the default weights should read as a share of 100 points, and sum to {total}"
    );
    for feature in Feature::ALL {
        let spec = feature.spec();
        assert!(spec.asks.len() > 12, "{} asks nothing", spec.id);
        assert!(!spec.unit.is_empty(), "{} has no unit", spec.id);
        assert!(spec.ruler.len() > 12, "{} has no ruler", spec.id);
        assert!(!spec.source.is_empty(), "{} has no source", spec.id);
        assert!(!spec.owner.is_empty(), "{} has no owner", spec.id);
        assert!(spec.default > 0.0, "{} is switched off by default", spec.id);
    }
    // The printed ceiling is not empty and names the graph hole, which is the one thing a reviewer most
    // needs told.
    assert!(!fathom_analyze::risk::ceiling().is_empty());
    assert!(fathom_analyze::risk::ceiling()[0].contains("module graph"));
}

#[test]
fn points_print_in_hundredths_and_sum_in_base_ten() {
    assert_eq!(Points(2400).to_string(), "24.00");
    assert_eq!(Points(0).to_string(), "0.00");
    assert_eq!(Points(19_500).to_string(), "195.00");
    let sum: Points = Points(1234)
        .to_string()
        .parse::<f64>()
        .map(|value| Points((value * 100.0).round() as i64))
        .unwrap();
    assert_eq!(
        sum,
        Points(1234),
        "a printed number must survive being read back"
    );
}

/// A small hand-checked example: the printed line and the arithmetic behind it, done twice, by hand and
/// by the code. Not a golden file — a worked case a document can quote.
#[test]
fn a_worked_example_adds_up_by_hand() {
    let vector = Vector::new(
        "ep-hand",
        [
            (Feature::Migration, 1.0),
            (Feature::Surface, 50.0),
            (Feature::Findings, 4.0),
            (Feature::Size, 0.0),
            (Feature::Noise, 0.0),
            (Feature::Breadth, 0.0),
            (Feature::Deletions, 0.0),
            (Feature::Dependency, 0.0),
            (Feature::TestRatio, 1.0),
            (Feature::Attribution, 1.0),
            (Feature::Authorship, 0.0),
        ]
        .into_iter()
        .map(|(feature, raw)| Measurement::measured(feature, raw, "hand check", "one line each"))
        .collect(),
    );
    let scored = score::score(&vector, &Weights::defaults());
    // Eleven of the twelve are measured, so the denominator is the shipped sum without `blast`:
    // 1.00 − 0.09 = 0.91, and every term is `100 · w · x / 0.91`.
    assert!(
        (scored.denominator - 0.91).abs() < 1e-9,
        "denominator {} is not the eleven measured weights' sum",
        scored.denominator
    );
    assert!(
        !scored
            .terms
            .iter()
            .any(|term| term.feature == Feature::Blast)
    );
    let migration = scored
        .terms
        .iter()
        .find(|term| term.feature == Feature::Migration)
        .unwrap();
    assert_eq!(
        migration.points,
        Points((10_000.0_f64 * 0.09 / 0.91).round() as i64),
        "a 0.09 weight at full value is 9.89 points once the missing axis is divided out"
    );
    let expected: i64 = Feature::ALL
        .iter()
        .filter(|feature| feature.id() != "blast")
        .map(|feature| {
            (10_000.0 * feature.default_weight() * feature.normalise(value_of(&vector, *feature))
                / 0.91)
                .round() as i64
        })
        .sum();
    assert_eq!(scored.total.0, expected.min(10_000));
    assert_eq!(
        scored.total.0,
        scored.terms.iter().map(|term| term.points.0).sum::<i64>(),
        "the printed terms have to be the total"
    );
    assert!(
        scored
            .vector()
            .starts_with(&format!("features-1 {} = ", scored.score))
    );
    // …and the number itself, quoted by `docs/concepts/risk-scoring.md` §4.
    assert_eq!(scored.score, 33, "\n{}", score::render(&scored).join("\n"));
}

fn value_of(vector: &Vector, feature: Feature) -> f64 {
    vector
        .get(feature)
        .and_then(|measurement| measurement.raw)
        .unwrap_or(0.0)
}

/// The façade re-exports what `spine-1` published; this says so loudly if it ever stops.
#[test]
fn the_spine_ranking_is_still_the_one_the_queue_reads() {
    use fathom_analyze::risk::{Risk, Signals, rank};
    let ranking: Risk = rank(&Signals {
        files: &[],
        n_add: 0,
        n_del: 0,
        n_files: 0,
        n_binary: 0,
        n_commits: 1,
        author: "unknown",
        partial_files: false,
    });
    // Nothing was measured except authorship, and an uncredited author is worth 6 in `spine-1`.
    assert_eq!(ranking.score, 6, "spine-1's own arithmetic changed");
    assert_eq!(fathom_analyze::risk::MODEL, "spine-1");
    let _ = BTreeMap::<u8, u8>::new();
    let _: Option<Score> = None;
}

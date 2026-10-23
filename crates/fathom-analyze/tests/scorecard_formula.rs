//! The scorecard formula, checked by hand (`e3.9`, `reco-al1`).
//!
//! The acceptance criterion is that the formula is "reproducible by hand from exported JSON", so every
//! test here *does the arithmetic in the test*, from the integers the scorecard publishes, and compares.
//! A test that only asserted `0..=100` would pass while the formula lay.
//!
//! What is covered:
//!
//! * the worked example in [`fathom_analyze::scorecard`]'s own module doc, end to end, to the hundredth;
//! * that the printed total is the exact sum of the printed parts, and the score is that total rounded
//!   once — the property that makes a reader's calculator agree with the index;
//! * determinism: the same measurements produce the same stored bytes;
//! * the refusal cases — an unmeasured dimension renormalises the denominator, an empty denominator
//!   refuses rather than flattering, and a foreign model row is refused rather than compared;
//! * the trend's bounded memory, thinned sample, window aggregates and least-squares direction;
//! * the "since my last review" anchor ladder and the delta it produces, including the rungs a rebase
//!   falls through;
//! * and that the text report and the `--json` document carry the same numbers, because two surfaces that
//!   disagree about a score mean the score means nothing (ADR-006 §5.1).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::risk::score::Points;
use fathom_analyze::scorecard::AnchorKind;
use fathom_analyze::scorecard::ApiSignal;
use fathom_analyze::scorecard::Curve;
use fathom_analyze::scorecard::Delta;
use fathom_analyze::scorecard::Direction;
use fathom_analyze::scorecard::Measurement;
use fathom_analyze::scorecard::Parse;
use fathom_analyze::scorecard::Policy;
use fathom_analyze::scorecard::Review;
use fathom_analyze::scorecard::Row;
use fathom_analyze::scorecard::Scorecard;
use fathom_analyze::scorecard::Trend;
use fathom_analyze::scorecard::dimension::{Dimension, badness_of};
use fathom_analyze::scorecard::report::Index;
use fathom_analyze::scorecard::report::Report;
use fathom_analyze::scorecard::resolve;
use fathom_analyze::scorecard::trend::sparkline;

/// The five measurements of the worked example in the module doc, in the doc's own numbers.
///
/// `api` is the one dimension that is *not* measured, because the example is a repository whose first
/// scored revision has nothing to diff against — the case a fresh index actually starts in.
fn worked_example() -> Vec<Measurement> {
    vec![
        Measurement::share(
            Dimension::Boundaries,
            3,
            412,
            "34 dependency rows judged by 3 rule(s)",
            "3 violation(s) with no live allowance",
        ),
        Measurement::share(
            Dimension::Coupling,
            340,
            640,
            "80 module(s) at the 8 edges/module ceiling",
            "340 dependency edges",
        ),
        Measurement::share(
            Dimension::Acyclicity,
            18,
            640,
            "640 node(s), 2 strongly-connected",
            "18 node(s) inside a cycle with no live allowance",
        ),
        Measurement::unmeasured(
            Dimension::Api,
            "no earlier revision in the index to compare against",
            "no API read was possible for this revision",
        ),
        Measurement::share(
            Dimension::Hotspots,
            7,
            80,
            "80 module(s) ranked",
            "7 module(s) at or above the hotspot bar",
        ),
    ]
}

/// A scorecard from the worked example, at a fixed clock.
fn card() -> Scorecard {
    Scorecard::of(
        &worked_example(),
        &Policy::defaults(),
        "a".repeat(40).as_str(),
        1,
    )
}

/// The **worked example**, to the hundredth.
///
/// These are the numbers from `fathom_analyze::scorecard`'s module doc, which are the numbers
/// `docs/concepts/thesis.md` §9.5 publishes. If this test fails, the documentation is wrong and so is every
/// scorecard the index has ever stored — which is why the arithmetic is spelled out here in integers.
#[test]
fn the_worked_example_in_the_documentation_scores_exactly_eighty_eight() {
    let card = card();
    assert!(card.scored);
    assert_eq!(card.model, "architecture-1");
    // W = 0.30 + 0.15 + 0.25 + 0.15 (api is unmeasured, so its 0.15 leaves the denominator).
    assert_eq!(card.denominator, 0.85);
    assert_eq!(card.weight_sum, 0.85);
    assert_eq!(card.measured, 4);
    assert_eq!(card.dimensions, 5);

    // Each term, hand-computed: points = round(10000 · w · h / W).
    let expect = [
        (Dimension::Boundaries, 0.0073, 0.9927, 3504),
        (Dimension::Coupling, 0.5313, 0.4687, 827),
        (Dimension::Acyclicity, 0.0281, 0.9719, 2859),
        (Dimension::Hotspots, 0.0875, 0.9125, 1610),
    ];
    for (dimension, badness, health, points) in expect {
        let term = card.term(dimension).expect("every dimension has a term");
        assert!(term.measured, "{dimension:?}");
        assert_eq!(term.badness, badness, "{dimension:?} badness");
        assert_eq!(term.health, health, "{dimension:?} health");
        assert_eq!(
            term.points, points,
            "{dimension:?}: round(10000 · {} · {health} / 0.85)",
            term.weight
        );
        // The ceiling — what the term would be worth at perfect health — is exported too, so a reader can
        // see how much of a term's weight the loss consumed.
        assert_eq!(
            term.ceiling,
            (term.weight * 10_000.0 / 0.85).round() as i64,
            "{dimension:?} ceiling"
        );
    }
    // api: refused, and said so.
    let api = card.term(Dimension::Api).expect("api has a term");
    assert!(!api.measured);
    assert_eq!(api.points, 0);
    assert_eq!(api.ceiling, 0);
    assert_eq!(
        api.unscored.as_deref(),
        Some("no earlier revision in the index to compare against")
    );

    // total = Σ points, and score is that total rounded once.
    assert_eq!(card.total, 3504 + 827 + 2859 + 1610);
    assert_eq!(card.total, 8800);
    assert_eq!(card.score, 88);
    assert_eq!(card.check_sum(), Points(8800));
    assert!(!card.alert, "88 is above the default alert floor of 50");

    // The contribution line, exactly: the model, the score, and each measured term's points in the model's
    // own order — a table a reader can find `boundaries` in, not a list sorted by the news.
    assert_eq!(
        card.vector(),
        "architecture-1 88 = boundaries 35.04 + coupling 8.27 + acyclicity 28.59 + hotspots 16.10"
    );
    assert_eq!(
        card.headline(),
        "architecture 88/100 · 4/5 dimensions measured · denominator 0.85"
    );
    // The sentence repeats the arithmetic on one line per dimension, including the refused one.
    let sentence = card.sentence();
    assert!(
        sentence.contains("88 of 100 · denominator 0.85 (weights over 4/5 dimensions measured)"),
        "{sentence}"
    );
    // Each line shows the points earned against the points available, so a reader sees how much of a
    // dimension's weight the loss consumed rather than only what is left.
    assert!(sentence.contains("35.04/35.29"), "{sentence}");
    assert!(sentence.contains("8.27/17.65"), "{sentence}");
    assert!(sentence.contains("3 / 412 = 0.0073"), "{sentence}");
    assert!(sentence.contains("api"), "{sentence}");
    assert!(
        sentence.contains("no earlier revision in the index"),
        "the refusal is printed, not blank: {sentence}"
    );
}

/// **The total is the sum of the printed parts, and the score is that sum rounded once.**
///
/// Asserted over a sweep of measurements rather than one fixture, because the failure mode is a rounding
/// drift that only shows up at some values.
#[test]
fn the_total_is_the_exact_sum_of_the_printed_parts_at_every_value() {
    for findings in [0_i64, 1, 7, 33, 99, 412, 5_000] {
        for modules in [1_i64, 8, 80, 640] {
            let measurements = vec![
                Measurement::share(Dimension::Boundaries, findings, modules * 6, "b", "r"),
                Measurement::share(Dimension::Coupling, findings * 3, modules * 8, "b", "r"),
                Measurement::share(Dimension::Acyclicity, findings, modules * 8, "b", "r"),
                Measurement::knee(Dimension::Api, findings, 4.0, "b", "r"),
                Measurement::share(Dimension::Hotspots, findings / 4, modules, "b", "r"),
            ];
            let card = Scorecard::of(&measurements, &Policy::defaults(), "rev", 7);
            let sum: i64 = card.terms.iter().map(|term| term.points).sum();
            assert_eq!(card.total, sum, "{findings}/{modules}");
            assert_eq!(card.check_sum(), Points(sum));
            let expected = ((sum as f64 / 100.0).round()).clamp(0.0, 100.0) as u8;
            assert_eq!(card.score, expected, "{findings}/{modules}");
            // The printed decimal of the total is the printed decimals of the parts, summed.
            let shown: f64 = card
                .terms
                .iter()
                .map(|term| f64::from(i32::try_from(term.points).unwrap_or(i32::MAX)) / 100.0)
                .fold(0.0_f64, |total, part| total + part);
            assert!(
                (shown - f64::from(i32::try_from(sum).unwrap_or(i32::MAX)) / 100.0).abs() < 5e-7,
                "{findings}/{modules}: {shown} vs {sum}"
            );
            assert!(card.score <= 100);
        }
    }
}

/// The two curves, as the two formulas and the two lines that quote them.
#[test]
fn every_curve_line_quotes_the_integers_it_used() {
    let share = Curve::Share {
        numerator: 3,
        denominator: 412,
    };
    assert_eq!(share.kind(), "share");
    assert_eq!(share.count(), 3);
    assert_eq!(share.formula(), "3 / 412 = 0.0073");
    assert_eq!(badness_of(&share), 0.0073);

    let knee = Curve::Knee {
        count: 4,
        knee: 4.0,
    };
    assert_eq!(knee.kind(), "knee");
    // The knee is the curve for a finding with no honest denominator: 4 vanished exports against a knee
    // of 4 is exactly a half, and the formula line says so in the order a reader will redo it.
    assert_eq!(knee.formula(), "4 / (4 + 4) = 0.5");
    assert_eq!(badness_of(&knee), 0.5);
    assert_eq!(
        badness_of(&Curve::Knee {
            count: 0,
            knee: 4.0
        }),
        0.0
    );
    assert!(
        badness_of(&Curve::Knee {
            count: 9,
            knee: 4.0
        }) < 1.0
    );
    assert!(
        badness_of(&Curve::Knee {
            count: 9,
            knee: 4.0
        }) > badness_of(&Curve::Knee {
            count: 8,
            knee: 4.0
        }),
        "the knee is monotone in the count"
    );

    // A share is a share: 0 when nothing was found, 1 when everything was, and never anything outside.
    assert_eq!(
        badness_of(&Curve::Share {
            numerator: 0,
            denominator: 9
        }),
        0.0
    );
    assert_eq!(
        badness_of(&Curve::Share {
            numerator: 9,
            denominator: 9
        }),
        1.0
    );
    assert_eq!(
        badness_of(&Curve::Share {
            numerator: 12,
            denominator: 9
        }),
        1.0
    );
    // A zero denominator is not "infinitely bad" and it is not a divide-by-zero: the dimension refuses
    // before this point, and the value here is the harmless one.
    assert_eq!(
        badness_of(&Curve::Share {
            numerator: 4,
            denominator: 0
        }),
        0.0
    );
    assert!(
        badness_of(&Curve::Share {
            numerator: 1,
            denominator: -3
        })
        .is_finite()
    );
}

/// **Same measurements ⇒ same bytes.**
///
/// Stated as the stored payload rather than as float equality, because the stored payload is what the trend
/// folds and what a year-old artefact is read from.
#[test]
fn measuring_the_same_graph_twice_produces_identical_bytes() {
    let first = card().to_stored();
    let second = card().to_stored();
    assert_eq!(first, second);
    let reparsed = Scorecard::from_stored(&first).expect("a stored scorecard reads back");
    assert_eq!(reparsed.score, 88);
    assert_eq!(reparsed.total, 8800);
    assert_eq!(
        reparsed.to_stored(),
        first,
        "and re-storing it is the same row"
    );
    assert_eq!(
        reparsed.term(Dimension::Api).map(|t| t.measured),
        Some(false)
    );
    // The prose is deliberately not stored — a trend that stored the same sentence 500 times would be a
    // trend that stored the same sentence 500 times — so a read-back has empty reason lines and says so.
    assert!(
        reparsed
            .term(Dimension::Boundaries)
            .unwrap()
            .reason
            .is_empty()
    );
    assert!(
        reparsed
            .term(Dimension::Api)
            .and_then(|term| term.unscored.as_deref())
            .unwrap_or_default()
            .contains("prose was not stored"),
        "an absence is named, not left blank"
    );

    // A different clock is a different row and the same score: `built_utc` orders the trend, it is not an
    // input to the arithmetic.
    let later = Scorecard::of(&worked_example(), &Policy::defaults(), &"a".repeat(40), 999);
    assert_eq!(later.score, 88);
    assert_ne!(later.to_stored(), first);
    // Garbage is refused rather than read as a zero.
    assert!(Scorecard::from_stored("").is_none());
    assert!(Scorecard::from_stored("not json").is_none());
    assert!(Scorecard::from_stored(r#"{"model":"x"}"#).is_none());
}

/// An unmeasured dimension renormalises the denominator; a zero denominator refuses to score at all.
#[test]
fn an_unmeasured_dimension_renormalises_and_an_empty_denominator_refuses() {
    let card = card();
    // The api weight left the denominator, and the report says which dimensions were measured.
    assert_eq!(card.denominator, 0.85);
    assert!(
        card.notices
            .iter()
            .any(|why| why == "api: no earlier revision in the index to compare against"),
        "{:?}",
        card.notices
    );

    // Every dimension unmeasured: no score, and the sentence says why it is not a zero.
    let nothing: Vec<Measurement> = Dimension::ALL
        .iter()
        .map(|dimension| {
            Measurement::unmeasured(*dimension, "this index cannot see it", "nothing examined")
        })
        .collect();
    let card = Scorecard::of(&nothing, &Policy::defaults(), "rev", 1);
    assert!(!card.scored);
    assert_eq!(card.score, 0);
    assert_eq!(card.total, 0);
    assert!(!card.alert, "an unscored revision is not an alerting one");
    assert!(
        card.vector().starts_with("architecture-1 — no score: "),
        "{}",
        card.vector()
    );
    assert!(
        card.unscored
            .as_deref()
            .unwrap()
            .contains("not the same as a score of zero"),
        "{:?}",
        card.unscored
    );
    assert!(card.headline().contains("no score"), "{}", card.headline());

    // A dimension weighted to zero is measured but contributes nothing, and its weight is still in the
    // denominator — which is the difference between "you told me not to care" and "I could not see it".
    let mut policy = Policy::defaults();
    policy.weights[Dimension::Hotspots.index()] = 0.0;
    let card = Scorecard::of(&worked_example(), &policy, "rev", 1);
    assert!(card.scored);
    // The denominator is the weights of the dimensions that were *measured*: 0.30 + 0.15 + 0.25 + 0.0.
    // The unmeasured api is not in it, and the zero-weighted hotspots is — a refusal and a choice are
    // different facts and they must not renormalise the same way.
    assert_eq!(card.denominator, 0.7);
    assert_eq!(card.term(Dimension::Hotspots).unwrap().points, 0);
    assert_eq!(card.term(Dimension::Hotspots).unwrap().ceiling, 0);
    assert!(
        card.term(Dimension::Hotspots).unwrap().measured,
        "measured, weighted zero, and printed as both"
    );

    // And a policy that zeroes every weight has nothing to say.
    let none = Policy {
        weights: [0.0; 5],
        ..Policy::defaults()
    };
    let card = Scorecard::of(&worked_example(), &none, "rev", 1);
    assert!(!card.scored);
    assert!(card.unscored.unwrap().contains("weighted zero"));
}

/// The `alert` flag is reported, and it is not an exit code — the gate belongs to `fathom check`.
#[test]
fn a_low_score_reports_alert_and_says_which_floor_it_used() {
    let measurements = vec![
        Measurement::share(Dimension::Boundaries, 300, 412, "b", "r"),
        Measurement::share(Dimension::Coupling, 600, 640, "b", "r"),
        Measurement::share(Dimension::Acyclicity, 200, 640, "b", "r"),
        Measurement::knee(Dimension::Api, 40, 4.0, "b", "r"),
        Measurement::share(Dimension::Hotspots, 60, 80, "b", "r"),
    ];
    let card = Scorecard::of(&measurements, &Policy::defaults(), "rev", 1);
    assert!(
        card.score < 50,
        "the fixture must actually be bad: {}",
        card.score
    );
    assert!(card.alert);
    assert_eq!(card.alert_below, 50);
    assert!(
        card.to_json()["alert_below"].as_u64() == Some(50),
        "the floor travels in the payload so a consumer does not hard-code it"
    );

    // Raising the floor does not change the score, only whether it is flagged.
    let loud = Policy {
        alert_below: 99,
        ..Policy::defaults()
    };
    let same = Scorecard::of(&measurements, &loud, "rev", 1);
    assert_eq!(same.score, card.score);
    assert!(same.alert);
    let quiet = Policy {
        alert_below: 0,
        ..Policy::defaults()
    };
    assert!(!Scorecard::of(&measurements, &quiet, "rev", 1).alert);
}

/// The API dimension is a knee, not a share, and it refuses at a grain that cannot see the answer.
#[test]
fn the_api_dimension_scores_vanished_exports_against_its_knee_and_refuses_otherwise() {
    let policy = Policy::defaults();
    assert_eq!(policy.api_knee, 4.0);
    let signal = ApiSignal {
        grain: fathom_analyze::scorecard::ApiGrain::Graph,
        breaking: 4,
        indeterminate: 0,
        arrived: 2,
        examined: 1048,
        complete: true,
        note: String::new(),
        measured: true,
    };
    let measurement = signal.measurement(&policy);
    assert!(measurement.measured);
    assert_eq!(
        measurement.badness(),
        0.5,
        "4 against a knee of 4 is a half"
    );
    assert!(measurement.reason.contains("4 export(s) left the graph"));
    assert!(
        measurement
            .evidence
            .contains(&("arrived".to_owned(), 2_i64)),
        "{:?}",
        measurement.evidence
    );

    // A signal that could not classify is a stated absence with its reason, not a zero.
    let blind =
        ApiSignal::absent("the graph grain cannot see a signature change").measurement(&policy);
    assert!(!blind.measured);
    assert_eq!(
        blind.unscored.as_deref(),
        Some("the graph grain cannot see a signature change")
    );
    // Indeterminate counts travel as caveats beside a score that is still printed.
    let fuzzy_signal = ApiSignal {
        indeterminate: 3,
        ..signal.clone()
    };
    let fuzzy = fuzzy_signal.measurement(&policy);
    assert!(
        fuzzy
            .caveats
            .iter()
            .any(|why| why.contains("3 change(s) this grain could not classify")),
        "{:?}",
        fuzzy.caveats
    );
    // Tuning the knee moves the badness, and the knob is reported as tuned.
    let tuned = Policy {
        api_knee: 36.0,
        ..Policy::defaults()
    };
    assert!((signal.measurement(&tuned).badness() - 0.1).abs() < 1e-9);
}

/// The trend is bounded: it keeps the newest half at full resolution and samples the rest, and it says how
/// much it dropped.
#[test]
fn a_long_history_is_sampled_in_bounded_memory_and_says_what_it_dropped() {
    let policy = Policy {
        trend_cap: 120,
        window: 30,
        ..Policy::defaults()
    };
    let mut trend = Trend::new(&policy);
    // Newest first, as the store returns them. The findings grow with age, so the series improves
    // chronologically and the window's fit has a direction to find.
    let mut fed: Vec<Scorecard> = Vec::new();
    for index in (0..5_000).rev() {
        let scorecard = Scorecard::of(
            &[Measurement::share(
                Dimension::Boundaries,
                4_999 - index,
                412,
                "b",
                "r",
            )],
            &Policy::defaults(),
            &format!("rev-{index:04}"),
            index,
        );
        trend.feed(&Row {
            revision: format!("rev-{index:04}"),
            built_utc: index,
            scorecard: scorecard.clone(),
        });
        fed.push(scorecard);
    }
    assert_eq!(trend.rows_read, 5_000);
    let points = trend.points();
    assert!(
        points.len() <= 120,
        "the cap is a ceiling: {} points",
        points.len()
    );
    assert!(
        trend.heap_bytes() < 500_000,
        "a trend that grew with the repository would be a leak: {} bytes",
        trend.heap_bytes()
    );
    assert!(
        trend.stride > 1,
        "the older half is sampled: stride {}",
        trend.stride
    );
    assert_eq!(trend.thinned(), 5_000 - points.len());
    // The head of the trend is the newest row fed, and `previous` is the one after it — the per-episode
    // step, which is a different question from the since-review delta.
    assert_eq!(trend.head.as_ref().unwrap().revision, "rev-4999");
    assert_eq!(trend.previous.as_ref().unwrap().revision, "rev-4998");
    let (head, previous) = trend.step().expect("two rows were fed");
    assert_eq!(head.revision, "rev-4999");
    assert_eq!(previous.revision, "rev-4998");
    assert_eq!(
        trend.window_points().len(),
        30,
        "the window is the newest 30 rows fed"
    );

    // The window's aggregates, recomputed here from the scorecards that were fed — the same thirty rows,
    // read a second way.
    let window = trend.window_summary();
    let thirty = &fed[..30];
    let scores: Vec<u8> = thirty.iter().map(|card| card.score).collect();
    let totals: Vec<i64> = thirty.iter().map(|card| card.total).collect();
    assert_eq!(window.revisions, 30);
    assert_eq!(window.requested, 30);
    assert_eq!(window.scored, 30);
    assert!(window.short_reason.is_none(), "{:?}", window.short_reason);
    assert_eq!(window.best.unwrap().0, *scores.iter().max().unwrap());
    assert_eq!(window.worst.unwrap().0, *scores.iter().min().unwrap());
    assert_eq!(window.first.unwrap().1, "rev-4970");
    assert_eq!(window.last.unwrap().1, "rev-4999");
    let hand_mean = (totals.iter().sum::<i64>() + 15) / 30;
    assert_eq!(window.mean_points, hand_mean);
    assert_eq!(
        window.mean_score,
        fathom_analyze::scorecard::dimension::round2(
            f64::from(i32::try_from(hand_mean).unwrap()) / 100.0
        )
    );
    // The least-squares fit, done independently: x runs 0..29 in chronological order, which is the
    // spelling the exported sums mean, and the exported sums are the ones printed.
    let (n, sum_x, sum_y, sum_xy, sum_xx) = scores.iter().rev().enumerate().fold(
        (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64),
        |(n, sum_x, sum_y, sum_xy, sum_xx), (x, score)| {
            let x = f64::from(i32::try_from(x).unwrap());
            let y = f64::from(u32::from(*score));
            (
                n + 1.0,
                sum_x + x,
                sum_y + y,
                sum_xy + x * y,
                sum_xx + x * x,
            )
        },
    );
    let hand_slope = (n * sum_xy - sum_x * sum_y) / (n * sum_xx - sum_x * sum_x);
    assert!(
        (window.slope - hand_slope).abs() < 1e-4,
        "{} vs {hand_slope}",
        window.slope
    );
    assert!(
        window.slope > 0.0,
        "the repository improved: {}",
        window.slope
    );
    assert_eq!(window.direction, Direction::Improved);
    assert_eq!(window.sums.n, 30.0);
    assert_eq!(window.sums.sum_x, sum_x);
    assert_eq!(window.sums.sum_y, sum_y);
    // Every dimension carries its own mean badness and the count it was averaged over — a mean over 4 of
    // 30 rows is a different claim from a mean over 30.
    let boundaries = window
        .mean_badness
        .iter()
        .find(|(id, ..)| id == "boundaries")
        .expect("five dimensions");
    assert_eq!(boundaries.3, 30);
    assert_eq!(boundaries.2, 30);
    // With one measured dimension the points *are* the health, so the mean badness is one minus the mean
    // of the points — the two exported aggregates have to be consistent with each other.
    let hand_badness = thirty
        .iter()
        .map(|card| 1.0 - f64::from(i32::try_from(card.total).unwrap()) / 10_000.0)
        .sum::<f64>()
        / 30.0;
    assert!(
        (boundaries.1 - hand_badness).abs() < 1e-3,
        "{} vs {hand_badness}",
        boundaries.1
    );
    let api = window
        .mean_badness
        .iter()
        .find(|(id, ..)| id == "api")
        .unwrap();
    assert_eq!(api.2, 0, "api was never measured, and the mean says so");
    assert_eq!(api.3, 30);

    // The sparkline is one glyph per plotted point.
    let spark = sparkline(&points);
    assert_eq!(spark.chars().count(), points.len());
    assert_eq!(sparkline(&[]), "");

    // A window longer than the index is a shorter window, and says why.
    let mut few = Trend::new(&policy);
    few.feed(&Row {
        revision: "only".to_owned(),
        built_utc: 1,
        scorecard: card(),
    });
    let summary = few.window_summary();
    assert_eq!(summary.revisions, 1);
    assert!(summary.short_reason.unwrap().contains("rather than 30"));
    assert!(few.step().is_none(), "one row is not a step");
}

/// Two refusals, two counters, and neither one averaged into the answer.
#[test]
fn a_row_from_another_model_is_refused_and_counted_not_folded_in() {
    let mut trend = Trend::new(&Policy::defaults());
    trend.feed(&Row {
        revision: "good".to_owned(),
        built_utc: 2,
        scorecard: card(),
    });
    trend.refuse_foreign();
    trend.refuse_foreign();
    trend.count_unreadable();
    assert_eq!(trend.rows_foreign, 2);
    assert_eq!(trend.rows_unreadable, 1);
    assert_eq!(trend.points().len(), 1, "a refused row is not a point");
    assert_eq!(trend.rows_read, 1, "and it is not counted as read either");
    let json = trend.to_json();
    assert_eq!(json["rows_read"], 1);
    assert_eq!(json["rows_foreign"], 2);
    assert_eq!(json["rows_unreadable"], 1);
    let rendered = trend.render().join("\n");
    assert!(
        rendered.contains("2"),
        "the render says what was refused:\n{rendered}"
    );

    // `Row::parse` is where a stored row is judged, and it distinguishes the two failures because they
    // have different remedies: an old model is `--refresh`, a truncated row is a corrupt index.
    let mut foreign = card().to_stored();
    foreign = foreign.replacen(
        r#""model":"architecture-1""#,
        r#""model":"architecture-0""#,
        1,
    );
    match Row::parse("rev", 1, &foreign) {
        Parse::Foreign { model } => assert_eq!(model, "architecture-0"),
        other => panic!("expected a foreign refusal, got {other:?}"),
    }
    assert!(matches!(
        Row::parse("rev", 1, "nonsense"),
        Parse::Unreadable
    ));
    assert!(matches!(
        Row::parse("rev", 1, &card().to_stored()),
        Parse::Row(_)
    ));
}

/// **The "since my last review" anchor, correct across a rebase.**
#[test]
fn the_anchor_ladder_falls_through_its_rungs_and_says_what_each_one_cost() {
    let rows: Vec<Row> = (0..6)
        .map(|index| {
            let mut scorecard = Scorecard::of(
                &[Measurement::share(
                    Dimension::Boundaries,
                    index,
                    412,
                    "b",
                    "r",
                )],
                &Policy::defaults(),
                &format!("rev-{index}"),
                1_000 - index * 100,
            );
            scorecard.episode = Some(format!("ep-{index}"));
            Row {
                revision: format!("rev-{index}"),
                built_utc: 1_000 - index * 100,
                scorecard,
            }
        })
        .collect();
    let review = Review {
        episode: "ep-4".to_owned(),
        fingerprint: "f".repeat(40),
        state: "accepted".to_owned(),
        actor: "Rue Reviewer <rue@example.invalid>".to_owned(),
        revision: Some("rev-4".to_owned()),
        instant: 500,
        relinked_via: "strong".to_owned(),
    };

    // (1) the reviewed revision is itself scored: the exact answer, and nothing else is consulted.
    let anchor = resolve(Some(&review), &rows);
    assert_eq!(anchor.kind, AnchorKind::Revision);
    assert_eq!(anchor.revision.as_deref(), Some("rev-4"));
    assert!(anchor.why.contains("the review's own revision is scored"));
    assert!(anchor.sentence().contains("accepted"));

    // (2) the history was rewritten so the reviewed revision is gone, but the episode survived the relink
    // and is still live: anchor on the row scored *as* that episode, and say that the original revision is
    // no longer in the index. This is the rebase case the acceptance criteria name.
    let rewritten = Review {
        revision: Some("rev-gone".to_owned()),
        ..review.clone()
    };
    let anchor = resolve(Some(&rewritten), &rows);
    assert_eq!(anchor.kind, AnchorKind::Episode);
    assert_eq!(anchor.revision.as_deref(), Some("rev-4"));
    assert!(anchor.why.contains("no longer in the index"));
    assert!(!AnchorKind::Episode.price().is_empty());

    // (3) no revision, no episode — only the reviewer's own timestamp. The anchor is the oldest scored
    // revision indexed at or after it, and the rung's weakness (index times, not commit times) is priced.
    let orphan = Review {
        episode: "ep-gone".to_owned(),
        revision: None,
        instant: 350,
        ..review.clone()
    };
    let anchor = resolve(Some(&orphan), &rows);
    assert_eq!(anchor.kind, AnchorKind::Instant);
    // rev-5 is the *oldest* scored revision indexed at or after the review: everything after it is
    // "since", which is the widest defensible reading of a timestamp that names no revision.
    assert_eq!(anchor.revision.as_deref(), Some("rev-5"));
    assert!(
        anchor
            .why
            .contains("oldest scored revision indexed after it")
    );

    // (4) nothing to tie to at all: the whole stored window is the delta, which is the honest answer and
    // the most expensive one.
    let before_index = Review {
        instant: 5_000,
        ..orphan.clone()
    };
    let anchor = resolve(Some(&before_index), &rows);
    assert_eq!(anchor.kind, AnchorKind::Window);
    assert!(anchor.why.contains("spans the 6 scored"));

    // (5) nobody has reviewed anything: the delta is the whole trend, and the report says who is at fault.
    let anchor = resolve(None, &rows);
    assert_eq!(anchor.kind, AnchorKind::None);
    assert!(anchor.why.contains("nobody has reviewed anything"));
    // A review that exists only as a row with no state is the same as none.
    assert!(!Review::none().exists());
    assert!(resolve(Some(&Review::none()), &rows).kind == AnchorKind::None);
    // No rows at all is a refusal, not a panic.
    let empty = resolve(Some(&review), &[]);
    assert!(empty.revision.is_none());
    assert_eq!(
        empty.kind,
        AnchorKind::Window,
        "nothing is scored, so the whole (empty) window is the delta"
    );
}

/// The delta is the one number the ticket asks for, with the points underneath it.
#[test]
fn the_delta_is_one_number_with_the_points_underneath_and_the_rungs_price_beside_it() {
    // Two revisions of the same repository, five measurements each, and one honest difference: the first
    // revision had no API read and the second does, which is the case a delta must refuse to compare
    // rather than invent a movement for.
    let before = Scorecard::of(
        &[
            Measurement::share(Dimension::Boundaries, 3, 412, "b", "r"),
            Measurement::share(Dimension::Coupling, 100, 640, "b", "r"),
            Measurement::share(Dimension::Acyclicity, 0, 640, "b", "r"),
            Measurement::unmeasured(Dimension::Api, "no earlier revision", "b"),
            Measurement::share(Dimension::Hotspots, 1, 80, "b", "r"),
        ],
        &Policy::defaults(),
        "rev-before",
        1,
    );
    let after = Scorecard::of(
        &[
            Measurement::share(Dimension::Boundaries, 90, 412, "b", "r"),
            Measurement::share(Dimension::Coupling, 120, 640, "b", "r"),
            Measurement::share(Dimension::Acyclicity, 40, 640, "b", "r"),
            Measurement::knee(Dimension::Api, 0, 4.0, "b", "r"),
            Measurement::share(Dimension::Hotspots, 1, 80, "b", "r"),
        ],
        &Policy::defaults(),
        "rev-after",
        2,
    );
    let anchor = anchor_of(AnchorKind::Revision);
    let delta = Delta::between(&before, &after, anchor, 23, 2);
    assert!(delta.comparable);
    assert_eq!(delta.from_score, before.score);
    assert_eq!(delta.to_score, after.score);
    assert_eq!(
        delta.score_delta,
        i64::from(after.score) - i64::from(before.score)
    );
    assert!(
        delta.score_delta < 0,
        "the fixture is a regression: {delta:?}"
    );
    assert_eq!(delta.direction, Direction::Declined);
    assert_eq!(
        delta.points_delta,
        after.points_delta(&before),
        "the points delta is the scorecards' own subtraction, in hundredths"
    );
    // The hand arithmetic: the total fell by exactly the sum of the five terms' movements.
    let hand: i64 = delta.terms.iter().map(|term| term.points_delta).sum();
    assert_eq!(delta.points_delta, hand, "the parts add to the whole");
    // Per dimension: badness before, after, the difference, and the points it cost.
    let boundaries = delta
        .terms
        .iter()
        .find(|term| term.dimension == Dimension::Boundaries)
        .expect("five terms");
    assert!(boundaries.comparable);
    assert_eq!(boundaries.from_badness, Some(0.0073));
    assert_eq!(boundaries.to_badness, Some(0.2184));
    assert_eq!(boundaries.badness_delta, Some(0.2111));
    assert_eq!(
        boundaries.direction,
        Direction::Declined,
        "more badness is a decline"
    );
    assert_eq!(
        boundaries.points_delta,
        after.term(Dimension::Boundaries).unwrap().points
            - before.term(Dimension::Boundaries).unwrap().points
    );
    // Unchanged in both: zero movement, and the band says flat.
    let hotspots = delta
        .terms
        .iter()
        .find(|term| term.dimension == Dimension::Hotspots)
        .expect("hotspots");
    assert_eq!(hotspots.badness_delta, Some(0.0));
    assert_eq!(hotspots.direction, Direction::Flat);
    // The dimensions that moved most, worst first.
    assert_eq!(delta.worst.first(), Some(&Dimension::Boundaries));
    // api was measured at one end only, so its movement is not knowable and says so.
    let api = delta
        .terms
        .iter()
        .find(|term| term.dimension == Dimension::Api)
        .expect("api");
    assert!(!api.comparable);
    assert_eq!(api.direction, Direction::Unknown);
    assert!(
        api.why
            .as_deref()
            .unwrap_or_default()
            .contains("measured at one end only"),
        "{api:?}"
    );

    // The headline is the ticket's sentence, and the basis names the anchor, the span and the band.
    assert!(
        delta
            .headline()
            .starts_with("since your last review (23 scored revisions): "),
        "{}",
        delta.headline()
    );
    assert!(
        delta
            .basis
            .contains("23 scored revision(s) · flat within 2 point(s)")
    );
    let rendered = delta.render().join("\n");
    assert!(rendered.contains("boundaries"), "{rendered}");
    assert!(
        rendered.contains(AnchorKind::Revision.price()),
        "the rung's price is printed beside the number it produced:\n{rendered}"
    );
    let json = delta.to_json();
    assert_eq!(json["revisions"], 23);
    assert_eq!(json["anchor"]["kind"], "revision");
    assert_eq!(json["anchor"]["price"], AnchorKind::Revision.price());
    assert_eq!(json["direction"], "declined");
    assert_eq!(json["worst"].as_array().unwrap()[0], "boundaries");
    // One scored side and one unscored side is not a movement, and says so instead of printing −74.
    let blind = Scorecard::of(&[], &Policy::defaults(), "rev-blind", 3);
    let delta = Delta::between(&before, &blind, anchor_of(AnchorKind::Window), 4, 2);
    assert!(!delta.comparable);
    assert_eq!(delta.direction, Direction::Unknown);
    assert!(delta.headline().contains("no score on one side"));
    assert!(
        delta
            .notices
            .iter()
            .any(|why| why.contains("not printed as movement")),
        "{:?}",
        delta.notices
    );
}

/// A tiny constructor so the delta test does not have to build an `Anchor` literal each time.
fn anchor_of(kind: AnchorKind) -> fathom_analyze::scorecard::Anchor {
    fathom_analyze::scorecard::Anchor {
        kind,
        revision: Some("rev-4".to_owned()),
        review: None,
        why: "anchored by the test".to_owned(),
    }
}

/// The flat band: a movement inside it is reported as flat, and the band is named either way.
#[test]
fn a_movement_inside_the_band_is_flat_and_the_band_is_named_either_way() {
    let healthy = Measurement::share(Dimension::Boundaries, 3, 412, "b", "r");
    let before = Scorecard::of(
        std::slice::from_ref(&healthy),
        &Policy::defaults(),
        "rev",
        1,
    );
    // One more violation in 412 rows moves the score by well under a point.
    let after = Scorecard::of(
        &[Measurement::share(Dimension::Boundaries, 4, 412, "b", "r")],
        &Policy::defaults(),
        "rev",
        2,
    );
    assert_eq!(before.score, after.score, "both round to the same score");
    assert!(
        before.points_delta(&after).abs() > 0,
        "but not the same points"
    );
    let anchor = anchor_of(AnchorKind::Instant);
    let delta = Delta::between(&before, &after, anchor, 1, 2);
    assert_eq!(delta.direction, Direction::Flat);
    assert!(!before.moved(&after, 2), "two points of band covers this");
    assert!(before.moved(&after, 0), "and a band of zero does not");
    assert!(delta.basis.contains("flat within 2 point(s)"));
    assert_eq!(Direction::Flat.arrow(), "±");
    assert_eq!(Direction::Improved.arrow(), "↑");
}

/// The whole report: one read, two renderings, and the numbers agree.
#[test]
fn one_read_prints_the_same_numbers_in_the_shell_and_in_the_payload() {
    let policy = Policy::defaults();
    let head = card();
    let previous = Scorecard::of(
        &[
            Measurement::share(Dimension::Boundaries, 1, 412, "b", "r"),
            Measurement::share(Dimension::Coupling, 100, 640, "b", "r"),
            Measurement::share(Dimension::Acyclicity, 0, 640, "b", "r"),
            Measurement::knee(Dimension::Api, 0, 4.0, "b", "r"),
            Measurement::share(Dimension::Hotspots, 1, 80, "b", "r"),
        ],
        &policy,
        "b".repeat(40).as_str(),
        2,
    );
    let mut trend = Trend::new(&policy);
    trend.feed(&Row {
        revision: "a".repeat(40),
        built_utc: 3,
        scorecard: head.clone(),
    });
    trend.feed(&Row {
        revision: "b".repeat(40),
        built_utc: 2,
        scorecard: previous.clone(),
    });
    trend.refuse_foreign();
    let anchor = resolve(
        Some(&Review {
            episode: "ep-old".to_owned(),
            fingerprint: "f".repeat(40),
            state: "read".to_owned(),
            actor: String::new(),
            revision: Some("b".repeat(40)),
            instant: 1,
            relinked_via: String::new(),
        }),
        &[],
    );
    let since = Delta::between(&previous, &head, anchor, 23, policy.flat_within);
    let report = Report {
        head: head.clone(),
        previous: Some(previous.clone()),
        since: Some(since),
        since_recorded: None,
        trend,
        policy: policy.clone(),
        index: Index {
            revisions: 41,
            scored: 2,
            read: 2,
            newest: "a".repeat(40),
            head: "a".repeat(40),
            episode: Some("ep-head".to_owned()),
            unscored: 39,
        },
        notices: vec!["a notice from the caller".to_owned()],
    };

    let json = report.to_json();
    assert_eq!(json["contract"], "fathom-architecture/1");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["headline"], report.headline());
    assert_eq!(json["scorecard"]["score"], u64::from(head.score));
    assert_eq!(json["scorecard"]["model"], "architecture-1");
    assert_eq!(json["previous"]["score"], u64::from(previous.score));
    assert_eq!(json["since_last_review"]["revisions"], 23);
    assert_eq!(json["index"]["revisions"], 41);
    assert_eq!(json["index"]["unscored"], 39);
    assert_eq!(json["trend"]["rows_foreign"], 1);
    assert_eq!(json["policy"]["weight_sum"], 1.0);
    assert_eq!(json["policy"]["untouched"], true);
    // Every term is exported with the integers a hand-check needs, twice each: inside the `formula` line
    // and as a named field.
    let terms = json["scorecard"]["terms"].as_array().expect("five terms");
    assert_eq!(terms.len(), 5);
    for term in terms {
        let formula = term["formula"].as_str().unwrap_or_default();
        assert!(
            term["badness"].is_number() && term["health"].is_number(),
            "{term}"
        );
        let points = term["points"].as_i64().unwrap();
        assert_eq!(
            term["points_shown"].as_str(),
            Some(Points(points).to_string().as_str()),
            "the shown form is the hundredths, printed: {term}"
        );
        if term["measured"].as_bool().unwrap() {
            assert!(formula.contains('/'), "{term}");
            assert!(term["question"].is_string());
            assert!(term["source"].as_str().unwrap().starts_with("e3."));
        } else {
            assert!(term["unscored"].is_string(), "{term}");
        }
    }
    // The notices the reader needs: the refused rows, the unscored revisions, and the caller's own.
    let notices = json["notices"].as_array().expect("notices");
    let joined = notices.iter().map(value_str).collect::<Vec<_>>().join("\n");
    assert!(joined.contains("another model"), "{joined}");
    assert!(joined.contains("39 of 41"), "{joined}");
    assert!(joined.contains("a notice from the caller"), "{joined}");
    assert!(
        joined.contains(AnchorKind::Window.price())
            || joined.contains(AnchorKind::Revision.price()),
        "the anchor's price is a complaint the reader sees: {joined}"
    );

    // And the shell prints the same score it exported, in the same read.
    let text = report.lines().join("\n");
    assert!(text.contains("architecture-1 ·"), "{text}");
    assert!(
        text.contains(&format!("{} → {}", previous.score, head.score)),
        "{text}"
    );
    assert!(text.contains("since last review"), "{text}");
    assert!(text.contains("since your last review"), "{text}");
    assert!(text.contains("scorecard"), "{text}");
    assert!(text.contains("trend"), "{text}");
    assert!(text.contains("knobs"), "{text}");
    assert!(text.contains("weight boundaries  0.30"), "{text}");
    assert!(text.contains("what this could not do"), "{text}");
    assert!(text.contains("3 / 412 = 0.0073"), "{text}");
    assert!(
        text.contains("no earlier revision in the index to compare against"),
        "the refusal is printed in the shell too: {text}"
    );
    // The headline is the score and the review delta in one line, which is what a CI log prints.
    assert_eq!(
        report.headline(),
        format!(
            "{} · {}",
            head.headline(),
            report.since.as_ref().unwrap().headline()
        )
    );
    // Text and JSON are one read: same length of trend, same number of notices.
    assert_eq!(
        json["trend"]["series"].as_array().map(Vec::len),
        Some(report.trend.points().len())
    );
    assert_eq!(json["trend"]["plotted"], report.trend.points().len());
    assert_eq!(json["trend"]["window"]["revisions"], 2);
    assert_eq!(json["since_last_review"]["anchor"]["kind"], "window");
    assert_eq!(json["since_last_review"]["direction"], "declined");
    assert_eq!(
        json["since_last_review"]["score_delta"],
        i64::from(head.score) - i64::from(previous.score)
    );
    assert_eq!(
        json["notices"].as_array().map(Vec::len),
        Some(report.complaints().len())
    );
}

/// One JSON value as the string a reader would see, for the notice assertions above.
fn value_str(value: &serde_json::Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// The knobs are config, the defaults are the published constants, and a knob that will not parse is
/// reported rather than coerced.
#[test]
fn the_knobs_are_the_published_constants_and_a_bad_one_is_reported_not_coerced() {
    let policy = Policy::defaults();
    assert_eq!(policy.weights, [0.30, 0.15, 0.25, 0.15, 0.15]);
    assert_eq!(policy.weight_sum(), 1.0);
    assert!(policy.untouched());
    assert_eq!(policy.window, 30);
    assert_eq!(policy.flat_within, 2);
    assert_eq!(policy.trend_cap, 240);
    assert_eq!(policy.coupling_ceiling, 8.0);
    assert_eq!(policy.api_knee, 4.0);
    assert_eq!(policy.hotspot_bar, 70);
    assert_eq!(policy.alert_below, 50);
    // Every dimension names the ticket that named it, the question it answers and the default it carries.
    for dimension in Dimension::ALL {
        assert!(!dimension.id().is_empty());
        assert!(
            dimension.question().ends_with('?'),
            "{:?}",
            dimension.question()
        );
        assert!(
            dimension.source().starts_with("e3."),
            "{:?}",
            dimension.source()
        );
        assert!((dimension.default_weight() - policy.weight(dimension)).abs() < 1e-9);
        assert_eq!(Dimension::parse(dimension.id()), Some(dimension));
    }
    assert_eq!(Dimension::Coupling.index(), 1);
    assert_eq!(Dimension::parse("nonsense"), None);

    let (tuned, notices) = fathom_analyze::scorecard::policy_from_config_text(
        r#"
[scorecard]
window = 12
flat_within = 5
trend_cap = 500
coupling_ceiling = 12.0
api_knee = 8
hotspot_bar = 60
alert_below = 40

[scorecard.weight]
boundaries = 0.5
coupling = "0.2"
acyclicity = 2
api = "not a number"
"#,
    );
    assert!(
        notices.iter().any(|why| why.contains("acyclicity")),
        "{notices:?}"
    );
    assert!(notices.iter().any(|why| why.contains("api")), "{notices:?}");
    assert!(!tuned.untouched());
    assert_eq!(tuned.weights[0], 0.5);
    assert_eq!(
        tuned.origin(Dimension::Boundaries),
        fathom_analyze::scorecard::Origin::Repo
    );
    assert_eq!(
        tuned.origin(Dimension::Hotspots),
        fathom_analyze::scorecard::Origin::Default
    );
    assert_eq!(
        tuned.weights[1], 0.2,
        "a quoted number is a number to a person"
    );
    assert_eq!(tuned.weights[2], 0.25, "the refused value kept the default");
    assert_eq!(tuned.window, 12);
    assert_eq!(tuned.flat_within, 5);
    assert_eq!(tuned.trend_cap, 500);
    assert_eq!(tuned.coupling_ceiling, 12.0);
    assert_eq!(tuned.api_knee, 8.0);
    assert_eq!(tuned.hotspot_bar, 60);
    assert_eq!(tuned.alert_below, 40);
    // A weight tuned into a scorecard changes the score, and the report says the denominator changed.
    let card = Scorecard::of(&worked_example(), &tuned, "rev", 1);
    // 0.50 + 0.20 + 0.25 + 0.15, api still unmeasured: the denominator is what the printed weights add
    // to, and a reader can add them up from the report.
    assert_eq!(card.denominator, 1.1);
    // The score itself is a weighted *mean* of the five healths, so retuning weights moves the terms
    // without necessarily moving the total — which is exactly why the terms are printed as well as the
    // number. Boundaries went from a third of the weight to nearly a half.
    assert_eq!(
        card.term(Dimension::Boundaries).unwrap().points,
        (10_000.0_f64 * 0.5 * 0.9927 / 1.1).round() as i64
    );
    assert!(card.term(Dimension::Boundaries).unwrap().ceiling > 4000);

    // A file with no `[scorecard]` table is not an error, and a file that does not parse says so.
    let (quiet, notices) =
        fathom_analyze::scorecard::policy_from_config_text("[hotspots]\nwindow = 5\n");
    assert!(notices.is_empty() && quiet.untouched());
    let (broken, notices) =
        fathom_analyze::scorecard::policy_from_config_text("this is = not = toml");
    assert!(
        notices.iter().any(|why| why.contains("does not parse")),
        "{notices:?}"
    );
    assert!(broken.untouched());
    // The knob lines name every knob, so the report can be audited from the terminal alone.
    let lines = policy.lines().join("\n");
    for knob in [
        "weight",
        "window",
        "flat within",
        "trend cap",
        "coupling_ceiling",
        "api_knee",
        "hotspot_bar",
        "alert_below",
    ] {
        assert!(lines.contains(knob), "{lines}");
    }
    assert!(policy.to_json()["weights"].as_array().unwrap().len() == 5);
}

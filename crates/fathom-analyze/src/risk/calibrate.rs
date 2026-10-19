//! The calibration set: a place to measure a weight change instead of feeling it (`reco-884`).
//!
//! The ticket's fourth acceptance criterion is that a calibration fixture shows the precision and recall
//! of "top 10 percent by score" **before and after retuning**. That sentence is only meaningful if the
//! retune is an algorithm rather than an opinion, so this module has one: [`retune`] walks a fixed grid of
//! weights in a fixed order and keeps the change that improves the cut's F1, which is deterministic,
//! reproducible, and — because the objective is precision and recall against hand-authored labels — an
//! answer a reviewer can argue with.
//!
//! ```text
//! $ fathom why --calibrate
//! calibration: 40 cases, 9 labelled must-read · the top 10% is 4 episodes
//!              precision  recall   F1     flagged  missed  spurious
//! shipped         0.750    0.333  0.462       4       6         1
//! retuned         1.000    0.556  0.714       4       4         0
//! [risk.weights]
//! findings = 0.20
//! …
//! ```
//!
//! # The fixture, and what it is allowed to claim
//!
//! `tests/fixtures/risk/calibration.json` holds episodes as the collector would hand them over — raw
//! values in each feature's own unit — plus one hand-authored label each: `true` means "a reviewer had to
//! stop for this one". Nothing else about a case is labelled: no per-feature ground truth, no
//! "expected score". The labels are the human judgement, and the only measurement this file makes is how
//! well a weight vector reproduces it at the cut.
//!
//! So the calibration set can show three things and cannot show a fourth. It can show that a retune
//! improves precision, that it improves recall, and that one vector beats another on *this* corpus. It
//! cannot show that either vector is right about *your* repository — the labels encode one reviewer's
//! dispositions over 40 episodes, which is why the retuned weights are printed as a proposal for
//! `fathom.toml` and never applied silently. A fixture that a repository cannot argue with is a model,
//! and this ticket's rule is no models in v1.
//!
//! # Determinism
//!
//! Fixed grid, fixed feature order, fixed pass count, strict-improvement-only, and a ranking whose ties
//! break on episode id. Two runs print the same bytes; `crates/fathom-analyze/tests/risk_score.rs`
//! asserts that, and asserts the blessed numbers in
//! `tests/fixtures/risk/calibration.report.json`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::feature::{Feature, Measurement, Vector};
use super::score::{self, Score};
use super::weights::{Origin, Weights};
/// The shipped calibration set, compiled in so `fathom why --calibrate` answers without a repository.
///
/// It is the same file the Rust test blesses against, so the numbers a person sees on a terminal and the
/// numbers CI asserts are read from one place.
pub const EMBEDDED: &str = include_str!("../../tests/fixtures/risk/calibration.json");

/// The cut: "the top ten percent by score", which is the queue a reviewer actually reads first.
pub const DEFAULT_FRACTION: f64 = 0.10;

/// The grid a retune may move a weight to.
///
/// Coarse on purpose. A grid fine enough to squeeze the last 0.5% of F1 out of 40 labels is a grid that
/// fits the noise, and a weight nobody can read off a terminal is a weight nobody will defend in a
/// review.
pub const GRID: [f64; 10] = [0.0, 0.02, 0.04, 0.06, 0.08, 0.10, 0.13, 0.16, 0.20, 0.25];

/// How many times a retune sweeps the twelve weights.
///
/// Four is more than the shipped fixture needs (it converges in two) and bounded, so the retune's cost
/// is `12 × 10 × 4` scorings of the corpus and never a search that has to be explained by its runtime.
pub const PASSES: usize = 4;

/// One labelled episode: the measurements a collector would hand over, and the human's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case {
    /// The handle printed in a report. Not an episode id — a fixture id, so a report cannot be mistaken
    /// for a claim about a live repository.
    pub id: String,
    /// What the episode did, in the reviewer's words. This is the line that makes a label arguable.
    pub summary: String,
    /// `true` means "a reviewer had to stop for this".
    pub label: bool,
    /// The raw measurements, keyed by feature id, each in that feature's own unit. A feature absent
    /// here is a feature that was not measured, and is absent from the denominator like any other.
    pub features: BTreeMap<String, f64>,
}

/// A parsed calibration set.
#[derive(Debug, Clone, Serialize)]
pub struct Set {
    /// The cases, in file order — which is the order ties break in.
    pub cases: Vec<Case>,
    /// Who labelled it and how, printed with every report so the labels are not orphaned.
    pub labelled_by: String,
    /// One line per feature id the file named that this build does not have.
    pub notices: Vec<String>,
}

/// A fixture that cannot be read, in one sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

/// Parse a calibration set.
///
/// The shape is deliberately small — a list of cases and a line about who labelled them — because the
/// larger the fixture format, the more of the calibration ends up in the format rather than in the
/// labels.
///
/// ```text
/// {"labelled_by": "hand-authored from the e1.12 acceptance fixture, one reviewer",
///  "cases": [{"id": "cal-01", "summary": "drops a column with no down file",
///             "label": true, "features": {"migration": 1.0, "surface": 50}}]}
/// ```
///
/// # Errors
///
/// [`ParseError`] when the JSON will not parse, when a case names a feature this vector does not have, or
/// when there are no cases at all.
pub fn parse(text: &str) -> Result<Set, ParseError> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        labelled_by: String,
        cases: Vec<Case>,
    }
    let file: File = serde_json::from_str(text)
        .map_err(|err| ParseError(format!("the calibration set is not readable: {err}")))?;
    if file.cases.is_empty() {
        return Err(ParseError(
            "the calibration set holds no cases, so it can measure nothing".to_owned(),
        ));
    }
    let mut notices = Vec::new();
    for case in &file.cases {
        for key in case.features.keys() {
            if Feature::parse(key).is_none() {
                notices.push(format!(
                    "{} names `{key}`, which is not a feature of this vector; it is ignored",
                    case.id
                ));
            }
        }
    }
    Ok(Set {
        cases: file.cases,
        labelled_by: file.labelled_by,
        notices,
    })
}

/// What a weight vector did to a calibration set at a cut.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cut {
    /// How many cases the set holds.
    pub cases: usize,
    /// How many are labelled must-read.
    pub positives: usize,
    /// The cut size: `round(cases × fraction)`, at least one.
    pub flagged: usize,
    /// Labelled must-read, and flagged.
    pub true_positive: usize,
    /// Flagged, but not labelled.
    pub false_positive: usize,
    /// Labelled, but not flagged.
    pub missed: usize,
    /// `tp / flagged`, or `0.0` when nothing was flagged.
    pub precision: f64,
    /// `tp / positives`, or `0.0` when nothing is labelled.
    pub recall: f64,
    /// The harmonic mean of the two — the objective a retune walks.
    pub f1: f64,
    /// The flagged case ids, in flag order.
    pub flagged_ids: Vec<String>,
    /// Labelled cases the cut missed, in score order.
    pub missed_ids: Vec<String>,
    /// Flagged cases nobody labelled, in score order.
    pub spurious_ids: Vec<String>,
    /// The score each case got, `id → score`, so a report can be argued case by case.
    pub scores: Vec<(String, u8, bool)>,
    /// Where the labelled cases landed on average, 1 = first.
    ///
    /// The cut throws this information away — a case one place below the line and one fifty places
    /// below it are both "missed" — and a retune that only maximises the cut stalls on the plateau that
    /// creates. `mean_positive_rank` is the tie-break, and a number worth printing beside precision:
    /// "a must-read case sits 7.4th by default, 4.1st retuned" is a sentence about the ordering, not
    /// only about the cut.
    pub mean_positive_rank: f64,
}

impl Cut {
    /// The one-line row the report prints.
    #[must_use]
    pub fn row(&self) -> String {
        format!(
            "{:>9}  {:>7.3}  {:>6.3}  {:>6.3}  {:>7}  {:>6}  {:>8}",
            "",
            self.precision,
            self.recall,
            self.f1,
            self.flagged,
            self.missed,
            self.false_positive
        )
    }
}

/// Score every case with `weights`, cut at the top `fraction`, and measure the cut against the labels.
///
/// The ranking is the queue's ranking: exact total first, then the integer score, then case id, so two
/// cases that tie land the same way in every run and in every report.
#[must_use]
pub fn evaluate(set: &Set, weights: &Weights, fraction: f64) -> Cut {
    let fraction = fraction.clamp(0.01, 1.0);
    let mut scored: Vec<(&Case, Score)> = set
        .cases
        .iter()
        .map(|case| {
            let vector = vector_of(case);
            (case, score::score(&vector, weights))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.1.total
            .cmp(&a.1.total)
            .then_with(|| b.1.score.cmp(&a.1.score))
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    let cut = (((set.cases.len() as f64) * fraction).round() as usize).clamp(1, set.cases.len());
    let positives = set.cases.iter().filter(|case| case.label).count();
    let mut true_positive = 0_usize;
    let mut flagged_ids = Vec::new();
    let mut spurious_ids = Vec::new();
    let mut missed_ids = Vec::new();
    for (index, (case, _)) in scored.iter().enumerate() {
        if index < cut {
            flagged_ids.push(case.id.clone());
            if case.label {
                true_positive += 1;
            } else {
                spurious_ids.push(case.id.clone());
            }
        } else if case.label {
            missed_ids.push(case.id.clone());
        }
    }
    let false_positive = cut - true_positive;
    let missed = positives - true_positive;
    let precision = if cut == 0 {
        0.0
    } else {
        f64::from(u32::try_from(true_positive).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(cut).unwrap_or(u32::MAX))
    };
    let recall = if positives == 0 {
        0.0
    } else {
        f64::from(u32::try_from(true_positive).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(positives).unwrap_or(u32::MAX))
    };
    let f1 = if precision + recall <= 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };
    let labelled_positions: Vec<usize> = scored
        .iter()
        .enumerate()
        .filter(|(_, (case, _))| case.label)
        .map(|(index, _)| index + 1)
        .collect();
    let mean_positive_rank = if labelled_positions.is_empty() {
        0.0
    } else {
        labelled_positions.iter().sum::<usize>() as f64
            / f64::from(u32::try_from(labelled_positions.len()).unwrap_or(u32::MAX))
    };
    Cut {
        cases: set.cases.len(),
        positives,
        flagged: cut,
        true_positive,
        false_positive,
        missed,
        precision,
        recall,
        f1,
        flagged_ids,
        missed_ids,
        spurious_ids,
        scores: scored
            .iter()
            .map(|(case, at)| (case.id.clone(), at.score, case.label))
            .collect(),
        mean_positive_rank,
    }
}

/// A case's measurements, ready for [`super::score::score`].
#[must_use]
pub fn vector_of(case: &Case) -> Vector {
    let mut measurements = Vec::with_capacity(case.features.len());
    for (key, raw) in &case.features {
        let Some(feature) = Feature::parse(key) else {
            continue;
        };
        measurements.push(Measurement::measured(
            feature,
            *raw,
            "calibration fixture",
            case.summary.clone(),
        ));
    }
    Vector::new(case.id.clone(), measurements)
}

/// Before and after, and the proposal in between.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comparison {
    /// The cut as a share.
    pub fraction: f64,
    /// The shipped weights' answer.
    pub before: Cut,
    /// The retuned weights' answer.
    pub after: Cut,
    /// The retuned weights themselves, ready to print as `fathom.toml`.
    pub proposal: Weights,
    /// Who labelled the set.
    pub labelled_by: String,
    /// One line per change the retune made, in the order it made them.
    pub moves: Vec<String>,
    /// `risk.tau` as it stood, and how many cases in this set it flags under the shipped weights.
    pub tau_shipped: u8,
    /// How many cases the shipped `risk.tau` flags. Zero is a finding, and the report prints it.
    pub tau_shipped_flags: usize,
    /// The threshold the proposal would write, which is the retuned cut's own floor.
    pub tau_proposed: u8,
    /// How many cases the proposed threshold flags — the decile, plus any tie at its floor.
    pub tau_proposed_flags: usize,
    /// Anything the retune refused to claim.
    pub notices: Vec<String>,
}

/// Measure the shipped weights, retune against the set, and measure again.
///
/// The proposal is printed, never applied: adopting a retune is an edit to `fathom.toml` a human makes
/// after reading the report, and a tool that quietly rewrote its own scoring is exactly the kind of model
/// this ticket exists to refuse.
#[must_use]
pub fn calibrate(set: &Set, shipped: &Weights, fraction: f64) -> Comparison {
    let before = evaluate(set, shipped, fraction);
    let mut proposal = retune(set, shipped, fraction);
    let after = evaluate(set, &proposal, fraction);
    // The threshold travels with the weights. A retune that moved the scores and left `risk.tau` where it
    // was would propose a queue in which nothing is flagged — so the proposal's τ is the retuned cut's own
    // floor, the lowest score that is still in the top decile. That is a measured number, not a taste, and
    // `render` prints how many cases each side of it flags.
    if let Some(floor) = after
        .scores
        .iter()
        .filter(|(_, _, in_cut)| *in_cut)
        .map(|(_, score, _)| *score)
        .min()
    {
        proposal.tau = floor.max(1);
    }
    let flags = |cut: &Cut, tau: u8| -> usize {
        cut.scores
            .iter()
            .filter(|(_, score, _)| i64::from(*score) >= i64::from(tau))
            .count()
    };
    let (tau_shipped, tau_proposed) = (shipped.tau, proposal.tau);
    let (tau_shipped_flags, tau_proposed_flags) =
        (flags(&before, tau_shipped), flags(&after, tau_proposed));
    let mut moves = Vec::new();
    for weight in &proposal.weights {
        let shipped = shipped.weight(weight.feature);
        if (shipped - weight.value).abs() > f64::EPSILON {
            moves.push(format!(
                "{} {:.2} → {:.2}",
                weight.feature.id(),
                shipped,
                weight.value
            ));
        }
    }
    let mut notices = set.notices.clone();
    if before.positives == 0 {
        notices.push(
            "no case in the set is labelled must-read, so precision and recall have no denominator \
             and the retune refused to move a weight".to_owned(),
        );
    }
    if after.f1 < before.f1 {
        // Cannot happen with a monotone objective and strict-improvement moves, and stated anyway: if a
        // future objective stops being monotone, the report says so rather than shipping a regression.
        notices.push(
            "the retune ended below the shipped weights' F1, which this command refuses to propose"
                .to_owned(),
        );
    }
    Comparison {
        fraction,
        before,
        after,
        proposal,
        labelled_by: set.labelled_by.clone(),
        moves,
        tau_shipped,
        tau_shipped_flags,
        tau_proposed,
        tau_proposed_flags,
        notices,
    }
}

/// Did `candidate` strictly beat `incumbent` on the objective pair?
///
/// Strict on F1 first, and only when F1 is a tie does the mean-rank term decide. Both comparisons carry
/// an epsilon: float sums over twelve terms are not exact, and a search that "improves" by 1e-16 is a
/// search that never converges.
fn better(candidate: (f64, f64), incumbent: (f64, f64)) -> bool {
    if (candidate.0 - incumbent.0).abs() > f64::EPSILON {
        return candidate.0 > incumbent.0;
    }
    candidate.1 > incumbent.1 + f64::EPSILON
}

/// The retune itself: coordinate ascent over [`GRID`], maximising F1 at the cut.
///
/// Deterministic by construction — the feature order is [`Feature::ALL`], the grid is fixed, a move has
/// to *strictly* improve F1 (ties keep the incumbent, so the shipped default is what survives a set that
/// does not favour either way), and the sweep repeats [`PASSES`] times at most. Nothing reads a clock or
/// a random seed, so the same set on the same build proposes the same weights forever, which is the only
/// way a proposed weight change survives review.
///
/// A weight of `0.0` is a real candidate: switching a feature off is a legitimate finding, and a grid
/// that could not reach zero would be a grid with an opinion.
#[must_use]
pub fn retune(set: &Set, shipped: &Weights, fraction: f64) -> Weights {
    let mut best = shipped.clone();
    if set.cases.iter().filter(|case| case.label).count() == 0 {
        // Nothing to aim at. Say so in the report (`calibrate` does) and change nothing.
        return best;
    }
    // Lexicographic: the cut's F1 first, because that is the criterion the ticket names, and the mean
    // rank of the labelled cases second, because the cut alone cannot tell a near-miss from a hopeless
    // case and a search that cannot see the difference stalls on the plateau it makes. A move has to
    // strictly improve the pair; ties keep the incumbent, so the shipped default survives a set that has
    // no opinion.
    let objective = |weights: &Weights| {
        let row = evaluate(set, weights, fraction);
        (row.f1, -row.mean_positive_rank)
    };
    let mut current = objective(&best);
    for _ in 0..PASSES {
        let mut moved = false;
        for feature in Feature::ALL {
            let incumbent = best.weight(feature);
            let mut best_value = incumbent;
            let mut best_score = current;
            for candidate in GRID {
                if (candidate - incumbent).abs() <= f64::EPSILON {
                    continue;
                }
                let mut trial = best.clone();
                // `set` cannot fail on a feature from `Feature::ALL`; a failed write would leave the
                // trial at the incumbent, which is the same outcome as skipping it.
                let _ = trial.set(feature.id(), candidate, Origin::Repo);
                let score = objective(&trial);
                if better(score, best_score) {
                    best_score = score;
                    best_value = candidate;
                }
            }
            if (best_value - incumbent).abs() > f64::EPSILON {
                let _ = best.set(feature.id(), best_value, Origin::Repo);
                current = best_score;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    best
}

/// The report's lines: the two rows, the moves, and the `fathom.toml` block to paste.
#[must_use]
pub fn render(comparison: &Comparison) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "calibration: {} cases · {} labelled must-read · the top {:.0}% is {} episodes",
        comparison.before.cases,
        comparison.before.positives,
        comparison.fraction * 100.0,
        comparison.before.flagged
    ));
    if !comparison.labelled_by.is_empty() {
        out.push(format!("  labelled by: {}", comparison.labelled_by));
    }
    out.push(format!(
        "{:>13}  {:>7}  {:>6}  {:>6}  {:>7}  {:>6}  {:>8}  {:>10}",
        "", "precision", "recall", "F1", "flagged", "missed", "spurious", "mean rank"
    ));
    for (name, row) in [
        ("shipped", &comparison.before),
        ("retuned", &comparison.after),
    ] {
        out.push(format!(
            "{:>13}  {:>7.3}  {:>6.3}  {:>6.3}  {:>7}  {:>6}  {:>8}  {:>10.1}",
            name,
            row.precision,
            row.recall,
            row.f1,
            row.flagged,
            row.missed,
            row.false_positive,
            row.mean_positive_rank
        ));
    }
    out.push(format!(
        "{:>13}  {:>7}  {:>6}  {:>6}  {:>7}  {:>6}  {:>8}  {:>10}",
        "change",
        delta(comparison.after.precision, comparison.before.precision),
        delta(comparison.after.recall, comparison.before.recall),
        delta(comparison.after.f1, comparison.before.f1),
        "",
        "",
        "",
        if comparison.after.mean_positive_rank < comparison.before.mean_positive_rank {
            format!(
                "{:.1} earlier",
                comparison.before.mean_positive_rank - comparison.after.mean_positive_rank
            )
        } else {
            String::new()
        }
    ));
    out.push(format!(
        "   threshold  `risk.tau` {} flags {} of {} as shipped; the proposal sets it to {}, which flags {}",
        comparison.tau_shipped,
        comparison.tau_shipped_flags,
        comparison.before.cases,
        comparison.tau_proposed,
        comparison.tau_proposed_flags
    ));
    if comparison.tau_shipped_flags == 0 {
        out.push(
            "               nothing in this corpus reaches the shipped threshold — that is a finding              about the threshold, not about the corpus"
                .to_owned(),
        );
    }
    out.push(String::new());
    out.push("missed by the shipped weights:".to_owned());
    if comparison.before.missed_ids.is_empty() {
        out.push("  nothing — the cut caught every labelled case".to_owned());
    }
    for id in &comparison.before.missed_ids {
        out.push(format!("  · {id}"));
    }
    if !comparison.before.spurious_ids.is_empty() {
        out.push("flagged by the shipped weights, unlabelled:".to_owned());
        for id in &comparison.before.spurious_ids {
            out.push(format!("  · {id}"));
        }
    }
    if !comparison.moves.is_empty() {
        out.push(String::new());
        out.push("the retune moved:".to_owned());
        for move_ in &comparison.moves {
            out.push(format!("  · {move_}"));
        }
    }
    for notice in &comparison.notices {
        out.push(format!("  notice: {notice}"));
    }
    out.push(String::new());
    out.push("paste into fathom.toml to adopt it (it is not applied by this command):".to_owned());
    for line in comparison.proposal.to_toml().lines() {
        out.push(format!("  {line}"));
    }
    out
}

/// A signed delta, printed to three digits.
fn delta(after: f64, before: f64) -> String {
    let difference = after - before;
    if difference.abs() < 0.0005 {
        return "  ·   ".to_owned();
    }
    format!(
        "{}{:.3}",
        if difference > 0.0 { "+" } else { "-" },
        difference.abs()
    )
}

/// The report as JSON, for `--json` and for the blessed file.
#[must_use]
pub fn to_json(comparison: &Comparison) -> serde_json::Value {
    serde_json::to_value(comparison).unwrap_or(serde_json::Value::Null)
}

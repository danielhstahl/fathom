//! The arithmetic: six measurements and six weights in, one explainable number out per module
//! (`e3.8`, `reco-wky`).
//!
//! ```text
//! value_i    = raw_i / (raw_i + knee_i)                    the published saturating curve
//! W          = Σ w_i        over the metrics that were measured
//! points_i   = round(10000 · w_i · value_i / W)           w_i in 0.0..=1.0
//! total      = Σ points_i                                 an exact integer sum of the printed parts
//! score      = clamp(round(total / 100), 0, 100)
//! ```
//!
//! That is the whole model, and it is deliberately the same model [`crate::risk::score`] runs, down to
//! the hundredths arithmetic: two scores in one product that both claim to be explainable must not be
//! explainable in two different ways. The four properties that claim buys are the same four, and
//! `crates/fathom-analyze/tests/hotspots.rs` holds this file to all of them:
//!
//! * **The printed parts sum to the printed total.** Terms are integer hundredths, so
//!   `18.75 + 16.00 + …` is exact in base 10 and a reviewer with a calculator arrives at the same
//!   number. Float accumulation does not, which is why the arithmetic is in hundredths.
//! * **Reproducible.** Same facts, same [`Policy`], same table. No clock, no locale, no `HashMap` on
//!   the path from measurement to printed line.
//! * **Monotone.** Raising a metric never lowers a module's place, and a module at or above another on
//!   every measured metric stays at or above it under *any* non-negative weights. That is the property
//!   that makes retuning the weights a deliberate act instead of a lottery.
//! * **Divided, not renormalised behind you.** `W` is over the measured metrics, so a missing metric
//!   redistributes its share instead of quietly lowering the score — and the row prints `4 of 6
//!   metrics measured; denominator 0.75`, because a denominator the reader cannot see is a second
//!   hidden layer.
//!
//! # Ranking is per-revision and total, with the truncation printed
//!
//! Every module in the graph gets a row; `hotspots.top` truncates the *print*, and the number dropped
//! travels with the table as [`Table::rows_dropped`]. A "top 10" that silently dropped 3 000 modules
//! would be a filter nobody chose.

use super::model::{
    Hotspot, Measurement, Metric, ModuleFacts, Origin, Quadrant, Shape, Table, Weight,
};
use super::policy::Policy;
use crate::risk::score::Points;

/// The shape tag a JSON reader checks first.
pub const CONTRACT: &str = "fathom.hotspots/1";

/// Rank every module in `facts` at `revision`.
///
/// `notices` is what the caller's read could not do — a partial chain, a grammar not compiled in, a
/// file the extractor refused — and it travels into the table untouched, because a ranking built on a
/// hole has to name the hole.
///
/// The empty state is chosen here and not at the call site, so that no surface can render a table of
/// zeroes: with no modules, or with a policy whose weights are all zero, [`Table::ranked`] is false,
/// [`Table::empty_reason`] says why, and [`Table::rows`] is empty.
#[must_use]
pub fn rank(revision: &str, facts: &[ModuleFacts], policy: &Policy, notices: &[String]) -> Table {
    let weights = policy.weights.clone();
    let knees = policy.knees.clone();
    // The caller's notices first — they describe the read — then the knobs' own disagreements, each
    // stated once.
    let mut notes: Vec<String> = notices.to_vec();
    for line in policy.coherence() {
        if !notes.contains(&line) {
            notes.push(line);
        }
    }
    let notes = notes;

    if facts.is_empty() {
        return Table {
            contract: CONTRACT,
            revision: revision.to_string(),
            window: policy.window,
            ranked: false,
            empty_reason: Some(
                "the module graph at this revision has no modules, so there is nothing to rank. A \
                 cold index or an empty tree is an empty table, not a ranking of zeroes."
                    .to_string(),
            ),
            rows: Vec::new(),
            modules_seen: 0,
            modules_scored: 0,
            rows_dropped: 0,
            notices: notes,
            weights,
            knees,
        };
    }

    let mut rows: Vec<Hotspot> = facts.iter().map(|facts| hotspot(facts, policy)).collect();
    let scored = rows.iter().filter(|row| row.scored).count();
    if scored == 0 {
        return Table {
            contract: CONTRACT,
            revision: revision.to_string(),
            window: policy.window,
            ranked: false,
            empty_reason: Some(
                "no module could be scored: every metric is either unmeasured at this revision or \
                 weighted 0 by `hotspots.weights`. The refusal is printed instead of a ranking of \
                 zeroes."
                    .to_string(),
            ),
            rows: Vec::new(),
            modules_seen: facts.len(),
            modules_scored: 0,
            rows_dropped: 0,
            notices: notes,
            weights,
            knees,
        };
    }

    // Score first, then place: the two are one sort with a stated tie-break, so two runs over the same
    // facts cannot hand out the same rank twice or order a tie by hash order.
    rows.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.total.cmp(&a.total))
            .then_with(|| a.module.cmp(&b.module))
    });
    for (place, row) in rows.iter_mut().enumerate() {
        row.rank = place + 1;
    }
    let total = rows.len();
    let kept: Vec<Hotspot> = rows.into_iter().take(policy.top.max(1)).collect();
    Table {
        contract: CONTRACT,
        revision: revision.to_string(),
        window: policy.window,
        ranked: true,
        empty_reason: None,
        rows_dropped: total.saturating_sub(kept.len()),
        rows: kept,
        modules_seen: facts.len(),
        modules_scored: scored,
        notices: notes,
        weights,
        knees,
    }
}

/// One module's row.
#[must_use]
pub fn hotspot(facts: &ModuleFacts, policy: &Policy) -> Hotspot {
    let mut items: Vec<Measurement> = Metric::ALL
        .iter()
        .map(|metric| measure(*metric, facts, policy))
        .collect();

    let denominator: f64 = items
        .iter()
        .filter(|item| item.measured)
        .map(|item| item.weight)
        .fold(0.0_f64, |sum, weight| sum + weight);
    // Printed, so stored rounded: the value the arithmetic divides by is the value on the page.
    let denominator = (denominator * 10_000.0).round() / 10_000.0;

    let mut hundredths: i64 = 0;
    let measured = items.iter().filter(|item| item.measured).count();
    if denominator > 0.0 {
        for item in &mut items {
            if !item.measured {
                continue;
            }
            item.points = Points(
                (10_000.0 * item.weight * item.value / denominator)
                    .round()
                    .max(0.0) as i64,
            );
            item.cap = Points((10_000.0 * item.weight / denominator).round().max(0.0) as i64);
            hundredths = hundredths.saturating_add(item.points.0);
        }
    }
    let scored = denominator > 0.0 && measured > 0;
    let unscored = if scored {
        None
    } else if measured == 0 {
        Some("nothing about this module could be measured".to_string())
    } else {
        Some("every metric measured here is weighted 0".to_string())
    };
    hundredths = hundredths.min(10_000);
    let score = u8::try_from(((hundredths + 50) / 100).clamp(0, 100)).unwrap_or(100);

    items.sort_by(|a, b| {
        b.points
            .cmp(&a.points)
            .then_with(|| index(b.metric).cmp(&index(a.metric)))
    });

    let values = |metric: Metric| -> f64 {
        items
            .iter()
            .find(|item| item.metric == metric)
            .filter(|item| item.measured)
            .map_or(0.0, |item| item.value)
    };
    let (quadrant, shape, shape_why) = describe(policy, &values);
    let caveats = caveats_for(facts, &items, policy);

    Hotspot {
        module: facts.module.clone(),
        layer: facts.layer.clone(),
        rank: 0,
        score,
        scored,
        unscored,
        total: Points(hundredths),
        denominator,
        measured,
        contributions: items,
        quadrant,
        shape,
        shape_why,
        caveats,
    }
}

/// A metric's position in the printed order, for a deterministic tie-break.
fn index(metric: Metric) -> usize {
    Metric::ALL
        .iter()
        .position(|candidate| *candidate == metric)
        .unwrap_or(Metric::ALL.len())
}

/// Measure one metric, or say precisely why it could not be measured.
fn measure(metric: Metric, facts: &ModuleFacts, policy: &Policy) -> Measurement {
    let weight = policy.weight(metric);
    let origin: Origin = policy.origin(metric);
    let knee = policy.knee(metric);
    let basis = metric.spec().basis.to_string();
    match metric {
        Metric::FanIn => {
            let raw = facts.fan_in_total() as f64;
            let mut reason = format!(
                "{} module(s) depend on it",
                group(facts.fan_in_total())
            );
            if facts.fan_in_wildcard > 0 {
                reason.push_str(&format!(
                    ", {} of them through a wildcard whose contents are unknown",
                    facts.fan_in_wildcard
                ));
            }
            if facts.fan_in_package > 0 {
                reason.push_str(&format!(
                    ", {} of them naming a package rather than a file",
                    facts.fan_in_package
                ));
            }
            Measurement::measured(metric, raw, knee, weight, origin, basis, reason)
        }
        Metric::FanOut => {
            let raw = facts.fan_out_total() as f64;
            let mut reason = format!(
                "it depends on {} module(s)",
                group(facts.fan_out_total())
            );
            if facts.fan_out_wildcard > 0 {
                reason.push_str(&format!(
                    ", {} of them a wildcard whose contents are unknown",
                    facts.fan_out_wildcard
                ));
            }
            Measurement::measured(metric, raw, knee, weight, origin, basis, reason)
        }
        Metric::Instability => match facts.instability() {
            Some(ratio) => Measurement::measured(
                metric,
                ratio,
                None,
                weight,
                origin,
                basis,
                format!(
                    "{} of its {} coupling points outward",
                    group(facts.fan_out_total()),
                    group(facts.coupling())
                ),
            ),
            None => Measurement::unmeasured(
                metric,
                weight,
                origin,
                "no dependency edges in either direction at this revision, so instability is \
                 undefined rather than zero — printing 0.00 here would claim a module is stable when \
                 nothing is known about it"
                    .to_string(),
            ),
        },
        Metric::Size => match facts.code_lines {
            Some(lines) => Measurement::measured(
                metric,
                lines as f64,
                knee,
                weight,
                origin,
                basis,
                format!("{} lines of code, comments and blanks excluded", group(lines)),
            ),
            None => Measurement::unmeasured(
                metric,
                weight,
                origin,
                "no structural record for this path at this revision: it was never read, its grammar \
                 is not compiled into this build, or it is a dependency the graph names but has no file \
                 for. Its weight is redistributed to the metrics that were measured, not treated as a \
                 zero"
                    .to_string(),
            ),
        },
        Metric::Complexity => match facts.complexity {
            Some(facts) => Measurement::measured(
                metric,
                facts.points as f64,
                knee,
                weight,
                origin,
                basis,
                format!(
                    "{} points over {} declaration(s); longest {}, deepest nesting {}",
                    group(facts.points),
                    group(facts.declarations),
                    group(facts.max_body_lines),
                    facts.max_nesting
                ),
            ),
            None => Measurement::unmeasured(
                metric,
                weight,
                origin,
                "no structural record for this path, so there are no spans to count. A module of one \
                 generated 40 000-line array and a module nobody read look identical here, which is \
                 why this is stated and not guessed"
                    .to_string(),
            ),
        },
        Metric::Churn => match facts.churn {
            Some(churn) if churn.window >= policy.min_history => Measurement::measured(
                metric,
                churn.per_revision(),
                knee,
                weight,
                origin,
                basis,
                format!(
                    "{} edge event(s) across {} of the last {} revision(s)",
                    group(churn.events),
                    churn.revisions,
                    churn.window
                ),
            ),
            Some(churn) => Measurement::unmeasured(
                metric,
                weight,
                origin,
                format!(
                    "the store holds {} revision(s) of history and `hotspots.min_history` is {}; \
                     velocity needs a window, so this is unmeasured and not zero",
                    churn.window, policy.min_history
                ),
            ),
            None => Measurement::unmeasured(
                metric,
                weight,
                origin,
                format!(
                    "no churn window: this revision has no earlier revision to be compared against \
                     (`hotspots.window` = {}, `hotspots.min_history` = {})",
                    policy.window, policy.min_history
                ),
            ),
        },
    }
}

/// The quadrant and the shape, decided from the saturated values and one published bar.
fn describe(policy: &Policy, values: &impl Fn(Metric) -> f64) -> (Quadrant, Shape, String) {
    let (hot, complex) = (
        policy.high(values(Metric::Churn)),
        policy.high(values(Metric::Complexity)),
    );
    let quadrant = match (hot, complex) {
        (true, true) => Quadrant::HotComplex,
        (true, false) => Quadrant::HotPlain,
        (false, true) => Quadrant::ColdComplex,
        (false, false) => Quadrant::Quiet,
    };
    let bar = format!("{:.2}", policy.god_bar);
    let shape = if policy.high(values(Metric::FanIn))
        && policy.high(values(Metric::Size))
        && policy.high(values(Metric::Complexity))
        && policy.high(values(Metric::Churn))
    {
        (
            Shape::God,
            format!(
                "a god module: fan-in, size, complexity and churn all clear the {bar} bar, so it is \
                 big, hard to hold, widely depended on, and still moving"
            ),
        )
    } else if policy.high(values(Metric::FanIn))
        && !policy.high(values(Metric::Complexity))
        && !policy.high(values(Metric::Size))
    {
        (
            Shape::Hub,
            format!(
                "a hub: fan-in clears {bar} while size and complexity do not, so many modules lean on \
                 a small one — cheap to read, expensive to change"
            ),
        )
    } else if policy.high(values(Metric::FanOut)) && values(Metric::FanIn) <= f64::EPSILON {
        (
            Shape::Sink,
            format!(
                "a dependency sink: fan-out clears {bar} with nothing depending on it, so it can be \
                 rewritten freely and is the first place a cycle starts"
            ),
        )
    } else if policy.high(values(Metric::Size)) && policy.high(values(Metric::Complexity)) {
        (
            Shape::Giant,
            format!(
                "big and complex with callers below the {bar} bar: real debt, and quiet, which is why \
                 it waits for somebody to be sent in"
            ),
        )
    } else {
        (Shape::None, String::new())
    };
    (quadrant, shape.0, shape.1)
}

/// What the row cannot see, derived from the facts and never from a template.
fn caveats_for(facts: &ModuleFacts, items: &[Measurement], policy: &Policy) -> Vec<String> {
    let mut out = Vec::new();
    if facts.fan_in_wildcard + facts.fan_out_wildcard > 0 {
        out.push(format!(
            "{} inbound and {} outbound dependency edge(s) are wildcards — a star import, a barrel \
             `export *`, a Go package import. They are counted as dependencies, because they are; what \
             is unknown is their contents, so a caller list for this module is not claimed complete",
            facts.fan_in_wildcard, facts.fan_out_wildcard
        ));
    }
    if facts.fan_in_package + facts.fan_out_package > 0 {
        out.push(format!(
            "{} edge(s) are package-granular: the language's dependency is the package, drawn once per \
             file because that is the granularity the store keys on",
            facts.fan_in_package + facts.fan_out_package
        ));
    }
    if facts.incomplete {
        out.push(
            "this module's declaration list may have shrunk rather than shrunk — a macro, a decorator \
             or a generated scope the extractor cannot enumerate — so its own fan-out and its \
             complexity are lower bounds"
                .to_string(),
        );
    }
    for item in items.iter().filter(|item| !item.measured) {
        out.push(format!(
            "{} was not measured: {}",
            item.metric.id(),
            item.unmeasured.clone().unwrap_or_default()
        ));
    }
    if facts.fan_in_wildcard + facts.fan_in_package > 0
        && policy.high(items_value(items, Metric::FanIn))
    {
        out.push(
            "the fan-in half of this row's score rests partly on edges whose contents nobody read — the \
             ranking is honest about it, a caller list built from it would not be"
                .to_string(),
        );
    }
    out
}

/// A measured metric's value, `0.0` when it was not measured.
fn items_value(items: &[Measurement], metric: Metric) -> f64 {
    items
        .iter()
        .find(|item| item.metric == metric)
        .filter(|item| item.measured)
        .map_or(0.0, |item| item.value)
}

/// Digit grouping, shared with [`super::model`] so a reason and a column agree.
fn group(value: usize) -> String {
    Metric::Size.format_raw(value as f64)
}

/// The weights and knees a table was built with, for the JSON envelope.
#[must_use]
pub fn provenance(policy: &Policy) -> (Vec<Weight>, Vec<Weight>) {
    (policy.weights.clone(), policy.knees.clone())
}

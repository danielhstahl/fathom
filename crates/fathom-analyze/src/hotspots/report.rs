//! Saying it in prose, in one place, so three surfaces cannot say it three ways (`e3.8`, `reco-wky`).
//!
//! ADR-006 §5.1's binding note is the reason this file exists: the sentence "0.63 = 20 / (20 + 12) ·
//! fan-in" has one wording, and the shell, `fathom hotspots`, the MCP resource and the exported
//! artefact all print that one. A renderer that lived in the CLI would be duplicated the first time a
//! second surface wanted the table.
//!
//! Two renderings, one per answer: [`render`] for the ranking and [`render_dead`] for the candidates.
//! Both are line-oriented (`Vec<String>`) for the same reason every renderer in this crate is: a shell
//! wraps, a JSON consumer does not, and neither should have to re-derive the sentence.

use crate::risk::score::Points;

use super::deadcode::{Candidate, CandidateKind, Confidence, Report as DeadReport};
use super::model::{Hotspot, Measurement, Table};
use super::policy::Policy;

/// Render the ranking.
///
/// The order is the order a reviewer reads: what was measured and over what window, the weights that
/// decided it, the table, then the per-row arithmetic in full for the rows that carry a shape, then the
/// caveats, then what the read could not do.
#[must_use]
pub fn render(table: &Table, policy: &Policy) -> Vec<String> {
    let mut out = Vec::new();
    if !table.ranked {
        out.push(format!(
            "hotspots-1 · {} · no ranking",
            short(&table.revision)
        ));
        out.push(format!(
            "  {}",
            table
                .empty_reason
                .clone()
                .unwrap_or_else(|| "nothing was measured".to_string())
        ));
        out.push(format!(
            "  window {} revision(s) · {} module(s) in the graph",
            table.window, table.modules_seen
        ));
        push_notices(&mut out, &table.notices);
        return out;
    }
    out.push(format!(
        "hotspots-1 · {} · ranked over {} module(s), window {} revision(s)",
        short(&table.revision),
        table.modules_seen,
        table.window
    ));
    out.push(format!(
        "  weights: {}",
        policy
            .weights
            .iter()
            .map(|weight| format!(
                "{} {:.2} ({})",
                weight.metric.id(),
                weight.value,
                weight.origin.as_str()
            ))
            .collect::<Vec<_>>()
            .join(" · ")
    ));
    out.push(format!(
        "  knees: {}",
        policy
            .knees
            .iter()
            .filter(|knee| knee.metric.spec().default_knee.is_some())
            .map(|knee| format!("{} {}", knee.metric.id(), knee.value))
            .collect::<Vec<_>>()
            .join(" · ")
    ));
    out.push(String::new());
    out.push(format!(
        "{:>3}  {:>5}  {:<40}  {:<13}  {:<6}  {}",
        "#", "score", "module", "quadrant", "shape", "layer"
    ));
    for row in &table.rows {
        out.push(format!(
            "{:>3}  {:>5}  {:<40}  {:<13}  {:<6}  {}",
            row.rank,
            if row.scored {
                row.score.to_string()
            } else {
                "—".to_string()
            },
            fit(&row.module, 40),
            row.quadrant.as_str(),
            row.shape.as_str(),
            if row.layer.is_empty() {
                "—"
            } else {
                &row.layer
            }
        ));
    }
    if table.rows_dropped > 0 {
        out.push(format!(
            "  … and {} more module(s) scored but not listed (`hotspots.top` = {})",
            table.rows_dropped, policy.top
        ));
    }
    if !table.shaped().is_empty() {
        out.push(String::new());
        out.push(format!(
            "shape calls ({} of {} rows cleared a bar; the bar is `god_bar` = {:.2}):",
            table.shaped().len(),
            table.rows.len(),
            policy.god_bar
        ));
        for row in table.shaped() {
            out.push(format!("  · {} — {}", row.module, row.shape_why));
        }
    }
    for row in &table.rows {
        if row.shape == super::model::Shape::None && row.rank > 5 {
            continue;
        }
        out.push(String::new());
        out.extend(render_row(row));
    }
    push_notices(&mut out, &table.notices);
    out
}

/// One row's full arithmetic: every metric, whether it was measured, and what it contributed.
#[must_use]
pub fn render_row(row: &Hotspot) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "{} — {}/100 · {} of 6 metrics measured · denominator {:.2}",
        row.module, row.score, row.measured, row.denominator
    ));
    out.push(format!("  {}", row.vector()));
    out.push(format!(
        "  {:<12} {:>16} {:>6} {:>7} {:>13}  {}",
        "metric", "measured", "value", "weight", "points", "why"
    ));
    for item in &row.contributions {
        out.push(format!(
            "  {:<12} {:>16} {:>6.2} {:>7.2} {:>13}  {}",
            item.metric.id(),
            item.raw_shown,
            item.value,
            item.weight,
            format!("{}/{}", item.points, item.cap),
            item.reason
        ));
    }
    out.push(format!(
        "  quadrant: {} — {}",
        row.quadrant.as_str(),
        row.quadrant.sentence()
    ));
    if !row.shape_why.is_empty() {
        out.push(format!(
            "  shape: {} — {}",
            row.shape.as_str(),
            row.shape_why
        ));
    }
    for caveat in &row.caveats {
        out.push(format!("  caveat: {caveat}"));
    }
    out
}

/// Render the dead-code candidates.
#[must_use]
pub fn render_dead(report: &DeadReport) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "dead code · {} · {} candidate(s): {} firm, {} uncertain, {} unverified",
        short(&report.revision),
        report.candidates.len(),
        report.firm,
        report.uncertain,
        report.unverified
    ));
    if report.candidates.is_empty() {
        out.push(format!(
            "  {}",
            report
                .empty_reason
                .clone()
                .unwrap_or_else(|| "nothing was found".to_string())
        ));
    }
    out.push(format!(
        "  {} new symbol(s) examined · {} gated group(s) not reported (declared on, or the build's to \
         set) · {} path(s) excluded by `hotspots.ignore`",
        report.symbols_checked, report.gated_not_reported, report.ignored_paths
    ));
    if !report.ignored_patterns.is_empty() {
        out.push(format!(
            "  excluded by: {}",
            report.ignored_patterns.join(", ")
        ));
    }
    for kind in [
        CandidateKind::Unreferenced,
        CandidateKind::Gated,
        CandidateKind::Duplicated,
    ] {
        let rows = report.of_kind(kind);
        if rows.is_empty() {
            continue;
        }
        out.push(String::new());
        out.push(format!("{} ({}):", kind.heading(), rows.len()));
        for candidate in rows {
            out.extend(render_candidate(candidate));
        }
    }
    push_notices(&mut out, &report.notices);
    out.push(String::new());
    out.push(format!(
        "what no static read of this repository can see ({} things):",
        report.limits.len()
    ));
    for limit in &report.limits {
        out.push(format!("  · {limit}"));
    }
    out
}

/// One candidate: the claim, the age, and every reason it might be wrong.
#[must_use]
pub fn render_candidate(candidate: &Candidate) -> Vec<String> {
    let mut out = vec![format!(
        "  [{}] {}",
        candidate.verdict.as_str(),
        candidate.subject
    )];
    if let Some(since) = &candidate.since {
        out.push(format!("    age: {since}"));
    }
    out.push(format!("    why: {}", candidate.why));
    for uncertainty in &candidate.uncertainty {
        out.push(format!("    could be wrong because: {uncertainty}"));
    }
    if !candidate.evidence.is_empty() {
        out.push(format!("    evidence: {}", candidate.evidence.join(", ")));
    }
    if let Some(decided_by) = &candidate.decided_by {
        out.push(format!("    decided by: {decided_by}"));
    }
    out
}

/// The JSON envelope the surfaces read: the model's own shape plus the knobs that produced it, so a
/// stored payload can be read a year later without guessing which weights were in force.
#[must_use]
pub fn to_json(table: &Table, policy: &Policy) -> serde_json::Value {
    let mut value = serde_json::to_value(table).unwrap_or(serde_json::Value::Null);
    if let Some(map) = value.as_object_mut() {
        map.insert(
            "policy".to_string(),
            serde_json::to_value(super::policy::Provenance::of(policy))
                .unwrap_or(serde_json::Value::Null),
        );
    }
    value
}

/// The dead-code answer as JSON.
#[must_use]
pub fn dead_to_json(report: &DeadReport) -> serde_json::Value {
    serde_json::to_value(report).unwrap_or(serde_json::Value::Null)
}

/// The one-line summary a pane that has 40 columns prints.
#[must_use]
pub fn summary(table: &Table) -> String {
    if !table.ranked {
        return format!(
            "no hotspot ranking: {}",
            table
                .empty_reason
                .clone()
                .unwrap_or_else(|| "nothing was measured".to_string())
        );
    }
    match table.first() {
        Some(top) => format!(
            "{} module(s) ranked; first is {} at {}/100 ({})",
            table.modules_scored,
            top.module,
            top.score,
            top.shape.as_str()
        ),
        None => format!("{} module(s) ranked, none placed", table.modules_scored),
    }
}

/// The candidate counts, as one line: `3 firm · 4 uncertain · 5 unverified`.
#[must_use]
pub fn dead_summary(report: &DeadReport) -> String {
    format!(
        "{} firm · {} uncertain · {} unverified",
        report.firm, report.uncertain, report.unverified
    )
}

/// Whether a candidate belongs to the subset a gate may act on.
#[must_use]
pub fn gateable(candidate: &Candidate) -> bool {
    candidate.verdict == Confidence::Firm
}

/// The points a row's parts sum to — a check a surface can assert rather than trust.
#[must_use]
pub fn check_sum(row: &Hotspot) -> Points {
    row.contributions
        .iter()
        .map(|item: &Measurement| item.points.0)
        .fold(Points(0), |total, points| Points(total.0 + points))
}

/// One line per thing the read could not do.
fn push_notices(out: &mut Vec<String>, notices: &[String]) {
    if notices.is_empty() {
        return;
    }
    out.push(String::new());
    for notice in notices {
        out.push(format!("  notice: {notice}"));
    }
}

/// A revision shortened the way every other pane shortens one.
fn short(revision: &str) -> String {
    if revision.len() <= 12 {
        return revision.to_string();
    }
    revision.chars().take(7).collect()
}

/// Truncate a path from the left, so the interesting end of `crates/x/src/y.rs` survives a column.
///
/// Char-safe: a repository with a non-ASCII path must not be able to panic a renderer by landing a
/// byte boundary inside a character.
fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut kept: Vec<char> = text.chars().rev().take(width.saturating_sub(1)).collect();
    kept.reverse();
    let mut out = String::from("…");
    out.extend(kept);
    out
}

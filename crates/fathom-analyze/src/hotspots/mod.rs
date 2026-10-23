//! Hotspots: which modules architectural attention pays off in, and what looks dead
//! (`e3.8`, `reco-wky`).
//!
//! The ticket's sentence, in full: *"Rank where architectural attention pays off. Per module: fan-in,
//! fan-out, instability, size, complexity proxy (from e3.1 spans), and churn velocity over a
//! configurable window, combined into an explainable hotspot score (high churn x high complexity is the
//! classic quadrant). Separately list dead-code candidates: newly added symbols with no inbound
//! references after N episodes, code behind flags that are off, and duplicated blocks that grew in the
//! same episode. Every metric shows its contribution to the score."* Every clause is a module here:
//!
//! | Clause | Where |
//! |---|---|
//! | the six metrics, their units, their knees | [`model`], one row per metric in [`model::Metric::ALL`] |
//! | fan-in / fan-out / instability | [`measure`], over [`crate::graph::query`]'s fan answers |
//! | size and complexity proxy, from `e3.1`'s spans | [`measure::FileFacts::of`] |
//! | churn velocity over a configurable window | [`measure::facts_of`] over `e3.3`'s [`crate::graph::ChurnReport`] |
//! | the score, and every metric's contribution to it | [`rank`] |
//! | the churn × complexity quadrant and the god-module shape | [`rank::describe`], [`model::Quadrant`], [`model::Shape`] |
//! | the knobs, in `fathom.toml` | [`policy`] |
//! | the three dead-code classes | [`deadcode`] |
//! | the words every surface prints | [`report`] |
//!
//! # Explainable is a contract with four clauses
//!
//! `e4.4` established what "explainable" means in this product and this module inherits it rather than
//! restating it: integer hundredths so the printed parts sum to the printed total, a denominator over
//! the *measured* metrics that is printed rather than implicit, one saturating curve per metric whose
//! knee is a published number, and monotonicity, so that retuning the weights is a deliberate act.
//! [`crate::risk::score`] uses the same arithmetic on the same hundredths type for the same reasons; two
//! scores in one product that both claim to be explainable must not be explainable in two ways. A row
//! looks like this and every number in it can be traced to a file:
//!
//! ```text
//! crates/core/src/engine.rs — 74/100 · 6 of 6 metrics measured · denominator 1.00
//!   fan_in     20 callers  0.63  0.25  18.75/25.00  20 module(s) depend on it, 2 wildcard
//!   complexity 260 points  0.68  0.20  16.67/20.00  260 points over 14 declaration(s); longest 210
//!   …
//! ```
//!
//! # What a ranking here is not
//!
//! **Not a quality judgement.** A high score says *this module is expensive to change and widely felt*,
//! which is true of the load-bearing wall in every codebase. The `e3.1` complexity numbers are
//! explicitly "proxies … not a cyclomatic-computation claim" and nothing here upgrades them.
//!
//! **Not a claim about unknown code.** A module with no structural record has no size and no complexity,
//! and those metrics are *unmeasured* — redistributed into the denominator and printed as a sentence —
//! rather than zero. `0` and "unknown" differ in the direction that matters: an unread generated file
//! ranked as tiny and stable is worse than no rank.
//!
//! **Not a ranking when there is nothing to rank.** A new repository has one revision and therefore no
//! velocity, and possibly no history to have a graph at all. [`rank::rank`] returns an empty state with
//! a reason rather than a table of zeroes, which is the third acceptance criterion, and the emptiness of
//! "no new symbols in the window" is a different sentence from the emptiness of "every new symbol is
//! named somewhere".
//!
//! **Not a proof for dead code.** Every candidate carries its age, the bar that made it a candidate, and
//! one line per mechanism that could make it wrong ([`deadcode::LIMITS`] is the permanent list). A
//! symbol added one episode ago is `unverified`, not dead; a module reached through a wildcard is
//! `uncertain`, because a star import can name a symbol without the graph seeing which one it meant.
//!
//! # Where the numbers come from, and where they do not
//!
//! This module reads no store, no filesystem, no clock and no environment (ADR-002 §4.5). The lane seam
//! is [`measure::facts_of`]: a caller that has materialised a revision's graph, `e3.1`'s records for
//! that revision and `e3.3`'s churn window hands them over as values and gets [`model::ModuleFacts`].
//! The same seam is what lets `crates/fathom-analyze/tests/hotspots.rs` seed a god module and assert the
//! whole ranking in a build with no SQLite and no grammars linked at all (ADR-002 §6.1).

pub mod deadcode;
pub mod measure;
pub mod model;
pub mod policy;
pub mod rank;
pub mod report;

pub use crate::hotspots::deadcode::{
    AddedLines, Age, Candidate, CandidateKind, Confidence, FlagKind, Gate, LIMITS, NewSymbol,
    Report as DeadCode, Source, duplicated_growth, gated, gates, unreferenced, unreferenced_report,
};
pub use crate::hotspots::measure::{FileFacts, facts_of, facts_of_records, missing_structures};
pub use crate::hotspots::model::{
    ChurnFacts, ComplexityFacts, Hotspot, Measurement, Metric, MetricSpec, ModuleFacts, Origin,
    Quadrant, Shape, Table, Weight,
};
pub use crate::hotspots::policy::Policy;
pub use crate::hotspots::policy::{Provenance, policy_from_config_text};
pub use crate::hotspots::rank::{CONTRACT, hotspot, rank};
pub use crate::hotspots::report::{render, render_dead, render_row, summary};

use crate::graph::{ChurnReport, Graph, Naming};

/// The default knee of a metric, without building a policy.
///
/// [`policy::Policy::knee`] is the real accessor; this exists for a caller that wants to print what a
/// metric's curve is before it has resolved any configuration.
#[must_use]
pub fn default_knee(metric: Metric) -> Option<f64> {
    metric.spec().default_knee
}

/// One call, whole pipeline: facts in, ranked table out.
///
/// `churn` is `None` when there is no earlier revision to measure against; the churn metric then prints
/// as unmeasured with the window it wanted, and the ranking proceeds on the five metrics that could be
/// measured. That is the cold-index case, and it is a ranking with a stated hole rather than a refusal,
/// because a repository's first index still has fan-in, fan-out, size and complexity to say.
#[must_use]
pub fn table_from(
    revision: &str,
    graph: &Graph,
    naming: &Naming,
    files: &[FileFacts],
    churn: Option<&ChurnReport>,
    policy: &Policy,
    notices: &[String],
) -> Table {
    let facts = facts_of(graph, naming, files, churn);
    rank(revision, &facts, policy, notices)
}

/// The three sweeps' shared input, as one value: eight positional arguments is a call that can be
/// written in the wrong order and still compile.
#[derive(Debug)]
pub struct DeadCodeInput<'a> {
    /// The revision being reviewed.
    pub revision: &'a str,
    /// The materialised graph at that revision.
    pub graph: &'a Graph,
    /// Its dictionary.
    pub naming: &'a Naming,
    /// The symbols that appeared inside the window, with how long ago they arrived.
    pub new_symbols: &'a [NewSymbol],
    /// The file texts the flag sweep reads.
    pub gated: &'a [Source<'a>],
    /// The added lines the duplication sweep reads.
    pub added: &'a [AddedLines<'a>],
    /// The knobs.
    pub policy: &'a Policy,
    /// What the caller's read could not do.
    pub notices: &'a [String],
}

/// The three dead-code questions, answered together against one policy.
///
/// The three sweeps are separate functions on purpose — a surface that can afford the graph read may not
/// be able to afford the file reads — but they share [`Policy`], one ordering and one list of limits, so
/// "the dead-code report" is one document and not three that were stapled together.
#[must_use]
pub fn dead_code_from(input: DeadCodeInput<'_>) -> DeadCode {
    let DeadCodeInput {
        revision,
        graph,
        naming,
        new_symbols,
        gated,
        added,
        policy,
        notices,
    } = input;
    let mut report = unreferenced_report(
        revision,
        graph,
        naming,
        new_symbols,
        policy.window,
        policy,
        notices.to_vec(),
    );
    let mut gated_report = deadcode::gated(revision, gated, policy);
    let mut growth = deadcode::duplicated_growth(revision, added, policy);
    report.candidates.append(&mut gated_report.candidates);
    report.candidates.append(&mut growth.candidates);
    report.gated_not_reported += gated_report.gated_not_reported;
    report.ignored_paths += gated_report.ignored_paths + growth.ignored_paths;
    report.notices.extend(gated_report.notices);
    report.notices.extend(growth.notices);
    report
        .candidates
        .sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.subject.cmp(&b.subject)));
    report.firm = report
        .candidates
        .iter()
        .filter(|candidate| candidate.verdict == Confidence::Firm)
        .count();
    report.uncertain = report
        .candidates
        .iter()
        .filter(|candidate| candidate.verdict == Confidence::Uncertain)
        .count();
    report.unverified = report
        .candidates
        .iter()
        .filter(|candidate| candidate.verdict == Confidence::Unverified)
        .count();
    if report.candidates.is_empty() && report.empty_reason.is_none() {
        report.empty_reason = Some("nothing was found by any of the three sweeps".to_string());
    }
    report
}

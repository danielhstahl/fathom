//! The per-episode path: two revisions in the store, one layering answer.
//!
//! This is the seam between [`crate::graph::lane`] and [`crate::layering::engine`], and it is
//! store-gated for the same reason the lane is. The whole reason it exists is the *order* it spends
//! money in:
//!
//! ```text
//!   episode(lane, from, to, rules, locate, ratchet, now)
//!       ├─ materialise(from)                  one chain walk — the only graph built
//!       ├─ changed_edges(from, to)            adjacent: the stored rows, one indexed query
//!       ├─ evaluate(from)                     one linear pass over its rows
//!       ├─ apply the ops to that graph        no second materialisation
//!       ├─ evaluate(after)                    one more linear pass
//!       └─ Report { delta, verdict }
//! ```
//!
//! A caller that materialised both revisions gets the same answer at twice the chain-walk cost, which
//! is the difference `s3` measured as roughly a thousand times; a caller that evaluated only the
//! changed rows gets a cheaper answer that cannot be right, because a boundary rule
//! (*only the api crate may reach db*) can be broken by an edge whose **target** is a file nobody
//! touched. So the judgment is whole-graph and the *materialisation* is what stays incremental, which
//! is the honest split and the one this file keeps.
//!
//! [`verify`] is the guard on that split: the graph built by applying the stored delta rows must
//! produce the same violations as the graph the lane materialises from the chain. A disagreement is a
//! bug in the lane or in the delta, not a state of the repository — which is why it is a test and a
//! check, and never something the keystroke path pays for.

use fathom_core::time::Timestamp;
use fathom_store::graph::ChainPartial;

use crate::cycles::debt::Ratchet;
use crate::graph::lane::Lane;
use crate::graph::model::{Graph, Op};
use crate::graph::query::Naming;
use crate::layering::engine::{Assessment, evaluate};
use crate::layering::locate::Locate;
use crate::layering::report::Report;
use crate::layering::rules::Rules;

/// What one revision owes the declared rules, with no episode to compare it to.
///
/// The cold-index answer, and what a surface prints when there is no earlier revision in the store.
/// Nothing is labelled *introduced* here, because there is nothing to introduce it relative to — a
/// report that called every violation new would be the noise the ratchet exists to prevent.
///
/// # Errors
///
/// [`ChainPartial`] when the revision's chain reaches no anchor within `graph.query.max_chain_walk`.
pub fn snapshot(
    lane: &Lane<'_>,
    tree: &str,
    rules: &Rules,
    locate: &Locate,
    ratchet: &Ratchet,
    now: Timestamp,
) -> Result<Report, ChainPartial> {
    let (graph, _) = lane.materialise(tree)?;
    let naming = naming_of(lane)?;
    let assessment = evaluate(&graph, &naming, rules, locate);
    Ok(Report::build(rules, assessment, None, locate, ratchet, now))
}

/// What one episode did to the declared architecture.
///
/// `from` is the revision the previous episode left behind and `to` the one being reviewed; the ops are
/// read out of `graph_delta` when the two are adjacent, which is the case that runs on every keystroke.
///
/// # Errors
///
/// [`ChainPartial`] when either revision's chain reaches no anchor within `graph.query.max_chain_walk`.
pub fn episode(
    lane: &Lane<'_>,
    from: &str,
    to: &str,
    rules: &Rules,
    locate: &Locate,
    ratchet: &Ratchet,
    now: Timestamp,
) -> Result<Report, ChainPartial> {
    let (graph, _) = lane.materialise(from)?;
    let naming = naming_of(lane)?;
    let delta = lane.changed_edges(from, to)?;
    let ops = ops_of(&delta);
    let before = evaluate(&graph, &naming, rules, locate);
    let after = stepping(&graph, &ops);
    let after = evaluate(&after, &naming, rules, locate);
    Ok(Report::build(
        rules,
        after,
        Some(&before),
        locate,
        ratchet,
        now,
    ))
}

/// The same episode, with the expensive second opinion: the violations of the graph the stored delta
/// rows produce, against the violations of the revision the lane materialises.
///
/// `false` is a bug in the lane or in the stored delta, not a state of the repository. It costs two
/// materialisations and four evaluation passes, which is exactly why nothing but a test pays for it.
///
/// # Errors
///
/// [`ChainPartial`] when either revision's chain reaches no anchor.
pub fn verify(
    lane: &Lane<'_>,
    from: &str,
    to: &str,
    rules: &Rules,
    locate: &Locate,
) -> Result<bool, ChainPartial> {
    let (before, _) = lane.materialise(from)?;
    let (expected, _) = lane.materialise(to)?;
    let naming = naming_of(lane)?;
    let delta = lane.changed_edges(from, to)?;
    let ops = ops_of(&delta);
    let stepped = stepping(&before, &ops);
    let keys = |graph: &Graph| -> Vec<String> {
        evaluate(graph, &naming, rules, locate)
            .violations
            .iter()
            .map(crate::layering::finding::Violation::key)
            .collect()
    };
    Ok(keys(&stepped) == keys(&expected))
}

/// The violations of a revision that is two chain steps away, from the graph already in hand.
///
/// Exposed for a surface that wants the *after* assessment as well as the report, so it does not pay a
/// second materialisation to ask a second question of the same revision.
#[must_use]
pub fn assessment_of(graph: &Graph, naming: &Naming, rules: &Rules, locate: &Locate) -> Assessment {
    evaluate(graph, naming, rules, locate)
}

/// The dictionary, with the store error collapsed into the chain partial: a store that cannot be read is
/// not a chain that is long, but the caller's vocabulary is the partial.
fn naming_of(lane: &Lane<'_>) -> Result<Naming, ChainPartial> {
    lane.naming().map_err(|_| ChainPartial::AnchorLost {
        tree_oid: String::new(),
        steps: 0,
    })
}

/// The graph that results from applying one revision's ops — a copy, so the caller keeps its own.
fn stepping(graph: &Graph, ops: &[Op]) -> Graph {
    let mut out = graph.clone();
    out.apply(ops.iter().copied());
    out
}

/// The row ops an edge delta applies, removals first.
///
/// The order is `crate::cycles::episode`'s, reused rather than restated: a moved dependency has to
/// read as one move in both engines, and two orderings would be two answers.
#[must_use]
pub fn ops_of(delta: &crate::graph::delta::EdgeDelta) -> Vec<Op> {
    crate::cycles::episode::ops_of(delta)
}

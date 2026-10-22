//! The per-episode path: two revisions in the store, one cycle answer.
//!
//! This is the seam between [`crate::graph::lane`] and [`Tracker`], and it is store-gated for the same
//! reason the lane is. Everything it does is already possible by hand — materialise a revision, read its
//! stored delta, feed the ops to a tracker — and it lives here anyway for one reason: the *order*. A
//! caller that materialises `to` and diffs the graphs gets the right answer at twice the price and no
//! incrementality; a caller that feeds the stored ops to a tracker built on `from` gets the cheap
//! answer. Getting those two consistent is the whole content of [`episode`] and [`verify`], so they are
//! written once, next to each other, and tested against each other.
//!
//! ```text
//!   episode(lane, from, to)
//!       ├─ materialise(from)                        one chain walk
//!       ├─ changed_edges(from, to)                  adjacent: the stored rows, one indexed query
//!       ├─ Tracker::build(from) + apply(ops)        the region the episode touched
//!       └─ Report
//!   verify(lane, from, to)
//!       ├─ the above, then
//!       ├─ materialise(to)                          the expensive second opinion
//!       └─ incremental partition == full recompute of `to`
//! ```
//!
//! # The naming is read once, and after the write
//!
//! [`crate::graph::lane::Lane::naming`] is the store's dictionary, which is a superset of any one
//! revision's nodes — so one read names both endpoints of the episode, including a module that `to`
//! introduced and `from` never saw. Reading it before a revision is recorded is how a surface ends up
//! printing `#4127` where a path belongs, which is why the dictionary is read here rather than handed in.

use fathom_core::time::Timestamp;
use fathom_store::graph::ChainPartial;

use crate::cycles::debt::Ratchet;
use crate::cycles::report::Report;
use crate::cycles::track::{Options, Tracker};
use crate::graph::lane::Lane;
use crate::graph::model::{Graph, Op};
use crate::graph::query::Naming;

/// The cycles of one revision, with nothing to compare them to: the cold-index answer, and what a
/// surface prints when there is no earlier revision in the store.
///
/// # Errors
///
/// [`ChainPartial`] when the revision's chain reaches no anchor within `graph.query.max_chain_walk`.
pub fn snapshot(
    lane: &Lane<'_>,
    tree: &str,
    ratchet: &Ratchet,
    options: Options,
    now: Timestamp,
) -> Result<Report, ChainPartial> {
    let (graph, _) = lane.materialise(tree)?;
    let naming = naming_of(lane)?;
    let tracker = Tracker::build(&graph, &naming, options);
    Ok(tracker.snapshot(ratchet, now))
}

/// What one episode did to the cycles, incrementally.
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
    ratchet: &Ratchet,
    options: Options,
    now: Timestamp,
) -> Result<Report, ChainPartial> {
    let (graph, _) = lane.materialise(from)?;
    let naming = naming_of(lane)?;
    let delta = lane.changed_edges(from, to)?;
    let ops = ops_of(&delta);
    let mut tracker = Tracker::build(&graph, &naming, options);
    Ok(tracker.episode(&ops, &naming, ratchet, now))
}

/// The same episode, with the expensive second opinion: the incrementally maintained partition against a
/// full recomputation of the revision the episode ended at.
///
/// `false` is a bug in [`crate::cycles::scc`], not a state of the repository. It costs two materialisations
/// and a whole-graph pass, which is exactly why it is a test and a bench check rather than something the
/// interactive path pays for.
///
/// # Errors
///
/// [`ChainPartial`] when either revision's chain reaches no anchor.
pub fn verify(
    lane: &Lane<'_>,
    from: &str,
    to: &str,
    options: Options,
) -> Result<bool, ChainPartial> {
    let (before, _) = lane.materialise(from)?;
    let (after, _) = lane.materialise(to)?;
    let naming = naming_of(lane)?;
    let delta = lane.changed_edges(from, to)?;
    let ops = ops_of(&delta);
    let mut tracker = Tracker::build(&before, &naming, options);
    tracker.episode(
        &ops,
        &naming,
        &Ratchet::empty(),
        Timestamp::from_unix_micros(0),
    );
    Ok(tracker.verify(&after, &naming))
}

/// The dictionary, with the store error collapsed into the chain partial: a store that cannot be read is
/// not a chain that is long, but the caller's vocabulary is the partial and its [`ChainPartial::sentence`]
/// is what the pane prints.
fn naming_of(lane: &Lane<'_>) -> Result<Naming, ChainPartial> {
    lane.naming().map_err(|_| ChainPartial::AnchorLost {
        tree_oid: String::new(),
        steps: 0,
    })
}

/// The row ops an edge delta applies, removals first.
///
/// The order is not cosmetic: [`crate::cycles::scc`] reads the pre-update partition when deciding what a
/// deletion could have broken, and a moved dependency has to read as a move.
#[must_use]
pub fn ops_of(delta: &crate::graph::delta::EdgeDelta) -> Vec<Op> {
    let mut ops: Vec<Op> = delta.removed.iter().map(|row| Op::remove(*row)).collect();
    ops.extend(delta.added.iter().map(|row| Op::add(*row)));
    ops
}

/// The graph a report was computed against, for a caller that wants to ask a graph question with the same
/// materialisation it paid for the cycle answer with.
///
/// Provided because the alternative — a second `materialise` in the same pane — is the cost this ticket is
/// supposed to avoid.
pub fn graph_at(lane: &Lane<'_>, tree: &str) -> Result<Graph, ChainPartial> {
    lane.materialise(tree).map(|(graph, _)| graph)
}

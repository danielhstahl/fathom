//! The delta: what one commit did to the graph, and how two snapshots differ.
//!
//! `s3`'s first line of the policy is *"store ops, not snapshots"*, and this module is the half of
//! that sentence that decides what an op is: the set difference between two revisions' row sets,
//! computed **per module** so that a commit touching 2.56 files costs 36.3 ops rather than the
//! whole graph's 54 025 rows.
//!
//! # Why the diff is per module and not per graph
//!
//! A whole-graph difference would need both revisions' complete row sets in hand, which is exactly
//! the O(graph) per-revision work the byte budget exists to forbid. Per module, the lane holds each
//! live module's row set (keyed by path, cheap: a `Vec<Row>` per module), and a module whose blob did
//! not change is not even decoded — the blob-keyed substrate does the "did this change" test for
//! free, which is the second line of the policy: *keep the per-file structural cache as the
//! substrate*.
//!
//! # The order, because it is part of the answer
//!
//! Removals first, then additions, each sorted by `(kind, src, dst)`. Order is not cosmetic here:
//! the reconstruction applies a revision's ops in stored order, and `remove`-before-`add` means a
//! rename reads as a move rather than as two nodes that never coexisted. Rebuilding the same chain
//! twice has to produce the same graph twice, and it does only if the order is a function of the
//! rows.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::graph::build::ModuleRows;
use crate::graph::model::{Graph, Op, Row};

/// The canonical module key for a path: separators normalised, no leading `./`, no duplicate slashes.
///
/// Interning is keyed on this string, and a stored id whose key moved would silently change what
/// every chunk means, so normalisation happens once, here, and every caller goes through it.
#[must_use]
pub fn module_key(path: &str) -> String {
    let path = path.strip_prefix("./").unwrap_or(path);
    let mut out = String::with_capacity(path.len());
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(segment);
    }
    out
}

/// The ops that turn `old` into `new`, per module.
///
/// Rows only present in `old` come out as removals, rows only in `new` as additions, and a row in
/// both is not an op at all — which is the entire reason an unchanged module costs nothing.
#[must_use]
pub fn module_ops(
    old: impl IntoIterator<Item = Row>,
    new: impl IntoIterator<Item = Row>,
) -> Vec<Op> {
    let before: BTreeSet<Row> = old.into_iter().collect();
    let after: BTreeSet<Row> = new.into_iter().collect();
    let mut ops: Vec<Op> = before
        .difference(&after)
        .map(|row| Op::remove(*row))
        .chain(after.difference(&before).map(|row| Op::add(*row)))
        .collect();
    // `false < true`, so keying on `add` puts every removal first: a rename then reads as a move
    // rather than as two nodes that never coexisted.
    ops.sort_by_key(|op| (op.add, op.row));
    ops
}

/// The ops between two revisions' module sets, covering removals of whole modules.
///
/// A module that vanished is not a special case anywhere else: its rows are simply in `old` and not
/// in `new`, and the delete arrives as the removal of every row it owned, including its node.
#[must_use]
pub fn revision_ops(
    old: &BTreeMap<String, ModuleRows>,
    new: &BTreeMap<String, ModuleRows>,
) -> Vec<Op> {
    let mut ops = Vec::new();
    // Changed or removed modules, in path order.
    for (path, before) in old {
        let after = new.get(path);
        ops.extend(module_ops(
            before.rows.iter().copied(),
            after
                .into_iter()
                .flat_map(|module| module.rows.iter().copied()),
        ));
    }
    // Added modules: nothing to subtract from.
    for (path, after) in new {
        if old.contains_key(path) {
            continue;
        }
        ops.extend(after.rows.iter().copied().map(Op::add));
    }
    ops.sort_by_key(|op| (op.add, op.row));
    ops
}

/// Which modules differ between two revisions — the lane's "what changed" list, which is what keeps
/// a marginal revision O(changed) instead of O(live).
#[must_use]
pub fn changed_modules(
    old: &BTreeMap<String, ModuleRows>,
    new: &BTreeMap<String, ModuleRows>,
) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for (path, before) in old {
        match new.get(path) {
            None => paths.push(path.clone()),
            Some(after) => {
                if before.blob != after.blob {
                    paths.push(path.clone());
                }
            }
        }
    }
    for path in new.keys() {
        if !old.contains_key(path) {
            paths.push(path.clone());
        }
    }
    paths.sort();
    paths
}

/// The edges two snapshots disagree about.
///
/// This is `e3.3`'s question — "what did this episode change about the architecture" — and the
/// honest note is that a non-adjacent pair costs two materialisations. `s3` measured that: 272 ms
/// p95 for a strategy that must rebuild, against 0.194 ms for reading a stored delta, which is why
/// [`crate::graph::lane::Lane::changed_edges`] takes the stored path when it can.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EdgeDelta {
    /// Rows in `to` that `from` does not have.
    pub added: Vec<Row>,
    /// Rows in `from` that `to` does not have.
    pub removed: Vec<Row>,
    /// Rows both have. Reported because a delta that only counts its own size cannot answer "how
    /// small was this change relative to the graph".
    pub unchanged: usize,
    /// Whether the answer came from stored rows rather than from two materialisations.
    pub from_store: bool,
}

impl EdgeDelta {
    /// Diff two materialised graphs.
    #[must_use]
    pub fn between(from: &Graph, to: &Graph) -> Self {
        let before: BTreeSet<Row> = from.rows().into_iter().collect();
        let after: BTreeSet<Row> = to.rows().into_iter().collect();
        Self {
            added: after.difference(&before).copied().collect(),
            removed: before.difference(&after).copied().collect(),
            unchanged: before.intersection(&after).count(),
            from_store: false,
        }
    }

    /// A delta read straight out of `graph_delta`, with the flag set.
    #[must_use]
    pub fn from_ops(ops: &[Op]) -> Self {
        let mut delta = Self {
            from_store: true,
            ..Self::default()
        };
        for op in ops {
            if op.add {
                delta.added.push(op.row);
            } else {
                delta.removed.push(op.row);
            }
        }
        delta.added.sort();
        delta.removed.sort();
        delta
    }

    /// How many rows moved.
    #[must_use]
    pub fn len(&self) -> usize {
        self.added.len() + self.removed.len()
    }

    /// Whether nothing moved, which for a graph is a real and common answer.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Apply ops to a graph and report the graph (the cursor's one-commit step is this and nothing else).
#[must_use]
pub fn apply_ops(mut graph: Graph, ops: impl IntoIterator<Item = Op>) -> Graph {
    graph.apply(ops);
    graph
}

//! Strongly-connected components, maintained rather than recomputed.
//!
//! The ticket's first sentence is the whole design brief: *"incremental strongly-connected-component
//! maintenance rather than a full recompute per episode"*. An episode is a commit or two, and the
//! repository `s3` measured changes 2.56 files and writes 36.3 rows per revision against a live graph
//! of 2 298 modules and 54 025 rows. Recomputing the components of a graph like that on every keystroke
//! is a full graph walk per keystroke; the arithmetic below touches the region an episode could have
//! affected and nothing else.
//!
//! # Why the region is what it is
//!
//! Both halves are elementary, and both need a proof rather than a comment, because the wrong region is
//! a wrong answer that looks plausible:
//!
//! * **A deletion can only split.** Removing edges destroys paths, and destroying paths can only break
//!   mutual reachability, never create it. So a split can only happen *inside* a component that lost one
//!   of its own internal edges (or lost a node), and no other component's membership can change.
//!   Restricting the recomputation to the broken component's own nodes is exact, not an approximation:
//!   suppose two nodes `u, v` of a component `C` are still mutually reachable after the deletion, by a
//!   path that leaves `C` through some node `d`. Then `d` was reachable from `C` and could reach `C`
//!   *before* the deletion too — every surviving path existed before it — so `d` was in `C`, which is a
//!   contradiction. Surviving paths between members of `C` stay inside `C`, which is what lets the
//!   induced subgraph answer the question.
//! * **An insertion can only merge, and it merges the region between the endpoints.** Adding `u → v`
//!   (with `u` in `Cu`, `v` in `Cv`, `Cu ≠ Cv`) creates a cycle if and only if `Cv` already reached `Cu`
//!   in the condensation. The new component is then exactly the components lying on a path from `Cv` to
//!   `Cu` — the intersection of *reachable from `Cv`* with *able to reach `Cu`* — because each of them
//!   is now mutually reachable with `u` and `v`, and nothing outside it has gained a path back. Two
//!   searches, each bounded by the region rather than by the graph.
//!
//! The two halves run in that order, because a delta that removes and adds in the same revision — every
//! moved dependency — is allowed to split and then re-merge. And each added edge is decided against the
//! flow *before* it gained that edge: decide against a flow that already contains it and the search
//! finds its way back along the new edge itself and merges two components that were never mutually
//! reachable. [`super::track`] owns that interleaving, which is the only reason this module's three
//! update calls are three instead of one.
//!
//! # Determinism
//!
//! Nodes are walked in id order and neighbours in id order in both passes, so component *ids* are a
//! function of the graph. They are still not stable across a split or a merge, and nothing may assume
//! they are: identity lives in [`super::cycle::Cycle::key`], which is made of module names.
//!
//! # The counters
//!
//! "Incremental rather than a full recompute" is a claim a test can only check if the code admits
//! numbers: [`Partition::full_recomputes`] counts whole-graph passes and [`Partition::nodes_searched`]
//! counts the nodes the incremental region searches actually walked. The tests assert the first stays
//! at one, and the bench asserts the second stays proportional to the episode rather than to the graph.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::cycles::flow::Flow;

/// One component id, minted here and never trusted across an update.
pub type Component = u32;

/// What an incremental update touched, so the caller knows which cycle records to re-derive.
#[derive(Debug, Clone, Default)]
pub struct Affected {
    /// Components that exist now and may have changed shape.
    pub touched: BTreeSet<Component>,
    /// Components that no longer exist.
    pub destroyed: BTreeSet<Component>,
    /// Splits performed (one induced recomputation each).
    pub splits: usize,
    /// Merges performed (each of two or more components).
    pub merges: usize,
}

impl Affected {
    /// Whether the update changed no component at all.
    #[must_use]
    pub fn is_quiet(&self) -> bool {
        self.touched.is_empty() && self.destroyed.is_empty()
    }

    /// Fold another outcome into this one, as an episode accumulates its steps.
    pub fn merge(&mut self, other: &Self) {
        self.touched.extend(other.touched.iter());
        self.destroyed.extend(other.destroyed.iter());
        self.splits += other.splits;
        self.merges += other.merges;
    }
}

/// The partition of a [`Flow`]'s nodes into strongly-connected components.
#[derive(Debug, Clone, Default)]
pub struct Partition {
    comp_of: BTreeMap<i64, Component>,
    members: BTreeMap<Component, BTreeSet<i64>>,
    next: Component,
    /// Whole-graph recomputations performed: one at construction, plus one per deliberate recompute.
    full_recomputes: usize,
    /// Incremental updates applied.
    updates: usize,
    /// Nodes the region searches and induced recomputations have walked in total.
    nodes_searched: usize,
}

impl Partition {
    /// The full computation: two iterative depth-first passes (Kosaraju) over forward then reverse
    /// edges, so a 5 000-node chain cannot overflow a stack. A recursive Tarjan is a stack overflow
    /// waiting for a generated barrel file, and every corpus has generated barrel files.
    #[must_use]
    pub fn compute(flow: &Flow) -> Self {
        let mut partition = Self {
            full_recomputes: 1,
            ..Self::default()
        };
        let mut order: Vec<i64> = Vec::with_capacity(flow.nodes().len());
        let mut visited: BTreeSet<i64> = BTreeSet::new();
        for &start in flow.nodes() {
            if !visited.insert(start) {
                continue;
            }
            let mut stack: Vec<Frame> = vec![Frame::new(start, flow)];
            while let Some(top) = stack.last_mut() {
                if top.cursor < top.nexts.len() {
                    let next = top.nexts[top.cursor];
                    top.cursor += 1;
                    if visited.insert(next) {
                        stack.push(Frame::new(next, flow));
                    }
                    continue;
                }
                order.push(top.node);
                stack.pop();
            }
        }
        for &start in order.iter().rev() {
            if partition.comp_of.contains_key(&start) {
                continue;
            }
            let id = partition.mint();
            let mut bucket = BTreeSet::new();
            let mut stack = vec![start];
            while let Some(node) = stack.pop() {
                if partition.comp_of.contains_key(&node) {
                    // Already claimed by an earlier component. Kosaraju's second pass may only *reach*
                    // a node it has already assigned; taking it again would overlap two components,
                    // which is not a slow answer but a wrong one.
                    continue;
                }
                if !bucket.insert(node) {
                    continue;
                }
                partition.nodes_searched += 1;
                stack.extend(flow.predecessors(node));
            }
            for &node in &bucket {
                partition.comp_of.insert(node, id);
            }
            partition.members.insert(id, bucket);
        }
        partition
    }

    /// The component a node belongs to.
    #[must_use]
    pub fn component_of(&self, node: i64) -> Option<Component> {
        self.comp_of.get(&node).copied()
    }

    /// The members of a component, if it still exists.
    #[must_use]
    pub fn members(&self, component: Component) -> Option<&BTreeSet<i64>> {
        self.members.get(&component)
    }

    /// Every component, by id.
    #[must_use]
    pub fn components(&self) -> &BTreeMap<Component, BTreeSet<i64>> {
        &self.members
    }

    /// How many components the partition holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the partition holds nothing. Required by the lint that requires `len`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// How many whole-graph passes this partition has had. One, for anything built and then updated —
    /// which is the acceptance criterion's "incremental", made countable.
    #[must_use]
    pub const fn full_recomputes(&self) -> usize {
        self.full_recomputes
    }

    /// How many incremental updates have been applied.
    #[must_use]
    pub const fn updates(&self) -> usize {
        self.updates
    }

    /// How many nodes the incremental searches have walked in total.
    #[must_use]
    pub const fn nodes_searched(&self) -> usize {
        self.nodes_searched
    }

    /// A stable reading of the components, for the assertion the tests and the bench make:
    /// *the incrementally maintained partition is the partition a full recompute produces.*
    #[must_use]
    pub fn signature(&self) -> Vec<Vec<i64>> {
        let mut out: Vec<Vec<i64>> = self
            .members
            .values()
            .map(|bucket| bucket.iter().copied().collect::<Vec<i64>>())
            .collect();
        for bucket in &mut out {
            bucket.sort_unstable();
        }
        out.sort();
        out
    }

    // -- an update, in the three steps the flow's ordering forces ----------------------

    /// Which components this episode's removals could have broken.
    ///
    /// Only a component that lost an edge *inside itself*, or lost a node, can split: a deletion cannot
    /// merge anything, and a cross-component edge was never part of anyone's mutual reachability. A
    /// self-loop going away does not change the partition at all, but it changes the *answer*, so the
    /// component that held it is marked for a fresh record.
    #[must_use]
    pub fn damage(
        &self,
        removed_edges: &[(i64, i64)],
        removed_nodes: &[i64],
        removed_self_loops: &[i64],
    ) -> BTreeSet<Component> {
        let mut dirty = BTreeSet::new();
        let mut mark = |component: Component| {
            if self.members.get(&component).is_some_and(|b| b.len() > 1) {
                dirty.insert(component);
            }
        };
        for (from, to) in removed_edges {
            if let (Some(a), Some(b)) = (self.comp_of.get(from), self.comp_of.get(to))
                && a == b
            {
                mark(*a);
            }
        }
        for node in removed_nodes {
            if let Some(component) = self.comp_of.get(node) {
                mark(*component);
            }
        }
        for node in removed_self_loops {
            if let Some(component) = self.comp_of.get(node) {
                dirty.insert(*component);
            }
        }
        dirty
    }

    /// Drop the removed nodes, then re-split every damaged region against the post-removal flow.
    pub fn repair_deletions(
        &mut self,
        flow: &Flow,
        dirty: &BTreeSet<Component>,
        removed_nodes: &[i64],
    ) -> Affected {
        let mut affected = Affected::default();
        for node in removed_nodes {
            let Some(component) = self.comp_of.remove(node) else {
                continue;
            };
            if let Some(bucket) = self.members.get_mut(&component) {
                bucket.remove(node);
                if bucket.is_empty() {
                    self.members.remove(&component);
                }
            }
        }
        for &component in dirty {
            let Some(members) = self.members.remove(&component) else {
                continue;
            };
            for node in &members {
                self.comp_of.remove(node);
            }
            let pieces = if members.len() < 2 {
                vec![members]
            } else {
                induced_components(flow, &members, &mut self.nodes_searched)
            };
            affected.splits += pieces.len().saturating_sub(1);
            // The first piece keeps the old id, so a component that merely shrank keeps the record its
            // key already had until its membership says otherwise. The rest are new.
            for (index, piece) in pieces.into_iter().enumerate() {
                let id = if index == 0 { component } else { self.mint() };
                for node in &piece {
                    self.comp_of.insert(*node, id);
                }
                self.members.insert(id, piece);
                affected.touched.insert(id);
            }
        }
        self.updates += 1;
        affected
    }

    /// Give a node its own component, if it has none. A dependency row can name a node no row ever
    /// declared, and a node with no history is a component of one.
    pub fn add_node(&mut self, node: i64) -> Option<Component> {
        if let Some(existing) = self.comp_of.get(&node) {
            return Some(*existing);
        }
        let id = self.mint();
        self.members.insert(id, [node].into_iter().collect());
        self.comp_of.insert(node, id);
        Some(id)
    }

    /// Decide what one added edge does, against a flow that does **not** yet contain it.
    ///
    /// Both endpoints must already be in the partition ([`Partition::add_node`]).
    pub fn consider(&mut self, flow: &Flow, from: i64, to: i64) -> Affected {
        let mut affected = Affected::default();
        let (Some(source), Some(target)) = (
            self.comp_of.get(&from).copied(),
            self.comp_of.get(&to).copied(),
        ) else {
            return affected;
        };
        if source == target {
            // A new chord inside a component that was already a cycle. The witness can get shorter, so
            // the record is refreshed; the partition cannot change.
            affected.touched.insert(source);
            self.updates += 1;
            return affected;
        }
        let region = self.region(flow, source, target);
        if region.len() < 2 {
            // No way back: a new dependency, not a new cycle. This is the common case, and it cost a
            // probe bounded by what the source can be reached from.
            self.updates += 1;
            return affected;
        }
        let keep = region.iter().min().copied().unwrap_or(source);
        let mut merged: BTreeSet<i64> = BTreeSet::new();
        for component in &region {
            if *component == keep {
                continue;
            }
            if let Some(bucket) = self.members.remove(component) {
                merged.extend(bucket);
                affected.destroyed.insert(*component);
            }
        }
        for node in &merged {
            self.comp_of.insert(*node, keep);
        }
        self.members.entry(keep).or_default().extend(merged);
        affected.touched.insert(keep);
        affected.merges += 1;
        self.updates += 1;
        affected
    }

    /// Mint a component id.
    fn mint(&mut self) -> Component {
        let id = self.next;
        self.next += 1;
        id
    }

    /// The components that get merged when `source → target` is added: exactly the components on a path
    /// from `target` back to `source`.
    ///
    /// Two searches, and the order is a cost decision rather than an accident. Every component on such a
    /// path can reach `source`, so the whole region is inside the *backward* set from `source`; searching
    /// forward from `target` **restricted to that set** therefore returns the region exactly, and costs
    /// at most the backward set. On the chain-shaped graphs real repositories are — one long tail of
    /// downstream modules — the unrestricted forward search would walk the tail and throw it away.
    ///
    /// So: probe backward from `source` under a budget, and if it stayed small, do the restricted forward
    /// search. If the probe blew the budget (a module with the whole repository depending on it) fall back
    /// to the unrestricted pair, which is the same answer at the whole-graph price. Both branches are
    /// exact; only the cost differs.
    fn region(&mut self, flow: &Flow, source: Component, target: Component) -> BTreeSet<Component> {
        let probe = self.search(flow, source, false, None, Some(PROBE_BUDGET));
        if probe.1 < PROBE_BUDGET {
            let bounded = probe.0;
            let forward = self.search(flow, target, true, Some(&bounded), None).0;
            if forward.contains(&source) {
                // Traversal was restricted to `bounded`, so `forward ⊆ bounded` and this *is* the region.
                return forward;
            }
            return BTreeSet::new();
        }
        let forward = self.search(flow, target, true, None, None).0;
        if !forward.contains(&source) {
            return BTreeSet::new();
        }
        let backward = self.search(flow, source, false, None, None).0;
        forward.intersection(&backward).copied().collect()
    }

    /// The components reachable from `start`, optionally restricted to `allow`, giving up past
    /// `budget` nodes.
    ///
    /// Returns the set and the nodes it cost. Expanding a component costs its members' edges and happens
    /// once per component, so a search costs the region it visited rather than the graph.
    fn search(
        &mut self,
        flow: &Flow,
        start: Component,
        forward: bool,
        allow: Option<&BTreeSet<Component>>,
        budget: Option<usize>,
    ) -> (BTreeSet<Component>, usize) {
        let mut seen = BTreeSet::from([start]);
        let mut queue = vec![start];
        let mut spent = 0_usize;
        while let Some(component) = queue.pop() {
            let Some(members) = self.members.get(&component) else {
                continue;
            };
            for node in members {
                spent += 1;
                if budget.is_some_and(|limit| spent > limit) {
                    return (seen, spent);
                }
                self.nodes_searched += 1;
                let nexts: Vec<i64> = if forward {
                    flow.successors(*node).collect()
                } else {
                    flow.predecessors(*node).collect()
                };
                for next in nexts {
                    let Some(other) = self.comp_of.get(&next) else {
                        continue;
                    };
                    if allow.is_some_and(|allowed| !allowed.contains(other)) {
                        continue;
                    }
                    if seen.insert(*other) {
                        queue.push(*other);
                    }
                }
            }
        }
        (seen, spent)
    }
}

/// How many nodes the probe search may spend before the fallback is cheaper anyway. A budget, not a
/// bound on the answer: exceeding it changes the algorithm, never the result.
const PROBE_BUDGET: usize = 4_096;

/// One DFS frame of the iterative first pass: a node, its successors, and how far it has got.
struct Frame {
    node: i64,
    nexts: Vec<i64>,
    cursor: usize,
}

impl Frame {
    fn new(node: i64, flow: &Flow) -> Self {
        Self {
            node,
            nexts: flow.successors(node).collect(),
            cursor: 0,
        }
    }
}

/// The components of the subgraph induced on `members`.
///
/// Induced, not "the whole graph filtered afterwards": the proof in the module docs is that a path
/// between two surviving members of a broken component stays inside it, so this is exact — and it is
/// what makes the cost proportional to the region rather than to the repository.
fn induced_components(
    flow: &Flow,
    members: &BTreeSet<i64>,
    searched: &mut usize,
) -> Vec<BTreeSet<i64>> {
    let mut order: Vec<i64> = Vec::with_capacity(members.len());
    let mut visited: BTreeSet<i64> = BTreeSet::new();
    for &start in members {
        if !visited.insert(start) {
            continue;
        }
        let nexts = |node: i64| -> Vec<i64> {
            flow.successors(node)
                .filter(|next| members.contains(next))
                .collect()
        };
        let mut stack: Vec<(i64, Vec<i64>, usize)> = vec![(start, nexts(start), 0)];
        *searched += 1;
        while let Some(top) = stack.last_mut() {
            if top.2 < top.1.len() {
                let next = top.1[top.2];
                top.2 += 1;
                if visited.insert(next) {
                    *searched += 1;
                    stack.push((next, nexts(next), 0));
                }
                continue;
            }
            order.push(top.0);
            stack.pop();
        }
    }
    let mut pieces: Vec<BTreeSet<i64>> = Vec::new();
    let mut claimed: BTreeSet<i64> = BTreeSet::new();
    for &start in order.iter().rev() {
        if claimed.contains(&start) {
            continue;
        }
        let mut piece = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            if claimed.contains(&node) {
                continue;
            }
            if !piece.insert(node) {
                continue;
            }
            *searched += 1;
            for next in flow
                .predecessors(node)
                .filter(|next| members.contains(next))
            {
                stack.push(next);
            }
        }
        claimed.extend(piece.iter().copied());
        pieces.push(piece);
    }
    // Lowest member first, so the piece that keeps the old component id is the same piece every time
    // the same facts are replayed.
    pieces.sort_by_key(|piece| piece.iter().next().copied().unwrap_or(i64::MAX));
    pieces
}

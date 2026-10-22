//! The tracker: the thing that is *maintained* across episodes instead of recomputed per one.
//!
//! [`Tracker`] owns one [`State`] per view, and its one interesting method, [`Tracker::episode`],
//! takes a revision's row ops and returns the whole answer: which cycles appeared, which went away,
//! which grew, what each of them owes the ratchet, and the two numbers the scorecard trends. It is the
//! only place the interleaving of flow and partition updates happens, because that ordering is a
//! correctness property (see [`super::scc`]) and two callers doing it themselves is two chances to get
//! it wrong.
//!
//! ```text
//!   the revision's row ops (graph_delta, or the difference between two materialisations)
//!        │
//!        ├─ damage: which components could have broken      ← reads the PRE partition
//!        ├─ flow.apply_removals                             ← the flow moves first here
//!        ├─ repair_deletions: re-split each broken region   ← induced SCC, region-sized
//!        ├─ for each added edge, in sorted order:
//!        │     ├─ consider: merge the region between the endpoints   ← flow WITHOUT this edge
//!        │     └─ flow.apply_addition                                  ← the flow moves here
//!        ├─ re-derive the records of the components the episode touched
//!        └─ diff the cycle keys → introduced · resolved · grew → CycleDelta → Report
//! ```
//!
//! # What is not recomputed, ever
//!
//! The partition is computed once, in [`State::build`]. After that a revision costs the region it could
//! have affected: one induced pass per broken component and at most two condensation searches per added
//! edge. The records of the components an episode did not touch are kept as they were, which is the
//! second half of the same claim — re-deriving every witness on every revision would put the whole graph
//! back into the cost of a keystroke.
//!
//! # The check that says the maintenance is correct
//!
//! Incremental data structures are wrong in ways that only show up after a hundred revisions, so
//! [`Tracker::verify`] re-runs the full computation and compares partitions.
//! `crates/fathom-analyze/tests/cycles.rs` walks a scripted history through the incremental path and
//! verifies after every revision, and so does the bench.

use std::collections::BTreeMap;

use crate::cycles::cycle::{Cycle, CycleDelta, Witness, record, witness};
use crate::cycles::debt::{Ratchet, Verdict};
use crate::cycles::flow::{Flow, View};
use crate::cycles::health::Health;
use crate::cycles::report::{Counters, Report};
use crate::cycles::scc::{Affected, Component, Partition};
use crate::graph::model::{Graph, Op, RowKind, Vertex};
use crate::graph::query::Naming;
use fathom_core::time::Timestamp;

/// How a tracker is configured.
///
/// These are code-level knobs, deliberately not `fathom.toml` keys: ADR-004 §5.1 declares this
/// analyzer's inputs as `layers.` and `ratchet.`, and a `graph.cycles.*` key belongs to the ticket that
/// registers it rather than to whichever ticket wants it. The symbol view is opt-in in exactly the sense
/// the ticket means — a caller asks for it and pays for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// The primary view: the one a [`Report`] answers in.
    pub view: View,
    /// Also maintain the symbol-level view, and answer in it too (`e3.5`'s opt-in).
    pub symbol_view: bool,
    /// How many cycles a delta lists before it says it stopped counting.
    pub max_reported: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            view: View::Module,
            symbol_view: false,
            max_reported: 64,
        }
    }
}

impl Options {
    /// Ask for the symbol-level view as well as the module view.
    #[must_use]
    pub const fn with_symbol_view(mut self) -> Self {
        self.symbol_view = true;
        self
    }

    /// The views this configuration maintains, primary first and the module view always present.
    #[must_use]
    pub fn views(&self) -> Vec<View> {
        let mut views = vec![self.view];
        if self.symbol_view && self.view != View::Symbol {
            views.push(View::Symbol);
        }
        if !views.contains(&View::Module) {
            views.push(View::Module);
        }
        views
    }
}

/// One view's state: the adjacency, the partition, the records, and the names both need.
#[derive(Debug, Clone)]
pub struct State {
    flow: Flow,
    partition: Partition,
    /// The current records, by component.
    records: BTreeMap<Component, Cycle>,
    /// The dictionary this view needs, grown as new ids arrive. The lane owns the authoritative copy;
    /// this is the projection of it that the records can print, kept so that a rebuild of a graph does
    /// not need a store in hand to name itself.
    vertices: BTreeMap<i64, Vertex>,
    options: Options,
}

impl State {
    /// Build a view from a materialised revision. This is the one and only full computation.
    #[must_use]
    pub fn build(graph: &Graph, naming: &Naming, options: Options, view: View) -> Self {
        let flow = Flow::build(graph, naming, view);
        let partition = Partition::compute(&flow);
        let vertices: BTreeMap<i64, Vertex> = flow
            .nodes()
            .iter()
            .map(|id| (*id, vertex_of(*id, &flow, naming)))
            .collect();
        let mut state = Self {
            flow,
            partition,
            records: BTreeMap::new(),
            vertices,
            options,
        };
        let dictionary = state.dictionary();
        let components: Vec<Component> = state.partition.components().keys().copied().collect();
        for component in components {
            state.refresh(component, &dictionary);
        }
        state
    }

    /// The view.
    #[must_use]
    pub const fn view(&self) -> View {
        self.flow.view()
    }

    /// The adjacency, for a caller that wants to ask a reachability question of its own.
    #[must_use]
    pub const fn flow(&self) -> &Flow {
        &self.flow
    }

    /// The partition, for the equality check in [`Tracker::verify`].
    #[must_use]
    pub const fn partition(&self) -> &Partition {
        &self.partition
    }

    /// Every cycle the view holds right now, sorted by key.
    #[must_use]
    pub fn cycles(&self) -> Vec<Cycle> {
        let mut out: Vec<Cycle> = self.records.values().cloned().collect();
        out.sort_by_key(Cycle::key);
        out
    }

    /// How many nodes sit inside a cycle — the health metric the ticket names.
    #[must_use]
    pub fn nodes_in_cycles(&self) -> usize {
        self.records.values().map(Cycle::len).sum()
    }

    /// How many full computations this view has had. One, for anything that was then updated.
    #[must_use]
    pub const fn full_recomputes(&self) -> usize {
        self.partition.full_recomputes()
    }

    /// How many nodes the incremental searches have walked in this view.
    #[must_use]
    pub const fn nodes_searched(&self) -> usize {
        self.partition.nodes_searched()
    }

    /// The nodes in the view: the denominator of "how much of the graph is inside a cycle".
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.flow.nodes().len()
    }

    /// Endpoints the dictionary could not name — a stated partial, not an absence.
    #[must_use]
    pub fn unnamed(&self) -> usize {
        self.flow.unnamed()
    }

    /// Apply one episode's ops and report what it did.
    pub fn apply(&mut self, ops: &[Op], naming: &Naming) -> CycleDelta {
        let before: BTreeMap<String, Cycle> = self
            .records
            .values()
            .map(|cycle| (cycle.key(), cycle.clone()))
            .collect();

        // -- deletions: read the pre-partition, move the flow, then repair -------------------------
        let removed_edges = self.flow.removed_pairs(ops, naming);
        let (added_nodes, mut removed_nodes) = self.flow.node_changes(ops, naming);
        let (added_loops, removed_loops) = self.flow.self_loop_changes(ops, naming);
        self.flow.apply_removals(ops, naming);
        // Nodes that only an now-gone row kept alive, plus the nodes the revision deleted: the partition
        // is told about both the same way, and the damage it can do is the same.
        removed_nodes.extend(self.flow.prune_endpoints());
        let dirty = self
            .partition
            .damage(&removed_edges, &removed_nodes, &removed_loops);
        let mut affected: Affected =
            self.partition
                .repair_deletions(&self.flow, &dirty, &removed_nodes);

        // -- additions: nodes first, then each edge decided before the flow gains it ----------------
        for node in &added_nodes {
            if let Some(component) = self.partition.add_node(*node) {
                affected.touched.insert(component);
            }
        }
        self.flow.apply_node_additions(ops, naming);
        let mut additions: Vec<(i64, i64, Op)> = Vec::new();
        for op in ops.iter().filter(|op| op.add) {
            let (kind, src, dst) = op.row;
            if kind.is_node() || !kind.is_dependency() {
                continue;
            }
            if let Some(pair) = self.flow.pair_of(naming, src, dst) {
                additions.push((pair.0, pair.1, *op));
            }
        }
        additions.sort_by_key(|(from, to, op)| (*from, *to, op.row.0));
        for (from, to, op) in additions {
            // A dependency row can name a node no row ever declared; such a node is a component of one,
            // and the edge has to be decided with both endpoints present.
            self.partition.add_node(from);
            self.partition.add_node(to);
            let step = self.partition.consider(&self.flow, from, to);
            affected.merge(&step);
            self.flow.apply_addition(op, naming);
        }
        for node in added_nodes.iter().chain(self.flow.nodes().iter()) {
            self.vertices
                .entry(*node)
                .or_insert_with(|| vertex_of(*node, &self.flow, naming));
        }

        // A self-loop going in or out changes the answer without changing the partition, so the component
        // holding it is marked by hand either way — the partition itself cannot see it.
        for node in added_loops.iter().chain(removed_loops.iter()) {
            if let Some(component) = self.partition.component_of(*node) {
                affected.touched.insert(component);
            }
        }

        // -- records: only what the episode could have changed --------------------------------------
        let dictionary = self.dictionary();
        let components: Vec<Component> = self.partition.components().keys().copied().collect();
        for component in components {
            // The components worth a second look are the ones the episode touched and the ones that could
            // hold a cycle and do not have a record yet. "Every component without a record" looks like the
            // same loop and is not: a singleton without a self-loop has no record *permanently*, so that
            // version re-examined all five thousand components on every episode to rediscover that five
            // thousand singletons are still not cycles.
            let new_candidate = !self.records.contains_key(&component)
                && self
                    .partition
                    .members(component)
                    .is_some_and(|members| members.len() > 1);
            if affected.touched.contains(&component) || new_candidate {
                self.refresh(component, &dictionary);
            }
        }
        self.records
            .retain(|component, _| self.partition.members(*component).is_some());

        let after: BTreeMap<String, Cycle> = self
            .records
            .values()
            .map(|cycle| (cycle.key(), cycle.clone()))
            .collect();
        let mut delta = CycleDelta {
            view: self.flow.view(),
            introduced: after
                .keys()
                .filter(|key| !before.contains_key(*key))
                .filter_map(|key| after.get(key).cloned())
                .collect(),
            resolved: before
                .keys()
                .filter(|key| !after.contains_key(*key))
                .filter_map(|key| before.get(key).cloned())
                .collect(),
            changed: Vec::new(),
            unchanged: after.keys().filter(|key| before.contains_key(*key)).count(),
            affected,
            truncated: false,
        };
        delta.pair_growths();
        delta.introduced.sort_by_key(Cycle::key);
        delta.resolved.sort_by_key(Cycle::key);
        if delta.introduced.len() > self.options.max_reported {
            delta.introduced.truncate(self.options.max_reported);
            delta.truncated = true;
        }
        if delta.resolved.len() > self.options.max_reported {
            delta.resolved.truncate(self.options.max_reported);
            delta.truncated = true;
        }
        delta
    }

    /// The health numbers of the current state.
    #[must_use]
    pub fn health(&self) -> Health {
        Health::of(self)
    }

    /// The dictionary as a `Naming`, for the record derivation.
    fn dictionary(&self) -> Naming {
        Naming::from_vertices(self.vertices.values().cloned())
    }

    /// Re-derive one component's record, or drop it if the component is not a cycle.
    fn refresh(&mut self, component: Component, dictionary: &Naming) {
        let Some(members) = self.partition.members(component).cloned() else {
            self.records.remove(&component);
            return;
        };
        let single_with_loop = members.len() == 1
            && members
                .iter()
                .next()
                .is_some_and(|node| self.flow.self_loops().contains(node));
        if members.len() < 2 && !single_with_loop {
            self.records.remove(&component);
            return;
        }
        let found = if single_with_loop {
            // The one-node cycle: the witness is the node itself, and the closing edge is the self-row
            // the flow kept out of the adjacency on purpose.
            Witness {
                nodes: members.iter().copied().collect(),
                exact: true,
            }
        } else {
            witness(&self.flow, dictionary, &members)
        };
        self.records.insert(
            component,
            record(&self.flow, dictionary, component, &members, &found),
        );
    }
}

/// The dictionary entry a record needs for one node: its name in this view, and its layer tag.
fn vertex_of(id: i64, flow: &Flow, naming: &Naming) -> Vertex {
    Vertex {
        id,
        key: flow.name(naming, id),
        kind: match flow.view() {
            View::Module => RowKind::Module,
            View::Symbol => naming
                .vertex(id)
                .map_or(RowKind::Module, |vertex| vertex.kind),
        },
        layer: naming.layer(id).to_string(),
        incomplete: naming.incomplete(id),
    }
}

/// The maintained tracker: one state per view, and the episode step across them.
#[derive(Debug, Clone, Default)]
pub struct Tracker {
    states: BTreeMap<View, State>,
    options: Options,
}

impl Tracker {
    /// Build a tracker over a materialised revision, in every view the options ask for.
    #[must_use]
    pub fn build(graph: &Graph, naming: &Naming, options: Options) -> Self {
        let states = options
            .views()
            .into_iter()
            .map(|view| (view, State::build(graph, naming, options, view)))
            .collect();
        Self { states, options }
    }

    /// The options in force.
    #[must_use]
    pub const fn options(&self) -> &Options {
        &self.options
    }

    /// One view's state.
    #[must_use]
    pub fn state(&self, view: View) -> Option<&State> {
        self.states.get(&view)
    }

    /// Every view the tracker maintains.
    #[must_use]
    pub fn views(&self) -> Vec<View> {
        self.states.keys().copied().collect()
    }

    /// The primary view's cycles.
    #[must_use]
    pub fn cycles(&self) -> Vec<Cycle> {
        self.states
            .get(&self.options.view)
            .map_or_else(Vec::new, State::cycles)
    }

    /// Apply one episode's ops to every maintained view and answer the whole question.
    ///
    /// `ops` is the revision's row ops — the stored `graph_delta` for an adjacent step, or the set
    /// difference between two materialisations for anything else. Removals and additions may arrive in
    /// any order: every removal is processed before every addition, which is what
    /// [`crate::graph::delta`] promises a revision's op order is anyway.
    pub fn episode(
        &mut self,
        ops: &[Op],
        naming: &Naming,
        ratchet: &Ratchet,
        now: Timestamp,
    ) -> Report {
        if ops.is_empty() {
            return self.snapshot(ratchet, now);
        }
        let mut primary = CycleDelta {
            view: self.options.view,
            ..CycleDelta::default()
        };
        let mut symbol: Option<CycleDelta> = None;
        for view in self.views() {
            let Some(state) = self.states.get_mut(&view) else {
                continue;
            };
            let delta = state.apply(ops, naming);
            if view == self.options.view {
                primary = delta;
            } else {
                symbol = Some(delta);
            }
        }
        self.report(primary, symbol, ratchet, now)
    }

    /// The current state with no delta: what this revision's cycles *are*, as opposed to what an episode
    /// did to them. This is what a cold index answers with, and what a surface prints when it has no
    /// earlier revision to compare against.
    #[must_use]
    pub fn snapshot(&self, ratchet: &Ratchet, now: Timestamp) -> Report {
        let delta = CycleDelta {
            view: self.options.view,
            unchanged: self
                .states
                .get(&self.options.view)
                .map_or(0, |state| state.records.len()),
            ..CycleDelta::default()
        };
        self.report(delta, None, ratchet, now)
    }

    /// The primary view's health.
    #[must_use]
    pub fn health(&self) -> Health {
        self.states
            .get(&self.options.view)
            .map_or_else(Health::empty, State::health)
    }

    /// Re-run the full computation and say whether the maintained partitions agree with it.
    ///
    /// `false` means the incremental maintenance has drifted from the definition — a bug in
    /// [`crate::cycles::scc`] and worth failing a test over, not a state a caller can act on, which is
    /// why this is an assertion a test runs rather than a field on a report.
    #[must_use]
    pub fn verify(&self, graph: &Graph, naming: &Naming) -> bool {
        self.states.iter().all(|(view, state)| {
            let fresh = Partition::compute(&Flow::build(graph, naming, *view));
            fresh.signature() == state.partition.signature()
        })
    }

    /// Assemble the answer from the parts.
    fn report(
        &self,
        delta: CycleDelta,
        symbol: Option<CycleDelta>,
        ratchet: &Ratchet,
        now: Timestamp,
    ) -> Report {
        let cycles = self.cycles();
        let verdict = Verdict::assess(&cycles, &delta, ratchet, now);
        let truncated = delta.truncated || symbol.as_ref().is_some_and(|delta| delta.truncated);
        Report {
            health: self.health(),
            verdict,
            delta,
            symbol,
            cycles,
            counters: Counters::of(self),
            truncated,
        }
    }
}

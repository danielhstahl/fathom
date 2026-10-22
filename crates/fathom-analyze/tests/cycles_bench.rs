//! The performance claim: cycle detection after an incremental update, on a five-thousand-node graph,
//! in under 100 ms (`e3.5` / `reco-8md`, AC 2).
//!
//! The unit being measured is one episode — a revision's worth of rows applied to a tracker that already
//! holds the previous revision — because that is the only number the ticket's word "incremental" makes
//! sense about. The cold build is timed alongside it and printed next to it, on purpose: the difference
//! between the two *is* the feature, and a bench that printed only the incremental number could not tell
//! a maintained partition from a recompute that had been renamed.
//!
//! # Why the corpus is synthetic
//!
//! Same argument as `deps_bench`: built in code, the shape is known rather than counted (a graph that
//! quietly lost half its edges would also quietly lose its cycles, and a timing test on it would pass),
//! the size is dialled to the acceptance criterion rather than to whatever a corpus happens to contain,
//! and `cargo test` needs no network or checkout of somebody else's repository.
//!
//! The shape is a spine with a fan across it and one loop parked at the far end — a dependency graph with
//! its debt in one corner. The episode closes a *different* loop near the front, which is the case the
//! incremental rule exists for: the region that changes is three components, and the graph is five
//! thousand nodes.
//!
//! The timings are printed on every run. A wall-clock assertion on a shared runner measures whatever else
//! the runner is doing, so the budget is generous in a debug build (the shipped number is the release
//! one) and the interesting figure is the printed pair, not the boolean.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use fathom_analyze::cycles::{Ratchet, Tracker, View};
use fathom_analyze::graph::model::{Graph, Op, Row, RowKind, Vertex};
use fathom_analyze::graph::query::Naming;
use fathom_core::time::Timestamp;

/// The graph's size, from the acceptance criterion.
const NODES: usize = 5_000;

/// The wall-clock budget: 100 ms shipped, and four times that in a debug build, where every comparator is
/// interpreted and the shipped number is the one that matters.
fn budget_ms() -> u128 {
    if cfg!(debug_assertions) { 400 } else { 100 }
}

/// A revision: interned keys and the rows they make, as small as it can be while still being the graph the
/// lane would hand a cycle pass.
struct Corpus {
    ids: BTreeMap<String, i64>,
    rows: Vec<Row>,
    next: i64,
}

impl Corpus {
    fn new() -> Self {
        Self {
            ids: BTreeMap::new(),
            rows: Vec::new(),
            next: 0,
        }
    }

    fn id(&mut self, key: &str) -> i64 {
        if let Some(id) = self.ids.get(key) {
            return *id;
        }
        let id = self.next;
        self.next += 1;
        self.ids.insert(key.to_string(), id);
        id
    }

    fn module(&mut self, path: &str) {
        let id = self.id(path);
        self.rows.push((RowKind::Module, id, 0));
    }

    fn imports(&mut self, from: &str, to: &str) {
        let (src, dst) = (self.id(from), self.id(to));
        self.rows.push((RowKind::Imports, src, dst));
    }

    fn graph(&self) -> Graph {
        Graph::from_rows(self.rows.iter().copied())
    }

    fn naming(&self) -> Naming {
        Naming::from_vertices(self.ids.iter().map(|(key, id)| Vertex {
            id: *id,
            key: key.clone(),
            kind: RowKind::Module,
            layer: String::new(),
            incomplete: false,
        }))
    }

    /// The rows this corpus has that `previous` does not, removals first — the same shape the lane's
    /// stored delta produces, without the store.
    fn episode_against(&self, previous: &Graph) -> Vec<Op> {
        let before: Vec<Row> = previous.rows().into_iter().collect();
        let mut ops: Vec<Op> = before
            .iter()
            .filter(|row| !self.rows.contains(row))
            .copied()
            .map(Op::remove)
            .collect();
        ops.extend(
            self.rows
                .iter()
                .filter(|row| !before.contains(row))
                .copied()
                .map(Op::add),
        );
        ops
    }
}

/// The repository: a spine, a fan over it, and one loop parked at the far end.
fn corpus() -> Corpus {
    let mut model = Corpus::new();
    let name = |index: usize| format!("m{index:05}.rs");
    for index in 0..NODES {
        model.module(&name(index));
    }
    for index in 0..NODES - 1 {
        model.imports(&name(index), &name(index + 1));
    }
    for index in (0..NODES - 7).step_by(7) {
        model.imports(&name(index), &name(index + 6));
    }
    model.imports(&name(NODES - 3), &name(NODES - 5));
    model
}

/// The instant the debt ledger is argued against; nothing here reads a clock (`fathom_core::time`).
fn now() -> Timestamp {
    Timestamp::from_unix_micros(1_700_000_000_000_000)
}

#[test]
fn an_incremental_cycle_update_on_five_thousand_nodes_stays_under_its_budget() {
    let before = corpus();
    let mut after = Corpus {
        ids: before.ids.clone(),
        rows: before.rows.clone(),
        next: before.next,
    };
    // The episode: one row near the front, closing a loop three nodes away from it.
    after.imports("m00003.rs", "m00001.rs");

    let before_graph = before.graph();
    let naming = after.naming();
    let ops = after.episode_against(&before_graph);
    assert_eq!(ops.len(), 1, "one row, which is what an episode is");

    // Best of three, because a wall-clock assertion on a shared runner is a flake waiting to happen. Each
    // run rebuilds the tracker from the previous revision, which is the honest starting point: a tracker
    // that had already applied the row would answer "nothing changed", and that is not the claim.
    let mut best_update = u128::MAX;
    let mut best_cold = u128::MAX;
    let mut searched = 0;
    for _ in 0..3 {
        let started = Instant::now();
        let mut tracker = Tracker::build(&before_graph, &naming, Default::default());
        let build = started.elapsed().as_millis();

        let started = Instant::now();
        let report = tracker.episode(&ops, &naming, &Ratchet::empty(), now());
        best_update = best_update.min(started.elapsed().as_millis());

        best_cold = best_cold.min(build);
        searched = tracker
            .state(View::Module)
            .map_or(0, |state| state.nodes_searched());

        assert_eq!(report.delta.introduced.len(), 1);
        assert_eq!(report.health.cycles, 2, "the parked loop and the new one");
        assert_eq!(
            tracker
                .state(View::Module)
                .map_or(0, |state| state.full_recomputes()),
            1,
            "the whole-graph pass happened once, at build, and the update did not ask for another"
        );
        assert!(
            tracker.verify(&after.graph(), &naming),
            "and the incrementally maintained components are the components"
        );
    }

    assert!(
        searched < 4 * NODES,
        "the update searched {searched} nodes on a {NODES}-node graph; it was supposed to cost the region"
    );
    assert!(
        best_update < budget_ms(),
        "the incremental update took {best_update} ms on a {NODES}-node graph; the budget is {} ms ({})",
        budget_ms(),
        if cfg!(debug_assertions) {
            "debug build, inflated 4×"
        } else {
            "release"
        }
    );
    eprintln!(
        "cycles: {NODES} nodes, {NODES} modules, one episode — cold build {best_cold} ms, \
         incremental update {best_update} ms (budget {} ms), {searched} nodes searched",
        budget_ms()
    );
}

#[test]
fn a_per_episode_recompute_is_what_the_incremental_path_declined_to_pay() {
    // The comparison the ticket's title turns on, stated as the cost of a *session* rather than of one
    // call: four episodes in a row, once by rebuilding the cycle pass from scratch every episode and once
    // by maintaining it. Both totals are printed even when both pass, because "incremental" is a claim
    // about a ratio, and a ratio nobody can see is a ratio nobody checks.
    let revisions = [
        ("m00003.rs", "m00001.rs"),
        ("m01250.rs", "m01248.rs"),
        ("m02500.rs", "m02498.rs"),
        ("m03750.rs", "m03748.rs"),
    ];
    let base = corpus();
    let naming = base.naming();
    let mut graphs = vec![base.graph()];
    let mut growing = Corpus {
        ids: base.ids.clone(),
        rows: base.rows.clone(),
        next: base.next,
    };
    for (from, to) in &revisions {
        growing.imports(from, to);
        graphs.push(growing.graph());
    }

    // The cold way: one full pass per episode, which is what "recompute per episode" costs.
    let started = Instant::now();
    let mut cold_cycles = 0;
    for graph in &graphs[1..] {
        cold_cycles = Tracker::build(graph, &naming, Default::default())
            .cycles()
            .len();
    }
    let cold = started.elapsed().as_millis();

    // The maintained way: build once at the first revision, then one episode per revision after it.
    let started = Instant::now();
    let build = Instant::now();
    let mut tracker = Tracker::build(&graphs[0], &naming, Default::default());
    eprintln!("   warm build: {} ms", build.elapsed().as_millis());
    let mut warm_cycles = 0;
    for (index, pair) in graphs.windows(2).enumerate() {
        let ops = ops_between(&pair[0], &pair[1]);
        let step = Instant::now();
        warm_cycles = tracker
            .episode(&ops, &naming, &Ratchet::empty(), now())
            .health
            .cycles;
        eprintln!(
            "   episode {index}: {} ms ({} ops)",
            step.elapsed().as_millis(),
            ops.len()
        );
    }
    let warm = started.elapsed().as_millis();

    assert_eq!(
        cold_cycles, warm_cycles,
        "the two ways must agree about how many cycles there are before they can disagree about cost"
    );
    assert!(
        warm < cold,
        "maintaining cost {warm} ms across four episodes and rebuilding cost {cold} ms — the ratio went
         the wrong way, which means the incremental path is no longer cheaper"
    );
    eprintln!(
        "cycles: four episodes over {NODES} nodes — recompute per episode {cold} ms, maintained {warm} ms \
         ({cold} vs {} per episode)",
        warm / 4
    );
}

/// The rows that take one graph to the next, removals first.
///
/// Set-membership rather than `Vec::contains`, because this runs over a whole ten-thousand-row revision:
/// the quadratic version spent most of this bench's time in the harness, which is not the thing under
/// measurement and made the library look thirty times slower than it is.
fn ops_between(before: &Graph, after: &Graph) -> Vec<Op> {
    let earlier: BTreeSet<Row> = before.rows().into_iter().collect();
    let later: BTreeSet<Row> = after.rows().into_iter().collect();
    let mut ops: Vec<Op> = earlier
        .difference(&later)
        .copied()
        .map(Op::remove)
        .collect();
    ops.extend(later.difference(&earlier).copied().map(Op::add));
    ops
}

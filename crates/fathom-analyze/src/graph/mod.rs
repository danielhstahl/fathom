//! `e3.2` (`reco-8s4`) — the per-revision module graph, built, stored as a delta chain, and queried.
//!
//! The ticket's sentence, in full: *"nodes are modules and exported symbols, edges are imports, uses,
//! implementations and inheritance, with layer tags from config. STORED AS s3 MEASURED: one graph
//! DELTA per revision in `graph_delta` plus a shared content-addressed anchor every
//! `graph.chain_full_every = 32` revisions, capped by `graph.query.max_chain_walk = 32`, derived from
//! the blob-keyed per-file structural substrate of `e1.4`/`e3.1` — measured 22 335 B/revision
//! against a 32 KiB budget."* Every clause of that is a module here:
//!
//! | Clause | Where |
//! |---|---|
//! | nodes, edges, the row, the materialised graph | [`model`] |
//! | layer tags from config | [`layers`] |
//! | the knobs `s3` set (`chain_full_every`, `max_chain_walk`) | [`policy`] |
//! | derived from the per-file structural substrate | [`build`] |
//! | one delta per revision | [`delta`] |
//! | the anchor, the chain walk, the store rows | [`lane`] over [`fathom_store::graph`] |
//! | the five questions the panes ask | [`query`] |
//! | the architecture delta, its four costs and its six findings | [`diff`] |
//!
//! # What this deliberately is not
//!
//! **Not a full snapshot per revision.** `s3` measured that at 16 737 373 B/revision against a
//! 32 KiB budget — 681× over, 155.94 GiB for the `medium` class — and it is not the expensive
//! version that lost on a rounding error but the shape that made the spike exist
//! ([`docs/concepts/graph-storage.md`](../../../docs/concepts/graph-storage.md)). Per-revision
//! membership lists are banned outright by the same measurement, which is why nothing in
//! [`lane`] writes one.
//!
//! **Not symbol-level call edges.** The graph is module-granularity edges over module +
//! exported-symbol nodes, which is the granularity the ticket implies and the one `s3` measured. A
//! caller→callee graph would be an order of magnitude larger per revision and would move every byte
//! number here by that factor; choosing it would mean re-running the spike, not extending this
//! module.
//!
//! # The three things §4.6 got wrong, and where they live
//!
//! `s3` found all three by writing into the DDL instead of reading it. They are mechanisms here, not
//! comments: the node op's `dst = ''` in [`fathom_store::DeltaOp::add_node`] and in the lane's
//! writer, the anchor manifest's own `graph_chunk` row in `Lane::write_anchor`, and the "two
//! commits, one tree" rule in [`fathom_store::Store::graph_write_snapshot`] — which is `INSERT
//! … ON CONFLICT DO NOTHING` rather than `REPLACE`, because the chain is walked and `REPLACE` would
//! rewrite a `base_tree` that a walk is already relying on.
//!
//! # Interning, which `s3` projected and did not bank
//!
//! `s3` named node-id interning as "should land near a third of the bytes" and deliberately spent the
//! headroom on nothing. [`model::Interner`] and [`fathom_store::Store::graph_intern`] bank it: the
//! delta rows carry small integers instead of 40-hex tree oids and path strings, and the dictionary
//! that turns them back into paths is `graph_node`. The bytes are reported as a measurement in
//! `benchmarks/reports/e3-2-graph-builder.json`, not as a projection.
//!
//! # Determinism
//!
//! Nothing here reads a clock, a filesystem, or an environment variable. A revision is a tree oid, a
//! parent tree oid, and a list of `(path, blob, structure)`; the ids that come out depend on the
//! order the list is walked, which is why every walk is sorted by path and why [`Lane::rebuild`]
//! compares by *key* rather than by id. A revision rebuilt from an empty cache yields the same graph
//! — the acceptance criterion, and the test that says so is `crates/fathom-analyze/tests/graph.rs`.
//!
//! # Where the numbers live
//!
//! `crates/fathom-analyze/examples/graph_harness.rs` is the bench: a 1 000-commit fixture, the
//! cold/marginal write costs, the bytes per revision against
//! `store.bytes_per_commit_graph`, the query latencies at 5 000 nodes, the materialisation latency,
//! and the delta-apply claim. Its committed report is
//! `benchmarks/reports/e3-2-graph-builder.json`, `--check` re-measures it in CI, and
//! `tests/test_graph_builder_contract.py` keeps the report, the budgets and this documentation
//! telling the same story.

pub mod build;
pub mod delta;
pub mod diff;
pub mod layers;
pub mod model;
pub mod policy;
pub mod query;

/// The store-facing lane: writing revisions and reading them back. Present only when a SQLite
/// backend is compiled in, exactly like [`crate::index`] — a lane with nowhere to store its rows is
/// not a lane.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod lane;

pub use crate::graph::build::{BuildOutcome, BuildSummary, Builder, ModuleInput, ModuleRows};
pub use crate::graph::delta::{EdgeDelta, changed_modules, module_key, module_ops, revision_ops};
pub use crate::graph::diff::{
    ArchitectureDelta, Band, CONTRACT, ChurnReport, Cost, EdgeFact, HOT_MODULES, LayerDrift,
    ModuleChurn, Move, MoveShape, NodeFact, Repoint, Reversal, RevisionOps, Rung, SurfaceChange,
    TaggedOp, compose, ops_between,
};
pub use crate::graph::layers::Layers;
pub use crate::graph::model::{
    FreshEntry, Graph, Interner, Op, Row, RowKind, SCHEMA_VERSION, Vertex, decode_rows, encode_rows,
};
pub use crate::graph::policy::{
    DEFAULT_CHAIN_FULL_EVERY, DEFAULT_IGNORED_DIRS, DEFAULT_MAX_CHAIN_WALK, DEFAULT_PATH_MAX_HOPS,
    GraphPolicy,
};
pub use crate::graph::query::{
    Fan, Naming, PathAnswer, Subgraph, dependencies_of, dependents_of, layer_subgraph, module_of,
    path,
};

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub use crate::graph::lane::{
    Cursor, Lane, MaterialiseReport, RecordSummary, Step, StepKind, StoredOps,
};

/// The policy and the layer tags, read out of a `fathom.toml` document in one go.
///
/// The two config surfaces belong together because one graph row is a function of both: the knobs
/// decide how it is stored and the `[layers]` table decides what its endpoints are tagged as, so a
/// caller that reads one and forgets the other has built a graph with half its config.
///
/// The notices are what a surface prints about anything it refused: a typo in a config file is a
/// warning and a default, never a lost architecture view.
#[must_use]
pub fn builder_from_config_text(text: &str) -> (Builder, Vec<String>) {
    let (policy, mut notices) = GraphPolicy::from_repo_config_text(text);
    let (layers, layer_notices) = Layers::from_repo_config_text(text);
    notices.extend(layer_notices);
    notices.extend(policy.notices());
    (Builder { policy, layers }, notices)
}

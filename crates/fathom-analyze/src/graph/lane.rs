//! The lane: writing one revision's graph, and reading one back.
//!
//! This is where `s3`'s policy becomes code. Three paths, and the ticket's acceptance criteria are
//! each about one of them:
//!
//! * [`Lane::record`] — the marginal write. A commit arrives with its tree, its parent's tree and the
//!   whole live file list; the lane compares each file's **blob** against what it already holds and
//!   re-encodes rows only for the ones that moved. That is the blob-keyed substrate doing the change
//!   test, and it is why the marginal cost is O(changed files) rather than O(live modules) — the
//!   50 ms-per-new-commit budget is spent here or not at all.
//! * [`Lane::materialise`] — the cold read: walk `base_tree` back to an anchor within
//!   `graph.query.max_chain_walk`, load the anchor's manifest, apply ≤ `chain_full_every` deltas.
//!   `graph.materialise_revision.ms` is this path's budget, and `s3` named it the cost that survived
//!   the spike.
//! * [`Lane::step`] — the warm read: the cursor moved one commit and a graph is in hand, so the work
//!   is one delta apply. When it cannot be — the revision is an anchor, or its base is not what is
//!   held — the outcome *says which of the three paths it took* rather than quietly rebuilding,
//!   because "the cursor moving one commit is a delta apply, not a rebuild" is an acceptance
//!   criterion and therefore has to be observable, not merely true today.
//!
//! # The anchor is a manifest, not a snapshot
//!
//! Writing an anchor costs one manifest blob (the content address of every live module's chunk) plus
//! only the chunks that are new. That is the difference between "an anchor costs O(graph) rows" and
//! "an anchor costs one row and O(graph) *shared* bytes", and `s3` measured it as the largest lever
//! it found. The manifest also gets its own `graph_chunk` row, because `graph_chunk_link`'s foreign
//! key reaches `graph_chunk` and not `blob` — `s3`'s second schema finding, and the reason the write
//! order here is chunk row, then link, then nothing else.
//!
//! # No clock in here
//!
//! [`fathom_core::time`]'s rule is that analyses do not read a clock: the caller supplies `now` to
//! [`Lane::record`]. That is what makes a rebuilt store comparable to the original byte for byte,
//! which is what the reproducibility criterion then asserts.

use std::collections::BTreeMap;

use fathom_store::error::Error;
use fathom_store::graph::{
    ANCHOR_SLOT, CHUNK_KIND_ADJACENCY, CHUNK_KIND_MANIFEST, ChainPartial, ChainWalk, DeltaOp,
    GraphWalkError, NodeEntry, Op as StoreOp, SnapshotRow, StoreBytes,
};
use fathom_store::spine::Store;

use crate::graph::build::BuildSummary;
use crate::graph::build::{BuildOutcome, Builder, ModuleInput, ModuleRows};
use crate::graph::delta::{EdgeDelta, changed_modules, module_key, revision_ops};
use crate::graph::model::{
    FreshEntry, Graph, Interner, Op, Row, RowKind, Vertex, decode_rows, encode_rows,
};
use crate::graph::query::Naming;

/// The cursor's state: one revision, materialised, with the per-module rows a delta is computed from.
#[derive(Debug, Clone)]
pub struct Cursor {
    /// The revision this graph is the graph of.
    pub tree_oid: String,
    /// The live modules and their rows — the lane's working set, and the substrate's mirror.
    pub modules: BTreeMap<String, ModuleRows>,
    /// The materialised graph, ready for a query without another read.
    pub graph: Graph,
}

/// What writing one revision cost.
#[derive(Debug, Clone, Default)]
pub struct RecordSummary {
    /// The revision written.
    pub tree_oid: String,
    /// Whether this revision is an anchor.
    pub is_anchor: bool,
    /// Ops written for it (zero for an anchor, which writes a manifest instead).
    pub ops: usize,
    /// Rows actually inserted.
    pub rows_written: usize,
    /// Modules whose rows changed — the lane's real unit of work.
    pub changed_modules: usize,
    /// New chunk payloads written (an anchor's shared half).
    pub new_chunks: usize,
    /// Manifest bytes, for an anchor.
    pub manifest_bytes: usize,
    /// Nodes in the revision's graph.
    pub nodes: usize,
    /// Edges in the revision's graph.
    pub edges: usize,
    /// Whether the tree was already stored, so nothing was written (rough edge 3).
    pub duplicate_tree: bool,
    /// What the build measured about the revision itself.
    pub build: BuildSummary,
}

/// One materialisation's account, so a pane can say what it paid.
#[derive(Debug, Clone, Default)]
pub struct MaterialiseReport {
    /// The anchor the reconstruction started from.
    pub anchor: String,
    /// Deltas replayed after it.
    pub deltas: usize,
    /// `base_tree` steps walked.
    pub steps: usize,
    /// Rows decoded from chunk payloads.
    pub chunk_rows: usize,
    /// Chunk or manifest lines that did not decode — a stated partial, never a silently smaller graph.
    pub undecoded: usize,
    /// Ops applied from the deltas.
    pub ops_applied: usize,
}

impl MaterialiseReport {
    /// The sentence for a pane's graph line.
    #[must_use]
    pub fn sentence(&self) -> String {
        let base = format!(
            "materialised from anchor {} + {} delta(s): {} row(s) from chunks, {} op(s) applied",
            self.anchor, self.deltas, self.chunk_rows, self.ops_applied
        );
        if self.undecoded == 0 {
            base
        } else {
            format!("{base}, {} line(s) unreadable", self.undecoded)
        }
    }
}

/// How the cursor got to the revision it was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// One delta applied: the interactive case, and the cheapest thing this lane does.
    DeltaApplied,
    /// The revision is an anchor, so there was nothing to apply and its manifest was loaded instead.
    AnchorLoaded,
    /// The revision is not adjacent to what was in hand, so it was reconstructed. `s3` measured that
    /// difference at 0.381 ms against 204.6 ms p95, which is why it is reported rather than hidden
    /// inside a function that "just returns a graph".
    Rebuilt,
}

/// What the lane did to move a cursor.
#[derive(Debug, Clone)]
pub struct Step {
    /// Which of the three paths it took.
    pub kind: StepKind,
    /// Ops applied, when it was a delta.
    pub ops: usize,
}

/// The store-facing lane.
pub struct Lane<'a> {
    /// Where the rows go.
    pub store: &'a mut Store,
    /// The knobs and layer tags the build runs under.
    pub builder: Builder,
    /// The id dictionary, seeded from the store and the authority on new ids until they are written.
    interner: Interner,
    /// Each interned id's layer tag, maintained alongside the dictionary: delta rows carry their
    /// endpoints' layers so a layering question can be answered from the delta without a rebuild.
    layers: BTreeMap<i64, String>,
    /// The ops staged for the revision being written, between `assemble` and the write.
    staged: Vec<DeltaOp>,
    /// How many modules changed in the revision being written, between `assemble` and the summary.
    staged_changed: usize,
    /// The last revision written or read, if any.
    pub cursor: Option<Cursor>,
    /// Deltas written since the last anchor.
    depth: usize,
    /// Revisions written, which is the denominator for bytes-per-revision.
    revisions: usize,
    /// Anchors written.
    anchors: usize,
}

impl<'a> Lane<'a> {
    /// A lane over an open store, with the dictionary already in it loaded into the interner.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the dictionary cannot be read.
    pub fn new(store: &'a mut Store, builder: Builder) -> Result<Self, Error> {
        let nodes = store.graph_nodes()?;
        let interner = Interner::seeded(nodes.values().map(|node| (node.key.as_str(), node.id)));
        let layers = nodes
            .iter()
            .map(|(id, node)| (*id, node.layer.clone()))
            .collect();
        Ok(Self {
            store,
            builder,
            interner,
            layers,
            staged: Vec::new(),
            staged_changed: 0,
            cursor: None,
            depth: 0,
            revisions: 0,
            anchors: 0,
        })
    }

    /// How many revisions this lane has written.
    #[must_use]
    pub fn revisions(&self) -> usize {
        self.revisions
    }

    /// The store behind the lane, read-only.
    ///
    /// A surface that has a lane should not have to keep a second handle to the same store in order
    /// to ask it a question the lane does not wrap.
    #[must_use]
    pub fn read(&self) -> &Store {
        self.store
    }

    /// How many anchors it has written.
    #[must_use]
    pub fn anchors(&self) -> usize {
        self.anchors
    }

    /// The dictionary as a naming view, read from the store — the authoritative copy, including the
    /// `incomplete` flags a rebuild needs and a lane-side mint does not carry.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the dictionary cannot be read.
    pub fn naming(&self) -> Result<Naming, Error> {
        Ok(Naming::from_vertices(
            self.store.graph_nodes()?.into_values().map(|node| Vertex {
                id: node.id,
                key: node.key,
                kind: RowKind::parse(&node.kind).unwrap_or(RowKind::Module),
                layer: node.layer,
                incomplete: node.incomplete,
            }),
        ))
    }

    /// Write one revision: an anchor every `graph.chain_full_every`, a delta in between.
    ///
    /// `inputs` is the whole live file list — the tree listing, not the commit's diff — because an
    /// anchor needs every live module and because deciding *which* modules changed is the lane's job,
    /// not the caller's. A module whose blob is unchanged is not even re-encoded.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] on any write failure, including a delta written before its snapshot
    /// row (a foreign-key violation, which is a lane bug and is meant to be loud).
    pub fn record(
        &mut self,
        tree_oid: &str,
        base_tree: Option<&str>,
        inputs: &[ModuleInput<'_>],
        now: i64,
    ) -> Result<RecordSummary, Error> {
        let outcome = self.assemble(tree_oid, base_tree, inputs)?;
        // An anchor is due when the *next* delta would put the chain past the period, so the number
        // of deltas between two anchors is `chain_full_every - 1` and a chain walk of
        // `chain_full_every` steps always reaches one. Off-by-one here is what makes a walk cap and an
        // anchor period the same number stop meaning the same thing.
        let anchored =
            base_tree.is_none() || self.depth + 1 >= self.builder.policy.chain_full_every;
        let mut snap = match (anchored, base_tree) {
            (true, _) | (false, None) => SnapshotRow::anchor(tree_oid),
            (false, Some(base)) => SnapshotRow::delta(
                tree_oid,
                base,
                i64::try_from(self.depth + 1).unwrap_or(i64::MAX),
            ),
        };
        snap.node_count = i64::try_from(outcome.graph.node_count()).unwrap_or(i64::MAX);
        snap.edge_count = i64::try_from(outcome.graph.edge_count()).unwrap_or(i64::MAX);
        snap.analyzer_version = i64::from(crate::graph::model::SCHEMA_VERSION);
        snap.built_utc = now;

        let written = self.store.graph_write_snapshot(&snap)?;
        let mut summary = RecordSummary {
            tree_oid: tree_oid.to_string(),
            is_anchor: anchored,
            nodes: outcome.graph.node_count(),
            edges: outcome.graph.edge_count(),
            duplicate_tree: !written,
            build: outcome.summary,
            ..RecordSummary::default()
        };
        if anchored {
            self.depth = 0;
            self.anchors += 1;
            let (new_chunks, manifest_bytes) = self.write_anchor(tree_oid, now)?;
            summary.new_chunks = new_chunks;
            summary.manifest_bytes = manifest_bytes;
        } else {
            self.depth += 1;
            let ops = std::mem::take(&mut self.staged);
            summary.ops = ops.len();
            summary.rows_written = self.store.graph_write_ops(tree_oid, &ops)?;
        }
        summary.changed_modules = self.staged_changed;
        self.revisions += 1;
        Ok(summary)
    }

    /// Recompute a revision in full, from nothing — the path a rebuild from an empty cache takes.
    ///
    /// The reproducibility claim is checked between this and [`Lane::record`]: the rows the two paths
    /// produce must be equal **compared by key**. Interned ids are minted in first-sighting order, so
    /// two orders of the same facts mint different numbers for the same thing; a test that compared
    /// ids would be comparing the order, not the graph.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the dictionary write fails.
    pub fn rebuild(&mut self, inputs: &[ModuleInput<'_>]) -> Result<BuildOutcome, Error> {
        self.interner = Interner::default();
        self.layers.clear();
        let outcome = self.builder.build(inputs, &mut self.interner);
        self.flush_interner()?;
        self.cursor = Some(Cursor {
            tree_oid: String::new(),
            modules: outcome.modules.clone(),
            graph: outcome.graph.clone(),
        });
        Ok(outcome)
    }

    /// Load a revision's graph: anchor, then ≤ `max_chain_walk` deltas.
    ///
    /// # Errors
    ///
    /// [`ChainPartial`] when no anchor is reachable — a stated partial whose
    /// [`ChainPartial::sentence`] is the answer a pane prints.
    pub fn materialise(&self, tree_oid: &str) -> Result<(Graph, MaterialiseReport), ChainPartial> {
        let walk = match self
            .store
            .graph_chain_walk(tree_oid, self.builder.policy.max_chain_walk)
        {
            Ok(walk) => walk,
            Err(GraphWalkError::Partial(partial)) => return Err(partial),
            // A store that cannot be read is not a chain that is long; the caller's vocabulary is the
            // partial, so this collapses to the anchor-lost shape with the target named.
            Err(GraphWalkError::Store(_)) => {
                return Err(ChainPartial::AnchorLost {
                    tree_oid: tree_oid.to_string(),
                    steps: 0,
                });
            }
        };
        let (mut graph, mut report) = self.read_anchor(&walk)?;
        report.anchor = walk.anchor.clone();
        report.steps = walk.steps;
        for tree in &walk.deltas {
            let Ok(ops) = self.store.graph_ops(tree) else {
                continue;
            };
            report.ops_applied += ops.len();
            graph.apply(ops.iter().filter_map(from_store_op));
        }
        report.deltas = walk.deltas.len();
        Ok((graph, report))
    }

    /// Move a cursor one commit. The warm path, and the one a keystroke pays.
    ///
    /// # Errors
    ///
    /// [`ChainPartial`] when the target cannot be reconstructed at all.
    pub fn step(&self, cursor: &Cursor, tree_oid: &str) -> Result<(Cursor, Step), ChainPartial> {
        let Some(snap) =
            self.store
                .graph_snapshot(tree_oid)
                .map_err(|_| ChainPartial::AnchorLost {
                    tree_oid: tree_oid.to_string(),
                    steps: 0,
                })?
        else {
            return Err(ChainPartial::AnchorLost {
                tree_oid: tree_oid.to_string(),
                steps: 0,
            });
        };
        if snap.kind == "full" {
            let (graph, _) = self.materialise(tree_oid)?;
            let built = self.cursor_with(tree_oid, graph);
            return Ok((
                built,
                Step {
                    kind: StepKind::AnchorLoaded,
                    ops: 0,
                },
            ));
        }
        if snap.base_tree.as_deref() == Some(cursor.tree_oid.as_str()) {
            let ops = self
                .store
                .graph_ops(tree_oid)
                .map_err(|_| ChainPartial::AnchorLost {
                    tree_oid: tree_oid.to_string(),
                    steps: 0,
                })?;
            let mut graph = cursor.graph.clone();
            graph.apply(ops.iter().filter_map(from_store_op));
            let n = ops.len();
            let built = self.cursor_with(tree_oid, graph);
            return Ok((
                built,
                Step {
                    kind: StepKind::DeltaApplied,
                    ops: n,
                },
            ));
        }
        let (graph, _) = self.materialise(tree_oid)?;
        let built = self.cursor_with(tree_oid, graph);
        Ok((
            built,
            Step {
                kind: StepKind::Rebuilt,
                ops: 0,
            },
        ))
    }

    /// The edges between two revisions — `e3.3`'s question, at whichever cost the store can answer it.
    ///
    /// Adjacent means the child's own delta rows, which is one indexed query. Anything else is two
    /// materialisations and a set difference; `s3` measured that difference at roughly a thousand
    /// times the price, so the answer carries `from_store` and no surface can present the expensive
    /// one as the cheap one.
    ///
    /// # Errors
    ///
    /// [`ChainPartial`] when either revision cannot be reconstructed.
    pub fn changed_edges(&self, from: &str, to: &str) -> Result<EdgeDelta, ChainPartial> {
        if let Ok(Some(snap)) = self.store.graph_snapshot(to)
            && snap.kind == "delta"
            && snap.base_tree.as_deref() == Some(from)
            && let Ok(ops) = self.store.graph_ops(to)
        {
            let model: Vec<Op> = ops.iter().filter_map(from_store_op).collect();
            let mut delta = EdgeDelta::from_ops(&model);
            delta.from_store = true;
            return Ok(delta);
        }
        let (before, _) = self.materialise(from)?;
        let (after, _) = self.materialise(to)?;
        Ok(EdgeDelta::between(&before, &after))
    }

    /// The graph's own bytes on the payload basis — the basis `s3` measured, and therefore the basis
    /// `store.bytes_per_commit_graph.*` means.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a count fails.
    pub fn graph_bytes(&self) -> Result<fathom_store::graph::GraphBytes, Error> {
        self.store.graph_payload_bytes()
    }

    /// The store's bytes, with the basis labelled — the number every byte claim in this ticket is
    /// checked against (§8.4).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the accounting fails.
    pub fn usage(&self) -> Result<StoreBytes, Error> {
        self.store.graph_usage()
    }

    /// The dictionary's layer tags, for a caller that wants them without another read.
    #[must_use]
    pub fn layers(&self) -> &BTreeMap<i64, String> {
        &self.layers
    }

    /// Flush freshly-minted dictionary entries into the store, and learn their layer tags here.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn flush_interner(&mut self) -> Result<(), Error> {
        let fresh: Vec<FreshEntry> = self.interner.fresh().to_vec();
        if fresh.is_empty() {
            return Ok(());
        }
        let entries: Vec<NodeEntry> = fresh
            .iter()
            .map(|entry| NodeEntry {
                key: entry.key.clone(),
                id: Some(entry.id),
                kind: entry.kind,
                layer: entry.layer.clone(),
                incomplete: entry.incomplete,
            })
            .collect();
        self.store.graph_intern(&entries)?;
        for entry in &fresh {
            self.layers.insert(entry.id, entry.layer.clone());
        }
        self.interner.mark_flushed();
        Ok(())
    }

    // -- internals ------------------------------------------------------------

    /// Compute one revision's rows against the cursor, stage its ops, and move the cursor.
    fn assemble(
        &mut self,
        tree_oid: &str,
        base_tree: Option<&str>,
        inputs: &[ModuleInput<'_>],
    ) -> Result<BuildOutcome, Error> {
        let previous = self.cursor.take();
        let stale = match &previous {
            Some(cursor) => base_tree.is_none_or(|base| base != cursor.tree_oid),
            None => true,
        };
        let outcome = match (&previous, stale) {
            (Some(cursor), false) => self.build_changed(cursor, inputs)?,
            _ => self.builder.build(inputs, &mut self.interner),
        };
        let ops = match &previous {
            Some(before) => revision_ops(&before.modules, &outcome.modules),
            None => outcome
                .modules
                .values()
                .flat_map(|module| module.rows.iter().copied())
                .map(Op::add)
                .collect(),
        };
        self.staged = self.to_store_ops(&ops);
        self.flush_interner()?;
        self.staged_changed = previous.map_or(outcome.modules.len(), |before| {
            changed_modules(&before.modules, &outcome.modules).len()
        });
        self.cursor = Some(Cursor {
            tree_oid: tree_oid.to_string(),
            modules: outcome.modules.clone(),
            graph: outcome.graph.clone(),
        });
        Ok(outcome)
    }

    /// Re-encode only the modules whose blob moved, keeping the rest of the cursor's rows.
    fn build_changed(
        &mut self,
        cursor: &Cursor,
        inputs: &[ModuleInput<'_>],
    ) -> Result<BuildOutcome, Error> {
        let mut modules: BTreeMap<String, ModuleRows> = BTreeMap::new();
        let mut summary = BuildSummary::default();
        let mut ordered: Vec<&ModuleInput<'_>> = inputs.iter().collect();
        ordered.sort_by(|a, b| a.path.cmp(b.path));
        for input in ordered {
            summary.modules += 1;
            if self.builder.policy.ignores(input.path) {
                summary.ignored += 1;
                continue;
            }
            let path = module_key(input.path);
            if let Some(known) = cursor.modules.get(&path)
                && known.blob == input.blob
            {
                // The substrate says nothing moved, so nothing is re-encoded and no op is
                // produced. This branch *is* the marginal-cost budget.
                modules.insert(path, known.clone());
                continue;
            }
            let Some(mut fresh) = self.builder.module_rows(input, &mut self.interner) else {
                summary.ignored += 1;
                continue;
            };
            fresh.rows.sort_unstable();
            fresh.rows.dedup();
            if fresh.unavailable.is_some() {
                summary.unavailable += 1;
            }
            if fresh.incomplete.is_some() {
                summary.incomplete += 1;
            }
            summary.external += fresh.external;
            summary.wildcard += fresh.wildcard;
            modules.insert(path, fresh);
        }
        let rows: Vec<Row> = modules.values().flat_map(|m| m.rows.clone()).collect();
        let graph = Graph::from_rows(rows);
        summary.nodes = graph.node_count();
        summary.edges = graph.edge_count();
        summary.dangling = graph.dangling().len();
        Ok(BuildOutcome {
            graph,
            modules,
            summary,
        })
    }

    /// Write an anchor: one chunk per live module (deduped by content, because content is the
    /// address) plus the manifest that names them.
    fn write_anchor(&mut self, tree_oid: &str, now: i64) -> Result<(usize, usize), Error> {
        let Some(cursor) = self.cursor.clone() else {
            return Ok((0, 0));
        };
        let mut new_chunks = 0_usize;
        let mut manifest = String::new();
        for module in cursor.modules.values() {
            let payload = encode_rows(&module.rows);
            let (sha, bytes, _storage) = self.store.put_blob("graph_chunk", &payload, now)?;
            self.store.graph_write_chunk(
                &sha,
                CHUNK_KIND_ADJACENCY,
                i64::try_from(bytes).unwrap_or(i64::MAX),
            )?;
            new_chunks += 1;
            manifest.push_str(&format!("{}\t{sha}\n", module.node));
        }
        let manifest_bytes = manifest.as_bytes();
        let (manifest_sha, manifest_len, _) =
            self.store.put_blob("graph_manifest", manifest_bytes, now)?;
        // `s3`'s finding 2, made structural: the link's foreign key reaches `graph_chunk`, so the
        // manifest — which is a blob — needs its chunk row before it can be linked.
        self.store.graph_write_chunk(
            &manifest_sha,
            CHUNK_KIND_MANIFEST,
            i64::try_from(manifest_len).unwrap_or(i64::MAX),
        )?;
        self.store
            .graph_link_chunk(tree_oid, ANCHOR_SLOT, &manifest_sha)?;
        Ok((
            new_chunks,
            usize::try_from(manifest_len).unwrap_or(usize::MAX),
        ))
    }

    /// Convert staged ops into the stored shape, stamping each endpoint's layer tag.
    fn to_store_ops(&self, ops: &[Op]) -> Vec<DeltaOp> {
        ops.iter()
            .map(|op| {
                let (kind, src, dst) = op.row;
                DeltaOp {
                    op: if op.add {
                        StoreOp::Add
                    } else {
                        StoreOp::Remove
                    },
                    kind: kind.as_str().to_string(),
                    src: src.to_string(),
                    // Rough edge 1: a node op has no object, and the only legal spelling of that is
                    // the empty string, because `dst` is in the primary key and is not nullable.
                    dst: if kind.is_node() {
                        String::new()
                    } else {
                        dst.to_string()
                    },
                    src_layer: self.layers.get(&src).cloned().unwrap_or_default(),
                    dst_layer: if kind.is_node() {
                        String::new()
                    } else {
                        self.layers.get(&dst).cloned().unwrap_or_default()
                    },
                }
            })
            .collect()
    }

    /// A cursor for `tree_oid` over a freshly-read graph, with module row sets rebuilt from it.
    fn cursor_with(&self, tree_oid: &str, graph: Graph) -> Cursor {
        Cursor {
            tree_oid: tree_oid.to_string(),
            modules: self.modules_of(&graph),
            graph,
        }
    }

    /// Rebuild the per-module row sets from a materialised graph, so a stepped cursor can be stepped
    /// again — and so a delta can be computed against a graph the lane read rather than wrote.
    fn modules_of(&self, graph: &Graph) -> BTreeMap<String, ModuleRows> {
        let mut modules: BTreeMap<String, ModuleRows> = BTreeMap::new();
        let blank = |path: String, node: i64| ModuleRows {
            layer: self.layers.get(&node).cloned().unwrap_or_default(),
            symbols: Vec::new(),
            rows: vec![(RowKind::Module, node, 0)],
            path,
            blob: String::new(),
            node,
            incomplete: None,
            unavailable: None,
            external: 0,
            wildcard: 0,
        };
        for (id, kind) in graph.nodes() {
            if *kind != RowKind::Module {
                continue;
            }
            let Some(key) = self.interner.key_of(*id) else {
                continue;
            };
            let path = module_key(key);
            modules.insert(path.clone(), blank(path, *id));
        }
        for row in graph.edges() {
            let (kind, src, dst) = *row;
            // Every row is owned by the module its source belongs to: a node row's source *is*
            // the module, and an edge row's source is the module that declared the dependency.
            let Some(key) = self.interner.key_of(src) else {
                continue;
            };
            let path = module_key(key);
            modules
                .entry(path.clone())
                .or_insert_with(|| blank(path, src))
                .rows
                .push((kind, src, dst));
        }
        for module in modules.values_mut() {
            module.rows.sort_unstable();
            module.rows.dedup();
        }
        modules
    }

    /// The anchor half of the cold read: manifest in, rows out.
    fn read_anchor(&self, walk: &ChainWalk) -> Result<(Graph, MaterialiseReport), ChainPartial> {
        let mut report = MaterialiseReport::default();
        let manifest = self
            .store
            .graph_anchor_manifest(&walk.anchor)
            .map_err(|_| ChainPartial::AnchorLost {
                tree_oid: walk.anchor.clone(),
                steps: walk.steps,
            })?
            .ok_or_else(|| ChainPartial::AnchorLost {
                tree_oid: walk.anchor.clone(),
                steps: walk.steps,
            })?;
        let mut shas: Vec<String> = Vec::new();
        for line in String::from_utf8_lossy(&manifest).lines() {
            let mut parts = line.split('\t');
            let (Some(id), Some(sha)) = (parts.next(), parts.next()) else {
                report.undecoded += 1;
                continue;
            };
            if id.parse::<i64>().is_err() {
                report.undecoded += 1;
                continue;
            }
            shas.push(sha.to_string());
        }
        let mut graph = Graph::default();
        let mut rows: Vec<Row> = Vec::new();
        for sha in shas {
            let payload = match self.store.read_blob(&sha) {
                Ok(Some(payload)) => payload,
                // A manifest naming a blob that is gone is a broken anchor, not a smaller graph.
                _ => {
                    report.undecoded += 1;
                    continue;
                }
            };
            let (decoded, skipped) = decode_rows(&payload);
            report.undecoded += skipped;
            report.chunk_rows += decoded.len();
            rows.extend(decoded);
        }
        graph.apply(rows.into_iter().map(Op::add));
        Ok((graph, report))
    }
}

/// The row a stored op names, or `None` when its endpoints are not numbers (a foreign row).
fn row_of(op: &DeltaOp) -> Option<Row> {
    let kind = RowKind::parse(&op.kind)?;
    let src = op.src.parse::<i64>().ok()?;
    let dst = if op.dst.is_empty() {
        0
    } else {
        op.dst.parse::<i64>().ok()?
    };
    Some((kind, src, dst))
}

/// The op a stored row is, or `None` when it cannot be read.
fn from_store_op(op: &DeltaOp) -> Option<Op> {
    row_of(op).map(|row| Op {
        add: op.op == StoreOp::Add,
        row,
    })
}

//! `graph.rs` — migration 8: the module graph, stored as a delta chain (`e3.2`, `reco-8s4`).
//!
//! ADR-004 §4.6 shaped four tables for this and §14.1 refused to choose between them until spike
//! `s3` had written all five candidates into the DDL and measured them
//! ([`docs/concepts/graph-storage.md`](../../../docs/concepts/graph-storage.md)). The policy `s3`
//! chose — and the one this module implements — is:
//!
//! > one `graph_delta` row set per revision holding **only the ops the commit caused**, plus one
//! > shared content-addressed **anchor** every `graph.chain_full_every = 32` revisions, on top of
//! > the blob-keyed per-file substrate `e1.4` already keeps.
//!
//! Measured 22 335 B/revision against the 32 KiB budget; a full snapshot per revision measured
//! 16 737 373 B/revision, and content-defined chunking — whose *payloads* are shared but whose
//! per-revision *membership list* is not — measured 1 089 412 B/revision. So the two things this
//! module is not allowed to do are stated plainly: **it never restates an unchanged module**, and
//! **it never writes a per-revision membership list**.
//!
//! # The three things §4.6 got wrong, as mechanisms
//!
//! `s3` found all three by writing into the schema rather than reading it, and each one is a
//! mechanism here rather than a comment someone hopes gets read:
//!
//! 1. **A node op carries `dst = ''`** ([`DeltaOp::add_node`]). `(src, dst)` is not nullable and
//!    `dst` is in the primary key, so a node add has no honest spelling other than the empty
//!    string. It is *not* a wildcard, *not* self, and *not* a deletion of every edge: a reader that
//!    invents a second spelling gets a second meaning for the same row. [`DeltaOp::is_node_op`] is
//!    the one predicate that knows.
//! 2. **An anchor's manifest gets a `graph_chunk` row of its own** (`kind='manifest'`, written by
//!    [`Store::graph_write_chunk`] before [`Store::graph_link_chunk`] runs). `graph_chunk_link`'s
//!    foreign key reaches `graph_chunk`, not `blob`, so without that row the anchor's insert is
//!    rejected — and the anchor is the one row the whole chain walk depends on.
//! 3. **A tree-keyed store cannot hold two rows for one tree.** [`Store::graph_write_snapshot`] is
//!    `INSERT … ON CONFLICT DO NOTHING`, never `INSERT OR REPLACE`: `REPLACE` would rewrite the
//!    first row's `base_tree`, and the chain is *walked*, not patched, so a patched chain walks
//!    somewhere else. A second commit reaching an already-stored tree is counted
//!    (`graph.snapshot.duplicate_tree`) and the first row is kept — `D16`'s upsert rule with a
//!    worse failure mode if broken.
//!
//! # Interning: the byte reduction `s3` projected and did not bank
//!
//! `s3`'s report says the prototype's rows carry "the tree OID as 40 hex characters and each
//! endpoint as a path string", and that a `WITHOUT ROWID` integer node dictionary "should land near
//! a third of the bytes" — named as a projection, deliberately not spent. `graph_node` is that
//! dictionary and [`Store::graph_intern`] is the interning: `src` and `dst` hold a
//! `graph_node.node_id` rendered as decimal text rather than a path.
//!
//! Note what that does *not* change: §4.6's DDL. `src` and `dst` are `TEXT NOT NULL` in ADR-004
//! Appendix A, so interning is a change of *vocabulary inside the column*, not a migration — which
//! is the only way the ADR's "no DDL changed" claim survives contact with the ticket that had to
//! bank the bytes. The dictionary is the one addition, and it is class **R**: derived, droppable,
//! re-derivable, like every other graph row here.
//!
//! # What lives where
//!
//! This module is the *rows*: writing them, walking a chain back to its anchor within
//! `graph.query.max_chain_walk`, and accounting for the bytes with §8.4's labelled basis. The graph
//! *model* — what a node is, which edge kinds exist, what "dependents of X" means and when the
//! answer has to admit it is incomplete — is `fathom-analyze`'s `graph` module, the analyzer
//! ADR-004 §5.1 registers as `graph.build`. Storage does not decide semantics; it stores them.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension as _, params};

use crate::error::Error;
use crate::spine::Store;
use crate::spine::corrupt;

/// Migration 8: ADR-004 Appendix A §4.6 verbatim — `graph_snapshot`, `graph_delta`, `graph_chunk`,
/// `graph_chunk_link` and their indexes — followed by the node dictionary that banks `s3`'s projected
/// interning. Written as one literal because `execute_batch` gets one string and `concat!` will not take
/// a `const`.
pub const MIGRATION_8: &str = "
CREATE TABLE IF NOT EXISTS graph_snapshot (
  tree_oid  TEXT PRIMARY KEY,
  base_tree TEXT,                          -- null for a full snapshot
  kind      TEXT NOT NULL CHECK (kind IN ('full','delta','chunked')),
  depth     INTEGER NOT NULL DEFAULT 0,    -- chain length back to the nearest full snapshot
  node_count INTEGER NOT NULL,
  edge_count INTEGER NOT NULL,
  bytes     INTEGER NOT NULL,
  analyzer_version INTEGER NOT NULL,
  built_utc INTEGER NOT NULL,
  cost_ms   INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS graph_snapshot_base ON graph_snapshot(base_tree);

-- graph_delta: the ops from `base_tree` to `tree_oid`. A query at revision R walks at
-- most `graph.chain_max` (§8.5) steps, which is what bounds p95 in §11.
--
-- `dst` is in the primary key and is not nullable, so a *node* op carries `dst = ''` — §4.6's rough
-- edge, found by `s3` writing into this table, and the only legal spelling of `DeltaOp::add_node`.
-- `src` and `dst` hold a `graph_node.node_id` as decimal text: `e3.2`'s interning, landed inside
-- the shipped DDL rather than as a migration of it.
CREATE TABLE IF NOT EXISTS graph_delta (
  tree_oid TEXT NOT NULL REFERENCES graph_snapshot(tree_oid) ON DELETE CASCADE,
  op   TEXT NOT NULL CHECK (op IN ('add','remove')),
  kind TEXT NOT NULL,                      -- node|edge|edge_kind|layer
  src  TEXT NOT NULL,
  dst  TEXT NOT NULL,
  src_layer TEXT,
  dst_layer TEXT,
  PRIMARY KEY (tree_oid, op, kind, src, dst)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS graph_delta_src ON graph_delta(src, tree_oid);
CREATE INDEX IF NOT EXISTS graph_delta_dst ON graph_delta(dst, tree_oid);

-- graph_chunk / graph_chunk_link: the content-defined-chunking strategy, kept so that
-- `s3` can measure it against the delta chain without a schema change.
CREATE TABLE IF NOT EXISTS graph_chunk (
  sha256 TEXT PRIMARY KEY,
  kind   TEXT NOT NULL,                    -- adjacency|node_table|layer_map|manifest
  bytes  INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS graph_chunk_link (
  tree_oid TEXT NOT NULL REFERENCES graph_snapshot(tree_oid) ON DELETE CASCADE,
  slot  INTEGER NOT NULL,
  chunk_sha TEXT NOT NULL REFERENCES graph_chunk(sha256),
  PRIMARY KEY (tree_oid, slot)
) WITHOUT ROWID, STRICT;

-- The node dictionary: `s3`'s projection - 'should land near a third of the bytes' - banked. Keyed by
-- *key* because that is the direction the writer needs: have I seen this path before, and what is its
-- number? The id index below is what the reader needs to spell an answer back as a path. Both
-- directions in one table, because a second table would be a second copy of one fact.
CREATE TABLE IF NOT EXISTS graph_node (
  node_key TEXT NOT NULL,                  -- module path, or `<path>::<symbol>`
  node_id  INTEGER NOT NULL,               -- what `graph_delta.src`/`dst` hold, as text
  kind     TEXT NOT NULL,                  -- module|symbol
  layer    TEXT NOT NULL DEFAULT '',       -- `[layers]` tag; '' means untagged, not unknown
  incomplete INTEGER NOT NULL DEFAULT 0,   -- 1: its declaration list may have shrunk, not shrunk
  PRIMARY KEY (node_key)
) WITHOUT ROWID, STRICT;
CREATE UNIQUE INDEX IF NOT EXISTS graph_node_id ON graph_node(node_id);
";

/// Every graph table this module owns. `fathom store usage` and the `e3.2` harness read the list
/// rather than hard-coding a copy of it.
pub const GRAPH_TABLES: [&str; 5] = [
    "graph_snapshot",
    "graph_delta",
    "graph_chunk",
    "graph_chunk_link",
    "graph_node",
];

/// The `kind` values that name a vertex rather than an edge. A node op is one whose `kind` is here;
/// see [`DeltaOp::is_node_op`].
pub const NODE_KINDS: [&str; 2] = ["module", "symbol"];

/// The `graph_chunk.kind` spelling of an anchor's manifest — the row `s3`'s finding 2 says
/// `graph_chunk_link`'s foreign key cannot do without.
pub const CHUNK_KIND_MANIFEST: &str = "manifest";

/// The `graph_chunk.kind` spelling of one module's adjacency chunk.
pub const CHUNK_KIND_ADJACENCY: &str = "adjacency";

/// The anchor's one link slot. A chain anchor is a single manifest, so slot `0` is the only one
/// written; the column stays a slot because §4.6's key is `(tree_oid, slot)` and the chunking
/// strategy it kept room for would use more than one.
pub const ANCHOR_SLOT: i64 = 0;

/// One revision the graph lane owes a row for (`Store::graph_pending_revisions`).
///
/// A writer needs the *commit* as well as the tree, because the live file list of a revision is built from
/// the file rows its commit touched (`git_commit_file`), and because the tree alone says nothing about
/// what changed on the way to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRevision {
    /// The commit row this revision came from.
    pub commit_oid: String,
    /// The revision itself: `graph_snapshot`'s key.
    pub tree_oid: String,
    /// The first parent's tree, when this store has a row for it.
    pub first_parent_tree: Option<String>,
    /// The committer's date, unix millis — the order the writer walks.
    pub when: i64,
}

/// The graph lane's work list: the revisions this pass may write, and how many the window refuses.
///
/// The count is not decoration. A lane that graphs the newest 32 revisions of a 4 000-revision history has
/// answered the reviewer's question about today and left 3 968 revisions with no graph; `owed -
/// revisions.len()` is that number, and it is the store's rather than an estimate the writer made on the
/// way past the cutoff.
///
/// It is deliberately **not a queue**. Nothing here promises the 3 968 arrive on a later run: `window` is a
/// cutoff over everything the store can reach, so a revision older than the window is refused, not deferred
/// ([`Store::graph_pending_revisions`] is where that is said). A writer that treated the difference as work
/// in progress would spend one quiet index per window-sized slice of history — which is the defect
/// `reco-r3x` closed.
#[derive(Debug, Clone, Default)]
pub struct PendingWork {
    /// The revisions to write, oldest first: owed *and* inside the window.
    pub revisions: Vec<PendingRevision>,
    /// Every revision the lane owes, window or not — so `owed - revisions.len()` is what the window
    /// refuses. Counted as distinct trees, because a tree is what a graph row is keyed by.
    pub owed: usize,
}

/// `add` or `remove`: the only two things a delta row can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Op {
    /// The row appeared.
    Add,
    /// The row went away.
    Remove,
}

impl Op {
    /// The column's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Op::Add => "add",
            Op::Remove => "remove",
        }
    }

    /// Parse the column. Anything else is `None` rather than a guess: a delta row whose op nobody
    /// recognises is a row that must not be applied to a graph.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "add" => Some(Op::Add),
            "remove" => Some(Op::Remove),
            _ => None,
        }
    }
}

/// One `graph_delta` row: `(op, kind, src, dst)` plus the two layer tags.
///
/// `src` and `dst` are interned node ids as decimal text — see the module docs on interning. The
/// layer tags are denormalised on purpose: they come from `[layers]` config, which a rebuild can
/// recompute, but the *layering* questions (`e3.4`'s rules, `e3.8`'s coupling) are asked of the
/// delta, and a delta whose edges cannot say which layers they cross is a delta that has to be
/// materialised to answer its own cheapest question.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeltaOp {
    /// Add or remove.
    pub op: Op,
    /// `module`, `symbol`, or an edge kind (`contains`, `imports`, `imports_package`, `uses`,
    /// `implements`, `inherits`, `wildcard`).
    pub kind: String,
    /// The subject's interned id, as text.
    pub src: String,
    /// The object's interned id as text, or `""` for a node op — rough edge 1.
    pub dst: String,
    /// The subject's layer tag, `""` when untagged.
    pub src_layer: String,
    /// The object's layer tag; `""` for a node op, where there is no object.
    pub dst_layer: String,
}

impl DeltaOp {
    /// A node add. `dst` is the empty string because `dst` is in the primary key and is not
    /// nullable, which is §4.6's rough edge and the only legal spelling of "this row is about one
    /// thing" (`s3`'s finding 1).
    #[must_use]
    pub fn add_node(
        kind: impl Into<String>,
        src: impl Into<String>,
        layer: impl Into<String>,
    ) -> Self {
        Self {
            op: Op::Add,
            kind: kind.into(),
            src: src.into(),
            dst: String::new(),
            src_layer: layer.into(),
            dst_layer: String::new(),
        }
    }

    /// An edge add, with each endpoint's layer tag.
    #[must_use]
    pub fn add_edge(
        kind: impl Into<String>,
        src: impl Into<String>,
        dst: impl Into<String>,
        src_layer: impl Into<String>,
        dst_layer: impl Into<String>,
    ) -> Self {
        Self {
            op: Op::Add,
            kind: kind.into(),
            src: src.into(),
            dst: dst.into(),
            src_layer: src_layer.into(),
            dst_layer: dst_layer.into(),
        }
    }

    /// The same row with `op='remove'`.
    #[must_use]
    pub fn removed(mut self) -> Self {
        self.op = Op::Remove;
        self
    }

    /// Whether this row names a vertex rather than an edge — the same question as "is `dst` the
    /// empty string", and the two must never disagree.
    #[must_use]
    pub fn is_node_op(&self) -> bool {
        NODE_KINDS.contains(&self.kind.as_str())
    }

    /// The row's identity without the layer tags: `(kind, src, dst)`, what a graph stores.
    #[must_use]
    pub fn row(&self) -> (String, String, String) {
        (self.kind.clone(), self.src.clone(), self.dst.clone())
    }
}

/// A `graph_snapshot` row: what one revision's graph *is*, and where its chain starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRow {
    /// The revision's tree oid — the key, and the only thing a revision can be keyed by.
    pub tree_oid: String,
    /// The tree this graph is a delta *from*; `None` for an anchor.
    pub base_tree: Option<String>,
    /// `full` for an anchor, `delta` for every other revision.
    pub kind: String,
    /// How many deltas back to the anchor: 0 at an anchor.
    pub depth: i64,
    /// Nodes in the revision's whole graph, as the builder counted them.
    pub node_count: i64,
    /// Edges, likewise.
    pub edge_count: i64,
    /// Bytes this revision's rows cost as written. §11's budget is checked against a *measured*
    /// store delta, not against this column, which is the writer's own estimate.
    pub bytes: i64,
    /// The `graph.build` analyzer version that produced it (§5).
    pub analyzer_version: i64,
    /// Unix seconds of the write.
    pub built_utc: i64,
    /// What the write cost, in ms.
    pub cost_ms: i64,
}

impl SnapshotRow {
    /// An anchor row: a chain root, so no `base_tree` and no depth.
    #[must_use]
    pub fn anchor(tree_oid: impl Into<String>) -> Self {
        Self {
            tree_oid: tree_oid.into(),
            base_tree: None,
            kind: String::from("full"),
            depth: 0,
            node_count: 0,
            edge_count: 0,
            bytes: 0,
            analyzer_version: 1,
            built_utc: 0,
            cost_ms: 0,
        }
    }

    /// A delta row, `depth` steps from its anchor.
    #[must_use]
    pub fn delta(tree_oid: impl Into<String>, base_tree: impl Into<String>, depth: i64) -> Self {
        Self {
            tree_oid: tree_oid.into(),
            base_tree: Some(base_tree.into()),
            kind: String::from("delta"),
            depth,
            node_count: 0,
            edge_count: 0,
            bytes: 0,
            analyzer_version: 1,
            built_utc: 0,
            cost_ms: 0,
        }
    }
}

/// One step of the node dictionary: a key, what kind of vertex it is, and its two read-time tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeEntry {
    /// `crates/x/src/y.rs`, or `crates/x/src/y.rs::Thing` for a symbol.
    pub key: String,
    /// The id to record, or `None` to let the store mint one.
    ///
    /// The lane passes `Some` — it mints ids so it can build rows before it writes them — and the
    /// store honours them, because two dictionaries that each invented their own numbers for one
    /// vertex would make every stored chunk mean something different depending on which one wrote
    /// it. A single writer (ADR-004 §7.1) is what keeps the mint collision-free.
    pub id: Option<i64>,
    /// `module` or `symbol`.
    pub kind: &'static str,
    /// The `[layers]` tag; empty when the path matches no layer.
    pub layer: String,
    /// Whether the declaration list may have shrunk rather than shrunk. Symbols inherit their
    /// module's answer; the flag is set on the module and read from it.
    pub incomplete: bool,
}

impl NodeEntry {
    /// A module vertex, with the id the caller minted for it.
    #[must_use]
    pub fn module(
        key: impl Into<String>,
        id: Option<i64>,
        layer: impl Into<String>,
        incomplete: bool,
    ) -> Self {
        Self {
            key: key.into(),
            id,
            kind: "module",
            layer: layer.into(),
            incomplete,
        }
    }

    /// A symbol vertex.
    #[must_use]
    pub fn symbol(key: impl Into<String>, id: Option<i64>, layer: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            id,
            kind: "symbol",
            layer: layer.into(),
            incomplete: false,
        }
    }
}

/// An interned vertex, read back out of the dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The interned id.
    pub id: i64,
    /// The path, or the `path::Symbol` spelling.
    pub key: String,
    /// `module` or `symbol`.
    pub kind: String,
    /// The layer tag; empty when untagged.
    pub layer: String,
    /// Whether this module's declaration list may have shrunk rather than shrunk (`s1` finding 1):
    /// a macro expansion, a decorated class, a barrel `export *`. Refreshed whenever the vertex is
    /// re-interned, which is whenever its blob moves — so it is current, not merely recent.
    pub incomplete: bool,
}

impl Node {
    /// Whether this vertex is a module rather than a symbol.
    #[must_use]
    pub fn is_module(&self) -> bool {
        self.kind == "module"
    }
}

/// A chain walk's result: the anchor to load, and the deltas to apply in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainWalk {
    /// The anchor's tree oid.
    pub anchor: String,
    /// The delta tree oids, oldest first — apply in this order.
    pub deltas: Vec<String>,
    /// How many `base_tree` steps were walked.
    pub steps: usize,
}

impl ChainWalk {
    /// Whether the revision *is* an anchor, so there is nothing to replay.
    #[must_use]
    pub fn is_anchor(&self) -> bool {
        self.deltas.is_empty()
    }
}

/// Why a chain walk could not produce a starting point. This is ADR-004 §8.5's stated partial: a
/// chain longer than the anchor period means *an anchor went missing*, which is a fact about the
/// store, not a slower answer — `s3`'s reason `max_chain_walk` is the same number as
/// `chain_full_every`.
///
/// It is deliberately not a variant of [`Error`]: the error vocabulary in [`crate::error`] is a
/// published, closed set mapped onto `fathom_core::error::ErrorCode`, and "this revision's graph
/// cannot be reconstructed" is an answer about the repository, not a fault in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainPartial {
    /// `base_tree` pointed at a tree with no row, or at nothing, before an anchor was reached.
    AnchorLost {
        /// The tree the walk gave up on.
        tree_oid: String,
        /// Steps taken first.
        steps: usize,
    },
    /// The walk exceeded `graph.query.max_chain_walk` while the chain was still intact.
    ChainTooLong {
        /// The cap that stopped it.
        max_walk: usize,
        /// How far it got.
        steps: usize,
    },
}

impl ChainPartial {
    /// The stable code a surface prints and `v_unknowns` carries.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            ChainPartial::AnchorLost { .. } => "graph.chain.anchor_lost",
            ChainPartial::ChainTooLong { .. } => "graph.chain.too_long",
        }
    }

    /// The sentence for the pane: what is missing, and what would fix it.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            ChainPartial::AnchorLost { tree_oid, steps } => format!(
                "the delta chain from {tree_oid} reaches no anchor after {steps} step(s): an \
                 anchor was lost, so this revision's graph cannot be reconstructed"
            ),
            ChainPartial::ChainTooLong { max_walk, steps } => format!(
                "the delta chain is longer than graph.query.max_chain_walk = {max_walk} \
                 (walked {steps}); a chain that long means an anchor is missing rather than that \
                 the answer is merely slow"
            ),
        }
    }
}

/// The graph's own bytes, counted as payload.
///
/// The basis is stated in the name because §8.4 makes that the rule: `store.bytes_per_commit_graph.*`
/// was measured against *row payload plus blob payload*, which is what `s3` measured and what a
/// budget can be compared against. Store pages are a different quantity, reported by
/// [`Store::graph_usage`] and never mixed into this one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphBytes {
    /// Delta rows stored.
    pub delta_rows: i64,
    /// Their `kind || src || dst` text, in bytes.
    pub delta_payload: i64,
    /// Distinct chunk rows — distinct adjacency payloads, shared across anchors.
    pub chunk_rows: i64,
    /// Their blob payloads.
    pub chunk_bytes: i64,
    /// Distinct manifest rows.
    pub manifest_rows: i64,
    /// Their blob payloads.
    pub manifest_bytes: i64,
    /// Node dictionary rows.
    pub dictionary_rows: i64,
    /// Their keys and tags, in bytes.
    pub dictionary_payload: i64,
}

impl GraphBytes {
    /// Everything this graph costs the store, on the payload basis.
    #[must_use]
    pub fn total(&self) -> i64 {
        self.delta_payload + self.chunk_bytes + self.manifest_bytes + self.dictionary_payload
    }

    /// The number with its basis attached, for a report that must not be ambiguous.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "{} B of graph ({} delta row(s) = {} B, {} chunk blob(s) = {} B, {} manifest blob(s) \
             = {} B, {} dictionary row(s) = {} B), counted as payload",
            self.total(),
            self.delta_rows,
            self.delta_payload,
            self.chunk_rows,
            self.chunk_bytes,
            self.manifest_rows,
            self.manifest_bytes,
            self.dictionary_rows,
            self.dictionary_payload
        )
    }
}

/// Store bytes, with the basis labelled — §8.4's rule that an estimate passed off as a measurement
/// is how a budget becomes fiction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreBytes {
    /// Store file pages plus `objects/`, in bytes.
    pub total: i64,
    /// Per-table bytes when `dbstat` is compiled in, empty otherwise.
    pub per_table: BTreeMap<String, i64>,
    /// `dbstat`, or the file-size fallback.
    pub basis: String,
}

impl StoreBytes {
    /// The number with its basis attached.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "{} B of store, measured as {}",
            self.total,
            if self.basis.is_empty() {
                "an unlabelled estimate"
            } else {
                &self.basis
            }
        )
    }
}

impl std::fmt::Display for ChainPartial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.sentence())
    }
}

/// Why [`Store::graph_chain_walk`] did not hand back a starting point.
#[derive(Debug, thiserror::Error)]
pub enum GraphWalkError {
    /// The chain reached no anchor: a stated partial about the repository, not a fault in the store.
    #[error("{0}")]
    Partial(ChainPartial),
    /// The store could not be read at all.
    #[error(transparent)]
    Store(#[from] crate::error::Error),
}

impl Store {
    /// Fold the WAL into the store file.
    ///
    /// Byte measurement needs this on *both* sides of a subtraction: the WAL is transient by §7.4,
    /// so counting it once and not the other way is how a strategy measures negative bytes — which
    /// is how `s3`'s note about it came to exist.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the checkpoint fails.
    pub fn graph_checkpoint(&self) -> Result<(), Error> {
        self.connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|err| corrupt("checkpointing the WAL", &err))
    }

    /// Store file plus `objects/`, in bytes, with the basis labelled (§8.4).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a pragma fails.
    pub fn graph_usage(&self) -> Result<StoreBytes, Error> {
        self.graph_checkpoint()?;
        let pages = scalar_i64(self.connection(), "PRAGMA page_count")?;
        let size = scalar_i64(self.connection(), "PRAGMA page_size")?;
        let objects = self.fathom_dir().map_or(0, |dir| object_tree_bytes(&dir));
        let (per_table, basis) = match dbstat(self.connection()) {
            Ok(sizes) => (sizes, String::from("dbstat")),
            Err(_) => (
                BTreeMap::new(),
                String::from("file size + objects/ (dbstat unavailable)"),
            ),
        };
        Ok(StoreBytes {
            total: pages * size + objects,
            per_table,
            basis,
        })
    }

    /// The graph's bytes on the payload basis — the number every `store.bytes_per_commit_graph.*`
    /// claim in this repository is checked against.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a count fails.
    pub fn graph_payload_bytes(&self) -> Result<GraphBytes, Error> {
        let conn = self.connection();
        let (delta_rows, delta_payload) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(LENGTH(kind) + LENGTH(src) + LENGTH(dst) + 12), 0)
                 FROM graph_delta",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|err| corrupt("counting delta rows", &err))?;
        let (chunk_rows, chunk_bytes) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(b.bytes), 0) FROM graph_chunk c
                 LEFT JOIN blob b ON b.sha256 = c.sha256 WHERE c.kind = ?1",
                params![CHUNK_KIND_ADJACENCY],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|err| corrupt("counting chunk bytes", &err))?;
        let (manifest_rows, manifest_bytes) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(b.bytes), 0) FROM graph_chunk c
                 LEFT JOIN blob b ON b.sha256 = c.sha256 WHERE c.kind = ?1",
                params![CHUNK_KIND_MANIFEST],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|err| corrupt("counting manifest bytes", &err))?;
        let (dictionary_rows, dictionary_payload) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(LENGTH(node_key) + LENGTH(layer) + 16), 0)
                 FROM graph_node",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|err| corrupt("counting dictionary rows", &err))?;
        Ok(GraphBytes {
            delta_rows,
            delta_payload,
            chunk_rows,
            chunk_bytes,
            manifest_rows,
            manifest_bytes,
            dictionary_rows,
            dictionary_payload,
        })
    }

    /// Intern a batch of vertices and return the dictionary keyed by `key`.
    ///
    /// Ids are allocated in the order the batch is given, so the caller's ordering *is* the
    /// reproducibility contract: a rebuild that walks the same tree in the same order mints the same
    /// ids, therefore the same chunk content, therefore the same content addresses. Existing keys
    /// keep their id — an id that moved would make every stored chunk mean something else — but
    /// their *tags* are refreshed, because a re-intern is exactly the moment the lane has fresh
    /// information about a vertex: a `[layers]` edit, or a module that grew a macro scope.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the dictionary cannot be read or written.
    pub fn graph_intern(&mut self, entries: &[NodeEntry]) -> Result<BTreeMap<String, i64>, Error> {
        let mut known: BTreeMap<String, i64> = BTreeMap::new();
        {
            let mut stmt = self
                .connection()
                .prepare("SELECT node_key, node_id FROM graph_node")
                .map_err(|err| corrupt("reading the node dictionary", &err))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|err| corrupt("reading the node dictionary", &err))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| corrupt("reading the node dictionary", &err))?;
            for (key, id) in rows {
                known.insert(key, id);
            }
        }
        let mut minted = 0_i64;
        let mut next_id = known.values().copied().max().unwrap_or(0) + 1;
        let mut stmt = self
            .conn_mut()
            .prepare(
                "INSERT INTO graph_node(node_key, node_id, kind, layer, incomplete)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(node_key) DO UPDATE SET layer = excluded.layer,
                                                     incomplete = excluded.incomplete",
            )
            .map_err(|err| corrupt("interning a graph node", &err))?;
        for entry in entries {
            let id = match known.get(&entry.key) {
                Some(id) => *id,
                None => {
                    let id = entry.id.unwrap_or(next_id);
                    next_id = next_id.max(id + 1);
                    minted += 1;
                    id
                }
            };
            stmt.execute(params![
                entry.key,
                id,
                entry.kind,
                entry.layer,
                i64::from(entry.incomplete)
            ])
            .map_err(|err| corrupt("interning a graph node", &err))?;
            known.insert(entry.key.clone(), id);
        }
        drop(stmt);
        if minted > 0 {
            self.bump_counter("graph.node.interned", minted);
        }
        Ok(known)
    }

    /// The whole dictionary, keyed by id. A materialised revision's answers need names, and one
    /// read beats one query per answer.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn graph_nodes(&self) -> Result<BTreeMap<i64, Node>, Error> {
        let mut stmt = self
            .connection()
            .prepare("SELECT node_id, node_key, kind, layer, incomplete FROM graph_node")
            .map_err(|err| corrupt("reading the node dictionary", &err))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    Node {
                        id: 0,
                        key: row.get::<_, String>(1)?,
                        kind: row.get::<_, String>(2)?,
                        layer: row.get::<_, String>(3)?,
                        incomplete: row.get::<_, i64>(4)? != 0,
                    },
                ))
            })
            .map_err(|err| corrupt("reading the node dictionary", &err))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("reading the node dictionary", &err))?;
        Ok(rows
            .into_iter()
            .map(|(id, mut node)| {
                node.id = id;
                (id, node)
            })
            .collect())
    }

    /// Look one key up. `None` is "not in this store", which a caller must not read as "not in the
    /// revision" — the graph knows which of the two it means.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn graph_node_id(&self, key: &str) -> Result<Option<i64>, Error> {
        self.connection()
            .query_row(
                "SELECT node_id FROM graph_node WHERE node_key = ?1",
                params![key],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|err| corrupt("looking up a graph node", &err))
    }

    /// Write a `graph_snapshot` row.
    ///
    /// `INSERT … ON CONFLICT DO NOTHING`, never `INSERT OR REPLACE` (rough edge 3): the chain is
    /// walked, and `REPLACE` would move a tree's `base_tree` out from under a walk already relying
    /// on it. A second commit reaching an already-stored tree is therefore neither an error nor a
    /// rewrite — it is a counted no-op, and the caller is told, because "this revision shares its
    /// graph with that one" is a fact a lane should count rather than discover.
    ///
    /// Returns whether the row was written (`false`: the tree was already stored).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the insert fails for any reason other than the row being there.
    pub fn graph_write_snapshot(&mut self, row: &SnapshotRow) -> Result<bool, Error> {
        let written = self
            .conn_mut()
            .execute(
                "INSERT INTO graph_snapshot(tree_oid, base_tree, kind, depth, node_count,
                                            edge_count, bytes, analyzer_version, built_utc, cost_ms)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(tree_oid) DO NOTHING",
                params![
                    row.tree_oid,
                    row.base_tree,
                    row.kind,
                    row.depth,
                    row.node_count,
                    row.edge_count,
                    row.bytes,
                    row.analyzer_version,
                    row.built_utc,
                    row.cost_ms
                ],
            )
            .map_err(|err| corrupt("writing a graph snapshot", &err))?;
        if written == 0 {
            self.bump_counter("graph.snapshot.duplicate_tree", 1);
            return Ok(false);
        }
        self.bump_counter(
            if row.kind == "full" {
                "graph.snapshot.anchor"
            } else {
                "graph.snapshot.delta"
            },
            1,
        );
        Ok(true)
    }

    /// The snapshot row for one tree, if it has one.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn graph_snapshot(&self, tree_oid: &str) -> Result<Option<SnapshotRow>, Error> {
        self.connection()
            .query_row(
                "SELECT tree_oid, base_tree, kind, depth, node_count, edge_count, bytes,
                        analyzer_version, built_utc, cost_ms
                 FROM graph_snapshot WHERE tree_oid = ?1",
                params![tree_oid],
                |row| {
                    Ok(SnapshotRow {
                        tree_oid: row.get::<_, String>(0)?,
                        base_tree: row.get::<_, Option<String>>(1)?,
                        kind: row.get::<_, String>(2)?,
                        depth: row.get::<_, i64>(3)?,
                        node_count: row.get::<_, i64>(4)?,
                        edge_count: row.get::<_, i64>(5)?,
                        bytes: row.get::<_, i64>(6)?,
                        analyzer_version: row.get::<_, i64>(7)?,
                        built_utc: row.get::<_, i64>(8)?,
                        cost_ms: row.get::<_, i64>(9)?,
                    })
                },
            )
            .optional()
            .map_err(|err| corrupt("reading a graph snapshot", &err))
    }

    /// Every tree oid with a snapshot row, newest-built first.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn graph_revisions(&self) -> Result<Vec<String>, Error> {
        let mut stmt = self
            .connection()
            .prepare("SELECT tree_oid FROM graph_snapshot ORDER BY built_utc DESC, tree_oid")
            .map_err(|err| corrupt("listing graph revisions", &err))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|err| corrupt("listing graph revisions", &err))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("listing graph revisions", &err))?;
        Ok(rows)
    }

    /// Every revision the graph lane owes inside `window`, oldest first — which is the order its caller
    /// needs, because a delta chain cannot be written before its base.
    ///
    /// This is the writer's work list (`e3.11`, `reco-dlp`), and `window` is a **cutoff over everything the
    /// store can reach, not a batch size**: the eligible revisions are the owed ones among the newest
    /// `window` trees. Five things the caller does not have to re-check:
    ///
    /// * **liveness** — the rows are the commits a stored ref reaches, the same root rule as
    ///   [`crate::inventory`]'s reachability walk, so a rebased-away commit does not get a graph of its
    ///   own. When the store holds no ref tips at all there is nothing to be live relative to, and the
    ///   walk is skipped rather than answering "nothing exists".
    /// * **which end** — the cutoff takes the *newest* revisions. A writer capped at the oldest ones would
    ///   never catch up on a repository that is behind: it would spend every run on the same ancient
    ///   window and the reviewer's question, which is always about today, would never be answerable.
    /// * **that a cutoff is a cutoff** — everything older than the window is *refused*, not queued, and
    ///   [`PendingWork::owed`] is measured against everything so the caller can say the difference out loud.
    ///   This is what keeps a quiet run quiet (`reco-r3x`): capped at the newest *owed* revisions instead, a
    ///   no-op index would settle one window-sized slice of history every time it ran and pay for it in
    ///   full — a second per run for ten runs on a 500-commit fixture, inside a verb whose whole promise is
    ///   that it is free when nothing changed (ADR-004 §6.6 R1). A repository that wants the whole trend
    ///   says so by raising `graph.window`, or by setting it to `0`, which is every owed revision with no
    ///   cutoff at all.
    /// * **one row per tree** — a graph row is keyed by tree, and a history can reach one tree twice (an
    ///   empty commit, a revert to a known tree), so a duplicated tree is listed once, by its newest
    ///   commit. Writing it twice would be the same bytes written twice.
    /// * **order** — committer date ascending, then oid, which is first-parent order for the linear
    ///   histories that make up nearly every repository, and a decent approximation everywhere else.
    ///   The writer re-reads a tree when the chain breaks; it is never wrong, only occasionally slower.
    ///
    /// `first_parent_tree` is the *first* parent's tree, because that is the side the spine diffed a merge
    /// against — the two lanes must not disagree about what a merge changed. It is null for a root commit
    /// and for a parent this store has no row for (a shallow boundary), which is the same "no base" answer
    /// the lane takes as "this revision is an anchor".
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    ///
    /// The `window = 0` case is the caller's, not this one's: `0` means "no cutoff", and the way to say
    /// that here is a limit nothing history reaches.
    pub fn graph_pending_revisions(&self, window: usize) -> Result<PendingWork, Error> {
        // One reachability walk, three readings of it: `inside` is the window (ranked over every reachable
        // tree, owed or not, which is the whole point), `owed` is what the lane has no row for inside it,
        // and `COUNT` is what it has no row for anywhere.
        const ROWS: &str = " \
            WITH RECURSIVE live(oid) AS ( \
                SELECT oid FROM ref_tip WHERE kind IN ('head','branch','remote') \
                UNION \
                SELECT parent.value FROM live \
                JOIN git_commit c ON c.oid = live.oid \
                JOIN json_each(c.parent_oids) AS parent \
            ), reachable AS ( \
                SELECT c.oid AS oid, c.tree_oid AS tree_oid, c.committer_utc AS when_ms, \
                       c.parent_oids AS parents \
                FROM git_commit c \
                WHERE (NOT EXISTS (SELECT 1 FROM ref_tip \
                                    WHERE kind IN ('head','branch','remote')) \
                       OR c.oid IN (SELECT oid FROM live)) \
            ), inside AS ( \
                SELECT tree_oid FROM reachable \
                GROUP BY tree_oid \
                ORDER BY max(when_ms) DESC, tree_oid DESC LIMIT ?1 \
            ), owed AS ( \
                SELECT r.oid AS oid, r.tree_oid AS tree_oid, r.when_ms AS when_ms, \
                       (SELECT pc.tree_oid FROM json_each(r.parents) AS p \
                          JOIN git_commit pc ON pc.oid = p.value \
                         WHERE p.key = 0) AS first_parent_tree \
                FROM reachable r \
                WHERE r.tree_oid IN (SELECT tree_oid FROM inside) \
                  AND NOT EXISTS (SELECT 1 FROM graph_snapshot g WHERE g.tree_oid = r.tree_oid) \
            ) \
            SELECT oid, tree_oid, when_ms, first_parent_tree FROM owed \
            ORDER BY when_ms DESC, oid DESC";
        const COUNT: &str = " \
            WITH RECURSIVE live(oid) AS ( \
                SELECT oid FROM ref_tip WHERE kind IN ('head','branch','remote') \
                UNION \
                SELECT parent.value FROM live \
                JOIN git_commit c ON c.oid = live.oid \
                JOIN json_each(c.parent_oids) AS parent \
            ) \
            SELECT count(DISTINCT c.tree_oid) FROM git_commit c \
            WHERE (NOT EXISTS (SELECT 1 FROM ref_tip \
                                WHERE kind IN ('head','branch','remote')) \
                   OR c.oid IN (SELECT oid FROM live)) \
              AND NOT EXISTS (SELECT 1 FROM graph_snapshot g WHERE g.tree_oid = c.tree_oid)";
        let cap = i64::try_from(window).unwrap_or(i64::MAX);
        let mut stmt = self
            .connection()
            .prepare(ROWS)
            .map_err(|err| corrupt("listing the revisions the graph lane owes", &err))?;
        let mut rows = stmt
            .query_map(params![cap], |row| {
                Ok(PendingRevision {
                    commit_oid: row.get(0)?,
                    tree_oid: row.get(1)?,
                    when: row.get(2)?,
                    first_parent_tree: row.get(3)?,
                })
            })
            .map_err(|err| corrupt("listing the revisions the graph lane owes", &err))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("listing the revisions the graph lane owes", &err))?;
        // Newest first out of SQLite; the chain order out of here. Two commits can reach one tree (an
        // empty commit, a revert to a tree that already existed) and a tree is one revision, so the first
        // sighting wins — which, newest-first, is the newest commit to have held it.
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        rows.retain(|row| seen.insert(row.tree_oid.clone()));
        rows.reverse();
        let owed = self
            .connection()
            .query_row(COUNT, [], |row| row.get::<_, i64>(0))
            .map_err(|err| corrupt("counting the revisions the graph lane owes", &err))?;
        Ok(PendingWork {
            revisions: Self::chain_order(rows),
            owed: usize::try_from(owed).unwrap_or(usize::MAX),
        })
    }

    /// The work list in the order a delta chain is written: every revision immediately after its first parent,
    /// oldest branch first.
    ///
    /// Committer date is what a human expects and it is the tie-break here, but it cannot be the primary key:
    /// `git` stores two commits with the same committer date without complaining (a rebase, a bot applying a
    /// series in one second, every test fixture that pins a date), and then a date order is an arbitrary order
    /// — and a writer handed revisions out of chain order must anchor each one, because nothing tells it the
    /// previous row was the base. That turns a delta chain into a pile of full manifests and quietly multiplies
    /// the cost of the pass by the anchor period. The first-parent links are already in the store, so the chain
    /// order is derived rather than guessed.
    ///
    /// Revisions whose first parent is not in the list — the usual case at a `graph.window` boundary, and the
    /// only case a store with no ref tips can have — are roots, and roots come in date order. Anything caught in
    /// a parent cycle (which the store should not be able to hold, and which is worth more than a panic if it
    /// does) is emitted once, in the order it arrived.
    fn chain_order(rows: Vec<PendingRevision>) -> Vec<PendingRevision> {
        // Indexed by position so the borrows stay simple: `rows` is moved out of at the end.
        let mut position: BTreeMap<&str, usize> = BTreeMap::new();
        for (index, row) in rows.iter().enumerate() {
            let _ = position.insert(row.tree_oid.as_str(), index);
        }
        let parent = rows
            .iter()
            .map(|row| {
                row.first_parent_tree
                    .as_deref()
                    .and_then(|tree| position.get(tree).copied())
            })
            .collect::<Vec<_>>();
        let mut children: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (index, parent) in parent.iter().enumerate() {
            if let Some(base) = parent {
                children.entry(*base).or_default().push(index);
            }
        }
        // `rows` arrives newest-first, so walking it backwards hands the roots over oldest-first and the stack
        // below has to push them newest-first for the pop order to come out oldest-first.
        let mut stack: Vec<usize> = (0..rows.len())
            .filter(|index| parent[*index].is_none())
            .collect();
        let mut order = Vec::with_capacity(rows.len());
        let mut emitted = vec![false; rows.len()];
        while let Some(index) = stack.pop() {
            if std::mem::replace(&mut emitted[index], true) {
                continue;
            }
            order.push(index);
            if let Some(kids) = children.get(&index) {
                // Newest child first on the stack, so the oldest is popped next and the main line stays ahead of
                // anything branched off it.
                for kid in kids {
                    stack.push(*kid);
                }
            }
        }
        // A cycle in the stored parent links leaves its members unreachable from any root. Emit them rather than
        // dropping the revisions: the writer will anchor them, which is the honest answer about a base it cannot
        // see, and the count in `PendingWork::owed` still adds up.
        for (index, seen) in emitted.iter().enumerate() {
            if !seen {
                order.push(index);
            }
        }
        order.into_iter().map(|index| rows[index].clone()).collect()
    }

    /// Throw away every derived graph row, and say what was there.
    ///
    /// The graph lane is derived data, and ADR-004 §9.5's rule for derived data is that it is recomputed
    /// rather than migrated: there is no sensible edit to a chain of deltas when the rules it was built
    /// under change, and a half-updated chain is a graph that lies. So the answer is deletion, and the
    /// writer's next pass re-derives all of it — which is why this is a whole-lane reset rather than a
    /// per-tree delete: a chain is only intact from an anchor, and dropping one revision out of the
    /// middle orphans every delta above it.
    ///
    /// `graph_node` goes with them. The dictionary is an interning convenience, not a record — the ids it
    /// mints mean nothing without the rows that use them, and `Lane::new` seeds its interner from it.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a delete fails.
    pub fn graph_reset(&mut self) -> Result<BTreeMap<String, i64>, Error> {
        let mut removed = BTreeMap::new();
        // Children first. The foreign keys are `ON DELETE CASCADE`, and a delete that names a child table
        // *and* fires the cascade is only correct by accident. A chunk nothing links to is a chunk nothing
        // will ever read again, so `graph_chunk` is emptied rather than decremented.
        for table in [
            "graph_delta",
            "graph_chunk_link",
            "graph_chunk",
            "graph_node",
            "graph_snapshot",
        ] {
            let deleted = self
                .conn_mut()
                .execute(&format!("DELETE FROM {table}"), [])
                .map_err(|err| corrupt("clearing a graph table", &err))?;
            let _ = removed.insert(table.to_owned(), i64::try_from(deleted).unwrap_or(i64::MAX));
        }
        self.bump_counter("graph.reset", 1);
        Ok(removed)
    }

    /// Write a revision's op rows.
    ///
    /// `INSERT OR IGNORE`, because `(tree_oid, op, kind, src, dst)` is the whole identity of an op:
    /// writing the same op twice for one revision means the same thing once.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails — including the ordinary case of a missing
    /// `graph_snapshot` parent, which is a lane bug rather than a data problem.
    pub fn graph_write_ops(&mut self, tree_oid: &str, ops: &[DeltaOp]) -> Result<usize, Error> {
        let mut written = 0_usize;
        {
            let conn = self.conn_mut();
            let mut stmt = conn
                .prepare(
                    "INSERT OR IGNORE INTO graph_delta(tree_oid, op, kind, src, dst, src_layer, dst_layer)
                     VALUES (?1,?2,?3,?4,?5,?6,?7)",
                )
                .map_err(|err| corrupt("writing a graph delta row", &err))?;
            for op in ops {
                let n = stmt
                    .execute(params![
                        tree_oid,
                        op.op.as_str(),
                        op.kind,
                        op.src,
                        op.dst,
                        op.src_layer,
                        op.dst_layer
                    ])
                    .map_err(|err| corrupt("writing a graph delta row", &err))?;
                written += n;
            }
        }
        self.bump_counter(
            "graph.delta.rows",
            i64::try_from(written).unwrap_or(i64::MAX),
        );
        Ok(written)
    }

    /// A revision's op rows, in primary-key order — which is the order a reconstruction applies
    /// them, and the reason the same store read twice produces the same graph.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn graph_ops(&self, tree_oid: &str) -> Result<Vec<DeltaOp>, Error> {
        self.graph_ops_where(tree_oid, false, None)
    }

    /// The ops one revision's delta says about `node_id` in either direction — the SQL answer to
    /// "what did this commit change about X", which is `e3.3`'s question at its cheapest.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn graph_ops_touching(&self, tree_oid: &str, node_id: i64) -> Result<Vec<DeltaOp>, Error> {
        self.graph_ops_where(tree_oid, true, Some(node_id.to_string()))
    }

    /// The one delta read both of the above go through, so the two cannot drift.
    fn graph_ops_where(
        &self,
        tree_oid: &str,
        by_node: bool,
        node_id: Option<String>,
    ) -> Result<Vec<DeltaOp>, Error> {
        let sql = if by_node {
            "SELECT op, kind, src, dst, src_layer, dst_layer FROM graph_delta
             WHERE tree_oid = ?1 AND (src = ?2 OR dst = ?2) ORDER BY op, kind, src, dst"
        } else {
            "SELECT op, kind, src, dst, src_layer, dst_layer FROM graph_delta
             WHERE tree_oid = ?1 ORDER BY op, kind, src, dst"
        };
        let mut stmt = self
            .connection()
            .prepare(sql)
            .map_err(|err| corrupt("reading a graph delta", &err))?;
        let read = |row: &rusqlite::Row| -> rusqlite::Result<DeltaOp> {
            Ok(DeltaOp {
                // An unparseable `op` is refused rather than defaulted: applying a row whose meaning
                // nobody recognises would invent graph facts out of a foreign row.
                op: Op::parse(&row.get::<_, String>(0)?)
                    .ok_or_else(|| rusqlite::Error::IntegralValueOutOfRange(0, 0))?,
                kind: row.get::<_, String>(1)?,
                src: row.get::<_, String>(2)?,
                dst: row.get::<_, String>(3)?,
                src_layer: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                dst_layer: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        };
        let rows = match node_id {
            Some(id) => stmt
                .query_map(params![tree_oid, id], read)
                .map_err(|err| corrupt("reading a graph delta", &err))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| corrupt("reading a graph delta", &err))?,
            None => stmt
                .query_map(params![tree_oid], read)
                .map_err(|err| corrupt("reading a graph delta", &err))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| corrupt("reading a graph delta", &err))?,
        };
        Ok(rows)
    }

    /// Record a content-addressed chunk's `graph_chunk` row — including the manifest row `s3`'s
    /// finding 2 says `graph_chunk_link`'s foreign key cannot do without.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn graph_write_chunk(&mut self, sha: &str, kind: &str, bytes: i64) -> Result<(), Error> {
        self.conn_mut()
            .execute(
                "INSERT OR IGNORE INTO graph_chunk(sha256, kind, bytes) VALUES (?1, ?2, ?3)",
                params![sha, kind, bytes],
            )
            .map_err(|err| corrupt("writing a graph chunk", &err))?;
        self.bump_counter("graph.chunk.rows", 1);
        Ok(())
    }

    /// Link a chunk to a revision. For an anchor that is one row at [`ANCHOR_SLOT`].
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails — including the case this table exists to make
    /// loud: linking a chunk with no `graph_chunk` row is a foreign-key violation, not a silent gap.
    pub fn graph_link_chunk(&mut self, tree_oid: &str, slot: i64, sha: &str) -> Result<(), Error> {
        self.conn_mut()
            .execute(
                "INSERT OR IGNORE INTO graph_chunk_link(tree_oid, slot, chunk_sha) VALUES (?1, ?2, ?3)",
                params![tree_oid, slot, sha],
            )
            .map_err(|err| corrupt("linking a graph chunk", &err))?;
        self.bump_counter("graph.chunk.link", 1);
        Ok(())
    }

    /// The chunk addresses linked from one revision, by slot.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn graph_chunk_links(&self, tree_oid: &str) -> Result<Vec<(i64, String)>, Error> {
        let mut stmt = self
            .connection()
            .prepare(
                "SELECT slot, chunk_sha FROM graph_chunk_link WHERE tree_oid = ?1 ORDER BY slot",
            )
            .map_err(|err| corrupt("reading graph chunk links", &err))?;
        let rows = stmt
            .query_map(params![tree_oid], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| corrupt("reading graph chunk links", &err))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("reading graph chunk links", &err))?;
        Ok(rows)
    }

    /// The anchor's manifest: the bytes its slot-`0` `graph_chunk_link` row names.
    ///
    /// `None` means the anchor has no manifest row at all, which is a broken anchor rather than an
    /// empty one; the materialiser reports it as [`ChainPartial::AnchorLost`].
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if either read fails.
    pub fn graph_anchor_manifest(&self, tree_oid: &str) -> Result<Option<Vec<u8>>, Error> {
        let Some((_, sha)) = self
            .graph_chunk_links(tree_oid)?
            .into_iter()
            .find(|(slot, _)| *slot == ANCHOR_SLOT)
        else {
            return Ok(None);
        };
        self.read_blob(&sha)
    }

    /// Walk `base_tree` back to the anchor and return the deltas to replay, oldest first.
    ///
    /// The walk is bounded by `max_walk` — `graph.query.max_chain_walk`, the same number as
    /// `chain_full_every` by `s3`'s policy, because a chain longer than the anchor period is an
    /// anchor that went missing, and that is a stated partial rather than a slow query.
    ///
    /// # Errors
    ///
    /// [`ChainPartial`] when no anchor is reachable: the caller prints
    /// [`ChainPartial::sentence`], it does not guess a graph.
    pub fn graph_chain_walk(
        &self,
        tree_oid: &str,
        max_walk: usize,
    ) -> Result<ChainWalk, GraphWalkError> {
        let mut walk = ChainWalk {
            anchor: String::new(),
            deltas: Vec::new(),
            steps: 0,
        };
        let mut tree = tree_oid.to_string();
        for steps in 0..=max_walk {
            let Some(row) = self.graph_snapshot(&tree)? else {
                return Err(GraphWalkError::Partial(ChainPartial::AnchorLost {
                    tree_oid: tree,
                    steps,
                }));
            };
            if row.kind == "full" {
                walk.anchor = tree;
                walk.steps = steps;
                // Walked head-to-anchor, applied anchor-then-oldest-first, so the list is turned
                // around here. A caller that applied these newest-first would resolve a remove that
                // preceded a re-add backwards, which is why the order is documented on [`ChainWalk`]
                // and pinned by a test rather than left to whoever reads `base_tree` next.
                walk.deltas.reverse();
                return Ok(walk);
            }
            let Some(base) = row.base_tree else {
                // A `delta` row with no base is a chain that dead-ends: the same partial, named.
                return Err(GraphWalkError::Partial(ChainPartial::AnchorLost {
                    tree_oid: tree,
                    steps,
                }));
            };
            walk.deltas.push(tree.clone());
            tree = base;
        }
        Err(GraphWalkError::Partial(ChainPartial::ChainTooLong {
            max_walk,
            steps: max_walk + 1,
        }))
    }

    /// Row counts per graph table, for `fathom store usage` and the harness's row accounting.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a count fails.
    pub fn graph_row_counts(&self) -> Result<BTreeMap<String, i64>, Error> {
        let mut out = BTreeMap::new();
        for table in GRAPH_TABLES {
            let n = scalar_i64(self.connection(), &format!("SELECT count(*) FROM {table}"))?;
            out.insert(table.to_string(), n);
        }
        Ok(out)
    }
}

/// Run one SQL statement that yields a single integer.
fn scalar_i64(conn: &rusqlite::Connection, sql: &str) -> Result<i64, Error> {
    conn.query_row(sql, [], |row| row.get::<_, i64>(0))
        .map_err(|err| corrupt(sql, &err))
}

/// Per-table bytes from `dbstat`, when the library has it (§8.4's labelled basis).
fn dbstat(conn: &rusqlite::Connection) -> Result<BTreeMap<String, i64>, Error> {
    let mut stmt = conn
        .prepare("SELECT name, sum(pgsize) FROM dbstat GROUP BY name")
        .map_err(|err| corrupt("probing dbstat", &err))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .map_err(|err| corrupt("reading dbstat", &err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| corrupt("reading dbstat", &err))?;
    Ok(rows
        .into_iter()
        .filter_map(|(name, size)| (!name.is_empty()).then(|| (name, size.unwrap_or(0))))
        .collect())
}

/// The bytes under `objects/` — `file`-class payloads live there, so a byte count that ignores them
/// counts the index and calls it the store.
fn object_tree_bytes(dir: &std::path::Path) -> i64 {
    let objects = dir.join("objects");
    let Ok(entries) = std::fs::read_dir(&objects) else {
        return 0;
    };
    let mut total = 0_i64;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            for file in std::fs::read_dir(entry.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                total += file
                    .metadata()
                    .map(|meta| i64::try_from(meta.len()).unwrap_or(i64::MAX))
                    .unwrap_or(0);
            }
        } else if let Ok(meta) = entry.metadata() {
            total += i64::try_from(meta.len()).unwrap_or(i64::MAX);
        }
    }
    total
}

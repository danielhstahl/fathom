//! The graph model: what a node is, what an edge is, and the row a revision is made of.
//!
//! # The shape, and why it is this shape
//!
//! Nodes are **modules and exported symbols**; edges are **imports, uses, implementations and
//! inheritance** — the granularity the ticket names, which is the granularity `s3` measured
//! ("module-granularity edges over module + exported-symbol nodes"). A graph that kept
//! caller→callee edges at symbol level would be an order of magnitude larger per revision and every
//! byte number in [`docs/concepts/graph-storage.md`](../../../docs/concepts/graph-storage.md) would
//! move by that factor; `e3.2` does not buy that, and says so.
//!
//! The unit of storage is a [`Row`]: `(kind, src, dst)` over interned ids. A revision's graph is a
//! *set* of rows, and a revision's delta is the set difference between it and its parent's. That is
//! the whole representation, and it is what makes the three things the policy needs fall out:
//!
//! * an unchanged module contributes **no rows**, because its row set is identical and the
//!   difference is empty — which is the only reason the byte budget is met at all;
//! * a chunk is the encoding of one module's rows, so content-addressing a chunk *is* the structural
//!   sharing between adjacent revisions;
//! * "changed edges between two snapshots" is a set difference, which is either read straight out of
//!   `graph_delta` (adjacent: one indexed query) or computed from two materialisations.
//!
//! # Every edge says how it got its answer
//!
//! Spike `s1`'s finding 2 is a shape requirement, not a footnote: a wildcard dependency — star
//! import, `pub use *`, barrel `export *`, whole-module import, side-effect import — is an edge of
//! its own kind ([`RowKind::Wildcard`]), because a graph that stores nothing there answers *who
//! depends on `models.py`?* with *nobody*, which is worse than unknown. The same argument made
//! [`RowKind::ImportsPackage`] a kind rather than an attribute: in Go the dependency is a package,
//! and a pane that draws it file-to-file is reporting a language-shaped claim nobody made (`s1`
//! finding 4).
//!
//! So a fan-in answer is never a bare list. [`crate::graph::query::Fan`] keeps the named, the
//! unknown-by-construction and the package-granular apart, and that split is the difference between
//! a caller list and a lie (finding 1: a `synthetic_eligible` scope may not be used to claim a
//! complete caller list).

use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// The version of *this* graph shape.
///
/// The `graph.build` analyzer stores rows keyed on its analyzer version (ADR-004 §5), and this is
/// the integer it stores: a change to what a row means — a new edge kind, a different symbol-node
/// rule, a different chunk encoding — bumps it, and every stored graph becomes unreachable rather
/// than misread.
pub const SCHEMA_VERSION: u32 = 1;

/// The row kinds, as they appear in `graph_delta.kind`.
///
/// Two groups, and the split is load-bearing: [`RowKind::is_node`] kinds are vertices and are stored
/// with `dst = ''` (ADR-004 §4.6's rough edge), and [`RowKind::is_dependency`] kinds are the ones a
/// "what depends on what" question walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RowKind {
    /// A module node. `src` is the module's id, `dst` is empty.
    Module,
    /// An exported symbol node. `src` is the `<path>::<symbol>` id, `dst` is empty.
    Symbol,
    /// A module contains one of its exported symbols.
    Contains,
    /// A file-granular dependency: this module imports that one.
    Imports,
    /// A package-granular dependency, recorded per file because that is the granularity the store
    /// keys on (Go). Its own kind precisely so a reader can say *"package-level, drawn per file"*.
    ImportsPackage,
    /// A use of a named symbol: this module uses that symbol. This is what makes symbol nodes more
    /// than decoration — it is the only edge that reaches one from a dependency question.
    Uses,
    /// `impl Shape for Rect`, `class A implements Shape`: a contract, not an ancestry.
    Implements,
    /// `class Circle(Shape)`, `class A extends B`: an ancestry.
    Inherits,
    /// A dependency whose unit is known and whose contents are not: a star import, a barrel
    /// `export *`, a whole-module or side-effect import, a Go package import. *Unknown*, not *none*.
    Wildcard,
}

impl RowKind {
    /// The stored spelling — the value of `graph_delta.kind`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RowKind::Module => "module",
            RowKind::Symbol => "symbol",
            RowKind::Contains => "contains",
            RowKind::Imports => "imports",
            RowKind::ImportsPackage => "imports_package",
            RowKind::Uses => "uses",
            RowKind::Implements => "implements",
            RowKind::Inherits => "inherits",
            RowKind::Wildcard => "wildcard",
        }
    }

    /// Parse the stored spelling. `None` for anything unrecognised, so a foreign row is refused
    /// rather than applied as something plausible.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "module" => Some(RowKind::Module),
            "symbol" => Some(RowKind::Symbol),
            "contains" => Some(RowKind::Contains),
            "imports" => Some(RowKind::Imports),
            "imports_package" => Some(RowKind::ImportsPackage),
            "uses" => Some(RowKind::Uses),
            "implements" => Some(RowKind::Implements),
            "inherits" => Some(RowKind::Inherits),
            "wildcard" => Some(RowKind::Wildcard),
            _ => None,
        }
    }

    /// Whether this kind names a vertex. A row of a node kind is stored with an empty `dst` and
    /// applied by adding or dropping the node, never by touching an index.
    #[must_use]
    pub const fn is_node(self) -> bool {
        matches!(self, RowKind::Module | RowKind::Symbol)
    }

    /// Whether a dependency question walks this kind. `contains` does not: it is composition, and
    /// counting it as a dependency would make every module depend on its own exports. `implements`
    /// and `inherits` do: "what breaks if `Shape` moves" is exactly a fan-in question about them.
    #[must_use]
    pub const fn is_dependency(self) -> bool {
        !matches!(self, RowKind::Module | RowKind::Symbol | RowKind::Contains)
    }

    /// Whether this kind is a *confident* dependency: the unit and its contents are both known.
    #[must_use]
    pub const fn is_named_import(self) -> bool {
        matches!(
            self,
            RowKind::Imports | RowKind::Uses | RowKind::Implements | RowKind::Inherits
        )
    }
}

/// One row: `(kind, src, dst)` over interned ids.
pub type Row = (RowKind, i64, i64);

/// A set of rows, as the delta arithmetic and the chunk codec see them.
pub type RowSet = BTreeSet<Row>;

/// Encode one module's rows as a chunk payload.
///
/// The encoding is a line per row — `kind<TAB>src<TAB>dst` — in the caller's order, and it is the
/// bytes a chunk's content address is computed over. It is *canonical* given the row set, so two
/// revisions that agree about a module agree about its chunk, which is what makes content-defined
/// sharing work instead of only looking like it does.
#[must_use]
pub fn encode_rows(rows: &[Row]) -> Vec<u8> {
    let mut out = String::with_capacity(rows.len() * 24);
    for (kind, src, dst) in rows {
        use std::fmt::Write as _;
        let _ = writeln!(out, "{}\t{src}\t{dst}", kind.as_str());
    }
    out.into_bytes()
}

/// Decode a chunk payload, returning `(rows, lines that could not be read)`.
///
/// A line that does not parse is counted rather than guessed: a chunk written by a newer shape is
/// *missing rows*, and that is a stated partial, not a quietly smaller graph.
#[must_use]
pub fn decode_rows(payload: &[u8]) -> (Vec<Row>, usize) {
    let text = String::from_utf8_lossy(payload);
    let mut rows = Vec::new();
    let mut skipped = 0_usize;
    for line in text.lines() {
        let mut parts = line.split('\t');
        let (Some(kind), Some(src), Some(dst)) = (parts.next(), parts.next(), parts.next()) else {
            skipped += 1;
            continue;
        };
        let Some(kind) = RowKind::parse(kind) else {
            skipped += 1;
            continue;
        };
        let (Ok(src), Ok(dst)) = (src.parse::<i64>(), dst.parse::<i64>()) else {
            skipped += 1;
            continue;
        };
        rows.push((kind, src, dst));
    }
    (rows, skipped)
}

/// One delta operation: a row appearing or going away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Op {
    /// Whether the row appeared.
    pub add: bool,
    /// The row.
    pub row: Row,
}

impl Op {
    /// An addition.
    #[must_use]
    pub fn add(row: Row) -> Self {
        Self { add: true, row }
    }

    /// A removal.
    #[must_use]
    pub fn remove(row: Row) -> Self {
        Self { add: false, row }
    }
}

/// The interned vertex dictionary, on the model side.
///
/// Ids come from the store's `graph_node` table, but the builder has to be usable without a store —
/// as a pure function, in a property test, and in the harness before a store exists — so the
/// dictionary is mirrored here and flushed through [`fathom_store::Store::graph_intern`] when there
/// is one. Ids already issued never move: a moved id would silently change what every stored chunk
/// means.
/// A freshly-minted dictionary entry: what the store needs to be told, and nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshEntry {
    /// The vertex key.
    pub key: String,
    /// The id the lane minted for it — the lane is the authority until it is written, so the store
    /// is not free to hand out a different number and desynchronise the two.
    pub id: i64,
    /// `module` or `symbol`.
    pub kind: &'static str,
    /// The layer tag as of this build.
    pub layer: String,
    /// Whether its declaration list may have shrunk rather than shrunk.
    pub incomplete: bool,
}

/// The interned vertex dictionary, on the model side.
///
/// Ids come from the store's `graph_node` table, but the builder has to be usable without a store —
/// as a pure function, in a property test, and in the harness before a store exists — so the
/// dictionary is mirrored here and flushed through [`fathom_store::Store::graph_intern`] when there
/// is one. Ids already issued never move: a moved id would silently change what every stored chunk
/// means.
#[derive(Debug, Clone, Default)]
pub struct Interner {
    ids: BTreeMap<String, i64>,
    by_id: BTreeMap<i64, String>,
    next: i64,
    fresh: Vec<FreshEntry>,
}

impl Interner {
    /// A dictionary seeded from existing `(key, id)` pairs.
    #[must_use]
    pub fn seeded<'a>(rows: impl Iterator<Item = (&'a str, i64)>) -> Self {
        let mut interner = Self::default();
        for (key, id) in rows {
            interner.ids.insert(key.to_string(), id);
            interner.by_id.insert(id, key.to_string());
            interner.next = interner.next.max(id + 1);
        }
        interner
    }

    /// The id of `key`, minting one if this is the first sighting.
    ///
    /// The kind and the two tags are carried so the lane can hand the store the same list it minted:
    /// `layer` because `[layers]` is config and config moves, `incomplete` because a module's
    /// declaration list may have shrunk rather than shrunk and a reader has to be told.
    #[must_use]
    pub fn id(&mut self, key: &str, kind: &'static str, layer: &str, incomplete: bool) -> i64 {
        if let Some(id) = self.ids.get(key) {
            return *id;
        }
        let id = self.next;
        self.next += 1;
        self.ids.insert(key.to_string(), id);
        self.by_id.insert(id, key.to_string());
        self.fresh.push(FreshEntry {
            key: key.to_string(),
            id,
            kind,
            layer: layer.to_string(),
            incomplete,
        });
        id
    }

    /// The id of `key` if it is already known.
    #[must_use]
    pub fn peek(&self, key: &str) -> Option<i64> {
        self.ids.get(key).copied()
    }

    /// The key an id was minted for. A materialised graph is ids and every answer is a path, so this
    /// is the read side of the same dictionary.
    #[must_use]
    pub fn key_of(&self, id: i64) -> Option<&str> {
        self.by_id.get(&id).map(String::as_str)
    }

    /// How many vertices the dictionary knows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether the dictionary is empty. Required by the lint that requires `len`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The entries minted since the last flush — what the lane hands to the store.
    #[must_use]
    pub fn fresh(&self) -> &[FreshEntry] {
        &self.fresh
    }

    /// Forget the minted-but-unflushed list, after it has been written.
    pub fn mark_flushed(&mut self) {
        self.fresh.clear();
    }
}

/// The upper bound of the kind range, for the `(endpoint, kind)` index lookups.
const KIND_MAX: RowKind = RowKind::Wildcard;

/// A materialised revision's graph: the state a query is asked in.
///
/// Both directions are indexed, and each is keyed `(endpoint, kind)` so that "every dependency of
/// X" is a prefix range rather than a scan. `s3`'s budget list spends `query.graph_dependents.ms` —
/// fan-*in* — at the same 20 ms as fan-out, and a structure that indexes only the source side makes
/// that a scan. Indexing both is a property of the data structure, not a comment about it.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    nodes: BTreeMap<i64, RowKind>,
    edges: BTreeSet<Row>,
    out: BTreeMap<(i64, RowKind), BTreeSet<i64>>,
    inc: BTreeMap<(i64, RowKind), BTreeSet<i64>>,
}

impl Graph {
    /// A graph from a row set.
    #[must_use]
    pub fn from_rows(rows: impl IntoIterator<Item = Row>) -> Self {
        let mut graph = Self::default();
        for row in rows {
            graph.apply_row(Op::add(row));
        }
        graph
    }

    /// Apply one operation.
    pub fn apply_row(&mut self, op: Op) {
        let (kind, src, dst) = op.row;
        if kind.is_node() {
            if op.add {
                self.nodes.insert(src, kind);
            } else {
                self.nodes.remove(&src);
            }
            return;
        }
        if op.add {
            self.edges.insert((kind, src, dst));
            self.out.entry((src, kind)).or_default().insert(dst);
            self.inc.entry((dst, kind)).or_default().insert(src);
            return;
        }
        self.edges.remove(&(kind, src, dst));
        if let Some(bucket) = self.out.get_mut(&(src, kind)) {
            bucket.remove(&dst);
            if bucket.is_empty() {
                self.out.remove(&(src, kind));
            }
        }
        if let Some(bucket) = self.inc.get_mut(&(dst, kind)) {
            bucket.remove(&src);
            if bucket.is_empty() {
                self.inc.remove(&(dst, kind));
            }
        }
    }

    /// Apply a batch.
    pub fn apply(&mut self, ops: impl IntoIterator<Item = Op>) {
        for op in ops {
            self.apply_row(op);
        }
    }

    /// The vertices, with the kind each was declared as.
    #[must_use]
    pub fn nodes(&self) -> &BTreeMap<i64, RowKind> {
        &self.nodes
    }

    /// The edge rows.
    #[must_use]
    pub fn edges(&self) -> &BTreeSet<Row> {
        &self.edges
    }

    /// How many nodes the revision has.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// How many edges the revision has.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Every row: nodes first in id order, then edges. This is the order a chunk payload is written
    /// in, and therefore part of the reproducibility contract.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        self.nodes
            .iter()
            .map(|(id, kind)| (*kind, *id, 0))
            .chain(self.edges.iter().copied())
            .collect()
    }

    /// Every edge leaving `src` whose kind satisfies `kinds`.
    #[must_use]
    pub fn reached(&self, src: i64, kinds: impl Fn(RowKind) -> bool) -> Vec<(RowKind, i64)> {
        let mut out = Vec::new();
        for ((_, kind), dsts) in self.out.range((src, RowKind::Module)..=(src, KIND_MAX)) {
            if kinds(*kind) {
                out.extend(dsts.iter().map(|dst| (*kind, *dst)));
            }
        }
        out.sort();
        out
    }

    /// Every edge arriving at `dst` whose kind satisfies `kinds`.
    #[must_use]
    pub fn arrived(&self, dst: i64, kinds: impl Fn(RowKind) -> bool) -> Vec<(RowKind, i64)> {
        let mut out = Vec::new();
        for ((_, kind), srcs) in self.inc.range((dst, RowKind::Module)..=(dst, KIND_MAX)) {
            if kinds(*kind) {
                out.extend(srcs.iter().map(|src| (*kind, *src)));
            }
        }
        out.sort();
        out
    }

    /// The outgoing buckets for one endpoint, for callers that want them raw.
    pub fn outgoing_at(&self, src: i64) -> impl Iterator<Item = (RowKind, &BTreeSet<i64>)> {
        self.out
            .range((src, RowKind::Module)..=(src, KIND_MAX))
            .map(|((_, kind), dsts)| (*kind, dsts))
    }

    /// The incoming buckets for one endpoint, for callers that want them raw.
    pub fn incoming_at(&self, dst: i64) -> impl Iterator<Item = (RowKind, &BTreeSet<i64>)> {
        self.inc
            .range((dst, RowKind::Module)..=(dst, KIND_MAX))
            .map(|((_, kind), srcs)| (*kind, srcs))
    }

    /// Whether the graph holds this exact row.
    #[must_use]
    pub fn holds(&self, row: Row) -> bool {
        if row.0.is_node() {
            return self.nodes.contains_key(&row.1);
        }
        self.edges.contains(&row)
    }

    /// Edges whose destination was never declared as a node — the dangling set.
    ///
    /// This is a *stated* state, not an error: `uses crates/x.rs::Thing` says the source named a
    /// symbol, and whether the corpus confirms it is a fact about the repository. `e3.6`'s dead-code
    /// and `e3.7`'s API-surface answers both have to say "named, not found" out loud, so the graph
    /// keeps the name rather than dropping the row.
    #[must_use]
    pub fn dangling(&self) -> Vec<Row> {
        self.edges
            .iter()
            .filter(|(_, _, dst)| !self.nodes.contains_key(dst))
            .copied()
            .collect()
    }
}

/// One dictionary entry as a reader sees it.
///
/// The store's own row type is `fathom_store::graph::Node`; this is the analyzer-side shape, so the
/// always-compiled half of this module does not depend on a SQLite backend being linked (ADR-002
/// §6.1 — a `--no-default-features` build still gets the graph model and the queries, and simply has
/// no store to read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vertex {
    /// The interned id.
    pub id: i64,
    /// The module path, or the `path::Symbol` spelling.
    pub key: String,
    /// Module or symbol.
    pub kind: RowKind,
    /// The `[layers]` tag; empty when untagged.
    pub layer: String,
    /// Whether the declaration list may have shrunk rather than shrunk.
    pub incomplete: bool,
}

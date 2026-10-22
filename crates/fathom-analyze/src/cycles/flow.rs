//! The adjacency a cycle question walks, in one view, kept up to date by row ops.
//!
//! [`crate::graph::model::Graph`] is the whole revision — nodes, contains-edges, symbol edges, every
//! row kind — and it is the right structure for "what depends on X". It is the wrong structure for
//! "which modules are in a cycle with which", for two reasons:
//!
//! * A cycle question only ever follows [`crate::graph::model::RowKind::is_dependency`] rows, and it
//!   follows them between *the things a cycle can be about*. Walking `contains` as well would make
//!   every module a cycle with its own exports.
//! * In **module** view the unit of the answer is a module, and a dependency that names a symbol
//!   (`uses crates/x.rs::Thing`) is a dependency on `crates/x.rs`. Projected that way, several
//!   symbol-level edges collapse into one module edge — so an edge here carries a **count**, and
//!   removing one of the symbol rows that projected onto it must not delete the module edge the
//!   others still hold.
//!
//! That collapsing is the whole of [`View::Module`], and it is why the two views cost different
//! amounts of work rather than different amounts of code: the same strongly-connected-component
//! machinery runs over whichever one was asked for ([`View::Symbol`] is the identity projection).
//!
//! # A cycle says how fine its own claim is
//!
//! Spike `s1`'s finding 4, in the ticket's own words: *a cycle drawn between Go files is a cycle the
//! language does not have*. A `imports_package` row is a package-to-package fact that happens to be
//! recorded per file, so a cycle whose interior crosses one is a claim about **packages**, and it is
//! printed at package granularity with that stated. [`Flow`] keeps the package-granular pairs apart
//! for exactly this, and the same argument keeps `wildcard` pairs (`unknown`: contents unread) apart
//! from the confident ones. Neither is a caveat appended to a sentence nobody reads; both are fields
//! on the [`Cycle`](super::cycle::Cycle) that a surface prints.
//!
//! # What is not here
//!
//! Endpoints the dictionary cannot name. A `uses` row that points at a symbol nobody ever interned is
//! a dependency that left the corpus (ADR-004 §8.5's "unknown, not none"), it has no out-edges, and it
//! cannot be in a cycle; it is counted in [`Flow::unnamed`] rather than invented as a node.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::graph::model::{Graph, Op, RowKind};
use crate::graph::query::{Naming, module_of};

/// Which graph a cycle question is asked against.
///
/// The default is [`View::Module`] and that is a decision, not a default value: the epic's question is
/// "did this turn make the architecture worse", and an import cycle between two *files* that name
/// each other's symbols is the same architectural fact as one module importing another. The symbol
/// view is opt-in (`e3.5`: *"with an opt-in symbol-level view"*) because it is a different question —
/// *which declarations are the ones that close the loop* — and it costs a second partition to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum View {
    /// Cycles between modules: symbol endpoints are projected onto the module that owns them.
    #[default]
    Module,
    /// Cycles between modules *and* symbols, at the granularity the extractor resolved.
    Symbol,
}

impl View {
    /// The stored and printed spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            View::Module => "module",
            View::Symbol => "symbol",
        }
    }

    /// Parse the stored spelling, `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "module" => Some(View::Module),
            "symbol" => Some(View::Symbol),
            _ => None,
        }
    }

    /// Whether this view keeps symbol nodes as themselves.
    #[must_use]
    pub const fn is_symbol(self) -> bool {
        matches!(self, View::Symbol)
    }
}

/// How fine a cycle's claim is, which is a fact about its edges and not about who is asking.
///
/// [`Claim::Package`] is the honest label for a cycle that contains a package-granular edge: the
/// language said *package A depends on package B*, and a pane that draws a file-to-file loop through
/// it is reporting a claim nobody made. A cycle with both file-granular and package-granular edges is
/// [`Claim::Package`] — a chain is as fine as its coarsest link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Claim {
    /// Every edge in the cycle is a file-to-file fact in the language's own terms.
    #[default]
    File,
    /// At least one edge is package-to-package (Go), so the cycle is a package-level fact.
    Package,
}

impl Claim {
    /// The stored and printed spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Claim::File => "file",
            Claim::Package => "package",
        }
    }

    /// Widen a claim: one package-granular edge makes the whole cycle package-granular.
    #[must_use]
    pub const fn widened(self, other: Self) -> Self {
        match (self, other) {
            (Claim::Package, _) | (_, Claim::Package) => Claim::Package,
            _ => Claim::File,
        }
    }
}

/// The directory a module belongs to — the package a Go cycle is *about*.
///
/// The last path segment is the file, so the package is everything before it. A module at the
/// repository root has no directory and is named by its own path rather than by the empty string,
/// because a cycle key that is the empty string matches every other empty string.
#[must_use]
pub fn package_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) if at > 0 => &path[..at],
        _ => path,
    }
}

/// The dependency adjacency of one revision, in one view, with the bookkeeping an update needs.
///
/// Built once from a materialised graph and then moved forward by row ops, which is what makes "the
/// partition is maintained, not recomputed" a property of the data structure rather than of the
/// caller's discipline.
#[derive(Debug, Clone)]
pub struct Flow {
    view: View,
    /// Every node this view can name, in id order.
    nodes: BTreeSet<i64>,
    /// The subset of `nodes` a *node* row declared. The difference matters on deletion: an endpoint a
    /// dependency row merely named disappears with its last edge, while a declared module stays a node
    /// whether or not anything still imports it. Cold, that rule falls out of rebuilding; maintained,
    /// it has to be a rule, or the two answers drift.
    declared: BTreeSet<i64>,
    /// Successors with multiplicity: how many graph rows project onto the pair.
    succ: BTreeMap<i64, BTreeMap<i64, u32>>,
    /// The same pairs the other way round, because a split search and a merge search run in opposite
    /// directions and neither may be a scan.
    pred: BTreeMap<i64, BTreeMap<i64, u32>>,
    /// Pairs that arrived on a package-granular row.
    package: BTreeSet<(i64, i64)>,
    /// Pairs that arrived on a wildcard row: a real dependency whose contents were never read.
    unknown: BTreeSet<(i64, i64)>,
    /// Nodes with a dependency row pointing at themselves. A one-node cycle is still a cycle.
    self_loops: BTreeSet<i64>,
    /// Endpoint ids named by a row the dictionary could not name — recorded, never guessed. A set
    /// rather than a counter, because a module with forty rows to one unknown endpoint is one unknown
    /// endpoint, not forty, and `unnamed()` is a claim about the graph's vocabulary.
    refused: BTreeSet<i64>,
    /// Symbol-level edges that collapsed into a module's own self-edge. Not cycles: a module that
    /// uses its own symbol has not gone round a loop.
    internal: usize,
    /// The projection cache: graph node id → the node this view asks about, or `None` when the
    /// endpoint has no name at all. Negative results are cached too, because a build walks the same
    /// dangling endpoint many times.
    projection: BTreeMap<i64, Option<i64>>,
}

impl Flow {
    /// Project the revision's whole dependency adjacency into `view`.
    #[must_use]
    pub fn build(graph: &Graph, naming: &Naming, view: View) -> Self {
        let mut flow = Self {
            view,
            ..Default::default()
        };
        // Nodes first, so a module with no edges is still a node it can be asked about: the graph's
        // own rule is that an unreadable file is a bare node rather than an absence.
        for id in graph.nodes().keys().copied() {
            if let Some(node) = flow.project(naming, id) {
                flow.declared.insert(node);
                flow.nodes.insert(node);
            }
        }
        for (kind, src, dst) in graph.edges() {
            if !kind.is_dependency() {
                continue;
            }
            flow.insert_edge(*kind, *src, *dst, naming);
        }
        flow
    }

    /// A flow with nothing in it but the view tag.
    #[must_use]
    pub fn empty(view: View) -> Self {
        Self {
            view,
            ..Default::default()
        }
    }

    /// The view this adjacency is projected into.
    #[must_use]
    pub const fn view(&self) -> View {
        self.view
    }

    /// The nodes this view can name.
    #[must_use]
    pub fn nodes(&self) -> &BTreeSet<i64> {
        &self.nodes
    }

    /// How many dependency pairs the flow holds.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.succ.values().map(BTreeMap::len).sum()
    }

    /// The successors of one node, in id order.
    pub fn successors(&self, id: i64) -> impl Iterator<Item = i64> + '_ {
        self.succ
            .get(&id)
            .into_iter()
            .flat_map(|map| map.keys().copied())
    }

    /// The predecessors of one node, in id order.
    pub fn predecessors(&self, id: i64) -> impl Iterator<Item = i64> + '_ {
        self.pred
            .get(&id)
            .into_iter()
            .flat_map(|map| map.keys().copied())
    }

    /// Whether this pair is an edge right now.
    #[must_use]
    pub fn holds(&self, from: i64, to: i64) -> bool {
        self.succ
            .get(&from)
            .is_some_and(|row| row.contains_key(&to))
    }

    /// The nodes that depend on themselves.
    #[must_use]
    pub fn self_loops(&self) -> &BTreeSet<i64> {
        &self.self_loops
    }

    /// Whether this pair arrived on a package-granular row.
    #[must_use]
    pub fn is_package(&self, from: i64, to: i64) -> bool {
        self.package.contains(&(from, to))
    }

    /// Whether this pair arrived on a wildcard row.
    #[must_use]
    pub fn is_unknown(&self, from: i64, to: i64) -> bool {
        self.unknown.contains(&(from, to))
    }

    /// How many endpoints could not be named. A non-zero answer is a stated partial.
    #[must_use]
    pub fn unnamed(&self) -> usize {
        self.refused.len()
    }

    /// How many symbol-level edges collapsed into one module's own self-edge.
    #[must_use]
    pub const fn internal(&self) -> usize {
        self.internal
    }

    /// The display key of a node in this view: a module path in [`View::Module`], the `path::Symbol`
    /// spelling in [`View::Symbol`], and `#<id>` for the nameless (which cannot be in a cycle, but can
    /// be named in a diagnostic).
    #[must_use]
    pub fn name(&self, naming: &Naming, id: i64) -> String {
        match naming.name(id) {
            Some(key) => match self.view {
                View::Module => module_of(key),
                View::Symbol => key.to_string(),
            },
            None => format!("#{id}"),
        }
    }

    /// The unit a cycle in this view is drawn in: `module`, `symbol`, or `package` once a
    /// package-granular edge widens the claim (the caller picks the third; this names the first two).
    #[must_use]
    pub const fn unit(&self) -> &'static str {
        match self.view {
            View::Module => "module",
            View::Symbol => "symbol",
        }
    }

    // -- updates ------------------------------------------------------------------
    //
    // Split in two on purpose. A removal has to be reflected in the flow *before* the partition
    // re-splits the region it broke (the split reads the post-removal adjacency), and an addition has
    // to be reflected *after* the partition has decided what the new edge does — deciding against a
    // flow that already contains the edge would find a path back along the edge itself and merge two
    // components that were never mutually reachable. [`super::track`] interleaves the two calls.

    /// Apply every removal in `ops`.
    pub fn apply_removals(&mut self, ops: &[Op], naming: &Naming) {
        for op in ops.iter().filter(|op| !op.add) {
            self.apply_one(*op, naming);
        }
    }

    /// Apply the node additions in `ops`, before the edge additions that may refer to them.
    pub fn apply_node_additions(&mut self, ops: &[Op], naming: &Naming) {
        for op in ops.iter().filter(|op| op.add && op.row.0.is_node()) {
            self.apply_one(*op, naming);
        }
    }

    /// Drop the nodes that existed only as an endpoint of a row that is now gone, and return them.
    ///
    /// A dependency row can name something no row ever declared — the target of an unresolved import, a
    /// symbol in a file nobody read. Cold, such a node exists only while an edge holds it; maintained, it
    /// would outlive its last edge and the maintained answer would keep a node a fresh computation does
    /// not have. Called after every removal in a revision is applied, and the returned ids go to
    /// [`crate::cycles::scc::Partition`] as removals of their own.
    pub fn prune_endpoints(&mut self) -> Vec<i64> {
        let gone: Vec<i64> = self
            .nodes
            .iter()
            .filter(|node| {
                !self.declared.contains(node)
                    && !self.succ.contains_key(node)
                    && !self.pred.contains_key(node)
                    && !self.self_loops.contains(node)
            })
            .copied()
            .collect();
        for node in &gone {
            self.nodes.remove(node);
            self.self_loops.remove(node);
        }
        gone
    }

    /// Whether this view can name `node` at all — declared or merely named by a row still standing.
    #[must_use]
    pub fn has_node(&self, node: i64) -> bool {
        self.nodes.contains(&node)
    }

    /// The pair a dependency row projects to in this view, or `None` when either half is nameless.
    /// A `src == dst` row projects to a pair with both halves equal, which is a one-node cycle.
    #[must_use]
    pub fn pair_of(&self, naming: &Naming, src: i64, dst: i64) -> Option<(i64, i64)> {
        self.project_pair(naming, src, dst)
    }

    /// The one-node cycles `ops` adds and removes, as view nodes.
    ///
    /// Split out because the partition cannot see a self-loop at all — a component of one node is a
    /// component of one node whether or not it points at itself — and a cycle record has to be
    /// re-derived for the component that lost one.
    #[must_use]
    pub fn self_loop_changes(&self, ops: &[Op], naming: &Naming) -> (Vec<i64>, Vec<i64>) {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        for op in ops {
            let (kind, src, dst) = op.row;
            if !kind.is_dependency() || src != dst {
                continue;
            }
            let Some(node) = self.project_of(naming, src) else {
                continue;
            };
            if op.add {
                added.push(node);
            } else {
                removed.push(node);
            }
        }
        added.sort_unstable();
        removed.sort_unstable();
        (added, removed)
    }

    /// Apply one addition.
    pub fn apply_addition(&mut self, op: Op, naming: &Naming) {
        if op.add {
            self.apply_one(op, naming);
        }
    }

    /// Apply every addition in `ops`, in the order given.
    pub fn apply_additions(&mut self, ops: &[Op], naming: &Naming) {
        for op in ops.iter().filter(|op| op.add) {
            self.apply_one(*op, naming);
        }
    }

    /// The removed dependency pairs in `ops`, projected into this view, in the order they appear.
    #[must_use]
    pub fn removed_pairs(&self, ops: &[Op], naming: &Naming) -> Vec<(i64, i64)> {
        self.pairs(ops, naming, false)
    }

    /// The added dependency pairs in `ops`, projected into this view, sorted.
    #[must_use]
    pub fn added_pairs(&self, ops: &[Op], naming: &Naming) -> Vec<(i64, i64)> {
        let mut pairs = self.pairs(ops, naming, true);
        pairs.sort_unstable();
        pairs
    }

    /// The nodes `ops` adds and removes in this view, which is what the partition grows and shrinks by.
    #[must_use]
    pub fn node_changes(&mut self, ops: &[Op], naming: &Naming) -> (Vec<i64>, Vec<i64>) {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        for op in ops {
            let (kind, src, _) = op.row;
            if !kind.is_node() {
                continue;
            }
            let Some(node) = self.project(naming, src) else {
                continue;
            };
            if op.add {
                added.push(node);
            } else {
                removed.push(node);
            }
        }
        added.sort_unstable();
        removed.sort_unstable();
        (added, removed)
    }

    /// The dependency rows `ops` adds or removes, as projected pairs.
    fn pairs(&self, ops: &[Op], naming: &Naming, want_add: bool) -> Vec<(i64, i64)> {
        let mut pairs = Vec::new();
        for op in ops {
            if op.add != want_add {
                continue;
            }
            let (kind, src, dst) = op.row;
            if !kind.is_dependency() {
                continue;
            }
            if let Some(pair) = self.project_pair(naming, src, dst) {
                pairs.push(pair);
            }
        }
        pairs
    }

    /// Apply one row.
    fn apply_one(&mut self, op: Op, naming: &Naming) {
        let (kind, src, dst) = op.row;
        if kind.is_node() {
            if let Some(node) = self.project(naming, src) {
                if op.add {
                    self.declared.insert(node);
                    self.nodes.insert(node);
                } else {
                    self.declared.remove(&node);
                    self.nodes.remove(&node);
                    self.self_loops.remove(&node);
                }
            }
            return;
        }
        if !kind.is_dependency() {
            return;
        }
        if src == dst {
            // A row that points at its own subject. Projected it is still a one-node loop in either
            // view, and a one-node loop is a cycle: `import "./self"` is a cycle the reviewer needs
            // to see, not a rounding error.
            if let Some(node) = self.project(naming, src) {
                if op.add {
                    self.nodes.insert(node);
                    self.self_loops.insert(node);
                } else {
                    self.self_loops.remove(&node);
                }
            }
            return;
        }
        let (Some(from), Some(to)) = (self.project(naming, src), self.project(naming, dst)) else {
            // A dependency that leaves the corpus: named by the row, unknown to the dictionary, and
            // counted as such rather than drawn to an invented node.
            return;
        };
        if from == to {
            // Two symbols in the same module: an edge inside a node, not a loop through other nodes.
            if op.add {
                self.internal += 1;
            } else {
                self.internal = self.internal.saturating_sub(1);
            }
            return;
        }
        let pair = (from, to);
        self.book(kind, pair, op.add);
        if op.add {
            *self.succ.entry(from).or_default().entry(to).or_insert(0) += 1;
            *self.pred.entry(to).or_default().entry(from).or_insert(0) += 1;
            self.nodes.insert(from);
            self.nodes.insert(to);
            return;
        }
        let Some(row) = self.succ.get_mut(&from) else {
            return;
        };
        let Some(count) = row.get_mut(&to) else {
            return;
        };
        *count = count.saturating_sub(1);
        // The reverse map gives up one count per row, not one per pair: several symbol-level rows can
        // project onto the same module edge, and if the backward side only shed its share on the last
        // row it would keep an edge the forward side no longer has. A split search runs backward, and a
        // stale reverse edge is a component that refuses to come apart.
        if let Some(back) = self.pred.get_mut(&to) {
            if let Some(reverse) = back.get_mut(&from) {
                *reverse = reverse.saturating_sub(1);
                if *reverse == 0 {
                    back.remove(&from);
                }
            }
            if back.is_empty() {
                self.pred.remove(&to);
            }
        }
        if *count > 0 {
            // The pair still has another row behind it, so the forward side keeps it standing.
            return;
        }
        row.remove(&to);
        if row.is_empty() {
            self.succ.remove(&from);
        }
    }

    /// Track which claim an edge carries, so a cycle can report its own.
    fn book(&mut self, kind: RowKind, pair: (i64, i64), add: bool) {
        match kind {
            RowKind::ImportsPackage => match add {
                true => {
                    self.package.insert(pair);
                }
                false => {
                    self.package.remove(&pair);
                }
            },
            RowKind::Wildcard => match add {
                true => {
                    self.unknown.insert(pair);
                }
                false => {
                    self.unknown.remove(&pair);
                }
            },
            _ => {}
        }
    }

    /// Insert one graph edge during [`Flow::build`].
    fn insert_edge(&mut self, kind: RowKind, src: i64, dst: i64, naming: &Naming) {
        self.apply_one(Op::add((kind, src, dst)), naming);
    }

    /// The view's node a graph node stands for, or `None` when it has no name.
    ///
    /// The cache holds the refusals as well as the answers, and the refusal is what makes
    /// [`Flow::unnamed`] count each nameless endpoint once instead of once per row that mentions it.
    fn project(&mut self, naming: &Naming, id: i64) -> Option<i64> {
        if let Some(known) = self.projection.get(&id) {
            return *known;
        }
        let projected = self.project_of(naming, id);
        if projected.is_none() {
            self.refused.insert(id);
        }
        self.projection.insert(id, projected);
        projected
    }

    /// The projection itself, without the cache: read-only, so the pair helpers can use it.
    fn project_of(&self, naming: &Naming, id: i64) -> Option<i64> {
        match self.view {
            View::Symbol => Some(id),
            View::Module => {
                let key = naming.name(id)?;
                let path = module_of(key);
                Some(naming.id(&path).unwrap_or(id))
            }
        }
    }

    /// A pair, projected, or `None` when either half is nameless.
    fn project_pair(&self, naming: &Naming, src: i64, dst: i64) -> Option<(i64, i64)> {
        Some((self.project_of(naming, src)?, self.project_of(naming, dst)?))
    }
}

impl Default for Flow {
    fn default() -> Self {
        Self {
            view: View::Module,
            nodes: BTreeSet::new(),
            declared: BTreeSet::new(),
            succ: BTreeMap::new(),
            pred: BTreeMap::new(),
            package: BTreeSet::new(),
            unknown: BTreeSet::new(),
            self_loops: BTreeSet::new(),
            refused: BTreeSet::new(),
            internal: 0,
            projection: BTreeMap::new(),
        }
    }
}

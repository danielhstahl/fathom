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
//! # A module owns the directory named after it
//!
//! One step further than "fold the symbols into their file": in Rust, `drift.rs` *is* the module
//! `drift`, and so is every file under `drift/`, because the language puts a module's submodules in a
//! directory named after it (`scorecard/mod.rs` is the other spelling of the same rule). The store
//! keys a row on the file, so `use super::detect::…` inside `drift.rs` arrives as a dependency of
//! `drift.rs` on `drift/detect.rs` — one module depending on itself under two names — and a cycle
//! drawn between the two is an architecture violation the language does not have. [`owning_module`]
//! folds a module's own directory into the module, which is what makes the fold in [`Flow`] the
//! language's and not an invented granularity; it is keyed to the Rust layout, and the function's own
//! doc says which layouts it declines to fold and why.
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

use crate::graph::model::{Graph, Op, Row, RowKind};
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

/// The directory basename a module's own directory would have, and its parent.
///
/// `src/drift/detect.rs` splits into `("src/drift", "detect")`; a repository-root file splits into
/// `("", "main")`.
fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(at) => (&path[..at], &path[at + 1..]),
        None => ("", path),
    }
}

/// The module a module path is *inside*, in the sense a reader means by "module", or `None` when the
/// path is its own module.
///
/// Rust gives a module two layouts and both of them put the module's submodules in a directory named
/// after it: `d/x.rs` is module `d::x`, and so is everything under `d/x/`; `d/mod.rs` is module `d`,
/// and so is everything under `d/`. The extractor keys a row on the file, which is the finest grain
/// the store can hold, so `use super::detect::…` in `drift.rs` arrives as a dependency of `drift.rs`
/// on `drift/detect.rs` — a module depending on itself, wearing two file names. Printed as a cycle
/// it is an architecture violation the language does not have, and on a real repository it is the
/// common case rather than the curiosity: every one of the ten Rust cycles in the 13 that `fathom
/// ratchet` reported on Fathom's own source tree was this (`reco-wk4`).
///
/// So the module view folds a module's own directory into the module: the answer is drawn between
/// *modules*, and this is the function that says which files are one module. Three things keep it
/// honest about its own reach:
///
/// * It fires only where the layout asserts the relationship — `d/x/` folds into `d/x.rs` only when
///   `d/x.rs` is a module of this revision, and `d/` folds into `d/mod.rs` only when it is. `present`
///   is that revision's module paths, not the store's dictionary, which remembers every file the
///   repository ever had: folding a live directory under a module that was deleted three months ago
///   is a wrong answer that prints as a plausible file name.
/// * It does not fire for `lib.rs` and `main.rs`. Those are crate roots, and `src/` is not a module
///   named `src` — folding them would fold a whole crate into one node and end the architecture
///   question with it.
/// * It does not fire for a Python package's `__init__.py`, a Java package directory, a Go package
///   directory or a TypeScript barrel. There the directory is a namespace the language does not fold:
///   two sibling modules that import each other are two importable units closing a loop, and in
///   Python a circular import is a runtime defect rather than a layout artefact. (Go is the
///   exception with its own machinery: it records dependency facts package-granular, which is
///   [`package_of`]'s business.) A repository whose non-Rust cycles are fixtures says so on the
///   ratchet ledger, which is what a ledger is for.
///
/// The fine grain the fold gives up is not lost, only demoted: a loop that stays inside one module is
/// not an architecture claim, and the opt-in [`View::Symbol`] still names the declarations that close
/// it.
#[must_use]
pub fn owning_module(present: &BTreeSet<String>, path: &str) -> Option<String> {
    let (dir, base) = split_path(path);
    // The Rust layout only, and `.rs` is how the corpus spells it.
    if !base.ends_with(".rs") {
        return None;
    }
    let mut dir = dir;
    loop {
        // `d/mod.rs` *is* the module named `d`, so everything under `d/` is inside it.
        let declared = if dir.is_empty() {
            "mod.rs".to_string()
        } else {
            format!("{dir}/mod.rs")
        };
        if declared != path && present.contains(&declared) {
            return Some(declared);
        }
        // …and `d/` is a module's own directory when `d.rs` sits beside it in the parent.
        let (parent, name) = split_path(dir);
        if dir.is_empty() || name.is_empty() {
            return None;
        }
        let namesake = if parent.is_empty() {
            format!("{name}.rs")
        } else {
            format!("{parent}/{name}.rs")
        };
        if namesake != path && present.contains(&namesake) {
            return Some(namesake);
        }
        dir = parent;
    }
}

/// The directory a module of this layout owns: the paths under it are inside the module.
///
/// `d/x.rs` owns `d/x/`, and `d/mod.rs` owns `d/`. `None` for a path that owns no directory, which is
/// what makes [`owning_module`] a lookup rather than a guess.
#[must_use]
fn owned_directory(path: &str) -> Option<String> {
    let (dir, base) = split_path(path);
    if base == "mod.rs" {
        return if dir.is_empty() {
            None
        } else {
            Some(format!("{dir}/"))
        };
    }
    let stem = base.strip_suffix(".rs")?;
    Some(if dir.is_empty() {
        format!("{stem}/")
    } else {
        format!("{dir}/{stem}/")
    })
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
    /// Edges that collapsed into one module's own self-edge: a symbol using its own module, and a
    /// module file reading a submodule of itself ([`owning_module`]). Not cycles — a module that uses
    /// itself has not gone round a loop.
    internal: usize,
    /// The module paths this revision has, which is what decides whether a directory is a module's own
    /// ([`owning_module`]). The revision's, not the dictionary's: the dictionary keeps every file the
    /// repository ever had, and a live directory must not fold under a module that was deleted last
    /// month.
    module_paths: BTreeSet<String>,
    /// The dependency rows this adjacency was built from, at graph grain, so the fold can be re-derived
    /// when the module set moves. Module view only — the symbol view folds nothing, so it pays nothing.
    source_rows: BTreeSet<Row>,
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
        // own rule is that an unreadable file is a bare node rather than an absence. The module paths
        // go in in their own pass, before any edge or any projection, because the fold is decided from
        // the layout and the layout is only complete once every node has been noted: interleaving the
        // two would make a module's owner depend on the order the dictionary happened to mint.
        if view == View::Module {
            for (&id, &kind) in graph.nodes() {
                if kind != RowKind::Module {
                    continue;
                }
                if let Some(key) = naming.name(id) {
                    flow.module_paths.insert(module_of(key));
                }
            }
        }
        for &id in graph.nodes().keys() {
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
            self.note_module(kind, src, op.add, naming);
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
        if self.view == View::Module {
            // Keep the row at graph grain. The fold is a function of the row set and the module set
            // together, so keeping the rows is what makes [`Flow::reproject`] possible when the module
            // set moves — and keeping them only in the view that folds is what keeps the symbol view
            // at the cost it had before.
            match op.add {
                true => {
                    self.source_rows.insert(op.row);
                }
                false => {
                    self.source_rows.remove(&op.row);
                }
            }
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

    /// Note that a module arrived or left, and re-derive the fold if that moves it for anything else.
    ///
    /// The only event that can change which module a file is inside is a module file arriving or going
    /// away, and the only ones that matter are the ones that own a directory somebody else is standing
    /// in: `d/x.rs` arriving next to `d/x/`, or `d/mod.rs` going out from under `d/`. Everything else
    /// about the adjacency is untouched, so the rebuild is gated on exactly that question.
    fn note_module(&mut self, kind: RowKind, id: i64, add: bool, naming: &Naming) {
        if self.view != View::Module || kind != RowKind::Module {
            return;
        }
        let Some(path) = naming.name(id).map(module_of) else {
            return;
        };
        let moves = self.fold_moves(&path);
        if add {
            self.module_paths.insert(path);
        } else {
            self.module_paths.remove(&path);
        }
        if moves {
            self.reproject(naming);
        }
    }

    /// Whether the directory this module owns holds another present module — in which case arriving or
    /// departing changes what that one is inside of.
    #[must_use]
    fn fold_moves(&self, path: &str) -> bool {
        let Some(prefix) = owned_directory(path) else {
            return false;
        };
        self.module_paths
            .range(prefix.clone()..)
            .take_while(|other| other.starts_with(prefix.as_str()))
            .any(|other| other != path)
    }

    /// Re-derive the whole folded adjacency from the rows this flow holds.
    ///
    /// The fold is a function of two things — the dependency rows and which module files exist — and
    /// only the second one ever moves under a maintained flow. When it does, re-projecting the rows is
    /// the only answer that can be right: the adjacency has already forgotten which file each folded
    /// edge came from, so there is nothing to re-key with. That is a rebuild of one view, paid only by
    /// a revision that moves a module in or out of a directory, which is the same trade [`crate::cycles::scc`]
    /// makes everywhere else — an honest worst case rather than a per-edge tax or a wrong answer.
    fn reproject(&mut self, naming: &Naming) {
        let rows = std::mem::take(&mut self.source_rows);
        let paths: Vec<String> = self.module_paths.iter().cloned().collect();
        self.succ.clear();
        self.pred.clear();
        self.package.clear();
        self.unknown.clear();
        self.self_loops.clear();
        self.nodes.clear();
        self.declared.clear();
        self.refused.clear();
        self.internal = 0;
        // The cache is a projection of the module set, and the module set just moved. Keeping it is a
        // fold that froze at build time and then quietly disagreed with a cold rebuild.
        self.projection.clear();
        for path in paths {
            let Some(id) = naming.id(&path) else {
                continue;
            };
            if let Some(node) = self.project(naming, id) {
                self.declared.insert(node);
                self.nodes.insert(node);
            }
        }
        for row in rows {
            self.apply_one(Op::add(row), naming);
        }
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
                // One step more than "the module that owns this symbol": a module and the directory
                // named after it are one node too ([`owning_module`]), which is the difference between
                // a cycle and a module talking to itself.
                let owner =
                    owning_module(&self.module_paths, &path).unwrap_or_else(|| path.to_string());
                Some(naming.id(&owner).unwrap_or(id))
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
            module_paths: BTreeSet::new(),
            source_rows: BTreeSet::new(),
            projection: BTreeMap::new(),
        }
    }
}

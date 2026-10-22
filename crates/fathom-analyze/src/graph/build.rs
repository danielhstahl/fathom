//! Building one revision's graph out of the per-file structural substrate.
//!
//! The input is what `e1.4` and `e3.1` already keep: per file, its blob and its extracted
//! [`crate::structure::FileStructure`]. Nothing here re-reads a file or re-runs a grammar — the lane
//! is O(changed files) precisely because the substrate is blob-keyed, and that is the third item of
//! `s3`'s policy ("keep the per-file structural cache as the substrate").
//!
//! # What a module contributes
//!
//! One [`ModuleRows`] per module: its own node, one node per exported symbol, a `contains` edge to
//! each of them, and its outbound edges. That set is a function of *one file plus the interner*, and
//! it has to be — a delta that needed the whole corpus to compute one module's rows would be an
//! O(graph) per-revision cost wearing an O(changed) costume.
//!
//! # Why a symbol-naming dependency writes two rows
//!
//! `use crate::geo::point::Point` produces `imports M→geo/point.rs` *and*
//! `uses M→geo/point.rs::Point`. One row would be cheaper and would answer only one of the two
//! questions the pane asks: fan-in at module granularity (`what would break if that file moved`)
//! and reach into the API surface (`who uses `Point` specifically`). The second is the only edge
//! kind that reaches a symbol node from a dependency question, so dropping it would make the symbol
//! half of the graph decorative. `s3` measured 36.3 ops per commit at 2.56 files; the second row
//! per symbol edge is inside the budget it measured against.
//!
//! # What is *not* stored, and where it goes instead
//!
//! An edge whose target is outside the corpus (a std type, a third-party crate, an internal name the
//! corpus does not contain) writes no row — the extractor already recorded it as
//! [`crate::structure::EdgeKind::Unresolved`], and a graph edge to nowhere would be a second, worse
//! spelling of "outside". It is counted in [`BuildSummary::external`] so the revision says how much
//! it left out.

use std::collections::BTreeMap;

use crate::graph::delta::module_key;
use crate::graph::layers::Layers;
use crate::graph::model::{Graph, Interner, Row, RowKind, Vertex, encode_rows};
use crate::graph::policy::GraphPolicy;
use crate::graph::query::Naming;
use crate::structure::FileStructure;

/// What a build measured about itself.
///
/// Everything here is a fact a surface has to repeat to be honest: modules with nothing readable
/// behind them, modules whose declaration list may shrink, dependencies that leave the corpus. The
/// counters are also what `store.bytes_per_commit_graph.*` is argued from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildSummary {
    /// Modules considered.
    pub modules: usize,
    /// Modules whose structure could not be read at all.
    pub unavailable: usize,
    /// Modules whose declaration list may have shrunk rather than shrunk (`s1` finding 1).
    pub incomplete: usize,
    /// Edges that named a target outside the corpus, and so are stored as nothing.
    pub external: usize,
    /// Edges of the wildcard kind (`s1` finding 2).
    pub wildcard: usize,
    /// Edges that name a symbol the corpus never declared.
    pub dangling: usize,
    /// Nodes in the revision's graph.
    pub nodes: usize,
    /// Edges in the revision's graph.
    pub edges: usize,
    /// Files skipped by `graph.ignored_dirs`.
    pub ignored: usize,
}

impl BuildSummary {
    /// The one-line statement a surface prints beside a graph: what it does not know.
    #[must_use]
    pub fn caveat(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.ignored > 0 {
            parts.push(format!(
                "{} file(s) skipped as ignored directories",
                self.ignored
            ));
        }
        if self.unavailable > 0 {
            parts.push(format!(
                "{} module(s) had no structure to read",
                self.unavailable
            ));
        }
        if self.incomplete > 0 {
            parts.push(format!(
                "{} module(s) may have shrunk rather than shrunk",
                self.incomplete
            ));
        }
        if self.wildcard > 0 {
            parts.push(format!(
                "{} dependency(ies) have unknown contents",
                self.wildcard
            ));
        }
        if self.external > 0 {
            parts.push(format!(
                "{} dependency(ies) point outside the corpus",
                self.external
            ));
        }
        if self.dangling > 0 {
            parts.push(format!(
                "{} edge(s) name a symbol nothing declares",
                self.dangling
            ));
        }
        if parts.is_empty() {
            None
        } else {
            Some(format!("this graph is partial: {}.", parts.join("; ")))
        }
    }
}

/// The policy a build runs under, with the layer tags beside it.
#[derive(Debug, Clone, Default)]
pub struct Builder {
    /// The knobs, including `chain_full_every` and `max_chain_walk` for whoever stores the result.
    pub policy: GraphPolicy,
    /// `[layers]`, as tags.
    pub layers: Layers,
}

/// One module as the substrate hands it over.
#[derive(Debug, Clone, Copy)]
pub struct ModuleInput<'a> {
    /// Corpus-relative path — the module key.
    pub path: &'a str,
    /// The blob's content address: the substrate's key, and the thing a lane compares to decide
    /// whether this module changed at all.
    pub blob: &'a str,
    /// What the extractor said about it. `None` is the substrate saying "nothing", which is a
    /// different fact from an empty structure and is stored as a node with no edges.
    pub file: Option<&'a FileStructure>,
}

/// One module's rows, as computed for one revision.
#[derive(Debug, Clone)]
pub struct ModuleRows {
    /// The module key.
    pub path: String,
    /// The blob they were computed from — the cache key that made them.
    pub blob: String,
    /// The rows, sorted: the node, its symbols, its `contains` edges and its outbound edges.
    pub rows: Vec<Row>,
    /// The module node's id, which the lane uses as the chunk's slot name.
    pub node: i64,
    /// Set when the declaration list may have shrunk rather than shrunk (`s1` finding 1): a surface
    /// may not print this module's caller list as complete.
    pub incomplete: Option<String>,
    /// Set when no structure could be read at all, with the reason's text.
    pub unavailable: Option<String>,
    /// Edges that named something outside the corpus.
    pub external: usize,
    /// Edges of the wildcard kind.
    pub wildcard: usize,
    /// `(id, key)` for each exported symbol, in key order — the naming view needs them, and reading
    /// them back out of the `contains` edges would be re-deriving what was just built.
    pub symbols: Vec<(i64, String)>,
    /// The layer tag this module carries in this revision (`[layers]`).
    pub layer: String,
}

impl ModuleRows {
    /// The chunk payload for this module's rows.
    #[must_use]
    pub fn payload(&self) -> Vec<u8> {
        encode_rows(&self.rows)
    }
}

/// A revision's graph, as built.
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    /// The materialised graph — the same shape a chain reconstruction produces, so the two paths are
    /// comparable and the tests can assert they agree.
    pub graph: Graph,
    /// The rows per module, keyed by module path: what the next revision's delta is computed
    /// against, and what an anchor's manifest names.
    pub modules: BTreeMap<String, ModuleRows>,
    /// What the build measured about itself.
    pub summary: BuildSummary,
}

impl BuildOutcome {
    /// The id→path dictionary this build's rows need to be spelled back as paths.
    ///
    /// A [`Graph`] is ids and every answer a surface prints is a path, so this is the read side of the
    /// same dictionary the builder minted. The two flags a stored node cannot say for itself later —
    /// that its declaration list may have shrunk rather than shrunk, and which `[layers]` tag it
    /// carried — are carried here too, because a delta that reports a disappeared export without that
    /// flag is reporting a fact it does not have (`s1` finding 1).
    #[must_use]
    pub fn naming(&self) -> Naming {
        Naming::from_vertices(self.modules.values().flat_map(|module| {
            std::iter::once(Vertex {
                id: module.node,
                key: module.path.clone(),
                kind: RowKind::Module,
                layer: module.layer.clone(),
                incomplete: module.incomplete.is_some(),
            })
            .chain(module.symbols.iter().map(move |(id, key)| Vertex {
                id: *id,
                key: key.clone(),
                kind: RowKind::Symbol,
                layer: module.layer.clone(),
                incomplete: module.incomplete.is_some(),
            }))
        }))
    }
}

impl Builder {
    /// A builder with these knobs and these layer tags.
    #[must_use]
    pub fn new(
        policy: crate::graph::policy::GraphPolicy,
        layers: crate::graph::layers::Layers,
    ) -> Self {
        Self { policy, layers }
    }

    /// One module's rows. `None` when the path is ignored by `graph.ignored_dirs`, in which case the
    /// file is not in the graph and is counted in [`BuildSummary::ignored`] by [`Builder::build`].
    pub fn module_rows(
        &self,
        input: &ModuleInput<'_>,
        interner: &mut Interner,
    ) -> Option<ModuleRows> {
        if self.policy.ignores(input.path) {
            return None;
        }
        let path = module_key(input.path);
        let layer = self.layers.layer_of(&path);
        // The two things a stored node cannot say for itself later — that its declaration list may
        // have shrunk rather than shrunk, and that nothing could be read out of it — are decided
        // *before* the node is interned, because the dictionary is where a reader looks.
        let (unavailable, incomplete) = match input.file {
            None => (
                Some(String::from("no structural record for this blob")),
                None,
            ),
            Some(record) => match &record.structure {
                None => (
                    Some(
                        record
                            .reason
                            .as_ref()
                            .map(crate::structure::UnavailableReason::sentence)
                            .unwrap_or_else(|| String::from("no structural record")),
                    ),
                    None,
                ),
                Some(structure) => (None, structure.may_have_shrunk()),
            },
        };
        let node = interner.id(&path, "module", &layer, incomplete.is_some());
        let mut rows: Vec<Row> = vec![(RowKind::Module, node, 0)];
        let mut out = ModuleRows {
            path: path.clone(),
            blob: input.blob.to_string(),
            rows: Vec::new(),
            node,
            incomplete,
            unavailable,
            external: 0,
            wildcard: 0,
            symbols: Vec::new(),
            layer: layer.clone(),
        };
        if out.unavailable.is_some() {
            out.rows = sorted_rows(rows);
            return Some(out);
        }
        let Some(structure) = input.file.and_then(|record| record.structure.as_ref()) else {
            // Unreachable in practice — `unavailable` was just set for exactly this case — but the
            // branch is written rather than `expect`d, because a warning-level lint gate that turns
            // a "can't happen" into a panic is how a panic ships.
            out.rows = sorted_rows(rows);
            return Some(out);
        };

        // Symbols: the exported ones only. A private `fn` is not architecture, and storing every
        // declaration is how a graph becomes an index of the source instead of a map of it.
        for symbol in &structure.symbols {
            if !symbol.exported {
                continue;
            }
            let key = symbol_key(&path, symbol);
            let id = interner.id(&key, "symbol", &layer, false);
            rows.push((RowKind::Symbol, id, 0));
            rows.push((RowKind::Contains, node, id));
            out.symbols.push((id, key));
        }

        // Dependency edges, as the extractor resolved them.
        for edge in &structure.edges {
            if edge.target.is_empty() {
                out.external += 1;
                continue;
            }
            let target = module_key(&edge.target);
            if target == path {
                // A file is not its own dependency; the extractor drops self-edges for the same
                // reason and the two paths must agree.
                continue;
            }
            let target_layer = self.layers.layer_of(&target);
            let target_id = interner.id(&target, "module", &target_layer, false);
            let kind = match edge.kind {
                crate::structure::EdgeKind::Wildcard => {
                    out.wildcard += 1;
                    if edge.is_package_granular() {
                        // Go's wildcard edges are the package-granular ones: the dependent named a
                        // package, and drawing that as a file-to-file fact is `s1` finding 4.
                        RowKind::ImportsPackage
                    } else {
                        RowKind::Wildcard
                    }
                }
                crate::structure::EdgeKind::Resolved => {
                    if edge.is_package_granular() {
                        RowKind::ImportsPackage
                    } else {
                        RowKind::Imports
                    }
                }
                // Unresolved is already a named state upstream. Storing it as an edge would be
                // inventing a target.
                crate::structure::EdgeKind::Unresolved => {
                    out.external += 1;
                    continue;
                }
            };
            rows.push((kind, node, target_id));
            if edge.kind == crate::structure::EdgeKind::Resolved && edge.symbol != "*" {
                let dst = interner.id(
                    &format!("{target}::{}", edge.symbol),
                    "symbol",
                    &target_layer,
                    false,
                );
                rows.push((RowKind::Uses, node, dst));
            }
        }

        // Implementation and inheritance, from the resolved supertypes. An unresolved base is a std
        // or third-party type and writes nothing: `impl Display for T` is not an architectural edge.
        for supertype in &structure.supertypes {
            let Some(resolved) = &supertype.resolved else {
                out.external += 1;
                continue;
            };
            let target = module_key(resolved);
            if target == path {
                continue;
            }
            let target_layer = self.layers.layer_of(&target);
            let src = interner.id(
                &format!("{path}::{}", supertype.of),
                "symbol",
                &layer,
                false,
            );
            let base = supertype
                .base
                .rsplit([':', '.'])
                .next()
                .unwrap_or(&supertype.base);
            let dst = interner.id(&format!("{target}::{base}"), "symbol", &target_layer, false);
            let kind = match supertype.kind {
                crate::structure::SupertypeKind::Base => RowKind::Inherits,
                _ => RowKind::Implements,
            };
            rows.push((kind, src, dst));
        }

        out.rows = sorted_rows(rows);
        Some(out)
    }

    /// Build a whole revision: every live module, and the graph their rows make.
    ///
    /// This is the cold path — the one an episode *selection* pays when nothing is in hand — and it is
    /// O(live modules) by construction. The warm path is [`crate::graph::delta`] on two revisions'
    /// module rows.
    pub fn build(&self, inputs: &[ModuleInput<'_>], interner: &mut Interner) -> BuildOutcome {
        let mut modules: BTreeMap<String, ModuleRows> = BTreeMap::new();
        let mut summary = BuildSummary::default();
        // Sorted order, because the interner mints ids in first-sighting order and the ids are part
        // of the chunk content: a rebuild that walked in a different order would produce different
        // content addresses and the reproducibility claim would be a coincidence.
        let mut ordered: Vec<&ModuleInput<'_>> = inputs.iter().collect();
        ordered.sort_by(|a, b| a.path.cmp(b.path));
        for input in ordered {
            summary.modules += 1;
            let Some(mut module) = self.module_rows(input, interner) else {
                summary.ignored += 1;
                continue;
            };
            if module.unavailable.is_some() {
                summary.unavailable += 1;
            }
            if module.incomplete.is_some() {
                summary.incomplete += 1;
            }
            summary.external += module.external;
            summary.wildcard += module.wildcard;
            module.rows.sort_unstable();
            module.rows.dedup();
            modules.insert(module.path.clone(), module);
        }
        let rows: Vec<Row> = modules
            .values()
            .flat_map(|module| module.rows.clone())
            .collect();
        let graph = Graph::from_rows(rows);
        summary.nodes = graph.node_count();
        summary.edges = graph.edge_count();
        summary.dangling = graph.dangling().len();
        BuildOutcome {
            graph,
            modules,
            summary,
        }
    }
}

/// The symbol node's key: the module, then the container path if it has one.
fn symbol_key(path: &str, symbol: &crate::structure::Symbol) -> String {
    if symbol.container.is_empty() {
        format!("{path}::{}", symbol.name)
    } else {
        format!("{path}::{}::{}", symbol.container, symbol.name)
    }
}

/// Rows carry a `dst` of `0` for node rows; `dst` is part of the sort key, so the order is total.
fn sorted_rows(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_unstable();
    rows.dedup();
    rows
}

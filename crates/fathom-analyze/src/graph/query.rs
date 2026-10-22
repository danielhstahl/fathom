//! The query API: the five questions an architecture pane asks of a materialised revision.
//!
//! These are `e3.2`'s own list, and they are the ones ADR-006's panes issue at every probe revision:
//! **dependencies of X, dependents of X, a path between A and B, the subgraph of a layer, and the
//! edges an episode changed**. They run against a *materialised* graph, which is the only state in
//! which a keystroke asks a graph question at all — `s3`'s framing, and the reason
//! `graph.materialise_revision.ms` is a separate budget from these.
//!
//! # An answer says what it does not know
//!
//! This is the part of `s1` that a query API can get wrong in the user's face:
//!
//! * [`dependents_of`] returns a [`Fan`], not a list: the named callers, the callers whose dependency
//!   has unknown contents (a wildcard: `s1` finding 2), and the callers whose dependency is
//!   package-granular rather than file-granular (`s1` finding 4). A pane that prints only `named` is
//!   the bug finding 2 names; a pane that merges them is a subtler version of the same bug.
//! * A module whose declaration list may have shrunk (`synthetic_eligible`) may not be used to claim
//!   a complete caller list — [`Fan::complete`] says whether the answer could be.
//! * [`path`] is bounded by `graph.query.path_max_hops` and answers `complete: false` when the bound
//!   stopped it, per ADR-004 §8.5: on exceeding a cap the answer is a stated partial, never a hang
//!   and never a silent "no path".
//! * [`layer_subgraph`] counts a layer's *internal* edges separately from the ones that cross it,
//!   because the layering question `e3.4` asks is about the crossings and a subgraph that only knows
//!   its own size cannot name them.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::graph::model::{Graph, RowKind, Vertex};

/// The id↔key dictionary, as a read-only view.
///
/// A materialised graph is ids; every answer is paths. This is the only thing that bridges them, and
/// it is built once per materialisation (`Store::graph_nodes`) rather than per query.
#[derive(Debug, Clone, Default)]
pub struct Naming {
    by_id: BTreeMap<i64, Vertex>,
    by_key: BTreeMap<String, i64>,
}

impl Naming {
    /// Index a dictionary read out of the store.
    #[must_use]
    pub fn from_vertices(vertices: impl IntoIterator<Item = Vertex>) -> Self {
        let mut naming = Self::default();
        for vertex in vertices {
            naming.by_key.insert(vertex.key.clone(), vertex.id);
            naming.by_id.insert(vertex.id, vertex);
        }
        naming
    }

    /// The union of two dictionaries, which is what a *pair* of revisions needs.
    ///
    /// A delta names both the rows that arrived and the rows that went away, and a caller that holds
    /// two materialised revisions holds two views of one dictionary. Where the two disagree — a
    /// module whose declaration list became untrustworthy between the revisions — the second (the
    /// later revision, by convention at every call site) wins, because the answer is about what the
    /// revision did and its own flags are the ones to print.
    #[must_use]
    pub fn merged(first: &Naming, second: &Naming) -> Self {
        Self::from_vertices(
            first
                .by_id
                .values()
                .cloned()
                .chain(second.by_id.values().cloned()),
        )
    }

    /// The key of an id, or `None` when the graph references something the dictionary never saw —
    /// which is reported, not renamed.
    #[must_use]
    pub fn name(&self, id: i64) -> Option<&str> {
        self.by_id.get(&id).map(|node| node.key.as_str())
    }

    /// The id of a key.
    #[must_use]
    pub fn id(&self, key: &str) -> Option<i64> {
        self.by_key.get(key).copied()
    }

    /// The layer tag of an id, `""` when untagged.
    #[must_use]
    pub fn layer(&self, id: i64) -> &str {
        self.by_id.get(&id).map_or("", |node| node.layer.as_str())
    }

    /// Whether this vertex's declaration list may have shrunk rather than shrunk (`s1` finding 1).
    /// A symbol inherits its module's answer only in the sense that the module carries the flag; a
    /// reader asks about the module.
    #[must_use]
    pub fn incomplete(&self, id: i64) -> bool {
        self.by_id.get(&id).is_some_and(|node| node.incomplete)
    }

    /// The dictionary entry for an id.
    #[must_use]
    pub fn vertex(&self, id: i64) -> Option<&Vertex> {
        self.by_id.get(&id)
    }

    /// Every node belonging to one module: the module itself and each symbol under it.
    ///
    /// The prefix walk is a range, not a scan: `<path>::` is a prefix of every symbol key the module
    /// owns, and `by_key` is sorted by key.
    #[must_use]
    pub fn module_ids(&self, path: &str) -> Vec<i64> {
        let mut ids = Vec::new();
        if let Some(id) = self.by_key.get(path) {
            ids.push(*id);
        }
        let prefix = format!("{path}::");
        let end = format!("{prefix}\u{10ffff}");
        ids.extend(self.by_key.range(prefix.clone()..=end).map(|(_, id)| *id));
        ids
    }

    /// How many vertices the dictionary knows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the dictionary is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// A fan-in or fan-out answer, in the three parts that must not be merged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fan {
    /// The subject the question was asked about.
    pub subject: String,
    /// The confident half: a dependency that names the unit, contents known.
    pub named: Vec<String>,
    /// Contents unknown by construction — a wildcard edge. `s1` finding 2: "nobody" is a worse
    /// answer than "one of these, and I cannot enumerate it".
    pub wildcard: Vec<String>,
    /// Package-granular edges: a Go dependency, drawn per file because that is the granularity the
    /// store keys on. A pane that prints these as file-to-file facts is reporting a claim nobody
    /// made (`s1` finding 4).
    pub package_granular: Vec<String>,
    /// Whether the answer *could* be complete: false when any participant's declaration list may
    /// have shrunk rather than shrunk (`s1` finding 1).
    pub complete: bool,
}

impl Fan {
    /// Every endpoint named, whichever bucket it fell in — for a count, never for a caller list.
    #[must_use]
    pub fn all(&self) -> Vec<String> {
        let mut all: Vec<String> = self
            .named
            .iter()
            .chain(self.wildcard.iter())
            .chain(self.package_granular.iter())
            .cloned()
            .collect();
        all.sort();
        all.dedup();
        all
    }

    /// How many endpoints the answer names in total.
    #[must_use]
    pub fn len(&self) -> usize {
        self.all().len()
    }

    /// Whether the answer is empty — which is a claim only a `complete` answer gets to make.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.named.is_empty() && self.wildcard.is_empty() && self.package_granular.is_empty()
    }

    /// The sentence a surface prints, with the caveat attached rather than assumed.
    #[must_use]
    pub fn sentence(&self) -> String {
        let base = if self.named.is_empty() && self.wildcard.is_empty() {
            format!("nothing in the graph depends on {}", self.subject)
        } else {
            format!(
                "{} depend on {}: {} named{}",
                self.len(),
                self.subject,
                self.named.len(),
                if self.wildcard.is_empty() {
                    String::new()
                } else {
                    format!(", {} with unknown contents", self.wildcard.len())
                }
            )
        };
        let mut caveats = Vec::new();
        if !self.package_granular.is_empty() {
            caveats.push(format!(
                "{} of them name a package rather than a file",
                self.package_granular.len()
            ));
        }
        if !self.complete {
            caveats.push(String::from(
                "at least one module's declaration list may have shrunk rather than shrunk, so this \
                 list is not claimed complete",
            ));
        }
        if caveats.is_empty() {
            base
        } else {
            format!("{base} ({})", caveats.join("; "))
        }
    }
}

/// What an incompletely-declared participant does to an answer.
///
/// The direction decides whose completeness matters. Asking *what does X depend on* is a question
/// about X's own declaration list — if X is a macro-heavy scope, its outbound list may be shorter
/// than the truth. Asking *what depends on X* is a question about every other module's list, so one
/// `synthetic_eligible` dependent is enough to stop the answer claiming to be complete
/// (`s1` finding 1: such a scope may not be used to claim a complete caller list).
fn declares_completely(naming: &Naming, ids: &[i64]) -> bool {
    ids.iter().all(|id| !naming.incomplete(*id))
}

/// What depends on `subject`, or what `subject` depends on, in one shape.
fn fan(graph: &Graph, naming: &Naming, subject: &str, outward: bool) -> Fan {
    let ids = naming.module_ids(subject);
    let mut out = Fan {
        subject: subject.to_string(),
        complete: !outward || declares_completely(naming, &ids),
        ..Fan::default()
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for id in &ids {
        let ends = if outward {
            graph.reached(*id, RowKind::is_dependency)
        } else {
            graph.arrived(*id, RowKind::is_dependency)
        };
        for (kind, other) in ends {
            let Some(name) = naming.name(other) else {
                continue;
            };
            let name = module_of(name);
            if name == subject || !seen.insert(name.clone()) {
                continue;
            }
            match kind {
                RowKind::Wildcard => out.wildcard.push(name),
                RowKind::ImportsPackage => out.package_granular.push(name),
                _ => {
                    if !outward && naming.id(&name).is_none_or(|id| naming.incomplete(id)) {
                        out.complete = false;
                    }
                    out.named.push(name);
                }
            }
        }
    }
    out.named.sort();
    out.wildcard.sort();
    out.package_granular.sort();
    out
}

/// The module half of a key: `crates/x.rs::Thing` belongs to `crates/x.rs`.
#[must_use]
pub fn module_of(key: &str) -> String {
    match key.find("::") {
        Some(at) => key[..at].to_string(),
        None => key.to_string(),
    }
}

/// What `subject` depends on.
#[must_use]
pub fn dependencies_of(graph: &Graph, naming: &Naming, subject: &str) -> Fan {
    fan(graph, naming, subject, true)
}

/// What depends on `subject`. The fan-in case ADR-004 §11 budgets at 20 ms, and the case `s1` says
/// a graph must never answer with a bare list.
#[must_use]
pub fn dependents_of(graph: &Graph, naming: &Naming, subject: &str) -> Fan {
    fan(graph, naming, subject, false)
}

/// A bounded path between two modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathAnswer {
    /// The modules on the path, starting at `from`.
    pub hops: Vec<String>,
    /// How many edges were crossed.
    pub depth: usize,
    /// Whether the walk ran out of graph before it ran out of bound.
    pub found: bool,
    /// The hop bound stopped the walk: the answer is a partial, and says so (ADR-004 §8.5).
    pub truncated: bool,
    /// The bound that applied.
    pub max_hops: usize,
}

impl PathAnswer {
    /// The sentence for the pane, including the reason a `None`-looking answer is not a claim of
    /// unreachability.
    #[must_use]
    pub fn sentence(&self) -> String {
        if self.found {
            return format!(
                "{} → {} in {} hop(s): {}",
                self.hops.first().map_or_else(String::new, String::clone),
                self.hops.last().map_or_else(String::new, String::clone),
                self.depth,
                self.hops.join(" → ")
            );
        }
        if self.truncated {
            return format!(
                "no path found within graph.query.path_max_hops = {} (stopped at depth {})",
                self.max_hops, self.depth
            );
        }
        String::from("no dependency path between the two in this revision")
    }
}

/// A shortest dependency path from `from` to `to`, walked to `max_hops` at most.
///
/// Breadth-first, over the dependency kinds, with the wildcard edges included: a wildcard
/// dependency is a real dependency whose contents are unknown, and a path that routes through one
/// is a path — `e3.4` will want to know that the only route leaves the known graph.
#[must_use]
pub fn path(graph: &Graph, naming: &Naming, from: &str, to: &str, max_hops: usize) -> PathAnswer {
    let mut answer = PathAnswer {
        hops: Vec::new(),
        depth: 0,
        found: false,
        truncated: false,
        max_hops,
    };
    let (Some(start), Some(target)) = (naming.id(from), naming.id(to)) else {
        return answer;
    };
    if start == target {
        answer.found = true;
        answer.hops = vec![from.to_string()];
        return answer;
    }
    let targets: BTreeSet<i64> = naming.module_ids(to).into_iter().collect();
    let mut parents: BTreeMap<i64, Option<i64>> = BTreeMap::new();
    parents.insert(start, None);
    let mut frontier = vec![start];
    for depth in 1..=max_hops {
        let mut next = Vec::new();
        for node in &frontier {
            for (_, dst) in graph.reached(*node, RowKind::is_dependency) {
                if parents.contains_key(&dst) {
                    continue;
                }
                parents.insert(dst, Some(*node));
                if targets.contains(&dst) {
                    let mut ids = vec![dst];
                    let mut cur = parents.get(&dst).copied().flatten();
                    while let Some(node) = cur {
                        ids.push(node);
                        cur = parents.get(&node).copied().flatten();
                    }
                    ids.reverse();
                    let mut hops: Vec<String> = ids
                        .iter()
                        .filter_map(|id| naming.name(*id))
                        .map(module_of)
                        .collect();
                    hops.dedup();
                    answer.hops = hops;
                    answer.depth = depth;
                    answer.found = true;
                    return answer;
                }
                next.push(dst);
            }
        }
        if next.is_empty() {
            answer.depth = depth;
            return answer;
        }
        answer.depth = depth;
        frontier = next;
    }
    answer.truncated = true;
    answer
}

/// One layer's subgraph, and what crosses its borders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Subgraph {
    /// The layer asked about.
    pub layer: String,
    /// Every node tagged with it, sorted by key.
    pub nodes: Vec<String>,
    /// Edges with both endpoints inside the layer.
    pub internal_edges: usize,
    /// Edges leaving it, by target layer.
    pub outbound: BTreeMap<String, usize>,
    /// Edges entering it, by source layer.
    pub inbound: BTreeMap<String, usize>,
}

impl Subgraph {
    /// Total crossings, which is the number `e3.4`'s rules and `e3.8`'s coupling both start from.
    #[must_use]
    pub fn crossings(&self) -> usize {
        self.outbound.values().sum::<usize>() + self.inbound.values().sum::<usize>()
    }

    /// The sentence for the pane.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "layer {} holds {} node(s) and {} internal edge(s), with {} crossing its boundary",
            if self.layer.is_empty() {
                "(untagged)"
            } else {
                &self.layer
            },
            self.nodes.len(),
            self.internal_edges,
            self.crossings()
        )
    }
}

/// The subgraph of one layer: every node the `[layers]` config tagged with it, and the edges that
/// know about it.
#[must_use]
pub fn layer_subgraph(graph: &Graph, naming: &Naming, layer: &str) -> Subgraph {
    let mut out = Subgraph {
        layer: layer.to_string(),
        ..Subgraph::default()
    };
    let inside: BTreeSet<i64> = graph
        .nodes()
        .keys()
        .copied()
        .filter(|id| naming.layer(*id) == layer)
        .collect();
    out.nodes = inside
        .iter()
        .filter_map(|id| naming.name(*id))
        .map(str::to_string)
        .collect();
    for (kind, src, dst) in graph.edges() {
        let (in_src, in_dst) = (inside.contains(src), inside.contains(dst));
        match (in_src, in_dst) {
            (true, true) => out.internal_edges += 1,
            (true, false) => {
                *out.outbound
                    .entry(naming.name(*dst).unwrap_or_default().to_string())
                    .or_default() += 1;
                let _ = kind;
            }
            (false, true) => {
                *out.inbound
                    .entry(naming.name(*src).unwrap_or_default().to_string())
                    .or_default() += 1;
            }
            (false, false) => {}
        }
    }
    out.nodes.sort();
    out
}

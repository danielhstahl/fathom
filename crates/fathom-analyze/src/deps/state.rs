//! The dependency *state* of one revision, and the graph a lockfile describes (e2.6 / reco-kvs).
//!
//! [`crate::deps::scan`] answers "what did these files say"; this module answers "what does this revision
//! depend on", which is a different question in three ways that matter:
//!
//! * **Files fold into packages.** A `package.json` and a `pnpm-lock.yaml` describe the same thing twice.
//!   The state holds one node per resolved package and one entry per declared requirement, so a reviewer
//!   is never told a dependency was added twice because two files changed.
//! * **Nothing is dropped, only attributed.** Every node carries *how* we know it is a direct dependency
//!   ([`Basis`]) and what we could not tell ([`Attribution::Unknown`]). A lock that records no edges —
//!   `go.sum`, a Gradle lockfile, a `requirements.txt` — cannot support a "transitive" claim, and says so
//!   instead of quietly guessing from absence.
//! * **A graph, so "who asked" is answerable.** Resolved edges are re-linked from the ranges the lock
//!   wrote, which is what lets the delta say *"net +140 packages from 1 new direct dependency"* rather
//!   than "+140 packages, cause unknown".
//!
//! The *scope* of a resolved node is the file that resolved it (a lockfile path), which is also the file a
//! review gets pointed at. That choice is deliberate: ADR-004 §6.6 R2 makes the warm path O(changed
//! paths), so state is built from whatever files the caller has — a whole tree on a cold scan, or the two
//! sides of one episode's diff on a warm one — and a scope that names a lockfile needs no tree to make
//! sense.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::deps::model::{Body, DepFile, Dependency, Ecosystem, Status};
use crate::deps::scan::Delta;
use crate::deps::version::{self, Satisfies};

/// Identity of a resolved node: scope, ecosystem, name, version.
type Key = (String, Ecosystem, String, String);

/// How many outgoing edges one node may carry before the rest are dropped with a note.
const MAX_EDGES_PER_NODE: usize = 512;

/// How many "nothing asked for this" notes one state writes before summarising the remainder.
const MAX_ORPHAN_NOTES: usize = 5;

/// Whether a package was asked for by this repository, by something it already depended on, or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    /// A manifest — or the lock itself — says this repository asked for it by name.
    Direct,
    /// Something else in the resolved graph asked for it.
    Transitive,
    /// This repository's own package, so not a dependency of it at all: a cargo workspace member, an npm
    /// workspace link, a NuGet `Project`.
    Local,
    /// The evidence does not exist, as opposed to being negative. See [`Basis::NoEvidence`].
    Unknown,
}

impl Attribution {
    /// The stable string, which is also the JSON spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Attribution::Direct => "direct",
            Attribution::Transitive => "transitive",
            Attribution::Local => "local",
            Attribution::Unknown => "unknown",
        }
    }

    /// Whether a walk up the graph may stop here — the answer to "who asked" lives at a root.
    #[must_use]
    pub const fn is_root(self) -> bool {
        matches!(self, Attribution::Direct | Attribution::Local)
    }

    /// The column this attribution occupies in a counts row: direct, transitive, local, unknown.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Attribution::Direct => 0,
            Attribution::Transitive => 1,
            Attribution::Local => 2,
            Attribution::Unknown => 3,
        }
    }
}

/// The evidence behind an [`Attribution`], kept because the claim is only as good as its proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// The lockfile states it: NuGet's `"type": "Direct"`, `Gemfile.lock`'s `DEPENDENCIES` block.
    Lockfile,
    /// A manifest in this repository declares the name.
    Manifest,
    /// Only a lock's importer block declares it (`pnpm-lock.yaml`'s `importers:`, npm's root package).
    Importer,
    /// The declaring manifest's own package name: this node *is* a package of this repository.
    SelfPackage,
    /// Some other resolved package's edge range covers it.
    Edges,
    /// Nothing states it: no declaring manifest, no lock statement, no inbound edge.
    NoEvidence,
}

impl Basis {
    /// The stable string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Basis::Lockfile => "lockfile",
            Basis::Manifest => "manifest",
            Basis::Importer => "importers",
            Basis::SelfPackage => "own package",
            Basis::Edges => "edges",
            Basis::NoEvidence => "none",
        }
    }

    /// One line of prose for the drill-down, so a reviewer can check the claim without reading this file.
    #[must_use]
    pub const fn explain(self) -> &'static str {
        match self {
            Basis::Lockfile => "the lockfile states whether it is a direct dependency",
            Basis::Manifest => "a manifest in this repository declares it",
            Basis::Importer => "a lockfile importer block declares it",
            Basis::SelfPackage => "it is one of this repository's own packages",
            Basis::Edges => "another resolved package asks for it",
            Basis::NoEvidence => "nothing states it and no resolved edge asks for it",
        }
    }
}

/// One resolved package: one name at one version inside one scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// This node's index in [`State::nodes`], used by the edge lists.
    pub id: usize,
    /// The file that resolved it — a lockfile path, or a manifest path where there is no lock.
    pub scope: String,
    /// Ecosystem.
    pub ecosystem: Ecosystem,
    /// Package name, in the lockfile's own spelling.
    pub name: String,
    /// Resolved version, verbatim.
    pub version: String,
    /// Every source label the file gave it (`registry`, `git:…`, `path:…`).
    pub sources: BTreeSet<String>,
    /// Every integrity/hash the file gave it, in the file's own prefix form.
    pub integrity: BTreeSet<String>,
    /// Declared licences, when the lockfile carries them.
    pub license: BTreeSet<String>,
    /// Whether the lockfile marks it development-only.
    pub dev: bool,
    /// Whether the lockfile itself stated directness, and what it said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marked_direct: Option<bool>,
    /// Direct, transitive, local, or unknowable.
    pub attribution: Attribution,
    /// What that attribution rests on.
    pub basis: Basis,
    /// Resolved packages that ask for this one.
    pub parents: Vec<usize>,
    /// Resolved packages this one asks for.
    pub children: Vec<usize>,
}

/// One requirement this repository *declared*, merged across the files that declare it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declared {
    /// Ecosystem.
    pub ecosystem: Ecosystem,
    /// The name as written, which for an npm alias is the alias rather than the package.
    pub name: String,
    /// The package actually installed, when the file renamed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    /// Every requirement spelling seen, joined with ` | ` when members disagree.
    pub requirement: String,
    /// The kinds seen — `normal`, `dev`, `peer`, `target:cfg(unix)`, …
    pub kinds: BTreeSet<String>,
    /// Source labels seen.
    pub sources: BTreeSet<String>,
    /// The manifest, or importer block, that declares it.
    pub scopes: BTreeSet<String>,
    /// The declaring package's name, when a manifest named one — whose peer dependency this is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// Whether any file declared it as a peer dependency.
    pub is_peer: bool,
    /// Two members alias one name onto two different packages.
    pub renamed_differently: bool,
}

impl Declared {
    /// The name the resolved graph carries: the rename target when there is one.
    #[must_use]
    pub fn resolved_name(&self) -> &str {
        self.actual.as_deref().unwrap_or(&self.name)
    }
}

/// Everything one revision depends on, folded out of the files offered.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Resolved packages, in `(scope, ecosystem, name, version)` order.
    pub nodes: Vec<Node>,
    /// Declared requirements, in `(ecosystem, name)` order.
    pub declared: Vec<Declared>,
    /// Statements about what could not be folded in, and why.
    pub notes: Vec<String>,
    /// Recognised files folded in.
    pub files: usize,
    /// Files offered and unread — every attribution involving them is weaker.
    pub unparsed: BTreeSet<String>,
    /// Whether every file offered was read.
    pub complete: bool,
    /// `(scope, ecosystem, name, version)` → index into [`State::nodes`].
    #[serde(skip, default)]
    index: BTreeMap<Key, usize>,
    /// `(scope, ecosystem, name)` → indices, for edge resolution and lookup.
    #[serde(skip, default)]
    by_name: BTreeMap<(String, Ecosystem, String), Vec<usize>>,
    /// Names a manifest declares, per ecosystem, rename targets included.
    #[serde(skip, default)]
    manifest_names: BTreeSet<(Ecosystem, String)>,
    /// Names a lock's importer block declares, per ecosystem.
    #[serde(skip, default)]
    importer_names: BTreeSet<(Ecosystem, String)>,
    /// Names one of this repository's own manifests gives its own package.
    #[serde(skip, default)]
    own_names: BTreeSet<(Ecosystem, String)>,
}

/// What one lockfile admits about who asked.
#[derive(Debug, Clone, Copy, Default)]
struct Quality {
    records_edges: bool,
    complete: bool,
}

/// What a package's file said, before the node exists.
#[derive(Default)]
struct Pending {
    sources: BTreeSet<String>,
    integrity: BTreeSet<String>,
    license: BTreeSet<String>,
    dev: bool,
    marked: Option<bool>,
    edges: Vec<(String, String)>,
}

/// What the files said about one declared requirement, before the join.
#[derive(Default)]
struct PendingDeclared {
    requirements: Vec<String>,
    kinds: BTreeSet<String>,
    sources: BTreeSet<String>,
    scopes: BTreeSet<String>,
    members: Vec<String>,
    actual: Option<String>,
    renamed_differently: bool,
    is_peer: bool,
}

impl State {
    /// Fold a set of parsed dependency files into one state.
    ///
    /// Order-independent, and total: a file that did not parse is recorded in [`State::unparsed`] and the
    /// rest still folds.
    #[must_use]
    pub fn build(files: &[DepFile]) -> State {
        let mut state = State {
            complete: true,
            ..State::default()
        };
        let mut pending: BTreeMap<Key, Pending> = BTreeMap::new();
        let mut declared: BTreeMap<(Ecosystem, String), PendingDeclared> = BTreeMap::new();
        let mut quality: BTreeMap<String, Quality> = BTreeMap::new();
        let mut edges: Vec<(Key, Vec<(String, String)>)> = Vec::new();

        for file in files {
            state.files += 1;
            if file.status == Status::Unparsed {
                state.complete = false;
                state.unparsed.insert(file.path.clone());
                continue;
            }
            match &file.body {
                Body::Manifest(manifest) => {
                    if let Some(name) = &manifest.name {
                        state.own_names.insert((file.ecosystem, name.clone()));
                    }
                    for dependency in &manifest.requirements {
                        declare(
                            &mut declared,
                            &mut state.manifest_names,
                            file.ecosystem,
                            &file.path,
                            manifest.name.as_deref(),
                            dependency,
                        );
                    }
                }
                Body::Lock(lock) => {
                    quality.insert(
                        file.path.clone(),
                        Quality {
                            records_edges: lock.records_edges,
                            complete: lock.complete,
                        },
                    );
                    for importer in &lock.importers {
                        for dependency in &importer.requirements {
                            declare(
                                &mut declared,
                                &mut state.importer_names,
                                file.ecosystem,
                                &file.path,
                                Some(&importer.dir),
                                dependency,
                            );
                        }
                    }
                    for package in &lock.packages {
                        let key = (
                            file.path.clone(),
                            file.ecosystem,
                            package.name.clone(),
                            package.version.clone(),
                        );
                        let entry = pending.entry(key.clone()).or_default();
                        entry.sources.insert(package.source.label());
                        if let Some(integrity) = &package.integrity {
                            entry.integrity.insert(integrity.clone());
                        }
                        if let Some(license) = &package.license {
                            entry.license.insert(license.clone());
                        }
                        entry.dev |= package.dev;
                        // `Some(true)` on any row wins: one file saying a package is direct outweighs
                        // another file's silence, which is all a merge of two lock shapes can mean.
                        entry.marked = match (entry.marked, package.direct) {
                            (Some(true), _) | (_, Some(true)) => Some(true),
                            (Some(false), _) | (_, Some(false)) => Some(false),
                            (None, None) => None,
                        };
                        if !package.edges.is_empty() {
                            if entry.edges.len() < MAX_EDGES_PER_NODE {
                                let take = MAX_EDGES_PER_NODE - entry.edges.len();
                                entry.edges.extend(package.edges.iter().take(take).cloned());
                            }
                            edges.push((key, package.edges.clone()));
                        }
                    }
                }
                Body::Workspace { members } => {
                    if !members.is_empty() {
                        state.notes.push(format!(
                            "`{}` is workspace membership ({} members), which adds no dependencies",
                            file.path,
                            members.len()
                        ));
                    }
                }
                Body::Unparsed(_) => {}
            }
        }

        // Nodes, in the deterministic order every golden file depends on.
        let mut node_edges: Vec<(usize, Vec<(String, String)>)> = Vec::with_capacity(pending.len());
        for (key, entry) in pending {
            let id = state.nodes.len();
            state.nodes.push(Node {
                id,
                scope: key.0.clone(),
                ecosystem: key.1,
                name: key.2.clone(),
                version: key.3.clone(),
                sources: entry.sources,
                integrity: entry.integrity,
                license: entry.license,
                dev: entry.dev,
                marked_direct: entry.marked,
                attribution: Attribution::Unknown,
                basis: Basis::NoEvidence,
                parents: Vec::new(),
                children: Vec::new(),
            });
            state.index.insert(key.clone(), id);
            state
                .by_name
                .entry((key.0, key.1, key.2))
                .or_default()
                .push(id);
        }
        for (key, list) in &edges {
            if let Some(&id) = state.index.get(key) {
                node_edges.push((id, list.clone()));
            }
        }

        // Link the graph: an edge names a package and a range, and the range picks which of the versions
        // present it asked for. A range this module cannot read (`1.0.200 (registry+…)`, cargo's spelling)
        // asks for *every* version present rather than none, because a missing edge would invent an orphan.
        let mut unresolved = 0usize;
        for (from, list) in node_edges {
            let scope = state.nodes[from].scope.clone();
            let ecosystem = state.nodes[from].ecosystem;
            for (name, range) in list {
                let candidates: Vec<usize> = state
                    .by_name
                    .get(&(scope.clone(), ecosystem, name.clone()))
                    .cloned()
                    .unwrap_or_default();
                let mut matched = false;
                for &to in &candidates {
                    let version = &state.nodes[to].version;
                    let holds = range.trim().is_empty()
                        || version::satisfies(&range, version) != Satisfies::No;
                    if holds {
                        state.nodes[from].children.push(to);
                        state.nodes[to].parents.push(from);
                        matched = true;
                    }
                }
                if !matched {
                    unresolved += 1;
                }
            }
        }
        for node in &mut state.nodes {
            node.parents.sort_unstable();
            node.parents.dedup();
            node.children.sort_unstable();
            node.children.dedup();
        }

        state.attribute(&quality);
        if unresolved > 0 {
            state.notes.push(format!(
                "{unresolved} resolved edge(s) named a package the same lock does not resolve; they are \
                 not counted as graph edges"
            ));
        }
        if !state.unparsed.is_empty() {
            state.notes.push(format!(
                "{} dependency file(s) on this side could not be read, so anything they alone proved is \
                 missing: {}",
                state.unparsed.len(),
                state.unparsed.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        state.finish_declared(declared);
        state
    }

    /// The state of a whole-tree scan.
    #[must_use]
    pub fn of(surface: &crate::deps::scan::Surface) -> State {
        State::build(&surface.files)
    }

    /// The two states of an episode: what it depended on before, and what it depends on after.
    ///
    /// Only the changed files are folded in, which is the point (ADR-004 §6.6 R2): an unchanged lockfile
    /// resolved the same packages on both sides and contributes nothing to the difference. What the warm
    /// path cannot see is an unchanged *manifest* beside a changed lock, and the attribution basis says so
    /// — a package only provable from a manifest that did not change comes back
    /// [`Attribution::Unknown`], never as a guess.
    #[must_use]
    pub fn of_delta(delta: &Delta) -> (State, State) {
        let mut before: Vec<DepFile> = Vec::new();
        let mut after: Vec<DepFile> = Vec::new();
        for row in &delta.paths {
            if let Some(file) = &row.before {
                before.push(file.clone());
            }
            if let Some(file) = &row.after {
                after.push(file.clone());
            }
        }
        (State::build(&before), State::build(&after))
    }

    /// How many resolved packages this state holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether nothing resolved at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Look a node up by its identity.
    #[must_use]
    pub fn node(
        &self,
        scope: &str,
        ecosystem: Ecosystem,
        name: &str,
        version: &str,
    ) -> Option<&Node> {
        self.index
            .get(&(
                scope.to_owned(),
                ecosystem,
                name.to_owned(),
                version.to_owned(),
            ))
            .map(|&id| &self.nodes[id])
    }

    /// The node at an index.
    #[must_use]
    pub fn get(&self, id: usize) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Resolved nodes with this name in this scope — usually one, more when two versions coexist.
    #[must_use]
    pub fn named(&self, scope: &str, ecosystem: Ecosystem, name: &str) -> &[usize] {
        self.by_name
            .get(&(scope.to_owned(), ecosystem, name.to_owned()))
            .map_or(&[], Vec::as_slice)
    }

    /// The nodes with this name in this scope, in version order.
    #[must_use]
    pub fn at(&self, scope: &str, ecosystem: Ecosystem, name: &str) -> Vec<&Node> {
        self.named(scope, ecosystem, name)
            .iter()
            .map(|&id| &self.nodes[id])
            .collect()
    }

    /// Every `(scope, ecosystem, name)` this state holds, sorted.
    #[must_use]
    pub fn scopes_and_names(&self) -> Vec<(String, Ecosystem, String)> {
        self.by_name.keys().cloned().collect()
    }

    /// The nodes, counted per ecosystem as `{ecosystem: count}`.
    #[must_use]
    pub fn by_ecosystem(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for node in &self.nodes {
            *out.entry(node.ecosystem.as_str().to_owned()).or_insert(0) += 1;
        }
        out
    }

    /// Nodes of one ecosystem, in index order.
    #[must_use]
    pub fn of_ecosystem(&self, ecosystem: Ecosystem) -> Vec<&Node> {
        self.nodes
            .iter()
            .filter(|node| node.ecosystem == ecosystem)
            .collect()
    }

    /// Every source label present in the state.
    #[must_use]
    pub fn sources(&self) -> BTreeSet<String> {
        self.nodes
            .iter()
            .flat_map(|node| node.sources.iter().cloned())
            .collect()
    }

    /// The declaring requirement for a name, when one exists.
    #[must_use]
    pub fn declaration(&self, ecosystem: Ecosystem, name: &str) -> Option<&Declared> {
        self.declared
            .iter()
            .find(|declared| declared.ecosystem == ecosystem && declared.resolved_name() == name)
    }

    /// Walk up the resolved graph to the root that asked for this package.
    ///
    /// Returns the chain from the root down to the package's own requester — `["foo", "bar"]` for `baz`
    /// means `foo` asked for `bar` which asked for `baz` — or an empty `Vec` when no root is reachable
    /// within `depth`. Breadth-first
    /// over sorted parents, so the answer is a shortest chain and, among equal lengths, always the same
    /// one.
    #[must_use]
    pub fn who_asked(&self, from: usize, depth: usize) -> Vec<String> {
        if self.nodes.get(from).is_none() {
            return Vec::new();
        }
        let mut frontier: Vec<usize> = vec![from];
        let mut paths: Vec<Vec<usize>> = vec![vec![from]];
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        seen.insert(from);
        for _ in 0..depth {
            let mut next: Vec<usize> = Vec::new();
            let mut next_paths: Vec<Vec<usize>> = Vec::new();
            for (position, &head) in frontier.iter().enumerate() {
                let mut parents = self.nodes[head].parents.clone();
                parents.sort_unstable();
                parents.dedup();
                for parent in parents {
                    if self.nodes[parent].attribution.is_root() {
                        let mut path = paths[position].clone();
                        path.push(parent);
                        path.reverse();
                        // The node itself is not part of an answer to "who asked for it".
                        path.pop();
                        return path.iter().map(|&id| self.nodes[id].name.clone()).collect();
                    }
                    if seen.insert(parent) {
                        let mut path = paths[position].clone();
                        path.push(parent);
                        next.push(parent);
                        next_paths.push(path);
                    }
                }
            }
            frontier = next;
            paths = next_paths;
            if frontier.is_empty() {
                break;
            }
        }
        Vec::new()
    }

    /// Decide direct / transitive / local / unknown for every node, and say why it could not.
    fn attribute(&mut self, quality: &BTreeMap<String, Quality>) {
        let mut orphans: Vec<String> = Vec::new();
        for node in &mut self.nodes {
            let declared = self
                .manifest_names
                .contains(&(node.ecosystem, node.name.clone()));
            let imported = self
                .importer_names
                .contains(&(node.ecosystem, node.name.clone()));
            let own = self
                .own_names
                .contains(&(node.ecosystem, node.name.clone()));
            let (attribution, basis) = if node.marked_direct == Some(true) {
                (Attribution::Direct, Basis::Lockfile)
            } else if node.marked_direct == Some(false) {
                (Attribution::Transitive, Basis::Lockfile)
            } else if declared {
                (Attribution::Direct, Basis::Manifest)
            } else if imported {
                (Attribution::Direct, Basis::Importer)
            } else if own {
                (Attribution::Local, Basis::SelfPackage)
            } else if !node.parents.is_empty() {
                (Attribution::Transitive, Basis::Edges)
            } else {
                (Attribution::Unknown, Basis::NoEvidence)
            };
            node.attribution = attribution;
            node.basis = basis;
        }
        for node in &self.nodes {
            if node.attribution != Attribution::Unknown || orphans.len() >= MAX_ORPHAN_NOTES {
                continue;
            }
            let lock = quality.get(&node.scope).copied().unwrap_or_default();
            let why = if !lock.records_edges {
                "its lockfile records no dependency edges, so nothing can be called transitive"
                    .to_owned()
            } else if !lock.complete {
                "its lockfile is not complete on its own, so who asked is not recorded".to_owned()
            } else {
                "no manifest declares it and no resolved edge asks for it".to_owned()
            };
            orphans.push(format!(
                "`{} {}` ({}) — {}",
                node.name, node.version, node.scope, why
            ));
        }
        let remaining = self
            .nodes
            .iter()
            .filter(|node| node.attribution == Attribution::Unknown)
            .count()
            .saturating_sub(orphans.len());
        self.notes.extend(orphans);
        if remaining > 0 {
            self.notes.push(format!(
                "{remaining} further resolved packages could not be attributed"
            ));
        }
    }

    fn finish_declared(&mut self, declared: BTreeMap<(Ecosystem, String), PendingDeclared>) {
        for ((ecosystem, name), entry) in declared {
            self.declared.push(Declared {
                ecosystem,
                name,
                actual: entry.actual,
                requirement: if entry.requirements.is_empty() {
                    "(any)".to_owned()
                } else {
                    entry.requirements.join(" | ")
                },
                kinds: entry.kinds,
                sources: entry.sources,
                scopes: entry.scopes,
                member: entry.members.first().cloned(),
                is_peer: entry.is_peer,
                renamed_differently: entry.renamed_differently,
            });
        }
    }
}

/// Fold one declared requirement into the per-name accumulator and the "who declares it" set.
fn declare(
    declared: &mut BTreeMap<(Ecosystem, String), PendingDeclared>,
    names: &mut BTreeSet<(Ecosystem, String)>,
    ecosystem: Ecosystem,
    scope: &str,
    member: Option<&str>,
    dependency: &Dependency,
) {
    // A `// indirect` go requirement is the resolver's bookkeeping, not a thing this repository asked for:
    // counting it as a declaration would call half a module's graph direct.
    let declared_is_direct = dependency.kind.as_str() != "other:indirect";
    if declared_is_direct {
        names.insert((ecosystem, dependency.name.clone()));
        if let Some(actual) = &dependency.actual {
            names.insert((ecosystem, actual.clone()));
        }
    }
    let entry = declared
        .entry((ecosystem, dependency.name.clone()))
        .or_default();
    let requirement = if dependency.requirement.is_empty() {
        "(any)".to_owned()
    } else {
        dependency.requirement.clone()
    };
    if !entry.requirements.contains(&requirement) {
        entry.requirements.push(requirement);
    }
    entry.kinds.insert(dependency.kind.as_str());
    entry.sources.insert(dependency.source.label());
    entry.scopes.insert(scope.to_owned());
    if let Some(member) = member
        && !entry.members.contains(&member.to_owned())
    {
        entry.members.push(member.to_owned());
    }
    if dependency.kind == crate::deps::model::DepKind::Peer {
        entry.is_peer = true;
    }
    match (&entry.actual, &dependency.actual) {
        (None, Some(actual)) => entry.actual = Some(actual.clone()),
        (Some(existing), Some(actual)) if existing != actual => entry.renamed_differently = true,
        _ => {}
    }
}

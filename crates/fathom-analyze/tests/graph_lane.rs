//! The graph lane: the delta chain, the anchor period, and the three schema findings `s3` measured
//! the hard way (`reco-8s4`).
//!
//! This is the store-facing half. What is asserted here is not "the SQL ran" but the four claims the
//! ticket is about: one delta per revision, a shared anchor every `graph.chain_full_every`, a
//! revision that reconstructs to exactly what a full rebuild would have produced, and a chain that
//! cannot be walked being a *stated* partial rather than a wrong answer.

use fathom_analyze::graph::{self, BuildOutcome, Builder, GraphPolicy, Interner, ModuleInput};
use fathom_analyze::structure::{
    Complexity, Edge, EdgeKind, FileLevel, FileStructure, Language, Resolution, Span, Structure,
    Supertype, SupertypeKind, Symbol, SymbolKind, SyntheticScope, Visibility,
};
use fathom_store::spine::Store;

/// The instant every revision here is stamped with.
const NOW: i64 = 2_000_000_000_000;

/// A fixture module: its path, its exports, and what it depends on.
#[derive(Debug, Clone)]
struct Fixture {
    path: String,
    exports: Vec<String>,
    deps: Vec<(String, String)>,
    bases: Vec<(String, String)>,
    synthetic: bool,
}

/// The shape a surface hands the graph: paths and names.
fn fixture(path: &str, exports: &[&str], deps: &[(&str, &str)]) -> Fixture {
    Fixture {
        path: path.to_string(),
        exports: exports.iter().map(|s| (*s).to_string()).collect(),
        deps: deps
            .iter()
            .map(|(t, s)| (t.to_string(), s.to_string()))
            .collect(),
        bases: Vec::new(),
        synthetic: false,
    }
}

impl Fixture {
    /// Add an export, as a commit would.
    fn with_export(mut self, name: &str) -> Self {
        self.exports.push(name.to_string());
        self
    }

    /// Add a dependency, as a commit would.
    fn with_dep(mut self, target: &str, symbol: &str) -> Self {
        self.deps.push((target.to_string(), symbol.to_string()));
        self
    }

    /// Declare the module's declaration list untrustworthy (`s1` finding 1).
    fn made_synthetic(mut self) -> Self {
        self.synthetic = true;
        self
    }
}

/// The structural record a fixture would have produced.
fn structure_of(fixture: &Fixture) -> FileStructure {
    let span = Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    let structure = Structure {
        symbols: fixture
            .exports
            .iter()
            .map(|name| Symbol {
                kind: SymbolKind::Fn,
                name: name.clone(),
                container: String::new(),
                span,
                visibility: Visibility::Public,
                exported: true,
                complexity: Complexity {
                    body_lines: 1,
                    branches: 0,
                    loops: 0,
                    params: 0,
                    returns: 0,
                    members: 0,
                    nesting: 0,
                },
                modifiers: Vec::new(),
            })
            .collect(),
        exports: fixture.exports.clone(),
        imports: Vec::new(),
        edges: fixture
            .deps
            .iter()
            .map(|(target, symbol)| Edge {
                target: target.clone(),
                symbol: symbol.clone(),
                kind: if symbol == "*" {
                    EdgeKind::Wildcard
                } else {
                    EdgeKind::Resolved
                },
                specifier: String::new(),
                via: None,
                form: String::new(),
                passes: 1,
                granularity: fathom_analyze::structure::Granularity::File,
                package: None,
            })
            .collect(),
        supertypes: fixture
            .bases
            .iter()
            .map(|(of, base)| Supertype {
                of: of.clone(),
                base: base.clone(),
                kind: SupertypeKind::Base,
                resolved: Some(base.clone()),
                span,
            })
            .collect(),
        synthetic_scopes: if fixture.synthetic {
            vec![SyntheticScope {
                scope: "file".to_string(),
                reasons: vec![String::from("macro_rules!")],
            }]
        } else {
            Vec::new()
        },
        notes: Vec::new(),
        resolution: Resolution {
            passes_used: 1,
            converged: true,
            bound: 4,
            truncated_chains: 0,
        },
        package: None,
    };
    FileStructure {
        path: fixture.path.clone(),
        language: Language::Rust,
        file_level: FileLevel {
            path: fixture.path.clone(),
            language: Language::Rust,
            bytes: 10,
            newlines: 1,
            blank_lines: 0,
            code_lines: 1,
            parse_errors: 0,
            refused: false,
        },
        structure: Some(structure),
        reason: None,
    }
}

/// The content address of a fixture: its content, verbatim.
///
/// A real blob is a git hash. All the lane cares about is equality, and using the content makes
/// "the blob moved" and "the file changed" the same event — which is what a content address is for.
fn blob_of(fixture: &Fixture) -> String {
    let mut parts: Vec<String> = fixture.exports.clone();
    parts.extend(fixture.deps.iter().map(|(t, s)| format!("{t}:{s}")));
    parts.extend(fixture.bases.iter().map(|(o, b)| format!("{o}<{b}")));
    if fixture.synthetic {
        parts.push(String::from("synthetic"));
    }
    format!("blob:{}", parts.join(","))
}

/// One revision's live file list, owned, so the hand-off can borrow from it.
///
/// The substrate hands over `(path, blob, structure)` triples by reference; owning the three parts
/// side by side is what lets a test say `record(&revision.inputs())` without a temporary dying in the
/// middle of the statement.
#[derive(Debug, Clone)]
struct Revision {
    files: Vec<Fixture>,
    structures: Vec<FileStructure>,
    blobs: Vec<String>,
}

impl Revision {
    fn new(files: Vec<Fixture>) -> Self {
        let structures = files.iter().map(structure_of).collect();
        let blobs = files.iter().map(blob_of).collect();
        Self {
            files,
            structures,
            blobs,
        }
    }

    /// The substrate's hand-off: one input per live module.
    fn inputs(&self) -> Vec<ModuleInput<'_>> {
        self.files
            .iter()
            .zip(self.structures.iter())
            .zip(self.blobs.iter())
            .map(|((fixture, file), blob)| ModuleInput {
                path: fixture.path.as_str(),
                blob: blob.as_str(),
                file: Some(file),
            })
            .collect()
    }

    /// Rewrite one module the way a commit would, and rebuild the derived halves.
    fn changed(mut self, path: &str, f: impl FnOnce(Fixture) -> Fixture) -> Self {
        for slot in &mut self.files {
            if slot.path == path {
                *slot = f(slot.clone());
                break;
            }
        }
        self.rebuild()
    }

    /// Add a module.
    fn plus(mut self, extra: Fixture) -> Self {
        self.files.push(extra);
        self.rebuild()
    }

    /// Remove a module.
    fn minus(mut self, path: &str) -> Self {
        self.files.retain(|file| file.path != path);
        self.rebuild()
    }

    fn rebuild(mut self) -> Self {
        self.structures = self.files.iter().map(structure_of).collect();
        self.blobs = self.files.iter().map(blob_of).collect();
        self
    }
}

fn builder(policy: GraphPolicy) -> Builder {
    Builder {
        policy,
        layers: graph::Layers::new([
            (String::from("l0"), String::from("core")),
            (String::from("l2"), String::from("state")),
            (String::from("tests"), String::from("core/tests")),
        ]),
    }
}

/// The shipped defaults, with the anchor period shortened so a test can watch a chain wrap.
fn policy() -> GraphPolicy {
    GraphPolicy {
        chain_full_every: 3,
        max_chain_walk: 3,
        ..GraphPolicy::default()
    }
}

/// Resolve a graph's rows to keys — the only comparison allowed across two dictionaries.
fn named(
    naming: &graph::Naming,
    graph: &fathom_analyze::graph::Graph,
) -> Vec<(String, String, String)> {
    let name = |id: i64| {
        naming
            .name(id)
            .map_or_else(|| format!("?{id}"), str::to_string)
    };
    let mut rows: Vec<(String, String, String)> = graph
        .rows()
        .into_iter()
        .map(|(kind, src, dst)| {
            (
                kind.as_str().to_string(),
                name(src),
                if kind.is_node() {
                    String::new()
                } else {
                    name(dst)
                },
            )
        })
        .collect();
    rows.sort();
    rows
}

/// The same resolution against a build's own dictionary.
fn named_from(outcome: &BuildOutcome) -> Vec<(String, String, String)> {
    let by_id: std::collections::BTreeMap<i64, String> = outcome
        .modules
        .values()
        .flat_map(|module| {
            std::iter::once((module.node, module.path.clone()))
                .chain(module.symbols.iter().cloned())
        })
        .collect();
    let name = |id: i64| by_id.get(&id).cloned().unwrap_or_else(|| format!("?{id}"));
    let mut rows: Vec<(String, String, String)> = outcome
        .graph
        .rows()
        .into_iter()
        .map(|(kind, src, dst)| {
            (
                kind.as_str().to_string(),
                name(src),
                if kind.is_node() {
                    String::new()
                } else {
                    name(dst)
                },
            )
        })
        .collect();
    rows.sort();
    rows
}

fn base_revision() -> Revision {
    Revision::new(vec![
        fixture("core/shape.rs", &["Shape"], &[]),
        fixture("core/circle.rs", &["Circle"], &[("core/shape.rs", "Shape")]),
        fixture(
            "state/store.rs",
            &["Store"],
            &[("core/circle.rs", "Circle")],
        ),
        fixture("app/main.rs", &["main"], &[("state/store.rs", "Store")]),
    ])
}

/// The history every chain test replays: eight revisions, one file each, including a deletion and an
/// insertion so both op directions appear.
fn history() -> Vec<Revision> {
    let mut history = vec![base_revision()];
    for step in 1..8 {
        let previous = history.last().expect("non-empty history").clone();
        let revision = if step % 4 == 0 {
            previous.minus("app/main.rs")
        } else if step % 3 == 0 {
            previous.plus(fixture(
                &format!("core/extra{step}.rs"),
                &[&format!("Extra{step}")],
                &[("core/shape.rs", "Shape")],
            ))
        } else {
            let name = format!("Extra{step}");
            previous.changed("core/circle.rs", move |file| file.with_export(&name))
        };
        history.push(revision);
    }
    history
}

/// Replay a history through one lane, returning each revision's tree oid.
fn replay(lane: &mut graph::Lane<'_>, history: &[Revision]) -> Vec<String> {
    replay_summaries(lane, history)
        .into_iter()
        .map(|(oid, _)| oid)
        .collect()
}

/// The same, keeping each revision's [`RecordSummary`], because the lane's own account of what it
/// wrote is the thing to assert against — a store read afterwards needs the lane's borrow released.
fn replay_summaries(
    lane: &mut graph::Lane<'_>,
    history: &[Revision],
) -> Vec<(String, graph::RecordSummary)> {
    let mut written: Vec<(String, graph::RecordSummary)> = Vec::new();
    for (index, revision) in history.iter().enumerate() {
        let oid = format!("tree{index:04}");
        let base = if index == 0 {
            None
        } else {
            Some(written[index - 1].0.as_str())
        };
        let when = NOW + i64::try_from(index).unwrap_or(0);
        let summary = lane
            .record(&oid, base, &revision.inputs(), when)
            .expect("record");
        written.push((oid, summary));
    }
    written
}

#[test]
fn every_revision_reconstructs_to_what_a_full_rebuild_would_have_produced() {
    // The acceptance criterion, on every revision of a history rather than a hand-picked one.
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    let oids = replay(&mut lane, &history);
    let cap = usize::try_from(policy().max_chain_walk).expect("cap");
    for (index, revision) in history.iter().enumerate() {
        let (graph, report) = lane.materialise(&oids[index]).expect("materialise");
        let rebuilt = lane.rebuild(&revision.inputs()).expect("rebuild");
        assert_eq!(
            named(&lane.naming().expect("naming"), &graph),
            named_from(&rebuilt),
            "revision {index} did not round-trip through the store"
        );
        assert!(
            report.steps < cap,
            "revision {index} walked {} steps, inside the cap",
            report.steps
        );
        assert_eq!(
            report.undecoded, 0,
            "nothing in the chain was unreadable, which would have made the graph quietly smaller"
        );
    }
}

#[test]
fn the_first_revision_of_a_repository_is_an_anchor() {
    // There is nothing to delta against, and `s3`'s "the first revision writes everything" line is
    // the same fact seen from the other side.
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    let summary = lane
        .record("tree0", None, &base_revision().inputs(), NOW)
        .expect("record");
    assert!(summary.is_anchor, "the first revision is an anchor");
    let snap = store
        .graph_snapshot("tree0")
        .expect("read")
        .expect("written");
    assert_eq!(snap.kind, "full");
    assert_eq!(snap.base_tree, None, "an anchor has no parent to walk to");
    assert!(summary.new_chunks > 0, "and it writes adjacency chunks");
}

#[test]
fn an_anchor_lands_on_the_configured_period_and_deltas_fill_the_rest() {
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    let oids = replay(&mut lane, &history);
    let kinds: Vec<String> = oids
        .iter()
        .map(|oid| {
            lane.read()
                .graph_snapshot(oid)
                .expect("read")
                .expect("written")
                .kind
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "full", "delta", "delta", "full", "delta", "delta", "full", "delta"
        ],
        "an anchor every {} revisions; the deltas between them are the chain depth",
        policy().chain_full_every
    );
    assert_eq!(lane.anchors(), 3);
    assert_eq!(lane.revisions(), 8);
}

#[test]
fn a_delta_costs_a_fraction_of_the_anchor_it_follows() {
    // The storage claim in miniature: not "the total is small" but "the total is small because the
    // anchor is shared", which is the whole argument of ADR-004 §13.7.
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    let oids = replay(&mut lane, &history);
    let bytes = lane.graph_bytes().expect("bytes");
    let revisions = i64::try_from(oids.len()).unwrap();
    assert_eq!(
        bytes.manifest_rows, 3,
        "one manifest per anchor, and the anchors landed on the period"
    );
    assert!(
        bytes.chunk_rows <= 4 * 3,
        "chunks are content-addressed and therefore shared: three anchors over a four-module \
         repository must not have written 3x duplicated adjacency, {} chunk rows",
        bytes.chunk_rows
    );
    assert!(
        bytes.delta_rows > 0,
        "the deltas between anchors wrote rows"
    );
    let per_revision = bytes.total() / revisions;
    assert!(
        per_revision < 4_096,
        "a four-file fixture costs {per_revision} B per revision against a 32 KiB budget; the \
         point of the strategy is that this is nowhere near it: {bytes:?}"
    );
}

#[test]
fn a_chain_with_no_anchor_inside_the_cap_is_a_stated_partial_not_a_wrong_answer() {
    // The one gotcha the schema cannot fix. A long chain is evidence of a lost anchor, and the only
    // safe response is to refuse and say so — never to return the partial graph as if it were the
    // revision.
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let stranded = GraphPolicy {
        chain_full_every: 64,
        max_chain_walk: 2,
        ..GraphPolicy::default()
    };
    let mut lane = graph::Lane::new(&mut store, builder(stranded)).expect("lane");
    let oids = replay(&mut lane, &history);
    let outcome = lane.materialise(oids.last().expect("last"));
    let Err(partial) = outcome else {
        panic!(
            "eight deltas with no reachable anchor must not reconstruct: {:?}",
            outcome.map(|(_, report)| report.sentence())
        );
    };
    assert!(
        partial.sentence().contains("anchor"),
        "the sentence names the missing thing: {}",
        partial.sentence()
    );
    assert_eq!(
        partial.code(),
        "graph.chain.too_long",
        "and the code distinguishes it from an anchor that is simply gone"
    );
}

#[test]
fn a_policy_that_could_strand_a_revision_says_so_before_anything_is_written() {
    // `graph.query.max_chain_walk` above `graph.chain_full_every` switches the tripwire off silently,
    // which is exactly the kind of config drift that ends in a confident wrong answer.
    let (policy, notices) = graph::GraphPolicy::from_repo_config_text(
        "[graph]\nchain_full_every = 8\n\n[graph.query]\nmax_chain_walk = 32\n",
    );
    assert_eq!(policy.chain_full_every, 8);
    assert!(
        policy
            .notices()
            .iter()
            .any(|notice| notice.contains("lost anchor")),
        "{:?}",
        policy.notices()
    );
    let _ = notices;
}

#[test]
fn a_second_commit_with_the_same_tree_keeps_the_first_base_tree() {
    // `s3` finding 3, found by asserting the wrong thing first. A merge commit and the commit it
    // merges share a tree; overwriting `base_tree` would move a snapshot's parent under a chain that
    // was already written against the old one, and the graph would become unreachable from itself.
    let mut store = Store::open_in_memory().expect("open store");
    let revision = base_revision();
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    lane.record("tree0", None, &revision.inputs(), NOW)
        .expect("first");
    let second = lane
        .record("tree1", Some("tree0"), &revision.inputs(), NOW + 1)
        .expect("second");
    let duplicate = lane
        .record("tree0", Some("tree1"), &revision.inputs(), NOW + 2)
        .expect("duplicate");
    assert!(
        duplicate.duplicate_tree,
        "the lane says the write was a duplicate"
    );
    assert_eq!(duplicate.rows_written, 0, "and nothing was written");
    let snap = store.graph_snapshot("tree0").expect("read").expect("there");
    assert_eq!(
        snap.base_tree, None,
        "the anchor kept its own parent; the later write was counted and dropped"
    );
    assert_eq!(snap.built_utc, NOW, "and it kept its timestamp too");
    assert!(!second.duplicate_tree);
}

#[test]
fn a_node_row_is_stored_with_an_empty_destination_because_the_key_leaves_no_choice() {
    // `s3` finding 1. Pinned so a future migration that "tidied" this by storing the id twice does
    // not silently double what every node row means.
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    lane.record("tree0", None, &base_revision().inputs(), NOW)
        .expect("record");
    let revision = base_revision().plus(fixture(
        "core/new.rs",
        &["New"],
        &[("core/shape.rs", "Shape")],
    ));
    lane.record("tree1", Some("tree0"), &revision.inputs(), NOW + 1)
        .expect("record");
    let ops = lane.read().graph_ops("tree1").expect("ops");
    let node_ops: Vec<_> = ops
        .iter()
        .filter(|op| {
            fathom_analyze::graph::RowKind::parse(&op.kind)
                .is_some_and(fathom_analyze::graph::RowKind::is_node)
        })
        .collect();
    assert!(!node_ops.is_empty(), "a new module writes its node row");
    assert!(
        node_ops.iter().all(|op| op.dst.is_empty()),
        "and the object slot is the empty string, never a repeated id: {node_ops:?}"
    );
    assert!(
        ops.iter()
            .filter(|op| {
                fathom_analyze::graph::RowKind::parse(&op.kind)
                    .is_some_and(|kind| !fathom_analyze::graph::RowKind::is_node(kind))
            })
            .all(|op| !op.dst.is_empty()),
        "an edge row always names its object"
    );
}

#[test]
fn an_anchor_manifest_is_a_chunk_row_of_its_own_or_the_link_cannot_point_at_it() {
    // `s3` finding 2: the finding a failing foreign key produced, not a reading of the schema.
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    lane.record("tree0", None, &base_revision().inputs(), NOW)
        .expect("record");
    let manifest = lane
        .read()
        .graph_anchor_manifest("tree0")
        .expect("read")
        .expect("an anchor has a manifest");
    let text = String::from_utf8(manifest).expect("utf-8");
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        !lines.is_empty(),
        "the manifest names one line per live module"
    );
    let bytes = lane.graph_bytes().expect("bytes");
    assert_eq!(
        bytes.manifest_rows, 1,
        "and it is a chunk row in its own right, because the link's foreign key reaches `graph_chunk`"
    );
    assert!(
        bytes.chunk_rows >= 1,
        "the adjacency chunks it names are counted too: {bytes:?}"
    );
}

#[test]
fn interning_is_a_dictionary_of_ids_and_a_delta_of_numbers() {
    // The reduction `s3` measured at a 15% saving and did not spend. Here it is spent: a delta row's
    // endpoints are dictionary ids, so a module named in a thousand commits costs a few bytes in each
    // of them and one row overall.
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    replay(&mut lane, &history);
    let nodes = lane.read().graph_nodes().expect("dictionary");
    let modules: Vec<&str> = nodes
        .values()
        .filter(|node| node.is_module())
        .map(|node| node.key.as_str())
        .collect();
    assert_eq!(
        modules.len(),
        modules
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        "a path is interned once, not once per revision"
    );
    for op in lane.read().graph_ops("tree0004").expect("ops") {
        assert!(
            op.src.parse::<i64>().is_ok(),
            "a delta's endpoint is an id, not a path: {op:?}"
        );
    }
    assert!(
        nodes
            .values()
            .any(|node| node.is_module() && node.layer == "l0"),
        "the dictionary carries the layer tag too, so a layering question costs one read"
    );
    let ids: Vec<i64> = nodes.keys().copied().collect();
    assert_eq!(
        ids.first().copied(),
        Some(0),
        "ids are dense from zero, which is what makes them cheap to store"
    );
}

#[test]
fn ids_survive_a_reopen_instead_of_being_minted_again() {
    // Interning is only a saving if it is durable. A lane that re-minted on reopen would grow the
    // dictionary forever and silently change what every stored row means.
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let oids = {
        let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
        replay(&mut lane, &history)
    };
    let before = store.graph_nodes().expect("dictionary");
    let mut second = graph::Lane::new(&mut store, builder(policy())).expect("reopen");
    let revision =
        history
            .last()
            .expect("last")
            .clone()
            .plus(fixture("core/late.rs", &["Late"], &[]));
    second
        .record(
            "tree99",
            oids.last().map(String::as_str),
            &revision.inputs(),
            NOW + 99,
        )
        .expect("record after reopen");
    let after = store.graph_nodes().expect("dictionary");
    assert_eq!(
        after.len(),
        before.len() + 2,
        "one module and one symbol were added, and nothing was re-minted"
    );
    for (id, node) in &before {
        assert_eq!(
            after.get(id).map(|n| n.key.as_str()),
            Some(node.key.as_str()),
            "id {id} still means the same thing"
        );
    }
}

#[test]
fn a_removal_then_a_re_add_within_one_chain_comes_back_present() {
    // The chain is applied oldest-first, and this is the case that breaks if it is not: the module's
    // rows are removed and then added again, so replaying the two deltas newest-first ends with the
    // module missing from a revision that has it. Every other test here uses a history where the
    // mistake happens to cancel out, which is exactly why this one is written out.
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    let base = base_revision();
    lane.record("tree0", None, &base.inputs(), NOW)
        .expect("anchor");
    let without = base.clone().minus("core/circle.rs");
    lane.record("tree1", Some("tree0"), &without.inputs(), NOW + 1)
        .expect("remove");
    lane.record("tree2", Some("tree1"), &base.inputs(), NOW + 2)
        .expect("re-add");
    let (graph, report) = lane.materialise("tree2").expect("materialise");
    assert_eq!(report.deltas, 2, "both deltas replayed onto the anchor");
    let naming = lane.naming().expect("naming");
    assert!(
        graph::dependents_of(&graph, &naming, "core/shape.rs")
            .named
            .iter()
            .any(|name| name.contains("core/circle.rs")),
        "circle came back: {:?}",
        graph::dependents_of(&graph, &naming, "core/shape.rs").named
    );
    let (gone, _) = lane.materialise("tree1").expect("materialise the middle");
    assert!(
        !graph::dependents_of(&gone, &naming, "core/shape.rs")
            .named
            .iter()
            .any(|name| name.contains("core/circle.rs")),
        "and it really was gone in between, or the two deltas are not being distinguished"
    );
}

#[test]
fn stepping_one_commit_applies_one_delta() {
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    let oids = replay(&mut lane, &history);
    let start = lane
        .step(&lane.cursor.clone().expect("cursor"), &oids[0])
        .expect("step onto the anchor")
        .0;
    let (moved, step) = lane.step(&start, &oids[1]).expect("step one forward");
    assert_eq!(step.kind, graph::StepKind::DeltaApplied);
    assert_eq!(
        step.ops,
        lane.read().graph_ops(&oids[1]).expect("ops").len(),
        "one commit, one delta, applied"
    );
    assert_eq!(moved.tree_oid, oids[1]);
    let (back, step) = lane.step(&moved, &oids[0]).expect("step back");
    assert_eq!(
        step.kind,
        graph::StepKind::AnchorLoaded,
        "onto an anchor there is nothing to apply"
    );
    assert_eq!(back.tree_oid, oids[0]);
}

#[test]
fn a_step_onto_a_revision_the_store_has_never_seen_is_not_an_empty_graph() {
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    replay(&mut lane, &history);
    let cursor = lane.cursor.clone().expect("cursor");
    let Err(partial) = lane.step(&cursor, "tree9999") else {
        panic!("an unknown revision must not answer with a graph");
    };
    assert!(partial.sentence().contains("tree9999"));
}

#[test]
fn changed_edges_between_adjacent_revisions_come_from_the_store() {
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    let oids = replay(&mut lane, &history);
    let adjacent = lane.changed_edges(&oids[1], &oids[2]).expect("changed");
    assert!(
        adjacent.from_store,
        "adjacent is one indexed read, and the answer admits which path it took"
    );
    let far = lane.changed_edges(&oids[0], &oids[7]).expect("changed far");
    assert!(
        !far.from_store,
        "non-adjacent costs two materialisations, and says so"
    );
    let itself = lane
        .changed_edges(&oids[3], &oids[3])
        .expect("same revision");
    assert!(
        itself.added.is_empty() && itself.removed.is_empty(),
        "a revision has no changed edges against itself"
    );
}

#[test]
fn a_materialised_revision_answers_the_five_queries() {
    // The queries are exercised against a graph the lane *read*, not one it just built — which is
    // the difference between testing the store and testing a data structure.
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    let revision = base_revision()
        .plus(fixture(
            "app/wildcard.rs",
            &["go"],
            &[("core/shape.rs", "*")],
        ))
        .plus(fixture("app/magic.rs", &["Magic"], &[]).with_dep("core/shape.rs", "Shape"))
        .changed("app/main.rs", |file| {
            file.with_dep("core/shape.rs", "Shape")
        });
    lane.record("tree0", None, &revision.inputs(), NOW)
        .expect("record");
    let (graph, _) = lane.materialise("tree0").expect("materialise");
    let naming = lane.naming().expect("naming");

    let deps = graph::dependencies_of(&graph, &naming, "app/main.rs");
    assert!(
        deps.named.iter().any(|n| n.contains("state/store.rs")),
        "{:?}",
        deps.named
    );

    let fans = graph::dependents_of(&graph, &naming, "core/shape.rs");
    assert!(
        fans.named.iter().any(|n| n.contains("core/circle.rs")),
        "{:?}",
        fans.named
    );
    assert!(
        fans.wildcard.iter().any(|n| n.contains("app/wildcard.rs")),
        "an unknown-contents dependency is reported separately, never folded into `nobody`: {:?}",
        fans.wildcard
    );

    let route = graph::path(&graph, &naming, "app/main.rs", "core/shape.rs", 6);
    assert!(route.found, "{:?}", route.hops);

    let layer = graph::layer_subgraph(&graph, &naming, "l0");
    assert!(
        layer
            .nodes
            .iter()
            .any(|node| node.starts_with("core/shape.rs")),
        "{:?}",
        layer.nodes
    );
}

#[test]
fn an_incomplete_module_keeps_a_dependent_list_from_claiming_completeness() {
    // `s1` finding 1, asserted through the store: the flag is a column on the dictionary row, so it
    // survives a rebuild and reaches the answer that needs it.
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    let revision = base_revision()
        .plus(fixture("app/magic.rs", &["Magic"], &[("core/shape.rs", "Shape")]).made_synthetic())
        .changed("app/main.rs", |file| {
            file.with_dep("core/shape.rs", "Shape")
        });
    lane.record("tree0", None, &revision.inputs(), NOW)
        .expect("record");
    let (graph, _) = lane.materialise("tree0").expect("materialise");
    let naming = lane.naming().expect("naming");
    let fans = graph::dependents_of(&graph, &naming, "core/shape.rs");
    assert!(
        !fans.complete,
        "one dependent's declaration list may have shrunk, so this list may be short: {fans:?}"
    );
    let nodes = lane.read().graph_nodes().expect("dictionary");
    assert!(
        nodes.values().any(|node| node.incomplete),
        "the store carries the flag, so a rebuild does not lose it"
    );
}

#[test]
fn the_store_reports_its_bytes_with_the_basis_attached() {
    let history = history();
    let mut store = Store::open_in_memory().expect("open store");
    let mut lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    replay(&mut lane, &history);
    let bytes = lane.graph_bytes().expect("graph bytes");
    assert!(bytes.delta_rows > 0 && bytes.chunk_rows > 0, "{bytes:?}");
    let sentence = bytes.sentence();
    assert!(
        sentence.contains("counted as payload"),
        "the number has to say what it is: {sentence}"
    );
    let store_bytes = lane.usage().expect("usage");
    assert!(
        store_bytes.sentence().contains("measured as"),
        "and so does the store-level one: {}",
        store_bytes.sentence()
    );
}

#[test]
fn the_interner_mints_once_and_offers_a_fresh_entry_once() {
    let mut interner = Interner::default();
    let first = interner.id("core/a.rs", "module", "l0", false);
    let again = interner.id("core/a.rs", "module", "l0", false);
    assert_eq!(first, again, "the same key is the same id");
    assert_eq!(
        interner.fresh().len(),
        1,
        "and it is offered to the store once"
    );
    interner.mark_flushed();
    assert!(interner.fresh().is_empty(), "and not offered twice");
    assert_eq!(
        interner.key_of(first),
        Some("core/a.rs"),
        "and the reverse direction works, because a materialised graph has to be named"
    );
}

#[test]
fn an_empty_store_refuses_to_materialise_a_revision_it_has_never_seen() {
    let mut store = Store::open_in_memory().expect("open store");
    let lane = graph::Lane::new(&mut store, builder(policy())).expect("lane");
    assert!(
        lane.materialise("nothing").is_err(),
        "nothing to reconstruct, and that is said out loud"
    );
    let counts = lane.read().graph_row_counts().expect("counts");
    assert_eq!(
        counts.get("graph_snapshot"),
        Some(&0),
        "and the tables exist and are empty, which is the migrated-schema check"
    );
}

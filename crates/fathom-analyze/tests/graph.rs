//! The graph model, the builder, the delta and the five queries (`e3.2`, `reco-8s4`).
//!
//! The store-facing half — anchors, chain walks, the three §4.6 rough edges — is
//! `tests/graph_lane.rs`, which needs a SQLite backend. What lives here needs no store at all, which
//! is the point: the graph's semantics are a pure function of the per-file substrate, and ADR-002
//! §6.1 says a build with no SQLite must still be able to say so.

use fathom_analyze::structure::{
    Complexity, Edge, EdgeKind, FileLevel, FileStructure, Granularity, Language, Resolution, Span,
    Structure, Supertype, SupertypeKind, Symbol, SymbolKind, SyntheticScope, Visibility,
};
use fathom_analyze::{graph, structure};

/// The policy every test runs under, with layer tags shaped like a repository's.
fn builder() -> graph::Builder {
    let policy = graph::GraphPolicy::default();
    let layers = graph::Layers::new([
        ("l0".to_string(), "core".to_string()),
        ("l2".to_string(), "state".to_string()),
        ("tests".to_string(), "core/tests".to_string()),
    ]);
    graph::Builder { policy, layers }
}

/// The blob for a fixture file: its content, verbatim.
///
/// The lane compares blobs to decide whether a module changed, so the fixture makes the blob a
/// function of the content exactly the way git's does — no hash is needed to make the property true,
/// and a fake hash would only blur which side of the seam the change test is on.
fn blob_of(file: &FileStructure) -> String {
    format!(
        "blob:{}",
        file.structure.as_ref().map_or_else(
            || "none".to_string(),
            |s| format!(
                "{}|{}|{}",
                s.exports.len(),
                s.edges.len(),
                s.supertypes.len()
            )
        )
    )
}

/// A fixture file: exports, dependencies as `(target, symbol)`, bases as `(of, base, kind)`.
fn file(
    path: &str,
    exports: &[&str],
    deps: &[(&str, &str)],
    bases: &[(&str, &str, &str, SupertypeKind)],
) -> FileStructure {
    let span = Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    let structure = Structure {
        symbols: exports
            .iter()
            .map(|name| Symbol {
                kind: SymbolKind::Fn,
                name: (*name).to_string(),
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
        exports: exports.iter().map(|name| (*name).to_string()).collect(),
        imports: Vec::new(),
        edges: deps
            .iter()
            .map(|(target, symbol)| Edge {
                target: (*target).to_string(),
                symbol: (*symbol).to_string(),
                kind: if *symbol == "*" {
                    EdgeKind::Wildcard
                } else {
                    EdgeKind::Resolved
                },
                specifier: String::new(),
                via: None,
                form: String::new(),
                passes: 1,
                granularity: Granularity::File,
                package: None,
            })
            .collect(),
        supertypes: bases
            .iter()
            .map(|(of, base, resolved, kind)| Supertype {
                of: (*of).to_string(),
                base: (*base).to_string(),
                kind: *kind,
                resolved: Some((*resolved).to_string()),
                span,
            })
            .collect(),
        synthetic_scopes: Vec::new(),
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
        path: path.to_string(),
        language: Language::Rust,
        file_level: FileLevel {
            path: path.to_string(),
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

/// A file whose grammar could not be read at all — the case that must never vanish from the graph.
fn unreadable(path: &str) -> FileStructure {
    let mut record = file(path, &[], &[], &[]);
    record.structure = None;
    record.reason = Some(structure::UnavailableReason::GrammarNotCompiled {
        feature: "lang-ruby".to_string(),
    });
    record
}

/// A file whose declaration list cannot be trusted: `synthetic_eligible`, `s1` finding 1.
fn synthetic(path: &str, exports: &[&str], deps: &[(&str, &str)]) -> FileStructure {
    let mut record = file(path, exports, deps, &[]);
    if let Some(inner) = record.structure.as_mut() {
        inner.synthetic_scopes = vec![SyntheticScope {
            scope: "file".to_string(),
            reasons: vec![String::from("macro_rules!")],
        }];
    }
    record
}

fn inputs<'a>(files: &'a [FileStructure], blobs: &'a [String]) -> Vec<graph::ModuleInput<'a>> {
    files
        .iter()
        .zip(blobs)
        .map(|(f, blob)| graph::ModuleInput {
            path: f.path.as_str(),
            blob: blob.as_str(),
            file: Some(f),
        })
        .collect()
}

fn build(files: &[FileStructure]) -> graph::BuildOutcome {
    let blobs: Vec<String> = files.iter().map(blob_of).collect();
    let mut interner = graph::Interner::default();
    builder().build(&inputs(files, &blobs), &mut interner)
}

/// Resolve a graph's rows back to `(kind, src, dst)` in keys: the comparison that ignores which
/// numbers the interner happened to hand out.
fn named(outcome: &graph::BuildOutcome) -> Vec<(String, String, String)> {
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

#[test]
fn a_revision_is_modules_symbols_and_their_edges() {
    let files = vec![
        file("core/shape.rs", &["Shape"], &[], &[]),
        file(
            "core/circle.rs",
            &["Circle"],
            &[("core/shape.rs", "Shape")],
            &[("Circle", "Shape", "core/shape.rs", SupertypeKind::Interface)],
        ),
    ];
    let outcome = build(&files);
    let rows = named(&outcome);
    assert!(
        rows.contains(&("module".to_string(), "core/shape.rs".into(), String::new())),
        "the module node is missing: {rows:?}"
    );
    assert!(
        rows.contains(&(
            "symbol".to_string(),
            "core/shape.rs::Shape".into(),
            String::new()
        )),
        "an exported symbol is a node: {rows:?}"
    );
    assert!(
        rows.contains(&(
            "contains".to_string(),
            "core/shape.rs".into(),
            "core/shape.rs::Shape".into()
        )),
        "a module contains its exports: {rows:?}"
    );
    assert!(
        rows.contains(&(
            "imports".to_string(),
            "core/circle.rs".into(),
            "core/shape.rs".into()
        )),
        "a resolved dependency is an import: {rows:?}"
    );
    assert!(
        rows.contains(&(
            "uses".to_string(),
            "core/circle.rs".into(),
            "core/shape.rs::Shape".into()
        )),
        "a named symbol dependency reaches the symbol node: {rows:?}"
    );
    assert!(
        rows.contains(&(
            "implements".to_string(),
            "core/circle.rs::Circle".into(),
            "core/shape.rs::Shape".into()
        )),
        "an implemented interface reaches the interface's own node: {rows:?}"
    );
}

#[test]
fn a_dependency_with_unknown_contents_is_its_own_edge_kind() {
    // `s1` finding 2, as a shape requirement: a wildcard dependency is stored, not dropped, or the
    // answer to "who depends on this" is "nobody".
    let files = vec![
        file("core/models.py", &["Model"], &[], &[]),
        file("app/views.py", &["view"], &[("core/models.py", "*")], &[]),
    ];
    let outcome = build(&files);
    assert_eq!(outcome.summary.wildcard, 1);
    let naming = graph::Naming::from_vertices(outcome.modules.values().flat_map(|module| {
        std::iter::once(graph::Vertex {
            id: module.node,
            key: module.path.clone(),
            kind: graph::RowKind::Module,
            layer: String::new(),
            incomplete: module.incomplete.is_some(),
        })
        .chain(module.symbols.iter().map(|(id, key)| graph::Vertex {
            id: *id,
            key: key.clone(),
            kind: graph::RowKind::Symbol,
            layer: String::new(),
            incomplete: false,
        }))
    }));
    let fans = graph::dependents_of(&outcome.graph, &naming, "core/models.py");
    assert_eq!(
        fans.named,
        Vec::<String>::new(),
        "a wildcard is not a named caller"
    );
    assert_eq!(fans.wildcard, vec!["app/views.py".to_string()]);
    assert!(!fans.is_empty(), "and it is certainly not \"nobody\"");
}

#[test]
fn a_package_granular_dependency_is_not_a_file_to_file_claim() {
    // `s1` finding 4: in Go the dependency is the package. The fixture marks one edge as
    // package-granular and checks the answer keeps it apart.
    let mut importer = file("main.go", &["main"], &[("shape/circle.go", "*")], &[]);
    if let Some(inner) = importer.structure.as_mut() {
        inner.edges[0].granularity = Granularity::Package;
        inner.edges[0].package = Some("shape".to_string());
    }
    let files = vec![file("shape/circle.go", &["Circle"], &[], &[]), importer];
    let outcome = build(&files);
    let rows = named(&outcome);
    assert!(
        rows.iter().any(|(kind, _, _)| kind == "imports_package"),
        "a Go dependency is its own kind: {rows:?}"
    );
}

#[test]
fn an_unreadable_file_is_still_a_node() {
    // A dropped file is an invisible dependency (`e3.1`'s rule, inherited whole).
    let files = vec![
        file("core/a.rs", &["A"], &[], &[]),
        unreadable("legacy/x.rb"),
    ];
    let outcome = build(&files);
    let rows = named(&outcome);
    assert!(
        rows.contains(&("module".to_string(), "legacy/x.rb".into(), String::new())),
        "the file that could not be read is still in the graph: {rows:?}"
    );
    assert_eq!(outcome.summary.unavailable, 1);
    assert!(
        outcome.summary.caveat().is_some(),
        "and the revision says so"
    );
}

#[test]
fn an_ignored_directory_is_not_in_the_graph() {
    let files = vec![
        file("core/a.rs", &["A"], &[], &[]),
        file("web/node_modules/left-pad/index.js", &["leftPad"], &[], &[]),
    ];
    let outcome = build(&files);
    assert_eq!(outcome.summary.ignored, 1);
    assert!(
        !named(&outcome)
            .iter()
            .any(|(_, src, _)| src.contains("node_modules")),
        "a vendored tree is excluded entirely"
    );
}

#[test]
fn an_unchanged_module_produces_no_ops() {
    let before = vec![
        file("core/a.rs", &["A"], &[], &[]),
        file("core/b.rs", &["B"], &[("core/a.rs", "A")], &[]),
    ];
    let after = vec![
        file("core/a.rs", &["A"], &[], &[]),
        file("core/b.rs", &["B"], &[("core/a.rs", "A")], &[]),
    ];
    let previous = build(&before);
    let next = build(&after);
    let ops = graph::revision_ops(&previous.modules, &next.modules);
    assert!(
        ops.is_empty(),
        "nothing moved, so nothing is written: {ops:?}"
    );
    assert!(
        graph::changed_modules(&previous.modules, &next.modules).is_empty(),
        "and no module even counts as changed"
    );
}

#[test]
fn a_removed_module_writes_the_removal_of_every_row_it_owned() {
    let before = vec![
        file("core/a.rs", &["A"], &[], &[]),
        file("core/b.rs", &["B"], &[("core/a.rs", "A")], &[]),
    ];
    let after = vec![file("core/a.rs", &["A"], &[], &[])];
    let previous = build(&before);
    let next = build(&after);
    let ops = graph::revision_ops(&previous.modules, &next.modules);
    assert!(
        ops.iter().all(|op| !op.add),
        "a deletion is only removals: {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.row.0 == graph::RowKind::Module
            && op.row.1 == previous.modules["core/b.rs"].node),
        "including the module node itself"
    );
}

#[test]
fn removals_are_applied_before_additions() {
    // A rename has to read as a move. `e3.2`'s reconstruction applies ops in stored order, so the
    // order is part of the answer and not a detail of it.
    let before = vec![file("core/old.rs", &["Old"], &[], &[])];
    let after = vec![file("core/new.rs", &["New"], &[], &[])];
    let ops = graph::revision_ops(&build(&before).modules, &build(&after).modules);
    let first_add = ops.iter().position(|op| op.add);
    let last_remove = ops.iter().rposition(|op| !op.add);
    assert!(
        first_add.is_none_or(|at| last_remove.is_none_or(|rm| at > rm)),
        "every removal precedes every addition: {ops:?}"
    );
}

#[test]
fn a_rebuild_from_nothing_yields_the_same_rows() {
    // The acceptance criterion, stated the only way it can be: compared by *key*. Two orders of the
    // same facts mint different ids, and a test that compared ids would be comparing the order.
    let files = vec![
        file("core/a.rs", &["A"], &[], &[]),
        file("core/b.rs", &["B"], &[("core/a.rs", "A")], &[]),
        file("core/c.rs", &["C"], &[("core/b.rs", "B")], &[]),
    ];
    let forward = build(&files);
    let mut shuffled = files.clone();
    shuffled.reverse();
    let backward = build(&shuffled);
    assert_eq!(named(&forward), named(&backward));
    assert_eq!(forward.summary, backward.summary);
}

#[test]
fn dependents_split_named_unknown_and_package_granular() {
    let mut go_style = file("main.go", &["main"], &[("shape/circle.go", "*")], &[]);
    if let Some(inner) = go_style.structure.as_mut() {
        inner.edges[0].granularity = Granularity::Package;
    }
    let files = vec![
        file("shape/circle.go", &["Circle"], &[], &[]),
        file(
            "app/one.rs",
            &["One"],
            &[("shape/circle.go", "Circle")],
            &[],
        ),
        go_style,
    ];
    let outcome = build(&files);
    let naming = naming_of(&outcome);
    let fans = graph::dependents_of(&outcome.graph, &naming, "shape/circle.go");
    assert_eq!(fans.named, vec!["app/one.rs".to_string()]);
    assert_eq!(fans.package_granular, vec!["main.go".to_string()]);
    assert!(
        fans.wildcard.is_empty(),
        "the Go edge is package-granular, not an unknown-contents wildcard"
    );
    assert!(
        fans.sentence()
            .contains("name a package rather than a file"),
        "and the sentence says which: {}",
        fans.sentence()
    );
}

#[test]
fn a_synthetic_scope_stops_a_caller_list_claiming_to_be_complete() {
    // `s1` finding 1, as an assertion about an answer rather than a note in a struct.
    let honest = file("core/a.rs", &["A"], &[], &[]);
    let files = vec![
        honest,
        synthetic("app/magic.rs", &["Magic"], &[("core/a.rs", "A")]),
    ];
    let outcome = build(&files);
    let naming = naming_of(&outcome);
    let fans = graph::dependents_of(&outcome.graph, &naming, "core/a.rs");
    assert_eq!(fans.named, vec!["app/magic.rs".to_string()]);
    assert!(
        !fans.complete,
        "one dependent's declaration list may have shrunk, so this list may be short"
    );
    assert!(fans.sentence().contains("may have shrunk"));
}

#[test]
fn a_path_is_found_or_declared_absent_within_the_bound() {
    let files = vec![
        file("core/a.rs", &["A"], &[], &[]),
        file("core/b.rs", &["B"], &[("core/a.rs", "B")], &[]),
        file("core/c.rs", &["C"], &[("core/b.rs", "C")], &[]),
    ];
    let outcome = build(&files);
    let naming = naming_of(&outcome);
    let found = graph::path(&outcome.graph, &naming, "core/c.rs", "core/a.rs", 6);
    assert!(found.found && !found.truncated);
    assert_eq!(found.hops, vec!["core/c.rs", "core/b.rs", "core/a.rs"]);

    let bound = graph::path(&outcome.graph, &naming, "core/c.rs", "core/a.rs", 1);
    assert!(!bound.found);
    assert!(
        bound.truncated,
        "the bound stopped it, and the answer says that rather than \"no path\": {}",
        bound.sentence()
    );

    let absent = graph::path(&outcome.graph, &naming, "core/a.rs", "core/c.rs", 6);
    assert!(
        !absent.found && !absent.truncated,
        "a is not a dependent of c"
    );
}

#[test]
fn a_layer_subgraph_knows_its_crossings() {
    let files = vec![
        file("core/a.rs", &["A"], &[], &[]),
        file("state/b.rs", &["B"], &[("core/a.rs", "B")], &[]),
        file("core/tests/t.rs", &["T"], &[("core/a.rs", "T")], &[]),
    ];
    let outcome = build(&files);
    let naming = naming_of(&outcome);
    let core = graph::layer_subgraph(&outcome.graph, &naming, "l0");
    assert!(
        core.nodes.contains(&"core/a.rs".to_string()),
        "{:?}",
        core.nodes
    );
    assert!(
        !core.nodes.iter().any(|n| n.starts_with("core/tests")),
        "the narrower rule wins, so the test directory is its own layer: {:?}",
        core.nodes
    );
    assert_eq!(
        core.nodes.iter().filter(|n| !n.contains("::")).count(),
        1,
        "one module, tagged l0"
    );
    assert!(
        core.inbound.contains_key("state/b.rs") || core.inbound.contains_key("core/tests/t.rs"),
        "edges entering the layer are counted separately from those inside it: {:?}",
        core.inbound
    );
    let tests = graph::layer_subgraph(&outcome.graph, &naming, "tests");
    assert_eq!(
        tests.internal_edges, 1,
        "the test module and its symbol are both in the layer, so its own `contains` edge is inside"
    );
    assert!(
        tests.crossings() > 0,
        "and its dependency on l0 crosses: {:?}",
        tests.outbound
    );
}

#[test]
fn changed_edges_between_two_revisions_names_what_moved() {
    // One interner across both revisions, as the store has one dictionary for the whole repository.
    // Comparing two graphs built by two dictionaries would be comparing numbers that mean different
    // things, which is the mistake `changed_edges` has to avoid more than it has to avoid being slow.
    let mut interner = graph::Interner::default();
    let one = vec![
        file("core/a.rs", &["A"], &[], &[]),
        file("core/b.rs", &["B"], &[("core/a.rs", "A")], &[]),
    ];
    let two = vec![
        file("core/a.rs", &["A"], &[], &[]),
        file("core/c.rs", &["C"], &[("core/a.rs", "C")], &[]),
    ];
    let before = {
        let blobs: Vec<String> = one.iter().map(blob_of).collect();
        builder().build(&inputs(&one, &blobs), &mut interner)
    };
    let after = {
        let blobs: Vec<String> = two.iter().map(blob_of).collect();
        builder().build(&inputs(&two, &blobs), &mut interner)
    };
    let delta = graph::EdgeDelta::between(&before.graph, &after.graph);
    assert!(delta.unchanged > 0, "most of the graph did not move");
    assert!(
        delta
            .removed
            .iter()
            .any(|(kind, _, _)| *kind == graph::RowKind::Imports),
        "b's import went away: {:?}",
        delta.removed
    );
    assert!(
        delta
            .added
            .iter()
            .any(|(kind, _, _)| *kind == graph::RowKind::Imports),
        "c's import arrived: {:?}",
        delta.added
    );
    assert!(!delta.from_store, "this pair was diffed, not read");
}

#[test]
fn the_chunk_codec_round_trips_and_refuses_what_it_cannot_read() {
    let rows = vec![
        (graph::RowKind::Module, 1_i64, 0_i64),
        (graph::RowKind::Imports, 1, 2),
        (graph::RowKind::Symbol, 2, 0),
    ];
    let payload = graph::encode_rows(&rows);
    let (back, skipped) = graph::decode_rows(&payload);
    assert_eq!(back, rows);
    assert_eq!(skipped, 0);
    let (partial, skipped) = graph::decode_rows(b"nonsense\nimports\t1\t2\n");
    assert_eq!(partial.len(), 1, "the readable line survived");
    assert_eq!(
        skipped, 1,
        "and the unreadable one is counted, not dropped silently"
    );
}

#[test]
fn layer_rules_are_specificity_ordered_and_globbed() {
    let layers = graph::Layers::new([
        ("l3".to_string(), "crates/fathom-analyze".to_string()),
        (
            "tests".to_string(),
            "crates/fathom-analyze/tests/**".to_string(),
        ),
    ]);
    assert_eq!(layers.layer_of("crates/fathom-analyze/src/x.rs"), "l3");
    assert_eq!(
        layers.layer_of("crates/fathom-analyze/tests/y.rs"),
        "tests",
        "the longer rule wins, so a test directory is carved out of its crate"
    );
    assert_eq!(
        layers.layer_of("docs/readme.md"),
        "",
        "untagged is not a guess"
    );
}

#[test]
fn the_policy_reads_its_config_and_notes_what_it_refused() {
    let text = "[graph]\nchain_full_every = 8\nignored_dirs = [\"vendor\"]\n\n[graph.query]\nmax_chain_walk = \"soon\"\n";
    let (policy, notices) = graph::GraphPolicy::from_repo_config_text(text);
    assert_eq!(policy.chain_full_every, 8);
    assert_eq!(
        policy.max_chain_walk,
        graph::DEFAULT_MAX_CHAIN_WALK,
        "a value that is not a number is noted and ignored, never guessed"
    );
    assert!(notices.iter().any(|n| n.contains("max_chain_walk")));
    assert!(
        policy.ignores("web/vendor/x.js"),
        "a configured directory is ignored at any depth"
    );
    assert!(
        policy.ignores("node_modules/x.js"),
        "and the built-in list is extended, not replaced"
    );
}

#[test]
fn a_policy_whose_walk_cap_exceeds_the_anchor_period_says_so() {
    let policy = graph::GraphPolicy {
        chain_full_every: 16,
        max_chain_walk: 64,
        ..graph::GraphPolicy::default()
    };
    assert!(
        policy.notices().iter().any(|n| n.contains("a lost anchor")),
        "the tripwire would have been switched off, and that is worth printing"
    );
}

/// A naming view over a build's dictionary, as the store would hand it back.
fn naming_of(outcome: &graph::BuildOutcome) -> graph::Naming {
    let layers = builder().layers;
    graph::Naming::from_vertices(outcome.modules.values().flat_map(|module| {
        let layer = layers.layer_of(&module.path);
        std::iter::once(graph::Vertex {
            id: module.node,
            key: module.path.clone(),
            kind: graph::RowKind::Module,
            incomplete: module.incomplete.is_some(),
            layer: layer.clone(),
        })
        .chain(module.symbols.iter().map(move |(id, key)| graph::Vertex {
            id: *id,
            key: key.clone(),
            kind: graph::RowKind::Symbol,
            incomplete: false,
            layer: layer.clone(),
        }))
    }))
}

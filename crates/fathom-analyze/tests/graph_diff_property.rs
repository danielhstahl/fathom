//! The third acceptance criterion, proved rather than sampled (`e3.3`, `reco-8wg`).
//!
//! *"Delta between two non-adjacent episodes matches the composition of the intermediate deltas."*
//! A fixture asserts that for one shape of history. The claim is about **every** shape, and the
//! shapes that break it are the ones nobody thinks to write: a symbol that leaves a module and comes
//! back three revisions later; a module deleted and re-added under the same path; a dependency
//! removed in one revision and re-added with a different kind in the next. So this file says the
//! property three ways:
//!
//! * **by enumeration** over every pair and every split point of small hand-built histories whose
//!   shapes were chosen to be the awkward ones;
//! * **by `proptest`**, over random histories of random shape, with every revision compared to
//!   every later revision, composed at every intermediate cut;
//! * **at the document level**, where the derived findings live — the composition must agree with
//!   the direct diff not merely about how many rows moved but about which ones were moves, which
//!   were re-points, which doors changed and which modules got reworked.
//!
//! The second and third are the same claim at two levels on purpose. [`compose`] cancels a row that
//! came back inside the span, which is the only reason the claim can be true at all; the document is
//! a function of the composed rows, so if the rows cancel one way and the document is derived
//! another, the property catches it at the level a reviewer reads.
//!
//! No store: the property is about the delta algebra, and ADR-002 §6.1 wants that algebra to compile
//! and hold with no SQLite linked in. The store's rungs are pinned in `graph_diff_lane.rs`, which
//! asserts the same equality against a *stored* chain.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::graph::{
    self, ArchitectureDelta, Builder, GraphPolicy, Interner, ModuleInput, Naming, ops_between,
};
use fathom_analyze::structure::{
    Complexity, Edge, EdgeKind, FileLevel, FileStructure, Granularity, Language, Resolution, Span,
    Structure, Symbol, SymbolKind, Visibility,
};
use proptest::prelude::*;

/// The paths a random history draws its modules from. Small on purpose: the interesting cases are
/// the ones where the same symbol name appears in three modules at once.
const PATHS: &[&str] = &[
    "core/a.rs",
    "core/b.rs",
    "core/c.rs",
    "app/main.rs",
    "app/cli.rs",
];

/// The symbol names in play, so a name can leave one module and arrive in another — or come back.
const NAMES: &[&str] = &["Alpha", "Beta", "Gamma", "Shared"];

/// The instant a revision is stamped with; unused by the delta, which reads no clock.
fn builder() -> Builder {
    Builder {
        policy: GraphPolicy::default(),
        layers: graph::Layers::new([(String::from("core"), String::from("l0"))]),
    }
}

/// One revision as a list of `(path, exports, deps)` triples — the whole input a build needs.
type Files = Vec<(String, Vec<String>, Vec<(String, String)>)>;

/// The structural record for one `(path, exports, deps)` triple.
fn structure_of(entry: &(String, Vec<String>, Vec<(String, String)>)) -> FileStructure {
    let (path, exports, deps) = entry;
    let span = Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    FileStructure {
        path: path.clone(),
        language: Language::Rust,
        file_level: FileLevel {
            path: path.clone(),
            language: Language::Rust,
            bytes: 10,
            newlines: 1,
            blank_lines: 0,
            code_lines: 1,
            parse_errors: 0,
            refused: false,
        },
        structure: Some(Structure {
            symbols: exports
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
            exports: exports.clone(),
            imports: Vec::new(),
            edges: deps
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
                    granularity: Granularity::File,
                    package: None,
                })
                .collect(),
            supertypes: Vec::new(),
            synthetic_scopes: Vec::new(),
            notes: Vec::new(),
            resolution: Resolution {
                passes_used: 1,
                converged: true,
                bound: 4,
                truncated_chains: 0,
            },
            package: None,
        }),
        reason: None,
    }
}

/// Build every revision of a history through one dictionary, as one repository has one dictionary.
fn build_all(history: &[Files]) -> Vec<fathom_analyze::graph::BuildOutcome> {
    let mut interner = Interner::default();
    history
        .iter()
        .map(|files| {
            let structures: Vec<FileStructure> = files.iter().map(structure_of).collect();
            let inputs: Vec<ModuleInput<'_>> = files
                .iter()
                .zip(structures.iter())
                .map(|(entry, file)| ModuleInput {
                    path: entry.0.as_str(),
                    // The blob is the content: "the blob moved" and "the file changed" are the same
                    // event, which is what a content address is for.
                    blob: entry.0.as_str(),
                    file: Some(file),
                })
                .collect();
            builder().build(&inputs, &mut interner)
        })
        .collect()
}

/// Every revision's dictionary in one view, so a delta can name a row that only one of them has.
fn view_of(outcomes: &[fathom_analyze::graph::BuildOutcome]) -> Naming {
    outcomes.iter().fold(Naming::default(), |view, outcome| {
        Naming::merged(&view, &outcome.naming())
    })
}

/// The property, stated once and checked at both levels.
///
/// `from` < `through` < `to`: composing the delta as far as `through` with the delta from there must
/// give the delta all the way, *and* the document derived from the composition must be the document
/// derived from the endpoints.
fn assert_composes(outcomes: &[fathom_analyze::graph::BuildOutcome], view: &Naming) {
    for from in 0..outcomes.len() {
        for to in from + 1..outcomes.len() {
            let direct = ops_between(&outcomes[from].graph, &outcomes[to].graph, view);
            // Every cut point: the composition must be independent of where the span is split, which
            // is what "composed from the intermediate deltas" means when there is more than one.
            for through in from + 1..to {
                let first = ops_between(&outcomes[from].graph, &outcomes[through].graph, view);
                let second = ops_between(&outcomes[through].graph, &outcomes[to].graph, view);
                let composed = graph::compose([&first[..], &second[..]]);
                assert_eq!(
                    composed, direct,
                    "splitting {}→{} at {through} composed to a different set of rows",
                    outcomes[from].summary.nodes, outcomes[to].summary.nodes
                );
            }
            let document = ArchitectureDelta::of(
                &format!("r{from}"),
                &format!("r{to}"),
                &direct,
                view,
                Some(&outcomes[from].graph),
                Some(&outcomes[to].graph),
                graph::Cost::chain(direct.len(), to - from),
            );
            let endpoint = ArchitectureDelta::between(
                &outcomes[from].graph,
                &outcomes[to].graph,
                view,
                &format!("r{from}"),
                &format!("r{to}"),
            );
            // The two documents must agree about every finding. Cost and notices are excluded: they
            // describe how the answer was obtained, which is the one thing allowed to differ.
            let mut from_composition = document.clone();
            let mut from_endpoints = endpoint.clone();
            from_composition.cost = graph::Cost::in_hand();
            from_endpoints.cost = graph::Cost::in_hand();
            from_composition.notices.clear();
            from_endpoints.notices.clear();
            assert_eq!(
                from_composition, from_endpoints,
                "the composed document disagrees with the endpoint document for {}→{}",
                from, to
            );
            assert_eq!(
                document.summary_lines(),
                endpoint.summary_lines(),
                "and they must not be two different cards either"
            );
        }
    }
}

/// One draw from a seeded, position-mixed generator.
///
/// No hidden state: every draw is a function of the seed and its position, so a failing case is
/// reproducible from the seed `proptest` prints and a re-run of the same seed builds the same
/// history byte for byte.
fn draw(seed: u64, revision: usize, slot: usize, pick: usize, modulus: usize) -> usize {
    let mut x = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(
            u64::try_from(revision)
                .unwrap_or(0)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15),
        )
        .wrapping_add(
            u64::try_from(slot)
                .unwrap_or(0)
                .wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
        )
        .wrapping_add(
            u64::try_from(pick)
                .unwrap_or(0)
                .wrapping_mul(0x1656_67B1_9E37_79F9),
        );
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    usize::try_from(x % u64::try_from(modulus.max(1)).unwrap_or(1)).unwrap_or(0)
}

/// Give each revision of a bare path list its exports and dependencies.
fn season(history: Vec<Files>, seed: u64) -> Vec<Files> {
    history
        .into_iter()
        .enumerate()
        .map(|(revision, entries)| {
            entries
                .into_iter()
                .enumerate()
                .map(|(slot, entry)| {
                    let path = entry.0;
                    let exports: Vec<String> = NAMES
                        .iter()
                        .enumerate()
                        .filter(|(pick, _)| draw(seed, revision, slot, *pick, 2) == 0)
                        .map(|(_, name)| (*name).to_string())
                        .collect();
                    let deps: Vec<(String, String)> = (0..draw(seed, revision, slot, 0, 3))
                        .map(|pick| {
                            let target = PATHS[draw(seed, revision, slot, pick + 1, PATHS.len())]
                                .to_string();
                            let symbol = if draw(seed, revision, slot, pick + 7, 4) == 0 {
                                String::from("*")
                            } else {
                                NAMES[draw(seed, revision, slot, pick + 3, NAMES.len())].to_string()
                            };
                            (target, symbol)
                        })
                        .filter(|(target, _)| *target != path)
                        .collect();
                    (path, exports, deps)
                })
                .collect()
        })
        .collect()
}

/// The strategy: a history of 2..6 revisions, each a subset of the paths with a subset of exports
/// and dependencies, the same paths recurring so that names move between modules.
fn history_strategy() -> impl Strategy<Value = Vec<Files>> {
    let bare = prop::collection::btree_set(prop::sample::select(PATHS), 1..PATHS.len()).prop_map(
        |paths| {
            paths
                .iter()
                .map(|path| (path.to_string(), Vec::new(), Vec::new()))
                .collect::<Files>()
        },
    );
    (prop::collection::vec(bare, 2..6), any::<u64>())
        .prop_map(|(history, seed)| season(history, seed))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    #[test]
    fn a_span_is_the_composition_of_its_parts(history in history_strategy()) {
        let outcomes = build_all(&history);
        let view = view_of(&outcomes);
        // A strategy that generated identical revisions would prove the property about nothing, so
        // the strategy is checked to be doing something before the property is.
        //
        // The check *rejects*; it does not fail. Whether a given draw happens to move anything is a fact
        // about the generator, not a counterexample to the property: roughly one draw in a few thousand
        // the seeds land so that every revision of a one-module history carries the same exports and the
        // same edges, and then every span in it is empty at every cut and the equality holds vacuously.
        // That is the input saying "this one does not exercise me", which is `assume`'s whole job; as an
        // `assert` it was a red run one unlucky seed away, on every machine that runs this more than a
        // couple of times. And the guard is not lost: a strategy that stopped moving *altogether* trips
        // `max_rejects`, which fails the test and blames the generator, which is what to blame.
        let moved = (1..outcomes.len())
            .any(|index| !ops_between(&outcomes[index - 1].graph, &outcomes[index].graph, &view).is_empty());
        prop_assume!(
            moved,
            "the generated history changed nothing at all, so the composition was never exercised"
        );
        assert_composes(&outcomes, &view);
    }
}

#[test]
fn a_symbol_that_leaves_and_comes_back_changes_nothing_over_the_span() {
    // The case a single fixture would miss: every intermediate revision differs from its neighbour,
    // and the endpoints are identical, so the composition has to cancel to nothing everywhere.
    let history: Vec<Files> = vec![
        vec![(
            "core/a.rs".to_string(),
            vec!["Alpha".to_string()],
            Vec::new(),
        )],
        vec![("core/a.rs".to_string(), Vec::new(), Vec::new())],
        vec![(
            "core/a.rs".to_string(),
            vec!["Alpha".to_string()],
            Vec::new(),
        )],
    ];
    let outcomes = build_all(&history);
    let view = view_of(&outcomes);
    assert_composes(&outcomes, &view);
    let whole = ops_between(&outcomes[0].graph, &outcomes[2].graph, &view);
    assert!(
        whole.is_empty(),
        "the endpoints are the same graph: {whole:?}"
    );
}

#[test]
fn a_dependency_removed_and_re_added_as_a_different_kind_is_a_re_point_at_the_end() {
    // Named, then unknown, then named again: three revisions whose intermediate deltas are both
    // re-points, and whose span is empty.
    let named = (
        "core/a.rs".to_string(),
        vec!["Alpha".to_string()],
        vec![("core/b.rs".to_string(), "Beta".to_string())],
    );
    let unknown = (
        "core/a.rs".to_string(),
        vec!["Alpha".to_string()],
        vec![("core/b.rs".to_string(), "*".to_string())],
    );
    let b = (
        "core/b.rs".to_string(),
        vec!["Beta".to_string()],
        Vec::new(),
    );
    let history = vec![
        vec![named.clone(), b.clone()],
        vec![unknown.clone(), b.clone()],
        vec![named, b],
    ];
    let outcomes = build_all(&history);
    let view = view_of(&outcomes);
    assert_composes(&outcomes, &view);
    let one_step =
        ArchitectureDelta::between(&outcomes[0].graph, &outcomes[1].graph, &view, "r0", "r1");
    assert_eq!(one_step.repointed.len(), 1, "{:?}", one_step.repointed);
}

#[test]
fn a_module_deleted_and_reborn_under_the_same_path_composes() {
    // The path is the identity: `core/a.rs` disappears and comes back with different exports, so
    // the composition is a surface change and nothing else.
    let history: Vec<Files> = vec![
        vec![(
            "core/a.rs".to_string(),
            vec!["Alpha".to_string()],
            Vec::new(),
        )],
        vec![],
        vec![
            (
                "core/a.rs".to_string(),
                vec!["Beta".to_string()],
                Vec::new(),
            ),
            (
                "core/b.rs".to_string(),
                vec!["Gamma".to_string()],
                vec![("core/a.rs".to_string(), "Beta".to_string())],
            ),
        ],
    ];
    let outcomes = build_all(&history);
    let view = view_of(&outcomes);
    assert_composes(&outcomes, &view);
    let whole =
        ArchitectureDelta::between(&outcomes[0].graph, &outcomes[2].graph, &view, "r0", "r2");
    assert!(
        !whole.surface.is_empty() || !whole.edges_added.is_empty(),
        "the span did do something: {:?}",
        whole.counts()
    );
}

#[test]
fn a_history_of_empty_revisions_composes_to_nothing() {
    // The degenerate history — nothing but identical revisions — is the one an implementation with a
    // "skip if empty" shortcut in the wrong place gets wrong.
    let file = (
        "core/a.rs".to_string(),
        vec!["Alpha".to_string()],
        Vec::new(),
    );
    let history: Vec<Files> = vec![vec![file.clone()], vec![file.clone()], vec![file]];
    let outcomes = build_all(&history);
    let view = view_of(&outcomes);
    assert_composes(&outcomes, &view);
    for outcome in &outcomes {
        let graph: &fathom_analyze::graph::Graph = &outcome.graph;
        assert!(
            graph.node_count() > 0,
            "the graph is not empty, only unchanging"
        );
    }
}

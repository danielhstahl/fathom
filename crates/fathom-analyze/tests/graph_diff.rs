//! The architecture delta, without a store: the four fixtures the ticket names, the two claims about
//! what a delta is allowed to say, and the algebra the composition property rests on
//! (`e3.3`, `reco-8wg`).
//!
//! The store-backed half — which rung answered, what it cost, and that every rung agrees — is in
//! `graph_diff_lane.rs`. What is asserted here is the part that must hold with no SQLite linked in
//! at all (ADR-002 §6.1): the four shapes produce the four expected answers, a move is one fact
//! rather than two, `changed` decomposes into the four things it really is, the churn bands separate
//! a module that keeps coming back from a module that changed once, and the JSON form is a function
//! of the facts rather than of the order the rows arrived in.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::graph::{
    self, ArchitectureDelta, Band, BuildOutcome, Builder, ChurnReport, Cost, Interner, ModuleInput,
    MoveShape, Naming, RowKind, Rung, compose,
};
use fathom_analyze::structure::{
    Complexity, Edge, EdgeKind, FileLevel, FileStructure, Granularity, Language, Resolution,
    Supertype, SyntheticScope, Visibility,
};

/// A fixture module: a path, its exports, and its dependencies as `(target, symbol)` pairs.
#[derive(Debug, Clone)]
struct Fixture {
    path: String,
    exports: Vec<String>,
    deps: Vec<(String, String)>,
    /// `(implementing symbol, base as written, resolved base module)` triples — the `implements`
    /// edge, which is a dependency kind and so survives as an arrow in its own right. The base is
    /// kept in both the forms an extractor produces: the name as written, which is what a graph
    /// row's destination is built from, and the module it resolved to.
    bases: Vec<(String, String, String)>,
    synthetic: bool,
}

fn fixture(path: &str, exports: &[&str], deps: &[(&str, &str)]) -> Fixture {
    Fixture {
        path: path.to_string(),
        exports: exports.iter().map(|name| (*name).to_string()).collect(),
        deps: deps
            .iter()
            .map(|(target, symbol)| (target.to_string(), symbol.to_string()))
            .collect(),
        bases: Vec::new(),
        synthetic: false,
    }
}

/// The same fixture, with dependencies already owned.
fn fixture_with(path: &str, exports: &[&str], deps: Vec<(String, String)>) -> Fixture {
    Fixture {
        path: path.to_string(),
        exports: exports.iter().map(|name| (*name).to_string()).collect(),
        deps,
        bases: Vec::new(),
        synthetic: false,
    }
}

/// A fixture whose first export implements a base declared in another module.
fn implements(
    path: &str,
    exports: &[&str],
    deps: &[(&str, &str)],
    base: &str,
    resolved: &str,
) -> Fixture {
    let mut out = fixture(path, exports, deps);
    if let Some(first) = exports.first() {
        out.bases = vec![(first.to_string(), base.to_string(), resolved.to_string())];
    }
    out
}

impl Fixture {
    /// The blob of a *different file with the same structure*: the shape a commit that touched only
    /// the body takes, and the one a graph delta must not react to.
    fn reworded(&self) -> String {
        format!("{};reworded", self.blob())
    }

    fn blob(&self) -> String {
        let mut parts: Vec<String> = self.exports.clone();
        parts.extend(self.deps.iter().map(|(t, s)| format!("{t}:{s}")));
        parts.extend(
            self.bases
                .iter()
                .map(|(of, base, resolved)| format!("{of}<{base}@{resolved}")),
        );
        if self.synthetic {
            parts.push(String::from("synthetic"));
        }
        format!("blob:{}", parts.join(","))
    }
}

/// The structural record a fixture would have produced, in the shape `e3.1` hands the graph builder.
fn structure_of(fixture: &Fixture) -> FileStructure {
    let span = fathom_analyze::structure::Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    let structure = fathom_analyze::structure::Structure {
        symbols: fixture
            .exports
            .iter()
            .map(|name| fathom_analyze::structure::Symbol {
                kind: fathom_analyze::structure::SymbolKind::Fn,
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
                granularity: Granularity::File,
                package: None,
            })
            .collect(),
        supertypes: fixture
            .bases
            .iter()
            .map(|(of, base, resolved)| Supertype {
                of: of.clone(),
                base: base.clone(),
                kind: fathom_analyze::structure::SupertypeKind::Interface,
                resolved: Some(resolved.clone()),
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

fn builder() -> Builder {
    Builder {
        policy: graph::GraphPolicy::default(),
        layers: graph::Layers::new([
            (String::from("core"), String::from("l0")),
            (String::from("app"), String::from("l2")),
        ]),
    }
}

/// One revision, held as the three parallel lists the substrate hands over.
#[derive(Debug, Clone)]
struct Revision {
    files: Vec<Fixture>,
    structures: Vec<FileStructure>,
    blobs: Vec<String>,
}

impl Revision {
    fn new(files: Vec<Fixture>) -> Self {
        let structures = files.iter().map(structure_of).collect();
        let blobs = files.iter().map(Fixture::blob).collect();
        Self {
            files,
            structures,
            blobs,
        }
    }

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

    /// Replace one module the way a commit would.
    fn changed(mut self, path: &str, f: impl FnOnce(Fixture) -> Fixture) -> Self {
        if let Some(index) = self.files.iter().position(|slot| slot.path == path) {
            let fresh = f(self.files[index].clone());
            self.files[index] = fresh;
        }
        self.rebuild()
    }

    fn minus(mut self, path: &str) -> Self {
        self.files.retain(|file| file.path != path);
        self.rebuild()
    }

    fn plus(mut self, extra: Fixture) -> Self {
        self.files.push(extra);
        self.rebuild()
    }

    /// Touch a file's *content* without touching anything the graph can see.
    fn reworded(mut self, path: &str) -> Self {
        for (index, file) in self.files.iter().enumerate() {
            if file.path == path {
                self.blobs[index] = file.reworded();
            }
        }
        self
    }

    fn rebuild(mut self) -> Self {
        self.structures = self.files.iter().map(structure_of).collect();
        self.blobs = self.files.iter().map(Fixture::blob).collect();
        self
    }
}

/// Build a revision against a shared dictionary, as the store keeps one dictionary per repository.
fn build(interner: &mut Interner, revision: &Revision) -> BuildOutcome {
    builder().build(&revision.inputs(), interner)
}

/// Several revisions' dictionaries in one.
///
/// The store keeps *one* dictionary for a repository, so a delta is read against every node the
/// repository ever interned, not just the two revisions being compared. A test that handed a delta
/// only the two revisions it was diffing would get `#unnamed` ids for anything that had come and
/// gone in between — which is a real state (`graph_node` is class R, droppable and re-derivable) but
/// not the one under test.
fn view_of(outcomes: &[&BuildOutcome]) -> Naming {
    outcomes.iter().fold(Naming::default(), |view, outcome| {
        Naming::merged(&view, &outcome.naming())
    })
}

/// The delta between two revisions, with both graphs in hand — the complete answer.
fn delta(before: &BuildOutcome, after: &BuildOutcome, from: &str, to: &str) -> ArchitectureDelta {
    let view = view_of(&[before, after]);
    ArchitectureDelta::between(&before.graph, &after.graph, &view, from, to)
}

fn has_node(list: &[graph::NodeFact], key: &str) -> bool {
    list.iter().any(|node| node.key == key)
}

fn has_edge(list: &[graph::EdgeFact], kind: RowKind, src: &str, dst: &str) -> bool {
    list.iter()
        .any(|edge| edge.kind == kind.as_str() && edge.src == src && edge.dst == dst)
}

fn surface_of<'a>(delta: &'a ArchitectureDelta, module: &str) -> Option<&'a graph::SurfaceChange> {
    delta.surface.iter().find(|change| change.module == module)
}

// ---------------------------------------------------------------------------------------------
// The four fixtures the ticket names.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_moved_symbol_is_one_move_and_not_a_deletion_plus_an_addition() {
    // The acceptance criterion, in its simplest shape: `Shape` leaves `core/shape.rs` for
    // `core/geo.rs`, and a third module that used it follows it.
    let mut interner = Interner::default();
    let before = build(
        &mut interner,
        &Revision::new(vec![
            fixture("core/shape.rs", &["Shape"], &[]),
            fixture("app/main.rs", &["main"], &[("core/shape.rs", "Shape")]),
        ]),
    );
    let after = build(
        &mut interner,
        &Revision::new(vec![
            fixture("core/shape.rs", &[], &[]),
            fixture("core/geo.rs", &["Shape"], &[]),
            fixture("app/main.rs", &["main"], &[("core/geo.rs", "Shape")]),
        ]),
    );
    let answer = delta(&before, &after, "r1", "r2");

    assert_eq!(answer.moves.len(), 1, "one event, not two rows");
    let mv = answer.moves.first().expect("one move");
    assert_eq!(mv.name, "Shape");
    assert_eq!(mv.shape, MoveShape::Moved);
    assert!(mv.confident, "one module out and one in is derivable");
    assert_eq!(mv.out, vec![String::from("core/shape.rs")]);
    assert_eq!(mv.into, vec![String::from("core/geo.rs")]);
    assert_eq!(
        mv.retargets, 1,
        "app/main.rs's `uses core/shape.rs::Shape` became `uses core/geo.rs::Shape`: the move's \
         invoice, not main.rs changing its mind"
    );

    // The two halves are not *also* a deletion and an addition: a reader who counted both would
    // report a net loss of a symbol that did not happen.
    assert!(
        !has_node(&answer.nodes_removed, "core/shape.rs::Shape")
            && !has_node(&answer.nodes_added, "core/geo.rs::Shape"),
        "the moved symbol's node rows belong to the move: {:?}",
        answer.nodes_removed
    );
    assert!(
        !has_edge(
            &answer.edges_removed,
            RowKind::Uses,
            "app/main.rs",
            "core/shape.rs::Shape"
        ) && !has_edge(
            &answer.edges_added,
            RowKind::Uses,
            "app/main.rs",
            "core/geo.rs::Shape"
        ),
        "and so do the dependents' re-pointed uses rows: {:?}",
        answer.edges_added
    );
    // The `imports` edge moved with the dependency, and it is still charged to main.rs: a module
    // that changed which file it leans on really did change its own wiring.
    assert!(
        has_edge(
            &answer.edges_removed,
            RowKind::Imports,
            "app/main.rs",
            "core/shape.rs"
        ) && has_edge(
            &answer.edges_added,
            RowKind::Imports,
            "app/main.rs",
            "core/geo.rs"
        ),
        "{:?}",
        answer.counts()
    );

    // Both doors changed, and the report says so on both sides.
    let lost = surface_of(&answer, "core/shape.rs").expect("the door that closed");
    assert_eq!(lost.removed, vec![String::from("Shape")]);
    let gained = surface_of(&answer, "core/geo.rs").expect("the door that opened");
    assert_eq!(gained.added, vec![String::from("Shape")]);
    assert!(gained.complete && lost.complete);

    // Churn bills the move to the modules it moved between, and to nobody else.
    assert!(
        answer
            .churn
            .iter()
            .any(|row| row.module == "app/main.rs" && row.band != Band::Stable),
        "the dependent that got re-pointed is part of the change: {:?}",
        answer.churn
    );
    assert!(
        answer.sentence().contains("symbols moved"),
        "{}",
        answer.sentence()
    );
}

#[test]
fn a_split_module_is_one_split_and_not_a_set_of_renames() {
    // `e2.4`'s lesson, applied to the graph: a 1→N move is a grouping. `legacy.rs` is cut into
    // `left.rs` and `right.rs`, and `Shared` ends up declared by both halves.
    let mut interner = Interner::default();
    let before = build(
        &mut interner,
        &Revision::new(vec![fixture(
            "core/legacy.rs",
            &["Alpha", "Beta", "Shared"],
            &[],
        )]),
    );
    let after = build(
        &mut interner,
        &Revision::new(vec![
            fixture("core/left.rs", &["Alpha", "Shared"], &[]),
            fixture("core/right.rs", &["Beta", "Shared"], &[]),
        ]),
    );
    let answer = delta(&before, &after, "r1", "r2");

    let shapes: Vec<(&str, MoveShape)> = answer
        .moves
        .iter()
        .map(|mv| (mv.name.as_str(), mv.shape))
        .collect();
    assert!(
        shapes.contains(&("Alpha", MoveShape::Moved)),
        "a symbol that went one place is a move: {shapes:?}"
    );
    let split = answer
        .moves
        .iter()
        .find(|mv| mv.name == "Shared")
        .expect("the symbol that went two places");
    assert_eq!(split.shape, MoveShape::Split);
    assert!(split.confident, "one module out and two in is derivable");
    assert_eq!(split.into.len(), 2);
    assert_eq!(
        answer
            .moves
            .iter()
            .filter(|mv| mv.shape == MoveShape::Split)
            .count(),
        1,
        "one split, not two moves dressed up as one fact each"
    );
    // The whole legacy module went, and the delta says which module closed.
    assert!(
        has_node(&answer.nodes_removed, "core/legacy.rs"),
        "{:?}",
        answer.nodes_removed
    );
    assert_eq!(
        surface_of(&answer, "core/legacy.rs")
            .expect("the closed module")
            .removed
            .len(),
        3,
        "all three exports left that file"
    );
}

#[test]
fn a_dependency_inversion_is_one_event_and_says_whether_both_arrows_survive() {
    // `app/main.rs` leaned on `state/store.rs`; now the store leans on the app layer. The delta
    // calls that one fact, not two unrelated pieces of churn.
    let mut interner = Interner::default();
    let before = build(
        &mut interner,
        &Revision::new(vec![
            fixture("app/main.rs", &["main"], &[("state/store.rs", "Store")]),
            fixture("state/store.rs", &["Store"], &[]),
        ]),
    );
    let after = build(
        &mut interner,
        &Revision::new(vec![
            fixture("app/main.rs", &["main"], &[]),
            fixture("state/store.rs", &["Store"], &[("app/main.rs", "main")]),
        ]),
    );
    let answer = delta(&before, &after, "r1", "r2");
    assert_eq!(answer.reversals.len(), 1, "{:?}", answer.reversals);
    let reversal = answer.reversals.first().expect("one inversion");
    assert_eq!(reversal.a, "app/main.rs");
    assert_eq!(reversal.b, "state/store.rs");
    assert_eq!(
        reversal.mutual,
        Some(false),
        "the old arrow is really gone, which is what makes it an inversion rather than a cycle"
    );

    // The same turn, but `app/main.rs` keeps a contract on the store: after the turn, both arrows
    // exist, and the answer says so instead of leaving a reader to rebuild the graph to find out.
    let cycled = build(
        &mut interner,
        &Revision::new(vec![
            implements("app/main.rs", &["main"], &[], "Store", "state/store.rs"),
            fixture("state/store.rs", &["Store"], &[("app/main.rs", "main")]),
        ]),
    );
    let answer = delta(&before, &cycled, "r1", "r3");
    assert_eq!(
        answer
            .reversals
            .iter()
            .map(|reversal| reversal.mutual)
            .collect::<Vec<_>>(),
        vec![Some(true)],
        "{:?}",
        answer.reversals
    );
    assert!(
        answer
            .summary_lines()
            .iter()
            .any(|line| line.contains("both arrows still exist")),
        "{:?}",
        answer.summary_lines()
    );

    // And a revision that merely *adds* the other arrow is not an inversion at all: nothing turned
    // around, a cycle just appeared, which is `e3.4`'s finding and not this one.
    let added = build(
        &mut interner,
        &Revision::new(vec![
            fixture("app/main.rs", &["main"], &[("state/store.rs", "Store")]),
            fixture("state/store.rs", &["Store"], &[("app/main.rs", "main")]),
        ]),
    );
    let answer = delta(&before, &added, "r1", "r4");
    assert!(
        answer.reversals.is_empty(),
        "no arrow was removed, so nothing was inverted: {:?}",
        answer.reversals
    );
}

#[test]
fn a_dead_edge_removal_is_told_apart_from_a_dependency_someone_dropped() {
    // Revision 2 deletes `core/gone.rs` while `core/keeper.rs` still names it: the edge is left
    // pointing at nothing, which is a stated state of the graph, not an error. Revision 3 removes
    // that edge and, separately, drops a live dependency on a module that is still there. The two
    // removals mean opposite things and the delta marks them oppositely.
    let mut interner = Interner::default();
    let rev1 = Revision::new(vec![
        fixture(
            "core/keeper.rs",
            &["Keeper"],
            &[("core/gone.rs", "Gone"), ("core/stay.rs", "Stay")],
        ),
        fixture("core/gone.rs", &["Gone"], &[]),
        fixture("core/stay.rs", &["Stay"], &[]),
    ]);
    // Revision 2 is a `git rm` and nothing else: keeper still names the file that is no longer there.
    let rev2 = rev1.clone().minus("core/gone.rs");
    // Revision 3 takes both of keeper's dependencies away. One of them points at a module that is
    // already gone — a tidy-up — and the other at one that is still there, which is a decision.
    let rev3 = rev2.clone().changed("core/keeper.rs", |_| {
        fixture("core/keeper.rs", &["Keeper"], &[])
    });
    let one = build(&mut interner, &rev1);
    let two = build(&mut interner, &rev2);
    let three = build(&mut interner, &rev3);

    // The dead edge is *not* the removal in revision 2 — it is the edge revision 2 leaves behind.
    let second = ArchitectureDelta::between(
        &one.graph,
        &two.graph,
        &view_of(&[&one, &two, &three]),
        "r1",
        "r2",
    );
    assert!(
        has_node(&second.nodes_removed, "core/gone.rs"),
        "the module went: {:?}",
        second.nodes_removed
    );
    assert!(
        !second
            .edges_removed
            .iter()
            .any(|edge| edge.dst == "core/gone.rs"),
        "and the edge that named it stayed, which is what makes it dead: {:?}",
        second.edges_removed
    );

    let third = ArchitectureDelta::between(
        &two.graph,
        &three.graph,
        &view_of(&[&one, &two, &three]),
        "r2",
        "r3",
    );
    let dead = third
        .edges_removed
        .iter()
        .find(|edge| edge.dst == "core/gone.rs")
        .expect("the dead edge was cleaned up");
    assert_eq!(dead.dead, Some(true), "the target was already gone");
    assert_eq!(
        dead.why, "dropped",
        "nothing deleted the target in this revision"
    );
    let live = third
        .edges_removed
        .iter()
        .find(|edge| edge.dst == "core/stay.rs")
        .expect("and a live dependency went too");
    assert_eq!(live.dead, Some(false), "that one was a decision");
    assert_ne!(dead.why, live.why, "and the delta says which is which");
}

// ---------------------------------------------------------------------------------------------
// The other things that are not `added` or `removed`.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_dependency_that_became_a_star_import_is_a_re_point_not_a_removal() {
    // The dependency count did not change and the certainty did. Reading this as one removal and one
    // addition is how a graph says "the architecture got simpler" when it got vaguer.
    let mut interner = Interner::default();
    let before = build(
        &mut interner,
        &Revision::new(vec![
            fixture("app/main.rs", &["main"], &[("core/lib.rs", "Thing")]),
            fixture("core/lib.rs", &["Thing"], &[]),
        ]),
    );
    let after = build(
        &mut interner,
        &Revision::new(vec![
            fixture("app/main.rs", &["main"], &[("core/lib.rs", "*")]),
            fixture("core/lib.rs", &["Thing"], &[]),
        ]),
    );
    let answer = delta(&before, &after, "r1", "r2");
    assert_eq!(answer.repointed.len(), 1, "{:?}", answer.repointed);
    let repoint = answer.repointed.first().expect("one re-point");
    assert_eq!(repoint.from_kind, RowKind::Imports.as_str());
    assert_eq!(repoint.to_kind, RowKind::Wildcard.as_str());
    assert!(
        repoint.meaning.contains("less precisely"),
        "the sentence has to name the direction of the change: {}",
        repoint.meaning
    );
    assert!(
        !answer
            .edges_added
            .iter()
            .any(|edge| edge.kind == RowKind::Wildcard.as_str()),
        "the wildcard row is inside the re-point, not counted as a new dependency"
    );
    // The symbol-level edge has no counterpart on the other side, so it stays visible as a removal:
    // the named unit really is no longer named.
    assert!(
        answer
            .edges_removed
            .iter()
            .any(|edge| edge.kind == RowKind::Uses.as_str()),
        "{:?}",
        answer.edges_removed
    );
}

#[test]
fn a_new_export_is_a_surface_change_and_a_name_nobody_declared_is_dangling() {
    let mut interner = Interner::default();
    let before = build(
        &mut interner,
        &Revision::new(vec![fixture("app/main.rs", &["main"], &[])]),
    );
    let after = build(
        &mut interner,
        &Revision::new(vec![fixture(
            "app/main.rs",
            &["main"],
            &[("ghost.rs", "Ghost")],
        )])
        .plus(fixture("core/lib.rs", &["Thing", "Extra"], &[])),
    );
    let answer = delta(&before, &after, "r1", "r2");
    let lib = surface_of(&answer, "core/lib.rs").expect("a brand-new file has a surface");
    assert_eq!(lib.added, vec!["Extra", "Thing"]);
    assert!(lib.removed.is_empty());
    // Two different "not there" facts, and the answer keeps them apart. `ghost.rs` was never a
    // module in any revision of this test, so the dictionary built from those revisions has no name
    // for its id at all: the rows that name it are reported under the only name there is —
    // `#unnamed:<id>` — and marked `unnamed`, because a delta that quietly dropped them would be a
    // smaller and wronger answer. An edge whose *target* the revision declares nowhere is
    // `dangling`, which is a state of the graph rather than an error in it.
    let ghost: Vec<&graph::EdgeFact> = answer
        .edges_added
        .iter()
        .filter(|edge| edge.unnamed)
        .collect();
    assert!(
        !ghost.is_empty() && ghost.iter().all(|edge| edge.dangling == Some(true)),
        "a dependency on a name nobody declares is a stated state, not a dropped row: {:?}",
        answer.edges_added
    );
    assert!(
        ghost.iter().all(|edge| edge.dst.starts_with("#unnamed:")),
        "and the name it prints is the honest one, not a guess: {ghost:?}"
    );
    assert!(
        !answer.to_json().contains("ghost"),
        "the dictionary is never handed a name it never minted: {}",
        answer.to_json()
    );
    assert!(
        ghost.iter().all(|edge| edge.unnamed),
        "and this dictionary has never seen that id, which the row says out loud: {ghost:?}"
    );
    assert!(
        answer
            .notices
            .iter()
            .any(|notice| notice.contains("cannot spell")),
        "{:?}",
        answer.notices
    );
}

#[test]
fn an_export_list_that_may_have_shrunk_is_not_reported_as_a_shrunk_one() {
    // `s1` finding 1, at the door: a macro-heavy module losing an export may be the extractor
    // losing sight of it, and a surface report that says `removed` without that flag is a lie.
    let mut interner = Interner::default();
    let mut macro_heavy = fixture("core/dsl.rs", &["One", "Two"], &[]);
    macro_heavy.synthetic = true;
    let before = build(&mut interner, &Revision::new(vec![macro_heavy.clone()]));
    let after = build(
        &mut interner,
        &Revision::new(vec![fixture("core/dsl.rs", &["One"], &[])]),
    );
    let answer = delta(&before, &after, "r1", "r2");
    let surface = surface_of(&answer, "core/dsl.rs").expect("one door moved");
    assert_eq!(surface.removed, vec![String::from("Two")]);
    assert!(
        surface.may_have_shrunk && !surface.complete,
        "the flag travels with the finding: {surface:?}"
    );
}

#[test]
fn a_move_that_cannot_be_paired_stays_a_candidate_and_keeps_both_rows() {
    // Two modules lose `parse`, two gain it. Any pairing this revision offers is a guess, and a
    // guess shown as a fact is the failure mode `e2.4` already named for renames.
    let mut interner = Interner::default();
    let before = build(
        &mut interner,
        &Revision::new(vec![
            fixture("a.rs", &["parse"], &[]),
            fixture("b.rs", &["parse"], &[]),
        ]),
    );
    let after = build(
        &mut interner,
        &Revision::new(vec![
            fixture("c.rs", &["parse"], &[]),
            fixture("d.rs", &["parse"], &[]),
        ]),
    );
    let answer = delta(&before, &after, "r1", "r2");
    assert_eq!(answer.moves.len(), 1);
    let candidate = answer.moves.first().expect("one candidate");
    assert_eq!(candidate.shape, MoveShape::Unresolved);
    assert!(!candidate.confident);
    assert!(
        !candidate.reason.is_empty(),
        "and it says why it is not a fact"
    );
    // Nothing pairs them, so nothing is consumed: two modules and two symbols on each side are all
    // still listed, because a reader has to be able to see both halves to pair them by hand.
    assert_eq!(
        answer.nodes_removed.len() + answer.nodes_added.len(),
        8,
        "both halves stay visible when nothing pairs them: {:?} {:?}",
        answer.nodes_removed,
        answer.nodes_added
    );
    assert!(
        has_node(&answer.nodes_removed, "a.rs::parse")
            && has_node(&answer.nodes_added, "c.rs::parse"),
        "{:?} / {:?}",
        answer.nodes_removed,
        answer.nodes_added
    );
    assert!(
        answer
            .notices
            .iter()
            .any(|notice| notice.contains("cannot pair")),
        "{:?}",
        answer.notices
    );
}

// ---------------------------------------------------------------------------------------------
// Churn: the metric that means "this module keeps getting reworked".
// ---------------------------------------------------------------------------------------------

#[test]
fn churn_names_the_module_that_keeps_getting_reworked_and_not_the_one_that_changed_once() {
    // Two modules, the same total amount of edge movement across five revisions: one of it in one
    // commit, the other spread over four. The ticket's sentence is about the second one.
    let mut interner = Interner::default();
    let mut history = vec![Revision::new(vec![
        fixture("core/hot.rs", &["Hot"], &[]),
        fixture("core/bulk.rs", &["Bulk"], &[]),
        fixture("core/touch.rs", &["Touch"], &[]),
    ])];
    for step in 0..4 {
        let previous = history.last().cloned().expect("non-empty");
        let mut files = previous.files.clone();
        // `core/hot.rs` gains and re-points a dependency in every revision, always against
        // `core/touch.rs`, which keeps having to answer for it.
        for file in &mut files {
            if file.path == "core/hot.rs" {
                let mut deps: Vec<(String, String)> = file
                    .deps
                    .iter()
                    .filter(|(target, _)| target != "core/touch.rs")
                    .cloned()
                    .collect();
                if step % 2 == 0 {
                    deps.push(("core/touch.rs".to_string(), "Touch".to_string()));
                }
                *file = fixture_with("core/hot.rs", &["Hot"], deps);
            }
        }
        history.push(Revision::new(files));
    }
    // The bulk change: one revision, everything at once.
    let mut bulk = history.last().expect("last").files.clone();
    for file in &mut bulk {
        if file.path == "core/bulk.rs" {
            *file = fixture(
                "core/bulk.rs",
                &["Bulk"],
                &[("core/hot.rs", "Hot"), ("core/touch.rs", "Touch")],
            );
        }
    }
    history.push(Revision::new(bulk));

    let mut outcomes = Vec::new();
    for revision in &history {
        outcomes.push(build(&mut interner, revision));
    }
    let view = outcomes.iter().fold(Naming::default(), |view, outcome| {
        Naming::merged(&view, &outcome.naming())
    });
    let steps: Vec<graph::RevisionOps> = outcomes
        .windows(2)
        .enumerate()
        .map(|(index, pair)| {
            let ops = graph::ops_between(&pair[0].graph, &pair[1].graph, &view);
            graph::RevisionOps::new(format!("r{index}"), ops)
        })
        .collect();
    let report = ChurnReport::measure(&steps, &view, outcomes.last().map(|last| &last.graph));

    // The wire is re-cut in most revisions of the window, at both ends of it: the module that keeps
    // changing its mind and the module it keeps changing its mind *about* are both reworked, which is
    // the point of counting the destination side as well as the source.
    let hot = report
        .find("core/hot.rs")
        .expect("re-cut every other revision");
    let touched = report.find("core/touch.rs").expect("on the receiving end");
    let bulk = report.find("core/bulk.rs").expect("touched once");
    assert_eq!(hot.band, Band::Reworked, "{hot:?}");
    assert_eq!(touched.band, Band::Reworked, "{touched:?}");
    assert!(
        hot.revisions >= 3 && touched.revisions >= 3,
        "{hot:?} {touched:?}"
    );
    assert!(
        bulk.churn >= 2,
        "the bulk commit really did move edges: {bulk:?}"
    );
    assert_ne!(
        bulk.band,
        Band::Reworked,
        "one big commit is not the same finding, however many edges it moved: {bulk:?}"
    );
    assert_eq!(bulk.revisions, 1, "it happened in exactly one revision");
    assert_eq!(hot.window, steps.len(), "the denominator is printed");
    assert!(
        report
            .hot
            .iter()
            .any(|module| module == "core/hot.rs" || module == "core/touch.rs"),
        "{}",
        report.sentence()
    );
    assert_eq!(report.reworked().len(), 2, "{:?}", report.per_module);
    // And the document form carries the band, so an exporter does not re-derive it.
    let read = ChurnReport::from_json(&report.to_json()).expect("round trip");
    assert_eq!(read, report);
}

// ---------------------------------------------------------------------------------------------
// The algebra, the empty answer, and the document.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_composition_of_two_deltas_is_the_delta_over_the_span() {
    // The one-line version of the property test in `graph_diff_property.rs`, on a hand-built triple:
    // a symbol that goes and comes back inside the span is not a change over the span.
    let mut interner = Interner::default();
    let one = Revision::new(vec![
        fixture("core/a.rs", &["Alpha"], &[]),
        fixture("core/b.rs", &["Beta"], &[("core/a.rs", "Alpha")]),
    ]);
    let two = Revision::new(vec![
        fixture("core/a.rs", &["Alpha"], &[]),
        fixture("core/b.rs", &["Beta"], &[]),
    ]);
    let three = Revision::new(vec![
        fixture("core/a.rs", &["Alpha"], &[]),
        fixture("core/b.rs", &["Beta"], &[("core/a.rs", "Alpha")]),
    ]);
    let g1 = build(&mut interner, &one);
    let g2 = build(&mut interner, &two);
    let g3 = build(&mut interner, &three);
    let view = Naming::merged(&g1.naming(), &Naming::merged(&g2.naming(), &g3.naming()));

    let first = graph::ops_between(&g1.graph, &g2.graph, &view);
    let second = graph::ops_between(&g2.graph, &g3.graph, &view);
    let composed = compose([&first[..], &second[..]]);
    let direct = graph::ops_between(&g1.graph, &g3.graph, &view);
    assert!(
        composed.is_empty(),
        "the dependency came back, so the span changed nothing: {composed:?}"
    );
    assert_eq!(direct, composed);

    // The same claim at the document level, where the derived findings live: composing the two
    // intermediate deltas must produce the same document as diffing the endpoints.
    let via_span = ArchitectureDelta::of(
        "r1",
        "r3",
        &composed,
        &view,
        Some(&g1.graph),
        Some(&g3.graph),
        Cost::chain(2, 2),
    );
    let endpoint = ArchitectureDelta::between(&g1.graph, &g3.graph, &view, "r1", "r3");
    assert_eq!(
        via_span.counts(),
        endpoint.counts(),
        "the composition and the direct diff must not disagree about what happened"
    );
    assert!(via_span.is_empty() && endpoint.is_empty());
}

#[test]
fn an_unchanged_revision_is_empty_and_says_which_rung_said_so() {
    let mut interner = Interner::default();
    let revision = Revision::new(vec![
        fixture("core/a.rs", &["Alpha"], &[]),
        fixture("core/b.rs", &["Beta"], &[("core/a.rs", "Alpha")]),
    ]);
    let before = build(&mut interner, &revision);
    // A commit that reworded a body: a new blob, the same structure, so no row moves at all.
    let after = build(&mut interner, &revision.reworded("core/a.rs"));
    let answer = delta(&before, &after, "r1", "r2");
    assert!(
        answer.is_empty(),
        "no row moved, and the answer says so: {:?}",
        answer.counts()
    );
    assert_eq!(answer.churn.len(), 0, "nothing to charge");
    assert!(
        answer.sentence().contains("nothing architectural changed"),
        "{}",
        answer.sentence()
    );
    // The constant-time rung, spelled out: it is the same empty answer at a different price.
    let cheap = ArchitectureDelta::empty("r1", "r2");
    assert_eq!(cheap.cost, Cost::empty());
    assert_eq!(cheap.cost.rung, Rung::Empty);
    assert!(!cheap.cost.touched_graph());
    assert!(cheap.cost.rung.from_store(), "it read one snapshot row");
    assert!(
        cheap
            .notices
            .iter()
            .any(|notice| notice.contains("no architectural change")),
        "{:?}",
        cheap.notices
    );
}

#[test]
fn the_json_form_is_a_function_of_the_facts_and_not_of_the_row_order() {
    let mut interner = Interner::default();
    let before = build(
        &mut interner,
        &Revision::new(vec![
            fixture("core/shape.rs", &["Shape"], &[]),
            fixture("app/main.rs", &["main"], &[("core/shape.rs", "Shape")]),
        ]),
    );
    let after = build(
        &mut interner,
        &Revision::new(vec![
            fixture("core/shape.rs", &[], &[]),
            fixture("core/geo.rs", &["Shape"], &[]),
            fixture("app/main.rs", &["main"], &[("core/geo.rs", "Shape")]),
        ]),
    );
    let view = view_of(&[&before, &after]);
    let mut ops = graph::ops_between(&before.graph, &after.graph, &view);
    let one = ArchitectureDelta::of(
        "r1",
        "r2",
        &ops,
        &view,
        Some(&before.graph),
        Some(&after.graph),
        Cost::adjacent(ops.len()),
    );
    ops.reverse();
    let other = ArchitectureDelta::of(
        "r1",
        "r2",
        &ops,
        &view,
        Some(&before.graph),
        Some(&after.graph),
        Cost::adjacent(ops.len()),
    );
    assert_eq!(
        one.to_json(),
        other.to_json(),
        "the same facts arriving in another order must not make another document"
    );
    let round = ArchitectureDelta::from_json(&one.to_json()).expect("it reads back");
    assert_eq!(round, one);
    assert!(one.to_json().contains(graph::CONTRACT));
    assert!(
        ArchitectureDelta::from_json("{ not json").is_err(),
        "and a truncated document is refused, not half-read"
    );
}

#[test]
fn every_finding_appears_in_the_summary_lines_in_a_fixed_order() {
    let mut interner = Interner::default();
    let before = build(
        &mut interner,
        &Revision::new(vec![
            fixture("core/shape.rs", &["Shape"], &[]),
            fixture("app/main.rs", &["main"], &[("core/shape.rs", "Shape")]),
        ]),
    );
    let after = build(
        &mut interner,
        &Revision::new(vec![
            fixture("core/shape.rs", &[], &[]),
            fixture("core/geo.rs", &["Shape"], &[]),
            fixture("app/main.rs", &["main"], &[("core/geo.rs", "Shape")]),
        ]),
    );
    let answer = delta(&before, &after, "r1", "r2");
    let lines = answer.summary_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("moved `Shape`") && line.contains("core/geo.rs")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("surface: core/geo.rs +1")),
        "{lines:?}"
    );
    // Two calls in the same process, and a re-read of the document, give the same lines: this is
    // what makes a golden over a delta document possible at all.
    assert_eq!(lines, answer.summary_lines());
    let again = ArchitectureDelta::from_json(&answer.to_json()).expect("reads back");
    assert_eq!(again.summary_lines(), lines);
    assert!(
        answer.cost.rung == Rung::InHand,
        "built from graphs, not from a store"
    );
}

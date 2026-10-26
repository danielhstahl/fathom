//! A Cargo workspace is one corpus, and a Rust module path is crate-relative (**reco-842**).
//!
//! The extractor used to resolve an import by joining the specifier's segments and asking the index for
//! that *corpus-relative* path. In a single flat crate that is the same thing; in a workspace it is
//! nothing — `crates/alpha/src/sub.rs` is not the module `crates::alpha::src::sub`, it is the module
//! `alpha::sub`. So every `use crate::…`, every `mod …;` and every `use sibling::…` in a workspace
//! asked the index for a path it did not hold and was stored as nothing, which is the fourth "thing that
//! may be wrong" on [the graph page](../../../../docs/concepts/architecture-graph.md): a dependency that
//! leaves the corpus writes no row. The consequence was that a `[layering]` block whose `[layers]`
//! entries are crate directories — the shape every Rust workspace writes — judged a graph with no
//! crate-boundary edge in it, and printed "0 findings" about an architecture it had not looked at.
//!
//! What these tests own:
//!
//! * a specifier is resolved **inside the crate that names it** — `crate::`, `self`, `super` and `mod x;`
//!   are anchored on the file's own crate root, and a leading workspace member name is anchored on
//!   *that* crate's root;
//! * a dependency that leaves a crate is now a row, so `[layering]` can judge a crate boundary at all;
//! * a third-party base still leaves the corpus and is still stored as nothing — that half of finding 4
//!   was right the first time;
//! * the edge is file-granular, because a resolved Rust import names a module, and a module is a file.
//!   (Go's is a package, and `tests/structure_tail.rs` owns that claim);
//! * and the report says out loud when the rules had nothing to disagree about — `[layering]` in force,
//!   rows judged, zero of them crossing a layer boundary, which is the state that let a whole repository
//!   print "0 findings" while its extractor wrote no crate-boundary edge at all.
//!
//! ```text
//! cargo test -p fathom-analyze --features lang-rust --test structure_workspace
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::graph::layers::Layers;
use fathom_analyze::graph::{Builder, GraphPolicy, Interner, ModuleInput};
use fathom_analyze::layering::locate::Locate;
use fathom_analyze::layering::{Rules, Violation, evaluate, load};
use fathom_analyze::structure::{
    self, EdgeKind, ExtractPolicy, FileStructure, Granularity, SourceFile,
};

/// One file of the fixture workspace: corpus-relative path and source.
type Source = (&'static str, &'static str);

/// The workspace: two crates whose package directories contain a hyphen (so the import name is not the
/// directory name verbatim), one façade re-export, one bare module import, one third-party base, and —
/// when asked — one deliberately uphill edge from the lower crate into the higher one.
fn workspace(uphill: bool) -> Vec<Source> {
    let mut files: Vec<Source> = vec![
        (
            "crates/alpha/src/lib.rs",
            "pub mod sub;\npub mod build;\n\
             pub use crate::sub::Sub;\n\
             use beta_kit::thing::Thing;\n\
             use beta_kit::{self, Kit};\n\
             use beta_kit::tool;\n\
             use spare::odd::Odd;\n\
             use serde::Serialize;\n\
             pub fn build(t: Thing) -> Box<dyn Serialize> { let _ = tool::dig(); Box::new(t) }\n",
        ),
        (
            "crates/alpha/src/build.rs",
            "use beta_kit::Thing;\npub fn make() -> Thing { Thing }\n",
        ),
        ("crates/alpha/src/sub.rs", "pub struct Sub;\n"),
        (
            "crates/beta-kit/src/lib.rs",
            "pub mod thing;\npub mod tool;\npub use crate::thing::Thing;\n\n\
             pub struct Kit;\n",
        ),
        (
            "crates/beta-kit/src/thing.rs",
            "use crate::tool::dig;\npub struct Thing;\n\nimpl Thing { pub fn new() -> Self { Self } }\n\
             pub fn lifted() -> usize { dig() }\n",
        ),
        (
            "crates/beta-kit/src/tool.rs",
            "pub fn dig() -> usize { 0 }\n",
        ),
        // A third crate with no `[layers]` rule of its own: its edges are counted untagged, never
        // guessed into a neighbour's layer.
        ("crates/spare/src/lib.rs", "pub mod odd;\n"),
        ("crates/spare/src/odd.rs", "pub struct Odd;\n"),
        // A binary crate, to show `main.rs` is a crate root too and that files under `src/` of a bin
        // package are modules of the same crate rather than a second one.
        (
            "crates/alpha-bin/src/main.rs",
            "mod greet;\nuse alpha::sub::Sub;\nfn main() { let _ = greet::hello(&Sub); }\n",
        ),
        (
            "crates/alpha-bin/src/greet.rs",
            "use alpha::sub::Sub;\npub fn hello(_: &Sub) {}\n",
        ),
        // An integration test: no `src`, so it keeps the corpus-wide namespace and names the crate under
        // test the way every integration test does.
        (
            "crates/alpha/tests/smoke.rs",
            "use alpha::sub::Sub;\n#[test] fn builds() { let _ = Sub; }\n",
        ),
    ];
    if uphill {
        // The seeded defect: the *lower* crate reaches into the higher one. Before reco-842 this was
        // invisible — the reference left the corpus, so no row was written and no rule had anything to
        // fire on.
        files.push((
            "crates/beta-kit/src/up.rs",
            "use alpha::sub::Sub;\npub fn leaked() -> Sub { Sub }\n",
        ));
    }
    files
}

fn extract(files: &[Source]) -> structure::CorpusStructure {
    let borrowed: Vec<SourceFile<'_>> = files
        .iter()
        .map(|(path, body)| SourceFile::new(path, body.as_bytes()))
        .collect();
    structure::extract(&borrowed, &ExtractPolicy::default())
}

fn record<'a>(corpus: &'a structure::CorpusStructure, path: &str) -> &'a FileStructure {
    corpus
        .files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("`{path}` is in the corpus and is never dropped"))
}

/// The resolved and wildcard edges of one file, as `target::symbol` for the readable assertion.
fn edges(corpus: &structure::CorpusStructure, path: &str) -> Vec<String> {
    let Some(structure) = record(corpus, path).structure.as_ref() else {
        panic!("`{path}` was readable");
    };
    structure
        .edges
        .iter()
        .filter(|edge| edge.kind != EdgeKind::Unresolved)
        .map(|edge| format!("{}::{}", edge.target, edge.symbol))
        .collect()
}

fn has_edges(corpus: &structure::CorpusStructure, path: &str, wanted: &[&str]) {
    let got = edges(corpus, path);
    for want in wanted {
        assert!(
            got.iter().any(|edge| edge == want),
            "`{path}` must depend on `{want}`; it reports {got:?}"
        );
    }
}

fn edge_to<'a>(
    corpus: &'a structure::CorpusStructure,
    path: &str,
    target: &str,
) -> Vec<&'a fathom_analyze::structure::Edge> {
    let structure = record(corpus, path).structure.as_ref().unwrap();
    structure
        .edges
        .iter()
        .filter(|edge| edge.target == target)
        .collect()
}

// ===========================================================================
// the specifier, read inside the crate that wrote it
// ===========================================================================

#[test]
fn a_sibling_crate_import_is_an_edge_and_not_an_absence() {
    let corpus = extract(&workspace(false));
    has_edges(
        &corpus,
        "crates/alpha/src/lib.rs",
        &[
            // `use beta_kit::thing::Thing` — the sibling's name is a corpus location now, and the
            // package directory's hyphen is spelled the way `use` spells it.
            "crates/beta-kit/src/thing.rs::Thing",
            // `use crate::sub::Sub` — the intra-crate half, which was missing just as completely.
            "crates/alpha/src/sub.rs::Sub",
        ],
    );
}

#[test]
fn a_facade_reexport_reaches_the_file_that_declares_the_name() {
    let corpus = extract(&workspace(false));
    // `use beta_kit::Kit` is declared at the crate root; `use beta_kit::Thing` is re-exported from
    // `thing.rs`. Both are crate-root lookups — the specifier names no module — and the re-export has to
    // be followed to the file that declares it, with the hop recorded.
    let landed: Vec<_> = edge_to(
        &corpus,
        "crates/alpha/src/build.rs",
        "crates/beta-kit/src/thing.rs",
    )
    .into_iter()
    .filter(|edge| edge.symbol == "Thing")
    .collect();
    assert!(
        !landed.is_empty(),
        "`use beta_kit::Thing` must land on the file that declares Thing: {:?}",
        edges(&corpus, "crates/alpha/src/build.rs")
    );
    let via: Vec<_> = landed.iter().filter(|edge| edge.via.is_some()).collect();
    assert!(
        !via.is_empty(),
        "the answer came through the crate root, and a re-export hop that is not recorded is a \
         forwarding fact nobody can blame: {:?}",
        landed
            .iter()
            .map(|edge| (&edge.target, &edge.symbol, &edge.via))
            .collect::<Vec<_>>()
    );
    has_edges(
        &corpus,
        "crates/alpha/src/lib.rs",
        &["crates/beta-kit/src/lib.rs::Kit"],
    );
}

#[test]
fn a_bare_module_import_names_the_module_and_not_a_symbol() {
    let corpus = extract(&workspace(false));
    // `use beta_kit::tool;` names the module `tool`, which `tool.rs` does not declare as a *symbol*.
    // The dependency is real and which of its items are used is unnamed — `s1` finding 2's kind, never
    // an absence.
    let hits = edge_to(
        &corpus,
        "crates/alpha/src/lib.rs",
        "crates/beta-kit/src/tool.rs",
    );
    assert!(
        hits.iter()
            .any(|edge| edge.kind == EdgeKind::Wildcard && edge.specifier == "beta_kit::tool"),
        "a module import has to keep the specifier it used: {:?}",
        edges(&corpus, "crates/alpha/src/lib.rs")
    );
    // `use beta_kit::{self, Kit}` is the same claim one level up: the specifier names the crate as a
    // whole, and the crate as a whole is its root file — not a symbol called `self`.
    let root = edge_to(
        &corpus,
        "crates/alpha/src/lib.rs",
        "crates/beta-kit/src/lib.rs",
    );
    assert!(
        root.iter()
            .any(|edge| edge.kind == EdgeKind::Wildcard && edge.specifier == "beta_kit::self"),
        "`beta_kit::self` is the crate root, which the crate declares: {:?}",
        edges(&corpus, "crates/alpha/src/lib.rs")
    );
}

#[test]
fn a_module_declaration_is_an_edge_inside_its_own_crate() {
    let corpus = extract(&workspace(false));
    has_edges(
        &corpus,
        "crates/beta-kit/src/lib.rs",
        &[
            "crates/beta-kit/src/thing.rs::thing",
            "crates/beta-kit/src/tool.rs::tool",
        ],
    );
    has_edges(
        &corpus,
        "crates/alpha/src/lib.rs",
        &["crates/alpha/src/sub.rs::sub"],
    );
    has_edges(
        &corpus,
        "crates/alpha-bin/src/main.rs",
        &["crates/alpha-bin/src/greet.rs::greet"],
    );
}

#[test]
fn a_module_path_is_crate_relative_not_corpus_relative() {
    let corpus = extract(&workspace(false));
    // The module a file *is* is what `self`, `super` and `mod x;` are anchored on, and it is stated in
    // the record: `crates/beta-kit/src/thing.rs` is the module `thing` of the crate `beta_kit`.
    // It shows up as the visibility path: `impl crate::thing::Thing` inside `thing.rs` resolves in the
    // crate, and `Thing` is public because every module between it and the crate root is `pub`.
    let thing = record(&corpus, "crates/beta-kit/src/thing.rs")
        .structure
        .as_ref()
        .unwrap();
    assert!(
        thing.exports.iter().any(|name| name == "Thing"),
        "`Thing` is public API of `beta_kit::thing` and its export path was computed from a \
         corpus-relative module, which no crate root is: {:?}",
        thing.exports
    );
    // And the same file's own-crate import resolves against the crate, not the corpus.
    assert!(
        thing
            .imports
            .iter()
            .any(|import| import.specifier == "crate::tool::dig"),
        "the statement is kept as written: {:?}",
        thing.imports
    );
    has_edges(
        &corpus,
        "crates/beta-kit/src/thing.rs",
        &["crates/beta-kit/src/tool.rs::dig"],
    );
}

#[test]
fn a_binary_crate_names_its_sibling_the_way_a_library_does() {
    let corpus = extract(&workspace(false));
    // `crates/alpha/tests/smoke.rs` and `crates/alpha-bin/src/greet.rs` have nothing to anchor on in
    // their own directory: one is a crate root of its own crate, the other a module of `alpha-bin`. Both
    // reach `alpha::sub::Sub` by name, which is how integration tests say it.
    has_edges(
        &corpus,
        "crates/alpha/tests/smoke.rs",
        &["crates/alpha/src/sub.rs::Sub"],
    );
    has_edges(
        &corpus,
        "crates/alpha-bin/src/greet.rs",
        &["crates/alpha/src/sub.rs::Sub"],
    );
}

#[test]
fn a_third_party_base_still_leaves_the_corpus() {
    let corpus = extract(&workspace(false));
    let structure = record(&corpus, "crates/alpha/src/lib.rs")
        .structure
        .as_ref()
        .unwrap();
    let unresolved: Vec<&str> = structure
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Unresolved)
        .map(|edge| edge.specifier.as_str())
        .collect();
    assert!(
        unresolved.contains(&"serde::Serialize"),
        "`serde` is not a workspace member and must stay a stated absence: {unresolved:?}"
    );
}

#[test]
fn a_cross_crate_edge_is_file_granular_because_a_module_is_a_file() {
    let corpus = extract(&workspace(false));
    let hits = edge_to(
        &corpus,
        "crates/alpha/src/lib.rs",
        "crates/beta-kit/src/thing.rs",
    );
    assert!(!hits.is_empty());
    for edge in hits {
        assert_eq!(
            edge.granularity,
            Granularity::File,
            "a resolved Rust import names a module, and a module is one file; `Granularity::Package` \
             is Go's claim and printing it here would overstate what the language declared"
        );
    }
}

// ===========================================================================
// the consequence: `[layering]` can judge a crate boundary
// ===========================================================================

/// The repository's declaration, in the shape a Cargo workspace writes: `[layers]` entries are crate
/// directories, and the rules are written between them.
const CONFIG: &str = r#"
[layers]
l0_kit = ["crates/beta-kit"]
l1_app = ["crates/alpha", "crates/alpha-bin"]

[layering]
mode = "warn"
order = ["l0_kit", "l1_app"]
no_upward = true
forbid = ["the-kit-may-not-reach-the-app: l0_kit -> l1_app"]
"#;

struct Gate {
    rules: Rules,
    violations: Vec<Violation>,
    counters: fathom_analyze::layering::engine::Counters,
    notices: Vec<String>,
}

/// Extract, build the revision's graph and judge it: the whole lane minus the store, on the corpus the
/// extractor actually read rather than on hand-written edges.
fn gate(files: &[Source]) -> Gate {
    gate_with(files, CONFIG)
}

fn gate_with(files: &[Source], config: &str) -> Gate {
    let corpus = extract(files);
    let (layers, mut notices) = Layers::from_repo_config_text(config);
    let (rules, rule_notices) = load(&layers, config);
    notices.extend(rule_notices);
    let inputs: Vec<ModuleInput<'_>> = corpus
        .files
        .iter()
        .map(|file| ModuleInput {
            path: &file.path,
            blob: file.path.as_str(),
            file: Some(file),
        })
        .collect();
    let locate = Locate::from_inputs(&inputs);
    let outcome =
        Builder::new(GraphPolicy::default(), layers).build(&inputs, &mut Interner::default());
    let assessment = evaluate(&outcome.graph, &outcome.naming(), &rules, &locate);
    Gate {
        rules,
        violations: assessment.violations,
        counters: assessment.counters,
        notices,
    }
}

fn keys(violations: &[Violation]) -> Vec<String> {
    violations.iter().map(Violation::key).collect()
}

#[test]
fn the_gate_loads_clean_and_the_fixture_workspace_is_clean() {
    let gate = gate(&workspace(false));
    assert_eq!(gate.rules.refused, 0, "{:?}", gate.notices);
    assert!(
        gate.violations.is_empty(),
        "a downhill workspace is clean: {}",
        keys(&gate.violations).join("\n")
    );
    // The claim the ticket was filed against: the graph has dependency rows at all, and more than the
    // zero crate-boundary edges the old extractor could write.
    assert!(
        gate.counters.checked > 0,
        "the layering check judged nothing, which is the state reco-842 is about"
    );
    assert!(
        gate.counters.cross_layer > 0,
        "the rules need a row that crosses a layer boundary to disagree about, or the zero they report \
         is the zero reco-842 was filed for: {:?}",
        gate.counters
    );
}

/// Every crate in one layer: the rules load, the rows are judged, and nothing can fire. This is the
/// state a `[layering]` block drifts into when `[layers]` is coarser than the rules, and the point of
/// the counter is that the report says so instead of looking clean.
const ONE_LAYER: &str = r#"
[layers]
everything = ["crates"]

[layering]
mode = "warn"
order = ["everything"]
no_upward = true
"#;

#[test]
fn a_rule_set_with_no_cross_layer_row_says_so_instead_of_looking_clean() {
    let gate = gate_with(&workspace(true), ONE_LAYER);
    assert_eq!(gate.rules.refused, 0, "{:?}", gate.notices);
    assert!(
        gate.violations.is_empty(),
        "one layer has no direction to be wrong in: {:?}",
        keys(&gate.violations)
    );
    assert!(gate.counters.checked > 0);
    assert_eq!(gate.counters.cross_layer, 0);
    assert!(
        gate.counters
            .sentence()
            .contains("no row crossed a layer boundary"),
        "{}",
        gate.counters.sentence()
    );
}

#[test]
fn a_crate_boundary_violation_is_found_and_named() {
    let gate = gate(&workspace(true));
    let found = keys(&gate.violations);
    assert!(
        found.iter().any(|key| key
            == "the-kit-may-not-reach-the-app::crates/beta-kit/src/up.rs->crates/alpha/src/sub.rs"),
        "the seeded uphill crate edge must be a violation: {}\n(counters: {:?})",
        found.join("\n"),
        gate.counters
    );
    assert!(
        found.iter().any(|key| key.starts_with("no_upward::")),
        "`no_upward` is on, and the same edge violates it: {}",
        found.join("\n")
    );
}

#[test]
fn an_untagged_crate_in_the_same_workspace_is_counted_not_guessed() {
    // `crates/spare` has no `[layers]` rule. Its edges are counted untagged and stay judgeable by a
    // path selector, which is the state the counters exist to distinguish from "nothing was found".
    let gate = gate(&workspace(false));
    assert!(
        gate.counters.checked > 0 && gate.counters.untagged > 0,
        "counters: {:?}",
        gate.counters
    );
}

//! The scorecard's store seam: one revision's graph in, one row out (`e3.9`, `reco-al1`).
//!
//! `tests/scorecard_formula.rs` checks the arithmetic with measurements in hand. This file checks where the
//! measurements come *from* — the graph the index already stores — because that is where a scorecard can go
//! quietly wrong in ways the arithmetic cannot catch:
//!
//! * a dimension that cannot be measured must arrive as a **stated absence**, never as zero: a revision with
//!   no earlier revision has no API story, and scoring that as perfect API hygiene would be the single most
//!   misleading thing this module could do;
//! * the counts must be the graph's own, so a cycle the store can see moves the cycle dimension and the
//!   score, and a graph with nothing in it does not;
//! * the same store read twice produces the same row, which is what makes a stored trend auditable after
//!   the fact;
//! * and a backfill must stop honestly when a chain will not materialise — keeping the rows it did score and
//!   saying where it stopped, instead of drawing half a trend and calling it history.
//!
//! The graphs are recorded through [`fathom_analyze::graph::Lane::record`], the same writer the indexer
//! uses, so the read path under test is the one `fathom architecture` takes.
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::graph::lane::Lane;
use fathom_analyze::graph::{Builder, ModuleInput};
use fathom_analyze::hotspots::policy_from_config_text;
use fathom_analyze::scorecard::dimension::Dimension;
use fathom_analyze::scorecard::lane::{Knobs, backfill, score};
use fathom_analyze::scorecard::policy::Policy;
use fathom_analyze::scorecard::score::Scorecard;
use fathom_core::time::Timestamp;
use fathom_store::spine::Store;

/// The clock the ratchet and the hotspot window are measured against — fixed, so a row is reproducible.
const NOW: i64 = 1_780_000_000_000_000;

/// The knobs a repository with no `fathom.toml` gets: default weights, no layering rules, no ratchet.
fn knobs() -> Knobs {
    Knobs::bare(
        Policy::defaults(),
        policy_from_config_text("").0,
        Timestamp::from_unix_micros(NOW),
    )
}

/// A module whose structure exports `exports` and depends on `(target, symbol)` pairs.
fn file(
    path: &str,
    exports: &[&str],
    deps: &[(&str, &str)],
) -> fathom_analyze::structure::FileStructure {
    use fathom_analyze::structure::{
        Complexity, Edge, EdgeKind, FileLevel, FileStructure, Granularity, Language, Resolution,
        Span, Structure, Symbol, SymbolKind, Visibility,
    };
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

/// One module's blob: a stand-in for git's content address with the one property the lane depends on — the
/// same exports and edges produce the same blob, and a changed module produces a different one. That is the
/// only reason the lane's per-module cache can be correct.
fn blob_of(f: &fathom_analyze::structure::FileStructure) -> String {
    format!(
        "blob-{}:{}",
        f.path,
        f.structure.as_ref().map_or_else(
            || "none".to_owned(),
            |s| format!(
                "{}|{}",
                s.exports.join(","),
                s.edges
                    .iter()
                    .map(|e| format!("{}:{}", e.target, e.symbol))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        )
    )
}

/// Record one or two revisions into a fresh store and hand back the store with a lane over it.
///
/// The lane is rebuilt after recording so nothing under test is served from the writer's own cache: a
/// scorecard that only worked while the same process that wrote the graph was reading it would be a
/// scorecard that could not be reproduced tomorrow.
fn recorded(revisions: &[&[fathom_analyze::structure::FileStructure]]) -> Store {
    recorded_with(revisions, fathom_analyze::graph::Layers::default())
}

/// The same, with layer tags in the builder so the stored rows arrive already tagged — which is the only
/// way the boundaries dimension can be judged at all.
fn recorded_with(
    revisions: &[&[fathom_analyze::structure::FileStructure]],
    layers: fathom_analyze::graph::Layers,
) -> Store {
    let mut store = Store::open_in_memory().expect("open");
    {
        let mut lane = Lane::new(
            &mut store,
            Builder {
                policy: Default::default(),
                layers,
            },
        )
        .expect("lane");
        let mut base: Option<String> = None;
        for (index, files) in revisions.iter().enumerate() {
            let tree = format!("tree{index}");
            let blobs: Vec<String> = files.iter().map(blob_of).collect();
            let given: Vec<ModuleInput<'_>> = files
                .iter()
                .zip(&blobs)
                .map(|(f, blob)| ModuleInput {
                    path: f.path.as_str(),
                    blob: blob.as_str(),
                    file: Some(f),
                })
                .collect();
            lane.record(
                &tree,
                base.as_deref(),
                &given,
                NOW + i64::try_from(index).unwrap(),
            )
            .expect("record");
            base = Some(tree);
        }
        lane.flush_interner().expect("interner flushed");
    }
    store
}

/// The lane reads a rebuilt store, never the writer's cache.
fn reopened(store: &mut Store) -> Lane<'_> {
    Lane::new(store, Builder::default()).expect("lane")
}

/// The five measurements of a recorded revision, through the real lane.
fn measured(lane: &Lane<'_>, tree: &str, previous: Option<&str>, knobs: &Knobs) -> Scorecard {
    score(lane, tree, previous, knobs)
        .expect("a recorded revision materialises")
        .scorecard
}

/// **The counts are the graph's counts, and every dimension answers.**
#[test]
fn each_dimension_measures_the_graph_that_is_actually_there() {
    let knobs = knobs();
    let files = [
        file("core.rs", &["Config"], &[]),
        file("state.rs", &["load", "save"], &[("core.rs", "Config")]),
        file(
            "api.rs",
            &["handle"],
            &[("core.rs", "Config"), ("state.rs", "load")],
        ),
    ];
    let mut store = recorded(&[&files]);
    let lane = reopened(&mut store);
    let card = measured(&lane, "tree0", None, &knobs);
    assert!(card.scored, "{}", card.sentence());
    // Three of five answer here, and the two that do not are refusals with reasons: api has no earlier
    // revision, and boundaries has no *written rule* to judge against. Both leave the denominator.
    assert_eq!(card.measured, 3, "{}", card.sentence());
    assert_eq!(card.denominator, 0.55);

    // Coupling: three modules and three dependency edges, against a denominator of modules × ceiling.
    let coupling = card.term(Dimension::Coupling).expect("coupling");
    assert!(coupling.measured);
    assert!(
        coupling.badness > 0.0,
        "three edges is not nothing: {coupling:?}"
    );
    assert!(
        coupling.badness < 1.0,
        "three edges of nine is far from saturated"
    );
    assert!(
        coupling.basis.contains('3'),
        "the basis names the counts: {}",
        coupling.basis
    );

    // Acyclicity: nothing loops here, and the dimension says so with a measured zero rather than a refusal.
    let acyclicity = card.term(Dimension::Acyclicity).expect("acyclicity");
    assert!(acyclicity.measured);
    assert_eq!(acyclicity.badness, 0.0, "{acyclicity:?}");

    // Boundaries: with no `[layering]` rules declared, "respected" has no definition, and the dimension
    // refuses rather than reporting a clean bill of health for a repository that never wrote a rule.
    let boundaries = card.term(Dimension::Boundaries).expect("boundaries");
    assert!(!boundaries.measured, "{boundaries:?}");
    assert!(
        boundaries
            .unscored
            .as_deref()
            .unwrap_or_default()
            .contains("unwritten"),
        "{boundaries:?}"
    );

    // Hotspots: the ranking runs on the graph's own metrics and says which ones it could not read.
    let hotspots = card.term(Dimension::Hotspots).expect("hotspots");
    assert!(
        hotspots
            .caveats
            .iter()
            .any(|why| why.contains("size and complexity")),
        "{hotspots:?}"
    );

    // API hygiene refuses: no earlier revision, which must never read as perfect hygiene.
    let api = card.term(Dimension::Api).expect("api");
    assert!(!api.measured);
    assert!(
        api.unscored
            .as_deref()
            .unwrap_or_default()
            .contains("no earlier revision"),
        "{api:?}"
    );

    // The total is the sum of the printed parts, recomputed here from the card's own terms.
    assert_eq!(
        card.total,
        card.terms.iter().map(|term| term.points).sum::<i64>()
    );

    // And the same store, read again, gives byte-identical rows.
    let again = measured(&lane, "tree0", None, &knobs);
    assert_eq!(again.to_stored(), card.to_stored(), "determinism, in bytes");
}

/// A cycle the store can see costs the cycle dimension and the score.
#[test]
fn a_cycle_in_the_graph_costs_the_cycle_dimension_and_the_score() {
    let knobs = knobs();
    let clean = [
        file("a.rs", &["a"], &[]),
        file("b.rs", &["b"], &[("a.rs", "a")]),
        file("c.rs", &["c"], &[("b.rs", "b")]),
    ];
    let cyclic = [
        file("a.rs", &["a"], &[("c.rs", "c")]),
        file("b.rs", &["b"], &[("a.rs", "a")]),
        file("c.rs", &["c"], &[("b.rs", "b")]),
    ];
    let mut clean_store = recorded(&[&clean]);
    let clean_lane = reopened(&mut clean_store);
    let clean_card = measured(&clean_lane, "tree0", None, &knobs);
    let mut cyclic_store = recorded(&[&cyclic]);
    let cyclic_lane = reopened(&mut cyclic_store);
    let cycle_card = measured(&cyclic_lane, "tree0", None, &knobs);

    assert_eq!(
        clean_card.term(Dimension::Acyclicity).unwrap().badness,
        0.0,
        "the fixture is acyclic"
    );
    let after = cycle_card.term(Dimension::Acyclicity).expect("term");
    assert!(
        after.badness > 0.0,
        "three modules in one knot is not nothing: {after:?}"
    );
    assert!(after.badness <= 1.0);
    assert!(
        cycle_card.score < clean_card.score,
        "the knot must cost the repository something: {} vs {}",
        cycle_card.score,
        clean_card.score
    );
    // The basis and the reason carry the integers — nodes inside a cycle, and how many cycles — so the
    // ratio can be checked against `fathom cycles` without re-deriving anything.
    assert!(
        after.basis.chars().any(|c| c.is_ascii_digit())
            && after.reason.chars().any(|c| c.is_ascii_digit()),
        "{after:?}"
    );
}

/// The API dimension needs a *before*; with one it measures, without one it refuses.
#[test]
fn the_api_dimension_measures_rows_that_arrived_and_refuses_without_a_before() {
    let knobs = knobs();
    let before = [
        file("core.rs", &["Config"], &[]),
        file("api.rs", &["handle"], &[("core.rs", "Config")]),
    ];
    // The second revision loses `api.rs`'s export: the kind of thing API hygiene exists to notice.
    let after = [
        file("core.rs", &["Config"], &[]),
        file("api.rs", &["handle_new"], &[("core.rs", "Config")]),
    ];
    let mut store = recorded(&[&before, &after]);
    let lane = reopened(&mut store);
    let head = measured(&lane, "tree1", Some("tree0"), &knobs);
    let api = head.term(Dimension::Api).expect("api");
    assert!(
        api.measured,
        "with a before, the dimension has something to compare: {api:?}"
    );
    // One export left the graph and one arrived, and the graph grain counted exactly that: `handle` gone,
    // `handle_new` present. The curve is the knee — this grain has no honest denominator — so one loss
    // against the published knee of 4 is 1/(1+4) = 0.2, the arithmetic printed beside it.
    assert_eq!(
        api.reason, "1 export(s) left the graph · 1 arrived",
        "{api:?}"
    );
    assert_eq!(api.badness, 0.2, "{api:?}");
    assert_eq!(
        api.formula.as_deref(),
        Some("1 / (1 + 4) = 0.2"),
        "the formula line is the arithmetic, in the order a reader redoos it"
    );
    // And the caveat says plainly what this grain cannot see, so a 0.2 is never read as "nothing broke".
    assert!(
        api.caveats
            .iter()
            .any(|why| why.contains("signature change that kept its name is invisible")),
        "{api:?}"
    );

    // The first revision has no before at all, and says so instead of claiming a clean sheet.
    let cold = measured(&lane, "tree0", None, &knobs);
    let cold_api = cold.term(Dimension::Api).expect("api");
    assert!(!cold_api.measured);
    assert!(
        cold_api
            .unscored
            .as_deref()
            .unwrap_or_default()
            .contains("no earlier revision"),
        "{cold_api:?}"
    );
    // The refusal costs nothing: the cold revision is not penalised for being first, and its denominator
    // shrinks instead — which is the same renormalisation the formula documents, arriving from the store.
    assert_eq!(cold_api.points, 0);
    assert_eq!(cold.denominator, 0.55);
    assert_eq!(head.denominator, 0.7);
}

/// A backfill scores what it can and says where it stopped.
#[test]
fn a_backfill_scores_the_history_it_has_and_says_where_it_stopped() {
    let knobs = knobs();
    let before = [
        file("a.rs", &["a"], &[]),
        file("b.rs", &["b"], &[("a.rs", "a")]),
    ];
    let after = [
        file("a.rs", &["a"], &[]),
        file("b.rs", &["b"], &[("a.rs", "a")]),
        file("c.rs", &["c"], &[("b.rs", "b")]),
    ];
    let mut store = recorded(&[&before, &after]);
    let lane = reopened(&mut store);
    let trees = vec![
        "tree1".to_owned(),
        "tree0".to_owned(),
        "tree-gone".to_owned(),
    ];
    let (rows, notices, scored) = backfill(&lane, &trees, &knobs);
    assert_eq!(scored, 2, "the two revisions that exist were scored");
    assert_eq!(rows.len(), 2);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].contains("tree-gone") && notices[0].contains("backfill stopped"),
        "{notices:?}"
    );
    for row in &rows {
        assert!(
            row.since.is_none(),
            "a backfilled row carries no review delta yet"
        );
        assert!(
            row.built_utc > 0,
            "but it carries the clock it was scored at"
        );
        let read = Scorecard::from_stored(&row.dims).expect("a row that reads back");
        assert_eq!(read.revision, row.tree_oid);
        assert!(read.scored);
    }
    // Scoring is a read: the lane wrote nothing, which is what makes `--read-only` honest.
    assert_eq!(lane.read().scorecard_count().expect("count"), 0);
    // Ordering matters: each revision's API dimension is compared with the next-oldest in the list, so the
    // newest row is the one that has a real before.
    let newest = Scorecard::from_stored(&rows[0].dims).expect("row");
    assert_eq!(newest.revision, "tree1");
    assert!(newest.term(Dimension::Api).expect("api").measured);
}

/// A measured revision writes one row, and the row reads back as the same score.
#[test]
fn a_measured_revision_writes_one_row_and_reads_back_as_the_same_score() {
    let knobs = knobs();
    let files = [
        file("a.rs", &["a"], &[]),
        file("b.rs", &["b"], &[("a.rs", "a")]),
    ];
    let mut store = recorded(&[&files]);
    let row = {
        let lane = reopened(&mut store);
        score(&lane, "tree0", None, &knobs)
            .expect("score")
            .row(Some(r#"{"from":90,"to":88,"revisions":1}"#.to_owned()))
    };
    assert_eq!(row.tree_oid, "tree0");
    assert!(
        row.since
            .as_deref()
            .unwrap_or_default()
            .contains("\"from\":90"),
        "the caller's delta travels into the row unchanged"
    );
    let card = Scorecard::from_stored(&row.dims).expect("the row reads back");
    assert!(store.scorecard_write(&row).expect("write"));
    assert!(
        !store.scorecard_write(&row).expect("second write"),
        "written once"
    );
    let back = store.scorecard_get("tree0").expect("get").expect("row");
    let stored = Scorecard::from_stored(&back.dims).expect("reads back");
    assert_eq!(stored.score, card.score);
    assert_eq!(stored.total, card.total);
    assert_eq!(store.scorecard_count().expect("count"), 1);
}

/// The rule set is what makes the boundaries dimension measurable at all — and a graph that breaks a written
/// rule pays for it.
#[test]
fn a_written_rule_moves_the_boundaries_dimension_and_an_unwritten_one_does_not() {
    // The same three-line grammar a repository writes in its `fathom.toml`: three layers, ordered, with
    // nothing allowed to point upward.
    const CONFIG: &str = r#"
[layers]
l1_store = ["crates/store"]
l2_core = ["crates/core"]
l3_api = ["crates/api"]

[layering]
order = ["l1_store", "l2_core", "l3_api"]
no_upward = true
"#;
    let (layers, notices) = fathom_analyze::graph::Layers::from_repo_config_text(CONFIG);
    assert!(notices.is_empty(), "{notices:?}");
    let (rules, notices) = fathom_analyze::layering::load(&layers, CONFIG);
    assert!(notices.is_empty(), "{notices:?}");
    assert!(!rules.active().is_empty(), "the order rule is active");

    let bare = knobs();
    let ruled = Knobs {
        rules,
        ..bare.clone()
    };
    // Downward only: api → store. Allowed by every reading of the order.
    let clean = [
        file("crates/store/s.rs", &["s"], &[]),
        file("crates/api/a.rs", &["a"], &[("crates/store/s.rs", "s")]),
    ];
    // Upward: store → api, which is exactly what `no_upward` forbids.
    let violating = [
        file("crates/store/s.rs", &["s"], &[("crates/api/a.rs", "a")]),
        file("crates/api/a.rs", &["a"], &[]),
    ];
    let mut clean_store = recorded_with(&[&clean], layers.clone());
    let clean_lane = reopened(&mut clean_store);
    let clean_card = measured(&clean_lane, "tree0", None, &ruled);
    let mut bad_store = recorded_with(&[&violating], layers);
    let bad_lane = reopened(&mut bad_store);
    let bad_card = measured(&bad_lane, "tree0", None, &ruled);

    let clean_boundaries = clean_card.term(Dimension::Boundaries).expect("boundaries");
    let bad_boundaries = bad_card.term(Dimension::Boundaries).expect("boundaries");
    // With a rule written, the dimension measures rather than refuses — in both directions of the result.
    assert!(clean_boundaries.measured, "{clean_boundaries:?}");
    assert!(bad_boundaries.measured, "{bad_boundaries:?}");
    assert_eq!(
        clean_boundaries.badness, 0.0,
        "a downward arrow is not a violation: {clean_boundaries:?}"
    );
    assert!(
        bad_boundaries.badness > 0.0,
        "an upward arrow is one: {bad_boundaries:?}"
    );
    assert!(bad_boundaries.badness <= 1.0);
    assert!(
        bad_card.score < clean_card.score,
        "and it costs the score: {} vs {}",
        bad_card.score,
        clean_card.score
    );
    // The reason names the count and the rule set it was judged by, so the verdict can be checked against
    // `fathom layers` rather than taken on trust.
    assert!(
        bad_boundaries.reason.chars().any(|c| c.is_ascii_digit()),
        "{bad_boundaries:?}"
    );
    // The same graph, judged with no rules at all, refuses — which is the point of the refusal: the score
    // cannot quietly imply that an unruled repository is a lawful one.
    let lawless = bare;
    let lawless_card = measured(&bad_lane, "tree0", None, &lawless);
    assert!(
        !lawless_card
            .term(Dimension::Boundaries)
            .expect("b")
            .measured
    );
}

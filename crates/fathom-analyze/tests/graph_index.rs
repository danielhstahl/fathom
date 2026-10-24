//! The index's graph pass, driven through the same entry point the CLI uses (`e3.11`, `reco-dlp`).
//!
//! `crates/fathom-cli/tests/index_graph.rs` owns the user-visible half — the report line, the JSON, the
//! refusal sentences. What lives here is the mechanism underneath, where the four claims that could actually
//! be got wrong are cheapest to assert:
//!
//! * **the clock the lane is stamped with is the commit's, not the run's.** `graph_revisions()` is ordered
//!   `built_utc DESC`, so a backfill that stamped `now` would make every revision equally new and turn "the
//!   newest revision the graph holds" into a coin toss;
//! * **a base is only claimed when the lane is standing on it.** The first revision of a cold pass has no
//!   base under the cursor, so it is an anchor; the ones after it chain, and `graph_snapshot.kind` is where
//!   that claim is recorded — a delta that lies about its parent is the failure `reco-8s4`'s three schema
//!   findings exist to catch;
//! * **the cost bounds belong to the pass, not to the caller**: `graph.window` and `graph.max_corpus_mib`
//!   refuse from inside [`fathom_analyze::graph::revise::run`], so no caller can forget to apply them;
//! * **a module the extractor refused is a counted hole**, not a silently missing module and not a reason to
//!   lose the revision around it.
//!
//! Python fixtures throughout: `from pkg.mod import name` says one dependency per line, and the module
//! granularity `graph.module_granularity` picks for Python is the spelling the resolver resolves against,
//! so the edges here are the real mechanism rather than a hand-built graph.
#![allow(clippy::unwrap_used, clippy::expect_used)]

// The fixture module also carries helpers this file does not need; `graph_lane` and `deps_repository`
// do, and each test binary is compiled on its own, so "unused here" is not "dead".
#[allow(dead_code)]
mod common;

use common::Fixture;
use fathom_analyze::graph::{GraphPolicy, Lane, builder_from_config_text};
use fathom_analyze::index::{self, GraphPass, PHASE_GRAPH};
use fathom_ingest::record::Unclassified;
use fathom_ingest::repo::Repository;
use fathom_store::spine::Store;
use std::path::{Path, PathBuf};

/// The instant every run here is stamped with — a Tuesday in 2030, chosen to be *nothing like* the date the
/// fixture's commits carry (1_735_689_600, `tests/common`'s pinned epoch), because the one claim about the
/// clock that matters is "the lane is stamped with the commit's date, not with the run's", and a test where
/// those two are equal cannot tell them apart.
const NOW: i64 = 1_893_456_000_000;

/// The date every commit in `tests/common`'s fixture carries, in milliseconds: what each graph row must be
/// stamped with instead of `NOW`.
const COMMIT_UTC: i64 = 1_735_689_600_000;

/// One index run, assembled the way the CLI assembles it.
fn index(store: &Path, repo: &Path, graph: GraphPass) -> index::Report {
    let repository = Repository::discover(repo).expect("the fixture is a repository");
    let mut store = Store::open(store).expect("the store opens");
    let options = index::Options {
        now: Some(NOW),
        graph,
        ..index::Options::head()
    };
    index::run(&repository, &mut store, &Unclassified, &options).expect("the index run succeeds")
}

/// A store beside the fixture, so `.fathom/` lands where ADR-004 §3 says it does.
fn store_path(fixture: &Fixture) -> PathBuf {
    let dir = PathBuf::from(format!("{}.store", fixture.path().display()));
    std::fs::create_dir_all(&dir).expect("create the sibling store directory");
    dir.join("index.sqlite3")
}

/// The lane's rows as the store has them — newest-listed-first, since `graph_revisions` orders `built_utc
/// DESC` and the whole point of the stamp is that order.
fn rows(store: &Path) -> Vec<fathom_store::graph::SnapshotRow> {
    let store = Store::open(store).expect("reopen");
    store
        .graph_revisions()
        .expect("rows")
        .iter()
        .map(|tree| {
            store
                .graph_snapshot(tree)
                .expect("the row a listing named")
                .expect("the row is still there")
        })
        .collect()
}

/// A repository with three revisions: `core` on `util`, then `api` on `core`, then `util` reaching back to
/// `api`, so the newest revision has a cycle for the lane to hold.
fn fixture(label: &str) -> Fixture {
    let fixture = Fixture::new(label);
    fixture.commit(
        1,
        "add core on top of util",
        &[
            ("pkg/__init__.py", ""),
            ("pkg/util.py", "HELPER = 1\n"),
            (
                "pkg/core.py",
                "from pkg.util import HELPER\n\n\nclass Thing:\n    size = HELPER\n",
            ),
        ],
    );
    fixture.commit(
        2,
        "add the api on top of core",
        &[(
            "pkg/api.py",
            "from pkg.core import Thing\n\n\ndef serve():\n    return Thing()\n",
        )],
    );
    fixture.commit(
        3,
        "close the loop between util and api",
        &[(
            "pkg/util.py",
            "from pkg.api import serve\n\nHELPER = 2\n\n\ndef loop():\n    return serve()\n",
        )],
    );
    fixture
}

/// **One row per revision, stamped with that revision's own date.**
#[test]
fn one_graph_row_per_revision_and_the_clock_is_the_commits() {
    let fixture = fixture("one-row");
    let store = store_path(&fixture);
    let report = index(&store, fixture.path(), GraphPass::on(""));
    assert_eq!(report.graph.revisions, 3, "{report:?}");
    assert!(
        report.graph.ran,
        "the pass ran, so the JSON will print a section for it"
    );
    assert_eq!(report.graph.unavailable, 0, "{report:?}");
    assert!(
        report.graph.edges > 0,
        "the imports became edges: {report:?}"
    );
    assert!(
        report.phases.iter().any(|(phase, _)| phase == PHASE_GRAPH),
        "the pass is its own measured phase, so a slow index can be argued about: {:?}",
        report.phases
    );

    let rows = rows(&store);
    assert_eq!(rows.len(), 3, "three revisions, three rows");
    for row in &rows {
        assert_eq!(
            row.built_utc, COMMIT_UTC,
            "{} is stamped with its own commit's date, not with the instant the run happened",
            row.tree_oid
        );
    }
    // `graph_revisions()` is ordered `built_utc DESC`, which is the ordering every surface means by "the
    // newest revision the graph holds". A fixture whose commits share a date cannot exercise that ordering,
    // so the claim asserted here is the one the fixture can actually support: no row was stamped with `NOW`.
    assert!(rows.iter().all(|row| row.built_utc != NOW));
}

/// **A base is only claimed when the lane is standing on it.** A cold pass has nothing under the cursor for
/// its first revision; after that the chain is intact, and `graph_snapshot.kind` is the record of which is
/// which.
#[test]
fn a_cold_pass_anchors_once_and_chains_the_rest() {
    let fixture = fixture("chain");
    let store = store_path(&fixture);
    let report = index(&store, fixture.path(), GraphPass::on(""));
    assert_eq!(report.graph.anchors, 1, "{report:?}");
    // Which row is which is not asserted by position: `graph_revisions()` lists `built_utc DESC` and this
    // fixture's three commits share one date, so the listing order among them is the tree oid's. The set of
    // kinds and depths is the claim, and it is the one that says a chain was written rather than a pile.
    let rows = rows(&store);
    let kinds: Vec<&str> = rows.iter().map(|row| row.kind.as_str()).collect();
    assert_eq!(
        kinds.iter().filter(|kind| **kind == "full").count(),
        1,
        "exactly one anchor, and the cursor kept up from there: {kinds:?}"
    );
    assert_eq!(
        kinds.iter().filter(|kind| **kind == "delta").count(),
        2,
        "the other two revisions are deltas against the row before them: {kinds:?}"
    );
    let mut depths: Vec<i64> = rows.iter().map(|row| row.depth).collect();
    depths.sort_unstable();
    assert_eq!(
        depths,
        vec![0, 1, 2],
        "and each delta knows how far it is from an anchor"
    );
}

/// A warm run owes nothing, says nothing, and leaves the lane alone.
#[test]
fn a_warm_run_writes_nothing() {
    let fixture = fixture("warm");
    let store = store_path(&fixture);
    index(&store, fixture.path(), GraphPass::on(""));
    let before = rows(&store);
    let report = index(&store, fixture.path(), GraphPass::on(""));
    assert_eq!(report.graph.revisions, 0, "{report:?}");
    assert_eq!(report.graph.partial, None, "nothing owed is not a partial");
    assert_eq!(rows(&store).len(), before.len(), "the lane is untouched");
}

/// **The upgrade path.** A store written before this pass existed is complete and quiet: nothing is new, no
/// ref has moved, and the ordinary run has to backfill the lane anyway or every architecture surface keeps
/// refusing forever.
#[test]
fn a_quiet_run_backfills_a_lane_that_was_never_written() {
    let fixture = fixture("backfill");
    let store = store_path(&fixture);
    let cold = index(&store, fixture.path(), GraphPass::Off);
    assert!(!cold.graph.ran, "the pass was not asked for: {cold:?}");
    assert!(rows(&store).is_empty());

    let warm = index(&store, fixture.path(), GraphPass::on(""));
    assert!(warm.quiet, "nothing was new: {warm:?}");
    assert_eq!(warm.graph.revisions, 3, "and the lane was written anyway");
}

/// `graph.enabled = false` is the repository declining, and the pass — not the caller — is what honours it,
/// so a caller that forgot to check cannot write a lane into a repository that refused one.
#[test]
fn a_repository_that_switched_the_pass_off_writes_no_lane() {
    let fixture = fixture("disabled");
    let store = store_path(&fixture);
    let config = "[graph]\nenabled = false\n";
    let (policy, _) = GraphPolicy::from_repo_config_text(config);
    assert!(policy.disabled_reason().is_some());
    let report = index(&store, fixture.path(), GraphPass::on(config));
    assert_eq!(report.graph.revisions, 0, "{report:?}");
    assert!(
        rows(&store).is_empty(),
        "a refusal writes no rows: the lane stays empty rather than half-built"
    );
}

/// `graph.window` is a per-pass cost bound, and the revisions it left behind stay owed.
#[test]
fn a_window_caps_one_pass_and_leaves_the_rest_owed() {
    let fixture = fixture("window");
    let store = store_path(&fixture);
    let config = "[graph]\nwindow = 1\n";
    let report = index(&store, fixture.path(), GraphPass::on(config));
    assert_eq!(report.graph.revisions, 1, "{report:?}");
    let partial = report
        .graph
        .partial
        .expect("a capped pass states its partial");
    assert!(
        partial.contains("graph.window") && partial.contains("2"),
        "the sentence names the knob and what is left: {partial}"
    );
    // The cap takes the *newest*: a writer that spent every run on the oldest window would never arrive at
    // the revision anyone is asking about.
    assert_eq!(
        rows(&store)
            .first()
            .expect("the newest revision was the one written")
            .depth,
        0,
        "one revision, so one anchor"
    );

    for expected in [1, 1, 0] {
        let next = index(&store, fixture.path(), GraphPass::on(config));
        assert_eq!(next.graph.revisions, expected, "{next:?}");
    }
}

/// A module over `extract.max_file_bytes` is a hole that is counted and named: the revision is still
/// written, because a graph with one unknown module is still a graph.
#[test]
fn a_module_the_extractor_refuses_is_counted_not_guessed_at() {
    let fixture = Fixture::new("unavailable");
    fixture.commit(
        1,
        "a module and a giant",
        &[
            ("pkg/small.py", "VALUE = 1\n"),
            ("pkg/giant.py", &format!("x = {}\n", "9".repeat(5_000))),
        ],
    );
    let store = store_path(&fixture);
    // 16 bytes: `small.py` fits and `giant.py` does not. The pass honours the same knob the structure lane
    // honours, because a module it could not read means the same thing in both.
    let report = index(
        &store,
        fixture.path(),
        GraphPass::on("[extract]\nmax_file_bytes = 16\n"),
    );
    assert_eq!(
        report.graph.revisions, 1,
        "the revision is still written: {report:?}"
    );
    assert_eq!(report.graph.unavailable, 1, "{report:?}");
    assert!(
        report.graph.notices.iter().any(|n| n.contains("giant.py")),
        "the notice names the module: {:?}",
        report.graph.notices
    );
}

/// The corpus cap refuses *before* reading, and names itself: a slow surprise is how the next person turns
/// the whole lane off.
#[test]
fn a_corpus_over_the_cap_refuses_before_reading_it() {
    let fixture = Fixture::new("corpus");
    fixture.commit(
        1,
        "a corpus bigger than the cap",
        &[
            ("pkg/a.py", &format!("a = {}\n", "1".repeat(600_000))),
            ("pkg/b.py", &format!("b = {}\n", "2".repeat(600_000))),
        ],
    );
    let store = store_path(&fixture);
    let report = index(
        &store,
        fixture.path(),
        GraphPass::on("[graph]\nmax_corpus_mib = 1\n"),
    );
    assert_eq!(report.graph.revisions, 0, "{report:?}");
    let partial = report.graph.partial.expect("a refused pass states why");
    assert!(
        partial.contains("graph.max_corpus_mib"),
        "the refusal names the knob: {partial}"
    );
    assert!(
        rows(&store).is_empty(),
        "and nothing was written while it was refused"
    );
}

/// `graph.ignored_dirs` is honoured before anything is read, so a vendored tree does not become four
/// thousand modules.
#[test]
fn ignored_directories_are_never_read_at_all() {
    let fixture = Fixture::new("ignored");
    fixture.commit(
        1,
        "a vendored tree and a real one",
        &[
            ("pkg/real.py", "VALUE = 1\n"),
            ("vendor/dep/index.js", "export const dep = 1;\n"),
            ("vendor/dep/more.js", "export const more = 2;\n"),
        ],
    );
    let store = store_path(&fixture);
    let report = index(&store, fixture.path(), GraphPass::on(""));
    assert!(
        report.graph.skipped >= 2,
        "the vendored files were never modules: {report:?}"
    );
}

/// The `[layers]` table reaches the lane through the config text the pass was handed — which is the whole
/// reason [`GraphPass`] carries the text rather than a pre-built builder: the CLI is the only crate allowed to
/// open `fathom.toml`, and this is the crate that has to obey it. A layer tag that was never written is a
/// layer question that answers empty, so this is the difference between the architecture surfaces being
/// configurable and being decorative.
#[test]
fn the_layer_tags_come_from_the_config_text_the_pass_was_handed() {
    let config = "[layers]\napi = [\"pkg/api.py\"]\nbase = [\"pkg/util.py\"]\n";
    let fixture = fixture("layers");
    let store = store_path(&fixture);

    // First, with no `[layers]`: every node is untagged, and the pass says that is why rather than letting
    // an empty layer question read as a clean answer.
    let untagged = index(&store, fixture.path(), GraphPass::on(""));
    assert_eq!(untagged.graph.revisions, 3);
    assert!(
        untagged
            .graph
            .notices
            .iter()
            .any(|notice| notice.contains("[layers]")),
        "no [layers] is a stated absence: {:?}",
        untagged.graph.notices
    );
    let mut bare = Store::open(&store).expect("reopen");
    let naming = Lane::new(&mut bare, builder_from_config_text("").0)
        .expect("the lane opens")
        .naming()
        .expect("the dictionary reads back");
    assert!(
        naming.vertices().all(|vertex| vertex.layer.is_empty()),
        "nothing was tagged, so nothing claims a layer"
    );
    drop(naming);

    // A refresh re-derives the lane under the tags, which is the documented way a `[layers]` edit reaches a
    // lane that already exists: derived data is recomputed, not patched (ADR-004 §9.5).
    let options = index::Options {
        now: Some(NOW),
        refresh: true,
        graph: GraphPass::on(config),
        ..index::Options::head()
    };
    let repository = Repository::discover(fixture.path()).expect("the fixture is a repository");
    let mut store = Store::open(&store).expect("reopen");
    index::run(&repository, &mut store, &Unclassified, &options).expect("the refresh ran");

    let lane = Lane::new(&mut store, builder_from_config_text(config).0).expect("the lane opens");
    let tagged: Vec<String> = lane
        .naming()
        .expect("the dictionary reads back")
        .vertices()
        .filter(|vertex| !vertex.layer.is_empty())
        .map(|vertex| format!("{}={}", vertex.key, vertex.layer))
        .collect();
    assert!(
        tagged.iter().any(|row| row.ends_with("=api")),
        "the module the config tagged came back tagged: {tagged:?}"
    );
    assert!(
        tagged.iter().any(|row| row.ends_with("=base")),
        "{tagged:?}"
    );
}

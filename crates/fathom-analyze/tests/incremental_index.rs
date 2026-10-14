//! `e1.4`'s acceptance criteria, measured (ADR-004 §6, §9.2; ticket `reco-f28`).
//!
//! Four things had to be true, and three of them are numbers:
//!
//! * a warm re-index after one new commit fits inside `benchmarks/budgets.json`'s wall clock, on the
//!   500-commit fixture, *and the counters show why it is fast* rather than merely being fast;
//! * editing one file invalidates only the analyses that consume that file — asserted as per-analyzer
//!   counters, because "we believe the keys are per-file" is not evidence;
//! * a `kill -9` in the middle of indexing leaves a store the next run finishes rather than one it has to
//!   explain;
//! * `fathom index --stats` can print hit/miss counts per analyzer, which is the same counters, printed.
//!
//! Everything here goes through the same entry point the CLI uses ([`index::run`]), because a cache that is
//! only warm when driven by a test harness is not warm.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Fixture, repository};
use fathom_analyze::cancel::Cancel;
use fathom_analyze::index::run as index_run;
use fathom_analyze::index::{self, Interrupted, Scope};
use fathom_core::error::Diagnosable;
use fathom_ingest::record::Unclassified;
use fathom_ingest::repo::Repository;
use fathom_store::cache::{Config, Versions};
use fathom_store::spine::Store;
use serde_json::json;
use std::path::{Path, PathBuf};

/// The instant every fixture run stamps with. Any fixed value works; this one is a Tuesday.
const NOW: i64 = 1_735_689_600_000;

/// One index run, from the same parts the CLI assembles: a repository, a store file, the default taxonomy.
fn index(store: &Path, repo: &Path, options: &index::Options) -> index::Report {
    let repository = Repository::discover(repo).expect("the fixture is a repository");
    let mut store = Store::open(store).expect("the store opens");
    index_run(&repository, &mut store, &Unclassified, options).expect("the index run succeeds")
}

/// Options that walk `HEAD` and stamp `NOW`.
fn options() -> index::Options {
    index::Options {
        now: Some(NOW),
        ..index::Options::head()
    }
}

/// Read the counters back out of the store, the way `fathom stats` will.
fn counters(store: &Path) -> std::collections::BTreeMap<String, i64> {
    let store = Store::open(store).expect("reopen");
    store.counters("cache.").expect("counters read")
}

/// A store path beside a fixture, so `.fathom/index.sqlite3` and `objects/` land where §3 says they do.
fn store_path(fixture: &Fixture) -> PathBuf {
    let dir = PathBuf::from(format!("{}.store", fixture.path().display()));
    std::fs::create_dir_all(&dir).expect("create the sibling store directory");
    dir.join("index.sqlite3")
}

/// A budget out of `benchmarks/budgets.json`, read rather than copied so the test cannot drift from the
/// file CI treats as normative. `small` is the 500-commit class, which is this fixture.
fn budget(id: &str) -> u128 {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/budgets.json"),
    )
    .expect("benchmarks/budgets.json is in the tree");
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("it is JSON");
    let budgets = parsed["budgets"].as_array().expect("a budget list");
    let found = budgets
        .iter()
        .find(|entry| entry["id"] == json!(id))
        .unwrap_or_else(|| panic!("{id} is not in benchmarks/budgets.json"));
    u128::from(found["budget"].as_u64().expect("a budget is a number"))
}

#[test]
fn a_warm_reindex_after_one_new_commit_looks_up_one_commit_and_computes_one_analysis() {
    // Acceptance criterion one, the part that is not a wall clock: five hundred commits, then one more, and
    // the second run must be able to *say* why it was fast. The milliseconds have their own ignored
    // release-only test below, for the reason `fathom-ingest`'s bench test gives — a debug build is three
    // to ten times slower than the release build the budget was written against, so asserting 150 ms here
    // would be asserting the optimiser was awake.
    let fixture = repository("warm", 500);
    let store = store_path(&fixture);

    let cold = index(&store, fixture.path(), &options());
    assert_eq!(cold.kind, fathom_ingest::ingest::RunKind::Cold);
    assert_eq!(cold.commits_walked, 500);
    assert_eq!(cold.commits_new, 500);
    assert!(
        cold.spine.failures.is_empty(),
        "the cold run degraded: {:?}",
        cold.spine.failures
    );

    // One more commit, touching files the previous 499 already knew about.
    fixture.edit(
        "src/lib.rs",
        "// revision 500\nfn new_half() -> u32 { 500 }\n",
    );

    let warm = index(&store, fixture.path(), &options());
    eprintln!(
        "cold index ms: {}, warm index ms: {}",
        cold.elapsed.as_millis(),
        warm.elapsed.as_millis()
    );
    assert_eq!(warm.kind, fathom_ingest::ingest::RunKind::Warm);
    assert_eq!(warm.commits_new, 1, "one commit was added");
    assert_eq!(
        warm.commits_considered, 1,
        "and exactly one commit's analyses were looked up — that is §6.1 R1"
    );
    // The counters are the proof. A run that was fast because it re-derived everything would show it here,
    // and this is the assertion that stops the wall clock from being satisfied by a machine with a fast disk.
    let per_file = &warm.analyzers["diff.per_file"];
    assert_eq!(
        per_file.cold, 1,
        "the one file the commit touched is the one analysis that missed: {per_file:?}"
    );
    assert_eq!(per_file.puts, 1, "and it was written once");
    let structure = &warm.analyzers["structure.delta"];
    assert_eq!(
        structure.cold, 1,
        "the new tree is a new key, and one key only: {structure:?}"
    );
    let spine = &warm.analyzers["ingest.normalize"];
    assert_eq!(spine.lookups, 501, "every commit was looked up");
    assert_eq!(spine.hits, 500, "and 500 of them were already the spine's");

    // And the whole-store totals make the same point over the run's lifetime. A warm run over one commit
    // performs exactly one `path_pair` lookup, so it cannot show thousands of hits — what it can show, and
    // what a run that was merely fast would fail, is that the cold run's rows are all still there and the
    // store gained exactly the one new row this commit needed.
    let totals = counters(&store);
    assert_eq!(
        totals["cache.put.diff.per_file"],
        cold.analyzers["diff.per_file"].puts + warm.analyzers["diff.per_file"].puts,
        "the warm run added one row and rewrote none of the cold run's: {totals:?}"
    );
    assert!(
        totals["cache.put.diff.per_file"] > 500,
        "the cold run wrote roughly a path-pair row per changed path across 500 commits, and those are the          rows the warm run did not recompute: {}",
        totals["cache.put.diff.per_file"]
    );
}

#[test]
fn a_run_with_nothing_new_looks_up_nothing_and_computes_nothing() {
    // §6.1 R1's other half, and the shape `warm.reindex.noop.ms.small` is written for. A warm run with no
    // new commit must not fall back to "check everything": zero lookups is the claim, and a driver that
    // re-walked the history to discover nothing had changed would report hundreds here.
    let fixture = repository("noop", 6);
    let store = store_path(&fixture);
    index(&store, fixture.path(), &options());

    let again = index(&store, fixture.path(), &options());
    assert_eq!(again.kind, fathom_ingest::ingest::RunKind::Warm);
    assert_eq!(again.commits_new, 0);
    assert_eq!(
        again.commits_considered, 0,
        "no commit was looked at for analysis"
    );
    for id in ["diff.per_file", "structure.delta"] {
        let stats = &again.analyzers[id];
        assert_eq!(
            stats.lookups, 0,
            "{id} was not asked anything at all: {stats:?}"
        );
        assert_eq!(stats.puts, 0);
        assert!(
            stats.hit_rate().is_none(),
            "no lookups means no hit rate to print: {stats:?}"
        );
    }
    let spine = &again.analyzers["ingest.normalize"];
    assert_eq!((spine.lookups, spine.hits, spine.cold), (6, 6, 0));
}

#[test]
#[ignore = "builds a 500-commit repository and measures wall clock; run with --release --ignored"]
fn a_warm_reindex_after_one_commit_fits_the_budget() {
    // Acceptance criterion one's wall clock, in the only build the budget in `benchmarks/budgets.json`
    // means anything for. `size_classes.small` *is* this fixture — "the 500-commit fixture of e1.4" — and
    // `warm.reindex.ms.small` is a p95, so the honest thing is one run per process, ignored by default,
    // and the number printed so a regression can be read rather than guessed.
    let fixture = repository("warm-budget", 500);
    let store = store_path(&fixture);
    index(&store, fixture.path(), &options());
    fixture.edit(
        "src/lib.rs",
        "// revision 500\nfn new_half() -> u32 { 500 }\n",
    );

    let warm = index(&store, fixture.path(), &options());
    let elapsed = warm.elapsed.as_millis();
    let limit = budget("warm.reindex.ms.small");
    eprintln!("warm re-index: {elapsed} ms against a {limit} ms p95");
    assert!(
        elapsed < limit,
        "warm re-index of the 500-commit fixture took {elapsed} ms, over its {limit} ms budget"
    );
    assert_eq!(
        warm.analyzers["diff.per_file"].cold, 1,
        "and it was fast for the right reason"
    );
}

#[test]
fn editing_one_file_invalidates_only_the_analyses_that_consume_that_file() {
    // Acceptance criterion two, asserted by counters rather than by wall clock. Three commits over three
    // files; then one file changes; then a `--refresh` pass looks up *every* analysis the store holds.
    // Exactly one lookup may miss.
    let fixture = repository("edit", 4);
    let store = store_path(&fixture);
    index(&store, fixture.path(), &options());

    fixture.edit(
        "src/main.rs",
        "// the edited one\nfn edited() -> u32 { 1 }\n",
    );
    let warm = index(&store, fixture.path(), &options());
    assert_eq!(warm.analyzers["diff.per_file"].cold, 1);

    let refresh = index(
        &store,
        fixture.path(),
        &index::Options {
            scope: Scope::Everything,
            ..options()
        },
    );
    let per_file = &refresh.analyzers["diff.per_file"];
    assert!(
        per_file.lookups > 5,
        "a refresh looks up every stored unit, not just the new one: {per_file:?}"
    );
    assert_eq!(
        per_file.cold, 0,
        "nothing is cold after the warm run has already filled it"
    );
    assert_eq!(
        per_file.bumped, 0,
        "and nothing moved: no version, config or schema changed between these runs"
    );
    assert_eq!(
        per_file.puts, 0,
        "so nothing was computed — the whole refresh was cache reads"
    );
    assert_eq!(
        refresh.analyzers["structure.delta"].puts, 0,
        "including the tree-scoped ones"
    );
}

#[test]
fn a_move_is_one_change_and_not_a_delete_wearing_a_mustache() {
    // The reason the analysis pass reads the spine's `git_commit_file` rows instead of comparing two trees
    // a second time. Rename detection lives in `fathom-ingest`'s normalize, so a warm path driven by a raw
    // tree comparison would see `git mv` as one path vanishing and another appearing — scoring a moved file
    // as its full size in churn, which is a wrong answer that no budget can make right.
    let fixture = repository("rename", 3);
    let store = store_path(&fixture);
    index(&store, fixture.path(), &options());

    fixture.rename("src/main.rs", "src/moved.rs");
    let warm = index(&store, fixture.path(), &options());
    assert_eq!(warm.commits_new, 1);
    assert_eq!(
        warm.analyzers["diff.per_file"].cold, 1,
        "one moved path is one analysis, not an add and a delete: {:?}",
        warm.analyzers["diff.per_file"]
    );

    // And the row says what it was, which is what lets a consumer tell a move from a churn spike. Named by
    // `git rev-parse HEAD` rather than by "the last row", because `git_commit` is stored in hex-id order,
    // which is the export's order and has nothing to do with chronology.
    let tip = fixture.head();
    let store = Store::open(&store).expect("reopen");
    let kinds: Vec<(String, Option<String>, String)> = store
        .connection()
        .prepare(
            "SELECT new_path, old_path, change_kind FROM git_commit_file WHERE commit_oid = ?1",
        )
        .expect("the file rows of the tip commit")
        .query_map([tip], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("read the file rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("one row per changed path");
    assert_eq!(
        kinds,
        vec![(
            "src/moved.rs".to_owned(),
            Some("src/main.rs".to_owned()),
            "rename".to_owned()
        )],
        "one row, both names, and the kind that says so"
    );
}

#[test]
fn an_analyzer_version_bump_moves_that_analyzer_and_everything_downstream_of_it() {
    // §6's rows three and four, measured. `diff.per_file` goes to 99; `structure.delta` depends on it, so
    // its `dep_digest` moves too; `ingest.normalize` is upstream of both and must not move at all.
    let fixture = repository("bump", 4);
    let store = store_path(&fixture);
    index(&store, fixture.path(), &options());

    let refresh = index(
        &store,
        fixture.path(),
        &index::Options {
            scope: Scope::Everything,
            versions: Versions::current().bumped("diff.per_file", 99),
            ..options()
        },
    );
    assert!(
        refresh.analyzers["diff.per_file"].bumped > 0,
        "the bumped analyzer's rows are all unreachable: {:?}",
        refresh.analyzers["diff.per_file"]
    );
    assert!(
        refresh.analyzers["structure.delta"].bumped > 0,
        "the bump cascades to what depends on it: {:?}",
        refresh.analyzers["structure.delta"]
    );
    assert_eq!(
        refresh.analyzers["ingest.normalize"].bumped, 0,
        "and does not cascade backwards"
    );

    // A second run at the same versions is warm again: the bump cost one recompute, not a broken cache.
    let again = index(
        &store,
        fixture.path(),
        &index::Options {
            scope: Scope::Everything,
            versions: Versions::current().bumped("diff.per_file", 99),
            ..options()
        },
    );
    assert_eq!(again.analyzers["diff.per_file"].puts, 0);
    assert_eq!(
        again.analyzers["diff.per_file"].hits,
        again.analyzers["diff.per_file"].lookups
    );
}

#[test]
fn a_config_change_moves_only_the_analyzer_that_declared_the_key() {
    // §6's row five. `diff.context_lines` is declared by `diff.per_file` and by nobody else, so a change
    // to it must not touch `structure.delta` — which depends on `diff.per_file`, and whose rows therefore
    // stay reachable even though the analyzer below them was recomputed (§5.5's cascade is config's, not
    // the dependency graph's).
    let fixture = repository("config", 4);
    let store = store_path(&fixture);
    index(&store, fixture.path(), &options());

    let refresh = index(
        &store,
        fixture.path(),
        &index::Options {
            scope: Scope::Everything,
            config: Config(json!({ "diff": { "context_lines": 9 } })),
            ..options()
        },
    );
    assert!(
        refresh.analyzers["diff.per_file"].bumped > 0,
        "the analyzer that declared the key moves: {:?}",
        refresh.analyzers["diff.per_file"]
    );
    assert_eq!(
        refresh.analyzers["structure.delta"].bumped, 0,
        "the one that did not does not: {:?}",
        refresh.analyzers["structure.delta"]
    );
}

#[test]
fn a_store_schema_bump_makes_every_row_unreachable_without_migrating_any_of_them() {
    // §6's row eleven and §9.5: a schema migration is not a data migration. Field 9 of the key is the
    // store's own version, so a bump is the whole invalidation.
    let fixture = repository("schema", 4);
    let store = store_path(&fixture);
    index(&store, fixture.path(), &options());

    let refresh = index(
        &store,
        fixture.path(),
        &index::Options {
            scope: Scope::Everything,
            versions: Versions::current().with_store_schema(99),
            ..options()
        },
    );
    for id in ["diff.per_file", "structure.delta"] {
        let stats = &refresh.analyzers[id];
        assert!(stats.bumped > 0, "{id} should be unreachable: {stats:?}");
        assert_eq!(stats.hits, 0, "{id} should not have found anything");
    }
}

#[test]
fn a_run_killed_mid_index_leaves_a_usable_store_that_the_next_run_completes() {
    // Acceptance criterion three, with a real `SIGKILL`: the child indexes until it has written a few
    // cache rows, then parks, and the parent kills it — the same signal `kill -9` sends, at a point in the
    // middle of durable work rather than before or after it.
    const CRASH_ENV: &str = "FATHOM_CRASH_INDEX_STORE";
    const CRASH_REPO_ENV: &str = "FATHOM_CRASH_INDEX_REPO";
    if let Some(path) = std::env::var_os(CRASH_ENV) {
        // The child's whole job is to get partway through an index and then stop responding to anything but
        // the signal the parent is about to send. It has until the parent sees the store file grow.
        let options = index::Options {
            park_after_units: Some(4),
            ..options()
        };
        let repo_path = std::env::var_os(CRASH_REPO_ENV).unwrap();
        let repository = Repository::discover(Path::new(&repo_path))
            .expect("child: the fixture is a repository");
        let mut store = Store::open(Path::new(&path)).expect("child: the store opens");
        let _ = index_run(&repository, &mut store, &Unclassified, &options);
        unreachable!("the driver parks instead of returning");
    }
    let fixture = repository("kill", 12);
    let store = store_path(&fixture);

    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("a_run_killed_mid_index_leaves_a_usable_store_that_the_next_run_completes")
        .arg("--nocapture")
        .env(CRASH_ENV, &store)
        .env(CRASH_REPO_ENV, fixture.path())
        .spawn()
        .expect("spawning the test binary as a child");
    // Wait until the child has clearly begun writing rows, then kill it while it is mid-run. The margin is
    // generous because the alternative — killing at a guessed instant — could just as well test startup.
    std::thread::sleep(std::time::Duration::from_millis(750));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the child finished instead of parking, so it never reached its crash point"
    );
    child.kill().expect("SIGKILL");
    let status = child.wait().expect("reaping the killed child");
    assert!(
        !status.success(),
        "a killed process must not look like a clean exit"
    );

    // The store is *usable*: it opens, it passes its own integrity check, and it says what happened.
    {
        let reopened = Store::open(&store).expect("the store opens after the kill");
        reopened.check().expect("the store is intact");
        let running: i64 = reopened
            .connection()
            .query_row(
                "SELECT count(*) FROM index_run WHERE status = 'running'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            running, 1,
            "the dead run is still marked running until someone notices"
        );
    }

    // The next run recovers — it stamps the dead one `crashed` — and completes to the same content a
    // single clean run would have produced. `content_digest` projects out the volatile bookkeeping, so
    // "the same store" here means the same commits, file rows, cache rows and payloads.
    let finished = index(&store, fixture.path(), &options());
    assert!(!finished.interrupted, "the resumed run finishes");
    {
        let reopened = Store::open(&store).expect("reopen");
        let crashed: i64 = reopened
            .connection()
            .query_row(
                "SELECT count(*) FROM index_run WHERE status = 'crashed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(crashed, 1, "recovery noticed the dead run (§9.2)");
    }

    let clean = repository("kill-clean", 12);
    let clean_store = store_path(&clean);
    index(&clean_store, clean.path(), &options());
    let recovered = Store::open(&store).unwrap();
    let from_scratch = Store::open(&clean_store).unwrap();
    assert_eq!(
        recovered.content_digest().unwrap(),
        from_scratch.content_digest().unwrap(),
        "a store finished after a kill holds the same derived content as one that never was"
    );
}

#[test]
fn cancellation_and_the_unit_budget_are_stated_partials_not_failures() {
    // §7.6: both are ways for a run to stop having done less, and to say so.
    let fixture = repository("partial", 6);
    let store = store_path(&fixture);

    let cancel = Cancel::new();
    cancel.trigger();
    let report = index(
        &store,
        fixture.path(),
        &index::Options {
            cancel: Some(cancel.clone()),
            ..options()
        },
    );
    assert!(report.interrupted);
    assert_eq!(report.reason, Some(Interrupted::Cancelled));
    assert_eq!(
        report.computed(),
        0,
        "a cancel set before the run means no analysis was computed at all"
    );
    // The run still left a usable store, and the spine is complete: cancellation stops the analyses, it
    // does not abandon half a commit queue.
    assert_eq!(report.commits_new, 6);
    let _ = cancel;

    let budgeted = index(
        &store,
        fixture.path(),
        &index::Options {
            unit_budget: Some(2),
            // `Everything` so there is more work than the budget allows.
            scope: Scope::Everything,
            ..options()
        },
    );
    assert!(budgeted.interrupted, "a budget that ran out says so");
    assert_eq!(budgeted.reason, Some(Interrupted::Budget));
    assert!(
        budgeted.computed() <= 2,
        "and did not overshoot it: {}",
        budgeted.computed()
    );
    // A run whose error would be `Cancelled` is the CLI's job; the library's job is to report it as data.
    assert_eq!(
        fathom_core::error::ErrorCode::Cancelled
            .to_string()
            .to_ascii_uppercase(),
        "CANCELLED"
    );
}

#[test]
fn an_uncommitted_path_is_analysed_and_its_result_is_the_committed_one() {
    // §6.4: the provisional record and the committed record share a key, because the identity of an
    // uncommitted file is the blob id its bytes already hash to. So committing it is a hit, not a re-run.
    let fixture = repository("worktree", 3);
    let store = store_path(&fixture);
    index(&store, fixture.path(), &options());

    // `src/main.rs` and not some new path, because the worktree pass reads *tracked* paths: an untracked
    // file is `UntrackedNotScanned`, which is a different rule and a different test.
    fixture.dirty(
        "src/main.rs",
        "// uncommitted work\nfn staged_soon() -> u32 { 7 }\n",
    );
    let provisional = index(&store, fixture.path(), &options());
    assert_eq!(
        provisional.worktree.len(),
        1,
        "the dirty path was analysed: {:?}",
        provisional.worktree
    );
    assert_eq!(
        provisional.worktree[0].reason,
        fathom_store::cache::Reason::Cold,
        "and it had never been seen before"
    );
    assert_eq!(
        (
            provisional.analyzers["diff.per_file"].lookups,
            provisional.analyzers["diff.per_file"].cold
        ),
        (1, 1),
        "the one lookup in this run *is* the working file's: the commit pass had nothing new to ask, and a          provisional lookup is a real lookup of the same analyzer by the same key — which is what §6.4          means by one key space rather than two"
    );

    // Commit it. The committed analysis is the *same key*, so it is already there.
    fixture.edit(
        "src/main.rs",
        "// uncommitted work\nfn staged_soon() -> u32 { 7 }\n",
    );
    let committed = index(&store, fixture.path(), &options());
    assert_eq!(committed.commits_new, 1);
    assert_eq!(
        committed.analyzers["diff.per_file"].cold, 0,
        "§6.4 R6: the provisional analysis was reused, not thrown away: {:?}",
        committed.analyzers["diff.per_file"]
    );

    // A staged path is the same story on the other side of the index.
    fixture.stage(
        "src/ingest.rs",
        "// staged\nfn already_there() -> u32 { 3 }\n",
    );
    let staged = index(&store, fixture.path(), &options());
    assert_eq!(staged.worktree.len(), 1);
    assert!(
        staged.worktree[0].staged,
        "and it knows which side of the index it came from"
    );
}

#[test]
fn a_working_file_too_large_to_hash_is_stated_rather_than_guessed() {
    // §7.6 again: a path the scan would not read has no identity to key on, and "no identity" is reported
    // as a skip rather than being answered with a guess.
    let fixture = repository("oversized", 3);
    let store = store_path(&fixture);
    index(&store, fixture.path(), &options());
    fixture.dirty("src/main.rs", &"line\n".repeat(4_000));

    let report = index(
        &store,
        fixture.path(),
        &index::Options {
            scan: fathom_ingest::state::ScanOptions {
                max_file_bytes: 64,
                ..Default::default()
            },
            ..options()
        },
    );
    assert!(report.worktree.is_empty(), "nothing was analysed");
    assert!(
        !report.worktree_skipped.is_empty(),
        "and the reason is in the report: {:?}",
        report.worktree_skipped
    );
}

#[test]
fn a_run_refuses_to_invent_its_own_instant() {
    // ADR-002 §4.4: the clock belongs to the CLI. A driver that read it here would make every golden test
    // in the workspace time-dependent.
    let fixture = repository("clock", 2);
    let store = store_path(&fixture);
    let repository = Repository::discover(fixture.path()).unwrap();
    let mut store = Store::open(&store).unwrap();
    let err = index_run(
        &repository,
        &mut store,
        &Unclassified,
        &index::Options::default(),
    )
    .expect_err("a run with no instant must refuse");
    assert_eq!(err.code(), fathom_core::error::ErrorCode::Usage);
    assert!(err.hint().is_some(), "and it says what to do about it");
}

#[test]
fn stats_are_per_run_and_are_the_numbers_the_store_holds() {
    // Acceptance criterion four, at the library level: the per-analyzer table is a delta of the store's
    // own counters, so `--stats` and `fathom stats` cannot tell different stories.
    let fixture = repository("stats", 4);
    let store = store_path(&fixture);
    let first = index(&store, fixture.path(), &options());
    let second = index(
        &store,
        fixture.path(),
        &index::Options {
            scope: Scope::Everything,
            ..options()
        },
    );
    assert_eq!(
        second.analyzers["diff.per_file"].lookups, first.analyzers["diff.per_file"].puts,
        "the refresh looked up exactly what the first run wrote, no more"
    );
    assert_eq!(
        second.analyzers["diff.per_file"].hits,
        second.analyzers["diff.per_file"].lookups
    );
    let totals = counters(&store);
    assert!(
        totals["cache.hit.diff.per_file"] >= first.analyzers["diff.per_file"].puts,
        "and the store has been counting the same hits all along: {totals:?}"
    );
}

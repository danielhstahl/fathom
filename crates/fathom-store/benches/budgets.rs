//! ADR-004 §11's budgets, as criterion benches (`e0.11`, `reco-pef`).
//!
//! Run it with:
//!
//! ```text
//! make bench-criterion            # or:
//! cargo bench -p fathom-store --features sqlite-bundled -- --sample-size 20 --measurement-time 0.5
//! ```
//!
//! `make bench` runs this and then compares the percentiles against `benchmarks/budgets.json` and
//! against the committed baseline in `benchmarks/baselines/perf-gate.json`; nothing in this file
//! asserts anything, because a bench that asserts its own threshold has two places where a budget
//! lives (`benchmarks/BUDGETS.md` explains why that is the failure mode). What this file does is
//! *measure the store-side primitives* the budget families name, at a corpus shape, and leave the
//! judgement to the gate.
//!
//! # Which budgets are here, and which are not
//!
//! Here: the reads a keystroke actually makes (`query.timeline.ms`, `query.card.ms`,
//! `query.frame_store_budget.ms`) and the write discipline behind every index (`write.batch_rows`,
//! whose throughput §13.1 measured at 444 k rows/s). Those are the budgets whose unit is
//! microseconds-to-milliseconds and whose variance criterion exists to characterise.
//!
//! Not here: anything that needs git, a terminal, or a process. Cold index, warm re-index,
//! diff-viewer open time and peak RSS are end-to-end properties of a *run*, and a resident set size in
//! particular cannot be measured from inside the process being measured — those come from the scripted
//! harness in `tools/perf_gate.py`, which shells out and samples `VmHWM` from outside. The two halves
//! are merged into one report by the gate, so a regression in either turns the same build red.
//!
//! # Corpus shape
//!
//! `FATHOM_BENCH_COMMITS` chooses the corpus, default 1 000 — the `small` class of §11.1. The store is
//! built in bench *setup*, outside the timed region: what is timed is one read, or one batched write,
//! never the fixture. Rows are written through the store's own API rather than through git for the same
//! reason `tests/episodes.rs` does it that way: at this layer the contract is about rows.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// A bench's output is its numbers, and criterion writes them to stdout; suppressing that would mean
// reading a file to see what just ran.
#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, Throughput};
use fathom_store::episode::{EpisodeQuery, EpisodeRow};
use fathom_store::spine::{CommitRow, FileRow, Store, Trigger};

/// The epoch every fixture row is stamped from. Fixed, so two runs are measuring the same dates.
const T0: i64 = 1_700_000_000_000;

/// Files per commit. Six is what the fixture corpus averages for a real agent turn: a change to code,
/// its test, and the odds and ends around them. It matters because the store's cost per commit is a
/// row count, not a byte count.
const FILES_PER_COMMIT: usize = 6;

/// The rail's page size — `e1.6`'s rail asks for 100 episodes at a time, which is what
/// `query.timeline.ms` is a budget for.
const PAGE: i64 = 100;

/// The rows-per-transaction cap of ADR-004 §7.3 (`write.batch_rows`). A batch at the cap is the
/// interesting shape: it is the biggest single transaction the writer is allowed to make.
const BATCH_ROWS: usize = 5_000;

fn commit_count() -> usize {
    std::env::var("FATHOM_BENCH_COMMITS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &usize| *value > 0)
        .unwrap_or(1_000)
}

/// The nth fixture oid, 40 hex characters the way git writes them.
fn oid(n: usize) -> String {
    format!("{:040x}", n)
}

/// A commit row touching [`FILES_PER_COMMIT`] paths, authored by one of four authors so the store's
/// author-kind columns have something to group over.
fn commit(n: usize) -> CommitRow {
    CommitRow {
        oid: oid(n),
        tree_oid: format!("{:040x}", n + 1),
        parent_oids: if n == 0 { Vec::new() } else { vec![oid(n - 1)] },
        author_name: format!("agent-{}", n % 4),
        author_email: format!("agent-{}@example.invalid", n % 4),
        author_utc: T0 + (n as i64) * 60_000,
        committer_utc: T0 + (n as i64) * 60_000,
        message_head: format!("feat: step {n}"),
        message_hash: format!("{:064x}", n),
        trailers: BTreeMap::new(),
        n_files: FILES_PER_COMMIT as i64,
        n_add: 10,
        n_del: 2,
        n_binary: 0,
        oversized: false,
        files: (0..FILES_PER_COMMIT)
            .map(|index| FileRow {
                path_hash: format!("{:064x}", index + n * FILES_PER_COMMIT),
                new_path: format!("src/module{index}/part{}.rs", n % 97),
                old_path: None,
                change_kind: "modify".to_owned(),
                mode_old: Some(0o100644),
                mode_new: Some(0o100644),
                is_binary: false,
                n_add: 10,
                n_del: 2,
                hunk_headers: vec!["@@ -1,2 +1,4 @@".to_owned()],
                blob_old: Some(format!("{:040x}", n.saturating_sub(1))),
                blob_new: Some(format!("{:040x}", n)),
                path_class: "source".to_owned(),
                taxonomy_version: 1,
                noise_class: "informative".to_owned(),
            })
            .collect(),
    }
}

/// One episode covering commits `[first, last]` — the shape `e1.3`'s cascade produces: a turn is a
/// run of commits by one author, not a single commit.
fn episode(first: usize, last: usize) -> EpisodeRow {
    let members: Vec<String> = (first..=last).map(oid).collect();
    EpisodeRow {
        id: format!("ep-{:0>6}", first),
        fingerprint: format!("{:064x}", first),
        fallback_fingerprint: format!("{:064x}", first + 7),
        kind: "committed".to_owned(),
        lineage_ref: "refs/heads/main".to_owned(),
        lineage_seq: 0,
        created_utc: T0,
        created_source: "index".to_owned(),
        created_confidence: 0.7,
        boundary_rule: "author".to_owned(),
        boundary_confidence: 0.75,
        author_kind: "agent".to_owned(),
        author_confidence: 0.85,
        first_oid: members.first().cloned().unwrap_or_default(),
        last_oid: members.last().cloned().unwrap_or_default(),
        members,
        n_files: FILES_PER_COMMIT as i64,
        n_add: 20,
        n_del: 4,
        first_utc: T0,
        last_utc: T0,
        author_name: "agent-0".to_owned(),
        author_email: "agent-0@example.invalid".to_owned(),
        summary: "feat: the thing".to_owned(),
        live: true,
        ..EpisodeRow::default()
    }
}

/// The store file, deleted when the bench ends. A bench that left its database behind would fill a
/// runner's disk one corpus at a time, and a bench that crashed would leave it forever, so cleanup is
/// a `Drop` rather than the last line of `main`.
struct BenchDir(std::path::PathBuf);

impl BenchDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fathom-budgets-{}-{}",
            std::process::id(),
            // Distinct per corpus size, so two `FATHOM_BENCH_COMMITS` runs in a row cannot read each
            // other's leftover pages and call them a warm cache.
            commit_count()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create the bench store directory");
        Self(path)
    }

    fn store_path(&self) -> std::path::PathBuf {
        self.0.join("index.sqlite3")
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A store holding `count` commits and `count / 2` episodes, on disk in a temporary directory rather
/// than in memory.
///
/// On disk is not a detail: the budgets are charged against a store a real process opens, whose pages
/// come through the OS page cache, and an in-memory database measures a different thing (it never
/// touches a file, and `rss.sqlite_page_cache.mib` is a budget about pages on a file).
fn seeded(count: usize) -> (Store, BenchDir) {
    let dir = BenchDir::new();
    let mut store = Store::open(&dir.store_path()).expect("open the bench store");
    let rows: Vec<CommitRow> = (0..count).map(commit).collect();
    let run = store.begin_run("cold", Trigger::Manual, T0).unwrap();
    // The same batch discipline the writer uses in production (§7.3): one transaction per
    // `write.batch_rows` rows, so what is timed is what ships.
    let per_batch = (BATCH_ROWS / (FILES_PER_COMMIT + 1)).max(1);
    for batch in rows.chunks(per_batch) {
        store.write_commits(run, T0, batch).unwrap();
    }
    store
        .replace_ref_tips(
            &[
                ("HEAD".to_owned(), oid(count - 1), "head"),
                ("refs/heads/main".to_owned(), oid(count - 1), "branch"),
            ],
            T0,
        )
        .unwrap();
    let episodes: Vec<EpisodeRow> = (0..count / 2)
        .map(|index| episode(index * 2, index * 2 + 1))
        .collect();
    store.write_episodes(T0, &episodes).unwrap();
    (store, dir)
}

fn main() {
    let count = commit_count();
    // `--profile-time` and friends come from the caller (`make bench` passes a short profile so the
    // whole budget run fits in a coffee-break budget of five minutes); nothing here hard-codes a
    // measurement policy, which is why criterion's knobs stay usable.
    // The defaults are the gate's, not criterion's: a budget run has to fit inside five minutes
    // alongside a fixture build and a release compile, so 20 samples of 400 ms is the shape (`make
    // bench-criterion` says so out loud). `configure_from_args` comes last on purpose — it is what lets
    // `cargo bench … -- --sample-size 100 --measurement-time 2` override these when someone wants a
    // number they would defend in an ADR.
    let mut criterion = Criterion::default()
        .sample_size(if count > 1_000 { 10 } else { 20 })
        .measurement_time(if count > 1_000 {
            Duration::from_millis(300)
        } else {
            Duration::from_millis(400)
        })
        .warm_up_time(Duration::from_millis(120))
        .without_plots()
        .configure_from_args();

    let (store, _dir) = seeded(count);
    let scale = format!("{count} commits");

    // --- query.timeline.ms ---------------------------------------------------------------
    // One page of the episode rail. `e1.6`'s rail asks for 100 rows with review state joined in, and
    // `episodes()` is exactly that read (page, members per row).
    {
        let query = EpisodeQuery {
            limit: PAGE,
            ..EpisodeQuery::default()
        };
        let mut group = criterion.benchmark_group("query.timeline.ms");
        group.throughput(Throughput::Elements(PAGE as u64));
        group.bench_function(format!("rail_page/{scale}"), |bencher| {
            bencher.iter(|| {
                let rows = store.episodes(black_box(&query)).expect("read a page");
                black_box(rows.len())
            })
        });
        group.finish();
    }

    // --- query.frame_store_budget.ms ------------------------------------------------------
    // The 8 ms hard limit is on *any single store call on the keystroke path*, so it is measured
    // against the calls a keystroke actually makes: the rail page's count (the strip's "N eps") and
    // the paths behind a page of rows. Both are one statement each.
    {
        let ids: Vec<String> = store
            .episodes(&EpisodeQuery {
                limit: PAGE,
                ..EpisodeQuery::default()
            })
            .expect("read a page")
            .iter()
            .map(|row| row.id.clone())
            .collect();
        let mut group = criterion.benchmark_group("query.frame_store_budget.ms");
        group.bench_function(format!("count_episodes/{scale}"), |bencher| {
            bencher.iter(|| {
                store
                    .count_episodes(black_box(&EpisodeQuery::default()))
                    .expect("count")
            })
        });
        group.bench_function(format!("episode_paths/{scale}"), |bencher| {
            bencher.iter(|| {
                let paths = store.episode_paths(black_box(&ids)).expect("read paths");
                black_box(paths.len())
            })
        });
        group.finish();
    }

    // --- query.card.ms --------------------------------------------------------------------
    // The summary card for the selected episode: the row, then its summed file rows. Two statements,
    // both on the keystroke path, and the budget (50 ms) is the render's, so the bench times the read
    // half of it the way the pane does.
    {
        let id = format!("ep-{:0>6}", count / 4 * 2);
        let mut group = criterion.benchmark_group("query.card.ms");
        group.bench_function(format!("episode_and_files/{scale}"), |bencher| {
            bencher.iter(|| {
                let row = store.episode(black_box(&id)).expect("read the episode");
                let files = store
                    .episode_files(std::slice::from_ref(&id))
                    .expect("read the files");
                black_box((row.map(|row| row.summary), files.rows))
            })
        });
        group.finish();
    }

    // --- write discipline -----------------------------------------------------------------
    // Not a latency budget: `write.batch_rows` is a *cap*, and what is worth a trend line is the
    // throughput it buys (§13.1 measured 444 k rows/s batched against 35 k/s one row per transaction).
    // A quiet collapse here is a cold index that gets 12× slower for a reason nobody chose, which is
    // exactly what a regression gate is for.
    {
        let rows: Vec<CommitRow> = (0..BATCH_ROWS / (FILES_PER_COMMIT + 1))
            .map(commit)
            .collect();
        let mut store = Store::open_in_memory().expect("in-memory store for the write bench");
        // `kind` is a checked column: the writer only ever opens a run of one of six kinds (§7.3), and
        // a batch at the cap is the warm path's shape.
        let run = store.begin_run("warm", Trigger::Manual, T0).unwrap();
        let mut group = criterion.benchmark_group("write.batch_rows");
        group.throughput(Throughput::Elements(
            (rows.len() * (FILES_PER_COMMIT + 1)) as u64,
        ));
        group.bench_function("one_batch_at_the_cap", |bencher| {
            // A fresh oid per iteration would be a hash of the loop counter and would measure the
            // formatting; instead every iteration rewrites the same rows (the store's write is an
            // `INSERT … ON CONFLICT DO UPDATE`, so this is the warm re-index write, and its cost is
            // the same shape).
            bencher.iter(|| {
                store
                    .write_commits(black_box(run), T0, black_box(&rows))
                    .expect("write one batch")
            })
        });
        group.finish();
    }

    criterion.final_summary();
}

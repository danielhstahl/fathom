//! The e1.2 performance case: 500 commits — every change kind, merges included — through walk, normalize
//! and store, recorded to `benchmarks/reports/`.
//!
//! Run it with:
//!
//! ```text
//! cargo test --release --offline -p fathom-ingest --test bench_ingest -- --ignored --nocapture
//! ```
//!
//! It is `#[ignore]`d like `bench_walk` is: the numbers only mean something in a release build, and a debug
//! build would report a three-fold slowdown that has nothing to do with the code being measured. The budget
//! it asserts is the one `benchmarks/budgets.json` already states — `cold.index.ms.small`, 1 500 ms p95 for
//! a repository of up to 1 000 commits — because e1.2 is the spine half of "cold index", not a new
//! measurement family.
//!
//! Two things are measured that a timing alone does not tell. The first is **memory**: peak resident memory
//! before and after, so that "bounded" is a number and not an adjective. The second is the **store's byte
//! model**: `store.bytes_per_commit_spine` is 4 096 B/commit, and a spine that silently grows to 8 KB is a
//! budget failure that no latency number would catch.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// A benchmark's output is its report: the numbers go to stdout as well as to the file, so a run with
// `--nocapture` is readable without opening anything.
#![allow(clippy::print_stdout)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_ingest::export;
use fathom_ingest::ingest::{Collector, IngestOptions, run};
use fathom_ingest::normalize::Options;
use fathom_ingest::record::Unclassified;
use fathom_ingest::repo::Repository;
use fathom_store::spine::Store;
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

/// `cold.index.ms.<class>` from `benchmarks/budgets.json`: the whole cold spine — walk, normalize, store —
/// for each size class the budget file names. The default run is the ticket's 500-commit fixture, which is
/// `small`; the table is here so `FATHOM_BENCH_COMMITS=10000` measures against `medium` instead of
/// quietly failing a budget that does not apply to it.
fn budget_ms(size_class: &str) -> u128 {
    match size_class {
        "medium" => 20_000,
        "large" => 75_000,
        "xlarge" => 240_000,
        _ => 1_500,
    }
}

/// `store.bytes_per_commit_spine.small`, in bytes.
const BYTE_MODEL: u64 = 4_096;

fn commit_count() -> usize {
    std::env::var("FATHOM_BENCH_COMMITS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(500)
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    ticket: &'static str,
    generated_utc: String,
    environment: Environment,
    corpus: Corpus,
    budget: Budget,
    measured: Measured,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct Environment {
    platform: String,
    cpu_count: usize,
    rustc: String,
    git: String,
    profile: &'static str,
}

#[derive(Serialize)]
struct Corpus {
    commits: usize,
    size_class: &'static str,
    shape: &'static str,
    files: usize,
    pack_bytes: u64,
    build_ms: u128,
}

#[derive(Serialize)]
struct Budget {
    /// The threshold for this corpus's size class, quoted from the budget file rather than invented.
    cold_index_ms: u128,
    id: String,
    size_class: &'static str,
    source: &'static str,
    store_bytes_per_commit_spine: u64,
}

#[derive(Serialize)]
struct Measured {
    /// Walk + normalize, into memory: the half that would run on a keystroke.
    cold_normalize_ms: u128,
    /// Walk + normalize + store, into a file-backed database: the cold spine, which is what the budget names.
    cold_index_ms: u128,
    /// The same run again in the same process — pack indexes built, page cache warm.
    warm_index_ms: u128,
    /// The same corpus with one worker, so the cost of rayon is visible rather than assumed.
    one_worker_ms: u128,
    /// `one_worker_ms / cold_normalize_ms`, which is the part rayon can parallelize (the store write is one
    /// transaction and is deliberately serial). Under 1.0 means the corpus is too small to show anything.
    parallel_speedup: f64,
    /// The second run against a store that already has everything: the resume path's floor.
    already_indexed_ms: u128,
    /// `git log --numstat` over the same repository: what git itself takes to produce per-file line counts,
    /// recorded as context and not as a competitor — it does not normalize, dedupe or store anything.
    git_log_numstat_ms: u128,
    export_json_ms: u128,
    commits_per_s: u64,
    ns_per_commit: u64,
    files_per_s: u64,
    workers: usize,
    commits_seen: usize,
    commits_written: usize,
    files_written: usize,
    rows_written: usize,
    export_bytes: usize,
    export_bytes_per_commit: u64,
    store_bytes: u64,
    store_bytes_per_commit: u64,
    byte_model_met: bool,
    peak_rss_kib_before: Option<u64>,
    peak_rss_kib_after: Option<u64>,
    rss_kib_after: Option<u64>,
    rss_delta_kib: Option<u64>,
    budget_met: bool,
}

#[test]
#[ignore = "builds a 500-commit repository and only means anything in release; run with --release --ignored"]
fn ingesting_the_small_corpus_fits_the_budget() {
    let count = commit_count();
    let class = size_class(count);
    let budget_ms = budget_ms(class);
    let started = Instant::now();
    let fixture = Fixture::new("bench-ingest");
    let repo_dir = fixture.init();
    fixture.mixed_history(&repo_dir, count);
    let build_ms = started.elapsed().as_millis();
    let pack_bytes = directory_bytes(&repo_dir.join(".git/objects"));
    assert_eq!(
        git_rev_list_count(&repo_dir),
        count,
        "the corpus is the size the budget talks about"
    );

    let repo = Repository::discover(&repo_dir).expect("discover");
    let peak_before = peak_rss_kib();

    // Walk + normalize, nothing durable. This is the number that would show up in an interactive path.
    let normalize = timed(|| {
        let mut collector = Collector::default();
        let report = run(
            &repo,
            &Unclassified,
            &IngestOptions::default(),
            &mut collector,
        )
        .expect("normalize");
        (report, collector.records)
    });
    let records = normalize.1.1;

    // The budget run: the same walk, plus the store, into a file-backed database. Cold means first in this
    // process, as in `bench_walk`; the OS page cache is not dropped, because that needs root.
    let db_path = fixture.path("fathom.sqlite3");
    let index = timed(|| {
        let mut store = Store::open(&db_path).expect("open the store");
        run(
            &repo,
            &Unclassified,
            &IngestOptions::default(),
            &mut StoreSink::new(&mut store),
        )
        .expect("ingest into the store")
    });
    let cold_index_ms = index.0.as_millis();
    let report = index.1;
    let peak_after = peak_rss_kib();

    // Warm: the same run against the store that now holds it, which is the resume path's floor — the walk
    // still happens, the diffs do not.
    let warm = timed(|| {
        let mut store = Store::open(&db_path).expect("reopen");
        run(
            &repo,
            &Unclassified,
            &IngestOptions::default(),
            &mut StoreSink::new(&mut store),
        )
        .expect("re-ingest")
    });
    assert_eq!(warm.1.commits_existing, count, "and it wrote nothing twice");

    // The same corpus with one worker. `bench_walk` showed the walk itself is single-threaded work; this is
    // where the diffing parallelism has to pay for itself.
    let one_worker = timed(|| {
        let mut collector = Collector::default();
        run(
            &repo,
            &Unclassified,
            &IngestOptions {
                parallelism: 1,
                ..IngestOptions::default()
            },
            &mut collector,
        )
        .expect("one-worker ingest")
    });
    assert_eq!(one_worker.1.commits_written, count);

    // The canonical export, because e1.2's contract is the JSON as much as the rows.
    let export =
        timed(|| export::to_json(&records, &export::Normalization::of(&Options::default(), 0)));
    let export_bytes = export.1.expect("export").len();

    let git_log = timed(|| git(&repo_dir, &["log", "--numstat", "--format=%H"]));
    assert!(
        git_log.1.lines().count() > count,
        "git's own numstat covers every commit plus the file rows it counted"
    );

    let store_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    let store_bytes_per_commit = if count == 0 {
        0
    } else {
        store_bytes / u64::try_from(count).unwrap_or(1)
    };
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let batch_commits = IngestOptions::default().commits_per_batch;
    let speedup = ratio(one_worker.0.as_millis(), normalize.0.as_millis());
    let rss_delta_kib = match (peak_before, peak_after) {
        (Some(before), Some(after)) => Some(after.saturating_sub(before)),
        _ => None,
    };

    let measured = Measured {
        cold_normalize_ms: normalize.0.as_millis(),
        cold_index_ms,
        warm_index_ms: warm.0.as_millis(),
        one_worker_ms: one_worker.0.as_millis(),
        parallel_speedup: speedup,
        already_indexed_ms: warm.1.elapsed.as_millis(),
        git_log_numstat_ms: git_log.0.as_millis(),
        export_json_ms: export.0.as_millis(),
        commits_per_s: throughput(cold_index_ms, count),
        ns_per_commit: ns_per(cold_index_ms, count),
        files_per_s: throughput(cold_index_ms, report.files_written),
        workers: report.workers,
        commits_seen: report.commits_seen,
        commits_written: report.commits_written,
        files_written: report.files_written,
        rows_written: report.rows_written,
        export_bytes,
        export_bytes_per_commit: u64::try_from(export_bytes / count.max(1)).unwrap_or(u64::MAX),
        store_bytes,
        store_bytes_per_commit,
        byte_model_met: store_bytes_per_commit <= BYTE_MODEL,
        peak_rss_kib_before: peak_before,
        peak_rss_kib_after: peak_after,
        rss_kib_after: current_rss_kib(),
        rss_delta_kib,
        budget_met: cold_index_ms <= budget_ms,
    };

    let report = Report {
        schema_version: 1,
        ticket: "reco-76g",
        generated_utc: utc_stamp(),
        environment: environment(&repo_dir),
        corpus: Corpus {
            commits: count,
            size_class: class,
            shape: "mixed rotation: add, modify, delete, rename, binary, mode-only, multi-file, trailer \
                    blocks, a non-UTF-8 path, one over-cap blob, and two merge commits",
            files: report.files_written,
            pack_bytes,
            build_ms,
        },
        budget: Budget {
            cold_index_ms: budget_ms,
            id: format!("cold.index.ms.{class}"),
            size_class: class,
            source: "`benchmarks/budgets.json`; the ticket's own 500-commit fixture is the `small` class",
            store_bytes_per_commit_spine: BYTE_MODEL,
        },
        measured,
        notes: vec![
            format!(
                "parallel speedup on the normalize phase was {:.2}x against one worker, with {count} commits on {cpus} cpus; the store write is one transaction per batch and is deliberately serial, so this ratio is the diffable half - and at a few hundred commits the diffs are too small for the pool to show much",
                speedup
            ),
            format!(
                "batches were {batch_commits} commits (about {} rows, counting a commit row plus its file rows), inside ADR-004 7.3's write.batch_rows of 5 000",
                batch_commits * (report.rows_written / report.commits_written.max(1)).max(1)
            ),
                            "cold = first ingest in this process; the OS page cache is not dropped (that needs root), so a \
                 first run on a cold machine will be slower than this".to_owned(),
            "`cold_index_ms` is walk + normalize + one SQLite transaction per batch, into a WAL database \
             with synchronous=NORMAL, which is ADR-004 §7's configuration and not a benchmark expedient"
                .to_owned(),
            "`already_indexed_ms` is the resume floor: the walk still runs, so it is a lower bound on \
             re-running against a store that is already current, not a measurement of skipped work".to_owned(),
            "`git_log_numstat_ms` is context, not a competitor: git does not normalize, dedupe, or write \
             anything, and fathom never shells out to it (ADR-001 §8.2)".to_owned(),
            "no `ingest.*` latency family exists in benchmarks/budgets.json; `cold.index.ms` is the family \
             this ticket actually has a budget in, and the caps are `ingest.file_size_cap.mib`".to_owned(),
        ],
    };

    let path = report_path();
    std::fs::create_dir_all(path.parent().expect("reports dir")).expect("create reports dir");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&report).expect("serialize") + "\n",
    )
    .expect("write the report");
    println!("wrote {}", path.display());
    println!(
        "{}",
        serde_json::to_string_pretty(&report.measured).expect("serialize")
    );

    assert!(
        cold_index_ms <= budget_ms,
        "ingesting and storing {count} commits took {cold_index_ms} ms against a {budget_ms} ms budget          ({class})"
    );
    assert!(
        store_bytes_per_commit <= BYTE_MODEL,
        "the spine cost {store_bytes_per_commit} B/commit against a {BYTE_MODEL} B/commit model"
    );
    // Memory, as a bound rather than an adjective. Two shapes of assertion, because the same word means
    // different things at different sizes: at the ticket's 500 commits the growth is dominated by fixed costs
    // (the pack index, gix's caches) so it gets an absolute ceiling; on a larger corpus the meaningful claim
    // is that growth is per *batch* and per *cache*, not per commit, so it gets a per-commit ceiling that a
    // blob slurped into memory would blow through on its own.
    if let Some(delta) = rss_delta_kib {
        if count <= 1_000 {
            assert!(
                delta < 64 * 1024,
                "peak resident memory grew by {delta} KiB during a {count}-commit ingest, which is not bounded"
            );
        } else {
            let per_commit = delta / u64::try_from(count).unwrap_or(1);
            assert!(
                per_commit < 32,
                "peak resident memory grew by {per_commit} KiB per commit, which grows with the corpus                  instead of with the batch"
            );
        }
    }
}

/// Time a closure and return the duration plus whatever it produced.
fn timed<T>(run: impl FnOnce() -> T) -> (std::time::Duration, T) {
    let started = Instant::now();
    let value = run();
    (started.elapsed(), value)
}

fn throughput(ms: u128, count: usize) -> u64 {
    if ms == 0 {
        return u64::MAX;
    }
    u64::try_from(count as u128 * 1000 / ms).unwrap_or(u64::MAX)
}

/// A ratio that survives a zero denominator, because a fast run can round to zero milliseconds.
fn ratio(numerator: u128, denominator: u128) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    f64::from(u32::try_from(numerator).unwrap_or(u32::MAX))
        / f64::from(u32::try_from(denominator).unwrap_or(u32::MAX))
}

fn ns_per(ms: u128, count: usize) -> u64 {
    if count == 0 {
        return 0;
    }
    u64::try_from(ms.saturating_mul(1_000_000) / u128::try_from(count).unwrap_or(1))
        .unwrap_or(u64::MAX)
}

/// The size class from `benchmarks/budgets.json`, so the report says which budget row it belongs to.
fn size_class(count: usize) -> &'static str {
    if count <= 1_000 {
        "small"
    } else if count <= 10_000 {
        "medium"
    } else if count <= 50_000 {
        "large"
    } else {
        "xlarge"
    }
}

fn environment(root: &Path) -> Environment {
    Environment {
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        cpu_count: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        rustc: rustc_version(),
        git: git(root, &["--version"]),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    }
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn directory_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        total += if metadata.is_dir() {
            directory_bytes(&entry.path())
        } else {
            metadata.len()
        };
    }
    total
}

fn utc_stamp() -> String {
    let secs = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
    .unwrap_or(0);
    // fathom's own calendar arithmetic, so the stamp is a date a human can read and nothing new to depend
    // on. A report is allowed to read a clock; a fixture is not (ADR-002 §4.4).
    let when = fathom_core::time::Timestamp::from_unix_micros(secs * 1_000_000);
    let (year, month, day) = when.civil();
    let (hour, minute, second) = when.time_of_day();
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn report_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/reports/e1-2-ingest.json")
}

//! The e1.1 performance case: a 100k-commit walk in under two seconds, in flat memory, recorded to
//! `benchmarks/reports/`.
//!
//! Run it with:
//!
//! ```text
//! cargo test --release --offline -p fathom-ingest --test bench_walk -- --ignored --nocapture
//! ```
//!
//! It is `#[ignore]`d because it builds a 100k-commit repository, which takes longer than the rest of the
//! suite put together; the numbers it writes are the evidence the ticket asked for, and the assertions are
//! the budget, so a regression fails the run rather than showing up in a report nobody reads.
//!
//! What "cold" means here, stated plainly: the process opens the repository and walks it *once*, having read
//! nothing from it before, so every object read is a first read for this process and every pack index is
//! built as it goes. What it does *not* mean: a cold operating-system page cache — dropping that needs root,
//! and a benchmark that needed root would never be run. The second walk in the same process is recorded as
//! `warm_*` so the difference between the two is visible instead of argued about.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// A benchmark's output *is* its output: the numbers go to stdout as well as to the report file, so that a
// run with `--nocapture` is readable without opening anything.
#![allow(clippy::print_stdout)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_ingest::graph::{WalkOptions, WalkOrder};
use fathom_ingest::repo::Repository;
use serde::Serialize;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The ticket's budget: a 100k-commit first-parent walk in under two seconds.
const BUDGET_MS: u128 = 2_000;

/// How many commits to build. Overridable so a developer can do a quick version of the same run.
fn commit_count() -> usize {
    std::env::var("FATHOM_BENCH_COMMITS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100_000)
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
    notes: Vec<&'static str>,
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
    packed: bool,
    pack_bytes: u64,
    build_ms: u128,
}

#[derive(Serialize)]
struct Budget {
    /// The threshold this ticket states. `benchmarks/budgets.json` has no `cold.walk` family yet, which the
    /// report's notes say out loud rather than quietly inventing one.
    cold_walk_ms: u128,
    id: &'static str,
    size_class: &'static str,
    source: &'static str,
}

#[derive(Serialize)]
struct Measured {
    /// The walk the budget is about: first parent, ids only, streamed and dropped.
    cold_walk_ms: u128,
    warm_walk_ms: u128,
    first_parent_headers_ms: u128,
    full_history_ids_ms: u128,
    date_ordered_ms: u128,
    oldest_first_ms: u128,
    buffered_headers_ms: u128,
    /// What `git rev-list --count` takes on the same repository, for context and not for comparison: it is
    /// the floor of what any reader can do, because it never decodes a commit.
    git_rev_list_count_ms: u128,
    commits_per_s: u64,
    ns_per_commit: u64,
    /// `VmHWM` before and after the streaming walk: the proof that a streamed walk does not grow with the
    /// history. On platforms without `/proc` these are `None`.
    peak_rss_kib_before: Option<u64>,
    peak_rss_kib_after: Option<u64>,
    rss_kib_after: Option<u64>,
    entries_walked: usize,
    /// The same walk with `Detail::Headers` buffered in memory, for the difference streaming is meant to
    /// make.
    buffered_peak_rss_kib: Option<u64>,
    budget_met: bool,
    rss_delta_kib_streaming: Option<u64>,
}

#[test]
#[ignore = "builds a 100k-commit repository; run with --release --ignored"]
fn walking_a_hundred_thousand_commits_fits_the_budget() {
    let count = commit_count();
    let started = Instant::now();
    let fixture = Fixture::new("bench-walk");
    let root = fixture.init();
    fixture.linear_history(&root, count);
    // Packed, like a real clone: a thousand loose objects is not what a walk meets in the wild.
    fixture.repack(&root, &["--window=250"]);
    let build_ms = started.elapsed().as_millis();
    let pack_bytes = directory_bytes(&root.join(".git/objects"));
    assert_eq!(
        git_rev_list_count(&root),
        count,
        "the corpus is the size the budget talks about"
    );

    let repo = Repository::discover(&root).expect("discover");
    let head = repo.head().expect("head").oid().expect("born");
    let peak_before = peak_rss_kib();

    // The budget walk: first parent, ids only, streamed, nothing kept.
    let cold = timed(|| {
        repo.log(
            WalkOptions::head()
                .starting_at([head])
                .first_parent()
                .ids_only(),
        )
        .count()
    });
    let cold_ms = cold.0.as_millis();
    assert_eq!(cold.1, count, "and it really walked the whole history");
    let peak_after_streaming = peak_rss_kib();

    // The same walk again, now that every pack index and object has been touched at least once.
    let warm = timed(|| {
        repo.log(
            WalkOptions::head()
                .starting_at([head])
                .first_parent()
                .ids_only(),
        )
        .count()
    });
    assert_eq!(warm.1, count);

    // Every other shape, because a number is only useful with the shapes around it.
    let first_parent_headers = timed(|| {
        repo.log(
            WalkOptions::head()
                .starting_at([head])
                .first_parent()
                .order(WalkOrder::DateNewestFirst),
        )
        .count()
    });
    assert_eq!(first_parent_headers.1, count);
    let full_ids = timed(|| {
        repo.log(
            WalkOptions::head()
                .starting_at([head])
                .order(WalkOrder::DateNewestFirst)
                .ids_only(),
        )
        .count()
    });
    assert_eq!(full_ids.1, count);
    let date_ordered = timed(|| {
        repo.log(
            WalkOptions::head()
                .starting_at([head])
                .order(WalkOrder::DateNewestFirst),
        )
        .count()
    });
    assert_eq!(date_ordered.1, count);
    let oldest_first = timed(|| {
        repo.log(
            WalkOptions::head()
                .starting_at([head])
                .order(WalkOrder::DateOldestFirst)
                .ids_only(),
        )
        .count()
    });
    assert_eq!(oldest_first.1, count);
    let buffered = timed(|| {
        repo.log(WalkOptions::head().starting_at([head]))
            .finish()
            .expect("walk")
    });
    let buffered_peak = peak_rss_kib();
    assert_eq!(
        buffered.1.entries.len(),
        count,
        "buffering keeps everything"
    );
    drop(buffered.1);

    let git_count = timed(|| git_rev_list_count(&root));
    let rss_delta_kib = match (peak_before, peak_after_streaming) {
        (Some(before), Some(after)) => Some(after.saturating_sub(before)),
        _ => None,
    };

    let measured = Measured {
        cold_walk_ms: cold_ms,
        warm_walk_ms: warm.0.as_millis(),
        first_parent_headers_ms: first_parent_headers.0.as_millis(),
        full_history_ids_ms: full_ids.0.as_millis(),
        date_ordered_ms: date_ordered.0.as_millis(),
        oldest_first_ms: oldest_first.0.as_millis(),
        buffered_headers_ms: buffered.0.as_millis(),
        git_rev_list_count_ms: git_count.0.as_millis(),
        commits_per_s: throughput(cold_ms, count),
        ns_per_commit: ns_per(cold_ms, count),
        peak_rss_kib_before: peak_before,
        peak_rss_kib_after: peak_after_streaming,
        rss_kib_after: current_rss_kib(),
        entries_walked: cold.1,
        buffered_peak_rss_kib: buffered_peak,
        budget_met: cold_ms <= BUDGET_MS,
        rss_delta_kib_streaming: rss_delta_kib,
    };

    let report = Report {
        schema_version: 1,
        ticket: "reco-cey",
        generated_utc: utc_stamp(),
        environment: environment(&root),
        corpus: Corpus {
            commits: count,
            size_class: size_class(count),
            shape: "linear history, one file changed per commit, packed",
            packed: true,
            pack_bytes,
            build_ms,
        },
        budget: Budget {
            cold_walk_ms: BUDGET_MS,
            id: "cold.walk.ms",
            size_class: size_class(count),
            source: "ticket reco-cey; `benchmarks/budgets.json` has no `cold.walk` family yet",
        },
        measured,
        notes: vec![
            "cold = first walk in a fresh process; the OS page cache is not dropped (that needs root), so a \
             first run on a cold machine will be slower than this",
            "the budget walk is first-parent, ids only, streamed and dropped: it is what `fathom log` and the \
             e1.2 store writer actually do",
            "`git rev-list --count` is recorded as a floor, not a competitor: it never decodes a commit",
            "no `cold.walk.ms` budget family exists in benchmarks/budgets.json; adding one (with \
             `rss.walk.peak_mib`) is the follow-up this report argues for",
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
        cold_ms <= BUDGET_MS,
        "a {count}-commit first-parent walk took {cold_ms} ms against a {BUDGET_MS} ms budget"
    );
    if let Some(delta) = rss_delta_kib {
        // The streaming walk may not grow with the history. 128 MiB of headroom is generous for 100k
        // commits — the walk's own frontier is a handful of vectors — and small enough that buffering
        // every header (which is what `buffered_headers_ms` measures) trips it.
        assert!(
            delta < 128 * 1024,
            "the streaming walk grew peak RSS by {delta} KiB, which is not flat"
        );
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
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/reports/e1-1-git-walk.json")
}

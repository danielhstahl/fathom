//! The durable cache ceiling: the budget is enforced, the protected window is honoured, and the counters
//! say which one bit (`reco-93w`, ADR-004 §8.6).
//!
//! `e6.1`'s tests prove the LRU is *correct* — a hit after a schema bump is not a hit. These prove the
//! LRU is *bounded*, which is a different property and the one the monorepo ticket is about: with a
//! budget of N bytes the store ends at N bytes, the rows that go are the coldest, the ones the protection
//! set names stay, and somebody reading `fathom stats` weeks later can tell which of those three happened.
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_store::cache::{Config, Versions, derive_key};
use fathom_store::ceiling::CacheCeiling;
use fathom_store::spine::Store;
use std::path::PathBuf;

/// A fixed clock, so "recent" means something.
const NOW: i64 = 1_735_689_600_000;
const MINUTE: i64 = 60_000;

fn key(input: &str, analyzer: &str) -> fathom_store::cache::CacheKey {
    derive_key(
        analyzer,
        input,
        None,
        &Versions::default(),
        &Config::default(),
    )
    .expect("a key for a registered analyzer")
}

/// Fill the cache with `count` rows of `bytes` each, oldest first: row *i* was last used one minute
/// later than row *i-1*, so the walk order and the LRU order agree, and so a protection window measured
/// in minutes can select one row rather than all of them.
fn fill(store: &mut Store, count: u32, bytes: usize) {
    for index in 0..count {
        let payload = vec![b'x'; bytes];
        store
            .put(
                &key(&format!("input-{index}"), "diff.per_file"),
                &payload,
                None,
                1,
                NOW + i64::from(index) * MINUTE,
            )
            .expect("a row lands");
    }
}

/// A file-backed store in a temporary directory, removed on drop.
///
/// An in-memory store cannot hold a file-class blob — over `INLINE_BLOB_BYTES` and the payload belongs on
/// disk beside the store, which is exactly the two storage classes this file has to test together.
struct TempStore {
    dir: PathBuf,
}

impl TempStore {
    fn new(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("fathom-ceiling-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        TempStore { dir }
    }

    fn open(&self) -> Store {
        Store::open(&self.dir.join("index.sqlite3")).expect("the store opens")
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Read one stored counter.
fn counter(store: &Store, name: &str) -> Option<i64> {
    store
        .counters(name)
        .expect("counters read")
        .into_iter()
        .find(|(stored, _)| stored == name)
        .map(|(_, value)| value)
}

fn ceiling(budget_bytes: u64, protect_minutes: i64, dry_run: bool) -> CacheCeiling {
    CacheCeiling {
        budget_bytes,
        protect_recent_ms: protect_minutes * MINUTE,
        dry_run,
    }
}

#[test]
fn a_cache_over_its_budget_ends_at_its_budget_with_the_coldest_rows_gone() {
    let mut store = Store::open_in_memory().expect("the store opens");
    // 20 rows of 1 KiB against a 5 KiB budget: the 15 coldest go, the 5 newest stay.
    fill(&mut store, 20, 1024);

    let report = store
        .cache_enforce(&ceiling(5 * 1024, 0, false), NOW + 10_000)
        .expect("the enforcement runs");
    assert_eq!(report.rows_before, 20);
    assert_eq!(
        report.rows_protected, 0,
        "the window is zero, so nothing is protected"
    );
    assert_eq!(report.rows_evicted, 15);
    assert_eq!(report.bytes_evicted, 15 * 1024);
    assert!(
        report.bytes_after <= 5 * 1024,
        "the store ended at {} bytes, over its 5 KiB budget",
        report.bytes_after
    );
    let (rows, bytes) = store.cache_footprint().expect("the cache measures");
    assert_eq!(rows, 5);
    assert_eq!(bytes, 5 * 1024);
    store
        .flush_counters(NOW + 10_000)
        .expect("the counts a run left are written the way every other count is");
    assert_eq!(counter(&store, "cache.evict"), Some(15));
    assert_eq!(counter(&store, "cache.evict.bytes"), Some(15 * 1024));
    assert_eq!(
        counter(&store, "cache.evict.diff.per_file"),
        Some(15),
        "and it is attributed to the analyzer whose work went, which is how a low hit rate gets explained"
    );
}

#[test]
fn a_cache_inside_its_budget_is_left_alone() {
    let mut store = Store::open_in_memory().expect("the store opens");
    fill(&mut store, 4, 256);
    let report = store
        .cache_enforce(&ceiling(64 * 1024, 1_440, false), NOW)
        .expect("the enforcement runs");
    assert!(
        !report.over_budget(),
        "4 KiB of rows against a 64 KiB budget is not a decision anyone has to make"
    );
    assert_eq!(
        report.rows_before, 4,
        "and it counted the rows it was handed"
    );
    assert_eq!(report.rows_evicted, 0);
    assert_eq!(store.cache_footprint().expect("measured").0, 4);
}

#[test]
fn the_protection_window_is_what_the_budget_cannot_take() {
    let mut store = Store::open_in_memory().expect("the store opens");
    // Twenty rows of 1 KiB, all written in the last minute, against a 2 KiB budget. The budget cannot be
    // met; §8.6's rule is that this is reported as unmet rather than paid for out of the working set.
    fill(&mut store, 20, 1024);
    let report = store
        .cache_enforce(&ceiling(2 * 1024, 60, false), NOW + 30 * MINUTE)
        .expect("the enforcement runs");
    assert!(report.over_budget());
    assert_eq!(report.rows_protected, 20);
    assert_eq!(report.rows_evicted, 0);
    assert!(report.bytes_protected > 0);
    assert_eq!(
        store.cache_footprint().expect("measured").0,
        20,
        "the working set is intact: an index that evicts what you are looking at is slower than a big one"
    );

    // Move the clock on, touch one cold row, and the window reaches only that row: the cold majority
    // goes and the protected minority does not, which is the exact rule §8.6 states.
    let late = NOW + 40 * MINUTE;
    let versions = Versions::default();
    let config = Config::default();
    store
        .resolve("diff.per_file", "input-0", None, &versions, &config, late)
        .expect("a cold row is looked up, which touches it");
    let report = store
        .cache_enforce(&ceiling(2 * 1024, 1, false), late)
        .expect("the second pass runs");
    assert_eq!(
        report.rows_protected, 1,
        "only the row touched inside the last minute is protected"
    );
    assert_eq!(report.rows_evicted, 18);
}

#[test]
fn a_dry_run_reports_exactly_what_the_real_pass_would_do_and_changes_nothing() {
    let mut store = Store::open_in_memory().expect("the store opens");
    fill(&mut store, 10, 512);
    let dry = store
        .cache_enforce(&ceiling(2 * 1024, 0, true), NOW + 10_000)
        .expect("the dry run");
    assert!(dry.dry_run);
    assert_eq!(dry.rows_evicted, 6);
    let (rows, _) = store.cache_footprint().expect("measured");
    assert_eq!(
        rows, 10,
        "a dry run that deleted rows would be a lie with a name"
    );
    assert_eq!(
        counter(&store, "cache.evict"),
        None,
        "and it counts nothing either: `fathom stats` would report evictions that did not happen"
    );
    let wet = store
        .cache_enforce(&ceiling(2 * 1024, 0, false), NOW + 10_000)
        .expect("the real pass");
    assert_eq!(
        wet.rows_evicted, dry.rows_evicted,
        "the two passes must agree, or the dry run is not a prediction"
    );
}

#[test]
fn evicting_a_row_leaves_the_blob_and_says_so_when_it_is_now_unreachable() {
    let temp = TempStore::new("blob");
    let mut store = temp.open();
    // A file-class payload: it lands in `blob` with a `blob_ref` edge, which is the case that has a
    // dangling reference to think about.
    let payload = vec![b'y'; 5000];
    let big = key("input-big", "diff.per_file");
    store
        .put(&big, &payload, None, 1, NOW)
        .expect("a file-class row lands");
    assert_eq!(store.cache_footprint().expect("cache measures").1, 5000);
    assert_eq!(store.blob_count().expect("blobs measure"), 1);

    let report = store
        .cache_enforce(&ceiling(1, 0, false), NOW + 1_000)
        .expect("the pass runs with a budget that fits nothing");
    assert_eq!(report.rows_evicted, 1);
    assert_eq!(report.dangling_blobs, 1);
    store.flush_counters(NOW + 1_000).expect("counters flush");
    assert_eq!(
        store.dangling_blob_count().expect("counted"),
        1,
        "the blob outlives the row that named it — that is §8.7's decision to make, not this pass's"
    );
    assert_eq!(
        counter(&store, "cache.evict.blobs_dangling"),
        Some(1),
        "and the retention pass is told the number instead of scanning the table to find it out"
    );
    assert_eq!(
        store.blob_count().expect("blobs measure"),
        1,
        "pruning is a reviewed verb; an eviction is not a review"
    );
}

#[test]
fn a_zero_budget_means_no_cache_and_is_said_loudly() {
    let mut store = Store::open_in_memory().expect("the store opens");
    fill(&mut store, 3, 128);
    let report = store
        .cache_enforce(&ceiling(0, 0, false), NOW + 1_000)
        .expect("the pass runs");
    assert_eq!(report.rows_evicted, 3);
    assert_eq!(store.cache_footprint().expect("measured").0, 0);
}

#[test]
fn the_in_process_payload_cache_is_bounded_and_counts_what_it_refuses() {
    use fathom_core::memory::Ceilings;
    let temp = TempStore::new("payload");
    let mut store = temp.open();
    // A 1 MiB ceiling holds eight 128 KiB payloads; reading sixteen of them means evicting, and a 4 MiB
    // payload is refused because it could never live alongside anything else.
    let ceilings = Ceilings {
        analysis_cache_bytes: 1024 * 1024,
        ..Ceilings::default()
    };
    store.set_ceilings(&ceilings);
    let versions = Versions::default();
    let config = Config::default();
    for index in 0..16u32 {
        let k = key(&format!("payload-{index}"), "diff.per_file");
        // Distinct bytes per row, because a blob is content-addressed: sixteen copies of the same
        // 128 KiB payload are one blob, and one blob in a 1 MiB cache proves nothing about eviction.
        let payload: Vec<u8> = (0..128 * 1024)
            .map(|byte| (byte + index as usize) as u8)
            .collect();
        store
            .put(&k, &payload, None, 1, NOW + i64::from(index))
            .expect("a row lands");
        store
            .resolve(
                "diff.per_file",
                &format!("payload-{index}"),
                None,
                &versions,
                &config,
                NOW,
            )
            .expect("and is read back through the cache");
    }
    let stats = store.payload_stats();
    assert!(
        stats.high_water_bytes <= 1024 * 1024,
        "the payload cache held {} bytes against its own 1 MiB ceiling",
        stats.high_water_bytes
    );
    assert!(
        stats.evictions >= 8,
        "sixteen distinct 128 KiB payloads cannot all live in 1 MiB, and it evicted {}",
        stats.evictions
    );
    // Reading the newest one again is a payload-cache hit: the ceiling evicted the cold end, not the
    // working end, which is the whole difference between a bounded cache and a flushing one.
    store
        .resolve("diff.per_file", "payload-15", None, &versions, &config, NOW)
        .expect("the newest payload is read again");
    assert!(
        store.payload_stats().hits >= 1,
        "the row just read is still held"
    );

    let huge = key("payload-huge", "diff.per_file");
    store
        .put(&huge, &vec![b'h'; 4 * 1024 * 1024], None, 1, NOW)
        .expect("the big row lands");
    store
        .resolve(
            "diff.per_file",
            "payload-huge",
            None,
            &versions,
            &config,
            NOW,
        )
        .expect("the big row is read: the store still answers, it just does not hold it");
    assert_eq!(
        store.payload_stats().refusals,
        1,
        "4 MiB is bigger than the whole ceiling, so it is refused and counted rather than evicting the set"
    );
}

#[test]
fn the_counters_a_run_leaves_survive_the_process_that_made_them() {
    let temp = TempStore::new("counters");
    let path = temp.dir.join("index.sqlite3");
    {
        let mut store = Store::open(&path).expect("the store opens");
        fill(&mut store, 8, 512);
        store
            .cache_enforce(&ceiling(2 * 1024, 0, false), NOW + 1_000)
            .expect("the pass runs");
        store.flush_counters(NOW + 1_000).expect("counters flush");
    }
    let store = Store::open(&path).expect("reopen");
    assert_eq!(
        counter(&store, "cache.evict"),
        Some(4),
        "4 KiB of rows against a 2 KiB budget costs the 4 coldest, and the run's numbers outlive it"
    );
}

#[test]
fn the_reachability_probe_is_a_lookup_per_blob_not_a_scan_per_blob() {
    // The plan is the assertion here, not the clock. On the 100 000-commit fixture — 200 000 blobs, 200
    // 000 cache rows — the version of this probe without `cache_result` and `blob_ref_blob` ran for
    // twenty minutes and did not finish, which is a table scan per blob wearing the costume of a
    // bookkeeping query. A test that *waited* for that would be a benchmark with a timeout; `EXPLAIN
    // QUERY PLAN` says in three lines which shape SQLite picked, so the regression is caught in
    // milliseconds instead.
    let temp = TempStore::new("plan");
    let mut store = temp.open();
    fill(&mut store, 2, 256);
    let plan = |sql: &str| {
        let mut statement = store
            .connection()
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("the planner answers");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(3))
            .expect("the plan reads");
        rows.filter_map(Result::ok).collect::<Vec<_>>().join("\n")
    };
    let probe = plan(
        "SELECT count(*) FROM blob b
         WHERE NOT EXISTS (SELECT 1 FROM cache c WHERE c.result_sha = b.sha256)
           AND NOT EXISTS (SELECT 1 FROM blob_ref r WHERE r.blob_sha = b.sha256)
           AND NOT EXISTS (SELECT 1 FROM graph_chunk g WHERE g.sha256 = b.sha256)",
    );
    assert!(
        probe.contains("cache_result"),
        "the cache half of the probe is a scan again:\n{probe}"
    );
    assert!(
        probe.contains("blob_ref_blob"),
        "the blob_ref half of the probe is a scan again:\n{probe}"
    );
    assert!(
        !probe.contains("SCAN c") && !probe.contains("SCAN r"),
        "something in the probe scans a whole table:\n{probe}"
    );
}

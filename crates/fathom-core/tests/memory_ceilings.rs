//! The memory ceilings and the two containers that obey them (`reco-93w`).
//!
//! The ticket's claim is that "peak RSS is a function of configuration, not of repository size", and a
//! claim about a *function* is tested by moving the argument and watching the result. So these tests
//! move each ceiling and assert the container's behaviour moves with it — the cache holds fewer bytes,
//! evicts more, refuses the entry that cannot fit; the queue blocks instead of growing.
//!
//! What is tested here is the mechanism, not the process: `/proc`-measured RSS belongs to the shipped
//! binary and to `crates/fathom-analyze/tests/memory_ceiling.rs`, which indexes a 100k-commit history
//! under a stated budget.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::memory::{BoundedCache, Ceilings, MAX_CEILING_BYTES, MAX_QUEUE_DEPTH, MIB, Queue};

/// Insert `n` entries of `weight` bytes each, newest last.
fn fill(cache: &BoundedCache<String, String>, n: usize, weight: u64) {
    for index in 0..n {
        cache.insert(format!("k{index}"), "v".to_owned(), weight, "test");
    }
}

// ---------------------------------------------------------------------------
// the ceilings
// ---------------------------------------------------------------------------

#[test]
fn the_shipped_defaults_are_the_numbers_the_documentation_prints() {
    // These five numbers appear in `docs/concepts/memory.md`, in `fathom.toml`'s comments and in the
    // budget file's rationale text. A default that drifts here is a default that has drifted everywhere
    // else too, silently — which is why the assertion is on the values rather than on a prose sentence.
    let ceilings = Ceilings::default();
    assert_eq!(ceilings.analysis_cache_bytes, 128 * MIB);
    assert_eq!(ceilings.graph_snapshot_bytes, 32 * MIB);
    assert_eq!(ceilings.decode_bytes, 8 * MIB);
    assert_eq!(ceilings.queue_batches, 2);
    assert_eq!(ceilings.durable_cache_bytes, 512 * MIB);
    assert_eq!(ceilings.protect_recent_ms, 24 * 60 * 60 * 1000);
}

#[test]
fn a_ceiling_can_be_moved_by_its_key() {
    let mut ceilings = Ceilings::default();
    assert_eq!(ceilings.set("snapshot_cache_mib", 4), Ok(None));
    assert_eq!(ceilings.graph_snapshot_bytes, 4 * MIB);
    assert_eq!(ceilings.set("queue_batches", 5), Ok(None));
    assert_eq!(ceilings.queue_batches, 5);
    assert_eq!(ceilings.set("protect_recent_minutes", 90), Ok(None));
    assert_eq!(ceilings.protect_recent_ms, 90 * 60 * 1000);
    assert_eq!(
        ceilings.set("analyzer_working_set_mib", 64),
        Ok(None),
        "and the working-set key moves the analysis cache, not the queue"
    );
    assert_eq!(ceilings.analysis_cache_bytes, 64 * MIB);
}

#[test]
fn the_shipped_defaults_are_the_medium_column_of_the_budget_file() {
    // The defaults above are the `medium` size class of `benchmarks/budgets.json`, because that is the
    // column the documentation quotes and the one a laptop-sized repository is in. Embedding the file
    // at compile time is what makes this a drift check rather than a second place to be wrong.
    let budgets: serde_json::Value =
        serde_json::from_str(include_str!("../../../benchmarks/budgets.json"))
            .expect("the budget file parses");
    let wanted = |id: &str| -> i64 {
        budgets["budgets"]
            .as_array()
            .expect("a budgets array")
            .iter()
            .find(|entry| entry["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("{id} is not in benchmarks/budgets.json"))["budget"]
            .as_i64()
            .unwrap_or(-1)
    };
    let ceilings = Ceilings::default();
    assert_eq!(
        ceilings.analysis_cache_bytes / MIB,
        wanted("rss.analyzer_working_set.mib.medium") as u64
    );
    assert_eq!(
        ceilings.durable_cache_bytes / MIB,
        wanted("cache.budget.mib.medium") as u64
    );
    assert_eq!(
        ceilings.decode_bytes / MIB,
        wanted("ingest.file_size_cap.mib.medium") as u64
    );
}

#[test]
fn an_absurd_ceiling_is_clamped_and_said_out_loud() {
    // A cloned `fathom.toml` is hostile input. The build refuses to be asked for 8 TiB: it caps the
    // number and returns the sentence that has to be printed, because a silently-clamped budget is a
    // support ticket a week later.
    let mut ceilings = Ceilings::default();
    let notice = ceilings
        .set("cache_budget_mib", 8 * 1024 * 1024)
        .unwrap()
        .expect("the clamp is reported");
    assert_eq!(ceilings.durable_cache_bytes, MAX_CEILING_BYTES);
    assert!(notice.contains("capped at"), "{notice}");
    assert!(
        notice.contains("4096"),
        "{notice} should name what it became"
    );

    let mut ceilings = Ceilings::default();
    let notice = ceilings
        .set("size_cap_mib", 0)
        .unwrap()
        .expect("zero is not a budget");
    assert!(notice.contains("is not a budget"), "{notice}");
    assert_eq!(ceilings.decode_bytes, 0, "and the value is not applied");
}

#[test]
fn a_queue_depth_past_the_floor_and_ceiling_is_said_and_clamped() {
    let mut ceilings = Ceilings::default();
    ceilings
        .set("queue_batches", 10_000)
        .unwrap()
        .expect("clamped");
    assert_eq!(ceilings.queue_batches, MAX_QUEUE_DEPTH);
    let notice = ceilings.set("queue_batches", 0).unwrap().unwrap();
    assert!(notice.contains("deadlock"), "{notice}");
    assert_eq!(
        ceilings.queue_batches, MAX_QUEUE_DEPTH,
        "a rejected value keeps the one that was in force"
    );
}

#[test]
fn an_unknown_key_is_an_error_rather_than_a_shrug() {
    let mut ceilings = Ceilings::default();
    let err = ceilings
        .set("everything_mib", 1)
        .expect_err("typo'd keys must not be silently ignored");
    assert!(err.contains("everything_mib"), "{err}");
}

#[test]
fn clamping_is_idempotent_and_never_leaves_a_zero_ceiling() {
    let zero = Ceilings {
        analysis_cache_bytes: 0,
        graph_snapshot_bytes: 0,
        decode_bytes: 0,
        durable_cache_bytes: 0,
        queue_batches: 0,
        protect_recent_ms: -1,
    }
    .clamped();
    assert_eq!(zero.analysis_cache_bytes, MIB);
    assert_eq!(zero.queue_batches, 1);
    assert_eq!(zero.protect_recent_ms, 0);
    assert_eq!(
        zero.clone().clamped(),
        zero,
        "clamping twice changes nothing"
    );
}

#[test]
fn the_observed_ceilings_are_the_keys_that_set_them() {
    let observed = Ceilings::default().observe();
    for key in fathom_core::memory::KEYS {
        assert!(
            observed.contains_key(key),
            "{key} sets a ceiling but is not printed by `observe`, so `fathom stats` would not show it"
        );
    }
}

// ---------------------------------------------------------------------------
// the bounded cache
// ---------------------------------------------------------------------------

#[test]
fn the_cache_holds_less_when_the_ceiling_is_smaller() {
    // The ticket's claim, as a function: move the argument, the held bytes move with it.
    let big = BoundedCache::<String, String>::new(10 * 100);
    fill(&big, 50, 100);
    let small = BoundedCache::<String, String>::new(4 * 100);
    fill(&small, 50, 100);
    assert_eq!(big.observe().bytes, 10 * 100);
    assert_eq!(small.observe().bytes, 4 * 100);
    assert_eq!(big.observe().entries, 10);
    assert_eq!(small.observe().entries, 4);
    assert_eq!(small.observe().evictions, 46);
    assert_eq!(small.observe().evicted_bytes, 46 * 100);
}

#[test]
fn eviction_is_least_recently_used_not_first_in() {
    let cache = BoundedCache::<String, String>::new(300);
    cache.insert("a".to_owned(), "v".to_owned(), 100, "structure");
    cache.insert("b".to_owned(), "v".to_owned(), 100, "structure");
    cache.insert("c".to_owned(), "v".to_owned(), 100, "structure");
    // Touch `a` so it is the most recently used of the three, then add two more.
    assert_eq!(cache.get_with(&"a".to_owned(), |_| ()), Some(()));
    cache.insert("d".to_owned(), "v".to_owned(), 100, "structure");
    cache.insert("e".to_owned(), "v".to_owned(), 100, "structure");
    assert!(
        cache.contains(&"a".to_owned()),
        "`a` was used most recently"
    );
    assert!(!cache.contains(&"b".to_owned()), "`b` was the coldest");
    assert!(cache.contains(&"d".to_owned()) && cache.contains(&"e".to_owned()));
    let stats = cache.observe();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.evictions, 2);
    assert_eq!(
        stats.evicted_by_class.get("structure"),
        Some(&2),
        "and what got evicted is counted by the class it was filed under"
    );
}

#[test]
fn an_entry_too_large_for_the_ceiling_is_refused_and_counted() {
    // The uncomfortable case, and the one a silent cache would hide: the work was done, the bytes were
    // produced, and the ceiling said no. Counting it is the difference between "the cache is small" and
    // "the ceiling is below the unit of work", which are different advice.
    let cache = BoundedCache::<String, String>::new(100);
    cache.insert("big".to_owned(), "v".repeat(500), 500, "diff");
    let stats = cache.observe();
    assert_eq!(stats.entries, 0);
    assert_eq!(stats.bytes, 0);
    assert_eq!(stats.refusals, 1);
    assert_eq!(stats.refused_bytes, 500);
    assert!(!cache.contains(&"big".to_owned()));
}

#[test]
fn lowering_the_ceiling_frees_the_bytes_now() {
    let cache = BoundedCache::<String, String>::new(1000);
    fill(&cache, 8, 100);
    assert_eq!(cache.observe().bytes, 800);
    let after = cache.set_limit(300);
    assert_eq!(
        after.bytes, 300,
        "a ceiling that shrinks later must still be a ceiling"
    );
    assert_eq!(after.entries, 3);
    assert_eq!(after.limit_bytes, 300);
    assert!(
        after.high_water_bytes >= 800,
        "the high water remembers the peak"
    );
}

#[test]
fn replacing_a_key_does_not_double_count_its_bytes() {
    let cache = BoundedCache::<String, String>::new(200);
    cache.insert("k".to_owned(), "old".to_owned(), 100, "graph");
    cache.insert("k".to_owned(), "new".to_owned(), 100, "graph");
    let stats = cache.observe();
    assert_eq!(stats.bytes, 100);
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.evictions, 0, "a replacement is not an eviction");
    assert_eq!(
        cache.get_with(&"k".to_owned(), |v| v.clone()),
        Some("new".to_owned())
    );
}

#[test]
fn a_zero_ceiling_holds_nothing_rather_than_asserting() {
    // Somebody typed `= 0`. The honest behaviour is a cache that is always empty and says so, not a
    // panic and not a silently-ignored request.
    let cache = BoundedCache::<String, String>::new(0);
    cache.insert("k".to_owned(), "v".to_owned(), 1, "x");
    assert_eq!(cache.observe().entries, 0);
    assert_eq!(cache.get_with(&"k".to_owned(), |_| ()), None);
    assert_eq!(cache.observe().misses, 1);
}

#[test]
fn the_high_water_is_the_peak_not_the_last_value() {
    let cache = BoundedCache::<String, String>::new(400);
    fill(&cache, 4, 100);
    cache.clear();
    fill(&cache, 1, 100);
    let stats = cache.observe();
    assert_eq!(stats.bytes, 100);
    assert_eq!(stats.high_water_bytes, 400);
    assert_eq!(stats.fill_percent(), 25);
}

#[test]
fn the_same_access_sequence_evicts_the_same_entries() {
    // No clock in the LRU, so the eviction set is a function of the access order alone — which is what
    // makes a cache's behaviour reviewable instead of machine-dependent.
    let once = || {
        let cache = BoundedCache::<String, String>::new(250);
        for index in 0..20 {
            cache.insert(format!("k{}", index % 7), "v".to_owned(), 50, "structure");
            cache.get_with(&format!("k{}", (index * 3) % 7), |_| ());
        }
        let mut keys: Vec<String> = cache
            .observe()
            .evicted_by_class
            .iter()
            .map(|(class, count)| format!("{class}:{count}"))
            .collect();
        keys.sort();
        (cache.observe(), keys)
    };
    assert_eq!(once(), once());
}

#[test]
fn hit_rate_is_absent_rather_than_zero_when_nothing_asked() {
    let cache = BoundedCache::<String, String>::new(10);
    assert_eq!(cache.observe().hit_rate(), None);
    cache.get_with(&"k".to_owned(), |_| ());
    assert_eq!(cache.observe().hit_rate(), Some(0.0));
    cache.insert("k".to_owned(), "v".to_owned(), 1, "x");
    cache.get_with(&"k".to_owned(), |_| ());
    assert_eq!(cache.observe().hit_rate(), Some(50.0));
}

// ---------------------------------------------------------------------------
// the bounded hand-off
// ---------------------------------------------------------------------------

#[test]
fn a_full_queue_makes_the_producer_wait_instead_of_growing() {
    // The back-pressure acceptance criterion, as a mechanism: the consumer is slow on purpose, and the
    // queue's high-water mark is the proof that the producer had more work ready and was held.
    const DEPTH: usize = 3;
    const ITEMS: usize = 40;
    let queue = std::sync::Arc::new(Queue::<usize>::new(DEPTH, 0));

    let producing = queue.clone();
    let producer = std::thread::spawn(move || {
        for index in 0..ITEMS {
            // Every push after the third has to wait for a pop: a consumer that takes 5 ms and a
            // producer that takes none is a guaranteed backlog.
            producing
                .push(index, 1)
                .expect("the consumer is still there");
        }
        producing.close();
    });

    let mut seen = Vec::new();
    while let Some(item) = queue.pop() {
        std::thread::sleep(std::time::Duration::from_millis(2));
        seen.push(item);
    }
    producer.join().unwrap();

    assert_eq!(seen, (0..ITEMS).collect::<Vec<_>>(), "nothing is lost");
    let stats = queue.observe();
    assert!(
        stats.high_water_items <= DEPTH,
        "{} items waited against a depth of {DEPTH}: the queue is not bounded",
        stats.high_water_items
    );
    assert!(
        stats.blocks > 0,
        "a producer that never waited was never blocked: {stats:?}"
    );
    assert_eq!(stats.pushed_items, ITEMS as u64);
}

#[test]
fn a_consumer_that_goes_wakes_the_producer_early() {
    // The failure mode this guards is a hung index: a writer thread that died on a SQLite error must
    // not leave the normalizer parked on a full queue holding the last batch nobody will take.
    let queue = std::sync::Arc::new(Queue::<usize>::new(2, 0));
    let producing = queue.clone();
    let producer = std::thread::spawn(move || {
        let mut pushed = 0;
        for index in 0..1000 {
            if producing.push(index, 1).is_err() {
                return pushed;
            }
            pushed += 1;
        }
        pushed
    });
    // Let the producer fill the queue and park, then walk away without draining it.
    std::thread::sleep(std::time::Duration::from_millis(30));
    queue.abort();
    let pushed = producer.join().unwrap();
    assert!(
        pushed < 1000,
        "the producer finished anyway ({pushed}), so the abort is not what stopped it"
    );
    assert_eq!(queue.observe().aborts, 1);
}

#[test]
fn the_byte_ceiling_bounds_a_batch_by_size_as_well_as_by_count() {
    // The spine's real shape: batches are wildly different sizes, and the ceiling that matters for RSS
    // is the byte one. An oversized single item still goes through — refusing a unit of work for being
    // big would stall the pipeline for a reason no knob can fix.
    let queue = std::sync::Arc::new(Queue::<usize>::new(64, 100));
    let producing = queue.clone();
    let producer = std::thread::spawn(move || {
        for index in 0..30 {
            producing.push(index, 40).expect("no abort");
        }
        producing.close();
    });
    let mut taken = 0;
    while queue.pop().is_some() {
        taken += 1;
    }
    producer.join().unwrap();
    assert_eq!(taken, 30);
    let stats = queue.observe();
    assert!(
        stats.high_water_bytes <= 140,
        "the byte ceiling is advisory per item, but 30 × 40 bytes must not all be resident: {}",
        stats.high_water_bytes
    );
    assert!(stats.blocks > 0, "and the producer was held: {stats:?}");
}

#[test]
fn a_closed_queue_drains_before_it_ends() {
    let queue = Queue::<usize>::new(4, 0);
    queue.push(1, 1).unwrap();
    queue.push(2, 1).unwrap();
    queue.close();
    assert_eq!(queue.pop(), Some(1));
    assert_eq!(queue.pop(), Some(2));
    assert_eq!(queue.pop(), None, "closed and drained");
    assert_eq!(queue.observe().pushed_items, 2);
}

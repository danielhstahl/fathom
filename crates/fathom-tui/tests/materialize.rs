//! The queue and the cache behind `e1.8`, tested as themselves rather than through a pane.
//!
//! The pane-level criteria live in `tests/diff_materialize.rs`; this file is about the two promises
//! those tests can only imply — that the in-flight cap is a wall and that the cache cap is a wall —
//! and about the arithmetic of who gets served first when there is more work than workers.
// The numbers these tests assert are measurements, and the measurement is what a reader has to see to
// believe the assertion — so the prints stay, and `cargo clippy --all-targets` stays quiet about them.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(clippy::print_stdout)]

use std::sync::Arc;
use std::time::Duration;

use fathom_tui::event::{Bus, Message, Received, WorkerMsg};
use fathom_tui::materialize::{
    Admit, Cache, Cost, Engine, Insert, Job, Outcome, Priority, Scheduler, Work,
};

/// A test payload with an honest byte count, because a cache cap can only be tested against a value
/// that can say how much it weighs.
#[derive(Debug)]
struct Chunk(Vec<u8>);

impl Cost for Chunk {
    fn cost(&self) -> usize {
        self.0.len()
    }
}

fn job(key: &str, priority: Priority) -> Job<String> {
    Job::new(key.to_owned(), priority, 1)
}

/// A cache key, spelled the way a `Cache<String, _>` wants it.
fn k(key: &str) -> String {
    key.to_owned()
}

// --- the in-flight cap -------------------------------------------------------------------------------

#[test]
fn the_cap_refuses_the_job_that_would_breach_it_and_counts_the_refusal() {
    let mut queue = Scheduler::<String>::new(4);
    let mut refusals = Vec::new();
    for n in 0..20 {
        refusals.push(queue.submit(job(&format!("hunk-{n}"), Priority::Visible)));
    }
    assert_eq!(queue.in_flight(), 4, "four is the cap");
    assert_eq!(queue.queued(), 4);
    assert_eq!(queue.running(), 0);
    assert_eq!(
        refusals
            .iter()
            .filter(|admit| **admit == Admit::DroppedFull)
            .count(),
        16,
        "sixteen jobs were refused, and every one of them was counted"
    );
    assert_eq!(queue.counts.dropped, 16);
    assert_eq!(
        queue.counts.peak_in_flight, 4,
        "and the queue knows its own high-water mark"
    );
}

#[test]
fn a_visible_job_evicts_a_prefetch_job_but_not_another_visible_one() {
    // The cap is not a door that shuts: what is on screen buys its place by pushing out a guess.
    let mut queue = Scheduler::<String>::new(2);
    assert_eq!(
        queue.submit(job("guess-a", Priority::Prefetch)),
        Admit::Queued
    );
    assert_eq!(
        queue.submit(job("guess-b", Priority::Prefetch)),
        Admit::Queued
    );
    let report = queue.submit_all(vec![
        job("screen-a", Priority::Visible),
        job("screen-b", Priority::Visible),
    ]);
    assert_eq!(report.queued, 2, "both visible jobs got in");
    assert_eq!(report.displaced, 2, "two guesses were evicted for them");
    assert_eq!(queue.counts.superseded, 2);
    let keys = queue.pending_keys();
    assert!(keys.contains(&k("screen-a")));
    assert!(keys.contains(&k("screen-b")));
    assert!(
        !keys.contains(&k("guess-a")),
        "the guess outlived the window"
    );
    assert_eq!(
        queue.submit(job("screen-c", Priority::Visible)),
        Admit::DroppedFull,
        "with nothing weaker left to evict, the third visible job waits for a free worker"
    );

    // Two visible jobs against each other: the cap decides, not a hunch about order.
    let mut queue = Scheduler::<String>::new(1);
    assert_eq!(queue.submit(job("first", Priority::Visible)), Admit::Queued);
    assert_eq!(
        queue.submit(job("second", Priority::Visible)),
        Admit::DroppedFull,
        "an equally-deserving job cannot displace one"
    );
}

#[test]
fn the_same_key_is_never_queued_twice() {
    let mut queue = Scheduler::<String>::new(8);
    assert_eq!(
        queue.submit(job("hunk-1", Priority::Visible)),
        Admit::Queued
    );
    assert_eq!(
        queue.submit(job("hunk-1", Priority::Visible)),
        Admit::AlreadyQueued,
        "a scroll over the same hunk must not double its work"
    );
    assert_eq!(queue.in_flight(), 1);
    assert_eq!(queue.counts.duplicated, 1);
}

#[test]
fn the_visible_tier_is_emptied_before_the_guesses() {
    let mut queue = Scheduler::<String>::new(16);
    queue.submit_all(vec![
        job("guess", Priority::Prefetch),
        job("next", Priority::Adjacent),
        job("screen", Priority::Visible),
    ]);
    let order: Vec<String> = (0..3)
        .filter_map(|_| queue.take_next())
        .map(|running| running.key)
        .collect();
    assert_eq!(
        order,
        vec!["screen".to_owned(), "next".to_owned(), "guess".to_owned()],
        "attention is served in order, not in arrival order"
    );
}

#[test]
fn cancelling_reaches_queued_work_and_tells_running_work_to_stop() {
    let mut queue = Scheduler::<String>::new(8);
    queue.submit_all((0..6).map(|n| job(&format!("hunk-{n}"), Priority::Visible)));
    // A scroll moves the whole window at once, so this is the shape the diff materialiser actually
    // asks for: everything out, including the one a worker is already half-way through.
    let running = queue.take_next().expect("six are queued");
    let freed = queue.cancel_where(|_| true);
    assert_eq!(
        freed, 6,
        "five were taken out of the queue, one was told to stop"
    );
    assert_eq!(queue.counts.superseded, 5, "five never started");
    assert_eq!(queue.counts.interrupted, 1, "one was mid-flight");
    assert!(running.cancel.is_cancelled(), "a running job got the word");
    assert_eq!(
        queue.in_flight(),
        1,
        "the running one is still counted — it is still reading, whatever the queue says"
    );
}

#[test]
fn an_older_generation_supersedes_itself() {
    let mut queue = Scheduler::<String>::new(16);
    queue.submit_all((0..8).map(|n| Job::new(format!("old-{n}"), Priority::Visible, 1)));
    assert_eq!(
        queue.supersede_before(2),
        8,
        "every job from generation 1 is gone"
    );
    queue.submit_all((0..3).map(|n| Job::new(format!("new-{n}"), Priority::Adjacent, 2)));
    assert_eq!(
        queue.supersede_before(2),
        0,
        "generation 2 is current, not stale"
    );
    assert_eq!(queue.queued(), 3);
}

// --- the bounded cache -------------------------------------------------------------------------------

#[test]
fn the_cache_evicts_down_to_its_cap_and_never_over_it() {
    // The unit test the acceptance criterion asks for: enough values that an unbounded cache would
    // hold four times the cap, and the cap holding anyway.
    let mut cache = Cache::<String, Chunk>::new(100);
    for n in 0..10 {
        let insert = cache.insert(format!("hunk-{n}"), Arc::new(Chunk(vec![n as u8; 30])), 30);
        assert!(matches!(insert, Insert::Cached { .. }), "{insert:?}");
        assert!(
            cache.bytes() <= 100,
            "the cache went over its cap on the way in: {} B",
            cache.bytes()
        );
    }
    let stats = cache.stats();
    assert!(
        stats.evictions >= 7,
        "ten values of 30 B against a 100 B cap: {stats:?}"
    );
    assert!(
        stats.refused == 0,
        "each one fits on its own, so none should be refused"
    );
    assert_eq!(cache.len(), 3, "three fit");
    assert_eq!(cache.bytes(), 90);
    assert!(
        !cache.contains(&k("hunk-0")) && cache.contains(&k("hunk-9")),
        "the oldest went first: {:?}",
        cache.keys()
    );
}

#[test]
fn reading_a_value_makes_it_the_last_to_go() {
    let mut cache = Cache::<String, Chunk>::new(60);
    for key in ["a", "b", "c"] {
        cache.insert(key.to_owned(), Arc::new(Chunk(vec![0; 20])), 20);
    }
    assert!(cache.get(&k("a")).is_some(), "a hit on the oldest");
    cache.insert(k("d"), Arc::new(Chunk(vec![0; 20])), 20);
    assert!(cache.contains(&k("a")), "the value that was read survived");
    assert!(!cache.contains(&k("b")), "the one nobody looked at went");
    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);
    assert!(
        cache.get(&k("nope")).is_none(),
        "a miss is counted as a miss"
    );
    assert_eq!(cache.stats().misses, 1);
}

#[test]
fn a_value_bigger_than_the_whole_cache_is_refused_not_squeezed() {
    let mut cache = Cache::<String, Chunk>::new(100);
    let insert = cache.insert(k("whale"), Arc::new(Chunk(vec![0; 1_000])), 1_000);
    assert_eq!(
        insert,
        Insert::TooBig {
            cost: 1_000,
            cap: 100
        }
    );
    assert_eq!(cache.len(), 0, "and nothing was stored");
    assert_eq!(
        cache.stats().refused,
        1,
        "the refusal is a number a test can read"
    );
    // And the refusal does not poison the cache: normal values still go in afterwards.
    cache.insert(k("minnow"), Arc::new(Chunk(vec![0; 10])), 10);
    assert_eq!(cache.len(), 1);
}

#[test]
fn replacing_a_value_uses_its_slot_and_not_two() {
    let mut cache = Cache::<String, Chunk>::new(100);
    cache.insert(k("hunk"), Arc::new(Chunk(vec![0; 40])), 40);
    cache.insert(k("hunk"), Arc::new(Chunk(vec![1; 40])), 40);
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.bytes(), 40, "one hunk, one slot");
    assert_eq!(
        cache.stats().evictions,
        0,
        "a replacement is not an eviction"
    );
}

#[test]
fn removing_by_predicate_frees_the_bytes_it_claims_to() {
    let mut cache = Cache::<String, Chunk>::new(1_000);
    for (origin, n) in [("old", 3), ("new", 2)] {
        for hunk in 0..n {
            let key = format!("{origin}/{hunk}");
            cache.insert(key, Arc::new(Chunk(vec![0; 100])), 100);
        }
    }
    assert_eq!(cache.bytes(), 500);
    assert_eq!(cache.remove_where(|key, _| key.starts_with("old/")), 3);
    assert_eq!(cache.bytes(), 200, "the bytes went with the entries");
    assert_eq!(cache.len(), 2);
}

// --- the engine: the two walls with threads on them --------------------------------------------------

fn doubler() -> Work<String, Chunk> {
    Arc::new(|job: &Job<String>, _cancel| Outcome::Ready(Chunk(job.key.as_bytes().to_vec())))
}

#[test]
fn the_engine_caches_what_a_worker_ran_and_reports_on_the_bus() {
    let engine = Engine::<String, Chunk>::new("test decode", 4_096, 16);
    let bus = Bus::new();
    let handles = engine.spawn(Some(Arc::clone(&bus)), 2, doubler());
    let report = engine.submit((0..10).map(|n| job(&format!("hunk-{n}"), Priority::Visible)));
    assert_eq!(
        report.queued, 10,
        "the cap has room for all ten: {report:?}"
    );

    // The work gets done without anyone driving it, which is the only thing threads are for here.
    let mut seen: Vec<String> = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    for n in 0..10 {
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "the workers stalled on hunk-{n}: {:?}",
                engine.stats()
            );
            if engine.cached(&k(&format!("hunk-{n}"))).is_some() {
                break;
            }
            if let Received::Message(message) = bus.recv_timeout(Duration::from_millis(20)) {
                seen.push(format!("{message:?}"));
            }
        }
    }
    // The last message can still be in the bus queue, so empty it before judging what was said.
    while let Received::Message(message) = bus.recv_timeout(Duration::from_millis(20)) {
        seen.push(format!("{message:?}"));
    }
    let said = seen;
    assert!(
        said.iter().any(|line| line.contains("Progress")),
        "no progress was reported: {said:?}"
    );
    assert!(
        said.iter().any(|line| line.contains("Quiet")),
        "the burst never announced itself over: {said:?}"
    );
    drop(handles);
}

#[test]
fn a_worker_that_panics_reports_a_death_instead_of_taking_the_terminal_with_it() {
    let engine = Engine::<String, Chunk>::new("test decode", 4_096, 4);
    let bus = Bus::new();
    let work: Work<String, Chunk> = Arc::new(|job: &Job<String>, _cancel| {
        if job.key == "boom" {
            panic!("the decode walked off the end");
        }
        Outcome::Ready(Chunk(vec![0]))
    });
    let handles = engine.spawn(Some(Arc::clone(&bus)), 1, work);
    engine.submit(vec![
        job("boom", Priority::Visible),
        job("fine", Priority::Visible),
    ]);

    let mut died = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while died.is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "nothing reported the panic"
        );
        if let Received::Message(Message::Worker(WorkerMsg::Died { task, why })) =
            bus.recv_timeout(Duration::from_millis(20))
        {
            assert_eq!(task, "test decode");
            died = Some(why);
        }
    }
    assert!(
        died.as_ref()
            .expect("a death message")
            .contains("walked off the end"),
        "the message did not say what happened: {died:?}"
    );
    // The worker survived its own panic: the second job still gets done, which is the whole point.
    for _ in 0..500 {
        if engine.cached(&k("fine")).is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        engine.cached(&k("fine")).is_some(),
        "one bad decode stopped the queue"
    );
    drop(handles);
}

#[test]
fn a_failure_is_stated_against_the_key_that_failed_and_not_merged_into_the_rest() {
    let engine = Engine::<String, Chunk>::new("test decode", 4_096, 4);
    let work: Work<String, Chunk> = Arc::new(|job: &Job<String>, _cancel| {
        if job.key == "bad" {
            Outcome::Failed("the hunk header lies about its length".to_owned())
        } else {
            Outcome::Ready(Chunk(vec![0]))
        }
    });
    engine.submit(vec![
        job("bad", Priority::Visible),
        job("good", Priority::Visible),
    ]);
    assert_eq!(engine.drain(&work), 2, "two jobs ran on this thread");
    let why = engine
        .failure(&"bad".to_owned())
        .expect("the failure is on record");
    assert!(why.contains("hunk header"), "{why}");
    assert!(
        engine.failure(&"good".to_owned()).is_none(),
        "an unrelated key inherited the complaint"
    );
    assert!(engine.cached(&"good".to_owned()).is_some());
}

#[test]
fn a_cancelled_job_is_not_cached() {
    let engine = Engine::<String, Chunk>::new("test decode", 4_096, 4);
    let running = {
        engine.submit(vec![job("gone", Priority::Visible)]);
        engine.take().expect("queued")
    };
    running.cancel.cancel();
    engine.execute(&running, &doubler());
    assert!(
        engine.cached(&k("gone")).is_none(),
        "a cancelled decode wrote itself into the cache anyway"
    );
    assert_eq!(
        engine.stats().counts.cancelled,
        1,
        "and it was counted as a cancellation"
    );
}

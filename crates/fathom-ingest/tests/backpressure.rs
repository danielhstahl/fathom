//! Back-pressure: a slow sink makes the pipeline wait instead of growing (`reco-93w`).
//!
//! The acceptance criterion is "analyzers apply back-pressure instead of growing unbounded when the
//! queue backs up", and the way to test that is to *make* the queue back up: a sink that sleeps, a
//! producer that does not, and a high-water mark that says how far ahead the producer was allowed to
//! get. A queue depth that is never exceeded is a bound; a `queue_blocks` counter that is non-zero is
//! proof the bound was actually reached, and not an accident of a fast writer.
//!
//! The second half of the claim is that the ceiling is a *function*: the same repository, the same
//! records, two settings of `queue_batches`, and the in-flight bytes move with the knob while the
//! output does not move at all.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::Fixture;
use fathom_core::memory::Ceilings;
use fathom_ingest::ingest::{Collector, IngestOptions, RunId, RunKind, RunReport, Sink};
use fathom_ingest::record::{CommitRecord, Totals, Unclassified};
use fathom_ingest::repo::Repository;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// How many commits to run over. Enough for a dozen batches at the batch size below, which is what
/// makes "the producer was held" a certainty rather than a scheduling question.
const COMMITS: usize = 240;

/// The batch size, kept small so the queue depth is the thing under test and not the batch.
const BATCH: usize = 8;

/// A sink that is slow on purpose, and that reports how much work was in its hands.
#[derive(Default)]
struct SlowSink {
    /// Milliseconds spent per batch — the stand-in for a SQLite commit.
    lag: u64,
    /// Batches written.
    batches: AtomicUsize,
    /// Commit rows written.
    rows: AtomicUsize,
    /// The most commits ever handed over while the producer was still producing. Observed inside
    /// `write`, which is the only place that knows what is in flight.
    peak_in_flight: Mutex<usize>,
    /// Oids written, for the "nothing is lost" assertion.
    seen: Mutex<HashSet<String>>,
}

impl Sink for SlowSink {
    type Error = std::convert::Infallible;

    fn begin(&mut self, kind: RunKind) -> Result<RunId, Self::Error> {
        let _ = kind;
        Ok(RunId(1))
    }

    fn known(
        &self,
        _oids: &[fathom_ingest::oid::Oid],
    ) -> Result<HashSet<fathom_ingest::oid::Oid>, Self::Error> {
        Ok(HashSet::new())
    }

    fn write(&mut self, _run: RunId, batch: &[CommitRecord]) -> Result<usize, Self::Error> {
        if self.lag > 0 {
            std::thread::sleep(Duration::from_millis(self.lag));
        }
        let in_flight = batch.iter().map(|record| record.files.len()).sum::<usize>();
        let mut peak = self.peak_in_flight.lock().unwrap();
        *peak = (*peak).max(in_flight);
        drop(peak);
        {
            let mut seen = self.seen.lock().unwrap();
            for record in batch {
                seen.insert(record.oid.hex().to_owned());
            }
        }
        self.batches.fetch_add(1, Ordering::Relaxed);
        self.rows.fetch_add(batch.len(), Ordering::Relaxed);
        Ok(batch.len())
    }

    fn finish(&mut self, _run: RunId, _report: &RunReport) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn fixture() -> (Fixture, std::path::PathBuf) {
    let fixture = Fixture::new("backpressure");
    let root = fixture.init();
    fixture.linear_history(&root, COMMITS);
    (fixture, root)
}

fn options(ceilings: Ceilings) -> IngestOptions {
    IngestOptions {
        // One worker: the point is the hand-off, and a parallel normalizer would make the timing about
        // rayon rather than about the queue.
        parallelism: 1,
        commits_per_batch: BATCH,
        resume: false,
        budget: None,
        ceilings,
        ..IngestOptions::default()
    }
}

fn run_slow(root: &std::path::Path, ceilings: Ceilings, lag: u64) -> (RunReport, SlowSink) {
    let repo = Repository::discover(root).expect("discover");
    let mut sink = SlowSink {
        lag,
        ..SlowSink::default()
    };
    let report = fathom_ingest::ingest::run(&repo, &Unclassified, &options(ceilings), &mut sink)
        .expect("ingest");
    (report, sink)
}

/// The per-batch sink lag that makes the writer the bottleneck **on this machine**, measured.
///
/// Both tests below need the sink to be the slow stage: that is the only condition under which the
/// hand-off fills and the producer is held. A hard-coded millisecond figure cannot say that — 4 ms per
/// batch is a bottleneck in a release build and disappears into noise in a debug one, which turns the
/// test into a coin toss on the machine it was not written on. So the lag is measured rather than
/// asserted: time one identical run with an idle sink, take twice its per-batch cost, and the writer
/// is guaranteed to fall behind by construction on a laptop, a CI runner, debug or release.
fn measured_lag_ms(root: &std::path::Path) -> u64 {
    let (report, sink) = run_slow(root, ceilings_with(2), 0);
    let batches = u64::try_from(sink.batches.load(Ordering::Relaxed).max(1)).unwrap_or(1);
    let lag = (report.elapsed.as_millis() / u128::from(batches)).saturating_mul(2);
    u64::try_from(lag).unwrap_or(200).clamp(1, 200)
}

fn ceilings_with(depth: usize) -> Ceilings {
    Ceilings {
        queue_batches: depth,
        ..Ceilings::default()
    }
}

#[test]
fn a_slow_sink_holds_the_producer_at_the_queue_depth() {
    let (fixture, root) = fixture();
    // The floor keeps the sleep real on a machine fast enough that a batch of eight commits costs a
    // microsecond; the measurement is what makes it a bottleneck on a machine slow enough that 4 ms is
    // nothing.
    let lag = measured_lag_ms(&root).max(2);
    let (report, sink) = run_slow(&root, ceilings_with(2), lag);
    assert_eq!(report.commits_written, COMMITS, "nothing was dropped");
    assert_eq!(sink.rows.load(Ordering::Relaxed), COMMITS);
    assert!(
        report.queue_high_water_batches <= 2,
        "{} batches were in flight against a depth of 2: the hand-off is not bounded",
        report.queue_high_water_batches
    );
    assert!(
        report.queue_blocks > 0,
        "the producer was never held, so this run proved nothing about back-pressure: {:?}",
        report.queue_blocks
    );
    assert!(
        report.queue_high_water_bytes > 0,
        "the hand-off's byte ledger is empty, so `queue_high_water_batches` is decoration"
    );
    assert!(
        sink.batches.load(Ordering::Relaxed) >= COMMITS / BATCH,
        "the sink saw {} batches; the run should have produced about {}",
        sink.batches.load(Ordering::Relaxed),
        COMMITS / BATCH
    );
    drop(fixture);
}

#[test]
fn the_in_flight_bytes_are_a_function_of_the_depth() {
    // The ticket's claim in its smallest form: move the knob, the resident bytes move with it, and the
    // records do not.
    let (fixture, root) = fixture();
    let (one, _) = run_slow(&root, ceilings_with(1), 3);
    let (four, _) = run_slow(&root, ceilings_with(4), 3);
    assert_eq!(
        one.commits_written, four.commits_written,
        "the same history produced the same work at either depth"
    );
    assert_eq!(one.queue_high_water_batches, 1, "depth 1 is depth 1");
    assert!(
        four.queue_high_water_batches <= 4,
        "depth 4 held {} batches",
        four.queue_high_water_batches
    );
    assert!(
        one.queue_high_water_bytes <= four.queue_high_water_bytes,
        "a quarter of the queue held more bytes ({} vs {})",
        one.queue_high_water_bytes,
        four.queue_high_water_bytes
    );
    drop(fixture);
}

#[test]
fn the_records_are_the_same_whatever_the_depth_is() {
    // Determinism is the property this could easily have broken: two stages on two threads is exactly
    // the shape that starts writing in arrival order.
    let (fixture, root) = fixture();
    let repo = Repository::discover(&root).expect("discover");
    let mut first = Collector::default();
    fathom_ingest::ingest::run(&repo, &Unclassified, &options(ceilings_with(1)), &mut first)
        .unwrap();
    let mut second = Collector::default();
    fathom_ingest::ingest::run(
        &repo,
        &Unclassified,
        &options(ceilings_with(5)),
        &mut second,
    )
    .unwrap();
    assert_eq!(first.records.len(), COMMITS);
    assert_eq!(
        first.records, second.records,
        "a queue depth is a memory knob, not a semantic one"
    );
    assert_eq!(
        first.records.first().map(|record| record.oid),
        second.records.first().map(|record| record.oid),
        "and the order is walk order at either depth"
    );
    drop(fixture);
}

#[test]
fn a_sink_that_fails_midway_ends_the_run_instead_of_hanging_it() {
    // The failure back-pressure creates: a writer that stops must wake the producer it left behind. A
    // hung `fathom index` on a full-disk store is the bug this guards.
    use std::sync::atomic::AtomicBool;

    struct FailingSink {
        fail_after: usize,
        writes: usize,
        failed: std::sync::Arc<AtomicBool>,
    }

    impl Sink for FailingSink {
        type Error = fathom_ingest::error::Error;

        fn begin(&mut self, _kind: RunKind) -> Result<RunId, Self::Error> {
            Ok(RunId(1))
        }

        fn known(
            &self,
            _oids: &[fathom_ingest::oid::Oid],
        ) -> Result<HashSet<fathom_ingest::oid::Oid>, Self::Error> {
            Ok(HashSet::new())
        }

        fn write(&mut self, _run: RunId, batch: &[CommitRecord]) -> Result<usize, Self::Error> {
            self.writes += 1;
            if self.writes > self.fail_after {
                self.failed.store(true, Ordering::SeqCst);
                return Err(fathom_ingest::error::Error::StoreWrite {
                    reason: "disk full (test)".to_owned(),
                });
            }
            Ok(batch.len())
        }

        fn finish(&mut self, _run: RunId, _report: &RunReport) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    let (fixture, root) = fixture();
    let repo = Repository::discover(&root).expect("discover");
    let failed = std::sync::Arc::new(AtomicBool::new(false));
    let mut sink = FailingSink {
        fail_after: 3,
        writes: 0,
        failed: failed.clone(),
    };
    // A depth of 1 with 30 batches of work means the producer had to be woken: it had 27 more batches
    // ready and nowhere to put them.
    let result =
        fathom_ingest::ingest::run(&repo, &Unclassified, &options(ceilings_with(1)), &mut sink);
    let error = result.expect_err("the sink failed, so the run must say so");
    assert!(
        error.to_string().contains("disk full"),
        "{error} is not the sink's own complaint"
    );
    assert!(failed.load(Ordering::SeqCst));
    drop(fixture);
}

#[test]
fn the_write_stage_overlaps_the_normalize_stage() {
    // The other half of the change: bounding the hand-off also let the two stages run at the same time.
    // The assertion is a difference, not an absolute — wall time on a shared machine is not a fact — and
    // it says that per-batch sink cost is *hidden* by normalization rather than added to it. A serial
    // pipeline would pay `baseline + lag × batches`; an overlapping one pays about the larger of the two.
    let (fixture, root) = fixture();
    let repo = Repository::discover(&root).expect("discover");
    // Derived from the same measurement as the assertion's own arithmetic: the sink's total cost has to
    // exceed the producer's for "hidden" to be a claim about overlap rather than about a lag too small
    // to notice.
    let lag_batches = measured_lag_ms(&root);

    let mut fast = SlowSink::default();
    let baseline =
        fathom_ingest::ingest::run(&repo, &Unclassified, &options(ceilings_with(2)), &mut fast)
            .expect("ingest")
            .elapsed;

    let mut slow = SlowSink {
        lag: lag_batches,
        ..SlowSink::default()
    };
    let slowed =
        fathom_ingest::ingest::run(&repo, &Unclassified, &options(ceilings_with(2)), &mut slow)
            .expect("ingest");
    let batches = u64::try_from(slow.batches.load(Ordering::Relaxed)).unwrap_or(0);
    let serialized = batches * lag_batches;
    let serial_would_cost = baseline.as_millis() + u128::from(serialized);
    assert_eq!(slowed.kind, RunKind::Cold);
    assert!(
        slowed.elapsed.as_millis() < serial_would_cost,
        "{} ms of wall time for {batches} batches at {lag_batches} ms each on top of a {} ms run: the \
         sink's cost was added, not hidden",
        slowed.elapsed.as_millis(),
        baseline.as_millis()
    );
    assert!(
        slowed.elapsed.as_millis() > baseline.as_millis(),
        "the lag changed nothing at all, so the two runs measured the same thing"
    );
    drop(fixture);
}

#[test]
fn a_record_is_counted_for_the_bytes_it_owns() {
    // `queue_high_water_bytes` is only a ceiling if the per-record estimate is the right shape: it grows
    // with the paths and hunk headers a record carries, and it does not pretend to hold blob bytes.
    let (fixture, root) = fixture();
    let repo = Repository::discover(&root).expect("discover");
    let mut collector = Collector::default();
    fathom_ingest::ingest::run(
        &repo,
        &Unclassified,
        &options(ceilings_with(2)),
        &mut collector,
    )
    .unwrap();
    let record = collector.records.first().expect("a record");
    let bytes = record.heap_bytes();
    assert!(bytes > 0);
    assert!(
        bytes < 4096,
        "a one-file commit's record estimated {bytes} bytes; the estimate is counting something it does \
         not own"
    );
    let totals = Totals {
        files: record.files.len(),
        ..Totals::default()
    };
    assert_eq!(totals.files, 1);
    drop(fixture);
}

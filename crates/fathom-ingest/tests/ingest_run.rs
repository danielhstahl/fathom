//! The pipeline: batching, parallel determinism, budget, resume (e1.2 / reco-76g).
//!
//! These are the ticket's "re-running ingestion is a no-op and interruption resumes from last durable
//! commit", plus the property that makes the parallel path defensible: the records do not depend on how
//! many threads produced them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Change, Fixture, StoreSink, date_at, export_text, git, git_head, projection};
use fathom_ingest::ingest::{Collector, IngestOptions, RunId, RunKind, RunReport, Sink, run};
use fathom_ingest::oid::Oid;
use fathom_ingest::record::{CommitRecord, Unclassified};
use fathom_ingest::repo::Repository;
use fathom_store::spine::{CommitRow, Store};
use std::collections::HashSet;
use std::path::Path;

/// A history of `count` ordinary commits, dates ascending so the walk order is the commit order reversed.
fn history(fixture: &Fixture, cwd: &Path, count: usize) {
    for index in 0..count {
        fixture.commit_changes(
            cwd,
            &format!("commit {index}"),
            &date_at(i64::try_from(index).unwrap_or(0)),
            // Distinct content every time, so no two commits share a blob: a test that deletes one object
            // then damages exactly one commit, which is what the damage test claims to test.
            &[Change::Rewrite {
                path: "file.txt",
                lines: index + 1,
            }],
        );
    }
}

#[test]
fn a_run_walks_normalizes_and_reports_what_it_did() {
    let fixture = Fixture::new("run-basic");
    let repo_dir = fixture.init();
    history(&fixture, &repo_dir, 20);
    let repo = Repository::discover(&repo_dir).expect("open");

    let mut collector = Collector::default();
    let report = run(
        &repo,
        &Unclassified,
        &IngestOptions {
            commits_per_batch: 8,
            parallelism: 4,
            ..IngestOptions::default()
        },
        &mut collector,
    )
    .expect("ingest");

    assert_eq!(report.commits_seen, 20);
    assert_eq!(report.commits_written, 20);
    assert_eq!(report.commits_existing, 0);
    assert!(report.failures.is_empty());
    assert!(!report.interrupted);
    assert_eq!(collector.records.len(), 20);
    assert_eq!(
        collector.batches, 3,
        "20 commits at 8 a batch is three writes"
    );
    assert_eq!(
        report.rows_written,
        collector
            .records
            .iter()
            .map(|record| 1 + record.files.len())
            .sum::<usize>(),
        "rows are commit rows plus file rows"
    );
    assert_eq!(report.totals.files, 20);
    assert!(report.commits_per_second() > 0);
    assert_eq!(report.workers, 4);
    assert_eq!(
        collector.records.first().unwrap().message.head,
        "commit 19",
        "the walk is newest-first and the collector keeps that order"
    );
}

#[test]
fn one_worker_and_sixteen_produce_the_same_bytes() {
    // The determinism claim in `ingest.rs`'s module doc, measured rather than asserted in prose.
    let fixture = Fixture::new("run-parallel");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let repo = Repository::discover(&repo_dir).expect("open");

    let mut one = Collector::default();
    run(
        &repo,
        &Unclassified,
        &IngestOptions {
            parallelism: 1,
            commits_per_batch: 3,
            ..IngestOptions::default()
        },
        &mut one,
    )
    .expect("ingest serially");

    let mut many = Collector::default();
    run(
        &repo,
        &Unclassified,
        &IngestOptions {
            parallelism: 16,
            commits_per_batch: 512,
            ..IngestOptions::default()
        },
        &mut many,
    )
    .expect("ingest in parallel");

    assert_eq!(
        export_text(&one.records),
        export_text(&many.records),
        "thread scheduling must not reach the bytes"
    );
}

#[test]
fn a_second_run_writes_nothing_and_leaves_the_store_untouched() {
    let fixture = Fixture::new("run-idempotent");
    let repo_dir = fixture.init();
    common::mixed(&fixture, &repo_dir);
    let repo = Repository::discover(&repo_dir).expect("open");
    let mut store = Store::open_in_memory().expect("store");

    let first = run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut StoreSink::new(&mut store),
    )
    .expect("first ingest");
    assert!(first.commits_written > 3);
    let durable_after_first = store.read_all().expect("rows");
    let counts_after_first = store.counts().expect("counts");

    let second = run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut StoreSink::new(&mut store),
    )
    .expect("second ingest");

    assert_eq!(
        second.commits_written, 0,
        "every commit was already durable, so nothing is redone"
    );
    assert_eq!(second.commits_existing, first.commits_written);
    assert_eq!(second.commits_seen, first.commits_seen);
    assert_eq!(store.counts().expect("counts"), counts_after_first);
    assert_eq!(store.read_all().expect("rows"), durable_after_first);
    store.check().expect("integrity after a no-op run");

    // And the durable set is exactly what a fresh normalization of the same history produces — which is the
    // statement that the first run's resume-skipping did not silently drop a commit.
    let mut fresh = Collector::default();
    run(
        &repo,
        &Unclassified,
        &IngestOptions {
            resume: false,
            ..IngestOptions::default()
        },
        &mut fresh,
    )
    .expect("re-normalize everything");
    // The store returns rows in id order (that is what `ORDER BY oid` means); the collector kept walk
    // order, so the comparison sorts the same way rather than pretending the two agree.
    let mut expected = fresh.records.clone();
    fathom_ingest::export::sort_commits(&mut expected);
    let expected: Vec<CommitRow> = expected.iter().map(projection).collect();
    let stored_oids: Vec<String> = durable_after_first
        .iter()
        .map(|row| row.oid.clone())
        .collect();
    let expected_oids: Vec<String> = expected.iter().map(|row| row.oid.clone()).collect();
    assert_eq!(
        stored_oids, expected_oids,
        "the same commits, in the same order"
    );
    assert_eq!(
        durable_after_first, expected,
        "the store holds every record's durable projection, in id order"
    );
}

#[test]
fn an_interrupted_run_resumes_from_the_last_durable_commit() {
    let fixture = Fixture::new("run-resume");
    let repo_dir = fixture.init();
    history(&fixture, &repo_dir, 30);
    let repo = Repository::discover(&repo_dir).expect("open");

    // The interrupted run: a budget of 7, in batches of 3, so it stops mid-history with durable batches.
    let mut store = Store::open_in_memory().expect("store");
    let partial = run(
        &repo,
        &Unclassified,
        &IngestOptions {
            commits_per_batch: 3,
            budget: Some(7),
            ..IngestOptions::default()
        },
        &mut StoreSink::new(&mut store),
    )
    .expect("partial ingest");
    assert!(
        partial.interrupted,
        "the budget tripped, and the run says so"
    );
    assert_eq!(partial.commits_written, 7);
    assert_eq!(store.counts().expect("counts").0, 7);

    // The resume: same repository, same store, no budget.
    let finished = run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut StoreSink::new(&mut store),
    )
    .expect("resumed ingest");
    assert_eq!(
        finished.commits_written, 23,
        "the rest of the history, and none of what was already there"
    );
    assert_eq!(store.counts().expect("counts").0, 30);

    // The claim that matters: resuming produced exactly what one uninterrupted run would have.
    let mut alone = Store::open_in_memory().expect("store");
    run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut StoreSink::new(&mut alone),
    )
    .expect("single ingest");
    assert_eq!(
        store.read_all().expect("resumed rows"),
        alone.read_all().expect("whole rows"),
        "a resumed store is indistinguishable from a store that was never interrupted"
    );
}

#[test]
fn a_failed_batch_leaves_the_previous_ones_durable() {
    // The kill -9 shape, minus the signal: the sink fails mid-run, and what was already committed stays put
    // and gets skipped on the next run rather than written twice.
    let fixture = Fixture::new("run-sink-fails");
    let repo_dir = fixture.init();
    history(&fixture, &repo_dir, 10);
    let repo = Repository::discover(&repo_dir).expect("open");
    let mut store = Store::open_in_memory().expect("store");

    let err = run(
        &repo,
        &Unclassified,
        &IngestOptions {
            commits_per_batch: 4,
            ..IngestOptions::default()
        },
        &mut StoreSink {
            store: &mut store,
            now: 1_700_000_000_000,
            fail_on_write: Some(2),
            writes: 0,
        },
    )
    .expect_err("the sink fails on the second batch");
    assert!(err.to_string().contains("the sink gave up"), "{err}");
    assert_eq!(
        store.counts().expect("counts").0,
        4,
        "the first batch is durable, and the run's failure does not take it back"
    );

    let report = run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut StoreSink::new(&mut store),
    )
    .expect("resume after a failed run");
    assert_eq!(report.commits_written, 6);
    assert_eq!(store.counts().expect("counts").0, 10);
}

#[test]
fn a_commit_that_cannot_be_normalized_is_a_failure_not_a_lost_run() {
    // ADR-001 §7.6: one damaged object degrades the run, it does not end it.
    let fixture = Fixture::new("run-damaged");
    let repo_dir = fixture.init();
    history(&fixture, &repo_dir, 5);
    let head = git_head(&repo_dir);
    // Take the newest commit's only blob out of the object database.
    let blob = git(&repo_dir, &["rev-parse", &format!("{head}:file.txt")]);
    fixture.remove_object(&repo_dir, blob.trim());

    let repo = Repository::discover(&repo_dir).expect("open");
    let mut collector = Collector::default();
    let report = run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut collector,
    )
    .expect("the run survives a damaged commit");
    assert_eq!(report.commits_seen, 5);
    assert_eq!(report.commits_written, 4);
    assert_eq!(report.failures.len(), 1);
    assert!(
        report.failures[0].contains(&head[..8]),
        "the failure names the commit it could not do: {:?}",
        report.failures
    );
    assert_eq!(
        collector.records.len(),
        4,
        "and the other four are there, whole"
    );
}

#[test]
fn the_walk_notices_what_it_could_not_reach() {
    // A shallow clone: the run must say the history is truncated rather than report a complete one.
    let source = Fixture::new("run-shallow-source");
    let source_dir = source.init();
    history(&source, &source_dir, 12);
    source.allow_filter(&source_dir);
    let shallow_dir = source.clone_shallow(&source_dir, "run-shallow", 3);
    let repo = Repository::discover(&shallow_dir).expect("open");

    let mut collector = Collector::default();
    let report = run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut collector,
    )
    .expect("ingest a shallow clone");
    assert!(report.commits_seen >= 3);
    assert!(
        report.truncated.is_some(),
        "history did not reach the root, and the report says so"
    );
}

#[test]
fn a_repository_with_no_commits_is_an_empty_run_not_an_error() {
    let fixture = Fixture::new("run-unborn");
    let repo_dir = fixture.init();
    let repo = Repository::discover(&repo_dir).expect("open");
    let mut collector = Collector::default();
    let report = run(
        &repo,
        &Unclassified,
        &IngestOptions::default(),
        &mut collector,
    )
    .expect("ingest an unborn repository");
    assert_eq!(report.commits_seen, 0);
    assert_eq!(report.commits_written, 0);
    assert_eq!(report.kind, RunKind::Cold);
    assert!(collector.records.is_empty());
}

#[test]
fn the_sink_sees_each_batch_exactly_once_and_in_order() {
    // A cheap contract check on the sink protocol itself: one `begin`, one `finish`, `write` per batch, and
    // every commit delivered exactly once.
    let fixture = Fixture::new("run-contract");
    let repo_dir = fixture.init();
    history(&fixture, &repo_dir, 9);
    let repo = Repository::discover(&repo_dir).expect("open");

    let mut counter = CountingSink::default();
    run(
        &repo,
        &Unclassified,
        &IngestOptions {
            commits_per_batch: 4,
            ..IngestOptions::default()
        },
        &mut counter,
    )
    .expect("ingest");
    assert_eq!(counter.begins, 1);
    assert_eq!(counter.finishes, 1);
    assert_eq!(counter.writes, 3);
    assert_eq!(counter.seen.len(), 9);
    assert_eq!(
        counter.known_calls.get(),
        1,
        "the resume lookup runs once per run"
    );
}

/// A sink that only counts, to check the protocol `ingest::run` promises.
#[derive(Default)]
struct CountingSink {
    begins: usize,
    finishes: usize,
    writes: usize,
    known_calls: std::cell::Cell<usize>,
    seen: HashSet<Oid>,
}

impl Sink for CountingSink {
    type Error = std::convert::Infallible;

    fn begin(&mut self, _kind: RunKind) -> Result<RunId, Self::Error> {
        self.begins += 1;
        Ok(RunId(7))
    }

    fn known(&self, _oids: &[Oid]) -> Result<HashSet<Oid>, Self::Error> {
        self.known_calls.set(self.known_calls.get() + 1);
        Ok(HashSet::new())
    }

    fn write(&mut self, _run: RunId, batch: &[CommitRecord]) -> Result<usize, Self::Error> {
        self.writes += 1;
        for record in batch {
            assert!(self.seen.insert(record.oid), "a commit was written twice");
        }
        Ok(batch.len())
    }

    fn finish(&mut self, _run: RunId, _report: &RunReport) -> Result<(), Self::Error> {
        self.finishes += 1;
        Ok(())
    }
}

//! `ingest.rs` — the pipeline from a repository to durable, normalized records (e1.2 / reco-76g).
//!
//! Walk, normalize in parallel, write in batches, and be able to say afterwards exactly what was and was
//! not looked at. The four properties the ticket names are the four sections below, and each is a decision
//! rather than an accident.
//!
//! # Idempotent
//!
//! A commit is identified by its own id, and a commit id determines everything in its record: parents,
//! tree, people, message, and the tree diff against its base are all functions of objects git never
//! changes. So the run asks the sink which ids it already has ([`Sink::known`]), normalizes the rest, and
//! writes each record with an upsert that ignores a duplicate rather than replacing it — ADR-004 §4.0's
//! "never `INSERT OR REPLACE` on a parent with cascade children" is exactly why `git_commit` may not be
//! replaced underneath `git_commit_file`'s `ON DELETE CASCADE`. Running the same ingestion twice writes
//! nothing the second time, and the store is unchanged afterwards.
//!
//! # Resumable
//!
//! Records are written in batches, one transaction per batch ([`Sink::write`]), so an interruption loses at
//! most the batch that was in flight — and the next run sees those ids in [`Sink::known`] and does not
//! redo them. Nothing about a commit depends on which batch it landed in, so "resumes from the last durable
//! commit" needs no journal and no torn-state repair: the durable set *is* the progress.
//!
//! # Parallel, deterministically
//!
//! The walk is serial and cheap ([`crate::graph`] streams ids at a few million a second); the per-commit
//! work — tree diff, blob reads, line counting — is where a cold index is spent, and it is embarrassingly
//! parallel. Each worker gets its own repository handle ([`crate::repo::Repository`] is `Send` and not
//! `Sync`, so one handle per thread is the honest shape, ADR-002 §10) and a shard of the commit list.
//! Output order is restored by index before writing, so what this writes does not depend on thread
//! scheduling: the same repository produces the same records at `parallelism = 1` and at
//! `parallelism = 16`, which is what makes the golden export in [`crate::export`] worth committing.
//!
//! # Bounded
//!
//! Memory is bounded by the batch size, not by the repository: a batch is normalized, handed to the sink
//! and dropped before the next one starts, so no commit's per-file detail outlives its batch. Per file the
//! cap is [`crate::record::Limits::blob_memory_cap`], enforced in [`crate::normalize`]. The walk's own
//! footprint is the list of ids — 20 bytes a commit, so 2 MB for a 100k-commit history — which is what
//! [`crate::graph::Detail::Ids`] is for.
//!
//! # What this is not yet
//!
//! The writer lease (ADR-004 §7.1), the warm fast path that diffs only what moved since the stored tip, and
//! cancellation by keystroke are `e1.4`'s and `e6.3`'s. What is here is the sink interface they slot into
//! and the budget hook ([`IngestOptions::budget`]) that makes a stated partial a real code path rather than
//! a promise.

use std::collections::HashSet;
use std::fmt;
use std::time::{Duration, Instant};

use rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator};

use crate::error::Error;
use crate::graph::{Detail, Notice, Truncation, WalkOptions};
use crate::normalize::{self, Normalizer};
use crate::oid::Oid;
use crate::record::{CommitRecord, Taxonomy, Totals};
use crate::repo::Repository;

/// Which kind of run this is, in ADR-004 §4.1's vocabulary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunKind {
    /// The store had nothing for this repository.
    #[default]
    Cold,
    /// The store had some of it.
    Warm,
}

impl RunKind {
    /// The store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RunKind::Cold => "cold",
            RunKind::Warm => "warm",
        }
    }
}

impl fmt::Display for RunKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The sink's identity for one run, handed back by [`Sink::begin`] and echoed to every later call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunId(pub i64);

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Where the records go.
///
/// Four calls, no iterators, so a SQLite writer, an in-memory collector and a `--dry-run` counter can all
/// implement it — and so the composition root (`e1.10`) decides which one an invocation uses without this
/// crate ever naming a database.
///
/// The associated error is `Into<Error>` rather than `Error` so an implementation may return its own typed
/// failure and have it mapped at the boundary (ADR-002 §7.1).
pub trait Sink {
    /// The sink's own failure type, convertible into [`Error`].
    type Error: Into<Error>;

    /// Open a run. Implementations stamp any run they find still `running` as `crashed` here — ADR-004
    /// §9.2's recovery pass, whose point is that the process which *notices* writes it, because the dead
    /// one never can.
    fn begin(&mut self, kind: RunKind) -> Result<RunId, Self::Error>;

    /// Which of `oids` are already durable. This is the resume query and the idempotency check at once: one
    /// lookup per run, not one per commit.
    fn known(&self, oids: &[Oid]) -> Result<HashSet<Oid>, Self::Error>;

    /// Persist one batch atomically: when this returns, either every record in it is durable or none is.
    /// Returns the number of *rows* written (commit rows plus file rows), which is what
    /// `index_run.rows_written` counts.
    fn write(&mut self, run: RunId, batch: &[CommitRecord]) -> Result<usize, Self::Error>;

    /// Close the run with its totals. A run that ends in `Err` is never closed cleanly, which is what lets
    /// the next [`begin`](Self::begin) find it.
    fn finish(&mut self, run: RunId, report: &RunReport) -> Result<(), Self::Error>;
}

/// Everything about *how* to run that is neither the repository nor the taxonomy.
#[derive(Debug, Clone)]
pub struct IngestOptions {
    /// Which commits to walk; defaults to `HEAD`, whole history (see [`WalkOptions`]).
    pub walk: WalkOptions,
    /// Size caps, merge policy and rename detection ([`normalize::Options`]).
    pub normalize: normalize::Options,
    /// Worker threads for the per-commit work. `0` means "whatever rayon's pool has", the default.
    pub parallelism: usize,
    /// Commits per durable batch. Each batch is one transaction, so this is both the unit of work an
    /// interruption can lose and the unit of memory the pipeline holds at a time.
    pub commits_per_batch: usize,
    /// Skip commits the sink already has (the resume path). `false` re-normalizes everything, which the
    /// upsert still makes a no-op in the store but which pays the full diff cost again.
    pub resume: bool,
    /// Stop after this many new commits and report the run as interrupted — ADR-004 §7.6's stated partial,
    /// and the shape `e1.4`'s cancel key and `e6.3`'s CPU budget will drive. `None` runs to completion.
    pub budget: Option<usize>,
}

impl Default for IngestOptions {
    fn default() -> Self {
        IngestOptions {
            walk: WalkOptions::head(),
            normalize: normalize::Options::default(),
            parallelism: 0,
            // 512 commits is small enough to stay bounded in memory (a commit's file records are a few
            // kilobytes) and large enough that a per-commit-transaction mistake shows up here first.
            // ADR-004 §7.3's `write.batch_rows = 5 000` counts *rows*; a commit is 1 + n_files of them.
            commits_per_batch: 512,
            resume: true,
            budget: None,
        }
    }
}

/// What one run did, and what it could not do.
#[derive(Debug, Clone, Default)]
pub struct RunReport {
    /// The sink's id for this run, once opened.
    pub run: Option<RunId>,
    /// Cold or warm.
    pub kind: RunKind,
    /// Commits the walk emitted.
    pub commits_seen: usize,
    /// Commits that were not already durable.
    pub commits_new: usize,
    /// Commits normalized and handed to the sink.
    pub commits_written: usize,
    /// Commits skipped because the sink already had them.
    pub commits_existing: usize,
    /// The sums over everything written this run.
    pub totals: Totals,
    /// File rows written, as distinct from commit rows.
    pub files_written: usize,
    /// Store rows written, commit rows and file rows together — what `index_run.rows_written` counts.
    pub rows_written: usize,
    /// The run stopped at [`IngestOptions::budget`] rather than at the end of the history.
    pub interrupted: bool,
    /// The walk did not reach the root of history (shallow clone, filter clone).
    pub truncated: Option<Truncation>,
    /// What the walk noticed verbatim, including a `LimitReached` if the caller set a limit.
    pub notices: Vec<Notice>,
    /// Commits that could not be normalized, with the reason rendered. A degraded run, not a failed one.
    pub failures: Vec<String>,
    /// Threads used.
    pub workers: usize,
    /// Wall time.
    pub elapsed: Duration,
}

impl RunReport {
    /// Commits per second, for the bench suite and for `fathom index --stats`.
    #[must_use]
    pub fn commits_per_second(&self) -> u64 {
        let micros = self.elapsed.as_micros();
        if micros == 0 {
            return u64::MAX;
        }
        u64::try_from(u128::try_from(self.commits_written).unwrap_or(0) * 1_000_000 / micros)
            .unwrap_or(u64::MAX)
    }
}

/// A [`Sink`] that keeps everything in memory.
///
/// Enough for `fathom export` against a repository with no store, and the readable reference
/// implementation of the contract: a test that wants to know what a run wrote reads it back out of here.
#[derive(Debug, Default)]
pub struct Collector {
    /// Every record written, in write order (which is walk order, batch by batch).
    pub records: Vec<CommitRecord>,
    /// How many times [`Sink::write`] was called, which is how a test asserts batching really happened.
    pub batches: usize,
    /// Oids to pretend are already durable, for exercising the resume path.
    pub preexisting: HashSet<Oid>,
}

impl Sink for Collector {
    type Error = std::convert::Infallible;

    fn begin(&mut self, _kind: RunKind) -> Result<RunId, Self::Error> {
        Ok(RunId(1))
    }

    fn known(&self, oids: &[Oid]) -> Result<HashSet<Oid>, Self::Error> {
        Ok(self
            .preexisting
            .iter()
            .filter(|oid| oids.contains(oid))
            .copied()
            .collect())
    }

    fn write(&mut self, _run: RunId, batch: &[CommitRecord]) -> Result<usize, Self::Error> {
        self.batches += 1;
        let rows: usize = batch.iter().map(|record| 1 + record.files.len()).sum();
        self.records.extend(batch.iter().cloned());
        Ok(rows)
    }

    fn finish(&mut self, _run: RunId, _report: &RunReport) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Ingest `repo` into `sink`.
///
/// This is the function `fathom index` calls (`e1.10`) and the one the acceptance fixtures run.
///
/// # Errors
///
/// Any [`Error`] from opening the sink, walking, or writing. Failing to normalize *one* commit is not an
/// error: it lands in [`RunReport::failures`] and the run continues, because a repository with one
/// damaged object must still produce the other 99 999 records (ADR-001 §7.6). Failing to *write* is an
/// error, and the batches already committed stay durable — which is the resume path, not a corruption.
pub fn run<S: Sink>(
    repo: &Repository,
    taxonomy: &dyn Taxonomy,
    options: &IngestOptions,
    sink: &mut S,
) -> Result<RunReport, Error> {
    let started = Instant::now();
    let workers = match options.parallelism {
        0 => rayon::current_num_threads().max(1),
        requested => requested,
    };

    // 1. Walk: cheap, serial, and its notices are what explains a truncated history.
    let mut walk = options.walk.clone();
    walk.detail = Detail::Ids;
    let outcome = repo.log(walk).finish()?;
    let seen: Vec<Oid> = outcome.entries.iter().map(|entry| entry.oid).collect();

    // 2. Ask the sink what it already has.
    let known = if options.resume {
        sink.known(&seen).map_err(Into::into)?
    } else {
        HashSet::new()
    };
    let new: Vec<Oid> = seen
        .iter()
        .copied()
        .filter(|oid| !known.contains(oid))
        .collect();

    let mut report = RunReport {
        kind: if known.is_empty() {
            RunKind::Cold
        } else {
            RunKind::Warm
        },
        commits_seen: seen.len(),
        commits_new: new.len(),
        commits_existing: known.len(),
        truncated: outcome.truncation,
        notices: outcome.notices,
        workers,
        ..RunReport::default()
    };
    let run = sink.begin(report.kind).map_err(Into::into)?;
    report.run = Some(run);

    // 3. Normalize and write, batch by batch, so nothing but the ids outlives a batch.
    let budget = options.budget.unwrap_or(usize::MAX);
    let batch_size = options.commits_per_batch.max(1);
    let mut written = 0_usize;
    let mut interrupted = false;
    for batch in new.chunks(batch_size) {
        if written >= budget {
            interrupted = true;
            break;
        }
        let take = (budget - written).min(batch.len());
        let records = normalize_batch(repo, taxonomy, options, &batch[..take], workers)?;
        let mut persistable: Vec<CommitRecord> = Vec::with_capacity(records.len());
        for record in records {
            match record {
                Ok(record) => {
                    report.totals.add(&record.totals);
                    report.files_written += record.files.len();
                    persistable.push(record);
                }
                Err(reason) => report.failures.push(reason),
            }
        }
        written += persistable.len();
        if !persistable.is_empty() {
            report.rows_written += sink.write(run, &persistable).map_err(Into::into)?;
        }
    }

    report.commits_written = written;
    report.interrupted = interrupted;
    report.elapsed = started.elapsed();
    sink.finish(run, &report).map_err(Into::into)?;
    Ok(report)
}

/// Normalize `commits` across `workers` handles, returning them in the order they came.
///
/// Shards are round-robin — worker `i` takes indices `i, i + workers, i + 2·workers, …` — and the results
/// are scattered back by index, so output order is input order whatever order the shards finish in. That
/// is the entire determinism argument for the parallel path, and `tests/ingest_run.rs` pins it by
/// comparing one worker against eight.
fn normalize_batch(
    repo: &Repository,
    taxonomy: &dyn Taxonomy,
    options: &IngestOptions,
    commits: &[Oid],
    workers: usize,
) -> Result<Vec<Result<CommitRecord, String>>, Error> {
    if commits.is_empty() {
        return Ok(Vec::new());
    }
    let workers = workers.clamp(1, commits.len());
    // One handle per worker: cloning is an `Arc` clone of the object and ref stores, not a re-read of the
    // pack index (ADR-002 §10).
    let handles: Vec<Repository> = (0..workers).map(|_| repo.clone()).collect();
    let normalize = options.normalize.clone();
    // `into_par_iter`, not `par_iter`: a repository handle is `Send` and deliberately not `Sync`, so each
    // worker takes one by value rather than sharing a reference (ADR-002 §10).
    let shards: Vec<Vec<(usize, Result<CommitRecord, String>)>> = handles
        .into_par_iter()
        .enumerate()
        .map(|(index, handle)| {
            let normalizer = Normalizer::new(&handle, taxonomy, normalize.clone());
            commits
                .iter()
                .enumerate()
                .filter(|(position, _)| position % workers == index)
                .map(|(position, oid)| {
                    (
                        position,
                        normalizer
                            .record(*oid)
                            .map_err(|err| format!("{oid}: {err}")),
                    )
                })
                .collect()
        })
        .collect();
    let mut out: Vec<Option<Result<CommitRecord, String>>> =
        (0..commits.len()).map(|_| None).collect();
    for shard in shards {
        for (position, record) in shard {
            out[position] = Some(record);
        }
    }
    Ok(out
        .into_iter()
        .map(|slot| slot.unwrap_or_else(|| Err(String::from("no record was produced"))))
        .collect())
}

//! `sink.rs` — writing ingest records into the store (ADR-002 §3's boundary).
//!
//! ADR-002 puts the meeting of ingest and storage in one place, and this is it: `fathom-ingest` produces
//! pure [`CommitRecord`]s and never names a database, `fathom-store` knows tables and never parses git, and
//! the projection from one to the other lives here, in the crate above both. The mapping is not boilerplate:
//! it is where the units of `e1.2`'s acceptance gate get decided ("the two implementations produce the same
//! store rows from the same fixture"), which is why it is a function and not an accident.
//!
//! `fathom-ingest`'s test fixture carries the same projection, because a test helper cannot be imported
//! across crates and `fathom-ingest`'s library is not allowed to link SQLite. When `e1.10` builds the real
//! composition root, that copy retires and this one is the only version of the truth.

use fathom_ingest::ingest::{RunId, RunKind, RunReport, Sink};
use fathom_ingest::oid::Oid;
use fathom_ingest::record::{CommitRecord, FileRecord};
use fathom_store::spine::{CommitRow, FileRow, RunStats, Store, Trigger};
use std::collections::HashSet;

/// A [`Sink`] that writes the spine: `git_commit`, `git_commit_file`, and the `index_run` bookkeeping that
/// makes an interrupted run a fact rather than a rumour.
///
/// Opening a run is what runs ADR-004 §9.2's recovery pass — [`Store::begin_run`] stamps any run still
/// marked `running` as `crashed`, because the process that *notices* is the only one that can write it.
/// That is the whole of crash handling on the write side, and it is why `e1.4` can promise that a `kill -9`
/// leaves a usable store: the durable unit is a committed batch, and an uncommitted one simply is not there.
pub struct StoreSink<'a> {
    /// The open store.
    pub store: &'a mut Store,
    /// The instant every `*_utc` column gets, supplied rather than read (ADR-002 §4.4), so one run stamps
    /// one instant and "written by this run" is a query rather than a range scan.
    pub now: i64,
    /// The run this sink opened, once it has opened one.
    ///
    /// Recorded here because the caller above owns the store but not the identity of the run: `ingest::run`
    /// calls [`Sink::begin`] itself. The driver needs it to ask the store what this run wrote
    /// ([`Store::oids_pending_derivation`]) instead of re-walking the history to find out.
    pub run_id: Option<i64>,
    /// Leave the `index_run` row open for the driver to close.
    ///
    /// The `index_run` row describes an *index* run, and the spine is only its first pass. Closing it when
    /// the commit rows land would stamp `ok` on a run whose analyses were then interrupted — and ADR-004
    /// §9.2's recovery turns on that stamp being the truth: a row that lies `ok` is a run whose unfinished
    /// work nobody will ever ask about again.
    pub defer_finish: bool,
}

impl<'a> StoreSink<'a> {
    /// A sink writing to `store`, stamped `now`.
    pub const fn new(store: &'a mut Store, now: i64) -> Self {
        StoreSink {
            store,
            now,
            run_id: None,
            defer_finish: false,
        }
    }

    /// A sink that opens the run but does not close it.
    ///
    /// The driver uses this and closes the row itself with [`run_stats`] once the analyses are done, so the
    /// `index_run` row describes the index run rather than its first pass.
    pub const fn deferring(store: &'a mut Store, now: i64) -> Self {
        StoreSink {
            store,
            now,
            run_id: None,
            defer_finish: true,
        }
    }
}

impl Sink for StoreSink<'_> {
    type Error = fathom_store::error::Error;

    /// Open the `index_run` row, which is also ADR-004 §9.2's recovery pass ([`Store::begin_run`] stamps
    /// any run a dead process left `running`) and records its id for the driver.
    fn begin(&mut self, kind: RunKind) -> Result<RunId, Self::Error> {
        let run = self
            .store
            .begin_run(kind.as_str(), Trigger::Manual, self.now)?;
        self.run_id = Some(run);
        Ok(RunId(run))
    }

    /// The resume query: which of these commit ids already have a row (§4.5 — for this analyzer the spine
    /// *is* the cache, so this lookup is the lookup).
    fn known(&self, oids: &[Oid]) -> Result<HashSet<Oid>, Self::Error> {
        let hex: Vec<String> = oids.iter().map(|oid| oid.hex()).collect();
        let found = self.store.known_oids(&hex)?;
        Ok(hex
            .iter()
            .filter(|oid| found.contains(*oid))
            .filter_map(|oid| oid.parse::<Oid>().ok())
            .collect())
    }

    fn write(&mut self, run: RunId, batch: &[CommitRecord]) -> Result<usize, Self::Error> {
        let rows: Vec<CommitRow> = batch.iter().map(commit_row).collect();
        self.store.write_commits(run.0, self.now, &rows)
    }

    fn finish(&mut self, run: RunId, report: &RunReport) -> Result<(), Self::Error> {
        if self.defer_finish {
            // Say nothing to the database: the row stays `running` until the driver says the *index* is
            // done, which is the difference between a resumable interruption and a false report.
            return Ok(());
        }
        self.store.finish_run(run.0, self.now, &run_stats(report))
    }
}

/// The `index_run` numbers for a run whose spine finished but whose index has not.
///
/// The spine's [`RunReport`] and the store's [`RunStats`] are the same facts about different halves of the
/// run; this is the projection, and the driver adds the second half (`interrupted`, `error`) because the
/// spine cannot know what happened after it.
#[must_use]
pub fn run_stats(report: &RunReport) -> RunStats {
    RunStats {
        commits_seen: i64::try_from(report.commits_seen).unwrap_or(i64::MAX),
        commits_added: i64::try_from(report.commits_written).unwrap_or(i64::MAX),
        rows_written: i64::try_from(report.rows_written).unwrap_or(i64::MAX),
        interrupted: report.interrupted,
        error: report.failures.first().cloned(),
    }
}

/// One record's row: everything ADR-004 §4.1's two tables keep, and nothing else.
#[must_use]
pub fn commit_row(record: &CommitRecord) -> CommitRow {
    CommitRow {
        oid: record.oid.hex(),
        tree_oid: record.tree.hex(),
        parent_oids: record.parents.iter().map(|oid| oid.hex()).collect(),
        author_name: record.author.name.clone(),
        author_email: record.author.email.clone(),
        // The `*_utc` columns are unix milliseconds — the unit the reference implementation writes — and
        // `gix` speaks microseconds, so the division happens here and nowhere else.
        author_utc: record.author.when.as_unix_micros() / 1_000,
        committer_utc: record.committer.when.as_unix_micros() / 1_000,
        message_head: record.message.head.clone(),
        message_hash: record.message.hash.clone(),
        trailers: record.trailers.to_map(),
        n_files: i64::try_from(record.totals.files).unwrap_or(i64::MAX),
        n_add: i64::try_from(record.totals.added).unwrap_or(i64::MAX),
        n_del: i64::try_from(record.totals.removed).unwrap_or(i64::MAX),
        n_binary: i64::try_from(record.totals.binary).unwrap_or(i64::MAX),
        oversized: record.oversized(),
        files: record.files.iter().map(file_row).collect(),
    }
}

/// One file's row.
///
/// The path *after* the change is the identity — `git_commit_file` is keyed by it — which is why a rename's
/// old name rides along as data rather than as part of the key. The path hash is sha256 of the *raw* bytes
/// (§4.1), not of the quoted form, so a path with a quote or a non-ASCII name hashes the same here as the
/// reference implementation hashes it.
#[must_use]
pub fn file_row(file: &FileRecord) -> FileRow {
    FileRow {
        path_hash: fathom_ingest::record::sha256_hex(file.path.as_bytes()),
        new_path: file.path.quoted(),
        old_path: file.old_path.as_ref().map(|path| path.quoted()),
        change_kind: file.change.as_str().to_owned(),
        mode_old: file.mode_old.map(i64::from),
        mode_new: file.mode_new.map(i64::from),
        is_binary: file.content.is_binary(),
        n_add: i64::from(file.added),
        n_del: i64::from(file.removed),
        // Column 11 of `e1.2`'s contract: the `@@ -a,b +c,d @@` lines, git's own spelling, in order.
        hunk_headers: file
            .hunks
            .iter()
            .map(fathom_ingest::record::Hunk::header)
            .collect(),
        blob_old: file.blob_old.map(|oid| oid.hex()),
        blob_new: file.blob_new.map(|oid| oid.hex()),
        path_class: file.taxonomy.path_class.clone(),
        taxonomy_version: i64::from(file.taxonomy.taxonomy_version),
        noise_class: file.taxonomy.noise.as_str().to_owned(),
    }
}

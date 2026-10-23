//! `spine.rs` — the git spine: the five tables `e1.2` writes, and the bookkeeping around them.
//!
//! ADR-004 Appendix A is the DDL, copied rather than reinterpreted: `meta`, `index_run`, `ref_tip`,
//! `git_commit`, `git_commit_file`. The rest of the schema (episodes, findings, the FTS index, the class-R
//! cache) belongs to later tickets, and this module creates only what those five tables need so a store
//! written by `e1.2` is a store `e1.4` can open and keep filling — ADR-004 §9.1's migration story is
//! "cover 1..N contiguously", which starts by having a real 1.
//!
//! # What is deliberately not here
//!
//! The writer lease (§7.1), the epoch machinery (§9.3), retention (§8) and the `--rebuild` staging dance
//! are `e1.4`'s. What *is* here is the part `e1.2`'s durability criterion depends on: WAL, one transaction
//! per batch, the crashed-run stamp, and an upsert that cannot lose rows.
//!
//! # Idempotency, in SQL
//!
//! A commit row is written with `ON CONFLICT(oid) DO NOTHING`, never `INSERT OR REPLACE`: ADR-004 §4.0's
//! rule is that a parent with cascade children may not be replaced underneath them, and `git_commit` is
//! exactly such a parent to `git_commit_file`'s `ON DELETE CASCADE`. A `REPLACE` would delete a commit row
//! that already exists and take its file rows with it — and because the re-inserted row is derived from the
//! same immutable commit, the damage would be invisible except as a run that wrote rows nobody asked for.
//! `DO NOTHING` is the idempotent form: same commit, existing row, nothing to do.
//!
//! # Time arrives as an argument
//!
//! Every `*_utc` column is filled from a `now` the caller passes in, never from a clock read here. A fixture
//! that reads the wall clock is a fixture that expires (ADR-002 §4.4), and the store's own times have to be
//! comparable against the run's, which is a fact the caller knows and this module must not invent.
//! Timestamps are **unix milliseconds**, the unit the reference implementation writes into the same
//! columns (`author_utc = int(at) * 1000`).

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension as _, params};
use sha2::Digest as _;

/// The schema version this build writes, recorded in `meta.schema_version` and `PRAGMA user_version`,
/// which ADR-004 §9.1 requires to agree.
///
/// It is also field 9 of every cache key ([`crate::cache::derive_key`]), which is what makes "a migration
/// never migrates derived data" (§9.5) a mechanism instead of an intention: bump this number and every
/// stored analysis becomes unreachable, so the rebuild path is the same code path a fresh clone takes —
/// the *tested* one.
///
/// Version 7 is `e4.6`'s `ledger_entry` ([`crate::ledger`]). The table is class M *mirror*, so
/// the bump costs nothing that D12 (ADR-004 §9.5) does not already license: nothing is migrated, every
/// derived row goes unreachable and is re-derived, and the ledger rows themselves come back from the
/// committed markdown on the next `fathom review status`.
///
/// Version 8 is `e3.2`'s graph lane ([`crate::graph`]): ADR-004 §4.6's four tables — which `s3` proved
/// hold the policy it chose without a migration of their own — plus `graph_node`, the node dictionary
/// that banks the interning `s3` projected. Every graph row is class R, so the same argument as
/// version 7's applies with more force: there is nothing to migrate, and a store that already has
/// these tables from a `--check` build just re-derives them.
/// Version 9 is `reco-93w`'s memory ceiling ([`crate::ceiling`]): the durable §8.6 budget, and the two
/// indexes its reachability probe needs to answer "is this blob still named by anything?" without a
/// table scan per blob. Same argument as version 8's — every byte under a ceiling is class R, so the
/// bump migrates nothing and re-derives what it evicts.
pub const SCHEMA_VERSION: i64 = 9;

/// The record shape the canonical export speaks, recorded in `meta.key_schema` so a store written by one
/// version of the record is not read as if it were another ([`crate::error::Error::NotImplemented`] is the
/// answer to a mismatch, not a guess).
pub const KEY_SCHEMA: &str = "fathom-commits/1";

/// The two SQLite capabilities this schema is written against, checked by behaviour rather than by name.
///
/// ADR-004 §4.0 asks for a loud failure rather than a schema whose meaning quietly changed. `STRICT` is a
/// language feature and JSON has been built in since 3.38 (so it is not in `pragma_compile_options` at
/// all), which means the only honest check of either is to use it and see whether the answer is what the
/// DDL assumes. A store that passes these two probes cannot be running a SQLite that reads
/// `trailers` or `mode_new` differently from the one that wrote them.
/// ADR-004 §7 pragmas, as a function of the per-connection page-cache cap.
const PAGE_CACHE_MIB: i64 = 8;

/// The `meta` key prefix under which "this commit could not be read" is recorded, keyed by commit id.
///
/// The fact has no row to live on — a commit the object database would not serve has no `git_commit` row —
/// so it is one `meta` key per commit, which is the same grain the session state uses
/// (`docs/concepts/session-state.md` §2: one key per fact, so a bad value is a small bad value). This is
/// what turns a repository with a damaged object from a silent hole in the index into a stated absence
/// (ADR-001 §7.6) that `fathom doctor`, `fathom stats` and `fathom show` can all name with the same
/// sentence.
pub const UNREADABLE_PREFIX: &str = "commit_unreadable.";

const PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
";

/// Read a `PRAGMA` that answers with a row (rusqlite's `pragma_get` is an optional feature, and a spine
/// that needs a feature-flagged accessor is a spine that fails to build in somebody's minimal profile).
fn pragma_text(conn: &Connection, name: &str) -> Result<String, rusqlite::Error> {
    conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, String>(0))
        .optional()
        .map(|value| value.unwrap_or_default())
}

/// The same for a `PRAGMA` whose answer is a number.
fn pragma_int(conn: &Connection, name: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, i64>(0))
        .optional()
        .map(|value| value.unwrap_or_default())
}

/// The migrations, in order, each tagged with the version it *produces*.
///
/// ADR-004 §9.5: forward-only, contiguous, and a released one is never edited, only added to. A unit
/// test asserts the contiguity, because a gap in this list is a store that silently skips a table.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, MIGRATION_1),
    (2, MIGRATION_2),
    (3, crate::episode::MIGRATION_3),
    (4, crate::episode::MIGRATION_4),
    (5, crate::episode::MIGRATION_5),
    (6, crate::episode::MIGRATION_6),
    (7, crate::ledger::MIGRATION_7),
    (8, crate::graph::MIGRATION_8),
    (9, crate::ceiling::MIGRATION_9),
];

/// Migration 1: the spine of ADR-004 Appendix A §4.1 and §4.2, verbatim.
const MIGRATION_1: &str = "
CREATE TABLE IF NOT EXISTS meta (
  key     TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS index_run (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('cold','warm','rebuild','relink','prune','maintenance')),
  trigger       TEXT NOT NULL,            -- commit|config|analyzer|event|manual|recovery
  epoch         INTEGER NOT NULL,
  status        TEXT NOT NULL CHECK (status IN ('running','ok','aborted','crashed')),
  started_utc   INTEGER NOT NULL,
  finished_utc  INTEGER,
  commits_seen  INTEGER NOT NULL DEFAULT 0,
  commits_added INTEGER NOT NULL DEFAULT 0,
  episodes_new  INTEGER NOT NULL DEFAULT 0,
  rows_written  INTEGER NOT NULL DEFAULT 0,
  cache_hits    INTEGER NOT NULL DEFAULT 0,
  cache_misses  INTEGER NOT NULL DEFAULT 0,
  analyzers_run INTEGER NOT NULL DEFAULT 0,
  peak_rss_kib  INTEGER,
  peak_wal_kib  INTEGER,
  interrupted   INTEGER NOT NULL DEFAULT 0,   -- progress-handler budget tripped (§7.6)
  error         TEXT
) STRICT;

CREATE TABLE IF NOT EXISTS ref_tip (
  name        TEXT PRIMARY KEY,
  oid         TEXT NOT NULL,
  kind        TEXT NOT NULL CHECK (kind IN ('branch','tag','head','remote')),
  updated_utc INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS git_commit (
  oid          TEXT PRIMARY KEY,          -- 40 hex chars, lowercase
  tree_oid     TEXT NOT NULL,
  parent_oids  TEXT NOT NULL,             -- JSON array, first-parent first
  author_name  TEXT NOT NULL,
  author_email TEXT NOT NULL,
  author_utc   INTEGER NOT NULL,
  committer_utc INTEGER NOT NULL,
  message_head TEXT NOT NULL,             -- first line, truncated at 512 B
  message_hash TEXT NOT NULL,             -- sha256 of the whole message
  trailers     TEXT NOT NULL,             -- JSON object of trailer key -> last value
  n_files      INTEGER NOT NULL,
  n_add        INTEGER NOT NULL,
  n_del        INTEGER NOT NULL,
  n_binary     INTEGER NOT NULL,
  oversized    INTEGER NOT NULL DEFAULT 0, -- a file exceeded the size cap (e1.2)
  ingested_utc INTEGER NOT NULL,
  run_id       INTEGER NOT NULL REFERENCES index_run(id)
) STRICT;
CREATE INDEX IF NOT EXISTS commit_author_time ON git_commit(author_utc);
CREATE INDEX IF NOT EXISTS commit_tree ON git_commit(tree_oid);

CREATE TABLE IF NOT EXISTS git_commit_file (
  commit_oid     TEXT NOT NULL REFERENCES git_commit(oid) ON DELETE CASCADE,
  path_hash      TEXT NOT NULL,
  new_path       TEXT NOT NULL,
  old_path       TEXT,                    -- non-null on rename/copy
  change_kind    TEXT NOT NULL CHECK (change_kind IN ('add','modify','delete','rename','copy','binary')),
  mode_old       INTEGER,
  mode_new       INTEGER,
  is_binary      INTEGER NOT NULL DEFAULT 0,
  n_add          INTEGER NOT NULL,
  n_del          INTEGER NOT NULL,
  hunk_headers   TEXT NOT NULL,           -- JSON array of \"@@ ...\" lines, capped 256 entries
  blob_old       TEXT,                    -- -> blob.sha256 if a payload was kept
  blob_new       TEXT,
  path_class     TEXT NOT NULL,           -- taxonomy decision (e0.9), never re-derived downstream
  taxonomy_version INTEGER NOT NULL,
  noise_class    TEXT NOT NULL CHECK (noise_class IN ('informative','generated','lockfile','reformat','vendored','unknown')),
  PRIMARY KEY (commit_oid, path_hash)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS git_commit_file_path ON git_commit_file(path_hash);
CREATE INDEX IF NOT EXISTS git_commit_file_noise ON git_commit_file(noise_class);
";

/// Migration 2: the analysis cache — `analyzer`, `cache`, `blob`, `blob_ref`, `counter` — which is
/// `e1.4`'s half of Appendix A and lives with the code that reads it, in [`crate::cache`].
use crate::cache::MIGRATION_2;

/// What set off a run, in `index_run.trigger`'s vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// A commit-hook event.
    Commit,
    /// A configuration change.
    Config,
    /// An analyzer asked for a re-derive.
    Analyzer,
    /// A harness event.
    Event,
    /// A human typed `fathom index`.
    Manual,
    /// The recovery pass is writing what a dead run could not.
    Recovery,
}

impl Trigger {
    /// The column's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Trigger::Commit => "commit",
            Trigger::Config => "config",
            Trigger::Analyzer => "analyzer",
            Trigger::Event => "event",
            Trigger::Manual => "manual",
            Trigger::Recovery => "recovery",
        }
    }
}

/// One `git_commit` row, in the store's own vocabulary: hex ids and plain strings, so this crate does not
/// need to know about git objects at all (ADR-002 §4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRow {
    /// 40 lowercase hex characters.
    pub oid: String,
    /// The root tree's id.
    pub tree_oid: String,
    /// Parents, first parent first.
    pub parent_oids: Vec<String>,
    /// The author's name, as committed.
    pub author_name: String,
    /// The author's address, as committed.
    pub author_email: String,
    /// The author's date, unix millis.
    pub author_utc: i64,
    /// The committer's date, unix millis.
    pub committer_utc: i64,
    /// The committer's name, as committed — an attribution input (`e4.1`): a rebase and a bot-applied
    /// change both move the committer and leave the author alone.
    pub committer_name: String,
    /// See [`CommitRow::committer_name`].
    pub committer_email: String,
    /// First line of the message, byte-capped.
    pub message_head: String,
    /// sha256 of the whole message, hex.
    pub message_hash: String,
    /// Trailers: lowercased key to last value.
    pub trailers: BTreeMap<String, String>,
    /// Files touched.
    pub n_files: i64,
    /// Lines added.
    pub n_add: i64,
    /// Lines removed.
    pub n_del: i64,
    /// Files measured as binary.
    pub n_binary: i64,
    /// A file exceeded the size cap.
    pub oversized: bool,
    /// The file rows.
    pub files: Vec<FileRow>,
}

/// One `git_commit_file` row.
///
/// `path_hash` is supplied by the caller because it is sha256 over the path's **raw bytes**, and a path is
/// allowed to hold bytes that are not valid UTF-8 (git allows it); a store that hashed a lossy rendering
/// would collide two distinct paths. The text columns carry the same quoted form the canonical export uses
/// ([`fathom_ingest::paths::RepoPath`]'s JSON spelling), so a row and an export line never disagree about
/// what a path is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    /// sha256 of the raw path bytes, hex — the primary key half that keeps unicode paths from breaking.
    pub path_hash: String,
    /// The path after the change, in the export's quoted form.
    pub new_path: String,
    /// The previous name, on a rename or copy.
    pub old_path: Option<String>,
    /// `add`, `modify`, `delete`, `rename` or `copy`.
    pub change_kind: String,
    /// The old mode, as an integer (`0o100644` and friends).
    pub mode_old: Option<i64>,
    /// The new mode.
    pub mode_new: Option<i64>,
    /// Measured binary, not "assumed because we did not look".
    pub is_binary: bool,
    /// Lines added.
    pub n_add: i64,
    /// Lines removed.
    pub n_del: i64,
    /// The `@@` headers, capped.
    pub hunk_headers: Vec<String>,
    /// The previous blob's id, if there was one.
    pub blob_old: Option<String>,
    /// The current blob's id, if there is one.
    pub blob_new: Option<String>,
    /// The taxonomy decision, stored and never re-derived downstream (ADR-004 §4.2).
    pub path_class: String,
    /// Which table made that decision.
    pub taxonomy_version: i64,
    /// How much to trust the row.
    pub noise_class: String,
}

/// The run-level numbers `index_run` carries when a run ends.
#[derive(Debug, Clone, Default)]
pub struct RunStats {
    /// Commits the walk emitted.
    pub commits_seen: i64,
    /// Commits written by this run.
    pub commits_added: i64,
    /// Store rows written by this run.
    pub rows_written: i64,
    /// The budget tripped before the history ended (ADR-004 §7.6).
    pub interrupted: bool,
    /// The failure that ended the run, if it did not end cleanly.
    pub error: Option<String>,
}

/// An open store with the schema migrated.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    /// Where the file is, when it is a file: the `objects/` directory of ADR-004 §3 lives beside it, and
    /// an in-memory store has nowhere to put one.
    path: Option<PathBuf>,
    /// Counter increments accumulated since the last [`flush_counters`](Self::flush_counters).
    ///
    /// Counters are writes, and a counter write per cache lookup on the warm path would be a write per
    /// unit of work — the opposite of §7.3's batch discipline. So they accumulate here and go to the
    /// database in one transaction when a run closes, which is also the only moment anyone reads them.
    pending: BTreeMap<String, i64>,
    /// The in-process analysis payload cache: the decoded bytes a warm read asks for again, bounded by
    /// `rss.analyzer_working_set.mib` (`reco-93w`).
    ///
    /// The spill target is the store itself. A payload is content-addressed and already durable — in the
    /// `blob` row or in `objects/` beside it — so evicting one costs a re-read and never a re-analysis.
    /// That is what makes a ceiling on this cache cheap to honour: the alternative to holding a hot
    /// payload is not losing the answer, it is paying SQLite again for an answer the store still has.
    pub(crate) payloads: fathom_core::memory::BoundedCache<String, std::sync::Arc<Vec<u8>>>,
    /// The payload-cache eviction and refusal totals already folded into `pending`, so
    /// [`flush_counters`](Self::flush_counters) writes each event once: the cache counts from the start
    /// of the process, the `counter` table counts from the start of the store, and the two only agree
    /// through this bookkeeping.
    payloads_folded: (u64, u64, u64),
}

impl Store {
    /// Open (creating if necessary) the store at `path`, apply the pragmas and migrate it.
    ///
    /// # Errors
    ///
    /// [`Error::SqliteCompileOptionMissing`] if the linked SQLite lacks `STRICT` or JSON1, and
    /// [`Error::StoreCorrupt`] if an existing store's `user_version` and `meta.schema_version` disagree —
    /// ADR-004 §9.1's first integrity check, checked *before* anything is written. Other rusqlite failures
    /// arrive as [`Error::StoreCorrupt`] with the database's own words, because at this layer a failed open
    /// is a store that cannot be used and the distinction that matters is which one and why.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let store = Self::from_connection(
            Connection::open(path).map_err(|err| corrupt("opening the store", &err))?,
        )?;
        Ok(Store {
            path: Some(path.to_path_buf()),
            ..store
        })
    }

    /// An in-memory store, for tests and for `--dry-run`.
    ///
    /// # Errors
    ///
    /// As [`open`](Self::open).
    pub fn open_in_memory() -> Result<Self, Error> {
        let store = Self::from_connection(
            Connection::open_in_memory().map_err(|err| corrupt("opening the store", &err))?,
        )?;
        Ok(Store {
            path: None,
            ..store
        })
    }

    /// Adopt an already-open connection: pragmas, then migration.
    ///
    /// # Errors
    ///
    /// As [`open`](Self::open).
    pub fn from_connection(conn: Connection) -> Result<Self, Error> {
        // `journal_mode` answers with the mode it settled on, so it cannot be set with `execute_batch`,
        // which discards rows. If WAL is refused (a filesystem that cannot lock), that is a store we can
        // still use — ADR-004 §7 accepts the fallback — but the run should know, so it is recorded in meta.
        let journal = pragma_text(&conn, "journal_mode")
            .map_err(|err| corrupt("reading journal_mode", &err))?;
        conn.execute_batch(PRAGMAS)
            .map_err(|err| corrupt("applying the store pragmas", &err))?;
        // `cache_size` in KiB (negative means KiB), which is `rss.sqlite_page_cache.mib` from
        // benchmarks/budgets.json — a per-connection cap, not a per-table one, and not scaled by size.
        conn.execute_batch(&format!("PRAGMA cache_size = -{};", PAGE_CACHE_MIB * 1024))
            .map_err(|err| corrupt("setting the page cache", &err))?;
        let mut store = Store {
            conn,
            path: None,
            pending: BTreeMap::new(),
            payloads: fathom_core::memory::BoundedCache::new(
                fathom_core::memory::Ceilings::default().analysis_cache_bytes,
            ),
            payloads_folded: (0, 0, 0),
        };
        store.check_compile_options()?;
        store.migrate()?;
        store.set_meta("journal_mode", &journal)?;
        // The manifest is code, so it is refreshed on every open rather than written once at creation: a
        // store that kept a stale copy would answer a lookup with yesterday's analyzer version, which is
        // the failure §5.7's CI gate exists to prevent, reintroduced by storage.
        store.register_analyzers()?;
        Ok(store)
    }

    /// The underlying connection, for the read-only surfaces of later tickets.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Where the file is, for `objects/`, `doctor` and the WAL-size check. `None` for an in-memory store.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The connection, mutably, for the cache module's writes. Crate-internal: a caller outside this
    /// crate goes through the typed methods, which is what keeps "the store is the only writer" checkable.
    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Begin a write transaction (§7.3's `BEGIN IMMEDIATE`, which takes the write lock up front rather
    /// than discovering mid-batch that someone else holds it).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the transaction cannot be begun.
    pub(crate) fn immediate_transaction(&mut self) -> Result<rusqlite::Transaction<'_>, Error> {
        self.conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| corrupt("beginning a write transaction", &err))
    }

    /// The journal mode the connection settled on, which `open` records and `doctor` reads.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the pragma cannot be read.
    pub fn journal_mode(&self) -> Result<String, Error> {
        pragma_text(&self.conn, "journal_mode").map_err(|err| corrupt("reading journal_mode", &err))
    }

    /// ADR-004 §4.0's startup check, limited to what this schema is actually written against: JSON,
    /// because `parent_oids`, `trailers` and `hunk_headers` are JSON columns that queries will read with
    /// `json_each`, and `STRICT`, because every table here declares it.
    ///
    /// Both are probed, not listed. `pragma_compile_options` does not mention JSON on a modern SQLite that
    /// has it built in, and `STRICT` is not a compile option at all; a check that looked for their names
    /// would fail on exactly the good builds and pass on a bad one.
    fn check_compile_options(&self) -> Result<(), Error> {
        let json: String = self
            .conn
            .query_row("SELECT json_array(1, 2)", [], |row| row.get(0))
            .map_err(|_| Error::SqliteCompileOptionMissing { option: "JSON1" })?;
        if json != "[1,2]" {
            return Err(Error::SqliteCompileOptionMissing { option: "JSON1" });
        }
        let strict = self.conn.execute_batch(
            "CREATE TEMP TABLE fathom_strict_probe (n INTEGER) STRICT;
             INSERT INTO fathom_strict_probe(n) VALUES ('text');",
        );
        let _ = self
            .conn
            .execute_batch("DROP TABLE IF EXISTS fathom_strict_probe;");
        match strict {
            // Refusing the TEXT is the pass condition: a STRICT table that takes it is a table whose
            // declared types are decoration, and every CHECK below that would then mean something else.
            Err(err) if err.to_string().to_ascii_lowercase().contains("text") => Ok(()),
            Err(err) => Err(corrupt("probing STRICT enforcement", &err)),
            Ok(_) => Err(Error::SqliteCompileOptionMissing { option: "STRICT" }),
        }
    }

    fn migrate(&mut self) -> Result<(), Error> {
        let mut version = pragma_int(&self.conn, "user_version")
            .map_err(|err| corrupt("reading user_version", &err))?;
        if version > SCHEMA_VERSION {
            return Err(Error::NotImplemented {
                what: "reading a store written by a newer fathom",
                ticket: "e1.4",
            });
        }
        // §9.5: forward-only and contiguous. Each migration is `CREATE TABLE IF NOT EXISTS`, so re-running
        // one against a store that already has the table is a no-op rather than a failure, which is what
        // lets a store whose `user_version` was never stamped (the `e1.2` ones) be picked up here.
        for (target, ddl) in MIGRATIONS {
            if version < *target {
                self.conn
                    .execute_batch(ddl)
                    .map_err(|err| corrupt(format!("applying migration {target}"), &err))?;
                self.conn
                    .execute_batch(&format!("PRAGMA user_version = {target};"))
                    .map_err(|err| corrupt("stamping user_version", &err))?;
                // The two stamps have to move together (§9.1), so a store that ran a migration does not
                // go on claiming the old one — and because `schema_version` is field 9 of every cache key,
                // moving it is what makes the migration invalidate every derived row instead of migrating
                // any of them (§9.5).
                self.set_meta("schema_version", &target.to_string())?;
                version = *target;
            }
        }
        // The column additions §9.5 keeps out of the migration strings, applied by a probe: reading the
        // table's own columns first is what makes re-running the step a no-op, which is the same property
        // `CREATE TABLE IF NOT EXISTS` gives the migration strings, and for the same reason — a store that
        // already has a column must not fail because it was asked for again (`e4.1`).
        for (table, column, definition) in crate::episode::ADDED_COLUMNS {
            let mut probe = self
                .conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .map_err(|err| corrupt("probing a table's columns", &err))?;
            let names = probe
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|err| corrupt("reading a table's columns", &err))?
                .collect::<Result<Vec<String>, _>>()
                .map_err(|err| corrupt("reading a table's columns", &err))?;
            drop(probe);
            if !names.iter().any(|name| name == column) {
                self.conn
                    .execute_batch(&format!(
                        "ALTER TABLE {table} ADD COLUMN {column} {definition};"
                    ))
                    .map_err(|err| corrupt(format!("adding {table}.{column}"), &err))?;
            }
        }

        // ADR-004 §4.8: the FTS index is created only when the running SQLite has FTS5, because the schema
        // may not depend on an optional compile option — and the *absence* is recorded rather than left
        // implicit, so `Store::search` can report `degraded: true` instead of silently running the LIKE
        // fallback. Idempotent for the same reason every other step here is: `CREATE ... IF NOT EXISTS`.
        let fts5 = crate::search::ensure_search_index(&self.conn)?;
        self.set_meta("search.fts5", if fts5 { "present" } else { "absent" })?;

        // §9.1: the two version stamps must agree, and the second one is only meaningful once a store
        // exists at all, so it is written here rather than by a separate init step.
        let declared = self.meta("schema_version")?;
        if declared.as_deref() != Some(SCHEMA_VERSION.to_string().as_str()) {
            return Err(Error::StoreCorrupt {
                what: format!(
                    "meta.schema_version says {} and this build writes {SCHEMA_VERSION}",
                    declared.as_deref().unwrap_or("nothing")
                ),
            });
        }
        // The companion scalars of §4.1: what this store *is*, as distinct from what it holds. Each is
        // written only if absent, because a store that has been running knows better than an open call
        // what its epoch is.
        if self.meta("key_schema")?.is_none() {
            self.set_meta("key_schema", KEY_SCHEMA)?;
        }
        // ADR-004 §4.1 names `key_schema` for the cache framing version; `e1.2` had already taken that
        // meta key for the *export record* shape, and renaming either would break a committed artifact,
        // so the framing version is recorded beside it under its own name rather than fought over.
        if self.meta("cache_key_schema")?.is_none() {
            self.set_meta("cache_key_schema", crate::cache::KEY_SCHEMA)?;
        }
        for (key, value) in [
            ("index_epoch", "1"),
            ("build_epoch", "1"),
            ("epoch_status", "ready"),
            ("writer_epoch", "0"),
        ] {
            if self.meta(key)?.is_none() {
                self.set_meta(key, value)?;
            }
        }
        Ok(())
    }

    /// Read one `meta` value.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn meta(&self, key: &str) -> Result<Option<String>, Error> {
        self.conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| corrupt("reading store metadata", &err))
    }

    /// Write one `meta` value, replacing whatever was there.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn set_meta(&mut self, key: &str, value: &str) -> Result<(), Error> {
        self.conn
            .execute(
                "INSERT INTO meta(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
            .map_err(|err| corrupt("writing store metadata", &err))
    }

    /// Record that a commit could not be read out of the object database, naming the commit.
    ///
    /// The row lives in `meta` under [`UNREADABLE_PREFIX`] rather than in `git_commit`, because the whole
    /// point is that this commit has no commit row: the fact is "fathom tried to index *this commit* and the
    /// object database would not serve it", and a commit id is the only key it has. ADR-001 §7.6's stated
    /// absence, made durable — `fathom doctor` and `fathom show` both read it back, and neither of them
    /// re-walks the history to guess at it.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn record_unreadable_commit(
        &mut self,
        oid: &str,
        reason: &str,
        run: i64,
        now: i64,
    ) -> Result<(), Error> {
        let value = serde_json::json!({"reason": reason, "run": run, "utc": now});
        self.set_meta(&format!("{UNREADABLE_PREFIX}{oid}"), &value.to_string())
    }

    /// Forget the unreadable-commit record for `oid`, because this run *did* read it.
    ///
    /// This is what keeps a repaired object from staying condemned in the store: `git fsck`, or a
    /// `git fetch` the user ran themselves, brings the object back, and the next index writes the row and
    /// clears the record.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the delete fails.
    pub fn clear_unreadable_commit(&mut self, oid: &str) -> Result<(), Error> {
        self.conn
            .execute(
                "DELETE FROM meta WHERE key = ?1",
                params![format!("{UNREADABLE_PREFIX}{oid}")],
            )
            .map(|_| ())
            .map_err(|err| corrupt("clearing an unreadable-commit record", &err))
    }

    /// Every commit this store could not read, oldest record first: `(commit oid, reason, utc)`.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn unreadable_commits(&self) -> Result<Vec<(String, String, i64)>, Error> {
        let mut statement = self
            .conn
            .prepare("SELECT key, value FROM meta WHERE key LIKE ?1 ORDER BY key")
            .map_err(|err| corrupt("reading unreadable commits", &err))?;
        let rows = statement
            .query_map(params![format!("{UNREADABLE_PREFIX}%")], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|err| corrupt("reading unreadable commits", &err))?;
        let mut out = Vec::new();
        for row in rows {
            let (key, value) = row.map_err(|err| corrupt("reading unreadable commits", &err))?;
            let oid = key
                .strip_prefix(UNREADABLE_PREFIX)
                .unwrap_or_default()
                .to_owned();
            let parsed: serde_json::Value = serde_json::from_str(&value).unwrap_or_default();
            out.push((
                oid,
                parsed["reason"]
                    .as_str()
                    .unwrap_or("the record carried no reason")
                    .to_owned(),
                parsed["utc"].as_i64().unwrap_or(0),
            ));
        }
        Ok(out)
    }

    /// Why this commit could not be read, when this store has been told before.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn unreadable_commit(&self, oid: &str) -> Result<Option<String>, Error> {
        let key = format!("{UNREADABLE_PREFIX}{oid}");
        self.meta(&key).map(|value| {
            value.map(|value| {
                serde_json::from_str::<serde_json::Value>(&value)
                    .ok()
                    .and_then(|parsed| parsed["reason"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| "the record carried no reason".to_owned())
            })
        })
    }

    /// The `store_id`, minted on first use.
    ///
    /// ADR-004 §12.3: a copied or swapped store file is detected by identity, and identity has to exist
    /// before it can be compared. The value is random, so the caller supplies the randomness — this crate
    /// does not read a clock or a RNG (ADR-002 §4.4).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read or the mint fails.
    pub fn store_id(&mut self, fresh: impl FnOnce() -> String) -> Result<String, Error> {
        if let Some(existing) = self.meta("store_id")? {
            return Ok(existing);
        }
        let minted = fresh();
        self.set_meta("store_id", &minted)?;
        Ok(minted)
    }

    /// Open a run, first stamping any run left `running` by a dead process as `crashed` (§9.2).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the bookkeeping fails.
    pub fn begin_run(&mut self, kind: &str, trigger: Trigger, now: i64) -> Result<i64, Error> {
        self.recover_runs(now)?;
        let epoch: i64 = self
            .meta("index_epoch")?
            .and_then(|text| text.parse().ok())
            .unwrap_or(1);
        self.conn
            .execute(
                "INSERT INTO index_run(kind, trigger, epoch, status, started_utc)
                 VALUES (?1, ?2, ?3, 'running', ?4)",
                params![kind, trigger.as_str(), epoch, now],
            )
            .map_err(|err| corrupt("opening an index run", &err))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Stamp every run still marked `running` as `crashed`, and say how many.
    ///
    /// The point of the row is that the process which notices is the one that writes it: a dead process is
    /// the only witness to its own death and cannot testify afterwards (§9.2).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the update fails.
    pub fn recover_runs(&mut self, now: i64) -> Result<usize, Error> {
        let dead: Vec<i64> = self
            .conn
            .prepare("SELECT id FROM index_run WHERE status = 'running'")
            .map_err(|err| corrupt("looking for abandoned runs", &err))?
            .query_map([], |row| row.get(0))
            .map_err(|err| corrupt("looking for abandoned runs", &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| corrupt("looking for abandoned runs", &err))?;
        for id in &dead {
            self.conn
                .execute(
                    "UPDATE index_run SET status = 'crashed', finished_utc = ?2,
                            error = 'stamped by recovery: the writing process was gone'
                     WHERE id = ?1",
                    params![id, now],
                )
                .map_err(|err| corrupt("stamping an abandoned run", &err))?;
        }
        Ok(dead.len())
    }

    /// Close a run with its numbers.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the update fails.
    pub fn finish_run(&mut self, run: i64, now: i64, stats: &RunStats) -> Result<(), Error> {
        let status = if stats.error.is_some() {
            "aborted"
        } else {
            "ok"
        };
        self.conn
            .execute(
                "UPDATE index_run
                 SET status = ?2, finished_utc = ?3, commits_seen = ?4, commits_added = ?5,
                     rows_written = ?6, interrupted = ?7, error = ?8
                 WHERE id = ?1",
                params![
                    run,
                    status,
                    now,
                    stats.commits_seen,
                    stats.commits_added,
                    stats.rows_written,
                    i64::from(stats.interrupted),
                    stats.error
                ],
            )
            .map(|_| ())
            .map_err(|err| corrupt("closing an index run", &err))
    }

    /// The tip rows for a ref scan, replacing the whole table: the tips are a snapshot of what the refs
    /// said at this instant, not a history (the history is `index_run`).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the rewrite fails.
    pub fn replace_ref_tips(
        &mut self,
        tips: &[(String, String, &str)],
        now: i64,
    ) -> Result<(), Error> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| corrupt("beginning a ref snapshot", &err))?;
        tx.execute("DELETE FROM ref_tip", [])
            .map_err(|err| corrupt("clearing the ref snapshot", &err))?;
        {
            let mut statement = tx
                .prepare(
                    "INSERT INTO ref_tip(name, oid, kind, updated_utc) VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(|err| corrupt("writing the ref snapshot", &err))?;
            for (name, oid, kind) in tips {
                statement
                    .execute(params![name, oid, kind, now])
                    .map_err(|err| corrupt("writing the ref snapshot", &err))?;
            }
        }
        tx.commit()
            .map_err(|err| corrupt("writing the ref snapshot", &err))
    }

    /// Drop the ref snapshot, so the next run cannot answer "nothing moved" from it (`reco-7tc`).
    ///
    /// Some changes are not ref movements. A correction to an attribution leaves every commit exactly where
    /// it is and changes only what the store should *say* about it; a rewritten correction does the same.
    /// The warm no-op check reads the snapshot to decide that nothing moved, so it would be holding a true
    /// fact and drawing a wrong conclusion. The write that stores or removes such a fact calls this, and
    /// calls it here rather than in the verb, for the reason every other write in this crate happens in one
    /// place: a bump with one call site cannot be forgotten at the others.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the delete fails.
    pub fn forget_ref_snapshot(&mut self) -> Result<(), Error> {
        self.conn
            .execute("DELETE FROM ref_tip", [])
            .map(|_| ())
            .map_err(|err| corrupt("clearing the ref snapshot", &err))
    }

    /// Which of `oids` already have a commit row — the resume and idempotency query, one lookup per run.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn known_oids(&self, oids: &[String]) -> Result<HashSet<String>, Error> {
        if oids.is_empty() {
            return Ok(HashSet::new());
        }
        // Chunked so the statement stays inside SQLITE_MAX_VARIABLE_NUMBER (999 by default) whatever the
        // history size; 500 at a time is well inside it and one query per 500 commits is not the bottleneck.
        let mut found = HashSet::new();
        let mut statement = self
            .conn
            .prepare("SELECT oid FROM git_commit WHERE oid IN (SELECT value FROM json_each(?1))")
            .map_err(|err| corrupt("looking up known commits", &err))?;
        for chunk in oids.chunks(500) {
            let json = serde_json::to_string(chunk).map_err(|err| {
                corrupt(
                    "encoding a commit list",
                    &rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    ),
                )
            })?;
            let rows = statement
                .query_map(params![json], |row| row.get::<_, String>(0))
                .map_err(|err| corrupt("looking up known commits", &err))?;
            for row in rows {
                found.insert(row.map_err(|err| corrupt("looking up known commits", &err))?);
            }
        }
        Ok(found)
    }

    /// Write commit rows in one transaction: `git_commit` first, then its file rows, so the foreign key
    /// holds at every commit point rather than only at the end.
    ///
    /// Returns the number of rows written.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if any statement fails; the transaction is rolled back whole, so a failed
    /// batch leaves nothing behind, which is what makes "resumes from the last durable commit" true.
    pub fn write_commits(
        &mut self,
        run: i64,
        now: i64,
        rows: &[CommitRow],
    ) -> Result<usize, Error> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| corrupt("beginning a commit batch", &err))?;
        let mut written = 0_usize;
        {
            let mut commit = tx
                .prepare(
                    "INSERT INTO git_commit(oid, tree_oid, parent_oids, author_name, author_email,
                        author_utc, committer_utc, message_head, message_hash, trailers,
                        n_files, n_add, n_del, n_binary, oversized, ingested_utc, run_id,
                        committer_name, committer_email)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
                     ON CONFLICT(oid) DO NOTHING",
                )
                .map_err(|err| corrupt("writing commit rows", &err))?;
            let mut file = tx
                .prepare(
                    "INSERT INTO git_commit_file(commit_oid, path_hash, new_path, old_path, change_kind,
                            mode_old, mode_new, is_binary, n_add, n_del, hunk_headers, blob_old, blob_new,
                            path_class, taxonomy_version, noise_class)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                     ON CONFLICT(commit_oid, path_hash) DO NOTHING",
                )
                .map_err(|err| corrupt("writing file rows", &err))?;
            for row in rows {
                let parents = json_array(&row.parent_oids);
                let trailers = serde_json::to_string(&row.trailers)
                    .map_err(|err| corrupt("encoding trailers", &json_error(err)))?;
                commit
                    .execute(params![
                        row.oid,
                        row.tree_oid,
                        parents,
                        row.author_name,
                        row.author_email,
                        row.author_utc,
                        row.committer_utc,
                        row.message_head,
                        row.message_hash,
                        trailers,
                        row.n_files,
                        row.n_add,
                        row.n_del,
                        row.n_binary,
                        i64::from(row.oversized),
                        now,
                        run,
                        row.committer_name,
                        row.committer_email
                    ])
                    .map_err(|err| corrupt("writing commit rows", &err))?;
                written += 1;
                for file_row in &row.files {
                    let hunks = json_array(&file_row.hunk_headers);
                    file.execute(params![
                        row.oid,
                        file_row.path_hash,
                        file_row.new_path,
                        file_row.old_path,
                        file_row.change_kind,
                        file_row.mode_old,
                        file_row.mode_new,
                        i64::from(file_row.is_binary),
                        file_row.n_add,
                        file_row.n_del,
                        hunks,
                        file_row.blob_old,
                        file_row.blob_new,
                        file_row.path_class,
                        file_row.taxonomy_version,
                        file_row.noise_class
                    ])
                    .map_err(|err| corrupt("writing file rows", &err))?;
                    written += 1;
                }
            }
        }
        // A commit this run *did* read is no longer a commit that could not be read, and the withdrawal
        // belongs in this transaction: a store must never carry both a commit row and a "could not be read"
        // for the same oid, because the second would outlive the first as a permanent false alarm after the
        // user ran `git fsck` or fetched the object back.
        if !rows.is_empty() {
            let oids: Vec<&str> = rows.iter().map(|row| row.oid.as_str()).collect();
            let json = serde_json::to_string(&oids)
                .map_err(|err| corrupt("encoding a commit list", &json_error(err)))?;
            tx.execute(
                "DELETE FROM meta WHERE key IN (SELECT ?1 || value FROM json_each(?2))",
                params![UNREADABLE_PREFIX, json],
            )
            .map_err(|err| corrupt("withdrawing unreadable-commit records", &err))?;
        }
        tx.commit()
            .map_err(|err| corrupt("committing a batch", &err))?;
        Ok(written)
    }

    /// The commit ids one run wrote, in the order [`all_oids`](Self::all_oids) uses.
    ///
    /// This is what makes "what is new since the last run" a query instead of a git walk: the run that
    /// wrote the rows stamped them, so the answer outlives the process that wrote them — including the run
    /// that was killed part-way through, whose partial work is exactly the list a resume needs.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn oids_written_by(&self, run: i64) -> Result<Vec<String>, Error> {
        let mut statement = self
            .conn
            .prepare("SELECT oid FROM git_commit WHERE run_id = ?1 ORDER BY oid")
            .map_err(|err| corrupt("reading a run's commits", &err))?;
        statement
            .query_map(params![run], |row| row.get(0))
            .map_err(|err| corrupt("reading a run's commits", &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| corrupt("reading a run's commits", &err))
    }

    /// The commit ids whose derived rows a run still owes.
    ///
    /// That is: what this run wrote, plus what any run that never closed cleanly wrote **or that closed
    /// after stopping early for its unit budget** — until some later run has finished without either
    /// caveat, which is what bounds the second half to exactly one run's worth of re-lookups. Without
    /// that bound a store would carry a crashed run's commits on its pending list forever, paying a small
    /// warm cost every run of the week for a crash that happened once.
    ///
    /// The second half is what closes the hole a naive "only the new commits" warm path leaves: a run killed
    /// halfway through its analyses may have written every commit row it meant to, so its commits would
    /// never be looked at again and the index would be quietly, permanently incomplete. Because
    /// [`crate::cache`] answers a *key* rather than a question, re-asking the already-finished ones is a
    /// lookup that hits, so being thorough costs lookups and not computations.
    ///
    /// A budget is the same hole with a politeness problem. `fathom watch`, `fathom ambient` and the open
    /// frame's own `<C-r>` all index in bounded slices, and a slice that runs out of budget stops with the
    /// commits written and the analyses unfinished — so it closes `ok`, and `status <> 'ok'` alone lets the
    /// next slice walk past everything it did not reach. That is how a quarter-second slice of a 500-commit
    /// backlog becomes an index that stopped at the quarter-second and says it finished (`reco-j7t`);
    /// `index_run.interrupted` is the column that says which run did that, and it is the reason the column
    /// exists (§7.6's stated partial).
    ///
    /// **The order is the answer, not a detail.** The rows come back in the order the walk wrote them,
    /// read backwards — a parent before its child — because every derivation that reads its parent's
    /// answer (which `structure.delta` does, §6.1 R2) can only hit if the parent has been asked first. The
    /// walk emits newest-first, so "backwards" is descending insertion order. `ORDER BY gc.oid`, which
    /// this query used until `e7.2` measured it against real repositories, is a *hash* order: it was
    /// stable and it was arbitrary, so a cold index of an 82 k-commit repository asked about every tree
    /// with its parent's answer missing, every time, and the cost of the whole history landed on every
    /// commit instead of on the change. Ordering is deterministic either way, which is the only property
    /// the old clause was buying.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn oids_pending_derivation(&self, run: i64) -> Result<Vec<String>, Error> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT gc.oid FROM git_commit gc
                     LEFT JOIN index_run ir ON ir.id = gc.run_id
                 WHERE gc.run_id = ?1
                    OR gc.run_id IS NULL
                    OR (ir.id IS NOT NULL AND (ir.status <> 'ok' OR ir.interrupted <> 0)
                            AND NOT EXISTS (SELECT 1 FROM index_run later
                                            WHERE later.status = 'ok' AND later.interrupted = 0
                                              AND later.id > ir.id))
                 ORDER BY gc.rowid DESC",
            )
            .map_err(|err| corrupt("finding commits that still need analysis", &err))?;
        let rows = statement
            .query_map(params![run], |row| row.get::<_, String>(0))
            .map_err(|err| corrupt("finding commits that still need analysis", &err))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| corrupt("finding commits that still need analysis", &err))?);
        }
        Ok(out)
    }

    /// The commits whose file rows include any of `path_hashes`, oldest first by oid.
    ///
    /// The named-path refresh (`fathom index --path src/a.rs`) asks this rather than re-diffing every
    /// commit, which is the difference between a refresh that costs the history and one that costs the
    /// answer.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn oids_touching(&self, path_hashes: &[String]) -> Result<Vec<String>, Error> {
        if path_hashes.is_empty() {
            return Ok(Vec::new());
        }
        let json = serde_json::to_string(path_hashes).map_err(|err| {
            corrupt(
                "encoding a path list",
                &rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                ),
            )
        })?;
        let mut statement = self
            .conn
            .prepare(
                "SELECT commit_oid FROM git_commit_file
                 WHERE path_hash IN (SELECT value FROM json_each(?1)) ORDER BY commit_oid",
            )
            .map_err(|err| corrupt("finding commits by path", &err))?;
        let rows = statement
            .query_map(params![json], |row| row.get::<_, String>(0))
            .map_err(|err| corrupt("finding commits by path", &err))?;
        let mut seen = Vec::new();
        for row in rows {
            let oid = row.map_err(|err| corrupt("finding commits by path", &err))?;
            if !seen.contains(&oid) {
                seen.push(oid);
            }
        }
        Ok(seen)
    }

    /// Every stored commit id, in the order [`crate::export`] sorts them — hex ascending — so a caller can
    /// compare a store against an export without either side having to sort twice.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn all_oids(&self) -> Result<Vec<String>, Error> {
        let mut statement = self
            .conn
            .prepare("SELECT oid FROM git_commit ORDER BY oid")
            .map_err(|err| corrupt("reading commit ids", &err))?;
        statement
            .query_map([], |row| row.get(0))
            .map_err(|err| corrupt("reading commit ids", &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| corrupt("reading commit ids", &err))
    }

    /// The commit rows, with their file rows attached, in the same order as [`all_oids`](Self::all_oids).
    ///
    /// This is what a projection test reads back to check the store against the export; it is also what
    /// `fathom export` will use when the store is warm.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if either read fails.
    pub fn read_all(&self) -> Result<Vec<CommitRow>, Error> {
        let mut out = Vec::new();
        let mut commits = self
            .conn
            .prepare(
                "SELECT oid, tree_oid, parent_oids, author_name, author_email, author_utc,
                        committer_utc, message_head, message_hash, trailers, n_files, n_add, n_del,
                        n_binary, oversized
                 FROM git_commit ORDER BY oid",
            )
            .map_err(|err| corrupt("reading commit rows", &err))?;
        let oids: Vec<String> = commits
            .query_map([], |row| row.get(0))
            .map_err(|err| corrupt("reading commit rows", &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| corrupt("reading commit rows", &err))?;
        drop(commits);
        for oid in oids {
            out.push(self.read_commit(&oid)?);
        }
        Ok(out)
    }

    /// One commit row by id, with its file rows.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if it is not there or cannot be read.
    pub fn read_commit(&self, oid: &str) -> Result<CommitRow, Error> {
        let row = self
            .conn
            .query_row(
                "SELECT oid, tree_oid, parent_oids, author_name, author_email, author_utc,
                        committer_utc, message_head, message_hash, trailers, n_files, n_add, n_del,
                        n_binary, oversized, run_id, committer_name, committer_email
                 FROM git_commit WHERE oid = ?1",
                params![oid],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, i64>(10)?,
                        row.get::<_, i64>(11)?,
                        row.get::<_, i64>(12)?,
                        row.get::<_, i64>(13)?,
                        row.get::<_, i64>(14)?,
                        row.get::<_, i64>(15)?,
                        row.get::<_, String>(16)?,
                        row.get::<_, String>(17)?,
                    ))
                },
            )
            .map_err(|err| corrupt("reading a commit row", &err))?;
        let (
            oid,
            tree_oid,
            parents,
            author_name,
            author_email,
            author_utc,
            committer_utc,
            message_head,
            message_hash,
            trailers,
            n_files,
            n_add,
            n_del,
            n_binary,
            oversized,
            _run_id,
            committer_name,
            committer_email,
        ) = row;
        let files = self.read_files(&oid)?;
        Ok(CommitRow {
            parent_oids: json_strings(&parents).map_err(|err| corrupt("decoding parents", &err))?,
            trailers: serde_json::from_str(&trailers)
                .map_err(|err| corrupt("decoding trailers", &json_error(err)))?,
            oid,
            tree_oid,
            author_name,
            author_email,
            author_utc,
            committer_utc,
            message_head,
            message_hash,
            n_files,
            n_add,
            n_del,
            n_binary,
            oversized: oversized != 0,
            committer_name,
            committer_email,
            files,
        })
    }

    /// One commit's file rows, ordered by `new_path` so the order is reproducible.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if they cannot be read.
    pub fn read_files(&self, oid: &str) -> Result<Vec<FileRow>, Error> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT path_hash, new_path, old_path, change_kind, mode_old, mode_new, is_binary,
                        n_add, n_del, hunk_headers, blob_old, blob_new, path_class, taxonomy_version,
                        noise_class
                 FROM git_commit_file WHERE commit_oid = ?1 ORDER BY new_path, path_hash",
            )
            .map_err(|err| corrupt("reading file rows", &err))?;
        let rows = statement
            .query_map(params![oid], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, String>(14)?,
                ))
            })
            .map_err(|err| corrupt("reading file rows", &err))?;
        let mut out = Vec::new();
        for row in rows {
            let (
                path_hash,
                new_path,
                old_path,
                change_kind,
                mode_old,
                mode_new,
                is_binary,
                n_add,
                n_del,
                hunks,
                blob_old,
                blob_new,
                path_class,
                taxonomy_version,
                noise_class,
            ) = row.map_err(|err| corrupt("reading file rows", &err))?;
            out.push(FileRow {
                path_hash,
                new_path,
                old_path,
                change_kind,
                mode_old,
                mode_new,
                is_binary: is_binary != 0,
                n_add,
                n_del,
                hunk_headers: json_strings(&hunks)
                    .map_err(|err| corrupt("decoding hunks", &err))?,
                blob_old,
                blob_new,
                path_class,
                taxonomy_version,
                noise_class,
            });
        }
        Ok(out)
    }

    /// Counts for a test or a stat line: `(commits, files)`.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if either count fails.
    pub fn counts(&self) -> Result<(i64, i64), Error> {
        let commits: i64 = self
            .conn
            .query_row("SELECT count(*) FROM git_commit", [], |row| row.get(0))
            .map_err(|err| corrupt("counting commits", &err))?;
        let files: i64 = self
            .conn
            .query_row("SELECT count(*) FROM git_commit_file", [], |row| row.get(0))
            .map_err(|err| corrupt("counting file rows", &err))?;
        Ok((commits, files))
    }

    /// The spine's own integrity checks, the subset ADR-004 §9.1 that this much schema can answer:
    /// foreign keys, and the two version stamps agreeing.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] naming whichever failed.
    pub fn check(&self) -> Result<(), Error> {
        let broken: i64 = self
            .conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .map_err(|err| corrupt("checking foreign keys", &err))?;
        if broken != 0 {
            return Err(Error::StoreCorrupt {
                what: format!("{broken} foreign key violations"),
            });
        }
        let version = pragma_int(&self.conn, "user_version")
            .map_err(|err| corrupt("reading user_version", &err))?;
        if version != SCHEMA_VERSION
            || self.meta("schema_version")?.as_deref() != Some(SCHEMA_VERSION.to_string().as_str())
        {
            return Err(Error::StoreCorrupt {
                what: format!("schema version stamps disagree with this build ({version})"),
            });
        }
        Ok(())
    }

    /// The epoch readers see (§6.3). A derived row is live iff its key matches *and* `row.epoch == epoch`.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if `meta` cannot be read.
    pub fn epoch(&self) -> Result<i64, Error> {
        Ok(self
            .meta("index_epoch")?
            .and_then(|text| text.parse().ok())
            .unwrap_or(1))
    }

    /// Move to the next epoch (§6.3, §9.3).
    ///
    /// Every row built in the old epoch becomes unreachable at once — not deleted, so the pruner can still
    /// say what it cost (§6.2) — and the next run rebuilds what it needs. `fathom index --rebuild` is this
    /// function plus a walk.
    ///
    /// The ADR's §9.3 staging file and atomic rename are deliberately not here: they exist so a *concurrent
    /// reader* never sees a half-rebuilt epoch, and there is no concurrent reader until the daemon and the
    /// TUI (`e6.3`, `e1.5`) open this store while a run is writing it. Saying so out loud is cheaper than
    /// letting a half-mechanism imply a guarantee it does not have: with the epoch bumped in place, a run
    /// that is interrupted leaves the previous epoch's rows still on disk and marked dead, which is
    /// recoverable by definition.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if `meta` cannot be written.
    pub fn advance_epoch(&mut self) -> Result<i64, Error> {
        let next = self.epoch()? + 1;
        self.set_meta("index_epoch", &next.to_string())?;
        self.set_meta("build_epoch", &next.to_string())?;
        self.set_meta("epoch_status", "building")?;
        Ok(next)
    }

    /// Mark the current epoch as fully built (the other half of §9.3's state machine).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if `meta` cannot be written.
    pub fn settle_epoch(&mut self) -> Result<(), Error> {
        self.set_meta("epoch_status", "ready")
    }

    /// Put the run's memory ceilings on this store (`reco-93w`).
    ///
    /// One call, from the composition root, before any analysis reads: the ceilings arrive as values
    /// because ADR-002 §4.5 keeps `fathom.toml` in `fathom-cli`, and a store that read its own budget
    /// would be a store whose cache behaviour cannot be reproduced from a bug report.
    ///
    /// Lowering the analysis-cache ceiling evicts to obey it now, so the number in the config file is
    /// the number in the resident set by the time this returns.
    pub fn set_ceilings(&mut self, ceilings: &fathom_core::memory::Ceilings) {
        self.payloads.set_limit(ceilings.analysis_cache_bytes);
    }

    /// The analysis payload cache's counters: hits, misses, high water, and what the ceiling evicted.
    #[must_use]
    pub fn payload_stats(&self) -> fathom_core::memory::CacheStats {
        self.payloads.observe()
    }

    /// Hand the analysis payload cache back to the allocator, and report the bytes that came back.
    ///
    /// The cache exists so the analyzers do not re-read what they just wrote — §11.8's *working set*, the
    /// one term of the sum that is a cache rather than a structure with a job. When they are done, the
    /// rest of the run has no use for it: the worktree pass reads files and the episode pass reads commit
    /// rows, neither of which asks for a commit payload again. A budget that measures the process's peak
    /// does not care that the phase which needed the bytes has finished, so the caller who knows that says
    /// so here, and the peak stops being the sum of every phase's worst moment.
    ///
    /// Nothing is forgotten: entries are a cache over the `blob` table, so a later miss is a read, and the
    /// counters are kept — `fathom stats` still says what the cache did while it was in use, and a released
    /// cache stays distinguishable from an unused one.
    pub fn release_payload_cache(&self) -> u64 {
        self.payloads.drain()
    }

    /// Accumulate a counter increment without writing it (§4.8).
    ///
    /// Counters are the observable behind `fathom index --stats` and behind ADR-004 §7.6's "degrade, never
    /// block" claim, and a cache lookup is not the place to pay for a write: the increments are summed in
    /// memory and flushed once per run by [`flush_counters`](Self::flush_counters).
    pub fn bump_counter(&mut self, name: &str, by: i64) {
        *self.pending.entry(name.to_owned()).or_insert(0) += by;
    }

    /// The accumulated, not-yet-written increments, for a report that wants them before a run closes.
    #[must_use]
    pub fn pending_counters(&self) -> &BTreeMap<String, i64> {
        &self.pending
    }

    /// Write the accumulated counters in one immediate transaction, returning how many names moved.
    ///
    /// The update is `value = value + excluded.value`: a counter is a running total, and `INSERT OR
    /// REPLACE` would turn a total into the last increment — §4.0's upsert rule again, and here a
    /// `REPLACE` would be a lie about history as well as a lost update.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the batch fails.
    pub fn flush_counters(&mut self, now: i64) -> Result<usize, Error> {
        // The payload cache's own ledger goes into the same transaction. It is folded as a delta,
        // because the cache counts from this process's start and the table counts from the store's;
        // writing the cumulative total every run would inflate the counter by whatever earlier runs of
        // the same process had already reported (`reco-93w`).
        let payloads = self.payloads.observe();
        let folded = self.payloads_folded;
        if payloads.evictions > folded.0 {
            self.bump_counter(
                "cache.payload.evict",
                i64::try_from(payloads.evictions - folded.0).unwrap_or(0),
            );
        }
        if payloads.evicted_bytes > folded.1 {
            self.bump_counter(
                "cache.payload.evict.bytes",
                i64::try_from(payloads.evicted_bytes - folded.1).unwrap_or(0),
            );
        }
        if payloads.refusals > folded.2 {
            self.bump_counter(
                "cache.payload.refused",
                i64::try_from(payloads.refusals - folded.2).unwrap_or(0),
            );
        }
        self.payloads_folded = (
            payloads.evictions,
            payloads.evicted_bytes,
            payloads.refusals,
        );
        if self.pending.is_empty() {
            return Ok(0);
        }
        let pending = std::mem::take(&mut self.pending);
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| corrupt("flushing counters", &err))?;
        {
            let mut statement = tx
                .prepare(
                    "INSERT INTO counter(name, value, updated_utc) VALUES (?1, ?2, ?3)
                     ON CONFLICT(name) DO UPDATE
                       SET value = value + excluded.value, updated_utc = excluded.updated_utc",
                )
                .map_err(|err| corrupt("writing counters", &err))?;
            for (name, value) in &pending {
                statement
                    .execute(params![name, value, now])
                    .map_err(|err| corrupt("writing counters", &err))?;
            }
        }
        tx.commit()
            .map_err(|err| corrupt("flushing counters", &err))?;
        Ok(pending.len())
    }

    /// Every stored counter whose name starts with `prefix`, in name order.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn counters(&self, prefix: &str) -> Result<BTreeMap<String, i64>, Error> {
        let mut statement = self
            .conn
            .prepare("SELECT name, value FROM counter WHERE name LIKE ?1 || \'%\' ORDER BY name")
            .map_err(|err| corrupt("reading counters", &err))?;
        let rows = statement
            .query_map(params![prefix], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|err| corrupt("reading counters", &err))?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (name, value) = row.map_err(|err| corrupt("reading counters", &err))?;
            let _ = out.insert(name, value);
        }
        Ok(out)
    }

    /// The §10.2 identity digest: a digest over the *content* of the derived tables, with the volatile
    /// bookkeeping projected out.
    ///
    /// Two stores built from one repository digest identically whatever order they were built in, how
    /// many runs it took, and when the clock said it happened. That is what makes "delete it and rebuild"
    /// a legal recovery (§9.1) rather than a gamble, and what lets a crash-recovery test assert that the
    /// recovered store *is* the store rather than merely a store that opens.
    ///
    /// Volatile by rule: any column whose name ends in `_utc`, plus `hits`, `cost_ms`, `interrupted`,
    /// `status`, `error` and `run_id`; the `meta` rows that record what the store process did rather than
    /// what the repository contains; and the whole tables `index_run`, `counter`, `store_event` and
    /// `usage_snapshot`, which are run logs — two legal builds reached the same place by different numbers
    /// of steps, and that is not a difference in the repository.
    pub fn content_digest(&self) -> Result<BTreeMap<String, String>, Error> {
        const VOLATILE_COLUMNS: &[&str] = &[
            "hits",
            "cost_ms",
            "interrupted",
            "status",
            "error",
            "run_id",
        ];
        const VOLATILE_META: &[&str] = &[
            "store_id",
            "created_utc",
            "index_epoch",
            "build_epoch",
            "epoch_status",
            "head",
            "writer_epoch",
            "journal_mode",
        ];
        const EXCLUDED_TABLES: &[&str] = &["index_run", "counter", "store_event", "usage_snapshot"];

        let tables: Vec<String> = {
            let mut statement = self
                .conn
                .prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table'
                       AND name NOT LIKE 'sqlite_%' ORDER BY name",
                )
                .map_err(|err| corrupt("listing tables", &err))?;
            statement
                .query_map([], |row| row.get(0))
                .map_err(|err| corrupt("listing tables", &err))?
                .collect::<Result<_, _>>()
                .map_err(|err| corrupt("listing tables", &err))?
        };
        let mut parts = BTreeMap::new();
        for table in tables {
            if EXCLUDED_TABLES.contains(&table.as_str()) {
                continue;
            }
            let columns: Vec<String> = {
                let mut statement = self
                    .conn
                    .prepare(&format!("PRAGMA table_info({table})"))
                    .map_err(|err| corrupt("listing columns", &err))?;
                statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .map_err(|err| corrupt("listing columns", &err))?
                    .collect::<Result<_, _>>()
                    .map_err(|err| corrupt("listing columns", &err))?
            };
            let keep: Vec<String> = columns
                .into_iter()
                .filter(|column| {
                    !column.ends_with("_utc") && !VOLATILE_COLUMNS.contains(&column.as_str())
                })
                .collect();
            if keep.is_empty() {
                continue;
            }
            let projection = keep
                .iter()
                .map(|column| format!("\"{column}\""))
                .collect::<Vec<_>>()
                .join(",");
            let query = if table == "meta" {
                let absent = VOLATILE_META
                    .iter()
                    .map(|key| format!("'{key}'"))
                    .collect::<Vec<_>>()
                    .join(",");
                // `commit_unreadable.*` is excluded by prefix rather than by name because there is one key
                // per commit, and each carries the instant it was written and the OS's own words about a
                // path on this machine. Two legal runs of the same damaged repository do not agree byte
                // for byte; they agree by commit id, which is how [`Store::unreadable_commits`] reads them.
                format!(
                    "SELECT {projection} FROM meta WHERE key NOT IN ({absent}) \
                     AND key NOT LIKE '{UNREADABLE_PREFIX}%' ORDER BY {projection}"
                )
            } else {
                format!("SELECT {projection} FROM {table} ORDER BY {projection}")
            };
            let mut hasher = sha2::Sha256::new();
            hasher.update(format!("{table}({projection})").as_bytes());
            // The projection is hashed before the rows, so a table whose columns were reordered digests
            // differently: the digest is over *this shape* of this table, not over a bag of its values.
            let mut statement = self
                .conn
                .prepare(&query)
                .map_err(|err| corrupt(format!("digesting {table}"), &err))?;
            let mut rows = statement
                .query([])
                .map_err(|err| corrupt(format!("digesting {table}"), &err))?;
            while let Some(row) = rows
                .next()
                .map_err(|err| corrupt(format!("digesting {table}"), &err))?
            {
                for index in 0..keep.len() {
                    let value: rusqlite::types::Value = row
                        .get(index)
                        .map_err(|err| corrupt(format!("reading {table}"), &err))?;
                    match value {
                        rusqlite::types::Value::Null => hasher.update(b"~"),
                        rusqlite::types::Value::Integer(value) => {
                            hasher.update(value.to_string());
                        }
                        rusqlite::types::Value::Real(value) => {
                            hasher.update(value.to_string());
                        }
                        rusqlite::types::Value::Text(text) => hasher.update(text),
                        rusqlite::types::Value::Blob(bytes) => hasher.update(bytes),
                    }
                    // One unit separator between columns: `ab|c` must not digest like `a|bc`.
                    hasher.update(b"\x1f");
                }
                hasher.update(b"\n");
            }
            parts.insert(table, crate::cache::sha256_hex(&hasher.finalize()));
        }
        Ok(parts)
    }
}

/// Encode a list as a JSON array of strings.
fn json_array(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| String::from("[]"))
}

/// Decode a JSON array of strings, tolerating the empty column a legacy row may hold.
fn json_strings(text: &str) -> Result<Vec<String>, rusqlite::Error> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(text).map_err(json_error)
}

/// Wrap a serde failure as a rusqlite failure, so every helper in this module can report one error type.
fn json_error(err: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
}

/// The store's one error-minting function, so the database's own words survive into the message.
pub(crate) fn corrupt(what: impl fmt::Display, err: &rusqlite::Error) -> Error {
    Error::StoreCorrupt {
        what: format!("{what}: {err}"),
    }
}

/// ADR-004 §4.0's loud failure type, re-exported so callers of the spine do not reach past it.
pub use crate::error::Error;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn commit(oid: &str) -> CommitRow {
        CommitRow {
            oid: oid.to_owned(),
            tree_oid: "b".repeat(40),
            parent_oids: vec!["a".repeat(40)],
            author_name: "Ada Lovelace".into(),
            author_email: "ada@example.invalid".into(),
            author_utc: 1_700_000_000_000,
            committer_utc: 1_700_000_000_000,
            committer_name: "t".to_owned(),
            committer_email: "t@example.invalid".to_owned(),
            message_head: "Add the thing".into(),
            message_hash: "c".repeat(64),
            trailers: BTreeMap::from([("agent-session".to_owned(), "s-1".to_owned())]),
            n_files: 1,
            n_add: 3,
            n_del: 1,
            n_binary: 0,
            oversized: false,
            files: vec![FileRow {
                path_hash: "d".repeat(64),
                new_path: "src/a.rs".into(),
                old_path: None,
                change_kind: "add".into(),
                mode_old: None,
                mode_new: Some(0o100644),
                is_binary: false,
                n_add: 3,
                n_del: 1,
                hunk_headers: vec!["@@ -1,2 +1,4 @@".into()],
                blob_old: None,
                blob_new: Some("e".repeat(40)),
                path_class: "unknown".into(),
                taxonomy_version: 0,
                noise_class: "unknown".into(),
            }],
        }
    }

    fn store() -> Store {
        Store::open_in_memory().expect("open")
    }

    #[test]
    fn pragmas_and_pragmas_are_what_adr_004_promise() {
        let store = store();
        assert_eq!(
            store.journal_mode().unwrap(),
            "memory",
            "in-memory has no WAL"
        );
        assert_eq!(
            pragma_int(&store.conn, "synchronous").unwrap(),
            1,
            "NORMAL is 1"
        );
        assert_eq!(pragma_int(&store.conn, "foreign_keys").unwrap(), 1);
        assert_eq!(pragma_int(&store.conn, "busy_timeout").unwrap(), 5000);
        assert_eq!(
            pragma_int(&store.conn, "cache_size").unwrap(),
            -(PAGE_CACHE_MIB * 1024),
            "the page cache is the budgeted per-connection cap"
        );
    }

    #[test]
    fn a_fresh_store_stamps_both_version_copies() {
        let store = store();
        assert_eq!(
            store.meta("schema_version").unwrap().unwrap(),
            SCHEMA_VERSION.to_string()
        );
        assert_eq!(store.meta("key_schema").unwrap().unwrap(), KEY_SCHEMA);
        assert_eq!(store.meta("epoch_status").unwrap().unwrap(), "ready");
        assert_eq!(
            pragma_int(&store.conn, "user_version").unwrap(),
            SCHEMA_VERSION
        );
        store.check().expect("integrity");
    }

    #[test]
    fn the_migrations_are_contiguous_and_cover_the_version_this_build_writes() {
        // ADR-004 §9.1: `MIGRATIONS` must cover 1..N with no gap, because a skipped migration is a store
        // missing a table that every later query assumes is there.
        for (index, (target, _)) in MIGRATIONS.iter().enumerate() {
            assert_eq!(i64::try_from(index + 1).unwrap_or_default(), *target);
        }
        assert_eq!(
            MIGRATIONS
                .last()
                .map(|(target, _)| *target)
                .unwrap_or_default(),
            SCHEMA_VERSION
        );
    }

    #[test]
    fn migration_two_lands_the_cache_tables_and_the_manifest() {
        let store = store();
        for table in ["analyzer", "cache", "blob", "blob_ref", "counter"] {
            let present: i64 = store
                .conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                present, 1,
                "{table} must exist or a later query is a panic waiting to happen"
            );
        }
        let rows: i64 = store
            .conn
            .query_row("SELECT count(*) FROM analyzer", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            usize::try_from(rows).unwrap_or_default(),
            crate::cache::ANALYZERS.len(),
            "the store carries the compiled-in manifest, not a copy of an old one"
        );
    }

    #[test]
    fn opening_an_e1_2_store_upgrades_it_in_place() {
        // The v1 stores `e1.2` wrote are on real disks already; migration 2 must pick them up rather than
        // refuse them.
        let fixture = temp_store();
        {
            let mut store = Store::open(&fixture).expect("open");
            store.conn.execute_batch("PRAGMA user_version = 1").unwrap();
            store
                .set_meta("schema_version", "1")
                .expect("pretend to be an e1.2 store");
        }
        let store = Store::open(&fixture).expect("reopen");
        assert_eq!(
            pragma_int(&store.conn, "user_version").unwrap(),
            SCHEMA_VERSION,
            "and the stamp follows the migration, not the other way round"
        );
        store.check().expect("integrity");
        drop(store);
        let _ = std::fs::remove_file(&fixture);
        let _ = std::fs::remove_file(format!("{}-wal", fixture.display()));
        let _ = std::fs::remove_file(format!("{}-shm", fixture.display()));
    }

    #[test]
    fn the_cheap_check_is_the_one_a_read_verb_can_afford() {
        // `reco-66q`. `fathom stats` and `fathom show` are budgeted in tens of milliseconds and, until
        // this ticket, paid a whole-database check on every process start to print "ok". The cheap path
        // (`Store::structural_check`, what every read verb now runs) therefore has to be able to say
        // something true — the geometry, the tables this build's readers assume, the version stamps — and
        // has to be *honest* about the things it does not look at, which is why the CLI prints which
        // level ran alongside the verdict.
        let fixture = temp_store();
        {
            let store = Store::open(&fixture).expect("open");
            let fresh = store.structural_check().expect("check");
            assert!(
                fresh.is_empty(),
                "a store this build just made is sound: {fresh:?}"
            );
            // A stamp that disagrees with this build is the cheap check's signature find: it costs nothing
            // to read, and it is the one that would otherwise present a v1 store's rows as if a v3 reader
            // had written them.
            store.conn.execute_batch("PRAGMA user_version = 7").unwrap();
            let cheap = store.structural_check().expect("check again");
            assert!(
                cheap.iter().any(|problem| problem.contains("user_version")),
                "the cheap check names the stamp: {cheap:?}"
            );
            assert!(
                store
                    .integrity_check(false)
                    .expect("doctor's battery")
                    .iter()
                    .any(|problem| problem.contains("user_version")),
                "and the expensive battery is a superset, not a different opinion"
            );
        }
        let _ = std::fs::remove_file(&fixture);
        let _ = std::fs::remove_file(format!("{}-wal", fixture.display()));
        let _ = std::fs::remove_file(format!("{}-shm", fixture.display()));
    }

    #[test]
    fn a_run_that_never_closed_leaves_its_commits_on_the_derivation_list() {
        let mut store = store();
        let doomed = store.begin_run("warm", Trigger::Commit, 1).unwrap();
        store
            .write_commits(doomed, 2, &[commit(&"1".repeat(40))])
            .unwrap();
        // The process died before `finish_run`, so the row still says `running` and its commit still owes
        // its derived rows — which is the state a warm run must not walk past.
        assert_eq!(store.oids_written_by(doomed).unwrap(), vec!["1".repeat(40)]);
        assert_eq!(
            store.oids_pending_derivation(doomed).unwrap(),
            vec!["1".repeat(40)],
            "the run in flight sees its own work, and the dead run's, and nothing else"
        );

        // The next run recovers the dead one and inherits its list: this is the whole of "the next run
        // finishes what a kill -9 interrupted", stated as a query rather than as a recovery pass.
        let next = store.begin_run("warm", Trigger::Commit, 3).unwrap();
        assert_eq!(
            store.oids_pending_derivation(next).unwrap(),
            vec!["1".repeat(40)]
        );

        // And once a run has closed cleanly the debt is paid: the crashed run's commits are not on every
        // later run's list, because a crash that happened once cannot be allowed to cost forever.
        store
            .finish_run(
                next,
                4,
                &RunStats {
                    commits_seen: 1,
                    commits_added: 1,
                    rows_written: 2,
                    interrupted: false,
                    error: None,
                },
            )
            .unwrap();
        let later = store.begin_run("warm", Trigger::Commit, 5).unwrap();
        assert!(
            store.oids_pending_derivation(later).unwrap().is_empty(),
            "one run's re-lookups is the bound"
        );
    }

    #[test]
    fn the_derivation_list_asks_a_parent_before_its_child() {
        // The walk emits newest-first, which is the order the rows land in, so the derivation list has to
        // come back the other way round: an analyzer that reads its parent's answer (`structure.delta`,
        // §6.1 R2) misses on every commit when the child is asked first, and a cold index then pays the
        // cost of the whole tree per commit instead of the cost of the change (`reco-66q`, measured on
        // `git/git`: 6.3 ms per commit that a parent-first order makes into microseconds).
        let mut store = store();
        let run = store.begin_run("cold", Trigger::Commit, 1).unwrap();
        store
            .write_commits(
                run,
                2,
                &[
                    commit(&"a".repeat(40)),
                    commit(&"b".repeat(40)),
                    commit(&"c".repeat(40)),
                ],
            )
            .unwrap();
        assert_eq!(
            store.oids_pending_derivation(run).unwrap(),
            vec!["c".repeat(40), "b".repeat(40), "a".repeat(40)],
            "walk order reversed: the oldest row first, so a parent is never asked about second"
        );
    }

    #[test]
    fn a_run_that_stopped_for_its_budget_leaves_its_commits_on_the_derivation_list() {
        // A slice budget is how every bounded indexer in this product works: `fathom watch`, `fathom
        // ambient`, and the open frame's own `<C-r>` all take what fits in a quarter second and come back
        // for the rest. Such a run does not *fail* — it closes `ok` with `interrupted` set — so a query
        // that only asks about unclean runs lets the next slice walk past everything the last one did not
        // reach, and a 500-commit backlog becomes an index that stopped at the first slice and said it
        // finished (`reco-j7t`).
        let mut store = store();
        let sliced = store.begin_run("warm", Trigger::Commit, 1).unwrap();
        store
            .write_commits(sliced, 2, &[commit(&"1".repeat(40))])
            .unwrap();
        store
            .finish_run(
                sliced,
                2,
                &RunStats {
                    commits_seen: 1,
                    commits_added: 1,
                    rows_written: 2,
                    interrupted: true,
                    error: None,
                },
            )
            .unwrap();

        let next = store.begin_run("warm", Trigger::Commit, 3).unwrap();
        assert_eq!(
            store.oids_pending_derivation(next).unwrap(),
            vec!["1".repeat(40)],
            "the next slice asks about what the last one did not reach"
        );

        // A chain of slices that all stop early keeps the debt: none of them has finished the work.
        store
            .finish_run(
                next,
                4,
                &RunStats {
                    commits_seen: 1,
                    commits_added: 1,
                    rows_written: 2,
                    interrupted: true,
                    error: None,
                },
            )
            .unwrap();
        let third = store.begin_run("warm", Trigger::Commit, 5).unwrap();
        assert_eq!(
            store.oids_pending_derivation(third).unwrap(),
            vec!["1".repeat(40)],
            "an interrupted run does not pay a previous one's debt"
        );

        // And one clean run pays it, so a budget that tripped once does not cost every warm run for the
        // rest of the week — the same bound `a_run_that_never_closed…` holds a crash to.
        store
            .finish_run(
                third,
                6,
                &RunStats {
                    commits_seen: 1,
                    commits_added: 1,
                    rows_written: 2,
                    interrupted: false,
                    error: None,
                },
            )
            .unwrap();
        let fourth = store.begin_run("warm", Trigger::Commit, 7).unwrap();
        assert!(
            store.oids_pending_derivation(fourth).unwrap().is_empty(),
            "the debt is one run's re-lookups, not a permanent one"
        );
    }

    /// A path in a private temporary directory, removed on drop.
    fn temp_store() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "fathom-store-test-{}-{}.sqlite3",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn strict_tables_refuse_the_wrong_type_rather_than_coercing() {
        let store = store();
        let err = store
            .conn
            .execute(
                "INSERT INTO git_commit_file(commit_oid, path_hash, new_path, change_kind, is_binary,
                    n_add, n_del, hunk_headers, path_class, taxonomy_version, noise_class)
                 VALUES ('x','y','z','add','not-an-int',1,1,'[]','unknown',0,'unknown')",
                [],
            )
            .unwrap_err();
        assert!(
            err.to_string()
                .to_ascii_lowercase()
                .contains("cannot store"),
            "STRICT should refuse a TEXT where INTEGER is declared: {err}"
        );
    }

    #[test]
    fn writing_the_same_commit_twice_writes_nothing_twice() {
        let mut store = store();
        let run = store.begin_run("cold", Trigger::Manual, 1).unwrap();
        let first = store
            .write_commits(run, 10, &[commit(&"1".repeat(40))])
            .unwrap();
        assert_eq!(first, 2, "one commit row and one file row");
        let again = store
            .write_commits(run, 11, &[commit(&"1".repeat(40))])
            .unwrap();
        assert_eq!(again, 2, "the statements run and change nothing");
        assert_eq!(store.counts().unwrap(), (1, 1));
        let known = store.known_oids(&["1".repeat(40), "2".repeat(40)]).unwrap();
        assert_eq!(known.len(), 1, "and the resume query agrees");
    }

    #[test]
    fn a_file_row_without_its_commit_is_refused_not_orphaned() {
        let store = store();
        let err = store.conn.execute(
            "INSERT INTO git_commit_file(commit_oid, path_hash, new_path, change_kind, is_binary,
                n_add, n_del, hunk_headers, path_class, taxonomy_version, noise_class)
             VALUES ('nope','y','z','add',0,1,1,'[]','unknown',0,'unknown')",
            [],
        ).unwrap_err();
        assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
    }

    #[test]
    fn a_run_left_running_is_stamped_crashed_by_the_next_one() {
        let mut store = store();
        let doomed = store.begin_run("cold", Trigger::Manual, 1).unwrap();
        // The process that opened it never came back; it cannot stamp its own row.
        let run = store.begin_run("warm", Trigger::Commit, 5).unwrap();
        assert_ne!(doomed, run);
        let (status, error): (String, Option<String>) = store
            .conn
            .query_row(
                "SELECT status, error FROM index_run WHERE id = ?1",
                params![doomed],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "crashed");
        assert!(error.unwrap().contains("recovery"));
    }

    #[test]
    fn a_run_closes_with_its_numbers() {
        let mut store = store();
        let run = store.begin_run("cold", Trigger::Manual, 1).unwrap();
        store
            .finish_run(
                run,
                2,
                &RunStats {
                    commits_seen: 7,
                    commits_added: 5,
                    rows_written: 11,
                    interrupted: true,
                    error: None,
                },
            )
            .unwrap();
        let row: (String, i64, i64, i64) = store
            .conn
            .query_row(
                "SELECT status, commits_seen, rows_written, interrupted FROM index_run WHERE id = ?1",
                params![run],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.0, "ok");
        assert_eq!((row.1, row.2, row.3), (7, 11, 1));
    }

    #[test]
    fn a_round_trip_keeps_json_columns_exact() {
        let mut store = store();
        let run = store.begin_run("cold", Trigger::Manual, 1).unwrap();
        let row = commit(&"9".repeat(40));
        store
            .write_commits(run, 10, std::slice::from_ref(&row))
            .unwrap();
        let back = store.read_commit(&"9".repeat(40)).unwrap();
        assert_eq!(
            back, row,
            "parents, trailers and hunks all survive their JSON columns"
        );
    }

    #[test]
    fn ref_tips_are_a_snapshot_not_a_history() {
        let mut store = store();
        store
            .replace_ref_tips(
                &[
                    ("refs/heads/main".into(), "1".repeat(40), "branch"),
                    ("refs/tags/v1".into(), "2".repeat(40), "tag"),
                ],
                3,
            )
            .unwrap();
        store
            .replace_ref_tips(&[("refs/heads/main".into(), "3".repeat(40), "branch")], 4)
            .unwrap();
        let rows: Vec<(String, String)> = store
            .conn
            .prepare("SELECT name, oid FROM ref_tip")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "3".repeat(40));
    }

    #[test]
    fn a_store_id_is_minted_once() {
        let mut store = store();
        assert_eq!(store.store_id(|| "mine".into()).unwrap(), "mine");
        assert_eq!(store.store_id(|| "other".into()).unwrap(), "mine");
    }
}

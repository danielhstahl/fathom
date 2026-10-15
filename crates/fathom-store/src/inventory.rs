//! `inventory.rs` — the read-only questions `fathom stats` and `fathom doctor` ask the store.
//!
//! ADR-002 §4.3 keeps SQL out of `fathom-cli`: the composition root parses arguments, calls downhill, and
//! picks an exit code. So the aggregate reads that two verbs need — how big is the repository, what state
//! is the index in, how much of the cache is answering — live here, next to the tables they read, and both
//! verbs print the same numbers because they ask the same function. That is the only reason this module
//! exists; a second `SELECT count(*) FROM git_commit` in the CLI would be the first drift.
//!
//! Everything here is a read. Nothing takes `&mut self`, nothing writes a counter, nothing decides what a
//! stale index *means* — that judgement is the verb's, and it is written down in
//! `docs/concepts/cli.md` next to the exit code it produces.
//!
//! The aggregates are plain structs of primitives and `BTreeMap`s rather than `serde_json::Value`: the
//! store reports facts, and the JSON key names are the CLI's published contract (`schemas/`), not this
//! crate's.

use std::collections::BTreeMap;
use std::fmt;

use rusqlite::{Connection, OptionalExtension as _, params};

use crate::error::Error;

fn corrupt(what: impl fmt::Display, err: &rusqlite::Error) -> Error {
    Error::StoreCorrupt {
        what: format!("{what}: {err}"),
    }
}

/// How much of the repository the store holds, and how heavy it is.
///
/// "Repo scale" in `fathom stats`, in the units the spine stores. Every figure is a column the ingest
/// pipeline already wrote, so this is a read and not a second walk — the difference ADR-004 §6.1 R1 makes
/// for a warm run, applied to a status command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scale {
    /// Rows in `git_commit`.
    pub commits: i64,
    /// Rows in `git_commit_file`.
    pub files: i64,
    /// Distinct `author_name <author_email>` pairs.
    pub authors: i64,
    /// Lines added, summed over commit rows (the store's own figures, not a re-diff).
    pub lines_added: i64,
    /// Lines removed, summed over commit rows.
    pub lines_removed: i64,
    /// File rows measured binary.
    pub binary_files: i64,
    /// Commits with at least one oversized file (the size cap tripped, ADR-004 §7.4).
    pub oversized_commits: i64,
    /// Oldest `author_utc` in the store, unix millis. `None` for an empty store.
    pub first_commit_utc: Option<i64>,
    /// Newest `author_utc` in the store, unix millis.
    pub last_commit_utc: Option<i64>,
    /// File rows per `noise_class`, the taxonomy's own vocabulary.
    pub noise: BTreeMap<String, i64>,
    /// File rows per `change_kind`.
    pub kinds: BTreeMap<String, i64>,
    /// The five most frequent authors, busiest first: `(author, commits)`.
    pub top_authors: Vec<(String, i64)>,
}

/// One `index_run` row, projected to what a status report prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    /// `index_run.id`.
    pub id: i64,
    /// `cold`, `warm`, `rebuild`, …
    pub kind: String,
    /// `manual`, `commit`, `recovery`, …
    pub trigger: String,
    /// `ok`, `running`, `aborted`, `crashed`.
    pub status: String,
    /// Unix millis.
    pub started_utc: i64,
    /// Unix millis, or `None` while the run is open.
    pub finished_utc: Option<i64>,
    /// Commits the walk emitted.
    pub commits_seen: i64,
    /// Commits this run wrote.
    pub commits_added: i64,
    /// Store rows this run wrote.
    pub rows_written: i64,
    /// The progress budget tripped before the walk finished (ADR-004 §7.6).
    pub interrupted: bool,
    /// The failure that ended the run, if it did not end cleanly.
    pub error: Option<String>,
}

/// The run log, summarised: what happened last, and how many runs did not finish.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunSummary {
    /// Total runs recorded.
    pub total: i64,
    /// Runs stamped `crashed` — a process that died holding the writer.
    pub crashed: i64,
    /// Runs still stamped `running` at read time. Either a live writer or a process that died between
    /// `begin_run` and the recovery pass; either way a status command should say so out loud.
    pub open: i64,
    /// Runs that ended `aborted`.
    pub aborted: i64,
    /// The newest run, if any was ever opened.
    pub last: Option<RunRow>,
}

/// How much derived data the cache holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheInventory {
    /// Rows in `cache`.
    pub rows: i64,
    /// Rows in `cache` unreachable in the reader's epoch (§6.3) — old epochs are dead weight until prune.
    pub stale_epoch_rows: i64,
    /// Rows whose `error` column is set: stored failures, which are answers (§5.6), not defects.
    pub stored_failures: i64,
    /// Analyzers registered from the compiled-in manifest.
    pub analyzers: i64,
    /// Analyzers registered in the store that this build's manifest no longer lists.
    pub unknown_analyzers: i64,
    /// Rows in `blob`.
    pub blobs: i64,
    /// Payload bytes across all blobs.
    pub blob_bytes: i64,
    /// Blobs kept in the row rather than under `objects/`.
    pub blobs_inline: i64,
    /// Blobs kept as files under `objects/`.
    pub blobs_on_disk: i64,
    /// Payload bytes served from cache over the store's lifetime (`counter`).
    pub served_bytes: i64,
}

/// The ref tips the last index saw, so a status command can say "the index is at X" rather than "whenever".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefTip {
    /// Ref name as stored (`refs/heads/main`, `HEAD`).
    pub name: String,
    /// The oid it pointed at.
    pub oid: String,
    /// `branch`, `tag`, `head`, `remote`.
    pub kind: String,
    /// Unix millis of the snapshot.
    pub updated_utc: i64,
}

/// One row of `pragma_compile_options` plus the version string, for `fathom doctor`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SqliteProvenance {
    /// `sqlite_version()`.
    pub version: String,
    /// The compile options the linked library reports.
    pub compile_options: Vec<String>,
    /// The journal mode this connection settled on (`wal`, or a fallback the filesystem forced).
    pub journal_mode: String,
    /// `page_size` in bytes.
    pub page_size: i64,
    /// `encoding`.
    pub encoding: String,
    /// Whether the four capabilities ADR-004 §4.0 is written against are usable *in this connection* —
    /// probed by using them, because `STRICT` and JSON are not reliably listed in `compile_options`.
    pub capabilities: BTreeMap<String, bool>,
}

impl crate::spine::Store {
    /// The integrity checks `fathom doctor` runs: `PRAGMA quick_check` always, `PRAGMA integrity_check`
    /// as well on `--deep` (ADR-004 §9.6).
    ///
    /// Returns the problems, which is empty when the store is sound, rather than a boolean: the sentence
    /// doctor prints has to name *which* page or index disagreed, and a caller that got `false` would
    /// print "corrupt" and nothing else.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a check statement itself could not run, which is a different fact from
    /// "the store reported a problem" and is reported as such.
    pub fn integrity_check(&self, deep: bool) -> Result<Vec<String>, Error> {
        let mut problems = self.check_statement("quick_check", "PRAGMA quick_check")?;
        if deep {
            problems.extend(self.check_statement("integrity_check", "PRAGMA integrity_check")?);
        }
        // ADR-004 §9.1's other two checks are the spine's own: foreign keys and the version stamps.
        let conn = self.connection();
        let broken: i64 = conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .map_err(|err| corrupt("checking foreign keys", &err))?;
        if broken != 0 {
            problems.push(format!("{broken} foreign key violations"));
        }
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|err| corrupt("reading user_version", &err))?;
        if version != crate::spine::SCHEMA_VERSION {
            problems.push(format!(
                "user_version is {version}, this build writes {}",
                crate::spine::SCHEMA_VERSION
            ));
        }
        Ok(problems)
    }

    /// Run one check pragma and collect its answer lines. A check that answers `ok` is a store with no
    /// problems, and `ok` is filtered here rather than at every call site.
    fn check_statement(&self, label: &str, sql: &str) -> Result<Vec<String>, Error> {
        let mut statement = self
            .connection()
            .prepare(sql)
            .map_err(|err| corrupt(format!("running {label}"), &err))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|err| corrupt(format!("running {label}"), &err))?;
        let mut out = Vec::new();
        for row in rows {
            let line = row.map_err(|err| corrupt(format!("reading {label}"), &err))?;
            if line != "ok" {
                out.push(format!("{label}: {line}"));
            }
        }
        Ok(out)
    }

    /// The repository's scale, as the store holds it.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if any aggregate read fails.
    pub fn scale(&self) -> Result<Scale, Error> {
        let conn = self.connection();
        let mut scale = Scale::default();
        (scale.commits, scale.files) = conn
            .query_row(
                "SELECT (SELECT count(*) FROM git_commit),
                        (SELECT count(*) FROM git_commit_file)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|err| corrupt("counting the spine", &err))?;
        (
            scale.authors,
            scale.lines_added,
            scale.lines_removed,
            scale.binary_files,
            scale.oversized_commits,
        ) = conn
            .query_row(
                "SELECT count(DISTINCT author_name || ' <' || author_email || '>'),
                        coalesce(sum(n_add), 0),
                        coalesce(sum(n_del), 0),
                        coalesce(sum(n_binary), 0),
                        coalesce(sum(oversized), 0)
                 FROM git_commit",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(|err| corrupt("summing the spine", &err))?;
        (scale.first_commit_utc, scale.last_commit_utc) = conn
            .query_row(
                "SELECT min(author_utc), max(author_utc) FROM git_commit",
                [],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .map_err(|err| corrupt("reading the date range", &err))?;
        scale.noise = self.grouped("noise_class")?;
        scale.kinds = self.grouped("change_kind")?;
        scale.top_authors = {
            let mut statement = conn
                .prepare(
                    "SELECT author_name || ' <' || author_email || '>', count(*) AS n
                     FROM git_commit GROUP BY author_name, author_email
                     ORDER BY n DESC, author_name LIMIT 5",
                )
                .map_err(|err| corrupt("grouping by author", &err))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|err| corrupt("grouping by author", &err))?;
            rows.collect::<Result<_, _>>()
                .map_err(|err| corrupt("grouping by author", &err))?
        };
        Ok(scale)
    }

    /// One `GROUP BY` over `git_commit_file`, for the two histograms in [`Scale`].
    fn grouped(&self, column: &str) -> Result<BTreeMap<String, i64>, Error> {
        // `column` is one of two literals below, never caller input; the interpolation is for the query
        // planner, not for a string the store's caller controls.
        debug_assert!(matches!(column, "noise_class" | "change_kind"));
        let mut statement = self
            .connection()
            .prepare(&format!(
                "SELECT {column}, count(*) FROM git_commit_file GROUP BY {column} ORDER BY {column}"
            ))
            .map_err(|err| corrupt("grouping file rows", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|err| corrupt("grouping file rows", &err))?;
        rows.collect::<Result<_, _>>()
            .map_err(|err| corrupt("grouping file rows", &err))
    }

    /// The run log, summarised.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn run_summary(&self) -> Result<RunSummary, Error> {
        let conn = self.connection();
        let mut summary = RunSummary::default();
        (
            summary.total,
            summary.crashed,
            summary.open,
            summary.aborted,
        ) = conn
            .query_row(
                "SELECT count(*),
                        coalesce(sum(status = 'crashed'), 0),
                        coalesce(sum(status = 'running'), 0),
                        coalesce(sum(status = 'aborted'), 0)
                 FROM index_run",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|err| corrupt("summarising the run log", &err))?;
        summary.last = conn
            .query_row(
                "SELECT id, kind, trigger, status, started_utc, finished_utc, commits_seen,
                        commits_added, rows_written, interrupted, error
                 FROM index_run ORDER BY id DESC LIMIT 1",
                [],
                |row| {
                    Ok(RunRow {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        trigger: row.get(2)?,
                        status: row.get(3)?,
                        started_utc: row.get(4)?,
                        finished_utc: row.get(5)?,
                        commits_seen: row.get(6)?,
                        commits_added: row.get(7)?,
                        rows_written: row.get(8)?,
                        interrupted: row.get::<_, i64>(9)? != 0,
                        error: row.get(10)?,
                    })
                },
            )
            .optional()
            .map_err(|err| corrupt("reading the last index run", &err))?;
        Ok(summary)
    }

    /// The cache's own inventory.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a read fails.
    pub fn cache_inventory(&self) -> Result<CacheInventory, Error> {
        let conn = self.connection();
        let epoch = self.epoch()?;
        let mut inventory = CacheInventory::default();
        (
            inventory.rows,
            inventory.stale_epoch_rows,
            inventory.stored_failures,
        ) = conn
            .query_row(
                "SELECT count(*),
                    coalesce(sum(epoch <> ?1), 0),
                    coalesce(sum(error IS NOT NULL), 0)
             FROM cache",
                params![epoch],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|err| corrupt("counting cache rows", &err))?;
        inventory.analyzers = conn
            .query_row("SELECT count(*) FROM analyzer", [], |row| row.get(0))
            .map_err(|err| corrupt("counting analyzers", &err))?;
        inventory.unknown_analyzers = conn
            .query_row(
                "SELECT count(*) FROM cache c
                 WHERE NOT EXISTS (SELECT 1 FROM analyzer a WHERE a.id = c.analyzer_id)",
                [],
                |row| row.get(0),
            )
            .map_err(|err| corrupt("looking for unregistered analyzers", &err))?;
        (
            inventory.blobs,
            inventory.blob_bytes,
            inventory.blobs_inline,
            inventory.blobs_on_disk,
        ) = conn
            .query_row(
                "SELECT count(*), coalesce(sum(bytes), 0),
                        coalesce(sum(storage = 'inline'), 0),
                        coalesce(sum(storage = 'file'), 0)
                 FROM blob",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|err| corrupt("counting blobs", &err))?;
        inventory.served_bytes = self
            .counters("cache.")?
            .iter()
            .filter(|(name, _)| name.starts_with("cache.bytes."))
            .map(|(_, value)| *value)
            .sum();
        // The counters count lookups that *computed* a payload as well as lookups that found one, so
        // `puts` is the row count the cache actually owes (§4.8); a first run would otherwise report an
        // inventory of zero rows next to a hit rate over three lookups.
        inventory.rows = self
            .counters("cache.")?
            .iter()
            .filter(|(name, _)| name.starts_with("cache.put."))
            .map(|(_, value)| *value)
            .sum();
        Ok(inventory)
    }

    /// The commits the index still owes an analysis for: what the newest open run wrote, plus anything a
    /// run that never closed wrote and no later clean run has finished off.
    ///
    /// This is the same query the warm index path uses, with no run of its own to add, which is the point:
    /// `fathom stats` and `fathom index` must agree about what "incomplete" means or the status command is
    /// decoration.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn pending_derivation(&self) -> Result<Vec<String>, Error> {
        self.oids_pending_derivation(0)
    }

    /// The ref tips the last index snapshot recorded.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn ref_tips(&self) -> Result<Vec<RefTip>, Error> {
        let mut statement = self
            .connection()
            .prepare("SELECT name, oid, kind, updated_utc FROM ref_tip ORDER BY name")
            .map_err(|err| corrupt("reading the ref snapshot", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok(RefTip {
                    name: row.get(0)?,
                    oid: row.get(1)?,
                    kind: row.get(2)?,
                    updated_utc: row.get(3)?,
                })
            })
            .map_err(|err| corrupt("reading the ref snapshot", &err))?;
        rows.collect::<Result<_, _>>()
            .map_err(|err| corrupt("reading the ref snapshot", &err))
    }

    /// Which of `oids` the store has a commit row for — the membership question behind "is HEAD indexed?".
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn indexed_oids(&self, oids: &[String]) -> Result<Vec<String>, Error> {
        let known = self.known_oids(oids)?;
        Ok(known.into_iter().collect())
    }

    /// What the linked SQLite can do, as the store's own connection reports it (ADR-004 §4.0).
    ///
    /// The four capabilities are probed by using them rather than by reading `pragma_compile_options`,
    /// for the reason `spine` gives: `STRICT` and JSON are not reliably listed there, and a check that
    /// reads a list nobody maintains is a check that passes.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a pragma cannot be read.
    pub fn sqlite_provenance(&self) -> Result<SqliteProvenance, Error> {
        let conn: &Connection = self.connection();
        let version = conn
            .query_row("SELECT sqlite_version()", [], |row| row.get(0))
            .map_err(|err| corrupt("reading the SQLite version", &err))?;
        let mut provenance = SqliteProvenance {
            version,
            ..SqliteProvenance::default()
        };
        {
            let mut statement = conn
                // The column's name is the pragma's own; spelling it out as `name` worked on one
                // build of SQLite and not on another, and a probe that depends on a column name it does
                // not own is a probe that fails on someone's distro SQLite.
                .prepare("SELECT DISTINCT * FROM pragma_compile_options ORDER BY 1")
                .map_err(|err| corrupt("reading compile options", &err))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|err| corrupt("reading compile options", &err))?;
            provenance.compile_options = rows
                .collect::<Result<_, _>>()
                .map_err(|err| corrupt("reading compile options", &err))?;
        }
        provenance.journal_mode = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap_or_default();
        provenance.page_size = conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .unwrap_or(0);
        provenance.encoding = conn
            .query_row("PRAGMA encoding", [], |row| row.get(0))
            .unwrap_or_default();
        for (name, probe) in [
            (
                "STRICT",
                "CREATE TEMP TABLE _fathom_probe_strict (x INTEGER) STRICT",
            ),
            ("JSON1", "SELECT json_extract('{\"a\":1}', '$.a')"),
            (
                "FTS5",
                "CREATE VIRTUAL TABLE temp._fathom_probe_fts USING fts5(x)",
            ),
        ] {
            // A probe that succeeds is a capability this connection can use; a refusal is the fact the
            // caller wants, so the error is thrown away and the boolean is the answer.
            let works = conn.execute_batch(probe).is_ok();
            let _ = conn.execute_batch("DROP TABLE IF EXISTS temp._fathom_probe_strict");
            let _ = conn.execute_batch("DROP TABLE IF EXISTS temp._fathom_probe_fts");
            provenance.capabilities.insert(name.to_owned(), works);
        }
        Ok(provenance)
    }
}

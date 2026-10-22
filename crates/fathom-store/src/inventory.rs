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
//! `docs/concepts/cli.md` next to the exit code it produces. The one exception is the reachability walk
//! [`Store::inventory`] needs, which materialises its working set in a `temp` table: per-connection, gone
//! when the read ends, and never a byte of the store itself.
//!
//! The aggregates are plain structs of primitives and `BTreeMap`s rather than `serde_json::Value`: the
//! store reports facts, and the JSON key names are the CLI's published contract (`schemas/`), not this
//! crate's.

use std::collections::BTreeMap;
use std::fmt;

use rusqlite::{Connection, OptionalExtension as _, params};

use crate::error::Error;
use crate::spine::Store;

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
///
/// The figures count the commits the refs still reach, not every row in the file: a rebase leaves five
/// commit rows behind whose commits git has already pruned, and a status command that adds them to the
/// repository's size is the defect `reco-218` is about. What was excluded is [`DeadRows`], read by the same
/// statement in the same [`Store::inventory`] call, so the two numbers cannot disagree about which rows are
/// which. When the store cannot judge reachability at all the figures fall back to every row — an
/// unjudgeable store is reported as a whole store with `unjudged` set, never as a store that shrank.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scale {
    /// Commit rows a ref still reaches.
    pub commits: i64,
    /// `git_commit_file` rows those commits carry.
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

/// The rows the store retains for commits no ref can reach any more.
///
/// A rewrite — `rebase`, `commit --amend`, a force-push — replaces commits by identity while leaving their
/// rows behind, and `git gc --prune=now` then deletes the objects, so the repository has *fewer* commits
/// than the index counts. The spine is append-only (ADR-004 §6.2: mark-dead, do not delete on the event
/// path) and the pruner is the only thing that deletes (§8.1), so the rows are expected; what was not
/// expected is a report that counts them as live and calls the difference nothing.
///
/// `unjudged` is the honest third case. A store cannot always tell dead from merely-missing, and saying so
/// is the difference between a count and a guess:
///
/// * a reachable commit names a parent this store has no row for (a shallow clone's graft, an object the
///   walk could not read), so everything below that break is unverifiable — the rows are not provably
///   dead, they are unprovable, and `dead` stays 0;
/// * no ref tip resolves to a row at all, or a row's parent list could not be read, so nothing was judged:
///   every row is `unjudged` and `reachable` equals `retained`, which is the same "no opinion keeps
///   everything" rule [`crate::episode`] applies to the segmentation input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeadRows {
    /// Rows in `git_commit`: everything the file holds, reachable or not.
    pub retained: i64,
    /// Rows a ref tip reaches through the stored parent pointers.
    pub reachable: i64,
    /// Rows retained and reached by nothing: provably the ghosts of a rewritten history.
    pub dead: i64,
    /// Rows that are neither reachable nor provably dead — see the type's docs.
    pub unjudged: i64,
    /// `git_commit_file` rows the dead commits carry: what a prune reclaims along with them.
    pub dead_files: i64,
    /// Ref tips with no commit row: the walk could not start from them, which is a fact worth printing
    /// beside an `unjudged` that is "everything".
    pub tips_without_rows: i64,
    /// Up to [`DeadRows::SAMPLE`] dead oids, newest first — enough for a report line to name the commits
    /// it means rather than gesture at them.
    pub sample: Vec<String>,
    /// Why nothing could be judged, when nothing could: the store's own error text for a walk that would
    /// not run. `None` whenever the walk ran, whatever it concluded.
    pub reason: Option<String>,
}

impl DeadRows {
    /// How many dead oids [`DeadRows::sample`] holds at most: a finding line has room for three, and a
    /// `--json` consumer wants a few more than that.
    pub const SAMPLE: usize = 12;

    /// Did the store reach a judgement at all?
    #[must_use]
    pub fn judged(&self) -> bool {
        self.reason.is_none()
    }

    /// Is the set empty — every retained row a ref still reaches?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dead == 0
    }
}

/// One read of the spine: how big the reachable repository is, and what the store is holding that is not
/// part of it.
///
/// One read rather than two because both halves need the same reachability walk, and a status command that
/// walked twice would be paying twice to be allowed to disagree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    /// The scale of what the refs reach.
    pub scale: Scale,
    /// What the refs do not reach.
    pub dead: DeadRows,
}

/// The SQL that fills `temp.fathom_live` with every commit row a stored ref tip reaches.
///
/// The roots are the tips of kind `head`, `branch` and `remote`: a commit a remote-tracking ref names is on
/// a branch this checkout has not adopted, which is a fact about the checkout and not about the repository,
/// so it is not dead. Tags are left out for the reason [`crate::episode::Store::ref_tip_oids`] gives — a tag
/// pinned to a rebased-away commit records the release that commit *was*, and is not a request to keep
/// showing the row. The episode step walks the narrower set because it segments one lineage; this walk
/// asks what the repository still contains, and every name a human could type answers that.
const LIVE_WALK: &str = "\
    INSERT OR IGNORE INTO temp.fathom_live (oid) \
    WITH RECURSIVE live(oid) AS ( \
        SELECT oid FROM ref_tip WHERE kind IN ('head','branch','remote') \
        UNION \
        SELECT parent.value FROM live \
        JOIN git_commit c ON c.oid = live.oid \
        JOIN json_each(c.parent_oids) AS parent \
    ) \
    SELECT oid FROM live";

/// Parent oids the reachable commits name that this store has no row for: the breaks in the chain.
const CHAIN_BREAKS: &str = "\
    SELECT count(*) FROM temp.fathom_live l \
    JOIN git_commit c ON c.oid = l.oid \
    JOIN json_each(c.parent_oids) AS parent \
    WHERE NOT EXISTS (SELECT 1 FROM git_commit g WHERE g.oid = parent.value)";

/// The commit rows the walk did not reach.
const DEAD_COUNT: &str = "\
    SELECT count(*) FROM git_commit \
    WHERE oid NOT IN (SELECT oid FROM temp.fathom_live)";

/// The same set as oids, exported for the pruner, which turns it into a work list.
pub(crate) const UNREACHABLE_OIDS: &str = "\
    SELECT oid FROM git_commit \
    WHERE oid NOT IN (SELECT oid FROM temp.fathom_live)";

/// The file rows those commits carry.
const DEAD_FILES: &str = "\
    SELECT count(*) FROM git_commit_file \
    WHERE commit_oid NOT IN (SELECT oid FROM temp.fathom_live)";

/// The same set, newest first, for a report line that has to name what it means.
const DEAD_SAMPLE: &str = "\
    SELECT oid FROM git_commit \
    WHERE oid NOT IN (SELECT oid FROM temp.fathom_live) \
    ORDER BY author_utc DESC, oid LIMIT ?1";

/// The ref tips the walk could not start from.
const TIPS_WITHOUT_ROWS: &str = "\
    SELECT count(*) FROM ref_tip t \
    WHERE t.kind IN ('head','branch','remote') \
    AND NOT EXISTS (SELECT 1 FROM git_commit g WHERE g.oid = t.oid)";

/// The reachability walk, materialised once in a temp table.
///
/// A temp table rather than a `WITH RECURSIVE` prefix on every statement because one read issues half a
/// dozen of them, and a CTE is re-evaluated per statement: the walk would run six times to answer one
/// question. Temp objects are already this module's idiom — [`Store::sqlite_provenance`] probes `STRICT`
/// and FTS5 by making one — and they are per-connection, so a reader in another process cannot see or
/// disturb them.
///
/// When the walk cannot run the table is filled with *every* commit row, which makes every join below a
/// no-op: an unjudgeable store reports its whole scale and says so through [`DeadRows::reason`] instead of
/// reporting a history that shrank for no reason.
pub(crate) struct LiveSet<'a> {
    store: &'a Store,
    /// Whether the walk ran, and so whether "not in the table" means "dead".
    walked: bool,
    /// Why it did not run, when it did not.
    reason: Option<String>,
    /// How many commit rows the table holds.
    matched: i64,
    /// How many commit rows exist.
    retained: i64,
    /// Whether the joins are worth issuing at all: when every row is in the set they would filter nothing.
    filter: bool,
    /// How many parents the reachable commits name that have no row.
    breaks: i64,
}

impl<'a> LiveSet<'a> {
    pub(crate) fn build(store: &'a Store) -> Result<Self, Error> {
        let conn = store.connection();
        let mut set = Self {
            store,
            walked: false,
            reason: None,
            matched: 0,
            retained: 0,
            filter: false,
            breaks: 0,
        };
        set.retained = conn
            .query_row("SELECT count(*) FROM git_commit", [], |row| row.get(0))
            .map_err(|err| corrupt("counting the spine", &err))?;
        // A fresh set every time: the table is this read's working memory, and a row left over from the
        // last read would be a live commit that stopped existing between two statements.
        if let Err(err) = conn.execute_batch(
            "DROP TABLE IF EXISTS temp.fathom_live; \
             CREATE TABLE temp.fathom_live (oid TEXT PRIMARY KEY) WITHOUT ROWID;",
        ) {
            set.reason = Some(format!("the reachability walk could not start: {err}"));
            return Ok(set);
        }
        match conn.execute(LIVE_WALK, []) {
            Ok(_) => set.walked = true,
            // The one way this statement can fail on a structurally sound store is a `parent_oids` the
            // JSON parser will not take — damage, not an absence. Either way the walk has no answer, and
            // the answer to "no answer" is to say so, not to report a shrunken history.
            Err(err) => set.reason = Some(format!("the reachability walk could not run: {err}")),
        }
        if !set.walked {
            // No opinion: every row is in the set, so nothing is excluded from the scale. If *this* fails
            // there is no read left to fall back on, and the store is as unreadable as the error says.
            let fill = conn.execute_batch(
                "DELETE FROM temp.fathom_live; \
                 INSERT OR IGNORE INTO temp.fathom_live (oid) SELECT oid FROM git_commit;",
            );
            if let Err(err) = fill {
                return Err(corrupt(
                    set.reason
                        .take()
                        .unwrap_or_else(|| "the reachability set could not be filled".to_owned()),
                    &err,
                ));
            }
        } else {
            // A tip that names nothing is not a tip with an empty history behind it — it is a tip the walk
            // could not read, and a store whose every tip is one has no roots to judge from. Same rule as
            // `crate::episode`'s reachability step, for the same reason: treating it as "reachable: none"
            // would report a whole repository as a graveyard.
            let rooted: i64 = conn
                .query_row(
                    "SELECT count(*) FROM temp.fathom_live l \
                     WHERE EXISTS (SELECT 1 FROM git_commit g WHERE g.oid = l.oid)",
                    [],
                    |row| row.get(0),
                )
                .map_err(|err| corrupt("reading the reachability set", &err))?;
            if rooted == 0 {
                set.walked = false;
                set.reason = Some(
                    "no ref tip has a commit row, so nothing could be judged reachable".to_owned(),
                );
                conn.execute_batch(
                    "DELETE FROM temp.fathom_live; \
                     INSERT OR IGNORE INTO temp.fathom_live (oid) SELECT oid FROM git_commit;",
                )
                .map_err(|err| corrupt("filling the reachability set", &err))?;
            }
        }
        set.matched = conn
            .query_row("SELECT count(*) FROM temp.fathom_live", [], |row| {
                row.get(0)
            })
            .map_err(|err| corrupt("counting the reachability set", &err))?;
        set.filter = set.walked && set.matched < set.retained;
        set.breaks = if set.walked {
            conn.query_row(CHAIN_BREAKS, [], |row| row.get(0))
                .map_err(|err| corrupt("walking the parent chain", &err))?
        } else {
            0
        };
        Ok(set)
    }

    /// Whether the aggregates have to join the set at all.
    fn filters(&self) -> bool {
        self.filter
    }

    /// Why this store cannot tell a dead row from a live one, if it cannot.
    ///
    /// Two ways: the walk never ran, or it ran into a break in the chain below a commit the refs still
    /// reach. [`crate::prune`] refuses on either, because a row whose death cannot be proved is a row that
    /// must not be deleted.
    pub(crate) fn refusal(&self) -> Option<String> {
        if !self.walked {
            return Some(
                self.reason
                    .clone()
                    .unwrap_or_else(|| "the reachability walk did not run".to_owned()),
            );
        }
        (self.breaks > 0).then(|| {
            format!(
                "the reachable commits name {} parent commit(s) this store has no row for",
                self.breaks
            )
        })
    }

    /// Every commit row in the store, whether reached or not.
    pub(crate) fn retained(&self) -> i64 {
        self.retained
    }
}

impl Drop for LiveSet<'_> {
    fn drop(&mut self) {
        // Best effort: a leftover temp table is invisible to every other connection and disappears with
        // this one, so a failure here is worth no error variant.
        let _ = self
            .store
            .connection()
            .execute_batch("DROP TABLE IF EXISTS temp.fathom_live");
    }
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
    /// The checks a read verb can afford: the ones whose cost does not depend on how many rows the store
    /// holds.
    ///
    /// This is what `fathom stats`, `fathom show`, `fathom analyze`, the daemon's snapshot and the TUI's
    /// status line run, and it is a separate function because the alternative is what `reco-66q` measured:
    /// with the whole-database checks in the shared read path, `fathom show --json` on a 190 MiB store
    /// built from a real repository spent ~0.5 s in `PRAGMA quick_check` and ~0.35 s in
    /// `pragma_foreign_key_check` — before doing whatever it was asked to do — on *every invocation*,
    /// which is a third of `query.card.ms`' entire budget paid for a check the reader did not ask for.
    /// ADR-004 §9.6 puts those checks in `doctor`; the comment in `describe_index` has said since `e1.10`
    /// that "a whole-database check on every `fathom stats` is a check nobody will run twice". The code
    /// now matches the comment.
    ///
    /// What is left is what a read verb can honestly claim to have verified: that the file is a SQLite
    /// database with the page geometry it claims, that the schema this build's readers assume is in it,
    /// and that the version stamps agree with this build. A store that passes these and is still corrupt
    /// is a store `fathom doctor` exists for — and the report says which checks ran
    /// (`fathom-cli`'s `integrity_checks` field), so a shallow pass is never read as a deep one.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a check statement itself could not run.
    pub fn structural_check(&self) -> Result<Vec<String>, Error> {
        let conn = self.connection();
        let mut problems = Vec::new();
        // A zero here is not a slow answer, it is a file that is not a database: an empty file, a
        // truncated one, or a directory entry that is not SQLite at all. Everything below assumes both
        // numbers, so they are checked before they are used.
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .map_err(|err| corrupt("reading page_count", &err))?;
        let page_size: i64 = conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .map_err(|err| corrupt("reading page_size", &err))?;
        if pages == 0 || page_size == 0 {
            problems.push(
                "the file reports no pages, so it is not a usable SQLite database".to_owned(),
            );
            return Ok(problems);
        }
        // The tables the readers in this crate assume exist. This is not the whole schema — `meta`,
        // `counter` and the FTS side tables are read defensively and a missing one is a finding, not a
        // corrupt store — and it is deliberately a name list rather than a `PRAGMA table_info` per table:
        // the check is meant to cost nothing, and `sqlite_master` is a handful of rows whatever the store
        // holds.
        let mut statement = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .map_err(|err| corrupt("reading the schema", &err))?;
        let have: std::collections::BTreeSet<String> = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|err| corrupt("reading the schema", &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| corrupt("reading the schema", &err))?;
        for table in [
            "meta",
            "index_run",
            "analyzer",
            "git_commit",
            "git_commit_file",
            "ref_tip",
            "episode",
            "episode_member",
            "cache",
            "blob",
            "blob_ref",
        ] {
            if !have.contains(table) {
                problems.push(format!("the schema has no `{table}` table"));
            }
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

    /// The integrity checks `fathom doctor` runs: [`structural_check`] and `PRAGMA quick_check` always,
    /// `PRAGMA integrity_check` as well on `--deep` (ADR-004 §9.6).
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
        // Everything the cheap path already verified, and then the checks whose cost is the size of the
        // database. Nothing here is skipped on `--deep`, and nothing cheap is paid twice: the structural
        // checks are microseconds, so `doctor` reporting a clean store can say it ran them too.
        let mut problems = self.structural_check()?;
        problems.extend(self.check_statement("quick_check", "PRAGMA quick_check")?);
        if deep {
            problems.extend(self.check_statement("integrity_check", "PRAGMA integrity_check")?);
        }
        // ADR-004 §9.1's other check is the spine's own: foreign keys.
        let conn = self.connection();
        let broken: i64 = conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .map_err(|err| corrupt("checking foreign keys", &err))?;
        if broken != 0 {
            problems.push(format!("{broken} foreign key violations"));
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
    /// A thin slice of [`Store::inventory`], which is where the one reachability walk happens.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if any aggregate read fails.
    pub fn scale(&self) -> Result<Scale, Error> {
        Ok(self.inventory()?.scale)
    }

    /// The commit rows no ref can reach any more.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the walk or an aggregate read fails.
    pub fn dead_rows(&self) -> Result<DeadRows, Error> {
        Ok(self.inventory()?.dead)
    }

    /// The scale of what the refs reach, and the rows they do not, from one walk of the stored parents.
    ///
    /// This is the read behind `fathom stats`' `scale` and `dead` lines and behind the `reco-218` finding
    /// `fathom doctor` prints for them, which is the point of putting them in one call: the reachable count
    /// and the dead count have to be two halves of one judgement, or a status report can print a repository
    /// whose halves do not add up.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the walk or an aggregate read fails.
    pub fn inventory(&self) -> Result<Inventory, Error> {
        let conn = self.connection();
        let live = LiveSet::build(self)?;
        let (commit_rows, file_rows) = if live.filters() {
            (
                "git_commit JOIN temp.fathom_live ON temp.fathom_live.oid = git_commit.oid",
                "git_commit_file JOIN temp.fathom_live ON temp.fathom_live.oid = git_commit_file.commit_oid",
            )
        } else {
            ("git_commit", "git_commit_file")
        };
        let mut scale = Scale::default();
        (scale.commits, scale.files) = conn
            .query_row(
                &format!(
                    "SELECT (SELECT count(*) FROM {commit_rows}), \
                            (SELECT count(*) FROM {file_rows})"
                ),
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
                &format!(
                    "SELECT count(DISTINCT author_name || ' <' || author_email || '>'), \
                            coalesce(sum(n_add), 0), \
                            coalesce(sum(n_del), 0), \
                            coalesce(sum(n_binary), 0), \
                            coalesce(sum(oversized), 0) \
                     FROM {commit_rows}"
                ),
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
                &format!("SELECT min(author_utc), max(author_utc) FROM {commit_rows}"),
                [],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .map_err(|err| corrupt("reading the date range", &err))?;
        scale.noise = self.grouped("noise_class", file_rows)?;
        scale.kinds = self.grouped("change_kind", file_rows)?;
        scale.top_authors = {
            let mut statement = conn
                .prepare(&format!(
                    "SELECT author_name || ' <' || author_email || '>', count(*) AS n \
                     FROM {commit_rows} GROUP BY author_name, author_email \
                     ORDER BY n DESC, author_name LIMIT 5"
                ))
                .map_err(|err| corrupt("grouping by author", &err))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|err| corrupt("grouping by author", &err))?;
            rows.collect::<Result<_, _>>()
                .map_err(|err| corrupt("grouping by author", &err))?
        };

        let mut dead = DeadRows {
            retained: live.retained,
            reachable: if live.filters() {
                scale.commits
            } else {
                live.retained
            },
            ..Default::default()
        };
        if !live.walked {
            // Nothing was judged, so nothing is dead: every row is on the unprovable side, and `reason` says
            // which kind of "could not tell" this is.
            dead.unjudged = live.retained;
            dead.reason = live.reason.clone();
            dead.tips_without_rows = conn
                .query_row(TIPS_WITHOUT_ROWS, [], |row| row.get(0))
                .unwrap_or(0);
            return Ok(Inventory { scale, dead });
        }
        dead.tips_without_rows = conn
            .query_row(TIPS_WITHOUT_ROWS, [], |row| row.get(0))
            .map_err(|err| corrupt("reading the ref tips", &err))?;
        let unreachable: i64 = conn
            .query_row(DEAD_COUNT, [], |row| row.get(0))
            .map_err(|err| corrupt("counting the unreachable rows", &err))?;
        if live.breaks > 0 {
            // The chain has a break below a commit the refs still reach, so the rows past it are only
            // unprovable. A shallow clone lands here by construction, and so does a store whose walk hit an
            // object the object database would not serve — both already said by their own findings.
            dead.unjudged = unreachable;
        } else {
            dead.dead = unreachable;
            dead.dead_files = conn
                .query_row(DEAD_FILES, [], |row| row.get(0))
                .map_err(|err| corrupt("counting the unreachable file rows", &err))?;
            let mut statement = conn
                .prepare(DEAD_SAMPLE)
                .map_err(|err| corrupt("reading the unreachable rows", &err))?;
            let rows = statement
                .query_map(
                    [i64::try_from(DeadRows::SAMPLE).unwrap_or(i64::MAX)],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|err| corrupt("reading the unreachable rows", &err))?;
            dead.sample = rows
                .collect::<Result<_, _>>()
                .map_err(|err| corrupt("reading the unreachable rows", &err))?;
        }
        Ok(Inventory { scale, dead })
    }

    /// One `GROUP BY` over `git_commit_file`, for the two histograms in [`Scale`].
    fn grouped(&self, column: &str, from: &str) -> Result<BTreeMap<String, i64>, Error> {
        // `column` is one of two literals below and `from` is one of two joins built a few lines up,
        // never caller input; the interpolation is for the query planner, not for a string the store's
        // caller controls.
        debug_assert!(matches!(column, "noise_class" | "change_kind"));
        let mut statement = self
            .connection()
            .prepare(&format!(
                "SELECT {column}, count(*) FROM {from} GROUP BY {column} ORDER BY {column}"
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

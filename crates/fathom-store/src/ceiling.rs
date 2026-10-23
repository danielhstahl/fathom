//! `ceiling.rs` — the durable cache budget, enforced (ADR-004 §8.6, `reco-93w`).
//!
//! §8.6 is four sentences in the storage ADR and one config key (`cache.budget.mib`), and until now it
//! was neither of those things in code: the `cache` table grew on every miss and shrank on nothing. The
//! ADR's own measurement is the argument for fixing that — the cache is **51 % of store bytes** in the
//! fixture (§13.4) — so `store.size.mib` was a ceiling on a store whose largest table had no ceiling.
//!
//! # The rule, and why it is LRU and not more invalidation
//!
//! Eviction is by `last_used_utc`, oldest first, with a **protection set**: a row used inside
//! `protect_recent_ms` is not evicted, because a session that is looking at a repository is about to
//! look at it again and re-deriving its analysis costs the whole analysis. §8.6 phrases the protection
//! set as a queue-backlog rule (rows for episodes still in the backlog are protected before rows for
//! fully-reviewed ones); recency is the version of that rule that needs no review state to be present,
//! and `last_used_utc` is already maintained on every hit ([`crate::cache::Store::resolve`]'s touch)
//! for exactly this purpose.
//!
//! Three things this deliberately is not:
//!
//! * **Not on an event path.** [`Store::cache_enforce`] runs when a run closes, or when a person asks —
//!   never inside a lookup. A cache lookup that can delete is a lookup whose cost depends on what else
//!   is in the table, which is the shape ADR-004 §7.3's batching discipline exists to avoid.
//! * **Not a blob reclaim.** Eviction removes `cache` rows and the `blob_ref` rows that named them. The
//!   `blob` rows stay, because §8.7's class-M guard — refuse the whole prune if a digest a ledger
//!   references would go unreachable — is a retention decision with a review consequence, and it is
//!   `e6.x`'s. What this module does is say how many blobs are now unreachable (`dangling_blobs`) so
//!   the retention pass has a number to start from rather than a whole-table scan to run.
//! * **Not silent.** Every eviction is counted, in total and per analyzer, into the durable `counter`
//!   table (`cache.evict`, `cache.evict.bytes`, `cache.evict.<analyzer>`), which is what `fathom stats`
//!   prints. "Bounded" and "observable" are one requirement: an eviction nobody can see is a cache that
//!   is quietly losing the answers somebody still wants.
//!
//! # The dry run
//!
//! [`CacheCeiling::dry_run`] reports the same numbers and deletes nothing, for the same reason
//! [`crate::prune`]'s does: the first question anybody asks about a deletion policy is what it would
//! have done, and the answer has to come from the same statements the real run uses.

use std::collections::BTreeMap;
use std::fmt;

use rusqlite::params;

use crate::error::Error;
use crate::spine::Store;

/// Migration 9: an index for each of the reachability probe's three questions (`reco-93w`).
///
/// [`Store::dangling_blob_count`] ends §8.6's eviction pass — *how many blobs did the ceiling leave
/// unreachable* — and the question is three correlated `NOT EXISTS`: does any `cache` row, `blob_ref`
/// row or `graph_chunk` row still name this blob? Two of the three had no index to answer them, so the
/// plan SQLite chose was a table scan per blob: on the 100 000-commit fixture (200 000 blobs, 200 000
/// cache rows) the probe had not finished after twenty minutes. That is the same mistake as a cache
/// cap nobody can observe, wearing a different hat — a budget whose enforcement is quadratic is a
/// budget that cannot be enforced at the size it exists for.
///
/// `graph_chunk` needed nothing: it is `WITHOUT ROWID` on `sha256`, so the probe already searched it.
/// And the eviction's own `DELETE FROM blob_ref WHERE owner_kind = 'cache' AND owner_id IN (…)` is
/// already PK-leading on `blob_ref(owner_kind, owner_id, blob_sha)`, which is why only two indexes
/// appear here rather than three.
pub const MIGRATION_9: &str = "
CREATE INDEX IF NOT EXISTS cache_result ON cache(result_sha);
CREATE INDEX IF NOT EXISTS blob_ref_blob ON blob_ref(blob_sha);
";

fn corrupt(what: &str, err: &rusqlite::Error) -> Error {
    Error::StoreCorrupt {
        what: format!("{what}: {err}"),
    }
}

/// A byte count as the rounded-terse GiB a status line wants.
fn gib(bytes: i64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// How many rows one `DELETE … IN (…)` statement names.
///
/// Five hundred, which is `prune.batch_rows`' order of magnitude and small enough that the statement's
/// placeholder list is not itself a memory problem. A budget that needs 40 000 rows gone deletes in
/// batches rather than building one enormous `IN` list — or, worse, doing it one row at a time.
const DELETE_BATCH: usize = 500;

/// The budget, as resolved by the CLI from `cache.budget.mib` and `protect_recent_minutes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheCeiling {
    /// The ceiling on the sum of `cache.result_bytes`, in bytes.
    pub budget_bytes: u64,
    /// Rows used within this many milliseconds of `now` are not candidates. Zero disables the
    /// protection set entirely — nothing is spared, including rows written by the run that is doing the
    /// evicting — which is what a CI job that wants the ceiling honoured exactly asked for.
    pub protect_recent_ms: i64,
    /// Report and change nothing.
    pub dry_run: bool,
}

impl Default for CacheCeiling {
    fn default() -> Self {
        CacheCeiling {
            budget_bytes: 512 * 1024 * 1024,
            protect_recent_ms: 24 * 60 * 60 * 1000,
            dry_run: false,
        }
    }
}

/// What one enforcement pass found and did.
///
/// Every field is either a size the ceiling is being argued about or a count of what the ceiling cost.
/// A caller that ignores the returned value still gets the counters, because the same counts go to the
/// `counter` table when the run closes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Eviction {
    /// The budget in force.
    pub budget_bytes: u64,
    /// Rows in `cache` before the pass.
    pub rows_before: i64,
    /// Payload bytes in `cache` before the pass.
    pub bytes_before: i64,
    /// Rows in `cache` after the pass.
    pub rows_after: i64,
    /// Payload bytes after the pass — the number the ceiling is the answer to.
    pub bytes_after: i64,
    /// Rows removed (or, under `dry_run`, rows that would be).
    pub rows_evicted: i64,
    /// Payload bytes freed.
    pub bytes_evicted: i64,
    /// Rows removed, counted by analyzer — the "what got evicted" half of the observability claim.
    pub by_analyzer: BTreeMap<String, i64>,
    /// Rows the protection set kept despite being cold enough.
    pub rows_protected: i64,
    /// Payload bytes the protection set kept.
    pub bytes_protected: i64,
    /// Blobs that no `cache` row or `blob_ref` row names any more. Not deleted here: see this module's
    /// note about §8.7's class-M guard. Under a dry run this is `0`: predicting it would mean re-running
    /// the reachability question with the victim set excluded, which is a second, much larger query for a
    /// number the budget does not depend on — retention, not the ceiling, is what needs it.
    pub dangling_blobs: i64,
    /// Whether the cache was over its budget when the pass started. Distinct from `rows_evicted > 0`
    /// because the protection set can make a pass that was over budget do nothing at all, and a report
    /// that called that "within budget" would be the lie this module exists to prevent.
    pub was_over_budget: bool,
    /// Whether anything was actually deleted.
    pub dry_run: bool,
}

impl Eviction {
    /// Whether the cache was over its budget when the pass started — which is not the same question as
    /// whether anything was evicted.
    #[must_use]
    pub const fn over_budget(&self) -> bool {
        self.was_over_budget
    }

    /// One terminal line: the ceiling, what was over it, and what was thrown away to fix that.
    #[must_use]
    pub fn sentence(&self) -> String {
        if !self.was_over_budget {
            return format!(
                "cache is {} of a {} budget — nothing to evict",
                gib(self.bytes_before),
                gib(i64::try_from(self.budget_bytes).unwrap_or(i64::MAX))
            );
        }
        if self.rows_evicted == 0 {
            return format!(
                "cache was {} against a {} budget and nothing was evicted: all {} row(s) over the line \
                 were used inside the protection window{}",
                gib(self.bytes_before),
                gib(i64::try_from(self.budget_bytes).unwrap_or(i64::MAX)),
                self.rows_protected,
                if self.dry_run { " (dry run)" } else { "" }
            );
        }
        format!(
            "cache was {}, over its {} budget: evicted {} row(s) ({}), now {}; {} row(s) kept by the \
             protection set{}{}; {} blob(s) left unreachable for retention to reclaim",
            gib(self.bytes_before),
            gib(i64::try_from(self.budget_bytes).unwrap_or(i64::MAX)),
            self.rows_evicted,
            gib(self.bytes_evicted),
            gib(self.bytes_after),
            self.rows_protected,
            if self.dry_run { " (dry run)" } else { "" },
            if self.by_analyzer.is_empty() {
                String::new()
            } else {
                let parts = self
                    .by_analyzer
                    .iter()
                    .map(|(analyzer, rows)| format!("{analyzer} {rows}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(" — by analyzer: {parts}")
            },
            self.dangling_blobs,
        )
    }
}

impl fmt::Display for Eviction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sentence())
    }
}

impl Store {
    /// The cache's current footprint: row count and payload bytes, across every epoch.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn cache_footprint(&self) -> Result<(i64, i64), Error> {
        self.connection()
            .query_row(
                "SELECT count(*), coalesce(sum(result_bytes), 0) FROM cache",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|err| corrupt("measuring the cache", &err))
    }

    /// Cut the durable cache back to `ceiling`, by least-recently-used with a protection set (§8.6).
    ///
    /// Returns what it found and did, and leaves the same counts in the pending counter set for
    /// [`Store::flush_counters`] to write: `cache.evict`, `cache.evict.bytes`,
    /// `cache.evict.<analyzer>` and `cache.evict.blobs_dangling`. Under [`CacheCeiling::dry_run`] the
    /// counters are left alone, because a dry run is a question and not an act.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a read or delete fails; the deletes are in one transaction, so a
    /// failure leaves the table exactly as it was.
    pub fn cache_enforce(&mut self, ceiling: &CacheCeiling, now: i64) -> Result<Eviction, Error> {
        let (rows_before, bytes_before) = self.cache_footprint()?;
        let mut report = Eviction {
            budget_bytes: ceiling.budget_bytes,
            rows_before,
            bytes_before,
            rows_after: rows_before,
            bytes_after: bytes_before,
            was_over_budget: bytes_before > i64::try_from(ceiling.budget_bytes).unwrap_or(i64::MAX),
            dry_run: ceiling.dry_run,
            ..Eviction::default()
        };
        let budget = i64::try_from(ceiling.budget_bytes).unwrap_or(i64::MAX);
        if bytes_before <= budget {
            return Ok(report);
        }
        let excess = bytes_before - budget;

        // The protection set first, so the report can say what the ceiling refused to touch.
        //
        // A zero window means *disabled*, which is not the same thing as "protect rows used in the last
        // zero milliseconds": with `now - 0` as the floor, a row stamped at or after `now` — which is
        // every row a run has just written — would read as protected, and the ceiling would quietly do
        // nothing on the cold-index case it most needs to bind. `i64::MAX` protects nothing and leaves
        // every row a candidate.
        let protect_floor = if ceiling.protect_recent_ms <= 0 {
            i64::MAX
        } else {
            now.saturating_sub(ceiling.protect_recent_ms)
        };
        (report.rows_protected, report.bytes_protected) = self
            .connection()
            .query_row(
                "SELECT count(*), coalesce(sum(result_bytes), 0) FROM cache WHERE last_used_utc > ?1",
                params![protect_floor],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|err| corrupt("counting protected cache rows", &err))?;

        // The candidates, oldest first, with the running total computed by SQLite rather than by an
        // Rust loop: the walk has to stop at the first row that takes the freed total past `excess`, and
        // a window function does that in one pass over the `cache_lru` index instead of reading the
        // whole table back into this process — which would be a memory ceiling defeated by the thing
        // meant to enforce one.
        let mut statement = self
            .connection()
            .prepare(
                "SELECT cache_key, analyzer_id, result_bytes,
                        sum(result_bytes) OVER (ORDER BY last_used_utc, cache_key) AS freed
                 FROM cache
                 WHERE last_used_utc <= ?1
                 ORDER BY last_used_utc, cache_key",
            )
            .map_err(|err| corrupt("choosing cache rows to evict", &err))?;
        let victims: Vec<(String, String, i64)> = statement
            .query_map(params![protect_floor], |row| {
                let key: String = row.get(0)?;
                let analyzer: String = row.get(1)?;
                let bytes: i64 = row.get(2)?;
                let freed: i64 = row.get(3)?;
                Ok((key, analyzer, bytes, freed))
            })
            .map_err(|err| corrupt("choosing cache rows to evict", &err))?
            // `freed` is inclusive of the current row, so a row is a victim while the *previous* rows
            // have not yet covered the excess. That keeps the set minimal: the last row in is the one
            // that crosses the line, and nothing after it is touched.
            .filter_map(|row| match row {
                Ok((key, analyzer, bytes, freed)) if freed - bytes < excess => {
                    Some((key, analyzer, bytes))
                }
                _ => None,
            })
            .collect();
        drop(statement);

        for key in &victims {
            report.rows_evicted += 1;
            report.bytes_evicted += key.2;
            *report.by_analyzer.entry(key.1.clone()).or_insert(0) += 1;
        }
        report.rows_after = rows_before - report.rows_evicted;
        report.bytes_after = bytes_before - report.bytes_evicted;
        if ceiling.dry_run {
            return Ok(report);
        }
        if victims.is_empty() {
            // Everything over the budget is protected. That is a real outcome — a huge warm cache asked
            // to stay warm — and it is reported rather than hidden, because a ceiling that silently did
            // nothing is the failure mode this whole module exists to prevent.
            return Ok(report);
        }

        let keys: Vec<String> = victims.iter().map(|(key, _, _)| key.clone()).collect();
        let tx = self.immediate_transaction()?;
        for chunk in keys.chunks(DELETE_BATCH) {
            let marks = vec!["?"; chunk.len()].join(",");
            let refs: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|key| key as &dyn rusqlite::types::ToSql)
                .collect();
            // Two statements, one list of keys: the rows and the ownership edges that named them. The
            // refs go with the rows that owned them — a `blob_ref` whose owner is gone is a row no join
            // can reach, and the dangling count below is about blobs, not about this noise.
            for statement in [
                format!("DELETE FROM cache WHERE cache_key IN ({marks})"),
                format!(
                    "DELETE FROM blob_ref WHERE owner_kind = 'cache' AND owner_id IN ({marks})"
                ),
            ] {
                let mut delete = tx
                    .prepare(&statement)
                    .map_err(|err| corrupt("evicting cache rows", &err))?;
                delete
                    .execute(rusqlite::params_from_iter(refs.iter().copied()))
                    .map_err(|err| corrupt("evicting cache rows", &err))?;
            }
        }
        tx.commit()
            .map_err(|err| corrupt("evicting cache rows", &err))?;
        report.dangling_blobs = self.dangling_blob_count()?;

        self.bump_counter("cache.evict", report.rows_evicted);
        self.bump_counter("cache.evict.bytes", report.bytes_evicted);
        for (analyzer, rows) in &report.by_analyzer {
            self.bump_counter(&format!("cache.evict.{analyzer}"), *rows);
        }
        if report.dangling_blobs > 0 {
            self.bump_counter("cache.evict.blobs_dangling", report.dangling_blobs);
        }
        Ok(report)
    }

    /// Blobs nothing can name any more.
    ///
    /// A blob is dangling when no `cache.result_sha`, no `blob_ref` and no `graph_chunk` reaches it. It
    /// is *not* deleted here: `graph_chunk` is checked precisely because a graph anchor's manifest chunk
    /// is a payload like any other, and §8.7's class-M guard belongs to whoever deletes the row.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn dangling_blob_count(&self) -> Result<i64, Error> {
        self.connection()
            .query_row(
                "SELECT count(*) FROM blob b
                 WHERE NOT EXISTS (SELECT 1 FROM cache c WHERE c.result_sha = b.sha256)
                   AND NOT EXISTS (SELECT 1 FROM blob_ref r WHERE r.blob_sha = b.sha256)
                   AND NOT EXISTS (SELECT 1 FROM graph_chunk g WHERE g.sha256 = b.sha256)",
                [],
                |row| row.get(0),
            )
            .map_err(|err| corrupt("counting unreachable blobs", &err))
    }

    /// Every blob row in the table — the denominator §8.7's prune reports against, and the number an
    /// eviction pass moves from without touching.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn blob_count(&self) -> Result<i64, Error> {
        self.connection()
            .query_row("SELECT count(*) FROM blob", [], |row| row.get(0))
            .map_err(|err| corrupt("counting blobs", &err))
    }
}

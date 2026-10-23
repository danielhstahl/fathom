//! `scorecard.rs` — migration 11: the scorecard rows a trend is made of (`e3.9`, `reco-al1`).
//!
//! ADR-004 Appendix A already had this table, and §10 already put it in class **R**: derived, droppable,
//! re-derivable from the graph rows and the committed rules. The DDL here is that table verbatim —
//! `tree_oid` primary key, `dims` JSON, the `since` companion, `built_utc` — with one addition and no
//! changes, and the addition is a claim:
//!
//! * **`scorecard_built`, an index on `built_utc`.** ADR-004's one-key-per-revision shape answers "what did
//!   revision R score", and the trend asks the other question — "the last N revisions, oldest-last" —
//!   which without an index is a sort of the whole table on every read of a 500-revision history. §8.4's
//!   byte accounting treats the index as part of the table's bytes, which is what makes its cost visible
//!   rather than free.
//!
//! # Why `dims` holds a document and not columns
//!
//! The ADR fixed the columns and left their contents as JSON, and this is the reason that is the right
//! choice rather than a convenient one: five dimensions and their arithmetic are a *document* — a term
//! without its denominator, its weight, its completeness flag and its reason is not re-checkable, and
//! §7.9 of ADR-001 forbids a number a reader cannot account for. Spreading it over twenty-two columns
//! would have made every future dimension a migration of a table whose DDL the ADR has frozen.
//!
//! The payload is **compact**: numbers, not prose. A trend that stored the full report for every revision
//! would store the same five sentences five hundred times, and ADR-004 §8.2's blob dedupe cannot help with
//! prose that differs by one count. [`fathom_analyze::scorecard::Scorecard::to_stored`] is the writer and
//! `from_stored` the reader, and the round trip is tested.
//!
//! # Who writes
//!
//! Nothing in this crate. The writer is the scorecard's lane seam (`fathom-analyze`'s
//! [`scorecard::lane`](../../fathom_analyze/scorecard/lane.rs), reached from `fathom architecture`), and
//! [`Store::scorecard_write`] is `INSERT … ON CONFLICT DO NOTHING` for the same reason
//! [`crate::graph::graph_write_snapshot`] is: a scorecard is a function of a revision, so a second write
//! for a tree that already has one is a no-op and not an update. A row that disagrees with its own
//! revision is a bug in the analyzer, and the way to fix it is a re-index, not a `REPLACE`.

use rusqlite::{OptionalExtension as _, params};

use crate::error::Error;
use crate::spine::Store;
use crate::spine::corrupt;

/// Migration 11: ADR-004 Appendix A's `scorecard`, plus the index the trend read needs.
pub const MIGRATION_11: &str = "
CREATE TABLE IF NOT EXISTS scorecard (
  tree_oid TEXT PRIMARY KEY,
  dims     TEXT NOT NULL,                  -- JSON: boundaries, coupling, acyclicity, api, hotspots
  since    TEXT,                           -- 'since my last review' companion (e3.9)
  built_utc INTEGER NOT NULL
) STRICT;
-- The ADR's key answers \"what did R score\"; the trend asks \"the last N, oldest last\", and that
-- ordering is what this index is for.
CREATE INDEX IF NOT EXISTS scorecard_built ON scorecard(built_utc);
";

/// Every table this module owns, for `fathom store usage` and the byte accounting.
pub const SCORECARD_TABLES: [&str; 1] = ["scorecard"];

/// One scorecard row: what revision it describes, what it scored, and when it was computed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScorecardRow {
    /// The revision, as `graph_snapshot.tree_oid` spells it.
    pub tree_oid: String,
    /// The scorecard payload: [`fathom_analyze::scorecard::Scorecard::to_stored`]'s output.
    pub dims: String,
    /// The "since my last review" companion, as a JSON document, or `None` when there was no anchor to
    /// compute one against. Stored rather than recomputed because the anchor is a fact about *then*: the
    /// review that was current when this revision was indexed may not be the one that is current now.
    pub since: Option<String>,
    /// The instant it was written, microseconds. The trend's ordering key.
    pub built_utc: i64,
}

impl ScorecardRow {
    /// Roughly the bytes this row is responsible for, index included — the figure §8.4 accounting wants.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.tree_oid.len() + self.dims.len() + self.since.as_deref().map_or(0, str::len) + 8
    }
}

impl Store {
    /// Write one scorecard row, keeping the first one for a tree.
    ///
    /// `true` when the row was written, `false` when the tree already had one. Never `REPLACE`: a scorecard
    /// is a function of (revision, rules, analyzer version), so a second write that disagrees is a stale
    /// read or a bug, and overwriting a good row with a worse one is the failure mode worth refusing.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn scorecard_write(&mut self, row: &ScorecardRow) -> Result<bool, Error> {
        if row.dims.is_empty() || serde_json::from_str::<serde_json::Value>(&row.dims).is_err() {
            // A `dims` that is not JSON would be read back as an unreadable row forever, and the store has
            // no way to tell a bad writer from a bad disk. Refuse the write where it happened.
            return Err(corrupt(
                format!(
                    "refusing a scorecard row for {} whose dims is not JSON",
                    row.tree_oid
                ),
                &rusqlite::Error::InvalidParameterName(String::new()),
            ));
        }
        let written = self
            .connection()
            .execute(
                "INSERT INTO scorecard(tree_oid, dims, since, built_utc)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(tree_oid) DO NOTHING",
                params![row.tree_oid, row.dims, row.since, row.built_utc],
            )
            .map_err(|err| corrupt("writing a scorecard row", &err))?;
        Ok(written > 0)
    }

    /// Fill in a stored row's `since` companion, once the anchor is known.
    ///
    /// The one `UPDATE` this table ever sees, and the only way it is reached: a row's five dimensions are a
    /// function of its revision, but "how many revisions since the reviewer's last review" is a function of
    /// the review too, and a scoring run learns the anchor only after it has written the row and read the
    /// trend back. Writing it here rather than leaving it for a later run is what makes the durable fact
    /// true of *this* scoring: the same review resolves to the same anchor, so the write is idempotent.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn scorecard_set_since(&mut self, tree_oid: &str, since: &str) -> Result<bool, Error> {
        if serde_json::from_str::<serde_json::Value>(since).is_err() {
            return Err(corrupt(
                format!("refusing a scorecard `since` for {tree_oid} that is not JSON"),
                &rusqlite::Error::InvalidParameterName(String::new()),
            ));
        }
        let changed = self
            .connection()
            .execute(
                "UPDATE scorecard SET since = ?2 WHERE tree_oid = ?1",
                params![tree_oid, since],
            )
            .map_err(|err| corrupt("writing a scorecard's since companion", &err))?;
        Ok(changed > 0)
    }

    /// Read one revision's scorecard row.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn scorecard_get(&self, tree_oid: &str) -> Result<Option<ScorecardRow>, Error> {
        self.connection()
            .query_row(
                "SELECT tree_oid, dims, since, built_utc FROM scorecard WHERE tree_oid = ?1",
                params![tree_oid],
                |row| {
                    Ok(ScorecardRow {
                        tree_oid: row.get(0)?,
                        dims: row.get(1)?,
                        since: row.get(2)?,
                        built_utc: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|err| corrupt("reading a scorecard row", &err))
    }

    /// The newest rows first — the order a trend folds, and the order a report prints a series oldest-last.
    ///
    /// `limit` of 0 or less means every row, which the CLI uses for a full-history trend: the reader is
    /// streaming and the fold is bounded, so there is no reason to make a 500-revision history pay a
    /// second query.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn scorecard_rows(&self, limit: i64) -> Result<Vec<ScorecardRow>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT tree_oid, dims, since, built_utc FROM scorecard
                 ORDER BY built_utc DESC, tree_oid ASC
                 LIMIT CASE WHEN ?1 > 0 THEN ?1 ELSE -1 END",
            )
            .map_err(|err| corrupt("listing scorecard rows", &err))?;
        let rows = statement
            .query_map(params![limit], |row| {
                Ok(ScorecardRow {
                    tree_oid: row.get(0)?,
                    dims: row.get(1)?,
                    since: row.get(2)?,
                    built_utc: row.get(3)?,
                })
            })
            .map_err(|err| corrupt("listing scorecard rows", &err))?
            .collect::<Result<Vec<ScorecardRow>, _>>()
            .map_err(|err| corrupt("listing scorecard rows", &err))?;
        Ok(rows)
    }

    /// The newest `limit` rows *strictly older than* `built_utc` — the second page of a trend, and the way
    /// a caller asks "what happened after the row I already have" without a cursor of its own.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn scorecard_rows_before(
        &self,
        built_utc: i64,
        limit: i64,
    ) -> Result<Vec<ScorecardRow>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT tree_oid, dims, since, built_utc FROM scorecard
                 WHERE built_utc < ?1
                 ORDER BY built_utc DESC, tree_oid ASC
                 LIMIT CASE WHEN ?2 > 0 THEN ?2 ELSE -1 END",
            )
            .map_err(|err| corrupt("paging scorecard rows", &err))?;
        let rows = statement
            .query_map(params![built_utc, limit], |row| {
                Ok(ScorecardRow {
                    tree_oid: row.get(0)?,
                    dims: row.get(1)?,
                    since: row.get(2)?,
                    built_utc: row.get(3)?,
                })
            })
            .map_err(|err| corrupt("paging scorecard rows", &err))?
            .collect::<Result<Vec<ScorecardRow>, _>>()
            .map_err(|err| corrupt("paging scorecard rows", &err))?;
        Ok(rows)
    }

    /// Whether a revision already has a scorecard row — the cheap test a backfill uses to skip work.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn scorecard_has(&self, tree_oid: &str) -> Result<bool, Error> {
        let count: i64 = self
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM scorecard WHERE tree_oid = ?1",
                params![tree_oid],
                |row| row.get(0),
            )
            .map_err(|err| corrupt("counting a scorecard row", &err))?;
        Ok(count > 0)
    }

    /// How many scorecard rows the store holds: the denominator of "12 of 480 revisions scored".
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn scorecard_count(&self) -> Result<i64, Error> {
        self.connection()
            .query_row("SELECT COUNT(*) FROM scorecard", [], |row| row.get(0))
            .map_err(|err| corrupt("counting scorecard rows", &err))
    }

    /// The bytes the scorecard rows account for, payload and keys — §8.4's labelled basis.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn scorecard_bytes(&self) -> Result<i64, Error> {
        self.connection()
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(tree_oid) + LENGTH(dims)
                                    + COALESCE(LENGTH(since), 0) + 8), 0)
                 FROM scorecard",
                [],
                |row| row.get(0),
            )
            .map_err(|err| corrupt("measuring the scorecard table", &err))
    }

    /// The newest row's instant — the stamp a caller compares against its own "is this stale" answer.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn scorecard_newest_utc(&self) -> Result<Option<i64>, Error> {
        self.connection()
            .query_row("SELECT MAX(built_utc) FROM scorecard", [], |row| {
                row.get::<_, Option<i64>>(0)
            })
            .map_err(|err| corrupt("reading the newest scorecard row", &err))
    }

    /// Drop every scorecard row. The rebuild path (`fathom architecture --refresh`, and a store rebuild)
    /// uses it because a scorecard is class R: the honest recovery from a row that disagrees with its
    /// revision is to re-derive it, not to patch it.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the delete fails.
    pub fn scorecard_clear(&mut self) -> Result<usize, Error> {
        self.connection()
            .execute("DELETE FROM scorecard", [])
            .map_err(|err| corrupt("clearing the scorecard table", &err))
    }
}

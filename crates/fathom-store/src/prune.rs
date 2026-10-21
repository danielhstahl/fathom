//! `prune.rs` — the one place the spine deletes rows.
//!
//! ADR-004 §8.1 makes this the only writer with a `DELETE` in it, and says why: invalidation is a key
//! mismatch and never a delete (§6.1 R1), because a mismatch is a miss you cannot forget while a delete is
//! a scan you must remember. So the pruner is not on any event path; it runs when a person asks
//! (`fathom index --prune`) and, when the daemon's idle pass lands, when the repository is otherwise
//! asleep. `fathom store prune` — the verb ADR-004 §14 lists as deferred — is the same storage semantics
//! behind a retention window; what lives here is the half `reco-218` needs: the commit rows a rewrite left
//! behind, counted by [`crate::inventory`] and removed here.
//!
//! # What it removes, and what it refuses
//!
//! It removes commit rows that **provably** describe commits no ref reaches: rows a rebase, an `--amend` or
//! a force-push replaced, and which `git gc --prune=now` has usually already turned into nothing. Their file
//! rows go with them by the schema's own cascade, and so do the episode rows that lose their last member.
//!
//! It refuses in two directions, and a refusal is a result rather than a shrug — the report says which
//! happened and why:
//!
//! * **Everything**, when the store cannot tell dead from live. A walk that could not run, or one that ran
//!   into a parent this store has no row for (a shallow clone's graft, an object the object database would
//!   not serve), means the rows below that break are unprovable rather than dead — and an unprovable row is
//!   not this verb's to delete.
//! * **One commit at a time**, when a review points at it: ADR-004 §8.2's rule that a prune may never lose a
//!   review, and §8.7's argument that the guard is allowed to be loose because a false positive costs bytes
//!   and a false negative costs a sign-off. A dead commit whose episode carries a decision, or a mirrored
//!   ledger entry, stays exactly where it is — and the episode relink ladder (ADR-003 §6.2) keeps its rungs,
//!   because a decision carried forward from a rewritten episode still has the episode it was written
//!   against.
//!
//! `--dry-run` reports the same numbers and writes nothing, which is §8.7's answer to "what would it have
//! done": the counts come from the same statements either way, so a dry run cannot drift from the real one.

use std::fmt;

use rusqlite::Connection;

use crate::error::Error;
use crate::inventory::{DeadRows, LiveSet, UNREACHABLE_OIDS};
use crate::spine::{RunStats, Store, Trigger};

fn corrupt(what: impl fmt::Display, err: &rusqlite::Error) -> Error {
    Error::StoreCorrupt {
        what: format!("{what}: {err}"),
    }
}

/// The work list: the commit rows to remove, in a temp table.
///
/// A table rather than a `Vec` of ids interpolated into `IN (…)` lists, because the set can be a whole
/// rewrite's worth of commits, several statements below join against it, and it has to survive from the
/// counting to the deleting phase. Dropped on the way out for the same reason the walk's set is.
struct Victims<'a> {
    store: &'a Store,
}

impl<'a> Victims<'a> {
    /// The unreachable rows, minus the ones the review guard keeps.
    fn fill(store: &'a Store) -> Result<Self, Error> {
        let conn = store.connection();
        conn.execute_batch(
            "DROP TABLE IF EXISTS temp.fathom_victim; \
             CREATE TABLE temp.fathom_victim (oid TEXT PRIMARY KEY) WITHOUT ROWID;",
        )
        .map_err(|err| corrupt("opening the prune's work list", &err))?;
        conn.execute(
            &format!("INSERT OR IGNORE INTO temp.fathom_victim (oid) {UNREACHABLE_OIDS}"),
            [],
        )
        .map_err(|err| corrupt("filling the prune's work list", &err))?;
        // The guard, applied by taking rows *out* of the list rather than by filtering every later query:
        // one place for the rule, so the count and the delete cannot disagree about what was withheld.
        conn.execute(
            "DELETE FROM temp.fathom_victim WHERE oid IN ( \
                 SELECT m.commit_oid FROM episode_member m \
                 WHERE m.episode_id IN ( \
                     SELECT e.id FROM episode e \
                     WHERE EXISTS (SELECT 1 FROM review_state rs \
                                   WHERE rs.episode_id = e.id AND rs.state <> 'unread') \
                        OR EXISTS (SELECT 1 FROM ledger_entry le WHERE le.episode_id = e.id) \
                 ) \
             )",
            [],
        )
        .map_err(|err| corrupt("applying the prune's review guard", &err))?;
        Ok(Self { store })
    }

    fn count(&self) -> Result<i64, Error> {
        self.store
            .connection()
            .query_row("SELECT count(*) FROM temp.fathom_victim", [], |row| {
                row.get(0)
            })
            .map_err(|err| corrupt("counting the prune's work list", &err))
    }

    fn sample(&self, limit: i64) -> Result<Vec<String>, Error> {
        let mut statement = self
            .store
            .connection()
            .prepare(
                "SELECT v.oid FROM temp.fathom_victim v JOIN git_commit c ON c.oid = v.oid \
                 ORDER BY c.author_utc DESC, v.oid LIMIT ?1",
            )
            .map_err(|err| corrupt("reading the prune's work list", &err))?;
        let rows = statement
            .query_map([limit], |row| row.get::<_, String>(0))
            .map_err(|err| corrupt("reading the prune's work list", &err))?;
        rows.collect::<Result<_, _>>()
            .map_err(|err| corrupt("reading the prune's work list", &err))
    }
}

impl Drop for Victims<'_> {
    fn drop(&mut self) {
        let _ = self
            .store
            .connection()
            .execute_batch("DROP TABLE IF EXISTS temp.fathom_victim");
    }
}

/// The episodes whose every member is on the work list, and which carry no decision of their own.
///
/// One `WHERE` for both the count and the delete, which is the whole reason it is a constant rather than two
/// near-identical statements: a report that says "1 episode retired" and a delete that retires 2 is the kind
/// of drift a status command is not allowed to have. The guard is the one
/// [`crate::episode`]'s `retire_dead_episodes` already applies to retired episodes, spelled the same way.
const EPISODES_EMPTY_WHERE: &str = "WHERE EXISTS (SELECT 1 FROM episode_member m \
                                      WHERE m.episode_id = e.id) \
       AND NOT EXISTS (SELECT 1 FROM episode_member m \
                       WHERE m.episode_id = e.id \
                         AND m.commit_oid NOT IN (SELECT oid FROM temp.fathom_victim)) \
       AND e.id NOT IN (SELECT episode_id FROM review_state) \
       AND e.id NOT IN (SELECT new_id FROM episode_rewrite)";

fn episodes_empty(what: &str) -> String {
    format!("SELECT {what} FROM episode e {EPISODES_EMPTY_WHERE}")
}

/// The file rows the work list carries — the cascade, counted before it happens.
const VICTIM_FILES: &str = "SELECT count(*) FROM git_commit_file \
     WHERE commit_oid IN (SELECT oid FROM temp.fathom_victim)";

/// What a prune took, and what it refused to take. Data only: the sentences belong to the verb.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// Whether this was a `--dry-run`: every number below is then what a real prune *would* do, from the
    /// same statements, not a different estimate.
    pub dry_run: bool,
    /// `git_commit` rows removed.
    pub commits_removed: i64,
    /// `git_commit_file` rows that went with them.
    pub files_removed: i64,
    /// Episode rows that lost their last member and carried no decision.
    pub episodes_removed: i64,
    /// Dead commits left in place because a review or a mirrored sign-off points at them (§8.2).
    pub withheld_reviewed: i64,
    /// Every row in the store, when the store could not tell dead from live. Paired with
    /// [`PruneReport::refusal`].
    pub withheld_undecidable: i64,
    /// Why nothing could be decided, when nothing could.
    pub refusal: Option<String>,
    /// Up to [`DeadRows::SAMPLE`] of the removed oids, newest first — enough for a report line to name what
    /// it means.
    pub sample: Vec<String>,
}

impl PruneReport {
    /// Did anything go — or, on a dry run, would anything?
    #[must_use]
    pub fn removed_anything(&self) -> bool {
        self.commits_removed > 0 || self.episodes_removed > 0
    }

    /// The prune did all it could and the store was already clean.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.removed_anything() && self.refusal.is_none()
    }
}

impl Store {
    /// Drop the commit rows no ref reaches, and everything that was only theirs.
    ///
    /// `now` stamps the `index_run` row this writes — `kind = 'prune'`, already in the column's vocabulary —
    /// because a prune is a run like any other and the run log is the evidence trail ADR-004 §11 reads. A
    /// dry run opens no run: it wrote nothing, and a log of no-ops is how a log stops being evidence.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a statement fails. A refusal is not an error: it is
    /// [`PruneReport::refusal`].
    pub fn prune_dead_commits(&mut self, dry_run: bool, now: i64) -> Result<PruneReport, Error> {
        if dry_run {
            return self.dead_prune(true);
        }
        // Opened before the work, so a prune killed partway is a run the next opener stamps `crashed` and
        // `fathom doctor` reports until something finishes (§9.2): a half-pruned store is a stated partial,
        // never a silent one.
        let run = self.begin_run("prune", Trigger::Manual, now)?;
        let report = self.dead_prune(false);
        let rows = report.as_ref().map_or(0, |report| {
            report.commits_removed + report.files_removed + report.episodes_removed
        });
        self.finish_run(
            run,
            now,
            &RunStats {
                commits_seen: report.as_ref().map_or(0, |report| {
                    report.commits_removed + report.withheld_reviewed + report.withheld_undecidable
                }),
                // A prune adds nothing, and writing `0` is what separates a run log that counts rows from
                // one that counts changes.
                commits_added: 0,
                rows_written: rows,
                interrupted: false,
                error: report.as_ref().err().map(|err| err.to_string()),
            },
        )?;
        report
    }

    /// Count the unreachable rows, and remove them unless `dry_run`.
    ///
    /// Takes `&self` because every statement here goes through the one connection the `Store` owns; the run
    /// bookkeeping around it is what needs `&mut` (and is [`Store::prune_dead_commits`]).
    fn dead_prune(&self, dry_run: bool) -> Result<PruneReport, Error> {
        // The same walk `fathom stats` reads, so this can never remove a row the status report did not
        // count as dead.
        let live = LiveSet::build(self)?;
        let mut report = PruneReport {
            dry_run,
            ..Default::default()
        };
        if let Some(why) = live.refusal() {
            report.refusal = Some(why);
            report.withheld_undecidable = live.retained();
            return Ok(report);
        }
        let victims = Victims::fill(self)?;
        let conn: &Connection = self.connection();
        let candidates: i64 = conn
            .query_row(
                "SELECT count(*) FROM git_commit \
                 WHERE oid NOT IN (SELECT oid FROM temp.fathom_live)",
                [],
                |row| row.get(0),
            )
            .map_err(|err| corrupt("counting the unreachable rows", &err))?;
        report.commits_removed = victims.count()?;
        report.withheld_reviewed = candidates - report.commits_removed;
        report.files_removed = conn
            .query_row(VICTIM_FILES, [], |row| row.get(0))
            .map_err(|err| corrupt("counting the file rows to remove", &err))?;
        report.episodes_removed = conn
            .query_row(&episodes_empty("count(*)"), [], |row| row.get(0))
            .map_err(|err| corrupt("counting the episodes to retire", &err))?;
        report.sample = victims.sample(i64::try_from(DeadRows::SAMPLE).unwrap_or(i64::MAX))?;
        if dry_run || report.commits_removed == 0 {
            return Ok(report);
        }
        // One transaction, because the three deletes are one act: an episode retired without its commits
        // going, or commits gone with the episodes left holding member rows that name nothing, is a store
        // whose report no longer follows from its rows.
        let tx = conn
            .unchecked_transaction()
            .map_err(|err| corrupt("opening the prune transaction", &err))?;
        tx.execute(
            &format!(
                "DELETE FROM episode WHERE id IN ({})",
                episodes_empty("e.id")
            ),
            [],
        )
        .map_err(|err| corrupt("retiring the empty episodes", &err))?;
        tx.execute(
            "DELETE FROM git_commit WHERE oid IN (SELECT oid FROM temp.fathom_victim)",
            [],
        )
        .map_err(|err| corrupt("deleting the unreachable commit rows", &err))?;
        // `episode_doc` is class R and has no foreign key to hang a cascade on; a document for a deleted
        // episode is a search hit on an episode that does not exist.
        tx.execute(
            "DELETE FROM episode_doc WHERE episode_id NOT IN (SELECT id FROM episode)",
            [],
        )
        .map_err(|err| corrupt("dropping the documents of retired episodes", &err))?;
        tx.commit()
            .map_err(|err| corrupt("committing the prune", &err))?;
        Ok(report)
    }
}

//! `ledger.rs` — migration 7: the mirror of the review ledger (`e4.6`, `reco-rlf`).
//!
//! ADR-004 §10 makes the sign-off class **M**: a human act that cannot be recomputed, and the only class
//! whose loss is an incident. The durable copy is a markdown file under `.fathom/reviews/` that is
//! *committed*; this table is the index over it. That direction is load-bearing and is the reason the only
//! write path here is "drop every row and re-read the files" ([`Store::mirror_ledger`]): a mirror that
//! merged, patched or reconciled would eventually disagree with the file it indexes, and then nobody could
//! say which one the reviewer signed. ADR-004 §9.1's recovery level 2 — "re-mirror: class-M files are the
//! truth, drop the mirrors and re-parse them, seconds" — is only legal while the mirror is disposable, and
//! disposable means *wholly rebuilt*.
//!
//! # The columns, and the ones this ticket added
//!
//! The DDL is ADR-004 Appendix A §4.7 reproduced with its column names, CHECK vocabularies and indexes
//! unchanged, in the same spirit as [`crate::episode`]: two copies of the shape would be the drift this
//! repository's whole tooling exists to prevent. The columns after the `-- e4.6 additions` marker are the
//! ones the ticket's own acceptance criteria need and the reference schema has no room for:
//!
//! * `fallback_fingerprint` / `shape_fingerprint` — the two weaker digests of ADR-003 §2.2, kept on the
//!   entry so a sign-off can be re-linked when the *id* moved (a reword, a squash that added files) rather
//!   than only when it survived;
//! * `content_fingerprint` — the digest of the *content* that was reviewed. ADR-003's digests are identity
//!   digests and deliberately ignore hunk text (§2.1), which is exactly why they cannot answer "did the
//!   content change after the sign-off?" — an amend that fixes a typo moves no identity digest at all. The
//!   ledger needs a second, orthogonal digest whose whole job is to move when the content does;
//! * `tampered` — see below;
//! * `note`, `linked_fingerprint`, `dead`, `n_members`, `n_files` — what a surface prints without re-parsing
//!   `scope`.
//!
//! # Why `tampered` is a flag and not a fourth `state`
//!
//! `state ∈ (fresh, stale, orphan)` is a closed vocabulary published in the ADR, and ADR-003 §6.4 exists
//! precisely because a second spelling of a closed vocabulary costs a migration of something that must
//! never need one. Tampering is also a different kind of claim: `state` answers *what is this sign-off
//! attached to now*, and `tampered` answers *can the row be read at all*. An entry whose body no longer
//! hashes to `content_sha` still has a fingerprint field to link, and the honest report is both sentences
//! at once — "stale, and its text was edited after it was written" — which a single enum cannot say. The
//! digest is a *detectability* claim, not a signature: it has no secret in it, so it catches a silent edit
//! (a rebase script that rewrites the file, a mis-merge, a hand fixing one field) and does not pretend to
//! stop a determined forger. git's own history is the signature; this is the tripwire in front of it.
//!
//! # No foreign key, and no epoch
//!
//! Like `triage_event` (migration 6), a ledger row refers to an episode by id and is *allowed* to dangle.
//! ADR-004 §8.7's rule that a prune may never lose a review, applied here, means a sign-off outlives the
//! episode row it was written against; `state='orphan'` is that fact recorded rather than a row silently
//! gone. There is no `epoch` column because a mirror is not derived data: it is rebuilt in full, and ADR-004
//! §6.3's mark-dead machinery has nothing to mark dead here.

use rusqlite::params;

use crate::error::Error;
use crate::spine::Store;
use crate::spine::corrupt;

/// Migration 7: the review-ledger mirror — ADR-004 Appendix A §4.7 with `e4.6`'s columns added.
pub const MIGRATION_7: &str = "
CREATE TABLE IF NOT EXISTS ledger_entry (
  id TEXT PRIMARY KEY,                     -- stable id inside the ledger file
  file TEXT NOT NULL,                      -- path relative to repo root
  anchor TEXT NOT NULL,                    -- line anchor for a jump target
  episode_id TEXT,                         -- resolved link, may be NULL (orphan)
  fingerprint TEXT NOT NULL,               -- what it was signed against
  decision TEXT NOT NULL CHECK (decision IN
       ('accepted','flagged','dismissed','snoozed')),
  reason_code TEXT,                        -- the closed dismiss vocabulary (ADR-001 §4.5)
  actor TEXT NOT NULL,
  actor_kind TEXT NOT NULL CHECK (actor_kind IN ('human','agent')),
  signed_utc  INTEGER NOT NULL,
  scope       TEXT NOT NULL,               -- JSON: which members/paths the sign-off covers
  content_sha TEXT NOT NULL,               -- sha256 of the entry body: tamper detection
  link_method TEXT NOT NULL CHECK (link_method IN ('exact','fingerprint','shape','member-set','manual','none')),
  link_score  REAL NOT NULL DEFAULT 1,
  state TEXT NOT NULL CHECK (state IN ('fresh','stale','orphan')),
  stale_reason TEXT,
  relinked_from TEXT,
  mirrored_utc INTEGER NOT NULL,
  -- e4.6 additions: the four digests and the two verdicts a re-link has to report.
  fallback_fingerprint TEXT NOT NULL DEFAULT '',  -- author+subject: what a squash rides on
  shape_fingerprint    TEXT NOT NULL DEFAULT '',  -- author+paths: what a reword rides on
  content_fingerprint  TEXT NOT NULL DEFAULT '',  -- the reviewed content: what staleness compares
  linked_fingerprint   TEXT NOT NULL DEFAULT '',  -- the live episode's digest, after the re-link
  note                 TEXT NOT NULL DEFAULT '',  -- the reviewer's own words, verbatim
  tampered             INTEGER NOT NULL DEFAULT 0, -- 1: the markdown body no longer hashes to content_sha
  dead                 INTEGER NOT NULL DEFAULT 0, -- 1: it links to an episode no current commit belongs to
  n_members            INTEGER NOT NULL DEFAULT 0, -- the extent `scope` summarises, typed
  n_files              INTEGER NOT NULL DEFAULT 0  -- see `n_members`
) STRICT;
CREATE INDEX IF NOT EXISTS ledger_fingerprint ON ledger_entry(fingerprint);
CREATE INDEX IF NOT EXISTS ledger_episode ON ledger_entry(episode_id);
CREATE INDEX IF NOT EXISTS ledger_state ON ledger_entry(state);
CREATE INDEX IF NOT EXISTS ledger_content ON ledger_entry(content_fingerprint);
CREATE INDEX IF NOT EXISTS ledger_tampered ON ledger_entry(tampered);
";

/// One mirrored ledger entry: the row the markdown file earned.
///
/// Every field here is *read out of the file*, including the ones that look derived (`state`,
/// `link_method`, `tampered`). The mirror is an index over the file, so a field that came from somewhere
/// else would be a second answer to the same question — and the two would disagree the first time an
/// index run and a re-mirror did not see the same world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LedgerRow {
    /// The entry id, stable inside the file (`sg-…`).
    pub id: String,
    /// The ledger file, relative to the repository root.
    pub file: String,
    /// The anchor a surface jumps to (the entry's heading).
    pub anchor: String,
    /// The episode the entry resolves to *now*; `None` when nothing does (orphan).
    pub episode_id: Option<String>,
    /// The episode identity digest it was signed against.
    pub fingerprint: String,
    /// The fallback digest at sign-off (author + subject).
    pub fallback_fingerprint: String,
    /// The shape digest at sign-off (author + paths).
    pub shape_fingerprint: String,
    /// The content digest at sign-off. Empty when the content was never measured.
    pub content_fingerprint: String,
    /// The live episode's digest the entry was linked to. Empty with no link.
    pub linked_fingerprint: String,
    /// `accepted`, `flagged`, `dismissed` or `snoozed`.
    pub decision: String,
    /// The dismiss vocabulary's code, when the decision used one.
    pub reason_code: Option<String>,
    /// The reviewer's own words, verbatim.
    pub note: String,
    /// Who signed, in `name <address>` form or the literal `unknown`.
    pub actor: String,
    /// `human` or `agent` — who the act is attributed to.
    pub actor_kind: String,
    /// When it was signed, unix millis.
    pub signed_utc: i64,
    /// The scope document as JSON: the members, paths and scale the sign-off covered.
    pub scope: String,
    /// `sha256` of the entry body as written — the tamper digest.
    pub content_sha: String,
    /// `exact`, `fingerprint`, `shape`, `member-set`, `manual` or `none`.
    pub link_method: String,
    /// How much to trust the link; `1.0` for an id that did not move.
    pub link_score: f64,
    /// `fresh`, `stale` or `orphan`.
    pub state: String,
    /// Why it is stale or orphaned, in the reviewer's words rather than a code.
    pub stale_reason: Option<String>,
    /// The episode id the entry was originally signed against, when the link moved it.
    pub relinked_from: Option<String>,
    /// Whether the entry's body still hashes to [`LedgerRow::content_sha`].
    pub tampered: bool,
    /// Whether the episode it links to is dead (no current commit belongs to it).
    pub dead: bool,
    /// Member commits the sign-off covered.
    pub n_members: i64,
    /// Files the sign-off covered.
    pub n_files: i64,
    /// When this row was mirrored, unix millis.
    pub mirrored_utc: i64,
}

impl Store {
    /// Rebuild the whole ledger mirror from the rows the caller just parsed.
    ///
    /// One transaction, and it deletes before it inserts: ADR-004 §9.1's "drop the mirrors and re-parse
    /// them" is the *write* path, not just the recovery path, because an entry deleted from the markdown
    /// has to disappear from the index too and an upsert would leave it behind as a sign-off nobody signed.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if either half fails — and then the mirror is exactly as it was, which is
    /// the property that makes a re-mirror safe to run at the drop of a hat.
    pub fn mirror_ledger(&mut self, now: i64, rows: &[LedgerRow]) -> Result<usize, Error> {
        let tx = self
            .conn_mut()
            .transaction()
            .map_err(|err| corrupt("opening a ledger mirror transaction", &err))?;
        tx.execute("DELETE FROM ledger_entry", [])
            .map_err(|err| corrupt("clearing the ledger mirror", &err))?;
        for row in rows {
            tx.execute(
                "INSERT INTO ledger_entry(id, file, anchor, episode_id, fingerprint, decision,
                        reason_code, actor, actor_kind, signed_utc, scope, content_sha,
                        link_method, link_score, state, stale_reason, relinked_from, mirrored_utc,
                        fallback_fingerprint, shape_fingerprint, content_fingerprint,
                        linked_fingerprint, note, tampered, dead, n_members, n_files)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,
                         ?19,?20,?21,?22,?23,?24,?25,?26,?27)",
                params![
                    row.id,
                    row.file,
                    row.anchor,
                    row.episode_id,
                    row.fingerprint,
                    row.decision,
                    row.reason_code,
                    row.actor,
                    row.actor_kind,
                    row.signed_utc,
                    row.scope,
                    row.content_sha,
                    row.link_method,
                    row.link_score,
                    row.state,
                    row.stale_reason,
                    row.relinked_from,
                    now,
                    row.fallback_fingerprint,
                    row.shape_fingerprint,
                    row.content_fingerprint,
                    row.linked_fingerprint,
                    row.note,
                    i64::from(row.tampered),
                    i64::from(row.dead),
                    row.n_members,
                    row.n_files,
                ],
            )
            .map_err(|err| corrupt("writing a ledger entry", &err))?;
        }
        tx.commit()
            .map_err(|err| corrupt("committing the ledger mirror", &err))?;
        Ok(rows.len())
    }

    /// Every mirrored entry, oldest sign-off first.
    ///
    /// Ordered by `signed_utc` then id — the order the ledger file appends in, so a reader who prints the
    /// rows prints the file. `mirrored_utc` is deliberately *not* the key: re-mirroring must not reorder
    /// history.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn ledger_rows(&self) -> Result<Vec<LedgerRow>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT id, file, anchor, episode_id, fingerprint, decision, reason_code, actor,
                        actor_kind, signed_utc, scope, content_sha, link_method, link_score,
                        state, stale_reason, relinked_from, fallback_fingerprint,
                        shape_fingerprint, content_fingerprint, linked_fingerprint, note,
                        tampered, dead, n_members, n_files, mirrored_utc
                 FROM ledger_entry ORDER BY signed_utc, id",
            )
            .map_err(|err| corrupt("reading the ledger mirror", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok(LedgerRow {
                    id: row.get(0)?,
                    file: row.get(1)?,
                    anchor: row.get(2)?,
                    episode_id: row.get(3)?,
                    fingerprint: row.get(4)?,
                    decision: row.get(5)?,
                    reason_code: row.get(6)?,
                    actor: row.get(7)?,
                    actor_kind: row.get(8)?,
                    signed_utc: row.get(9)?,
                    scope: row.get(10)?,
                    content_sha: row.get(11)?,
                    link_method: row.get(12)?,
                    link_score: row.get(13)?,
                    state: row.get(14)?,
                    stale_reason: row.get(15)?,
                    relinked_from: row.get(16)?,
                    fallback_fingerprint: row.get(17)?,
                    shape_fingerprint: row.get(18)?,
                    content_fingerprint: row.get(19)?,
                    linked_fingerprint: row.get(20)?,
                    note: row.get(21)?,
                    tampered: row.get::<_, i64>(22)? != 0,
                    dead: row.get::<_, i64>(23)? != 0,
                    n_members: row.get(24)?,
                    n_files: row.get(25)?,
                    mirrored_utc: row.get(26)?,
                })
            })
            .map_err(|err| corrupt("reading the ledger mirror", &err))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| corrupt("reading the ledger mirror", &err))?);
        }
        Ok(out)
    }

    /// How many entries sit in each state — the status line's `ledger 12 · 1 stale`.
    ///
    /// Tampering is counted in the same pass because a tampered row is not a fourth state
    /// ([`LedgerRow::tampered`] is orthogonal), and a status line that printed only `state` would print
    /// `fresh` about an entry whose text somebody edited.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn ledger_counts(&self) -> Result<LedgerCounts, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT state, COUNT(*), SUM(tampered), SUM(dead) FROM ledger_entry GROUP BY state",
            )
            .map_err(|err| corrupt("counting the ledger mirror", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|err| corrupt("counting the ledger mirror", &err))?;
        let mut out = LedgerCounts::default();
        for row in rows {
            let (state, count, tampered, dead) =
                row.map_err(|err| corrupt("counting the ledger mirror", &err))?;
            out.total += count;
            out.tampered += tampered;
            out.dead += dead;
            match state.as_str() {
                "fresh" => out.fresh += count,
                "stale" => out.stale += count,
                "orphan" => out.orphan += count,
                // An unexpected spelling is still counted, in `total`, rather than dropped: the total is
                // the number a reader can check against `fathom review list | wc -l`.
                _ => out.unknown += count,
            }
        }
        Ok(out)
    }

    /// The episodes a ledger row points at, whether or not the link resolved.
    ///
    /// `linked_fingerprint` is empty for an orphan, so a caller that wants to re-check a link needs the id
    /// it has; this is the query that hands it over without a second read of the whole mirror.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn ledger_episode_ids(&self) -> Result<Vec<String>, Error> {
        let mut statement = self
            .connection()
            .prepare("SELECT DISTINCT episode_id FROM ledger_entry WHERE episode_id IS NOT NULL")
            .map_err(|err| corrupt("reading the ledger's episode links", &err))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|err| corrupt("reading the ledger's episode links", &err))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| corrupt("reading the ledger's episode links", &err))?);
        }
        Ok(out)
    }

    /// Record the rewrite a re-link discovered, using ADR-003 Appendix B's closed vocabularies.
    ///
    /// ADR-003 RL5 makes "did my sign-off survive the rebase?" a query rather than a hope, and §6.4
    /// reserves `method=exact` and `relation=amend` for this ticket for exactly that reason. The ledger is
    /// the second writer of `episode_rewrite`: it records the links *it* proved, which are not always the
    /// ones the segmentation pass proved (a sign-off re-linked across a squash whose episode relink was
    /// refused by RL2 because the live episode already had a marker of its own).
    ///
    /// Returns the number of rows written, which is the number of links that were news.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the write fails.
    pub fn record_ledger_rewrites(
        &mut self,
        links: &[(String, String, String, String, f64)],
        now: i64,
    ) -> Result<usize, Error> {
        let tx = self
            .conn_mut()
            .transaction()
            .map_err(|err| corrupt("opening a ledger rewrite transaction", &err))?;
        let mut written = 0_usize;
        for (old_id, new_id, relation, method, score) in links {
            let changed = tx
                .execute(
                    "INSERT INTO episode_rewrite(old_id, new_id, relation, method, score, at_utc)
                     VALUES (?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(old_id, new_id) DO NOTHING",
                    params![old_id, new_id, relation, method, score, now],
                )
                .map_err(|err| corrupt("recording a ledger re-link", &err))?;
            written += changed;
        }
        tx.commit()
            .map_err(|err| corrupt("committing ledger re-links", &err))?;
        Ok(written)
    }
}

/// How many ledger entries the store holds, by verdict.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LedgerCounts {
    /// Every entry mirrored.
    pub total: i64,
    /// Linked, and the content still matches what was signed.
    pub fresh: i64,
    /// Linked, and the content is not what was signed.
    pub stale: i64,
    /// Linked to nothing.
    pub orphan: i64,
    /// A spelling this build does not know, counted rather than dropped.
    pub unknown: i64,
    /// Entries whose markdown body no longer hashes to their own digest.
    pub tampered: i64,
    /// Entries linked to an episode no current commit belongs to.
    pub dead: i64,
}

impl LedgerCounts {
    /// Is the ledger trustworthy? `true` with nothing stale, nothing orphaned and nothing tampered.
    ///
    /// Note what this does *not* claim: that every episode has been reviewed. That question is about the
    /// index and the ledger together, and `fathom review status` answers it; a method on the counts alone
    /// would be a function named `clean` that lies about half the word.
    #[must_use]
    pub const fn trustworthy(&self) -> bool {
        self.stale == 0 && self.orphan == 0 && self.tampered == 0 && self.unknown == 0
    }
}

/// One `(episode, member ordinal, path)` row of the diff, for the content fingerprint (`e4.6`).
///
/// Deliberately narrower than [`crate::episode::EpisodeFile`]: that one is summed per path because the
/// episode card asks "what did this turn do to this file", and the content fingerprint asks a different
/// question — *what was the content at the end*, which a sum cannot answer (a squash of three commits that
/// churned a file four times has the same final text as one commit that wrote it once, and a reviewer who
/// signed off the final text has not had it changed under them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeDiffRow {
    /// The episode the row belongs to.
    pub episode: String,
    /// The member's ordinal within the episode, so "last touch wins" is decidable.
    pub ordinal: i64,
    /// The post-change path.
    pub path: String,
    /// `add`, `modify`, `delete`, `rename`, `copy` or `binary`, as the indexer recorded it.
    pub change_kind: String,
    /// Lines added by *this member*, for the scale a status line prints.
    pub add: i64,
    /// Lines removed by *this member*.
    pub del: i64,
    /// The new content's git blob oid, when the indexer kept one. `None` for a deletion and for a payload
    /// over the size cap, and the caller must treat the absence as "not measured", never as "unchanged".
    pub blob_new: Option<String>,
}

impl Store {
    /// The per-member file rows of every **live** episode, for the content fingerprint.
    ///
    /// Ordered by `(episode, ordinal, path)` so the caller can fold it into a per-episode profile in one
    /// pass. Capped by [`Store::EPISODE_DIFF_CAP`] with `truncated` set rather than the read silently
    /// coming up short: a profile built from a truncated read must not be reported as a content digest
    /// (`e4.6` prints `not examined` instead), which is ADR-001 §7.6's "unknown is a first-class value"
    /// applied to the one digest whose whole job is to be trusted.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn episode_diff_rows(&self) -> Result<EpisodeDiffRows, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT m.episode_id, m.ordinal, f.new_path, f.change_kind, f.n_add, f.n_del,
                        f.blob_new
                   FROM episode_member m
                   JOIN episode e ON e.id = m.episode_id AND e.live = 1
                   JOIN git_commit_file f ON f.commit_oid = m.commit_oid
                  ORDER BY m.episode_id, m.ordinal, f.new_path
                  LIMIT ?1",
            )
            .map_err(|err| corrupt("reading episode diff rows", &err))?;
        let rows = statement
            .query_map(params![Self::EPISODE_DIFF_CAP], |row| {
                Ok(EpisodeDiffRow {
                    episode: row.get(0)?,
                    ordinal: row.get(1)?,
                    path: row.get(2)?,
                    change_kind: row.get(3)?,
                    add: row.get(4)?,
                    del: row.get(5)?,
                    blob_new: row.get(6)?,
                })
            })
            .map_err(|err| corrupt("reading episode diff rows", &err))?;
        let mut out = EpisodeDiffRows::default();
        for row in rows {
            out.rows
                .push(row.map_err(|err| corrupt("reading episode diff rows", &err))?);
        }
        out.truncated = out.rows.len() == Self::EPISODE_DIFF_CAP as usize;
        Ok(out)
    }

    /// How many `(episode, member, path)` rows [`Store::episode_diff_rows`] will read before it stops.
    ///
    /// 200 000 rows is a repository with tens of thousands of live episodes at a few files each; the cap
    /// exists to bound the memory of a status command, not to bind a real repository, and when it does bind
    /// the caller says so out loud.
    pub const EPISODE_DIFF_CAP: i64 = 200_000;
}

/// The rows [`Store::episode_diff_rows`] read, plus whether the cap bound.
#[derive(Debug, Default)]
pub struct EpisodeDiffRows {
    /// The rows, in `(episode, ordinal, path)` order.
    pub rows: Vec<EpisodeDiffRow>,
    /// Whether the read stopped at [`Store::EPISODE_DIFF_CAP`].
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::LedgerCounts;

    /// `trustworthy` is the ledger half of "all reviewed", and says nothing about the rest of it.
    #[test]
    fn a_fresh_ledger_is_trustworthy_and_anything_else_is_not() {
        assert!(
            LedgerCounts {
                total: 3,
                fresh: 3,
                ..LedgerCounts::default()
            }
            .trustworthy()
        );
        assert!(
            !LedgerCounts {
                total: 3,
                fresh: 2,
                stale: 1,
                ..LedgerCounts::default()
            }
            .trustworthy()
        );
        assert!(
            !LedgerCounts {
                total: 3,
                fresh: 2,
                orphan: 1,
                ..LedgerCounts::default()
            }
            .trustworthy()
        );
        // The one this ticket would rather not have caught itself forgetting: a tampered entry that is
        // otherwise perfectly fresh is not trustworthy, and `state` alone would have called it clean.
        assert!(
            !LedgerCounts {
                total: 1,
                fresh: 1,
                tampered: 1,
                ..LedgerCounts::default()
            }
            .trustworthy()
        );
    }
}

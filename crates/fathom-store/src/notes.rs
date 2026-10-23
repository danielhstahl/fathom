//! `notes.rs` — migration 10: the mirror of the review memory (`e6.5`, `reco-552`).
//!
//! ADR-004 §10 makes this row class **M** by the same argument that makes a sign-off class M: it is a human
//! act that cannot be recomputed, and its authoritative copy is a committed markdown file
//! (`.fathom/reviews/notes.md`, [`crate::notes`]'s mirror is the table here). The write path is therefore
//! the same one [`crate::ledger`] runs: *drop every row and re-read the files* (§9.1's recovery level 2,
//! used as the only write path precisely so it stays legal).
//!
//! # The columns, and the ones this ticket added
//!
//! The DDL up to the `-- e6.5 additions` marker is ADR-004 Appendix A's `decision_note` reproduced with its
//! column names, CHECK vocabularies and indexes unchanged — including `path_hash` (a note pinned to a path
//! rather than an episode is a first-class case in this ticket, not an afterthought: a path digest is the
//! only handle that survives a rewrite of both the episode *and* the line numbers) and `body_sha` (the
//! entry's own tamper digest, same framing rule as [`crate::ledger`]).
//!
//! Two ADR columns are deliberately not filled, and both omissions are claims:
//!
//! * `blob_sha` stays NULL. The note's prose lives in the file, and the mirror copies it inline into `text`
//!   exactly as `ledger_entry` copies a sign-off's `note` inline. Putting it in `blob` as well would either
//!   duplicate the file for no benefit or make class-M prose depend on §8.2's prune guard to survive, and a
//!   decision whose survival depends on a cache-retention rule is a decision with a bug waiting in it.
//!   `blob_sha` is the column the day a note grows an *attachment* — a profile, a flamegraph, a failing
//!   test's output — has a use for; an attachment is class R by nature and belongs in `blob`.
//! * `status` is *derived*, not typed. Nothing a person types says "this question is still open"; what makes
//!   one closed is a later note naming it ([`crate::notes::entry::open_questions`]), and a status column
//!   filled from the note's own text would be a second answer that goes stale the moment its answer is
//!   appended. [`Store::mirror_notes`] fills it from the whole set for exactly that reason — the caller has
//!   to hand over the verdict computed over every note, because one note cannot know it.
//!
//! The columns after the marker are the ones this ticket's acceptance criteria need and the reference schema
//! has no room for: the two weaker digests a re-link rides on, the link verdict (`link_method`,
//! `link_score`, `state`, `stale_reason`, `relinked_from`, `dead`, `tampered`) — same vocabulary as
//! [`crate::ledger`]'s, deliberately, because "is this still attached" is one question whatever kind of
//! entry asks it — plus `text`, `target`, `about`, `withdraws`, `via` and the counts a surface prints
//! without re-parsing anything.

use rusqlite::params;

use crate::error::Error;
use crate::spine::Store;
use crate::spine::corrupt;

/// Migration 10: the review-memory mirror — ADR-004 Appendix A's `decision_note` with `e6.5`'s columns.
pub const MIGRATION_10: &str = "
CREATE TABLE IF NOT EXISTS decision_note (
  id TEXT PRIMARY KEY,
  file TEXT NOT NULL,
  anchor TEXT NOT NULL,
  episode_id TEXT,
  fingerprint TEXT,
  path_hash TEXT,                          -- note attached to a path rather than an episode
  kind TEXT NOT NULL CHECK (kind IN ('decision','question','followup','annotation')),
  status TEXT NOT NULL CHECK (status IN ('open','answered','withdrawn')),
  blob_sha TEXT REFERENCES blob(sha256),   -- the link §8.2's prune guard exists to protect
  author TEXT NOT NULL,
  created_utc INTEGER NOT NULL,
  body_sha TEXT NOT NULL,
  mirrored_utc INTEGER NOT NULL,
  -- e6.5 additions: the link verdict, and what a surface prints without re-reading the file.
  text               TEXT NOT NULL DEFAULT '',  -- the note's own words, verbatim
  author_kind        TEXT NOT NULL CHECK (author_kind IN ('human','agent')),
  target             TEXT NOT NULL DEFAULT '',  -- `path:line` evidence anchor as written
  about              TEXT NOT NULL DEFAULT '',  -- the note this one answers
  withdraws          TEXT NOT NULL DEFAULT '',  -- the note this one retracts
  via                TEXT NOT NULL DEFAULT '',  -- which door wrote it
  fallback_fingerprint TEXT NOT NULL DEFAULT '', -- author+subject: what a squash rides on
  shape_fingerprint    TEXT NOT NULL DEFAULT '', -- author+paths: what a reword rides on
  content_fingerprint  TEXT NOT NULL DEFAULT '', -- the content it was written against
  linked_fingerprint   TEXT NOT NULL DEFAULT '', -- the live episode's digest, after the re-link
  -- `path` is not a rung of the relink ladder: it is the verdict a path-anchored note gets, which never
  -- consults an episode at all. It is in the vocabulary because the mirror reports how a note is attached,
  -- and being attached to the file it names is an answer, not a failure.
  link_method TEXT NOT NULL CHECK (link_method IN ('exact','fingerprint','shape','member-set','manual','path','none')),
  link_score  REAL NOT NULL DEFAULT 1,
  state TEXT NOT NULL CHECK (state IN ('fresh','stale','orphan')),
  stale_reason TEXT,
  relinked_from TEXT,
  tampered           INTEGER NOT NULL DEFAULT 0, -- 1: the markdown body no longer hashes to body_sha
  dead               INTEGER NOT NULL DEFAULT 0, -- 1: it links to an episode no current commit belongs to
  path_anchored      INTEGER NOT NULL DEFAULT 0, -- 1: it was anchored to a path, with no episode to link
  n_members          INTEGER NOT NULL DEFAULT 0,
  n_files            INTEGER NOT NULL DEFAULT 0,
  scope              TEXT NOT NULL DEFAULT '{}'  -- JSON: members and paths, as the mirror holds them
) STRICT;
CREATE INDEX IF NOT EXISTS note_episode ON decision_note(episode_id);
CREATE INDEX IF NOT EXISTS note_path ON decision_note(path_hash);
CREATE INDEX IF NOT EXISTS note_kind ON decision_note(kind, status);
CREATE INDEX IF NOT EXISTS note_tampered ON decision_note(tampered);
";

/// One mirrored note: the row the markdown file earned.
///
/// As with [`crate::ledger::LedgerRow`], every field here is read out of the file or derived from the whole
/// set of notes (`status`) — nothing comes from anywhere else, because a mirror that mixed sources could
/// disagree with the file it indexes and then nobody could say which one the reviewer wrote.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteRow {
    /// The note id (`nt-…`).
    pub id: String,
    /// The memory file, relative to the repository root.
    pub file: String,
    /// The anchor a surface jumps to (the entry's heading).
    pub anchor: String,
    /// The episode it resolves to *now*; `None` for a path-anchored note or an orphan.
    pub episode_id: Option<String>,
    /// The episode identity digest at write time (ADR-004's `fingerprint`, nullable there and here).
    pub fingerprint: String,
    /// The sha256 of the path this note is anchored to, when it is path-anchored — the indexed handle
    /// `fathom context <path>` asks with.
    pub path_hash: Option<String>,
    /// `decision`, `question`, `followup`, `annotation`.
    pub kind: String,
    /// `open`, `answered`, `withdrawn` — the verdict over the whole set, not a field the author typed.
    pub status: String,
    /// Who wrote it.
    pub author: String,
    /// `human` or `agent`, as the file recorded it — `e6.5`'s column, for the ledger's reason: a note an
    /// agent wrote on a human's behalf is a different fact, and the distinction has to be recorded rather
    /// than guessed at from the shape of the name.
    pub author_kind: String,
    /// When, unix milliseconds.
    pub created_utc: i64,
    /// The entry's tamper digest (`body_sha`).
    pub body_sha: String,
    /// The note's words.
    pub text: String,
    /// `src/a.rs:42`, or empty.
    pub target: String,
    /// The note this one answers; empty when none.
    pub about: String,
    /// The note this one retracts; empty when none.
    pub withdraws: String,
    /// Which door wrote it.
    pub via: String,
    /// The author+subject digest at write time.
    pub fallback_fingerprint: String,
    /// The author+paths digest at write time.
    pub shape_fingerprint: String,
    /// The content digest at write time.
    pub content_fingerprint: String,
    /// The live episode's digest the note was linked to. Empty with no link.
    pub linked_fingerprint: String,
    /// The link verdict — [`crate::ledger`]'s vocabulary, on purpose.
    pub link_method: String,
    /// The ladder's published price for that method.
    pub link_score: f64,
    /// `fresh`, `stale`, `orphan`.
    pub state: String,
    /// Why it is stale or orphaned; `None` when fresh.
    pub stale_reason: Option<String>,
    /// The episode it was written against, when the re-link moved it.
    pub relinked_from: Option<String>,
    /// The markdown no longer hashes to [`NoteRow::body_sha`].
    pub tampered: bool,
    /// Whether the episode it links to is dead (no current commit belongs to it).
    pub dead: bool,
    /// Whether it was anchored to a path with no episode behind it — a state, not a failure.
    pub path_anchored: bool,
    /// Member commits the note listed.
    pub n_members: i64,
    /// Paths the note listed.
    pub n_files: i64,
    /// JSON: the members and paths, in full.
    pub scope: String,
    /// When this row was mirrored, unix millis.
    pub mirrored_utc: i64,
}

/// How many notes sit in each status and each link state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoteCounts {
    /// Rows in the mirror.
    pub total: usize,
    /// …of which open, answered, withdrawn — the question half of `fathom context`.
    pub open: usize,
    /// See [`NoteCounts::open`].
    pub answered: usize,
    /// See [`NoteCounts::open`].
    pub withdrawn: usize,
    /// Rows whose markdown no longer hashes to `body_sha`.
    pub tampered: usize,
    /// Rows linked to an episode no current commit belongs to.
    pub orphan: usize,
    /// Rows of an unexpected spelling, still counted in `total` rather than dropped.
    pub unknown: usize,
}

impl Store {
    /// Rebuild the whole review-memory mirror from the rows the caller just parsed.
    ///
    /// One transaction, deleting before inserting: ADR-004 §9.1's "drop the mirrors and re-parse them" is
    /// the *write* path here, for the ledger's reason — a note deleted from the markdown has to disappear
    /// from the index too, and an upsert would leave behind a decision nobody made.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if either half fails — and then the mirror is exactly as it was.
    pub fn mirror_notes(&mut self, now: i64, rows: &[NoteRow]) -> Result<usize, Error> {
        let tx = self
            .conn_mut()
            .transaction()
            .map_err(|err| corrupt("opening a review-memory mirror transaction", &err))?;
        tx.execute("DELETE FROM decision_note", [])
            .map_err(|err| corrupt("clearing the review-memory mirror", &err))?;
        for row in rows {
            tx.execute(
                "INSERT INTO decision_note(id, file, anchor, episode_id, fingerprint, path_hash, kind,
                        status, blob_sha, author, created_utc, body_sha, mirrored_utc,
                        text, author_kind, target, about, withdraws, via, fallback_fingerprint,
                        shape_fingerprint, content_fingerprint, linked_fingerprint, link_method,
                        link_score, state, stale_reason, relinked_from, tampered, dead, path_anchored,
                        n_members, n_files, scope)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL,?9,?10,?11,?12,
                         ?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,
                         ?29,?30,?31,?32,?33)",
                params![
                    row.id,
                    row.file,
                    row.anchor,
                    row.episode_id,
                    row.fingerprint,
                    row.path_hash,
                    row.kind,
                    row.status,
                    row.author,
                    row.created_utc,
                    row.body_sha,
                    now,
                    row.text,
                    row.author_kind,
                    row.target,
                    row.about,
                    row.withdraws,
                    row.via,
                    row.fallback_fingerprint,
                    row.shape_fingerprint,
                    row.content_fingerprint,
                    row.linked_fingerprint,
                    row.link_method,
                    row.link_score,
                    row.state,
                    row.stale_reason,
                    row.relinked_from,
                    i64::from(row.tampered),
                    i64::from(row.dead),
                    i64::from(row.path_anchored),
                    row.n_members,
                    row.n_files,
                    row.scope,
                ],
            )
            .map_err(|err| corrupt("writing a review-memory note", &err))?;
        }
        tx.commit()
            .map_err(|err| corrupt("committing the review-memory mirror", &err))?;
        Ok(rows.len())
    }

    /// Every mirrored note, oldest first — the order the file appends in, so a surface that prints the rows
    /// prints the file.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn note_rows(&self) -> Result<Vec<NoteRow>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT id, file, anchor, episode_id, fingerprint, path_hash, kind, status, author,
                        created_utc, body_sha, text, author_kind, target, about, withdraws, via,
                        fallback_fingerprint, shape_fingerprint, content_fingerprint,
                        linked_fingerprint, link_method, link_score, state, stale_reason,
                        relinked_from, tampered, dead, path_anchored, n_members, n_files, scope,
                        mirrored_utc
                 FROM decision_note ORDER BY created_utc, id",
            )
            .map_err(|err| corrupt("reading the review-memory mirror", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok(NoteRow {
                    id: row.get(0)?,
                    file: row.get(1)?,
                    anchor: row.get(2)?,
                    episode_id: row.get(3)?,
                    fingerprint: row.get(4)?,
                    path_hash: row.get(5)?,
                    kind: row.get(6)?,
                    status: row.get(7)?,
                    author: row.get(8)?,
                    created_utc: row.get(9)?,
                    body_sha: row.get(10)?,
                    text: row.get(11)?,
                    author_kind: row.get(12)?,
                    target: row.get(13)?,
                    about: row.get(14)?,
                    withdraws: row.get(15)?,
                    via: row.get(16)?,
                    fallback_fingerprint: row.get(17)?,
                    shape_fingerprint: row.get(18)?,
                    content_fingerprint: row.get(19)?,
                    linked_fingerprint: row.get(20)?,
                    link_method: row.get(21)?,
                    link_score: row.get(22)?,
                    state: row.get(23)?,
                    stale_reason: row.get(24)?,
                    relinked_from: row.get(25)?,
                    tampered: row.get::<_, i64>(26)? != 0,
                    dead: row.get::<_, i64>(27)? != 0,
                    path_anchored: row.get::<_, i64>(28)? != 0,
                    n_members: row.get(29)?,
                    n_files: row.get(30)?,
                    scope: row.get(31)?,
                    mirrored_utc: row.get(32)?,
                })
            })
            .map_err(|err| corrupt("reading the review-memory mirror", &err))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| corrupt("reading the review-memory mirror", &err))?);
        }
        Ok(out)
    }

    /// How many notes sit in each status — the line `fathom note list` opens with.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn note_counts(&self) -> Result<NoteCounts, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT status, COUNT(*), SUM(tampered), SUM(state = 'orphan')
                 FROM decision_note GROUP BY status",
            )
            .map_err(|err| corrupt("counting the review-memory mirror", &err))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|err| corrupt("counting the review-memory mirror", &err))?;
        let mut out = NoteCounts::default();
        for row in rows {
            let (status, count, tampered, orphan) =
                row.map_err(|err| corrupt("counting the review-memory mirror", &err))?;
            out.total += usize::try_from(count).unwrap_or(usize::MAX);
            out.tampered += usize::try_from(tampered).unwrap_or(usize::MAX);
            out.orphan += usize::try_from(orphan).unwrap_or(usize::MAX);
            match status.as_str() {
                "open" => out.open += usize::try_from(count).unwrap_or(usize::MAX),
                "answered" => out.answered += usize::try_from(count).unwrap_or(usize::MAX),
                "withdrawn" => out.withdrawn += usize::try_from(count).unwrap_or(usize::MAX),
                // An unexpected spelling is still counted, in `total`: the total is the number a reader can
                // check against `fathom note list | wc -l`.
                _ => out.unknown += usize::try_from(count).unwrap_or(usize::MAX),
            }
        }
        Ok(out)
    }

    /// The notes whose path anchor is `path`, oldest first — `fathom context <path>`'s indexed handle.
    ///
    /// The digest is computed here rather than by the caller so the caller cannot get it wrong: `path_hash`
    /// exists to be joined on, and a second spelling of it (a shortened digest, a normalised one) is a
    /// second answer to "what did somebody annotate here".
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn note_rows_for_path(&self, path: &str) -> Result<Vec<NoteRow>, Error> {
        let digest = crate::cache::sha256_hex(path.as_bytes());
        let all = self.note_rows()?;
        Ok(all
            .into_iter()
            .filter(|row| row.path_hash.as_deref() == Some(digest.as_str()))
            .collect())
    }
}

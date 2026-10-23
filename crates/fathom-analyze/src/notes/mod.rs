//! `fathom_analyze::notes` — the review memory: the annotations, decisions, questions and follow-ups a
//! review produces and that must outlive the session (`e6.5`, `reco-552`).
//!
//! The glossary's sentence for the ledger is "the durable, committable record of human dispositions"; this
//! module is the *other* half of that sentence, the half the ledger deliberately refuses to be. A sign-off
//! says **what was decided about a change** in a closed vocabulary (`accepted`, `flagged`, `dismissed`,
//! `snoozed`) and nothing else — its `note:` field is one line of prose bolted onto a disposition. Most of
//! what a review actually produces is not a disposition:
//!
//! * a **decision** and its *reason* — "we deliberately allow a cycle here because the payment table is
//!   append-only";
//! * a **question** somebody asked in the diff and did not answer, and the answer that eventually closed it;
//! * a **follow-up** — "this works, but nobody has worked out what happens at 10k tenants";
//! * an **annotation** on a line, which is none of the above and is not a review either.
//!
//! That knowledge is the part that walks out the door when the session ends, and it is exactly the part a
//! later reader — a teammate, or the agent asking *why did we do X* — cannot recompute from the commits.
//!
//! # The shape, and where it lives
//!
//! ```text
//!   .fathom/reviews/notes.md            the durable record: markdown, append-only, committed
//!            │  parse (format.rs)
//!            ▼
//!   Note { kind, episode, digests, text, target, about, author, written }   (entry.rs)
//!            │  relink::link_over (the same ladder the ledger rides, over the same digests)
//!            ▼
//!   notes::collect::refresh → store.mirror_notes → `fathom context`, `fathom note list`,
//!                              `fathom export`, the MCP `review_memory` tool
//! ```
//!
//! The direction of that arrow is the whole design and it is inherited from `crate::ledger` on purpose:
//! **the file is the truth and the index is a disposable mirror of it** (ADR-004 §10, class M). Deleting
//! `.fathom/index.sqlite3` loses no note; deleting `notes.md` loses the review memory, which is why
//! the must-survive check (`fathom review status`, `fathom doctor`) already covers every
//! `*.md` in that directory, this file included.
//! # Why it is a second file and not more blocks in `ledger.md`
//!
//! Because the two files change for different reasons, and a shared file would make the ledger — the one
//! artefact a gate reads — churn with prose. A note is cheap and plentiful (a dozen to an episode); a
//! sign-off is rare, deliberate and is the north-star metric's unit. Appending a question to the ledger
//! would make every `git log -- .fathom/reviews/ledger.md` answer "what changed here?" with "someone wrote
//! a paragraph", and the two formats carry different field sets: reading one with the other's parser would
//! report a stack of "unknown field" problems about a file that is perfectly well-formed. So there are two
//! format tags ([`format::FORMAT_TAG`] beside `fathom-review/1`), and each reader skips a file that declares
//! the other's.
//!
//! # Why a note is keyed to an episode rather than to a line
//!
//! A line number is a coordinate in a revision that stops existing the first time anything above it moves —
//! and the ticket's first acceptance criterion is that a note survives a rebase. So a note carries ADR-003's
//! three episode digests plus its member set, exactly like a sign-off, and rides the same re-link ladder
//! ([`crate::ledger::relink::Anchor`] is what lets one ladder serve both). The `target:` line (a path and,
//! optionally, a line) is kept as *evidence* — where the note was pointing when it was written — not as the
//! handle. A note pinned only to a path is allowed, and prints as path-anchored rather than as an orphan:
//! a path is a stable-enough handle for an annotation, and pretending otherwise would be a verdict about
//! something that never claimed to be about an episode.
//!
//! # Questions are closed by appending, never by editing
//!
//! `status:` is not a field. Whether a question is still open is a fact about the *set* of notes: it is open
//! until some later note carries `about: nt-…` naming it ([`entry::open_questions`]). Closing one is
//! therefore an append — an answer, with an author and a date — which is the same rule the ledger runs its
//! corrections by ("do not edit a committed block; append a new one saying so"), and it means the answer
//! arrives with the two things an answer needs: who said it, and when.
//!
//! # What is deliberately *not* here
//!
//! **No clock, no config, no git, no store below [`collect`].** Same discipline as the ledger: the verb
//! supplies the instant and the identity, and everything else is a pure function a test can call with
//! literals.
//!
//! **No secret scanner in the note's text.** Notes are redacted on the *write* path (the CLI and the MCP
//! tool both run [`fathom_core::redact`] before this module sees the text), because redaction is a policy about how text arrives, not a property
//! of the file — and a note that was written by hand before the policy existed must still be readable.
//!
//! **No second ledger.** A note does not settle an episode and is never counted as a review; the north-star
//! metric counts sign-offs ([`crate::ledger`]) and only sign-offs. `fathom context` prints both, labelled,
//! because a person asking "what do we know about this file?" wants both on one screen — but the JSON says
//! which is which, and so does the file.
//!
//! **No ratchet interlock, because there is no ratchet to interlock with yet.** The ticket pairs this module
//! with `e3.10` (`reco-5ht`, not built as this is written): granting an allowance is meant to write *both*
//! the `[[allowance]]` in `.fathom/ratchet.toml` and a `decision` note saying why, because the reason a
//! cycle is allowed is precisely the knowledge nobody can recover from a TOML row later. That write belongs
//! to `e3.10`'s verb and not here — it owns the owner, the expiry and the digest of the loosening — and it
//! has [`collect::append`] to make it with. What this module refuses is to *become* the ratchet file: an
//! allowance is a dated permission with a matcher, a note is a dated claim with an author, and one field
//! set cannot serve both without the ratchet carrying a second, unparseable copy of its own rules.

#![deny(missing_docs)]

pub mod entry;
pub mod format;

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod collect;

pub use entry::{
    Draft, Kind, MAX_TEXT_BYTES, Note, NoteError, NoteTarget, Status, add, answers, note_digest,
    note_id, open_questions, statuses,
};
pub use format::{
    FORMAT_TAG, MAX_NOTE_FILE_BYTES, NOTES_DIR, NOTES_FILE, Parsed, append_notes, discover, parse,
    parse_text, preamble, render, render_entry,
};

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub use collect::{
    AppendError, Attachment, Context, EpisodeBrief, Recorded, Report, append, context,
    default_path, find_episode, read, refresh, rows, verify,
};

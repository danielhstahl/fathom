//! `entry.rs` — one note: what it is attached to, what it says, and the digests that keep it attached.
//!
//! The type is plain data, and the only constructor is [`add`], which refuses to build a note that could not
//! be defended later. Everything here is a pure function of its arguments — no clock, no store, no files —
//! for the reason every module in this crate has: a rule that is a function is a rule with a golden test.
//!
//! # The four kinds, and why so few
//!
//! | kind | what it claims | open until |
//! |---|---|---|
//! | `decision` | *why* it is this way; the answer to "why did we do X" | — |
//! | `question` | something somebody asked and may not have answered | another note names it in `about:` |
//! | `followup` | work this review found and did not do | another note names it in `about:` |
//! | `annotation` | something worth knowing, or "look here", with no disposition attached | — |
//!
//! Four, and the list is ADR-004 Appendix A's `decision_note.kind` CHECK verbatim — the storage ADR named
//! this table before this ticket built it, and a second spelling of a closed vocabulary is the mistake
//! ADR-003 §6.4 counts the cost of. Four because each one changes what a surface prints: `decision` is what
//! the MCP tool answers with, `question` and `followup` are what stay on the list until they are closed,
//! `annotation` is everything that is neither. `fathom-mcp`'s `add_review_note` also accepts the interim
//! word `note` and the word `flag`, and both arrive here as `annotation`.
//!
//! # What a note carries about what it is attached to
//!
//! The episode's id, its three ADR-003 digests, its content digest and its member-commit prefixes — the same
//! six facts a sign-off carries, which is what lets the same re-link ladder serve both
//! ([`crate::ledger::relink::Anchor`]). Plus a `target:` (`src/a.rs:42`), which is *evidence*: it says where
//! the note was pointing when it was written, and a later reader can check it, but nothing is re-attached by
//! it. A note written against a path with no episode behind it carries an empty `episode:` and is reported
//! as path-anchored, not as a failed link ([`Note::is_path_anchored`]).
//!
//! # Ids, and the tamper digest
//!
//! [`note_id`] derives the id from the note's own content (kind, episode, author, instant, text), so the same
//! note written twice is the same id — `fathom note add` run twice by a retry cannot produce two entries,
//! and a mirror rebuild is idempotent. [`note_digest`] hashes the field lines, framed by its own schema tag:
//! a separate frame from the ledger's ([`crate::ledger::entry::REVIEW_SCHEMA`]) because an entry of one kind
//! must never validate as an entry of the other.

use sha2::{Digest as _, Sha256};

use crate::ledger::entry::{Actor, EpisodeFacts, PathFact, iso_utc, ledger_clock};
use crate::ledger::format::one_line;
use crate::ledger::relink::Anchor;

/// The framing prefix of every note digest. Moving it re-addresses every note, so it moves only when a
/// digest's *meaning* changes (the rule ADR-003 §2.4 sets for `IDENTITY_SCHEMA`, and the reason this is not
/// the ledger's `fathom-review/1`: a sign-off must not be able to validate as a note).
pub const NOTE_SCHEMA: &str = "fathom-review-note/1";

/// The largest note body this build will write, in UTF-8 bytes.
///
/// Eight kibibytes: twice the MCP write bound (`fathom_mcp::tools::MAX_BODY_BYTES`, which this has to be at
/// least as generous as, or the agent could not write what the CLI accepts), and far past anything a person
/// types in a terminal. The bound exists because the file this lands in is *committed* and read whole by
/// every reader of the review memory — a body with no ceiling is a way to make a repository's own review
/// file hostile to read.
pub const MAX_TEXT_BYTES: usize = 8 * 1024;

/// What kind of thing a note is — the closed vocabulary `decision_note.kind` CHECKs, spelled exactly as
/// ADR-004 Appendix A spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Why the change is this way. What "why did we do X" returns.
    Decision,
    /// An open question until something answers it.
    Question,
    /// Work this review found and did not do. Closed the same way a question is.
    Followup,
    /// Something worth knowing, or "look here", with no disposition attached.
    Annotation,
}

impl Kind {
    /// The file's and the store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Decision => "decision",
            Kind::Question => "question",
            Kind::Followup => "followup",
            Kind::Annotation => "annotation",
        }
    }

    /// Parse a spelling.
    ///
    /// The aliases are not slop tolerance, they are two specific published vocabularies meeting one file:
    /// `e6.4`'s MCP `add_review_note` advertises `note` (so an agent's `note` arrives as an `annotation`,
    /// which is what it always was), and `follow-up` is the spelling a JSON writer reaches for by instinct
    /// when the store's word is `followup`. Both are accepted on the way in and never written: the file uses
    /// one spelling per kind, because two spellings of a closed vocabulary is the mistake ADR-003 §6.4
    /// exists to name.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "decision" => Some(Kind::Decision),
            "question" => Some(Kind::Question),
            "followup" | "follow-up" | "follow_up" => Some(Kind::Followup),
            "annotation" | "note" | "flag" => Some(Kind::Annotation),
            _ => None,
        }
    }

    /// Whether this kind is a loose end until something closes it.
    #[must_use]
    pub const fn wants_an_answer(self) -> bool {
        matches!(self, Kind::Question | Kind::Followup)
    }
}

/// Where a note stands in its own life — `decision_note.status`, spelled as ADR-004 spells it.
///
/// Derived from the whole set of notes and never typed by the author, for the reason [`crate::notes`] gives:
/// what makes a question answered is that an answer exists, and one note cannot know that about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Live, unclosed. For a question or follow-up this is the state that keeps it on the list.
    Open,
    /// A later note answers it ([`Note::about"]).
    Answered,
    /// A later note retracts it ([`Note::withdraws`]).
    Withdrawn,
}

impl Status {
    /// The store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Answered => "answered",
            Status::Withdrawn => "withdrawn",
        }
    }
}

/// One note, as the memory file keeps it.
///
/// The typed fields are what the code reasons over; [`Note::body`] is what the *file* says, kept separately
/// for the same reason the ledger keeps its own: tamper detection has to hash the bytes a human committed,
/// not the values this build managed to parse out of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The stable id (`nt-…`).
    pub id: String,
    /// The file it was read from, relative to the repository root; empty until [`collect::read`] sets it.
    pub file: String,
    /// What kind of claim this is.
    pub kind: Kind,
    /// The episode it was written against; empty when it is anchored to a path alone.
    pub episode: String,
    /// Who wrote it.
    pub author: Actor,
    /// When, unix milliseconds, truncated to the second the file can round-trip ([`ledger_clock`]).
    pub written_utc: i64,
    /// The prose. One line on disk (newlines folded to `⏎`), full text here.
    pub text: String,
    /// The path the note points at, if any.
    pub path: String,
    /// The line it points at, when the writer knew one.
    pub line: Option<u32>,
    /// The note this one answers or follows (`nt-…`); empty when none.
    pub about: String,
    /// The note this one retracts (`nt-…`); empty when none. The append-only way to take a recorded decision
    /// back: the withdrawn block stays exactly as it was, and this block names it and says why.
    pub withdraws: String,
    /// What wrote it — `fathom note add`, `fathom mcp add_review_note`, `fathom note close`.
    pub via: String,
    /// The episode's strong digest at write time.
    pub fingerprint: String,
    /// The author+subject digest at write time.
    pub fallback: String,
    /// The author+paths digest at write time.
    pub shape: String,
    /// The content digest at write time — what makes "the code moved after this was written" decidable.
    pub content: String,
    /// The member commits it was written against, in the printed eight-hex form.
    pub members: Vec<String>,
    /// The paths it was written against, with their content facts where the index knew them.
    pub paths: Vec<PathFact>,
    /// The field lines as the file holds them, in canonical order.
    pub body: Vec<(String, String)>,
    /// The tamper digest over those lines.
    pub digest: String,
}

impl Note {
    /// The keys a note file may carry, and the only ones its reader accepts. Unknown keys are a reported
    /// problem, not a skipped line: `fingreprint:` would otherwise read as "this note cannot be re-found",
    /// which is the mistake a review memory may not make silently.
    #[must_use]
    pub fn known_keys() -> &'static [&'static str] {
        &[
            "about",
            "author",
            "author-kind",
            "content",
            "episode",
            "fallback",
            "fingerprint",
            "kind",
            "members",
            "paths",
            "shape",
            "target",
            "text",
            "via",
            "withdraws",
            "written",
        ]
    }

    /// `src/a.rs:42`, `src/a.rs`, or `-` — the file's spelling of the evidence anchor.
    #[must_use]
    pub fn target(&self) -> String {
        if self.path.is_empty() {
            return "-".to_owned();
        }
        match self.line {
            Some(line) => format!("{}:{}", self.path, line),
            None => self.path.clone(),
        }
    }

    /// Whether this note is anchored to a path with no episode behind it. A state, not a failure: the note
    /// never claimed to be about an episode, so there is no link for the ladder to have missed.
    #[must_use]
    pub fn is_path_anchored(&self) -> bool {
        self.episode.is_empty()
    }

    /// The field lines a newly built note writes — the order the digest is computed over, so the order is a
    /// contract. Same principle as the ledger's: what a reader needs first (what it is, who, when, what it
    /// says), then the digests in the order their questions get asked, then the evidence.
    fn canonical_body(&self) -> Vec<(String, String)> {
        let mut body = vec![
            ("kind".to_owned(), self.kind.as_str().to_owned()),
            (
                "episode".to_owned(),
                if self.episode.is_empty() {
                    "-".to_owned()
                } else {
                    self.episode.clone()
                },
            ),
            ("author".to_owned(), self.author.display.clone()),
            (
                "author-kind".to_owned(),
                self.author.kind.as_str().to_owned(),
            ),
            ("written".to_owned(), iso_utc(self.written_utc)),
        ];
        if !self.about.is_empty() {
            body.push(("about".to_owned(), self.about.clone()));
        }
        if !self.withdraws.is_empty() {
            body.push(("withdraws".to_owned(), self.withdraws.clone()));
        }
        body.push(("text".to_owned(), self.text.clone()));
        body.push(("target".to_owned(), self.target()));
        body.extend([
            (
                "fingerprint".to_owned(),
                or_unmeasured(&self.fingerprint, "fingerprint"),
            ),
            (
                "fallback".to_owned(),
                or_unmeasured(&self.fallback, "fallback"),
            ),
            ("shape".to_owned(), or_unmeasured(&self.shape, "shape")),
            (
                "content".to_owned(),
                or_unmeasured(&self.content, "content"),
            ),
            ("members".to_owned(), join8(&self.members)),
            ("paths".to_owned(), render_paths(&self.paths)),
        ]);
        if !self.via.is_empty() {
            body.push(("via".to_owned(), self.via.clone()));
        }
        body
    }

    /// The episode this note names as written, or `None` when it is anchored to a path alone.
    #[must_use]
    pub fn written_episode(&self) -> Option<&str> {
        if self.episode.is_empty() {
            None
        } else {
            Some(self.episode.as_str())
        }
    }

    /// Whether the note's own text still hashes to its `digest:` line.
    #[must_use]
    pub fn intact(&self) -> bool {
        note_digest(&self.id, &self.body) == self.digest
    }
}

impl Anchor for Note {
    fn anchor_id(&self) -> &str {
        &self.id
    }
    fn anchor_episode(&self) -> &str {
        &self.episode
    }
    fn anchor_fingerprint(&self) -> &str {
        &self.fingerprint
    }
    fn anchor_fallback(&self) -> &str {
        &self.fallback
    }
    fn anchor_shape(&self) -> &str {
        &self.shape
    }
    fn anchor_members(&self) -> &[String] {
        &self.members
    }
    /// A note is not a disposition, so it never shields an episode from a weaker-tier re-link of a real
    /// sign-off (ADR-003 RL0). Two hundred annotations about a change are not evidence that it was reviewed.
    fn claims_episode(&self) -> bool {
        false
    }
    fn intact(&self) -> bool {
        Note::intact(self)
    }
    fn signed_content(&self) -> Option<(&str, &[PathFact])> {
        // A path-anchored note makes no claim about an episode's content, so none of its content can be
        // stale: "not measurable" and "not claimed" are different sentences and only one of them is a
        // complaint.
        if self.episode.is_empty() {
            None
        } else {
            Some((&self.content, &self.paths))
        }
    }
    fn fallback_summary(&self) -> &str {
        &self.text
    }
}

/// What a note is attached to, as resolved by the caller (which is the only thing holding an index).
///
/// Built from an [`EpisodeFacts`] when the target is or resolves to an episode, and/or from a path and line.
/// The CLI and the MCP tool both resolve here so the two doors cannot write two different shapes of note.
#[derive(Debug, Clone, Default)]
pub struct NoteTarget {
    /// The episode the note is about, with its digests. `None` for a path-only note.
    pub episode: Option<EpisodeFacts>,
    /// The path the note points at, if any.
    pub path: String,
    /// The line it points at, when one was given.
    pub line: Option<u32>,
}

impl NoteTarget {
    /// A note about an episode, optionally narrowed to a path and line inside it.
    #[must_use]
    pub fn episode(episode: EpisodeFacts) -> Self {
        Self {
            episode: Some(episode),
            path: String::new(),
            line: None,
        }
    }

    /// A note about an episode narrowed to a path and, if known, a line.
    #[must_use]
    pub fn at(episode: EpisodeFacts, path: &str, line: Option<u32>) -> Self {
        Self {
            episode: Some(episode),
            path: path.to_owned(),
            line,
        }
    }

    /// A note anchored to a path alone.
    #[must_use]
    pub fn path(path: &str, line: Option<u32>) -> Self {
        Self {
            episode: None,
            path: path.to_owned(),
            line,
        }
    }

    /// The target a **reply** to an existing note should carry: the same episode when the index still has it
    /// under that id, and the same path anchor either way.
    ///
    /// An answer to a question whose episode has left the index cannot be given the digests the question was
    /// written against — those belonged to a history this one does not carry — so it keeps the path and the
    /// `about:` link, which are the halves that still mean something. Copying the old digests quietly would
    /// make the answer look as though it had been written against whatever the change became, which is the
    /// one claim it cannot honestly make.
    #[must_use]
    pub fn reply_to(note: &Note, episodes: &[EpisodeFacts]) -> Self {
        let named = note.written_episode().unwrap_or_default();
        match episodes
            .iter()
            .find(|facts| !named.is_empty() && facts.id == named)
        {
            Some(facts) if note.path.is_empty() => Self::episode((*facts).clone()),
            Some(facts) => Self::at((*facts).clone(), &note.path, note.line),
            None => Self::path(&note.path, note.line),
        }
    }
}

/// Why a note was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteError {
    /// No author, or the literal `unknown`. A note nobody can be attributed to is an anonymous comment on
    /// a repository that records who wrote every line — the review memory's whole value is that a name and
    /// a date travel with the claim.
    NoAuthor,
    /// An empty note.
    NoText,
    /// A note with nothing to point at: no episode and no path. Written and committed, it would be
    /// indistinguishable from a note that was deleted, and no surface could find it again.
    NoTarget,
    /// A note body over [`MAX_TEXT_BYTES`].
    TooLong(usize),
    /// The note claims to answer a note id that is not a note id (`nt-…`), or is not in the memory.
    BadAbout(String),
}

impl NoteError {
    /// The refusal, phrased as the advice it is.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            NoteError::NoAuthor => "a note needs an author: set `git config user.name`/`user.email`, or \
                 `$FATHOM_REVIEWER`, or `[review] identity = \"Name <address>\"` in fathom.toml, or pass \
                 `--author`"
                .to_owned(),
            NoteError::NoText => "a note with no text in it records nothing; pass the text as an argument \
                 or with --text"
                .to_owned(),
            NoteError::NoTarget => "a note needs something to be about: name an episode (--episode), a \
                 commit, or a path (--path src/x.rs:42); a note that points at nothing will be \
                 indistinguishable from a note that was deleted"
                .to_owned(),
            NoteError::TooLong(len) => format!(
                "the note is {len} bytes, over the {MAX_TEXT_BYTES} byte bound; a note is a paragraph, not \
                 a document — put the long form somewhere the note can point at"
            ),
            NoteError::BadAbout(value) => format!(
                "`{value}` is not a note id to answer; `fathom note list` prints the nt-… ids"
            ),
        }
    }
}

impl std::error::Error for NoteError {}

impl std::fmt::Display for NoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// What the caller hands [`add`]: the note's own words, what it points at, and what it closes.
///
/// A struct rather than eight positional parameters because three of these are optional ids and a call site
/// that swaps two `&str`s silently produces a note that answers the wrong question.
#[derive(Debug, Clone)]
pub struct Draft<'a> {
    /// What kind of claim this is.
    pub kind: Kind,
    /// What it is attached to.
    pub target: &'a NoteTarget,
    /// Who wrote it.
    pub author: &'a Actor,
    /// What it says.
    pub text: &'a str,
    /// A note this one answers, if any.
    pub about: &'a str,
    /// A note this one retracts, if any.
    pub withdraws: &'a str,
    /// Which door wrote it.
    pub via: &'a str,
    /// When. Truncated to the second by [`add`].
    pub written_utc: i64,
}

impl<'a> Draft<'a> {
    /// The plainest draft there is: a note of kind `kind` about `target`, written by `author` at `when`.
    ///
    /// Use struct-update syntax for the three optional fields — `Draft { about: id, ..Draft::new(…) }` — so a
    /// call site cannot silently answer the wrong note by getting eight positional arguments in the wrong
    /// order.
    #[must_use]
    pub fn new(
        kind: Kind,
        target: &'a NoteTarget,
        author: &'a Actor,
        text: &'a str,
        written_utc: i64,
    ) -> Self {
        Self {
            kind,
            target,
            author,
            text,
            about: "",
            withdraws: "",
            via: "fathom note add",
            written_utc,
        }
    }
}

/// Write a note.
///
/// This is the only way to make a [`Note`], and it refuses anything that would make the memory weaker: no
/// author, no text, an over-long body, or an `about:`/`withdraws:` that cannot be a note id. The instant is
/// truncated to the second for the reason `ledger_clock` gives — the file cannot round-trip finer, and an id
/// that does not survive a write-and-re-read is an id that duplicates itself.
///
/// # Errors
///
/// A [`NoteError`] for each refusal above.
pub fn add(draft: &Draft<'_>) -> Result<Note, NoteError> {
    let kind = draft.kind;
    let target = draft.target;
    let author = draft.author;
    let written_utc_input = draft.written_utc;
    let text = one_line(draft.text);
    if author.display.trim().is_empty() || author.display == "unknown" {
        return Err(NoteError::NoAuthor);
    }
    if text.is_empty() {
        return Err(NoteError::NoText);
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(NoteError::TooLong(text.len()));
    }
    if target.episode.is_none() && target.path.is_empty() {
        return Err(NoteError::NoTarget);
    }
    let about = draft.about.trim().to_owned();
    if !about.is_empty() && !about.starts_with("nt-") {
        return Err(NoteError::BadAbout(about));
    }
    let withdraws = draft.withdraws.trim().to_owned();
    if !withdraws.is_empty() && !withdraws.starts_with("nt-") {
        return Err(NoteError::BadAbout(withdraws));
    }
    let written_utc = ledger_clock(written_utc_input);
    let episode = target.episode.as_ref();
    let members: Vec<String> = episode.map_or_else(Vec::new, |facts| {
        facts
            .members
            .iter()
            .map(|oid| oid.chars().take(8).collect())
            .collect()
    });
    let mut note = Note {
        id: note_id(
            kind,
            episode.map_or("", |facts| facts.id.as_str()),
            author,
            written_utc,
            &text,
        ),
        file: String::new(),
        kind,
        episode: episode.map_or(String::new(), |facts| facts.id.clone()),
        author: author.clone(),
        written_utc,
        text,
        path: target.path.clone(),
        line: target.line,
        about,
        withdraws,
        via: draft.via.to_owned(),
        fingerprint: episode.map_or(String::new(), |facts| facts.fingerprint.clone()),
        fallback: episode.map_or(String::new(), |facts| facts.fallback_fingerprint.clone()),
        shape: episode.map_or(String::new(), |facts| facts.shape_fingerprint.clone()),
        content: episode.map_or(String::new(), |facts| facts.content.digest.clone()),
        members,
        paths: episode.map_or_else(Vec::new, EpisodeFacts::paths),
        body: Vec::new(),
        digest: String::new(),
    };
    note.body = note.canonical_body();
    note.digest = note_digest(&note.id, &note.body);
    Ok(note)
}

/// The hash framing every note digest starts with, then the id, then the field lines.
#[must_use]
pub fn note_digest(id: &str, body: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(NOTE_SCHEMA.as_bytes());
    hasher.update([0]);
    hasher.update(id.as_bytes());
    hasher.update([0]);
    for (key, value) in body {
        hasher.update(key.as_bytes());
        hasher.update(b": ");
        hasher.update(value.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", hex_digest(&hasher.finalize()))
}

/// A note's id, derived from the four things that make a note *this* note plus what it says.
///
/// Content-addressed on purpose: `about` is in the hash of the answering note, not the answered one, so
/// closing a question never re-addresses the question.
#[must_use]
pub fn note_id(kind: Kind, episode: &str, author: &Actor, written_utc: i64, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(NOTE_SCHEMA.as_bytes());
    hasher.update([0]);
    hasher.update(kind.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(episode.as_bytes());
    hasher.update([0]);
    hasher.update(author.display.as_bytes());
    hasher.update([0]);
    hasher.update(written_utc.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(text.as_bytes());
    format!("nt-{}", &hex_digest(&hasher.finalize())[..16])
}

/// Each note's [`Status`], in the order the notes were passed in.
///
/// The verdict is a function of the whole set, which is why it is a function and not a field:
///
/// A note is **withdrawn** if a *later* note names it in `withdraws:`; else it is **answered** if a
/// *later* note names it in `about:`; else it is **open**.
///
/// "Later" is position in the slice, which is file order, because the file appends. An answer that landed in
/// the file *before* its question — two branches, one asking and one answering, merged in either order — is
/// not treated as closing it: reading that as an answer would be reading causality backwards, and the
/// honest state of a question nobody has answered after the merge is `open`.
///
/// Withdrawal outranks an answer, and the reason is the shape of the file: if somebody retracted a decision,
/// an earlier attempt to answer it is not evidence that the decision stands.
#[must_use]
pub fn statuses(notes: &[Note]) -> Vec<Status> {
    let index: std::collections::BTreeMap<&str, usize> = notes
        .iter()
        .enumerate()
        .map(|(i, note)| (note.id.as_str(), i))
        .collect();
    let mut answered = vec![false; notes.len()];
    let mut withdrawn = vec![false; notes.len()];
    for (later, note) in notes.iter().enumerate() {
        for (reference, slot) in [
            (&note.about, &mut answered),
            (&note.withdraws, &mut withdrawn),
        ] {
            if reference.is_empty() {
                continue;
            }
            // A reference to an id that is not in this set is a dangling pointer in the file, and
            // `fathom note verify` is where that gets named; here it simply closes nothing.
            if let Some(earlier) = index.get(reference.as_str())
                && *earlier < later
            {
                slot[*earlier] = true;
            }
        }
    }
    notes
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if withdrawn[i] {
                Status::Withdrawn
            } else if answered[i] {
                Status::Answered
            } else {
                Status::Open
            }
        })
        .collect()
}

/// The questions and follow-ups nothing has answered — [`statuses`]' result narrowed to what a reviewer owes.
#[must_use]
pub fn open_questions<'a>(notes: &'a [Note], statuses: &[Status]) -> Vec<&'a Note> {
    notes
        .iter()
        .zip(statuses)
        .filter(|(note, status)| note.kind.wants_an_answer() && **status == Status::Open)
        .map(|(note, _)| note)
        .collect()
}

/// The notes that answer or follow `id`, in file order.
#[must_use]
pub fn answers<'a>(notes: &'a [Note], id: &str) -> Vec<&'a Note> {
    notes.iter().filter(|note| note.about == id).collect()
}

// The body is folded to one line by the ledger's `one_line`, above: same marker (` ⏎ `) and same whitespace
// collapse, so a note and a sign-off's `note:` field written from the same paragraph come out identical.

/// The hex of a finished digest.
fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// `unmeasured (…)` for a digest the index could not compute, so the file says which one and why that entry
/// cannot be re-found by it.
fn or_unmeasured(value: &str, which: &str) -> String {
    if value.is_empty() {
        format!("unmeasured ({which} not known at write time)")
    } else {
        value.to_owned()
    }
}

/// The printed member list: eight hex each, comma-separated — the ledger's own spelling ([`join8`] there
/// prints the same prefixes), and the ladder matches on prefixes for the same reason it does there.
fn join8(members: &[String]) -> String {
    if members.is_empty() {
        return "-".to_owned();
    }
    members
        .iter()
        .map(|oid| oid.chars().take(8).collect::<String>())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `paths:` field, in the ledger's spelling (`src/a.rs@1a2b3c4d5e6f`).
fn render_paths(paths: &[PathFact]) -> String {
    if paths.is_empty() {
        return "-".to_owned();
    }
    paths
        .iter()
        .map(PathFact::display)
        .collect::<Vec<_>>()
        .join(", ")
}

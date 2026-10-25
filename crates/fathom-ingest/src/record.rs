//! `record.rs` — the normalized commit record (e1.2 / reco-76g).
//!
//! This is the shape a git commit becomes on its way into Fathom: one value per commit, holding the
//! identity ([`CommitRecord::oid`], its parents, its tree), the two people and their clocks
//! ([`CommitRecord::author`], [`CommitRecord::committer`], each with its UTC offset), the message
//! facts ([`MessageFacts`]), its trailers ([`Trailers`]), one [`FileRecord`] per path it touched, and
//! the totals a surface reads without summing anything. Nothing in here is a `gix` type (ADR-002 §4.2),
//! nothing in here is a database row, and — the part that matters for the snapshot tests — **the value
//! is stable**: the same commit normalizes to the same bytes on every machine, in every run, whatever
//! order the commits were walked in.
//!
//! Two rules hold across the whole module.
//!
//! **Every field here is either durable or explainably not.** The durable projection is ADR-004 §4.2's
//! two tables, `git_commit` and `git_commit_file`, and [`crate::store`] writes exactly those columns —
//! it adds no column and invents no state. Some facts in this module have no column there: the UTC
//! offsets, git's rename score, a file's blob sizes, and *why* a file was not measured. They are kept
//! anyway because a normalized record is the input to a decision (e1.3 segments on author identity,
//! e2.4 re-scores renames, e1.7 renders a hunk list) and because the canonical export
//! ([`crate::export`]) is defined over this type, not over a partial one. Each non-durable field says
//! so where it is defined, so a later ticket that wants it durable adds a column rather than
//! discovering it was silently dropped.
//!
//! **Absence is a state, not a zero.** A file whose line counts were never computed — because the blob
//! is over [`Limits::blob_memory_cap`], or because it is binary, or because the object is not in this
//! clone — does not carry `0` and pretend it was measured. [`Content`] names the reason, and the
//! totals say which numbers were summed and which were not. A review tool that prints "0 lines
//! changed" about a 900 MB blob has told a comfortable lie, and the whole thesis of this product is
//! that it does not do that (ADR-001 §7.6).
//!
//! # What is deliberately not here
//!
//! * The message body. It stays in git: the store keeps the first line and a digest
//!   ([`MessageFacts`]), and a consumer that needs the whole message re-reads the commit object, which
//!   is one lookup in a repository this crate already has open. Copying every message body into every
//!   record is how an index of a 100k-commit history grows a second copy of its own commit log.
//! * Diff text. Hunks are recorded as their headers ([`Hunk`]) plus counts; the bytes are rendered on
//!   demand by `e1.7` (ADR-004 §8.1's memory rule: the spine stores facts about changes, not the
//!   changes).
//! * Path taxonomy *decisions*. [`Taxonomy`] is a lookup this crate calls and does not own; `e0.9`'s
//!   engine and `e2.1`'s classifier produce the answer, and ADR-004 §6.6 R3 forbids re-deriving it
//!   downstream. [`Unclassified`] is the honest default for a build that has no policy loaded yet.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::Unavailable;
use crate::graph::Signature;
use crate::oid::Oid;
use crate::paths::RepoPath;

// ---------------------------------------------------------------------------
// limits
// ---------------------------------------------------------------------------

/// The size caps that keep ingestion inside its memory budget.
///
/// These are not performance tuning; they are the difference between "a repository with a 500 MB file
/// indexes slowly" and "fathom is swapped out and the terminal is unusable". The default blob cap is
/// `benchmarks/budgets.json`'s `ingest.file_size_cap.mib` — the same number ADR-004 §11 publishes for
/// every size class, so a cap a user can read is also the cap CI can fail.
///
/// A cap is a *decision to not look*, and a decision not to look has to be recorded, which is what
/// [`Content::NotExamined`] is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    /// Largest blob that enters memory, in bytes. One blob over the cap and that file is recorded as
    /// [`Content::NotExamined`] with [`NotExamined::Oversized`]: its tree entry is still read, its mode
    /// and path are still recorded, and nothing is inflated.
    ///
    /// `8 MiB`, from `ingest.file_size_cap.mib`. Note this is a per-*blob* cap: a commit that touches a
    /// thousand 7 MiB files still costs a thousand blobs' worth of transient memory, which is why the
    /// diff resource cache is cleared per file rather than per commit ([`crate::normalize`]).
    pub blob_memory_cap: u64,
    /// Bytes of the first line kept in [`MessageFacts::head`] — ADR-004 §4.2's `message_head` is
    /// "first line, truncated at 512 B". Truncated on *bytes*, at a UTF-8 boundary, because a store
    /// column is a byte budget and a record that split a grapheme is a record that cannot be printed.
    pub message_head_bytes: usize,
    /// Per-file cap on stored hunk headers, the "capped 256 entries" of ADR-004 §4.2's
    /// `hunk_headers` column. A generated file with 40 000 hunks does not need 40 000 rows of `@@`; it
    /// needs the count, which [`FileRecord::hunk_total`] carries.
    pub hunk_headers_per_file: usize,
    /// Lines of context a hunk header is measured over — git's `-U`, and its default of `3`. It is here
    /// rather than hardcoded because a hunk header is a claim about *which lines a reviewer would see*, and
    /// changing the context changes that claim; the export header echoes it for the same reason
    /// ([`crate::export::Normalization`]).
    pub hunk_context_lines: u32,
    /// Per-commit cap on [`CommitRecord::files`]. A commit that touches more paths than this records
    /// the first N *in git's own tree order* (which is byte order over names, so it is deterministic)
    /// and sets [`CommitRecord::files_truncated`] — a stated partial, not a silent one. The budget
    /// `store.bytes_per_commit_spine` (ADR-004 §11.6) is arithmetic over rows per commit, and one
    /// vendoring commit with 200 000 paths would otherwise be the outlier that eats it.
    pub files_per_commit: usize,
    /// Renames and copies are detected by content similarity; this is git's `-M<n>%` threshold as a
    /// percentage, `50` matching git's default and `100` meaning "identical content only".
    pub rename_threshold: u8,
    /// How many pairs the fuzzy rename search may test — git's `-l<n>`, and the reason a commit that
    /// renames 5 000 files does not cost 25 M comparisons.
    pub rename_pair_limit: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            // `ingest.file_size_cap.mib` = 8 for every size class (benchmarks/budgets.json).
            blob_memory_cap: 8 * 1024 * 1024,
            message_head_bytes: 512,
            hunk_headers_per_file: 256,
            hunk_context_lines: 3,
            files_per_commit: 20_000,
            rename_threshold: 50,
            rename_pair_limit: 1_000,
        }
    }
}

impl Limits {
    /// The documented defaults, named so a caller does not have to write `::default()` where the
    /// meaning is the point.
    #[must_use]
    pub fn standard() -> Self {
        Self::default()
    }
}

// ---------------------------------------------------------------------------
// message and trailers
// ---------------------------------------------------------------------------

/// What the spine keeps about a commit message: the first line and a digest.
///
/// The digest is SHA-256 over the raw message bytes, which is what makes "same message" a question with
/// an answer after a rebase has changed the commit id and the timestamps: `message_hash` is the part of
/// a commit's identity that survives a rewrite, and it is why ADR-004 §4.2 stores a hash instead of a
/// second copy of the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageFacts {
    /// The first line, truncated to [`Limits::message_head_bytes`] at a character boundary. Empty for
    /// an empty message; git's own `--oneline` of an empty message is an empty subject too.
    pub head: String,
    /// Lowercase hex SHA-256 of the message body as git stores it — the bytes below the header block,
    /// which is the part a human or an agent wrote, and the part that survives a rebase unchanged.
    pub hash: String,
    /// Bytes in the raw message body, so "the message is empty" and "the message is 4 KB" are both
    /// answers. Not a store column: it is cheap to recompute and nothing queries on it.
    pub bytes: usize,
}

impl MessageFacts {
    /// Digest a message body into the durable facts, applying the head cap.
    #[must_use]
    pub fn new(raw: &[u8], head_cap: usize) -> Self {
        let head = first_line(raw)
            .map(|line| crate::paths::to_str_lossy(line).into_owned())
            .unwrap_or_default();
        MessageFacts {
            head: truncate_bytes(&head, head_cap),
            hash: sha256_hex(raw),
            bytes: raw.len(),
        }
    }
}

/// The trailer block of a commit message, keys lowercased, last value wins.
///
/// git's rules ([`git-interpret-trailers(1)`](https://git-scm.com/docs/git-interpret-trailers)) are the
/// ones a user has already learned, so they are the ones implemented here: the trailer block is the
/// *last paragraph*, it is a block only when a blank line separates it from what comes before, every
/// line of it must look like `Key: value`, a key is case-insensitive and may contain hyphens, a line
/// that starts with whitespace continues the previous value, and a repeated key keeps its last value.
/// `CommitHeader::trailer` in [`crate::graph`] answers one key; this is the whole map, which is what
/// `e1.3` (boundary rules), `e4.1` (attribution) and the store's `trailers` column all need, from one
/// parse.
///
/// The separating blank line is the rule that keeps a subject out of the map (`reco-2ss`). A message
/// that is one paragraph has no trailer block at all — it *is* the subject — and a conventional-commit
/// subject is shaped exactly like a trailer, so reading trailers out of the first paragraph invents one
/// for every `fix: …`, `chore: …` and `feat: …` commit in the repository. What a reviewer then reads in
/// the `trailers` column is the subject wearing a trailer's clothes, and `e1.3`'s boundary rules and
/// `e4.1`'s attribution read it the same way a human does.
///
/// Two edges where this is *stricter* than git, written down so "git's rules" above is a claim with its
/// edges drawn: a paragraph that mixes trailer lines with prose yields no block here, where git prints
/// the trailer lines it found, and `Key:value` with no space after the colon is not a trailer here.
/// Neither can invent a trailer, which is the failure mode this parser is judged on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trailers(BTreeMap<String, String>);

impl Trailers {
    /// Parse the trailer block out of a raw message body.
    #[must_use]
    pub fn parse(message: &[u8]) -> Self {
        let text = String::from_utf8_lossy(message);
        // `lines()` splits on LF and drops the carriage return a CRLF message leaves behind, so the
        // paragraph scan below does not have to know which line ending the platform wrote.
        let mut lines: Vec<&str> = text.lines().collect();
        while matches!(lines.last(), Some(line) if line.trim().is_empty()) {
            lines.pop();
        }
        // The trailer block is the paragraph after the last blank line — so a message with no blank
        // line has no trailer block, which is git's rule and the whole of this defect: `git
        // interpret-trailers --parse` reads nothing out of a bare `chore: gitignore foo`, and neither
        // does this. A run of spaces counts as a blank line, as it does in git.
        let Some(index) = lines.iter().rposition(|line| line.trim().is_empty()) else {
            return Trailers::default();
        };
        let mut out: BTreeMap<String, String> = BTreeMap::new();
        let mut current: Option<String> = None;
        // The range after that blank line holds no blank line itself, which is what "one paragraph"
        // means here; every line of it has to be a trailer line or a continuation of one.
        for line in lines[index + 1..].iter().copied() {
            if line.starts_with([' ', '\t']) {
                // Continuation of the previous trailer's value, git's rule.
                if let Some(key) = current.as_ref()
                    && let Some(value) = out.get_mut(key)
                {
                    value.push(' ');
                    value.push_str(line.trim());
                } else {
                    // A continuation with nothing to continue is not a trailer block at all.
                    return Trailers::default();
                }
                continue;
            }
            let Some((key, value)) = split_trailer_line(line) else {
                // The first non-trailer line means this paragraph is prose, not trailers. git's own
                // `interpret-trailers` then reports no trailers, and so does this.
                return Trailers::default();
            };
            let key = key.to_ascii_lowercase();
            out.insert(key.clone(), value.trim().to_owned());
            current = Some(key);
        }
        Trailers(out)
    }

    /// The value of one key, case-insensitively, or `None`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(&key.to_ascii_lowercase()).map(String::as_str)
    }

    /// Every trailer, keys ascending — the order the store's JSON object and the canonical export use.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// The bytes the trailer strings own, for [`CommitRecord::heap_bytes`].
    #[must_use]
    pub(crate) fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .0
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>()
    }

    /// Are there no trailers?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The map the store's `trailers` column holds: key to last value, already lowercased and already
    /// ordered (a `BTreeMap` inside, so the JSON a run writes is byte-stable).
    #[must_use]
    pub fn to_map(&self) -> BTreeMap<String, String> {
        self.0.clone()
    }

    /// How many distinct keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The map as JSON text, which is what ADR-004 §4.2's `trailers` column holds (`{}` when empty).
    #[must_use]
    pub fn to_json(&self) -> String {
        // `serde_json::to_string` over a `BTreeMap` is canonical: key order is the map's order, and
        // there is one way to write a string in JSON.
        serde_json::to_string(&self.0).unwrap_or_else(|_| "{}".to_owned())
    }
}

impl fmt::Display for Trailers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (key, value)) in self.iter().enumerate() {
            if index > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{key}: {value}")?;
        }
        Ok(())
    }
}

impl Serialize for Trailers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Trailers {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = BTreeMap::<String, String>::deserialize(deserializer)?;
        // Round-tripping must not resurrect mixed case: `parse` lowercases, so a reader that was handed
        // `{"Co-Authored-By": …}` from somewhere else lowercases too, or the two producers of a record
        // would disagree in the golden file.
        Ok(Trailers(
            map.into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
        ))
    }
}

/// Split one `Key: value` line the way git's trailer grammar requires: the separator is `: ` or `:` at
/// the end of the line, and a key is a run of printable characters with no space and no colon.
fn split_trailer_line(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    if key.is_empty() || key.contains([' ', '\t']) {
        return None;
    }
    if !key.chars().all(|c| !c.is_control()) {
        return None;
    }
    // git requires the separator to be a colon followed by a space, or a colon at end of line.
    if !value.is_empty() && !value.starts_with(' ') {
        return None;
    }
    Some((key, value))
}

// ---------------------------------------------------------------------------
// hunks
// ---------------------------------------------------------------------------

/// One hunk's location, kept as git writes it in a `@@` header.
///
/// The header *text* is the canonical form (it is what ADR-004 §4.2's `hunk_headers` column stores, and
/// what a snapshot test compares), so it is what this type serialises to; the four numbers are the
/// parsed view a caller wants when it is sorting hunks rather than printing them.
///
/// git's formatting rules are followed exactly, including the two easy to forget: a count of one is
/// written without `,1`, and an empty side is `0,0` rather than `1,0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hunk {
    /// First line removed, 1-based; `0` when nothing was removed.
    pub old_start: u32,
    /// How many lines were removed.
    pub old_count: u32,
    /// First line added, 1-based; `0` when nothing was added.
    pub new_start: u32,
    /// How many lines were added.
    pub new_count: u32,
}

impl Hunk {
    /// Build a hunk from 1-based starts and counts.
    #[must_use]
    pub const fn new(old_start: u32, old_count: u32, new_start: u32, new_count: u32) -> Self {
        Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
        }
    }

    /// The `@@ -a,b +c,d @@` line, with git's omission of `,1`.
    #[must_use]
    pub fn header(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(24);
        let _ = write!(out, "@@ -{}", side(self.old_start, self.old_count));
        let _ = write!(out, " +{}", side(self.new_start, self.new_count));
        out.push_str(" @@");
        out
    }

    /// Parse a header back. `None` for anything that is not a well-formed header, because a store row
    /// written by an older build is data to report on, not a reason to fail.
    #[must_use]
    pub fn parse(header: &str) -> Option<Hunk> {
        let rest = header.strip_prefix("@@ -")?.strip_suffix(" @@")?;
        let (old, new) = rest.split_once(" +")?;
        let (old_start, old_count) = parse_side(old)?;
        let (new_start, new_count) = parse_side(new)?;
        Some(Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
        })
    }
}

/// One side of a hunk header: `start`, or `start,count` when the count is not one.
fn side(start: u32, count: u32) -> String {
    if count == 1 {
        format!("{start}")
    } else if count == 0 {
        // git prints `0,0` for an empty side: a reader must be able to tell "nothing here" from
        // "one line at line 0", and `0` alone would collide with the 1-based first line.
        "0,0".to_owned()
    } else {
        format!("{start},{count}")
    }
}

fn parse_side(text: &str) -> Option<(u32, u32)> {
    match text.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((text.parse().ok()?, 1)),
    }
}

impl Serialize for Hunk {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.header())
    }
}

impl<'de> Deserialize<'de> for Hunk {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Hunk::parse(&text).ok_or_else(|| D::Error::custom(format!("not a hunk header: {text:?}")))
    }
}

// ---------------------------------------------------------------------------
// per-file records
// ---------------------------------------------------------------------------

/// What happened to one path.
///
/// The six values of ADR-004 §4.2's `change_kind` CHECK constraint are represented by five of these:
/// `add`, `modify`, `delete`, `rename`, `copy`. The sixth, `binary`, is deliberately never written by
/// this pipeline — binary-ness is a property of content ([`Content`]) and not a kind of change, so a
/// binary modification stays a `modify` with [`Content::Binary`] rather than losing the fact that it
/// was a modification. The value stays in the constraint because a store written by a future build may
/// choose to use it, and refusing it at read time would make that store unreadable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    /// The path did not exist before.
    Add,
    /// The path existed and its content or mode changed. A mode-only change is a `modify` — git's own
    /// `--raw` letter for it is `M` as well.
    Modify,
    /// The path is gone.
    Delete,
    /// git paired a deleted path with this added path by content similarity.
    Rename,
    /// git paired an existing path with this added path without deleting the source (a copy).
    Copy,
}

impl ChangeKind {
    /// The store's and the export's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Add => "add",
            ChangeKind::Modify => "modify",
            ChangeKind::Delete => "delete",
            ChangeKind::Rename => "rename",
            ChangeKind::Copy => "copy",
        }
    }

    /// The reverse, `None` for a value outside the CHECK list.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "add" => ChangeKind::Add,
            "modify" => ChangeKind::Modify,
            "delete" => ChangeKind::Delete,
            "rename" => ChangeKind::Rename,
            "copy" => ChangeKind::Copy,
            _ => return None,
        })
    }

    /// Does this kind mean the path is new?
    #[must_use]
    pub const fn is_addition(self) -> bool {
        matches!(self, ChangeKind::Add | ChangeKind::Copy)
    }
}

impl fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a file's content was not examined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum NotExamined {
    /// The blob is larger than [`Limits::blob_memory_cap`] (the 500 MB case the ticket names). Its
    /// tree entry is still read, so the path, mode and blob id are known; nothing is inflated.
    Oversized {
        /// The blob's decompressed size, from its object header — a header read costs no inflation.
        size: u64,
    },
    /// The blob is not in this clone (behind a shallow graft, or omitted by a clone filter), with the
    /// same classification [`crate::error::Unavailable`] gives a walk.
    Unavailable {
        /// Why the object is missing, as far as the repository can say.
        why: Unavailable,
    },
    /// A `.gitattributes` diff driver asked for an external command to produce the text form (a
    /// `textconv` for a database file, say). Running a program the repository names is out of the
    /// question for a review tool — ADR-001 §8.2's spirit, and `tests/no_git_subprocess.rs`'s letter —
    /// so the content is recorded as unexamined and the driver is named.
    ExternalDiffDriver {
        /// The driver name from `.gitattributes`.
        driver: String,
    },
    /// The tree entry is not a blob: a gitlink (a submodule pointer, mode `0o160000`), or a tree where a
    /// blob was expected. There is no line content to diff, and saying so beats reading a commit object
    /// as if it were source.
    NotABlob {
        /// What the entry actually points at, e.g. `commit`.
        found: String,
    },
}

impl NotExamined {
    /// The human-facing reason, for a status line and for `fathom doctor`.
    #[must_use]
    pub fn explain(&self) -> String {
        match self {
            NotExamined::Oversized { size } => format!("{size} bytes, over the ingest memory cap"),
            NotExamined::Unavailable { why } => why.to_string(),
            NotExamined::NotABlob { found } => {
                format!("the entry points at a {found} object, not a blob")
            }
            NotExamined::ExternalDiffDriver { driver } => {
                format!("text is produced by the external diff driver {driver:?}")
            }
        }
    }
}

/// What was learned about a path's content.
///
/// This is the type that keeps "not measured" from becoming "zero", and the reason it is an enum rather
/// than a `bool` is the acceptance criterion: a 500 MB file *must* ingest, which means the pipeline must
/// be able to say what it did not do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Content {
    /// Text that was diffed: the counts and hunks in [`FileRecord`] are measurements.
    Text,
    /// Binary by git's own rule (a NUL in the first 8 KiB of either side, or a `binary` attribute).
    /// git's `--numstat` prints `-` for these; so does this, by writing 0 and saying why.
    Binary,
    /// Not looked at, with the reason.
    #[serde(rename = "unexamined")]
    NotExamined {
        /// Why not.
        reason: NotExamined,
    },
}

impl Content {
    /// The store's `is_binary` column: `true` only for content known to be binary. An unexamined file
    /// is *not* recorded as binary — that would be a different lie — and the durable place the "we did
    /// not look" fact lives is `git_commit.oversized` plus, in this build, the run report.
    #[must_use]
    pub const fn is_binary(&self) -> bool {
        matches!(self, Content::Binary)
    }

    /// Were line counts measured?
    #[must_use]
    pub const fn measured(&self) -> bool {
        matches!(self, Content::Text)
    }
}

/// git's own rename/copy pairing for one path — a *candidate*, and named that way on purpose.
///
/// `e2.4` (reco-a0v) owns rename confidence with content similarity of its own, because git's heuristic
/// misses renames-with-rewrites, splits and anything across a merge. This is the raw answer the tree
/// diff produced, kept so a later ticket can agree with it, disagree with it, or replace it — and so
/// that "which rename detector said this?" is a question with an answer in the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameCandidate {
    /// The source path (the deleted path for a rename, the surviving one for a copy).
    pub source: RepoPath,
    /// The blob the source pointed at, when it is known.
    pub source_blob: Option<Oid>,
    /// git's similarity score for the pair, 0..=100. `None` when the pairing came from identical blob
    /// ids, which needs no scoring, or when the score was not computed.
    ///
    /// Not durable: ADR-004 §4.2 has no column for it, and `e2.4` will store its own confidence rather
    /// than borrow this one.
    pub score: Option<u8>,
    /// Whether the pairing is a copy (source survives) rather than a rename.
    pub copy: bool,
}

/// The taxonomy decision for one path: ADR-004 §4.2's three taxonomy columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathTaxonomy {
    /// The class (`source`, `test`, `docs`, `lockfile`, …), verbatim from the policy engine.
    pub path_class: String,
    /// The noise class, which is a *different* question and the reason there are two columns
    /// (`e0.9`: "may I fold it" versus "what is it").
    pub noise: NoiseClass,
    /// Which version of the built-in table decided this, so a bump is visible in the row rather than
    /// only in the config. `0` means "no engine was consulted" — see [`Unclassified`].
    pub taxonomy_version: u32,
}

impl PathTaxonomy {
    /// The unclassified decision: every path is `unknown` at version `0`.
    #[must_use]
    pub fn unclassified() -> Self {
        PathTaxonomy {
            path_class: "unknown".to_owned(),
            noise: NoiseClass::Unknown,
            taxonomy_version: 0,
        }
    }
}

impl Default for PathTaxonomy {
    fn default() -> Self {
        Self::unclassified()
    }
}

/// ADR-004 §4.3's six noise classes — the store's CHECK list, so a parse failure here is a store that
/// contains something outside its own constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoiseClass {
    /// Worth reviewing.
    Informative,
    /// Generated by a build or a code generator.
    Generated,
    /// A dependency lockfile.
    Lockfile,
    /// Whitespace or formatter churn.
    Reformat,
    /// Vendored third-party code.
    Vendored,
    /// No policy was consulted, or it declined to answer.
    Unknown,
}

impl NoiseClass {
    /// Did no policy answer?
    #[must_use]
    pub const fn is_unknown(self) -> bool {
        matches!(self, NoiseClass::Unknown)
    }

    /// The store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            NoiseClass::Informative => "informative",
            NoiseClass::Generated => "generated",
            NoiseClass::Lockfile => "lockfile",
            NoiseClass::Reformat => "reformat",
            NoiseClass::Vendored => "vendored",
            NoiseClass::Unknown => "unknown",
        }
    }

    /// The reverse.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "informative" => NoiseClass::Informative,
            "generated" => NoiseClass::Generated,
            "lockfile" => NoiseClass::Lockfile,
            "reformat" => NoiseClass::Reformat,
            "vendored" => NoiseClass::Vendored,
            "unknown" => NoiseClass::Unknown,
            _ => return None,
        })
    }
}

impl fmt::Display for NoiseClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The taxonomy lookup the pipeline calls.
///
/// A trait rather than a data table because the engine that answers is not this crate's: `e0.9`'s policy
/// (built-ins plus `fathom.toml`) is the owner, `e2.1` layers content evidence on top, and ADR-004
/// §6.6 R3's rule — a stored `path_class` is never re-derived downstream — only means something if the
/// decision is made once, at the door, by whoever owns it. Until a Rust policy engine is wired in,
/// [`Unclassified`] is the default and every row says `unknown` at version `0`, which `v_unknowns`
/// (ADR-004 §4.7) makes visible instead of pretending is informative.
pub trait Taxonomy: Send + Sync {
    /// The decision for one path.
    fn classify(&self, path: &RepoPath) -> PathTaxonomy;

    /// The version of the table in force for this run, reported once per run as well as per row.
    fn version(&self) -> u32 {
        0
    }
}

/// The [`Taxonomy`] that admits it knows nothing: every path `unknown`, version `0`.
///
/// This is the default, and it is not a placeholder. An ingestion run that has not been given a policy
/// must still produce every row (the store is complete) and must not invent a class: a path quietly
/// bucketed as `source` by a heuristic nobody wrote is a path that will be reviewed as code by
/// everything downstream.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unclassified;

impl Taxonomy for Unclassified {
    fn classify(&self, _path: &RepoPath) -> PathTaxonomy {
        PathTaxonomy::unclassified()
    }

    fn version(&self) -> u32 {
        0
    }
}

/// One path touched by one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    /// What happened to the path.
    pub change: ChangeKind,
    /// The path as it exists after the commit (for a delete: as it existed before).
    pub path: RepoPath,
    /// The source path of a rename or copy; `None` otherwise. This is the durable half of
    /// [`RenameCandidate`] — ADR-004 §4.2's `old_path`.
    pub old_path: Option<RepoPath>,
    /// The mode before, `0o100644`-style, `None` when there was no previous entry.
    pub mode_old: Option<u32>,
    /// The mode after, `None` when the path is gone.
    pub mode_new: Option<u32>,
    /// The blob before, when known and present. Not durable: §4.2's `blob_old`/`blob_new` columns
    /// address a *retained payload* (sha256), and retaining payloads is §4.5's `blob` table, which is
    /// not this ticket.
    pub blob_old: Option<Oid>,
    /// The blob after, when known and present.
    pub blob_new: Option<Oid>,
    /// What was learned about the content, and therefore whether the counts below are measurements.
    pub content: Content,
    /// Lines added. `0` unless [`Content::measured`], exactly as `git log --numstat` prints `-`.
    pub added: u32,
    /// Lines removed. See [`FileRecord::added`].
    pub removed: u32,
    /// The hunk headers, in file order, capped at [`Limits::hunk_headers_per_file`].
    pub hunks: Vec<Hunk>,
    /// Total hunks seen before the cap, so a 40 000-hunk generated file still says how big it was.
    pub hunk_total: usize,
    /// The raw rename/copy pairing, if there was one.
    pub rename: Option<RenameCandidate>,
    /// Byte size of the before-blob, from its object header (no inflation).
    pub size_old: Option<u64>,
    /// Byte size of the after-blob, from its object header.
    pub size_new: Option<u64>,
    /// The taxonomy decision made once, at the door.
    pub taxonomy: PathTaxonomy,
}

impl FileRecord {
    /// A record for a path whose content was not examined, with the reason.
    #[must_use]
    pub fn unexamined(change: ChangeKind, path: RepoPath, reason: NotExamined) -> Self {
        FileRecord {
            change,
            path,
            old_path: None,
            mode_old: None,
            mode_new: None,
            blob_old: None,
            blob_new: None,
            content: Content::NotExamined { reason },
            added: 0,
            removed: 0,
            hunks: Vec::new(),
            hunk_total: 0,
            rename: None,
            size_old: None,
            size_new: None,
            taxonomy: PathTaxonomy::unclassified(),
        }
    }

    /// The rename source path, if this row came from a rename or copy.
    #[must_use]
    pub fn rename_source(&self) -> Option<&RepoPath> {
        self.rename.as_ref().map(|candidate| &candidate.source)
    }

    /// The bytes this row holds on the heap, estimated the same way
    /// [`CommitRecord::heap_bytes`] is — owned buffers at their own length, no payload, no allocator
    /// overhead.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.path.as_bytes().len()
            + self
                .old_path
                .as_ref()
                .map_or(0, |path| path.as_bytes().len())
            + self.hunks.len() * std::mem::size_of::<Hunk>()
    }
}

// ---------------------------------------------------------------------------
// the commit record
// ---------------------------------------------------------------------------

/// The sums a surface reads without recomputing anything.
///
/// These are store columns (`n_files`, `n_add`, `n_del`, `n_binary`, `oversized`) because a timeline row
/// and a risk score both need them, and because a derived number that is never stored is a number every
/// surface derives slightly differently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    /// Paths touched — [`CommitRecord::files`]'s length, after any per-commit cap.
    pub files: usize,
    /// Lines added across every measured file.
    pub added: u64,
    /// Lines removed across every measured file.
    pub removed: u64,
    /// Files recorded as [`Content::Binary`].
    pub binary: usize,
    /// Files recorded as unexamined.
    pub unexamined: usize,
    /// Hunks seen, before any per-file cap.
    pub hunks: usize,
    /// Files whose rename/copy pairing came from git's tree diff.
    pub renames: usize,
}

impl Totals {
    /// Nothing at all was seen — an empty commit, or a merge under `MergePolicy::Skip`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Totals::default()
    }

    /// Fold another set of totals into these — how [`crate::ingest`] accumulates a run.
    pub fn add(&mut self, other: &Totals) {
        self.files += other.files;
        self.added += other.added;
        self.removed += other.removed;
        self.binary += other.binary;
        self.unexamined += other.unexamined;
        self.hunks += other.hunks;
        self.renames += other.renames;
    }
}

/// One commit, normalized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRecord {
    /// The commit's own id — the SHA the ticket names first, and the only identity a rebase preserves
    /// nothing of. (What survives a rebase is [`MessageFacts::hash`] plus parentage; that is `e1.3`'s
    /// and ADR-003's problem, not this record's.)
    pub oid: Oid,
    /// The root tree this commit introduces.
    pub tree: Oid,
    /// Parents in the commit's own order; the first is the first parent. Empty for a root commit, two
    /// or more for a merge.
    pub parents: Vec<Oid>,
    /// Who wrote it, with their UTC offset.
    pub author: Signature,
    /// Who applied it, with their UTC offset. Different from the author after a rebase, and the reason
    /// both are kept is ADR-003's boundary rules, not pedantry.
    pub committer: Signature,
    /// First line and digest of the message.
    pub message: MessageFacts,
    /// The trailer block.
    pub trailers: Trailers,
    /// One row per path touched, in git's tree-diff order (deterministic: it is the order the tree walk
    /// emits, not the order threads finished).
    pub files: Vec<FileRecord>,
    /// The sums.
    pub totals: Totals,
    /// More paths were touched than [`Limits::files_per_commit`]; [`CommitRecord::totals`] describes
    /// only the rows that were kept. A stated partial, which is the only acceptable kind.
    pub files_truncated: bool,
    /// The first parent's tree was the diff base. Recorded because "what did this merge change" has two
    /// answers and a reviewer is entitled to know which one the numbers came from.
    pub diff_base: DiffBase,
}

impl CommitRecord {
    /// Is this a merge (more than one parent)?
    #[must_use]
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// Does any file row carry no content measurement because of a size cap? The durable answer to
    /// "were all the numbers in this row measured?", and the mapping onto `git_commit.oversized`.
    #[must_use]
    pub fn oversized(&self) -> bool {
        self.files.iter().any(|file| {
            matches!(
                file.content,
                Content::NotExamined {
                    reason: NotExamined::Oversized { .. },
                }
            )
        })
    }

    /// The sum of added and removed lines, which is the "size" a risk score starts from.
    #[must_use]
    pub fn churn(&self) -> u64 {
        self.totals.added + self.totals.removed
    }

    /// The bytes this record holds on the heap, estimated — the number the write queue bounds itself
    /// with (`reco-93w`).
    ///
    /// "Estimated" in a specific sense: every owned buffer the record can be asked to hold is counted at
    /// its own length (paths, the message head, trailer strings, one `Hunk` per header), plus
    /// `size_of` for the fixed parts. Allocator overhead, capacity slack and shared data are not, and
    /// no *payload* is counted because a record never owns one — a blob's bytes are in git, and a
    /// hunk is a header, not the hunk's text. So this under-reads reality a little and never by orders
    /// of magnitude, which is the right direction for a queue ceiling: it is the quantity that decides
    /// when a producer waits, and waiting a batch late costs nothing.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        let signature = |who: &crate::graph::Signature| {
            std::mem::size_of_val(who) + who.name.len() + who.email.len()
        };
        std::mem::size_of::<Self>()
            + self.parents.len() * std::mem::size_of::<Oid>()
            + signature(&self.author)
            + signature(&self.committer)
            + self.message.head.len()
            + self.message.hash.len()
            + self.trailers.heap_bytes()
            + self.files.iter().map(FileRecord::heap_bytes).sum::<usize>()
    }
}

/// Which tree a commit was diffed against, and therefore what "this commit changed" means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DiffBase {
    /// There is no parent: the diff is against the empty tree.
    EmptyTree,
    /// The single parent's tree.
    Parent {
        /// The parent's commit id.
        parent: Oid,
    },
    /// A merge, diffed against nothing — git's own default (`git log --raw` prints no file rows for a
    /// merge), and the default here because the alternative double-counts every change on the merged
    /// branch once in the branch commit and once in the merge.
    MergeSkipped {
        /// How many parents the merge has.
        parents: usize,
    },
    /// The parent this commit names is not in this clone — behind a shallow graft, or omitted by a clone
    /// filter — so there is nothing to diff against. Diffing against the empty tree instead would report
    /// the entire tree as newly added, which is the worse lie.
    UnreadableParent {
        /// The parent whose object is missing.
        parent: Oid,
    },
    /// A merge, diffed against its first parent because the caller asked for it
    /// ([`crate::normalize::MergePolicy::FirstParent`]).
    MergeFirstParent {
        /// The first parent's commit id.
        parent: Oid,
        /// How many parents the merge has, so a caller can tell an octopus from a normal merge.
        parents: usize,
    },
}

// ---------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------

/// Lowercase hex SHA-256 of `bytes`.
///
/// Public because `git_commit_file.path_hash` is this function applied to a path's raw bytes
/// (ADR-004 Appendix A), and two implementations of one hash is how a key stops being a key.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The first line of a message, without its terminator.
fn first_line(message: &[u8]) -> Option<&[u8]> {
    let trimmed = match message.iter().position(|byte| *byte == b'\n') {
        Some(index) => &message[..index],
        None => message,
    };
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed)
}

/// Truncate to `limit` bytes without splitting a UTF-8 sequence.
fn truncate_bytes(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use fathom_core::time::Timestamp;

    #[test]
    fn trailer_block_is_the_last_paragraph_and_last_value_wins() {
        let message = "Add the thing\n\
                        \n\
                        A long body that mentions Co-authored-by: nobody in prose.\n\
                        \n\
                        Co-authored-by: Ada Lovelace <ada@example.invalid>\n\
                        agent-session: s-17\n\
                        agent-turn: 4\n\
                        agent-turn: 5\n"
            .as_bytes();
        let trailers = Trailers::parse(message);
        assert_eq!(
            trailers.get("Co-Authored-By"),
            Some("Ada Lovelace <ada@example.invalid>")
        );
        assert_eq!(trailers.get("agent-session"), Some("s-17"));
        assert_eq!(
            trailers.get("Agent-Turn"),
            Some("5"),
            "a repeated key keeps its last value, and keys are case-insensitive"
        );
        assert_eq!(
            trailers.len(),
            3,
            "co-authored-by, agent-session, agent-turn"
        );
    }

    #[test]
    fn prose_paragraph_is_not_a_trailer_block() {
        // The last paragraph is prose, so there are no trailers. Taking the last *line* that happens to
        // contain a colon is the bug this guards.
        let trailers = Trailers::parse(b"Subject\n\nNotes:\nSee the note about the thing.\n");
        assert!(trailers.is_empty());
    }

    #[test]
    fn continuation_lines_extend_the_previous_value() {
        let trailers = Trailers::parse(b"Subject\n\nCo-authored-by: Ada\n <ada@example.invalid>\n");
        assert_eq!(
            trailers.get("co-authored-by"),
            Some("Ada <ada@example.invalid>"),
            "git folds an indented continuation into the value"
        );
    }

    #[test]
    fn a_subject_shaped_like_a_trailer_is_not_one() {
        // The defect this guards (`reco-2ss`): a message with no trailer paragraph yields no trailers.
        // Every conventional-commit subject is shaped like `Key: value`, so reading the *first*
        // paragraph as a trailer block invented a trailer for most commits in a real repository — and
        // `fathom show` printed the subject under `trailers`.
        let trailers = Trailers::parse(
            b"chore: gitignore .transcripts/ \xe2\x80\x94 session logs land in the working tree\n",
        );
        assert!(
            trailers.is_empty(),
            "a one-paragraph message is a subject, not a trailer block: {trailers:?}"
        );
        // Not even when the whole (single) paragraph looks like trailers: git's `interpret-trailers`
        // finds no block until a blank line separates one from what precedes it.
        assert!(Trailers::parse(b"Co-authored-by: Ada\nSigned-off-by: Ada\n").is_empty());
        // And not for the two shapes that used to reach the map from a body-less message.
        assert!(Trailers::parse(b"fix: the parser lied\n").is_empty());
        assert!(Trailers::parse(b"Reviewed-by: Ada\n").is_empty());
        assert!(Trailers::parse(b"").is_empty());
    }

    #[test]
    fn the_blank_line_is_what_makes_the_block() {
        // The other half of the pair: the same subject, plus a body and a trailer block. Move the
        // trailer paragraph down one line — from "no blank line before it" to "a blank line before
        // it" — and it stops being the subject and starts being a trailer.
        let subject_only = Trailers::parse(b"agent-session: s-1\n");
        assert!(subject_only.is_empty(), "{subject_only:?}");
        let with_a_block = Trailers::parse(b"agent-session: s-1\n\nagent-session: s-2\n");
        assert_eq!(
            with_a_block.get("agent-session"),
            Some("s-2"),
            "the block is the last paragraph, and only the last paragraph"
        );
        // A CRLF message is the same message: `git interpret-trailers` does not care who wrote the
        // line endings, and neither may this.
        let crlf = Trailers::parse(b"Subject\r\n\r\nagent-session: s-3\r\n");
        assert_eq!(crlf.get("agent-session"), Some("s-3"), "{crlf:?}");
        // A paragraph of nothing but spaces separates blocks, as it does in git.
        let spaces = Trailers::parse(b"Subject\n\n   \nSigned-off-by: Ada\n");
        assert_eq!(spaces.get("signed-off-by"), Some("Ada"), "{spaces:?}");
    }

    #[test]
    fn hunk_headers_round_trip_and_follow_git_s_omission_rules() {
        assert_eq!(Hunk::new(1, 1, 1, 2).header(), "@@ -1 +1,2 @@");
        assert_eq!(Hunk::new(0, 0, 1, 3).header(), "@@ -0,0 +1,3 @@");
        assert_eq!(Hunk::new(12, 7, 12, 9).header(), "@@ -12,7 +12,9 @@");
        for header in [
            "@@ -1 +1,2 @@".to_owned(),
            "@@ -0,0 +1,3 @@".to_owned(),
            "@@ -12,7 +12,9 @@".to_owned(),
            "@@ -5 +5 @@".to_owned(),
        ] {
            let parsed = Hunk::parse(&header).expect("parses");
            assert_eq!(parsed.header(), header, "{header} round-trips");
        }
        assert!(Hunk::parse("@@ -1,2 +3 @@ extra").is_none());
        assert!(Hunk::parse("not a hunk").is_none());
    }

    #[test]
    fn message_head_is_byte_capped_at_a_character_boundary() {
        // A three-byte-across message: cutting at 5 bytes must not cut the second grapheme in half.
        let raw = "日本語\nsecond line\n".as_bytes();
        let facts = MessageFacts::new(raw, 5);
        assert_eq!(
            facts.head, "日",
            "truncated to 3 bytes, the last whole character"
        );
        assert_eq!(facts.hash.len(), 64);
        assert_eq!(facts.bytes, raw.len());
        assert_eq!(MessageFacts::new(b"", 512).head, "");
        assert_eq!(MessageFacts::new(b"one line", 512).head, "one line");
    }

    #[test]
    fn content_states_are_not_the_same_as_zero_counts() {
        let big = 500 * 1024 * 1024;
        let binary = FileRecord::unexamined(
            ChangeKind::Modify,
            RepoPath::new("a"),
            NotExamined::Oversized { size: big },
        );
        assert!(!binary.content.is_binary(), "unexamined is not binary");
        assert!(!binary.content.measured());
        assert!(Content::Binary.is_binary());
        assert!(Content::Text.measured());
    }

    #[test]
    fn record_json_is_stable_and_round_trips() {
        let record = CommitRecord {
            oid: Oid::ZERO,
            tree: Oid::ZERO,
            parents: vec![],
            author: Signature {
                name: "Ada".into(),
                email: "ada@example.invalid".into(),
                when: Timestamp::from_unix_micros(1_700_000_000_000_000),
                offset_minutes: 60,
            },
            committer: Signature {
                name: "Ada".into(),
                email: "ada@example.invalid".into(),
                when: Timestamp::from_unix_micros(1_700_000_000_000_000),
                offset_minutes: 0,
            },
            message: MessageFacts::new(b"Subject\n\nBody.\n", 512),
            trailers: Trailers::parse(b"Subject\n\nBody.\n\nagent-session: s-1\n"),
            files: vec![FileRecord::unexamined(
                ChangeKind::Add,
                RepoPath::new("src/a.rs"),
                NotExamined::Oversized { size: 1 },
            )],
            totals: Totals::default(),
            files_truncated: false,
            diff_base: DiffBase::EmptyTree,
        };
        let text = serde_json::to_string(&record).expect("serialises");
        let back: CommitRecord = serde_json::from_str(&text).expect("deserialises");
        assert_eq!(back, record);
        // A re-serialisation is byte-identical: this is what makes a golden file honest.
        assert_eq!(
            text,
            serde_json::to_string(&back).expect("serialises again")
        );
    }
}

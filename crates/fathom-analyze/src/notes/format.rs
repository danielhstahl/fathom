//! `format.rs` — the markdown the review memory is written in (`e6.5`, `reco-552`).
//!
//! The same contract `crate::ledger::format` established, because the two files sit in the same directory,
//! are read by the same person in a PR, and have the same job of surviving a rebase and a merge:
//!
//! ```text
//! # Fathom review memory
//!
//! <!-- format: fathom-review-note/1 -->
//! …
//! ## nt-1f5c0a2b9d3e4a77 | decision | 2026-10-19T14:22:03Z | Dana Ruiz <dana@example.com>
//!
//! - kind: decision
//! - episode: ep-…
//! …
//! - digest: sha256:…
//! ```
//!
//! # Why this is a separate format from the ledger's
//!
//! Different field sets, different cadence, same directory. A note is prose and plentiful; a sign-off is a
//! disposition and is what a gate reads. Keeping them apart means `git log -- .fathom/reviews/ledger.md`
//! stays the answer to "what has been accepted here", each reader refuses a file that declares the other's
//! format tag, and appending a question to the memory cannot churn the artefact a CI job parses.
//!
//! # Diff-friendliness, which is the acceptance criterion this file exists to satisfy
//!
//! * **Append-only, oldest first.** Two reviewers appending on parallel branches produce two additions at
//!   the end of one file — the one conflict shape a merge resolves in seconds. Nothing here rewrites an
//!   entry it did not just write ([`append_notes`] is the only writer of an existing file), and the digest
//!   makes a rewritten entry visible.
//! * **One field per line, one line per field.** A markdown table would re-render every row whenever the
//!   longest note changed; a 64-hex digest will not fit in a column anyway. The unit of a diff is one note's
//!   one field.
//! * **The id is in the heading, the digest is the last line.** A PR comment can quote `nt-…`, and a hunk
//!   that changes a note's text changes exactly the lines between its heading and its digest — which is what
//!   makes the "someone edited the recorded decision" case visible in the diff itself rather than only to
//!   `fathom note verify`.
//! * **No sorting, no re-wrapping, no alignment.** Every byte between two entries is a byte the writer chose.
//!
//! # Reading is deliberately loud
//!
//! Same rule as the ledger: an unknown key, a missing `digest:`, a malformed stamp or a block with no fields
//! is a reported problem carrying file and line number. A review memory that silently skipped the note it
//! could not read would answer "why did we do X" with "nothing is recorded", which is the one answer that is
//! worse than an error.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::entry::{Kind, Note};
use crate::ledger::entry::{Actor, ActorKind, PathFact, parse_iso_utc};
use crate::ledger::format::{LEDGER_DIR, Problem, resolves_inside};

/// The directory the review memory shares with the ledger (ADR-004 §3, §12.1).
///
/// The same directory on purpose: ADR-004 §12's gitignore stanza un-ignores `.fathom/reviews/` and nothing
/// else under `.fathom/`, and the must-survive check walks that directory for `*.md`. A note in a directory
/// of its own would have to be added to both lists, and the failure mode of forgetting is a silent loss of
/// the memory — which is the failure this whole module exists to prevent.
pub const NOTES_DIR: &str = LEDGER_DIR;

/// The default note file's name.
pub const NOTES_FILE: &str = "notes.md";

/// The format tag line, and the format this build writes.
pub const FORMAT_TAG: &str = "fathom-review-note/1";

/// The largest note file [`parse`] will read.
///
/// Same ceiling and same reason as the ledger's ([`crate::ledger::format::MAX_LEDGER_BYTES`]): a file named
/// `*.md` in that directory is read by every reader of the review memory, and a clone that names a 4 GiB
/// file — or a device — `notes.md` must not be able to make a reader allocate it (`e7.8`, threat model F-7).
pub const MAX_NOTE_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// The preamble a freshly created memory file gets — prose for the same reason the ledger's is.
#[must_use]
pub fn preamble() -> String {
    format!(
        concat!(
            "# Fathom review memory\n",
            "\n",
            "<!-- format: {FORMAT_TAG} -->\n",
            "<!--\n",
            "     One block per note, decision, question or follow-up, appended oldest first. This file is\n",
            "     the durable record of what a review concluded and what it left open; the index in\n",
            "     .fathom/index.sqlite3 is a rebuildable mirror of it, and deleting the store loses nothing.\n",
            "\n",
            "     Do not edit, reorder or re-wrap a block that has been committed. The `digest:` line at the\n",
            "     end of each block hashes the lines above it, so an edit is detectable and `fathom note\n",
            "     verify` will name it. To answer a question, append a new block with `about:` naming it.\n",
            "\n",
            "     `episode:`, `fingerprint:`, `fallback:`, `shape:` and `members:` are what the note is\n",
            "     attached to; `fathom context <path-or-episode>` follows them onto the episode the change\n",
            "     became after a rebase. `target:` is the path and line the note was pointing at, kept as\n",
            "     evidence rather than as the handle.\n",
            "-->\n"
        ),
        FORMAT_TAG = FORMAT_TAG
    )
}

/// The rendered block for one note: heading, fields, digest.
#[must_use]
pub fn render_entry(note: &Note) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n## {} | {} | {} | {}\n\n",
        note.id,
        note.kind.as_str(),
        crate::ledger::entry::iso_utc(note.written_utc),
        note.author.display
    ));
    for (key, value) in &note.body {
        out.push_str(&format!("- {key}: {value}\n"));
    }
    out.push_str(&format!("- digest: {}\n", note.digest));
    out
}

/// The whole file, notes in the order given.
#[must_use]
pub fn render(notes: &[Note]) -> String {
    let mut out = preamble();
    for note in notes {
        out.push_str(&render_entry(note));
    }
    out
}

/// One memory file's parse output.
#[derive(Debug, Default)]
pub struct Parsed {
    /// The notes, in file order (oldest first by convention).
    pub notes: Vec<Note>,
    /// Everything wrong with the file, each with a line number.
    pub problems: Vec<Problem>,
    /// The `format:` tag the file declared, if it declared one.
    pub format: Option<String>,
}

/// The memory files to read, sorted by name: the default file first when it exists, then any others.
///
/// A file that declares a format other than [`FORMAT_TAG`] is skipped — that is the ledger's file, and it is
/// `fathom review verify`'s job to read it and to complain about it. A file that declares nothing is read,
/// and its missing tag is the complaint this reader makes, so a hand-made file in this directory gets a
/// diagnosis rather than silence.
#[must_use]
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let dir = root.join(NOTES_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().is_some_and(|ext| ext == "md")
                && declared_format(&path).is_some_and(|tag| tag == FORMAT_TAG))
            .then_some(path)
        })
        .collect();
    files.sort();
    files.sort_by_key(|path| {
        (
            path.file_name() != Some(std::ffi::OsStr::new(NOTES_FILE)),
            path.clone(),
        )
    });
    files
}

/// The format tag a file declares, read from the first bytes of it.
///
/// Containment is [`parse`]'s refusal to make, not this scanner's: a symlink that points out of the
/// repository must reach the reader so it is *reported* there, and a file this cannot read is a file this
/// cannot exclude on a guess.
fn declared_format(path: &Path) -> Option<String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = vec![0u8; 4096];
    let read = file.read(&mut buffer).ok()?;
    buffer.truncate(read);
    let text = std::str::from_utf8(&buffer).ok()?;
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("<!-- format:")
            .map(|rest| rest.trim_end_matches("-->").trim().to_owned())
    })
}

/// Parse one memory file. Never fails: an unreadable file is a [`Problem`] with the file's name, because
/// "the memory holds 3 notes and one file could not be read" is a report and "error: parse" is not.
#[must_use]
pub fn parse(path: &Path, root: &Path) -> Parsed {
    let display = relative(path, root);
    if let Some(what) = refusal(path, root) {
        return Parsed {
            problems: vec![Problem {
                file: display,
                line: 1,
                what,
            }],
            ..Parsed::default()
        };
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Parsed {
            problems: vec![Problem {
                file: display,
                line: 1,
                what: "this review-memory file could not be read (missing, or not valid UTF-8)"
                    .to_owned(),
            }],
            ..Parsed::default()
        };
    };
    parse_text(&display, &text)
}

/// Why this path may not be read, in words, or `None` when it may.
///
/// Containment first, with the ledger's own wording ([`resolves_inside`]): a clone that points a memory file
/// at a file outside the repository is refused here as loudly as it is refused by the ledger's reader, and
/// with the same sentence, so a repository cannot have two different explanations of the same symlink.
fn refusal(path: &Path, root: &Path) -> Option<String> {
    if !resolves_inside(root, path) {
        return Some(
            "this review-memory file resolves outside the repository (it is a symlink, or sits under one), \
             and its contents are not read — a clone must not be able to point a note at a file of \
             somebody else's"
                .to_owned(),
        );
    }
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_file() => Some(
            "this is not a regular file (a directory, a device or a fifo was named `*.md`), so it is not \
             read"
                .to_owned(),
        ),
        Ok(meta) if meta.len() > MAX_NOTE_FILE_BYTES => Some(format!(
            "this review-memory file is {} bytes, over the {} byte cap, so it is not read",
            meta.len(),
            MAX_NOTE_FILE_BYTES
        )),
        Err(err) => Some(format!("this review-memory file could not be stat'ed: {err}")),
        _ => None,
    }
}

/// Parse a memory file from its text. Split out so a test and a golden file exercise the reader the file
/// actually goes through.
#[must_use]
pub fn parse_text(display: &str, text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut block: Option<Block> = None;

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if out.format.is_none()
            && let Some(rest) = trimmed.strip_prefix("<!-- format:")
        {
            out.format = Some(rest.trim_end_matches("-->").trim().to_owned());
        }
        if let Some(heading) = trimmed.strip_prefix("## ") {
            if let Some(finished) = block.take() {
                out.push_block(finished, display);
            }
            let mut words = heading.split('|').map(str::trim);
            block = Some(Block {
                line,
                id: words.next().unwrap_or_default().to_owned(),
                // The heading repeats the block's kind and author so a `git log -p` of this file is readable
                // without opening the fields; the fields below remain the authority, and a heading that
                // disagrees with them is a heading that lies about nothing the code reads.
                heading_kind: words.next().unwrap_or_default().to_owned(),
                body: Vec::new(),
                digest: None,
                digest_line: 0,
                saw_fields: false,
            });
            continue;
        }
        let Some(block) = block.as_mut() else {
            continue;
        };
        if trimmed.is_empty() || trimmed.starts_with("<!") || trimmed.starts_with("# ") {
            continue;
        }
        let Some(field) = trimmed.strip_prefix("- ") else {
            out.problems.push(Problem {
                file: display.to_owned(),
                line,
                what: format!(
                    "text inside a note block that is not a `- key: value` line: {trimmed}"
                ),
            });
            continue;
        };
        let Some((key, value)) = field.split_once(':') else {
            out.problems.push(Problem {
                file: display.to_owned(),
                line,
                what: format!("a `-` line with no `key: value` in it: {field}"),
            });
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        block.saw_fields = true;
        if key == "digest" {
            block.digest = Some(value.to_owned());
            block.digest_line = line;
            continue;
        }
        if !Note::known_keys().contains(&key) {
            out.problems.push(Problem {
                file: display.to_owned(),
                line,
                what: format!(
                    "`{key}` is not a field this format knows; a typo here would read as a missing value"
                ),
            });
            continue;
        }
        block.body.push((key.to_owned(), value.to_owned()));
    }
    if let Some(finished) = block.take() {
        out.push_block(finished, display);
    }
    if out.format.is_none() {
        out.problems.push(Problem {
            file: display.to_owned(),
            line: 1,
            what: format!("no `format: {FORMAT_TAG}` tag, so this file's shape is unclaimed"),
        });
    }
    out
}

struct Block {
    line: usize,
    id: String,
    heading_kind: String,
    body: Vec<(String, String)>,
    digest: Option<String>,
    digest_line: usize,
    saw_fields: bool,
}

impl Parsed {
    fn push_block(&mut self, block: Block, display: &str) {
        if !block.saw_fields {
            self.problems.push(Problem {
                file: display.to_owned(),
                line: block.line,
                what: "a note block with no fields in it".to_owned(),
            });
            return;
        }
        let Some(digest) = block.digest.clone() else {
            self.problems.push(Problem {
                file: display.to_owned(),
                line: block.line,
                what: format!(
                    "note `{}` has no `digest:` line, so it cannot be checked for edits",
                    block.id
                ),
            });
            return;
        };
        // The heading's second word is prose for the reader of a `git log -p`, and the `kind:` field is the
        // authority. When they disagree the reader is being shown one thing and indexed as another, which is
        // worth a line: it happens when a hand edits one and not the other, and nothing else would notice.
        let declared = block
            .body
            .iter()
            .find(|(key, _)| key == "kind")
            .map(|(_, value)| value.as_str())
            .unwrap_or_default();
        if !block.heading_kind.is_empty()
            && !declared.is_empty()
            && block.heading_kind != declared
            && Kind::parse(&block.heading_kind) != Kind::parse(declared)
        {
            self.problems.push(Problem {
                file: display.to_owned(),
                line: block.line,
                what: format!(
                    "note `{}`: its heading says `{}`, its `kind:` field says `{}` — the field is the \
                     authority, so fix the heading",
                    block.id, block.heading_kind, declared
                ),
            });
        }
        let expected = super::entry::note_digest(&block.id, &block.body);
        if expected != digest {
            self.problems.push(Problem {
                file: display.to_owned(),
                line: block.digest_line,
                what: format!(
                    "note `{}`: its `digest:` is {digest} and its lines hash to {expected} — the text \
                     changed after it was written",
                    block.id
                ),
            });
        }
        match to_note(&block.id, &block.body, &digest) {
            Ok(note) => self.notes.push(note),
            Err(what) => self.problems.push(Problem {
                file: display.to_owned(),
                line: block.line,
                what: format!("note `{}`: {what}", block.id),
            }),
        }
    }
}

/// Turn parsed field lines into a [`Note`], refusing a block whose required fields are unusable.
fn to_note(id: &str, body: &[(String, String)], digest: &str) -> Result<Note, String> {
    let get = |key: &str| {
        body.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default()
    };
    let kind = match Kind::parse(get("kind")) {
        Some(kind) => kind,
        None => {
            return Err(format!(
                "`{}` is not a kind (note, decision, question, follow-up, flag)",
                get("kind")
            ));
        }
    };
    let written_utc = match parse_iso_utc(get("written")) {
        Some(stamp) => stamp,
        None => {
            return Err(format!(
                "`written: {}` is not a UTC instant like 2026-10-19T14:22:03Z",
                get("written")
            ));
        }
    };
    let text = get("text").to_owned();
    if text.is_empty() {
        return Err("a note with no `text:` records nothing".to_owned());
    }
    let author = Actor {
        display: if get("author").is_empty() {
            "unknown".to_owned()
        } else {
            get("author").to_owned()
        },
        kind: ActorKind::parse(get("author-kind")),
    };
    let target = get("target").trim();
    let (path, line) = split_target(target);
    let episode = get("episode").trim();
    let episode = if episode == "-" { "" } else { episode };
    let note = Note {
        id: id.to_owned(),
        file: String::new(),
        kind,
        episode: episode.to_owned(),
        author,
        written_utc,
        text,
        path,
        line,
        about: get("about").to_owned(),
        withdraws: get("withdraws").to_owned(),
        via: get("via").to_owned(),
        fingerprint: digest_field(get("fingerprint")),
        fallback: digest_field(get("fallback")),
        shape: digest_field(get("shape")),
        content: digest_field(get("content")),
        members: split_list(get("members")),
        paths: split_list(get("paths"))
            .iter()
            .map(|text| PathFact::parse(text.as_str()))
            .collect(),
        body: body.to_vec(),
        digest: digest.to_owned(),
    };
    Ok(note)
}

/// `src/a.rs:42` into (`src/a.rs`, `Some(42)`). A colon that is not followed by digits is part of the path —
/// a Windows drive letter, or a path with a colon in it — and is kept whole rather than eaten.
fn split_target(text: &str) -> (String, Option<u32>) {
    if text.is_empty() || text == "-" {
        return (String::new(), None);
    }
    if let Some((path, tail)) = text.rsplit_once(':')
        && !path.is_empty()
        && let Ok(line) = tail.parse::<u32>()
    {
        return (path.to_owned(), Some(line));
    }
    (text.to_owned(), None)
}

/// `unmeasured (…)` and a bare 64-hex digest are the two spellings; anything else is kept as written so a
/// reader can see it rather than being shown an empty string.
fn digest_field(value: &str) -> String {
    if value.starts_with("unmeasured") {
        String::new()
    } else {
        value.to_owned()
    }
}

/// A comma-separated field into a list; empty and `-` both mean none.
fn split_list(value: &str) -> Vec<String> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        return Vec::new();
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Append notes to a memory file, creating it (with its preamble) when it does not exist.
///
/// The file is flushed **and fsynced before returning** — ADR-004 §7.4's R8, for the reason the ledger gives
/// for it: a mirrored note whose file is still in the OS buffer cache is a lost decision with a row claiming
/// otherwise. The mirror write happens after this call returns, never before.
///
/// Duplicate ids are skipped and counted: the id is derived from the note's content, so an id already in the
/// file *is* the same note, and appending it twice would be a diff that says nothing twice.
pub fn append_notes(path: &Path, notes: &[Note]) -> std::io::Result<usize> {
    if notes.is_empty() {
        return Ok(0);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let have: std::collections::BTreeSet<String> = parse_text("", &existing)
        .notes
        .into_iter()
        .map(|note| note.id)
        .collect();
    let fresh = !path.exists();
    let trailing_newline = !existing.is_empty() && !existing.ends_with('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if fresh {
        file.write_all(preamble().as_bytes())?;
    } else if trailing_newline {
        // Append onto a proper last line: a file whose final newline was lost by a hand edit would otherwise
        // glue the new note's heading onto the previous note's digest.
        file.write_all(b"\n")?;
    }
    let mut written = 0_usize;
    for note in notes {
        if have.contains(&note.id) {
            continue;
        }
        file.write_all(render_entry(note).as_bytes())?;
        written += 1;
    }
    file.flush()?;
    file.sync_all()?;
    Ok(written)
}

/// The file's display path, repo-relative and with forward slashes, as every complaint here prints it.
fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

//! The small, defensive text helpers every parser in `deps` shares (e2.5 / reco-j83).
//!
//! Two of these rules are load-bearing enough to state once here rather than eighteen times:
//!
//! * **Nothing in this module panics on hostile bytes.** A lockfile is repository content, and repository
//!   content is an attacker (ADR-004 §7.7). Slicing is always char-boundary-safe, every decode is lossy
//!   or typed, and every limit is a parameter.
//! * **A failed parse says where and why.** [`syntax_reason`] turns a parser's own message into
//!   [`UnparsedReason::Truncated`] versus [`UnparsedReason::MalformedSyntax`], which is the difference
//!   between telling a reviewer "your lockfile is corrupt" and "we stopped reading at the cap".

use std::borrow::Cow;

use crate::deps::model::UnparsedReason;

/// The byte count used for the binary test, and for the leading preview.
pub const PREVIEW_BYTES: usize = 512;

/// The same test `fathom-ingest` uses for a blob (`normalize::is_binary`): a NUL in the leading bytes.
///
/// Duplicated deliberately rather than imported: `fathom-analyze` sits above `fathom-ingest` and could
/// reach it, but "is this text" is a fact about *this* module's input contract, and a parser that changed
/// its mind because an ingest heuristic was retuned would be a parser with two answers.
#[must_use]
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8_192)].contains(&0)
}

/// Decode as UTF-8, or say why not.
pub fn decode(bytes: &[u8]) -> Result<Cow<'_, str>, UnparsedReason> {
    if is_binary(bytes) {
        return Err(UnparsedReason::NotText);
    }
    std::str::from_utf8(bytes)
        .map(Cow::Borrowed)
        .map_err(|_| UnparsedReason::NotText)
}

/// Cut a string to at most `max` bytes, on a char boundary, without an ellipsis.
#[must_use]
pub fn clip(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A leading excerpt for [`Unparsed`](crate::deps::model::Unparsed): whole lines up to `max_bytes`, and
/// whether anything was left out.
///
/// Whole lines because a half line of a lockfile is a lie about its shape; the last line is dropped rather
/// than split. The returned string ends in `…` exactly when it is a prefix, which is what
/// [`DepFile::unparsed`](crate::deps::model::DepFile::unparsed) reads back.
#[must_use]
pub fn preview(text: &str, max_bytes: usize) -> (String, bool) {
    let mut out = String::new();
    let mut truncated = false;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() && index == 0 {
            continue;
        }
        let candidate = out.len() + line.len() + 1;
        if candidate > max_bytes {
            truncated = true;
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        if out.len() >= max_bytes / 2 {
            // Enough to identify the file and its header, and no more: a preview that grows into the
            // whole document is a second copy of the blob in every cache row.
            truncated = true;
            break;
        }
    }
    if out.is_empty() && !text.is_empty() {
        // One line longer than the budget: fall back to a clipped head rather than to nothing.
        out.push_str(clip(text, max_bytes / 2));
        truncated = true;
    }
    if truncated {
        out.push('…');
    }
    (out, truncated)
}

/// A preview of bytes that did not decode as text.
///
/// "Not text" with no raw text at all is one step from dropping the episode: the reviewer sees a filename and
/// a shrug. This is the lossy rendering, with control characters replaced by `·` so that the *kind* of blob is
/// legible — `\xc2\x7fELF` says "a binary was committed where a manifest belongs", a run of `\x00` between
/// every character says "UTF-16, which no packaging tool writes" — without the preview itself becoming
/// control output. The bytes stay where they are; this is a caption, not a copy.
#[must_use]
pub fn preview_binary(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    let head = &bytes[..bytes.len().min(max_bytes.saturating_mul(2))];
    let mut out = String::new();
    for ch in String::from_utf8_lossy(head).chars() {
        if out.len() >= max_bytes {
            break;
        }
        if ch == '\n' || ch == '\t' {
            out.push(ch);
        } else if ch.is_control() {
            out.push('·');
        } else {
            out.push(ch);
        }
    }
    let truncated = head.len() < bytes.len() || out.len() >= max_bytes;
    out = out.trim().to_owned();
    if truncated {
        out.push('…');
    }
    (out, truncated)
}

/// The lines of a document, `\r\n` tolerated, without allocating a `Vec` of owned strings.
pub fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// Strip one layer of matching quotes.
#[must_use]
pub fn unquote(text: &str) -> &str {
    let trimmed = text.trim();
    if trimmed.len() >= 2 {
        let (head, tail) = (trimmed.as_bytes()[0], trimmed.as_bytes()[trimmed.len() - 1]);
        if (head == b'"' && tail == b'"') || (head == b'\'' && tail == b'\'') {
            return &trimmed[1..trimmed.len() - 1];
        }
    }
    trimmed
}

/// An inline `{a: b}` / `[a, b]` flow collection, kept as text.
///
/// Lockfiles use flow style only for metadata a reviewer never reads (pnpm's `engines`, yarn's
/// `optionalDependencies` on old entries), so parsing it properly would buy a larger parser for a note
/// nobody opens. This returns the raw text and lets the caller add that note.
#[must_use]
pub fn is_flow(value: &str) -> bool {
    let value = value.trim();
    (value.starts_with('{') && value.ends_with('}'))
        || (value.starts_with('[') && value.ends_with(']'))
}

/// Which of "corrupt" and "cut off" a JSON failure means.
///
/// `serde_json` distinguishes these for us: a truncated document fails with an `EOF while parsing …`
/// message, a corrupt one fails on a token. Classifying wrongly is worse than not classifying, because
/// `Truncated` carries advice to raise a cap, and a cap raise is the wrong remedy for a bad file.
#[must_use]
pub fn json_reason(err: &serde_json::Error) -> UnparsedReason {
    // `serde_json` keeps its error classes unstable, but the message is stable enough for the one
    // distinction that matters here: an input that ran out versus an input that was wrong.
    if err.to_string().contains("EOF while parsing") {
        UnparsedReason::Truncated
    } else {
        UnparsedReason::MalformedSyntax
    }
}

/// Which of "corrupt" and "cut off" a TOML failure means.
///
/// `toml_edit` keeps the byte offset it failed at, so a cut is detectable by position as well as by
/// message: a failure at or past the last non-blank byte means the document ended where we stopped
/// reading. Both signals are used, and a message that names an unterminated construct is trusted even
/// when the offset is mid-file (a `[[package]]` table opened and never written is the same bug shape).
#[must_use]
pub fn toml_reason(message: &str, offset: usize, text: &str) -> UnparsedReason {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("unclosed")
        || lowered.contains("unterminated")
        || lowered.contains("unexpected end")
        || lowered.contains("expected newline")
        || lowered.contains("incomplete")
    {
        return UnparsedReason::Truncated;
    }
    if offset + 8 >= text.trim_end().len() {
        return UnparsedReason::Truncated;
    }
    UnparsedReason::MalformedSyntax
}

/// Accumulates what a parser wants a reviewer to know, and derives the status that follows from it.
///
/// One type so that "we understood most of it" is a decision a parser cannot forget to record: the status
/// is derived from what is in here rather than from a flag that can be left at its default.
///
/// Two kinds, because they mean opposite things:
///
/// - a **note** is a deficiency — a line skipped, a version guessed, a section not understood. Any note of
///   this kind makes the file [`Partial`](crate::deps::model::Status::Partial), because the surface is
///   missing something a reader would have wanted.
/// - an **observation** (`info`) is a fact that was read perfectly well and is worth repeating: a yarn entry
///   that answers two specs, a package locked under a marker, a virtual workspace manifest. It travels with
///   the file and never downgrades it. Without this second kind, every informative note is a false alarm —
///   a healthy `yarn.lock` would be `partial` for being normal.
#[derive(Debug, Default, Clone)]
pub struct Notes {
    entries: Vec<(String, bool)>,
}

impl Notes {
    /// An empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Notes::default()
    }

    /// Record one skip, downgrade or guess. This is what makes a parse `Partial`.
    pub fn push(&mut self, note: impl Into<String>) {
        self.entries.push((note.into(), true));
    }

    /// Record one with a `line N: ` prefix, which is how a reviewer finds the line in the diff.
    pub fn at(&mut self, line: usize, note: impl std::fmt::Display) {
        self.entries.push((format!("line {line}: {note}"), true));
    }

    /// Record a fact that was read cleanly and is worth surfacing. Never changes the status.
    pub fn info(&mut self, note: impl Into<String>) {
        self.entries.push((note.into(), false));
    }

    /// The notes, in the order they were recorded, and the status they imply: `Partial` when any of them is
    /// a deficiency.
    #[must_use]
    pub fn finish(self) -> (Vec<String>, crate::deps::model::Status) {
        let status = if self.entries.iter().any(|(_, deficiency)| *deficiency) {
            crate::deps::model::Status::Partial
        } else {
            crate::deps::model::Status::Parsed
        };
        (
            self.entries.into_iter().map(|(note, _)| note).collect(),
            status,
        )
    }
}

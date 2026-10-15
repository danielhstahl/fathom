//! The lockfile hunk that proves a claim (e2.6 / reco-kvs).
//!
//! A dependency delta that only says *"foo 2.0.0 → 2.1.0"* asks the reviewer to take it on trust. Every
//! row this module produces points at the text that makes it true: the hunk of the lockfile diff in which
//! the claim's line actually changed. That is the drill-down the ticket asks for, and it is also the only
//! answer available when the file cannot be parsed at all — a hunk is always available where a parse is
//! not, which is why [`crate::deps`] keeps the raw text alongside the parsed shape.
//!
//! The diff is line-based, computed with the one diff crate already in the tree, and bounded: a context
//! radius, a hunk cap and a line cap, with a note when either bit. Lockfiles are the largest text files in
//! a repository and an unbounded diff of two of them is how a review tool stops being interactive.
//!
//! Locating a claim ([[`locate`]]) is a search over *changed* lines only, because a hunk that shows the
//! package in unchanged context proves nothing about the change. Preference order is stated in
//! [`Evidence`] and is deterministic: name plus version beats name, more matched lines beats fewer, and
//! the earliest hunk wins a tie.

use serde::{Deserialize, Serialize};
use similar::TextDiff;

/// How much context to keep, and how much of a file's diff to keep at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Context lines around each change, the `-U` of a `git diff`.
    pub context: usize,
    /// Stop after this many hunks and say so.
    pub max_hunks: usize,
    /// Stop after this many lines within one hunk and say so.
    pub max_lines_per_hunk: usize,
    /// Skip the diff entirely above this file size, and say so: a 40 MiB lockfile is not worth a hang.
    pub max_file_bytes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            context: 3,
            max_hunks: 400,
            max_lines_per_hunk: 400,
            max_file_bytes: 4 * 1024 * 1024,
        }
    }
}

/// One hunk of a file's diff, in the shape a reviewer reads: a header and prefixed lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// Zero-based index of this hunk in the file's diff.
    pub index: usize,
    /// First old line, 1-based, as the header states it.
    pub old_start: usize,
    /// How many old lines the hunk covers.
    pub old_len: usize,
    /// First new line, 1-based.
    pub new_start: usize,
    /// How many new lines the hunk covers.
    pub new_len: usize,
    /// The header, `@@ -120,6 +120,11 @@`, so a viewer can print the hunk without knowing these fields.
    pub header: String,
    /// The lines, each prefixed with ` `, `+` or `-`, trailing newline removed.
    pub lines: Vec<String>,
    /// Changed lines added.
    pub added: usize,
    /// Changed lines removed.
    pub removed: usize,
    /// Whether the line cap cut this hunk short.
    pub truncated: bool,
}

impl Hunk {
    /// The hunk as text: header, then lines.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(self.header.len() + self.lines.len() * 40);
        out.push_str(&self.header);
        out.push('\n');
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        if self.truncated {
            out.push_str("… hunk truncated\n");
        }
        out
    }
}

/// Which side of the hunk a claim needs to see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The claim is about something the change introduced: look for `+` lines.
    Added,
    /// The claim is about something the change removed: look for `-` lines.
    Removed,
    /// Either side proves it.
    Either,
}

/// What to look for in a hunk.
#[derive(Debug, Clone, Copy)]
pub struct Evidence<'a> {
    /// The package name, matched as a substring of the line — the lockfile's own spelling.
    pub name: &'a str,
    /// The version the claim is about, when the claim has one.
    pub version: Option<&'a str>,
    /// Which side must carry it.
    pub side: Side,
}

impl<'a> Evidence<'a> {
    /// Evidence about a package the change brought in.
    #[must_use]
    pub fn added(name: &'a str, version: Option<&'a str>) -> Self {
        Evidence {
            name,
            version,
            side: Side::Added,
        }
    }

    /// Evidence about a package the change took away.
    #[must_use]
    pub fn removed(name: &'a str, version: Option<&'a str>) -> Self {
        Evidence {
            name,
            version,
            side: Side::Removed,
        }
    }

    /// Evidence about a package that is still there, differently.
    #[must_use]
    pub fn changed(name: &'a str, version: Option<&'a str>) -> Self {
        Evidence {
            name,
            version,
            side: Side::Either,
        }
    }
}

/// One file's diff, with everything the caller needs to say whether it is the whole answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    /// The path the diff is of.
    pub path: String,
    /// The hunks, in file order.
    pub hunks: Vec<Hunk>,
    /// Whether the caps cut the diff short.
    pub truncated: bool,
    /// Why the diff is partial, or why there is none.
    pub notes: Vec<String>,
    /// Whether either side was missing, which makes the whole file one big add or delete.
    pub one_sided: bool,
}

impl FileDiff {
    /// A diff with nothing in it, for a file that was not diffed.
    #[must_use]
    pub fn none(path: impl Into<String>, note: impl Into<String>) -> FileDiff {
        FileDiff {
            path: path.into(),
            hunks: Vec::new(),
            truncated: false,
            notes: vec![note.into()],
            one_sided: false,
        }
    }
}

/// Diff one file's two sides into hunks.
///
/// `None` on either side is a file added or deleted, which is diffed against nothing rather than skipped —
/// an added lockfile's first hunk is the proof of every package it introduces.
#[must_use]
pub fn diff(path: &str, before: Option<&str>, after: Option<&str>, options: &Options) -> FileDiff {
    let (before, after) = match (before, after) {
        (None, None) => return FileDiff::none(path, "neither side of the diff has this file"),
        (Some(before), None) => (Some(before), Some("")),
        (None, Some(after)) => (Some(""), Some(after)),
        (Some(before), Some(after)) => (Some(before), Some(after)),
    };
    let one_sided = before == Some("") || after == Some("");
    for (side, text) in [("before", before), ("after", after)] {
        if let Some(text) = text
            && text.len() > options.max_file_bytes
        {
            return FileDiff {
                path: path.to_owned(),
                hunks: Vec::new(),
                truncated: true,
                notes: vec![format!(
                    "the {side} side is {} bytes, over the {}-byte diff cap, so no hunk is shown",
                    text.len(),
                    options.max_file_bytes
                )],
                one_sided,
            };
        }
    }
    let before_text = before.unwrap_or("");
    let after_text = after.unwrap_or("");
    let differ = TextDiff::from_lines(before_text, after_text);
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut truncated = false;
    for group in differ.grouped_ops(options.context) {
        if hunks.len() >= options.max_hunks {
            truncated = true;
            continue;
        }
        let index = hunks.len();
        let old_start = group
            .iter()
            .map(|op| op.old_range().start)
            .min()
            .unwrap_or(0);
        let old_end = group.iter().map(|op| op.old_range().end).max().unwrap_or(0);
        let new_start = group
            .iter()
            .map(|op| op.new_range().start)
            .min()
            .unwrap_or(0);
        let new_end = group.iter().map(|op| op.new_range().end).max().unwrap_or(0);
        let mut lines: Vec<String> = Vec::new();
        let mut added = 0usize;
        let mut removed = 0usize;
        let mut hunk_truncated = false;
        for op in &group {
            for change in differ.iter_changes(op) {
                if lines.len() >= options.max_lines_per_hunk {
                    hunk_truncated = true;
                    truncated = true;
                    continue;
                }
                let prefix = match change.tag() {
                    similar::ChangeTag::Equal => " ",
                    similar::ChangeTag::Delete => {
                        removed += 1;
                        "-"
                    }
                    similar::ChangeTag::Insert => {
                        added += 1;
                        "+"
                    }
                };
                lines.push(format!(
                    "{prefix}{}",
                    change.value().trim_end_matches(['\n', '\r'])
                ));
            }
        }
        hunks.push(Hunk {
            index,
            old_start: old_start + 1,
            old_len: old_end.saturating_sub(old_start),
            new_start: new_start + 1,
            new_len: new_end.saturating_sub(new_start),
            header: format!(
                "@@ -{},{} +{},{} @@",
                old_start + 1,
                old_end.saturating_sub(old_start),
                new_start + 1,
                new_end.saturating_sub(new_start)
            ),
            lines,
            added,
            removed,
            truncated: hunk_truncated,
        });
    }
    let mut notes = Vec::new();
    if truncated {
        notes.push(format!(
            "the diff of `{path}` was cut at {} hunks / {} lines per hunk",
            options.max_hunks, options.max_lines_per_hunk
        ));
    }
    if hunks.is_empty() {
        notes.push(format!("`{path}` changed in content but not in any line"));
    }
    FileDiff {
        path: path.to_owned(),
        hunks,
        truncated,
        notes,
        one_sided,
    }
}

/// The hunk that proves an [`Evidence`] claim, if any hunk does.
///
/// Only *changed* lines count as proof: a hunk that shows the package in unchanged context says nothing
/// about this episode. A hunk matching the name *and* the version outranks one matching the name alone, and
/// more matched lines outrank fewer, so a row about `foo 2.1.0` lands on the hunk where `foo 2.1.0` was
/// written rather than on the first hunk that mentions the word `foo`.
#[must_use]
pub fn locate<'a>(diff: &'a FileDiff, evidence: Evidence<'_>) -> Option<&'a Hunk> {
    let mut best: Option<(i64, usize, &Hunk)> = None;
    for hunk in &diff.hunks {
        let score: i64 = hunk
            .lines
            .iter()
            .map(|line| line_points(line, &evidence))
            .sum();
        if score == 0 {
            continue;
        }
        let candidate = (score, hunk.index, hunk);
        match best {
            Some((best_score, best_index, _))
                if best_score > candidate.0
                    || (best_score == candidate.0 && best_index <= candidate.1) => {}
            _ => best = Some(candidate),
        }
    }
    best.map(|(_, _, hunk)| hunk)
}

/// How strongly one diff line evidences one claim.
///
/// A changed line naming the package *and* the version claimed outranks a line naming either, and a line on
/// the side the claim is about outranks one on the other side.
fn line_points(line: &str, evidence: &Evidence<'_>) -> i64 {
    // One character of prefix, then the file's own text — `split_once` on the prefix characters
    // would eat the `+` of a line like `+++ b/Cargo.lock` and call a `node_modules/foo` addition
    // an unchanged line.
    let (sign, body) = line.split_at(line.chars().next().map_or(0, char::len_utf8));
    if !body.contains(evidence.name) {
        return 0;
    }
    let version_hit = evidence
        .version
        .is_some_and(|version| !version.is_empty() && body.contains(version));
    let points = match (sign, evidence.side) {
        ("+", Side::Added | Side::Either) | ("-", Side::Removed | Side::Either) => 4,
        (" ", Side::Either) => 1,
        _ => return 0,
    };
    points + i64::from(version_hit) * 8
}

impl Hunk {
    /// The line in this hunk that best evidences the claim.
    ///
    /// A mass refresh packs a dozen packages into one hunk, so "the hunk that proves it" is only useful
    /// once it is cut round the proving line; this names the line to centre that cut on.
    #[must_use]
    pub fn centre(&self, evidence: &Evidence<'_>) -> Option<usize> {
        let mut best: Option<(i64, usize)> = None;
        for (index, line) in self.lines.iter().enumerate() {
            let points = line_points(line, evidence);
            if points > 0 && best.is_none_or(|(previous, _)| points > previous) {
                best = Some((points, index));
            }
        }
        best.map(|(_, index)| index)
    }
}

/// The hunk, or a one-line statement that there is none — a proof is either shown or explained.
#[must_use]
pub fn explain(diff: &FileDiff, evidence: Evidence<'_>) -> Option<String> {
    locate(diff, evidence).map(Hunk::text)
}

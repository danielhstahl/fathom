//! `fathom_tui::diff::doc` — the rows a hunk pane draws, and only the rows it can see.
//!
//! The pane shows one file at a time (ADR-006 §5.3), and what it shows is a *window*: the file's
//! [`super::model::Hunk`] anchors say which hunks intersect the viewport, those hunks' byte ranges are
//! re-read through [`super::lines`], and [`super::plan`] turns them into rows. A 50 000-line file
//! therefore costs one window of rows, and moving the viewport re-reads rather than accumulates —
//! using the same arithmetic the scanner used, from the same code, so a hunk boundary cannot be in two
//! places at once.
//!
//! # The shapes
//!
//! **Unified** is a gutter of `old new ±` and the line. **Split** is `old ±` on the left, `new ±` on
//! the right, one row per pair, with `·` filling the side that has nothing; it is opt-in and it needs
//! [`super::model::SPLIT_MIN_COLS`] before it will draw at all, because a clipped split is worse than
//! unified (ADR-006 §6.3). A narrow pane drops the line-number gutter rather than the line: a diff
//! hunk never loses a column of *content*, and a truncated one prints `…` (§6.3 again).
//!
//! # The pinned footer
//!
//! The last one or three rows are the footer: the word-diff sentence for the line under the top of the
//! window, the whitespace rule and what it is hiding, and the evidence ids. They are pinned because
//! they are what tells a reviewer whether the window they are reading is the whole story, and a legend
//! that scrolls away is a legend for nothing. In the ADR's own mock they are the two lines at the
//! bottom of the `hunks` pane, which is exactly where they land here.

use std::ops::Range;

use crate::style::{Face, StyledLine};

use super::budget::Budget;
use super::highlight::{self, Language};
use super::lines::LineReader;
use super::model::{FileEntry, Layout, Row, Sign, slot};
use super::plan::Planner;
use super::source::PatchReader;
use super::word;

/// Tabs expand to this many cells before anything else looks at the line, so a Go file's indentation
/// lines up in a grid whose width function counts a tab as zero cells.
const TAB_CELLS: usize = 4;

/// What the pane asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// Unified or split.
    pub layout: Layout,
    /// Whether whitespace-only differences are hidden.
    pub ignore_ws: bool,
    /// Whether intra-line changes are emphasised.
    pub word_diff: bool,
    /// Columns the pane has.
    pub width: usize,
    /// Rows the pane has, footer included.
    pub height: usize,
    /// First doc row to draw.
    pub scroll: usize,
}

impl Request {
    /// The default first request for a pane of `height` rows.
    #[must_use]
    pub const fn new(layout: Layout, width: usize, height: usize) -> Self {
        Request {
            layout,
            ignore_ws: false,
            word_diff: false,
            width,
            height,
            scroll: 0,
        }
    }
}

/// What the pane got.
#[derive(Debug, Clone)]
pub struct Doc {
    /// Exactly `height` lines, padded to `width`.
    pub lines: Vec<StyledLine>,
    /// Rows the whole file's doc has in this shape.
    pub doc_rows: usize,
    /// Rows the scrollable body has, with the footer's taken off.
    pub body_rows: usize,
    /// The first hunk drawn, for the strip and for `[`/`]`.
    pub first_hunk: Option<usize>,
    /// The last hunk drawn.
    pub last_hunk: Option<usize>,
    /// The footer as text — what a golden file and a test read.
    pub footer: Vec<String>,
    /// Anything the build wants said: a window that ran out, a file with no text, a mode-only change.
    pub notes: Vec<String>,
    /// The new-side line number of the first drawn hunk row — where `E` sends `$EDITOR`.
    pub first_line: Option<u32>,
    /// Source lines this build read. The memory claim in `docs/concepts/diff-viewer.md` is about this
    /// number staying near the window's height instead of the file's, and a test asserts on it.
    pub read_lines: usize,
    /// Bytes those lines cost — the number a chunked read is measured by.
    pub read_bytes: usize,
}

impl Doc {
    /// A doc that says there is nothing to draw, with the reason in the first line.
    #[must_use]
    pub fn stated(width: usize, height: usize, lines: &[String]) -> Doc {
        let taken: Vec<StyledLine> = lines
            .iter()
            .take(height.max(1))
            .map(|line| StyledLine::plain(line.clone()).padded(width))
            .collect();
        let count = taken.len();
        Doc {
            doc_rows: count,
            body_rows: count,
            lines: taken,
            first_hunk: None,
            last_hunk: None,
            footer: Vec::new(),
            notes: lines.to_vec(),
            first_line: None,
            read_lines: 0,
            read_bytes: 0,
        }
    }

    /// The first line of the doc, which is the file header — what a copy-path key would want.
    #[must_use]
    pub fn first(&self) -> Option<&str> {
        self.lines.first().map(StyledLine::text)
    }
}

/// The row each hunk's header lands on, in this shape — what `[`/`]` jump between.
#[must_use]
pub fn anchors(file: &FileEntry, layout: Layout, ignore_ws: bool) -> Vec<(usize, usize)> {
    let index = slot(layout, ignore_ws);
    file.hunks
        .iter()
        .enumerate()
        .map(|(hunk, record)| (hunk, record.anchors[index]))
        .collect()
}

/// Rows the file's doc has in this shape.
#[must_use]
pub fn height_of(file: &FileEntry, layout: Layout, ignore_ws: bool) -> usize {
    file.doc_rows(layout, ignore_ws)
}

/// Build the window: read only what the viewport needs, draw only that, and say what was left out.
#[must_use]
pub fn build(
    file: &FileEntry,
    reader: &mut dyn PatchReader,
    request: &Request,
    budget: &Budget,
) -> Doc {
    let width = request.width;
    let height = request.height.max(1);
    let language = Language::for_path(&file.path);
    let evidence = if file.annotation.evidence.is_empty() {
        None
    } else {
        Some(format!("evidence {}", file.annotation.evidence.join(" ")))
    };
    // Three rows are reserved under the body: the word-diff state, the whitespace state, and the row
    // for whatever else this window has to admit — evidence ids, a cap that bound, a read that failed.
    // Reserving it up front is what lets a note discovered mid-window be printed instead of dropped,
    // and a note that is dropped is a fact the pane knows and the reviewer never sees.
    let footer_rows = 3.min(height.saturating_sub(1));
    let body_rows = height.saturating_sub(footer_rows).max(1);
    let index = slot(request.layout, request.ignore_ws);
    let doc_rows = file.doc_rows(request.layout, request.ignore_ws);

    // A file with no line text is a stated line, not an empty pane. Every degenerate case the ticket
    // names lands here: binary, mode-only, oversize, not-available-offline, submodule.
    if let Some(skip) = &file.skip {
        return stated(file, width, height, &skip.statements(), evidence, request);
    }
    if file.hunks.is_empty() {
        let why = match file.kind {
            super::model::FileKind::Renamed => {
                vec!["rename with no edits — the bytes did not change".to_owned()]
            }
            // The tree says `mode 00644→00755` in nine characters; the pane has the room to say it as
            // a sentence, and a mode-only change is exactly the case where a reviewer looking at an
            // empty diff pane assumes the file was untouched.
            super::model::FileKind::ModeOnly => match (&file.mode_old, &file.mode_new) {
                (Some(old), Some(new)) => {
                    vec![format!("mode {old} → {new} — the bytes did not change")]
                }
                _ => vec!["mode change only — the bytes did not change".to_owned()],
            },
            _ => vec!["no hunks — the patch carries no line text".to_owned()],
        };
        return stated(file, width, height, &why, evidence, request);
    }

    let scroll = request.scroll.min(doc_rows.saturating_sub(1));
    let window = scroll..(scroll.saturating_add(body_rows)).min(doc_rows);

    let mut drawn: Vec<StyledLine> = Vec::with_capacity(body_rows);
    let mut notes: Vec<String> = Vec::new();
    let mut first_hunk: Option<usize> = None;
    let mut last_hunk: Option<usize> = None;
    let mut first_line: Option<u32> = None;
    let mut sentence: Option<String> = None;
    let mut read_lines = 0_usize;
    let mut read_bytes = 0_usize;
    let mut clipped = 0_usize;

    if scroll == 0 {
        drawn.push(header(file, width));
    }

    'hunk: for (number, hunk) in file.hunks.iter().enumerate() {
        let base = hunk.anchors[index];
        let end = base + hunk.rows[index];
        if end <= window.start || base >= window.end {
            continue;
        }
        if read_lines >= budget.window_lines || read_bytes >= budget.window_bytes {
            notes.push(format!(
                "the {}-line window budget ran out before hunk {} — `[` and `]` jump to it",
                crate::app::format_thousands(budget.window_lines as u64),
                number + 1
            ));
            break;
        }
        first_hunk = Some(first_hunk.unwrap_or(number));
        last_hunk = Some(number);

        let mut planner = Planner::new(request.layout, request.ignore_ws, budget.pair_run);
        // A `Cell`, not a plain counter: the sink needs to bump it while the loop below also reads it
        // to decide when the hunk is fully drawn, and two mutable borrows of the same counter would
        // be the compiler telling me to think again.
        let emitted = std::cell::Cell::new(0_usize);
        let mut lines = LineReader::new(reader, hunk.byte_start, hunk.byte_end, budget);
        let mut last_sign = Sign::Context;
        let mut stop = false;
        let mut sink = |row: &Row| {
            let seen = emitted.get();
            emitted.set(seen + 1);
            let doc_row = base + seen;
            if doc_row >= window.end || doc_row < window.start {
                return;
            }
            if sentence.is_none()
                && request.word_diff
                && let Some((old, new)) = pair_of(row)
            {
                sentence = Some(word::summary(old, new, 2));
            }
            if first_line.is_none() {
                first_line = line_number(row);
            }
            drawn.push(draw(row, request, language));
        };
        // The hunk's own header row, and the line counters behind it. Skipping this is how a pane
        // draws hunk 2 starting at line 1: the numbers come from the `@@` header, exactly as they do
        // in the scan that counted the rows — which is the other half of why a jump cannot drift.
        planner.begin_hunk(
            number,
            &hunk.header,
            hunk.old_start,
            hunk.new_start,
            &mut sink,
        );
        while !stop {
            let line = match lines.next_line() {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(err) => {
                    notes.push(format!("hunk {} could not be read: {err}", number + 1));
                    break;
                }
            };
            read_lines += 1;
            read_bytes += line.byte_len as usize;
            let oversized = line.oversized;
            if oversized {
                clipped += 1;
            }
            let text = line.text.clone();
            // The carriage return is taken off and carried as a flag, exactly as the scan does it: the
            // two planners must see the *same* strings or the row count and the drawing disagree, and
            // it is the flag that decides the `␍` marker, not a character baked into the text.
            let (body, crlf) = match text.strip_suffix('\r') {
                Some(head) => (head, true),
                None => (text.as_str(), false),
            };
            match text.as_bytes().first().copied() {
                Some(b'+') => {
                    last_sign = Sign::Added;
                    planner.added(
                        expand(body.get(1..).unwrap_or("")),
                        oversized,
                        crlf,
                        &mut sink,
                    );
                }
                Some(b'-') => {
                    last_sign = Sign::Removed;
                    planner.removed(
                        expand(body.get(1..).unwrap_or("")),
                        oversized,
                        crlf,
                        &mut sink,
                    );
                }
                Some(b' ') => {
                    last_sign = Sign::Context;
                    planner.context(
                        expand(body.get(1..).unwrap_or("")),
                        oversized,
                        crlf,
                        &mut sink,
                    );
                }
                Some(b'\\') => planner.no_newline(
                    if last_sign == Sign::Context {
                        Sign::Added
                    } else {
                        last_sign
                    },
                    &mut sink,
                ),
                _ => stop = true,
            }
            if base + emitted.get() >= window.end {
                stop = true;
            }
            if read_lines >= budget.window_lines {
                notes.push(format!(
                    "hunk {} is longer than the {}-line window budget — the rest is one keystroke \
                     away, not missing",
                    number + 1,
                    crate::app::format_thousands(budget.window_lines as u64)
                ));
                break 'hunk;
            }
        }
        planner.end_hunk(&mut sink);
    }

    let mut footer = vec![
        // Telegraphic, because the strip has to survive a 40-column pane: state, the number that
        // follows from it, and the key that changes it. The long form of each sentence is the help
        // overlay's and the palette's job (ADR-006 §6.3: a mode change is confirmed in one line).
        if request.word_diff {
            match &sentence {
                Some(said) => format!("word-diff on · {said} · W off"),
                None => "word-diff on · no pair in view · W off".to_owned(),
            }
        } else {
            "word-diff off · W on".to_owned()
        },
        if request.ignore_ws {
            let hidden: u32 = file.hunks.iter().map(|hunk| hunk.ws_hidden).sum();
            format!(
                "ws ignored · {} pair(s) folded · = shows",
                crate::app::format_thousands(u64::from(hidden))
            )
        } else {
            "ws as written · = ignores CRLF/indent".to_owned()
        },
    ];
    if let Some(evidence) = evidence {
        footer.push(evidence);
    }
    if clipped > 0 {
        // The line cap is a fact about what the pane is hiding, and a clipped tail that looks like the
        // end of the line is the one way this viewer could mislead a reviewer.
        notes.push(format!(
            "{clipped} line(s) clipped at the {} B cap",
            crate::app::format_thousands(budget.max_line_bytes as u64)
        ));
    }
    if !notes.is_empty() {
        footer.push(notes.join(" · "));
    }
    footer.truncate(footer_rows.max(1));

    let mut lines: Vec<StyledLine> = drawn.into_iter().take(body_rows).collect();
    while lines.len() < body_rows {
        lines.push(StyledLine::plain("").padded(width));
    }
    for note in &footer {
        lines.push(StyledLine::with_face(note, Face::Muted).padded(width));
    }
    while lines.len() < height {
        lines.push(StyledLine::plain("").padded(width));
    }

    Doc {
        lines,
        doc_rows,
        body_rows,
        first_hunk,
        last_hunk,
        footer,
        notes,
        first_line,
        read_lines,
        read_bytes,
    }
}

/// The line number a row would open at, preferring the new side — that is the file on disk.
fn line_number(row: &Row) -> Option<u32> {
    match row {
        Row::Context { new, .. } => Some(*new),
        Row::Pair { add, .. } => Some(add.line),
        Row::One { side, .. } => Some(side.line),
        Row::Header { .. } | Row::WhitespaceHidden { .. } | Row::NoNewline { .. } => None,
    }
}

/// A file whose text is not there: header, the stated reason, then blanks.
fn stated(
    file: &FileEntry,
    width: usize,
    height: usize,
    reasons: &[String],
    evidence: Option<String>,
    request: &Request,
) -> Doc {
    let mut lines = vec![header(file, width)];
    for reason in reasons {
        // `file_level_rows` already put the mode statement in `reasons` when there is one, so this
        // function adds no mode-specific words of its own: two places saying "mode 100644 → 100755"
        // differently is exactly the drift this crate keeps refusing.
        lines.push(StyledLine::with_face(reason, Face::Warning).padded(width));
    }
    lines.push(
        StyledLine::with_face(
            format!(
                "{} · {} · {} layout (\\)",
                file.describe(),
                file.counts(),
                request.layout.name()
            ),
            Face::Muted,
        )
        .padded(width),
    );
    if let Some(evidence) = evidence {
        lines.push(StyledLine::with_face(evidence, Face::Muted).padded(width));
    }
    while lines.len() < height {
        lines.push(StyledLine::plain("").padded(width));
    }
    lines.truncate(height);
    Doc {
        footer: lines
            .iter()
            .skip(1)
            .map(|line| line.text().to_owned())
            .collect(),
        doc_rows: 1,
        body_rows: 1,
        lines,
        first_hunk: None,
        last_hunk: None,
        notes: reasons.to_vec(),
        first_line: None,
        read_lines: 0,
        read_bytes: 0,
    }
}

/// Expand tabs and make `\r` visible, before any byte range is computed on the text.
fn expand(text: &str) -> String {
    if !text.contains(['\t', '\r']) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\t' => out.push_str(&" ".repeat(TAB_CELLS)),
            '\r' => out.push('␍'),
            other => out.push(other),
        }
    }
    out
}

/// The file's own header row: path, what happened, the counts, and the key that moves on.
fn header(file: &FileEntry, width: usize) -> StyledLine {
    let mut line = StyledLine::new();
    line.push(Face::Path, &file.path);
    line.push_str("  ");
    line.push(Face::Marker, file.marker());
    if let Some(modes) = file.mode_change() {
        line.push_str(" ");
        line.push(Face::Marker, &modes);
    }
    line.push_str(" ");
    line.push(Face::Emphasis, &file.counts());
    if let Some(note) = file.rename_note() {
        line.push_str("  ");
        line.push(Face::Meta, &note);
    }
    // Which highlighter was assumed, printed as words. The colour of a keyword is decoration, and in
    // `--plain`, on a monochrome terminal, or through a screen reader the colour is gone — a reviewer
    // reading a C file highlighted as shell should be able to tell that is what happened.
    line.push_str("  ");
    line.push(Face::Meta, highlight::Language::for_path(&file.path).name());
    line.padded(width)
}

/// The two sides a row compares, when it compares two things.
fn pair_of(row: &Row) -> Option<(&str, &str)> {
    match row {
        Row::Pair { del, add } => Some((&del.text, &add.text)),
        Row::One {
            pair: Some(other),
            side,
            ..
        } => {
            Some((&other.text, &side.text)) // (old, new) regardless of which half this row is
        }
        _ => None,
    }
}

/// One row, drawn and clipped to the pane.
fn draw(row: &Row, request: &Request, language: Language) -> StyledLine {
    match request.layout {
        Layout::Unified => unified(row, request.width, request, language),
        Layout::Split => split(row, request.width, request, language),
    }
}

/// The `old new ±` gutter, or just `±` when the pane is too narrow for numbers.
fn gutter(old: Option<u32>, new: Option<u32>, sign: Sign, numbers: bool) -> StyledLine {
    let mut line = StyledLine::new();
    if numbers {
        line.push(
            Face::Muted,
            &format!(
                "{:>4} {:>4} ",
                old.map_or_else(String::new, |number| number.to_string()),
                new.map_or_else(String::new, |number| number.to_string()),
            ),
        );
    }
    line.push(sign.face(), &format!("{} ", sign.glyph()));
    line
}

/// One side's text with its faces: the line's own colour, the syntax on top of it, and the
/// word-diff emphasis on top of both.
fn side(
    text: &str,
    base: Face,
    language: Language,
    ranges: &[Range<usize>],
    word_face: Face,
) -> StyledLine {
    let mut line = StyledLine::new();
    line.push(base, text);
    for (range, face) in highlight::tokens(text, language) {
        line.override_face(range, face);
    }
    for range in ranges {
        line.override_face(range.clone(), word_face);
    }
    line
}

fn unified(row: &Row, width: usize, request: &Request, language: Language) -> StyledLine {
    let numbers = width >= 44;
    match row {
        Row::Header { text, .. } => StyledLine::with_face(text, Face::HunkHeader).padded(width),
        Row::Context {
            old,
            new,
            text,
            truncated,
            crlf,
        } => {
            let mut out = gutter(Some(*old), Some(*new), Sign::Context, numbers);
            out.push_line(&tail(text, *truncated, *crlf, Sign::Context, language, &[]));
            out.padded(width)
        }
        Row::Pair { del, add } => {
            // Defensive: the unified planner splits pairs into two `One` rows, so this only runs if
            // a caller hands a split row to a unified pane. Draw the removed half, say why.
            let _ = add;
            let mut out = gutter(Some(del.line), None, Sign::Removed, numbers);
            out.push_line(&tail(
                &del.text,
                del.truncated,
                del.crlf,
                Sign::Removed,
                language,
                &[],
            ));
            out.padded(width)
        }
        Row::One { sign, side, pair } => {
            let (old, new) = match sign {
                Sign::Removed => (Some(side.line), None),
                Sign::Added => (None, Some(side.line)),
                Sign::Context => (Some(side.line), Some(side.line)),
            };
            let ranges = match (request.word_diff, pair) {
                (true, Some(other)) => {
                    let (removed, added) = word::regions(
                        if *sign == Sign::Removed {
                            &side.text
                        } else {
                            &other.text
                        },
                        if *sign == Sign::Removed {
                            &other.text
                        } else {
                            &side.text
                        },
                    );
                    if *sign == Sign::Removed {
                        removed
                    } else {
                        added
                    }
                }
                _ => Vec::new(),
            };
            let mut out = gutter(old, new, *sign, numbers);
            out.push_line(&tail(
                &side.text,
                side.truncated,
                side.crlf,
                *sign,
                language,
                &ranges,
            ));
            out.padded(width)
        }
        Row::WhitespaceHidden { count } => hidden_line(*count, width),
        Row::NoNewline { sign } => StyledLine::with_face(
            format!("{} no newline at end of file", sign.glyph()),
            Face::Whitespace,
        )
        .padded(width),
    }
}

/// The line's text with its marker faces, after the gutter.
fn tail(
    text: &str,
    truncated: bool,
    crlf: bool,
    sign: Sign,
    language: Language,
    word_ranges: &[Range<usize>],
) -> StyledLine {
    let face = if sign == Sign::Added {
        Face::WordNew
    } else if sign == Sign::Removed {
        Face::WordOld
    } else {
        Face::Text
    };
    let mut line = side(text, sign.face(), language, word_ranges, face);
    if crlf {
        line.push(Face::Whitespace, "␍");
    }
    if truncated {
        line.push(Face::Muted, "…");
    }
    line
}

fn hidden_line(count: u32, width: usize) -> StyledLine {
    let pairs = if count == 1 { "pair" } else { "pairs" };
    StyledLine::with_face(
        format!(
            "≈ {} whitespace-only {pairs} (= shows)",
            crate::app::format_thousands(u64::from(count))
        ),
        Face::Whitespace,
    )
    .padded(width)
}

fn split(row: &Row, width: usize, request: &Request, language: Language) -> StyledLine {
    let side_width = width.saturating_sub(1) / 2;
    let numbers = side_width >= 24;
    let (left, right): (StyledLine, StyledLine) = match row {
        Row::Header { text, .. } => {
            // A `@@` line has no old half and new half: it spans the row.
            return StyledLine::with_face(text, Face::HunkHeader).padded(width);
        }
        Row::Context {
            old,
            new,
            text,
            truncated,
            crlf,
        } => (
            half(
                *old,
                Sign::Context,
                text,
                *truncated,
                *crlf,
                language,
                &[],
                side_width,
                numbers,
            ),
            half(
                *new,
                Sign::Context,
                text,
                *truncated,
                *crlf,
                language,
                &[],
                side_width,
                numbers,
            ),
        ),
        Row::Pair { del, add } => {
            let (removed, added) = if request.word_diff {
                word::regions(&del.text, &add.text)
            } else {
                (Vec::new(), Vec::new())
            };
            (
                half(
                    del.line,
                    Sign::Removed,
                    &del.text,
                    del.truncated,
                    del.crlf,
                    language,
                    &removed,
                    side_width,
                    numbers,
                ),
                half(
                    add.line,
                    Sign::Added,
                    &add.text,
                    add.truncated,
                    add.crlf,
                    language,
                    &added,
                    side_width,
                    numbers,
                ),
            )
        }
        Row::One { sign, side, pair } => {
            let (removed, added) = match (request.word_diff, sign, pair) {
                (true, Sign::Removed, Some(other)) => {
                    let (a, b) = word::regions(&side.text, &other.text);
                    (a, b)
                }
                (true, Sign::Added, Some(other)) => {
                    let (a, b) = word::regions(&other.text, &side.text);
                    (a, b)
                }
                _ => (Vec::new(), Vec::new()),
            };
            match sign {
                Sign::Added => (
                    filler(side_width),
                    half(
                        side.line,
                        Sign::Added,
                        &side.text,
                        side.truncated,
                        side.crlf,
                        language,
                        &added,
                        side_width,
                        numbers,
                    ),
                ),
                _ => (
                    half(
                        side.line,
                        *sign,
                        &side.text,
                        side.truncated,
                        side.crlf,
                        language,
                        &removed,
                        side_width,
                        numbers,
                    ),
                    filler(side_width),
                ),
            }
        }
        Row::WhitespaceHidden { count } => (
            StyledLine::with_face(
                format!(
                    "≈ {} pair(s) hidden by the whitespace rule (W)",
                    crate::app::format_thousands(u64::from(*count))
                ),
                Face::Whitespace,
            )
            .padded(side_width),
            StyledLine::plain("").padded(side_width),
        ),
        Row::NoNewline { sign } => (
            StyledLine::with_face(format!("{} no newline", sign.glyph()), Face::Whitespace)
                .padded(side_width),
            StyledLine::plain("").padded(side_width),
        ),
    };
    let mut out = StyledLine::new();
    out.push_line(&left.padded(side_width));
    out.push(Face::Muted, "│");
    out.push_line(&right.padded(side_width));
    out.padded(width)
}

/// One half of a split row, already clipped to `width`.
#[allow(clippy::too_many_arguments)]
fn half(
    line: u32,
    sign: Sign,
    text: &str,
    truncated: bool,
    crlf: bool,
    language: Language,
    ranges: &[Range<usize>],
    width: usize,
    numbers: bool,
) -> StyledLine {
    let mut out = StyledLine::new();
    if numbers {
        out.push(Face::Muted, &format!("{line:>4}"));
    }
    out.push_str(" ");
    out.push(sign.face(), &sign.glyph().to_string());
    out.push_str(" ");
    out.push_line(&tail(text, truncated, crlf, sign, language, ranges));
    out.padded(width)
}

/// A marker and blanks, for the side of a split row that has nothing on it.
fn filler(width: usize) -> StyledLine {
    let mut line = StyledLine::new();
    line.push(Face::Faint, "·");
    line.padded(width)
}

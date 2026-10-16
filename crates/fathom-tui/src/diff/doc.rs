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
use std::sync::Arc;

use crate::materialize::Cancel;
use crate::style::{Face, StyledLine};

use super::budget::Budget;
use super::highlight::{self, Language};
use super::lines::LineReader;
use super::model::{FileEntry, Layout, Row, Shape, Sign, slot};
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
    /// The shape these rows were drawn for. A frame checks this against what it asked for: content
    /// from another shape is stale content, and `e1.8`'s last criterion is that it never appears.
    pub shape: Shape,
    /// Hunks this window asked for and did not get — the count behind a `⋯ decoding` row. Zero means
    /// the window is complete.
    pub pending: usize,
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
            shape: Shape::of(Layout::Unified, false),
            pending: 0,
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

/// How a request splits its height: `(body rows, footer rows)`.
///
/// Three rows are reserved under the body: the word-diff state, the whitespace state, and the row for
/// whatever else this window has to admit — evidence ids, a cap that bound, a read that failed, a
/// decode that has not arrived. Reserving them up front is what lets a note discovered mid-window be
/// printed instead of dropped, and a note that is dropped is a fact the pane knows and the reviewer
/// never sees.
fn viewport(request: &Request) -> (usize, usize) {
    let height = request.height.max(1);
    let footer_rows = 3.min(height.saturating_sub(1));
    (height.saturating_sub(footer_rows).max(1), footer_rows)
}

/// Which hunks a request's viewport intersects, as `(first, one past last)`.
///
/// The queue asks this before it submits, which is what "priority to the visible window" is a priority
/// *of*: the answer is derived from the same anchors the drawing is derived from, so a hunk is visible
/// to the scheduler exactly when it is visible on screen.
#[must_use]
pub fn window_hunks(file: &FileEntry, request: &Request) -> (usize, usize) {
    if file.hunks.is_empty() {
        return (0, 0);
    }
    let (body_rows, _) = viewport(request);
    let index = Shape::of(request.layout, request.ignore_ws).slot();
    let doc_rows = file.doc_rows(request.layout, request.ignore_ws);
    if doc_rows == 0 {
        return (0, 0);
    }
    let scroll = request.scroll.min(doc_rows.saturating_sub(1));
    let end = scroll.saturating_add(body_rows).min(doc_rows);
    let mut first: Option<usize> = None;
    let mut last = 0_usize;
    for (number, hunk) in file.hunks.iter().enumerate() {
        let base = hunk.anchors[index];
        if base + hunk.rows[index] <= scroll || base >= end {
            continue;
        }
        if first.is_none() {
            first = Some(number);
        }
        last = number + 1;
    }
    (first.unwrap_or(0), last)
}

/// One hunk, decoded into rows for one shape.
///
/// This is the unit `e1.8` moves off the UI thread. Reading a hunk's bytes and running them through
/// [`Planner`] is the expensive half of drawing a pane, and it depends on nothing the frame owns:
/// not the width, not the scroll, not the syntax highlighter, not whether word-diff is on. The rows
/// it holds are exactly the rows [`build_from`] draws, which is why a cached decode and a
/// synchronous one cannot disagree — the same argument [`super::plan`] makes about the scan and the
/// pane, one level down.
#[derive(Debug, Clone)]
pub struct Decoded {
    /// Which hunk this is, echoed back so a pane can check it got what it asked for.
    pub hunk: usize,
    /// The rows, in doc order, starting at the hunk's own anchor row.
    pub rows: Vec<Row>,
    /// Source lines read to make them.
    pub read_lines: usize,
    /// Bytes those lines cost.
    pub read_bytes: usize,
    /// Lines the length cap cut.
    pub clipped: usize,
    /// What the decode wants said: a cap that bound, a read that failed.
    pub notes: Vec<String>,
    /// Did the decode finish? `false` means a partial, and a partial is printed as one.
    pub complete: bool,
    /// Was it stopped because somebody cancelled it? A cancelled decode is not a failure, and this
    /// is the only place that distinction survives.
    pub cancelled: bool,
}

impl Decoded {
    /// The row overhead the cache's byte arithmetic uses: the `Row` enum, its `Vec`, the two line
    /// numbers and the allocation each `String` drags along. An estimate that says what it ignores,
    /// like the scan's.
    const ROW_OVERHEAD: usize = 64;

    /// Bytes this retains, as the decode cache counts them.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.rows
            .iter()
            .map(|row| Self::ROW_OVERHEAD + row_text_bytes(row))
            .sum::<usize>()
            + self.notes.iter().map(String::len).sum::<usize>()
            + self.hunk.to_string().len()
    }

    /// A decode that produced nothing, with the reason.
    #[must_use]
    pub fn empty(hunk: usize, note: impl Into<String>) -> Decoded {
        Decoded {
            hunk,
            rows: Vec::new(),
            read_lines: 0,
            read_bytes: 0,
            clipped: 0,
            notes: vec![note.into()],
            complete: true,
            cancelled: false,
        }
    }

    /// A decode that was told to stop.
    #[must_use]
    pub fn cancelled(hunk: usize) -> Decoded {
        Decoded {
            hunk,
            rows: Vec::new(),
            read_lines: 0,
            read_bytes: 0,
            clipped: 0,
            notes: Vec::new(),
            complete: false,
            cancelled: true,
        }
    }
}

impl crate::materialize::Cost for Decoded {
    fn cost(&self) -> usize {
        self.retained_bytes()
    }
}

/// Bytes of text a row holds.
fn row_text_bytes(row: &Row) -> usize {
    match row {
        Row::Header { text, .. } => text.len(),
        Row::Context { text, .. } => text.len(),
        Row::Pair { del, add } => del.text.len() + add.text.len(),
        Row::One {
            side,
            pair,
            sign: _,
        } => side.text.len() + pair.as_ref().map_or(0, |other| other.text.len()),
        Row::WhitespaceHidden { .. } | Row::NoNewline { .. } => 8,
    }
}

/// What to read for one hunk: the offsets and the header the scan kept, and nothing else.
///
/// A decode input that carries no `FileEntry` and no `Scan` is what lets it run on a worker: the key
/// a job carries is self-contained (ADR-002 §3 — a worker holds no shared index, so there is nothing
/// to lock and nothing to go stale under it). `Ord + Hash` for the same reason: it is the cache key's
/// payload.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Plan {
    /// Which hunk of the file, echoed back with the rows.
    pub hunk: usize,
    /// The `@@ … @@` header, verbatim.
    pub header: String,
    /// Byte offset of the first line after the header.
    pub byte_start: u64,
    /// Byte offset one past the hunk's last line.
    pub byte_end: u64,
    /// First old line number.
    pub old_start: u32,
    /// First new line number.
    pub new_start: u32,
}

impl Plan {
    /// The plan for one hunk of an index entry.
    #[must_use]
    pub fn of(hunk: usize, record: &super::model::Hunk) -> Self {
        Plan {
            hunk,
            header: record.header.clone(),
            byte_start: record.byte_start,
            byte_end: record.byte_end,
            old_start: record.old_start,
            new_start: record.new_start,
        }
    }
}

/// Read one hunk and turn it into the rows one shape draws.
///
/// This is the whole cost of a pane, and it is the part that does not need a screen: no width, no
/// scroll, no highlighter, no word-diff. `cancel` is checked once per line — an atomic load, cheaper
/// than the byte it stops the reader from reading — and a decode that stops because of it returns
/// [`Decoded::cancelled`] rather than a partial that would be cached as if it were whole.
///
/// `limit` is how many rows the caller can actually draw — a pane asking for one window passes the
/// rows between the top of its viewport and the hunk's end, and stops reading there instead of
/// decoding a 10 000-line hunk to show 20 lines of it. `None` means "the whole hunk", which is what a
/// worker decoding for the cache passes, because a cached value has to serve every window that later
/// intersects it. Stopping at a limit is not a partial: the decode finished what was asked.
///
/// `Decoded::complete` is `false` for any decode that stopped early for anyone *else's* reason — a
/// budget that bound, a read that failed — and `true` for a complete hunk and for a stopped-at-limit
/// window.
#[must_use]
pub fn decode(
    plan: &Plan,
    reader: &mut dyn PatchReader,
    shape: Shape,
    budget: &Budget,
    cancel: Option<&Cancel>,
    limit: Option<usize>,
) -> Decoded {
    let hunk = plan.hunk;
    let mut planner = Planner::new(shape.layout, shape.ignore_ws, budget.pair_run);
    let mut lines = LineReader::new(reader, plan.byte_start, plan.byte_end, budget);
    let mut out: Vec<Row> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut read_lines = 0_usize;
    let mut read_bytes = 0_usize;
    let mut clipped = 0_usize;
    let mut last_sign = Sign::Context;
    let mut stop = false;
    // `false` until something makes the loop quit before the hunk's last line. A partial decode is a
    // fact about the rows in here, so it travels with them rather than being inferred downstream.
    let mut partial = false;
    // A `Cell`, not a plain counter: the sink bumps it while the loop below also reads it to decide
    // when the window is full, and two mutable borrows of the same counter would be the compiler
    // telling me to think again.
    let emitted = std::cell::Cell::new(0_usize);
    let mut sink = |row: &Row| {
        emitted.set(emitted.get() + 1);
        out.push(row.clone());
    };
    // The hunk's own header row, and the line counters behind it. Skipping this is how a pane draws
    // hunk 2 starting at line 1: the numbers come from the `@@` header, exactly as they do in the
    // scan that counted the rows — which is the other half of why a jump cannot drift.
    planner.begin_hunk(
        hunk,
        &plan.header,
        plan.old_start,
        plan.new_start,
        &mut sink,
    );
    while !stop {
        if cancel.is_some_and(|token| token.is_cancelled()) {
            return Decoded::cancelled(hunk);
        }
        let line = match lines.next_line() {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(err) => {
                notes.push(format!("hunk {} could not be read: {err}", hunk + 1));
                partial = true;
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
        // two planners must see the *same* strings or the row count and the drawing disagree, and it
        // is the flag that decides the `␍` marker, not a character baked into the text.
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
            // Anything else is the end of the hunk: the next file's header, or a patch that stopped.
            _ => stop = true,
        }
        if limit.is_some_and(|rows| emitted.get() >= rows) {
            // The bottom of the window is here. Reading on would be the pane paying for rows it has
            // no room to draw, which is the guarantee `a_window_build_reads_only_the_lines_it_draws`
            // holds this function to.
            break;
        }
        if read_lines >= budget.window_lines {
            notes.push(format!(
                "hunk {} is longer than the {}-line window budget — the rest is one keystroke \
                 away, not missing",
                hunk + 1,
                crate::app::format_thousands(budget.window_lines as u64)
            ));
            partial = true;
            stop = true;
        }
    }
    planner.end_hunk(&mut sink);
    Decoded {
        hunk,
        rows: out,
        read_lines,
        read_bytes,
        clipped,
        notes,
        complete: !partial,
        cancelled: false,
    }
}

/// Where the pane gets a hunk's rows from.
///
/// Two implementations, one code path: [`Live`] reads them now, on the UI thread, which is what a
/// `--plain` run and every test that has not started a worker does; the engine behind
/// [`super::materialize`] hands over what a worker already decoded and is honest about the rest.
pub trait Hunks {
    /// The rows for hunk `n`, if they are here now.
    fn decoded(&mut self, n: usize) -> Option<Arc<Decoded>>;

    /// Is it still on the way? `true` prints `decoding`; `false` prints
    /// [`Hunks::why_missing`], and neither one prints an empty rectangle.
    fn coming(&mut self, _n: usize) -> bool {
        false
    }

    /// Why it is not here, in the words the footer prints.
    fn why_missing(&mut self, _n: usize) -> Option<String> {
        None
    }

    /// One line about the source as a whole — how many hunks are queued, say — for the footer.
    fn note(&self) -> Option<String> {
        None
    }
}

/// The synchronous source: decode on demand, exactly as the pane did before `e1.8`.
///
/// The window budget is enforced *across* the hunks one window touches, which is what the old
/// single-pass code did and what the test that says a window reads only the lines it draws asserts.
pub struct Live<'r> {
    file: &'r FileEntry,
    reader: &'r mut dyn PatchReader,
    request: Request,
    shape: Shape,
    budget: &'r Budget,
    read: usize,
    stopped_at: Option<usize>,
}

impl<'r> Live<'r> {
    /// A source that reads through `reader` for `shape`.
    #[must_use]
    pub fn new(
        file: &'r FileEntry,
        reader: &'r mut dyn PatchReader,
        request: Request,
        shape: Shape,
        budget: &'r Budget,
    ) -> Self {
        Live {
            file,
            reader,
            request,
            shape,
            budget,
            read: 0,
            stopped_at: None,
        }
    }

    /// How many of hunk `n`'s rows the viewport can actually draw — the read limit that keeps a
    /// window reading the lines it draws instead of the hunk it happens to land in.
    fn need(&self, n: usize) -> usize {
        let (body_rows, _) = viewport(&self.request);
        let index = self.shape.slot();
        let doc_rows = self
            .file
            .doc_rows(self.request.layout, self.request.ignore_ws);
        let scroll = self.request.scroll.min(doc_rows.saturating_sub(1));
        let end = scroll.saturating_add(body_rows).min(doc_rows);
        self.file.hunks.get(n).map_or(body_rows, |hunk| {
            end.saturating_sub(hunk.anchors[index]).max(1)
        })
    }
}

impl Hunks for Live<'_> {
    fn decoded(&mut self, n: usize) -> Option<Arc<Decoded>> {
        if self.read >= self.budget.window_lines {
            self.stopped_at = Some(n);
            return None;
        }
        let Some(record) = self.file.hunks.get(n) else {
            return Some(Arc::new(Decoded::empty(
                n,
                format!("this file has no hunk {}", n + 1),
            )));
        };
        let plan = Plan::of(n, record);
        let need = self.need(n).max(1);
        let decoded = decode(
            &plan,
            self.reader,
            self.shape,
            self.budget,
            None,
            Some(need),
        );
        self.read += decoded.read_lines;
        Some(Arc::new(decoded))
    }

    fn why_missing(&mut self, n: usize) -> Option<String> {
        (self.stopped_at == Some(n)).then(|| {
            format!(
                "the {}-line window budget ran out before hunk {} — `[` and `]` jump to it",
                crate::app::format_thousands(self.budget.window_lines as u64),
                n + 1
            )
        })
    }
}

/// Build the window: read only what the viewport needs, draw only that, and say what was left out.
#[must_use]
pub fn build(
    file: &FileEntry,
    reader: &mut dyn PatchReader,
    request: &Request,
    budget: &Budget,
) -> Doc {
    let shape = Shape::of(request.layout, request.ignore_ws);
    let mut live = Live::new(file, reader, *request, shape, budget);
    build_from(file, request, budget, &mut live)
}

/// The same window, drawn from whatever [`Hunks`] can produce right now.
///
/// This is the function `e1.8` exists to make possible: the frame calls it once per draw, it never
/// blocks, and a hunk it does not have becomes a stated row rather than a wait or a lie.
#[must_use]
pub fn build_from(
    file: &FileEntry,
    request: &Request,
    budget: &Budget,
    hunks: &mut dyn Hunks,
) -> Doc {
    let width = request.width;
    let height = request.height.max(1);
    let language = Language::for_path(&file.path);
    let evidence = if file.annotation.evidence.is_empty() {
        None
    } else {
        Some(format!("evidence {}", file.annotation.evidence.join(" ")))
    };
    let (body_rows, footer_rows) = viewport(request);
    let shape = Shape::of(request.layout, request.ignore_ws);
    let index = shape.slot();
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
    let mut pending = 0_usize;
    let mut queued = 0_usize;

    if scroll == 0 {
        drawn.push(header(file, width));
    }

    for (number, record) in file.hunks.iter().enumerate() {
        let base = record.anchors[index];
        let end = base + record.rows[index];
        if end <= window.start || base >= window.end {
            continue;
        }
        first_hunk = Some(first_hunk.unwrap_or(number));
        last_hunk = Some(number);
        let Some(decoded) = hunks.decoded(number) else {
            // Not here. One row at the hunk's own anchor row — the row the anchors table says it
            // belongs to, so the window does not shift when the real rows land — and the count in
            // the footer. This is the whole "progressively correct" rule: what is on screen is the
            // requested window, with holes that name themselves.
            pending += 1;
            let coming = hunks.coming(number);
            if coming {
                queued += 1;
            }
            let said = match (hunks.why_missing(number), coming) {
                (Some(why), _) => why,
                (None, true) => format!(
                    "⋯ hunk {} ({} row(s)) is being decoded — this window is not complete yet",
                    number + 1,
                    record.rows[index]
                ),
                (None, false) => format!(
                    "hunk {} is not decoded and nothing is fetching it",
                    number + 1
                ),
            };
            if !notes.contains(&said) {
                notes.push(said.clone());
            }
            if base >= window.start {
                drawn.push(StyledLine::with_face(said, Face::Warning).padded(width));
            }
            continue;
        };
        read_lines += decoded.read_lines;
        read_bytes += decoded.read_bytes;
        clipped += decoded.clipped;
        for note in &decoded.notes {
            if !notes.contains(note) {
                notes.push(note.clone());
            }
        }
        for (offset, row) in decoded.rows.iter().enumerate() {
            let doc_row = base + offset;
            if doc_row < window.start || doc_row >= window.end {
                continue;
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
        }
    }
    if let Some(note) = hunks.note()
        && !notes.contains(&note)
    {
        notes.push(note);
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
    if queued > 0 {
        // The decode worker has work outstanding for this window: a count, not a spinner.
        notes.push(format!("{queued} hunk(s) in this window still decoding"));
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
        shape,
        pending,
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
        shape: Shape::of(request.layout, request.ignore_ws),
        pending: 0,
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

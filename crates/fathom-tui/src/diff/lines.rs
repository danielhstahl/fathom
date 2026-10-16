//! `fathom_tui::diff::lines` — the chunked reader every other module in here reads through.
//!
//! This is the module the first acceptance criterion is really about. A line at a time, from a
//! positioned reader, in reads of [`super::Budget::chunk_bytes`]: at no point does the viewer hold
//! more than one chunk plus the line it is assembling, so a 50 000-line patch costs the same to scan
//! as a 50-line one. The scanner walks the whole thing; the pane re-opens the range it needs when the
//! viewport moves. Both go through here so "chunked" is one implementation and not a promise.
//!
//! Two details with teeth:
//!
//! * **A line longer than the cap is cut, not buffered.** `String::from_utf8_lossy` on a 12 MB minified
//!   bundle is how a review tool dies; here the first `max_line_bytes` are kept, the rest is skipped
//!   with the count, and the row says `… 12 345 678 B` so the truncation is visible.
//! * **Bytes that are not UTF-8 survive as replacement characters and a note.** A patch can contain
//!   any bytes at all; dropping the line would be dropping evidence.

use std::io;

use super::budget::Budget;
use super::source::PatchReader;

/// One logical line, with the bookkeeping a scanner needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLine {
    /// The text, newline stripped, non-UTF-8 replaced. A trailing `\r` is **kept**: CRLF churn is a
    /// fact the pane renders, not whitespace to tidy away.
    pub text: String,
    /// Byte offset of the line's first byte.
    pub byte_start: u64,
    /// Bytes the line occupied, including its newline.
    pub byte_len: u64,
    /// The line was longer than the cap and `text` is only its first `max_line_bytes`.
    pub oversized: bool,
    /// Bytes that were skipped because of the cap.
    pub dropped_bytes: usize,
    /// The text contained a NUL byte — the classic "this is not text" tell.
    pub has_nul: bool,
}

/// A sequential line cursor over a byte range of a [`PatchReader`].
pub struct LineReader<'r> {
    reader: &'r mut dyn PatchReader,
    buf: Vec<u8>,
    base: u64,
    fill: usize,
    cursor: u64,
    stop: u64,
    max_line: usize,
    scratch: Vec<u8>,
    /// Lines handed out so far.
    pub lines: u64,
    /// Bytes read from the source so far.
    pub bytes: u64,
}

impl<'r> LineReader<'r> {
    /// Read lines in `[start, stop)`. Pass the source's own length for `stop` when you want it all.
    #[must_use]
    pub fn new(reader: &'r mut dyn PatchReader, start: u64, stop: u64, budget: &Budget) -> Self {
        let chunk = budget.chunk_bytes.max(1);
        LineReader {
            reader,
            buf: vec![0; chunk],
            base: start,
            fill: 0,
            cursor: start,
            stop,
            max_line: budget.max_line_bytes,
            scratch: Vec::new(),
            lines: 0,
            bytes: 0,
        }
    }

    /// The next line, or `None` at the end of the range.
    ///
    /// # Errors
    ///
    /// The reader's own error.
    pub fn next_line(&mut self) -> io::Result<Option<RawLine>> {
        self.scratch.clear();
        let start = self.cursor;
        let mut oversized = false;
        let mut dropped = 0_usize;
        let mut nul = false;
        let mut saw_newline = false;
        while self.cursor < self.stop {
            if self.cursor >= self.base.saturating_add(self.fill as u64) {
                self.base = self.cursor;
                self.fill = self.reader.read_at(self.cursor, &mut self.buf)?;
                self.bytes += self.fill as u64;
                if self.fill == 0 {
                    break;
                }
            }
            let mut index = (self.cursor - self.base) as usize;
            while index < self.fill {
                let byte = self.buf[index];
                index += 1;
                self.cursor += 1;
                if byte == b'\n' {
                    saw_newline = true;
                    break;
                }
                if byte == 0 {
                    nul = true;
                }
                if self.scratch.len() < self.max_line {
                    self.scratch.push(byte);
                } else {
                    oversized = true;
                    dropped += 1;
                }
            }
            if saw_newline {
                break;
            }
        }
        if self.cursor == start {
            return Ok(None);
        }
        self.lines += 1;
        Ok(Some(RawLine {
            text: String::from_utf8_lossy(&self.scratch).into_owned(),
            byte_start: start,
            // `cursor` has already stepped over the newline, so the span is simply what it walked.
            // Adding the newline again here shifted every hunk's byte range one byte late, which is a
            // silent corruption: the first body line arrives without its `+`, `-` or ` `.
            byte_len: self.cursor - start,
            oversized,
            dropped_bytes: dropped,
            has_nul: nul,
        }))
    }
}

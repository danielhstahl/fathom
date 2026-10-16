//! `fathom_tui::diff::budget` — the numbers the diff viewer will not exceed, in one place.
//!
//! `e1.7`'s first acceptance criterion is that a 50 000-line diff file renders **under a documented
//! memory cap**, and "documented" is the operative word: the cap is these constants, they are read by
//! the code that enforces them, and `tests/diff_memory.rs` asserts the measured behaviour against
//! them. `docs/concepts/diff-viewer.md` does the arithmetic in prose.
//!
//! The shape is deliberate. Nothing in the viewer is allowed to hold a whole patch: bytes arrive in
//! [`Budget::chunk_bytes`] pieces and the only retained state is the **index** (one entry per file and
//! per hunk, whatever the file's size) plus the **window** (the lines around the viewport, rebuilt from
//! the source when the viewport moves). That is why the cap does not multiply with the diff, and why
//! `e1.8`'s decode cache can be bolted on without moving a number here.

/// Every ceiling the diff viewer enforces, in bytes and in counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Bytes per read from a [`super::source::PatchReader`] — the "chunked" in "chunked reads".
    pub chunk_bytes: usize,
    /// Longest single line kept. Past this the line is cut and marked `…`; the rest is skipped, not
    /// buffered. A minified JS bundle is a one-line file and must not be an allocation.
    pub max_line_bytes: usize,
    /// Total retained by the index for one patch: file entries plus hunk entries.
    pub index_bytes: usize,
    /// Source lines a single window build will read and analyse (see `e1.8` for making that cheap).
    pub window_lines: usize,
    /// Bytes a single window build may retain for rendered lines.
    pub window_bytes: usize,
    /// File entries kept before the scan stops and says so.
    pub max_files: usize,
    /// Hunk entries kept before the scan stops and says so.
    pub max_hunks: usize,
    /// Longest removed/added run that is still *paired* (for split alignment and for the
    /// whitespace-ignore decision). A longer run is counted exactly but never paired.
    pub pair_run: usize,
}

impl Budget {
    /// The shipped numbers. Every one is justified in `docs/concepts/diff-viewer.md`.
    pub const DEFAULT: Budget = Budget {
        chunk_bytes: 64 * 1024,
        max_line_bytes: 8 * 1024,
        index_bytes: 4 * 1024 * 1024,
        window_lines: 4096,
        window_bytes: 1024 * 1024,
        max_files: 2000,
        max_hunks: 20_000,
        pair_run: 64,
    };

    /// A budget with everything at half, to prove a limit is a limit and not a coincidence.
    #[must_use]
    pub const fn half(self) -> Budget {
        Budget {
            chunk_bytes: self.chunk_bytes / 2,
            max_line_bytes: self.max_line_bytes / 2,
            index_bytes: self.index_bytes / 2,
            window_lines: self.window_lines / 2,
            window_bytes: self.window_bytes / 2,
            max_files: self.max_files / 2,
            max_hunks: self.max_hunks / 2,
            pair_run: self.pair_run / 2,
        }
    }

    /// Worst-case bytes one window build can retain: the line cap times the window, or the window
    /// byte cap, whichever binds first. Stated so a test can assert against the same expression the
    /// code uses instead of a copy of it.
    #[must_use]
    pub const fn window_worst_bytes(&self) -> usize {
        let by_lines = self.window_lines.saturating_mul(self.max_line_bytes);
        if self.window_bytes < by_lines {
            self.window_bytes
        } else {
            by_lines
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Budget::DEFAULT
    }
}

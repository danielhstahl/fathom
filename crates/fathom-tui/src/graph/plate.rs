//! `fathom_tui::graph::plate` — the two-dimensional grid the canvas draws into.
//!
//! The rest of the shell renders rows: [`crate::render`] builds `Vec<String>` and the blit paints it. A
//! graph is not rows — an arrow has to land two columns left and three rows down from the box it left — so
//! the canvas draws into a grid of cells and hands the finished picture back as rows at the end. One rule
//! makes the difference between a picture and a smear:
//!
//! * **A cell knows whether it is load-bearing.** A box, a label or a glyph is *solid*; an arrow is not. An
//!   arrow may cross another arrow, and the crossing prints `┼`. An arrow may not print through a module's
//!   name, and the one that cannot get around it is **counted as undrawn** rather than drawn over the label.
//!   That count is the "counts when a delta exceeds what can be drawn" criterion, and it is a number in the
//!   strip instead of a visual mess on the canvas.
//!
//! Glyphs carry a [`Face`] and not a colour (ADR-006 §10.2): the face is a role, the theme chooses the
//! colour, and `NO_COLOR` strips it at the sink without moving a cell.

use crate::style::{Face, StyledLine};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One cell of the plate.
#[derive(Debug, Clone)]
struct Glyph {
    /// What to print. A space is still a cell.
    ch: char,
    /// The role it plays.
    face: Face,
    /// Whether something may print over it.
    solid: bool,
}

impl Glyph {
    fn blank() -> Self {
        Glyph {
            ch: ' ',
            face: Face::Text,
            solid: false,
        }
    }
}

/// The drawing surface: `cols` × `rows` cells, each with a glyph and a face.
#[derive(Debug, Clone)]
pub struct Plate {
    cols: usize,
    rows: usize,
    cells: Vec<Glyph>,
    /// Arrows that could not be routed because a solid cell was in the way.
    pub undrawn: usize,
    /// Arrows that were drawn.
    pub drawn: usize,
}

impl Plate {
    /// An empty plate of the given size.
    #[must_use]
    pub fn new(cols: usize, rows: usize) -> Self {
        Plate {
            cols,
            rows,
            cells: (0..cols.saturating_mul(rows).max(1))
                .map(|_| Glyph::blank())
                .collect(),
            undrawn: 0,
            drawn: 0,
        }
    }

    /// How many rows the plate has.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// Is this cell inside the plate?
    #[must_use]
    pub fn holds(&self, col: usize, row: usize) -> bool {
        col < self.cols && row < self.rows
    }

    /// Is the cell free for an arrow?
    #[must_use]
    pub fn free(&self, col: usize, row: usize) -> bool {
        !self.holds(col, row) || !self.cells[self.index(col, row)].solid
    }

    fn index(&self, col: usize, row: usize) -> usize {
        row.saturating_mul(self.cols) + col
    }

    /// Print a solid glyph: a box rule, a label, a mark. Solid glyphs overwrite blanks and each other,
    /// because the layout places them without overlap by construction.
    pub fn solid(&mut self, col: usize, row: usize, ch: char, face: Face) {
        if !self.holds(col, row) {
            return;
        }
        let at = self.index(col, row);
        self.cells[at] = Glyph {
            ch,
            face,
            solid: true,
        };
    }

    /// Print text left-to-right starting at `(col, row)`, stopping at the plate's edge. Solid.
    ///
    /// A wide glyph takes two cells and the second is left empty rather than filled with a space, which is
    /// the same rule [`crate::render`] holds for the frame grid: a space in the second cell pushes every
    /// glyph after it one cell right and the picture stops lining up with its own rules.
    pub fn write(&mut self, col: usize, row: usize, text: &str, face: Face) {
        let mut at = col;
        for glyph in text.graphemes(true) {
            let wide = UnicodeWidthStr::width(glyph);
            if at >= self.cols {
                return;
            }
            let ch = glyph
                .chars()
                .next()
                .unwrap_or(if wide > 1 { '…' } else { ' ' });
            self.solid(at, row, ch, face);
            if wide > 1 {
                at += 2;
            } else {
                at += 1;
            }
        }
    }

    /// Can an arrow be printed at this cell?
    #[must_use]
    pub fn arrow_fits(&self, col: usize, row: usize) -> bool {
        self.holds(col, row) && self.free(col, row)
    }

    /// Print an arrow glyph, merging with an arrow already there. Returns `false` when a solid cell was in
    /// the way, which is the caller's cue to count the arrow as undrawn.
    pub fn arrow(&mut self, col: usize, row: usize, ch: char, face: Face) -> bool {
        if !self.holds(col, row) {
            return false;
        }
        let index = self.index(col, row);
        if self.cells[index].solid {
            return false;
        }
        let here = self.cells[index].ch;
        let merged = if here == ' ' {
            ch
        } else if here == ch {
            here
        } else {
            '┼'
        };
        self.cells[index] = Glyph {
            ch: merged,
            face,
            solid: false,
        };
        true
    }

    /// A run of arrow glyphs on one row, `[from, to]` inclusive, pointing right when `to > from` and left
    /// otherwise. Fails as soon as a solid cell blocks it, having drawn nothing, so a rejected arrow leaves
    /// no partial mark behind.
    ///
    /// `body` is the line style, which is how a removed edge is *ghosted* rather than merely red: `╌╌╌╌▶` is
    /// a different fact from `────▶` even before the theme is asked for a colour.
    pub fn run(&mut self, from: usize, to: usize, row: usize, face: Face, body: char) -> bool {
        let (low, high) = if from <= to { (from, to) } else { (to, from) };
        for col in low..=high {
            if !self.arrow_fits(col, row) {
                return false;
            }
        }
        for col in low..=high {
            let head = if to >= from { high } else { low };
            let ch = if col == head && high != low {
                if to >= from { '▶' } else { '◀' }
            } else if col == low || col == high {
                '─'
            } else {
                body
            };
            self.arrow(col, row, ch, face);
        }
        true
    }

    /// A vertical arrow run on one column, headed `▼` or `▲` by the direction taken.
    pub fn drop(&mut self, col: usize, from: usize, to: usize, face: Face, body: char) -> bool {
        let (low, high) = if from <= to { (from, to) } else { (to, from) };
        for row in low..=high {
            if !self.arrow_fits(col, row) {
                return false;
            }
        }
        for row in low..=high {
            let head = if to >= from { high } else { low };
            let ch = if row == head && high != low {
                if to >= from { '▼' } else { '▲' }
            } else {
                body
            };
            self.arrow(col, row, ch, face);
        }
        true
    }

    /// The finished picture, as styled rows of exactly `cols` cells.
    #[must_use]
    pub fn lines(&self) -> Vec<StyledLine> {
        (0..self.rows)
            .map(|row| {
                let mut line = StyledLine::new();
                for col in 0..self.cols {
                    let glyph = &self.cells[self.index(col, row)];
                    line.push(glyph.face, &glyph.ch.to_string());
                }
                line
            })
            .collect()
    }
}

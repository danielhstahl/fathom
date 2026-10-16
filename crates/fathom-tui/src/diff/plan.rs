//! `fathom_tui::diff::plan` — one hunk's lines in, drawn rows out, and the only place row counts
//! are decided.
//!
//! The scanner needs row counts it can never exceed (so that `[`/`]` can jump to a hunk without
//! materialising the hunks before it) and the pane needs the rows themselves. Both come from this one
//! state machine, which is the reason they cannot disagree: the scan feeds it every line and counts
//! what it emits, the pane feeds it the lines around the viewport and keeps what it emits. If pairing
//! ever changes, the anchors move with it automatically — the alternative is two implementations of
//! "how many rows is this hunk", and that is a bug with a timer on it.
//!
//! The state machine buffers a *change block*: the run of removed and added lines between two context
//! lines. That is what makes pairing possible at all, and it is also the only thing that can make the
//! scan's memory grow with a hunk's size — so a block longer than [`super::Budget::pair_run`] lines
//! stops being buffered and is emitted line by line, unpaired. The counts stay exact either way; what
//! the cap costs is intra-hunk pairing, which is a display nicety, and it is stated in the pane.

use super::model::{Row, Side, Sign};

/// Rows this pass will emit, one call per drawn row.
pub type Sink<'a> = &'a mut dyn FnMut(&Row);

/// The two-line test the whitespace rule applies: different text, same text once whitespace is gone.
#[must_use]
pub fn ws_only(a: &str, b: &str) -> bool {
    if a == b {
        // Identical once compared: the only thing between the two sides is the whitespace we were
        // asked to ignore, which is the case this whole function exists to catch.
        return true;
    }
    let mut left = a.chars().filter(|c| !c.is_whitespace());
    let mut right = b.chars().filter(|c| !c.is_whitespace());
    loop {
        match (left.next(), right.next()) {
            (Some(x), Some(y)) if x == y => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// One pass over one file's hunk lines.
pub struct Planner {
    layout: super::model::Layout,
    ignore_ws: bool,
    slot: usize,
    pair_run: usize,
    old: u32,
    new: u32,
    dels: Vec<Side>,
    adds: Vec<Side>,
    /// This block outgrew the buffer, so the rest of it arrives unpaired.
    untracked: bool,
    /// Pairs the whitespace rule hid, total for this file.
    pub hidden: u32,
}

impl Planner {
    /// A pass for one shape of the pane.
    #[must_use]
    pub const fn new(layout: super::model::Layout, ignore_ws: bool, pair_run: usize) -> Self {
        Planner {
            layout,
            ignore_ws,
            slot: super::model::slot(layout, ignore_ws),
            pair_run,

            old: 1,
            new: 1,
            dels: Vec::new(),
            adds: Vec::new(),
            untracked: false,
            hidden: 0,
        }
    }

    /// Which of the four shapes this pass is for — see [`super::model::slot`].
    #[must_use]
    pub const fn slot(&self) -> usize {
        self.slot
    }

    /// Start a hunk: closes any open block, then emits the header row.
    pub fn begin_hunk(
        &mut self,
        index: usize,
        header: &str,
        old_start: u32,
        new_start: u32,
        sink: Sink<'_>,
    ) {
        self.end_hunk(sink);
        self.old = old_start;
        self.new = new_start;
        sink(&Row::Header {
            hunk: index,
            text: header.to_owned(),
        });
    }

    /// Close the hunk: an unfinished change block belongs to it, so it flushes here.
    pub fn end_hunk(&mut self, sink: Sink<'_>) {
        self.flush_block(sink);
        self.untracked = false;
    }

    /// A context line.
    pub fn context(&mut self, text: String, truncated: bool, crlf: bool, sink: Sink<'_>) {
        self.flush_block(sink);
        self.untracked = false;
        let (old, new) = (self.old, self.new);
        self.old += 1;
        self.new += 1;
        sink(&Row::Context {
            old,
            new,
            text,
            truncated,
            crlf,
        });
    }

    /// A `-` line.
    pub fn removed(&mut self, text: String, truncated: bool, crlf: bool, sink: Sink<'_>) {
        let line = self.old;
        self.old += 1;
        self.push(
            Sign::Removed,
            Side {
                line,
                text,
                truncated,
                crlf,
            },
            sink,
        );
    }

    /// A `+` line.
    pub fn added(&mut self, text: String, truncated: bool, crlf: bool, sink: Sink<'_>) {
        let line = self.new;
        self.new += 1;
        self.push(
            Sign::Added,
            Side {
                line,
                text,
                truncated,
                crlf,
            },
            sink,
        );
    }

    /// `\ No newline at end of file`, which says something about one side and must be drawn.
    pub fn no_newline(&mut self, sign: Sign, sink: Sink<'_>) {
        self.flush_block(sink);
        sink(&Row::NoNewline { sign });
    }

    fn push(&mut self, sign: Sign, side: Side, sink: Sink<'_>) {
        if self.untracked {
            sink(&Row::One {
                sign,
                side,
                pair: None,
            });
            return;
        }
        match sign {
            Sign::Added => self.adds.push(side),
            Sign::Removed => self.dels.push(side),
            Sign::Context => return,
        }
        if self.dels.len() + self.adds.len() > self.pair_run {
            self.escape(sink);
        }
    }

    /// The block outgrew the buffer: emit what is buffered, unpaired, and stop buffering the rest.
    fn escape(&mut self, sink: Sink<'_>) {
        let dels = std::mem::take(&mut self.dels);
        let adds = std::mem::take(&mut self.adds);
        for side in dels {
            sink(&Row::One {
                sign: Sign::Removed,
                side,
                pair: None,
            });
        }
        for side in adds {
            sink(&Row::One {
                sign: Sign::Added,
                side,
                pair: None,
            });
        }
        self.untracked = true;
    }

    fn flush_block(&mut self, sink: Sink<'_>) {
        if self.untracked {
            self.untracked = false;
            return;
        }
        let dels = std::mem::take(&mut self.dels);
        let adds = std::mem::take(&mut self.adds);
        if dels.is_empty() && adds.is_empty() {
            return;
        }
        let pairs = dels.len().min(adds.len());
        let mut survivors: Vec<usize> = Vec::with_capacity(pairs);
        let mut hidden = 0_u32;
        for index in 0..pairs {
            if self.ignore_ws && ws_only(&dels[index].text, &adds[index].text) {
                hidden += 1;
                continue;
            }
            survivors.push(index);
        }
        match self.layout {
            super::model::Layout::Unified => {
                // One row per line, in git's order — all the removed lines, then all the added ones
                // — each carrying its partner so word-diff still works in this shape.
                for index in survivors.iter().copied().chain(pairs..dels.len()) {
                    sink(&Row::One {
                        sign: Sign::Removed,
                        side: dels[index].clone(),
                        pair: adds.get(index).cloned(),
                    });
                }
                for index in survivors.iter().copied().chain(pairs..adds.len()) {
                    sink(&Row::One {
                        sign: Sign::Added,
                        side: adds[index].clone(),
                        pair: dels.get(index).cloned(),
                    });
                }
            }
            super::model::Layout::Split => {
                for index in survivors {
                    sink(&Row::Pair {
                        del: dels[index].clone(),
                        add: adds[index].clone(),
                    });
                }
                for side in &dels[pairs..] {
                    sink(&Row::One {
                        sign: Sign::Removed,
                        side: side.clone(),
                        pair: None,
                    });
                }
                for side in &adds[pairs..] {
                    sink(&Row::One {
                        sign: Sign::Added,
                        side: side.clone(),
                        pair: None,
                    });
                }
            }
        }
        if hidden > 0 {
            // Stated, never silently dropped (ADR-001 §7.2): the pairs are counted and the key that
            // would show them is named.
            sink(&Row::WhitespaceHidden { count: hidden });
            self.hidden += hidden;
        }
    }
}

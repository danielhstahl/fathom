//! `fathom_tui::diff::model` — the shapes a patch is made of, and what the store says about it.
//!
//! Everything here is a *fact with a source*. The patch text says what happened to the bytes; the
//! [`Annotations`] a source carries says what Fathom's own tables already decided (taxonomy, noise,
//! whether the reviewer has read it, whether the working tree has moved since). Nothing here is
//! re-derived from a path downstream, which is ADR-004 §6.6 R3, and a missing annotation is stored as
//! "not supplied", not guessed at — the same reason the spine prints `unknown` instead of `0`.

use std::fmt;

/// What happened to one file, as the patch's own preamble says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// `new file mode` — the file did not exist before.
    Added,
    /// Content differs on both sides.
    Modified,
    /// `deleted file mode` — the file is gone.
    Deleted,
    /// `rename from`/`rename to`, with or without edits.
    Renamed,
    /// `copy from`/`copy to`.
    Copied,
    /// `old mode`/`new mode` and nothing else: a `chmod`, which is a change with no content.
    ModeOnly,
    /// A file replaced by a symlink, a directory replaced by a file.
    TypeChanged,
    /// `Binary files … differ` or a `GIT binary patch`: there is no line text to show.
    Binary,
    /// A submodule/gitlink bump: a sha changed, no content.
    Submodule,
    /// The preamble said nothing we recognise. Stated, not smoothed over.
    Unknown,
}

impl FileKind {
    /// The tree's one-column-ish marker, as ADR-006 §5.3's mock spells it.
    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            FileKind::Added => "new",
            FileKind::Modified | FileKind::TypeChanged => "~",
            FileKind::Deleted => "del",
            FileKind::Renamed => "ren",
            FileKind::Copied => "cp",
            FileKind::ModeOnly => "mode",
            FileKind::Binary => "bin",
            FileKind::Submodule => "sub",
            FileKind::Unknown => "?",
        }
    }

    /// The same fact in words, for a pane that has room for it.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            FileKind::Added => "new file",
            FileKind::Modified => "modified",
            FileKind::Deleted => "deleted",
            FileKind::Renamed => "renamed",
            FileKind::Copied => "copied",
            FileKind::ModeOnly => "mode change only",
            FileKind::TypeChanged => "type changed",
            FileKind::Binary => "binary",
            FileKind::Submodule => "submodule",
            FileKind::Unknown => "kind unknown",
        }
    }
}

/// Why a file has no hunk text, when that is the case. Every variant is a line the pane prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// The blob is over the materialisation cap, so decoding it is not free.
    Oversize {
        /// Size the source reported, in bytes.
        bytes: u64,
        /// The cap in force when the source decided.
        cap: u64,
    },
    /// The diff is not available offline — the blob is not in the local object store.
    Unavailable {
        /// The source's own words, printed verbatim.
        reason: String,
        /// What to run, if the source knows.
        hint: Option<String>,
    },
    /// There is no text for this change at all (binary, mode-only, submodule).
    NoText,
}

impl Skip {
    /// The word a sentence uses for this: what the file is, not what happened to it.
    #[must_use]
    pub const fn words(&self) -> &'static str {
        match self {
            Skip::Oversize { .. } => "oversize",
            Skip::Unavailable { .. } => "not available offline",
            Skip::NoText => "no line text",
        }
    }

    /// The `oversize`, `off`, `no text` marker in the tree.
    #[must_use]
    pub const fn marker(&self) -> &'static str {
        match self {
            Skip::Oversize { .. } => "big",
            Skip::Unavailable { .. } => "off",
            Skip::NoText => "no text",
        }
    }
}

impl fmt::Display for Skip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Skip::Oversize { bytes, cap } => write!(
                f,
                "over the {} materialisation cap (this file is {}) — hunk text is not decoded",
                crate::app::format_thousands(*cap),
                crate::app::format_thousands(*bytes)
            ),
            Skip::Unavailable { reason, hint } => {
                write!(f, "not available offline — {reason}")?;
                if let Some(hint) = hint {
                    write!(f, " · run: {hint}")?;
                }
                Ok(())
            }
            Skip::NoText => write!(f, "no line text for this change"),
        }
    }
}

impl Skip {
    /// The same fact in lines short enough to read in a 40-column pane.
    ///
    /// A sentence that arrives as `over the 5 242 880 materialis…` has stopped being an explanation.
    /// The pane prints these as separate rows, so each one survives its own clip.
    #[must_use]
    pub fn statements(&self) -> Vec<String> {
        match self {
            Skip::Oversize { bytes, cap } => vec![
                format!(
                    "over the {} materialisation cap",
                    crate::app::format_thousands(*cap)
                ),
                format!(
                    "this file is {} — its text is not decoded",
                    crate::app::format_thousands(*bytes)
                ),
            ],
            Skip::Unavailable { reason, hint } => {
                let mut out = vec![format!("not available offline — {reason}")];
                if let Some(hint) = hint {
                    out.push(format!("run: {hint}"));
                }
                out
            }
            Skip::NoText => vec!["no line text for this change".to_owned()],
        }
    }
}

/// One `@@ -a,b +c,d @@` header and everything under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The header line, verbatim (including the function context git appends).
    pub header: String,
    /// First old line number.
    pub old_start: u32,
    /// Old line count.
    pub old_count: u32,
    /// First new line number.
    pub new_start: u32,
    /// New line count.
    pub new_count: u32,
    /// Byte offset of the first line after the header.
    pub byte_start: u64,
    /// Byte offset one past the hunk's last line.
    pub byte_end: u64,
    /// Rows this hunk occupies, including its own header row, indexed by
    /// [`slot`] for each (layout, whitespace) pair the pane can be in.
    pub rows: [usize; 4],
    /// Where this hunk's header row lands in the file's doc, same indexing as [`Hunk::rows`].
    pub anchors: [usize; 4],
    /// Added lines, counted from the text, not from the header.
    pub adds: u32,
    /// Removed lines, ditto.
    pub dels: u32,
    /// Context lines.
    pub context: u32,
    /// Change pairs the whitespace-ignore rule hides.
    pub ws_hidden: u32,
}

impl Hunk {
    /// The `@@ -12,7 +12,9 @@` summary a strip or a golden line wants.
    #[must_use]
    pub fn short_header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_count, self.new_start, self.new_count
        )
    }
}

/// What Fathom's own tables already know about one path.
///
/// This is the seam `e2.11`'s delta model will fill: the fields are the ones the tree pane is
/// allowed to print (its `reads` list), and every one of them is `Option` because a patch file on
/// its own knows none of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Annotation {
    /// Taxonomy class (`source`, `test`, `lockfile`, …). `None` prints as `unknown`.
    pub class: Option<String>,
    /// Noise class, when the path is noisy. `Some` with a rule means it is hidden until `b`.
    pub noise: Option<String>,
    /// The rule that made it noise, printed with it — ADR-001 §7.2, nothing hidden without a reason.
    pub noise_rule: Option<String>,
    /// The reviewer has already read this file.
    pub read: Option<bool>,
    /// The working tree differs from the indexed commit for this path.
    pub dirty: Option<bool>,
    /// Evidence ids attached to this file (`ev-1 epoch.rs:1-10`).
    pub evidence: Vec<String>,
}

impl Annotation {
    /// The class as the tree prints it: the stored answer, or the word `unknown`.
    #[must_use]
    pub fn class_label(&self) -> &str {
        self.class.as_deref().unwrap_or("unknown")
    }

    /// Is this file noise the tree hides by default?
    #[must_use]
    pub fn is_noise(&self) -> bool {
        self.noise.is_some()
    }

    /// Did the store say anything at all about this file? A file with no hunk text and no annotation is
    /// a preamble artefact, not a change; a file with an annotation is a change whose text somebody
    /// decided not to send, which is a different thing entirely.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.class.is_none()
            && self.noise.is_none()
            && self.read.is_none()
            && self.dirty.is_none()
            && self.evidence.is_empty()
    }
}

/// One file's entry in the tree, and the index of its hunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Repository-relative path after the change (the new name for a rename).
    pub path: String,
    /// The name before, for a rename or a copy.
    pub old_path: Option<String>,
    /// What happened.
    pub kind: FileKind,
    /// Old mode, as git spells it (`100644`).
    pub mode_old: Option<String>,
    /// New mode.
    pub mode_new: Option<String>,
    /// Rename/copy similarity, percent.
    pub similarity: Option<u8>,
    /// Added lines, counted from the text.
    pub adds: u32,
    /// Removed lines.
    pub dels: u32,
    /// The hunks, in order.
    pub hunks: Vec<Hunk>,
    /// Why there is no text, when there is none.
    pub skip: Option<Skip>,
    /// Byte size the patch states for a binary file, when it states one.
    pub bytes: Option<u64>,
    /// Rows this file's doc occupies (header row included), indexed by [`slot`].
    pub rows: [usize; 4],
    /// Annotations from the source.
    pub annotation: Annotation,
    /// Rough bytes of index memory this entry retains.
    pub retained_bytes: usize,
}

impl FileEntry {
    /// The `+412 −90` pair the tree and the header line print.
    #[must_use]
    pub fn counts(&self) -> String {
        match self.skip.as_ref().and_then(|s| match s {
            Skip::Oversize { bytes, .. } => Some(*bytes),
            _ => None,
        }) {
            Some(bytes) => format!("{} B", crate::app::format_thousands(bytes)),
            None => format!("+{} −{}", self.adds, self.dels),
        }
    }

    /// What this file is, as the pane says it: the kind, or — when there is no kind because the patch
    /// sent no text and only a reason — the reason itself. "kind unknown" is the tool confusing
    /// itself; "oversize" is the reviewer being told something.
    #[must_use]
    pub fn describe(&self) -> String {
        match (self.kind, &self.skip) {
            (FileKind::Unknown, Some(skip)) => skip.words().to_owned(),
            _ => self.kind.words().to_owned(),
        }
    }

    /// The tree's one-word marker: what happened, or why there is nothing to show.
    #[must_use]
    pub fn marker(&self) -> &'static str {
        match (self.kind, &self.skip) {
            (FileKind::Unknown, Some(skip)) => skip.marker(),
            _ => self.kind.marker(),
        }
    }

    /// The mode change, spelled out the way the ADR's mock does: `mode 644→755`.
    #[must_use]
    pub fn mode_change(&self) -> Option<String> {
        match (&self.mode_old, &self.mode_new) {
            (Some(old), Some(new)) if old != new => Some(format!(
                "{}→{}",
                old.trim_start_matches('1'),
                new.trim_start_matches('1')
            )),
            _ => None,
        }
    }

    /// How many rows the file's doc gets in a given shape.
    #[must_use]
    pub fn doc_rows(&self, layout: Layout, ignore_ws: bool) -> usize {
        self.rows[slot(layout, ignore_ws)]
    }

    /// The rename note, when the file moved: `← old/path (92%)`.
    #[must_use]
    pub fn rename_note(&self) -> Option<String> {
        let old = self.old_path.as_deref()?;
        Some(match self.similarity {
            Some(score) => format!("← {old} ({score}%)"),
            None => format!("← {old}"),
        })
    }
}

/// Which side of a change a line is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    /// Unchanged context.
    Context,
    /// Present in the new side only.
    Added,
    /// Present in the old side only.
    Removed,
}

impl Sign {
    /// The one-cell gutter glyph.
    #[must_use]
    pub const fn glyph(self) -> char {
        match self {
            Sign::Context => ' ',
            Sign::Added => '+',
            Sign::Removed => '-',
        }
    }

    /// The face the gutter and the whole line carry.
    #[must_use]
    pub const fn face(self) -> crate::style::Face {
        match self {
            Sign::Context => crate::style::Face::Text,
            Sign::Added => crate::style::Face::Added,
            Sign::Removed => crate::style::Face::Removed,
        }
    }
}

/// One side of a hunk line: its number, its text, and whether the text was clipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    /// Line number on that side (1-based; 0 when the header said the side is empty).
    pub line: u32,
    /// The text, without the leading `+`/`-`/` `.
    pub text: String,
    /// The line was longer than the scanner's cap and was cut.
    pub truncated: bool,
    /// The raw line ended in `\\r` — CRLF churn is a fact, not invisible whitespace.
    pub crlf: bool,
}

/// A row the hunk pane draws. One row per drawn line, in both modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// The `@@ … @@` header.
    Header {
        /// Index into the file's hunk list.
        hunk: usize,
        /// The header text.
        text: String,
    },
    /// An unchanged line, shown identically on both sides.
    Context {
        /// Old-side line number.
        old: u32,
        /// New-side line number.
        new: u32,
        /// The text.
        text: String,
        /// The text was clipped by the line cap.
        truncated: bool,
        /// The line carried a trailing `\\r`.
        crlf: bool,
    },
    /// A removed line and the added line it pairs with. `split` draws them side by side, `unified`
    /// draws them one under the other.
    Pair {
        /// The old side.
        del: Side,
        /// The new side.
        add: Side,
    },
    /// A line with nothing to pair it with — or, in unified mode, one half of a pair, with its
    /// partner kept so word-diff still has something to compare against.
    One {
        /// Which side.
        sign: Sign,
        /// The line.
        side: Side,
        /// The other half of the change block this line is paired with, when it has one.
        pair: Option<Side>,
    },
    /// A group of pairs the whitespace-ignore rule hid, stated as one line.
    WhitespaceHidden {
        /// How many pairs.
        count: u32,
    },
    /// `\ No newline at end of file`.
    NoNewline {
        /// Which side it was said about.
        sign: Sign,
    },
}

impl Row {
    /// Is this a change row (as opposed to context or a header)?
    #[must_use]
    pub fn is_change(&self) -> bool {
        matches!(self, Row::Pair { .. } | Row::One { .. })
    }
}

/// Which of the four row-count tables to read: the pane has two layouts and the whitespace rule has
/// two states, and a hidden pair changes how many rows there are. One function, so a caller cannot
/// pick the wrong table by hand.
#[must_use]
pub const fn slot(layout: Layout, ignore_ws: bool) -> usize {
    (match layout {
        Layout::Unified => 0,
        Layout::Split => 2,
    }) | (if ignore_ws { 1 } else { 0 })
}

/// The pane's two shapes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Layout {
    /// One column, `+`/`-` under each other. The default, and the only one under ~100 columns.
    #[default]
    Unified,
    /// Old and new side by side, inside the pane — ADR-006 §1 R1's one exception, opt-in.
    Split,
}

impl Layout {
    /// The word the strip prints.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Layout::Unified => "unified",
            Layout::Split => "split",
        }
    }

    /// The other one, which is what `\` switches to.
    #[must_use]
    pub const fn other(self) -> Layout {
        match self {
            Layout::Unified => Layout::Split,
            Layout::Split => Layout::Unified,
        }
    }
}

/// Columns a pane must have before a split can show both sides without clipping one.
///
/// ADR-006 §6.3 fixes the number at "about 100 columns" for the *frame*; this is the number for the
/// pane, which is what the renderer can actually measure: two number gutters, two signs, the rule
/// between them, and two sides of at least ~28 cells — enough for a call and its argument, which is
/// the shortest side that is still a diff and not a riddle.
///
/// Below this the split is refused out loud rather than drawn narrow, because a clipped-away right
/// half is worse than a refusal that names the key to press again (§6.3).
pub const SPLIT_MIN_COLS: usize = 72;

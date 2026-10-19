//! `fathom_tui::style` — what a run of cells *means*, and the one place a colour may be decided.
//!
//! The frame's product has always been `Vec<String>` ([`crate::render`]), and that stays the surface
//! the golden tests assert on: text carries every fact, and ADR-006 §10.2's "colour never carries a
//! fact alone" is true by construction when the text is complete without it. What this module adds is
//! the second channel the screen needs — a [`Face`] per run of cells — without giving the text a
//! second, drift-prone producer: a [`StyledLine`] *is* its text, and `frame_lines` is its projection.
//!
//! Two rules:
//!
//! * **A face is a meaning, never a colour.** `Face::Added` is "this line was added"; `Face::Keyword`
//!   is "this token is a keyword". [`Face::style`] is the only function in the crate that names a
//!   [`ratatui::style::Color`], which is what makes `e5.6`'s theming a swap of one `match` instead of
//!   a hunt through every view — and what makes "is this colour wrong?" answerable by reading one file.
//! * **Spans are normalized on construction.** Ranges are byte offsets into the line's own text, they
//!   never overlap, and a `Face::Text` span is not stored at all — the default style *is* text. That is
//!   what lets [`StyledLine::override_face`] layer syntax over a diff colour and word-diff over the
//!   syntax without the three of them arguing at draw time.

use std::ops::Range;

use ratatui::style::{Color, Modifier, Style};

/// `style` with every colour taken off and every shape left.
///
/// The single place a colour is removed — see [`Face::monochrome`] for why that is a legitimate thing
/// to do to a frame, and `blit_in_colour` for the one caller that needs it on an already-built style.
#[must_use]
pub fn strip_colour(style: Style) -> Style {
    Style {
        fg: None,
        bg: None,
        underline_color: None,
        ..style
    }
}
use unicode_segmentation::UnicodeSegmentation;

/// What a run of cells means. Names are the vocabulary a theme maps, not the colours themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    /// Default foreground — the absence of a face, and therefore never stored.
    Text,
    /// Chrome and secondary text: rules, gutters, the `…` that says a line was clipped.
    Muted,
    /// Something present but deliberately de-emphasised (a collapsed run, an unread hint).
    Faint,
    /// The row under the cursor, and the one fact on a strip.
    Emphasis,
    /// Structure that is not content: `diff --git`, `@@`, mode lines, our own `X-Fathom-` headers.
    Meta,
    /// A state marker: `new`, `bin`, `mode`, `✓`, `[n]`.
    Marker,
    /// A path.
    Path,
    /// A directory or taxonomy group in the tree.
    Dir,
    /// An added line, or an added side of a word-diff.
    Added,
    /// A removed line, or a removed side of a word-diff.
    Removed,
    /// The `@@ -a,b +c,d @@` header itself.
    HunkHeader,
    // --- the syntax faces: what a *token* is, as far as a terminal-only highlighter can tell -------
    /// `fn`, `if`, `def`, `return`, `class` …
    Keyword,
    /// A type or constructor-shaped identifier.
    Type,
    /// A string, char or raw literal.
    Literal,
    /// A number.
    Number,
    /// A comment, including the `#` that starts it.
    Comment,
    /// A preprocessor or attribute line: `#include`, `#[derive]`, `@override`.
    Preproc,
    /// The name in a definition or call.
    Function,
    /// A run *within* a line that word-diff says was removed.
    WordOld,
    /// A run *within* a line that word-diff says was added.
    WordNew,
    /// Whitespace that was made visible, or a whitespace-only difference.
    Whitespace,
    /// A problem worth a second look.
    Warning,
    /// A failure, stated rather than implied.
    Failure,
    /// Something that worked, and what it did.
    Success,
}

impl Face {
    /// The same face with its colours taken off and its shapes left, which is what a terminal that
    /// said `NO_COLOR` or `TERM=dumb` is given (ADR-006 §6.7).
    ///
    /// This is the only place a colour is *removed*, for the same reason [`Face::style`] is the only
    /// place one is chosen. It is a legitimate removal because of §10.2's rule that no fact may be
    /// carried by colour alone: every face here also has an intensity, an underline or a glyph, so a
    /// monochrome frame is a complete frame rather than a lossy one.
    #[must_use]
    pub fn monochrome(self) -> Style {
        strip_colour(self.style())
    }

    /// The registry name a theme file and `fathom tui --theme` would use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Face::Text => "text",
            Face::Muted => "muted",
            Face::Faint => "faint",
            Face::Emphasis => "emphasis",
            Face::Meta => "meta",
            Face::Marker => "marker",
            Face::Path => "path",
            Face::Dir => "dir",
            Face::Added => "added",
            Face::Removed => "removed",
            Face::HunkHeader => "hunk",
            Face::Keyword => "keyword",
            Face::Type => "type",
            Face::Literal => "literal",
            Face::Number => "number",
            Face::Comment => "comment",
            Face::Preproc => "preproc",
            Face::Function => "function",
            Face::WordOld => "word-old",
            Face::WordNew => "word-new",
            Face::Whitespace => "whitespace",
            Face::Warning => "warning",
            Face::Failure => "failure",
            Face::Success => "success",
        }
    }

    /// The only place a colour is chosen. `e5.6` replaces this table with the theme; nothing else
    /// may name a colour, and `tests/style.rs` is what keeps that sentence true.
    ///
    /// Every face also has a *shape* or an intensity so it survives `NO_COLOR` and a monochrome
    /// terminal (ADR-006 §10.2): added/removed differ by sign in the text as well as by hue.
    #[must_use]
    pub fn style(self) -> Style {
        match self {
            Face::Text => Style::default(),
            Face::Muted => Style::default().fg(Color::DarkGray),
            Face::Faint => Style::default().add_modifier(Modifier::DIM),
            Face::Emphasis => Style::default().add_modifier(Modifier::BOLD),
            Face::Meta => Style::default().fg(Color::DarkGray).italic(),
            Face::Marker => Style::default().fg(Color::Yellow),
            Face::Path => Style::default().fg(Color::Blue),
            Face::Dir => Style::default().fg(Color::Cyan),
            Face::Added => Style::default().fg(Color::Green),
            Face::Removed => Style::default().fg(Color::Red),
            Face::HunkHeader => Style::default().fg(Color::Cyan).bold(),
            Face::Keyword => Style::default().fg(Color::Magenta),
            Face::Type => Style::default().fg(Color::Yellow),
            Face::Literal => Style::default().fg(Color::Green).italic(),
            Face::Number => Style::default().fg(Color::LightBlue),
            Face::Comment => Style::default().fg(Color::DarkGray).italic(),
            Face::Preproc => Style::default().fg(Color::LightMagenta),
            Face::Function => Style::default().fg(Color::Blue).bold(),
            Face::WordOld => Style::default().fg(Color::Red).bg(Color::DarkGray).bold(),
            Face::WordNew => Style::default().fg(Color::Green).bg(Color::DarkGray).bold(),
            Face::Whitespace => Style::default().fg(Color::DarkGray).reversed(),
            Face::Warning => Style::default().fg(Color::Yellow).bold(),
            Face::Failure => Style::default().fg(Color::Red).bold(),
            Face::Success => Style::default().fg(Color::Green).bold(),
        }
    }

    /// Every face, in the order a theme listing prints them.
    pub const ALL: [Face; 24] = [
        Face::Text,
        Face::Muted,
        Face::Faint,
        Face::Emphasis,
        Face::Meta,
        Face::Marker,
        Face::Path,
        Face::Dir,
        Face::Added,
        Face::Removed,
        Face::HunkHeader,
        Face::Keyword,
        Face::Type,
        Face::Literal,
        Face::Number,
        Face::Comment,
        Face::Preproc,
        Face::Function,
        Face::WordOld,
        Face::WordNew,
        Face::Whitespace,
        Face::Warning,
        Face::Failure,
        Face::Success,
    ];
}

/// A run of cells that means the same thing. `start`/`end` are byte offsets into the owning line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First byte of the run.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
    /// What the run means.
    pub face: Face,
}

impl Span {
    /// How many bytes the run covers.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Is the run empty?
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// A line of text plus the faces of its runs — the unit every pane hands the renderer.
///
/// The text is authoritative: `cells()` is what the layout fits against, and [`StyledLine::text`] is
/// what the golden files and `--plain` print. The spans only ever decorate that text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyledLine {
    text: String,
    spans: Vec<Span>,
}

impl StyledLine {
    /// An empty line.
    #[must_use]
    pub fn new() -> Self {
        StyledLine::default()
    }

    /// A line with no faces at all — the shape every non-diff pane's rows already are.
    #[must_use]
    pub fn plain(text: impl Into<String>) -> Self {
        StyledLine {
            text: text.into(),
            spans: Vec::new(),
        }
    }

    /// A line whose whole content carries one face.
    #[must_use]
    pub fn with_face(text: impl AsRef<str>, face: Face) -> Self {
        let mut line = StyledLine::plain("");
        line.push(face, text.as_ref());
        line
    }

    /// A line from already-normalized spans. Spans must be ordered, non-overlapping and inside the
    /// text; anything else is dropped rather than drawn in the wrong place.
    #[must_use]
    pub fn from_spans(text: impl Into<String>, spans: Vec<Span>) -> Self {
        let text = text.into();
        let spans = normalize(spans, text.len());
        StyledLine { text, spans }
    }

    /// Append `text` carrying `face` (a `Face::Text` run appends no span, which is the default).
    pub fn push(&mut self, face: Face, text: &str) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if face == Face::Text {
            return;
        }
        if let Some(last) = self.spans.last_mut()
            && last.end == start
            && last.face == face
        {
            last.end = end;
            return;
        }
        self.spans.push(Span { start, end, face });
    }

    /// Append text in the default face.
    pub fn push_str(&mut self, text: &str) {
        self.push(Face::Text, text);
    }

    /// Append another line's runs, shifted onto the end of this one.
    pub fn push_line(&mut self, line: &StyledLine) {
        let base = self.text.len();
        self.text.push_str(&line.text);
        for span in &line.spans {
            self.spans.push(Span {
                start: span.start + base,
                end: span.end + base,
                face: span.face,
            });
        }
    }

    /// The text, which is the whole truth about what the frame says.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The normalized runs.
    #[must_use]
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Display cells the line occupies.
    #[must_use]
    pub fn cells(&self) -> usize {
        crate::text::width(&self.text)
    }

    /// Is there nothing on this line?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Clip to `width` cells with a `…`, keeping the faces of whatever survives.
    ///
    /// This is [`crate::layout::fit_text`]'s rule for the text channel; the span channel is clipped
    /// to the same byte, so a run cannot be coloured past the ellipsis. The cut is a grapheme
    /// boundary — [`crate::text::cut`] says which byte — because a styled line is the same text as
    /// the plain one and must be cut in the same place, or `--plain` and the frame would disagree
    /// about what the reviewer was shown.
    #[must_use]
    pub fn fit(&self, width: usize) -> StyledLine {
        if width == 0 {
            return StyledLine::plain("");
        }
        if self.cells() <= width {
            return self.clone();
        }
        let cut = crate::text::cut(&self.text, width).unwrap_or(self.text.len());
        let mut out = StyledLine::new();
        for (offset, cluster) in self.text[..cut].grapheme_indices(true) {
            out.push(self.face_at(offset), cluster);
        }
        out.push(Face::Muted, "…");
        out
    }

    /// Fit to `width` and pad with blanks so the column lines up.
    #[must_use]
    pub fn padded(&self, width: usize) -> StyledLine {
        let mut out = self.fit(width);
        let pad = width.saturating_sub(out.cells());
        out.push_str(&" ".repeat(pad));
        out
    }

    /// The face in force at a byte offset — `Face::Text` where nothing was recorded.
    #[must_use]
    pub fn face_at(&self, offset: usize) -> Face {
        self.spans
            .iter()
            .find(|s| s.start <= offset && offset < s.end)
            .map_or(Face::Text, |s| s.face)
    }

    /// Repaint `range` with `face`, splitting runs as needed. This is how a syntax face lands on top
    /// of the line's added/removed colour, and how word-diff lands on top of the syntax.
    pub fn override_face(&mut self, range: Range<usize>, face: Face) {
        let range = Range {
            start: range.start.min(self.text.len()),
            end: range.end.min(self.text.len()),
        };
        if range.start >= range.end {
            return;
        }
        let mut out: Vec<Span> = Vec::new();
        for span in &self.spans {
            let span_range = span.start..span.end;
            // The part before the override, if any.
            if span_range.start < range.start {
                out.push(Span {
                    start: span_range.start,
                    end: span_range.start.min(range.start),
                    face: span.face,
                });
            }
            // The part after, if any — kept even when the override is `Text`, which erases a face
            // only inside the range and must not erase it after it.
            if span_range.end > range.end {
                out.push(Span {
                    start: span_range.end.max(range.end),
                    end: span_range.end,
                    face: span.face,
                });
            }
        }
        if face != Face::Text {
            out.push(Span {
                start: range.start,
                end: range.end,
                face,
            });
        }
        self.spans = normalize(out, self.text.len());
    }

    /// Join lines with a one-cell rule, as a framed row is assembled.
    #[must_use]
    pub fn join(parts: &[StyledLine], rule: char) -> StyledLine {
        let mut out = StyledLine::plain("");
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                out.push(Face::Muted, &rule.to_string());
            }
            out.push_line(part);
        }
        out
    }

    /// The line as `(text, column, style)` runs, which is all the blit needs.
    ///
    /// Columns are display cells, not bytes, so a path with CJK in it cannot shift the rest of the
    /// row by one — the failure mode ADR-006 §6.2 says turns a box into a wrapped box.
    ///
    /// The runs cover the whole line: a gap between spans is emitted in the default style rather
    /// than left for the row behind it to show through, which is what makes a styled row and an
    /// unstyled one say the same thing.
    #[must_use]
    pub fn segments(&self) -> Vec<(String, usize, Style)> {
        // One face per byte, so that "who wins where" is decided once, here, instead of at draw time.
        let mut faces = vec![Face::Text; self.text.len()];
        for span in &self.spans {
            if span.end > self.text.len() {
                continue;
            }
            for slot in &mut faces[span.start..span.end] {
                *slot = span.face;
            }
        }
        let mut out: Vec<(String, usize, Style)> = Vec::new();
        let mut column = 0;
        let mut run_start = 0;
        let mut run_face = Face::Text;
        for (offset, ch) in self.text.char_indices() {
            let face = faces[offset];
            if face != run_face {
                out.push((
                    self.text[run_start..offset].to_owned(),
                    column,
                    run_face.style(),
                ));
                column += crate::text::width(&self.text[run_start..offset]);
                run_start = offset;
                run_face = face;
            }
            let _ = ch;
        }
        if run_start < self.text.len() || out.is_empty() {
            out.push((self.text[run_start..].to_owned(), column, run_face.style()));
        }
        out
    }
}

/// Sort and de-overlap a span list, dropping empty, `Text` and out-of-range runs.
fn normalize(spans: Vec<Span>, len: usize) -> Vec<Span> {
    let mut clean: Vec<Span> = spans
        .into_iter()
        .filter(|s| s.face != Face::Text && s.start < s.end && s.end <= len)
        .collect();
    clean.sort_by_key(|s| s.start);
    let mut out: Vec<Span> = Vec::with_capacity(clean.len());
    for span in clean {
        if let Some(last) = out.last_mut()
            && last.end > span.start
        {
            last.end = span.start;
            if last.is_empty() {
                out.pop();
            }
        }
        out.push(span);
    }
    out
}

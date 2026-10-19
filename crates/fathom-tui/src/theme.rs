//! `fathom_tui::theme` — the one place a colour is chosen, and the audit that says whether it was a
//! good choice.
//!
//! Colour carries facts in Fathom: added and removed, a risk rung, a layer violation, a state that
//! needs a second look. That is exactly the load a palette has to be *audited* for rather than
//! admired for, and an audit needs a lookup, not a literal. So no call site below this module names a
//! colour (a `match` in a view is unfalsifiable: you cannot paint a `match` through a deuteranopia
//! simulation, list it for a user to choose, or override it from a file), every face is resolved
//! through a [`Theme`], and this module ships the three checks the ticket asked for as functions the
//! tests, `--show-theme` and `fathom doctor` all read from the same table:
//!
//! * **contrast** — [colour::contrast_ratio] against the background the theme actually paints, at
//!   [`Theme::MIN_CONTRAST`] (4.5:1, WCAG 2.1 AA for body text);
//! * **dichromacy** — [colour::delta_e_simulated] for the pairs in [`Theme::REQUIRED_PAIRS`] under
//!   protanopia and deuteranopia, at [`Theme::MIN_DELTA_E`];
//! * **quantization** — [`Theme::resolve`], which folds a truecolour theme down to xterm-256 or to the
//!   sixteen ANSI colours a minimal terminal actually has, and answers `NO_COLOR` by taking every
//!   colour off and leaving every shape ([`Face::monochrome`]'s rule, now applied to a whole theme).
//!
//! A theme is a name, a background, a foreground, and one [`Paint`] per [`Face`]. It is *data*: it can
//! be printed, diffed, written by a user in TOML ([`Theme::from_toml`]) and measured. The three
//! built-ins live in [`builtin`]; a user's file is read by `fathom-cli`, because
//! [`crate`] has no files and no environment (ADR-002 §5) — this module parses text it is handed.

mod builtin;
mod file;

use ratatui::style::{Color, Modifier, Style};

use crate::colour::{self, Rgb};
use crate::style::Face;
use crate::terminal::ColorDepth;

pub use builtin::{NAMES, ansi, dark, light};
pub use file::parse as parse_toml;

/// Every face, in table order — the length of [`Theme::paints`].
pub const FACE_COUNT: usize = 24;

/// How much colour a terminal may be given, and therefore how much of a theme survives.
///
/// Owned by [`crate::terminal`] (it is a fact about the terminal, not about the theme) and re-exported
/// here because a theme is resolved against it.
pub use crate::terminal::ColorDepth as Depth;

/// A colour token as a theme file writes it.
///
/// Four spellings, because terminals are not one thing: `default` (leave the terminal's own colour
/// alone — the only honest answer for a `TERM=dumb` run and for a user whose terminal palette *is*
/// their accessibility setting), one of the sixteen ANSI slot names (`red`, `bright-green`, `8`,
/// ratatui's `light-green`), an xterm-256 index (`129`), or truecolour (`#rrggbb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// The terminal's own colour. Painted as [`Color::Reset`], which is what "unset this cell" means.
    Default,
    /// One of the sixteen ANSI slots; emitted as `Color::Indexed(0..=15)`, which crossterm prints with
    /// the portable `30-37`/`90-97` sequences rather than a 256-colour one.
    Ansi(u8),
    /// An xterm-256 palette index, 16..=255. Below 16 it is an ANSI slot wearing a different hat, and
    /// [`Ink::parse`] puts it in the right variant so quantization and auditing agree.
    Index(u8),
    /// Twenty-four bits.
    True(Rgb),
}

impl Ink {
    /// Parse one ink token, strictly: a misspelled colour name is reported, never drawn as a default.
    #[must_use]
    pub fn parse(text: &str) -> Option<Ink> {
        let text = text.trim();
        if text.is_empty() || text == "default" || text == "reset" || text == "none" {
            return Some(Ink::Default);
        }
        if let Some(rgb) = Rgb::parse(text) {
            return Some(Ink::True(rgb));
        }
        let digits = text
            .strip_prefix("index:")
            .or_else(|| text.strip_prefix("color:"))
            .unwrap_or(text);
        if let Ok(value) = digits.parse::<u16>() {
            if value > 255 {
                return None;
            }
            let value = value as u8;
            return Some(if value < 16 {
                Ink::Ansi(value)
            } else {
                Ink::Index(value)
            });
        }
        colour::ansi_of_name(text).map(Ink::Ansi)
    }

    /// The reference RGB for this ink — the number the audit measures.
    ///
    /// `Ansi` and `Index` are resolved through the reference palette in [`colour::ANSI16`] and the
    /// xterm cube, which is what makes an audit of a sixteen-colour theme *possible* while leaving the
    /// documented fact standing: on such a terminal the hues are the user's, and the audit is a check
    /// of slot assignment, not of hues.
    #[must_use]
    pub fn rgb(self) -> Option<Rgb> {
        match self {
            Ink::Default => None,
            Ink::Ansi(slot) => colour::ANSI16.get(slot as usize).copied(),
            Ink::Index(index) => Some(colour::xterm_rgb(index)),
            Ink::True(rgb) => Some(rgb),
        }
    }

    /// What this ink costs as a colour, after `depth` has had its say.
    #[must_use]
    pub fn color(self, depth: Depth) -> Color {
        match depth {
            ColorDepth::None => Color::Reset,
            ColorDepth::Basic => match self.rgb() {
                None => Color::Reset,
                Some(rgb) => Color::Indexed(colour::nearest_ansi(rgb)),
            },
            ColorDepth::Ansi256 => match self {
                Ink::Default => Color::Reset,
                Ink::Ansi(slot) => Color::Indexed(slot),
                Ink::Index(index) => Color::Indexed(index),
                Ink::True(rgb) => Color::Indexed(colour::to_xterm256(rgb)),
            },
            ColorDepth::TrueColor => match self {
                Ink::Default => Color::Reset,
                Ink::Ansi(slot) => Color::Indexed(slot),
                Ink::Index(index) => Color::Indexed(index),
                Ink::True(rgb) => rgb.to_color(),
            },
        }
    }

    /// The theme-file spelling of this ink, which is the spelling `--show-theme` prints so that what
    /// a reviewer reads can be pasted back into a file.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Ink::Default => "default".to_owned(),
            Ink::Ansi(slot) => format!("{} ({})", colour::ansi_name(slot), slot),
            Ink::Index(index) => format!("index {index}"),
            Ink::True(rgb) => rgb.to_hex(),
        }
    }

    /// Is this a truecolour ink? Used to say which audit bar applies.
    #[must_use]
    pub const fn is_true(self) -> bool {
        matches!(self, Ink::True(_))
    }
}

/// What one face is painted with: an ink, an optional ground under it, and a shape.
///
/// The shape is not decoration. It is the half of the design that ADR-006 §10.2 makes load-bearing:
/// `NO_COLOR` may only be a subtraction because a bold, a dim, an underline or a reversed block is
/// still standing when the colour is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paint {
    /// The face's own ink, or [`Ink::Default`] for the theme's foreground.
    pub ink: Ink,
    /// A ground for this face only (`None` paints the theme's background). Only the word-diff faces
    /// and the made-visible whitespace use it in the built-ins.
    pub ground: Option<Ink>,
    /// Bold.
    pub bold: bool,
    /// Dim. Held to a taller contrast bar — see [`Theme::MIN_CONTRAST_DIM`].
    pub dim: bool,
    /// Italic.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
    /// Reverse video.
    pub reverse: bool,
}

impl Paint {
    /// A plain face: this ink, the theme's ground, no shape.
    #[must_use]
    pub const fn ink(ink: Ink) -> Self {
        Paint {
            ink,
            ground: None,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            reverse: false,
        }
    }

    /// A plain truecolour face from its hex spelling.
    #[must_use]
    pub fn hex(text: &str) -> Self {
        Paint::ink(Ink::parse(text).unwrap_or(Ink::Default))
    }

    /// A face with a shape and no colour of its own.
    #[must_use]
    pub const fn shape(shape: Modifier) -> Self {
        Paint {
            ink: Ink::Default,
            ground: None,
            bold: shape.contains(Modifier::BOLD),
            dim: shape.contains(Modifier::DIM),
            italic: shape.contains(Modifier::ITALIC),
            underline: shape.contains(Modifier::UNDERLINED),
            reverse: shape.contains(Modifier::REVERSED),
        }
    }

    /// With a shape added.
    #[must_use]
    pub const fn with_shape(mut self, shape: Modifier) -> Self {
        self.bold |= shape.contains(Modifier::BOLD);
        self.dim |= shape.contains(Modifier::DIM);
        self.italic |= shape.contains(Modifier::ITALIC);
        self.underline |= shape.contains(Modifier::UNDERLINED);
        self.reverse |= shape.contains(Modifier::REVERSED);
        self
    }

    /// With a ground of its own.
    #[must_use]
    pub const fn on(mut self, ground: Ink) -> Self {
        self.ground = Some(ground);
        self
    }

    /// The ratatui modifiers this paint asks for.
    #[must_use]
    pub const fn modifiers(self) -> Modifier {
        let mut out = Modifier::empty();
        if self.bold {
            out = out.union(Modifier::BOLD);
        }
        if self.dim {
            out = out.union(Modifier::DIM);
        }
        if self.italic {
            out = out.union(Modifier::ITALIC);
        }
        if self.underline {
            out = out.union(Modifier::UNDERLINED);
        }
        if self.reverse {
            out = out.union(Modifier::REVERSED);
        }
        out
    }
}

/// One pair of faces that must stay tellable apart, and why it is in the list.
#[derive(Debug, Clone, Copy)]
pub struct Separation {
    /// The first face.
    pub one: Face,
    /// The second face.
    pub other: Face,
    /// The sentence a failing audit prints.
    pub why: &'static str,
}

/// The audited pairs.
///
/// The ticket's words are "added/removed/violation states", so those three are separated from each
/// other *and* from plain text — a state colour that cannot be told from the text it sits in is not
/// carrying a fact either. Syntax faces are deliberately absent: `fn` being purple is a convenience
/// for eyes that work, and a reviewer for whom they do not is reading the words `fn` and `let`, which
/// is what makes them text in the first place.
pub const REQUIRED_PAIRS: [Separation; 6] = [
    Separation {
        one: Face::Added,
        other: Face::Removed,
        why: "the two halves of a diff have to be tellable apart",
    },
    Separation {
        one: Face::Added,
        other: Face::Warning,
        why: "a violation marked on a line that was also added is two facts, not one",
    },
    Separation {
        one: Face::Removed,
        other: Face::Warning,
        why: "a violation must not read as a deletion",
    },
    Separation {
        one: Face::Added,
        other: Face::Text,
        why: "added lines have to read as added",
    },
    Separation {
        one: Face::Removed,
        other: Face::Text,
        why: "removed lines have to read as removed",
    },
    Separation {
        one: Face::Warning,
        other: Face::Text,
        why: "a warning has to read as a warning",
    },
];

/// The measured result for one [`Separation`].
#[derive(Debug, Clone, Copy)]
pub struct Measured {
    /// The pair that was measured.
    pub pair: Separation,
    /// The dichromacy the pair was measured under.
    pub kind: colour::Cvd,
    /// ΔE between the two inks, after simulation.
    pub delta_e: f64,
}

/// A theme: a name, a ground, a foreground, and one [`Paint`] per [`Face`].
#[derive(Debug, Clone)]
pub struct Theme {
    /// The name `--theme` takes and the frame prints.
    pub name: String,
    /// The background every cell is painted with, unless the face brings its own.
    ///
    /// `Ink::Default` means "the terminal's" — which is what the `ansi` theme does, and what any theme
    /// may do; it also means the contrast audit declines to guess (see [`Theme::contrast`]).
    pub background: Ink,
    /// The ink a [`Face::Text`] run gets.
    pub foreground: Ink,
    paints: Box<[Paint; FACE_COUNT]>,
    /// A hand-written sixteen-colour mapping, for a theme that has one.
    ///
    /// Quantizing a truecolour palette to the nearest of sixteen slots is a lossy business: the nearest
    /// slot to `dark`'s added teal is a cyan, and then "added" and "directory" are the same colour on
    /// the one terminal class where the hues are not even the user's choice. So the built-in `dark`
    /// carries the sixteen-slot table that was *audited as a sixteen-slot table* (the `ansi` palette), and
    /// a user theme without one gets the quantization — with `--show-theme --audit` there to tell its
    /// author what collapsed.
    basic: Option<Box<Theme>>,
    /// Which theme this one was built over: a built-in name, or `"none"`.
    ///
    /// Recorded rather than derived, because a theme file that says `inherits = "light"` and then
    /// overrides two faces is a different document from one that names twenty-two, and `--list-themes`
    /// prints the difference — "4 faces overridden" is the answer to "did my file do anything?".
    pub inherits: String,
    /// Where this theme's colours came from, for the report: a built-in name, or a file that was read.
    pub source: String,
}

impl Theme {
    /// WCAG 2.1 AA's body-text floor: every face of a default theme clears it, on the ink as written.
    pub const MIN_CONTRAST: f64 = 4.5;
    /// A taller floor for a face carrying `DIM`, because the modifier pushes the ink toward the
    /// background by an amount only the terminal knows. A dim face that cannot clear this is the grey
    /// you meant instead of the dim you asked for.
    pub const MIN_CONTRAST_DIM: f64 = 6.0;
    /// ΔE floor for a pair under dichromat simulation, for themes that name truecolour inks.
    pub const MIN_DELTA_E: f64 = 14.0;
    /// The same floor for a sixteen-colour theme, lower on purpose: the sixteen hues are the user's
    /// terminal palette, so what is being checked is the *slot assignment* — enough lightness and
    /// enough hue distance that a palette like xterm's default keeps the states apart. Below this the
    /// claim would be about the reference palette rather than about the terminal in front of you.
    pub const MIN_DELTA_E_ANSI: f64 = 12.0;

    /// A theme that paints nothing: every face falls back to the terminal's own colours and shapes.
    ///
    /// This is what `inherits = "none"` starts from, and what a test writes a one-face theme over.
    #[must_use]
    pub fn bare(name: &str) -> Theme {
        Theme::assemble(
            name,
            Ink::Default,
            Ink::Default,
            [Paint::shape(Modifier::empty()); FACE_COUNT],
            format!("{name} (no colours)"),
        )
    }

    /// A user theme from TOML text, over `fallback` unless the file says `inherits`.
    ///
    /// `source` names the file (or the `--theme` argument) in every problem reported; the error half
    /// is the whole list, because "run it again to find the next typo" is not a way to edit a file.
    pub fn from_toml(text: &str, fallback: &Theme, source: &str) -> Result<Theme, Vec<String>> {
        file::parse(text, fallback, source)
    }

    /// The theme a name asks for, resolved for a terminal of `depth`.
    ///
    /// `Ok` is the theme to paint with; `Err` is the sentence that says the name meant nothing, which
    /// the caller prints rather than swallows — a fallback that is not mentioned is how a reviewer
    /// spends an afternoon wondering why their new theme did not arrive.
    ///
    /// A name containing `/` or ending in `.toml` is read as a path by the caller (`fathom-cli`, which
    /// owns files) before it gets here; this function only knows the built-ins.
    pub fn resolve_named(name: &str, depth: Depth) -> Result<Theme, String> {
        match Theme::builtin(name) {
            Some(theme) => Ok(theme.resolve(depth)),
            None => Err(Theme::unknown_notice(name)),
        }
    }

    /// The theme a run uses when nobody said otherwise: `dark`.
    #[must_use]
    pub fn default_theme() -> Theme {
        dark()
    }

    /// Assemble a theme from its parts.
    ///
    /// Crate-internal because a theme a user wrote arrives through [`Theme::from_toml`], which has to
    /// report what it could not understand, and a theme a test wrote has to say which test wrote it.
    /// `source` is the sentence `--show-theme` prints for "where did these colours come from".
    #[must_use]
    pub(crate) fn assemble(
        name: &str,
        background: Ink,
        foreground: Ink,
        paints: [Paint; FACE_COUNT],
        source: String,
    ) -> Theme {
        Theme {
            name: name.to_owned(),
            background,
            foreground,
            paints: Box::new(paints),
            basic: None,
            inherits: "none".to_owned(),
            source,
        }
    }

    /// How many faces and grounds differ from `base` — the size of a user's edit over what it inherits.
    #[must_use]
    pub fn differs_from(&self, base: &Theme) -> usize {
        Face::ALL
            .iter()
            .filter(|face| self.paint(**face) != base.paint(**face))
            .count()
            + usize::from(self.background != base.background)
            + usize::from(self.foreground != base.foreground)
    }

    /// Every built-in, in the order `--list-themes` prints them.
    #[must_use]
    pub fn builtins() -> Vec<Theme> {
        vec![dark(), light(), ansi()]
    }

    /// A built-in by name, or `None` (the caller reports the name it was given; see
    /// [`Theme::unknown_notice`]).
    #[must_use]
    pub fn builtin(name: &str) -> Option<Theme> {
        match name {
            "dark" => Some(dark()),
            "light" => Some(light()),
            "ansi" => Some(ansi()),
            _ => None,
        }
    }

    /// The names a `--theme` argument may use without a path, for the message that names what *was*
    /// available.
    #[must_use]
    pub fn builtin_names() -> &'static [&'static str] {
        &NAMES
    }

    /// The sentence printed when a name matched nothing, naming the names that would have worked.
    #[must_use]
    pub fn unknown_notice(name: &str) -> String {
        format!(
            "theme {name} is not a theme Fathom knows, so {} is in force. Built-ins: {}. A file is \
             --theme <path.toml>, or `[theme] name` in your own config.",
            Theme::default_theme().name,
            Theme::builtin_names().join(", "),
        )
    }

    /// The paint for one face.
    #[must_use]
    pub fn paint(&self, face: Face) -> Paint {
        self.paints[face.index()]
    }

    /// Every paint, in [`Face::ALL`] order — what a table printer walks.
    #[must_use]
    pub fn paints(&self) -> &[Paint; FACE_COUNT] {
        &self.paints
    }

    /// Replace one face's paint (a user file overriding one row of a built-in).
    pub fn set(&mut self, face: Face, paint: Paint) {
        self.paints[face.index()] = paint;
    }

    /// One face's ink, set from a theme-file spelling. A test and a user file both reach for this.
    #[must_use]
    pub fn with_ink(mut self, face: Face, ink: &str) -> Theme {
        let parsed = Ink::parse(ink).unwrap_or(Ink::Default);
        self.set(face, Paint::ink(parsed));
        self
    }

    /// Attach the sixteen-colour mapping this theme should fold to when a terminal has sixteen colours.
    #[must_use]
    pub fn with_basic(mut self, basic: Theme) -> Theme {
        self.basic = Some(Box::new(basic));
        self
    }

    /// The ink a face will actually be seen in: its own, else the theme's foreground.
    #[must_use]
    pub fn ink_of(&self, face: Face) -> Ink {
        match self.paint(face).ink {
            Ink::Default => self.foreground,
            other => other,
        }
    }

    /// The background a face will actually be seen on: its own ground, else the theme's background.
    #[must_use]
    pub fn ground_of(&self, face: Face) -> Ink {
        self.paint(face).ground.unwrap_or(self.background)
    }

    /// The [`Style`] for one face at this theme's own depth (no quantization).
    ///
    /// An unset ink is [`Color::Reset`] rather than an absent one, and that is load-bearing: ratatui
    /// starts each [`Frame`](ratatui::Frame) with the *previous* buffer, so a cell that stopped being
    /// coloured would otherwise keep the colour it had three frames ago — the bug ADR-006 §10.2 points
    /// at, in a shell that repaints a pane in place.
    #[must_use]
    pub fn style(&self, face: Face) -> Style {
        let paint = self.paint(face);
        let mut style = Style::default()
            .fg(self.ink_of(face).color(Depth::TrueColor))
            .bg(self.ground_of(face).color(Depth::TrueColor))
            .add_modifier(paint.modifiers());
        if paint.reverse {
            style = style.remove_modifier(Modifier::BOLD);
        }
        style
    }

    /// This theme, folded down to what a terminal with `depth` can be given.
    ///
    /// This is where `NO_COLOR` and `TERM=dumb` are honoured: at `ColorDepth::None` every ink becomes
    /// unset and every colour-bearing modifier goes, which is [`Face::monochrome`]'s rule applied to
    /// the whole theme — the frame is laid out identically and every glyph, sign and word stays
    /// (ADR-006 §6.7, §10.2). At `Basic` the inks go to the sixteen slots by ΔE; at `Ansi256` through
    /// the xterm cube. The *shape* survives every depth, because it is a modifier, not a colour.
    #[must_use]
    pub fn resolve(&self, depth: Depth) -> Theme {
        if depth == Depth::TrueColor {
            return self.clone();
        }
        // The hand-written sixteen-slot table wins over quantization when there is one, and only at
        // `Basic` — a 256-colour terminal is not being cheated, it is being given the nearest cube entry.
        if depth == Depth::Basic
            && let Some(basic) = &self.basic
        {
            let mut swapped = (**basic).clone();
            swapped.name = self.name.clone();
            swapped.source = format!("{} (sixteen-colour mapping)", self.source);
            return swapped;
        }
        let mut out = self.clone();
        for paint in out.paints.iter_mut() {
            if depth == Depth::None {
                // The `NO_COLOR` half, and it is a subtraction and nothing else: the ink and the ground
                // go, every shape stays. That is [`crate::style::strip_colour`]'s rule applied to a
                // whole theme, and ADR-006 §10.2 is why it is the right rule — the bold, the dim, the
                // underline and the reversed block are what carry the fact once there is no hue to carry
                // it, so dropping them would turn "no colour" into "no information".
                paint.ink = Ink::Default;
                paint.ground = None;
                continue;
            }
            paint.ink = quantize(paint.ink, depth);
            paint.ground = paint.ground.map(|ground| quantize(ground, depth));
        }
        out.background = quantize(out.background, depth);
        out.foreground = quantize(out.foreground, depth);
        out
    }

    /// Contrast of one face against the ground it will be seen on, or `None` when either side is the
    /// terminal's own colour.
    ///
    /// Declining is the honest answer: `ansi` deliberately paints no background, because a clone of
    /// this repository cannot know that your terminal's `#2d2d2a` is what you want behind your own
    /// palette, and inventing a background to measure against would be a contrast claim about a
    /// surface nobody is looking at.
    #[must_use]
    pub fn contrast(&self, face: Face) -> Option<f64> {
        let ink = self.ink_of(face).rgb()?;
        let ground = self.ground_of(face).rgb()?;
        Some(colour::contrast_ratio(ink, ground))
    }

    /// Every face with a measurable contrast, in table order.
    #[must_use]
    pub fn contrasts(&self) -> Vec<(Face, f64)> {
        Face::ALL
            .iter()
            .filter_map(|face| self.contrast(*face).map(|ratio| (*face, ratio)))
            .collect()
    }

    /// The faces that fail the contrast floor, with the number and the floor in the sentence.
    #[must_use]
    pub fn contrast_findings(&self) -> Vec<String> {
        self.contrasts()
            .into_iter()
            .filter_map(|(face, ratio)| {
                let floor = if self.paint(face).dim {
                    Self::MIN_CONTRAST_DIM
                } else {
                    Self::MIN_CONTRAST
                };
                (ratio < floor).then(|| {
                    format!(
                        "{}: {} on {} is {:.2}:1, under the {floor:.1}:1 floor",
                        face.name(),
                        self.ink_of(face).label(),
                        self.ground_of(face).label(),
                        ratio
                    )
                })
            })
            .collect()
    }

    /// The ΔE floor that applies here: the lower one if any audited ink came from the sixteen slots.
    #[must_use]
    pub fn delta_e_floor(&self) -> f64 {
        let ansi = REQUIRED_PAIRS
            .iter()
            .any(|pair| !self.ink_of(pair.one).is_true() || !self.ink_of(pair.other).is_true());
        if ansi {
            Self::MIN_DELTA_E_ANSI
        } else {
            Self::MIN_DELTA_E
        }
    }

    /// Every audited pair under every simulated dichromacy.
    #[must_use]
    pub fn separations(&self) -> Vec<Measured> {
        let mut out = Vec::new();
        for pair in REQUIRED_PAIRS {
            let (one, other) = (self.ink_of(pair.one).rgb(), self.ink_of(pair.other).rgb());
            let (Some(one), Some(other)) = (one, other) else {
                continue;
            };
            for kind in colour::Cvd::ALL {
                out.push(Measured {
                    pair,
                    kind,
                    delta_e: colour::delta_e_simulated(one, other, kind),
                });
            }
        }
        out
    }

    /// The pairs a dichromat reviewer could *not* tell apart, stated as findings.
    ///
    /// Named findings, not a boolean: the reviewer who is colour-blind needs to know which pair
    /// collapsed so they can change the theme or add the glyph, and "audit failed" does not say what
    /// to do about it (ADR-001 §7's remedy rule).
    #[must_use]
    pub fn separation_findings(&self) -> Vec<String> {
        let floor = self.delta_e_floor();
        let mut worst: std::collections::BTreeMap<(usize, usize), Measured> =
            std::collections::BTreeMap::new();
        // One finding per pair, quoting the worst (smallest) ΔE of the simulations, so a fix has one
        // number to beat rather than three.
        for measured in self.separations() {
            if measured.delta_e >= floor {
                continue;
            }
            let slot = (measured.pair.one.index(), measured.pair.other.index());
            let keep = worst
                .get(&slot)
                .is_some_and(|kept| kept.delta_e <= measured.delta_e);
            if !keep {
                worst.insert(slot, measured);
            }
        }
        worst
            .values()
            .map(|measured| {
                format!(
                    "{} and {} are ΔE {:.1} under {} simulation ({}); floor {:.1} — {}",
                    measured.pair.one.name(),
                    measured.pair.other.name(),
                    measured.delta_e,
                    measured.kind.label(),
                    measured.pair.why,
                    floor,
                    "every colour-coded state also has a glyph and a word (ADR-006 §10.2), so this \
                     is a legibility cost and not a lost fact"
                )
            })
            .collect()
    }

    /// Both audits as lines, for `--show-theme`, `fathom doctor` and a bug report.
    #[must_use]
    pub fn audit_lines(&self) -> Vec<String> {
        let mut out = vec![format!(
            "theme {} · background {} · foreground {}",
            self.name,
            self.background.label(),
            self.foreground.label()
        )];
        for (face, ratio) in self.contrasts() {
            let floor = if self.paint(face).dim {
                Self::MIN_CONTRAST_DIM
            } else {
                Self::MIN_CONTRAST
            };
            out.push(format!(
                "  {:<11} {:<18} {:>6.2}:1 {}",
                face.name(),
                self.ink_of(face).label(),
                ratio,
                if ratio < floor {
                    format!("BELOW {floor:.1}")
                } else {
                    "ok".to_owned()
                }
            ));
        }
        for face in Face::ALL {
            if self.contrast(face).is_none() {
                out.push(format!(
                    "  {:<11} {:<18}   ----   terminal's own colour, not measured",
                    face.name(),
                    self.ink_of(face).label()
                ));
            }
        }
        out.push(format!(
            "  dichromacy floor: ΔE {:.1} ({})",
            self.delta_e_floor(),
            if self.delta_e_floor() == Self::MIN_DELTA_E {
                "truecolour inks"
            } else {
                "sixteen-colour theme: the hues are the terminal's"
            }
        ));
        let mut worst_pair: Option<(f64, String)> = None;
        for measured in self.separations() {
            let label = format!(
                "{} vs {} under {}",
                measured.pair.one.name(),
                measured.pair.other.name(),
                measured.kind.label()
            );
            let replace = match worst_pair {
                Some((delta, _)) => measured.delta_e < delta,
                None => true,
            };
            if replace {
                worst_pair = Some((measured.delta_e, label));
            }
            if measured.delta_e < self.delta_e_floor() {
                out.push(format!(
                    "  {} vs {} · {} · ΔE {:.1} BELOW",
                    measured.pair.one.name(),
                    measured.pair.other.name(),
                    measured.kind.label(),
                    measured.delta_e
                ));
            }
        }
        // The margin, not only the verdict. "pass" on its own tells the next person to edit a palette
        // nothing about how much room there was, which is how a floor quietly gets shaved.
        let worst_contrast = self.contrasts().into_iter().min_by(|(_, one), (_, other)| {
            one.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
        });
        // Two lines, because the two audits can decline independently: a theme that paints no background
        // still separates states, and saying "nothing measurable" about both would hide that.
        match worst_contrast {
            Some((face, ratio)) => out.push(format!(
                "  measured margin: worst face {} at {:.2}:1 (floor {:.1})",
                face.name(),
                ratio,
                Self::MIN_CONTRAST
            )),
            None => out.push(String::from(
                "  measured margin: no face is measurable — this theme paints no background of its own",
            )),
        }
        match worst_pair {
            Some((delta, pair)) => out.push(format!(
                "  closest audited pair: {pair} at ΔE {:.1} (floor {:.1})",
                delta,
                self.delta_e_floor()
            )),
            None => out.push(String::from(
                "  closest audited pair: nothing to measure — the inks are the terminal's own",
            )),
        }
        out
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::default_theme()
    }
}

/// Fold one ink down to `depth`.
fn quantize(ink: Ink, depth: Depth) -> Ink {
    match ink {
        Ink::Default => Ink::Default,
        value => match depth {
            ColorDepth::None => Ink::Default,
            ColorDepth::Basic => value
                .rgb()
                .map_or(Ink::Default, |rgb| Ink::Ansi(colour::nearest_ansi(rgb))),
            ColorDepth::Ansi256 => value
                .rgb()
                .map_or(Ink::Default, |rgb| Ink::Index(colour::to_xterm256(rgb))),
            ColorDepth::TrueColor => value,
        },
    }
}

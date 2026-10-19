//! The built-in themes, and the audit results that justify them.
//!
//! Every ink here is a measured choice, not a taste: the two default themes clear
//! [`Theme::MIN_CONTRAST`](super::Theme::MIN_CONTRAST) on every face against the background they
//! themselves paint, and the pairs in [`REQUIRED_PAIRS`](super::Theme::REQUIRED_PAIRS) stay at least
//! [`Theme::MIN_DELTA_E`](super::Theme::MIN_DELTA_E) apart under simulated protanopia and
//! deuteranopia. `cargo test -p fathom-tui --test theming` re-measures both, so a later "this amber is
//! warmer" commit fails loudly instead of quietly costing a reviewer the ability to tell an addition
//! from a violation. `fathom tui --show-theme` prints the same numbers for whoever is in front of the
//! terminal.
//!
//! ## Why the diff colours are not plain green and red
//!
//! They are — shifted, and separated by lightness as well as hue. A green and a red at the same
//! lightness are *the same colour* to a deuteranope and to a protanope, and "read the sign in front of
//! the line instead" is a worse experience than "use a colour pair that survives your eyes". So the
//! dark theme pairs a light teal-green with a warm salmon that sits lower in lightness, and the light
//! theme pairs a deep green with a blue-shifted crimson — the blue channel is the one dichromats keep,
//! which is why a crimson and an amber stay ~30 ΔE apart under both simulations where a plain red and a
//! plain amber sit under 10.
//!
//! ## Why `dark` and `light` paint a background and `ansi` does not
//!
//! A contrast claim is a claim about two colours. `dark` and `light` own both of theirs, so the claim is
//! checkable. `ansi` is the theme for a terminal with sixteen colours and its own ideas: painting that
//! terminal's background from a guess would both override the palette the reviewer tuned and make the
//! audit measure a surface nobody chose, so it sets `background = default` and its audit **declines**
//! the contrast half while keeping the separation half. A stated limit, not a silent one — and the
//! reason the sixteen-colour fallback is a slot assignment rather than a set of hues.

use ratatui::style::Modifier;

use super::{FACE_COUNT, Ink, Paint, Theme};
use crate::style::Face;

/// The names `--theme` accepts without a path.
pub const NAMES: [&str; 3] = ["dark", "light", "ansi"];

/// A face with no shape at all — just an ink.
const PLAIN: Paint = Paint::shape(Modifier::empty());

/// A face whose only extra is a shape.
///
/// Every ink in the tables below is a literal, and a typo in one parses to [`Ink::Default`]; that is
/// caught by the "no face was left unpainted" check in `crates/fathom-tui/tests/theming.rs` rather than
/// by a `const` assertion, because a colour parse cannot be `const`.
const fn shape(shape: Modifier) -> Paint {
    Paint::shape(shape)
}

/// A row of a palette table: the face, its ink as a theme file would write it, and its shape.
type Row = (Face, &'static str, Paint);

/// The dark default: navy-black ground, near-white text, a diff pair separated by hue *and* lightness.
#[must_use]
pub fn dark() -> Theme {
    let rows: Vec<Row> = vec![
        (Face::Text, "#e4e6ec", PLAIN),
        (Face::Muted, "#9ba3b0", PLAIN),
        (Face::Faint, "#b7bdc9", shape(Modifier::DIM)),
        (Face::Emphasis, "#ffffff", shape(Modifier::BOLD)),
        (Face::Meta, "#959dad", shape(Modifier::ITALIC)),
        (Face::Marker, "#f2c770", PLAIN),
        (Face::Path, "#7cb7ff", PLAIN),
        (Face::Dir, "#63d3ea", PLAIN),
        (Face::Added, "#4fdca4", PLAIN),
        (Face::Removed, "#ff8e7d", PLAIN),
        (Face::HunkHeader, "#79c0ff", shape(Modifier::BOLD)),
        (Face::Keyword, "#d7a6ff", PLAIN),
        (Face::Type, "#ffcf7a", PLAIN),
        (Face::Literal, "#93e6a2", shape(Modifier::ITALIC)),
        (Face::Number, "#8ab4ff", PLAIN),
        (Face::Comment, "#8b93a2", shape(Modifier::ITALIC)),
        (Face::Preproc, "#e7a9ff", PLAIN),
        (Face::Function, "#7cb7ff", shape(Modifier::BOLD)),
        (Face::WordOld, "#ffe9e5", shape(Modifier::BOLD)),
        (Face::WordNew, "#d8ffe9", shape(Modifier::BOLD)),
        (Face::Whitespace, "#7d8595", shape(Modifier::REVERSED)),
        (Face::Warning, "#ffd25e", shape(Modifier::BOLD)),
        (Face::Failure, "#ff7096", shape(Modifier::BOLD)),
        (Face::Success, "#5fe0b0", shape(Modifier::BOLD)),
    ];
    let theme = build("dark", "#151821", "#e4e6ec", &rows);
    // The word-diff faces sit on their own ground: a within-line change is the one place where two
    // states have to live on the same row, so they get grounds rather than relying on the hue alone.
    let theme = with_grounds(theme, "#6e2a24", "#14504a");
    // On a terminal with sixteen colours, `dark` *is* the ANSI palette: see [`ansi`]'s note on why a
    // hand-written slot assignment beats quantizing these twenty-four inks.
    theme.with_basic(ansi())
}

/// The light default: paper ground, inks that clear the same floor on it.
#[must_use]
pub fn light() -> Theme {
    let rows: Vec<Row> = vec![
        (Face::Text, "#1f2328", PLAIN),
        (Face::Muted, "#5b6470", PLAIN),
        (Face::Faint, "#3e4650", shape(Modifier::DIM)),
        (Face::Emphasis, "#0b0d10", shape(Modifier::BOLD)),
        (Face::Meta, "#5b6470", shape(Modifier::ITALIC)),
        (Face::Marker, "#6b4e00", PLAIN),
        (Face::Path, "#0b5cc4", PLAIN),
        (Face::Dir, "#0a6070", PLAIN),
        (Face::Added, "#0d7a4f", PLAIN),
        (Face::Removed, "#b3143c", PLAIN),
        (Face::HunkHeader, "#0b5cc4", shape(Modifier::BOLD)),
        (Face::Keyword, "#7a3fbf", PLAIN),
        (Face::Type, "#7a4a00", PLAIN),
        (Face::Literal, "#136b3f", shape(Modifier::ITALIC)),
        (Face::Number, "#2a4fbd", PLAIN),
        (Face::Comment, "#5f6873", shape(Modifier::ITALIC)),
        (Face::Preproc, "#8b2fa8", PLAIN),
        (Face::Function, "#0b4fa8", shape(Modifier::BOLD)),
        (Face::WordOld, "#ffffff", shape(Modifier::BOLD)),
        (Face::WordNew, "#ffffff", shape(Modifier::BOLD)),
        (Face::Whitespace, "#5b6470", shape(Modifier::REVERSED)),
        (Face::Warning, "#8a6300", shape(Modifier::BOLD)),
        (Face::Failure, "#5f1030", shape(Modifier::BOLD)),
        (Face::Success, "#0f6b46", shape(Modifier::BOLD)),
    ];
    let theme = build("light", "#f7f7f4", "#1f2328", &rows);
    let theme = with_grounds(theme, "#a01038", "#0d5c4b");
    // The same slot table as `dark`, and the limitation that comes with it: the sixteen colours are tuned
    // against a dark ground, because a bare ANSI terminal has one, and the palette has no
    // paper-readable green — `green` on white is 3.5:1. A reviewer on paper-white in a sixteen-colour
    // terminal gets this table plus the shape channel; `--show-theme` says which floor it declined to
    // claim, and a user who wants the other reading writes the file (that is what `--theme` and
    // `inherits` are for, and the audit will measure what they wrote).
    theme.with_basic(ansi())
}

/// The sixteen-colour fallback: slot names only, no background of its own.
///
/// This is what a `TERM=vt100`, a `screen` with a plain `termcapinfo`, or a Linux virtual console gets,
/// and what `dark` and `light` fold into when the probe answers `Basic`. The slots are chosen so the
/// audited pairs differ in **lightness as well as hue** — bright green added against bright red removed,
/// a *dark* yellow for a violation so it cannot read as either, a bold dark red for a failure so it
/// cannot read as a deletion — because on a sixteen-colour terminal the hues are whatever the reviewer's
/// terminal was configured with, and the only thing Fathom controls is which slot each face asks for.
///
/// That is also why this table is written by hand rather than derived: quantizing `dark`'s inks to the
/// nearest of sixteen puts "added" on the same slot as "directory".
#[must_use]
pub fn ansi() -> Theme {
    let rows: Vec<Row> = vec![
        (Face::Text, "default", PLAIN),
        (Face::Muted, "gray", PLAIN),
        (Face::Faint, "white", shape(Modifier::DIM)),
        (Face::Emphasis, "bright-white", shape(Modifier::BOLD)),
        (Face::Meta, "gray", shape(Modifier::ITALIC)),
        (Face::Marker, "bright-yellow", PLAIN),
        (Face::Path, "bright-blue", PLAIN),
        (Face::Dir, "bright-cyan", PLAIN),
        (Face::Added, "bright-green", PLAIN),
        (Face::Removed, "bright-red", PLAIN),
        (Face::HunkHeader, "blue", shape(Modifier::BOLD)),
        (Face::Keyword, "bright-magenta", PLAIN),
        (Face::Type, "yellow", PLAIN),
        (Face::Literal, "green", shape(Modifier::ITALIC)),
        (Face::Number, "bright-blue", PLAIN),
        (Face::Comment, "gray", shape(Modifier::ITALIC)),
        (Face::Preproc, "magenta", PLAIN),
        (Face::Function, "blue", shape(Modifier::BOLD)),
        // Bold *and* underlined, so an inline change reads as inline rather than as another changed
        // line; the bright ends again, because they are the pair that stays apart in greyscale.
        (
            Face::WordOld,
            "bright-red",
            shape(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        (
            Face::WordNew,
            "bright-green",
            shape(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        (Face::Whitespace, "white", shape(Modifier::REVERSED)),
        (Face::Warning, "yellow", shape(Modifier::BOLD)),
        (Face::Failure, "red", shape(Modifier::BOLD)),
        (Face::Success, "green", shape(Modifier::BOLD)),
    ];
    // No grounds, and that is deliberate. This theme paints no background, so it does not know what is
    // under the text, and a ground chosen from somebody else's sixteen hues is a contrast claim about a
    // palette Fathom has not seen — white on someone's green is 3.0:1 and on someone else's 7:1. What
    // this theme can honestly control, and does, is which *slot* each face asks for.
    build("ansi", "default", "default", &rows)
}

/// Give the two word-diff faces their grounds.
fn with_grounds(theme: Theme, old_ground: &str, new_ground: &str) -> Theme {
    let mut theme = theme;
    let old = Ink::parse(old_ground).unwrap_or(Ink::Default);
    let new = Ink::parse(new_ground).unwrap_or(Ink::Default);
    let old_paint = theme.paint(Face::WordOld).on(old);
    let new_paint = theme.paint(Face::WordNew).on(new);
    theme.set(Face::WordOld, old_paint);
    theme.set(Face::WordNew, new_paint);
    theme
}

/// Assemble a theme from its rows.
///
/// Rows are `(face, ink, shape)` rather than `(face, paint)` because the ink has to be parsed and a
/// `const fn` cannot do that; the shapes come from the table above unchanged, so the tables remain the
/// whole truth about what each face looks like.
fn build(name: &str, background: &str, foreground: &str, rows: &[Row]) -> Theme {
    let mut paints = [PLAIN; FACE_COUNT];
    for (face, ink, shape) in rows {
        paints[face.index()] = Paint {
            ink: Ink::parse(ink).unwrap_or(Ink::Default),
            ..*shape
        };
    }
    Theme::assemble(
        name,
        Ink::parse(background).unwrap_or(Ink::Default),
        Ink::parse(foreground).unwrap_or(Ink::Default),
        paints,
        format!("built-in {name}"),
    )
}

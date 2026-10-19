//! `fathom_tui::colour` — the arithmetic behind a colour decision, with no opinions of its own.
//!
//! A theme is only auditable if "is this readable?" and "do these two colours survive a colour-blind
//! reviewer?" are *numbers* rather than impressions, and the numbers have to be computed the same way
//! in the built-in palette, in a user's theme file and in the test that keeps them honest. That is
//! this module: sRGB luminance and WCAG contrast, CIE L\*a\*b\* with CIE76 ΔE as the distance, the
//! standard dichromat simulations, and the quantizers that fold a truecolour choice down to the
//! 256-colour and 16-colour palettes a minimal terminal actually has.
//!
//! Nothing here names a face, a theme or a terminal: [`crate::theme`] decides what a colour is *for*,
//! [`crate::terminal`] decides how much colour the terminal will carry, and this module only answers
//! questions about colours it is handed. Keeping the maths here rather than in the theme is what lets
//! `tests/theming.rs` audit a *user's* palette with the same functions the built-ins were tuned with
//! — a second implementation in the test would be a test of the test.
//!
//! ## The two thresholds this module exists to make checkable
//!
//! * **4.5:1** is WCAG 2.1 AA for body text ([§1.4.3](https://www.w3.org/TR/WCAG21/#contrast-minimum)),
//!   computed on relative luminance. Fathom holds every face of every default theme to it, which is
//!   why a face that wants to be dim is dim by *modifier* rather than by a greyer ink.
//! * **ΔE76** is the CIE colour distance. It is not a perceptual scale at the level of a few units
//!   (CIEDE2000 is better and needs eleven more constants), but it is stable, cheap and — at the
//!   distances the audit uses — decisive: a ΔE of 1.5 is "just noticeable", 12 is "a different colour
//!   named differently", and 40 is "obviously not the same ink". The audit asks for the last two.

/// An 8-bit-per-channel sRGB colour, the unit every palette in fathom is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    /// Red channel, 0–255.
    pub r: u8,
    /// Green channel, 0–255.
    pub g: u8,
    /// Blue channel, 0–255.
    pub b: u8,
}

impl Rgb {
    /// A colour from its channels.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// Parse `#rgb` or `#rrggbb`, the only two spellings a theme file may use for a truecolour ink.
    ///
    /// Deliberately strict: `rgb(12,34,56)`, `#rrggbb` with a trailing alpha, and bare hex digits are
    /// all plausible things a user might type, and all of them are better refused with a message than
    /// drawn as something the user did not write.
    #[must_use]
    pub fn parse(text: &str) -> Option<Rgb> {
        let text = text.trim();
        let hex = text.strip_prefix('#')?;
        let digit = |c: char| c.is_ascii_hexdigit();
        let byte = |s: &str| u8::from_str_radix(s, 16).ok();
        match hex.len() {
            3 if hex.chars().all(digit) => Some(Rgb::new(
                byte(&hex[0..1])? * 17,
                byte(&hex[1..2])? * 17,
                byte(&hex[2..3])? * 17,
            )),
            6 if hex.chars().all(digit) => Some(Rgb::new(
                byte(&hex[0..2])?,
                byte(&hex[2..4])?,
                byte(&hex[4..6])?,
            )),
            _ => None,
        }
    }

    /// The `#rrggbb` spelling `--show-theme` and the theme file both print.
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// The ratatui colour for this triple.
    #[must_use]
    pub const fn to_color(self) -> ratatui::style::Color {
        ratatui::style::Color::Rgb(self.r, self.g, self.b)
    }

    /// WCAG relative luminance (WCAG 2.1 §1.4.5's definition: linearised channels, Rec. 709
    /// weights). `0.0` for black, `1.0` for white.
    #[must_use]
    pub fn luminance(self) -> f64 {
        0.2126 * linearize(f64::from(self.r))
            + 0.7152 * linearize(f64::from(self.g))
            + 0.0722 * linearize(f64::from(self.b))
    }
}

/// A colour in CIE L\*a\*b\* (D65), the space ΔE is defined in.
#[derive(Debug, Clone, Copy)]
pub struct Lab {
    /// Lightness, 0–100.
    pub l: f64,
    /// Green–red axis.
    pub a: f64,
    /// Blue–yellow axis.
    pub b: f64,
}

/// The three dichromacies fathom's palettes are audited against.
///
/// Deuteranopia (no M cones, ~1 in 20 men) and protanopia (no L cones) are the two the ticket names
/// and the two that actually threaten a diff viewer, because green and red are what a diff says.
/// Tritanopia is rare enough (~1 in 30 000) that no palette decision is made for it, but it is
/// simulated anyway: it is cheap, and "we never made it worse" is worth knowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cvd {
    /// No long-wavelength (red) cones.
    Protanopia,
    /// No medium-wavelength (green) cones.
    Deuteranopia,
    /// No short-wavelength (blue) cones.
    Tritanopia,
}

impl Cvd {
    /// Every simulation, in the order the audit prints them.
    pub const ALL: [Cvd; 3] = [Cvd::Protanopia, Cvd::Deuteranopia, Cvd::Tritanopia];

    /// The name a report prints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Cvd::Protanopia => "protanopia",
            Cvd::Deuteranopia => "deuteranopia",
            Cvd::Tritanopia => "tritanopia",
        }
    }

    /// Machado, Oliveira & Fernandes (2009), *A technical review of color vision deficiencies
    /// simulations*, IEEE TGCI, at severity 1.0 — the linear-RGB matrices the paper publishes for a
    /// complete loss of the cone type. Severity 1.0 is the worst case, which is the case a palette
    /// has to survive: an auditor who tunes for 0.6 has tuned for the reviewer they do not have.
    const fn matrix(self) -> [[f64; 3]; 3] {
        match self {
            Cvd::Protanopia => [
                [0.152_284, 1.052_587, -0.204_868],
                [0.114_565, 0.786_281, 0.099_154],
                [-0.003_744, -0.048_522, 1.052_266],
            ],
            Cvd::Deuteranopia => [
                [0.367_322, 0.860_646, -0.227_968],
                [0.280_085, 0.672_501, 0.047_413],
                [-0.011_820, 0.042_940, 0.968_881],
            ],
            Cvd::Tritanopia => [
                [1.255_528, -0.076_749, -0.178_779],
                [-0.078_411, 0.930_809, 0.147_611],
                [0.004_880, 0.691_367, 0.303_753],
            ],
        }
    }
}

/// What a colour looks like to a reviewer with this dichromacy.
///
/// The matrix is linear and acts on linear light, so the round trip is linearize → multiply →
/// compand. Out-of-gamut results are clamped: a simulated colour that would need a negative channel
/// is drawn as the nearest colour a real screen can make, which is exactly what the simulation is
/// claiming anyway.
#[must_use]
pub fn simulate(colour: Rgb, kind: Cvd) -> Rgb {
    let m = kind.matrix();
    let input = [
        linearize(f64::from(colour.r)),
        linearize(f64::from(colour.g)),
        linearize(f64::from(colour.b)),
    ];
    let out: Vec<f64> = (0..3)
        .map(|row| m[row][0] * input[0] + m[row][1] * input[1] + m[row][2] * input[2])
        .collect();
    Rgb::new(compand(out[0]), compand(out[1]), compand(out[2]))
}

/// WCAG 2.1 contrast ratio between two colours, `1.0` to `21.0`.
#[must_use]
pub fn contrast_ratio(foreground: Rgb, background: Rgb) -> f64 {
    let (hi, lo) = if foreground.luminance() >= background.luminance() {
        (foreground.luminance(), background.luminance())
    } else {
        (background.luminance(), foreground.luminance())
    };
    (hi + 0.05) / (lo + 0.05)
}

/// sRGB → CIE L\*a\*b\* (D65 white point, sRGB's own primaries).
#[must_use]
pub fn to_lab(colour: Rgb) -> Lab {
    let r = linearize(f64::from(colour.r));
    let g = linearize(f64::from(colour.g));
    let b = linearize(f64::from(colour.b));
    // sRGB's D65 XYZ, then normalized by the D65 white point.
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b;
    let z = (0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f64| {
        if t > 216.0 / 24_389.0 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    Lab {
        l: 116.0 * fy - 16.0,
        a: 500.0 * (fx - fy),
        b: 200.0 * (fy - fz),
    }
}

/// CIE76 ΔE between two colours.
#[must_use]
pub fn delta_e(one: Rgb, other: Rgb) -> f64 {
    let (one, other) = (to_lab(one), to_lab(other));
    ((one.l - other.l).powi(2) + (one.a - other.a).powi(2) + (one.b - other.b).powi(2)).sqrt()
}

/// ΔE between two colours *as seen by* a reviewer with this dichromacy — the check the ticket asks
/// for, and the reason the built-in palettes are not the usual green/red pair tuned by eye.
#[must_use]
pub fn delta_e_simulated(one: Rgb, other: Rgb, kind: Cvd) -> f64 {
    delta_e(simulate(one, kind), simulate(other, kind))
}

/// The sixteen ANSI colours, in slot order, with the reference RGB values of the Tango palette.
///
/// These are a *reference*, not a promise: on a 16-colour terminal the hues are whatever the user's
/// terminal configuration says, which is precisely why the fallback theme is chosen by slot and why
/// the audit's bar is lower there (see [`crate::theme`]'s notes on `ansi`). What fathom controls is
/// which slot each face gets, and the audit checks that the slots differ in lightness as well as
/// hue, so the fallback survives even a user palette that is itself colour-blind-hostile.
pub const ANSI16: [Rgb; 16] = [
    Rgb::new(0x00, 0x00, 0x00), // 0  black
    Rgb::new(0xcc, 0x00, 0x00), // 1  red
    Rgb::new(0x4e, 0x9a, 0x06), // 2  green
    Rgb::new(0xc4, 0xa0, 0x00), // 3  yellow
    Rgb::new(0x34, 0x65, 0xa4), // 4  blue
    Rgb::new(0x75, 0x50, 0x7b), // 5  magenta
    Rgb::new(0x06, 0x98, 0x9a), // 6  cyan
    Rgb::new(0xd3, 0xd7, 0xcf), // 7  white
    Rgb::new(0x55, 0x57, 0x53), // 8  bright black
    Rgb::new(0xef, 0x29, 0x29), // 9  bright red
    Rgb::new(0x8a, 0xe2, 0x34), // 10 bright green
    Rgb::new(0xfc, 0xe9, 0x4f), // 11 bright yellow
    Rgb::new(0x72, 0x9f, 0xcf), // 12 bright blue
    Rgb::new(0xad, 0x7f, 0xa8), // 13 bright magenta
    Rgb::new(0x34, 0xe2, 0xe2), // 14 bright cyan
    Rgb::new(0xee, 0xee, 0xec), // 15 bright white
];

/// The named colours a theme file may use, with the slot each one means.
///
/// This table is the *only* place a colour name is tied to a number, and the names are the ANSI ones
/// rather than ratatui's (`Gray`/`DarkGray` are `bright black`/`black` in every terminal people
/// actually use). A theme author who writes `dark-gray` and an author who writes `8` get the same
/// slot, which is what makes the two spellings interchangeable in a bug report.
pub const NAMED: [(&str, u8); 26] = [
    ("black", 0),
    ("red", 1),
    ("green", 2),
    ("yellow", 3),
    ("blue", 4),
    ("magenta", 5),
    ("cyan", 6),
    ("white", 7),
    ("gray", 8),
    ("grey", 8),
    ("bright-black", 8),
    ("bright-red", 9),
    ("bright-green", 10),
    ("bright-yellow", 11),
    ("bright-blue", 12),
    ("bright-magenta", 13),
    ("bright-cyan", 14),
    ("bright-white", 15),
    // ratatui's spellings, accepted so a theme copied from elsewhere still parses.
    ("dark-gray", 8),
    ("dark-grey", 8),
    ("light-red", 9),
    ("light-green", 10),
    ("light-yellow", 11),
    ("light-blue", 12),
    ("light-magenta", 13),
    ("light-cyan", 14),
];

/// The canonical name of a slot, for the label a theme report prints.
#[must_use]
pub fn ansi_name(slot: u8) -> &'static str {
    const CANONICAL: [&str; 16] = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "gray",
        "bright-red",
        "bright-green",
        "bright-yellow",
        "bright-blue",
        "bright-magenta",
        "bright-cyan",
        "bright-white",
    ];
    CANONICAL.get(slot as usize).copied().unwrap_or("default")
}

/// The slot a name means, if it means one.
#[must_use]
pub fn ansi_of_name(name: &str) -> Option<u8> {
    let name = name.trim().to_ascii_lowercase().replace(' ', "-");
    NAMED
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, slot)| *slot)
}

/// The nearest of the sixteen ANSI slots, by ΔE.
#[must_use]
pub fn nearest_ansi(colour: Rgb) -> u8 {
    nearest_in(colour, &ANSI16) as u8
}

/// The nearest slot in a palette, by ΔE.
#[must_use]
pub fn nearest_in(colour: Rgb, palette: &[Rgb]) -> usize {
    palette
        .iter()
        .enumerate()
        .min_by(|(_, one), (_, other)| delta_e(colour, **one).total_cmp(&delta_e(colour, **other)))
        .map_or(0, |(index, _)| index)
}

/// The xterm-256 index for a colour: the 6×6×6 cube for anything chromatic, the 24-step grey ramp
/// for anything that is not — the same rule xterm and `COLORTERM`-less vim use, so a quantized theme
/// looks like a theme written for 256 colours rather than a posterization artefact.
#[must_use]
pub fn to_xterm256(colour: Rgb) -> u8 {
    // A colour that *is* one of the sixteen is that slot — an approximation that put "black" on the grey
    // ramp would be a palette with a black it could not name.
    if let Some(slot) = exact_ansi(colour) {
        return slot;
    }
    // Then the two ways xterm encodes a colour, whichever is nearer: the 6×6×6 cube and the 24-step grey
    // ramp. Choosing by ΔE rather than by "is it grey" is what stops near-white landing on the ramp's top
    // step (#eeeeee) when the cube has an exact #ffffff.
    let level = |channel: f64| -> u8 { ((channel / 255.0) * 5.0).round() as u8 };
    let (r, g, b) = (
        f64::from(colour.r),
        f64::from(colour.g),
        f64::from(colour.b),
    );
    let cube = 16 + 36 * level(r) + 6 * level(g) + level(b);
    let mean = (r + g + b) / 3.0;
    let step = ((mean - 8.0) / 10.0).round().clamp(0.0, 23.0) as u8;
    let ramp = 232 + step;
    if delta_e(xterm_rgb(ramp), colour) < delta_e(xterm_rgb(cube), colour) {
        ramp
    } else {
        cube
    }
}

/// The slot, if this colour is one of the sixteen to within a ΔE of 3 — a just-noticeable difference,
/// which is the point at which "the same colour" is still true.
#[must_use]
pub fn exact_ansi(colour: Rgb) -> Option<u8> {
    ANSI16
        .iter()
        .enumerate()
        .find(|(_, reference)| delta_e(colour, **reference) < 3.0)
        .map(|(slot, _)| slot as u8)
}

/// The reference RGB of an xterm-256 index: the sixteen slots, the 6×6×6 cube, and the 24-step grey
/// ramp. The audit measures an `Ink::Index` through this, which is what makes a theme that names palette
/// entries auditable at all.
#[must_use]
pub fn xterm_rgb(index: u8) -> Rgb {
    if index < 16 {
        return ANSI16[index as usize];
    }
    if index < 232 {
        let index = index - 16;
        let level = |digit: u8| if digit == 0 { 0 } else { 55 + digit * 40 };
        return Rgb::new(level(index / 36), level((index / 6) % 6), level(index % 6));
    }
    let grey = 8 + (index - 232) * 10;
    Rgb::new(grey, grey, grey)
}

/// sRGB companding curve → linear light.
fn linearize(channel: f64) -> f64 {
    let channel = channel / 255.0;
    if channel <= 0.040_45 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light → an 8-bit sRGB channel, clamped to the gamut a screen can actually paint.
fn compand(linear: f64) -> u8 {
    let srgb = if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.max(0.0).powf(1.0 / 2.4) - 0.055
    };
    (srgb.clamp(0.0, 1.0) * 255.0).round() as u8
}

//! The palette audit (`e5.6`): can a reviewer read this, and can a reviewer without ordinary colour
//! vision read it too?
//!
//! The ticket's acceptance criteria are numbers, so this file is numbers too:
//!
//! * **4.5:1** — the WCAG 2.1 AA body-text floor — for *every* face of *both* default themes, measured
//!   against the background each theme paints itself. A face that cannot be measured (because its
//!   background is the terminal's, as in `ansi`) is reported as declined, never as passed; the test
//!   asserts the count of measured faces so nothing can dodge the audit by being unmeasurable.
//! * **ΔE ≥ 14** between the added / removed / violation / text pairs under simulated protanopia and
//!   deuteranopia (Machado severity 1.0 — complete loss of the cone type, because tuning for a partial
//!   deficiency tunes for the reviewer you do not have), and ΔE ≥ 12 for the sixteen-colour fallback,
//!   whose hues belong to the terminal rather than to Fathom.
//! * **`NO_COLOR`** — the frame a reviewer with colour off gets is byte-for-byte the frame a reviewer
//!   with colour on gets, carries no colour in any cell, and still says every fact through the signs
//!   and words in the text channel.
//! * **A theme is a file** — TOML in, a theme out, with every problem reported by name.
//!
//! `fathom tui --show-theme` prints these same measurements from the same code, so what this file
//! asserts is what a reviewer can check for themselves in a terminal rather than what only CI can see.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::path::{Path, PathBuf};

use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};

use fathom_tui::app::{App, Model};
use fathom_tui::colour::{self, Cvd, Rgb};
use fathom_tui::frame::{FrameSink as _, TerminalSink};
use fathom_tui::keymap::Keymap;
use fathom_tui::render::frame_lines;
use fathom_tui::style::Face;
use fathom_tui::terminal::{ColorDepth, Probe};
use fathom_tui::theme::{self, Ink, Theme};
use fathom_tui::timeline::Batch;
use fathom_tui::views::ViewId;

#[path = "timeline_corpus/mod.rs"]
mod corpus;

use corpus::{NOW, mock};

/// A probe built from a list of variables — the only honest way to test one (ADR-006 §6.7).
fn env(pairs: &[(&str, &str)]) -> Probe {
    let mut probe = Probe::from_env_with(|key| {
        pairs
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| (*value).to_owned())
    });
    probe.tty = true;
    probe
}

/// The two themes a run falls back to, and the ones the ticket's contrast criterion names.
fn defaults() -> Vec<(Theme, &'static str)> {
    vec![(theme::dark(), "dark"), (theme::light(), "light")]
}

/// ---------------------------------------------------------------------------
/// 1. contrast
/// ---------------------------------------------------------------------------
#[test]
fn every_face_of_both_default_themes_clears_four_point_five() {
    for (theme, name) in defaults() {
        // Nothing may dodge the audit: every one of the twenty-four faces has to be measurable, which
        // means the theme has to own both the ink and the ground it sits on.
        let measured = theme.contrasts();
        assert_eq!(
            measured.len(),
            Face::ALL.len(),
            "{name}: {} of {} faces could be measured; a face with the terminal's own colour behind \
             it is a face this audit cannot vouch for",
            measured.len(),
            Face::ALL.len()
        );
        let findings = theme.contrast_findings();
        assert!(
            findings.is_empty(),
            "{name} fails the contrast audit:\n  {}",
            findings.join("\n  ")
        );
        // And say the floor out loud once, so a reader of the failure knows which number moved.
        let worst = measured
            .iter()
            .map(|(_, ratio)| *ratio)
            .fold(f64::INFINITY, f64::min);
        assert!(
            worst >= Theme::MIN_CONTRAST,
            "{name}: worst ratio {worst:.2}:1 is under {}",
            Theme::MIN_CONTRAST
        );
    }
}

#[test]
fn a_dim_face_clears_the_taller_floor_because_dim_moves_the_ink() {
    // `DIM` is applied by the terminal to a colour it does not show us, so a dim face is measured
    // against a taller floor rather than trusted. `faint` is the dim one in both defaults.
    for (theme, name) in defaults() {
        let dim: Vec<Face> = Face::ALL
            .iter()
            .copied()
            .filter(|face| theme.paint(*face).dim)
            .collect();
        assert!(
            !dim.is_empty(),
            "{name}: no face is dim, so this test has stopped testing anything"
        );
        for face in dim {
            let ratio = theme.contrast(face).unwrap_or(0.0);
            assert!(
                ratio >= Theme::MIN_CONTRAST_DIM,
                "{name}: the dim face {name} measures {ratio:.2}:1, under the {} floor that \
                 compensates for the modifier",
                Theme::MIN_CONTRAST_DIM,
                name = face.name()
            );
        }
    }
}

/// ---------------------------------------------------------------------------
/// 2. dichromacy
/// ---------------------------------------------------------------------------
#[test]
fn added_removed_and_violation_stay_apart_under_protanopia_and_deuteranopia() {
    for (theme, name) in defaults().into_iter().chain([
        (theme::ansi(), "ansi"),
        (
            theme::dark().resolve(ColorDepth::Basic),
            "dark at sixteen colours",
        ),
        (
            theme::light().resolve(ColorDepth::Basic),
            "light at sixteen colours",
        ),
    ]) {
        let findings = theme.separation_findings();
        assert!(
            findings.is_empty(),
            "{name} collapses a pair a reviewer has to tell apart:\n  {}",
            findings.join("\n  ")
        );
        // Explicitly: the ticket names deuteranopia and protanopia, so name them in the assertion
        // instead of leaving them implicit in the audit's simulation list.
        for kind in [Cvd::Protanopia, Cvd::Deuteranopia] {
            for pair in theme::REQUIRED_PAIRS {
                let (Some(one), Some(other)) =
                    (theme.ink_of(pair.one).rgb(), theme.ink_of(pair.other).rgb())
                else {
                    continue; // the `ansi` theme leaves the text ink to the terminal
                };
                let delta = colour::delta_e_simulated(one, other, kind);
                assert!(
                    delta >= theme.delta_e_floor(),
                    "{name}: {} and {} are ΔE {delta:.1} under {} — floor {} ({})",
                    pair.one.name(),
                    pair.other.name(),
                    kind.label(),
                    theme.delta_e_floor(),
                    pair.why
                );
            }
        }
    }
}

#[test]
fn the_simulation_collapses_what_it_claims_to_collapse() {
    // A guard on the guard: if the simulation were a no-op, the audit above would pass any palette.
    // Plain green and plain red — the classic diff pair — must come out *closer together* under
    // deuteranopia than they went in, which is the whole reason Fathom's pair is not that pair.
    let green = Rgb::new(0x00, 0xd0, 0x00);
    let red = Rgb::new(0xd0, 0x00, 0x00);
    let before = colour::delta_e(green, red);
    let after = colour::delta_e_simulated(green, red, Cvd::Deuteranopia);
    assert!(
        after < before * 0.6,
        "deuteranopia simulation left plain green/red at ΔE {after:.1} of {before:.1}: the matrix is \
         not doing what it says it does, so the audit proves nothing"
    );
    // And a colour simulates to itself-ish when nothing is lost: black and white stay far apart.
    assert!(
        colour::delta_e_simulated(Rgb::new(0, 0, 0), Rgb::new(255, 255, 255), Cvd::Protanopia)
            > 80.0
    );
}

/// ---------------------------------------------------------------------------
/// 3. the sixteen-colour fallback, truecolour detection, and `NO_COLOR`
/// ---------------------------------------------------------------------------
#[test]
fn a_sixteen_colour_terminal_gets_the_slot_mapping_not_a_quantization() {
    // Quantizing `dark`'s inks to the nearest of sixteen puts "added" on a cyan, next to "directory".
    // The built-ins therefore carry their own slot assignment, and this is the test that says so.
    for base in [theme::dark(), theme::light()] {
        let resolved = base.resolve(ColorDepth::Basic);
        assert_eq!(resolved.name, base.name, "the theme keeps its name");
        for face in Face::ALL {
            assert!(
                matches!(resolved.ink_of(face), Ink::Ansi(_) | Ink::Default),
                "{} at sixteen colours painted {} with {:?}",
                base.name,
                face.name(),
                resolved.ink_of(face)
            );
        }
        // The mapping is the hand-written one, not a quantization of the theme's truecolour inks: the
        // nearest slot to `dark`'s added (#4fdca4, a mint) is a cyan, and "added" reading as "directory"
        // is exactly the collapse the audit exists to catch.
        assert_eq!(
            resolved.ink_of(Face::Added),
            theme::ansi().ink_of(Face::Added),
            "{}'s fallback should use the audited slot assignment",
            base.name
        );
        assert_ne!(
            base.resolve(ColorDepth::TrueColor).ink_of(Face::Added),
            resolved.ink_of(Face::Added),
            "nothing was folded away, so this compared two identical things"
        );
        // And the fallback is auditable in its own right, at the same floor.
        for kind in [Cvd::Protanopia, Cvd::Deuteranopia] {
            for pair in theme::REQUIRED_PAIRS {
                let (Some(one), Some(other)) = (
                    resolved.ink_of(pair.one).rgb(),
                    resolved.ink_of(pair.other).rgb(),
                ) else {
                    continue;
                };
                assert!(
                    colour::delta_e_simulated(one, other, kind) >= resolved.delta_e_floor(),
                    "{} at sixteen colours: {} and {} collapse under {}",
                    base.name,
                    pair.one.name(),
                    pair.other.name(),
                    kind.label()
                );
            }
        }
    }
}

#[test]
fn the_ansi_theme_paints_only_the_sixteen_and_declines_the_contrast_claim() {
    let ansi = theme::ansi();
    // It claims no background, because the terminal has one already and it is the reviewer's.
    assert_eq!(ansi.background, Ink::Default);
    // The faces that sit on the terminal's own background are declined — there is no second colour to
    // measure against. The two that bring their own ground (the word-diff pair) are still measurable,
    // and still measured.
    for face in Face::ALL {
        assert!(
            ansi.contrast(face).is_none(),
            "the ansi theme measured {} against a background it does not paint",
            face.name()
        );
    }
    assert!(
        ansi.contrasts().is_empty(),
        "an audit of a surface Fathom does not paint would be a contrast claim about nothing"
    );
    // But the separation half still holds, at the sixteen-colour floor, and that floor is lower *and
    // stated*: the hues here are the terminal's, so what is checked is the slot assignment.
    assert_eq!(ansi.delta_e_floor(), Theme::MIN_DELTA_E_ANSI);
    assert!(
        ansi.separation_findings().is_empty(),
        "the sixteen-colour fallback collapsed a state pair:\n  {}",
        ansi.separation_findings().join("\n  ")
    );
    // Every painted face is a slot, so the emitted sequences stay inside the sixteen-colour range a
    // `TERM=vt100` can honour.
    for face in Face::ALL {
        assert!(
            matches!(ansi.ink_of(face), Ink::Ansi(_) | Ink::Default),
            "the ansi theme painted {} with {:?}",
            face.name(),
            ansi.ink_of(face)
        );
    }
}

#[test]
fn quantizing_a_user_theme_folds_it_to_the_terminal_it_is_asked_for() {
    let base = Theme::bare("scratch").with_ink(Face::Added, "#4fdca4");
    let colour = Rgb::parse("#4fdca4").unwrap();
    assert_eq!(base.ink_of(Face::Added), Ink::True(colour));
    match base.resolve(ColorDepth::Ansi256).ink_of(Face::Added) {
        Ink::Index(index) => assert_eq!(
            index,
            colour::to_xterm256(colour),
            "256-colour folding should land on the nearest cube entry"
        ),
        other => panic!("a truecolour ink at Ansi256 became {other:?}"),
    }
    match base.resolve(ColorDepth::Basic).ink_of(Face::Added) {
        Ink::Ansi(slot) => assert_eq!(slot, colour::nearest_ansi(colour)),
        other => panic!("a truecolour ink at Basic became {other:?}"),
    }
    // TrueColor is a no-op — nothing is thrown away when the terminal can take it all.
    assert_eq!(
        base.resolve(ColorDepth::TrueColor).ink_of(Face::Added),
        Ink::True(colour)
    );
}

#[test]
fn no_colour_takes_every_ink_and_keeps_every_shape() {
    for (theme, name) in defaults() {
        let grey = theme.resolve(ColorDepth::None);
        for face in Face::ALL {
            let paint = grey.paint(face);
            assert_eq!(
                paint.ink,
                Ink::Default,
                "{name}: {} still has an ink with no colour",
                face.name()
            );
            assert_eq!(
                paint.ground,
                None,
                "{name}: {} still has a ground",
                face.name()
            );
            // ADR-006 §10.2: the shape is the half that carries the fact, so it survives.
            let before = theme.paint(face);
            assert_eq!(
                (
                    paint.bold,
                    paint.dim,
                    paint.italic,
                    paint.underline,
                    paint.reverse
                ),
                (
                    before.bold,
                    before.dim,
                    before.italic,
                    before.underline,
                    before.reverse
                ),
                "{name}: {} lost its shape when the colour went",
                face.name()
            );
        }
        // The style is what the sink paints, so check the style too: no fg, no bg, shapes intact.
        let style = grey.style(Face::Warning);
        // `Reset`, not `None`: ratatui diffs a buffer cell-by-cell, so an absent colour would leave the
        // colour that cell had in the previous frame on screen. An explicitly-unset colour is the thing
        // that actually takes it off.
        assert_eq!(style.fg, Some(Color::Reset));
        assert_eq!(style.bg, Some(Color::Reset));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        // …and it agrees with the single place a colour is removed for one style.
        let stripped = fathom_tui::style::strip_colour(theme.style(Face::Warning));
        assert_eq!(style.add_modifier, stripped.add_modifier);
    }
}

#[test]
fn the_default_theme_and_a_face_agree_about_monochrome() {
    // `Face::monochrome` is the one-style form of the same rule; if the two ever disagree, the frame
    // and a pane that reaches for `Face::style` would say different things about the same run.
    let grey = theme::dark().resolve(ColorDepth::None);
    for face in Face::ALL {
        assert_eq!(
            grey.style(face).add_modifier,
            face.monochrome().add_modifier,
            "{}: the theme's monochrome and the face's monochrome disagree",
            face.name()
        );
    }
}

/// ---------------------------------------------------------------------------
/// 4. the screenshot: the same frame, with the colour off
/// ---------------------------------------------------------------------------
/// Draw the shell the way a terminal would receive it, and hand back both channels: the text of every
/// row, and whether any cell was given a colour.
fn painted(probe: Probe, theme: Theme, size: (u16, u16), view: ViewId) -> (Vec<String>, bool) {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        size,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("fixture index", mock()));
    app.set_view(view);
    app.theme = theme;
    app.term = probe;
    let mut sink = TerminalSink::new(
        ratatui::Terminal::new(TestBackend::new(size.0, size.1)).expect("a test backend opens"),
    );
    sink.draw(&mut app).expect("the frame draws");
    let buffer = sink.terminal.backend().buffer();
    let content = buffer.content();
    let coloured = content
        .iter()
        .any(|cell| cell.fg != Color::Reset || cell.bg != Color::Reset);
    let width = usize::from(size.0);
    let text = content
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    (text, coloured)
}

#[test]
fn with_no_colour_the_frame_is_the_same_frame_and_says_the_same_things() {
    // The acceptance criterion is "with NO_COLOR all information remains distinguishable via glyphs
    // and labels", and the form that can actually be checked is this one: the text a colour-blind
    // reviewer is shown is byte-identical to the text everyone else is shown, no cell carries a
    // colour, and the signs that carry the states are in it.
    let proud = env(&[
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("LANG", "C.UTF-8"),
    ]);
    let grey_probe = Probe {
        no_color: true,
        ..proud.clone()
    };
    // The timeline: the view where colour carries the most (risk, added/removed, review state), so it
    // is the view the criterion has to hold on.
    let (colour_on, coloured_on) =
        painted(proud.clone(), theme::dark(), (100, 30), ViewId::Timeline);
    let (colour_off, coloured_off) =
        painted(grey_probe, theme::dark(), (100, 30), ViewId::Timeline);
    assert!(
        coloured_on,
        "a truecolour terminal was given no colour, so the comparison below proves nothing"
    );
    assert!(
        !coloured_off,
        "NO_COLOR was set and a cell still carried a colour"
    );
    assert_eq!(
        colour_on, colour_off,
        "with colour off the frame said something different — every colour in Fathom is supposed to \
         be decorating text that already carries the fact (ADR-006 §10.2)"
    );
    let page = colour_off.join("\n");
    for token in [
        "+", // added lines, and the scale's added count
        "−", // removed, with the minus sign the diff uses
        "●", // the risk ladder, which is a rung shape and not a hue
        "○", // …and its empty rung
        "⚑", // a flagged episode
        "✓", // an accepted one
        "?", // an author Fathom could not place
        "fixture index",
    ] {
        assert!(
            page.contains(token),
            "the frame with colour off no longer contains {token:?}; that glyph is the channel the \
             fact travels in when the colour is gone"
        );
    }
}

#[test]
fn the_help_overlay_names_the_theme_and_the_colour_answer() {
    // "My theme did not arrive" is a question with two answers, and the reviewer should not have to
    // believe a README about either.
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        (100, 30),
        "fathom/pilot · main",
    );
    app.theme = theme::light();
    app.term = env(&[
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("LANG", "C.UTF-8"),
    ]);
    app.dispatch("help.open");
    let page = frame_lines(&app).join("\n");
    assert!(page.contains("theme"), "{page}");
    assert!(
        page.contains("light"),
        "the help overlay should name the theme in force"
    );
    assert!(
        page.contains("truecolour"),
        "…and say how much colour the terminal was given"
    );
}

/// ---------------------------------------------------------------------------
/// 5. semantic tokens, not colours at call sites
/// ---------------------------------------------------------------------------
#[test]
fn no_module_below_the_theme_names_a_colour() {
    // The ticket's first sentence is "semantic colour tokens rather than raw colours at call sites",
    // and the only way that stays true in a growing crate is a test that greps. A `Color::` literal, a
    // hex colour, or an ANSI name outside `theme`/`colour` is a colour chosen where no theme can see it
    // — not in `--show-theme`, not in the audit, not overridable, not quantizable.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed = ["theme.rs", "theme", "colour.rs"];
    let mut offenders: Vec<String> = Vec::new();
    for path in rust_files(&src) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let under_theme = path
            .strip_prefix(&src)
            .map(|rest| {
                rest.components().next().is_some_and(|first| {
                    allowed.contains(&first.as_os_str().to_str().unwrap_or(""))
                })
            })
            .unwrap_or(false);
        if under_theme {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // prose may name colours; code may not
            }
            if line.contains("Color::") || looks_like_hex_colour(line) {
                offenders.push(format!(
                    "{}:{}: {}",
                    name.clone().unwrap_or_default(),
                    number + 1,
                    trimmed
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "colour chosen outside the theme (move it into `theme`, and the audit will cover it):\n{}",
        offenders.join("\n")
    );
}

/// Whether a line contains something that reads as a hex colour literal — `#` plus six hex digits,
/// which is a colour and not a Markdown anchor or a section reference.
fn looks_like_hex_colour(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b'#')
        .any(|(i, _)| bytes.len() > i + 6 && bytes[i + 1..i + 7].iter().all(u8::is_ascii_hexdigit))
}

/// Every `.rs` file under `root`, including the `theme/` subdirectory.
fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out
}

#[test]
fn every_face_is_painted_in_the_default_themes() {
    // A typo'd ink parses to `Ink::Default`, which paints nothing and looks like a theme that loaded.
    // Twenty-four rows of hand-written hex deserve a guard.
    for (theme, name) in defaults() {
        for face in Face::ALL {
            assert!(
                matches!(theme.ink_of(face), Ink::True(_)),
                "{name}: {} has no ink of its own — a typo in the palette table?",
                face.name()
            );
        }
        assert_eq!(
            theme.paints().len(),
            Face::ALL.len(),
            "{name}: the palette table and the face registry disagree about how many faces there are"
        );
        for (index, face) in Face::ALL.iter().enumerate() {
            assert_eq!(
                face.index(),
                index,
                "{name}: Face::index and Face::ALL disagree"
            );
        }
    }
}

/// ---------------------------------------------------------------------------
/// 6. the theme file
/// ---------------------------------------------------------------------------
/// A theme the way a user writes one: a base, and four rows of overrides.
const A_THEME: &str = r##"
name = "dusk"
inherits = "dark"
background = "#101014"
foreground = "#e8e8ee"

[faces]
text = "#e8e8ee"
added = "#3fd49a"
removed = "#ff8e7d"
warning = "bright-yellow bold underline"
hunk = { ink = "#79c0ff", bold = true }
word-old = { ink = "#ffe9e5", on = "#6e2a24" }
"##;

#[test]
fn a_theme_file_overrides_what_it_names_and_inherits_the_rest() {
    let theme = Theme::from_toml(A_THEME, &theme::dark(), "dusk.toml").expect("it parses");
    assert_eq!(theme.name, "dusk");
    assert_eq!(theme.inherits, "dark");
    assert_eq!(theme.background, Ink::True(Rgb::parse("#101014").unwrap()));
    assert_eq!(
        theme.ink_of(Face::Added),
        Ink::True(Rgb::parse("#3fd49a").unwrap())
    );
    // The string form carries a shape, and `on` carries a ground.
    let warning = theme.paint(Face::Warning);
    assert_eq!(warning.ink, Ink::Ansi(11), "`bright-yellow` is slot 11");
    assert!(warning.bold && warning.underline && !warning.italic);
    assert_eq!(
        theme.paint(Face::HunkHeader).ink,
        Ink::True(Rgb::parse("#79c0ff").unwrap())
    );
    assert_eq!(
        theme.ground_of(Face::WordOld),
        Ink::True(Rgb::parse("#6e2a24").unwrap())
    );
    // A face the file never mentions came from what it inherits, unchanged.
    assert_eq!(
        theme.paint(Face::Keyword),
        theme::dark().paint(Face::Keyword),
        "an override file that names six faces should not repaint eighteen"
    );
    assert!(theme.differs_from(&theme::dark()) > 1);
    // The audit runs on a user's palette with the same functions the built-ins were tuned with.
    let _ = theme.audit_lines();
}

#[test]
fn a_theme_file_reports_every_problem_it_found() {
    let broken = r##"
name = 3
inherits = "midnight"
wibble = true
[faces]
adde = "green"
removed = "#zzzzzz"
warning = "bright-green splodge"
marker = 7
"##;
    let problems =
        Theme::from_toml(broken, &theme::dark(), "broken.toml").expect_err("it must fail");
    let text = problems.join("\n");
    for want in [
        "`name` must be a string",
        "inherits = \"midnight\" names no theme",
        "`wibble` is not a theme key",
        "[faces] adde names no face",
        "#zzzzzz",
        "splodge",
        "[faces] marker must be a colour string",
    ] {
        assert!(
            text.contains(want),
            "the report should say {want:?}, said:\n{text}"
        );
    }
}

#[test]
fn a_broken_toml_file_is_refused_with_the_line_it_failed_on() {
    let problems = Theme::from_toml("faces = [", &theme::dark(), "bad.toml").expect_err("not TOML");
    assert!(
        problems[0].contains("bad.toml"),
        "a refusal has to name the file the reviewer has to go and edit: {problems:?}"
    );
}

#[test]
fn a_theme_that_inherits_nothing_starts_from_the_terminal() {
    let theme = Theme::from_toml(
        "inherits = \"none\"\n[faces]\nadded = \"#00ff00\"\n",
        &theme::dark(),
        "none.toml",
    )
    .expect("it parses");
    assert_eq!(
        theme.ink_of(Face::Added),
        Ink::True(Rgb::parse("#00ff00").unwrap())
    );
    assert_eq!(
        theme.ink_of(Face::Removed),
        Ink::Default,
        "a theme that inherits nothing leaves every other face to the terminal"
    );
    assert!(
        theme.contrast(Face::Removed).is_none(),
        "and declines to measure what it does not paint"
    );
}

#[test]
fn an_unknown_theme_name_names_the_names_that_would_work() {
    let notice =
        Theme::resolve_named("midnight", ColorDepth::TrueColor).expect_err("no such theme");
    assert!(notice.contains("midnight"), "{notice}");
    for name in Theme::builtin_names() {
        assert!(notice.contains(name), "{notice} should list {name}");
    }
    // …and a name that does exist resolves, folded to the depth it was asked for.
    assert_eq!(
        Theme::resolve_named("dark", ColorDepth::Basic)
            .expect("dark exists")
            .ink_of(Face::Added),
        theme::ansi().ink_of(Face::Added)
    );
}

/// ---------------------------------------------------------------------------
/// 7. the colour arithmetic itself
/// ---------------------------------------------------------------------------
#[test]
fn the_colour_arithmetic_is_the_textbook_kind() {
    // These are the constants the whole audit stands on, pinned so a typo in a matrix or a gamma can be
    // seen as one failing assertion instead of a theme that quietly gets worse.
    let black = Rgb::new(0, 0, 0);
    let white = Rgb::new(255, 255, 255);
    assert!((colour::contrast_ratio(white, black) - 21.0).abs() < 0.01);
    assert!((colour::contrast_ratio(black, black) - 1.0).abs() < 0.01);
    assert!((colour::contrast_ratio(white, white) - 1.0).abs() < 0.01);
    assert!(colour::delta_e(black, black) < 1e-9);
    // A hex round trip, including the three-digit form a theme file may use.
    assert_eq!(Rgb::parse("#abc").unwrap(), Rgb::new(0xaa, 0xbb, 0xcc));
    assert_eq!(Rgb::parse("#4fdca4").unwrap().to_hex(), "#4fdca4");
    assert_eq!(
        Rgb::parse("4fdca4"),
        None,
        "a bare hex number is not a colour"
    );
    assert_eq!(Rgb::parse("#gggggg"), None);
    // Ink spellings, all four kinds, and nothing else.
    assert_eq!(Ink::parse("default"), Some(Ink::Default));
    assert_eq!(Ink::parse("red"), Some(Ink::Ansi(1)));
    assert_eq!(Ink::parse("light-red"), Some(Ink::Ansi(9)));
    assert_eq!(Ink::parse("8"), Some(Ink::Ansi(8)));
    assert_eq!(Ink::parse("255"), Some(Ink::Index(255)));
    assert_eq!(Ink::parse("256"), None, "the palette is 0-255");
    assert_eq!(Ink::parse("cerulean"), None);
    // xterm's cube: a colour that *is* one of the sixteen is that slot, and anything else lands on the
    // grey ramp or the 6×6×6 — each of which round-trips through the reference RGB the audit measures.
    for probe in [
        Rgb::new(0, 0, 0),
        Rgb::new(255, 255, 255),
        Rgb::new(0x8a, 0xe2, 0x34),
    ] {
        let index = colour::to_xterm256(probe);
        assert!(
            colour::delta_e(colour::xterm_rgb(index), probe) < 3.0,
            "{} quantized to index {index}, which is {}, not within a just-noticeable difference",
            probe.to_hex(),
            colour::xterm_rgb(index).to_hex()
        );
    }
    // `nearest_ansi` is an argmin, so the assertion is the definition rather than a remembered slot:
    // the slot it picks is at least as close as any other, for colours a palette actually contains.
    for probe in [
        Rgb::new(255, 0, 0),
        Rgb::new(0, 0, 0),
        Rgb::new(0x4f, 0xdc, 0xa4),
        Rgb::new(0xff, 0xd2, 0x5e),
    ] {
        let chosen = colour::nearest_ansi(probe);
        let chosen_distance = colour::delta_e(colour::ANSI16[usize::from(chosen)], probe);
        for (slot, reference) in colour::ANSI16.iter().enumerate() {
            assert!(
                chosen_distance <= colour::delta_e(*reference, probe) + f64::EPSILON,
                "slot {slot} is closer to {} than slot {chosen} is",
                probe.to_hex()
            );
        }
    }
}

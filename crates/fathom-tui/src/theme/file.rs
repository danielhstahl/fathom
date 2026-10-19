//! A theme as a user writes it: TOML in, a [`Theme`] and a list of everything that was not understood.
//!
//! This module reads **text it is handed** and never a file — `fathom-cli` opens the file, because
//! [`crate`] opens nothing and reads no environment (ADR-002 §5). What it returns is either a complete
//! theme or every problem found in the document, never a theme with a silent hole in it: a palette that
//! half-applied is a palette whose author cannot tell what they changed, which is the failure mode
//! `fathom.toml`'s key handling already refuses for unknown keys.
//!
//! ## The file
//!
//! ```toml
//! # ~/.config/fathom/themes/dusk.toml — or any path given to `fathom --theme`
//! name = "dusk"                 # optional; the file stem, or the argument, when absent
//! inherits = "dark"             # optional; "dark", "light", "ansi", "dark-ansi", or "none"
//! background = "#181a20"        # optional; "default" = the terminal's own, unpainted
//! foreground = "#e4e6ec"        # optional; the ink of a plain text run
//!
//! [faces]
//! text     = "#e4e6ec"
//! added    = "#3fd49a"
//! removed  = "#ff8e7d"
//! warning  = "bright-yellow bold underline"
//! hunk     = { ink = "#79c0ff", bold = true }
//! word-old = { ink = "#ffe9e5", on = "#6e2a24" }
//! ```
//!
//! A value is either a string — `<ink> [on <ink>] [shape …]`, space-separated, in any order — or a
//! table with `ink` (or `fg`), `on` (or `bg`), and the boolean shapes `bold`, `dim`, `italic`,
//! `underline`, `reverse`, plus `none = true` to take every shape off. Ink spellings are exactly the
//! four [`Ink`] documents: `default`, an ANSI slot name, a 0–255 index, or `#rgb`/`#rrggbb`.
//!
//! Faces are named by their registry name (`word-old`, `hunk`, `hunk-header` — the [`Face::name`]
//! spellings, with `_` accepted for `-`), and only faces named in the file are overridden: `inherits`
//! supplies the rest, which is why a four-line file is a useful theme.

use ratatui::style::Modifier;
use toml_edit::{Item, TableLike};

use super::{Ink, Paint, Theme};
use crate::style::Face;

/// Parse a theme document over a base theme.
///
/// `fallback` is the base used when the document does not name an `inherits`, so `--theme` and the
/// config layer can decide what an incomplete file means without knowing anything about themes, and
/// `source` is the name the problems are attributed to (a path, or `--theme dusk`).
///
/// The error half is every problem, not the first one: a user who mistyped two colours should read one
/// report, not run the command twice.
pub fn parse(text: &str, fallback: &Theme, source: &str) -> Result<Theme, Vec<String>> {
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(err) => return Err(vec![format!("{source} does not parse as TOML: {err}")]),
    };
    let mut problems: Vec<String> = Vec::new();

    // --- top-level keys, and the list that refuses anything else -----------------------------
    let mut inherits = String::new();
    let mut name = String::new();
    let mut background: Option<Ink> = None;
    let mut foreground: Option<Ink> = None;
    let mut face_rows: Vec<(Face, Paint)> = Vec::new();

    for (key, item) in document.iter() {
        match key {
            "name" => match item.as_str() {
                Some(text) => name = text.to_owned(),
                None => problems.push(format!("{source}: `name` must be a string")),
            },
            "inherits" => match item.as_str() {
                Some(text) => inherits = text.to_owned(),
                None => problems.push(format!("{source}: `inherits` must be a string")),
            },
            "background" | "background-color" => {
                if let Some(ink) = ink_of(key, item, source, &mut problems) {
                    background = Some(ink);
                }
            }
            "foreground" | "foreground-color" => {
                if let Some(ink) = ink_of(key, item, source, &mut problems) {
                    foreground = Some(ink);
                }
            }
            "faces" => {
                let Some(table) = item.as_table_like() else {
                    problems.push(format!(
                        "{source}: [faces] must be a table of name = colour"
                    ));
                    continue;
                };
                for (raw, entry) in table.iter() {
                    if let Some(row) = face_paint(raw, entry, source, &mut problems) {
                        face_rows.push(row);
                    }
                }
            }
            other => problems.push(format!(
                "{source}: `{other}` is not a theme key; the keys are name, inherits, background, \
                 foreground and [faces]"
            )),
        }
    }

    // `inherits = "none"` is how a file says "start from nothing": every face it does not name then
    // falls back to the terminal, which is a legitimate minimal theme and not a missing field.
    let mut theme = match inherits.as_str() {
        "" => fallback.clone(),
        "none" | "nothing" => Theme::bare(if name.is_empty() { source } else { &name }),
        other => match Theme::builtin(other) {
            Some(base) => base,
            None => {
                problems.push(format!(
                    "{source}: inherits = \"{other}\" names no theme; the built-ins are {}, or \
                     \"none\" to start from the terminal's own colours",
                    Theme::builtin_names().join(", ")
                ));
                fallback.clone()
            }
        },
    };
    theme.inherits = inherits.clone();
    if !name.is_empty() {
        theme.name = name;
    }
    theme.source = source.to_owned();
    if theme.inherits.is_empty() {
        theme.inherits = fallback.name.clone();
    }
    if let Some(ink) = background {
        theme.background = ink;
    }
    if let Some(ink) = foreground {
        theme.foreground = ink;
    }
    for (face, paint) in face_rows {
        theme.set(face, paint);
    }
    if problems.is_empty() {
        Ok(theme)
    } else {
        Err(problems)
    }
}

/// One `[faces]` row: the face it names and the paint it asks for.
fn face_paint(
    raw: &str,
    item: &Item,
    source: &str,
    problems: &mut Vec<String>,
) -> Option<(Face, Paint)> {
    let want = raw.replace('_', "-");
    let face = Face::ALL.iter().find(|face| face.name() == want).copied();
    let Some(face) = face else {
        problems.push(format!(
            "{source}: [faces] {raw} names no face; the names are {}",
            Face::ALL
                .iter()
                .map(|face| face.name())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        return None;
    };
    let paint = if let Some(text) = item.as_str() {
        match paint_from_text(text) {
            Ok(paint) => paint,
            Err(problem) => {
                problems.push(format!("{source}: [faces] {raw} = \"{text}\": {problem}"));
                return None;
            }
        }
    } else if let Some(table) = item.as_table_like() {
        paint_from_table(raw, table, source, problems)?
    } else {
        problems.push(format!(
            "{source}: [faces] {raw} must be a colour string (\"#rrggbb bold\") or a table \
             ({{ ink = \"#rrggbb\", bold = true }})"
        ));
        return None;
    };
    Some((face, paint))
}

/// The string form: `<ink> [on <ink>] [shape …]`.
fn paint_from_text(text: &str) -> Result<Paint, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Err("empty; write a colour, or \"default\"".to_owned());
    }
    let mut paint = Paint::shape(Modifier::empty());
    // The ink, then optionally `on <ink>`, then any number of shape words — the order a person reads a
    // colour out loud in, which is the order they will type it in.
    let first = words[0];
    let Some(ink) = Ink::parse(first) else {
        return Err(format!(
            "\"{first}\" is not a colour: use #rgb, #rrggbb, an index 0-255, an ANSI name (red, \
             bright-green, gray …) or \"default\""
        ));
    };
    paint.ink = ink;
    let mut rest = &words[1..];
    if rest.first() == Some(&"on") {
        let Some(ground) = rest.get(1) else {
            return Err("`on` needs a colour after it".to_owned());
        };
        let Some(ink) = Ink::parse(ground) else {
            return Err(format!("\"{ground}\" after `on` is not a colour"));
        };
        paint.ground = Some(ink);
        rest = &words[2..];
    }
    for word in rest {
        apply_shape(&mut paint, word)?;
    }
    Ok(paint)
}

/// The table form.
fn paint_from_table(
    raw: &str,
    table: &dyn TableLike,
    source: &str,
    problems: &mut Vec<String>,
) -> Option<Paint> {
    let mut paint = Paint::shape(Modifier::empty());
    let mut broken = false;
    for (key, entry) in table.iter() {
        match key {
            "ink" | "fg" | "foreground" | "color" | "colour" => {
                match ink_of(&format!("[faces] {raw}.{key}"), entry, source, problems) {
                    Some(ink) => paint.ink = ink,
                    None => broken = true,
                }
            }
            "on" | "bg" | "background" | "ground" => {
                match ink_of(&format!("[faces] {raw}.{key}"), entry, source, problems) {
                    Some(ink) => paint.ground = Some(ink),
                    None => broken = true,
                }
            }
            "bold" | "dim" | "italic" | "underline" | "reverse" => {
                let Some(want) = entry.as_bool() else {
                    problems.push(format!(
                        "{source}: [faces] {raw}.{key} must be true or false"
                    ));
                    broken = true;
                    continue;
                };
                match key {
                    "bold" => paint.bold = want,
                    "dim" => paint.dim = want,
                    "italic" => paint.italic = want,
                    "underline" => paint.underline = want,
                    _ => paint.reverse = want,
                }
            }
            "none" => {
                paint = Paint::shape(Modifier::empty());
            }
            other => {
                problems.push(format!(
                    "{source}: [faces] {raw}.{other} is not a paint key; the keys are ink, on, \
                     bold, dim, italic, underline, reverse and none"
                ));
                broken = true;
            }
        }
    }
    (!broken).then_some(paint)
}

/// One ink, from a string value. Always reports, never fails quietly: an ink that could not be read is
/// the one mistake a theme author must be told about, because the alternative is a face painted in the
/// terminal's own colour and a mystery.
fn ink_of(key: &str, item: &Item, source: &str, problems: &mut Vec<String>) -> Option<Ink> {
    let Some(text) = item.as_str() else {
        problems.push(format!("{source}: `{key}` must be a colour string"));
        return None;
    };
    match Ink::parse(text) {
        Some(ink) => Some(ink),
        None => {
            problems.push(format!(
                "{source}: `{key} = \"{text}\"` is not a colour: use #rgb, #rrggbb, an index \
                 0-255, an ANSI name or \"default\""
            ));
            None
        }
    }
}

/// Apply one shape word.
fn apply_shape(paint: &mut Paint, word: &str) -> Result<(), String> {
    match word {
        "bold" => paint.bold = true,
        "dim" => paint.dim = true,
        "italic" => paint.italic = true,
        "underline" => paint.underline = true,
        "reverse" | "reversed" => paint.reverse = true,
        "none" | "plain" => *paint = Paint::shape(Modifier::empty()),
        other => {
            return Err(format!(
                "\"{other}\" is not a shape; the shapes are bold, dim, italic, underline, \
                 reverse, none"
            ));
        }
    }
    Ok(())
}

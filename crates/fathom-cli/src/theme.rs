//! Which theme a run gets, and what Fathom says about it (`e5.6`, ADR-006 §10.2).
//!
//! This is a composition-root module on purpose. Choosing a theme means reading a flag, a variable, a
//! user file and the terminal's own colour answer, and `fathom-tui` may do none of those (ADR-002 §5:
//! it is handed the world, it does not go and fetch it). So the whole selection lives here, and what
//! goes *into* the shell is one resolved [`Theme`] beside the [`Probe`] it was resolved against.
//!
//! ## Where a theme may come from, and where it may not
//!
//! In precedence order:
//!
//! 1. `--theme <name|path>` on the command line;
//! 2. `$FATHOM_THEME`;
//! 3. `[theme] name` in the **user** config — `$XDG_CONFIG_HOME/fathom/config.toml`, then
//!    `~/.config/fathom/config.toml`, then `~/.fathom/config.toml`, first one that exists wins;
//! 4. the built-in `dark`.
//!
//! Deliberately absent: the repository's own `fathom.toml`. A clone repaints your terminal is not a
//! feature — the same rule that keeps a repository out of `privacy.*` and out of `mcp.auth_token`
//! (ADR-001 §4.6: config a clone provides is untrusted config) applies to how your screen looks. A
//! repository may say what its *keymap* is; it may not say what your *eyes* want.
//!
//! ## What is printed about it
//!
//! A theme the reviewer cannot see is a theme that silently did not apply, which is the complaint this
//! whole ticket is a reply to. So: the help overlay names the theme and the colour depth
//! ([`crate::tui`]), `--show-theme` prints the resolved theme's face table with its measured contrast
//! and dichromacy separations, `--list-themes` prints the ones Fathom ships and the files it found,
//! and every fallback — an unknown name, a file that would not parse — is a notice saying what was used
//! instead, not a shrug.

use std::path::{Path, PathBuf};

use eyre::Result;
use fathom_core::exit::ExitCode;
use fathom_tui::terminal::{ColorDepth, Probe};
use fathom_tui::theme::{self, Theme};

use crate::output;
use crate::verbs::Globals;

/// A theme name that means "the built-in default", for the messages that name the fallback.
const DEFAULT_THEME: &str = "dark";

/// Where the user's own theme files are looked up, in order, best first.
///
/// `$FATHOM_THEME_DIR` is first because it is the one that costs nothing to set for a dotfiles repo,
/// which is how a team ends up sharing a palette.
fn theme_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("FATHOM_THEME_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(dir) = config_dir() {
        dirs.push(dir.join("themes"));
    }
    dirs
}

/// The user config directory, in the order the layers are consulted elsewhere in this binary
/// (`fathom doctor` prints these same candidates).
fn config_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(home).join("fathom"));
    }
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join(".config").join("fathom"))
}

/// The user config files, first-existing-wins, in precedence order.
fn user_config_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = config_dir() {
        out.push(dir.join("config.toml"));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        out.push(PathBuf::from(home).join(".fathom").join("config.toml"));
    }
    out
}

/// The `[theme] name` the user config asks for, and any problem reading it.
///
/// A missing `[theme]` table is the normal case and says nothing at all; a config that does not parse
/// is reported once and then ignored, because the alternative is that a typo in an unrelated section
/// stops the reviewer from opening the TUI.
fn configured_name() -> (Option<String>, Vec<String>) {
    for path in user_config_candidates() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let label = path.display().to_string();
        let document = match text.parse::<toml_edit::DocumentMut>() {
            Ok(document) => document,
            Err(err) => {
                return (
                    None,
                    vec![format!(
                        "{label} does not parse, so no configured theme was read: {err}"
                    )],
                );
            }
        };
        let Some(section) = document.get("theme").and_then(toml_edit::Item::as_table) else {
            return (None, Vec::new());
        };
        let name = section
            .get("name")
            .and_then(toml_edit::Item::as_str)
            .map(str::to_owned);
        for (key, _) in section.iter() {
            if key != "name" {
                return (
                    name,
                    vec![format!(
                        "{label}: [theme] {key} is not a key Fathom reads; the keys are `name` \
                         (a built-in name, a file in your theme directory, or a path)"
                    )],
                );
            }
        }
        return (name, Vec::new());
    }
    (None, Vec::new())
}

/// Resolve the theme for a run, and the notices the shell must show.
///
/// What comes back is the theme **as written**, not folded to the terminal: the sink folds it
/// ([`Theme::resolve`]) at draw time, against the probe it is about to paint into. That is one less
/// place that has to know the depth, and it is what lets `--show-theme` audit the palette a reviewer
/// chose even in a terminal that was told to paint no colour at all.
#[must_use]
pub fn resolve(globals: &Globals, _probe: &Probe) -> (Theme, Vec<String>) {
    let (configured, mut notices) = configured_name();
    let asked = globals
        .theme
        .clone()
        .or_else(|| {
            std::env::var("FATHOM_THEME")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or(configured);
    let name = match asked.as_deref() {
        Some(name) => name,
        // Nobody asked, so nothing is reported: a default that announces itself is noise, and the help
        // overlay says what is in force without being asked.
        None => return (theme::dark(), notices),
    };
    match load(name, &mut notices) {
        Loaded::Theme(chosen) => (chosen, notices),
        // A file *was* found and refused: the notices above already say which file and what was wrong
        // with it, and adding "that theme does not exist" on top would describe a different problem.
        Loaded::Explained => (theme::dark(), notices),
        Loaded::Missing => {
            notices.push(Theme::unknown_notice(name));
            // Where a file *would* have been found, because that is the sentence after "I don't know
            // that name", and the directories are this crate's knowledge rather than the TUI's.
            let mut places: Vec<String> = Vec::new();
            if let Some(dir) = std::env::var_os("FATHOM_THEME_DIR") {
                places.push(Path::new(&dir).display().to_string());
            }
            places.extend(theme_dirs().iter().map(|dir| dir.display().to_string()));
            // The printer adds the `fathom: ` prefix, as it does for every notice.
            notices.push(format!(
                "`{name}.toml` was looked for in: {}.",
                places.join(", ")
            ));
            (theme::dark(), notices)
        }
    }
}

/// What `load` came back with, told apart because the fallback sentence differs.
enum Loaded {
    /// A theme, from a built-in name, a theme-directory file, or a path.
    Theme(Theme),
    /// Something matched the name but was refused, and `notices` already says why.
    Explained,
    /// Nothing matched the name at all.
    Missing,
}

/// Find one theme by name or path, returning it resolved for `depth`, or `None` with a notice explaining.
///
/// A path is anything that looks like one — it contains a separator or ends in `.toml` — and a bare name
/// is a built-in first, then a file in the theme directories. That order means a file called
/// `dark.toml` in a user's directory cannot shadow the built-in `dark`, which is what you want when the
/// built-in is the fallback you just fell back to.
fn load(name: &str, notices: &mut Vec<String>) -> Loaded {
    let looks_like_path = name.contains('/') || name.ends_with(".toml");
    if !looks_like_path {
        if let Some(built) = Theme::builtin(name) {
            return Loaded::Theme(built);
        }
        let file = theme_dirs()
            .into_iter()
            .map(|dir| dir.join(format!("{name}.toml")))
            .find(|path| path.is_file());
        let Some(file) = file else {
            return Loaded::Missing;
        };
        return read(&file, notices);
    }
    let path = PathBuf::from(name);
    if !path.is_file() {
        notices.push(format!(
            "--theme {name} is not a file Fathom could read. A path is relative to the directory \
             you ran fathom in; a bare name is looked up as a built-in and then as \
             <theme directory>/{name}.toml."
        ));
        return Loaded::Explained;
    }
    read(&path, notices)
}

/// Read and parse one theme file. Every problem in it is a notice: a half-read theme is worse than the
/// fallback, because a half-read theme looks like a theme you wrote.
fn read(path: &Path, notices: &mut Vec<String>) -> Loaded {
    let label = path.display().to_string();
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            notices.push(format!("{label} could not be read: {err}"));
            return Loaded::Explained;
        }
    };
    match Theme::from_toml(&text, &Theme::default_theme(), &label) {
        Ok(theme) => Loaded::Theme(theme),
        Err(problems) => {
            for problem in problems {
                notices.push(problem);
            }
            notices.push(format!(
                "{label} was refused, so {DEFAULT_THEME} is in force. `fathom tui --show-theme` \
                 prints the theme in force and its audit."
            ));
            Loaded::Explained
        }
    }
}

/// `fathom tui --list-themes`: every theme Fathom knows, built-in and file, one line each.
///
/// Printed with the same table the audit uses so that "which one should I pick?" is answerable from the
/// terminal rather than from a screenshot in a README.
pub fn list() -> ExitCode {
    for built in Theme::builtins() {
        output::line(format_args!(
            "{:<11} {:<22} background {:<14} foreground {}",
            built.name,
            format!("({})", built.source),
            built.background.label(),
            built.foreground.label()
        ));
    }
    let mut found = false;
    for dir in theme_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "toml") {
                continue;
            }
            found = true;
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let note =
                match Theme::from_toml(&text, &Theme::default_theme(), &path.display().to_string())
                {
                    Ok(theme) => format!("{} faces overridden", count_overridden(&theme)),
                    Err(problems) => {
                        format!("refused: {} problem(s) — {}", problems.len(), problems[0])
                    }
                };
            output::line(format_args!(
                "{:<11} {:<22} {note}",
                stem,
                path.display().to_string()
            ));
        }
    }
    if !found {
        let where_to_look = theme_dirs()
            .iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        output::problem(format_args!(
            "fathom: no theme files found in {where_to_look} — a theme file is a TOML document with \
             `[faces]` rows; `fathom tui --show-theme --theme dark` prints the shape of one"
        ));
    }
    ExitCode::Ok
}

/// How many faces a parsed theme actually overrode — the number that tells a user whether their
/// four-line file did anything, and how much of it came from what it inherits.
fn count_overridden(theme: &Theme) -> usize {
    let base = Theme::builtin(&theme.inherits).unwrap_or_else(|| Theme::bare(&theme.inherits));
    theme.differs_from(&base)
}

/// `fathom tui --show-theme [--audit]`: the resolved theme, its table, and its measurements.
///
/// This is the automated check the ticket asked for, in the hands of the person who has to decide
/// something: a reviewer choosing a theme, a maintainer reviewing a theme file in a merge request, or
/// whoever is being told a colour "looks wrong". `--audit` is the strict form — it exits non-zero when
/// the theme fails its own floors, so it can be a gate on a palette the way `fathom check` is a gate
/// on a codebase.
pub fn show(globals: &Globals, probe: &Probe, audit: bool) -> Result<ExitCode> {
    let (theme, notices) = resolve(globals, probe);
    let depth = probe.color();
    for notice in &notices {
        output::problem(format_args!("fathom: {notice}"));
    }
    for line in theme.audit_lines() {
        output::line(format_args!("{line}"));
    }
    output::line(format_args!("  terminal: {}", probe.label()));
    if depth == ColorDepth::None {
        output::line(format_args!(
            "  note: this terminal was told to carry no colour, so none of this palette will be \
             painted; the frame keeps every shape and every word (ADR-006 §10.2)"
        ));
    }
    let contrast = theme.contrast_findings();
    let separation = theme.separation_findings();
    if contrast.is_empty() && separation.is_empty() {
        // A pass says what was measured. "pass" from a theme that could not be measured at all would be
        // the one sentence in this report that a reader should not trust.
        let (measured, separations) = (theme.contrasts().len(), theme.separations().len());
        let half = if measured == 0 {
            format!(
                "no face was measurable against a background this theme does not paint, and {} \
                 pair-simulations cleared ΔE {:.1}",
                separations,
                theme.delta_e_floor()
            )
        } else {
            format!(
                "{} faces cleared {:.1}:1 and {} pair-simulations cleared ΔE {:.1}",
                measured,
                Theme::MIN_CONTRAST,
                separations,
                theme.delta_e_floor()
            )
        };
        output::line(format_args!(
            "  audit: pass — {half}, under protanopia and deuteranopia simulation"
        ));
        return Ok(ExitCode::Ok);
    }
    for finding in contrast.iter().chain(separation.iter()) {
        output::line(format_args!("  FINDING {finding}"));
    }
    if audit {
        return Err(crate::verbs::pending(
            ExitCode::CheckViolations,
            format!(
                "theme {} failed its own audit: {} contrast finding(s), {} separation finding(s)",
                theme.name,
                contrast.len(),
                separation.len()
            ),
            Some(
                "raise the contrast or separate the hues; `fathom tui --show-theme --theme dark` \
                 prints a palette that passes"
                    .to_owned(),
            ),
        ));
    }
    Ok(ExitCode::Ok)
}

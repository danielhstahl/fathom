//! `fathom` with no verb: the terminal shell (ADR-006 §1, §6.7; ADR-002 §4.3, §4.5).
//!
//! This module is the seam between the binary and `fathom-tui`, and it owns exactly three things,
//! because those are the three things the shell is not allowed to read for itself:
//!
//! * the **keymap overrides** — `keys.overrides` is configuration, and ADR-002 §4.5 says
//!   configuration is read here and nowhere below the CLI;
//! * the **repository label** the tab bar prints, which needs git, and git is `fathom-ingest`'s;
//! * the **terminal contract** the user asked for: alternate screen, `--no-alt-screen`, or `--plain`
//!   for a pipe (ADR-006 §6.7: no terminal is a different contract, not a degraded one).
//!
//! Everything else — the loop, the focus model, the layout ladder, the frame — is the shell's, and
//! this file deliberately contains none of it.

use std::collections::BTreeMap;
use std::io::IsTerminal as _;

use eyre::Result;
use fathom_core::exit::ExitCode;
use fathom_core::time::Clock as _;
use fathom_tui::frame::{Options, render_plain, run};
use fathom_tui::keymap::{Keymap, Override, OverrideTables, overrides_from};

use crate::output;
use crate::verbs::Globals;

/// Run the shell, or print its frame as text.
///
/// # Errors
///
/// Whatever the shell raised, wrapped: a screen that could not be taken is an error exit, not a
/// panic, and the terminal is already back the way it was by the time this returns.
pub fn dispatch(globals: &Globals) -> Result<ExitCode> {
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let (overrides, mut notices) = keymap_overrides(&root);
    // What the index holds, read once, before the first frame: the empty states need it (`reco-1nu`) and
    // the rail's first page is asked for in the same terms, so the two cannot disagree about the epoch.
    let index = index_facts(&root);
    // The session (`e1.11`): read before the shell starts, so the reviewer's cursor, filter and layout
    // are in the first frame rather than two keystrokes away, and written back only if the reviewer
    // changed something. `--fresh` still reads it — so that a fresh run that *does* change something can
    // write against what is really stored — it just does not apply what it read.
    let loaded = crate::session::load(&root, crate::clock::now_millis());
    for warning in &loaded.warnings {
        notices.push(warning.clone());
    }
    if globals.fresh {
        notices.push(
            "--fresh: the stored session was left unread, so this run starts from defaults \
             (`e1.11`)"
                .to_owned(),
        );
    }
    // A pipe is not a failure and not a degraded mode: it is the other contract (ADR-006 §6.7). The
    // shell would fail to take a screen it does not have, so the choice is made here and *said*.
    let plain = globals.plain || !std::io::stdout().is_terminal();
    if plain && !globals.plain {
        notices.push(
            "stdout is not a terminal, so the frame is drawn as text (ADR-006 §6.7)".to_owned(),
        );
    }
    // In plain mode stdout is the artefact, so the complaints go to stderr with everything else the
    // user is meant to act on (ADR-002 §7.5); in the shell they belong inside the frame, where they
    // stay visible after a notice would otherwise have timed out.
    let frame_notices = if plain {
        Vec::new()
    } else {
        std::mem::take(&mut notices)
    };
    let options = Options {
        alt_screen: !globals.no_alt_screen,
        overrides,
        repo_label: repo_label(&root),
        // No forced size: the terminal answers for itself. `--plain` at 80 columns is the smallest
        // promised layout (ADR-002 §3), which is the one worth piping.
        size: None,
        max_frames: None,
        notices: frame_notices,
        // The clock is read here, at the edge, and passed in (ADR-002 §4.4).
        now: crate::clock::SystemClock.now(),
        // So is the environment the copy and `$EDITOR` plans are written against: the shell may not
        // read variables or walk `PATH` for itself (ADR-002 §3), so this crate does it instead.
        handoff_env: crate::handoff::environment(),
        // The timeline's rows come from a batch, never from a store handle the shell would hold
        // (ADR-002 §5.2). `reco-wzt` is the read: the composition root pulls the live episodes out of the
        // spine — identity, authorship, scale, paths, session/turn and review state are all rows the
        // index already writes — and hands the rail rows instead of an apology. `None` is still what an
        // unindexed repository gets, and the rail says which of the two it is looking at.
        episodes: crate::episodes::load(&root, &index),
        // ...and the shell is told what the index *does* hold, in the same breath, because it cannot go
        // and look (`reco-1nu`): an empty pane over a finished index must not send the reviewer to
        // `fathom index`, which is already done and changes nothing.
        index,
        session: if globals.fresh {
            Vec::new()
        } else {
            loaded.entries.clone()
        },
    };
    if plain {
        for notice in &notices {
            output::problem(format_args!("fathom: {notice}"));
        }
        // The same frame the terminal would get, one line at a time, no escape sequences. This is
        // what `e5.5`'s pipes and screen readers read, and it is drawn by the same renderer.
        for line in render_plain(&options) {
            output::line(format_args!("{line}"));
        }
        return Ok(ExitCode::Ok);
    }
    let report = run(&options).map_err(|err| {
        crate::verbs::pending(
            ExitCode::Error,
            format!("the terminal could not be taken: {err}"),
            Some(
                "`fathom --plain` draws the same frame as text, with no terminal taken".to_owned(),
            ),
        )
    })?;
    // The screen is back, so a printed complaint about the write cannot land under a raw-mode pane.
    // An untouched run writes nothing at all — no row, no `mtime` (`e1.11`).
    if report.session.changed {
        let saved =
            crate::session::save(&loaded, &report.session.entries, crate::clock::now_millis());
        crate::session::report_saved(&saved);
    }
    // The run's own numbers are not the answer anyone asked for, so they
    // stay in the report and out of the pipe; `fathom doctor` is where a slow frame gets reported.
    Ok(ExitCode::Ok)
}

/// `fathom tui --show-keys`: the merged table as text, for a pipe, CI, or a reader that wants the
/// whole keymap at once.
///
/// This is the same `Keymap` the shell resolves keystrokes against, including whatever
/// `keys.overrides` contributed and whatever it refused — the refusal is the part a hand-written
/// list would leave out.
pub fn show_keys() -> Result<ExitCode> {
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let (overrides, notices) = keymap_overrides(&root);
    let keymap = Keymap::merged(&overrides);
    for notice in notices.into_iter().chain(keymap.problems()) {
        output::problem(format_args!("fathom: {notice}"));
    }
    for line in keymap.cheatsheet() {
        output::line(format_args!("{line}"));
    }
    Ok(ExitCode::Ok)
}

/// `keys.overrides` from the repository's own `fathom.toml`, as the shell wants it.
///
/// A missing table is the normal case and says nothing; a table that names an unknown mode or an
/// unknown action is reported by [`fathom_tui::keymap::Keymap::problems`] once the keymap is
/// merged, and the shell shows those in the help overlay and on stderr.
fn keymap_overrides(root: &std::path::Path) -> (Vec<Override>, Vec<String>) {
    let text = match std::fs::read_to_string(root.join("fathom.toml")) {
        Ok(text) => text,
        // No file, no overrides: an unconfigured repository is the default keymap (ADR-006 §8.3).
        Err(_) => return (Vec::new(), Vec::new()),
    };
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(err) => {
            return (
                Vec::new(),
                vec![format!(
                    "fathom.toml does not parse, so the default keymap is in force: {err}"
                )],
            );
        }
    };
    let Some(section) = document
        .get("keys")
        .and_then(toml_edit::Item::as_table)
        .and_then(|keys| keys.get("overrides"))
        .and_then(toml_edit::Item::as_table)
    else {
        return (Vec::new(), Vec::new());
    };
    let mut tables: OverrideTables = OverrideTables::new();
    let mut notices = Vec::new();
    for (mode, binds) in section {
        let Some(binds) = binds.as_table() else {
            notices.push(format!(
                "keys.overrides.{mode} is not a table of chord = action"
            ));
            continue;
        };
        let mut entry = BTreeMap::new();
        for (chord, action) in binds {
            let Some(action) = action.as_str() else {
                notices.push(format!(
                    "keys.overrides.{mode}.{chord} must be a string (\"\" unbinds it)"
                ));
                continue;
            };
            entry.insert(chord.to_owned(), action.to_owned());
        }
        tables.insert(mode.to_owned(), entry);
    }
    let (overrides, problems) = overrides_from(&tables);
    notices.extend(problems);
    (overrides, notices)
}

/// The tab bar's right-hand label: `pilot · main`, or what is honestly known instead.
///
/// The branch is what `HEAD` names; a detached or unborn head says so rather than printing a sha
/// nobody asked for, and a directory that is not a repository says that too (ADR-001 §7.7: unknown
/// is a value).
fn repo_label(root: &std::path::Path) -> String {
    use fathom_ingest::repo::{Head, Repository};
    match Repository::discover(root) {
        Ok(repo) => {
            let name = repo.identity().display_name.clone();
            match repo.head() {
                Ok(Head::Branch { name: branch, .. }) | Ok(Head::Unborn { name: branch }) => {
                    format!("{name} · {branch}")
                }
                Ok(_) => format!("{name} · detached"),
                Err(err) => format!("{name} · head unknown ({err})"),
            }
        }
        Err(_) => root.file_name().map_or_else(
            || "no repository".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        ),
    }
}

/// What the store says about this repository, as the shell's empty states need it (`reco-1nu`).
///
/// The shell may not open a store (ADR-002 §5.2, lint `E01`), so the composition root looks once,
/// before the first frame, and hands the numbers over. This is the read `fathom stats` makes with the
/// checks left off: the shell is not diagnosing a store, it is deciding which sentence an empty pane
/// prints — and which one matters more than it looks, because *"run `fathom index`"* over an index
/// that already finished is a minute of an acceptance run spent walking into a dead end.
///
/// A number that could not be read stays `None` rather than becoming a zero: `unknown` is a value
/// (ADR-006 §0.1), and `0 commits` printed over a store that would not open is the same lie with a
/// digit on it.
#[cfg(feature = "sqlite-bundled")]
fn index_facts(root: &std::path::Path) -> fathom_tui::index::Facts {
    use fathom_store::episode::EpisodeQuery;
    use fathom_store::spine::Store;

    let width = |value: Result<i64, _>| value.ok().and_then(|value| u64::try_from(value).ok());
    let path = crate::session::store_path(root);
    if !path.is_file() {
        return fathom_tui::index::Facts::absent();
    }
    let Ok(store) = Store::open(&path) else {
        return fathom_tui::index::Facts::unreadable();
    };
    fathom_tui::index::Facts::present(
        width(store.epoch()),
        store.meta("epoch_status").ok().flatten(),
        store
            .scale()
            .ok()
            .and_then(|scale| u64::try_from(scale.commits).ok()),
        width(store.count_episodes(&EpisodeQuery::default())),
    )
}

/// A binary built without a SQLite backend cannot read any store, and says so instead of reporting
/// that the repository is empty (`fathom --version` names the feature that is missing).
#[cfg(not(feature = "sqlite-bundled"))]
fn index_facts(_root: &std::path::Path) -> fathom_tui::index::Facts {
    fathom_tui::index::Facts::no_backend()
}

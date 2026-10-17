//! The composition root's half of the session (`e1.11`).
//!
//! `fathom-tui` encodes the reviewer's working state and knows what every key means; it has no store
//! handle and no business opening one (ADR-002 §5.2, lint `E01`). This module is the four things that
//! *are* the binary's job:
//!
//! * **read** the rows before the shell starts, from the store at `.fathom/index.sqlite3` — in the
//!   store and not the working tree, because the state is class E (ADR-004 §10): a review session is
//!   worth resuming and not worth a `git status` entry, and `.fathom/` is disposable by contract;
//! * **heal** what cannot be read, by quarantining it and starting clean with a warning, per ADR-004
//!   §9.1 step 4 and §9.4 ("a torn write is dropped, defaults are used, a warning is printed");
//! * **write back** what the reviewer changed, one row at a time, through a compare-and-set, so a
//!   second fathom on the same repository cannot be overwritten by the first one leaving;
//! * **say what happened**, because a session that silently came back wrong is worse than one that
//!   never came back — `fathom tui --show-session` is the same rows in the reviewer's own words.
//!
//! # Why the write is a compare-and-set and not a save
//!
//! Two fathoms on one repository is not a mistake: one on the laptop's terminal, one inside the tmux
//! pane that also runs the build, or the `fathom watch` daemon reading the same store. A "write the
//! whole session on exit" save makes whichever quits last the winner, so a reviewer who opened a second
//! window to check one thing comes back to a cursor, a filter and a read set that are not theirs — and
//! nothing on screen says so.
//!
//! So each run remembers what it read (`Loaded`), and at exit writes only the rows that differ from
//! that, each one conditional on still holding the value it read ([`fathom_store::episode::Store::set_ui_state_if`]).
//! Two consequences, both of them the acceptance criterion:
//!
//! * a session that changed nothing writes nothing, which is also what makes `--fresh` free;
//! * two sessions that changed *different* rows both land, and two that changed the *same* row leave
//!   the loser told — "another fathom wrote `timeline.filter` 4 s ago; yours was not saved" — rather
//!   than quietly overwritten.
//!
//! # What a store we cannot read is, and is not
//!
//! No store at all is the ordinary case (a repository nobody has indexed): it is reported as that, with
//! `fathom index` as the remedy, and no state is written either, because minting an empty store from a
//! TUI would make `fathom doctor` report a repository with commits and an empty index as a degradation
//! it just caused.
//!
//! A store whose file is **not a SQLite database** — truncated to a few hundred bytes, full of zeros,
//! the first block of a tarball — is quarantined: moved under `.fathom/quarantine/` with its `-wal` and
//! `-shm` siblings and named in the warning. That is the whole of ADR-004 §9.1 step 4 applied to class
//! R and class E data, and the check is deliberately narrow (the file's own 16-byte magic, not a
//! judgment about contents): a store that opens but fails a deeper check is left exactly where it is
//! for `fathom doctor` to adjudicate, because "could not open, maybe locked" and "is not a database"
//! must not have the same consequence.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fathom_core::exit::ExitCode;
use serde_json::{Value, json};

use crate::output;
use crate::sanitize::Redactor;

/// The store this repository's session lives in.
///
/// Same function every other verb uses, so `fathom index`, `fathom doctor` and the shell cannot end up
/// with three ideas of where the state is.
#[must_use]
pub fn store_path(root: &Path) -> PathBuf {
    fathom_analyze::index::default_store_path(root)
}

/// Where damaged state is moved to: `.fathom/quarantine/` (ADR-004 §9.1 step 4), which is gitignored
/// like the rest of `.fathom/` because nothing in it is must-survive.
#[must_use]
pub fn quarantine_dir(store: &Path) -> PathBuf {
    store
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        .join("quarantine")
}

/// What the store said when the run started.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    /// The rows to hand the shell, damage already removed.
    pub entries: Vec<(String, String)>,
    /// One line per thing the reviewer has to be told before the frame appears.
    pub warnings: Vec<String>,
    /// The store that was read (or the one that should have been).
    pub path: PathBuf,
    /// Files moved aside: a whole unreadable store, or the quarantine record for damaged rows.
    pub quarantined: Vec<PathBuf>,
    /// Set when there is no store to read, which is also "do not try to write one".
    pub absent: bool,
    /// Set when the store exists and could not be opened at all; `absent` stays false, because the
    /// difference between "nothing to remember" and "could not read what to remember" is the whole
    /// point of printing a warning.
    pub broken: bool,
    /// `key → updated_utc` of the rows read, for `--show-session`.
    pub written: BTreeMap<String, i64>,
}

impl Loaded {
    /// The rows as a map, for the diff at exit.
    #[must_use]
    pub fn map(&self) -> BTreeMap<String, String> {
        self.entries.iter().cloned().collect()
    }
}

/// What a write-back did.
#[derive(Debug, Default)]
pub struct Saved {
    /// Rows written.
    pub wrote: Vec<String>,
    /// Rows deleted (a mark cleared, a read-set row that aged out).
    pub removed: Vec<String>,
    /// Rows *not* written because another session had changed them: `(key, what is stored instead)`.
    pub skipped: Vec<(String, String)>,
    /// Why nothing was written, when nothing was written for a reason other than "nothing changed".
    pub note: Option<String>,
}

/// Read this repository's session.
#[must_use]
pub fn load(root: &Path, now_ms: i64) -> Loaded {
    let path = store_path(root);
    if !path.exists() {
        return Loaded {
            path,
            absent: true,
            warnings: vec![
                "there is no index here yet, so this session will not be remembered — `fathom index` \
                 is what makes one"
                    .to_owned(),
            ],
            ..Loaded::default()
        };
    }
    #[cfg(feature = "sqlite-bundled")]
    {
        read_store(&path, now_ms)
    }
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = now_ms;
        Loaded {
            path,
            broken: true,
            warnings: vec![
                "this binary was built without a SQLite backend, so the session in \
                 .fathom/index.sqlite3 cannot be read or written — use the shipped binary"
                    .to_owned(),
            ],
            ..Loaded::default()
        }
    }
}

/// The read, the damage hunt and the quarantine, in that order: a damaged row is written out *before*
/// it is deleted, so a crash between the two leaves the value twice rather than gone.
#[cfg(feature = "sqlite-bundled")]
fn read_store(path: &Path, now_ms: i64) -> Loaded {
    use fathom_store::spine::Store;

    if !looks_like_sqlite(path) {
        return quarantined_store(path, now_ms);
    }
    let mut store = match Store::open(path) {
        Ok(store) => store,
        Err(err) => {
            // The store is where the state lives, so this is the one case where "state unreadable" and
            // "index unreadable" are the same sentence. It is still not this process's call to move the
            // file: `fathom doctor` is the verb that adjudicates a store that opens badly.
            return Loaded {
                path: path.to_path_buf(),
                broken: true,
                warnings: vec![format!(
                    "the session state in {} could not be read ({err}) — starting from defaults; \
                     `fathom doctor` says what is wrong with the store",
                    redact_path(path)
                )],
                ..Loaded::default()
            };
        }
    };
    let rows = match store.ui_state_rows("") {
        Ok(rows) => rows,
        Err(err) => {
            return Loaded {
                path: path.to_path_buf(),
                broken: true,
                warnings: vec![format!(
                    "the session state in {} could not be read ({err}) — starting from defaults",
                    redact_path(path)
                )],
                ..Loaded::default()
            };
        }
    };
    let ours: Vec<(String, String, i64)> = rows
        .into_iter()
        .filter(|(key, _, _)| fathom_tui::session::owns(key))
        .collect();
    let damaged = fathom_tui::session::damaged(
        &ours
            .iter()
            .map(|(key, value, _)| (key.clone(), value.clone()))
            .collect::<Vec<_>>(),
    );
    let mut quarantined = Vec::new();
    let mut warnings = Vec::new();
    if !damaged.is_empty() {
        let record = json!({
            "found_utc": now_ms,
            "store": redact_path(path),
            "why": "session_state rows this build could not parse — ADR-004 §9.4",
            "rows": damaged.iter().map(|(key, reason)| json!({
                "key": key,
                "value": reason_value(&ours, key),
                "reason": reason,
            })).collect::<Vec<Value>>(),
        });
        let file = quarantine_dir(path).join(format!("session-state-{now_ms}.json"));
        if let Err(err) = write_quarantine(&file, &record) {
            warnings.push(format!(
                "{} session row(s) could not be read and could not be quarantined either ({err}) — \
                 they are left in place and ignored",
                damaged.len()
            ));
        } else {
            for (key, reason) in &damaged {
                if let Err(err) = store.set_ui_state(key, None, now_ms) {
                    warnings.push(format!(
                        "`{key}` was quarantined but not deleted ({err}) — it will be ignored anyway"
                    ));
                }
                warnings.push(format!(
                    "stored session row `{key}` was damaged ({reason}) — moved to {} and dropped; \
                     the default is in force for it",
                    redact_path(&file)
                ));
            }
            quarantined.push(file);
        }
    }
    let broken_keys: BTreeSet<&str> = damaged.iter().map(|(key, _)| key.as_str()).collect();
    let mut written = BTreeMap::new();
    let mut entries = Vec::new();
    for (key, value, at) in &ours {
        if broken_keys.contains(key.as_str()) {
            continue;
        }
        written.insert(key.clone(), *at);
        entries.push((key.clone(), value.clone()));
    }
    Loaded {
        entries,
        warnings,
        path: path.to_path_buf(),
        quarantined,
        absent: false,
        broken: false,
        written,
    }
}

/// The whole-store case: the file is not a SQLite database, so nothing in it is reachable.
#[cfg(feature = "sqlite-bundled")]
fn quarantined_store(path: &Path, now_ms: i64) -> Loaded {
    let mut moved: Vec<PathBuf> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let dir = quarantine_dir(path);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return Loaded {
            path: path.to_path_buf(),
            broken: true,
            warnings: vec![format!(
                "{} is not a SQLite database and could not be moved aside ({err}) — starting from \
                 defaults",
                redact_path(path)
            )],
            ..Loaded::default()
        };
    }
    for sibling in [
        path.to_path_buf(),
        path.with_extension("sqlite3-wal"),
        path.with_extension("sqlite3-shm"),
    ] {
        if !sibling.exists() {
            continue;
        }
        let name = sibling.file_name().map_or_else(
            || "store".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let dest = dir.join(format!("{name}.{}", now_ms));
        match std::fs::rename(&sibling, &dest) {
            Ok(()) => moved.push(dest),
            Err(err) => failures.push(format!("{} ({err})", redact_path(&sibling))),
        }
    }
    let where_to = if moved.is_empty() {
        "nothing could be moved".to_owned()
    } else {
        format!(
            "{} moved aside",
            moved
                .iter()
                .map(|path| redact_path(path))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut warnings = vec![format!(
        "{} is not a SQLite database, so the session in it is unreadable — {where_to}; the app starts \
         from defaults and `fathom index` rebuilds the store (ADR-004 §9.1: the store is disposable, \
         nothing in `.fathom/` is must-survive)",
        redact_path(path)
    )];
    for failure in failures {
        warnings.push(format!(
            "a sibling of the store could not be moved aside: {failure}"
        ));
    }
    Loaded {
        path: path.to_path_buf(),
        broken: true,
        quarantined: moved,
        warnings,
        ..Loaded::default()
    }
}

/// Write back what the run changed, one compare-and-set per row.
///
/// `loaded` is what this run read at startup; `entries` is the whole snapshot as the frame came down.
/// A row whose stored value is neither what this run read nor absent is another session's answer and is
/// left alone, with the loser told which row and what won.
#[must_use]
pub fn save(loaded: &Loaded, entries: &[(String, String)], now_ms: i64) -> Saved {
    if loaded.absent || loaded.broken {
        return Saved {
            note: Some(
                "nothing was written: there was no readable store to write it to".to_owned(),
            ),
            ..Saved::default()
        };
    }
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (entries, now_ms);
        return Saved {
            note: Some("this binary has no SQLite backend".to_owned()),
            ..Saved::default()
        };
    }
    #[cfg(feature = "sqlite-bundled")]
    {
        use fathom_store::spine::Store;

        let mut store = match Store::open(&loaded.path) {
            Ok(store) => store,
            Err(err) => {
                return Saved {
                    note: Some(format!(
                        "nothing was written: {} could not be opened ({err})",
                        redact_path(&loaded.path)
                    )),
                    ..Saved::default()
                };
            }
        };
        let before = loaded.map();
        let after: BTreeMap<&str, &str> = entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let mut out = Saved::default();
        for (key, value) in &after {
            let (key, value) = (*key, *value);
            let expected = before.get(key).map(String::as_str);
            if expected == Some(value) {
                continue;
            }
            match store.set_ui_state_if(key, expected, Some(value), now_ms) {
                Ok(true) => out.wrote.push(key.to_owned()),
                Ok(false) => out.skipped.push((
                    key.to_owned(),
                    store
                        .ui_state(key)
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| "(gone)".to_owned()),
                )),
                Err(err) => out.skipped.push((key.to_owned(), err.to_string())),
            }
        }
        for (key, value) in &before {
            if after.contains_key(key.as_str()) {
                continue;
            }
            match store.set_ui_state_if(key, Some(value), None, now_ms) {
                Ok(true) => out.removed.push(key.clone()),
                Ok(false) => out.skipped.push((
                    key.clone(),
                    store
                        .ui_state(key)
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| "(gone)".to_owned()),
                )),
                Err(err) => out.skipped.push((key.clone(), err.to_string())),
            }
        }
        out
    }
}

/// `fathom tui --show-session`: the stored session, in the reviewer's own words.
///
/// Every key the shell writes has to have a reader, and for the ones a not-yet-landed view owns that
/// reader is here: the rows, when each was written, and what this build would and would not apply.
pub fn show(root: &Path, json: bool) -> eyre::Result<ExitCode> {
    let now_ms = crate::clock::now_millis();
    let loaded = load(root, now_ms);
    let redactor = Redactor::for_root(root);
    let mut probe = fathom_tui::app::App::new(
        fathom_tui::keymap::Keymap::merged(&[]),
        fathom_tui::app::Model::empty("no store was opened to draw rows"),
        fathom_tui::frame::HEADLESS_SIZE,
        redactor.path(root),
    );
    let applied = fathom_tui::session::restore(&mut probe, &loaded.entries);
    if json {
        let rows: Vec<Value> = loaded
            .entries
            .iter()
            .map(|(key, value)| {
                json!({
                    "key": key,
                    "value": redactor.text(value),
                    "updated_utc": loaded.written.get(key).copied(),
                })
            })
            .collect();
        output::line(format_args!(
            "{}",
            json!({
                "store": redactor.path(&loaded.path),
                "present": !loaded.absent && !loaded.broken,
                "rows": rows,
                "applied": applied.applied,
                "warnings": loaded
                    .warnings
                    .iter()
                    .chain(applied.warnings.iter())
                    .map(|line| redactor.text(line))
                    .collect::<Vec<_>>(),
                "quarantined": loaded
                    .quarantined
                    .iter()
                    .map(|path| redactor.path(path))
                    .collect::<Vec<_>>(),
            })
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "session state in {}",
        redactor.path(&loaded.path)
    ));
    for warning in loaded.warnings.iter().chain(applied.warnings.iter()) {
        output::problem(format_args!("fathom: {}", redactor.text(warning)));
    }
    if loaded.entries.is_empty() {
        output::line(format_args!("  (nothing stored)"));
    }
    for (key, value) in &loaded.entries {
        let at = loaded.written.get(key).map_or_else(String::new, |at| {
            format!("   ← written {}", written_at(*at))
        });
        output::line(format_args!("  {key:<22} = {}{at}", redactor.text(value)));
    }
    for line in &applied.applied {
        output::line(format_args!("  applied: {}", redactor.text(line)));
    }
    Ok(ExitCode::Ok)
}

/// Print what a write-back did, including the rows another session won.
pub fn report_saved(saved: &Saved) {
    let redactor = Redactor::for_root(Path::new("."));
    for (key, winner) in &saved.skipped {
        output::problem(format_args!(
            "fathom: your `{key}` was not saved — another session wrote it (it holds `{}` now). \
             `fathom tui --show-session` shows what is stored",
            redactor.text(winner)
        ));
    }
    if let Some(note) = &saved.note {
        output::problem(format_args!("fathom: {}", redactor.text(note)));
    }
}

/// The value a damaged row held, for the quarantine record.
#[cfg(feature = "sqlite-bundled")]
fn reason_value(rows: &[(String, String, i64)], key: &str) -> String {
    rows.iter()
        .find(|(have, _, _)| have == key)
        .map_or_else(String::new, |(_, value, _)| value.clone())
}

/// Write the quarantine record: create the directory, then the file.
#[cfg(feature = "sqlite-bundled")]
fn write_quarantine(file: &Path, record: &Value) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(record)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?;
    std::fs::write(file, text)
}

/// A `session_state` stamp in the reviewer's clock. The store keeps milliseconds and `Timestamp` is
/// microseconds (ADR-004 §4.1), and neither is what a person reads.
fn written_at(millis: i64) -> String {
    let stamp = fathom_core::time::Timestamp::from_unix_micros(millis.saturating_mul(1_000));
    let (year, month, day) = stamp.civil();
    let (hour, minute, _) = stamp.time_of_day();
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

/// Does this file begin with the bytes every SQLite database begins with?
///
/// Deliberately the *cheapest* possible test, because it decides whether a file gets moved out of the
/// way: a truncated file, a zero-filled one, and a file that is not a database at all all fail it, and
/// nothing that *is* a database does. A store that begins correctly and fails deeper is left for
/// `fathom doctor`, which is the verb with the remedies.
#[cfg(feature = "sqlite-bundled")]
fn looks_like_sqlite(path: &Path) -> bool {
    const MAGIC: &[u8; 16] = b"SQLite format 3\0";
    let Ok(bytes) = std::fs::read(path) else {
        // Unreadable is not the same as not-a-database: a permission error must not cost the file.
        return true;
    };
    if bytes.is_empty() {
        // SQLite treats an empty file as a database to be created, so there is nothing to heal.
        return true;
    }
    bytes.starts_with(MAGIC)
}

/// A path, made safe to print (the same redactor every other verb prints paths through).
fn redact_path(path: &Path) -> String {
    Redactor::for_root(Path::new(".")).path(path)
}

#[cfg(all(test, feature = "sqlite-bundled"))]
mod tests {
    //! The writer's four promises, against a real SQLite file: read, heal, merge, and stay out of the
    //! way when nothing changed. Every one of them is about two processes, so every one of them opens
    //! the store twice.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use fathom_store::spine::Store;

    /// A scratch repository, unique per test, with `.fathom/` in it and nothing else.
    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fathom-session-{name}-{}-{}",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("main")
                .trim_start_matches("test ")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".fathom")).expect("create .fathom");
        dir
    }

    /// A store with the schema in it, ready for session rows.
    fn indexed(name: &str) -> PathBuf {
        let root = repo(name);
        let store = Store::open(&store_path(&root)).expect("open a fresh store");
        drop(store);
        root
    }

    fn rows(root: &Path) -> Vec<(String, String)> {
        Store::open(&store_path(root))
            .expect("open the store")
            .ui_state_all("")
            .expect("read session_state")
    }

    fn write_rows(root: &Path, pairs: &[(&str, &str)]) {
        let mut store = Store::open(&store_path(root)).expect("open the store");
        for (key, value) in pairs {
            store
                .set_ui_state(key, Some(*value), 1_700_000_000_000)
                .expect("write a row");
        }
    }

    /// What a reviewer's state looks like when they leave: a filter, a cursor, a read set.
    fn reviewer_state(filter: &str, episode: &str) -> Vec<(String, String)> {
        vec![
            ("tui.state_version".to_owned(), "1".to_owned()),
            ("tui.view".to_owned(), "timeline".to_owned()),
            ("tui.filter".to_owned(), filter.to_owned()),
            ("timeline.filter".to_owned(), filter.to_owned()),
            ("timeline.selection".to_owned(), episode.to_owned()),
            (format!("unread:{episode}"), "read".to_owned()),
        ]
    }

    #[test]
    fn two_sessions_on_one_repository_do_not_clobber_each_other() {
        // The acceptance criterion, stated as the two cases it has to hold for.
        let root = indexed("two-sessions");
        write_rows(
            &root,
            &[("timeline.filter", "group=day"), ("tui.view", "queue")],
        );
        // Two fathoms open the same repository: both read the same rows.
        let a = load(&root, 1);
        let b = load(&root, 1);
        assert_eq!(a.entries, b.entries, "both started from the same state");

        // A narrows the filter and leaves first. A run's snapshot is always the whole state, so the
        // row A has no opinion about is still in it, unchanged.
        let a_saved = save(
            &a,
            &[
                (
                    "timeline.filter".to_owned(),
                    "author=pi;group=day".to_owned(),
                ),
                ("tui.view".to_owned(), "queue".to_owned()),
            ],
            2,
        );
        assert_eq!(a_saved.wrote, vec!["timeline.filter".to_owned()]);
        assert!(a_saved.skipped.is_empty(), "nothing moved under A");

        // B, which never saw A's change, goes away having only switched views: A's filter survives.
        let b_saved = save(
            &b,
            &[
                ("timeline.filter".to_owned(), "group=day".to_owned()),
                ("tui.view".to_owned(), "diff".to_owned()),
            ],
            3,
        );
        assert!(
            b_saved.wrote == vec!["tui.view".to_owned()],
            "B writes only what it changed: {:?}",
            b_saved.wrote
        );
        let stored = rows(&root);
        assert_eq!(
            stored
                .iter()
                .find(|(key, _)| key == "timeline.filter")
                .map(|(_, value)| value.clone()),
            Some("author=pi;group=day".to_owned()),
            "A's filter is what is stored — B did not roll back a change it never saw"
        );

        // Now the case a last-writer-wins save gets wrong: both change the *same* row.
        let c = load(&root, 4);
        let d = load(&root, 4);
        let c_saved = save(
            &c,
            &[
                ("timeline.filter".to_owned(), "area=src/store".to_owned()),
                ("tui.view".to_owned(), "diff".to_owned()),
            ],
            5,
        );
        assert_eq!(
            c_saved.wrote,
            vec!["timeline.filter".to_owned()],
            "C read the row it changed and lands it: {:?}",
            c_saved
        );
        let d_saved = save(
            &d,
            &[
                ("timeline.filter".to_owned(), "state=unread".to_owned()),
                ("tui.view".to_owned(), "diff".to_owned()),
            ],
            6,
        );
        assert_eq!(
            d_saved
                .skipped
                .iter()
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>(),
            vec!["timeline.filter".to_owned()],
            "D expected the value it read, and C moved it"
        );
        assert_eq!(
            d_saved.skipped[0].1, "area=src/store",
            "and D is told what won, in the same breath as what it lost"
        );
        assert_eq!(
            rows(&root)
                .iter()
                .find(|(key, _)| key == "timeline.filter")
                .map(|(_, value)| value.clone()),
            Some("area=src/store".to_owned()),
            "C's answer stands; D's is on D's screen and nowhere else"
        );
    }

    #[test]
    fn a_run_that_changed_nothing_writes_nothing_at_all() {
        // The `--fresh` criterion, and the precondition for the merge above: an unchanged snapshot asks
        // the store for nothing — no row, no write, no `mtime` bump to argue about.
        let root = indexed("unchanged-run");
        write_rows(
            &root,
            &[("tui.view", "timeline"), ("timeline.filter", "group=day")],
        );
        let before = rows(&root);
        let loaded = load(&root, 1);
        let saved = save(&loaded, &loaded.entries.clone(), 2);
        assert!(saved.wrote.is_empty() && saved.removed.is_empty() && saved.skipped.is_empty());
        assert_eq!(
            rows(&root),
            before,
            "the store is byte-for-byte the state it was"
        );
    }

    #[test]
    fn a_row_a_reviewer_cleared_is_deleted_and_one_they_never_saw_is_left() {
        let root = indexed("cleared-row");
        write_rows(&root, &[("tui.mark", "e8f1a2"), ("queue.filter", "unread")]);
        let loaded = load(&root, 1);
        assert!(
            !loaded.entries.iter().any(|(key, _)| key == "queue.filter"),
            "another subsystem's row is not ours to read, write or delete"
        );
        let saved = save(&loaded, &[], 2);
        assert_eq!(
            saved.removed,
            vec!["tui.mark".to_owned()],
            "the mark was cleared, so the row goes"
        );
        assert!(
            rows(&root).iter().any(|(key, _)| key == "queue.filter"),
            "and the row we do not own is exactly where it was"
        );
    }

    #[test]
    fn a_truncated_store_is_quarantined_and_the_run_starts_clean() {
        // The "state file is corrupt" acceptance bullet, at the granularity it can actually be corrupt
        // at when the state lives in a database: the file stops being a database.
        let root = indexed("truncated");
        write_rows(
            &root,
            &[("tui.view", "timeline"), ("timeline.filter", "author=pi")],
        );
        // A torn write: the first block is no longer a SQLite header.
        std::fs::write(store_path(&root), b"the first thousand bytes of a tarball")
            .expect("garble");

        let loaded = load(&root, 1_700_000_001_000);
        assert!(loaded.broken, "the run knows it could not read anything");
        assert!(loaded.entries.is_empty(), "and applies nothing");
        assert!(
            !store_path(&root).exists(),
            "the unreadable store is not left where the next run will trip over it again"
        );
        let quarantined = std::fs::read_dir(quarantine_dir(&store_path(&root)))
            .expect("the quarantine directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        assert_eq!(
            quarantined.len(),
            1,
            "one file moved aside: {quarantined:?}"
        );
        assert!(
            quarantined[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("index.sqlite3"),
            "under its own name, so a person can put it back: {:?}",
            quarantined[0]
        );
        assert!(
            loaded.warnings.iter().any(|line| line.contains("not a SQLite database")
                && line.contains("fathom index")),
            "the warning says what happened and what to type: {:?}",
            loaded.warnings
        );

        // And the write side does not try to resurrect it.
        let saved = save(&loaded, &[("tui.view".to_owned(), "diff".to_owned())], 2);
        assert!(saved.wrote.is_empty());
        assert!(saved.note.is_some(), "and says why: {:?}", saved.note);
    }

    #[test]
    fn a_damaged_row_is_quarantined_and_the_good_rows_still_come_back() {
        // The other half of self-healing: one bad row in a store that is otherwise fine. The bad row is
        // moved out and deleted, the good ones are handed over, and the run is told which was which.
        let root = indexed("damaged-row");
        write_rows(
            &root,
            &[
                ("tui.view", "timeline"),
                ("tui.size", "banana"),
                ("timeline.filter", "author=pi;group=day"),
                ("unread:e8f1a2", "read"),
            ],
        );
        let loaded = load(&root, 1_700_000_002_000);
        assert!(!loaded.broken, "the store itself is fine");
        assert!(
            loaded
                .warnings
                .iter()
                .any(|line| line.contains("tui.size") && line.contains("quarantined")),
            "the bad row is named: {:?}",
            loaded.warnings
        );
        let keys: Vec<&str> = loaded.entries.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["timeline.filter", "tui.view", "unread:e8f1a2"],
            "everything else came back, and only what was whole"
        );
        assert_eq!(
            Store::open(&store_path(&root))
                .expect("open")
                .ui_state("tui.size")
                .expect("read"),
            None,
            "the damaged row is gone from the table, not merely ignored"
        );
        let record = &loaded.quarantined[0];
        let text = std::fs::read_to_string(record).expect("read the quarantine record");
        assert!(
            text.contains("\"tui.size\"") && text.contains("banana") && text.contains("COLSxROWS"),
            "the record says which key, what it held and why it is wrong: {text}"
        );

        // A second run does not re-quarantine what is already gone.
        let again = load(&root, 1_700_000_003_000);
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
        assert_eq!(again.entries, loaded.entries);
    }

    #[test]
    fn no_store_is_reported_as_no_store_and_nothing_is_written() {
        // An unindexed repository is the ordinary case. Minting a store from the shell would make
        // `fathom doctor` report a degradation the shell just caused, so it does not.
        let root = repo("no-store");
        let loaded = load(&root, 1);
        assert!(loaded.absent && !loaded.broken);
        assert!(
            loaded
                .warnings
                .iter()
                .any(|line| line.contains("fathom index")),
            "{:?}",
            loaded.warnings
        );
        assert!(
            !store_path(&root).exists(),
            "no store was created to say so in"
        );
        let saved = save(&loaded, &reviewer_state("group=day", "e8f1a2"), 2);
        assert!(saved.wrote.is_empty() && saved.note.is_some());
    }

    #[test]
    fn what_one_run_writes_the_next_run_reads_back() {
        // The end of the loop, with a real snapshot from `fathom-tui` rather than hand-written rows:
        // what the shell says to store is what the next shell is handed, and what it is handed is what it
        // applies.
        let root = indexed("round-trip");
        let mut probe = fathom_tui::app::App::new(
            fathom_tui::keymap::Keymap::defaults(),
            fathom_tui::app::Model::empty("no rows in this test"),
            (100, 30),
            "fathom/pilot · main",
        );
        probe.last_search = Some("panic=use_after_free".to_owned());
        let snapshot = fathom_tui::session::snapshot(&probe);
        let loaded = load(&root, 1);
        let saved = save(&loaded, &snapshot, 1_700_000_004_000);
        assert!(
            saved.wrote.len() == snapshot.len(),
            "every row landed: {:?}",
            saved.wrote
        );

        let back = load(&root, 1_700_000_005_000);
        assert_eq!(
            back.entries.len(),
            snapshot.len(),
            "and every row came back"
        );
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        let mut fresh = fathom_tui::app::App::new(
            fathom_tui::keymap::Keymap::defaults(),
            fathom_tui::app::Model::empty("no rows in this test"),
            (80, 24),
            "fathom/pilot · main",
        );
        let applied = fathom_tui::session::restore(&mut fresh, &back.entries);
        assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);
        assert_eq!(
            fresh.last_search.as_deref(),
            Some("panic=use_after_free"),
            "the search the reviewer typed is the search they come back to"
        );
        assert_eq!(fresh.size, (100, 30), "at the frame size they left it at");

        // And because the snapshot is stable, a run that reads it and changes nothing writes nothing.
        let again = save(&back, &fathom_tui::session::snapshot(&fresh), 9);
        assert!(
            again.wrote.is_empty() && again.removed.is_empty(),
            "the state the reviewer resumed is the state they leave: {:?}",
            (again.wrote, again.removed)
        );
    }

    #[test]
    fn the_quarantine_directory_lives_in_the_store_not_the_working_tree() {
        // ADR-004 §9.1 step 4 names the directory; this pins that nothing lands where a reviewer would
        // find it in `git status`.
        let root = indexed("quarantine-place");
        assert_eq!(
            quarantine_dir(&store_path(&root)),
            root.join(".fathom").join("quarantine")
        );
    }
}

//! A bad keymap line names its file and its line (`e5.4`, `reco-mdb` criterion 2).
//!
//! The criterion is *"conflicting keybindings are reported at startup with the file and line to
//! fix"*, and the second half is the whole difficulty: `ADR-006 §8.3` already made Fathom refuse a
//! bad `keys.overrides` entry and say why, but "keys.overrides.timeline.p shadows the globally
//! reserved key 'p'" is a sentence about a file the reviewer has to find with `grep`. So the CLI —
//! the only crate allowed to read configuration (`ADR-002 §4.5`) — records where each line started,
//! which `toml_edit` already knows, and the refusal ends in `fathom.toml:5`.
//!
//! Both halves are checked from outside the process, because the criterion is about what a reviewer
//! reads: `--show-keys` is the pipe-able report, and a plain frame is what the shell prints at
//! startup before it draws anything.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Every test in this file drives `fathom tui`, which ADR-002 §6.1 compiles only with the `tui` feature, so
// the headless `minimal` binary answers `FEATURE_ABSENT` (9) and there is no frame, no notice and no exit
// code left for these assertions to read. Gated in the spelling `tests/check_verb.rs` and
// `tests/show_trailers.rs` use; on every build with the shell the whole suite still runs.
#![cfg(feature = "tui")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fathom(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("the built fathom binary is runnable")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("fathom exits with a code")
}

/// A repository whose `fathom.toml` has one of each of the four ways a keymap line goes wrong, on
/// lines this test can name.
fn repository(name: &str, config: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-keymap-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    std::fs::write(dir.join("fathom.toml"), config).expect("write fathom.toml");
    dir
}

#[test]
fn every_refused_override_names_the_line_to_fix() {
    let dir = repository(
        "refusals",
        r#"[keys.overrides.global]
x = "no.such.action"

[keys.overrides.timeline]
p = "view.queue"

[keys.overrides.notamode]
j = "review.accept"
"#,
    );
    let out = fathom(&dir, &["tui", "--show-keys"]);
    let problems = stderr(&out);
    // The table is still printed: a refused line is a refusal, not a crash, and the rest of the
    // keymap is still what the shell will honour.
    assert_eq!(code(&out), 0, "{problems}");
    assert!(
        stdout(&out).contains("palette.open"),
        "the cheatsheet still prints"
    );

    for (needle, line) in [
        // Line 2: an action that is not in the registry.
        ("unknown action 'no.such.action'", 2),
        // Line 5: a view mode trying to take a globally reserved key (K4).
        ("shadows the globally reserved key 'p'", 5),
        // Line 7: a mode that does not exist, named on the table header it was written on.
        ("unknown mode", 7),
    ] {
        let wanted = format!("fathom.toml:{line}");
        assert!(
            problems.contains(needle) && problems.contains(&wanted),
            "expected {needle:?} to be reported with {wanted}, got:\n{problems}"
        );
        // The dotted path the reviewer wrote is in the same line, so the sentence is not a riddle.
        assert!(
            problems.contains("keys.overrides."),
            "the report should name the config path: {problems}"
        );
    }
    let _ = dir;
}

#[test]
fn a_clean_keymap_says_nothing_and_points_at_nothing() {
    // The absence matters as much as the presence: a report that names a line when nothing is wrong
    // trains a reviewer to ignore the report.
    let dir = repository(
        "clean",
        r#"[keys.overrides.global]
y = "review.accept"
"#,
    );
    let out = fathom(&dir, &["tui", "--show-keys"]);
    assert_eq!(code(&out), 0);
    assert_eq!(stderr(&out).trim(), "", "a legal override is not a problem");
    let keys = stdout(&out);
    assert!(
        keys.contains('y'),
        "the merged table prints the reviewer's key: {keys}"
    );
}

#[test]
fn the_startup_notice_carries_the_same_line_the_report_does() {
    // `fathom tui` (here `--plain`, which is the same path with no terminal) pushes the keymap's
    // problems into its notices at startup; the frame is where a reviewer actually sees them.
    let dir = repository(
        "startup",
        r#"[keys.overrides]
[keys.overrides.global]
x = "no.such.action"
"#,
    );
    let out = fathom(&dir, &["tui", "--plain", "--lines"]);
    let frame = stdout(&out) + &stderr(&out);
    assert!(
        frame.contains("no.such.action") && frame.contains("fathom.toml:3"),
        "the frame should say what was refused and where: {frame}"
    );
}

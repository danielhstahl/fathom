//! The text contract, from the outside (`e5.5`, ADR-006 §6.7).
//!
//! `--plain` is the second contract of this shell, and its acceptance sentence is on the ticket: *"a
//! `--plain` line mode for pipes, CI logs and screen readers"*. Three words in that sentence are
//! testable from outside the process and one is not:
//!
//! * **no escape sequences** — a CI log that grows a `\x1b[?1049h` is a broken CI log, and this is the
//!   one check a screen reader's user cannot make for you;
//! * **line mode** — the grid draws its panes side by side, which in text means reading three
//!   interleaved columns, and *nobody* reads that. `--lines` is the promise that the same frame is
//!   printed one pane after another, in reading order, with every fact still on the page;
//! * **pipes and a missing terminal** — a redirected stdout is not an error and not a silent fallback:
//!   the run says which contract it is in and why.
//!
//! The untestable word is "useful", and the nearest honest proxy is that every visible character of
//! the grid's frame turns up in the line mode: a text render that drops a pane is not less useful, it
//! is a different and smaller frame, and the ticket did not ask for one.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Every test in this file drives `fathom tui`, which ADR-002 §6.1 compiles only with the `tui` feature, so
// the headless `minimal` binary answers `FEATURE_ABSENT` (9) and there is no frame, no notice and no exit
// code left for these assertions to read. Gated in the spelling `tests/check_verb.rs` and
// `tests/show_trailers.rs` use; on every build with the shell the whole suite still runs.
#![cfg(feature = "tui")]

use std::path::Path;
use std::process::{Command, Output};

/// Run the binary with a *stated* environment.
///
/// The harness's own terminal is not the terminal under test: a developer running `cargo test` from
/// inside tmux, with `NO_COLOR` set because they are colour-blind, would otherwise get a tmux notice
/// in a test about `TERM=dumb`. So every variable that changes the probe's answer is either set by the
/// caller or removed here, and nothing leaks in from outside.
fn fathom_in(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fathom"));
    command
        .current_dir(dir)
        .args(args)
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8");
    for key in ["TMUX", "STY", "SSH_TTY", "SSH_CONNECTION", "NO_COLOR"] {
        command.env_remove(key);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    command
        .output()
        .expect("the built fathom binary is runnable")
}

fn fathom_in_dir(dir: &Path, args: &[&str]) -> Output {
    fathom_in(dir, args, &[])
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

/// A directory with nothing in it: an unindexed repository is the smallest honest input, and the
/// frame still has to be a frame.
fn bare_dir(tag: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("fathom-text-contract-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

#[test]
fn the_plain_render_writes_no_escape_sequences_to_stdout() {
    let dir = bare_dir("escape");
    let out = fathom_in_dir(&dir, &["tui", "--plain", "--fresh"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        !text.contains('\u{1b}'),
        "ADR-006 §6.7: the text render writes no escape sequences at all:\n{text:?}"
    );
    assert!(
        text.contains("worklist") || text.contains("Queue"),
        "it is still the frame, not an apology:\n{text}"
    );
    // ...and the two sequences it would have written went nowhere near stdout.
    let err = stderr(&out);
    assert!(
        !err.contains("\x1b[?1049h"),
        "the alt-screen sequence belongs to neither stream: {err}"
    );
}

#[test]
fn the_line_mode_prints_the_same_frame_in_reading_order() {
    let dir = bare_dir("lines");
    let grid = stdout(&fathom_in_dir(&dir, &["tui", "--plain", "--fresh"]));
    let flow = stdout(&fathom_in_dir(
        &dir,
        &["tui", "--plain", "--lines", "--fresh"],
    ));
    assert!(
        !flow.contains('\u{1b}'),
        "the line mode writes no escape sequences either: {flow:?}"
    );
    // The grid, in text, is three panes of interleaved columns; the line mode must not be. If this
    // ever fails, `--lines` has become `--plain` and the flag is doing nothing.
    assert!(
        grid.contains('│'),
        "the grid render should still have its column rules at this width, or this test is checking \
         an empty comparison:\n{grid}"
    );
    assert!(
        !flow.contains('│') && !flow.contains('┌'),
        "the line mode is reading order, so it has no columns to separate:\n{flow}"
    );

    // The parity claim, stated the way it can be checked: every row of every pane in the grid turns up
    // in the line mode. It is allowed to turn up *longer* — a grid row is clipped to its column and the
    // line mode has no columns, which is the whole reason `--lines` exists — so a clipped row is
    // compared as the prefix it is.
    let flowed: Vec<String> = flow
        .lines()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect();
    let mut missing: Vec<String> = Vec::new();
    for row in grid_rows(&grid) {
        let want = row.trim_end_matches('…').trim();
        if want.is_empty() {
            continue;
        }
        if !flowed.iter().any(|have| have.starts_with(want)) {
            missing.push(row);
        }
    }
    assert!(
        missing.is_empty(),
        "the line mode lost rows the grid drew:\n{}\n--- line mode ---\n{flow}",
        missing.join("\n")
    );

    // The chrome is there too, in the form the line mode gives it: the frame's own facts, labelled.
    assert!(
        flow.contains("complete") || flow.contains("degraded") || flow.contains("single"),
        "the line mode says which layout state this frame is in:\n{flow}"
    );
    for fact in ["epoch", "queue"] {
        assert!(
            flow.contains(fact),
            "the status line's facts survive into the line mode ({fact:?}):\n{flow}"
        );
    }

    // And the order is reading order: the panes come one after another, in the order the frame rings
    // them, each named by its own header.
    let worklist = flow
        .find("worklist")
        .expect("the line mode names the worklist pane");
    let card = flow
        .find("card")
        .expect("the line mode names the card pane");
    assert!(
        worklist < card,
        "the primary pane is read before the secondary one:\n{flow}"
    );
    assert!(
        flow.contains("pane 1 of 2") && flow.contains("pane 2 of 2"),
        "each pane says which one it is, because in text there is no box to point at:\n{flow}"
    );
}

/// The grid's *pane* rows, with the column rules taken out: what a reader of the text render would
/// otherwise have to read three columns at once.
///
/// Deliberately not every row of the grid. The tab bar, the footer strips and the hint line are chrome,
/// and the line mode re-renders chrome as labelled lines (`fathom · Queue · complete · 80×24`,
/// `hint · …`) rather than as the clipped grid row — which is the point of the mode, not a deviation
/// from it. Pane content is what must survive unchanged.
fn grid_rows(grid: &str) -> Vec<String> {
    let lines: Vec<&str> = grid.lines().collect();
    let last_body = lines
        .iter()
        .rposition(|line| line.starts_with('└'))
        .unwrap_or(lines.len())
        .saturating_sub(2);
    let mut out = Vec::new();
    for line in lines.iter().take(last_body + 1) {
        if !line.starts_with('│') {
            continue;
        }
        for segment in line.split(['│', '┬']) {
            let text = segment.trim();
            if !text.is_empty() {
                out.push(text.to_owned());
            }
        }
    }
    out
}

#[test]
fn a_redirected_stdout_says_which_contract_it_is_in() {
    // The test harness *is* a pipe, which is the whole point: this is exactly the case a user meets
    // the first time they write `fathom tui > note.txt`.
    let dir = bare_dir("redirect");
    let out = fathom_in_dir(&dir, &["tui", "--fresh"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("not a terminal"),
        "the fallback has to be said out loud:\n{}",
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("worklist") || stdout(&out).contains("Queue"),
        "and it still draws the frame:\n{}",
        stdout(&out)
    );
}

#[test]
fn a_flag_that_cannot_apply_here_says_so() {
    let dir = bare_dir("flags");
    // `--lines` on a run that is drawing a grid does nothing, and a silently-ignored flag is what
    // ADR-001 §7 forbids. In a pipe the grid is never drawn, so the notice is *not* printed — and
    // that asymmetry is the thing worth pinning.
    let piped = fathom_in_dir(&dir, &["tui", "--lines", "--fresh"]);
    assert_eq!(code(&piped), 0, "{}", stderr(&piped));
    assert!(
        !stderr(&piped).contains("--lines orders"),
        "in a pipe `--lines` does apply, so saying it does not would be the lie:\n{}",
        stderr(&piped)
    );

    // `--mouse` cannot do anything without a terminal to take the pointer from.
    let mouse = fathom_in_dir(&dir, &["tui", "--mouse", "--plain", "--fresh"]);
    assert_eq!(code(&mouse), 0, "{}", stderr(&mouse));
    assert!(
        stderr(&mouse).contains("--mouse"),
        "`--mouse` on a text run has to say the pointer is off:\n{}",
        stderr(&mouse)
    );
    assert!(
        stderr(&mouse).contains("key"),
        "and say what to do instead:\n{}",
        stderr(&mouse)
    );
}

#[test]
fn a_degraded_terminal_is_reported_through_the_binary() {
    // The probe is `fathom-tui`'s, but the *notices* have to reach the reviewer, which is this
    // crate's job. Every case here is a real session somebody is in right now.
    let dir = bare_dir("term");
    for (env, want) in [
        (vec![("TERM", "dumb")], "TERM=dumb"),
        (vec![("TERM", "vt100")], "vt100"),
        (vec![("LANG", "C"), ("LC_ALL", "C")], "locale"),
        (vec![("NO_COLOR", "1")], "NO_COLOR"),
        (
            vec![("TERM", "screen"), ("TMUX", "/tmp/tmux-1000/default,1,0")],
            "tmux",
        ),
        (
            vec![("TERM", "xterm-256color"), ("SSH_TTY", "/dev/pts/3")],
            "ssh",
        ),
    ] {
        let out = fathom_in(&dir, &["tui", "--plain", "--fresh"], &env);
        assert_eq!(code(&out), 0, "{env:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(want),
            "{env:?} should have been reported as {want:?}:\n{}",
            stderr(&out)
        );
    }
}

#[test]
fn a_healthy_terminal_is_not_apologised_for() {
    // The other half of "reported, not silent": a run with nothing to complain about must not print a
    // wall of caveats, or the one real notice will be read as noise.
    let dir = bare_dir("healthy");
    let out = fathom_in_dir(&dir, &["tui", "--plain", "--fresh"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let err = stderr(&out);
    for unwanted in ["TERM=", "NO_COLOR", "locale", "tmux"] {
        assert!(
            !err.contains(unwanted),
            "nothing was wrong, and the run said {unwanted:?} anyway:\n{err}"
        );
    }
}

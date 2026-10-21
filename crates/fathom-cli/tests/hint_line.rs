//! The hint line, from the outside (`e5.4`, `reco-mdb`).
//!
//! The criterion is two halves and they pull opposite ways: the hint line shows the next likely
//! actions, *and* it "hides itself once learned" — which in a program means a reviewer can turn it off
//! and the program stays off. A TUI test can say the frame option exists; only running the binary says
//! whether the setting in a repository's own `fathom.toml` reaches the frame.
//!
//! `--plain --lines` is the door: the hint line is a row of the text frame (ADR-006 §6.7), so both
//! halves — printed, and not printed — are observable without a terminal, and so is the third case, a
//! setting that is neither yes nor no. A hint line that could only be turned off by a value that parses
//! is a hint line that stays on for anyone who guesses wrong, which is why the refusal is a notice and
//! the line stays up.
#![allow(clippy::unwrap_used, clippy::expect_used)]

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

/// A directory with nothing in it but a `fathom.toml`, so the frame is the plainest one on offer.
fn repository(name: &str, config: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-hints-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    if !config.is_empty() {
        std::fs::write(dir.join("fathom.toml"), config).expect("write the fixture config");
    }
    dir
}

/// The hint line, if the frame printed one.
fn hint_of(output: &Output) -> Option<String> {
    stdout(output)
        .lines()
        .find(|line| line.starts_with("hint ·"))
        .map(str::to_owned)
}

#[test]
fn the_hint_line_is_there_until_the_reviewer_turns_it_off() {
    let on = repository("on", "");
    let before = fathom(&on, &["tui", "--plain", "--lines"]);
    let hint = hint_of(&before).unwrap_or_else(|| {
        panic!(
            "a shell that has not been told otherwise shows the next likely actions:\n{}",
            stdout(&before)
        )
    });
    // The line is the palette's own vocabulary, not a fixed string: the actions a reviewer is most
    // likely to press next, with the key each one currently has.
    for wanted in ["Queue", "Search", "Help"] {
        assert!(
            hint.contains(wanted),
            "`{wanted}` is one of the next likely actions: {hint}"
        );
    }

    let off = repository("off", "[ui]\nhints = false\n");
    let after = fathom(&off, &["tui", "--plain", "--lines"]);
    let hidden_frame = stdout(&after);
    assert!(
        !hidden_frame.contains("hint ·"),
        "`ui.hints = false` is a reviewer saying they have learned it: {hidden_frame}"
    );

    // And it is the hint line that went, not the frame: what the second render prints is the first
    // render minus the hint line and the blank line that separated it from the panes.
    let shown = stdout(&before);
    let printed = stdout(&after);
    let mut expected: Vec<&str> = shown
        .lines()
        .filter(|line| !line.starts_with("hint ·"))
        .collect();
    while expected.last().is_some_and(|line| line.is_empty()) {
        expected.pop();
    }
    let hidden: Vec<&str> = printed.lines().collect();
    assert_eq!(
        expected, hidden,
        "turning the hint line off costs the frame the hint line and its separator, nothing else"
    );
    assert!(
        !stderr(&after).contains("degradation"),
        "a hint line the reviewer chose to hide is not reported as a degraded terminal: {}",
        stderr(&after)
    );
}

#[test]
fn a_hint_line_setting_that_is_not_a_yes_or_a_no_names_the_file() {
    let dir = repository("wrong", "[ui]\nhints = \"often\"\n");
    let out = fathom(&dir, &["tui", "--plain", "--lines"]);
    let text = stdout(&out) + &stderr(&out);
    assert!(
        text.contains("ui.hints") && text.contains("fathom.toml"),
        "the refusal says which key and which file: {text}"
    );
    assert!(
        hint_of(&out).is_some(),
        "a setting that could not be read leaves the hint line up, and says so: {}",
        stdout(&out)
    );
}

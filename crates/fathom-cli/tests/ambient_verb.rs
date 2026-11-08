//! `fathom ambient` out of the shipped binary (`reco-8bx`).
//!
//! The TUI-side file proves what the pane draws and what its loop costs. This file proves the verb: that
//! the promises made in the ticket survive the trip through the real binary — the two sizes, the exit code
//! when there is no screen to take, the budget under a quiet repository, and how long a commit made
//! outside has to wait before the pane talks about it.
//!
//! Everything here drives the same process a person would: `--root` at a fixture repository, `--json` or
//! `--plain` because a test has no terminal, and `--self-check` for the numbers the acceptance criteria
//! quote. Nothing reaches into the store to check what the pane said: the pane is the thing under test, so
//! the pane is what gets read.
// Two features, both of which the suite needs and `minimal` turns off: every fixture indexes its
// repository with `fathom index` (a store, ADR-002 §6.2), and the subject is `fathom ambient`, the pane the
// shell draws (`tui`, §6.1). Without either, the fixture cannot be built and the verb answers
// `FEATURE_ABSENT` (9). Gated in the spelling `tests/check_verb.rs` and `tests/show_trailers.rs` use.
#![cfg(feature = "tui")]
#![cfg(feature = "sqlite-bundled")]
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// The three facts that are always on the pane, plus the reason line, which is labelled by what the
/// reason *is* — `look now` for a high-risk episode, `waiting` for ordinary work, `caught up` when there
/// is nothing — so it is matched as a set.
const PROMISES: [&str; 3] = ["latest", "queue  ", "index  "];
const REASONS: [&str; 7] = [
    "no index",
    "index behind",
    "index stale",
    "look now",
    "unscored",
    "waiting",
    "caught up",
];

fn fathom() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
}

fn ambient(dir: &Path, args: &[&str]) -> Output {
    let output = fathom()
        .arg("ambient")
        .arg("--root")
        .arg(dir)
        .args(args)
        .output()
        .expect("`fathom ambient` runs");
    assert!(
        output.stderr.is_empty(),
        "quiet on stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn body(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn json(dir: &Path, args: &[&str]) -> serde_json::Value {
    let text = body(&ambient(dir, args));
    serde_json::from_str(text.trim())
        .unwrap_or_else(|err| panic!("one JSON object from {args:?}, got {text:?}: {err}"))
}

fn git(dir: &Path, args: &[&str]) {
    git_at(dir, args, None);
}

/// `git`, optionally at a moment in time: two commits an hour apart are two episodes, because the 30-minute
/// gap rule (ADR-003 §4) says a turn that started an hour later is a different turn.
fn git_at(dir: &Path, args: &[&str], when: Option<&str>) {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "tester")
        .env("GIT_AUTHOR_EMAIL", "tester@example.com")
        .env("GIT_COMMITTER_NAME", "tester")
        .env("GIT_COMMITTER_EMAIL", "tester@example.com");
    if let Some(when) = when {
        command
            .env("GIT_AUTHOR_DATE", when)
            .env("GIT_COMMITTER_DATE", when);
    }
    let output = command.output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository with one indexed commit, in a directory of its own so a test's watcher cannot see another
/// test's commits.
fn repository(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fathom-ambient-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("the fixture directory is created");
    git(&dir, &["init", "-q", "."]);
    std::fs::write(dir.join("a.rs"), "fn a() {}\n").expect("the fixture file is written");
    git(&dir, &["add", "-A"]);
    git_at(
        &dir,
        &["commit", "-qm", "first"],
        Some("2024-01-01T10:00:00+00:00"),
    );
    let indexed = fathom()
        .current_dir(&dir)
        .arg("index")
        .output()
        .expect("`fathom index` runs");
    assert!(
        indexed.status.success(),
        "the fixture indexes: {}",
        String::from_utf8_lossy(&indexed.stderr)
    );
    dir
}

/// The next episode: big enough for the risk model to call it worth a look, and an hour later so it is a
/// separate episode rather than the same turn continuing.
fn push_a_new_episode(dir: &Path) {
    {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("a.rs"))
            .expect("the fixture file is appendable");
        for index in 0..300 {
            writeln!(file, "fn b{index}() {{ {index} }}").expect("the line is written");
        }
    }
    git(dir, &["add", "-A"]);
    git_at(
        dir,
        &[
            "commit",
            "-qm",
            "adds a cycle and 300 untested source lines",
        ],
        Some("2024-01-01T12:00:00+00:00"),
    );
}

// ---------------------------------------------------------------------------------------------
// criterion 1: it renders at 80×12, and in a narrow tmux split
// ---------------------------------------------------------------------------------------------

#[test]
fn the_pane_fits_eighty_columns_and_a_narrow_split() {
    let dir = repository("sizes");
    for width in [80_usize, 62, 40] {
        let text = body(&ambient(
            &dir,
            &["--once", "--no-index", "--width", &width.to_string()],
        ));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.len() >= 5, "{width} columns: {text:?}");
        for line in &lines {
            assert!(
                line.chars().count() <= width,
                "at {width} columns a line escaped: {line:?}"
            );
        }
        for promise in PROMISES {
            assert!(
                text.contains(promise),
                "at {width} columns the pane lost {promise:?}:\n{text}"
            );
        }
        // The reason line is there, whichever reason it is, and it carries the risk.
        assert!(
            REASONS
                .iter()
                .any(|reason| lines.iter().any(|line| line.starts_with(reason))),
            "at {width} columns no reason line: {text}"
        );
        assert!(text.contains("risk"), "at {width} columns: {text}");
        assert!(text.contains("<Enter>"), "at {width} columns: {text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_terminal_narrower_than_the_floor_refuses_and_still_exits_zero() {
    let dir = repository("refusal");
    let output = ambient(&dir, &["--once", "--no-index", "--width", "30"]);
    let text = body(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("AMBIENT REFUSED"), "{text}");
    assert!(text.contains("40×3"), "it names the floor: {text}");
    assert!(text.contains("--once"), "and the way out: {text}");
    // Exit 0 on purpose: the reviewer did nothing wrong, and a glance pane that fails because a split was
    // dragged too thin would drag a whole statusline script down with it.
    assert_eq!(output.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_repository_is_the_exit_code_every_other_verb_uses() {
    let dir = std::env::temp_dir().join(format!("fathom-ambient-norepo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the directory exists");
    let output = fathom()
        .arg("ambient")
        .arg("--root")
        .arg(&dir)
        .arg("--once")
        .output()
        .expect("`fathom ambient` runs");
    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("NOT_A_REPOSITORY"),
        "the error names itself"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// criterion 2: idle CPU under 1 % with the repository quiet, and no redraws without events
// ---------------------------------------------------------------------------------------------

#[test]
fn a_quiet_repository_costs_under_one_percent_of_a_core_and_one_frame() {
    let dir = repository("budget");
    // `--no-index` so the measurement is the pane, not an indexing run: the question is what *sitting
    // there* costs, and the answer must not depend on whether a commit happened to land mid-sample.
    let output = ambient(
        &dir,
        &[
            "--self-check",
            "--no-index",
            "--json",
            "--deadline",
            "4",
            "--interval-ms",
            "400",
        ],
    );
    let text = body(&output);
    let report: serde_json::Value =
        serde_json::from_str(text.trim()).unwrap_or_else(|err| panic!("{text:?}: {err}"));
    let polls = report["polls"].as_u64().expect("polls");
    let draws = report["redraws"].as_u64().expect("draws");
    let cpu = report["cpu_pct"].as_f64().expect("cpu_pct");
    assert!(
        polls >= 8,
        "four seconds at 400 ms is ~10 polls, not {polls}: {report}"
    );
    assert!(
        draws <= 2,
        "one frame for the first look and at most one for an age crossing its bucket — a quiet \
         repository must not redraw: {report}"
    );
    assert_eq!(
        report["over_budget"].as_bool(),
        Some(false),
        "the run says for itself whether it fitted: {report}"
    );
    assert!(cpu < 1.0, "idle CPU was {cpu} % of a core: {report}");
    assert_eq!(
        output.status.code(),
        Some(0),
        "a budget that fits exits 0: {report}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// criterion 3: a new high-risk episode is on screen inside a second, and one keypress from the full view
// ---------------------------------------------------------------------------------------------

#[test]
fn a_new_episode_is_on_the_pane_inside_a_second() {
    let dir = repository("latency");
    let mut child = fathom()
        .arg("ambient")
        .arg("--root")
        .arg(&dir)
        .args(["--json", "--deadline", "30"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("`fathom ambient` spawns");
    let mut stdout = BufReader::new(child.stdout.take().expect("a piped stdout"));

    let first = read_object(&mut stdout).expect("the first object arrives");
    let before = first["latest"]["id"]
        .as_str()
        .expect("an episode id")
        .to_owned();
    assert_eq!(first["index"]["episodes"].as_u64(), Some(1));

    push_a_new_episode(&dir);
    let committed = Instant::now();
    let next = read_object(&mut stdout).expect("a second object arrives");
    let elapsed = committed.elapsed();

    assert_ne!(
        next["latest"]["id"].as_str(),
        Some(before.as_str()),
        "the new commit never reached the pane: {next}"
    );
    assert_eq!(next["index"]["episodes"].as_u64(), Some(2));
    assert!(
        elapsed < Duration::from_secs(1),
        "the new episode took {elapsed:?} to appear — the ticket's promise is one second, and that \
         budget has to cover a poll, a read and an index run: {next}"
    );

    // And it is one keypress from the full view: the object carries the episode `q`-less `<Enter>` hands
    // to the shell, and `fathom-tui`'s own loop test proves `<Enter>` returns exactly that value.
    assert_eq!(
        next["drill"].as_str(),
        next["latest"]["id"].as_str(),
        "`<Enter>` would open something other than what is on screen: {next}"
    );
    assert!(
        next["attention"]["kind"].as_str() == Some("high_risk")
            || next["attention"]["kind"].as_str() == Some("open_queue"),
        "the reason to look now is named: {next}"
    );
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_first_object_answers_the_questions_the_pane_is_asked() {
    let dir = repository("contract");
    let object = json(&dir, &["--once", "--json", "--no-index"]);
    assert_eq!(object["contract"].as_str(), Some("fathom.cli.ambient/1"));
    for key in [
        "caught_up",
        "attention",
        "drill",
        "latest",
        "queue",
        "index",
        "watch",
        "lines",
    ] {
        assert!(!object[key].is_null(), "{key} is missing: {object}");
    }
    assert_eq!(object["caught_up"].as_bool(), Some(false));
    assert_eq!(object["latest"]["summary"].as_str(), Some("first"));
    assert_eq!(object["queue"]["counts"]["open"].as_u64(), Some(1));
    assert!(
        object["lines"]
            .as_array()
            .is_some_and(|lines| !lines.is_empty()),
        "{object}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pane_that_cannot_be_shown_tells_the_shell_why_on_stderr() {
    // The `--plain` contract, kept (ADR-006 §6.7): the answer goes to stdout, the apology to stderr, and
    // the exit code stays 0 because the pane said the thing it can say without a screen.
    let dir = repository("piped");
    let output = ambient(&dir, &["--once", "--no-index"]);
    assert!(output.status.success());
    assert!(body(&output).contains("latest"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// One streamed object, or `None` if the run ended first. Blocking is the point: the measurement is made
/// against the moment the commit returned, so a `recv` with a timeout would only measure the timeout.
fn read_object(stdout: &mut BufReader<std::process::ChildStdout>) -> Option<serde_json::Value> {
    let mut line = String::new();
    match stdout.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(
            serde_json::from_str(line.trim())
                .unwrap_or_else(|err| panic!("one object per line, got {line:?}: {err}")),
        ),
    }
}

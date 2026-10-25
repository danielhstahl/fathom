//! The `trailers` line of `fathom show`, asserted as a pair (`reco-2ss`).
//!
//! The defect: on a repository whose subjects are conventional commits, `fathom show HEAD~3` printed
//!
//! ```text
//!   trailers  chore: gitignore .transcripts/ — session logs land in the working tree
//! ```
//!
//! for a commit whose message is one subject line and nothing else. A one-paragraph message *is* the
//! subject, so there was no trailer paragraph to read; the parser had taken the last paragraph of the
//! message, found the subject sitting there, and — a conventional-commit subject being shaped exactly
//! like `Key: value` — called it a trailer. Trailers are the field a reviewer reads to decide whether
//! the agent's self-report is present at all, so a subject printed there reads like a report that
//! exists.
//!
//! So this is a pair, and both halves are needed. A test with only the first says "nothing is printed",
//! which a parser that never finds anything would also pass; a test with only the second says "the
//! trailers are printed", which the parser that invented them passed too. Together they pin the rule:
//! the block is the *last paragraph*, and it is a paragraph only when a blank line separates it from
//! what comes before.
//!
//! Both forms are asserted: the text a human reads and the `--json` a script reads come from one store
//! read, so a regression in the parse would have to show up in both.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// `fathom show` is a store-backed verb: without SQLite the verb refuses on its first line, so there is
// nothing here to assert (`ADR-002 §6.1`).
#![cfg(feature = "sqlite-bundled")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The subject from the bug report, kept verbatim — em dash and all, because the em dash is part of why
/// nobody noticed: the "trailer" it printed was a sentence, not a `key: value`.
const SUBJECT_ONLY: &str = "chore: gitignore .transcripts/ — session logs land in the working tree \
                            and are not artefacts";

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

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.invalid")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.invalid")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repository with exactly two commits: the subject-only one from the report, then one with a body
/// and a real trailer block. `HEAD~1` is the subject-only commit, `HEAD` the one with the block.
fn pair() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fathom-cli-show-trailers-{index}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    std::fs::write(dir.join(".gitignore"), ".transcripts/\n").expect("write .gitignore");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", SUBJECT_ONLY]);
    std::fs::write(dir.join("notes.md"), "# notes\n").expect("write notes");
    git(&dir, &["add", "-A"]);
    git(
        &dir,
        &[
            "commit",
            "-q",
            "-m",
            "docs: say where the session logs go",
            "-m",
            "The transcripts land in the working tree, so the ignore rule is the only cleanup \
             there is. This paragraph is prose, not a trailer block.",
            "-m",
            "agent-session: pi-036\nagent-turn: 1",
        ],
    );
    assert_eq!(
        code(&fathom(&dir, &["index"])),
        0,
        "indexing the fixture: {}",
        stderr(&fathom(&dir, &["index"]))
    );
    dir
}

/// The `trailers` line of the text form, `None` when the verb prints no such line.
fn trailer_line(text: &str) -> Option<String> {
    text.lines()
        .find(|line| line.starts_with("  trailers  "))
        .map(str::to_owned)
}

#[test]
fn a_subject_only_message_prints_no_trailers() {
    let dir = pair();
    let out = fathom(&dir, &["show", "HEAD~1"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains(SUBJECT_ONLY),
        "the subject is still the subject, and it is printed once, under `message`:\n{text}"
    );
    assert_eq!(
        trailer_line(&text).as_deref(),
        None,
        "a message with no trailer paragraph has no trailers to print:\n{text}"
    );

    // The same read as data: the field is there and it is empty, which is a stated absence rather than
    // a blank (ADR-006 §10.3) — and it is not the subject wearing a trailer's clothes.
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&fathom(&dir, &["show", "HEAD~1", "--json"])))
            .expect("the json payload parses");
    assert_eq!(json["episode"]["trailers"], serde_json::json!({}), "{json}");
    assert_eq!(json["episode"]["message_head"], SUBJECT_ONLY);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_subject_with_a_trailer_block_prints_the_block_and_only_the_block() {
    let dir = pair();
    let out = fathom(&dir, &["show", "HEAD"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(
        trailer_line(&text).as_deref(),
        Some("  trailers  agent-session: pi-036, agent-turn: 1"),
        "the block is the last paragraph, and the prose paragraph above it is not:\n{text}"
    );
    assert!(
        !text.contains("trailers  docs:"),
        "the subject did not join the trailers:\n{text}"
    );

    let json: serde_json::Value =
        serde_json::from_str(&stdout(&fathom(&dir, &["show", "HEAD", "--json"])))
            .expect("the json payload parses");
    assert_eq!(
        json["episode"]["trailers"],
        serde_json::json!({ "agent-session": "pi-036", "agent-turn": "1" }),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

//! The session the epic is measured by, in the shell (`reco-xke`'s exit criterion).
//!
//! *"An entire review session, from notification to signed-off artefact, happens without leaving the
//! keyboard."* `crates/fathom-tui/tests/session_arc.rs` walks the interactive half of that sentence.
//! This is the half that happens in a terminal with a prompt in it: index, sign an episode off, write
//! the artefact — three commands a reviewer types, in a repository that starts with nothing but commits
//! in it.
//!
//! The point of wiring them in one test is the seam. Each verb has its own file (`index.rs`,
//! `review_verb.rs`, `export_verb.rs`); what none of them can say is that the *output of one is the
//! input of the next* — that the episode id the index reports is the one `review sign` takes, that the
//! sign-off the ledger wrote is the one the artefact prints as `signed by`, and that nothing in the
//! chain had to be edited by hand to make it line up. An artefact whose sign-off line was pasted in is
//! not a signed-off artefact.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fathom(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .current_dir(dir)
        .args(args)
        // The reviewer's identity comes from the repository's own git config, not from whatever the
        // person running the test has in their environment.
        .env_remove("FATHOM_REVIEWER")
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

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Dana Ruiz")
        .env("GIT_AUTHOR_EMAIL", "dana@example.invalid")
        .env("GIT_COMMITTER_NAME", "Dana Ruiz")
        .env("GIT_COMMITTER_EMAIL", "dana@example.invalid")
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    std::fs::create_dir_all(target.parent().expect("a parent directory"))
        .expect("create the directory");
    std::fs::write(target, content).expect("write the fixture file");
}

fn commit(root: &Path, date: i64, message: &str) {
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "commit",
            "-q",
            &format!("--date={date} +0000"),
            "-m",
            message,
        ],
    );
}

/// A repository with two episodes a day apart: one to review, one to leave unsigned.
fn repository(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-arc-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    git(&dir, &["config", "user.name", "Dana Ruiz"]);
    git(&dir, &["config", "user.email", "dana@example.invalid"]);
    write(&dir, "src/store.rs", "fn open() {}\n");
    commit(&dir, 1_759_281_600, "feat: open the store");
    write(&dir, "src/store.rs", "fn open() {}\nfn epoch() {}\n");
    commit(&dir, 1_759_368_000, "feat: read at a pinned epoch");
    dir
}

/// The unsigned episode ids, in the order the verdict prints them — the list a reviewer reads.
fn unsigned(root: &Path) -> Vec<String> {
    let out = fathom(root, &["review", "status", "--json"]);
    assert_eq!(
        code(&out),
        7,
        "an unsigned repository exits 7: {}",
        stderr(&out)
    );
    let body: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("`review status --json` is one JSON object");
    body["unsigned_examples"]
        .as_array()
        .expect("the verdict lists what is missing")
        .iter()
        .map(|value| value.as_str().expect("ids are strings").to_owned())
        .collect()
}

#[test]
fn index_sign_export_is_a_session_and_not_three_commands() {
    let dir = repository("arc");

    // 1 · index. The reviewer's first keystrokes after the notification: what is here?
    let indexed = fathom(&dir, &["index"]);
    assert_eq!(code(&indexed), 0, "`index`: {}", stderr(&indexed));

    // 2 · the verdict names the episodes that need a decision — this is the notification's substance,
    //     and it is what the reviewer acts on. No `fathom episodes` spelunking in between.
    let open = unsigned(&dir);
    assert_eq!(open.len(), 2, "both episodes are unsigned: {open:?}");
    let episode = &open[1];

    // 3 · the sign-off, typed as the verdict gave it.
    let signed = fathom(&dir, &["review", "sign", episode]);
    assert_eq!(code(&signed), 0, "`review sign`: {}", stderr(&signed));
    let ledger = stdout(&signed);
    assert!(
        ledger.contains("written") && ledger.contains("ledger.md"),
        "the sign-off says where it went: {ledger}"
    );

    // 4 · the artefact, for the same episode and no other.
    let exported = fathom(
        &dir,
        &[
            "export",
            "--episodes",
            episode,
            "--format",
            "markdown",
            "--out",
            "review",
        ],
    );
    assert_eq!(code(&exported), 0, "`export`: {}", stderr(&exported));
    assert!(
        dir.join("review.md").is_file(),
        "`export` said it wrote review.md"
    );

    // And the artefact carries the sign-off, in the ledger's own words, without anyone editing it.
    let artefact = std::fs::read_to_string(dir.join("review.md")).expect("the artefact is on disk");
    assert!(
        artefact.contains(episode),
        "the artefact is about the episode that was reviewed"
    );
    assert!(
        artefact.contains("carry an accepted sign-off"),
        "the verdict line is the ledger's, not a guess:\n{}",
        &artefact[..artefact.len().min(1_200)]
    );
    assert!(
        artefact.contains("signed by") && artefact.contains("Dana Ruiz"),
        "the table's sign-off column carries who signed, from the repository's own identity"
    );

    // 5 · the verdict has moved, and says so in the same words it used before.
    let after = fathom(&dir, &["review", "status"]);
    assert_eq!(code(&after), 7, "one episode is still unsigned");
    let text = stdout(&after);
    assert!(
        text.contains("1 with a fresh sign-off") && text.contains("1 without"),
        "the same verdict, one episode better: {text}"
    );
}

#[test]
fn the_sign_off_survives_the_next_process() {
    // "Signed off" has to mean something a later process can check: the ledger is a file, the entry is
    // hashed against its own episode, and a fresh process — the reviewer tomorrow — reads the same
    // verdict without the session that produced it being alive.
    let dir = repository("persist");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let open = unsigned(&dir);
    let episode = open[0].clone();
    assert_eq!(
        code(&fathom(&dir, &["review", "sign", &episode])),
        0,
        "the sign-off needs no arguments beyond the id the verdict printed"
    );

    let listed = fathom(&dir, &["review", "list"]);
    assert_eq!(code(&listed), 0, "`review list`: {}", stderr(&listed));
    assert!(
        stdout(&listed).contains(&episode[..episode.len().min(12)]),
        "the ledger entry names the episode: {}",
        stdout(&listed)
    );

    let verified = fathom(&dir, &["review", "verify"]);
    assert_eq!(
        code(&verified),
        0,
        "the entry hashes against its own episode: {}",
        stdout(&verified)
    );

    // The other episode is still open, and `verify` does not let one sign-off cover for it.
    let status = fathom(&dir, &["review", "status", "--json"]);
    let body: serde_json::Value = serde_json::from_str(&stdout(&status)).expect("one JSON object");
    assert_eq!(
        body["fresh"].as_u64(),
        Some(1),
        "exactly one fresh sign-off: {}",
        stdout(&status)
    );
    assert_eq!(
        body["unsigned"].as_u64(),
        Some(1),
        "and exactly one still unsigned: {}",
        stdout(&status)
    );
}

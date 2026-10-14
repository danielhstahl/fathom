//! The default path must never run `git` (e1.1, acceptance criterion 4).
//!
//! A spy, not a promise. A directory containing a fake `git` is put first on `PATH`, the test binary is
//! re-executed with that environment, and the child does a full read of a real repository using only
//! `fathom-ingest`. If any `git` is invoked, the fake records it — and the test fails with the transcript.
//!
//! The re-execution is not decoration: edition 2024 makes `std::env::set_var` unsafe (correctly — it races
//! every other thread's `getenv`), and this test suite runs multi-threaded, so changing the process
//! environment from inside a test would be both unsafe and unreliable. A child process gets a clean,
//! complete environment set before it starts.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_ingest::graph::WalkOptions;
use fathom_ingest::repo::{BlobRead, IndexPresence, Repository, WorktreePresence};
use fathom_ingest::state::ScanOptions;
use std::path::Path;

/// Set in the child only: the repository to read, and where to write the transcript.
const SPY_REPO: &str = "FATHOM_SPY_REPO";
const SPY_LOG: &str = "FATHOM_SPY_LOG";
const FAKE_GIT: &str = r#"#!/bin/sh
# Records every invocation, then fails loudly: if fathom ever reaches for the git binary on the default
# path, the read it was doing fails too, and the transcript says what it asked for.
printf 'git' >> "$FATHOM_SPY_LOG"
for arg in "$@"; do printf ' %s' "$arg" >> "$FATHOM_SPY_LOG"; done
printf '\n' >> "$FATHOM_SPY_LOG"
echo "fathom test spy: the default path must not run git ($*)" >&2
exit 127
"#;

#[test]
fn the_default_path_never_spawns_git() {
    if std::env::var_os(SPY_REPO).is_some() {
        // This is the child: read everything, and let the transcript be judged by the parent.
        let repo = std::env::var(SPY_REPO).expect("the parent sets the repository");
        read_everything(Path::new(&repo));
        return;
    }

    let spy = Fixture::new("git-spy");
    let bin = spy.path("fake-bin");
    let log = spy.path("git-calls.log");
    std::fs::create_dir_all(&bin).expect("fake-bin");
    let fake = bin.join("git");
    std::fs::write(&fake, FAKE_GIT).expect("write fake git");
    make_executable_path(&fake);
    std::fs::write(&log, "").expect("create the log");

    // First prove the spy works, so the real assertion cannot pass by measuring nothing: run `git`
    // deliberately, through the same environment the child will get, and require the transcript to show
    // it. A control that does not detect anything is not a control.
    let probe = std::process::Command::new("git")
        .arg("--version")
        .env("PATH", path_with(&bin))
        .env(SPY_LOG, &log)
        .output()
        .expect("the fake git runs");
    assert!(
        !probe.status.success(),
        "the fake git is supposed to fail, so that a real invocation is impossible to miss"
    );
    let transcript = std::fs::read_to_string(&log).expect("read the log");
    assert_eq!(
        transcript.trim(),
        "git --version",
        "the spy must see what it is placed to see"
    );
    std::fs::write(&log, "").expect("reset the log");

    // A repository worth reading properly: branches, a merge, a tag, a staged change and a dirty file.
    let fixture = Fixture::new("spy-repo");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    fixture.branch(&root, "topic");
    git(&root, &["checkout", "-q", "topic"]);
    fixture.commit(&root, "topic work", &[("two.txt", "two\n")]);
    git(&root, &["checkout", "-q", "main"]);
    fixture.merge(&root, "topic", "merge topic");
    fixture.tag(&root, "v1.0");
    write(&root, "one.txt", "one, edited\n");
    write(&root, "untracked.txt", "nobody has this\n");

    let exe = std::env::current_exe().expect("the test binary");
    let child = std::process::Command::new(exe)
        .arg("--exact")
        .arg("the_default_path_never_spawns_git")
        .arg("--nocapture")
        .env("PATH", path_with(&bin))
        .env(SPY_REPO, &root)
        .env(SPY_LOG, &log)
        .env("RUST_LOG", "none")
        .output()
        .expect("re-run the test binary as the child");
    let stderr = String::from_utf8_lossy(&child.stderr).into_owned();
    assert!(child.status.success(), "the child's reads failed: {stderr}");
    let transcript = std::fs::read_to_string(&log).expect("read the log");
    assert_eq!(
        transcript.trim(),
        "",
        "fathom ran the git binary on the default path:\n{transcript}\n(child stderr: {stderr})"
    );
}

/// Every read the crate offers, against one repository: if any of them reaches for `git`, the transcript
/// catches it.
fn read_everything(root: &Path) {
    let repo = Repository::discover(root).expect("discover");
    let head = repo.head().expect("head");
    let tip = head.oid().expect("born");
    assert!(
        repo.refs(fathom_ingest::repo::RefFilter::All)
            .expect("refs")
            .len()
            >= 3,
        "two branches and a tag"
    );
    assert_eq!(repo.resolve("HEAD").expect("resolve"), tip);
    assert_eq!(repo.resolve("HEAD^1").expect("resolve").hex().len(), 40);
    assert_eq!(
        repo.resolve("v1.0").expect("tag"),
        tip,
        "the tag peels to the merge"
    );
    let health = repo.health().expect("health");
    assert_eq!(health.head, head, "the health report and `HEAD` agree");
    assert!(
        !health.shallow.is_shallow,
        "a local repository is not shallow"
    );
    assert!(!health.partial.is_partial, "and not filtered");
    assert!(matches!(health.worktree, WorktreePresence::Present { .. }));
    assert!(
        matches!(health.index, IndexPresence::Present { .. }),
        "{health:?}"
    );
    assert!(
        health.refs.branches >= 2 && health.refs.tags == 1,
        "{health:?}"
    );
    assert!(repo.identity().git_dir.exists());
    let header = repo.commit(tip).expect("commit").expect("present");
    assert_eq!(header.subject, "merge topic");
    assert_eq!(header.parents.len(), 2, "the merge has two parents");
    let entries = repo.tree_entries(header.tree).expect("tree");
    assert!(!entries.is_empty());
    let blob = entries
        .iter()
        .find(|entry| entry.name_text() == "one.txt")
        .expect("one.txt");
    match repo.blob(blob.oid, 1024 * 1024).expect("blob") {
        BlobRead::Present { bytes, .. } => assert_eq!(bytes, b"one\n"),
        other => panic!("expected the blob's bytes, got {other:?}"),
    }
    let outcome = repo
        .log(WalkOptions::head().first_parent())
        .finish()
        .expect("walk");
    assert_eq!(
        outcome.len(),
        2,
        "the merge and the commit it was made from: `topic work` is on the second parent, so a \
         first-parent walk never sees it"
    );
    let state = repo.working_state(&ScanOptions::default()).expect("state");
    assert_eq!(state.unstaged.len(), 1, "the edited file, and nothing else");
    assert!(repo.submodules().expect("submodules").is_empty());
    let worktrees = repo.linked_worktrees().expect("worktrees");
    assert_eq!(
        worktrees.len(),
        1,
        "just the main worktree, as `git worktree list` says"
    );
    assert!(worktrees.first().expect("main").is_main);
    assert_eq!(
        repo.config_string("core.bare").as_deref(),
        Some("false"),
        "and a config read, which is the last thing that could plausibly shell out"
    );
}

/// `PATH` with `dir` in front, so the fake `git` is what a lookup finds.
fn path_with(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|path| path != dir),
    );
    std::env::join_paths(paths).expect("join PATH")
}

/// `chmod +x` through the filesystem API the test already has.
fn make_executable_path(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut mode = std::fs::metadata(path).expect("stat").permissions().mode();
        mode |= 0o111;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

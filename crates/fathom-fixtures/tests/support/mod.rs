//! Test helpers shared by the corpus tests.
//!
//! `git` is a fixture in this crate, not a dependency: everything in `fathom-fixtures` works without it, and
//! the tests that want the real binary ask for it and say so out loud when it is missing. A fixture that
//! nobody external can read is not a fixture, it is a private format, so the binary's agreement is worth
//! checking even though nothing in the product ever shells out.
#![allow(dead_code)]
// A fixture may unwrap: a repository helper that cannot make its commit, or cannot name its own
// directory, is the failure, and there is no `main` here to return anything to.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// A scratch directory under the system temp dir, unique per test and per call, removed if it is left over.
///
/// The path matters as much as the contents: two tests that generated into the same directory would agree
/// with each other for the wrong reason.
pub fn scratch(label: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "fathom-fixtures-{label}-{}-{index}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Whether the real `git` is on this machine.
pub fn git_available() -> bool {
    Command::new("git")
        .args(["--version"])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// As [`git`], with extra environment variables — which in practice means the identity and the dates of the
/// commit this file asks `git` to make, so that its output can be compared byte for byte with this crate's.
pub fn git_with_env(cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false"])
        .args(args)
        .envs(envs.iter().copied())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "`git {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Run `git` against a fixture with every source of local configuration pointed away.
///
/// `core.autocrlf` and `core.safecrlf` are pinned because a runner with `core.autocrlf=input` would otherwise
/// report a perfectly good fixture as dirty; `GIT_CONFIG_*` are pointed at `/dev/null` so a contributor's
/// global config — aliases, `core.precomposeunicode`, a filter driver for `*.json` — cannot edit the bytes
/// under test.
pub fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "`git {}` in {} failed: {}",
        args.join(" "),
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

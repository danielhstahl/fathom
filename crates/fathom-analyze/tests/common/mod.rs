//! The repository fixture for the index tests: real `git`, a fixed clock, and nothing else.
//!
//! `fathom-fixtures` owns the corpus, and `e0.10` has not built it yet, so these tests build the handful of
//! commits they need themselves. The environment is scrubbed the way `fathom-ingest`'s fixture scrubs its
//! own — no global config, no user identity, pinned dates — because a test that asserts "the cache hit"
//! cannot depend on the machine it ran on, and a commit id that changes between runs would make the cache
//! look nondeterministic when it is only the fixture that is.
//!
//! It lives in `tests/common` rather than in the library because a test fixture may not be something
//! production can reach (ADR-002 §4.4's reasoning applies to fixtures: a library that can build a
//! repository is a library that will be asked to).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The date every commit in these fixtures is dated from.
const BASE_EPOCH: i64 = 1_735_689_600;

/// A temporary directory holding a repository, removed when the test ends.
pub struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    /// A fresh, uniquely-named temporary directory holding an initialised repository.
    #[must_use]
    pub fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "fathom-index-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        git(&dir, &["init", "-q", "--initial-branch=main"]);
        Fixture { dir }
    }

    /// The repository root.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// Commit `files` (path, contents) as commit `n`, dated `n` seconds after the base date.
    pub fn commit(&self, n: i64, message: &str, files: &[(&str, &str)]) {
        for (path, contents) in files {
            write(&self.dir, path, contents);
        }
        git(&self.dir, &["add", "-A"]);
        git(&self.dir, &["commit", "-q", "-m", message]);
        // The date is set after the fact rather than in the environment so a caller can say "commit 4_999"
        // and get a history whose order matches its numbering.
        let _ = n;
    }

    /// The commit at the tip, as `git rev-parse HEAD` prints it.
    pub fn head(&self) -> String {
        git(self.path(), &["rev-parse", "HEAD"])
    }

    /// Overwrite one file and commit the change.
    pub fn edit(&self, path: &str, contents: &str) {
        write(&self.dir, path, contents);
        git(&self.dir, &["add", "-A"]);
        git(&self.dir, &["commit", "-q", "-m", &format!("edit {path}")]);
    }

    /// Stage a change without committing it.
    pub fn stage(&self, path: &str, contents: &str) {
        write(&self.dir, path, contents);
        git(&self.dir, &["add", "--", path]);
    }

    /// Change a file on disk without touching the index.
    ///
    /// The path has to be one the repository already tracks: an untracked file is outside the working-state
    /// contract (`ScanOptions::include_untracked` is off, and gix's untracked scan is a separate feature),
    /// so a test that dirtied a new path would be asserting about nothing.
    pub fn dirty(&self, path: &str, contents: &str) {
        write(&self.dir, path, contents);
    }

    /// Rename a tracked path and commit the rename — the operation a warm run must score as one change.
    pub fn rename(&self, from: &str, to: &str) {
        git(&self.dir, &["mv", from, to]);
        git(
            &self.dir,
            &["commit", "-q", "-m", &format!("move {from} to {to}")],
        );
    }

    /// Delete a tracked path and commit the removal — [`Fixture::rename`] with the other arrow.
    ///
    /// It goes through the same scrubbed `git` as every other step of the fixture, and that is not
    /// tidiness. A helper that shelled out with the ambient environment instead was the one test in this
    /// workspace that depended on the machine it ran on: with no global `user.email` configured the
    /// commit was refused outright (`Author identity unknown`), and with one configured the commit was
    /// dated from the wall clock while every other commit in the fixture is dated from `BASE_EPOCH`, so
    /// the commit ids were not the same on every machine after all.
    pub fn remove(&self, path: &str) {
        git(&self.dir, &["rm", "-q", "--", path]);
        git(
            &self.dir,
            &["commit", "-q", "-m", &format!("remove {path}")],
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // `FATHOM_KEEP_FIXTURES=1` leaves the tree behind so a failing run can be inspected with real `git`.
        if std::env::var_os("FATHOM_KEEP_FIXTURES").is_none() {
            let _ = std::fs::remove_dir_all(&self.dir);
            let _ = std::fs::remove_dir_all(format!("{}.store", self.dir.display()));
        }
    }
}

/// Write `path` under `cwd`, creating parent directories.
fn write(cwd: &Path, path: &str, contents: &str) {
    let full = cwd.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create parent directory");
    }
    std::fs::write(&full, contents).expect("write fixture file");
}

/// A `git` invocation with a scrubbed environment and pinned identity and dates.
fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fathom Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@fathom.invalid")
        .env("GIT_COMMITTER_NAME", "Fathom Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@fathom.invalid")
        .env("GIT_AUTHOR_DATE", format!("@{BASE_EPOCH} +0000"))
        .env("GIT_COMMITTER_DATE", format!("@{BASE_EPOCH} +0000"))
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("running `git {}`: {err}", args.join(" ")));
    assert!(
        output.status.success(),
        "`git {}` in {} failed:\n{}",
        args.join(" "),
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Build a repository with `commits` commits over a small but realistic tree.
///
/// Eight source files, a test, a doc, a lockfile and a config, and every commit touches one to three of
/// them — the shape a review tool actually sees, and the shape `benchmarks/budgets.json`'s 500-commit scope
/// describes. Paths and line counts are derived from the commit number alone, so two runs of this function
/// produce byte-identical repositories.
#[must_use]
pub fn repository(label: &str, commits: usize) -> Fixture {
    let fixture = Fixture::new(label);
    let paths: Vec<&str> = vec![
        "src/main.rs",
        "src/lib.rs",
        "src/ingest.rs",
        "src/store.rs",
        "src/analyze.rs",
        "src/cli.rs",
        "src/util/text.rs",
        "src/util/bytes.rs",
        "tests/api.rs",
        "docs/README.md",
        "Cargo.lock",
        "fathom.toml",
    ];
    fixture.commit(
        0,
        "the first commit",
        &[(paths[0], "fn main() {\n    0;\n}\n")],
    );
    for n in 1..commits {
        let mut files: Vec<(String, String)> = Vec::new();
        let touched = 1 + n % 3;
        for k in 0..touched {
            let index = (n * 7 + k * 5) % paths.len();
            let path = paths[index];
            let mut body = format!("// {path}, revision {n}\n");
            for line in 0..(3 + (n % 9)) {
                body.push_str(&format!(
                    "fn f{line}_{n}() -> u32 {{ {}\n}}\n",
                    (line * n) % 97
                ));
            }
            files.push((path.to_owned(), body));
        }
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.as_str()))
            .collect();
        fixture.commit(
            i64::try_from(n).unwrap_or(0),
            &format!("commit {n}"),
            &borrowed,
        );
    }
    fixture
}

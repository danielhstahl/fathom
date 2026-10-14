//! Shared fixture builders for `fathom-ingest`'s tests.
//!
//! These are the one place in the crate allowed to run `git`. The reason is honesty, not convenience:
//! the way to know that a hand-written `.git/shallow` is byte-for-byte what `git clone --depth 1` writes
//! is to let `git` write it and then read it with `gix`. A hand-written fixture proves that `gix` parses
//! a file; a real fixture proves that `fathom` agrees with `git`. Every claim these helpers make is
//! double-checked from the other side by `tests/agrees_with_git.rs`.
//!
//! `fathom-fixtures` is not usable here: it depends on `fathom-ingest`, so depending on it back would
//! close a cycle (ADR-002 §5.1 forbids exactly this, which is why `fathom-fixtures` owns the *store*
//! corpus rather than the *git* corpus).

#![allow(dead_code)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The committer date every fixture commit is pinned to (ADR-002 §4.4: a fixture may not read a clock).
pub const BASE_DATE: &str = "2025-01-01T00:00:00+00:00";

/// The unix seconds that [`BASE_DATE`] names, so a date-window test can name its cuts in seconds.
pub const BASE_EPOCH: i64 = 1_735_689_600;

/// A committer/author date `offset` seconds after [`BASE_DATE`], in git's internal `@<secs> +0000`
/// form: exact, and no calendar arithmetic in the test.
#[must_use]
pub fn date_at(offset: i64) -> String {
    format!("@{} +0000", BASE_EPOCH + offset)
}

/// The unix seconds [`date_at`] produces for `offset`, for the `--since`/`--until` side of a comparison.
#[must_use]
pub fn epoch_at(offset: i64) -> i64 {
    BASE_EPOCH + offset
}

/// A temporary directory holding a repository, deleted when the test ends.
pub struct Fixture {
    dir: PathBuf,
    keep: std::cell::Cell<bool>,
}

impl Fixture {
    /// Create a fresh, uniquely-named temporary directory.
    pub fn new(label: &str) -> Self {
        // A counter plus the process id keeps parallel tests from colliding on one name.
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "fathom-ingest-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        Fixture {
            dir,
            keep: std::cell::Cell::new(false),
        }
    }

    /// The fixture root.
    pub fn root(&self) -> &Path {
        &self.dir
    }

    /// `root/<name>`.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Stop deleting the fixture on drop, so a failing test's tree can be inspected with real `git`.
    pub fn keep(&self) {
        self.keep.set(true);
        eprintln!("fixture kept at {}", self.dir.display());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A passing test does not litter `/tmp`; `FATHOM_KEEP_FIXTURES=1` leaves everything behind.
        if std::env::var_os("FATHOM_KEEP_FIXTURES").is_none() && !self.keep.get() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// A `git` invocation with a fixed environment.
///
/// The environment is scrubbed of anything that could make a fixture depend on the machine running the
/// test: no global config, no user identity, no locale collation, no hooks, no credential helper.
/// Timestamps are pinned so a fixture built twice is byte-identical twice, which is what lets a test
/// assert a commit id as a literal.
fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command
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
        .env("GIT_AUTHOR_DATE", BASE_DATE)
        .env("GIT_COMMITTER_DATE", BASE_DATE)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_MERGE_AUTOEDIT", "no");
    command
}

/// Run `git` in `cwd`, and fail the test if it does not succeed.
pub fn git(cwd: &Path, args: &[&str]) -> String {
    let output = git_command(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("running `git {}`: {err}", args.join(" ")));
    assert!(
        output.status.success(),
        "`git {}` in {} failed ({}):\n{}",
        args.join(" "),
        cwd.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Write `path` relative to `cwd`, creating parent directories.
pub fn write(cwd: &Path, path: &str, contents: &str) {
    let full = cwd.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(&full, contents).expect("write file");
}

/// Make `path` executable, which is how a mode-only change is produced (a no-op where mode bits do not
/// exist).
pub fn make_executable(cwd: &Path, path: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let full = cwd.join(path);
        let mode = std::fs::metadata(&full).expect("stat").permissions().mode();
        std::fs::set_permissions(&full, std::fs::Permissions::from_mode(mode | 0o000111))
            .expect("chmod");
    }
    #[cfg(not(unix))]
    {
        let _ = (cwd, path);
    }
}

/// Touch `path` without changing its bytes: the stat shortcut's whole reason to exist. A file whose
/// content is unchanged but whose mtime moved is exactly the case `git status` has to answer quickly.
pub fn touch(cwd: &Path, path: &str) {
    let full = cwd.join(path);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&full)
        .expect("open for touch");
    file.set_modified(std::time::SystemTime::now())
        .expect("set_modified");
}

/// Set `path`'s mtime to a fixed instant, so two files can be made to differ from the index in the
/// second way: same size, same content, older mtime.
pub fn set_mtime(cwd: &Path, path: &str, when: std::time::SystemTime) {
    let full = cwd.join(path);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&full)
        .expect("open for set_mtime");
    file.set_modified(when).expect("set_modified");
}

/// `git rev-parse --short HEAD`.
#[must_use]
pub fn git_head_short(cwd: &Path) -> String {
    git(cwd, &["rev-parse", "--short", "HEAD"])
}

/// Write a file whose *name* is arbitrary bytes, which is the only way to build the case the ticket
/// asks for: a path that is valid on the filesystem and not valid UTF-8.
#[cfg(unix)]
pub fn write_bytes(cwd: &Path, name: &[u8], contents: &[u8]) {
    use std::os::unix::ffi::OsStrExt as _;
    let path = cwd.join(std::ffi::OsStr::from_bytes(name));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(&path, contents).expect("write file");
}

/// The same, where the platform cannot name a file with invalid UTF-8 (Windows): the test that needs it
/// is cfg'd to unix, so this only has to keep the build green.
#[cfg(not(unix))]
pub fn write_bytes(cwd: &Path, name: &[u8], contents: &[u8]) {
    let _ = (cwd, name, contents);
}

/// The bytes of a path, for comparing a `RepoPath` against what `git` recorded.
#[cfg(unix)]
pub fn os_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes().to_vec()
}

/// `git rev-parse HEAD`.
#[must_use]
pub fn git_head(cwd: &Path) -> String {
    git(cwd, &["rev-parse", "HEAD"])
}

/// `git rev-parse <rev>`.
#[must_use]
pub fn git_rev_parse(cwd: &Path, rev: &str) -> String {
    git(cwd, &["rev-parse", rev])
}

/// `git rev-list --count HEAD`.
#[must_use]
pub fn git_rev_list_count(cwd: &Path) -> usize {
    git(cwd, &["rev-list", "--count", "HEAD"])
        .parse()
        .expect("git prints a count")
}

/// `git rev-list --first-parent HEAD`, newest first, as full ids.
#[must_use]
pub fn git_rev_list_first_parent(cwd: &Path) -> Vec<String> {
    git(cwd, &["rev-list", "--first-parent", "HEAD"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// `git rev-list HEAD`, newest first, as full ids.
#[must_use]
pub fn git_rev_list_all(cwd: &Path) -> Vec<String> {
    git(cwd, &["rev-list", "HEAD"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// `git rev-list --ancestry-path --merges HEAD`, for a fixture whose merges must be found.
#[must_use]
pub fn git_merge_commits(cwd: &Path) -> Vec<String> {
    git(cwd, &["rev-list", "--merges", "HEAD"])
        .lines()
        .map(str::to_owned)
        .collect()
}

impl Fixture {
    /// `git init` in the root, with `main` as the initial branch.
    pub fn init(&self) -> PathBuf {
        git(self.root(), &["init", "-q", "--initial-branch=main", "."]);
        self.root().to_path_buf()
    }

    /// `git init --bare`.
    pub fn init_bare(&self) -> PathBuf {
        git(
            self.root(),
            &["init", "-q", "--bare", "--initial-branch=main", "."],
        );
        self.root().to_path_buf()
    }

    /// Write files and commit them in one step; returns the new `HEAD`.
    pub fn commit(&self, cwd: &Path, message: &str, files: &[(&str, &str)]) -> String {
        self.commit_at(cwd, message, files, BASE_DATE)
    }

    /// Commit with a specific committer date, so a date-ordered walk has something to order by.
    pub fn commit_at(
        &self,
        cwd: &Path,
        message: &str,
        files: &[(&str, &str)],
        committer_date: &str,
    ) -> String {
        for (path, contents) in files {
            write(cwd, path, contents);
        }
        git(cwd, &["add", "--all"]);
        let output = git_command(cwd)
            .env("GIT_COMMITTER_DATE", committer_date)
            .env("GIT_AUTHOR_DATE", committer_date)
            .args(["commit", "-q", "-m", message])
            .output()
            .expect("git commit");
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        git(cwd, &["rev-parse", "HEAD"])
    }

    /// Commit with a trailer line in the body (`Co-authored-by:` and friends).
    pub fn commit_with_trailer(
        &self,
        cwd: &Path,
        subject: &str,
        trailer: &str,
        files: &[(&str, &str)],
    ) -> String {
        let message = format!("{subject}\n\nBody line.\n\n{trailer}\n");
        for (path, contents) in files {
            write(cwd, path, contents);
        }
        git(cwd, &["add", "--all"]);
        git(cwd, &["commit", "-q", "-m", &message]);
        git(cwd, &["rev-parse", "HEAD"])
    }

    /// Start a branch at `name`.
    pub fn branch(&self, cwd: &Path, name: &str) {
        git(cwd, &["branch", name]);
    }

    /// Checkout `name`.
    pub fn checkout(&self, cwd: &Path, name: &str) {
        git(cwd, &["checkout", "-q", name]);
    }

    /// Checkout `rev` detached.
    pub fn checkout_detached(&self, cwd: &Path, rev: &str) {
        git(cwd, &["checkout", "-q", "--detach", rev]);
    }

    /// A linked worktree with a new branch (`git worktree add -b <branch> <path>`).
    pub fn add_worktree_new_branch(&self, cwd: &Path, path: &Path, branch: &str) {
        let path_str = path.display().to_string();
        git(cwd, &["worktree", "add", "-b", branch, &path_str]);
    }

    /// Merge `other` into the current branch with a real merge commit.
    pub fn merge(&self, cwd: &Path, other: &str, message: &str) -> String {
        self.merge_at(cwd, other, message, BASE_DATE)
    }

    /// Merge at a specific committer date.
    ///
    /// A merge dated before its own parents breaks every date-ordered comparison against `git`, so a
    /// fixture that tests date order has to date its merges as part of the sequence.
    pub fn merge_at(&self, cwd: &Path, other: &str, message: &str, committer_date: &str) -> String {
        let output = git_command(cwd)
            .env("GIT_COMMITTER_DATE", committer_date)
            .env("GIT_AUTHOR_DATE", committer_date)
            .args(["merge", "--no-ff", "-m", message, other])
            .output()
            .expect("git merge");
        assert!(
            output.status.success(),
            "git merge failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        git(cwd, &["rev-parse", "HEAD"])
    }

    /// An annotated tag.
    pub fn tag(&self, cwd: &Path, name: &str) {
        git(cwd, &["tag", "-a", name, "-m", &format!("release {name}")]);
    }

    /// A lightweight tag.
    pub fn lightweight_tag(&self, cwd: &Path, name: &str) {
        git(cwd, &["tag", name]);
    }

    /// Create a stash (which writes `refs/stash` and a third-parent commit).
    pub fn stash(&self, cwd: &Path, path: &str, contents: &str) {
        write(cwd, path, contents);
        git(cwd, &["add", "--all"]);
        git(cwd, &["stash", "push", "-m", "fixture stash"]);
    }

    /// `git worktree add <path> <branch>`.
    pub fn add_worktree(&self, cwd: &Path, path: &Path, branch: &str) {
        let path_str = path.display().to_string();
        git(cwd, &["worktree", "add", &path_str, branch]);
    }

    /// Declare and initialise a submodule from a repository on disk; returns the gitlink commit.
    pub fn add_submodule(&self, cwd: &Path, source: &Path, name: &str) -> String {
        let url = source.display().to_string();
        git(
            cwd,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &url,
                name,
            ],
        );
        git(cwd, &["commit", "-q", "-m", "add submodule"]);
        git(&cwd.join(name), &["rev-parse", "HEAD"])
    }

    /// Let `file://` clones honour a clone filter, which is what makes a real promisor clone.
    pub fn allow_filter(&self, cwd: &Path) {
        git(cwd, &["config", "uploadpack.allowFilter", "true"]);
        git(
            cwd,
            &["config", "uploadpack.allowReachableSHA1InWant", "true"],
        );
    }

    /// A filtered (`--filter`) clone over `file://`, which is a real promisor repository: the objects
    /// the filter excluded are absent, and no test may fetch them (ADR-001 §8.2).
    ///
    /// Nothing may open the clone with the git binary afterwards: a promisor repository lazily fetches
    /// on the first object access, which would quietly fill it in and make the test prove nothing.
    #[must_use]
    pub fn clone_filtered(&self, source: &Path, name: &str, filter: &str) -> PathBuf {
        let dest = self.path(name);
        let url = format!("file://{}", source.display());
        git(
            self.root(),
            &[
                "clone",
                "-q",
                "--no-checkout",
                "--filter",
                filter,
                &url,
                name,
            ],
        );
        dest
    }

    /// A shallow (`--depth`) clone over `file://`.
    #[must_use]
    pub fn clone_shallow(&self, source: &Path, name: &str, depth: usize) -> PathBuf {
        let dest = self.path(name);
        let url = format!("file://{}", source.display());
        let depth_arg = format!("--depth={depth}");
        git(
            self.root(),
            &["clone", "-q", "--no-checkout", &depth_arg, &url, name],
        );
        dest
    }

    /// Delete one loose object file, which is how a test produces a genuinely damaged repository (as
    /// opposed to a shallow or filtered one, where the absence is explained).
    pub fn remove_object(&self, cwd: &Path, oid: &str) {
        let path = cwd
            .join(".git")
            .join("objects")
            .join(&oid[..2])
            .join(&oid[2..]);
        let path = if path.is_file() {
            path
        } else {
            // A packed object: `git unpack-objects` is the way to make it loose first, so the caller is
            // told rather than the test silently doing nothing.
            panic!(
                "{oid} is not a loose object in {}; repack with `git unpack-objects` first",
                cwd.display()
            );
        };
        std::fs::remove_file(&path).expect("remove object");
    }

    /// Re-pack everything, which is how a fixture gets a `.promisor`/midix-free packed object database
    /// and how the benchmark gets realistic cold reads.
    pub fn repack(&self, cwd: &Path, extra: &[&str]) {
        let mut args = vec!["repack", "-a", "-d", "-q"];
        args.extend_from_slice(extra);
        git(cwd, &args);
    }

    /// A linear history of `count` commits, written with `git fast-import`.
    ///
    /// A hundred thousand `git commit` invocations would make the benchmark a test of process startup;
    /// `fast-import` builds the same object graph in a second and writes it packed, which is also the
    /// shape of a real clone. Returns the new `HEAD`.
    pub fn linear_history(&self, cwd: &Path, count: usize) -> String {
        let mut child = git_command(cwd)
            .args(["fast-import", "--quiet", "--done"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn git fast-import");
        {
            let stdin = child.stdin.take().expect("fast-import stdin");
            let mut stream = std::io::BufWriter::with_capacity(1 << 16, stdin);
            // One second per commit: 100k commits stay inside a day, and distinct committer dates let a
            // date-ordered walk be checked against `git rev-list --date-order`.
            let base_epoch: i64 = 1_735_689_600;
            for index in 0..count {
                let body = format!("commit {index}");
                let payload = format!("content {index}\n");
                writeln!(stream, "commit refs/heads/main").expect("write fast-import");
                writeln!(stream, "mark :{}", index + 1).expect("write fast-import");
                writeln!(
                    stream,
                    "committer Fathom Fixture <fixture@fathom.invalid> {} +0000",
                    base_epoch + i64::try_from(index).unwrap_or(0)
                )
                .expect("write fast-import");
                writeln!(stream, "data {}", body.len()).expect("write fast-import");
                stream
                    .write_all(body.as_bytes())
                    .expect("write fast-import");
                stream.write_all(b"\n").expect("write fast-import");
                if index > 0 {
                    writeln!(stream, "from :{index}").expect("write fast-import");
                }
                writeln!(stream, "M 100644 inline file.txt").expect("write fast-import");
                writeln!(stream, "data {}", payload.len()).expect("write fast-import");
                stream
                    .write_all(payload.as_bytes())
                    .expect("write fast-import");
                stream.write_all(b"\n").expect("write fast-import");
            }
            // `--done` means the stream must actually end with `done`, or fast-import dumps a crash report.
            stream.write_all(b"done\n").expect("write the terminator");
            stream.flush().expect("flush fast-import");
        }
        let output = child.wait_with_output().expect("wait fast-import");
        assert!(
            output.status.success(),
            "fast-import failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        git(cwd, &["rev-parse", "HEAD"])
    }
}

/// Make `link` a symlink to `target` (relative, so the repository stays relocatable).
#[cfg(unix)]
pub fn symlink_to(dir: &Path, target: &str, link: &str) {
    std::os::unix::fs::symlink(target, dir.join(link)).expect("symlink");
}

/// On a platform where the test cannot make a symlink, a file whose contents are the target is the
/// closest stand-in, and the test that needs the real thing is unix-only.
#[cfg(not(unix))]
pub fn symlink_to(dir: &Path, target: &str, link: &str) {
    write(dir, link, target);
}

/// The outcome of a `git` run whose exit status is part of the assertion (a nonzero status is the
/// answer, not a failure: `git symbolic-ref -q HEAD` on a detached head).
#[must_use]
pub struct GitStatus {
    succeeded: bool,
    stdout: String,
    stderr: String,
}

impl GitStatus {
    /// Did the command fail?
    #[must_use]
    pub fn never_succeeded(&self) -> bool {
        !self.succeeded
    }

    /// What it printed, trimmed.
    #[must_use]
    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    /// What it complained about, trimmed — for a failure message that says why.
    #[must_use]
    pub fn stderr(&self) -> &str {
        &self.stderr
    }
}

/// Run `git` and return its stdout as *bytes*.
///
/// Needed by the unicode tests: `git()` lossy-converts, and a test that checks a byte-exact path
/// through a lossy conversion proves nothing.
#[must_use]
pub fn git_bytes(cwd: &Path, args: &[&str]) -> Vec<u8> {
    let output = git_command(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("running `git {}`: {err}", args.join(" ")));
    assert!(
        output.status.success(),
        "`git {}` in {} failed: {}",
        args.join(" "),
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

/// Run `git` without asserting on its exit status.
pub fn git_status(cwd: &Path, args: &[&str]) -> GitStatus {
    let output = git_command(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("running `git {}`: {err}", args.join(" ")));
    GitStatus {
        succeeded: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    }
}

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

    /// Commit one generated file of exactly `len` bytes, streamed, without this process holding it.
    ///
    /// The streaming is not a performance flourish: a memory test whose *fixture* allocates the same size as
    /// the object it measures lets the allocator hand the measured read a buffer the fixture just freed, and
    /// the peak never moves — the test then passes whether or not the code behaves. `git fast-import` takes
    /// the bytes in 64 KiB pieces, so the only large allocation left in the process is the one under test.
    pub fn commit_generated(
        &self,
        cwd: &Path,
        message: &str,
        path: &str,
        len: usize,
        epoch: i64,
        from: Option<&str>,
    ) -> String {
        let mut child = git_command(cwd)
            .args(["fast-import", "--quiet", "--done"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn git fast-import");
        {
            let stdin = child.stdin.take().expect("fast-import stdin");
            let mut stream = std::io::BufWriter::with_capacity(1 << 16, stdin);
            writeln!(stream, "commit refs/heads/main").expect("write fast-import");
            writeln!(
                stream,
                "committer Fathom Fixture <fixture@fathom.invalid> {epoch} +0000"
            )
            .expect("write fast-import");
            writeln!(stream, "data {}", message.len()).expect("write fast-import");
            stream
                .write_all(message.as_bytes())
                .expect("write fast-import");
            stream.write_all(b"\n").expect("write fast-import");
            if let Some(parent) = from {
                writeln!(stream, "from {parent}").expect("write fast-import");
            }
            writeln!(stream, "M 100644 inline {path}").expect("write fast-import");
            writeln!(stream, "data {len}").expect("write fast-import");
            let chunk = filler_bytes(1 << 16);
            let mut written = 0usize;
            while written < len {
                let take = (len - written).min(chunk.len());
                stream.write_all(&chunk[..take]).expect("write chunk");
                written += take;
            }
            stream.write_all(b"\n").expect("write terminator");
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

    /// Apply `changes` and commit them, with a pinned committer date; returns the new `HEAD`.
    ///
    /// The [`Change`] vocabulary exists so that a test about a rename, a mode change or a deletion is one
    /// line about it, rather than three lines of working-tree manipulation that hide which change kind is
    /// under test.
    pub fn commit_changes(
        &self,
        cwd: &Path,
        message: &str,
        when: &str,
        changes: &[Change],
    ) -> String {
        for change in changes {
            change.apply(cwd);
        }
        git(cwd, &["add", "--all"]);
        let output = git_command(cwd)
            .env("GIT_COMMITTER_DATE", when)
            .env("GIT_AUTHOR_DATE", when)
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

    /// A history of `count` commits containing every change kind the normalizer distinguishes.
    ///
    /// The rotation is the point. A golden file over a history where every commit rewrites `file.txt`
    /// pins nothing about renames, binary content, mode-only changes, multi-file commits, trailers,
    /// non-UTF-8 paths, oversized blobs or merges — and those are exactly the cases a normalizer gets
    /// wrong silently. Which rule fires is a function of the commit index, first match wins, so the graph
    /// is the same on every machine and on every run.
    ///
    /// Everything but the last four commits is written by `git fast-import`, which builds the object graph
    /// in a fraction of a second; the last four are real `git` commits, because a merge commit is the one
    /// shape `fast-import` will not build without a worktree to resolve it in, and two seconds of process
    /// startup buys the golden file's only merge coverage.
    pub fn mixed_history(&self, cwd: &Path, count: usize) -> String {
        assert!(count > 40, "the rotation needs room to rotate");
        self.fast_mixed_history(cwd, count - 4);
        // fast-import writes objects and refs and never touches the worktree; the merge needs one.
        git(cwd, &["checkout", "-q", "-f", "main"]);
        git(cwd, &["checkout", "-b", "side"]);
        self.commit_changes(
            cwd,
            "side: add the exporter",
            &date_at(i64::try_from(count - 3).unwrap_or(0)),
            &[Change::Add {
                path: "side/exporter.rs",
                lines: 12,
            }],
        );
        self.commit_changes(
            cwd,
            "side: cover the exporter",
            &date_at(i64::try_from(count - 2).unwrap_or(0)),
            &[Change::Add {
                path: "side/exporter_test.rs",
                lines: 8,
            }],
        );
        git(cwd, &["checkout", "-q", "-f", "main"]);
        self.commit_changes(
            cwd,
            "main: the last straight line",
            &date_at(i64::try_from(count - 1).unwrap_or(0)),
            &[Change::Add {
                path: "docs/tail.md",
                lines: 3,
            }],
        );
        let merged = git_command(cwd)
            .env(
                "GIT_COMMITTER_DATE",
                date_at(i64::try_from(count).unwrap_or(0)),
            )
            .env(
                "GIT_AUTHOR_DATE",
                date_at(i64::try_from(count).unwrap_or(0)),
            )
            .args([
                "merge",
                "--no-ff",
                "-q",
                "-m",
                "Merge branch 'side'",
                "side",
            ])
            .output()
            .expect("git merge");
        assert!(
            merged.status.success(),
            "git merge failed: {}",
            String::from_utf8_lossy(&merged.stderr)
        );
        git(cwd, &["rev-parse", "HEAD"])
    }

    /// A commit with no changes at all (`git commit --allow-empty`), pinned to `when`.
    ///
    /// Empty commits are common in real histories — a revert-and-revert, a hook, an agent retrying — and a
    /// normalizer that assumes "at least one file changed" fails on them.
    pub fn empty_commit(&self, cwd: &Path, message: &str, when: &str) -> String {
        let output = git_command(cwd)
            .env("GIT_COMMITTER_DATE", when)
            .env("GIT_AUTHOR_DATE", when)
            .args(["commit", "-q", "--allow-empty", "-m", message])
            .output()
            .expect("git commit");
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        git(cwd, &["rev-parse", "HEAD"])
    }

    /// The `fast-import` half of [`Self::mixed_history`].
    fn fast_mixed_history(&self, cwd: &Path, count: usize) -> String {
        let mut child = git_command(cwd)
            .args(["fast-import", "--quiet", "--done"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn git fast-import");
        {
            let stdin = child.stdin.take().expect("fast-import stdin");
            let mut stream = std::io::BufWriter::with_capacity(1 << 20, stdin);
            for index in 0..count {
                let message = if index % 23 == 9 {
                    // A body plus a trailer block: what an agent-written commit looks like.
                    format!(
                        "commit {index}: the long one\n\nA body that mentions Co-authored-by: nobody in \
                         prose, so the parser has to read the last paragraph and only that one.\n\n\
                         Co-authored-by: Ada Lovelace <ada@example.invalid>\n\
                         agent-session: s-{index}\nagent-turn: 3\n"
                    )
                } else {
                    format!("commit {index}")
                };
                write_commit_header(&mut stream, index, &message);
                if index % 7 == 3 {
                    // Stage the file the next commit renames, plus a neighbour that stays put so the
                    // pairing has something to reject. Same seed as the rename commit, or the two blobs
                    // differ on every line and nothing pairs.
                    let round = index / 7;
                    let content = lines_of("renamed/payload", 20);
                    write_inline(
                        &mut stream,
                        format!("renamed/source-{round}.txt").as_bytes(),
                        0o100644,
                        content.as_bytes(),
                    );
                    inline(&mut stream, &format!("renamed/neighbour-{round}.txt"), 4);
                } else if index % 7 == 4 {
                    // The rename itself: the *same bytes* under a new name, old name gone. The content is
                    // seeded on a fixed string rather than the path, or the two names would differ on every
                    // line and nothing would pair — which is a fact about similarity thresholds, not about
                    // renames, and this fixture is about renames.
                    let round = (index - 4) / 7;
                    let content = lines_of("renamed/payload", 20);
                    write_inline(
                        &mut stream,
                        format!("renamed/target-{round}.txt").as_bytes(),
                        0o100644,
                        content.as_bytes(),
                    );
                    delete(&mut stream, &format!("renamed/source-{round}.txt"));
                } else if index % 11 == 5 {
                    let round = (index - 5) / 11;
                    if round > 0 {
                        delete(&mut stream, &format!("scratch/gone-{}.txt", round - 1));
                    }
                    inline(&mut stream, &format!("scratch/gone-{round}.txt"), 6);
                } else if index % 13 == 6 {
                    // Binary content: a NUL inside the first kilobyte is git's rule, not ours alone.
                    binary(
                        &mut stream,
                        &format!("assets/blob-{}.bin", index / 13),
                        2048,
                    );
                } else if index % 17 == 7 {
                    // A mode-only change: identical content, mode flipping between 644 and 755 on
                    // alternate visits, so every one of these commits after the first changes only modes.
                    let mode = if (index / 17) % 2 == 0 {
                        0o100644
                    } else {
                        0o100755
                    };
                    inline_mode(&mut stream, "scripts/run.sh", 5, mode);
                } else if index % 19 == 8 {
                    for part in 0..5 {
                        inline(&mut stream, &format!("multi/part-{part}.rs"), 3 + index % 4);
                    }
                } else if index % 29 == 10 {
                    // A path git stores and Rust would not name: 0xff where UTF-8 should be.
                    let mut path = b"unicode/caf".to_vec();
                    path.push(0xff);
                    path.extend_from_slice(format!("-{index}.cpperc").as_bytes());
                    raw_inline(&mut stream, &path, b"non-utf8 path fixture\n");
                } else if index % 37 == 12 {
                    // A big text file under the cap: thousands of lines, all of them counted.
                    inline(&mut stream, &format!("big/text-{index}.md"), 4000);
                } else if index % 101 == 50 {
                    // Over the default 8 MiB cap, so the record has to say the content was not examined.
                    // Compresses to nothing, which is the only reason the fixture can afford it.
                    oversized(&mut stream, &format!("big/oversized-{index}.log"));
                } else {
                    inline(&mut stream, "file.txt", 1 + index % 5);
                }
            }
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

/// Start one commit in the `fast-import` stream, parented on the previous one.
fn write_commit_header<W: std::io::Write>(stream: &mut W, index: usize, message: &str) {
    writeln!(stream, "commit refs/heads/main").expect("write fast-import");
    writeln!(stream, "mark :{}", index + 1).expect("write fast-import");
    writeln!(
        stream,
        "committer Fathom Fixture <fixture@fathom.invalid> {} +0000",
        BASE_EPOCH + i64::try_from(index).unwrap_or(0)
    )
    .expect("write fast-import");
    writeln!(stream, "data {}", message.len()).expect("write fast-import");
    stream
        .write_all(message.as_bytes())
        .expect("write fast-import");
    stream.write_all(b"\n").expect("write fast-import");
    if index > 0 {
        writeln!(stream, "from :{index}").expect("write fast-import");
    }
}

/// `M <mode> inline <path>` with `lines` deterministic lines of content.
fn inline_mode<W: std::io::Write>(stream: &mut W, path: &str, lines: usize, mode: u32) {
    let content = lines_of(path, lines);
    write_inline(stream, path.as_bytes(), mode, content.as_bytes());
}

fn inline<W: std::io::Write>(stream: &mut W, path: &str, lines: usize) {
    inline_mode(stream, path, lines, 0o100644);
}

fn raw_inline<W: std::io::Write>(stream: &mut W, path: &[u8], content: &[u8]) {
    write_inline(stream, path, 0o100644, content);
}

fn write_inline<W: std::io::Write>(stream: &mut W, path: &[u8], mode: u32, content: &[u8]) {
    let mode = format!("{mode:o}");
    stream
        .write_all(format!("M {mode} inline ").as_bytes())
        .expect("write fast-import");
    stream.write_all(path).expect("write fast-import");
    stream.write_all(b"\n").expect("write fast-import");
    writeln!(stream, "data {}", content.len()).expect("write fast-import");
    stream.write_all(content).expect("write fast-import");
    stream.write_all(b"\n").expect("write fast-import");
}

fn delete<W: std::io::Write>(stream: &mut W, path: &str) {
    writeln!(stream, "D {path}").expect("write fast-import");
}

/// Deterministic text a fixture commits: no clock, no randomness, so a golden file can be compared byte
/// for byte tomorrow.
#[must_use]
pub fn lines_of(seed: &str, count: usize) -> String {
    let mut out = String::with_capacity(count.saturating_mul(24));
    for line in 0..count {
        out.push_str(&format!("// {seed} line {line}\n"));
    }
    out
}

/// Deterministic binary content: printable runs broken by a NUL every 512 bytes, which is what makes git
/// call a file binary.
#[must_use]
pub fn binary_bytes(len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    for index in 0..len {
        if index % 512 == 511 {
            out.push(0);
        } else {
            let byte = 0x20 + (index % 90);
            out.push(u8::try_from(byte).unwrap_or(b'x'));
        }
    }
    out
}

fn binary<W: std::io::Write>(stream: &mut W, path: &str, len: usize) {
    write_inline(stream, path.as_bytes(), 0o100644, &binary_bytes(len));
}

/// Content over the default 8 MiB blob cap, in the repeating shape that keeps the pack small.
#[must_use]
pub fn oversized_bytes() -> Vec<u8> {
    filler_bytes(9 * 1024 * 1024)
}

/// Deterministic text of exactly `len` bytes, made of one repeated line (truncated mid-line if need be,
/// which is what makes a "one byte over the cap" fixture possible). The repetition is the point:
/// the pack stays tiny, so a test about *decompression* is not also a test about disk throughput, while
/// the *inflated* size — which is what the ingest cap is measured against — is exactly as large as asked.
#[must_use]
pub fn filler_bytes(len: usize) -> Vec<u8> {
    let line = "the same line again and again so the pack stays small\n";
    let mut out = String::with_capacity(len + line.len());
    while out.len() < len {
        out.push_str(line);
    }
    out.truncate(len);
    out.into_bytes()
}

/// Text with a changed line every `spacing` lines: a file whose diff has `len / spacing` hunks, which is
/// the "tens of thousands of hunks" case the memory note in the ticket asks for.
#[must_use]
pub fn hunkye_lines(len: usize, spacing: usize, marked: bool) -> Vec<u8> {
    let mut out = String::with_capacity(len.saturating_mul(24));
    for line in 0..len {
        if marked && line % spacing == spacing - 1 {
            out.push_str(&format!("// changed line {line}\n"));
        } else {
            out.push_str(&format!("// steady line {line}\n"));
        }
    }
    out.into_bytes()
}

// ---------------------------------------------------------------------------
// Measuring this process, because "it did not allocate" has to be a number.
// ---------------------------------------------------------------------------

/// `VmHWM` from `/proc/self/status`: the high-water mark of resident memory, which is what "no allocation
/// blowup" means. `None` where `/proc` does not exist; callers skip the assertion there rather than
/// asserting a number they cannot read.
#[must_use]
pub fn peak_rss_kib() -> Option<u64> {
    proc_field("VmHWM")
}

/// `VmRSS`: what is resident right now, as opposed to the peak.
#[must_use]
pub fn current_rss_kib() -> Option<u64> {
    proc_field("VmRSS")
}

fn proc_field(name: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix(name) {
            let value = rest
                .trim_start_matches(':')
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()?;
            return Some(value);
        }
    }
    None
}

fn oversized<W: std::io::Write>(stream: &mut W, path: &str) {
    write_inline(stream, path.as_bytes(), 0o100644, &oversized_bytes());
}

/// One file-level change, for [`Fixture::commit_changes`].
#[derive(Debug)]
pub enum Change {
    /// A new file of `lines` deterministic lines.
    Add {
        /// Where it goes.
        path: &'static str,
        /// How many lines.
        lines: usize,
    },
    /// A new file with exactly these bytes — the binary and oversized cases.
    AddBytes {
        /// Where it goes.
        path: &'static str,
        /// What goes in it.
        bytes: Vec<u8>,
    },
    /// Content replaced wholesale.
    Rewrite {
        /// Which file.
        path: &'static str,
        /// How many lines now.
        lines: usize,
    },
    /// Lines appended to the end.
    Append {
        /// Which file.
        path: &'static str,
        /// How many to add.
        lines: usize,
    },
    /// Truncated to zero bytes.
    Empty {
        /// Which file.
        path: &'static str,
    },
    /// Removed from the tree.
    Delete {
        /// Which file.
        path: &'static str,
    },
    /// `chmod +x` and nothing else: a mode-only change.
    Exec {
        /// Which file.
        path: &'static str,
    },
    /// Moved, content left alone: the rename case.
    Rename {
        /// Where it was.
        from: &'static str,
        /// Where it is.
        to: &'static str,
    },
    /// A second name for the same bytes: the copy case.
    Copy {
        /// Where from.
        from: &'static str,
        /// Where to.
        to: &'static str,
    },
}

impl Change {
    fn apply(&self, cwd: &Path) {
        match self {
            Change::Add { path, lines } | Change::Rewrite { path, lines } => {
                write(cwd, path, &lines_of(path, *lines));
            }
            Change::AddBytes { path, bytes } => write_bytes(cwd, path.as_bytes(), bytes),
            Change::Append { path, lines } => {
                let mut text = std::fs::read_to_string(cwd.join(path)).unwrap_or_default();
                text.push_str(&lines_of(path, *lines));
                write(cwd, path, &text);
            }
            Change::Empty { path } => write(cwd, path, ""),
            Change::Delete { path } => {
                let file = cwd.join(path);
                assert!(file.exists(), "fixture: nothing to delete at {path}");
                std::fs::remove_file(file).expect("remove file");
            }
            Change::Exec { path } => make_executable(cwd, path),
            Change::Rename { from, to } => {
                let (old, new) = (cwd.join(from), cwd.join(to));
                if let Some(parent) = new.parent() {
                    std::fs::create_dir_all(parent).expect("create parent dir");
                }
                assert!(old.exists(), "fixture: nothing to rename at {from}");
                std::fs::rename(old, new).expect("rename file");
            }
            Change::Copy { from, to } => {
                let (old, new) = (cwd.join(from), cwd.join(to));
                if let Some(parent) = new.parent() {
                    std::fs::create_dir_all(parent).expect("create parent dir");
                }
                assert!(old.exists(), "fixture: nothing to copy at {from}");
                std::fs::copy(old, new).expect("copy file");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The durable half: the SQLite spine, written through `fathom_ingest::ingest::Sink`.
// ---------------------------------------------------------------------------

use fathom_ingest::export;
use fathom_ingest::ingest::{RunId, RunKind, RunReport, Sink};
use fathom_ingest::oid::Oid;
use fathom_ingest::record::CommitRecord;
use fathom_store::spine::{CommitRow, FileRow, RunStats, Store, Trigger};
use std::collections::HashSet;

/// The `CommitRecord` → `CommitRow` projection, which is the whole difference between the record and the
/// store: ADR-004 Appendix A keeps less than a record holds (no rename score, no hunk bodies, no per-file
/// "why was this not examined"), and everything the store *does* keep is here, unmodified.
///
/// `tests/store_round_trip.rs` reads this back out of SQLite and compares it against the record, so a field
/// that silently stops being persisted fails there rather than in a year.
#[must_use]
pub fn projection(record: &CommitRecord) -> CommitRow {
    CommitRow {
        oid: record.oid.hex(),
        tree_oid: record.tree.hex(),
        parent_oids: record.parents.iter().map(Oid::hex).collect(),
        author_name: record.author.name.clone(),
        author_email: record.author.email.clone(),
        // The schema's `*_utc` columns are unix milliseconds, the unit the reference implementation writes.
        author_utc: record.author.when.as_unix_micros() / 1_000,
        committer_utc: record.committer.when.as_unix_micros() / 1_000,
        committer_name: record.committer.name.clone(),
        committer_email: record.committer.email.clone(),
        message_head: record.message.head.clone(),
        message_hash: record.message.hash.clone(),
        trailers: record.trailers.to_map(),
        n_files: i64::try_from(record.totals.files).unwrap_or(i64::MAX),
        n_add: i64::try_from(record.totals.added).unwrap_or(i64::MAX),
        n_del: i64::try_from(record.totals.removed).unwrap_or(i64::MAX),
        n_binary: i64::try_from(record.totals.binary).unwrap_or(i64::MAX),
        oversized: record.oversized(),
        files: record.files.iter().map(file_projection).collect(),
    }
}

/// One file's projection. The path *after* the change is the identity (`git_commit_file` is keyed by it),
/// which is why a rename's old name rides along as data rather than as the key.
#[must_use]
pub fn file_projection(file: &fathom_ingest::record::FileRecord) -> FileRow {
    FileRow {
        path_hash: fathom_ingest::record::sha256_hex(file.path.as_bytes()),
        new_path: file.path.quoted(),
        old_path: file.old_path.as_ref().map(|path| path.quoted()),
        change_kind: file.change.as_str().to_owned(),
        mode_old: file.mode_old.map(i64::from),
        mode_new: file.mode_new.map(i64::from),
        is_binary: file.content.is_binary(),
        n_add: i64::from(file.added),
        n_del: i64::from(file.removed),
        hunk_headers: file
            .hunks
            .iter()
            .map(fathom_ingest::record::Hunk::header)
            .collect(),
        blob_old: file.blob_old.map(|oid| oid.hex()),
        blob_new: file.blob_new.map(|oid| oid.hex()),
        path_class: file.taxonomy.path_class.clone(),
        taxonomy_version: i64::from(file.taxonomy.taxonomy_version),
        noise_class: file.taxonomy.noise.as_str().to_owned(),
    }
}

/// A [`Sink`] that writes the spine. This is the shape `e1.10`'s composition root will build; it lives in the
/// test fixture because `fathom-ingest`'s library code must not name a database (ADR-002 §4.2), and a test
/// is exactly where the two are allowed to meet.
pub struct StoreSink<'a> {
    /// The open store.
    pub store: &'a mut Store,
    /// The instant every `*_utc` column gets, supplied rather than read (ADR-002 §4.4).
    pub now: i64,
    /// Fail this write (1-based) instead of writing, which is how a test says "the process died here"
    /// without forking a process to die.
    pub fail_on_write: Option<usize>,
    /// How many times [`Sink::write`] has been called, which is what `fail_on_write` counts against.
    pub writes: usize,
}

impl<'a> StoreSink<'a> {
    /// A sink that writes to `store` at a pinned instant and never fails.
    pub fn new(store: &'a mut Store) -> Self {
        StoreSink {
            store,
            now: 1_700_000_000_000,
            fail_on_write: None,
            writes: 0,
        }
    }
}

impl Sink for StoreSink<'_> {
    type Error = fathom_store::error::Error;

    fn begin(&mut self, kind: RunKind) -> Result<RunId, Self::Error> {
        self.store
            .begin_run(kind.as_str(), Trigger::Manual, self.now)
            .map(RunId)
    }

    fn known(&self, oids: &[Oid]) -> Result<HashSet<Oid>, Self::Error> {
        let hex: Vec<String> = oids.iter().map(Oid::hex).collect();
        let found = self.store.known_oids(&hex)?;
        Ok(hex
            .iter()
            .filter(|oid| found.contains(*oid))
            .filter_map(|oid| oid.parse::<Oid>().ok())
            .collect())
    }

    fn write(&mut self, run: RunId, batch: &[CommitRecord]) -> Result<usize, Self::Error> {
        self.writes += 1;
        if self.fail_on_write == Some(self.writes) {
            return Err(fathom_store::error::Error::StoreCorrupt {
                what: String::from("the sink gave up"),
            });
        }
        let rows: Vec<CommitRow> = batch.iter().map(projection).collect();
        self.store.write_commits(run.0, self.now, &rows)
    }

    fn finish(&mut self, run: RunId, report: &RunReport) -> Result<(), Self::Error> {
        self.store.finish_run(
            run.0,
            self.now,
            &RunStats {
                commits_seen: i64::try_from(report.commits_seen).unwrap_or(i64::MAX),
                commits_added: i64::try_from(report.commits_written).unwrap_or(i64::MAX),
                rows_written: i64::try_from(report.rows_written).unwrap_or(i64::MAX),
                interrupted: report.interrupted,
                error: report.failures.first().cloned(),
            },
        )
    }
}

/// The export bytes for a set of records, with the header that describes how they were made.
#[must_use]
pub fn export_text(records: &[CommitRecord]) -> String {
    export::to_json(
        records,
        &export::Normalization::of(&fathom_ingest::normalize::Options::default(), 0),
    )
    .expect("export")
}

/// A small repository with every change kind in it, for the tests that need breadth rather than volume.
///
/// Twelve commits, in a fixed order with dates that ascend, so a walk sees them newest-first and a golden
/// comparison has something to say about each kind.
pub fn mixed(fixture: &Fixture, cwd: &Path) {
    fixture.commit_changes(
        cwd,
        "the readme",
        &date_at(0),
        &[Change::Add {
            path: "README.md",
            lines: 2,
        }],
    );
    fixture.commit_changes(
        cwd,
        "a module, and a script",
        &date_at(1),
        &[
            Change::Add {
                path: "src/lib.rs",
                lines: 10,
            },
            Change::Add {
                path: "run.sh",
                lines: 3,
            },
        ],
    );
    fixture.commit_changes(
        cwd,
        "grow the module",
        &date_at(2),
        &[Change::Append {
            path: "src/lib.rs",
            lines: 4,
        }],
    );
    fixture.commit_changes(
        cwd,
        "make the script executable",
        &date_at(3),
        &[Change::Exec { path: "run.sh" }],
    );
    fixture.commit_changes(
        cwd,
        "move the module",
        &date_at(4),
        &[Change::Rename {
            from: "src/lib.rs",
            to: "src/core.rs",
        }],
    );
    fixture.commit_changes(
        cwd,
        "a picture is worth a kilobyte",
        &date_at(5),
        &[Change::AddBytes {
            path: "assets/logo.png",
            bytes: binary_bytes(2048),
        }],
    );
    fixture.commit_changes(
        cwd,
        "vendor a generated file",
        &date_at(6),
        &[Change::Add {
            path: "vendor/generated.js",
            lines: 30,
        }],
    );
    fixture.empty_commit(cwd, "an empty commit, which happens", &date_at(7));
    fixture.commit_changes(
        cwd,
        "drop the readme",
        &date_at(8),
        &[Change::Delete { path: "README.md" }],
    );
    fixture.commit_changes(
        cwd,
        "empty out the script",
        &date_at(9),
        &[Change::Empty { path: "run.sh" }],
    );
    fixture.commit_changes(
        cwd,
        "an overlong log",
        &date_at(10),
        &[Change::AddBytes {
            path: "logs/huge.log",
            bytes: oversized_bytes(),
        }],
    );
    fixture.commit_changes(
        cwd,
        "rewrite the module at both ends",
        &date_at(11),
        &[Change::Rewrite {
            path: "src/core.rs",
            lines: 7,
        }],
    );
}

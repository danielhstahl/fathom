//! A hostile repository: what a clone may and may not make this binary do (`e7.8`, `reco-41a`).
//!
//! The acceptance criterion this file exists for is one sentence in the ticket — *"cloning a hostile
//! repository cannot execute code or write outside `.fathom/`"* — and the honest version of that sentence
//! has three parts, each a section below ([threat model F-1, F-3, F-8](../../../docs/security/threat-model.md)):
//!
//! * **write containment.** Every path the verbs build is `root.join(".fathom")…`, so a `..` cannot appear;
//!   what a join cannot see is the filesystem, and a clone can ship a **symlink** (`git` stores symlinks as
//!   ordinary file content and `core.symlinks` is on by default). So the rule under test is about where a
//!   path *resolves*, not about how it was spelled.
//! * **nothing outside `.fathom/` at all**, asserted by inventory rather than by trust: the whole working
//!   tree is listed before a run and after it, and the only permitted difference is under `.fathom/`.
//! * **no code execution**, which for Fathom means git plumbing and nothing else: the binary has one place
//!   that runs a program at all ([`fathom_cli::handoff`]), and its program cannot come from the repository.
//!   The test here is the complement — a clone's own git hooks, the one program a clone can install that a
//!   repository-owning tool might plausibly run, is never run.
//!
//! Refusals are asserted as *refusals*: a non-zero published code, a sentence naming the path and where it
//! really resolved, a remedy, and — the part a containment check most often gets wrong — nothing written
//! before the refusal.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// The tests that index — including the containment ones, whose whole assertion is what `fathom index`
// refuses to follow — are `sqlite-bundled`-gated, with the fixture's `index` call and the symlink helpers
// they alone use: a build with no store refuses to index for a different reason, and the refusal under test
// is not the one it prints (ADR-002 §6.1). The two that never index stay ungated.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A scratch base for one scenario: a repository that was *cloned*, an "outside" directory that stands for
/// everything the clone must not reach, and a fake home so the machine's own config cannot join the test.
struct Hostile {
    base: PathBuf,
    repo: PathBuf,
    outside: PathBuf,
}

impl Hostile {
    fn new(tag: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("fathom-hostile-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        let outside = base.join("outside");
        std::fs::create_dir_all(repo.join("src")).expect("create the repository");
        std::fs::create_dir_all(&outside).expect("create the outside directory");
        std::fs::write(base.join("gitconfig"), b"").expect("empty gitconfig");
        let hostile = Hostile {
            base,
            repo,
            outside,
        };
        git(
            &hostile.repo,
            &["init", "-q", "--initial-branch=main"],
            &hostile.base,
        );
        git(
            &hostile.repo,
            &["config", "user.name", "Cloner Person"],
            &hostile.base,
        );
        git(
            &hostile.repo,
            &["config", "user.email", "cloner@example.invalid"],
            &hostile.base,
        );
        hostile.write("README.md", "# harbour\n");
        hostile.commit("chore: initialise");
        for subject in [
            "feat: narrow the booking query",
            "fix: review feedback on the query",
            "test: cover the booking query",
        ] {
            hostile.write("src/store.rs", &format!("// {subject}\n"));
            hostile.commit(subject);
        }
        hostile
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.repo.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("create the directory");
        std::fs::write(path, text).expect("write the file");
    }

    fn commit(&self, subject: &str) {
        git(&self.repo, &["add", "-A"], &self.base);
        git(&self.repo, &["commit", "-q", "-m", subject], &self.base);
    }

    fn fathom(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fathom"))
            .current_dir(&self.repo)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.base.join("home"))
            .env("XDG_CONFIG_HOME", self.base.join("home").join(".config"))
            .env("GIT_CONFIG_GLOBAL", self.base.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("TERM", "dumb")
            .env("NO_COLOR", "1")
            .output()
            .expect("the built fathom binary is runnable")
    }

    /// Everything under the working tree, as sorted relative paths. `.fathom` is excluded because the whole
    /// point is to notice a run writing *elsewhere*.
    fn inventory(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        walk(&self.repo, &self.repo, &mut out);
        out
    }

    #[cfg(feature = "sqlite-bundled")]
    fn index(&self) -> Output {
        self.fathom(&["index", "."])
    }
}

impl Drop for Hostile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let relative = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .display()
            .to_string();
        if relative.starts_with(".fathom") || relative.starts_with(".git/") || relative == ".git" {
            continue;
        }
        // A symlink is recorded as the link itself: following it here would make an escape look like the
        // repository's own content, which is the mistake the containment rule exists to avoid.
        out.insert(relative);
        if path.is_dir()
            && !path
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        {
            walk(base, &path, out);
        }
    }
}

fn git(working: &Path, args: &[&str], base: &Path) -> String {
    let out = Command::new("git")
        .current_dir(working)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", base.join("home"))
        .env("GIT_CONFIG_GLOBAL", base.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("fathom leaves a code, not a signal")
}

#[cfg(all(unix, feature = "sqlite-bundled"))]
fn symlink(target: &Path, link: &Path) {
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::os::unix::fs::symlink(target, link).expect("create the symlink");
}

#[cfg(all(not(unix), feature = "sqlite-bundled"))]
fn symlink(target: &Path, link: &Path) {
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::os::windows::fs::symlink_dir(target, link).expect("create the link");
}

/// The every-run assertion: a refusal must be a refusal, not a crash and not a usage error.
#[cfg(feature = "sqlite-bundled")]
fn assert_refused(out: &Output, what: &str) {
    let printed = text(out);
    assert_ne!(code(out), 0, "{what} should have been refused:\n{printed}");
    assert_ne!(
        code(out),
        2,
        "{what} was rejected as a bad command line:\n{printed}"
    );
    assert_ne!(code(out), 101, "{what} panicked:\n{printed}");
    assert!(
        !printed.contains("panicked at"),
        "{what} panicked:\n{printed}"
    );
}

// ---------------------------------------------------------------------------
// 1. Write containment: a clone's symlink is refused, not written through
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_fathom_directory_that_points_outside_the_repository_is_refused() {
    let hostile = Hostile::new("state-escape");
    symlink(&hostile.outside, &hostile.repo.join(".fathom"));

    let out = hostile.index();
    assert_refused(&out, "fathom index through a symlinked .fathom");
    let printed = text(&out);
    assert!(
        printed.contains("outside the repository"),
        "the refusal must name the containment rule:\n{printed}"
    );
    assert!(
        printed.contains(&hostile.outside.display().to_string()),
        "the refusal must say where the link goes:\n{printed}"
    );
    assert!(
        printed.contains("symlink"),
        "the remedy must name the thing to fix:\n{printed}"
    );
    let leftovers: Vec<_> = std::fs::read_dir(&hostile.outside)
        .expect("the outside directory still exists")
        .filter_map(|entry| entry.ok())
        .collect();
    assert!(
        leftovers.is_empty(),
        "nothing may be created outside the repository: {leftovers:?}"
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_ledger_that_points_outside_the_repository_is_never_appended_to() {
    // The case with teeth: the ledger write is an *append* of attacker-shaped text (episode id, actor,
    // note), so an escape here is "append to any file the reviewer can write" — `~/.ssh/authorized_keys`
    // is the spelling that makes the point.
    let hostile = Hostile::new("ledger-escape");
    assert_eq!(code(&hostile.index()), 0, "the clean index must work first");
    let authorized = hostile.outside.join("authorized_keys");
    std::fs::write(&authorized, "ssh-ed25519 AAAAexistingkey reviewer@host\n")
        .expect("seed the file");
    symlink(
        &hostile.outside,
        &hostile.repo.join(".fathom").join("reviews"),
    );
    let before = std::fs::read(&authorized).expect("the seeded file");

    let out = hostile.fathom(&["review", "sync"]);
    assert_refused(
        &out,
        "fathom review sync through a symlinked .fathom/reviews",
    );
    assert!(
        text(&out).contains("outside the repository"),
        "{}",
        text(&out)
    );
    assert_eq!(
        std::fs::read(&authorized).expect("the seeded file"),
        before,
        "an appended byte would mean the escape worked"
    );
    assert!(
        !hostile
            .repo
            .join(".fathom")
            .join("reviews")
            .join("ledger.md")
            .exists()
            || hostile
                .repo
                .join(".fathom")
                .join("reviews")
                .symlink_metadata()
                .map(|meta| meta.file_type().is_symlink())
                .unwrap_or(false),
        "no ledger may be created through the link"
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_link_that_stays_inside_the_repository_is_allowed() {
    // The rule is containment, not "no symlinks": a store moved to a bigger disk *inside* the working tree
    // is something people do on purpose, and refusing it would refuse the repository.
    let hostile = Hostile::new("state-inside");
    let real = hostile.repo.join("big-disk").join("state");
    std::fs::create_dir_all(&real).expect("create the real state directory");
    symlink(&real, &hostile.repo.join(".fathom"));

    let out = hostile.index();
    assert_eq!(
        code(&out),
        0,
        "an in-repository link must work:\n{}",
        text(&out)
    );
    assert!(
        real.join("index.sqlite3").exists(),
        "the store must land where the link points, inside the repository"
    );
}

// ---------------------------------------------------------------------------
// 2. Nothing outside `.fathom/` — asserted by inventory, on a repository that tries hard
// ---------------------------------------------------------------------------

#[test]
fn a_run_creates_nothing_outside_the_state_directory() {
    let hostile = Hostile::new("inventory");
    // A `fathom.toml` that is unfriendly in every way the config layer is asked to be tolerant of: bad
    // globs, `..` paths, absolute paths, unknown keys, a taxonomy class that names a system directory.
    hostile.write(
        "fathom.toml",
        r#"[noise]
globs = ["**a**a**a**a**a**a**a**a", "../../**", "/etc/**", "[]bad", "{unbalanced"]
keep = ["!../../escape"]

[test]
globs = ["**/*.rs", "/**/*"]

[scope]
roots = ["../../../../etc"]

[taxonomy.classes.generated]
data = ["../../etc/shadow"]

[paths]
store = "/etc/needswrite"
"#,
    );
    hostile.write("src/evil.rs", "// **a**a**a**a**a**a**a**a\n");
    hostile.commit("chore: a hostile configuration");
    let before = hostile.inventory();

    for args in [
        vec!["index", "."],
        vec!["review", "status"],
        vec!["doctor"],
        vec!["stats"],
    ] {
        let out = hostile.fathom(&args);
        assert_ne!(
            code(&out),
            101,
            "`fathom {}` panicked:\n{}",
            args.join(" "),
            text(&out)
        );
        assert_eq!(
            hostile.inventory(),
            before,
            "`fathom {}` created or changed something outside .fathom/",
            args.join(" ")
        );
    }
    assert!(
        std::fs::read_dir(&hostile.outside)
            .expect("the outside directory")
            .count()
            == 0,
        "nothing may be created outside the working tree either"
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_hostile_repository_leaves_the_reviewer_with_a_real_answer() {
    // Containment that turns a usable repository into an unusable one is not a control, it is an outage.
    // The same hostile configuration has to be *answered*: the verbs succeed and say what they ignored.
    let hostile = Hostile::new("answer");
    hostile.write(
        "fathom.toml",
        "[noise]\nglobs = [\"**a**a**a**a**a**a**a**a\", \"../../**\"]\n",
    );
    hostile.write("src/hostile_glob.rs", "// aaaaaaaaaaaaaaaaaaaaaab\n");
    hostile.commit("chore: a glob that would have hung the old matcher");
    let out = hostile.index();
    assert_eq!(code(&out), 0, "{}", text(&out));
    let review = hostile.fathom(&["review", "status", "--json"]);
    // 7 is "not everything is signed off", which is a verdict; 0 would mean it signed off its own work.
    assert!(
        matches!(code(&review), 0 | 7),
        "the verdict must be a verdict, not a crash:\n{}",
        text(&review)
    );
    assert!(
        text(&review).contains("episode"),
        "the verdict is still a verdict:\n{}",
        text(&review)
    );
}

// ---------------------------------------------------------------------------
// 3. No code execution: the clone gets to name data, not programs
// ---------------------------------------------------------------------------

#[test]
fn a_clone_owns_no_git_hook_that_fathom_runs() {
    // The one program a clone can install in a repository is a git hook, and the only way it runs is if a
    // tool invokes a git command that fires it. Everything Fathom runs is read-only plumbing (`log`,
    // `diff`, `rev-parse`, `config`), none of which fires a hook — but that is a property of the *command
    // list*, so it is asserted here rather than assumed, and a future `git checkout` or `git worktree`
    // added to that list will fail this test.
    let hostile = Hostile::new("hooks");
    let marker = hostile.base.join("hook-ran");
    for hook in ["pre-commit", "post-checkout", "reference-transaction"] {
        let path = hostile.repo.join(".git").join("hooks").join(hook);
        std::fs::create_dir_all(path.parent().expect("hooks directory")).expect("hooks directory");
        // The path is written into the script rather than passed in the environment: a hook is run by git
        // with a scrubbed environment of its own, and a fixture that relied on passing one would be a
        // fixture that could pass silently.
        std::fs::write(
            &path,
            format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
        )
        .expect("write the hook");
        #[cfg(unix)]
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("make the hook executable");
    }
    for args in [
        vec!["index", "."],
        vec!["review", "status"],
        vec!["review", "list"],
        vec!["doctor"],
    ] {
        let out = hostile.fathom(&args);
        assert_ne!(code(&out), 101, "`fathom {}` panicked", args.join(" "));
    }
    assert!(!marker.exists(), "a git hook from the clone was executed");
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_commit_message_carrying_a_command_is_still_just_text() {
    // The other classic: a commit subject that *is* a shell command. Fathom renders subjects into a
    // terminal and into the ledger; none of that is a shell, and this test is the cheap way to keep it
    // that way — the marker file is the tell.
    let hostile = Hostile::new("subject");
    let marker = hostile.base.join("subject-ran");
    hostile.write("src/quoted.rs", "fn quoted() {}\n");
    git(
        &hostile.repo,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            &format!("feat: touch {}", marker.display()),
            "-m",
            &format!(
                "Co-authored-by: touch {} <x@example.invalid>",
                marker.display()
            ),
        ],
        &hostile.base,
    );
    assert_eq!(code(&hostile.index()), 0);
    hostile.fathom(&["review", "status"]);
    hostile.fathom(&["episodes"]);
    assert!(
        !marker.exists(),
        "a commit subject was interpreted as a command"
    );
}

//! `fathom review`, driven as a binary (`reco-rlf`, `e4.6`).
//!
//! `crates/fathom-analyze/tests/ledger.rs` proves the ledger's rules — relink after a rewrite, staleness,
//! tampering, the file's shape. A library test cannot prove the four things a *verb* owns, and those are what
//! this file is for:
//!
//! * the exit codes, because a gate is only as good as the code it reads (`0` all reviewed, `7` not, per
//!   ADR-002 §7.3);
//! * that a refusal wrote nothing — a sign-off that failed for want of an identity must leave no partial
//!   entry behind, or the ledger records an act nobody made;
//! * that `--json` is one parseable value on stdout carrying the contract id, and that stdout holds nothing
//!   else (ADR-006 §10.1);
//! * that the identity resolution a human actually experiences — `--identity`, `$FATHOM_REVIEWER`, the
//!   repository's `fathom.toml`, git's own config — is the one documented in the verb's help.
//!
//! The fixture is four commits: one human, one agent turn (three commits, one episode, per ADR-003 §4). It is
//! deliberately small; the relink fixtures live where the relink logic does.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The reviewer this file signs as, in the spelling a git identity takes.
const DANA: &str = "Dana Ruiz <dana@example.invalid>";

/// `fathom …` inside `dir`, with the environment the reviewer's own shell would have — minus the one variable
/// each refusal test needs absent, which is removed by the caller that cares.
fn fathom(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .current_dir(dir)
        .args(args)
        .env_remove("FATHOM_REVIEWER")
        .output()
        .expect("the built fathom binary is runnable")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// `fathom …` with no inherited environment beyond `PATH`: the state of a machine that knows nothing about
/// the reviewer. A CI runner's `~/.gitconfig` has a `user.name` in it, and a test that inherits it never
/// actually asks the question it claims to ask.
fn naked(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("the built fathom binary is runnable")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

/// A git repository with an index, the way `fathom index` leaves it.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-review-cli-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"], &[]);
        git(&dir, &["config", "user.name", "Pi Agent"], &[]);
        git(
            &dir,
            &["config", "user.email", "agent@example.invalid"],
            &[],
        );
        let repo = Repo { dir };
        repo.write(&[("README.md", "# harbour\n")]);
        repo.commit("chore: initialise", &[("README.md", "# harbour\n")], 0);
        for (offset, subject) in [
            (0_i64, "feat: narrow the booking query"),
            (3, "fix: review feedback on the query"),
            (6, "test: cover the booking query"),
        ] {
            let minute = 60 + offset;
            repo.write(&[("src/store.rs", &format!("// {subject}\n"))]);
            repo.commit(
                subject,
                &[("src/store.rs", &format!("// {subject}\n"))],
                minute,
            );
        }
        let out = fathom(&repo.dir, &["index", "."]);
        assert_eq!(code(&out), 0, "indexing failed: {}", stderr(&out));
        repo
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn ledger(&self) -> PathBuf {
        self.dir.join(".fathom").join("reviews").join("ledger.md")
    }

    fn ledger_text(&self) -> String {
        std::fs::read_to_string(self.ledger()).expect("the ledger was written")
    }

    fn set_ledger(&self, text: &str) {
        std::fs::write(self.ledger(), text).expect("rewrite the ledger");
    }

    fn write(&self, files: &[(&str, &str)]) {
        for (path, body) in files {
            let full = self.dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create the file's directory");
            }
            std::fs::write(full, body).expect("write the fixture file");
        }
    }

    fn commit(&self, subject: &str, files: &[(&str, &str)], minute: i64) {
        self.write(files);
        for (path, _) in files {
            git(self.path(), &["add", "--", path], &[]);
        }
        git(
            self.path(),
            &["commit", "-q", "-m", subject],
            &[("MINUTE", &minute.to_string())],
        );
    }

    /// Commit the ledger itself — what makes it class M rather than a scratch file (ADR-004 §10.1).
    fn commit_ledger(&self) {
        git(self.path(), &["add", "--", ".fathom/reviews"], &[]);
        git(
            self.path(),
            &["commit", "-q", "-m", "docs: record the review ledger"],
            &[("MINUTE", "500")],
        );
    }

    /// Amend the tip with new content: the case a fresh sign-off must not survive.
    fn amend_content(&self) {
        self.write(&[("src/store.rs", "// changed after the review\n")]);
        git(self.path(), &["add", "--", "src/store.rs"], &[]);
        git(
            self.path(),
            &["commit", "-q", "--amend", "--no-edit"],
            &[("MINUTE", "400")],
        );
        let out = fathom(self.path(), &["index", "."]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
    }

    /// Re-index after any rewrite, so the ledger is re-linked against what git now says.
    fn reindex(&self) {
        let out = fathom(self.path(), &["index", "."]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
    }

    /// Every unsigned live episode the verdict named, newest first as printed.
    fn unsigned(&self) -> Vec<String> {
        let out = fathom(self.path(), &["review", "status", "--json"]);
        let payload: serde_json::Value =
            serde_json::from_str(&stdout(&out)).expect("status --json is one JSON value");
        payload["unsigned_examples"]
            .as_array()
            .expect("the payload names the unsigned episodes")
            .iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect()
    }

    /// Sign off every episode the verdict says is unsigned, and return how many were signed.
    fn sign_everything(&self) -> usize {
        let mut signed = 0;
        // Sign one at a time: an episode's sign-off can change nothing about the others, but the *list* is
        // re-read each pass, which is what a reviewer does and what keeps the loop honest.
        while let Some(episode) = self.unsigned().first().cloned() {
            let out = fathom(
                self.path(),
                &["review", "sign", &episode, "--identity", DANA, "--json"],
            );
            assert_eq!(code(&out), 0, "signing {episode} failed: {}", stderr(&out));
            signed += 1;
            if signed > 20 {
                panic!("the verdict never ran out of unsigned episodes; it is not converging");
            }
        }
        signed
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        if std::env::var_os("FATHOM_KEEP_FIXTURES").is_none() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// `git` with a scrubbed environment; `MINUTE` is this file's name for "commit at base + n minutes".
fn git(cwd: &Path, args: &[&str], extra: &[(&str, &str)]) {
    let minute: i64 = extra
        .iter()
        .find(|(key, _)| *key == "MINUTE")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let date = format!("@{} +0000", 1_700_000_000 + minute * 60);
    let output = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
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
}

/// AC: an unsigned repository is not "all reviewed", and it exits the code CI reads.
#[test]
fn an_unsigned_repository_exits_seven_and_says_which_episodes() {
    let repo = Repo::new("unsigned");
    let out = fathom(repo.path(), &["review", "status"]);
    assert_eq!(code(&out), 7, "{}", stdout(&out));
    let text = stdout(&out);
    assert!(
        text.contains("NOT ALL REVIEWED"),
        "the verdict line is the headline: {text}"
    );
    assert!(
        text.contains("no fresh sign-off"),
        "and the blocker names the reason: {text}"
    );
    assert!(
        !repo.ledger().exists(),
        "asking for a verdict must not create a ledger: {:?}",
        repo.ledger()
    );

    let json: serde_json::Value = serde_json::from_str(&stdout(&fathom(
        repo.path(),
        &["review", "status", "--json"],
    )))
    .expect("status --json parses");
    assert_eq!(json["contract"], "fathom.cli.review/1");
    assert_eq!(json["clean"], false);
    assert!(
        json["unsigned"].as_u64().unwrap_or(0) >= 2,
        "the human commit and the agent turn are both unsigned: {json}"
    );
    assert!(json["unsigned_examples"].is_array(), "{json}");
}

/// Bare `fathom review` is `review status`, because the thing a reviewer wants by default is the verdict.
#[test]
fn bare_review_is_status() {
    let repo = Repo::new("bare");
    let bare = fathom(repo.path(), &["review"]);
    let explicit = fathom(repo.path(), &["review", "status"]);
    assert_eq!(code(&bare), code(&explicit));
    assert_eq!(stdout(&bare), stdout(&explicit));
}

/// AC: every episode signed off, the ledger committed, and the verdict closes — and the commit that records
/// the reviews does not reopen them.
#[test]
fn a_signed_off_and_committed_ledger_closes_the_verdict() {
    let repo = Repo::new("clean");
    let signed = repo.sign_everything();
    assert!(
        signed >= 2,
        "the fixture has a human commit and an agent turn"
    );
    assert!(repo.ledger().exists(), "signing wrote the file");

    // Not yet committed: fresh, but not durable, so still not clean (ADR-004 §9.6).
    let out = fathom(repo.path(), &["review", "status"]);
    assert_eq!(code(&out), 7, "{}", stdout(&out));
    assert!(
        stdout(&out).contains("not committed"),
        "the blocker says the ledger is not committed: {}",
        stdout(&out)
    );

    repo.commit_ledger();
    repo.reindex();
    let out = fathom(repo.path(), &["review", "status"]);
    assert_eq!(
        code(&out),
        0,
        "committed and signed off means clean:\n{}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("ALL REVIEWED"));

    // And the JSON agrees with the exit code, which is the only reason a CI step can trust either.
    let json: serde_json::Value = serde_json::from_str(&stdout(&fathom(
        repo.path(),
        &["review", "status", "--json"],
    )))
    .expect("parses");
    assert_eq!(json["clean"], true, "{json}");
    assert_eq!(json["stale"], 0);
    assert_eq!(json["orphan"], 0);
    assert_eq!(json["tampered"], 0);
}

/// A sign-off without an identity is not a sign-off, and a refusal leaves no file behind.
#[test]
fn a_sign_off_without_an_identity_is_refused_and_writes_nothing() {
    let repo = Repo::new("noidentity");
    // A reviewer with no identity anywhere: no `--identity`, no `$FATHOM_REVIEWER`, and a git whose global
    // and system configs are out of the picture — which is the only way to ask the question honestly, because
    // a CI runner's `~/.gitconfig` otherwise answers it for you.
    git(
        repo.path(),
        &["config", "--local", "--unset", "user.name"],
        &[],
    );
    git(
        repo.path(),
        &["config", "--local", "--unset", "user.email"],
        &[],
    );
    let episode = repo
        .unsigned()
        .first()
        .cloned()
        .expect("the fixture has unsigned episodes");
    let out = naked(
        repo.path(),
        &["review", "sign", &episode, "--note", "looks fine"],
    );
    assert_ne!(
        code(&out),
        0,
        "signing without an identity must not succeed"
    );
    let message = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        message.to_lowercase().contains("identity"),
        "the refusal names the missing thing: {message}"
    );
    for knob in ["--identity", "FATHOM_REVIEWER", "fathom.toml"] {
        assert!(
            message.contains(knob),
            "and the remedy lists the knob `{knob}` that would fix it: {message}"
        );
    }
    assert!(
        !repo.ledger().exists(),
        "a refused sign-off wrote no ledger"
    );

    // The same command, with the flag supplying what git could not.
    let out = naked(
        repo.path(),
        &["review", "sign", &episode, "--identity", DANA],
    );
    assert_eq!(code(&out), 0, "{}{}", stdout(&out), stderr(&out));
    assert!(repo.ledger().exists());
    assert!(
        repo.ledger_text().contains(DANA),
        "the entry records who signed"
    );
}

/// `$FATHOM_REVIEWER` is the knob for a shell that already knows who you are.
#[test]
fn the_environment_can_supply_the_identity() {
    let repo = Repo::new("env");
    let episode = repo.unsigned().first().cloned().expect("unsigned");
    let out = Command::new(env!("CARGO_BIN_EXE_fathom"))
        .current_dir(repo.path())
        .args(["review", "sign", &episode])
        .env("FATHOM_REVIEWER", "Ana Ruiz <ana@example.invalid>")
        .output()
        .expect("the binary runs");
    assert_eq!(code(&out), 0, "{}{}", stdout(&out), stderr(&out));
    assert!(
        repo.ledger_text()
            .contains("Ana Ruiz <ana@example.invalid>"),
        "{}",
        repo.ledger_text()
    );
}

/// An unknown episode is refused with the list that would have helped, not a stack of nothing.
#[test]
fn an_episode_nothing_names_is_refused_with_a_reminder() {
    let repo = Repo::new("unknown");
    let out = fathom(
        repo.path(),
        &["review", "sign", "ep-doesnotexist", "--identity", DANA],
    );
    assert_ne!(code(&out), 0);
    assert!(
        format!("{}{}", stdout(&out), stderr(&out)).contains("names no episode"),
        "{}",
        stdout(&out)
    );
}

/// AC: tampering is detectable from the file alone — and `verify`, unlike `status`, never needed the index.
#[test]
fn an_edited_entry_fails_verify_and_the_verdict() {
    let repo = Repo::new("tamper");
    repo.sign_everything();
    let clean = fathom(repo.path(), &["review", "verify"]);
    assert_eq!(code(&clean), 0, "{}", stdout(&clean));
    assert!(
        stdout(&clean).contains("every entry hashes to its own digest"),
        "{}",
        stdout(&clean)
    );

    let text = repo.ledger_text();
    let edited = text.replacen("- decision: accepted", "- decision: dismissed", 1);
    assert_ne!(
        text, edited,
        "the fixture must have a decision to tamper with"
    );
    repo.set_ledger(&edited);

    let out = fathom(repo.path(), &["review", "verify"]);
    assert_eq!(code(&out), 7, "{}", stdout(&out));
    assert!(
        stdout(&out).contains("text changed after it was written"),
        "{}",
        stdout(&out)
    );

    let json: serde_json::Value = serde_json::from_str(&stdout(&fathom(
        repo.path(),
        &["review", "verify", "--json"],
    )))
    .expect("parses");
    assert_eq!(json["clean"], false, "{json}");
    assert!(
        json["complaints"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "{json}"
    );

    // And the gate sees it too: a tampered entry is not a review.
    let status = fathom(repo.path(), &["review", "status"]);
    assert_eq!(code(&status), 7);
    assert!(stdout(&status).contains("tampered"), "{}", stdout(&status));
}

/// Content that moved after the sign-off is stale, and `list --state stale` is the actionable view.
#[test]
fn content_changed_after_sign_off_shows_as_stale_in_list() {
    let repo = Repo::new("stale");
    repo.sign_everything();
    assert!(repo.ledger().exists());

    repo.amend_content();
    let out = fathom(repo.path(), &["review", "status"]);
    assert_eq!(code(&out), 7, "{}", stdout(&out));
    assert!(
        stdout(&out).contains("1 stale"),
        "the count moved, not just the wording: {}",
        stdout(&out)
    );

    let stale = fathom(repo.path(), &["review", "list", "--state", "stale"]);
    assert_eq!(code(&stale), 0);
    assert!(stdout(&stale).contains("STALE"), "{}", stdout(&stale));
    assert!(
        stdout(&stale).contains("src/store.rs"),
        "and it names the file that changed: {}",
        stdout(&stale)
    );
    assert!(
        !stdout(&stale).contains("fresh"),
        "the filter filtered: {}",
        stdout(&stale)
    );
}

/// `review sync` is the bridge from the queue's act log; with no acts it says so and writes nothing.
#[test]
fn sync_with_no_acts_renders_nothing_and_says_so() {
    let repo = Repo::new("sync");
    let out = fathom(repo.path(), &["review", "sync"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).starts_with("rendered 0 act(s)"),
        "{}",
        stdout(&out)
    );
    assert!(!repo.ledger().exists(), "nothing to render means no file");

    let json: serde_json::Value =
        serde_json::from_str(&stdout(&fathom(repo.path(), &["review", "sync", "--json"])))
            .expect("parses");
    assert_eq!(json["action"], "sync");
    assert_eq!(json["rendered"], 0);
}

/// A ledger git is *ignoring* is a different failure from a ledger git has not been asked to hold, and the
/// two need different remedies (ADR-004 §12.1: the required stanza is `.fathom/*` plus `!.fathom/reviews/`).
#[test]
fn an_ignored_ledger_says_so_rather_than_telling_you_to_commit_it() {
    let repo = Repo::new("ignored");
    repo.sign_everything();
    // The stanza a repository writes when it treats `.fathom/` as scratch — which is exactly the mistake
    // ADR-004 §12.1 says destroys the ledger on clone.
    repo.write(&[(".gitignore", ".fathom/*\n")]);
    git(repo.path(), &["add", "--", ".gitignore"], &[]);
    git(
        repo.path(),
        &["commit", "-q", "-m", "chore: ignore the scratch directory"],
        &[("MINUTE", "300")],
    );

    let out = fathom(repo.path(), &["review", "status"]);
    assert_eq!(code(&out), 7, "{}", stdout(&out));
    let text = stdout(&out);
    assert!(
        text.contains("is ignored by git"),
        "the finding is the ignore, not a missing commit: {text}"
    );
    assert!(
        text.contains("!.fathom/reviews/"),
        "and the remedy is the line that fixes it: {text}"
    );
    assert!(
        !text.contains("is not committed"),
        "it must not also tell you to commit a file git refuses to add: {text}"
    );

    // Fix the stanza, and the same verdict asks for the commit instead.
    repo.write(&[(".gitignore", ".fathom/*\n!.fathom/reviews/\n")]);
    let out = fathom(repo.path(), &["review", "status"]);
    assert_eq!(code(&out), 7);
    assert!(
        stdout(&out).contains("is not committed"),
        "{}",
        stdout(&out)
    );

    // And once it is committed but with uncommitted changes, that is the finding.
    repo.commit_ledger();
    repo.set_ledger(&format!("{}\n", repo.ledger_text()));
    let out = fathom(repo.path(), &["review", "status"]);
    assert_eq!(code(&out), 7);
    assert!(
        stdout(&out).contains("changes that are not committed"),
        "{}",
        stdout(&out)
    );
}

/// `fathom review --help` is the documentation, so the subverbs have to be in it.
#[test]
fn the_help_names_every_subverb() {
    let out = fathom(repo_dir(), &["review", "--help"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    for verb in ["status", "sign", "sync", "list", "verify"] {
        assert!(
            text.contains(&format!("  {verb}")),
            "`{verb}` is not in `fathom review --help`:\n{text}"
        );
    }
}

/// A directory that is not a repository, for the help test, which needs no index.
fn repo_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

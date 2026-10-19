//! `fathom drift`, driven as a binary (`e4.8`, `reco-cpr`).
//!
//! `crates/fathom-analyze/tests/drift.rs` proves the rules. This file checks the verb, which owns four
//! things the library cannot:
//!
//! * **the read** — an index that does not exist is exit 12 with the remedy, and an index with episodes is a
//!   report whose first line says what was compared;
//! * **the intent column, for real** — the plan the report quotes is the one `e4.2` wrote into the store,
//!   through the JSON round trip no library fixture exercises;
//! * **the machine contract** — `--json` is one parseable value carrying `fathom-drift/1`, its policy and
//!   its six tallies;
//! * **the gate** — `--fail-on-findings` is exit 7 and nothing else is, and `--dismiss` survives the round
//!   trip from one run's output into the next run's flag.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

struct Repo {
    dir: PathBuf,
}

impl Repo {
    /// One agent turn that stayed inside its declared scope, then a turn that wandered: `src/pool/**` was
    /// what the intent bounded, and the last commit touched `docs/runbook.md` and a manifest.
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-drift-cli-{label}-{}-{index}",
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
        repo.commit(
            "chore: initialise harbour",
            &[("README.md", "# harbour\n")],
            0,
        );
        for (minute, subject, files) in [
            (
                180,
                "feat: split the worker pool",
                vec![("src/pool/worker.rs", "fn worker() {}\n")],
            ),
            (
                186,
                "feat: route the clone off the pool",
                vec![("src/pool/clone.rs", "fn clone() {}\n")],
            ),
            (
                192,
                "chore: note the new pool in the runbook",
                vec![
                    ("docs/runbook.md", "# runbook\n"),
                    ("Cargo.toml", "[package]\nname = \"harbour\"\n"),
                ],
            ),
        ] {
            repo.commit(
                &format!(
                    "{subject}\n\nAgent-Intent: split the worker pool so a slow clone cannot starve the rail\nAgent-Scope: src/pool/**\nCo-authored-by: looprs <looprs@example.invalid>"
                ),
                &files,
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

    fn commit(&self, message: &str, files: &[(&str, &str)], minute: i64) {
        for (path, body) in files {
            let full = self.dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create the file's directory");
            }
            std::fs::write(full, body).expect("write the fixture file");
            git(self.path(), &["add", "--", path], &[]);
        }
        git(
            self.path(),
            &["commit", "-q", "-m", message],
            &[("MINUTE", &minute.to_string())],
        );
    }
}

fn git(dir: &Path, args: &[&str], extra_env: &[(&str, &str)]) {
    let minute: i64 = extra_env
        .iter()
        .find(|(key, _)| *key == "MINUTE")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let date = format!("@{} +0000", 1_700_000_000 + minute * 60);
    let mut command = Command::new("git");
    command.args(args).current_dir(dir);
    command.env("GIT_AUTHOR_DATE", &date);
    command.env("GIT_COMMITTER_DATE", &date);
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("HOME", dir);
    command.env("LC_ALL", "C");
    let out = command.output().expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn drift_without_an_index_refuses_with_the_remedy() {
    let dir = std::env::temp_dir().join(format!("fathom-drift-cli-noindex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    let out = fathom(&dir, &["drift"]);
    assert_eq!(code(&out), 12, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("fathom index"),
        "a refusal without a remedy is a dead end: {}",
        stderr(&out)
    );
}

#[test]
fn the_report_prints_both_sides_of_what_went_outside_the_plan() {
    let repo = Repo::new("print");
    let out = fathom(repo.path(), &["drift"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("fathom-drift/1"), "{text}");
    assert!(text.contains("policy:"), "{text}");
    assert!(
        text.contains("said") && text.contains("history"),
        "a contradiction with one side is an opinion: {text}"
    );
    assert!(
        text.contains("docs/runbook.md"),
        "the path outside the declared scope is named: {text}"
    );
    assert!(
        text.contains("src/pool/**"),
        "the glob it was outside is quoted beside it: {text}"
    );
    assert!(
        text.contains("dismiss with `drift:"),
        "every finding says how to stop seeing it: {text}"
    );
}

#[test]
fn the_json_is_one_value_carrying_the_contract_the_policy_and_the_six_kinds() {
    let repo = Repo::new("json");
    let out = fathom(repo.path(), &["drift", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "one JSON value on stdout: {text}");
    let value: serde_json::Value = serde_json::from_str(lines[0]).expect("`--json` is JSON");
    assert_eq!(value["contract"], "fathom-drift/1");
    assert_eq!(value["schema_version"], 1);
    let policy = value["policy"]
        .as_array()
        .expect("the policy the report ran under");
    let row = |key: &str| {
        policy
            .iter()
            .find(|pair| pair[0].as_str() == Some(key))
            .map(|pair| pair[1].as_str().unwrap_or_default().to_owned())
    };
    assert_eq!(row("min_overlap").as_deref(), Some("0.34"), "{value}");
    assert_eq!(row("window").as_deref(), Some("4"), "{value}");
    assert_eq!(value["tallies"].as_array().map(Vec::len), Some(6));
    let findings = value["findings"].as_array().expect("a findings array");
    assert!(
        !findings.is_empty(),
        "the fixture wandered out of its scope: {value}"
    );
    for finding in findings {
        assert_eq!(finding["owner"], "e4.8");
        assert_eq!(finding["rule_version"], 1);
        assert!(
            finding["claim"]["address"]
                .as_str()
                .is_some_and(|address| address.starts_with("fathom")),
            "side A is an address: {finding}"
        );
        assert!(
            finding["evidence"]["address"]
                .as_str()
                .is_some_and(|address| address.starts_with("fathom")),
            "side B is an address: {finding}"
        );
        assert!(
            finding["suppression_key"]
                .as_str()
                .is_some_and(|key| key.starts_with("drift:")),
            "{finding}"
        );
        let confidence = finding["confidence"].as_f64().expect("a priced finding");
        assert!((0.0..=0.95).contains(&confidence), "{confidence}");
    }
    assert!(
        value["unchecked"]
            .as_array()
            .is_some_and(|skips| !skips.is_empty()),
        "the episodes that stated no plan are counted, not ignored: {value}"
    );
}

#[test]
fn the_gate_is_one_bit_and_everything_else_is_zero() {
    let repo = Repo::new("gate");
    assert_eq!(code(&fathom(repo.path(), &["drift"])), 0);
    let out = fathom(repo.path(), &["drift", "--fail-on-findings"]);
    assert_eq!(code(&out), 7, "{}", stderr(&out));
    // The same run with the finding's kind switched off is a clean run, which is what proves the 7 came
    // from the finding and not from the verb failing to read anything.
    let out = fathom(
        repo.path(),
        &[
            "drift",
            "--fail-on-findings",
            "--off",
            "out-of-scope-edit",
            "--off",
            "unplanned-dependency",
        ],
    );
    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
}

#[test]
fn a_dismissal_carries_from_one_run_to_the_next() {
    let repo = Repo::new("dismiss");
    let first = fathom(repo.path(), &["drift", "--json"]);
    let value: serde_json::Value =
        serde_json::from_str(stdout(&first).lines().next().expect("one line")).expect("JSON");
    let key = value["findings"][0]["suppression_key"]
        .as_str()
        .expect("a key")
        .to_owned();

    let second = fathom(repo.path(), &["drift", "--dismiss", &key, "--json"]);
    let again: serde_json::Value =
        serde_json::from_str(stdout(&second).lines().next().expect("one line")).expect("JSON");
    let keys: Vec<&str> = again["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter_map(|finding| finding["suppression_key"].as_str())
        .collect();
    assert!(!keys.contains(&key.as_str()), "{key} came back: {keys:?}");
    assert!(
        again["suppressed"]
            .as_array()
            .is_some_and(|suppressed| suppressed.contains(&serde_json::Value::String(key.clone()))),
        "counted rather than dropped: {again}"
    );

    let text = fathom(repo.path(), &["drift", "--dismiss", &key]);
    assert!(
        stdout(&text).contains("dismissed, not re-reported"),
        "{}",
        stdout(&text)
    );
}

#[test]
fn the_window_and_the_cap_are_flags_and_the_report_shows_which_ones_ran() {
    let repo = Repo::new("flags");
    let out = fathom(
        repo.path(),
        &[
            "drift",
            "--window",
            "1",
            "--min-overlap",
            "0.9",
            "--max",
            "1",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("window = 1"), "{text}");
    assert!(text.contains("min_overlap = 0.90"), "{text}");
    assert!(text.contains("max_findings = 1"), "{text}");
    let findings = text.lines().filter(|line| line.starts_with("  [")).count();
    assert!(findings <= 1, "the cap is a cap: {text}");
}

#[test]
fn drift_help_is_part_of_the_documented_surface() {
    let out = fathom(repo_none(), &["drift", "--help"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    for flag in [
        "--session",
        "--window",
        "--min-overlap",
        "--dismiss",
        "--off",
        "--fail-on-findings",
    ] {
        assert!(text.contains(flag), "{flag} is not in the help:\n{text}");
    }
}

/// `--help` needs no repository at all, which is worth asserting on its own.
fn repo_none() -> &'static Path {
    Path::new(".")
}

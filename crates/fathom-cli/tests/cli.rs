//! The binary, from the outside — this file *is* acceptance criterion 4 of `e0.7` (reco-e77):
//! "`fathom --version` and `fathom --help` work from the built binary", plus the exit-code contract the
//! scripts in `e6.6` will depend on (ADR-002 §7.3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::{Command, Output};

fn fathom(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
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

#[test]
fn version_works_and_names_the_resolved_feature_set() {
    let out = fathom(&["--version"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("fathom "), "{text}");
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
    assert!(
        text.contains("features:"),
        "ADR-002 §6.4: a bug report has to say what was built — {text}"
    );
    assert_eq!(fathom(&["-V"]).status.code().map(|c| c == 0), Some(true));
}

#[test]
fn help_works_and_lists_the_verbs() {
    let out = fathom(&["--help"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    for verb in [
        "index", "show", "stats", "check", "doctor", "export", "mcp", "watch",
    ] {
        assert!(
            text.contains(&format!("\n  {verb}")),
            "{verb} missing from --help:\n{text}"
        );
    }
}

#[test]
fn a_rejected_command_line_exits_two() {
    // `USAGE` (ADR-002 §7.3): clap owns this code and fathom does not fight it.
    assert_eq!(code(&fathom(&["--not-a-real-flag"])), 2);
    assert_eq!(code(&fathom(&["no-such-verb"])), 2);
}

#[test]
fn an_unimplemented_verb_refuses_and_names_its_ticket() {
    let out = fathom(&["check"]);
    assert_eq!(
        code(&out),
        1,
        "a typed failure is ERROR, not a panic: {}",
        stderr(&out)
    );
    let text = stderr(&out);
    assert!(
        text.contains("e6.6"),
        "the refusal points at the owner: {text}"
    );
    assert!(
        text.contains("CHECK_VIOLATIONS") || text.contains("ERROR"),
        "{text}"
    );
    assert!(!stdout(&out).contains("error"), "diagnosis goes to stderr");
}

#[test]
fn export_uses_the_facades_format_list_and_reaches_the_facades_code() {
    let bad = fathom(&["export", "--format", "yaml"]);
    assert_eq!(
        code(&bad),
        2,
        "--format yaml is a usage error: {}",
        stderr(&bad)
    );
    let good = fathom(&["export", "--format", "md"]);
    assert_eq!(code(&good), 1, "{}", stderr(&good));
    assert!(stderr(&good).contains("e5.7"), "{}", stderr(&good));
}

#[test]
fn doctor_reports_this_build() {
    let out = fathom(&["doctor", "--privacy"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("sqlite:"), "{text}");
    for key in [
        "privacy.advisories",
        "privacy.llm",
        "privacy.mcp_http_transport",
    ] {
        assert!(
            text.contains(key),
            "{key} missing from doctor --privacy:\n{text}"
        );
    }
    // With `--all-features` the net features *are* compiled in, and the report has to say so: the whole
    // point of `doctor --privacy` is that a user can tell which of the two gates is holding (ADR-002 §6.3).
    #[cfg(not(feature = "net-advisories"))]
    assert!(
        text.contains("none — no HTTP client"),
        "a default build reports zero outbound features: {text}"
    );
    #[cfg(feature = "net-advisories")]
    assert!(
        text.contains("outbound features compiled in: net-advisories"),
        "a build with the net features reports them: {text}"
    );
}

#[test]
fn doctor_json_is_json_and_says_the_same_thing() {
    let out = fathom(&["doctor", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("doctor --json prints one JSON value");
    assert_eq!(parsed["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(parsed["views_ready"].as_bool(), Some(false));
    let features = parsed["features"].as_array().expect("a feature array");
    // §6.4's promise is that the list is the *resolved* set, so it has to be read against whichever
    // configuration this test binary was built in: the default set under `cargo test --workspace`,
    // `minimal` under the headless job §8.3 runs (§6.1).
    #[cfg(feature = "sqlite-bundled")]
    assert!(
        features.contains(&serde_json::json!("sqlite-bundled")),
        "the shipped default set is what this test built: {features:?}"
    );
    #[cfg(not(feature = "sqlite-bundled"))]
    assert!(
        features.contains(&serde_json::json!("minimal")),
        "the headless set is what this test built: {features:?}"
    );
    assert_eq!(parsed["network_features"].as_array().map(Vec::len), Some(3));
}

/// A throwaway repository with three commits, for the verbs that read a real one.
///
/// Built with the `git` binary rather than with `gix` because the point of these tests is the process
/// boundary: the fixture has to be a repository the way a user's is, warts and index file and all.
#[cfg(feature = "sqlite-bundled")]
fn repository() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-cli-index-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    for n in 0..3 {
        std::fs::write(dir.join("src.rs"), format!("fn f{n}() -> u32 {{ {n} }}\n"))
            .expect("write the fixture file");
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", &format!("commit {n}")]);
    }
    dir
}

/// One `git` call, with the identity and the dates pinned so two runs of this file are the same repository.
#[cfg(feature = "sqlite-bundled")]
fn git(cwd: &std::path::Path, args: &[&str]) {
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
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn index_writes_the_store_where_adr_004_puts_it() {
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    let out = fathom(&["index", root]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        dir.join(".fathom").join("index.sqlite3").exists(),
        "ADR-004 §3: the store is `.fathom/index.sqlite3` under the repository root"
    );
    assert!(stdout(&out).contains("3 new"), "{}", stdout(&out));

    // Warm, and with the table acceptance criterion four asks for: one row per analyzer, with counters
    // that say the second run did not recompute the first one's work. `--refresh` looks up every unit the
    // store holds — a warm run with nothing new looks up nothing at all, which is the other promise
    // (§6.1 R1) and is tested at the library level rather than here.
    let stats = fathom(&["index", root, "--stats", "--refresh"]);
    assert_eq!(code(&stats), 0, "{}", stderr(&stats));
    let text = stdout(&stats);
    assert!(text.contains("ANALYZER"), "{text}");
    for id in ["ingest.normalize", "diff.per_file", "structure.delta"] {
        assert!(text.contains(id), "{id} missing from --stats:\n{text}");
    }
    assert!(
        text.matches("100.0%").count() >= 3,
        "every analyzer found its whole answer on a repository that had not changed:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn index_json_is_one_object_on_stdout_and_nothing_else_there() {
    // ADR-002 §7.5: everything a human must act on goes to stderr, so `fathom index --json | jq` parses.
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    fathom(&["index", root]);
    let out = fathom(&["index", root, "--json", "--refresh"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let parsed: serde_json::Value =
        serde_json::from_str(stdout(&out).trim()).expect("one JSON value on stdout");
    assert_eq!(parsed["kind"].as_str(), Some("warm"));
    assert_eq!(parsed["commits"]["new"].as_u64(), Some(0));
    assert_eq!(parsed["interrupted"].as_bool(), Some(false));
    let analyzers = parsed["analyzers"].as_array().expect("an analyzer array");
    assert_eq!(analyzers.len(), 3, "one row per analyzer this build runs");
    let per_file = analyzers
        .iter()
        .find(|row| row["id"].as_str() == Some("diff.per_file"))
        .expect("diff.per_file is in the report");
    assert!(per_file["lookups"].as_u64().unwrap_or_default() >= 3);
    assert_eq!(
        per_file["hit_rate"].as_f64(),
        Some(100.0),
        "and the numbers are the store's, not a parallel set: {parsed}"
    );
    assert!(parsed["failures"].as_array().is_some_and(|v| v.is_empty()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn indexing_a_directory_that_is_not_a_repository_exits_three() {
    // ADR-002 §7.3: the exit code is the contract CI branches on, and §4 says `3` is "not a repository".
    let dir = std::env::temp_dir().join(format!("fathom-cli-not-a-repo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create a plain directory");
    let out = fathom(&["index", dir.to_str().expect("utf-8 path")]);
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    assert!(stderr(&out).contains("code:"), "{}", stderr(&out));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(feature = "sqlite-bundled"))]
#[test]
fn index_names_the_feature_a_minimal_binary_is_missing() {
    // The `minimal` binary has no SQLite and says so with the code that means "rebuild differently"
    // rather than pretending to have indexed something (ADR-002 §6.1, §8.3's headless job).
    let out = fathom(&["index", "."]);
    assert_eq!(code(&out), 9, "{}", stderr(&out));
    assert!(stderr(&out).contains("sqlite-bundled"), "{}", stderr(&out));
}

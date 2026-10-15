//! The binary, from the outside — this file *is* acceptance criterion 4 of `e0.7` (reco-e77):
//! "`fathom --version` and `fathom --help` work from the built binary", plus the exit-code contract the
//! scripts in `e6.6` will depend on (ADR-002 §7.3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::BufRead;
use std::process::{Command, Output, Stdio};

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

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_root_of_dot_is_the_repository_here() {
    // `fathom index .` is the sentence a user types, and `fathom watch` defaults its root to exactly that.
    // Collapsing `.` lexically used to leave *nothing*, which the working-tree reader then read as "there is
    // no work tree at " — an empty path, and a failure no user can act on.
    let dir = repository();
    let out = Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(["index", "."])
        .current_dir(&dir)
        .output()
        .expect("the built fathom binary is runnable");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("3 new"), "{}", stdout(&out));
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

/// One `fathom watch` run whose stdout is a stream of JSON objects, one per state change.
#[cfg(feature = "watch")]
fn watch_json_lines(dir: &std::path::Path, args: &[&str]) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fathom watch starts")
}

/// Parse one printed state line. `fathom watch --json` promises one object per line, so a line that does
/// not parse is the contract failing and is asserted as such rather than skipped.
#[cfg(feature = "watch")]
fn state_of(line: &str) -> serde_json::Value {
    serde_json::from_str(line.trim())
        .unwrap_or_else(|err| panic!("not a JSON state line {line:?}: {err}"))
}

/// Read state lines until `seen` says we have what we came for. The child always has a `--deadline`, so the
/// read cannot hang: the watcher exits, the pipe closes, and the iterator ends.
#[cfg(feature = "watch")]
fn read_until(
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    seen: impl Fn(&serde_json::Value) -> bool,
) -> Option<serde_json::Value> {
    for line in lines.by_ref() {
        let line = line.expect("stdout is utf-8 JSON");
        let value = state_of(&line);
        if seen(&value) {
            return Some(value);
        }
    }
    None
}

/// `fathom watch`, run the way a user runs it: no arguments, from inside the repository.
#[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
#[test]
fn a_commit_made_in_another_terminal_is_named_and_indexed() {
    // Acceptance criterion 1, end to end through the real binary: a second process commits, and the watcher
    // names the ref move and puts the commit in the store without being restarted.
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    let mut child = watch_json_lines(&dir, &["watch", ".", "--json", "--deadline", "120"]);
    let pipe = child.stdout.take().expect("piped stdout");
    let mut lines = std::io::BufReader::new(pipe).lines();

    // The startup refresh runs first: a watcher that showed yesterday's timeline until the next commit
    // would be trusted for one commit too long.
    let baseline = read_until(&mut lines, |value| value["phase"] == "idle")
        .expect("the initial refresh reports idle before anything changes");
    let already = baseline["counters"]["commits_indexed"]
        .as_u64()
        .unwrap_or(0);
    assert_eq!(
        already, 3,
        "the three fixture commits were indexed at startup: {baseline}"
    );

    std::fs::write(dir.join("src.rs"), "fn f3() -> u32 { 3 }\n").expect("write the new file");
    git(&dir, &["add", "-A"]);
    let started = std::time::Instant::now();
    git(&dir, &["commit", "-q", "-m", "made in the other terminal"]);

    let named = read_until(&mut lines, |value| {
        value["counters"]["commits_indexed"].as_u64().unwrap_or(0) > already
    })
    .expect("the watcher names the commit the other process made");
    let elapsed = started.elapsed();
    // The deadline is there to bound a *read*, not to be the test's runtime: stop the watcher now.
    let _ = child.kill();
    let _ = child.wait();

    let change = &named["last_change"];
    assert_eq!(change["name"].as_str(), Some("HEAD"), "{change}");
    assert_eq!(change["kind"].as_str(), Some("moved"), "{change}");
    assert_ne!(
        change["from"].as_str(),
        change["to"].as_str(),
        "a ref that did not move is not a change: {change}"
    );
    assert_eq!(named["counters"]["errors"].as_u64(), Some(0), "{named}");
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "detection took {elapsed:?}, which is not the 1s the criterion asks for — the budget itself is
         asserted at the library level, release-only; this is the loose process-level bound"
    );

    // And it is really *in the store*, not merely announced: a plain index now finds nothing new.
    let after = fathom(&["index", root, "--json"]);
    assert_eq!(code(&after), 0, "{}", stderr(&after));
    let parsed = state_of(&stdout(&after));
    assert_eq!(
        parsed["commits"]["new"].as_u64(),
        Some(0),
        "the watcher had already indexed the commit: {parsed}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
#[test]
fn watch_writes_a_heartbeat_and_doctor_reads_it() {
    // The heartbeat is the cheap half of the daemon contract (`reco-kvk` formalises it) and the thing that
    // makes criterion four checkable: `fathom doctor` reports the watcher and its backend from a file, with
    // no socket and no lock.
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    let out = fathom(&["watch", root, "--once", "--quiet"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(stdout(&out), "", "--quiet prints nothing on stdout");

    let heartbeat = dir.join(".fathom").join("watch").join("status.json");
    let written = std::fs::read_to_string(&heartbeat)
        .unwrap_or_else(|err| panic!("{} should exist: {err}", heartbeat.display()));
    let status = state_of(&written);
    assert_eq!(status["phase"].as_str(), Some("idle"), "{status}");
    assert_eq!(status["counters"]["commits_indexed"].as_u64(), Some(3));
    assert!(
        status["backend"]["name"]
            .as_str()
            .is_some_and(|name| !name.is_empty()),
        "a watcher that cannot say where its signals come from is a watcher nobody trusts: {status}"
    );

    // The watcher has exited, so doctor must report a stopped one rather than a live one: a heartbeat whose
    // process is gone is "stale", said out loud (a `kill -9` leaves the file behind).
    let text = fathom(&["doctor", "--root", root]);
    assert_eq!(code(&text), 0, "{}", stderr(&text));
    let printed = stdout(&text);
    assert!(printed.contains("watch: backend here"), "{printed}");
    assert!(printed.contains("a watcher stopped"), "{printed}");

    let json = fathom(&["doctor", "--root", root, "--json"]);
    assert_eq!(code(&json), 0, "{}", stderr(&json));
    let report = state_of(&stdout(&json));
    assert_eq!(report["watch"]["repository"].as_bool(), Some(true));
    assert_eq!(report["watch"]["running"]["alive"].as_bool(), Some(false));
    assert!(
        report["watch"]["backend"]["kind"].as_str().is_some(),
        "doctor --json has to name the backend the probe chose: {report}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
#[test]
fn watching_a_quiet_repository_costs_a_fraction_of_a_core() {
    // Acceptance criterion two, measured on the real binary rather than on a scripted job: `watch.idle_cpu.pct`
    // is 1% of one core, and the number asserted here is the watcher's own meter — the same figure it prints
    // and the same one `fathom doctor` can be asked about — read out of the heartbeat after four quiet seconds.
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    let out = fathom(&["watch", root, "--quiet", "--deadline", "4"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let written = std::fs::read_to_string(dir.join(".fathom").join("watch").join("status.json"))
        .expect("the run wrote its heartbeat");
    let status = state_of(&written);
    let ticks = status["counters"]["poll_ticks"].as_u64().unwrap_or(0);
    assert!(
        ticks >= 3,
        "a watcher that never polled is not idle, it is stopped: {status}"
    );
    let share = status["cpu_share"].as_f64().unwrap_or(1.0) * 100.0;
    assert!(
        share < 1.0,
        "a quiet repository cost {share:.3}% of a core against the 1% budget watch.idle_cpu.pct: {status}"
    );
    assert_eq!(status["counters"]["errors"].as_u64(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(all(feature = "watch", feature = "sqlite-bundled"))]
#[test]
fn watch_answers_poll_and_refuses_a_backend_that_does_not_exist() {
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");

    // `--backend poll` is the escape hatch for the filesystems where notifications lie, and it has to be
    // visible in the state it reports, not silently honoured.
    let polled = fathom(&[
        "watch",
        root,
        "--once",
        "--json",
        "--backend",
        "poll",
        "--interval-ms",
        "50",
    ]);
    assert_eq!(code(&polled), 0, "{}", stderr(&polled));
    let printed = stdout(&polled);
    let last = printed
        .lines()
        .last()
        .expect("the run printed its state at least once");
    let final_state = state_of(last);
    assert_eq!(final_state["backend"]["kind"].as_str(), Some("polling"));
    assert_eq!(
        final_state["backend"]["reason"].as_str(),
        Some("asked for with `--backend poll`"),
        "the reason a fallback happened is part of the answer: {final_state}"
    );

    let typo = fathom(&["watch", root, "--backend", "sideways"]);
    assert_eq!(code(&typo), 2, "{}", stderr(&typo));
    assert!(stderr(&typo).contains("USAGE"), "{}", stderr(&typo));

    // The heartbeat is opt-out, and a script that wants stdout only takes it. A previous run in this same
    // fixture wrote one, so clear the way first: the claim is "this run wrote no file", not "none exists".
    let heartbeat = dir.join(".fathom").join("watch").join("status.json");
    let _ = std::fs::remove_file(&heartbeat);
    let bare = fathom(&["watch", root, "--once", "--quiet", "--no-status-file"]);
    assert_eq!(code(&bare), 0, "{}", stderr(&bare));
    assert!(
        !heartbeat.exists(),
        "--no-status-file means this run writes no heartbeat: {}",
        heartbeat.display()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "watch")]
#[test]
fn watching_a_directory_that_is_not_a_repository_exits_three() {
    let dir = std::env::temp_dir().join(format!("fathom-cli-watch-plain-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create a plain directory");
    let out = fathom(&["watch", dir.to_str().expect("utf-8 path")]);
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("NOT_A_REPOSITORY"),
        "{}",
        stderr(&out)
    );

    // `doctor` is the verb that explains this, and it has to explain it rather than crash.
    let doctor = fathom(&["doctor", "--root", dir.to_str().expect("utf-8 path")]);
    assert_eq!(code(&doctor), 0, "{}", stderr(&doctor));
    assert!(
        stdout(&doctor).contains("is not a git repository"),
        "{}",
        stdout(&doctor)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(feature = "watch"))]
#[test]
fn watch_names_the_feature_a_build_without_it_is_missing() {
    let out = fathom(&["watch", "."]);
    assert_eq!(code(&out), 9, "{}", stderr(&out));
    assert!(stderr(&out).contains("watch"), "{}", stderr(&out));
    // The same gate, reported: doctor must not promise a watcher this binary cannot run.
    let doctor = fathom(&["doctor"]);
    assert_eq!(code(&doctor), 0, "{}", stderr(&doctor));
    assert!(
        stdout(&doctor).contains("not compiled"),
        "{}",
        stdout(&doctor)
    );
}

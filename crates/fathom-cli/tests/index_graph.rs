//! `fathom index` writing the module graph, driven as a binary (`e3.11`, `reco-dlp`).
//!
//! Epic 3 built the lane, the scorecard, the ratchet and the boundary scan on top of `graph_snapshot` rows
//! that nothing in the shipped binary wrote: `graph::Lane::record` had a test and no producer, so every
//! architecture surface answered "this index holds no module graph" at the end of a successful `fathom
//! index`. These tests are the claim that it no longer does, in the only place the claim can be made — the
//! binary, on a real repository:
//!
//! * **the index writes the lane**, and the surfaces that read it stop refusing (`architecture` goes from
//!   exit 12 to a scorecard);
//! * **the JSON says what was written** (`graph.revisions`, `graph.edges`, `graph.partial`), because a CI
//!   gate that cannot tell "nothing owed" from "refused" will gate on the wrong one;
//! * **every refusal is a named choice** — `--no-graph`, `graph.enabled = false`, `graph.window`,
//!   `graph.max_corpus_mib` — printed as itself rather than as an empty lane;
//! * **a warm run owes nothing and says so**, and a store that has never had a lane gets one from an ordinary
//!   quiet run, which is how an index upgraded by tomorrow's binary catches up without a special command;
//! * **`--refresh` re-derives the lane**, because ADR-004 §9.5's rule for derived data is to recompute it.

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

fn git(root: &Path, args: &[&str], when: &str) {
    let out = Command::new("git")
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Rue Reviewer")
        .env("GIT_AUTHOR_EMAIL", "rue@example.invalid")
        .env("GIT_COMMITTER_NAME", "Rue Reviewer")
        .env("GIT_COMMITTER_EMAIL", "rue@example.invalid")
        .env("GIT_AUTHOR_DATE", when)
        .env("GIT_COMMITTER_DATE", when)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repository of Python modules: one package per layer, an import that crosses them, and a cycle that
/// closes on the last commit.
///
/// Python rather than Rust because the import statement says what it means in one line: `from pkg.mod
/// import name` resolves against the corpus at the granularity `graph.module_granularity` picks, which is
/// the whole mechanism the graph is made of, and a fixture that needs four `use` lines and a `mod` to say
/// "this module depends on that one" is a fixture people misread.
struct Repo {
    dir: PathBuf,
    config: String,
}

impl Repo {
    fn new(label: &str) -> Repo {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-index-graph-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"], WHEN[0]);
        let exclude = dir.join(".git").join("info").join("exclude");
        std::fs::create_dir_all(exclude.parent().expect("git made info/")).expect("info/");
        std::fs::write(&exclude, ".fathom/\n").expect("write the local exclude");
        let repo = Repo {
            dir,
            config: String::new(),
        };
        // Commit 1: two packages, one dependency, no cycle.
        repo.write("pkg/__init__.py", "");
        repo.write("pkg/util.py", "HELPER = 1\n");
        repo.write(
            "pkg/core.py",
            "from pkg.util import HELPER\n\n\nclass Thing:\n    size = HELPER\n",
        );
        repo.commit(0, "add core on top of util");
        // Commit 2: a third package that reaches into core.
        repo.write(
            "pkg/api.py",
            "from pkg.core import Thing\n\n\ndef serve():\n    return Thing()\n",
        );
        repo.commit(1, "add the api on top of core");
        // Commit 3: util reaches back up to api. Now the lane has a cycle to find.
        repo.write(
            "pkg/util.py",
            "from pkg.api import serve\n\nHELPER = 2\n\n\ndef loop():\n    return serve()\n",
        );
        repo.commit(2, "close the loop between util and api");
        repo
    }

    fn write(&self, path: &str, contents: &str) {
        let full = self.dir.join(path);
        std::fs::create_dir_all(full.parent().expect("a path with a parent")).expect("mkdir");
        std::fs::write(full, contents).expect("write the fixture file");
    }

    fn commit(&self, n: usize, subject: &str) {
        git(&self.dir, &["add", "-A"], WHEN[n]);
        git(&self.dir, &["commit", "-q", "-m", subject], WHEN[n]);
    }

    /// Replace the repository's `fathom.toml` and commit it, so the config is part of the history the
    /// graph is built from rather than an uncommitted stray.
    fn set_config(&mut self, config: &str) {
        self.config = config.to_string();
        std::fs::write(self.dir.join("fathom.toml"), config).expect("write fathom.toml");
        git(&self.dir, &["add", "-A"], WHEN[2]);
        git(
            &self.dir,
            &["commit", "-q", "-m", "describe the layers"],
            WHEN[2],
        );
    }

    fn index(&self, args: &[&str]) -> Output {
        let mut all = vec!["index", "."];
        all.extend_from_slice(args);
        fathom(&self.dir, &all)
    }

    fn architecture(&self, args: &[&str]) -> Output {
        let mut all = vec!["architecture"];
        all.extend_from_slice(args);
        fathom(&self.dir, &all)
    }

    /// `fathom index --json`, parsed. Every test that asserts a number goes through this rather than
    /// grepping prose, because prose is allowed to be rewritten and a contract is not.
    fn index_json(&self, args: &[&str]) -> serde_json::Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let out = self.index(&all);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        serde_json::from_str(&stdout(&out)).expect("the index prints one JSON value")
    }
}

const WHEN: [&str; 3] = [
    "2026-06-01T09:00:00+0000",
    "2026-06-02T10:00:00+0000",
    "2026-06-03T11:00:00+0000",
];

/// **The ticket's sentence.** After `fathom index`, the architecture surfaces have a graph to read.
#[test]
fn an_index_writes_the_module_graph_the_architecture_surfaces_read() {
    let repo = Repo::new("writes");
    let out = repo.index(&[]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("module graph"),
        "the run says it wrote a graph: {text}\n{}",
        stderr(&out)
    );
    assert!(
        !text.contains("no module graph"),
        "the report line is not an apology: {text}"
    );

    // And the surface that was refusing now answers. `architecture` exits 12 when the lane is empty; a 0
    // here is the whole difference between Epic 3 being reachable from `fathom index` and not.
    let arch = repo.architecture(&["--plain"]);
    assert_eq!(code(&arch), 0, "{}", stderr(&arch));
    let text = stdout(&arch);
    assert!(
        !text.contains("no module graph"),
        "the scorecard still says the lane is empty: {text}"
    );
    assert!(text.contains("coupling"), "a scorecard was printed: {text}");
}

/// The numbers, as data: `fathom index --json`'s `graph` section, which is what a CI gate reads.
#[test]
fn the_index_json_counts_the_revisions_modules_and_edges_it_wrote() {
    let repo = Repo::new("json");
    let value = repo.index_json(&[]);
    let graph = &value["graph"];
    assert!(!graph.is_null(), "the pass ran: {value}");
    // Three commits, three revisions. The count is per revision, not per changed file, which is the point
    // of a per-revision graph.
    assert_eq!(graph["revisions"].as_u64(), Some(3), "{graph}");
    assert!(
        graph["modules"].as_u64().unwrap_or(0) >= 3,
        "three packages, at least three modules: {graph}"
    );
    assert!(
        graph["edges"].as_u64().unwrap_or(0) >= 3,
        "the imports in the fixture became edges: {graph}"
    );
    assert_eq!(
        graph["unavailable"].as_u64(),
        Some(0),
        "nothing was refused, so nothing is a hole in the graph: {graph}"
    );
    assert_eq!(graph["partial"], serde_json::Value::Null, "{graph}");
    assert!(
        value["phases_ms"].get("graph").is_some(),
        "the pass is its own phase, so a slow index can be argued about: {}",
        value["phases_ms"]
    );
}

/// A warm run has nothing to write, and does not pretend it did work.
#[test]
fn a_warm_index_owes_no_graph_and_says_nothing_about_it() {
    let repo = Repo::new("warm");
    let first = repo.index_json(&[]);
    assert_eq!(first["graph"]["revisions"].as_u64(), Some(3));

    let out = repo.index(&[]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        !stdout(&out).contains("module graph"),
        "a run that wrote no graph rows does not print a graph line: {}",
        stdout(&out)
    );

    let second = repo.index_json(&[]);
    assert_eq!(second["graph"]["revisions"].as_u64(), Some(0));
    assert_eq!(second["graph"]["partial"], serde_json::Value::Null);
    assert_eq!(
        second["graph"]["reset_rows"].as_u64(),
        Some(0),
        "nothing was thrown away"
    );
}

/// **A store that never had a lane gets one from an ordinary run.** This is the upgrade path: an index built
/// by yesterday's binary is quiet today, and a quiet run that skipped the backfill would leave every
/// architecture surface refusing forever.
#[test]
fn a_quiet_run_backfills_a_lane_that_was_never_written() {
    let repo = Repo::new("backfill");
    let before = repo.index_json(&["--no-graph"]);
    assert!(
        before["graph"].is_null(),
        "--no-graph means the pass did not run: {before}"
    );
    assert_eq!(
        code(&repo.architecture(&[])),
        12,
        "and the surface says so rather than scoring nothing"
    );

    // The next run is quiet — nothing is new, no ref moved — and it still owes three revisions of graph.
    let out = repo.index(&["--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("index --json");
    assert_eq!(
        value["nothing_new"].as_bool(),
        Some(true),
        "this really was the quiet path: {value}"
    );
    assert_eq!(value["graph"]["revisions"].as_u64(), Some(3));
    assert_eq!(code(&repo.architecture(&[])), 0);
}

/// `--no-graph` is a stated refusal with a sentence attached, not a silent omission.
#[test]
fn no_graph_says_which_flag_refused_and_leaves_the_lane_empty() {
    let repo = Repo::new("noflag");
    let out = repo.index(&["--no-graph"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stderr(&out) + &stdout(&out);
    assert!(
        text.contains("--no-graph"),
        "the refusal names the flag: {text}"
    );

    let json = repo.index_json(&["--no-graph"]);
    assert!(json["graph"].is_null(), "{json}");
    let arch = repo.architecture(&[]);
    assert_eq!(code(&arch), 12);
    assert!(
        stderr(&arch).contains("no module graph"),
        "the surfaces keep refusing, honestly: {}",
        stderr(&arch)
    );
}

/// `graph.enabled = false` is the durable form of the same refusal, and the index says which key did it.
#[test]
fn a_repository_can_switch_the_pass_off_and_says_so() {
    let mut repo = Repo::new("disabled");
    repo.set_config("[graph]\nenabled = false\n");
    let out = repo.index(&[]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out) + &stderr(&out);
    assert!(
        text.contains("graph.enabled"),
        "the notice names the key that refused: {text}"
    );
    let json = repo.index_json(&[]);
    assert!(json["graph"].is_null(), "{json}");
}

/// `graph.window` is a cost bound, and a pass that stopped early has to say how much is left — otherwise a
/// partial lane reads as a finished one.
#[test]
fn a_window_that_stops_short_says_what_is_left() {
    let mut repo = Repo::new("window");
    repo.set_config("[graph]\nwindow = 1\n");
    let value = repo.index_json(&[]);
    assert_eq!(
        value["graph"]["revisions"].as_u64(),
        Some(1),
        "one revision per pass, oldest-first, so the tail is what gets written: {}",
        value["graph"]
    );
    let partial = value["graph"]["partial"]
        .as_str()
        .unwrap_or_else(|| panic!("a capped pass reports its partial: {}", value["graph"]));
    assert!(
        partial.contains("graph.window") && partial.contains("older"),
        "the sentence names the knob and says what is behind it: {partial}"
    );
    // The newest revision is the one a reviewer asks about, so a capped window takes the newest and leaves
    // the ancients for later runs — never the other way round, which would spend every run on history.
    assert_eq!(
        value["graph"]["anchors"].as_u64(),
        Some(1),
        "one revision, therefore one anchor: {}",
        value["graph"]
    );

    // And the rest is owed to the next runs: the window is a per-pass cap, not a retention policy.
    let mut runs = 1;
    let mut wrote = value["graph"]["revisions"].as_u64().unwrap_or(0);
    let total = wrote;
    loop {
        let next = repo.index_json(&[]);
        let more = next["graph"]["revisions"].as_u64().unwrap_or(0);
        runs += 1;
        if more == 0 {
            break;
        }
        wrote += more;
        assert_eq!(
            more, 1,
            "one revision per run while the window is 1: {}",
            next["graph"]
        );
        assert!(runs < 12, "the window never let the lane catch up");
    }
    assert!(
        wrote > total,
        "the quiet runs finished what the first one capped: {wrote} of {total}"
    );
}

/// A corpus over `graph.max_corpus_mib` is a refusal, not a slow surprise: the pass declines before it
/// reads anything, and says which knob did it.
#[test]
fn a_corpus_over_the_cap_is_refused_by_name_before_anything_is_read() {
    let mut repo = Repo::new("corpus");
    // Over the 1 MiB cap the pass is allowed to read, and under `extract.max_file_bytes`, so the refusal
    // can only have come from the corpus cap and not from the per-file one.
    repo.write("pkg/big.py", &format!("# {}\n", "x".repeat(1_400_000)));
    git(&repo.dir, &["add", "-A"], WHEN[2]);
    git(
        &repo.dir,
        &[
            "commit",
            "-q",
            "-m",
            "a module too big to be worth extracting",
        ],
        WHEN[2],
    );
    repo.set_config("[graph]\nmax_corpus_mib = 1\n");
    let value = repo.index_json(&[]);
    // The pass writes oldest first, and the corpus grows with the history: the revisions before the big
    // file are under the cap and are written, and the pass stops where the source gets too big.
    let wrote = value["graph"]["revisions"].as_u64().unwrap_or(99);
    assert!(
        (1..=3).contains(&wrote),
        "the small revisions were written and the big one refused: {}",
        value["graph"]
    );
    let partial = value["graph"]["partial"]
        .as_str()
        .unwrap_or_else(|| panic!("a refused pass reports its partial: {}", value["graph"]));
    assert!(
        partial.contains("graph.max_corpus_mib"),
        "the refusal names the knob: {partial}"
    );
    // The refusal is printed on the human path too — a run that silently wrote no graph is the failure
    // mode this whole ticket is about.
    let plain = repo.index(&[]);
    assert!(
        (stdout(&plain) + &stderr(&plain)).contains("graph.max_corpus_mib"),
        "{}",
        stdout(&plain)
    );
}

/// `--refresh` re-derives the lane, which is ADR-004 §9.5's rule for derived data: delete and recompute,
/// never patch a delta chain by hand.
#[test]
fn refresh_derives_the_lane_again_from_scratch() {
    let repo = Repo::new("refresh");
    assert_eq!(code(&repo.index(&[])), 0);
    let out = repo.index(&["--refresh", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("index --json");
    assert!(
        value["graph"]["reset_rows"]
            .as_u64()
            .is_some_and(|rows| rows > 0),
        "the old rows were counted as deleted: {}",
        value["graph"]
    );
    assert_eq!(
        value["graph"]["revisions"].as_u64(),
        Some(3),
        "and all three were derived again: {}",
        value["graph"]
    );
    // Exactly one anchor. The reset emptied the lane, so the first revision has no base and must be a full
    // manifest; the two after it chain onto the row this same pass just wrote, which is what keeps a
    // refresh the same cost as the cold index it replaces rather than N anchors.
    assert_eq!(
        value["graph"]["anchors"].as_u64(),
        Some(1),
        "{}",
        value["graph"]
    );
}

/// The layering and cycle halves of `fathom check` read the same lane. Before `reco-dlp` they were always
/// `blocked`; now they judge, on a repository whose only crime is an import.
#[test]
fn check_judges_layering_and_cycles_from_an_ordinary_index() {
    let mut repo = Repo::new("check");
    repo.set_config(
        "[layers]\nbase = [\"pkg/util\"]\ntop = [\"pkg/api\", \"pkg/core\"]\n\n\
         [layering]\norder = [\"base\", \"top\"]\nno_upward = true\n",
    );
    assert_eq!(code(&repo.index(&[])), 0, "{}", stderr(&repo.index(&[])));
    let out = fathom(
        &repo.dir,
        &[
            "check", "--base", "HEAD~1", "--head", "HEAD", "--format", "json",
        ],
    );
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap_or_else(|_| {
        panic!(
            "check printed JSON on stdout: {} / {}",
            stdout(&out),
            stderr(&out)
        )
    });
    let rows = value["checks"].as_array().expect("rows");
    for name in ["layering", "cycles"] {
        let row = rows
            .iter()
            .find(|row| row["check"] == name)
            .expect("the row");
        assert_ne!(
            row["status"].as_str(),
            Some("blocked"),
            "{name} had a lane to read and still said it had nothing: {row}"
        );
    }
}

//! The out-of-tree example's own contract (`e6.7`, `reco-raw`).
//!
//! Two things are being tested here, and the second is the one CI exists for.
//!
//! 1. **The two detectors say what they claim to.** Each fires on the document it is for and stays silent
//!    on the four shapes nearby: a human author, a test file that arrived, a change under the floor. A
//!    detector that fires on everything is a detector somebody mutes within a week.
//! 2. **The crate builds and runs out-of-tree.** These tests compile against `fathom_analyze::plugin` and
//!    nothing deeper, read a blessed `fathom analyze --json` document from `fixtures/`, and drive the
//!    compiled binary the way a reviewer would — including `--check`, which is how the repository blesses
//!    a third party's output without importing the third party's source.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use acme_detectors::{UNSEEN_REACH, UNTESTED_AGENT_LOGIC, UnseenReach, UntestedAgentLogic};
use fathom_analyze::detectors::Severity;
use fathom_analyze::detectors::policy::Selection;
use fathom_analyze::insight::{self, EpisodeDelta, EpisodeRef, FileDelta, Input, Provenance};
use fathom_analyze::plugin::{Input as PluginInput, Limits, Registry};
use std::process::Command;
use std::sync::Arc;

fn fixture(name: &str) -> EpisodeDelta {
    let path = format!("{}/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"),);
    let text = std::fs::read_to_string(&path).expect("a committed fixture");
    serde_json::from_str(&text).expect("a fixture is a document `fathom analyze --json` printed")
}

fn write_fixture(name: &str, delta: &EpisodeDelta) {
    let path = format!("{}/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        &path,
        format!("{}\n", serde_json::to_string(delta).expect("serialisable")),
    )
    .expect("writable");
}

/// A synthetic document: one author kind, N source files, M test files, no history.
fn authored(author_kind: &str, source: &[(&str, i64)], tests: &[(&str, i64)]) -> EpisodeDelta {
    let mut input = Input::new(EpisodeRef::sparse(
        "ep-synthetic",
        "an episode a test wrote",
    ));
    input.episode.author_kind = author_kind.to_owned();
    let mut files: Vec<FileDelta> = Vec::new();
    for (path, add) in source.iter().chain(tests.iter()) {
        let test = tests.iter().any(|(test_path, _)| test_path == path);
        files.push(FileDelta {
            path: (*path).to_owned(),
            old_path: None,
            change: "added".to_owned(),
            add: *add,
            del: 0,
            binary: false,
            commits: 1,
            path_class: if test { "test" } else { "source" }.to_owned(),
            noise_class: "none".to_owned(),
            surface: "unclassified".to_owned(),
            basis: Provenance::DETAIL_FULL.to_owned(),
        });
    }
    input.totals.n_files = files.len() as i64;
    input.totals.n_add = files.iter().map(|file| file.add).sum();
    input.totals.n_del = 0;
    input.files = files;
    insight::compose(&input)
}

fn registry(detectors: Vec<Arc<dyn fathom_analyze::plugin::Detector>>) -> Registry {
    let mut registry = Registry::new();
    registry.register_all(detectors);
    registry
}

fn both() -> Vec<Arc<dyn fathom_analyze::plugin::Detector>> {
    vec![
        Arc::new(UntestedAgentLogic::default()),
        Arc::new(UnseenReach::default()),
    ]
}

fn on() -> Selection {
    Selection::parse(&format!("+{UNTESTED_AGENT_LOGIC},+{UNSEEN_REACH}"))
}

fn run(
    detectors: Vec<Arc<dyn fathom_analyze::plugin::Detector>>,
    delta: &EpisodeDelta,
) -> fathom_analyze::plugin::Report {
    registry(detectors).run(&PluginInput::new(delta), &on(), &Limits::default())
}

// --- what the binary prints, on documents the product printed ------------------------------------

#[test]
fn the_blessed_document_produces_the_blessed_findings() {
    let path = format!(
        "{}/fixtures/miniature.expected.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let report = run(both(), &fixture("miniature"));
    let printed: Vec<String> = report
        .findings
        .iter()
        .map(|finding| finding.to_line().expect("serialisable"))
        .collect();
    let written = format!("{}\n", printed.join("\n"));
    if std::env::var("FATHOM_BLESS").is_ok_and(|value| value == "1") {
        std::fs::write(&path, &written).expect("writable");
    }
    let expected =
        std::fs::read_to_string(&path).expect("a blessed file, committed or blessed now");
    assert_eq!(
        written, expected,
        "re-bless with FATHOM_BLESS=1 cargo test --test contract"
    );
    assert_eq!(report.findings.len(), 1, "one rule, one finding");
    assert_eq!(
        report.findings[0].finding_type.as_str(),
        UNTESTED_AGENT_LOGIC
    );
    assert!(report.notices.is_empty(), "{:?}", report.notices);
}

#[test]
fn a_human_episode_with_a_test_file_does_not_earn_the_agent_rule() {
    // The miniature's side-effects episode: a human, `tests/core.rs` in the change, and 62 lines of churn
    // with the module graph unread. Exactly one of this crate's two rules applies, and only that one fires.
    let report = run(both(), &fixture("side-effects"));
    let kinds: Vec<String> = report
        .findings
        .iter()
        .map(|finding| finding.finding_type.to_string())
        .collect();
    assert_eq!(
        kinds,
        vec![UNSEEN_REACH.to_owned()],
        "{:?}",
        report.findings
    );
    assert_eq!(
        report.runs.len(),
        2,
        "both rows are in the accounting either way"
    );
    assert_eq!(
        report.run(UNTESTED_AGENT_LOGIC).expect("a row").findings,
        0,
        "the author is human and a test file arrived, either of which is enough"
    );
    assert!(report.notices.is_empty(), "{:?}", report.notices);
}

#[test]
fn a_finding_carries_the_namespaced_type_the_key_and_the_jump() {
    let report = run(both(), &fixture("miniature"));
    let finding = &report.findings[0];
    assert_eq!(finding.finding_type.namespace(), "acme");
    assert_eq!(finding.finding_type.id(), "untested_agent_logic");
    assert_eq!(
        finding.suppression_key,
        format!("{UNTESTED_AGENT_LOGIC}:src/core/mod.rs"),
        "the key is the type plus the subject, so `detectors.suppress` takes what the finding prints"
    );
    assert_eq!(finding.jump.view, "diff");
    assert!(finding.jump.target.starts_with("ep-7090b6693637812919bd#"));
    assert_eq!(finding.schema_version, 1);
    assert!(!finding.evidence.is_empty(), "every claim cites a row");
}

#[test]
fn a_detector_off_by_default_stays_off_until_somebody_names_it() {
    let delta = fixture("miniature");
    let quiet = registry(both()).run(
        &PluginInput::new(&delta),
        &Selection::default(),
        &Limits::default(),
    );
    assert!(quiet.findings.is_empty());
    assert!(
        quiet
            .runs
            .iter()
            .all(|run| run.outcome == fathom_analyze::plugin::Outcome::Disabled),
        "two detectors, both `disabled`, both accounted for"
    );
    let one = registry(both()).run(
        &PluginInput::new(&delta),
        &Selection::parse(&format!("+{UNTESTED_AGENT_LOGIC}")),
        &Limits::default(),
    );
    assert_eq!(one.findings.len(), 1);
    assert_eq!(
        one.run(UNSEEN_REACH).expect("a row").outcome,
        fathom_analyze::plugin::Outcome::Disabled
    );
}

#[test]
fn one_key_silences_one_rule_and_the_row_says_it_was_suppressed() {
    let delta = fixture("miniature");
    let limits = Limits {
        suppress: vec![UNTESTED_AGENT_LOGIC.to_owned()],
        ..Default::default()
    };
    let report = registry(both()).run(&PluginInput::new(&delta), &on(), &limits);
    assert!(report.findings.is_empty());
    assert_eq!(
        report.run(UNTESTED_AGENT_LOGIC).expect("a row").outcome,
        fathom_analyze::plugin::Outcome::Suppressed
    );
}

// --- the rules on synthetic shapes, where the edges are controllable ------------------------------

#[test]
fn untested_agent_logic_fires_on_the_shape_it_is_for_and_on_nothing_nearby() {
    let fired = run(
        vec![Arc::new(UntestedAgentLogic::default())],
        &authored("agent", &[("src/core.rs", 40)], &[]),
    );
    assert_eq!(fired.findings.len(), 1);
    assert_eq!(fired.findings[0].severity, Severity::Medium);

    // A test arrived: not this rule.
    let tested = run(
        vec![Arc::new(UntestedAgentLogic::default())],
        &authored("agent", &[("src/core.rs", 40)], &[("tests/core.rs", 12)]),
    );
    assert!(tested.findings.is_empty(), "a test file arrived");

    // A human wrote it: not this rule, whatever the size.
    let human = run(
        vec![Arc::new(UntestedAgentLogic::default())],
        &authored("human", &[("src/core.rs", 400)], &[]),
    );
    assert!(human.findings.is_empty(), "the rule is about authorship");

    // Under the floor: too small to owe anything.
    let small = run(
        vec![Arc::new(UntestedAgentLogic::new(50))],
        &authored("agent", &[("src/core.rs", 40)], &[]),
    );
    assert!(
        small.findings.is_empty(),
        "40 is under the 50 the caller set"
    );

    // Three times the floor is a block-worthy one, and rule R4 is what makes it cite evidence.
    let big = run(
        vec![Arc::new(UntestedAgentLogic::default())],
        &authored("agent", &[("src/core.rs", 60)], &[]),
    );
    assert_eq!(big.findings[0].severity, Severity::High);
    assert_eq!(big.findings[0].evidence.len(), 1);
}

#[test]
fn untested_agent_logic_refuses_to_guess_at_an_unknown_author() {
    let report = run(
        vec![Arc::new(UntestedAgentLogic::default())],
        &authored("unknown", &[("src/core.rs", 400)], &[]),
    );
    assert!(
        report.findings.is_empty(),
        "an unknown author is not an agent, and a finding about a label is not a finding about a change"
    );
}

#[test]
fn unseen_reach_speaks_when_the_graph_was_never_read() {
    let mut big = authored("human", &[("src/core.rs", 50)], &[("tests/core.rs", 10)]);
    // `compose` marks blast `unavailable` when nothing was lent, which is exactly the state the rule wants.
    big.totals.churn = 60;
    let report = run(vec![Arc::new(UnseenReach::new(40))], &big);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].severity, Severity::Low);
    assert!(report.findings[0].message.contains("graph was not read"));

    // Below the floor the gap is not worth a row, and the gap itself is still in the document.
    let small = authored("human", &[("src/core.rs", 5)], &[("tests/core.rs", 1)]);
    let report = run(vec![Arc::new(UnseenReach::new(40))], &small);
    assert!(report.findings.is_empty());
}

#[test]
fn a_document_survives_the_round_trip_a_pipeline_would_put_it_through() {
    let original = fixture("miniature");
    let text = serde_json::to_string(&original).expect("a delta serialises");
    let read: EpisodeDelta = serde_json::from_str(&text).expect("and deserialises");
    assert_eq!(read, original);
    write_fixture("round-trip", &read);
    let path = format!("{}/fixtures/round-trip.json", env!("CARGO_MANIFEST_DIR"));
    let again: EpisodeDelta =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("written"))
            .expect("and reads back");
    let _ = std::fs::remove_file(&path);
    assert_eq!(again, original);
}

// --- the binary, as a reviewer runs it ----------------------------------------------------------

fn command() -> std::process::Command {
    Command::new(env!("CARGO_BIN_EXE_acme-detectors"))
}

#[test]
fn the_binary_reads_a_file_and_prints_one_finding_per_line() {
    let output = command()
        .args(["--delta", "fixtures/miniature.json", "--json"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the binary built");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1);
    let value: serde_json::Value =
        serde_json::from_str(lines[0]).expect("one JSON object per line");
    assert_eq!(value["finding_type"], UNTESTED_AGENT_LOGIC);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["severity"], "medium");
}

#[test]
fn the_binary_reads_stdin_too_because_that_is_the_pipeline() {
    use std::io::Write as _;
    let text = std::fs::read_to_string(format!(
        "{}/fixtures/miniature.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("committed");
    let mut child = command()
        .args(["--text"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the binary started");
    child
        .stdin
        .take()
        .expect("a stdin")
        .write_all(text.as_bytes())
        .expect("wrote the document");
    let status = child.wait_with_output().expect("it finished");
    assert!(status.status.success());
    let stdout = String::from_utf8(status.stdout).expect("utf-8");
    let stderr = String::from_utf8(status.stderr).expect("utf-8");
    assert!(
        stdout.contains(UNTESTED_AGENT_LOGIC),
        "the document arrived by pipe and the rule ran on it: stdout={stdout:?} stderr={stderr:?}"
    );
}

#[test]
fn the_binary_blesses_and_checks_and_says_which_it_did() {
    let blessed = format!(
        "{}/fixtures/miniature.expected.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let output = command()
        .args([
            "--delta",
            "fixtures/miniature.json",
            "--json",
            "--check",
            &blessed,
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        // The child inherits `FATHOM_BLESS` when the test suite itself is blessing; a check that blessed
        // instead of comparing would pass by construction, which is not a check.
        .env("FATHOM_BLESS", "")
        .output()
        .expect("the binary built");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("byte-identical"),
        "a check that passes has to say it passed"
    );
}

#[test]
fn the_binary_refuses_a_document_that_is_not_a_document_and_says_what_it_wanted() {
    let output = command()
        .args(["--delta", "Cargo.toml"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the binary built");
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).expect("utf-8");
    assert!(
        stderr.contains("fathom analyze"),
        "the remedy is in the message: {stderr}"
    );
}

#[test]
fn an_unknown_flag_is_a_usage_error_naming_the_flag() {
    let output = command()
        .arg("--detecters")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the binary built");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--detecters"),
        "a typo is named, not guessed at"
    );
}

#[test]
fn listing_the_crate_prints_both_rules_and_what_each_declares() {
    let output = command()
        .arg("--list")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the binary built");
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert!(stdout.contains("id=acme.untested_agent_logic"), "{stdout}");
    assert!(stdout.contains(&format!("declares={UNTESTED_AGENT_LOGIC}")));
    assert!(stdout.contains("id=acme.unseen_reach"));
    assert!(
        stdout.contains("default=off"),
        "both ship off until a host opts in"
    );
}

#[test]
fn a_detectors_flag_takes_one_rule_back_off_again() {
    let output = command()
        .args([
            "--delta",
            "fixtures/miniature.json",
            "--text",
            "--detectors",
            &format!("-{UNTESTED_AGENT_LOGIC}"),
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the binary built");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("{UNTESTED_AGENT_LOGIC} disabled")),
        "{stderr}"
    );
}

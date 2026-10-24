//! The extensibility flags, driven as a binary (`e6.7`, **reco-raw**).
//!
//! [`crates/fathom-analyze/tests/plugin_api.rs`] proves the plugin API and `policy.rs`'s own tests prove the
//! selection grammar. What this file proves is the part only the shipped verb can prove: that a flag typed at
//! a shell changes what the scan ran, that a name this build does not know is a sentence and not a failure,
//! and that the event stream a run writes is a stream `fathom events` — the reader in the same binary — can
//! read back. A rule that lives in the library and never reaches the verb is a rule nobody can use.
//!
//! The repository is two commits: an initialisation, then one agent turn that adds a module with a `TODO`
//! and no test beside it. That shape is chosen so at least one default-on detector answers — *which* one it
//! answers with is read out of the run rather than assumed here, because an assumption about which detector
//! fires is exactly the thing that rots quietly when a detector's threshold is retuned.
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

/// The one episode's `findings` tally from `fathom why --json`, as `(total, detectors_ran, detectors)`.
fn findings_total(dir: &Path, extra: &[&str]) -> (usize, usize, usize) {
    let mut args = vec!["why", "HEAD", "--root", ".", "--json"];
    args.extend_from_slice(extra);
    let out = fathom(dir, &args);
    assert_eq!(code(&out), 0, "why failed: {}", stderr(&out));
    let text = stdout(&out);
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("--json prints exactly one JSON object");
    let findings = &value["findings"];
    let number =
        |key: &str| usize::try_from(findings[key].as_i64().unwrap_or(-1)).unwrap_or(usize::MAX);
    (
        number("total"),
        number("detectors_ran"),
        number("detectors"),
    )
}

/// The first detector the text report says raised a finding, read out of the report rather than assumed.
///
/// The text form prints `  {severity} {detector_id} — {message}` per finding, which is the product's own
/// answer to "who spoke", and this parses that instead of a fixture of expected ids that would need
/// retuning every time a threshold moved.
fn first_fired_detector(dir: &Path) -> String {
    let out = fathom(dir, &["why", "HEAD", "--root", "."]);
    assert_eq!(code(&out), 0, "why failed: {}", stderr(&out));
    for line in stdout(&out).lines() {
        let trimmed = line.trim_start();
        for severity in ["high", "medium", "low"] {
            if let Some(rest) = trimmed.strip_prefix(severity) {
                let id = rest.trim_start().split(' ').next().unwrap_or_default();
                if !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                    return id.to_owned();
                }
            }
        }
    }
    String::new()
}

/// A repository with one indexable agent turn, indexed.
fn repository(label: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "fathom-extens-cli-{label}-{}-{index}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"], 0);
    git(&dir, &["config", "user.name", "Pi Agent"], 0);
    git(&dir, &["config", "user.email", "agent@example.invalid"], 0);
    write_and_commit(
        &dir,
        0,
        "chore: initialise harbour",
        &[("README.md", "# harbour\n")],
    );
    // One agent turn: new logic, a piece of explicitly unfinished work, and no test anywhere. Several
    // default-on detectors answer on this, and none of them needs a graph.
    let body =
        "def gaugereading():\n    # TODO: the tidal datum is a guess\n    return 3\n".repeat(12);
    write_and_commit(
        &dir,
        180,
        "feat: read the tide gauge\n\nCo-authored-by: looprs <looprs@example.invalid>",
        &[("src/harbour/gauge.py", &body)],
    );
    let out = fathom(&dir, &["index", "."]);
    assert_eq!(code(&out), 0, "indexing failed: {}", stderr(&out));
    dir
}

fn write_and_commit(dir: &Path, minute: i64, message: &str, files: &[(&str, &str)]) {
    for (path, body) in files {
        let full = dir.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create the file's directory");
        }
        std::fs::write(full, body).expect("write the fixture file");
        git(dir, &["add", "--", path], minute);
    }
    git(dir, &["commit", "-q", "-m", message], minute);
}

fn git(dir: &Path, args: &[&str], minute: i64) {
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

/// The file the stream goes to, inside the repository, as the flag takes it.
const STREAM: &str = ".fathom/extens-events.jsonl";

#[test]
fn a_turn_with_new_logic_and_no_test_earns_at_least_one_finding() {
    // The premise of every test below: the fixture actually earns something, and the report says who said it.
    let dir = repository("premise");
    let (total, ran, detectors) = findings_total(&dir, &[]);
    assert!(
        total >= 1,
        "the fixture earns no finding, so the flag tests below prove nothing: {total}/{ran}/{detectors}"
    );
    assert_eq!(
        detectors, 12,
        "the report accounts for every built-in detector"
    );
    assert!(ran > 0 && ran < detectors, "some ran, some did not: {ran}");
    assert!(
        !first_fired_detector(&dir).is_empty(),
        "the text report must name the detector that spoke"
    );
}

#[test]
fn turning_one_detector_off_takes_its_findings_out_of_the_answer() {
    let dir = repository("disable");
    let fired = first_fired_detector(&dir);
    assert!(
        !fired.is_empty(),
        "nothing fired, so there was nothing to switch off"
    );
    let before = findings_total(&dir, &[]);
    let out = fathom(
        &dir,
        &["why", "HEAD", "--root", ".", "--disable-detector", &fired],
    );
    assert_eq!(code(&out), 0, "why failed: {}", stderr(&out));
    assert_eq!(
        first_fired_detector(&dir),
        fired,
        "the baseline must not change between two reads of the same repository"
    );
    let after = findings_total(&dir, &["--disable-detector", &fired]);
    assert!(
        after.1 < before.1,
        "one fewer detector should have run: {before:?} → {after:?}"
    );
    assert!(
        after.0 < before.0,
        "the disabled detector's findings should be gone: {before:?} → {after:?}"
    );
    // And the switch is visible, not merely effective: the row for a detector switched off on purpose is
    // printed, so "did it look?" still has an answer in the report that says it did not.
    let text = stdout(&out);
    assert!(
        !text.contains(&format!("  medium {fired}")) && !text.contains(&format!("  high {fired}")),
        "a disabled detector still raised a row: {text}"
    );
}

#[test]
fn the_short_spec_says_exactly_what_the_long_flag_says() {
    let dir = repository("spec");
    let fired = first_fired_detector(&dir);
    assert!(
        !fired.is_empty(),
        "nothing fired, so there was nothing to switch off"
    );
    let long = findings_total(&dir, &["--disable-detector", &fired]);
    let short = findings_total(&dir, &[&format!("--detectors=-{fired}")]);
    assert_eq!(
        long, short,
        "`--detectors -id` and `--disable-detector id` are one switch"
    );
    let none = findings_total(&dir, &["--detectors", "none"]);
    assert_eq!(none.0, 0, "`none` asks for nothing, so nothing is found");
    assert_eq!(none.1, 0, "and no detector ran");
    assert_eq!(none.2, 12, "and the report still accounts for all twelve");
    let one = findings_total(&dir, &["--detectors", "none,+todo_debt"]);
    assert!(one.1 <= 1, "`none,+id` runs one detector at most: {one:?}");
    assert_eq!(one.2, 12, "and names the other eleven as not running");
}

#[test]
fn a_name_this_build_does_not_know_is_a_sentence_and_not_a_failure() {
    let dir = repository("unknown");
    let out = fathom(
        &dir,
        &[
            "why",
            "HEAD",
            "--root",
            ".",
            "--disable-detector",
            "nonsense_detector",
        ],
    );
    assert_eq!(
        code(&out),
        0,
        "an unknown name must not fail the run: {}",
        stderr(&out)
    );
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        text.contains("nonsense_detector"),
        "the run should name what it could not use: {text}"
    );
    // A name that matches nothing must not be reported as a switch that was thrown.
    assert!(
        !text.contains("nonsense_detector is disabled")
            && !text.contains("disabled: nonsense_detector"),
        "a name that matches nothing must not claim it switched something off: {text}"
    );
}

#[test]
fn a_namespaced_name_says_the_plugin_is_not_linked_in() {
    // The honest answer to `+acme.thing` in a binary that registers no plugins. Silence here would read as
    // "the detector ran and found nothing", which is the one thing this product refuses to print.
    let dir = repository("plugin");
    let out = fathom(
        &dir,
        &[
            "why",
            "HEAD",
            "--root",
            ".",
            "--detectors",
            "+acme.untested_agent_logic",
        ],
    );
    assert_eq!(code(&out), 0, "why failed: {}", stderr(&out));
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        text.contains("acme.untested_agent_logic") && text.contains("links none"),
        "the run should say no plugin detector is linked: {text}"
    );
}

#[test]
fn the_event_stream_records_the_run_and_fathom_events_reads_it_back() {
    let dir = repository("stream");
    let out = fathom(&dir, &["why", "HEAD", "--root", ".", "--events", STREAM]);
    assert_eq!(code(&out), 0, "why failed: {}", stderr(&out));
    let written = std::fs::read_to_string(dir.join(STREAM)).expect("the run wrote the stream");
    let lines: Vec<&str> = written.lines().collect();
    assert!(
        lines.len() > 12,
        "one line per detector plus the run's own: {lines:?}"
    );
    let first: serde_json::Value =
        serde_json::from_str(lines[0]).expect("one JSON object per line");
    let last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).expect("JSONL");
    assert_eq!(first["kind"], "run.started");
    assert_eq!(last["kind"], "run.finished");
    assert_eq!(first["contract"], "fathom-analysis-events");
    assert_eq!(first["schema_version"], 1);
    // Every line of one run carries the same run id and a sequence number, or a reader that interleaves
    // two runs cannot tell them apart.
    let run = first["run"].as_str().unwrap_or_default();
    assert!(!run.is_empty());
    assert_eq!(last["run"].as_str(), Some(run));
    assert!(
        !last["ts"].is_null(),
        "a stream written by the CLI is stamped with when it happened"
    );
    for (index, line) in lines.iter().enumerate() {
        let value: serde_json::Value = serde_json::from_str(line).expect("JSONL");
        assert_eq!(
            value["seq"].as_i64(),
            Some(i64::try_from(index).unwrap() + 1),
            "sequence numbers are this bus's own and in order"
        );
    }
    assert!(
        written.contains("detector.finished")
            && written.contains("detector.skipped")
            && written.contains("selection.resolved")
            && written.contains("finding.raised"),
        "the scan's own account of itself: {written}"
    );

    // And the reader in the same binary reads it back, filtered and tailed.
    let read = fathom(&dir, &["events", "--root", ".", "--path", STREAM, "--json"]);
    assert_eq!(code(&read), 0, "events failed: {}", stderr(&read));
    let every = stdout(&read);
    assert_eq!(
        every.lines().count(),
        lines.len(),
        "the reader prints every line it read"
    );
    let filtered = fathom(
        &dir,
        &[
            "events",
            "--root",
            ".",
            "--path",
            STREAM,
            "--on",
            "detector.*",
            "--json",
        ],
    );
    let kept_text = stdout(&filtered);
    let kept: Vec<&str> = kept_text.lines().collect();
    assert!(
        !kept.is_empty() && kept.iter().all(|line| line.contains("detector.")),
        "a family filter keeps a family: {kept:?}"
    );
    let tailed = fathom(
        &dir,
        &[
            "events", "--root", ".", "--path", STREAM, "--tail", "1", "--json",
        ],
    );
    let tailed_text = stdout(&tailed);
    let tailed_lines: Vec<&str> = tailed_text.lines().collect();
    assert_eq!(tailed_lines.len(), 1, "--tail keeps the last line");
    assert!(
        tailed_lines[0].contains("run.finished"),
        "the last line is the run's end"
    );
    // The text rendering says the same thing in a sentence, because a reviewer reads it in a terminal.
    let rendered = fathom(
        &dir,
        &["events", "--root", ".", "--path", STREAM, "--tail", "2"],
    );
    assert_eq!(code(&rendered), 0);
    let shown = stdout(&rendered);
    assert!(
        shown.contains("run.finished") && shown.contains("code=0"),
        "the text form names the kind and its counts: {shown}"
    );
}

#[test]
fn events_to_stderr_leave_stdout_the_same_single_document() {
    // The pipe contract: `--json` promises stdout holds one document, so the stream goes to stderr and a
    // reviewer can still `| jq`. That split is the whole reason `-` exists.
    let dir = repository("stderr");
    let out = fathom(
        &dir,
        &["why", "HEAD", "--root", ".", "--json", "--events", "-"],
    );
    assert_eq!(code(&out), 0, "why failed: {}", stderr(&out));
    let text = stdout(&out);
    let one: serde_json::Value =
        serde_json::from_str(&text).expect("stdout is still exactly one JSON object");
    assert_eq!(one["contract"], "fathom.cli.why/1");
    let err = stderr(&out);
    assert!(
        err.contains("\"kind\":\"run.started\""),
        "the stream went to stderr: {err}"
    );
    assert!(
        err.contains("\"kind\":\"run.finished\""),
        "and ended: {err}"
    );
    assert!(!dir.join(STREAM).exists(), "`--events -` writes no file");
}

#[test]
fn nothing_writes_a_stream_until_the_repository_or_the_reviewer_asks() {
    // ADR-001 §4.6: the product writes no file the repository did not ask for. A default run leaves nothing
    // behind, and says nothing about it.
    let dir = repository("quiet");
    let out = fathom(&dir, &["why", "HEAD", "--root", ".", "--json"]);
    assert_eq!(code(&out), 0, "why failed: {}", stderr(&out));
    assert!(!dir.join(STREAM).exists());
    assert!(!dir.join(".fathom").join("analysis-events.jsonl").exists());
    assert!(
        !stderr(&out).contains("events"),
        "a run that writes nothing does not advertise that it wrote nothing"
    );
}

#[test]
fn a_repository_can_switch_the_stream_on_for_itself() {
    let dir = repository("config");
    std::fs::write(
        dir.join("fathom.toml"),
        "[events]\nenabled = true\non = [\"run.*\", \"analysis.*\"]\n",
    )
    .expect("write the config");
    let out = fathom(&dir, &["why", "HEAD", "--root", ".", "--json"]);
    assert_eq!(code(&out), 0, "why failed: {}", stderr(&out));
    let text = std::fs::read_to_string(dir.join(".fathom/analysis-events.jsonl"))
        .expect("`[events] enabled` writes the default stream");
    assert!(
        text.contains("run.started") && text.contains("analysis.finished"),
        "{text}"
    );
    assert!(
        !text.contains("detector.finished"),
        "`on` is a filter, not a label: {text}"
    );
    // And the reader reports the filter it read, which is the answer to "why is this stream so short".
    let explain = fathom(&dir, &["events", "--root", ".", "--explain"]);
    assert_eq!(code(&explain), 0);
    let shown = stdout(&explain);
    assert!(
        shown.contains("enabled: true") && shown.contains("run.*"),
        "{shown}"
    );
}

#[test]
fn a_repository_config_may_not_point_the_stream_outside_the_repository() {
    // The `paths.*` rule, applied here: a file that arrives with `git clone` does not get to choose where
    // on your machine this process writes.
    let dir = repository("escape");
    std::fs::write(
        dir.join("fathom.toml"),
        "[paths]\nanalysis_events = \"../outside-analysis.jsonl\"\n\n[events]\nenabled = true\n",
    )
    .expect("write the config");
    let out = fathom(&dir, &["why", "HEAD", "--root", ".", "--json"]);
    assert_eq!(code(&out), 0, "why failed: {}", stderr(&out));
    assert!(
        !dir.parent()
            .unwrap()
            .join("outside-analysis.jsonl")
            .exists(),
        "the escape was honoured, which is the bug"
    );
    let complaints = stderr(&out);
    assert!(
        complaints.contains("outside the repository"),
        "the refusal is said: {complaints}"
    );
    assert!(
        dir.join(".fathom/analysis-events.jsonl").exists(),
        "and the default stream is what got written instead"
    );
}

#[test]
fn a_stream_with_a_half_line_or_a_future_version_is_read_anyway() {
    // The format's rule, proved at the reader: a run killed mid-write leaves a torn line, and an event
    // written by a newer build is skipped by name rather than making this verb fail.
    let dir = repository("torn");
    let path = dir.join("stream.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"contract\":\"fathom-analysis-events\",\"schema_version\":1,\"kind\":\"run.started\",",
            "\"seq\":1,\"run\":\"why-1\"}\n",
            "{\"contract\":\"fathom-analysis-events\",\"schema_version\":1,\"kind\":\"a_brand_new_ki",
            "\"note\":\"torn by a killed run\"\n",
            "{\"contract\":\"fathom-analysis-events\",\"schema_version\":99,\"kind\":\"run.finished\",",
            "\"seq\":3,\"run\":\"why-1\"}\n",
            "{\"contract\":\"fathom-analysis-events\",\"schema_version\":1,\"kind\":\"some.future.kind\",",
            "\"seq\":4,\"run\":\"why-1\",\"a_field_this_build_does_not_know\":true}\n",
        ),
    )
    .expect("write the stream");
    let out = fathom(
        &dir,
        &["events", "--root", ".", "--path", "stream.jsonl", "--json"],
    );
    assert_eq!(
        code(&out),
        0,
        "a bad line is not a failure: {}",
        stderr(&out)
    );
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "the good line and the unknown kind survive: {lines:?}"
    );
    assert!(lines[0].contains("run.started"));
    assert!(
        lines[1].contains("some.future.kind"),
        "an unknown kind is a kind"
    );
    let complaints = stderr(&out);
    assert!(
        complaints.contains("ignored"),
        "the torn line and the unreadable version are both named: {complaints}"
    );
}

#[test]
fn a_run_that_gives_up_still_closes_its_stream() {
    // A stream whose last line is `run.started` is a stream that recorded a crash, and the difference
    // between those two is the whole reason to read a stream instead of a log line.
    let dir = repository("abandoned");
    let out = fathom(
        &dir,
        &["why", "nosuchepisode", "--root", ".", "--events", STREAM],
    );
    assert_ne!(code(&out), 0, "an unknown episode is a failure");
    let written = std::fs::read_to_string(dir.join(STREAM)).expect("the run wrote the stream");
    let last = written.lines().last().unwrap_or_default();
    assert!(
        last.contains("\"kind\":\"run.failed\""),
        "the terminal event is written on the way out: {written}"
    );
}

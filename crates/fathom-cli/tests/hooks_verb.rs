//! `fathom hooks`, driven as a binary (`e6.1`, `reco-afh`).
//!
//! `crates/fathom-ingest/tests/hooks_v1.rs` proves the reader's rules; `crates/fathom-cli/tests/schemas.rs`
//! proves the payload conforms to `schemas/hooks.schema.json`. What only a binary test can prove is the part
//! a *verb* owns: the exit code, that stdout is exactly one JSON value, that a diagnostic changed nothing on
//! disk, and that the durable offset survives being invoked twice.
//!
//! The fixtures are written as plain appends because that is what an adapter does; every event file here is
//! deliberately damaged in some way, because the damage is the feature.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
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

/// A throwaway directory holding an `events.jsonl`, with the damage the reader is meant to meet.
struct Stream {
    dir: PathBuf,
}

impl Stream {
    fn new(label: &str, body: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("fathom-hooks-cli-{label}-{}", index));
        let _ = std::fs::remove_dir_all(&dir);
        let fathom_dir = dir.join(".fathom");
        std::fs::create_dir_all(&fathom_dir).expect("create .fathom");
        std::fs::write(fathom_dir.join("events.jsonl"), body).expect("write the events file");
        Stream { dir }
    }

    fn root(&self) -> &str {
        self.dir.to_str().expect("a temp path is utf-8")
    }

    fn state_dir(&self) -> PathBuf {
        self.dir.join(".fathom").join("hooks")
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if std::env::var_os("FATHOM_KEEP_FIXTURES").is_none() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// The ADR-005 §3.3 canonical records, plus damage: an undecodable line, a schema version from the future, a
/// path that climbs out of the session, and a session end.
fn damaged() -> String {
    let mut out = String::new();
    out.push_str(r#"{"harness":"pi_json","schema_version":1,"seq":1,"session_id":"s7","source":"turn_start","ts":"2026-01-01T00:00:00.000Z"}"#);
    out.push('\n');
    out.push_str(r#"{"harness":"pi_json","payload":{"path":"src/a.py","change":"update"},"schema_version":1,"seq":2,"session_id":"s7","source":"file.edited","ts":"2026-01-01T00:00:01.000Z"}"#);
    out.push('\n');
    out.push_str("this line is not json\n");
    out.push_str(r#"{"harness":"pi_json","schema_version":2,"seq":3,"session_id":"s7","source":"turn_end","ts":"2026-01-01T00:00:02.000Z"}"#);
    out.push('\n');
    out.push_str(r#"{"harness":"pi_json","payload":{"path":"../../etc/passwd"},"schema_version":1,"seq":4,"session_id":"s7","source":"file.edited","ts":"2026-01-01T00:00:03.000Z"}"#);
    out.push('\n');
    out.push_str(r#"{"harness":"pi_json","schema_version":1,"seq":5,"session_id":"s7","source":"session_end","ts":"2026-01-01T00:00:04.000Z"}"#);
    out.push('\n');
    out
}

#[test]
fn check_is_the_default_reports_the_damage_and_changes_nothing() {
    let stream = Stream::new("check", &damaged());
    // `--check` is the default, and it is the shape ADR-005 names: `fathom hooks --check`.
    let out = fathom(&["hooks", stream.root(), "--check", "--json"]);
    assert_eq!(
        code(&out),
        0,
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    let plain = fathom(&["hooks", stream.root(), "--json"]);
    assert_eq!(code(&plain), 0, "{}", stderr(&plain));
    assert_eq!(
        stdout(&plain),
        stdout(&out),
        "`fathom hooks` and `fathom hooks --check` are the same report — the flag only says out loud what \
         was already the default"
    );

    let state = stream.state_dir();
    assert!(
        !state.join("cursor.json").exists(),
        "a check must not write a cursor: reporting on the queue is not consuming it, and a monitor that \
         consumed would silently starve the watcher"
    );
    assert!(
        !state.join("quarantine.jsonl").exists(),
        "a check must not write quarantine records: every poll would re-record the same damage and push the \
         real records out of a bounded file"
    );
}

#[test]
fn the_verb_names_itself_and_the_stream_contract() {
    let stream = Stream::new("contract", &damaged());
    let out = fathom(&["hooks", stream.root(), "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let payload: serde_json::Value =
        serde_json::from_str(stdout(&out).trim()).expect("--json prints exactly one JSON value");
    assert_eq!(payload["contract"], "fathom.cli.hooks/1");
    assert_eq!(payload["verb"], "fathom hooks");
    assert_eq!(payload["action"], "check");
    assert_eq!(payload["stream"]["protocol"], "fathom-hooks/1");
    assert_eq!(payload["stream"]["event_schema"], 1);
    assert_eq!(
        payload["stream"]["events_file"], ".fathom/events.jsonl",
        "the path is repository-relative, because this payload is meant to be pasted into a bug report"
    );
}

#[test]
fn the_text_report_says_what_is_wrong_in_a_sentence_an_operator_can_act_on() {
    let stream = Stream::new("text", &damaged());
    let out = fathom(&["hooks", stream.root()]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("fathom hooks"), "{text}");
    assert!(text.contains(".fathom/events.jsonl"), "{text}");
    assert!(text.contains("would be refused"), "{text}");
    for reason in ["undecodable", "schema_version", "path_escape"] {
        assert!(
            text.contains(reason),
            "`{reason}` is one of the refusals in this fixture and the text report must name it:\n{text}"
        );
    }
    assert!(text.contains("NOT OK"), "{text}");
    assert!(
        text.contains("exit 0 —"),
        "the exit line is part of the report, as every other verb prints it:\n{text}"
    );
}

#[test]
fn a_healthy_stream_is_ok_and_a_damaged_one_only_fails_when_asked() {
    let healthy = Stream::new(
        "healthy",
        concat!(
            r#"{"harness":"pi_json","schema_version":1,"seq":1,"session_id":"s7","source":"turn_start","ts":"2026-01-01T00:00:00.000Z"}"#,
            "\n",
            r#"{"harness":"pi_json","schema_version":1,"seq":2,"session_id":"s7","source":"session_end","ts":"2026-01-01T00:00:01.000Z"}"#,
            "\n",
        ),
    );
    let out = fathom(&["hooks", healthy.root(), "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let payload: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(payload["stream"]["ok"], true, "{payload}");
    assert_eq!(code(&fathom(&["hooks", healthy.root(), "--strict"])), 0);

    let damaged = Stream::new("strict", &damaged());
    let out = fathom(&["hooks", damaged.root(), "--json"]);
    assert_eq!(
        code(&out),
        0,
        "without --strict a bad stream is still a report that answered: {}",
        stdout(&out)
    );
    let payload: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(payload["stream"]["ok"], false, "{payload}");
    assert!(!payload["stream"]["reasons"].as_array().unwrap().is_empty());

    let strict = fathom(&["hooks", damaged.root(), "--strict"]);
    assert_eq!(
        code(&strict),
        7,
        "`--strict` is the CI gate and 7 is CHECK_VIOLATIONS (ADR-002 §7.3):\n{}",
        stdout(&strict)
    );
    assert!(stdout(&strict).contains("exit 7 —"));
}

#[test]
fn tail_advances_the_offset_once_and_the_next_tail_reads_only_what_arrived() {
    let body = damaged();
    let stream = Stream::new("tail", &body);

    let first = fathom(&["hooks", stream.root(), "--tail", "--json"]);
    assert_eq!(code(&first), 0, "{}", stderr(&first));
    let first: serde_json::Value = serde_json::from_str(stdout(&first).trim()).unwrap();
    let seqs: Vec<u64> = first["read"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["seq"].as_u64().unwrap())
        .collect();
    assert_eq!(
        seqs,
        vec![1, 2, 5],
        "the accepted records only — the undecodable line, the future schema and the escaping path are \
         refusals, not events"
    );
    assert_eq!(first["read"]["refusals"].as_array().unwrap().len(), 3);
    let offset = first["stream"]["offset"].as_u64().unwrap();
    assert!(offset > 0);
    assert!(
        stream.state_dir().join("cursor.json").exists(),
        "`--tail` is the consuming action, so it persists"
    );
    assert!(stream.state_dir().join("quarantine.jsonl").exists());

    let again = fathom(&["hooks", stream.root(), "--tail", "--json"]);
    let again: serde_json::Value = serde_json::from_str(stdout(&again).trim()).unwrap();
    assert_eq!(
        again["read"]["events"].as_array().map(Vec::len),
        Some(0),
        "the second tail must not reprocess: replaying events twice would double-count every tool call \
         and every edit in the episode"
    );
    assert_eq!(again["stream"]["offset"].as_u64(), Some(offset));

    // And what arrives next is read exactly once, from the persisted offset.
    let path = stream.dir.join(".fathom").join("events.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("append to the events file");
    use std::io::Write as _;
    writeln!(
        file,
        r#"{{"harness":"pi_json","schema_version":1,"seq":6,"session_id":"s7","source":"turn_start","ts":"2026-01-01T00:01:00.000Z"}}"#
    )
    .unwrap();
    drop(file);
    let third = fathom(&["hooks", stream.root(), "--tail", "--json"]);
    let third: serde_json::Value = serde_json::from_str(stdout(&third).trim()).unwrap();
    let events = third["read"]["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "only the new record: {third}");
    assert_eq!(events[0]["seq"], 6);
    assert_eq!(
        events[0]["turn_id"], "s7:2",
        "the turn id is allocated from the ledger the cursor carried, not restarted: {third}"
    );
}

#[test]
fn replay_is_deterministic_across_processes_and_says_so() {
    let stream = Stream::new("replay", &damaged());
    let mut digests = Vec::new();
    let mut turn_ids = Vec::new();
    for _ in 0..2 {
        let out = fathom(&["hooks", stream.root(), "--replay", "--json"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        let payload: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
        digests.push(
            payload["stream"]["replay"]["ledger_digest"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
        assert_eq!(payload["stream"]["replay"]["from_offset"], 0);
        assert_eq!(payload["stream"]["replay"]["performed"], true);
        assert_eq!(payload["stream"]["replay"]["all_session_qualified"], true);
        turn_ids.push(payload["stream"]["turns"]["carried"].as_u64().unwrap());
    }
    assert_eq!(
        digests[0], digests[1],
        "two rebuilds of the same bytes, different processes"
    );
    assert_eq!(turn_ids[0], turn_ids[1]);
    let text = fathom(&["hooks", stream.root(), "--replay"]);
    assert!(
        stdout(&text).contains("replayed from byte 0"),
        "{}",
        stdout(&text)
    );
}

#[test]
fn trailer_emits_the_pair_rung_three_reads_and_rejects_nonsense() {
    let stream = Stream::new("trailer", &damaged());
    let out = fathom(&["hooks", stream.root(), "--trailer", "s7:4", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let payload: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(payload["action"], "trailer");
    assert_eq!(payload["trailer"]["turn_id"], "s7:4");
    assert_eq!(payload["trailer"]["keys"]["turn"], "Agent-Turn");
    let block = payload["trailer"]["block"].as_str().unwrap();
    assert_eq!(block, "Agent-Session: s7\nAgent-Turn: s7:4\n");
    assert!(
        payload["stream"].is_null(),
        "a trailer request reads nothing: {payload}"
    );

    let text = fathom(&["hooks", stream.root(), "--trailer", "s7:4"]);
    assert_eq!(code(&text), 0, "{}", stderr(&text));
    assert_eq!(
        stdout(&text),
        "Agent-Session: s7\nAgent-Turn: s7:4\n",
        "the text form is copy-pasteable into a commit message"
    );

    for bad in ["s7", "s7:x", ":3"] {
        let out = fathom(&["hooks", stream.root(), "--trailer", bad]);
        assert_eq!(
            code(&out),
            1,
            "`--trailer {bad}` names no session and turn, which is an argument-value error (code 1)"
        );
        assert!(!stderr(&out).trim().is_empty());
    }
}

#[test]
fn a_missing_events_file_is_a_report_not_an_error() {
    let dir = std::env::temp_dir().join("fathom-hooks-cli-missing");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.to_str().unwrap();
    let out = fathom(&["hooks", root, "--json"]);
    assert_eq!(
        code(&out),
        0,
        "a repository with no harness events is normal: {}",
        stderr(&out)
    );
    let payload: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(payload["stream"]["exists"], false);
    assert_eq!(payload["stream"]["size_bytes"], 0);
    assert_eq!(payload["stream"]["lag_bytes"], 0);
    assert!(
        payload["stream"]["unknowns"]
            .as_array()
            .map(|rows| !rows.is_empty())
            == Some(true),
        "the report must say what it cannot say: {payload}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_broken_config_is_a_notice_and_a_default_not_a_failure() {
    let stream = Stream::new("config", &damaged());
    std::fs::write(
        Path::new(stream.root()).join("fathom.toml"),
        "harness.max_bytes_per_session_mib = 0\n",
    )
    .expect("write a fathom.toml that will not parse as the table it should be");
    let out = fathom(&["hooks", stream.root(), "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let payload: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    let notices = payload["notices"].as_array().unwrap();
    assert!(
        !notices.is_empty(),
        "a config key that could not be honoured is admitted in the payload, not swallowed (ADR-002 §8): {}",
        stdout(&out)
    );
    assert!(
        notices.iter().any(|notice| notice
            .as_str()
            .unwrap_or_default()
            .contains("max_bytes_per_session_mib")),
        "{notices:?}"
    );
}

//! The hooks v1 read side (`e6.1`, `reco-afh`), asserted line by line.
//!
//! Every test here is one clause of [ADR-005](../../../docs/adr/0005-harness-contract.md) or one line of
//! the ticket's acceptance criteria:
//!
//! | Criterion | Test |
//! |---|---|
//! | truncated and mid-write lines are handled without loss on the next read | `a_torn_line_is_not_lost_and_is_not_double_counted` |
//! | malformed events are quarantined and counted, and the stream keeps flowing | `damage_is_quarantined_counted_and_stepped_over` |
//! | replay from offset 0 reproduces identical episode linkage | `replay_from_zero_reproduces_the_same_linkage`, `a_restart_resumes_without_reprocessing` |
//! | turn ids are session-qualified, and that closes ADR-003 §18.2 **D4** | `two_sessions_turn_three_are_two_turns` |
//! | the trailer the writer emits is the key rung 3 reads, and rung 3 fires on evidence this ingest produced | `rung_three_fires_on_the_trailer_this_ingest_writes` |
//!
//! The events files are written with plain `append` because that is what an adapter does; the byte offsets
//! the assertions name are the reader's own contract, not a fixture detail.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::path::PathBuf;

use fathom_ingest::hooks::{
    self, ClosedBy, Cursor, Event, FathomEvent, Limits, Reader, RejectReason, TRAILER_TURN,
    TRAILER_TURN_KEY,
};
use fathom_ingest::segment::{self, CommitFacts, Rule, Rules};

/// A unique temporary directory, removed at the end of the test.
struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "fathom-hooks-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        Temp { dir }
    }

    fn events(&self) -> PathBuf {
        self.dir.join(".fathom").join("events.jsonl")
    }

    fn reader(&self) -> Reader {
        std::fs::create_dir_all(self.events().parent().unwrap()).expect("create .fathom");
        Reader::new(self.events())
    }

    fn append(&self, lines: &[String]) {
        self.append_bytes(lines.join("\n").as_bytes());
    }

    /// Finish the last line: an adapter's newline lands after its own bytes, not with the next record.
    fn terminate(&self) {
        self.append_bytes(b"\n");
    }

    fn append_bytes(&self, bytes: &[u8]) {
        let path = self.events();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create .fathom");
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open the events file for append");
        file.write_all(bytes).expect("append");
        file.flush().expect("flush");
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        if std::env::var_os("FATHOM_KEEP_FIXTURES").is_none() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// One wire record, as an adapter writes it: keys sorted, one line, no trailing whitespace.
fn line(seq: u64, source: &str, session: &str, timestamp: &str, payload: &str) -> String {
    format!(
        r#"{{"cwd":"/srv/app","event_id":"e{seq}","harness":"pi_json","payload":{payload},"schema_version":1,"seq":{seq},"session_id":"{session}","source":"{source}","ts":"{timestamp}"}}"#
    )
}

/// The same, with the fields a file-edit event carries.
fn edit(seq: u64, session: &str, timestamp: &str, path: &str) -> String {
    line(
        seq,
        "file.edited",
        session,
        timestamp,
        &format!(r#"{{"path":"{path}","change":"update"}}"#),
    )
}

/// A timestamp `seconds` after the fixture's base, in the format vendors actually send.
fn ts(seconds: i64) -> String {
    let base = 1_770_000_000_i64;
    let total = base + seconds;
    let days = total.div_euclid(86_400);
    let secs_of_day = total.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// The inverse of the reader's own parse, so a fixture can name a date without a calendar library.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Millis for one of [`ts`]'s timestamps, for the commit side of an attribution test.
fn millis(seconds: i64) -> i64 {
    hooks::parse_timestamp_millis(&ts(seconds)).expect("the fixture timestamp parses")
}

/// A commit fact with a date and paths, nothing else.
fn commit(oid: &str, seconds: i64, paths: &[&str]) -> CommitFacts {
    CommitFacts {
        oid: oid.to_owned(),
        author_name: "agent".to_owned(),
        author_email: "agent@example.invalid".to_owned(),
        author_utc: millis(seconds),
        subject: format!("feat: {oid}"),
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        n_files: i64::try_from(paths.len()).unwrap_or(0),
        ..CommitFacts::default()
    }
}

/// Read everything after the cursor, as the pipeline would.
fn drain(reader: &Reader, cursor: &mut Cursor) -> hooks::Batch {
    reader.read_after(cursor, millis(10_000)).expect("read")
}

// ---------------------------------------------------------------------------
// Acceptance criterion 1: torn and mid-write lines
// ---------------------------------------------------------------------------

#[test]
fn a_torn_line_is_not_lost_and_is_not_double_counted() {
    let temp = Temp::new("torn");
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");

    // A complete record, then a second one caught with its JSON half-written.
    let complete = line(1, "UserPromptSubmit", "s1", &ts(0), r#"{"prompt":"first"}"#);
    let half = r#"{"harness":"pi_json","payload":{"prompt":"second"#;
    temp.append_bytes(complete.as_bytes());
    temp.terminate();
    temp.append_bytes(half.as_bytes());

    let batch = drain(&reader, &mut cursor);
    assert_eq!(batch.events.len(), 1, "the complete line is read");
    assert_eq!(batch.events[0].seq, 1);
    let torn = batch
        .torn_tail
        .expect("the partial line is reported as a torn tail");
    assert_eq!(torn.bytes, half.len() as u64);
    assert_eq!(
        cursor.offset,
        complete.len() as u64 + 1,
        "the offset stops before the partial bytes, so they are read again, not lost"
    );
    assert_eq!(
        cursor.counters.bad_lines, 0,
        "a line that is merely unfinished is not counted as damage"
    );
    assert_eq!(cursor.counters.torn_tails, 1);

    // The writer finishes the line. The next read must produce it exactly once.
    let rest = "\"},\"schema_version\":1,\"seq\":2,\"session_id\":\"s1\",\"source\":\"UserPromptSubmit\",\"ts\":\"T\",\"cwd\":\"/srv/app\"}";
    temp.append_bytes(rest.as_bytes());
    temp.terminate();
    let batch = drain(&reader, &mut cursor);
    assert_eq!(
        batch.events.len(),
        1,
        "the finished line arrives exactly once"
    );
    assert_eq!(batch.events[0].seq, 2);
    assert_eq!(batch.events[0].source, "UserPromptSubmit");
    assert!(batch.torn_tail.is_none());
    assert_eq!(cursor.last_seq, 2);
    assert_eq!(
        cursor.offset,
        temp.reader().size(),
        "the whole file is consumed"
    );
}

#[test]
fn interleaved_writers_are_read_as_separate_lines() {
    // Two writers that both appended in one kernel write each: `O_APPEND` keeps a single line atomic, so
    // what lands is whole lines in arrival order, from two different sessions.
    let temp = Temp::new("interleave");
    let a = line(1, "UserPromptSubmit", "s-a", &ts(0), "{}");
    let b = line(2, "UserPromptSubmit", "s-b", &ts(1), "{}");
    let a2 = edit(3, "s-a", &ts(2), "src/a.rs");
    temp.append(&[a, b, a2]);
    temp.terminate();

    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    let batch = drain(&reader, &mut cursor);
    assert_eq!(batch.events.len(), 3);
    assert_eq!(cursor.last_seq, 3);
    assert_eq!(
        cursor.ledger.turns.len(),
        2,
        "two sessions, so two turns, whatever the interleaving looks like"
    );
    let ids: Vec<&str> = cursor
        .ledger
        .turns
        .iter()
        .map(|t| t.turn_id.as_str())
        .collect();
    assert_eq!(ids, vec!["s-a:1", "s-b:1"]);
}

// ---------------------------------------------------------------------------
// Acceptance criterion 2: malformed events are quarantined and counted
// ---------------------------------------------------------------------------

#[test]
fn damage_is_quarantined_counted_and_stepped_over() {
    let temp = Temp::new("damage");
    let good_one = line(1, "SessionStart", "s1", &ts(0), "{}");
    let junk = "{ this is not json at all";
    let not_object = "[1,2,3]";
    let no_seq = r#"{"source":"Stop","schema_version":1}"#;
    let bad_version =
        r#"{"seq":2,"schema_version":2,"harness":"x","source":"Stop","ts":"T","payload":{}}"#;
    let no_version = r#"{"seq":3,"harness":"x","source":"Stop","ts":"T","payload":{}}"#;
    let escape = r#"{"seq":4,"schema_version":1,"harness":"x","source":"file.edited","session_id":"s1","cwd":"/srv/app","ts":"T","payload":{"path":"../../etc/shadow"}}"#;
    let good_two = line(5, "Stop", "s1", &ts(3), "{}");
    temp.append(&[
        good_one,
        junk.to_owned(),
        not_object.to_owned(),
        no_seq.to_owned(),
        bad_version.to_owned(),
        no_version.to_owned(),
        escape.to_owned(),
        good_two,
    ]);
    temp.terminate();

    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    let batch = drain(&reader, &mut cursor);

    assert_eq!(
        batch.events.len(),
        2,
        "the two good records are read; the damage is between them and stopped nothing"
    );
    assert_eq!(
        batch.events[1].seq, 5,
        "the stream kept flowing past six refusals"
    );
    let reasons: Vec<&str> = batch.rejections.iter().map(|r| r.reason.as_str()).collect();
    assert_eq!(
        reasons,
        vec![
            "undecodable",
            "not_an_object",
            "missing_seq",
            "schema_version",
            "schema_version",
            "path_escape",
        ],
        "every refusal is named: {:?}",
        batch.rejections
    );
    assert_eq!(
        cursor.counters.bad_lines, 2,
        "undecodable and not-an-object are bad lines"
    );
    assert_eq!(cursor.counters.accepted, 2);
    assert_eq!(cursor.counters.rejected_total(), 6);
    assert_eq!(
        *cursor
            .counters
            .rejections
            .get(RejectReason::PathEscape.as_str())
            .unwrap_or(&0),
        1
    );

    // The quarantine holds one record per refusal, with the diagnostics to explain it.
    let quarantined = std::fs::read_to_string(reader.quarantine_path()).expect("quarantine file");
    let records: Vec<&str> = quarantined
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(records.len(), 6, "one quarantined record per refusal");
    assert!(
        records[0].contains("undecodable") && records[0].contains("sha256"),
        "a quarantine record names the reason and carries a digest: {}",
        records[0]
    );
    assert!(
        records[5].contains("path_escape") && records[5].contains("climbs out"),
        "the escape record names the rule and the reason an operator can act on: {}",
        records[5]
    );
    // The events file itself is untouched: damage is counted and copied, never deleted (ADR-005 §3.6).
    let on_disk = std::fs::read_to_string(temp.events()).expect("events file");
    assert!(
        on_disk.contains(junk),
        "the malformed line is still in the file"
    );

    // And the health report says so.
    let health = hooks::Health::measure(&reader, &cursor, millis(4), None).expect("measure");
    assert!(!health.ok, "a refused path escape is not a healthy stream");
    assert!(
        health
            .reasons
            .iter()
            .any(|reason| reason.contains("path outside the session directory")),
        "{:?}",
        health.reasons
    );
}

#[test]
fn an_oversized_line_is_refused_whole_never_truncated() {
    let temp = Temp::new("oversized");
    let mut big = line(1, "PostToolUse", "s1", &ts(0), "{}");
    // Push one line over the reader's cap with a payload that would otherwise be accepted.
    let pad = "x".repeat(hooks::MAX_LINE_BYTES);
    big = big.replace(
        r#""payload":{}"#,
        &format!(r#""payload":{{"blob":"{pad}"}}"#),
    );
    let good = line(2, "Stop", "s1", &ts(1), "{}");
    temp.append(&[big, good]);
    temp.terminate();

    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    let batch = drain(&reader, &mut cursor);
    assert_eq!(
        batch.events.len(),
        1,
        "the oversized line is refused, the next is read"
    );
    assert_eq!(batch.events[0].seq, 2);
    assert_eq!(cursor.counters.oversized, 1);
    assert_eq!(
        batch.rejections.first().map(|r| r.reason),
        Some(RejectReason::Oversized)
    );
    let health = hooks::Health::measure(&reader, &cursor, millis(2), None).expect("measure");
    assert!(
        !health.ok,
        "an oversized line is a hard finding, not a shrug"
    );
}

#[test]
fn sequence_duplicates_are_refused_and_gaps_are_a_finding() {
    let temp = Temp::new("sequence");
    temp.append(&[
        line(1, "UserPromptSubmit", "s1", &ts(0), "{}"),
        line(2, "Stop", "s1", &ts(1), "{}"),
        // seq 3 never arrives, and a second copy of seq 2 shows up afterwards.
        line(2, "Stop", "s1", &ts(2), "{}"),
        line(6, "UserPromptSubmit", "s1", &ts(3), "{}"),
    ]);
    temp.terminate();

    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);
    assert_eq!(cursor.counters.sequence_duplicates, 1);
    assert_eq!(cursor.counters.sequence_gaps, 1);
    assert_eq!(
        cursor.counters.sequence_gap_records, 3,
        "seq 3, 4 and 5 are missing"
    );
    let health = hooks::Health::measure(&reader, &cursor, millis(4), None).expect("measure");
    assert!(!health.ok);
    assert!(
        health
            .reasons
            .iter()
            .any(|r| r.contains("missing from the sequence")),
        "{:?}",
        health.reasons
    );
}

#[test]
fn an_unprojectable_source_is_counted_not_dropped() {
    let temp = Temp::new("unprojectable");
    temp.append(&[
        line(1, "WorktreeCreate", "s1", &ts(0), "{}"),
        line(2, "Stop", "s1", &ts(1), "{}"),
    ]);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    let batch = drain(&reader, &mut cursor);
    assert_eq!(
        batch.events.len(),
        2,
        "an unknown event type is additive-safe (ADR-005 §5.1)"
    );
    assert_eq!(batch.events[0].fathom_event, FathomEvent::None);
    assert_eq!(cursor.counters.unprojectable_total(), 1);
    assert_eq!(
        cursor.counters.unprojectable.get("WorktreeCreate"),
        Some(&1),
        "and it is counted under the vendor's own name"
    );
}

#[test]
fn a_session_over_its_byte_budget_is_quarantined() {
    let temp = Temp::new("quota");
    // Two edits that together blow a tiny budget, then one more that must be refused.
    temp.append(&[
        edit(1, "s1", &ts(0), "src/a.rs"),
        edit(2, "s1", &ts(1), "src/b.rs"),
        edit(3, "s1", &ts(2), "src/c.rs"),
    ]);
    temp.terminate();
    let reader = Reader::with_limits(
        temp.events(),
        Limits {
            max_bytes_per_session: 200,
            ..Limits::default()
        },
    );
    let mut cursor = Cursor::fresh("events.jsonl");
    let batch = drain(&reader, &mut cursor);
    assert!(
        batch.events.len() < 3,
        "the run stops accepting the runaway session"
    );
    assert_eq!(cursor.counters.sessions_quarantined, 1);
    assert_eq!(cursor.ledger.quarantined_sessions(), vec!["s1".to_owned()]);
    let health = hooks::Health::measure(&reader, &cursor, millis(3), None).expect("measure");
    assert!(!health.ok, "a quarantined session is not a healthy stream");
    assert!(health.reasons.iter().any(|r| r.contains("quarantined")));
}

#[test]
fn a_secret_in_a_payload_is_redacted_before_it_is_kept() {
    let temp = Temp::new("redact");
    temp.append(&[line(
        1,
        "PostToolUse",
        "s1",
        &ts(0),
        r#"{"tool_response":{"body":"key = AKIAABCDEFGHIJKLMNOP done"}}"#,
    )]);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    let batch = drain(&reader, &mut cursor);
    let event = &batch.events[0];
    let stored = serde_json::to_string(&event.payload).expect("payload is JSON");
    assert!(
        !stored.contains("AKIAABCDEFGHIJKLMNOP"),
        "the raw key must not survive ingest: {stored}"
    );
    assert!(stored.contains("[REDACTED:aws-access-key]"), "{stored}");
    assert_eq!(cursor.counters.redacted.get("aws-access-key"), Some(&1));
}

// ---------------------------------------------------------------------------
// Acceptance criterion 3: the durable offset, and deterministic replay
// ---------------------------------------------------------------------------

#[test]
fn a_restart_resumes_without_reprocessing() {
    let temp = Temp::new("offset");
    temp.append(&[
        line(1, "UserPromptSubmit", "s1", &ts(0), "{}"),
        edit(2, "s1", &ts(1), "src/a.rs"),
    ]);
    temp.terminate();

    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    let first = drain(&reader, &mut cursor);
    assert_eq!(first.events.len(), 2);
    assert_eq!(first.bytes, reader.size());
    reader.save_cursor(&cursor).expect("save the cursor");

    // A new process, the same files: the cursor says there is nothing to do.
    let (mut reloaded, notice) = reader.open_cursor(millis(9));
    assert!(notice.is_none());
    assert_eq!(reloaded.offset, cursor.offset);
    assert_eq!(reloaded.last_seq, 2);
    assert_eq!(reloaded.counters.accepted, 2);
    let second = drain(&reader, &mut reloaded);
    assert!(
        second.events.is_empty(),
        "a restart does not reprocess the records it already counted"
    );
    assert_eq!(reloaded.counters.accepted, 2);
    assert_eq!(
        reloaded.ledger.turns.len(),
        1,
        "and it kept its turn allocation"
    );

    // New records after the restart are read once, and the turn counter continues where it stopped.
    temp.append(&[edit(3, "s1", &ts(5), "src/b.rs")]);
    temp.terminate();
    let third = drain(&reader, &mut reloaded);
    assert_eq!(third.events.len(), 1);
    assert_eq!(reloaded.last_seq, 3);
    assert_eq!(reloaded.counters.accepted, 3);
}

#[test]
fn a_corrupt_cursor_replays_instead_of_losing_the_stream() {
    let temp = Temp::new("corrupt-cursor");
    temp.append(&[
        line(1, "UserPromptSubmit", "s1", &ts(0), "{}"),
        edit(2, "s1", &ts(1), "src/a.rs"),
    ]);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);
    reader.save_cursor(&cursor).expect("save");
    std::fs::write(reader.cursor_path(), "{\"version\": 1, \"offset\": 9").expect("corrupt it");

    let (mut recovered, _notice) = reader.open_cursor(millis(10));
    assert_eq!(
        recovered.offset, 0,
        "a cursor that cannot be read restarts the read"
    );
    let batch = drain(&reader, &mut recovered);
    assert_eq!(
        batch.events.len(),
        2,
        "which is a replay, and a replay is complete"
    );
    assert_eq!(recovered.counters.accepted, 2);
}

#[test]
fn a_truncated_file_restarts_cleanly_and_says_so() {
    let temp = Temp::new("rotate");
    temp.append(&[
        line(1, "UserPromptSubmit", "s1", &ts(0), "{}"),
        edit(2, "s1", &ts(1), "src/a.rs"),
    ]);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);
    assert_eq!(cursor.last_seq, 2);

    // The operator rotates the file and a new one begins at seq 1.
    std::fs::write(
        temp.events(),
        format!(
            "{}\n",
            line(1, "UserPromptSubmit", "s2", ts(50).as_str(), "{}")
        ),
    )
    .expect("rotate");
    let batch = drain(&reader, &mut cursor);
    assert_eq!(cursor.rotations, 1);
    assert_eq!(batch.events.len(), 1, "the new file is read from its start");
    assert_eq!(cursor.last_seq, 1);
    assert!(
        batch.notices.iter().any(|n| n.kind == "rotated"),
        "and the rotation is on the report, not only in the log"
    );
}

/// A stream with two sessions, several turns each, and enough edits that attribution can be tested.
fn scenario(temp: &Temp) -> Vec<CommitFacts> {
    temp.append(&[
        line(1, "SessionStart", "s1", &ts(0), "{}"),
        line(2, "UserPromptSubmit", "s1", &ts(10), "{}"),
        edit(3, "s1", &ts(12), "src/a.rs"),
        edit(4, "s1", &ts(13), "src/b.rs"),
        line(5, "Stop", "s1", &ts(20), r#"{"stop_reason":"end_turn"}"#),
        line(6, "UserPromptSubmit", "s1", &ts(30), "{}"),
        edit(7, "s1", &ts(32), "src/c.rs"),
        line(8, "Stop", "s1", &ts(40), "{}"),
        // A second session that reaches its third turn: `s2:3` beside `s1:3`.
        line(9, "UserPromptSubmit", "s2", &ts(31), "{}"),
        edit(10, "s2", &ts(33), "src/other.rs"),
        line(11, "Stop", "s2", &ts(41), "{}"),
    ]);
    temp.terminate();
    vec![
        commit("c1", 12, &["src/a.rs"]),
        commit("c2", 13, &["src/b.rs"]),
        commit("c3", 33, &["src/other.rs"]),
        commit("c4", 32, &["src/c.rs"]),
        commit("c5", 200, &["src/none-of-this-was-touched.rs"]),
    ]
}

/// Drain a file *incrementally*, in small writes, the way a live tail would, and return the cursor.
fn drain_in_chunks(reader: &Reader, chunks: usize) -> Cursor {
    let mut cursor = Cursor::fresh("events.jsonl");
    for _ in 0..chunks {
        drain(reader, &mut cursor);
    }
    cursor
}

#[test]
fn replay_from_zero_reproduces_the_same_linkage() {
    let temp = Temp::new("replay");
    let commits = scenario(&temp);
    let reader = temp.reader();

    // One full pass.
    let replay = reader.replay(millis(100)).expect("replay");
    let linkage_once = hooks::attribute(&commits, &replay.0.ledger);

    // Again, from byte 0, in a fresh process reading a file that grew in between.
    let replay_again = reader.replay(millis(200)).expect("replay again");
    let linkage_twice = hooks::attribute(&commits, &replay_again.0.ledger);

    assert_eq!(
        linkage_once.digest, linkage_twice.digest,
        "replay from offset 0 must reproduce identical episode linkage"
    );
    assert_eq!(replay.0.counters.accepted, 11);
    assert_eq!(replay_again.0.counters.accepted, 11);

    // And the same linkage as reading the file in pieces, which is what a restart actually does.
    let incremental = drain_in_chunks(&reader, 4);
    let linkage_incremental = hooks::attribute(&commits, &incremental.ledger);
    assert_eq!(
        linkage_once.digest, linkage_incremental.digest,
        "chunked reads must not disagree with a rebuild"
    );

    // A rebuild with the commits walked in a different order gives the same digest: the digest is a
    // property of the mapping, not of the walk.
    let mut shuffled = commits.clone();
    shuffled.reverse();
    assert_eq!(
        linkage_once.digest,
        hooks::attribute(&shuffled, &replay.0.ledger).digest
    );
}

#[test]
fn intent_text_does_not_move_the_linkage_digest() {
    // The other half of `Turn::digest_fields`'s comment, asserted. A vendor that starts sending
    // `payload.prompt` where it used to send only `prompt_sha256` changes the *content* of a turn, not the
    // mapping of commits onto turns. If intent text were in the digest, that vendor upgrade would
    // invalidate every stored linkage in the repository — and ADR-003 §6 is what a review is keyed to.
    let commits = vec![commit("c1", 12, &["src/a.rs"])];

    let quiet = Temp::new("linkage-quiet");
    quiet.append(&[
        line(1, "UserPromptSubmit", "s1", &ts(10), "{}"),
        edit(2, "s1", &ts(12), "src/a.rs"),
        line(3, "Stop", "s1", &ts(20), "{}"),
    ]);
    quiet.terminate();
    let quiet_ledger = quiet
        .reader()
        .replay(millis(100))
        .expect("the quiet stream reads")
        .0
        .ledger;

    let loud = Temp::new("linkage-loud");
    loud.append(&[
        line(
            1,
            "UserPromptSubmit",
            "s1",
            &ts(10),
            r#"{"prompt":"say something the quiet stream never said","plan":["one","two"],"declared_scope":["src/**"]}"#,
        ),
        edit(2, "s1", &ts(12), "src/a.rs"),
        line(3, "Stop", "s1", &ts(20), "{}"),
    ]);
    loud.terminate();
    let loud_ledger = loud
        .reader()
        .replay(millis(100))
        .expect("the loud stream reads")
        .0
        .ledger;

    // Both streams describe the same turn, and the loud one really did capture more.
    let quiet_turn = quiet_ledger
        .turns
        .iter()
        .find(|turn| turn.session_id == "s1")
        .expect("one turn");
    let loud_turn = loud_ledger
        .turns
        .iter()
        .find(|turn| turn.session_id == "s1")
        .expect("one turn");
    assert!(
        quiet_turn.prompt.is_none() && quiet_turn.plan.is_empty(),
        "the quiet stream stated nothing: {quiet_turn:?}"
    );
    assert_eq!(
        loud_turn.prompt.as_deref(),
        Some("say something the quiet stream never said"),
        "the loud one stated a prompt"
    );
    assert_eq!(loud_turn.plan.len(), 2);
    assert_eq!(loud_turn.scope, vec!["src/**".to_owned()]);

    let quiet_link = hooks::attribute(&commits, &quiet_ledger);
    let loud_link = hooks::attribute(&commits, &loud_ledger);
    assert!(
        quiet_link.by_event > 0,
        "and the fixture actually links the commit by event, or this proves nothing: {quiet_link:?}"
    );
    assert_eq!(
        quiet_link.digest, loud_link.digest,
        "the same commits, the same turns, the same mapping — text is not linkage"
    );
}

#[test]
fn turns_are_allocated_at_ingest_and_close_on_the_next_start() {
    let temp = Temp::new("turns");
    scenario(&temp);
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);

    let ids: Vec<&str> = cursor
        .ledger
        .turns
        .iter()
        .map(|t| t.turn_id.as_str())
        .collect();
    assert_eq!(ids, vec!["s1:1", "s1:2", "s2:1"]);
    let first = cursor.ledger.turn("s1:1").expect("the first turn");
    assert_eq!(first.closed_by, ClosedBy::TurnEnd);
    assert_eq!(first.edits, 2);
    assert_eq!(first.paths.len(), 2);
    assert!(
        !first.inferred,
        "s1 turn 1 was declared by a UserPromptSubmit"
    );

    // A turn that never ends is closed by the next start, not left open forever.
    let temp2 = Temp::new("turns-crash");
    temp2.append(&[
        line(1, "UserPromptSubmit", "s9", &ts(0), "{}"),
        edit(2, "s9", &ts(1), "src/a.rs"),
        line(3, "UserPromptSubmit", "s9", &ts(9), "{}"),
        edit(4, "s9", &ts(10), "src/b.rs"),
    ]);
    temp2.terminate();
    let reader2 = temp2.reader();
    let mut cursor2 = Cursor::fresh("events.jsonl");
    drain(&reader2, &mut cursor2);
    let crashed = cursor2.ledger.turn("s9:1").expect("the crashed turn");
    assert_eq!(crashed.closed_by, ClosedBy::NextTurn);
    assert_eq!(crashed.ended_seq, Some(3), "closed by the next turn_start");
    assert_eq!(cursor2.ledger.closed_by_next, 1);
}

#[test]
fn a_harness_with_no_turn_start_still_produces_turns_and_says_so() {
    let temp = Temp::new("inferred");
    temp.append(&[
        edit(1, "s5", &ts(0), "src/a.rs"),
        edit(2, "s5", &ts(1), "src/b.rs"),
    ]);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);
    assert_eq!(cursor.ledger.turns.len(), 1);
    assert!(cursor.ledger.turn("s5:1").expect("turn").inferred);
    assert_eq!(cursor.ledger.inferred, 1, "counted, not guessed quietly");
    let health = hooks::Health::measure(&reader, &cursor, millis(2), None).expect("measure");
    assert!(
        health.warnings.iter().any(|w| w.contains("inferred")),
        "{:?}",
        health.warnings
    );
    assert!(health.ok, "inference is a warning, not a broken stream");
}

#[test]
fn a_turn_scoped_event_with_no_session_is_not_turn_evidence() {
    let temp = Temp::new("no-session");
    temp.append(&[
        r#"{"seq":1,"schema_version":1,"harness":"x","source":"Stop","ts":"T","payload":{}}"#
            .to_owned(),
    ]);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    let batch = drain(&reader, &mut cursor);
    assert!(batch.events.is_empty());
    assert_eq!(
        batch.rejections.first().map(|r| r.reason),
        Some(RejectReason::MissingSessionId)
    );
    assert!(cursor.ledger.turns.is_empty());
}

// ---------------------------------------------------------------------------
// Acceptance criterion 4: what `fathom hooks --check` reports
// ---------------------------------------------------------------------------

#[test]
fn the_health_report_names_lag_offset_counts_and_rejections() {
    let temp = Temp::new("check");
    scenario(&temp);
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);
    // Leave one line unread so lag is a real number rather than zero.
    temp.append(&[edit(12, "s1", &ts(60), "src/d.rs")]);
    temp.terminate();

    let health = hooks::Health::measure(&reader, &cursor, millis(70), None).expect("measure");
    assert!(health.ok, "{:?}", health.reasons);
    assert_eq!(health.offset, cursor.offset);
    assert_eq!(health.size_bytes, reader.size());
    assert!(health.lag_bytes > 0, "the unread line is lag");
    assert_eq!(health.lag_lines, 1);
    assert_eq!(health.last_seq_in_file, 12);
    assert_eq!(health.last_seq_read, 11);
    assert_eq!(
        health.age_seconds,
        Some(70 - 41),
        "the last read event's age"
    );
    let json = health.to_json();
    assert_eq!(json["by_event"]["file_edit"].as_u64(), Some(4));
    assert_eq!(json["by_event"]["turn_start"].as_u64(), Some(3));
    assert_eq!(json["turns"]["all_session_qualified"].as_bool(), Some(true));
    assert_eq!(json["counts"]["accepted"].as_u64(), Some(11));
    assert!(json.get("offset").is_some() && json.get("rejections").is_some());
    let text = health.lines();
    assert!(
        text.concat().contains("behind"),
        "the text form says the same thing as the JSON: {text:?}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance criteria 5 and 6: session-qualified ids, and rung 3's key
// ---------------------------------------------------------------------------

#[test]
fn two_sessions_turn_three_are_two_turns() {
    // ADR-003 §18.2 D4: the collision s2 found in the corpus is `s1:3` beside `s2:3`.
    assert_ne!(
        hooks::qualify_turn_id("s1", 3),
        hooks::qualify_turn_id("s2", 3),
        "a bare turn index is the defect, not the id"
    );
    let (session, index) = hooks::parse_turn_id("s1:3").expect("qualified ids parse back");
    assert_eq!((session.as_str(), index), ("s1", 3));
    assert!(
        hooks::parse_turn_id("3").is_none(),
        "an unqualified id is not a turn id, so nothing can quietly emit one"
    );

    // And the qualification is what makes the boundary fire: two sessions' turn 3, adjacent in the walk,
    // are two episodes.
    let a = CommitFacts {
        oid: "a".to_owned(),
        author_utc: millis(0),
        subject: "feat: one".to_owned(),
        turn_id: Some(hooks::qualify_turn_id("s1", 3)),
        session_id: Some("s1".to_owned()),
        ..CommitFacts::default()
    };
    let b = CommitFacts {
        oid: "b".to_owned(),
        author_utc: millis(1),
        subject: "feat: two".to_owned(),
        turn_id: Some(hooks::qualify_turn_id("s2", 3)),
        session_id: Some("s2".to_owned()),
        ..CommitFacts::default()
    };
    let (starts, rule, confidence) = segment::decide(&a, &b, &Rules::default());
    assert!(starts, "two sessions are two turns are two episodes");
    assert_eq!(rule, Rule::Hook);
    assert_eq!(confidence, 1.0);
}

#[test]
fn rung_three_fires_on_the_trailer_this_ingest_writes() {
    // The writer's key and rung 3's key are the same constant, so this is a type-level fact — asserted
    // anyway, because the acceptance criterion is about a *name* being shared across two modules and a
    // rename is exactly the kind of change that passes review.
    assert_eq!(
        TRAILER_TURN.to_ascii_lowercase(),
        TRAILER_TURN_KEY,
        "the emitted spelling and the read key are one key, case-insensitively as git stores trailers"
    );

    let turn = hooks::TurnRef {
        session_id: "s7".to_owned(),
        turn_index: 4,
        turn_id: hooks::qualify_turn_id("s7", 4),
    };
    let trailer_text = hooks::trailer_block(&turn);
    assert_eq!(trailer_text, "Agent-Session: s7\nAgent-Turn: s7:4\n");

    // Read the emitted trailer back the way the index pass does — git's own trailer parsing, keys
    // lower-cased — and hand it to the cascade.
    let trailers = fathom_ingest::record::Trailers::parse(
        format!("feat: the change\n\n{trailer_text}").as_bytes(),
    );
    let map = trailers.to_map();
    let commit = CommitFacts {
        oid: "c1".to_owned(),
        author_utc: millis(0),
        subject: "feat: the change".to_owned(),
        trailers: map.clone(),
        ..CommitFacts::default()
    };
    let previous = CommitFacts {
        oid: "c0".to_owned(),
        author_utc: millis(0),
        subject: "feat: the one before".to_owned(),
        ..CommitFacts::default()
    };
    let (starts, rule, confidence) = segment::decide(&previous, &commit, &Rules::default());
    assert!(starts, "a trailer naming a new turn opens an episode");
    assert_eq!(
        rule,
        Rule::Trailer,
        "and it is rung 3 that fired, not an accident of another rule"
    );
    assert_eq!(confidence, 0.9);

    // Two commits carrying the same qualified turn id are one episode: rung 3 says "same", and rung 2 has
    // no opinion because neither commit's `turn_id` field was filled by an event.
    let again = CommitFacts {
        oid: "c2".to_owned(),
        author_utc: millis(1),
        subject: "fix: the follow-up".to_owned(),
        trailers: map.clone(),
        ..CommitFacts::default()
    };
    let (starts, rule, _confidence) = segment::decide(&commit, &again, &Rules::default());
    assert!(
        !starts,
        "the same turn does not open a second episode: {:?}",
        rule
    );
}

#[test]
fn events_become_commit_turns_and_the_hook_rung_fires() {
    let temp = Temp::new("linkage");
    let commits = scenario(&temp);
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);

    let linkage = hooks::attribute(&commits, &cursor.ledger);
    assert_eq!(
        linkage.by_event, 4,
        "four commits have path-and-window evidence"
    );
    assert_eq!(
        linkage.unmapped, 1,
        "the untouched commit is not attributed"
    );
    assert_eq!(
        linkage.link("c1").expect("c1").turn_id.as_deref(),
        Some("s1:1"),
        "{:?}",
        linkage.links
    );
    assert_eq!(
        linkage.link("c3").expect("c3").turn_id.as_deref(),
        Some("s2:1")
    );
    assert!(
        linkage.links.iter().all(|link| link
            .turn_id
            .as_deref()
            .is_none_or(|id| hooks::parse_turn_id(id).is_some())),
        "every emitted turn id is session-qualified: {:?}",
        linkage.links
    );

    // Fill the facts the way the index pass would, and segment.
    let mut facts = commits.clone();
    let filled = hooks::fill_turn_ids(&mut facts, &linkage);
    assert_eq!(filled, 4);
    let episodes = segment::segment(&facts, &Rules::default());
    let rules: Vec<Rule> = episodes.iter().map(|episode| episode.rule).collect();
    assert_eq!(
        rules,
        vec![Rule::Root, Rule::Hook, Rule::Hook],
        "every boundary the harness could name fired at confidence 1.0: {episodes:?}"
    );
    assert_eq!(
        episodes[2].members,
        vec!["c4".to_owned(), "c5".to_owned()],
        "c5 has no turn evidence, so it is grouped by the git heuristics and the episode says so"
    );
    assert_eq!(episodes[0].turn_id.as_deref(), Some("s1:1"));
    assert_eq!(episodes[1].turn_id.as_deref(), Some("s2:1"));
    assert_eq!(episodes[2].turn_id.as_deref(), Some("s1:2"));
}

#[test]
fn a_trailer_that_disagrees_with_the_stream_is_counted_and_the_stream_wins() {
    let temp = Temp::new("conflict");
    let mut commits = scenario(&temp);
    // c4 really belongs to `s1:2` by path and window; its trailer claims something else.
    commits[3]
        .trailers
        .insert(TRAILER_TURN_KEY.to_owned(), "somewhere:else".to_owned());
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);
    let linkage = hooks::attribute(&commits, &cursor.ledger);
    let link = linkage.link("c4").expect("c4");
    assert_eq!(
        link.turn_id.as_deref(),
        Some("s1:2"),
        "ADR-003 §9.2: the event wins"
    );
    assert!(link.conflict);
    assert_eq!(linkage.conflicts, 1);
}

#[test]
fn a_test_command_is_reported_even_when_nothing_recognised_it() {
    // ADR-005 §2.2: an unrecognised command surfaces as `test_run` with `detected_by: none` rather than
    // vanishing, because a detector that misses half the runs flatters the loop it measures. The four
    // spellings are §2.2's own words, because they are a published value of a published field.
    assert_eq!(
        hooks::detect_test_run("cargo test --workspace"),
        Some(hooks::DetectedBy::Pattern),
        "`cargo test` is a named test *target*, not a runner's name"
    );
    assert_eq!(
        hooks::detect_test_run("pytest -q"),
        Some(hooks::DetectedBy::Name),
        "`pytest` first is a runner's name"
    );
    assert_eq!(
        hooks::detect_test_run("cargo nextest run"),
        Some(hooks::DetectedBy::Heuristic),
        "recognisable as a test invocation, and labelled as the heuristic rather than quietly promoted"
    );
    assert_eq!(
        hooks::detect_test_run("./scripts/check-something.sh"),
        Some(hooks::DetectedBy::None)
    );
    assert_eq!(
        hooks::detect_test_run("cargo build"),
        None,
        "a build is not a test run"
    );
    assert_eq!(
        hooks::detect_test_run("ls -la"),
        Some(hooks::DetectedBy::None),
        "`detect_test_run` classifies *any* command, because §2.2 wants the unrecognised ones counted; what          does not count as a test run is `none` — see the histogram test below"
    );

    let temp = Temp::new("test-run");
    temp.append(&[
        line(1, "UserPromptSubmit", "s1", &ts(0), "{}"),
        line(
            2,
            "PostToolUse",
            "s1",
            &ts(1),
            r#"{"tool_input":{"command":"cargo test --lib"}}"#,
        ),
        line(
            3,
            "PostToolUse",
            "s1",
            &ts(2),
            r#"{"tool_input":{"command":"./weird-runner"}}"#,
        ),
    ]);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);
    let turn = cursor.ledger.turn("s1:1").expect("the turn");
    assert_eq!(turn.test_runs.get("pattern"), Some(&1));
    assert_eq!(
        turn.test_runs.get("none"),
        Some(&1),
        "and the unrecognised command is in there too, as `detected_by: none` — §2.2's requirement that a          missed detection surfaces instead of vanishing"
    );
}

#[test]
fn the_canonical_digest_matches_the_reference_writer() {
    // `canonical_sha256` in `src/recommender/harness/harness.py` is
    // sha256(json.dumps(obj, sort_keys=True, separators=(",", ":"))). The digest is the evidence chain
    // (ADR-005 §6): if the two implementations hashed different bytes, a hash written by an adapter would
    // be unverifiable here. The literal below was produced by the Python side; `tests/test_hooks_contract.py`
    // recomputes it and fails if either side's canonical form changes.
    let value: serde_json::Value =
        serde_json::from_str(r#"{"b": [1, 2], "a": "café", "tool": "edit"}"#)
            .expect("fixture json");
    assert_eq!(
        hooks::digest_bytes(&hooks::canonical_bytes(&value)),
        "c05e4040e76e3259450b23ce050ba8e9636567b4ac264c3c4b39a76cfdb259fa",
        "canonical form drifted from the reference writer"
    );
}

#[test]
fn unknown_fields_are_preserved_verbatim() {
    let temp = Temp::new("extra");
    temp.append(&[format!(
        r#"{{"seq":1,"schema_version":1,"harness":"pi_json","source":"SessionStart","session_id":"s1","ts":"{}","payload":{{}},"vendor_future_field":{{"kept":true}}}}"#,
        ts(0)
    )]);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    let batch = drain(&reader, &mut cursor);
    let event: &Event = &batch.events[0];
    assert_eq!(
        event
            .extra
            .get("vendor_future_field")
            .and_then(|v| v.get("kept")),
        Some(&serde_json::Value::Bool(true)),
        "an additive vendor field survives to whoever needs it (ADR-005 §5.1)"
    );
}

#[test]
fn path_containment_refuses_every_shape_that_escapes() {
    for escape in [
        "../outside",
        "src/../../etc/passwd",
        "/etc/passwd",
        "~/.ssh/id_ed25519",
        "src/\u{0}hidden",
    ] {
        assert!(
            hooks::contain_path("/srv/app", escape).is_err(),
            "`{escape}` must not be accepted"
        );
    }
    assert_eq!(
        hooks::contain_path("/srv/app", "src/app.rs").ok(),
        Some("src/app.rs".to_owned())
    );
    assert_eq!(
        hooks::contain_path("/srv/app", "/srv/app/src/app.rs").ok(),
        Some("/srv/app/src/app.rs".to_owned()),
        "an absolute path inside the session directory is the same path spelled differently"
    );
    assert!(
        hooks::contain_path("/srv/app", "/srv/apple/x.rs").is_err(),
        "a prefix is not a directory"
    );
}

#[test]
fn a_contained_path_comes_back_in_the_spellings_git_uses() {
    // `attribute` matches the answer against a commit's git paths, and git paths are `/`-separated on
    // every platform. The old normalisation joined with the host separator instead, so on Windows this
    // returned `src\app.rs`, nothing ever matched, and every agent turn on that platform lost its intent
    // while the tally blamed the harness (`reco-raz3`). These three lines are the promise, and on
    // Windows they are the red-before-green guard.
    assert_eq!(
        hooks::contain_path("/srv/app", "./src/./app.rs/").ok(),
        Some("src/app.rs".to_owned()),
        "`./` goes, `.` goes, the trailing slash goes"
    );
    assert_eq!(
        hooks::contain_path("/srv/app", "src/deep/nested/app.rs").ok(),
        Some("src/deep/nested/app.rs".to_owned()),
        "a nested path keeps every separator as `/`"
    );
    assert_eq!(
        hooks::contain_path("/srv/app", "/srv/app/src/app.rs").ok(),
        Some("/srv/app/src/app.rs".to_owned()),
        "the root of an absolute answer is `/`, not the host's rendering of a root"
    );
}

#[cfg(not(windows))]
#[test]
fn a_backslash_is_an_ordinary_character_where_it_is_one() {
    // The fold the next two tests assert is a property of Windows, not a translation this reader
    // performs. On a POSIX platform `a\b` is one file with a backslash in its name, and renaming it to
    // `a/b` would be a lie about the file an edit touched.
    assert_eq!(
        hooks::contain_path("/srv/app", "src\\app.rs").ok(),
        Some("src\\app.rs".to_owned()),
        "the name survives untouched"
    );
    assert_eq!(
        hooks::contain_path("/srv/app", "notes\\").ok(),
        Some("notes\\".to_owned()),
        "and a name that ends in one is a name, not a trailing separator"
    );
}

#[cfg(windows)]
#[test]
fn a_stream_written_on_windows_is_read_like_every_other_stream() {
    // Windows is where a writer and a git path can disagree about nothing but the separator:
    // `os.path.relpath`, `path.relative` and `Path::components` all spell a relative path with `\` there,
    // while git's are always `/`. `\` and `/` are one separator on this platform, so one spelling arrives
    // at the store — and a drive root still contains what is under it (`reco-raz3`). The last assertion is
    // the one only this platform can make: elsewhere `C:\repo2` is not an absolute path at all, so there is
    // no containment to refuse.
    assert_eq!(
        hooks::contain_path(r"C:\repo", r"src\app.rs").ok(),
        Some("src/app.rs".to_owned()),
        "the writer's separator is not the store's"
    );
    assert_eq!(
        hooks::contain_path(r"C:\repo", r"C:\repo\src\app.rs").ok(),
        Some("C:/repo/src/app.rs".to_owned()),
        "an absolute path inside the session directory is the same path spelled differently"
    );
    assert!(
        hooks::contain_path(r"C:\repo", r"C:\repo2\x.rs").is_err(),
        "a prefix is not a directory, on any platform"
    );
}

#[cfg(windows)]
#[test]
fn a_turn_written_with_windows_separators_still_owns_its_commit() {
    // The same claim one level up, at the matching itself: a stream that spells its paths with `\` must
    // attribute the commit that touched `src/card.rs`, because on this platform those are one path.
    let temp = Temp::new("win-separators");
    temp.append(&[
        line(1, "SessionStart", "s1", &ts(0), "{}"),
        line(
            2,
            "UserPromptSubmit",
            "s1",
            &ts(10),
            r#"{"prompt":"the card scrolls"}"#,
        ),
        // `\\` in the JSON, so the value the reader parses is one backslash: what `os.path.relpath`
        // writes on this platform.
        edit(3, "s1", &ts(12), r"src\\card.rs"),
        line(4, "Stop", "s1", &ts(20), "{}"),
    ]);
    temp.terminate();

    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);
    let linkage = hooks::attribute(&[commit("c1", 13, &["src/card.rs"])], &cursor.ledger);
    assert_eq!(
        linkage.by_event, 1,
        "the path the turn recorded is the path the commit touched: {:?}",
        linkage.links
    );
}

#[test]
fn the_timestamp_parser_accepts_what_vendors_send() {
    assert_eq!(
        hooks::parse_timestamp_millis("2026-02-11T10:00:00.000Z"),
        hooks::parse_timestamp_millis("2026-02-11T10:00:00Z")
    );
    assert_eq!(
        hooks::parse_timestamp_millis("2026-02-11T11:00:00+01:00"),
        hooks::parse_timestamp_millis("2026-02-11T10:00:00Z"),
        "an offset is a spelling, not a different instant"
    );
    assert!(hooks::parse_timestamp_millis("10:00").is_none());
    assert!(hooks::parse_timestamp_millis("2026-13-01T00:00:00Z").is_none());
    assert_eq!(
        hooks::parse_timestamp_millis("1970-01-01T00:00:01Z"),
        Some(1000)
    );
}

/// ADR-005 §3.3's canonical stored record carries **no** `payload` key, and the reference reader defaults a
/// missing one to `{}`. Refusing it would quarantine the ADR's own example, which is the kind of bug only a
/// test written against the example catches.
#[test]
fn an_absent_payload_is_an_empty_payload_not_a_rejection() {
    let temp = Temp::new("no-payload");
    // Byte for byte §3.3's "Agent turn starts" example, plus the `session_end` it shows.
    let lines = vec![
        r#"{"schema_version": 1, "seq": 1, "source": "turn_start", "harness": "codex-cli/0.19", "session_id": "s7", "cwd": "/home/dev/app", "git_branch": "main", "ts": "2026-07-28T14:43:32.418Z"}"#.to_owned(),
        r#"{"schema_version": 1, "seq": 2, "source": "session_end", "harness": "codex-cli/0.19", "session_id": "s7", "cwd": "/home/dev/app", "ts": "2026-07-28T14:51:39.202Z"}"#.to_owned(),
    ];
    temp.append(&lines);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    let batch = drain(&reader, &mut cursor);

    assert!(
        batch.rejections.is_empty(),
        "the ADR's own example is refused: {:?}",
        batch
            .rejections
            .iter()
            .map(|r| (&r.reason, &r.detail))
            .collect::<Vec<_>>()
    );
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.events[0].fathom_event, FathomEvent::TurnStart);
    assert_eq!(batch.events[0].payload, serde_json::json!({}));
    assert_eq!(batch.events[1].fathom_event, FathomEvent::SessionEnd);
    // And the turn the example describes is a real turn: 1 open, then closed by the session end.
    assert_eq!(cursor.ledger.turns.len(), 1);
    assert_eq!(cursor.ledger.turns[0].closed_by, ClosedBy::SessionEnd);
    assert_eq!(cursor.counters.unprojectable_total(), 0);
}

/// Validate one record and return its projection, failing the test with the rejection's own words if the
/// reader refused it.
fn projected(record: serde_json::Value) -> FathomEvent {
    hooks::validate_record(&record, 0, 10)
        .map(|event| event.fathom_event)
        .unwrap_or_else(|reject| panic!("refused as {}: {}", reject.reason, reject.detail))
}

/// A stored line's `source` may already be a canonical name — that is what §3.3's example writes, and what
/// happens when one build's output is replayed into another. A reader that could not read its own canonical
/// names would report its own examples as unprojectable.
#[test]
fn a_stored_line_naming_a_canonical_event_reads_as_itself() {
    for (name, expected) in [
        ("session_start", FathomEvent::SessionStart),
        ("turn_start", FathomEvent::TurnStart),
        ("turn_end", FathomEvent::TurnEnd),
        ("tool_call", FathomEvent::ToolCall),
        ("tool_result", FathomEvent::ToolResult),
        ("file_edit", FathomEvent::FileEdit),
        ("test_run", FathomEvent::TestRun),
        ("approval", FathomEvent::Approval),
        ("error", FathomEvent::Error),
        ("subagent_start", FathomEvent::SubagentStart),
        ("subagent_end", FathomEvent::SubagentEnd),
        ("session_end", FathomEvent::SessionEnd),
    ] {
        assert_eq!(
            projected(serde_json::json!({
                "schema_version": 1, "seq": 1, "source": name, "harness": "x/1",
                "session_id": "s1", "ts": "2026-07-28T14:43:32.418Z"
            })),
            expected,
            "`{name}` in the `source` position must project to itself"
        );
    }
}

/// A writer that states `fathom_event` is stating a fact about itself, and an unknown claim is dropped
/// rather than stored — the field the index groups on stays a closed vocabulary.
#[test]
fn a_claimed_fathom_event_is_honoured_when_this_build_knows_it() {
    let record = |name: &str| {
        projected(serde_json::json!({
            "schema_version": 1, "seq": 1, "source": "SomeVendorThing", "harness": "x/1",
            "session_id": "s1", "ts": "2026-07-28T14:43:32.418Z", "fathom_event": name
        }))
    };
    assert_eq!(
        record("test_run"),
        FathomEvent::TestRun,
        "a vendor that names its own event gets believed"
    );
    assert_eq!(
        record("banana"),
        FathomEvent::None,
        "an event type this build has never heard of is not allowed into the field the index groups on"
    );
}

#[test]
fn the_test_run_histogram_is_a_separate_partition_and_not_a_second_counting_of_the_same_events() {
    // ADR-005 §11.1 publishes both `events.by_event` and `events.test_run`, and says nothing about whether
    // a derived test run appears in both. Counting it in both would make `by_event` sum to more than
    // `accepted`, which is the kind of histogram nobody notices is wrong until someone adds the column.
    // So: `by_event` is a partition of the accepted count, and `test_run` is a *separate* view of one
    // slice of it, keyed by outcome (pass / fail / none) rather than by detection method.
    let temp = Temp::new("test-run-histogram");
    temp.append(&[
        line(1, "UserPromptSubmit", "s1", &ts(0), "{}"),
        line(
            2,
            "PostToolUse",
            "s1",
            &ts(1),
            r#"{"tool_input":{"command":"cargo test --lib"},"exit_code":0}"#,
        ),
        line(
            3,
            "PostToolUse",
            "s1",
            &ts(2),
            r#"{"tool_input":{"command":"pytest -q"},"exit_code":1}"#,
        ),
        line(
            4,
            "PostToolUse",
            "s1",
            &ts(3),
            r#"{"tool_input":{"command":"cargo nextest run"}}"#,
        ),
        line(
            5,
            "PostToolUse",
            "s1",
            &ts(4),
            r#"{"tool_input":{"command":"ls -la"}}"#,
        ),
        line(6, "UserPromptSubmit", "s1", &ts(5), "{}"),
    ]);
    temp.terminate();
    let reader = temp.reader();
    let mut cursor = Cursor::fresh("events.jsonl");
    drain(&reader, &mut cursor);

    let health = hooks::Health::measure(&reader, &cursor, millis(6), None).expect("measure");
    let by_event = &health.counters.by_event;
    let test_run = &health.counters.test_run;

    assert_eq!(
        health.counters.accepted, 6,
        "five tool/prompt events plus the second turn start"
    );
    assert_eq!(
        by_event.values().sum::<u64>(),
        health.counters.accepted,
        "`by_event` must sum to `accepted`, so it stays checkable: {by_event:?}"
    );
    assert_eq!(by_event.get("tool_result"), Some(&1), "only the `ls`");
    assert_eq!(by_event.get("turn_start"), Some(&2), "both prompt submits");
    assert_eq!(
        by_event.get("test_run"),
        Some(&3),
        "the three recognised runs"
    );
    assert_eq!(
        test_run.get("pass"),
        Some(&1),
        "`cargo test --lib` exited 0: {test_run:?}"
    );
    assert_eq!(test_run.get("fail"), Some(&1), "`pytest -q` exited 1");
    assert_eq!(
        test_run.get("none"),
        Some(&1),
        "`cargo nextest run` reported no exit code"
    );
    assert_eq!(
        test_run.values().sum::<u64>(),
        by_event.get("test_run").copied().unwrap_or(0),
        "the outcome histogram covers exactly the recognised test runs, no more and no fewer"
    );
    assert_eq!(
        health.counters.unrecognised_commands, 1,
        "the `ls` is not a test run, but §2.2 says a missed detection must not vanish: it is counted here \
         so an optimistic detector cannot quietly flatter the loop"
    );
}

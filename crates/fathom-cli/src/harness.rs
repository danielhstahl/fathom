//! `harness.rs` — the `fathom hooks` verb: the harness event stream's health, replay and trailer writer
//! (`e6.1`, `reco-afh`).
//!
//! The reading, validating, quarantining and turn-allocation all live in
//! [`fathom_ingest::hooks`]; this module owns the three things a *verb* owns and nothing else: where the
//! files are ([`locate`], with ADR-005 §3.1's resolution order), what to print, and which exit code to
//! leave with (ADR-002 §7.1, §7.3).
//!
//! # The actions
//!
//! | Action | Mutates? | Prints |
//! |---|---|---|
//! | [`Action::Check`] (the default) | no — reading the dashboard must not consume the queue | lag, offset, counts by type, rejection reasons, verdict |
//! | [`Action::Replay`] | yes: rebuilds the cursor from byte 0 | the same report, plus the linkage digest that proves a rebuild reproduced itself |
//! | [`Action::Tail`] | yes: advances the durable offset | the records between the offset and the end of the file |
//! | [`Action::Trailer`] | no | the `Agent-Session:` / `Agent-Turn:` pair for one turn (ADR-003 §18.2 **D5**: this ticket is the writer) |
//!
//! # Exit codes
//!
//! `--check` is a diagnostic and so exits 0 with `ok: false` rather than raising, exactly as ADR-005 §11.1
//! rules its own status payload — a non-zero exit buried in a diagnostic is what `fathom doctor` exists to
//! interpret. `--strict` is the opt-in a CI gate wants, and it leaves with `7` (`CHECK_VIOLATIONS`), the
//! published code for "findings above the threshold".

use std::path::{Path, PathBuf};

use crate::verbs::Pending;
use fathom_core::exit::ExitCode;
use fathom_ingest::hooks::{self, Cursor, Limits, Reader};

use crate::output;
use crate::verbs::HooksArgs;

/// The contract name `--json` prints, so a consumer can branch on the shape and not on the binary's
/// version string.
pub const CONTRACT: &str = "fathom.cli.hooks/1";

/// What the verb was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Report stream health.
    Check,
    /// Rebuild from byte 0 and report.
    Replay,
    /// Read forward from the durable offset.
    Tail,
    /// Emit the trailer pair for one turn.
    Trailer,
}

/// Where the stream is, and what the reader enforces about it.
pub struct Located {
    /// The events file.
    pub events: PathBuf,
    /// The limits, resolved from `fathom.toml`.
    pub limits: Limits,
    /// One line per config key that was read and could not be honoured.
    pub notices: Vec<String>,
}

/// Resolve the events file the way ADR-005 §3.1 orders it: explicit `--events`, then the environment, then
/// `paths.events` from `fathom.toml`, then `<root>/.fathom/events.jsonl`.
///
/// The environment names are the ADR's (`RECO_EVENTS_FILE`, `RECO_EVENTS_DIR`) *and* the `FATHOM_` spellings,
/// because the reference writer in `src/recommender/harness/harness.py` honours the ADR's and a reader that
/// disagreed with the writer about where the file lives would be a bug report waiting to happen.
#[must_use]
pub fn locate(root: &Path, explicit: Option<&str>, config: &Configured) -> Located {
    let mut notices = config.notices.clone();
    let path = explicit
        .map(PathBuf::from)
        .or_else(|| env_path("FATHOM_EVENTS_FILE").or_else(|| env_path("RECO_EVENTS_FILE")))
        .or_else(|| {
            env_dir("FATHOM_EVENTS_DIR")
                .or_else(|| env_dir("RECO_EVENTS_DIR"))
                .map(|dir| dir.join("events.jsonl"))
        })
        .or_else(|| {
            config
                .events
                .as_ref()
                .map(|path| resolve_relative(root, path))
        })
        .unwrap_or_else(|| root.join(".fathom").join("events.jsonl"));
    if config.events.is_some() && explicit.is_none() {
        notices.push("paths.events came from fathom.toml".to_owned());
    }
    let limits = Limits {
        max_bytes_per_session: config
            .max_bytes_per_session_mib
            .map(|mib| (mib as u64) * 1024 * 1024)
            .unwrap_or(hooks::DEFAULT_MAX_BYTES_PER_SESSION),
        ..Limits::default()
    };
    Located {
        events: path,
        limits,
        notices,
    }
}

/// The `fathom.toml` keys this verb reads. Absent is normal, not a default that has to be announced.
#[derive(Debug, Default, Clone)]
pub struct Configured {
    /// `paths.events`.
    pub events: Option<String>,
    /// `harness.max_bytes_per_session_mib`.
    pub max_bytes_per_session_mib: Option<i64>,
    /// Keys that were read and could not be used.
    pub notices: Vec<String>,
}

/// Read `[paths] events` and `[harness] max_bytes_per_session_mib` out of a `fathom.toml` document.
///
/// `fathom-cli` is the crate ADR-002 §4.5 lets read configuration, so the values leave here as data and
/// `fathom_ingest::hooks` never opens the file.
#[must_use]
pub fn config_from_toml(text: &str) -> Configured {
    let mut out = Configured::default();
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(err) => {
            out.notices.push(format!(
                "fathom.toml does not parse, so the event stream used its default location: {err}"
            ));
            return out;
        }
    };
    if let Some(events) = document
        .get("paths")
        .and_then(toml_edit::Item::as_table)
        .and_then(|table| table.get("events"))
        .and_then(toml_edit::Item::as_value)
        .and_then(toml_edit::Value::as_str)
    {
        out.events = Some(events.to_owned());
    }
    if let Some(mib) = document
        .get("harness")
        .and_then(toml_edit::Item::as_table)
        .and_then(|table| table.get("max_bytes_per_session_mib"))
        .and_then(toml_edit::Item::as_value)
        .and_then(toml_edit::Value::as_integer)
    {
        if mib <= 0 {
            out.notices.push(format!(
                "harness.max_bytes_per_session_mib = {mib} is not a budget; the default {} MiB was used",
                hooks::DEFAULT_MAX_BYTES_PER_SESSION / (1024 * 1024)
            ));
        } else {
            out.max_bytes_per_session_mib = Some(mib);
        }
    }
    out
}

/// The event stream, in the shape an index run reads (`e4.2`, `reco-c54`).
pub struct Stream {
    /// The turns the stream describes, or `None` when there is no stream at all.
    pub ledger: Option<hooks::Ledger>,
    /// Where it was looked for, which is printed even when nothing was there — "no events file" is a fact
    /// about a path, and a report that does not name the path cannot be checked.
    pub events: PathBuf,
    /// Events the reader accepted, so a run report can say how much evidence the intent came from.
    pub accepted: u64,
    /// Turns the ledger holds, trimmed ones excluded.
    pub turns: usize,
    /// Lines the reader refused. Kept separate from `turns` because "the file is empty" and "the file is
    /// unreadable" are different reasons for the same zero, and only one of them is a state a reviewer
    /// should be told about in the same words.
    pub refused: u64,
    /// One line per thing that was read and could not be used.
    pub notices: Vec<String>,
}

/// Read the stream for an index run: from byte 0, read-only, and without touching the durable cursor.
///
/// Three rules, each one someone would otherwise get wrong:
///
/// * **It does not consume.** The offset belongs to whoever owns the queue; an index that advanced it would
///   silently eat the events a watcher had not accounted for yet — the same argument [`Action::Check`]
///   is built on.
/// * **It quarantines nothing.** A diagnostic or an index that writes the sidecar re-records the same
///   damage on every run and pushes the real records out of a bounded file.
/// * **Absence is normal.** Most repositories have no adapter installed. `no stream` is printed as a
///   state, and the trailer fallback is the answer, not a consolation prize.
#[must_use]
pub fn stream(root: &Path, now: i64) -> Stream {
    let configured = configured(root);
    let located = locate(root, None, &configured);
    let mut notices = located.notices;
    if !located.events.exists() {
        return Stream {
            ledger: None,
            events: located.events,
            accepted: 0,
            turns: 0,
            refused: 0,
            notices,
        };
    }
    let reader = Reader::with_limits(
        &located.events,
        Limits {
            write_quarantine: false,
            ..located.limits.clone()
        },
    );
    match reader.replay(now) {
        Ok((cursor, batch)) => {
            // A stream that exists and yields nothing is two different facts, and the index line must not
            // print the reassuring one. "no stream at X" is reserved for a path that is not there; a file
            // whose lines were all refused says so, with the reader's own already-sanitized reason.
            if !batch.rejections.is_empty() {
                let first = &batch.rejections[0];
                notices.push(format!(
                    "the event stream at {} refused {} of {} line(s); first: {}",
                    located.events.display(),
                    batch.rejections.len(),
                    batch.lines,
                    first.detail
                ));
            }
            Stream {
                turns: cursor.ledger.turns.len(),
                ledger: Some(cursor.ledger),
                events: located.events,
                accepted: batch.events.len() as u64,
                refused: u64::try_from(batch.rejections.len()).unwrap_or(u64::MAX),
                notices,
            }
        }
        Err(err) => {
            notices.push(format!(
                "the event stream at {} could not be read: {err}",
                located.events.display()
            ));
            Stream {
                ledger: None,
                events: located.events,
                accepted: 0,
                turns: 0,
                refused: 0,
                notices,
            }
        }
    }
}

/// Read the repository's own `fathom.toml`, if it has one.
#[must_use]
pub fn configured(root: &Path) -> Configured {
    match std::fs::read_to_string(root.join("fathom.toml")) {
        Ok(text) => config_from_toml(&text),
        Err(_) => Configured::default(),
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|text| !text.is_empty())
        .map(PathBuf::from)
}

fn env_dir(key: &str) -> Option<PathBuf> {
    env_path(key)
}

/// A configured path is relative to the repository root unless it is absolute (ADR-005 §3.1).
fn resolve_relative(root: &Path, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }
    root.join(candidate)
}

/// Run `fathom hooks`.
pub fn run(args: &HooksArgs) -> eyre::Result<ExitCode> {
    let root = Path::new(&args.root);
    let configured = configured(root);
    let located = locate(root, args.events.as_deref(), &configured);
    let redactor = crate::sanitize::Redactor::for_root(root);
    let reader = Reader::with_limits(&located.events, located.limits.clone());
    let now = crate::clock::now_millis();

    if let Some(spec) = &args.trailer {
        return trailer(spec, args.json);
    }

    let action = if args.replay {
        Action::Replay
    } else if args.tail {
        Action::Tail
    } else {
        Action::Check
    };

    // Which cursor to start from is the whole difference between the three read actions: `check` reads the
    // stored one and writes nothing back, `tail` advances it, `replay` throws it away and rebuilds it from
    // byte 0.
    let mut probe_note: Option<hooks::Probe> = None;
    let (mut cursor, open_notice) = if action == Action::Replay {
        let (cursor, _batch) = reader.replay(now).map_err(failure)?;
        (cursor, None)
    } else {
        let (cursor, notice) = reader.open_cursor(now);
        (cursor, notice)
    };
    let batch = if action == Action::Tail {
        Some(reader.read_after(&mut cursor, now).map_err(failure)?)
    } else {
        None
    };
    if action != Action::Check {
        reader.save_cursor(&cursor).map_err(failure)?;
    }

    // A check reads the unread region into a *copy* of the cursor. Reporting must not consume: the watcher
    // owns the offset, and a monitoring command that advanced it would silently eat the events it was asked
    // to report on (ADR-005 §11.1's status is a diagnostic for exactly this reason).
    if action == Action::Check {
        let probe_reader = Reader::with_limits(
            &located.events,
            Limits {
                write_quarantine: false,
                ..located.limits.clone()
            },
        );
        let before = cursor.ledger.clone();
        let mut probe_cursor = cursor.clone();
        if let Ok(batch) = probe_reader.read_after(&mut probe_cursor, now) {
            probe_note = Some(hooks::Probe::of(&batch, &before, &probe_cursor.ledger));
        }
    }

    let mut health = hooks::Health::measure(&reader, &cursor, now, None).map_err(failure)?;
    if let Some(probe) = probe_note {
        health.note_probe(probe);
    }
    // Every path this report prints goes through the CLI's one redactor, so a bug report about the stream
    // carries the same repo-relative spelling as a bug report about anything else (ADR-002 §11.1).
    health.events_file = redactor.path(&reader.events);
    health.cursor_file = redactor.path(&reader.cursor_path());
    health.quarantine_file = redactor.path(&reader.quarantine_path());
    let code = if args.strict && !health.ok {
        ExitCode::CheckViolations
    } else {
        ExitCode::Ok
    };

    if args.json {
        let stream = report_json(
            &reader,
            &redactor,
            &health,
            &cursor,
            action,
            args.strict,
            code,
        );
        output::line(format_args!(
            "{}",
            serde_json::json!({
                "contract": CONTRACT,
                "verb": "fathom hooks",
                "action": action.name(),
                "notices": notices(&located, open_notice.as_ref()),
                "trailer": serde_json::Value::Null,
                "read": batch.as_ref().map(|batch| serde_json::json!({
                    "events": batch.events.iter().map(|event| redact_event(event, &redactor)).collect::<Vec<_>>(),
                    "lines": batch.lines,
                    "bytes": batch.bytes,
                    // Present whenever `read` is present, including a `--tail` that nobody asked the events
                    // of: "how many did this read take, and how many did it refuse" is the first question
                    // about a read, not a detail behind a flag.
                    "accepted": batch.events.len(),
                    "rejected": batch.rejections.len(),
                    "refusals": batch.rejections.iter().map(|r| r.to_json()).collect::<Vec<_>>(),
                    "torn_bytes": batch.torn_tail.map(|torn| torn.bytes),
                })),
                "stream": stream,
            })
        ));
        return Ok(code);
    }

    output::line(format_args!(
        "fathom hooks — {} / event schema {}",
        hooks::PROTOCOL,
        hooks::EVENT_SCHEMA
    ));
    for notice in notices(&located, open_notice.as_ref()) {
        output::line(format_args!("  notice: {notice}"));
    }
    if let Some(batch) = &batch {
        for event in batch.events.iter().take(args.max.unwrap_or(usize::MAX)) {
            output::line(format_args!("{}", event_line(event)));
        }
        output::line(format_args!(
            "read {} event(s) of {} line(s), {} byte(s); offset is now {} of {}",
            batch.events.len(),
            batch.lines,
            batch.bytes,
            cursor.offset,
            redactor.path(&reader.events)
        ));
        if let Some(torn) = batch.torn_tail {
            output::line(format_args!(
                "  {} byte(s) at offset {} are mid-write and stay unread until the newline lands",
                torn.bytes, torn.offset
            ));
        }
        for refusal in &batch.rejections {
            output::line(format_args!(
                "  refused at {}: {} — {}",
                refusal.offset, refusal.reason, refusal.detail
            ));
        }
    }
    for line in health.lines() {
        output::line(format_args!("{line}"));
    }
    if action == Action::Replay {
        output::line(format_args!(
            "  replayed from byte 0: {} turn(s), ledger digest {}",
            health.ledger.turns.len(),
            cursor.ledger_digest()
        ));
    }
    for unknown in &health.unknowns {
        output::line(format_args!("  unknown: {unknown}"));
    }
    output::line(format_args!(
        "exit {} — {}",
        code.as_i32(),
        if code == ExitCode::Ok {
            if health.ok {
                "the stream is healthy"
            } else {
                "the stream has findings; pass --strict to make CI fail on them"
            }
        } else {
            "findings above the threshold (ADR-002 §7.3 code 7)"
        }
    ));
    Ok(code)
}

/// One read event, as the text report prints it: envelope, turn, path — never the payload (ADR-005 §4.3).
fn event_line(event: &hooks::Event) -> String {
    let kind = event.fathom_event.as_str().unwrap_or(event.source.as_str());
    let turn = event
        .turn
        .as_ref()
        .map(|turn| format!(" turn {}", turn.turn_id))
        .unwrap_or_default();
    let path = event
        .path
        .as_ref()
        .map(|path| format!(" {path}"))
        .unwrap_or_default();
    format!(
        "  {:>6}  {:>14}  {}{}{}",
        event.seq,
        kind,
        event.session_id.clone().unwrap_or_else(|| "-".to_owned()),
        turn,
        path
    )
}

/// One read event as output, with the one field in it that is an absolute path redacted like any other.
///
/// `payload.cwd` is the harness's working directory — a real home-directory path (ADR-005 §6 rule 1) — and
/// ADR-002 §11.1 does not care that the program did not invent it.
fn redact_event(event: &hooks::Event, redactor: &crate::sanitize::Redactor) -> serde_json::Value {
    let mut value = event.to_json();
    if let Some(cwd) = event.cwd.as_deref() {
        value["cwd"] = serde_json::Value::String(redactor.path(Path::new(cwd)));
    }
    value
}

/// The notices worth printing: the config ones, plus whatever the cursor open reported.
fn notices(located: &Located, open: Option<&hooks::Notice>) -> Vec<String> {
    let mut out = located.notices.clone();
    if let Some(notice) = open {
        out.push(format!("{}: {}", notice.kind, notice.detail));
    }
    out
}

/// The `--json` stream section, with paths redacted to the repository they belong to.
fn report_json(
    reader: &Reader,
    redactor: &crate::sanitize::Redactor,
    health: &hooks::Health,
    cursor: &Cursor,
    action: Action,
    strict: bool,
    code: ExitCode,
) -> serde_json::Value {
    let _ = (reader, redactor);
    let mut stream = health.to_json();
    stream["protocol"] = serde_json::Value::String(hooks::PROTOCOL.to_owned());
    stream["event_schema"] = serde_json::json!(hooks::EVENT_SCHEMA);
    stream["action"] = serde_json::Value::String(action.name().to_owned());
    stream["reads"] = serde_json::json!(cursor.reads);
    stream["strict"] = serde_json::json!(strict);
    stream["exit_code"] = serde_json::json!(code.as_i32());
    stream["exit_name"] = serde_json::Value::String(code.name().to_owned());
    stream["replay"] = serde_json::json!({
        "performed": action == Action::Replay,
        "from_offset": if action == Action::Replay { Some(0) } else { None },
        "turns": cursor.ledger.turns.len(),
        "ledger_digest": cursor.ledger_digest(),
        "all_session_qualified": cursor.ledger.turns.iter().all(|turn| {
            hooks::parse_turn_id(&turn.turn_id)
                .is_some_and(|(session, index)| session == turn.session_id && index == turn.turn_index)
        }),
        "linkage": "needs commits: Linkage::digest(attribute(commits, &cursor.ledger))",
    });
    stream
}

/// Any ingest failure, as a typed failure carrying the published exit code (ADR-002 §7.1).
fn failure(err: String) -> eyre::Report {
    Pending::new(ExitCode::Error, err, None::<String>).into()
}

/// `--trailer <session>:<turn>`: the writer's half of ADR-003 §18.2 **D5**.
fn trailer(spec: &str, as_json: bool) -> eyre::Result<ExitCode> {
    let (session, index) = split_trailer_spec(spec)?;
    let block = hooks::trailer_block_for(&session, index);
    if as_json {
        // The same envelope as the other actions, discriminated by `action` (schemas/hooks.schema.json):
        // a consumer that has already branched on `contract` should not have to branch again on the action
        // to know which keys exist.
        output::line(format_args!(
            "{}",
            serde_json::json!({
                "contract": CONTRACT,
                "verb": "fathom hooks",
                "action": Action::Trailer.name(),
                "notices": [],
                "read": serde_json::Value::Null,
                "stream": serde_json::Value::Null,
                "trailer": {
                    "session_id": session,
                    "turn_index": index,
                    "turn_id": hooks::qualify_turn_id(&session, index),
                    "block": block,
                    "keys": { "session": hooks::TRAILER_SESSION, "turn": hooks::TRAILER_TURN },
                },
            })
        ));
        return Ok(ExitCode::Ok);
    }
    for line in block.lines() {
        output::line(format_args!("{line}"));
    }
    Ok(ExitCode::Ok)
}

/// `<session>:<index>`, with the index a positive integer. The session is split on the *last* colon so an
/// id that is itself colon-shaped (`ci:build:7`) still parses the way a reader would expect.
fn split_trailer_spec(spec: &str) -> Result<(String, u64), eyre::Report> {
    // A rejected argument *value* is code 1, not code 2: clap accepted the command line, and ADR-002 §7.3
    // puts a value that means nothing under ERROR with the reason on stderr.
    let bad = |why: String| -> eyre::Report {
        Pending::new(
            ExitCode::Error,
            format!("`--trailer {spec}` {why}"),
            Some("example: `fathom hooks --trailer s7:4` prints the two trailer lines for turn 4 of s7".to_owned()),
        )
        .into()
    };
    let Some((session, index)) = spec.rsplit_once(':') else {
        return Err(bad(
            "names no turn index; the form is `<session>:<turn>`".to_owned()
        ));
    };
    let Ok(index) = index.parse::<u64>() else {
        return Err(bad("names a turn index that is not a number".to_owned()));
    };
    if session.is_empty() {
        return Err(bad("names no session".to_owned()));
    }
    if index == 0 {
        return Err(bad(
            "names turn 0, and turn indices start at 1 (ADR-005 §3.3)".to_owned(),
        ));
    }
    Ok((session.to_owned(), index))
}

impl Action {
    /// The name `--json` prints.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Action::Check => "check",
            Action::Replay => "replay",
            Action::Tail => "tail",
            Action::Trailer => "trailer",
        }
    }
}

/// The one-line summary `fathom doctor` prints about the stream, so a bug report carries it.
#[must_use]
pub fn doctor_lines(root: &Path) -> Vec<String> {
    let configured = configured(root);
    let located = locate(root, None, &configured);
    let reader = Reader::with_limits(&located.events, located.limits.clone());
    let now = crate::clock::now_millis();
    let (cursor, _notice) = reader.open_cursor(now);
    match hooks::Health::measure(&reader, &cursor, now, None) {
        Ok(health) => vec![format!(
            "hooks: {} — offset {} of {} ({} byte(s) behind, {} event(s) read, {} rejection(s)), {}",
            health.events_file,
            health.offset,
            health.size_bytes,
            health.lag_bytes,
            health.counters.accepted,
            health.counters.rejected_total(),
            if health.ok { "ok" } else { "not ok" }
        )],
        Err(err) => vec![format!("hooks: the event stream could not be read: {err}")],
    }
}

/// The same, as data, for `fathom doctor --json`.
#[must_use]
pub fn doctor_json(root: &Path) -> serde_json::Value {
    let configured = configured(root);
    let located = locate(root, None, &configured);
    let reader = Reader::with_limits(&located.events, located.limits.clone());
    let now = crate::clock::now_millis();
    let (cursor, _notice) = reader.open_cursor(now);
    match hooks::Health::measure(&reader, &cursor, now, None) {
        Ok(health) => serde_json::json!({
            "events_file": health.events_file,
            "offset": health.offset,
            "size_bytes": health.size_bytes,
            "lag_bytes": health.lag_bytes,
            "accepted": health.counters.accepted,
            "rejections": health.counters.rejections,
            "ok": health.ok,
            "reasons": health.reasons,
        }),
        Err(err) => serde_json::json!({ "error": err }),
    }
}

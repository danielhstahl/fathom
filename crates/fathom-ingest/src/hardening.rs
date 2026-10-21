//! `hardening` — the named entry point the hook-event fuzzer fuzzes (`e7.8`, `reco-41a`).
//!
//! `.fathom/events.jsonl` is written by a harness, and a harness is a program a repository can name: the
//! file is repository content, and repository content is hostile until proven otherwise. The contract's own
//! shapes are covered by `tests/hooks_v1.rs`; what lives here is the single call the fuzzer and the test
//! suite share, so a crash found at night is replayable by `cargo test` in the morning.
//!
//! The seed is the file format itself — one JSON object per line, raw bytes — so a libFuzzer artefact is a
//! valid event file a person can read.

use serde_json::Value;

use crate::hooks::{
    self, Ledger, Limits, contain_path, detect_test_run, parse_timestamp_millis, qualify_turn_id,
};

/// Fuzz the event reader with one seed: the raw bytes of one or more lines of `.fathom/events.jsonl`.
pub fn hook_event(seed: &[u8]) {
    let text = String::from_utf8_lossy(seed);
    let mut ledger = Ledger::new();
    let limits = Limits::default();
    let mut offset = 0u64;
    for line in text.lines() {
        let len = line.len() as u64;
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => {
                offset += len + 1;
                continue;
            }
        };
        if let Ok(mut event) = hooks::validate_record(&value, offset, len) {
            let _ = ledger.apply(&mut event, &limits);
        }
        // Every string in the record is something a path, a command or a timestamp may be built from, so
        // each one goes through the four readers that take harness-shaped text.
        walk(&value, &mut |string| {
            let _ = contain_path("/srv/session", string);
            let _ = contain_path(string, "src/main.rs");
            let _ = detect_test_run(string);
            let _ = parse_timestamp_millis(string);
            let _ = hooks::FathomEvent::from_name(string);
        });
        // A turn id is derived from two fields the writer chose, and the reader has to be able to say no.
        let session = string_field(&value, "session_id");
        let index = value
            .get("turn_index")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let qualified = qualify_turn_id(session, index);
        let _ = hooks::parse_turn_id(&qualified);
        // The cursor round-trip is the other place a hostile ledger can land.
        let _ = Ledger::from_json(&ledger.to_json());
        offset += len + 1;
    }
}

/// One top-level string field, or "" when it is not there or not a string.
fn string_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Every string leaf of a JSON value, with the walk itself bounded.
///
/// The bound is here because the walker is *also* hostile-input code: a record whose nesting is deep enough
/// to be an attack must not be met with a recursive walk of its own.
fn walk(value: &Value, visit: &mut dyn FnMut(&str)) {
    const MAX_DEPTH: usize = 32;
    let mut stack = vec![(0usize, value)];
    while let Some((depth, node)) = stack.pop() {
        match node {
            Value::String(text) => visit(text),
            Value::Array(items) if depth < MAX_DEPTH => {
                stack.extend(items.iter().map(|item| (depth + 1, item)));
            }
            Value::Object(map) if depth < MAX_DEPTH => {
                for (key, item) in map {
                    visit(key);
                    stack.push((depth + 1, item));
                }
            }
            _ => {}
        }
    }
}

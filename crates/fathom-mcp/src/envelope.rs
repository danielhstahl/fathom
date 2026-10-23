//! The response envelope: the fields every answer carries, and the cursor that makes a truncated answer
//! continuable (ADR-005 §4.1, §5.1).
//!
//! Three cross-cutting rules from §4.1 live here and nowhere else, so that no tool can forget one:
//!
//! * **Every response carries `unknowns`.** "Confidence 0.9 with empty `unknowns`" must be impossible, so
//!   the field is written even when the list is empty — an absent field and an empty list are different
//!   claims and a client should not have to guess which one it got.
//! * **Every response carries `cost_ms`, `bytes_returned` and `degraded`.** A tool call runs inside the
//!   agent's turn budget; an unbounded result is a latency outage wearing a feature's clothes.
//! * **Nothing throws at the caller.** An error is a value with a `code` from the closed set in
//!   [`crate::source::Failure`], on the same envelope, with the same cost fields.

use serde_json::{Map, Value, json};

use crate::source::Batch;
use crate::tools::ToolSpec;

/// The contract every payload carries (§5.1).
pub const CONTRACT_VERSION: u32 = 1;

/// The range this server publishes for that contract (§5.1).
pub const CONTRACT_RANGE: [u32; 2] = [1, 1];

/// The prefix that says a cursor came from this server and means "an offset".
///
/// A cursor is opaque to the caller and legible to the operator: a person reading an agent's transcript
/// should be able to see that the agent was on row 250, which is the difference between a debuggable
/// truncation and a magic token.
const CURSOR_PREFIX: &str = "fathom.mcp.v1.offset.";

/// Encode an offset as a cursor.
#[must_use]
pub fn cursor(offset: usize) -> String {
    format!("{CURSOR_PREFIX}{offset}")
}

/// Decode a cursor, or say why it is not one of ours.
///
/// `None` on anything this server did not issue: a made-up cursor is refused with `bad_args` rather than
/// silently read as zero, because an agent that typos a cursor and gets page one again will report it as
/// "the tool lost my place".
#[must_use]
pub fn decode_cursor(cursor: &str) -> Option<usize> {
    let tail = cursor.strip_prefix(CURSOR_PREFIX)?;
    if tail.is_empty() || !tail.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    tail.parse().ok()
}

/// Build the success envelope for one tool call.
///
/// `bytes_returned` is the size of the envelope as it is actually returned. Measuring it is self-referential
/// — the number is in the thing being measured — so the document is written once with the counter at zero,
/// measured, rewritten with the real number, and re-measured once if the digit count grew. The alternative
/// is a number that is wrong by the width of itself, and a field whose whole job is honesty should not
/// start by lying about its own length.
#[must_use]
pub fn success(
    spec: &ToolSpec,
    server_version: &str,
    batch: &Batch,
    offset: usize,
    limit: usize,
    cost_ms: u128,
    extra_unknowns: &[String],
) -> Value {
    let returned = batch.items.len();
    let truncated = batch.total.saturating_sub(offset) > returned;
    let mut unknowns = batch.unknowns.clone();
    unknowns.extend(extra_unknowns.iter().cloned());
    unknowns.sort_unstable();
    unknowns.dedup();

    let mut map = Map::new();
    map.insert("contract_version".into(), json!(CONTRACT_VERSION));
    map.insert("contract_range".into(), json!(CONTRACT_RANGE));
    map.insert("server_version".into(), json!(server_version));
    map.insert("tool".into(), json!(spec.name));
    map.insert("capability".into(), json!(spec.capability.as_str()));
    map.insert("degraded".into(), json!(!unknowns.is_empty()));
    map.insert("unknowns".into(), json!(unknowns));
    map.insert("cost_ms".into(), json!(cost_ms));
    map.insert("bytes_returned".into(), json!(0));
    for (key, value) in &batch.fields {
        map.insert(key.clone(), value.clone());
    }
    if spec.default_items > 0 {
        map.insert("offset".into(), json!(offset));
        map.insert("limit".into(), json!(limit));
        map.insert("total".into(), json!(batch.total));
        map.insert("returned".into(), json!(returned));
        map.insert("truncated".into(), json!(truncated));
        if truncated {
            map.insert(
                "next_cursor".into(),
                json!(cursor(offset.saturating_add(returned))),
            );
        }
    }
    map.insert("items".into(), Value::Array(batch.items.clone()));
    settle_bytes(&mut map)
}

/// Build the refusal envelope. Same shape, same cost fields, and an `error` where the answer would be.
#[must_use]
pub fn failure(
    tool: &str,
    server_version: &str,
    code: &str,
    message: &str,
    cost_ms: u128,
    remedy: Option<&str>,
) -> Value {
    let mut map = Map::new();
    map.insert("contract_version".into(), json!(CONTRACT_VERSION));
    map.insert("contract_range".into(), json!(CONTRACT_RANGE));
    map.insert("server_version".into(), json!(server_version));
    map.insert("tool".into(), json!(tool));
    map.insert("cost_ms".into(), json!(cost_ms));
    map.insert("bytes_returned".into(), json!(0));
    map.insert("error".into(), json!({ "code": code, "message": message }));
    if let Some(remedy) = remedy {
        map.insert("remedy".into(), json!(remedy));
    }
    // §4.1's three fields are on a refusal too, and a refusal is always `degraded`: the answer did not
    // happen. Dropping them here would make the envelope's shape depend on the outcome, which is how a
    // client ends up parsing two protocols where there is supposed to be one.
    map.insert("degraded".into(), json!(true));
    map.insert("unknowns".into(), json!([format!("{code}: {message}")]));
    settle_bytes(&mut map)
}

/// A `tools/list` result's own note about what is missing, so a client that caches the list can tell a
/// short list from a complete one.
#[must_use]
pub fn tools_list_meta(writes_enabled: bool) -> Value {
    json!({
        "contract_version": CONTRACT_VERSION,
        "contract_range": CONTRACT_RANGE,
        "writes_enabled": writes_enabled,
        "not_advertised": if writes_enabled {
            Vec::<String>::new()
        } else {
            vec!["mark_reviewed".to_owned(), "add_review_note".to_owned()]
        },
        "why": if writes_enabled {
            "the write tools are on: `mcp.allow_writes = true`, and every call to them is audited."
        } else {
            "the write tools are off by default and are absent from this list rather than present and \
             refusing (ADR-005 §4.3); `mcp.allow_writes = true` turns them on and starts an audit line \
             per call."
        },
    })
}

/// Write the byte counter into the map and return the finished document.
fn settle_bytes(map: &mut Map<String, Value>) -> Value {
    let first = document_len(map);
    map.insert("bytes_returned".into(), json!(first));
    let second = document_len(map);
    if second != first {
        map.insert("bytes_returned".into(), json!(second));
    }
    Value::Object(map.clone())
}

/// The length of the serialised document, compact, as the caller will receive it.
fn document_len(map: &Map<String, Value>) -> usize {
    serde_json::to_string(&Value::Object(map.clone()))
        .map(|text| text.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::tools::find;

    #[test]
    fn the_byte_counter_counts_the_document_that_is_actually_returned() {
        let spec = find("explain_episode").expect("the tool is registered");
        let batch = Batch::new()
            .item(serde_json::json!({"path": "src/a.rs", "add": 3}))
            .item(serde_json::json!({"path": "src/b.rs", "add": 1}))
            .total(2);
        let document = success(spec, "fathom-mcp/test", &batch, 0, 2, 7, &[]);
        let declared = document["bytes_returned"]
            .as_u64()
            .expect("the counter is there");
        let actual = serde_json::to_string(&document).unwrap_or_default().len();
        assert_eq!(
            declared as usize, actual,
            "a counter whose own width is ignored is wrong by the width of itself"
        );
    }
}

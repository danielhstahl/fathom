//! The gates and the bounds, exercised without a repository: a stub [`Source`] answers, and every assertion
//! is about what the server did with the answer (ADR-005 §4.1, §4.2, §4.3).
//!
//! These are the unit half of the ticket's second and third acceptance criteria. The other half — a real
//! client session against the shipped binary and the shipped config — is `tests/test_mcp_contract.py`, and
//! the two are deliberately different: this file can assert internals (that a refusal was audited, that a
//! cursor decodes), and that file can only assert the wire.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use fathom_mcp::policy::Policy;
use fathom_mcp::server::FathomMcp;
use fathom_mcp::source::{Args, AuditLine, Batch, Failure, Source, Window};
use serde_json::{Value, json};

/// A source that answers one canned thing per method and remembers how many audit lines it was given.
#[derive(Default)]
struct Stub {
    audit: std::sync::Mutex<Vec<AuditLine>>,
    long_body: bool,
}

impl Stub {
    fn page(&self, window: Window) -> Batch {
        let all: Vec<Value> = (0..70)
            .map(|index| {
                json!({
                    "change_id": format!("chg-{index}"),
                    "path": format!("src/module_{index}.rs"),
                    "add": index,
                    "del": 0,
                })
            })
            .collect();
        let items = all
            .into_iter()
            .skip(window.offset)
            .take(window.limit)
            .collect();
        Batch::new()
            .items(items)
            .total(70)
            .field("model_version", "spine-1")
    }

    fn audited(&self) -> Vec<String> {
        self.audit
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|line| format!("{}:{}", line.tool, line.outcome))
            .collect()
    }
}

impl Source for Stub {
    fn explain_episode(&self, _args: &Args, window: Window) -> Result<Batch, Failure> {
        Ok(self.page(window))
    }

    fn changes_between(&self, _args: &Args, window: Window) -> Result<Batch, Failure> {
        Ok(self.page(window))
    }

    fn architecture_delta(&self, _args: &Args, window: Window) -> Result<Batch, Failure> {
        Ok(self.page(window))
    }

    fn dependency_delta(&self, _args: &Args, window: Window) -> Result<Batch, Failure> {
        Ok(self.page(window))
    }

    fn risk_report(&self, _args: &Args, window: Window) -> Result<Batch, Failure> {
        Ok(self.page(window))
    }

    fn search_history(&self, args: &Args, window: Window) -> Result<Batch, Failure> {
        // One hit per token, and a body-shaped key the source should never have emitted, so the egress
        // filter has something to refuse.
        let mut batch = self.page(window);
        batch.items = vec![json!({
            "episode_id": "ep-1",
            "title": args.str("query").unwrap_or("?"),
            "score": 9,
            "matched_on": ["title"],
            "snippet": "a search snippet must never leave the process",
            "text": "line one\nline two\nline three",
        })];
        batch.total = 1;
        Ok(batch)
    }

    fn mark_reviewed(&self, args: &Args) -> Result<Batch, Failure> {
        Ok(Batch::new()
            .field("entry_id", "sg-1")
            .field("change_id", args.str("change_id").unwrap_or("?"))
            .field(
                "body",
                if self.long_body {
                    "x".repeat(9000)
                } else {
                    "a quoted line from a file".to_owned()
                },
            ))
    }

    fn add_review_note(&self, args: &Args) -> Result<Batch, Failure> {
        Ok(Batch::new().field("target", args.str("target").unwrap_or("?")))
    }

    fn audit(&self, line: &AuditLine) -> Result<(), String> {
        self.audit
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(line.clone());
        Ok(())
    }
}

fn server(policy: Policy, stub: Arc<Stub>) -> FathomMcp {
    FathomMcp::new(policy, stub)
}

fn read_only() -> Policy {
    Policy {
        writes_enabled: false,
        ..Policy::default()
    }
}

#[test]
fn the_write_tools_are_absent_from_the_list_when_writes_are_off() {
    let stub = Arc::new(Stub::default());
    let server = server(read_only(), stub.clone());
    let names: Vec<String> = server
        .list_tools()
        .get("tools")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no tools"))
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "explain_episode",
            "changes_between",
            "architecture_delta",
            "dependency_delta",
            "risk_report",
            "search_history",
        ],
        "the callable surface, and nothing that would refuse"
    );
    assert!(
        !names.contains(&"mark_reviewed".to_owned()),
        "a tool you can see and cannot call is a trap (ADR-005 §4.3)"
    );
}

#[test]
fn a_write_named_anyway_is_refused_with_no_capability_and_audited_as_refused() {
    let stub = Arc::new(Stub::default());
    let server = server(read_only(), stub.clone());
    let answer = server.call_tool(
        "mark_reviewed",
        serde_json::from_value(json!({"change_id": "ep-1", "reviewer": "Ana"})).unwrap(),
    );
    assert_eq!(
        answer["error"]["code"].as_str(),
        Some("no_capability"),
        "{answer}"
    );
    assert!(
        stub.audited()
            .iter()
            .any(|line| line == "mark_reviewed:no_capability"),
        "a refused write is exactly the event an auditor wants: {:?}",
        stub.audited()
    );
    assert!(
        answer.get("cost_ms").is_some() && answer.get("bytes_returned").is_some(),
        "a refusal is still a response: {answer}"
    );
}

#[test]
fn a_write_enabled_by_config_still_needs_a_token_bound_to_the_change() {
    let stub = Arc::new(Stub::default());
    let policy = Policy {
        writes_enabled: true,
        approval_secret: Some("a-secret".to_owned()),
        ..Policy::default()
    };
    let server = server(policy.clone(), stub.clone());
    let missing = server.call_tool(
        "mark_reviewed",
        serde_json::from_value(json!({"change_id": "ep-1", "reviewer": "Ana"})).unwrap(),
    );
    assert_eq!(missing["error"]["code"].as_str(), Some("no_capability"));

    let wrong = server.call_tool(
        "mark_reviewed",
        serde_json::from_value(json!({
            "change_id": "ep-1", "reviewer": "Ana", "approval_token": "guess",
        }))
        .unwrap(),
    );
    assert_eq!(wrong["error"]["code"].as_str(), Some("no_capability"));

    let right = policy
        .approval_token("ep-1")
        .expect("a configured secret mints a token");
    let ok = server.call_tool(
        "mark_reviewed",
        serde_json::from_value(json!({
            "change_id": "ep-1", "reviewer": "Ana", "approval_token": right,
        }))
        .unwrap(),
    );
    assert!(
        ok.get("error").is_none(),
        "the bound token opens exactly one change: {ok}"
    );
    assert_eq!(ok["entry_id"].as_str(), Some("sg-1"));
    // The note the caller wrote does not appear in the audit line: the line carries a digest (`e6.4`).
    let lines = stub
        .audit
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let last = lines.last().expect("the accepted write was audited");
    assert!(last.args_digest.starts_with("sha256:"));
    assert_eq!(last.outcome, "ok");
    assert_eq!(last.reviewer.as_deref(), Some("Ana"));
}

#[test]
fn an_oversized_answer_is_cut_and_handed_back_with_a_cursor_that_continues_it() {
    let stub = Arc::new(Stub::default());
    let server = server(read_only(), stub);
    let first = server.call_tool(
        "changes_between",
        serde_json::from_value(json!({"ref_a": "a", "ref_b": "b", "limit": 3})).unwrap(),
    );
    assert_eq!(first["total"].as_u64(), Some(70));
    assert_eq!(first["returned"].as_u64(), Some(3));
    assert_eq!(first["truncated"].as_bool(), Some(true));
    let cursor = first["next_cursor"]
        .as_str()
        .expect("a truncated answer carries a cursor")
        .to_owned();
    let second = server.call_tool(
        "changes_between",
        serde_json::from_value(json!({"ref_a": "a", "ref_b": "b", "limit": 3, "cursor": cursor}))
            .unwrap(),
    );
    assert_eq!(second["offset"].as_u64(), Some(3));
    assert_eq!(
        second["items"][0]["change_id"].as_str(),
        Some("chg-3"),
        "the cursor continues where the page stopped: {second}"
    );
    assert_ne!(
        first["items"][0]["change_id"].as_str(),
        second["items"][0]["change_id"].as_str()
    );
}

#[test]
fn a_limit_above_the_tools_bound_is_clamped_and_said() {
    let stub = Arc::new(Stub::default());
    let server = server(read_only(), stub);
    // `risk_report` answers at most 100; the stub only has 70, so the clamp shows up as the page size.
    let answer = server.call_tool(
        "risk_report",
        serde_json::from_value(json!({"limit": 5000})).unwrap(),
    );
    assert!(
        answer["limit"].as_u64().unwrap_or(0) <= 100,
        "the tool's own bound is the ceiling: {answer}"
    );
    assert!(answer["bytes_returned"].as_u64().unwrap_or(0) > 0);
    assert_eq!(
        answer["bytes_returned"].as_u64(),
        Some(serde_json::to_string(&answer).unwrap_or_default().len() as u64),
        "`bytes_returned` is the size of the document that was actually returned"
    );
}

#[test]
fn arguments_are_the_tools_own_and_a_bound_is_a_code_not_an_exception() {
    let stub = Arc::new(Stub::default());
    let server = server(read_only(), stub);
    let missing = server.call_tool(
        "explain_episode",
        serde_json::from_value(json!({})).unwrap(),
    );
    assert_eq!(missing["error"]["code"].as_str(), Some("bad_args"));
    let unknown_key = server.call_tool(
        "explain_episode",
        serde_json::from_value(json!({"episode_id": "ep-1", "please": "everything"})).unwrap(),
    );
    assert_eq!(unknown_key["error"]["code"].as_str(), Some("bad_args"));
    let too_big = server.call_tool(
        "search_history",
        serde_json::from_value(json!({"query": "x", "k": 99})).unwrap(),
    );
    assert_eq!(too_big["error"]["code"].as_str(), Some("out_of_bounds"));
    let invented = server.call_tool("drop_the_database", serde_json::Map::new());
    assert_eq!(invented["error"]["code"].as_str(), Some("unknown_tool"));
    let invented_cursor = server.call_tool(
        "changes_between",
        serde_json::from_value(json!({"ref_a": "a", "ref_b": "b", "cursor": "hunter2"})).unwrap(),
    );
    assert_eq!(invented_cursor["error"]["code"].as_str(), Some("bad_args"));
}

#[test]
fn no_snippet_no_body_and_no_long_text_leaves_the_process() {
    let stub = Arc::new(Stub::default());
    let server = server(read_only(), stub);
    let answer = server.call_tool(
        "search_history",
        serde_json::from_value(json!({"query": "what did we do"})).unwrap(),
    );
    let text = serde_json::to_string(&answer).unwrap_or_default();
    assert!(
        !text.contains("a search snippet must never leave"),
        "a snippet is file-shaped content and §4.3 refuses it: {text}"
    );
    assert!(
        !text.contains("line two"),
        "a `text` field is a body whatever it holds: {text}"
    );
    assert!(
        text.contains("[refused: file contents never leave the model"),
        "the refusal is said, not silent: {text}"
    );
    assert!(
        answer["unknowns"]
            .as_array()
            .is_some_and(|notes| notes.iter().any(|note| {
                note.as_str()
                    .unwrap_or("")
                    .contains("file contents never leave the model")
            })),
        "and it is in `unknowns`, so the caller can tell: {answer}"
    );
}

#[test]
fn a_secret_in_an_answer_is_redacted_on_the_way_out() {
    let server = FathomMcp::new(read_only(), Arc::new(Secretive));
    let answer = server.call_tool(
        "explain_episode",
        serde_json::from_value(json!({"episode_id": "ep-1"})).unwrap(),
    );
    let text = serde_json::to_string(&answer).unwrap_or_default();
    assert!(
        !text.contains("AKIAABCDEFGHIJKLMNOP"),
        "the key was redacted at egress: {text}"
    );
    assert!(text.contains("[REDACTED:aws-access-key]"), "{text}");
    assert!(
        text.contains("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
        "a git-shaped digest stays, or the answer cannot cite its own evidence: {text}"
    );
}

/// A source that answers with a payload holding a credential and an identity digest side by side.
#[derive(Default)]
struct Secretive;

impl Source for Secretive {
    fn explain_episode(&self, _args: &Args, _window: Window) -> Result<Batch, Failure> {
        Ok(Batch::new()
            .item(json!({
                "fingerprint": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                "note": "the deploy key is AKIAABCDEFGHIJKLMNOP and the id is fine",
            }))
            .total(1))
    }

    fn changes_between(&self, _args: &Args, _window: Window) -> Result<Batch, Failure> {
        Ok(Batch::new())
    }

    fn architecture_delta(&self, _args: &Args, _window: Window) -> Result<Batch, Failure> {
        Ok(Batch::new())
    }

    fn dependency_delta(&self, _args: &Args, _window: Window) -> Result<Batch, Failure> {
        Ok(Batch::new())
    }

    fn risk_report(&self, _args: &Args, _window: Window) -> Result<Batch, Failure> {
        Ok(Batch::new())
    }

    fn search_history(&self, _args: &Args, _window: Window) -> Result<Batch, Failure> {
        Ok(Batch::new())
    }

    fn mark_reviewed(&self, _args: &Args) -> Result<Batch, Failure> {
        Ok(Batch::new())
    }

    fn add_review_note(&self, _args: &Args) -> Result<Batch, Failure> {
        Ok(Batch::new())
    }

    fn audit(&self, _line: &AuditLine) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn every_response_carries_the_three_cross_cutting_fields() {
    let stub = Arc::new(Stub::default());
    let server = server(read_only(), stub);
    for (tool, args) in [
        ("explain_episode", json!({"episode_id": "ep-1", "limit": 2})),
        ("risk_report", json!({})),
        ("search_history", json!({"query": "x", "k": 1, "limit": 1})),
        ("changes_between", json!({"ref_a": "a", "ref_b": "b"})),
    ] {
        let answer = server.call_tool(
            tool,
            serde_json::from_value(args).unwrap_or_else(|_| panic!("{tool}")),
        );
        for key in [
            "cost_ms",
            "bytes_returned",
            "degraded",
            "unknowns",
            "contract_version",
            "server_version",
            "tool",
        ] {
            assert!(
                answer.get(key).is_some(),
                "{tool} answered without {key}: {answer}"
            );
        }
        assert_eq!(answer["tool"].as_str(), Some(tool));
        assert_eq!(answer["contract_version"].as_u64(), Some(1));
        assert!(
            answer["unknowns"].is_array(),
            "an absent `unknowns` and an empty one are different claims"
        );
    }
}

#[test]
fn reads_are_not_audited_unless_the_operator_asks_for_it() {
    let stub = Arc::new(Stub::default());
    let quiet = server(read_only(), stub.clone());
    let _ = quiet.call_tool(
        "explain_episode",
        serde_json::from_value(json!({"episode_id": "ep-1"})).unwrap(),
    );
    assert!(
        stub.audited().is_empty(),
        "a read audit is a log of what an agent was curious about, which is `mcp.audit_reads`'s to decide"
    );
    let noisy = server(
        Policy {
            audit_reads: true,
            ..read_only()
        },
        stub.clone(),
    );
    let _ = noisy.call_tool(
        "explain_episode",
        serde_json::from_value(json!({"episode_id": "ep-1"})).unwrap(),
    );
    assert_eq!(
        stub.audited(),
        vec!["explain_episode:ok".to_owned()],
        "with the flag on, the read is in the log too"
    );
}

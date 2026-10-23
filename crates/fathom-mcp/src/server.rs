//! The JSON-RPC half: the tool surface, the gates in front of it, and the discipline behind it
//! (ADR-005 §4, §5.2).
//!
//! One dispatch function, [`FathomMcp::dispatch`], does the whole protocol-level job for a read or a write,
//! and it is written as a pipeline on purpose, because every stage is one of the ticket's acceptance criteria
//! and none of them may be skipped by a tool that is in a hurry:
//!
//! 1. **Is the tool here at all?** An unknown name is `unknown_tool`, not a panic.
//! 2. **Is it allowed?** A write tool is invisible when `mcp.allow_writes` is off and refuses when called
//!    anyway (§4.2, §4.3).
//! 3. **Are the arguments its own?** Required keys present, unknown keys refused, bounded keys inside their
//!    bound — `bad_args` and `out_of_bounds`, both structured (§4.1).
//! 4. **Read.** Through [`crate::source::Source`], which is a trait precisely so that this crate never
//!    holds a store handle or a file handle.
//! 5. **Filter.** Redaction, file-body refusal and per-leaf bounds ([`crate::egress`]).
//! 6. **Bound.** The envelope, and — if it will not fit in `mcp.max_response_bytes` — fewer rows and a
//!    cursor, with the truncation said out loud.
//! 7. **Audit.** Every gated call, refused or not, gets one line: the gate is ADR-005 §4.2's, and the
//!     line is `e6.4`'s — a named tool call, a digest of its arguments, and what came of it.
//!
//! Nothing on this path returns a JSON-RPC error for a tool that ran. A protocol error ends the agent's
//! turn; the boundary returns a value and lets the agent decide what to do next (§4.1).

use std::io::Write as _;
use std::sync::Arc;
use std::time::Instant;

use rmcp::model::*;
use rmcp::{ErrorData, ServerHandler};
use serde_json::{Map, Value, json};

use crate::egress::{self, Filtered};
use crate::envelope;
use crate::policy::Policy;
use crate::source::{Args, AuditLine, Batch, Failure, Source, Window};
use crate::tools::{self, Capability, ToolSpec};

/// The server: a policy, a source, and the tool table.
///
/// Cheap to clone and `Send + Sync`, which is what rmcp asks of a handler.
#[derive(Clone)]
pub struct FathomMcp {
    policy: Arc<Policy>,
    source: Arc<dyn Source>,
}

impl FathomMcp {
    /// Build the server over a policy and the source the composition root supplied.
    #[must_use]
    pub fn new(policy: Policy, source: Arc<dyn Source>) -> Self {
        Self {
            policy: Arc::new(policy),
            source,
        }
    }

    /// The resolved policy, so the composition root can print what it decided.
    #[must_use]
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Every tool call and `tools/list` answer goes through here: the envelope as a JSON value.
    ///
    /// # Errors
    ///
    /// Only [`Failure::Internal`], and only when the source said the server itself broke. Every other
    /// outcome — bad arguments, a missing capability, a refusal — is a value.
    #[must_use]
    pub fn call_tool(&self, name: &str, arguments: Map<String, Value>) -> Value {
        let started = Instant::now();
        match self.route(name, arguments, &started) {
            Ok(document) => document,
            Err(failure) => {
                let remedy = failure.remedy();
                envelope::failure(
                    name,
                    &self.policy.server_version,
                    failure.code(),
                    failure.message(),
                    started.elapsed().as_millis(),
                    Some(remedy.as_str()),
                )
            }
        }
    }

    /// The `tools/list` payload, honouring §4.3's rule that a tool the session cannot call is absent.
    #[must_use]
    pub fn list_tools(&self) -> Value {
        let visible = tools::visible(self.policy.writes_enabled);
        json!({
            "tools": visible
                .iter()
                .map(|spec| {
                    json!({
                        "name": spec.name,
                        "title": spec.title,
                        "description": spec.description,
                        "inputSchema": spec.input_schema(),
                        "annotations": {
                            "readOnlyHint": spec.capability == Capability::Read,
                            "idempotentHint": true,
                        },
                        "x-fathom-capability": spec.capability.as_str(),
                    })
                })
                .collect::<Vec<_>>(),
            "nextCursor": Value::Null,
            "_meta": envelope::tools_list_meta(self.policy.writes_enabled),
        })
    }

    /// The surface as the CLI prints it for `fathom mcp tools --json`: one array, bounds included.
    #[must_use]
    pub fn tool_surface(&self) -> Vec<Value> {
        tools::ALL
            .iter()
            .map(|spec| {
                json!({
                    "name": spec.name,
                    "capability": spec.capability.as_str(),
                    "required": spec.required,
                    "optional": spec.optional,
                    "default_items": spec.default_items,
                    "max_items": spec.max_items,
                    "bound": spec.bound_note,
                    "advertised": self.policy.writes_enabled
                        || spec.capability == Capability::Read,
                })
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // the pipeline
    // ------------------------------------------------------------------

    fn route(
        &self,
        name: &str,
        arguments: Map<String, Value>,
        started: &Instant,
    ) -> Result<Value, Failure> {
        let Some(spec) = tools::find(name) else {
            return Err(Failure::UnknownTool(format!(
                "`{name}` is not a tool this server offers; `tools/list` is the callable surface"
            )));
        };
        let args = Args(arguments.into_iter().collect());
        if spec.capability == Capability::Write && !self.policy.writes_enabled {
            // Refused and audited: a refused write is exactly the event an auditor wants to see.
            let document = self.refusal(
                spec,
                Failure::NoCapability(format!(
                    "`{}` needs the `write` capability, and `mcp.allow_writes` is false — the write tools \
                     are off in this build's config",
                    spec.name
                )),
                &args,
                started,
            );
            return Ok(document);
        }
        if let Some(bad) = self.validate(spec, &args) {
            return self.refusal_err(spec, &args, bad, started);
        }
        let window = self.window(spec, &args)?;
        let result = match spec.name {
            "explain_episode" => self.source.explain_episode(&args, window),
            "changes_between" => self.source.changes_between(&args, window),
            "architecture_delta" => self.source.architecture_delta(&args, window),
            "dependency_delta" => self.source.dependency_delta(&args, window),
            "risk_report" => self.source.risk_report(&args, window),
            "search_history" => self.source.search_history(&args, window),
            "mark_reviewed" => self.mark_reviewed(&args),
            "add_review_note" => self.add_review_note(&args),
            other => Err(Failure::UnknownTool(other.to_owned())),
        };
        let mut batch = match result {
            Ok(batch) => batch,
            Err(failure) => {
                if spec.capability == Capability::Write || self.policy.audit_reads {
                    self.audit(spec, &args, Err(&failure), 0, started, &[]);
                }
                return Err(failure);
            }
        };

        // 5 · egress. Whatever the source assembled, nothing secret and no file body leaves here.
        let mut filtered = Filtered::default();
        for item in &mut batch.items {
            egress::filter(item, &mut filtered);
        }
        for (_, value) in &mut batch.fields {
            egress::filter(value, &mut filtered);
        }
        let mut unknowns = filtered.notes();

        // 6 · bound. Oversized answers are cut and handed back with a cursor, which is what makes cutting
        //     them a fact the caller can act on instead of a mystery.
        let mut limit = window.limit;
        loop {
            let probe = envelope::success(
                spec,
                &self.policy.server_version,
                &batch,
                window.offset,
                limit,
                started.elapsed().as_millis(),
                &unknowns,
            );
            let bytes = byte_len(&probe);
            if bytes <= self.policy.max_response_bytes || batch.items.len() <= 1 {
                if bytes > self.policy.max_response_bytes {
                    unknowns.push(format!(
                        "one row is larger than mcp.max_response_bytes ({} bytes); it is returned whole \
                         and the answer is truncated here — narrow the query",
                        self.policy.max_response_bytes
                    ));
                    let note = envelope::success(
                        spec,
                        &self.policy.server_version,
                        &batch,
                        window.offset,
                        limit,
                        started.elapsed().as_millis(),
                        &unknowns,
                    );
                    return self.finish(spec, &args, note, started, &batch);
                }
                return self.finish(spec, &args, probe, started, &batch);
            }
            let drop = batch.items.len() - (batch.items.len() / 2);
            batch.items.truncate(drop);
            limit = batch.items.len();
        }
    }

    /// Everything §4.1's three cross-cutting rules owe the caller, once, for a success.
    fn finish(
        &self,
        spec: &ToolSpec,
        args: &Args,
        document: Value,
        started: &Instant,
        batch: &Batch,
    ) -> Result<Value, Failure> {
        if spec.capability == Capability::Write || self.policy.audit_reads {
            self.audit(spec, args, Ok(&document), byte_len(&document), started, &[]);
        }
        let _ = batch;
        Ok(document)
    }

    fn audit(
        &self,
        spec: &ToolSpec,
        args: &Args,
        outcome: Result<&Value, &Failure>,
        bytes_returned: usize,
        started: &Instant,
        _extra: &[String],
    ) {
        let outcome = match outcome {
            Ok(document) => document
                .get("error")
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| "ok".to_owned()),
            Err(failure) => failure.code().to_owned(),
        };
        let line = AuditLine {
            tool: spec.name.to_owned(),
            capability: spec.capability.as_str(),
            args_digest: crate::policy::args_digest(&args.0),
            outcome,
            cost_ms: started.elapsed().as_millis(),
            bytes_returned,
            reviewer: args.str("reviewer").map(str::to_owned),
        };
        if let Err(problem) = self.source.audit(&line) {
            // The audit is not allowed to fail the answer it is auditing. stdout is the protocol, so the
            // operator's channel is stderr, and the line says which call went unlogged.
            let _ = writeln!(
                std::io::stderr(),
                "fathom mcp: audit line for `{}` could not be written: {problem}",
                spec.name
            );
        }
    }

    fn refusal(&self, spec: &ToolSpec, failure: Failure, args: &Args, started: &Instant) -> Value {
        let document = envelope::failure(
            spec.name,
            &self.policy.server_version,
            failure.code(),
            failure.message(),
            started.elapsed().as_millis(),
            Some("set `mcp.allow_writes = true` in the user config to turn the write tools on"),
        );
        self.audit(spec, args, Ok(&document), byte_len(&document), started, &[]);
        document
    }

    #[allow(clippy::result_large_err)]
    fn refusal_err(
        &self,
        spec: &ToolSpec,
        args: &Args,
        failure: Failure,
        started: &Instant,
    ) -> Result<Value, Failure> {
        if spec.capability == Capability::Write || self.policy.audit_reads {
            self.audit(spec, args, Err(&failure), 0, started, &[]);
        }
        Err(failure)
    }

    fn validate(&self, spec: &ToolSpec, args: &Args) -> Option<Failure> {
        let missing: Vec<&str> = spec
            .required
            .iter()
            .filter(|key| args.get(key).is_none())
            .copied()
            .collect();
        if !missing.is_empty() {
            return Some(Failure::BadArgs(format!(
                "{} requires {}",
                spec.name,
                missing.join(", ")
            )));
        }
        let mut allowed: Vec<&str> = spec.required.to_vec();
        allowed.extend_from_slice(spec.optional);
        if spec.default_items > 0 {
            allowed.extend_from_slice(ToolSpec::PAGINATION);
            allowed.push("offset");
        }
        let unexpected: Vec<&str> = args
            .keys()
            .iter()
            .filter(|key| !allowed.contains(key))
            .copied()
            .collect();
        if !unexpected.is_empty() {
            return Some(Failure::BadArgs(format!(
                "{} does not take {}; it takes {}",
                spec.name,
                unexpected.join(", "),
                allowed.join(", ")
            )));
        }
        // §4.1: a `k` above the bound is its own code, because the caller can fix it by asking for less.
        if let Some(k) = args.usize("k")
            && k > spec.max_items
        {
            return Some(Failure::OutOfBounds(format!(
                "`k` is {k}; this tool answers at most {}",
                spec.max_items
            )));
        }
        if let Some(body) = args.str("body")
            && body.len() > tools::MAX_BODY_BYTES
        {
            return Some(Failure::OutOfBounds(format!(
                "body is {} bytes; the bound is {}",
                body.len(),
                tools::MAX_BODY_BYTES
            )));
        }
        if let Some(note) = args.str("note")
            && note.len() > tools::MAX_BODY_BYTES
        {
            return Some(Failure::OutOfBounds(format!(
                "note is {} bytes; the bound is {}",
                note.len(),
                tools::MAX_BODY_BYTES
            )));
        }
        for key in ["episode_id", "change_id", "target", "query", "reviewer"] {
            if let Some(value) = args.str(key)
                && value.is_empty()
            {
                return Some(Failure::BadArgs(format!("`{key}` may not be empty")));
            }
        }
        None
    }

    fn window(&self, spec: &ToolSpec, args: &Args) -> Result<Window, Failure> {
        if spec.default_items == 0 {
            return Ok(Window {
                offset: 0,
                limit: 1,
            });
        }
        let offset = match args.str("cursor") {
            Some(cursor) => {
                crate::envelope::decode_cursor(cursor).ok_or_else(|| {
                    Failure::BadArgs(format!(
                        "`cursor` `{cursor}` was not issued by this server; pass back the `next_cursor` \
                         from a previous page"
                    ))
                })?
            }
            None => args.usize("offset").unwrap_or(0),
        };
        let asked = args.usize("limit").unwrap_or(spec.default_items);
        let limit = asked
            .min(spec.max_items)
            .min(self.policy.page_limit_max.max(1))
            .max(1);
        let _ = asked; // clamped above; the envelope's `limit` is what the caller is owed
        Ok(Window { offset, limit })
    }

    // ------------------------------------------------------------------
    // the gated half
    // ------------------------------------------------------------------

    fn mark_reviewed(&self, args: &Args) -> Result<Batch, Failure> {
        let change_id = args
            .str("change_id")
            .ok_or_else(|| Failure::BadArgs("change_id is required".to_owned()))?;
        let secret_configured = self.policy.approval_secret.is_some();
        let supplied = args.str("approval_token");
        if !secret_configured {
            return Err(Failure::NoCapability(
                "mark_reviewed needs an approval token bound to the change, and no approval secret is \
                 configured (mcp.approval_token); an agent may not mark a change reviewed on its own \
                 behalf (ADR-005 §4.2)"
                    .to_owned(),
            ));
        }
        if supplied.is_none() {
            return Err(Failure::NoCapability(format!(
                "mark_reviewed requires an approval token bound to `{change_id}`"
            )));
        }
        if !self.policy.approval_ok(change_id, supplied) {
            return Err(Failure::NoCapability(format!(
                "the approval token supplied for `{change_id}` does not authorise this change"
            )));
        }
        self.source.mark_reviewed(args)
    }

    fn add_review_note(&self, args: &Args) -> Result<Batch, Failure> {
        self.source.add_review_note(args)
    }
}

/// The size of the document as the caller will receive it.
fn byte_len(document: &Value) -> usize {
    serde_json::to_string(document)
        .map(|text| text.len())
        .unwrap_or(0)
}

/// The wrapper rmcp hands the tool implementations: a name plus a free-form object.
#[derive(Debug, serde::Deserialize)]
pub struct ToolCall {
    /// The tool to call.
    pub name: String,
    /// The arguments, if any.
    #[serde(default)]
    pub arguments: Option<Map<String, Value>>,
}

impl ServerHandler for FathomMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "fathom",
                self.policy.server_version.clone(),
            ))
            .with_instructions(format!(
                "Fathom answers questions about this repository's reviewed change model. Every answer is \
                 one JSON object with `contract_version`, `cost_ms`, `bytes_returned`, `degraded` and \
                 `unknowns`; a long answer is paginated and carries `next_cursor`. Nothing in an answer is \
                 a file body: paths, ids, counts and the model's own words only. This build's contract \
                 range is [{}; {}]; writes are {}.",
                envelope::CONTRACT_RANGE[0],
                envelope::CONTRACT_RANGE[1],
                if self.policy.writes_enabled {
                    "on, and every call to them is audited"
                } else {
                    "off, and the write tools are absent from tools/list"
                }
            ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let payload = self.list_tools();
        let tools: Vec<Tool> = payload
            .get("tools")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| serde_json::from_value::<Tool>(row.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = request.arguments.clone().unwrap_or_default();
        let document = self.call_tool(&request.name, arguments);
        let refused = document.get("error").is_some();
        let text = serde_json::to_string_pretty(&document).unwrap_or_else(|_| "{}".to_owned());
        let mut result = if refused {
            CallToolResult::error(vec![ContentBlock::text(text)])
        } else {
            CallToolResult::success(vec![ContentBlock::text(text)])
        };
        result.structured_content = Some(document);
        Ok(result.into())
    }
}

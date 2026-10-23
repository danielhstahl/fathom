//! `fathom mcp` — the composition root's half of the MCP server (`e6.4`, reco-p3i).
//!
//! `fathom-mcp` is the protocol, the gates and the bounds; **this file is the only thing that touches a
//! repository on its behalf.** That split is not a layering inconvenience, it is the design: ADR-002 §5
//! forbids `fathom-mcp` a store handle (the agent and the human must answer from one model, and lint `M02`
//! enforces it), so the reads that build an answer live here, in the one crate allowed to hold a store, and
//! are handed across [`fathom_mcp::source::Source`].
//!
//! Three verbs:
//!
//! * **`fathom mcp serve`** — speak MCP on stdio until the client hangs up. This is what an agent configures.
//! * **`fathom mcp tools`** — print the surface and its bounds, resolved against *this* repository's
//!   config, so "is the write half on here?" is a question a script can ask.
//! * **`fathom mcp approval-token <change>`** — mint the bound token `mark_reviewed` demands. Whoever runs
//!   this decided that one change may be marked reviewed; nobody else can, which is the whole point of the
//!   binding.
//!
//! # The shipped posture, and where each rule is enforced
//!
//! | Rule | Key | Enforced in |
//! |---|---|---|
//! | writes off by default | `mcp.allow_writes` = `false` | [`resolve`] → [`fathom_mcp::policy::Policy`] |
//! | a write needs a token bound to the change | `mcp.approval_token` (user scope) | [`fathom_mcp::policy::Policy::approval_ok`] |
//! | one audit line per gated call | — | [`fathom_mcp::server`] here, [`Review::audit`] there |
//! | responses bounded and paginated | `mcp.max_response_bytes` | [`fathom_mcp::tools`] + [`fathom_mcp::server`] |
//! | no secrets, no file bodies | — | [`fathom_mcp::egress`] + the pointer-shaped rows below |
//! | stdio only, no listener | `privacy.mcp_http_transport` | [`fathom_mcp::transport::serve`] |

use std::path::{Path, PathBuf};

use fathom_analyze::insight;
use fathom_core::exit::ExitCode;
use fathom_mcp::policy::{Policy, Transport};
use fathom_mcp::source::{Args, AuditLine, Batch, Failure, Source, Window};
use fathom_store::episode::EpisodeQuery;
use fathom_store::spine::Store;
use serde_json::{Value, json};

use crate::verbs::{McpAction, McpArgs};

/// Where one server run appends its audit lines, relative to the repository.
///
/// Under `.fathom/logs/` because that is where the daemon's log already lives (ADR-004 §2's namespace for
/// the tool's own bookkeeping), and because an audit trail an operator cannot `tail` is a bulletin board.
/// It is deliberately *not* the review ledger: the ledger is the claim that a human reviewed something and
/// is committed; the audit log is the record of what the agent asked for and is not.
pub const AUDIT_LOG: &str = ".fathom/logs/mcp-audit.jsonl";

/// Where review notes recorded through `add_review_note` wait for `e6.5`.
///
/// `e6.5` (reco-552) owns the committable markdown memory with a mirrored index, its relink behaviour and
/// its TUI surface. Until that lands the note has to live *somewhere* durable enough to be read back in the
/// next session, and a JSONL line under `.fathom/reviews/` is the honest interim: it is appended, it is
/// readable, it says what it is, and it does not pretend to be the ledger.
pub const NOTE_LOG: &str = ".fathom/reviews/notes.jsonl";

/// `fathom mcp …`
pub(crate) fn run(args: &McpArgs, root: &Path) -> eyre::Result<ExitCode> {
    match &args.action {
        McpAction::Serve => serve(root),
        McpAction::Tools { root, json } => tools(Path::new(root), *json),
        McpAction::ApprovalToken { change_id, root } => approval_token(Path::new(root), change_id),
    }
}

// ---------------------------------------------------------------------------
// the verbs
// ---------------------------------------------------------------------------

/// Resolve the policy, print what was decided, and serve.
fn serve(root: &Path) -> eyre::Result<ExitCode> {
    let policy = resolve(root)?;
    let source = std::sync::Arc::new(Review::open(root)?);
    // The operator needs to know which posture is live before an agent starts asking questions, and stdout
    // is about to belong to the protocol — so this goes out first, and on stderr.
    eprintln!(
        "fathom mcp: {} over {}; write tools {}; audit {}",
        policy.server_version,
        policy.transport.name(),
        if policy.writes_enabled {
            "ON (each call is audited)"
        } else {
            "off (absent from tools/list)"
        },
        match policy.audit_reads {
            true => "reads and writes",
            false => "writes only",
        }
    );
    let consented: Vec<&'static str> = crate::features::consent_states()
        .into_iter()
        .filter(|state| state.granted)
        .map(|state| state.key)
        .collect();
    fathom_mcp::transport::serve_blocking(policy, source, &consented)?;
    Ok(ExitCode::Ok)
}

/// `fathom mcp tools [--json]`: the surface as this repository's config resolves it.
fn tools(root: &Path, as_json: bool) -> eyre::Result<ExitCode> {
    let policy = resolve(root)?;
    let source = std::sync::Arc::new(Review::open(root)?);
    let server = fathom_mcp::server::FathomMcp::new(policy.clone(), source);
    let surface = server.tool_surface();
    if as_json {
        let payload = json!({
            "contract": "fathom.mcp.surface/1",
            "contract_version": fathom_mcp::envelope::CONTRACT_VERSION,
            "contract_range": fathom_mcp::envelope::CONTRACT_RANGE,
            "server_version": policy.server_version,
            "transport": policy.transport.name(),
            "writes_enabled": policy.writes_enabled,
            "approval_secret_configured": policy.approval_secret.is_some(),
            "max_response_bytes": policy.max_response_bytes,
            "audit_log": AUDIT_LOG,
            "tools": surface,
        });
        crate::output::line(format_args!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{{}}".to_owned())
        ));
        return Ok(ExitCode::Ok);
    }
    crate::output::line(format_args!(
        "fathom mcp · {} · {} · writes {}",
        policy.transport.name(),
        policy.server_version,
        if policy.writes_enabled { "on" } else { "off" }
    ));
    for tool in &surface {
        let advertised = tool
            .get("advertised")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        crate::output::line(format_args!(
            "  {:<19} {:<5} {:>3}–{:<3} {}{}",
            tool["name"].as_str().unwrap_or("?"),
            tool["capability"].as_str().unwrap_or("?"),
            tool["default_items"].as_u64().unwrap_or(0),
            tool["max_items"].as_u64().unwrap_or(0),
            tool["bound"].as_str().unwrap_or(""),
            if advertised {
                ""
            } else {
                "   [not offered to clients]"
            }
        ));
    }
    crate::output::line(format_args!(
        "  one response is at most {} bytes; anything longer is cut and handed back with a cursor",
        policy.max_response_bytes
    ));
    Ok(ExitCode::Ok)
}

/// `fathom mcp approval-token <change>`: mint the bound token for one change.
fn approval_token(root: &Path, change_id: &str) -> eyre::Result<ExitCode> {
    let policy = resolve(root)?;
    let Some(token) = policy.approval_token(change_id) else {
        return Err(crate::verbs::pending(
            ExitCode::ConfigInvalid,
            "no approval secret is configured, so no token can be minted",
            Some(
                "set `mcp.approval_token` in your own config — it is user-scoped on purpose: a token a \
                 clone could write is a token that is already leaked"
                    .to_owned(),
            ),
        ));
    };
    crate::output::line(format_args!("{token}"));
    Ok(ExitCode::Ok)
}

// ---------------------------------------------------------------------------
// the policy, out of the config layers
// ---------------------------------------------------------------------------

/// Read `[mcp]` from the repository's `fathom.toml`, then the user's own config over it, and produce the
/// [`Policy`] the server obeys.
///
/// Precedence follows the layering every other surface uses (repo, then user), with one hard rule that is
/// not a precedence: **`mcp.auth_token` and `mcp.approval_token` are read from the user's config only.** A
/// secret in a repository file is a secret in a clone (ADR-005 §4.3), so the repository's own file is not
/// consulted for those two keys however it is spelled.
pub fn resolve(root: &Path) -> Result<Policy, crate::verbs::Pending> {
    let repo = read_table(&root.join("fathom.toml"));
    let mut user = Vec::new();
    for candidate in crate::userconfig::candidates() {
        if candidate.is_file() {
            user = read_table(&candidate);
            break;
        }
    }
    let pick = |key: &str| -> Option<String> {
        user.iter()
            .find(|(name, _)| name == key)
            .or_else(|| repo.iter().find(|(name, _)| name == key))
            .map(|(_, value)| value.clone())
    };
    let secret = |key: &str| -> Option<String> {
        user.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .filter(|value| !value.trim().is_empty())
    };
    let boolean = |key: &str, default: bool| -> bool {
        match pick(key).as_deref().map(str::to_ascii_lowercase) {
            None => default,
            Some(value) => matches!(value.as_str(), "true" | "1" | "yes" | "on"),
        }
    };
    let bind = pick("bind").unwrap_or_else(|| "127.0.0.1".to_owned());
    let transport = match pick("transport").as_deref().unwrap_or("stdio") {
        "http" => Transport::Http {
            bind: bind.clone(),
            auth_configured: secret("auth_token").is_some(),
        },
        _ => Transport::Stdio,
    };
    let policy = Policy {
        server_version: format!("fathom-mcp/{}", env!("CARGO_PKG_VERSION")),
        transport,
        writes_enabled: boolean("allow_writes", false),
        approval_secret: secret("approval_token"),
        audit_reads: boolean("audit_reads", false),
        max_response_bytes: pick("max_response_bytes")
            .and_then(|value| value.replace('_', "").parse::<usize>().ok())
            .filter(|value| (4096..=8 * 1024 * 1024).contains(value))
            .unwrap_or(256 * 1024),
        ..Policy::default()
    };
    if matches!(policy.transport, Transport::Http { .. }) && !policy.transport.loopback() {
        return Err(crate::verbs::Pending::new(
            ExitCode::ConfigInvalid,
            format!("`mcp.bind` is `{bind}`, which is not loopback"),
            Some(
                "a listener on a shared host is a disclosure, not a convenience: bind to 127.0.0.1 or \
                 do not bind at all (ADR-005 §4.3)"
                    .to_owned(),
            ),
        ));
    }
    Ok(policy)
}

/// The `[mcp]` table of one TOML file, flattened to `key = value` strings.
///
/// The same deliberate smallness as every other `[section]` the shell reads: the MCP keys are five scalars,
/// and a parser that can only read them cannot mis-read a section it was not given. An unparseable file
/// yields no keys, because `fathom index` has already told the reviewer that file does not parse.
fn read_table(path: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let Some(table) = document.get("mcp").and_then(|item| item.as_table()) else {
        return Vec::new();
    };
    table
        .iter()
        .filter_map(|(key, value)| {
            let scalar = value.as_str().map(str::to_owned).or_else(|| {
                value.as_bool().map(|flag| {
                    if flag {
                        "true".to_owned()
                    } else {
                        "false".to_owned()
                    }
                })
            })?;
            Some((key.to_owned(), scalar))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// the source
// ---------------------------------------------------------------------------

/// The repository the tools answer questions about.
///
/// Holds paths, not handles: `Store` is not `Send`, and `fathom-mcp` needs its `Source` to be, so a store is
/// opened per call and dropped at the end of it. An open is a file open and a schema check — the reads are
/// the expensive part, and they were going to happen anyway.
pub struct Review {
    root: PathBuf,
    store_path: PathBuf,
}

impl Review {
    /// Check that the paths the tools need are nameable, and remember them.
    pub fn open(root: &Path) -> Result<Self, crate::verbs::Pending> {
        let store_path = fathom_analyze::index::default_store_path(root);
        if !store_path.exists() {
            return Err(crate::verbs::Pending::new(
                ExitCode::IndexDegraded,
                format!(
                    "no index at {} yet, so there is nothing for the agent to ask about",
                    store_path.display()
                ),
                Some(crate::state::REMEDY_INDEX.to_owned()),
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
            store_path,
        })
    }

    fn store(&self) -> Result<Store, Failure> {
        Store::open(&self.store_path).map_err(|err| Failure::Internal(err.to_string()))
    }

    fn repository(&self) -> Result<fathom_ingest::repo::Repository, Failure> {
        fathom_ingest::repo::Repository::discover(&self.root)
            .map_err(|err| Failure::Internal(err.to_string()))
    }

    /// The two endpoints of a range, defaulting to the last two indexed episodes.
    ///
    /// A default matters more than it looks: an agent that calls `architecture_delta` with no arguments is
    /// asking "what did the last thing I did do to the architecture", and answering that with a usage error
    /// is how a tool ends up unused.
    fn range(
        &self,
        store: &Store,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<(String, String), Failure> {
        let to = to.map(str::to_owned).unwrap_or_else(|| "HEAD".to_owned());
        let from = match from.map(str::to_owned) {
            Some(found) => found,
            None => {
                let mut rows = store
                    .episodes(&EpisodeQuery::default())
                    .map_err(|err| Failure::Internal(err.to_string()))?;
                rows.sort_by(|left, right| {
                    (left.last_utc, &left.lineage_ref, left.lineage_seq).cmp(&(
                        right.last_utc,
                        &right.lineage_ref,
                        right.lineage_seq,
                    ))
                });
                if rows.len() < 2 {
                    return Err(Failure::NoModel(
                        "the index holds fewer than two episodes, so there is no earlier revision to \
                         compare against; name `from` and `to` explicitly"
                            .to_owned(),
                    ));
                }
                rows[rows.len() - 2].id.clone()
            }
        };
        Ok((from, to))
    }

    /// The range for a tool whose endpoints are *revisions* (a dependency scan, a graph span).
    ///
    /// [`Self::range`] answers in episode ids, which is right for the tools that read the episode model and
    /// wrong here: a revision resolver will not take `ep-…` and the caller gets `bad_args` for a question
    /// that was fine. So the default `from` is the revision before `to`, taken from the commit graph itself,
    /// and the refusal when there is no earlier revision says that instead of blaming the arguments.
    fn revision_range(&self, args: &Args) -> Result<(String, String), Failure> {
        use fathom_ingest::revspec;
        let repository = self.repository()?;
        let to_token = args
            .str("to")
            .or_else(|| args.str("ref_b"))
            .map(str::to_owned)
            .unwrap_or_else(|| "HEAD".to_owned());
        let to_oid = revspec::resolve(&repository, &to_token).map_err(|err| {
            Failure::BadArgs(format!("`{to_token}` is not a revision here: {err}"))
        })?;
        let from_token = match args
            .str("from")
            .or_else(|| args.str("ref_a"))
            .map(str::to_owned)
        {
            Some(found) => found,
            None => repository
                .commit(to_oid)
                .ok()
                .flatten()
                .and_then(|header| header.parents.first().copied())
                .map(|oid| oid.hex())
                .ok_or_else(|| {
                    Failure::NoModel(format!(
                        "`{to_token}` has no parent revision in this repository, so there is no earlier \
                         revision to compare against; name `from` explicitly"
                    ))
                })?,
        };
        Ok((from_token, to_token))
    }

    /// The composed range, by the analysis that owns ranges (`e5.3`) — the same one `fathom cmp` prints.
    fn compare(
        &self,
        from: &str,
        to: &str,
    ) -> Result<fathom_analyze::compare::Comparison, Failure> {
        crate::compare::mcp_compare(&self.root, from, to).map_err(Failure::BadArgs)
    }

    /// `target` must resolve to something this index knows (§4.2: "target must resolve to a known id").
    fn resolve_target(&self, store: &Store, target: &str) -> Result<(String, String), Failure> {
        if let Some(row) = store
            .episode(target)
            .map_err(|err| Failure::Internal(err.to_string()))?
        {
            return Ok((row.id, "episode".to_owned()));
        }
        if let Some(row) = store
            .episode_for_commit(target)
            .map_err(|err| Failure::Internal(err.to_string()))?
        {
            return Ok((row.id, "commit".to_owned()));
        }
        let rows = store
            .episodes(&EpisodeQuery::default())
            .map_err(|err| Failure::Internal(err.to_string()))?;
        let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
        let paths = store
            .episode_paths(&ids)
            .map_err(|err| Failure::Internal(err.to_string()))?;
        if paths
            .values()
            .flatten()
            .any(|path| path == target || path.starts_with(&format!("{target}/")))
        {
            return Ok((target.to_owned(), "path".to_owned()));
        }
        Err(Failure::NotReviewable(format!(
            "`{target}` resolves to no episode, commit or path in this index; a note that points at \
             nothing will be indistinguishable from a note that was deleted"
        )))
    }
}

impl Source for Review {
    fn explain_episode(&self, args: &Args, window: Window) -> Result<Batch, Failure> {
        let asked = args
            .str("episode_id")
            .ok_or_else(|| Failure::BadArgs("episode_id is required".to_owned()))?;
        let store = self.store()?;
        let found = insight::for_episode(&store, asked)
            .map_err(|err| Failure::Internal(err.to_string()))?
            .or_else(|| {
                let repository = self.repository().ok()?;
                let oid = fathom_ingest::revspec::resolve(&repository, asked).ok()?;
                insight::for_commit(&store, &oid.hex()).ok().flatten()
            })
            .ok_or_else(|| {
                Failure::NotReviewable(format!(
                    "`{asked}` names no episode in this index; `fathom episodes --json` lists what it \
                     holds, and `fathom analyze {asked} --json` answers the same question from the shell"
                ))
            })?;
        let document = serde_json::to_value(&found)
            .map_err(|err| Failure::Internal(format!("the delta would not serialise: {err}")))?;
        let files = found.files.clone();
        let total = files.len();
        let items: Vec<Value> = files
            .into_iter()
            .skip(window.offset)
            .take(window.limit)
            .filter_map(|file| serde_json::to_value(file).ok())
            .collect();
        // Everything but the paginated collection travels as envelope fields, so the document a client
        // gets is `EpisodeDelta` with `files` cut to the page — the same document, not a summary of it.
        let mut batch = Batch::new().items(items).total(total);
        if let Some(map) = document.as_object() {
            for (key, value) in map {
                if key != "files" {
                    batch = batch.field(key, value.clone());
                }
            }
        }
        if args.flag("include_events").unwrap_or(false) {
            let members = store.episode_members(&found.episode.id).unwrap_or_default();
            batch = batch.field(
                "events",
                members
                    .iter()
                    .map(|oid| json!({ "commit": oid }))
                    .collect::<Vec<_>>(),
            );
            batch = batch.unknown(
                "events are member-commit identities only: harness event bodies and commit messages do \
                 not leave through this door (ADR-005 §4.3)",
            );
        }
        for gap in found.gaps.iter().take(24) {
            batch = batch.unknown(format!("{}: {}", gap.section, gap.short));
        }
        Ok(batch)
    }

    fn changes_between(&self, args: &Args, window: Window) -> Result<Batch, Failure> {
        let from = args.str("ref_a").unwrap_or("HEAD~1");
        let to = args.str("ref_b").unwrap_or("HEAD");
        let comparison = self.compare(from, to)?;
        let filter = args.str("path_filter").map(str::to_owned);
        let rows: Vec<_> = comparison
            .delta
            .files
            .iter()
            .filter(|file| {
                filter
                    .as_deref()
                    .is_none_or(|prefix| file.path.starts_with(prefix))
            })
            .collect();
        let total = rows.len();
        let spec = format!("{from}..{to}");
        let items: Vec<Value> = rows
            .into_iter()
            .skip(window.offset)
            .take(window.limit)
            .map(|file| {
                json!({
                    "change_id": format!("chg:{}:{}", comparison.to.episode, file.path),
                    "kind": file.change,
                    "risk_tier": tier_of(file),
                    "tier_basis": "this path's own churn and surface, not the episode's score",
                    "paths": [file.path],
                    "add": file.add,
                    "del": file.del,
                    "binary": file.binary,
                    "surface": file.surface,
                    "path_class": file.path_class,
                    "noise_class": file.noise_class,
                    "evidence": [
                        format!("episode:{}", comparison.to.episode),
                        format!("range:{spec}"),
                    ],
                })
            })
            .collect();
        let mut batch = Batch::new()
            .items(items)
            .total(total)
            .field("from", comparison.from.spec.clone())
            .field("to", comparison.to.spec.clone())
            .field("lineage", format!("{:?}", comparison.lineage))
            .field("episodes", comparison.episodes.len())
            .field("range", spec)
            .field(
                "totals",
                serde_json::to_value(&comparison.delta.totals).unwrap_or(Value::Null),
            )
            .field("contract", comparison.contract.clone());
        for note in &comparison.notes {
            batch = batch.unknown(note.clone());
        }
        for gap in comparison.delta.gaps.iter().take(16) {
            batch = batch.unknown(format!("{}: {}", gap.section, gap.short));
        }
        Ok(batch)
    }

    fn architecture_delta(&self, args: &Args, window: Window) -> Result<Batch, Failure> {
        use fathom_analyze::graph::Lane;
        let mut store = self.store()?;
        let (from, to) = self.range(
            &store,
            args.str("from").or_else(|| args.str("ref_a")),
            args.str("to").or_else(|| args.str("ref_b")),
        )?;
        // Say *which* kind of "cannot answer" this is. A graph with no snapshots at all is a different
        // sentence from a graph that has them but no anchor across this span, and the difference is the
        // difference between "this build's indexer has not written the graph yet" — the write path is
        // Epic 3's — and "your range is unindexable". An agent that gets the second message for the first
        // reason will retry forever.
        let graph_is_empty = store
            .graph_payload_bytes()
            .is_ok_and(|bytes| bytes.manifest_rows == 0);
        if graph_is_empty {
            return Err(Failure::NoModel(
                "this index holds no module graph, so no architecture delta can be computed from it; \
                 the indexer does not write graph snapshots in this build, so `nodes`, `edges` and \
                 `layers` are unknowns here rather than empty"
                    .to_owned(),
            ));
        }
        let lane = Lane::new(&mut store, fathom_analyze::graph::build::Builder::default())
            .map_err(|err| Failure::NoModel(format!("the graph lane would not open: {err}")))?;
        let delta = lane.architecture_delta(&from, &to).map_err(|err| {
            Failure::NoModel(format!(
                "the graph cannot answer between `{from}` and `{to}` from this index: {} — name the \
                 endpoints `fathom episodes` lists, or rebuild with `fathom index`",
                err.sentence()
            ))
        })?;
        let mut items: Vec<Value> = Vec::new();
        let mut push = |kind: &str, facts: Vec<Value>| {
            for fact in facts {
                let subject = fact
                    .get("key")
                    .or_else(|| fact.get("module"))
                    .or_else(|| fact.get("path"))
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_owned();
                items.push(json!({
                    "finding": kind,
                    "subject": subject,
                    "detail": fact,
                    "evidence": [
                        format!("graph:{from}..{to}"),
                        format!("rung:{}", format!("{:?}", delta.cost.rung).to_ascii_lowercase()),
                    ],
                }));
            }
        };
        push("node_added", values(&delta.nodes_added)?);
        push("node_removed", values(&delta.nodes_removed)?);
        push("edge_added", values(&delta.edges_added)?);
        push("edge_removed", values(&delta.edges_removed)?);
        push("repointed", values(&delta.repointed)?);
        push("move", values(&delta.moves)?);
        push("reversal", values(&delta.reversals)?);
        push("surface_change", values(&delta.surface)?);
        push("layer_drift", values(&delta.layer_drift)?);
        let total = items.len();
        let items: Vec<Value> = items
            .into_iter()
            .skip(window.offset)
            .take(window.limit)
            .collect();
        let counts: Vec<Value> = delta
            .counts()
            .into_iter()
            .map(|(kind, count)| json!({ "kind": kind, "count": count }))
            .collect();
        let mut batch = Batch::new()
            .items(items)
            .total(total)
            .field("from", from)
            .field("to", to)
            .field("adjacent", delta.adjacent)
            .field("cost", json_of(&delta.cost)?)
            .field("counts", counts)
            .field("modules_touched", delta.modules())
            .field("contract", delta.contract.clone());
        for notice in &delta.notices {
            batch = batch.unknown(notice.clone());
        }
        Ok(batch)
    }

    fn dependency_delta(&self, args: &Args, window: Window) -> Result<Batch, Failure> {
        use fathom_analyze::deps::{DependencyOptions, dependency_episode_repository};
        let (from, to) = self.revision_range(args)?;
        let repository = self.repository()?;
        let to_oid = fathom_ingest::revspec::resolve(&repository, &to)
            .map_err(|err| Failure::BadArgs(format!("`{to}` is not a revision here: {err}")))?;
        let from_oid = fathom_ingest::revspec::resolve(&repository, &from)
            .map_err(|err| Failure::BadArgs(format!("`{from}` is not a revision here: {err}")))?;
        let tree_of = |oid: fathom_ingest::oid::Oid| {
            repository.commit(oid).ok().flatten().map(|head| head.tree)
        };
        let episode = dependency_episode_repository(
            &repository,
            tree_of(from_oid),
            tree_of(to_oid),
            &DependencyOptions::default(),
        )
        .map_err(|err| Failure::Internal(format!("the dependency scan failed: {err}")))?;
        let model = episode.model;
        let wanted = args.str("package").map(str::to_owned);
        let rows: Vec<_> = model
            .changes
            .iter()
            .filter(|change| {
                wanted
                    .as_deref()
                    .is_none_or(|name| change.name == name || change.name.contains(name))
            })
            .collect();
        let total = rows.len();
        let items: Vec<Value> = rows
            .into_iter()
            .skip(window.offset)
            .take(window.limit)
            .map(|change| {
                // Pointer-shaped on purpose. `change.proof` holds a lockfile hunk — real lines from a real
                // file — and a dependency row does not need it to be checkable: the path and the hunk
                // header are the citation, and the bytes are one `git show` away, behind the agent's own
                // permission to read files (ADR-005 §4.3).
                let evidence = match &change.proof {
                    Some(proof) if proof.found_hunk() => {
                        format!("hunk:{}:{}", proof.path, proof.header)
                    }
                    Some(proof) => format!(
                        "noproof:{}:{}",
                        proof.path,
                        proof.missing.clone().unwrap_or_default()
                    ),
                    None => format!("manifest:{}", change.scope),
                };
                json!({
                    "change_id": format!(
                        "dep:{:?}:{:?}:{}",
                        change.ecosystem, change.kind, change.name
                    ),
                    "kind": format!("{:?}", change.kind),
                    "ecosystem": format!("{:?}", change.ecosystem),
                    "package": change.package(),
                    "before": change.before,
                    "after": change.after,
                    "attribution": format!("{:?}", change.attribution),
                    "dev": change.dev,
                    "asked_by": change.asked_by,
                    "detail": change.detail,
                    "evidence": [evidence],
                })
            })
            .collect();
        let mut batch = Batch::new()
            .items(items)
            .total(total)
            .field("from", from)
            .field("to", to)
            .field("headline", model.headline.clone())
            .field("counts", json_of(&model.counts)?)
            .field("graph", json_of(&model.graph)?)
            .field("sources", json_of(&model.sources)?)
            .field("sources_gone", json_of(&model.sources_gone)?)
            .field("licenses", json_of(&model.licenses)?)
            .field("pattern", format!("{:?}", model.pattern))
            .field("complete", model.complete)
            .field("manifest_files_read", model.files);
        for note in &model.notes {
            batch = batch.unknown(note.clone());
        }
        if !model.complete {
            batch = batch
                .unknown("one or both sides of the dependency comparison were read only partially");
        }
        Ok(batch)
    }

    fn risk_report(&self, args: &Args, window: Window) -> Result<Batch, Failure> {
        let store = self.store()?;
        let min_tier = args.str("min_tier").map(str::to_ascii_lowercase);
        let span: Vec<String> = match (args.str("ref_a"), args.str("ref_b")) {
            (Some(from), Some(to)) => self.compare(from, to)?.episodes,
            _ => {
                let query = EpisodeQuery {
                    limit: i64::try_from(window.limit).unwrap_or(i64::MAX),
                    ..EpisodeQuery::default()
                };
                store
                    .episodes(&query)
                    .map_err(|err| Failure::Internal(err.to_string()))?
                    .into_iter()
                    .rev()
                    .map(|row| row.id)
                    .collect()
            }
        };
        let mut rows: Vec<Value> = Vec::new();
        let mut unknowns: Vec<String> = Vec::new();
        for id in &span {
            let Some(delta) = insight::for_episode(&store, id)
                .map_err(|err| Failure::Internal(err.to_string()))?
            else {
                unknowns.push(format!("{id}: the index has no row for it any more"));
                continue;
            };
            let Some(risk) = delta.risk.as_ref() else {
                unknowns.push(format!("{id}: no risk score was computed for this episode"));
                continue;
            };
            if !tier_allows(min_tier.as_deref(), risk.score) {
                continue;
            }
            rows.push(json!({
                "change_id": id,
                "author_kind": delta.episode.author_kind,
                "score": risk.score,
                "risk_tier": tier_of_score(risk.score),
                "factors": risk.components,
                "surfaces": risk.surfaces,
                "vector": risk.vector,
                "model_version": risk.model,
                "model_artifact_id": Value::Null,
                "training_corpus_version": Value::Null,
                "trained": false,
                "grounding": "heuristic — no trained model exists (reco-884's bar is unmet)",
                "rank": risk.rank,
                "ranked_of": risk.ranked,
                "paths": delta.totals.n_files,
                "churn": delta.totals.churn,
                "commits": delta.episode.n_commits,
                "reviewed": delta.episode.review.clone(),
                "evidence": [format!("episode:{id}"), format!("model:{}", risk.model)],
            }));
            for spot in &risk.blind_spots {
                unknowns.push(spot.clone());
            }
        }
        rows.sort_by(|left, right| {
            right["score"]
                .as_i64()
                .unwrap_or(0)
                .cmp(&left["score"].as_i64().unwrap_or(0))
                .then_with(|| left["change_id"].as_str().cmp(&right["change_id"].as_str()))
        });
        let total = rows.len();
        let items: Vec<Value> = rows
            .into_iter()
            .skip(window.offset)
            .take(window.limit)
            .collect();
        let mut batch = Batch::new()
            .items(items)
            .total(total)
            .field("model_version", insight::MODEL)
            .field("model_artifact_id", Value::Null)
            .field("training_corpus_version", Value::Null)
            .field("trained", false)
            .field(
                "grounding",
                "heuristic — no trained model exists (reco-884's calibration bar is unmet)",
            )
            .field("min_tier", min_tier.unwrap_or_else(|| "all".to_owned()))
            .field("span", span.len());
        unknowns.sort();
        unknowns.dedup();
        for note in unknowns.into_iter().take(32) {
            batch = batch.unknown(note);
        }
        Ok(batch)
    }

    fn search_history(&self, args: &Args, window: Window) -> Result<Batch, Failure> {
        use fathom_analyze::search::{Index, query::Query};
        let asked = args
            .str("query")
            .ok_or_else(|| Failure::BadArgs("query is required".to_owned()))?;
        let corpus = crate::search::corpus_for(&self.root).ok_or_else(|| {
            Failure::NoModel(
                "the search corpus could not be built from this index; `fathom index` rebuilds it"
                    .to_owned(),
            )
        })?;
        let index = Index::build(&corpus);
        let now = {
            use fathom_core::time::Clock as _;
            crate::clock::SystemClock.now()
        };
        let report = index.search(&Query::parse(asked, now), window.limit);
        let since = args
            .str("since")
            .and_then(fathom_analyze::ledger::parse_iso_utc);
        let author = args.str("author").map(str::to_ascii_lowercase);
        let items: Vec<Value> = report
            .hits
            .iter()
            // Pointer-shaped, always: `Hit` carries a snippet and a per-token `why`, and neither leaves
            // here. The answer is "which episode, when, what is it called, how sure are we, what matched"
            // — the rest is what `explain_episode` is for (ADR-005 §4.1).
            .filter(|hit| {
                since.is_none_or(|since| {
                    hit.utc
                        .is_none_or(|stamp| stamp.as_unix_micros() / 1000 >= since)
                }) && author
                    .as_deref()
                    .is_none_or(|name| hit.label.to_ascii_lowercase().contains(name))
            })
            .map(|hit| {
                json!({
                    "episode_id": hit.episode.clone().unwrap_or_else(|| hit.id.clone()),
                    "kind": hit.kind.as_str(),
                    "ts": hit
                        .utc
                        .map(|stamp| fathom_analyze::ledger::iso_utc(stamp.as_unix_micros() / 1000))
                        .unwrap_or_default(),
                    "title": hit.label,
                    "score": hit.score,
                    "matched_on": hit.matched_on,
                })
            })
            .collect();
        let total = items.len();
        let mut batch = Batch::new()
            .items(items)
            .total(total)
            .field("query", report.query.clone())
            .field("tokens", report.tokens.clone())
            .field("documents_searched", report.documents)
            .field("candidates", report.candidates)
            .field("scored", report.scored)
            .field("cost_store_ms", report.micros / 1000)
            .field("contract", fathom_analyze::search::CONTRACT);
        for note in report
            .notes
            .iter()
            .chain(report.unknown.iter())
            .chain(report.gaps.iter().map(|gap| &gap.because))
        {
            batch = batch.unknown(note.clone());
        }
        if report.truncated {
            batch = batch.unknown(
                "the ranking held rows back; `total` counts what this call was willing to return, and \
                 a narrower query is the way to see the rest",
            );
        }
        Ok(batch)
    }

    fn mark_reviewed(&self, args: &Args) -> Result<Batch, Failure> {
        use fathom_analyze::ledger::{Decision, collect, format as ledger_format, sign};
        let change_id = args
            .str("change_id")
            .ok_or_else(|| Failure::BadArgs("change_id is required".to_owned()))?;
        let reviewer = args
            .str("reviewer")
            .ok_or_else(|| Failure::BadArgs("reviewer is required".to_owned()))?;
        let note = args.str("note").unwrap_or("");
        let mut store = self.store()?;
        let facts =
            collect::episode_facts(&store).map_err(|err| Failure::Internal(err.to_string()))?;
        let episode = crate::review::find_episode_for(&facts, change_id).ok_or_else(|| {
            Failure::NotReviewable(format!(
                "`{change_id}` is not an episode this index knows, and a sign-off that points at nothing \
                 is worse than no sign-off; `fathom episodes --json` lists the ids"
            ))
        })?;
        let actor = crate::review::mcp_actor(reviewer);
        let already = collect::read(&self.root)
            .0
            .into_iter()
            .any(|entry| entry.episode == episode.id && entry.actor.display == actor.display);
        if already {
            return Ok(Batch::new()
                .field("already", true)
                .field("change_id", episode.id.clone())
                .field("reviewer", actor.display.clone())
                .field(
                    "written",
                    "an entry for this reviewer and this change is already in the ledger; nothing was \
                     appended (idempotent per (change_id, reviewer))",
                ));
        }
        let now = crate::clock::now_millis();
        let entry = sign(&episode, &actor, Decision::Accepted, note, now).map_err(|err| {
            Failure::NotReviewable(format!("the ledger refused the entry: {}", err.message()))
        })?;
        let path = ledger_format::default_path(&self.root);
        ledger_format::append_entries(&path, std::slice::from_ref(&entry))
            .map_err(|err| Failure::Internal(format!("writing the ledger failed: {err}")))?;
        let blockers = crate::review::must_survive_blockers(&self.root);
        let report = collect::refresh(&mut store, &self.root, now, &blockers)
            .map_err(|err| Failure::Internal(format!("mirroring the ledger failed: {err}")))?;
        Ok(Batch::new()
            .field("already", false)
            .field("entry_id", entry.id.clone())
            .field("change_id", episode.id.clone())
            .field("reviewer", actor.display.clone())
            .field("actor_kind", actor.kind.as_str())
            .field("decision", Decision::Accepted.as_str())
            .field("written_to", display(&path, &self.root))
            .field("ledger_files", report.files.len())
            .unknown(
                "a sign-off recorded by an agent on a human's behalf is a claim about that human: it \
                 names them, it is audited, and it is not the same act as a person signing in the TUI",
            ))
    }

    fn add_review_note(&self, args: &Args) -> Result<Batch, Failure> {
        let target = args
            .str("target")
            .ok_or_else(|| Failure::BadArgs("target is required".to_owned()))?;
        let body = args
            .str("body")
            .ok_or_else(|| Failure::BadArgs("body is required".to_owned()))?;
        let kind = match args.str("kind").unwrap_or("note") {
            "note" | "question" | "decision" | "flag" => args.str("kind").unwrap_or("note"),
            other => {
                return Err(Failure::BadArgs(format!(
                    "kind `{other}` is not one of note, question, decision or flag"
                )));
            }
        };
        let store = self.store()?;
        let (target, target_kind) = self.resolve_target(&store, target)?;
        let line = json!({
            "contract": "fathom.review_note/1",
            "ts_utc": crate::clock::now_millis(),
            "kind": kind,
            "target": target.clone(),
            "target_kind": target_kind.clone(),
            "author": args.str("reviewer").unwrap_or("unattributed"),
            "via": "fathom mcp add_review_note",
            "body": fathom_core::redact::redact(body).0,
            "body_bytes": body.len(),
        });
        let path = self.root.join(NOTE_LOG);
        append_line(&path, &line).map_err(|err| {
            Failure::Internal(format!(
                "the note could not be appended to {}: {err}",
                path.display()
            ))
        })?;
        Ok(Batch::new()
            .field("written_to", display(&path, &self.root))
            .field("kind", kind)
            .field("target", target)
            .field("target_kind", target_kind)
            .field("body_bytes", body.len())
            .unknown(
                "notes recorded here are the interim store; e6.5 (reco-552) owns the committable \
                 markdown memory with a mirrored index, and this file is what it will index",
            ))
    }

    fn audit(&self, line: &AuditLine) -> Result<(), String> {
        let path = self.root.join(AUDIT_LOG);
        let record = json!({
            "contract": "fathom.mcp.audit/1",
            "ts_utc": crate::clock::now_millis(),
            "tool": line.tool,
            "capability": line.capability,
            "args_sha256": line.args_digest,
            "outcome": line.outcome,
            "cost_ms": line.cost_ms,
            "bytes_returned": line.bytes_returned,
            "reviewer": line.reviewer,
        });
        append_line(&path, &record).map_err(|err| err.to_string())
    }
}

// ---------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------

/// Serialise a value into the envelope, or refuse with the one sentence the caller can act on.
fn json_of<T: serde::Serialize + ?Sized>(value: &T) -> Result<Value, Failure> {
    serde_json::to_value(value)
        .map_err(|err| Failure::Internal(format!("the answer would not serialise: {err}")))
}

/// The same, for a field the model publishes as a list of objects.
fn values<T: serde::Serialize + ?Sized>(value: &T) -> Result<Vec<Value>, Failure> {
    match json_of(value)? {
        Value::Array(rows) => Ok(rows),
        _ => Err(Failure::Internal("expected a list".to_owned())),
    }
}

/// Append one JSON line, creating the parent directory the first time.
fn append_line(path: &Path, record: &Value) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    file.flush()
}

/// A repository-relative path for a message.
fn display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// The four tiers every risk answer is filtered by, in the same words everywhere.
fn tier_of_score(score: i64) -> &'static str {
    match score {
        0..=24 => "low",
        25..=49 => "medium",
        50..=74 => "high",
        _ => "critical",
    }
}

/// Does a row of this score survive a `min_tier` filter?
fn tier_allows(min_tier: Option<&str>, score: i64) -> bool {
    let rank = |tier: &str| match tier {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        "critical" => 3,
        _ => 0,
    };
    min_tier.is_none_or(|wanted| rank(wanted) <= rank(tier_of_score(score)))
}

/// The tier of one file row, from the churn and surface the model already assigned it.
///
/// Coarse on purpose, and labelled as coarse in the row (`tier_basis`): this is a filter a reader narrows,
/// not a second risk score. Deriving a second score in the composition root would be exactly the second
/// analysis lint `M03` exists to refuse.
fn tier_of(file: &insight::FileDelta) -> &'static str {
    let churn = file.churn().max(0);
    let weight: i64 = insight::classify(&file.path, &file.path_class, &file.noise_class)
        .weight()
        .try_into()
        .unwrap_or(0);
    match churn + weight {
        0..=40 => "low",
        41..=150 => "medium",
        151..=600 => "high",
        _ => "critical",
    }
}

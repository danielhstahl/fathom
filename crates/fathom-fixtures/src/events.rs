//! `events.rs` — the harness event log that goes with a generated repository.
//!
//! The corpus's repositories are agent-driven, and ADR-005 says what that means on disk: `.fathom/events.jsonl`,
//! one JSON object per line, keys sorted, `seq` strictly increasing from 1, a `fathom_event` projection on every
//! record, and no file contents crossing the boundary — digests, paths and counts only. So a fixture's events are
//! written to that contract and `e6.1`'s ingestion can be tested against a stream whose commits it can also read:
//! the trailer on commit 7 and the `file_edit` naming the same path describe one edit twice, once as the developer
//! recorded it and once as the harness saw it — which is the redundancy an episode claim is supposed to survive.
//!
//! Two layers, per §2, and they are easy to conflate:
//!
//! * the **wire layer** is the vendor's own record — `seq`, `ts`, `harness`, `source`, `session_id`, `cwd`,
//!   `git_branch`, and a `payload` that is "the vendor's JSON verbatim, never reshaped";
//! * the **canonical layer** is `fathom_event`: the name Fathom's own union gives that record, or `null` when
//!   nothing in Fathom means it.
//!
//! The mapping is not this file's to invent. `src/recommender/harness/harness.py`'s `project()` is the
//! executable contract, and its table is reproduced in this module's own `projection` — including the vendor-neutral origins
//! (`file_change`), which is how a deletion or a `git mv` becomes a `file_edit` at all: `PostToolUse` on a Bash
//! call projects to `tool_result`, and an ingest that had to infer the edit from a shell string would be
//! inferring what the fixture already knows.
//!
//! Two things follow from the boundary rule that are worth stating because they are not obvious:
//!
//! * a prompt is stored as `prompt_sha256` and `prompt_chars`, the way §2.1's `turn_start` row names them, not
//!   as text. A corpus whose digests moved because somebody reworded a subject line would be a corpus nobody
//!   re-blesses on purpose;
//! * no `test_run` is written here. §2.2 says it is *derived* from a recognisable test command by the ingest,
//!   so a fixture that wrote one would be testing its own invention instead of the derivation. What the fixture
//!   does write is the `tool_call`/`tool_result` pair for `cargo test`, which is the input the derivation reads.

use std::collections::BTreeMap;
use std::path::Path;

use crate::Result;
use crate::digest::sha256_hex;
use crate::generate::{Build, CommitEntry};
use crate::git::{self, File, Mode};
use crate::policy;
use crate::script::Events;

/// The path ADR-005 §3.1 gives the event log by default, relative to the repository root.
pub const PATH: &str = ".fathom/events.jsonl";

/// The trailer that asks for one extra record on a commit.
///
/// The value is used verbatim as the record's `source`, so it can name an event the projection table has no row
/// for — which is the point. ADR-005 §2 requires an unprojectable event to be stored with `fathom_event: null`
/// and *counted*, not dropped, and a corpus with no such record is a corpus where that counter is never tested.
pub const HOOK_TRAILER: &str = "fathom-hook";

/// Where the event log went, and how to tell it is the same one twice.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EventsSummary {
    /// See [`PATH`].
    pub path: &'static str,
    /// Whether a stream was written at all.
    pub written: bool,
    /// How many events.
    pub count: usize,
    /// When the stream covers only the first commits, how many; `None` means the whole history.
    pub truncated_at: Option<usize>,
    /// Digest of the file with the machine-dependent root replaced by `<ROOT>`.
    ///
    /// The paths inside the log are absolute because ADR-005 §6.1 resolves containment against them, and an
    /// absolute path is the one thing in a fixture that cannot be portable. Normalising exactly that string is
    /// what lets the digest mean "the same stream" on three machines.
    pub digest: String,
}

/// A JSON object with sorted keys, which is what ADR-005 §3.2 requires and what a `BTreeMap` gives.
type Map = BTreeMap<String, serde_json::Value>;

macro_rules! json {
    ($($key:expr => $value:expr),* $(,)?) => {{
        let mut map: Map = Map::new();
        $( map.insert($key.to_owned(), $value); )*
        map
    }};
}

macro_rules! text {
    ($value:expr) => {
        serde_json::Value::String($value.into())
    };
}

/// A nested JSON object, for a payload that carries its own object.
fn object(map: Map) -> serde_json::Value {
    serde_json::Value::Object(map.into_iter().collect())
}

macro_rules! number {
    ($value:expr) => {
        serde_json::Value::from($value)
    };
}

/// Which `fathom_event` a wire `source` projects to, `None` meaning "stored and counted" (§2.1).
///
/// This is `project()`'s table from the reference implementation, kept in the same order as that source so the
/// two can be read against each other. Drift between them is not silent: the contract audit re-derives every
/// record's projection through the Python function itself.
fn projection(source: &str) -> Option<&'static str> {
    Some(match source {
        "SessionStart" | "session-start" | "session.created" => "session_start",
        "SessionEnd" | "session-end" => "session_end",
        "UserPromptSubmit" | "prompt.submit" => "turn_start",
        "Stop" | "turn.stop" => "turn_end",
        "StopFailure" | "PostToolUseFailure" | "PreCompact" | "error" => "error",
        "PreToolUse" | "tool.start" | "tool.execute.before" => "tool_call",
        "PostToolUse" | "tool.end" => "tool_result",
        "Write" | "Edit" | "MultiEdit" | "file_change" | "file.change" | "file.edited" => {
            "file_edit"
        }
        "PermissionRequest" | "PermissionDenied" | "permission.request" => "approval",
        "SubagentStart" => "subagent_start",
        "SubagentStop" => "subagent_end",
        _ => return None,
    })
}

/// The `change` vocabulary ADR-005 §2.1 gives `file_edit`, which is not the manifest's.
///
/// The manifest says `added`/`deleted`/`renamed`/`modified` because that is what a tree diff reads like; the
/// contract says `add`/`delete`/`update`. A rename is an `update` — the path is the thing that changed, and the
/// record carries both paths precisely so a reader does not have to guess which.
fn edit_change(change: &str) -> &'static str {
    match change {
        "added" => "add",
        "deleted" => "delete",
        _ => "update",
    }
}

/// The wire `source` a commit's change is reported under.
///
/// An add is a `Write` and an edit an `Edit`, because that is what the harness saw. Everything else — a deletion,
/// a rename, a mode change — came from a shell command, and the vendor-neutral `file_change` origin exists for
/// exactly that: reporting it as a `Bash` tool result would leave the edit to be inferred from a command string.
fn edit_source(change: &str) -> &'static str {
    match change {
        "added" => "Write",
        "modified" => "Edit",
        _ => "file_change",
    }
}

/// Write `.fathom/events.jsonl` for the commits the fixture shows.
pub(crate) fn write(dest: &Path, build: &Build, policy_events: &Events) -> Result<EventsSummary> {
    let limit = match policy_events {
        Events::None => {
            return Ok(EventsSummary {
                path: PATH,
                written: false,
                count: 0,
                truncated_at: None,
                digest: String::new(),
            });
        }
        Events::All => usize::MAX,
        Events::First(limit) => *limit,
    };
    let dir = dest.join(".fathom");
    std::fs::create_dir_all(&dir).map_err(|source| git::Error::Io {
        context: dir.display().to_string(),
        source,
    })?;

    let commits: Vec<CommitEntry> = build.visible().take(limit).cloned().collect();
    let mut stream = Stream::new(&dest.display().to_string());

    // Where each turn and each session ends, so the closing events land on the commit where they happened. A
    // turn's last commit is not necessarily the turn's only commit: a multi-commit turn exists precisely to
    // show an episode that spans commits, and closing it at the first one would describe a turn that stopped
    // before the work did.
    let mut last_of_turn: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut last_of_session: BTreeMap<String, usize> = BTreeMap::new();
    let mut turn_number: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut sessions: Vec<String> = Vec::new();
    for (position, entry) in commits.iter().enumerate() {
        let (Some(session), Some(turn)) = (entry.session.clone(), entry.turn.clone()) else {
            continue;
        };
        if !sessions.contains(&session) {
            sessions.push(session.clone());
        }
        let pair = (session.clone(), turn.clone());
        let index = *turn_number.get(&pair).unwrap_or(&0) + 1;
        turn_number.insert(pair.clone(), index);
        last_of_turn.insert(pair.clone(), position);
        last_of_session.insert(session, position);
    }

    let mut open_sessions: Vec<String> = Vec::new();
    let mut open_turns: Vec<(String, String)> = Vec::new();
    for (position, entry) in commits.iter().enumerate() {
        let (Some(session), Some(turn)) = (entry.session.clone(), entry.turn.clone()) else {
            continue;
        };
        let pair = (session.clone(), turn.clone());
        if !open_sessions.contains(&session) {
            open_sessions.push(session.clone());
            stream.emit(
                entry,
                "SessionStart",
                &session,
                None,
                json! {
                    "model" => text!(policy::identity(entry.author).map_or("unknown", |id| id.tool)),
                    "permission_mode" => text!("acceptEdits"),
                    "source" => text!("startup"),
                },
            );
        }
        if !open_turns.contains(&pair) {
            open_turns.push(pair.clone());
            // §2.1's `turn_start` row names `prompt_sha256`, `prompt_chars` and `turn_id`. The prompt itself
            // stays out: the corpus's own prompts are commit subjects, which git stores anyway, and a fixture
            // whose digests moved every time somebody reworded a subject line is a fixture nobody re-blesses
            // deliberately.
            let mut payload = json! {
                "prompt_sha256" => text!(sha256_hex(entry.subject.as_bytes())),
                "prompt_chars" => number!(u64::try_from(entry.subject.chars().count()).unwrap_or(u64::MAX)),
                "turn_id" => text!(format!("{session}:{}", turn_number[&pair])),
            };
            if let Some(intent) = entry.trailers.get("fathom-intent") {
                payload.insert("declared_intent".to_owned(), text!(intent.clone()));
            }
            stream.emit(entry, "UserPromptSubmit", &session, None, payload);
        }

        // One tool pair per moved path, then one `file_edit` for the edit itself.
        let before = build.tree_at(position.saturating_sub(1));
        let after = build.tree_at(position);
        for change in &entry.changes {
            let tool = match change.change {
                "added" => "Write",
                "deleted" | "renamed" => "Bash",
                _ => "Edit",
            };
            let command = match change.change {
                "deleted" => format!("git rm -- {}", change.path),
                "renamed" => format!(
                    "git mv -- {} {}",
                    change.from.clone().unwrap_or_default(),
                    change.path
                ),
                _ => String::new(),
            };
            let input = if command.is_empty() {
                json! { "file_path" => text!(change.path.clone()) }
            } else {
                json! { "command" => text!(command) }
            };
            stream.emit(
                entry,
                "PreToolUse",
                &session,
                None,
                json! {
                    "tool_name" => text!(tool),
                    "tool_use_id" => text!(format!("{}:{}", entry.sha, change.path)),
                    "tool_input" => object(input.clone()),
                    "tool_input_sha256" => text!(sha256_hex(
                        serde_json::to_string(&object(input)).unwrap_or_default().as_bytes()
                    )),
                },
            );
            let mut result = json! {
                "tool_name" => text!(tool),
                "ok" => serde_json::Value::Bool(true),
                "is_error" => serde_json::Value::Bool(false),
                "exit_code" => number!(0i64),
                // Deterministic because the content is deterministic: a fixture that invented a duration would
                // be putting a number no analysis may read into a file whose digest is blessed.
                "duration_ms" => number!(u64::try_from(change.path.len()).unwrap_or(u64::MAX) * 13 + 41),
                "output_sha256" => text!(sha256_hex(format!("{tool} {}", change.path).as_bytes())),
            };
            if let Some(from) = &change.from {
                result.insert("from_path".to_owned(), text!(from.clone()));
            }
            stream.emit(entry, "PostToolUse", &session, None, result);

            // The edit itself, on the origin the contract maps to `file_edit`. The digests are computed from the
            // trees the commit was built from, which is the same information a real adapter would have: there is
            // no point asserting a `file_edit` whose after-digest does not match the blob the commit contains.
            let mut edit = json! {
                "path" => text!(format!("{}/{}", stream.root, change.path)),
                "change" => text!(edit_change(change.change)),
                "lines_added" => number!(u64::try_from(change.adds).unwrap_or(u64::MAX)),
                "lines_removed" => number!(u64::try_from(change.dels).unwrap_or(u64::MAX)),
                "content_sha256_before" => hash_of(content_of(
                    before,
                    change.from.as_deref().unwrap_or(&change.path),
                )),
                "content_sha256_after" => hash_of(content_of(after, &change.path)),
                "diff_available" => serde_json::Value::Bool(!change.binary),
            };
            if let Some(diff) = diff_handle(before, after, change) {
                edit.insert("diff_sha256".to_owned(), text!(diff));
            }
            stream.emit(
                entry,
                edit_source(change.change),
                &session,
                Some(&change.path),
                edit,
            );
        }

        // A commit can declare that its turn ended in failure rather than in a normal stop; see
        // [`HOOK_TRAILER`]. Emitting both would describe a turn that finished and did not.
        let failed = entry
            .trailers
            .get(HOOK_TRAILER)
            .is_some_and(|hook| projection(hook) == Some("error"));
        if last_of_turn.get(&pair) == Some(&position) && !failed {
            // The commands the turn ended with. `test_run` is derived from these by the ingest, not written
            // here, because a derivation invented by a fixture is a derivation nobody tested.
            for command in ["cargo test", "cargo clippy --all-targets"] {
                let input = json! { "command" => text!(command.to_owned()) };
                let payload = json! {
                    "tool_name" => text!("Bash"),
                    "tool_use_id" => text!(format!("{}:{}", entry.sha, command)),
                    "tool_input" => object(input.clone()),
                    "tool_input_sha256" => text!(sha256_hex(
                        serde_json::to_string(&object(input)).unwrap_or_default().as_bytes()
                    )),
                };
                stream.emit(entry, "PreToolUse", &session, None, payload.clone());
                let mut result = payload;
                result.insert("ok".to_owned(), serde_json::Value::Bool(true));
                result.insert("is_error".to_owned(), serde_json::Value::Bool(false));
                result.insert("exit_code".to_owned(), number!(0i64));
                result.insert(
                    "output_sha256".to_owned(),
                    text!(sha256_hex(format!("output of {command}").as_bytes())),
                );
                stream.emit(entry, "PostToolUse", &session, None, result);
            }
            stream.emit(
                entry,
                "Stop",
                &session,
                None,
                json! {
                    "stop_reason" => text!("end_turn"),
                    "last_assistant_message_sha256" => text!(sha256_hex(entry.subject.as_bytes())),
                },
            );
        }
        if last_of_session.get(&session) == Some(&position) {
            stream.emit(
                entry,
                "SessionEnd",
                &session,
                None,
                json! { "reason" => text!("shutdown") },
            );
        }
        // The trailer asked for a record with no Fathom meaning. It is written, counted and given a null
        // projection, which is what §2's "counted, not dropped" rule needs a corpus to contain.
        if let Some(hook) = entry.trailers.get(HOOK_TRAILER) {
            let mut payload = json! { "hook_event_name" => text!(hook.clone()) };
            if projection(hook) == Some("error") {
                // §2.1's `error` row: a kind, a digest of the detail, and whether retrying would help. A
                // finding that an episode was abandoned needs a record that says the run stopped on purpose
                // and would not have got further.
                payload.insert("kind".to_owned(), text!("budget_exhausted"));
                payload.insert(
                    "detail_sha256".to_owned(),
                    text!(sha256_hex(b"stopped after the third failed attempt")),
                );
                payload.insert("retryable".to_owned(), serde_json::Value::Bool(false));
            }
            stream.emit(entry, hook, &session, None, payload);
        }
    }

    let (count, digest) = stream.write_to(&dest.join(PATH))?;
    Ok(EventsSummary {
        path: PATH,
        written: true,
        count,
        truncated_at: match limit {
            usize::MAX => None,
            other => Some(other),
        },
        digest,
    })
}

/// A digest of file bytes, or null when there are none.
///
/// An add has no before-blob and a delete no after-blob, and both of those are facts the contract needs stated:
/// a reader that cannot tell "no before" from "empty before" cannot tell a new file from a cleared one.
/// The bytes a path held in a tree, or `None` when the path held no bytes to hash.
///
/// A gitlink is the case that matters: a submodule reference has a commit id and no content, and hashing the
/// empty blob an uninitialised submodule's mount point contains would claim the edit emptied the file.
fn content_of<'a>(tree: Option<&'a BTreeMap<String, File>>, path: &str) -> Option<&'a Vec<u8>> {
    tree?
        .get(path)
        .filter(|file| file.mode != Mode::Gitlink)
        .map(|file| &file.blob)
}

fn hash_of(bytes: Option<&Vec<u8>>) -> serde_json::Value {
    match bytes {
        Some(bytes) => text!(sha256_hex(bytes)),
        None => serde_json::Value::Null,
    }
}

/// A hash of the change's own bytes, used as the `diff_sha256` handle.
///
/// A real harness hashes a `git diff` of the hunk. Hashing the before and after bytes instead says the same
/// thing — this edit, identifiable, available to anything that can read the object store — and hashing a diff
/// of the fixture's own content would be hashing a rendering the fixture does not own. A binary change has no
/// textual diff at all, which is what `diff_available: false` is for, so nothing is claimed here that a reader
/// could not reproduce.
fn diff_handle(
    before: Option<&BTreeMap<String, File>>,
    after: Option<&BTreeMap<String, File>>,
    change: &crate::generate::Change,
) -> Option<String> {
    if change.binary {
        return None;
    }
    let from = content_of(before, change.from.as_deref().unwrap_or(&change.path))?;
    let to = content_of(after, &change.path)?;
    if from == to {
        // A metadata-only change has no textual diff to point at, and the record says so by omitting the field
        // rather than by pointing at the hash of nothing.
        return None;
    }
    let mut joined = from.clone();
    joined.extend_from_slice(b"\n---\n");
    joined.extend_from_slice(to);
    Some(sha256_hex(&joined))
}

/// The stream under construction: a sequence number, a root to normalise out, and the lines so far.
struct Stream {
    seq: usize,
    root: String,
    lines: Vec<String>,
}

impl Stream {
    fn new(root: &str) -> Stream {
        Stream {
            seq: 0,
            root: root.to_owned(),
            lines: Vec::new(),
        }
    }

    /// Append one record.
    ///
    /// The envelope is §3's, and only that: `seq`, `event_id`, `schema_version`, `harness`, `source`,
    /// `fathom_event`, `session_id`, `cwd`, `git_branch`, `ts`, and a `payload` that is the vendor's shape.
    ///
    /// The timestamp is derived from the sequence number rather than read from a clock, which is what keeps this
    /// file byte-identical across machines. §3.3 says ordering is by `seq` and never by `ts`, precisely because
    /// vendor clocks lie; a fixture is allowed to be tidier than a vendor, and a monotonically increasing `ts`
    /// also means the corpus exercises "timestamps in order" without ever calling a clock.
    fn emit(
        &mut self,
        entry: &CommitEntry,
        source: &str,
        session: &str,
        path: Option<&str>,
        payload: Map,
    ) {
        self.seq += 1;
        let harness =
            policy::identity(entry.author).map_or("unknown", |identity| identity.harness());
        let mut record: Map = Map::new();
        record.insert(
            "seq".to_owned(),
            number!(u64::try_from(self.seq).unwrap_or(u64::MAX)),
        );
        record.insert("event_id".to_owned(), text!(format!("e{}", self.seq)));
        record.insert("schema_version".to_owned(), number!(1u64));
        record.insert("harness".to_owned(), text!(harness));
        record.insert("source".to_owned(), text!(source));
        // A projection that names nothing in Fathom is `null`, not absent and not dropped (§2).
        record.insert(
            "fathom_event".to_owned(),
            projection(source).map_or(serde_json::Value::Null, |name| text!(name)),
        );
        record.insert("session_id".to_owned(), text!(session));
        record.insert("cwd".to_owned(), text!(self.root.clone()));
        record.insert("git_branch".to_owned(), text!(entry.branch.clone()));
        record.insert(
            "ts".to_owned(),
            text!(policy::iso_utc(
                policy::BASE_EPOCH + entry.at + i64::try_from(self.seq / 1_000).unwrap_or(i64::MAX),
                u32::try_from(self.seq % 1_000).unwrap_or(u32::MAX)
            )),
        );
        if let Some(path) = path {
            record.insert("path".to_owned(), text!(format!("{}/{}", self.root, path)));
        }
        record.insert("payload".to_owned(), object(payload));
        // `to_string` of a `BTreeMap` cannot fail for these values; if it ever did, a counted bad line is what
        // the contract asks for, not a panic in a fixture builder.
        self.lines.push(
            serde_json::to_string(&record)
                .unwrap_or_else(|source| format!("{{\"bad\": \"{source}\"}}")),
        );
    }

    fn write_to(self, path: &Path) -> Result<(usize, String)> {
        let body = if self.lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", self.lines.join("\n"))
        };
        std::fs::write(path, body.as_bytes()).map_err(|source| git::Error::Io {
            context: path.display().to_string(),
            source,
        })?;
        let normalized = body.replace(&self.root, "<ROOT>");
        Ok((self.seq, sha256_hex(normalized.as_bytes())))
    }
}

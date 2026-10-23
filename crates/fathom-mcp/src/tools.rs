//! The tool catalogue: what the server offers, what each tool costs, and who may see it
//! (ADR-005 §4.1, §4.2, §4.3).
//!
//! One table, three uses. `tools/list` renders it, the dispatcher enforces it (required keys, unknown keys,
//! bounds, capability) and the docs and the tests read the same numbers out of it, so "the surface is
//! bounded" is a fact about one place rather than a claim spread across prose, Rust and Python.
//!
//! A write tool that is switched off is **absent from the list**, not present-and-refusing (§4.3): a tool a
//! client can see and cannot call is a trap, and the honest surface is the callable one. The refusal is
//! still made when a client calls the name anyway — an agent that guessed the name is told the capability
//! it lacks — but it is not advertised.

use serde_json::{Value, json};

/// Which half of the surface a tool belongs to (ADR-005 §4.1, §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Answers a question. Always available.
    Read,
    /// Writes something durable. Off unless `mcp.allow_writes` says otherwise, and every call is audited.
    Write,
}

impl Capability {
    /// The wire name, which is also the capability string a client would negotiate.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// One tool: its name, its contract, its bounds and its argument grammar.
#[derive(Debug, Clone, Copy)]
pub struct ToolSpec {
    /// The wire name, as ADR-005 §4 spells it.
    pub name: &'static str,
    /// One-line title a client shows.
    pub title: &'static str,
    /// The description the agent reads before deciding to call this: what it answers, what it will not
    /// return, and what the bounds are.
    pub description: &'static str,
    /// Read or write.
    pub capability: Capability,
    /// Keys that must be present.
    pub required: &'static [&'static str],
    /// Keys the tool understands beyond those.
    pub optional: &'static [&'static str],
    /// How many rows a caller gets when it does not ask (0 for a tool that returns a document, not a
    /// collection).
    pub default_items: usize,
    /// The largest answer this build will produce for this tool.
    pub max_items: usize,
    /// The response's own token discipline, in the caller's words.
    pub bound_note: &'static str,
}

impl ToolSpec {
    /// The pagination arguments every list-taking tool accepts.
    pub const PAGINATION: &'static [&'static str] = &["limit", "cursor"];

    /// The input schema to publish: a closed object, typed per key, with the bounds printed in the
    /// description of the bounded keys rather than left to a 4 MiB surprise.
    #[must_use]
    pub fn input_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        for key in self.required.iter().chain(self.optional) {
            let (kind, note): (&str, &str) = match *key {
                "episode_id" | "change_id" | "target" => (
                    "string",
                    "An id this index knows; a prefix is accepted when it is unique.",
                ),
                "ref_a" | "ref_b" | "from" | "to" => (
                    "string",
                    "A revision or episode handle: `HEAD`, `main~3`, an oid prefix or an episode id.",
                ),
                "query" => (
                    "string",
                    "Free text; matched against the index, never against file bodies.",
                ),
                "reviewer" => (
                    "string",
                    "The human who looked, named by them, not guessed.",
                ),
                "kind" => (
                    "string",
                    "`note`, `question`, `decision` or `flag`. A decision that grants an allowance is `e3.10`'s, not this tool's.",
                ),
                "body" => ("string", "The note itself, at most 4096 UTF-8 bytes."),
                "note" => (
                    "string",
                    "Why, in the reviewer's own words; at most 4096 UTF-8 bytes.",
                ),
                "path_filter" => (
                    "string",
                    "A path or directory prefix to confine the answer to.",
                ),
                "package" => (
                    "string",
                    "One package to answer for; absent means every package.",
                ),
                "min_tier" => (
                    "string",
                    "Lowest tier to report: `low`, `medium`, `high` or `critical`.",
                ),
                "author" => ("string", "Restrict to one author, as the index spells it."),
                "since" => (
                    "string",
                    "An ISO-8601 date or datetime; anything older is not returned.",
                ),
                "k" | "limit" => ("integer", "Rows to return, clamped to this tool's bound."),
                "include_events" => (
                    "boolean",
                    "Also list the member commits: identity and time, never message bodies.",
                ),
                "offset" => ("integer", "Ignored when `cursor` is present."),
                "cursor" => (
                    "string",
                    "The `next_cursor` from the previous page; pass it back to continue.",
                ),
                other => (other, "undocumented"),
            };
            properties.insert(
                (*key).to_owned(),
                json!({ "type": kind, "description": note }),
            );
        }
        let mut pagination: Vec<&str> = Vec::new();
        if self.default_items > 0 {
            pagination.push("limit");
            pagination.push("cursor");
            pagination.push("offset");
        }
        for key in pagination {
            if !properties.contains_key(key) {
                properties.insert(
                    key.to_owned(),
                    json!({ "type": if key == "cursor" { "string" } else { "integer" } }),
                );
            }
        }
        json!({
            "type": "object",
            "properties": properties,
            "required": self.required,
            "additionalProperties": false,
            "x-fathom-bound": {
                "capability": self.capability.as_str(),
                "default_items": self.default_items,
                "max_items": self.max_items,
                "note": self.bound_note,
            },
        })
    }
}

/// Every read tool, in the order ADR-005 §4.1 lists them.
///
/// `search` (the snippet-returning sibling in the same table) is **not** here, and that is a decision rather
/// than an omission: §4.3 lets a snippet through only to a session holding a `snippets` capability, and
/// this build has no capability negotiation that could grant one. A tool that would answer `no_capability`
/// to every caller is not a tool, it is a decoy; `search_history` answers the same question with pointers.
pub const READ_TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "explain_episode",
        title: "Explain one episode",
        description: "One episode as one versioned document: identity, rows, totals, the seven analysis \
                      sections with their evidence ids, the risk ranking and every gap that has an owner. \
                      Paths and counts only — no file bodies. Rows are paginated.",
        capability: Capability::Read,
        required: &["episode_id"],
        optional: &["include_events"],
        default_items: 50,
        max_items: 200,
        bound_note: "file rows are paginated, 50 by default and 200 at most",
    },
    ToolSpec {
        name: "changes_between",
        title: "What changed between two revisions",
        description: "The change rows between two endpoints: kind, risk tier, paths and evidence ids, \
                      capped at 200 rows with an explicit cursor past the cap.",
        capability: Capability::Read,
        required: &["ref_a", "ref_b"],
        optional: &["path_filter"],
        default_items: 100,
        max_items: 200,
        bound_note: "changes are capped at 200 with a cursor",
    },
    ToolSpec {
        name: "architecture_delta",
        title: "What a range did to the architecture",
        description: "Packages and symbols added, removed, moved or re-pointed, the cycles a range added, \
                      the boundary crossings and the layer reassignments — with the cost that was paid to \
                      answer, and the rung the answer came from.",
        capability: Capability::Read,
        required: &[],
        optional: &["from", "to"],
        default_items: 50,
        max_items: 200,
        bound_note: "findings are capped at 200 and cycles at 50, each with a cursor",
    },
    ToolSpec {
        name: "dependency_delta",
        title: "What a range did to the dependencies",
        description: "Declared and locked dependencies added, removed or re-pinned, the production split, \
                      and anything removed from the manifest while still imported — 200 entries at most.",
        capability: Capability::Read,
        required: &[],
        optional: &["from", "to", "package"],
        default_items: 50,
        max_items: 200,
        bound_note: "entries are capped at 200 with a cursor",
    },
    ToolSpec {
        name: "risk_report",
        title: "The ranked risk of a range",
        description: "The changes in a range, ranked, each with its factors, its confidence and the model \
                      identity that produced it — including whether there is a trained model at all. 100 \
                      rows at most.",
        capability: Capability::Read,
        required: &[],
        optional: &["ref_a", "ref_b", "min_tier"],
        default_items: 50,
        max_items: 100,
        bound_note: "ranked rows are capped at 100 with a cursor",
    },
    ToolSpec {
        name: "search_history",
        title: "Search the indexed history",
        description: "Pointer-shaped hits over the episodes this index holds: episode id, timestamp, title, \
                      score and which fields matched. Content is never returned, not as a snippet, not in \
                      any field.",
        capability: Capability::Read,
        required: &["query"],
        optional: &["k", "since", "author"],
        default_items: 8,
        max_items: 32,
        bound_note: "`k` defaults to 8 and is refused above 32",
    },
    ToolSpec {
        name: "review_memory",
        title: "What a human recorded about a change",
        description: "The durable answers a review left behind, read back from the committed markdown under \
                      `.fathom/reviews/`: the recorded decisions with their reasons, the questions and \
                      follow-ups still open, and the annotations — each with its author, the instant it was \
                      written, how it is attached to the change now (including after a rebase), and the \
                      evidence it cites: episode id and digests, member commits, the path and line it \
                      pointed at, and the file and block to quote. This is the tool for \"why did we do \
                      X\"; it returns what was written down, never an inference about why.",
        capability: Capability::Read,
        required: &["target"],
        optional: &["kind", "status", "include_answered"],
        default_items: 20,
        max_items: 100,
        bound_note: "notes are capped at 100 with a cursor; a body prints as written, up to 8 KiB",
    },
];

/// The two gated write tools (ADR-005 §4.2).
pub const WRITE_TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "mark_reviewed",
        title: "Record that a human reviewed a change",
        description: "Write a review-ledger entry saying a named human accepted a change. Needs the `write` \
                      capability and an approval token bound to the change, and is idempotent per \
                      (change, reviewer). This is a claim about a person, so it is refused rather than \
                      guessed at.",
        capability: Capability::Write,
        required: &["change_id", "reviewer"],
        optional: &["note", "approval_token"],
        default_items: 0,
        max_items: 1,
        bound_note: "one ledger entry per (change_id, reviewer); a repeat reports `already: true`",
    },
    ToolSpec {
        name: "add_review_note",
        title: "Attach a note to an episode, a change or a path",
        description: "Record a note, question, decision or follow-up against a target this index knows, \
                      with the author and the time it was written, by appending it to the committed \
                      review memory at `.fathom/reviews/notes.md` — the same file a person writes \
                      through `fathom note add`, so an agent's note and a human's are one record that \
                      travels in the same PR. The body is bounded at 4096 bytes, is redacted on the way \
                      in, and the target must resolve. `reviewer` names the human the note is \
                      attributed to; without it the note is the agent's own claim. Answering a \
                      recorded question is `about: nt-…`, never an edit.",
        capability: Capability::Write,
        required: &["target", "body"],
        optional: &["kind", "about", "reviewer"],
        default_items: 0,
        max_items: 1,
        bound_note: "body is capped at 4096 UTF-8 bytes; one block appended per call, a repeat is skipped",
    },
];

/// The body bound §4.2 puts on a note, in bytes.
pub const MAX_BODY_BYTES: usize = 4096;

/// The absolute ceiling on any page, whatever a tool's own bound says.
pub const PAGE_LIMIT_MAX: usize = 500;

/// Every tool, read tools first.
pub const ALL: &[ToolSpec] = &[
    // Concatenation at compile time is not worth a macro for nine entries.
    READ_TOOLS[0],
    READ_TOOLS[1],
    READ_TOOLS[2],
    READ_TOOLS[3],
    READ_TOOLS[4],
    READ_TOOLS[5],
    READ_TOOLS[6],
    WRITE_TOOLS[0],
    WRITE_TOOLS[1],
];

/// Look a tool up by name.
#[must_use]
pub fn find(name: &str) -> Option<&'static ToolSpec> {
    ALL.iter().find(|spec| spec.name == name)
}

/// The tools a session may see: everything, minus the write tools when writes are off (§4.3).
#[must_use]
pub fn visible(writes_enabled: bool) -> Vec<&'static ToolSpec> {
    ALL.iter()
        .filter(|spec| writes_enabled || spec.capability == Capability::Read)
        .collect()
}

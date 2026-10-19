//! The JSON Schema for [`EpisodeDelta`], generated from this crate and published as
//! `insight::schema_document()` — the same module-privacy choice as the model itself: the crate's
//! public surface is `fathom_analyze::insight` and nothing deeper (`W4`).
//!
//! Why a generator and not a hand-written file: `ADR-002 §6.1`'s argument about `fathom.schema.json`, applied
//! one directory over. A hand-edited schema is worse than none, because the editor becomes the confident
//! party. So `schemas/episode-delta.schema.json` is the *output* of [`to_text`], a test diffs the checked-in
//! file against it (`crates/fathom-analyze/tests/delta_schema.rs`), and the only way to change the schema is
//! to change the types or this document and re-run the generator.
//!
//! # The compatibility policy, in the file it governs
//!
//! The document carries an `x-fathom` block: the contract id, the version, and the rule.
//!
//! * **Additive changes keep the version.** A new optional field, a new figure, a new section appended after
//!   the seven, a new key in `provenance.models`. Readers ignore what they do not know — no struct here uses
//!   `deny_unknown_fields`, and that is a published promise rather than an accident, tested in
//!   `tests/delta_schema.rs` by feeding a document a future version's fields.
//!   This is also why every object in the generated schema says `additionalProperties: true`: a consumer
//!   validating tomorrow's v1 document against today's v1 schema must accept it, or "additive" would be a
//!   word with no meaning. Closedness is enforced by [`EpisodeDelta::verify`], which rejects a *model* whose
//!   shape is wrong, and by the compat checker, which refuses a change that is additive in name only.
//! * **Breaking changes bump the version.** Removing or renaming a field, narrowing a type, narrowing or
//!   reordering an enum, changing what a required key means. `tools/check_delta_compat.py` diffs the
//!   committed schema against the archived baseline for its declared version and fails CI when the two
//!   differ in a breaking way at the same version, and when a new version was declared without archiving the
//!   schema it belongs to.
//! * **The version is a number in the document, not a rumour.** Every payload repeats `schema_version`, so a
//!   consumer can refuse what it cannot read before it reads it.
//!
//! The keyword subset is the one the checked-in schemas use — the same subset the round-trip validator in
//! `crates/fathom-cli/tests/schemas.rs` understands, on purpose: a schema using a keyword no validator here
//! implements is a schema whose tests pass for the wrong reason.

#![deny(missing_docs)]

use serde_json::{Value, json};

use super::delta::{CONTRACT, FIGURE_CAP, MEMBER_CAP, ROLLUP_CAP, STATE_ALL, SectionId, VERSION};
use crate::risk::Surface;

/// The `$id` the schema publishes, and the path it is checked in at.
pub const SCHEMA_FILE: &str = "schemas/episode-delta.schema.json";

/// The schema document.
///
/// Every map here is a `serde_json::Map`, which is a `BTreeMap` unless `preserve_order` is compiled in
/// (it is not, ADR-002 §6.1), so the emitted bytes are alphabetical by key and the diff is a real diff.
#[must_use]
pub fn document() -> Value {
    let defs = json!({
        "evidence": {
            "additionalProperties": true,
            "description": "Where a figure's proof lives: a JSON pointer inside this document, a command the reviewer can run, or the plan key that owns the missing answer. At least one is always set — `verify` fails a figure that carries none.",
            "properties": {
                "owner": {"description": "The plan key that owns the answer, e.g. `e3.2`.", "type": "string"},
                "pointer": {"description": "A JSON pointer into this document, e.g. `#/files`.", "type": "string"},
                "run": {"description": "A command this binary answers to, e.g. `fathom show HEAD~3`.", "type": "string"}
            },
            "type": "object"
        },
        "figure": {
            "additionalProperties": true,
            "description": "One printed claim, with the integers behind it and the pointer to its evidence. `value` is what a surface prints verbatim; `raw` is what a machine compares. A figure that was not measured is `known: false` and carries the reason it was not — never a zero.",
            "properties": {
                "confidence": {
                    "description": "0..=1, present only when the figure is an estimate.",
                    "maximum": 1.0,
                    "minimum": 0.0,
                    "type": "number"
                },
                "evidence": {"$ref": "#/$defs/evidence"},
                "known": {"description": "Whether anything was measured. `false` prints as `?`.", "type": "boolean"},
                "label": {"description": "The short label printed before the value.", "type": "string"},
                "raw": {
                    "additionalProperties": {"type": "integer"},
                    "description": "The integers behind `value`, sorted by key.",
                    "type": "object"
                },
                "reason": {"description": "Why it is unknown. Required in substance when `known` is false.", "type": "string"},
                "value": {"description": "The value as the product words it. Empty when `known` is false.", "type": "string"}
            },
            "required": ["evidence", "known", "label", "value"],
            "type": "object"
        },
        "gap": {
            "additionalProperties": true,
            "description": "Why a section is not a full answer, who owns the rest, and what to run meanwhile.",
            "properties": {
                "owner": {"description": "The plan key that owns the answer.", "pattern": "^([esdm][0-9]+(\\.[0-9]+)?|reco-[a-z0-9]{3})$", "type": "string"},
                "short": {"description": "The one-line version, for the brief: a card row has room for this and not for `reason`.", "maxLength": 60, "minLength": 1, "type": "string"},
                "reason": {"description": "What is missing, stated as a fact about this build.", "type": "string"},
                "remedy": {"description": "A command the reviewer can run instead, when one exists.", "type": "string"},
                "section": {"description": "The section this gap belongs to.", "type": "string"}
            },
            "required": ["owner", "reason", "section", "short"],
            "type": "object"
        },
        "sectionState": {
            "description": "The three claims a reviewer acts on differently.",
            "enum": STATE_ALL.iter().map(|(name, _)| *name).collect::<Vec<&str>>(),
            "type": "string"
        },
        "section": {
            "additionalProperties": true,
            "description": "One of the seven questions. All seven are always present; a section that could not answer is `unavailable` with a gap, never missing.",
            "properties": {
                "figures": {
                    "description": "The figures, capped and ordered; the remainder is counted, not cut.",
                    "items": {"$ref": "#/$defs/figure"},
                    "maxItems": FIGURE_CAP,
                    "type": "array"
                },
                "gap": {"$ref": "#/$defs/gap"},
                "id": {
                    "description": "The section id. The seven below are the contract; a reader must ignore one it does not know rather than fail on it, which is what makes an appended section an additive change.",
                    "type": "string"
                },
                "owner": {"description": "The plan key whose analysis owns a full answer.", "type": "string"},
                "question": {"description": "The question this section answers.", "type": "string"},
                "remainder": {
                    "description": "How many figures the cap dropped — counted, never silently cut.",
                    "minimum": 0,
                    "type": "integer"
                },
                "state": {"$ref": "#/$defs/sectionState"},
                "title": {"description": "The row label.", "type": "string"}
            },
            "required": ["figures", "id", "owner", "question", "state", "title"],
            "type": "object"
        },
        "file": {
            "additionalProperties": true,
            "description": "One path the episode touched, with the taxonomy decisions it was classified by. Most figure pointers land here, so the rows carry their own provenance.",
            "properties": {
                "add": {"description": "Lines added.", "type": "integer"},
                "basis": {
                    "description": "`full` (per-commit rows) or `rollup` (the summed per-path read a rail page uses).",
                    "enum": ["full", "rollup"],
                    "type": "string"
                },
                "binary": {"description": "Whether the change was binary.", "type": "boolean"},
                "change": {
                    "description": "What git asserted about the path. `unrecorded` means the read did not look, not that nothing happened.",
                    "enum": ["added", "modified", "deleted", "renamed", "mode-only", "unrecorded"],
                    "type": "string"
                },
                "commits": {"description": "Member commits that touched this path.", "type": "integer"},
                "del": {"description": "Lines removed.", "type": "integer"},
                "noise_class": {"description": "`e0.9`'s noise class as stored, or `unknown`.", "type": "string"},
                "old_path": {"description": "The path it came from, when the index asserted a move.", "type": "string"},
                "path": {"description": "Repo-relative, slash-separated.", "type": "string"},
                "path_class": {"description": "`e0.9`'s path class as stored, or `unknown`.", "type": "string"},
                "surface": {
                    "description": "The reviewer-facing surface, from the one classifier in `fathom_analyze::risk`.",
                    "enum": surface_names(),
                    "type": "string"
                }
            },
            "required": ["add", "basis", "binary", "change", "commits", "del", "noise_class", "path", "path_class", "surface"],
            "type": "object"
        },
        "episode": {
            "additionalProperties": true,
            "description": "The episode's identity, carried from the spine so a payload can be relinked after a rewrite without asking the store again.",
            "properties": {
                "attribution": {
                    "additionalProperties": true,
                    "description": "The attribution behind `author_kind`, with the evidence that produced it (`e4.1`). Absent on a row written before migration 4, which stored the label and nothing else.",
                    "properties": {
                        "agent": {"description": "The agent the evidence named, when it named exactly one.", "type": "string"},
                        "confidence": {"description": "Confidence in `kind`; 0.0 means nothing was known, not that nothing matters.", "maximum": 1.0, "minimum": 0.0, "type": "number"},
                        "evidence": {"description": "Every rung that fired, strongest first.", "items": {"additionalProperties": true, "properties": {"confidence": {"type": "number"}, "detail": {"description": "The literal, in the commit's own spelling.", "type": "string"}, "source": {"type": "string"}}, "required": ["confidence", "detail", "source"], "type": "object"}, "type": "array"},
                        "kind": {"description": "`agent`, `human` or `unknown` — the same vocabulary as `author_kind`.", "type": "string"},
                        "reason": {"description": "Why the verdict is `unknown`, or why a label was refused rather than guessed.", "type": "string"},
                        "source": {"description": "The rung that decided: `correction`, `harness`, `trailer`, `declared`, `author`, `committer`, `message`, `identity`, `absent`.", "type": "string"}
                    },
                    "required": ["confidence", "evidence", "kind", "source"],
                    "type": "object"
                },
                "intent": {
                    "additionalProperties": true,
                    "description": "What the episode was for, with the evidence for that (`e4.2`, `reco-c54`): the text, the harness session and turn it arrived on, the plan items, the declared scope globs and the rung that decided. Absent on a row written before migration 5, which stored no intent at all — a different fact from `state: none`, which means intent was looked for through every source and nothing stated it.",
                    "properties": {
                        "confidence": {"description": "Confidence in `text`; 0.0 means nothing was recorded, not that nothing matters.", "maximum": 1.0, "minimum": 0.0, "type": "number"},
                        "evidence": {"description": "Every rung that fired, strongest first, each in the source's own spelling.", "items": {"additionalProperties": true, "properties": {"confidence": {"type": "number"}, "detail": {"type": "string"}, "source": {"type": "string"}}, "required": ["confidence", "detail", "source"], "type": "object"}, "type": "array"},
                        "plan": {"description": "The plan items, in the order they were stated, empty when nothing stated a plan.", "items": {"additionalProperties": true, "properties": {"status": {"description": "The item's state in the harness's own words, when it had one.", "type": "string"}, "text": {"type": "string"}}, "required": ["text"], "type": "object"}, "type": "array"},
                        "prompt": {"additionalProperties": true, "description": "The turn's prompt digests, and whether its text came with them. A digest with no text is `state: digest-only`, not an absence.", "properties": {"chars": {"minimum": 0, "type": "integer"}, "sha256": {"type": "string"}, "text_captured": {"type": "boolean"}}, "required": ["text_captured"], "type": "object"},
                        "reason": {"description": "What was looked for and not found, when there is no text. Required reading for `state: none`.", "type": "string"},
                        "schema": {"description": "The document's own namespace, so a consumer of the store column does not have to guess which shape it is reading.", "type": "string"},
                        "scope": {"description": "The declared scope, as path globs relative to the repository root, normalised, sorted and deduplicated. `e4.3`'s scope-creep detector measures the change against these (`fathom_analyze::scope::deviation` is that input); nothing in this build enforces them.", "items": {"type": "string"}, "type": "array"},
                        "scope_rejected": {"description": "Declared scope values that were refused rather than quietly dropped — an absolute path, a `..`, a NUL.", "items": {"type": "string"}, "type": "array"},
                        "session_id": {"description": "The harness session the intent arrived on, when an event or an `Agent-Session:` trailer named one.", "type": "string"},
                        "source": {"description": "The rung that decided: `hook-prompt`, `hook-intent`, `agent-intent-trailer`, `plan-item`, `task-trailer`, `prompt-digest`, `none`.", "type": "string"},
                        "state": {"description": "`recorded`, `digest-only` or `none` — the last is a state, never a blank.", "type": "string"},
                        "text": {"description": "The request, in the asker's own words. Absent unless `state` is `recorded`.", "type": "string"},
                        "truncated": {"description": "Whether `text` was cut at the stored byte cap; a quote that stops mid-sentence says so.", "type": "boolean"},
                        "turn_id": {"description": "The harness turn, ditto (ADR-005 §3.3's session-qualified id).", "type": "string"}
                    },
                    "required": ["confidence", "plan", "scope", "schema", "source", "state"],
                    "type": "object"
                },
                "author_confidence": {"maximum": 1.0, "minimum": 0.0, "type": "number"},
                "author_email": {"type": "string"},
                "author_kind": {"description": "`agent`, `human` or `unknown` — `unknown` is a value, never a guess.", "type": "string"},
                "author_name": {"type": "string"},
                "boundary_confidence": {"maximum": 1.0, "minimum": 0.0, "type": "number"},
                "boundary_rule": {"description": "Which segmentation rule opened this episode (`ADR-003 §4.7`).", "type": "string"},
                "fallback_fingerprint": {"description": "Relink tier 1 (author, subject).", "type": "string"},
                "first_utc": {"description": "First member timestamp in milliseconds since the epoch — the store unit.", "type": "integer"},
                "fingerprint": {"description": "The strong digest (author, subject, path set).", "type": "string"},
                "id": {"description": "ADR-003 §2.1's derived id — what a review is keyed to.", "type": "string"},
                "last_utc": {"description": "Last member timestamp in milliseconds since the epoch.", "type": "integer"},
                "lineage_ref": {"description": "The lineage the episode sits on.", "type": "string"},
                "lineage_seq": {"description": "Its position in that lineage.", "type": "integer"},
                "live": {"description": "Whether a current commit still belongs to it.", "type": "boolean"},
                "member_remainder": {"minimum": 0, "type": "integer"},
                "members": {"description": "Member commit SHAs, capped; `n_commits` is the truth.", "items": {"type": "string"}, "type": "array"},
                "n_commits": {"minimum": 0, "type": "integer"},
                "review": {"description": "The stored review marker.", "enum": ["unread", "read", "accepted", "flagged", "dismissed", "snoozed"], "type": "string"},
                "session": {"description": "The harness session, when one was recorded (`ADR-005`).", "type": "string"},
                "shape_fingerprint": {"description": "Relink tier 2 (author, path set).", "type": "string"},
                "summary": {"description": "The summary line.", "type": "string"},
                "turn": {"description": "The harness turn, when one was recorded.", "type": "string"}
            },
            "required": ["author_email", "author_kind", "author_name", "boundary_rule", "fallback_fingerprint", "first_utc", "fingerprint", "id", "last_utc", "lineage_ref", "lineage_seq", "live", "members", "n_commits", "review", "shape_fingerprint", "summary"],
            "type": "object"
        },
        "totals": {
            "additionalProperties": true,
            "description": "The episode's scale, and what of it a review reads. The derived four are arithmetic, and `verify` re-does it.",
            "properties": {
                "churn": {"description": "`n_add + n_del`.", "type": "integer"},
                "discounted_churn": {"description": "Churn discounted as noise: three quarters of the churn on generated, vendored, lockfile, binary and dependency paths.", "type": "integer"},
                "files_read": {"description": "Grouped file rows the read returned.", "type": "integer"},
                "files_truncated": {"description": "Whether the read hit its cap.", "type": "boolean"},
                "n_add": {"type": "integer"},
                "n_binary": {"type": "integer"},
                "n_del": {"type": "integer"},
                "n_files": {"type": "integer"},
                "reconciles": {"description": "Whether the rows seen sum to the episode counts; absent when the cap bound and the question cannot be answered.", "type": "boolean"},
                "reviewable_churn": {"description": "`churn − discounted_churn`.", "type": "integer"}
            },
            "required": ["churn", "discounted_churn", "files_read", "files_truncated", "n_add", "n_binary", "n_del", "n_files", "reviewable_churn"],
            "type": "object"
        },
        "risk": {
            "additionalProperties": true,
            "description": "The ranking, with its arithmetic and its blind spots. Absent means unranked, which is not rank 0.",
            "properties": {
                "blind_spots": {"items": {"type": "string"}, "type": "array"},
                "caveat": {"type": "string"},
                "components": {
                    "items": {
                        "additionalProperties": true,
                        "properties": {
                            "cap": {"type": "integer"},
                            "detail": {"type": "string"},
                            "name": {"type": "string"},
                            "points": {"type": "integer"}
                        },
                        "required": ["cap", "detail", "name", "points"],
                        "type": "object"
                    },
                    "type": "array"
                },
                "model": {"description": "Which ranking produced the number (`spine-1` today).", "type": "string"},
                "rank": {"minimum": 1, "type": "integer"},
                "ranked": {"minimum": 0, "type": "integer"},
                "score": {"maximum": 100, "minimum": 0, "type": "integer"},
                "surfaces": {"items": {"enum": surface_names(), "type": "string"}, "type": "array"},
                "vector": {"description": "`spine-1 88 = surfaces 50 + size 12 + …` — the arithmetic, checkable from the row.", "type": "string"}
            },
            "required": ["blind_spots", "components", "model", "score", "surfaces", "vector"],
            "type": "object"
        },
        "provenance": {
            "additionalProperties": true,
            "description": "Where the rows came from, so a figure can be traced to a read.",
            "properties": {
                "asked": {"description": "The handle the user typed.", "type": "string"},
                "epoch": {"description": "The index epoch the rows came from.", "type": "integer"},
                "files_detail": {
                    "description": "`full` when the per-commit file rows were read, `rollup` when the summed per-path read was used.",
                    "enum": ["full", "rollup"],
                    "type": "string"
                },
                "models": {
                    "additionalProperties": {"type": "string"},
                    "description": "Which model produced which number.",
                    "type": "object"
                },
                "produced_by": {"description": "`fathom <version>`.", "type": "string"},
                "source": {"description": "`store`, `fixture` or `synthetic`.", "type": "string"},
                "store": {"description": "The redacted store path.", "type": "string"}
            },
            "required": ["files_detail", "models", "produced_by", "source"],
            "type": "object"
        },
        "integrity": {
            "additionalProperties": true,
            "description": "The document's own verdict on itself, from `EpisodeDelta::verify`.",
            "properties": {
                "checks": {"description": "What was checked, so `verified` has a definition.", "items": {"type": "string"}, "type": "array"},
                "problems": {"description": "One line per problem found; empty when `verified`.", "items": {"type": "string"}, "type": "array"},
                "verified": {"type": "boolean"}
            },
            "required": ["checks", "problems", "verified"],
            "type": "object"
        }
    });

    json!({
        "$comment": "Generated by `fathom_analyze::insight::schema_document()`. Do not hand-edit: change the types or the generator and re-run `fathom analyze --schema --write`. Owner: `e2.11` (`reco-tt4`).",
        "$id": "https://fathom.dev/schemas/episode-delta.schema.json",
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": defs,
        "additionalProperties": true,
        "description": "One episode as one versioned document: identity, rows, derived totals, the seven analysis sections with their evidence, the ranking, the gaps and a self-check. The TUI card, `fathom analyze --json`, the review artefact (`e5.7`) and the MCP tools (`e6.4`) are renderings of this and compute nothing of their own.",
        "properties": {
            "contract": {"const": CONTRACT},
            "episode": {"$ref": "#/$defs/episode"},
            "files": {"description": "The rows the figures are derived from, in path order.", "items": {"$ref": "#/$defs/file"}, "type": "array"},
            "gaps": {"description": "Every gap in one list, section order — the same objects the sections carry.", "items": {"$ref": "#/$defs/gap"}, "type": "array"},
            "integrity": {"$ref": "#/$defs/integrity"},
            "provenance": {"$ref": "#/$defs/provenance"},
            "risk": {"$ref": "#/$defs/risk"},
            "schema_version": {"const": VERSION},
            "sections": {
                "description": "The seven sections, in contract order. `minItems` is seven because a missing section is a missing question; a newer producer may append one, which is why there is no `maxItems`.",
                "items": {"$ref": "#/$defs/section"},
                "minItems": 7,
                "type": "array"
            },
            "totals": {"$ref": "#/$defs/totals"}
        },
        "required": ["contract", "episode", "files", "gaps", "integrity", "provenance", "schema_version", "sections", "totals"],
        "title": "Fathom episode delta",
        "type": "object",
        "x-fathom": {
            "capability": "fathom analyze <episode> --json",
            "compatibility": "additive changes keep the version; breaking changes bump it",
            "consumer": ["fathom-tui", "fathom-cli", "export (e5.7)", "mcp (e6.4)"],
            "contract": CONTRACT,
            "owner": "e2.11",
            "reader_ignores_unknown_fields": true,
            "version": VERSION
        }
    })
}

/// The schema as the checked-in file's bytes: two-space pretty-printing and a trailing newline.
#[must_use]
pub fn to_text() -> String {
    let mut text = serde_json::to_string_pretty(&document()).unwrap_or_else(|_| String::from("{}"));
    text.push('\n');
    text
}

/// The section table, as the docs and `fathom analyze --sections` print it.
///
/// Generated from [`SectionId`] so the documents cannot carry a fourth hand-copied copy of the list
/// (`ADR-006 §0`'s argument, applied to the delta).
#[must_use]
pub fn describe() -> String {
    let mut out = vec![
        "| section | question | owner | this build |".to_owned(),
        "| --- | --- | --- | --- |".to_owned(),
    ];
    for id in SectionId::ALL {
        out.push(format!(
            "| `{}` — {} | {} | {} | {} |",
            id.as_str(),
            id.title(),
            id.question(),
            id.owner(),
            id.default_state()
        ));
    }
    out.push(String::new());
    out.join("\n")
}

/// The surface vocabulary, most consequential first — the order `crate::risk` weights them in.
///
/// Listed here rather than derived because `Surface` has no `ALL`: a closed enum whose members are spelled
/// out in a schema is a closed enum the schema has to keep honest, and dropping one from this list narrows
/// the schema, which `tools/check_delta_compat.py` calls breaking.
const SURFACES: &[Surface] = &[
    Surface::Migration,
    Surface::Ci,
    Surface::Infra,
    Surface::Container,
    Surface::Dependency,
    Surface::Schema,
    Surface::Env,
    Surface::Source,
    Surface::Test,
    Surface::Docs,
    Surface::Generated,
    Surface::Vendored,
    Surface::Asset,
    Surface::Binary,
    Surface::Other,
];

fn surface_names() -> Vec<&'static str> {
    SURFACES.iter().map(|surface| surface.as_str()).collect()
}

/// The caps this build publishes, so the docs quote the numbers rather than remembering them.
#[must_use]
pub fn limits() -> [(&'static str, usize); 3] {
    [
        ("figure cap per section", FIGURE_CAP),
        ("rollup rows per figure", ROLLUP_CAP),
        ("member commits listed", MEMBER_CAP),
    ]
}

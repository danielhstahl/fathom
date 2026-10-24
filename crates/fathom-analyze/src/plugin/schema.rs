//! The JSON Schema for a plugin [`Finding`], published as `schemas/plugin-finding.schema.json`
//! (`e6.7`, `reco-raw`).
//!
//! Written the way `schemas/episode-delta.schema.json` is written — generated from this crate, diffed by
//! a test, blessed by changing the types and re-running — for the reason that file's header gives: a
//! hand-edited schema is a second source of truth with a nicer name, and the party who edited it becomes
//! the confident one.
//!
//! The version rule is the delta's rule, stated once more because this is a *different* contract with its
//! own consumers (an out-of-tree plugin, and anything reading a plugin's stdout):
//!
//! * **Additive keeps [`super::finding::SCHEMA_VERSION`].** A new optional field, a new key inside
//!   `evidence`, a new severity word added by a future ADR amendment. `additionalProperties` is `true` on
//!   every object here for exactly that reason, and [`super::finding::gate`] — not the schema — is what
//!   decides whether a value is *usable*.
//! * **Breaking bumps it.** A removed field, a rename, a narrowed type, a new required key, a reordered
//!   severity vocabulary. A finding that declares a version this build cannot read is ignored with a
//!   notice (rule R1), which is the one behaviour the version number actually buys.

use serde_json::{Value, json};

use super::finding::{MAX_SEGMENT, SCHEMA_VERSION};
use crate::detectors::view;
use crate::insight;

/// The `$id` the schema publishes, and the path it is checked in at.
pub const SCHEMA_FILE: &str = super::finding::SCHEMA_FILE;

/// The contract id a plugin finding carries in its `x-fathom` block.
pub const CONTRACT: &str = "fathom-plugin-finding";

/// The plan key that owns this contract.
pub const OWNER: &str = "e6.7";

/// The severity vocabulary, repeated from [`crate::detectors::Severity`] so the schema cannot disagree
/// with the type it describes.
const SEVERITIES: [&str; 3] = ["high", "medium", "low"];

/// The evidence grades, from [`crate::detectors::Grade`].
const GRADES: [&str; 4] = ["observed", "inferred", "asserted", "absent"];

/// The evidence kinds, from [`crate::detectors::EvidenceKind`].
const EVIDENCE_KINDS: [&str; 6] = ["range", "hunk", "test", "config_key", "path", "episode"];

/// The finding-type pattern: `namespace.id`, both segments `[a-z][a-z0-9_]{0,MAX_SEGMENT-1}`.
fn type_pattern() -> String {
    format!(
        "^[a-z][a-z0-9_]{{0,{max}}}\\.[a-z][a-z0-9_]{{0,{max}}}$",
        max = MAX_SEGMENT - 1
    )
}

/// The schema document.
#[must_use]
pub fn document() -> Value {
    let defs = json!({
        "evidence": {
            "additionalProperties": true,
            "description": "Why the plugin thinks so — the same shape the built-in catalogue emits, so a surface renders one evidence type and not two. `grade` is ADR-004's: `observed` quotes the lines, `inferred` derived the claim, `asserted` is the repository's own statement, `absent` is a stated absence.",
            "properties": {
                "detail": {"description": "What is there, quoted or stated, bounded by the detector.", "type": "string"},
                "grade": {"description": "How the detector knows.", "enum": GRADES, "type": "string"},
                "kind": {"description": "What kind of locator `locator` is.", "enum": EVIDENCE_KINDS, "type": "string"},
                "locator": {"description": "A resolvable pointer: `src/parser.rs:240-262`, a path, an episode id.", "type": "string"}
            },
            "required": ["kind", "locator", "detail", "grade"],
            "type": "object"
        },
        "jump": {
            "additionalProperties": true,
            "description": "Where the surface opens when the reviewer accepts the finding. A view this build has no such name for is replaced with the episode and said out loud (rule R7) rather than guessed at.",
            "properties": {
                "target": {"description": "What to open in it: `<episode>` or `<episode>#<path-or-region>`.", "type": "string"},
                "view": {"description": "One of ADR-006 §4's registry ids.", "enum": view::ALL, "type": "string"}
            },
            "required": ["view", "target"],
            "type": "object"
        }
    });

    json!({
        "$id": "https://fathom.dev/schemas/plugin-finding.schema.json",
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "additionalProperties": true,
        "description": "One finding raised by a detector written outside this repository (`e6.7`). The four parts every finding in this product carries — a sentence, a jump, a confidence, one key that mutes it — plus the version and the namespace that make a third-party claim safe to print beside a built-in's. Unknown fields are ignored on read; a finding whose `schema_version` this build cannot read, whose `finding_type` its plugin did not declare, or whose message or subject is empty is ignored with a notice naming it, and the run continues. The rules and the numbers they move are in `fathom_analyze::plugin::finding`.",
        "properties": {
            "confidence": {
                "description": "`0..=1`. A value outside the range is clamped and said (rule R5), and `1.0` on a claim whose evidence is every piece `inferred` is reduced to `0.95` — the price the built-in catalogue pays for the same claim.",
                "maximum": 1.0,
                "minimum": 0.0,
                "type": "number"
            },
            "episode_id": {"description": "The episode the finding is about.", "type": "string"},
            "evidence": {
                "description": "Why the plugin thinks so. A `high` with an empty list here is demoted to `medium` and the notice says so (rule R4).",
                "items": {"$ref": "#/$defs/evidence"},
                "type": "array"
            },
            "finding_type": {
                "description": "`namespace.id`. A plugin may only raise types inside its own namespace and only ones it declared at registration (rule R2); the bare, dotless spelling belongs to the built-in catalogue, so a plugin cannot take a built-in's suppression family.",
                "pattern": type_pattern(),
                "type": "string"
            },
            "jump": {"$ref": "#/$defs/jump"},
            "message": {
                "description": "The claim, in one sentence a person wrote. Empty is not a finding (rule R3).",
                "minLength": 1,
                "type": "string"
            },
            "plugin": {"description": "The plugin that raised it, as it named itself.", "type": "string"},
            "plugin_version": {
                "description": "The plugin's own version, so a re-run after the plugin changed is distinguishable from the last one.",
                "minimum": 0,
                "type": "integer"
            },
            "schema_version": {
                "description": "The version of this shape the finding was written against. A number greater than the reader's is a refusal, not a partial read.",
                "enum": [SCHEMA_VERSION],
                "type": "integer"
            },
            "severity": {
                "description": "ADR-004 Appendix A's `finding.severity` CHECK, and the only severity vocabulary in the product with a constraint behind it. There is no fourth word.",
                "enum": SEVERITIES,
                "type": "string"
            },
            "subject": {
                "description": "What within the episode: a path, a symbol, a test name. Empty is not a finding (rule R3).",
                "minLength": 1,
                "type": "string"
            },
            "suppression_key": {
                "description": "The one key that silences this finding: `<type>` or `<type>:<subject>`. A key outside the plugin's own namespace is rewritten (rule R6) — a plugin may mute its own findings and nothing else.",
                "minLength": 1,
                "type": "string"
            }
        },
        "required": [
            "schema_version",
            "finding_type",
            "plugin",
            "plugin_version",
            "episode_id",
            "subject",
            "message",
            "severity",
            "confidence",
            "jump",
            "suppression_key"
        ],
        "title": "Fathom plugin finding",
        "type": "object",
        "x-fathom": {
            "capability": "fathom_analyze::plugin::Detector::detect",
            "compatibility": "additive changes keep the version; breaking changes bump it; an unreadable version is ignored with a notice",
            "consumer": ["fathom-cli (e6.7)", "out-of-tree plugin crates", "export (e5.7) via the delta document"],
            "contract": CONTRACT,
            "owner": OWNER,
            "reader_ignores_unknown_fields": true,
            "severity_vocabulary": SEVERITIES,
            "delta_contract": insight::CONTRACT,
            "views": view::ALL,
            "version": SCHEMA_VERSION
        },
        "$defs": defs
    })
}

/// The schema as the checked-in file's bytes: two-space pretty-printing and a trailing newline.
#[must_use]
pub fn to_text() -> String {
    let mut text = serde_json::to_string_pretty(&document()).unwrap_or_else(|_| String::from("{}"));
    text.push('\n');
    text
}

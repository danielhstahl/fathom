//! The extraction policy: the numbers that bound the work, and where they come from.
//!
//! ADR-002 §4.5 says a crate below `fathom-cli` does not read a file to find out how it should
//! behave, so this module parses configuration it is *handed* and never opens one. The key names
//! are the `[extract]` section of `fathom.toml`, declared in
//! [`src/recommender/config/schema.py`](../../../../src/recommender/config/schema.py); the defaults
//! below are the same integers, and `tests/test_structure_core_contract.py` fails if the two files
//! disagree about a number.
//!
//! Every key here exists because spike `s1` refused to leave a super-linear step unbounded
//! (`docs/concepts/structure-extraction.md`, "What this changes for `e3.1`", item 6). Resolution is
//! iterative, and the only thing that makes an iterative step safe at repository scale is a bound
//! that lives in the config surface rather than in a `const` somebody forgets to revisit. A
//! rejected value is *noted and ignored*, never fatal: a typo in a config file must not be able to
//! lose an episode.

use serde::{Deserialize, Serialize};
use toml_edit::Item as TomlItem;
use toml_edit::Table as TomlTable;

/// Fixpoint rounds across the corpus. One round resolves every file's statements against what is
/// known so far; a re-export chain of length *n* needs *n* rounds, which is why four is enough for
/// the corpus and why the number has to be a knob. `extract.resolution.max_passes`.
pub const DEFAULT_MAX_RESOLUTION_PASSES: usize = 4;

/// How far one symbol's re-export chain is followed before the answer is declared truncated.
/// `extract.resolution.max_hops`.
pub const DEFAULT_MAX_RESOLUTION_HOPS: usize = 4;

/// Files above this size get file-level analysis and a stated refusal. Every corpus file is three
/// orders of magnitude under it; the number exists for the generated 6 MB JSON-lookalike.
/// `extract.max_file_bytes`.
pub const DEFAULT_MAX_FILE_BYTES: usize = 2_000_000;

/// Node budget for the complexity count of one declaration. Beyond it the counters stop and the
/// note `complexity_truncated` is recorded, so a generated 40 000-entry table is a *stated* cost
/// rather than a quadratic surprise. `extract.complexity.max_nodes`.
pub const DEFAULT_MAX_COMPLEXITY_NODES: usize = 20_000;

/// The policy an extraction run obeys. Echoed into every report and into every stored row's
/// provenance, so a number can be read years later without guessing which build made it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractPolicy {
    /// `extract.resolution.max_passes`.
    pub max_resolution_passes: usize,
    /// `extract.resolution.max_hops`.
    pub max_resolution_hops: usize,
    /// `extract.max_file_bytes`.
    pub max_file_bytes: usize,
    /// `extract.complexity.max_nodes`.
    pub max_complexity_nodes: usize,
}

impl Default for ExtractPolicy {
    fn default() -> Self {
        ExtractPolicy {
            max_resolution_passes: DEFAULT_MAX_RESOLUTION_PASSES,
            max_resolution_hops: DEFAULT_MAX_RESOLUTION_HOPS,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_complexity_nodes: DEFAULT_MAX_COMPLEXITY_NODES,
        }
    }
}

impl ExtractPolicy {
    /// Read the `[extract]` table out of a `fathom.toml` document, alongside the notices a surface
    /// should print about anything it refused.
    pub fn from_repo_config_text(text: &str) -> (ExtractPolicy, Vec<String>) {
        let mut policy = ExtractPolicy::default();
        let mut notices: Vec<String> = Vec::new();
        let document = match text.parse::<toml_edit::DocumentMut>() {
            Ok(document) => document,
            Err(err) => {
                notices.push(format!(
                    "fathom.toml does not parse, so every extraction limit stayed at its built-in \
                     default: {err}"
                ));
                return (policy, notices);
            }
        };
        let Some(table) = document.get("extract").and_then(TomlItem::as_table) else {
            return (policy, notices);
        };
        for (path, slot) in [
            (
                "max_file_bytes",
                Limit::FileBytes(&mut policy.max_file_bytes),
            ),
            (
                "complexity.max_nodes",
                Limit::ComplexityNodes(&mut policy.max_complexity_nodes),
            ),
            (
                "resolution.max_passes",
                Limit::Passes(&mut policy.max_resolution_passes),
            ),
            (
                "resolution.max_hops",
                Limit::Hops(&mut policy.max_resolution_hops),
            ),
        ] {
            let Some(value) = knob(table, path).and_then(TomlItem::as_value) else {
                continue;
            };
            let Some(number) = value.as_integer() else {
                notices.push(format!(
                    "extract.{path} is not a whole number, so it was not used"
                ));
                continue;
            };
            if number < 1 {
                notices.push(format!(
                    "extract.{path} = {number} would do nothing; the built-in default stood"
                ));
                continue;
            }
            *slot.inner() = usize::try_from(number).unwrap_or(usize::MAX);
        }
        // Every key that was *not* read is named. A limit typed as `max_file_size`, or under the
        // wrong table, is a limit that does not exist — and the difference between "I ignored your
        // typo" and "I never saw your typo" is the difference between a config surface and a rumour.
        let known: std::collections::BTreeSet<&str> = [
            "max_file_bytes",
            "complexity.max_nodes",
            "resolution.max_passes",
            "resolution.max_hops",
        ]
        .into_iter()
        .collect();
        let mut seen = Vec::new();
        flatten(table, "", &mut seen);
        for key in seen {
            if !known.contains(key.as_str()) {
                notices.push(format!(
                    "extract.{key} is not an extraction limit fathom reads, so it did nothing"
                ));
            }
        }
        (policy, notices)
    }

    /// The policy as a report prints it: the printed key names, in the order the config reference
    /// documents them.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "extract.resolution.max_passes": self.max_resolution_passes,
            "extract.resolution.max_hops": self.max_resolution_hops,
            "extract.max_file_bytes": self.max_file_bytes,
            "extract.complexity.max_nodes": self.max_complexity_nodes,
        })
    }
}

/// Which field a printed config path writes, so the read loop is one list rather than four copies
/// of the same six lines.
enum Limit<'p> {
    FileBytes(&'p mut usize),
    ComplexityNodes(&'p mut usize),
    Passes(&'p mut usize),
    Hops(&'p mut usize),
}

impl<'p> Limit<'p> {
    fn inner(self) -> &'p mut usize {
        match self {
            Limit::FileBytes(slot)
            | Limit::ComplexityNodes(slot)
            | Limit::Passes(slot)
            | Limit::Hops(slot) => slot,
        }
    }
}

/// Look one knob up by its printed dotted name.
///
/// TOML reads `resolution.max_passes = 2` inside `[extract]` as a *nested* table, so the dotted
/// names this module prints have to be walked rather than used as one key. The literal dotted key
/// is tried first so a repository that quoted it (`"resolution.max_passes" = 2`) reads the same way
/// — the same tolerance `detectors/policy.rs`'s `knob` implements.
fn knob<'t>(table: &'t TomlTable, path: &str) -> Option<&'t TomlItem> {
    if let Some(item) = table.get(path) {
        return Some(item);
    }
    let mut segments = path.split('.');
    let mut item: Option<&TomlItem> = table.get(segments.next()?);
    for segment in segments {
        item = item?.get(segment);
    }
    item
}

/// Every dotted key under a table, so an unrecognised one can be named.
fn flatten(table: &toml_edit::Table, prefix: &str, out: &mut Vec<String>) {
    for (key, item) in table.iter() {
        let path = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        match item.as_table() {
            Some(inner) => flatten(inner, &path, out),
            None => out.push(path),
        }
    }
}

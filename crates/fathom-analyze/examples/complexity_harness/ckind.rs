//! **C-KIND**, made executable.
//!
//! ADR-007 §4's rule, verbatim:
//!
//! > Every node kind named in an implementation's decision-point list must be present in the pinned
//! > grammar's `node-types.json`, and must be *produced by at least one file* in that language's
//! > complexity corpus. A kind that satisfies the first half and not the second is a bug in the list or
//! > in the corpus, and CI says so.
//!
//! This is not a stylistic preference. §11.1 audited `e3.1`'s shipped proxy against the same six
//! `node-types.json` files and found **fourteen** kinds the pinned grammars never emit — entries that
//! count zero forever, silently, and print as a confident low number. A list copied from a language
//! reference page carries the same disease, and the only cure is to check the list against the grammar
//! the build actually links and against a file that proves the kind occurs.
//!
//! Both halves are checked here:
//!
//! * **In the grammar.** `node-types.json` is read out of the vendored crate `Cargo.lock` pins — the
//!   same route §11.1's reproduce-command takes, and the same crate the C build compiled, so the check
//!   is against the grammar that produced the figures and not against a page describing it. Anonymous
//!   operator tokens (`&&`, `||`, `??`, `&.`) are collected from the field and child type lists, which is
//!   where a grammar spells them.
//! * **In the corpus.** Every kind the corpus parses produced, collected by
//!   [`fathom_analyze::structure::complexity::node_kinds`]. A declared kind no corpus file emits is
//!   reported even though it is "in the grammar", because a kind nobody exercises is a kind whose
//!   counting code has never run.

use super::Measured;
use fathom_analyze::structure::complexity::Rules;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

pub(crate) struct Ckind {
    /// The grammar row for the report: crate, version, how many kinds it emits, whether its
    /// `node-types.json` was reachable at all.
    pub grammar: Value,
    pub absent_from_grammar: Vec<String>,
    pub unproduced_in_corpus: Vec<String>,
    produced: usize,
    declared: usize,
}

impl Ckind {
    pub(crate) fn row(&self) -> Value {
        json!({
            "rule": "ADR-007 §4 C-KIND",
            "declared_kinds": self.declared,
            "produced_kinds": self.produced,
            "absent_from_grammar": self.absent_from_grammar,
            "unproduced_in_corpus": self.unproduced_in_corpus,
            "verdict": if self.absent_from_grammar.is_empty() && self.unproduced_in_corpus.is_empty() {
                "clean"
            } else {
                "violated"
            },
            "sentence": "a decision-point list is not evidence that the grammar emits what the list \
                         says; this section is the evidence, and it is re-derived from the pinned \
                         crate and from real parses on every run",
        })
    }
}

/// Check one language's declared list against the pinned grammar and against the corpus.
pub(crate) fn check(
    rules: &Rules,
    files: &[Measured],
    lock: &str,
    dir: &Path,
) -> Result<Ckind, String> {
    let crate_name = grammar_crate(rules.language).ok_or_else(|| {
        format!(
            "{:?}: no grammar crate to check C-KIND against",
            rules.language
        )
    })?;
    let version = lock_version(lock, crate_name).unwrap_or_default();
    let node_types = node_type_kinds(crate_name, &version, rules.language);
    let declared: BTreeSet<String> = rules
        .declared_kinds()
        .into_iter()
        .map(str::to_string)
        .collect();
    let produced: BTreeSet<String> = files
        .iter()
        .flat_map(|file| file.kinds.iter().cloned())
        .collect();

    let absent_from_grammar = match &node_types {
        // A grammar whose `node-types.json` cannot be found is not a pass: the check would silently
        // degrade to the corpus half, and the corpus half cannot see a kind that nothing emits.
        None => declared
            .iter()
            .filter(|kind| is_named_kind(kind))
            .cloned()
            .collect(),
        Some(kinds) => declared
            .iter()
            .filter(|kind| is_named_kind(kind) && !kinds.contains(*kind))
            .cloned()
            .collect(),
    };
    let unproduced_in_corpus = declared
        .iter()
        .filter(|kind| !produced.contains(*kind))
        .cloned()
        .collect();

    Ok(Ckind {
        grammar: json!({
            "crate": crate_name,
            "version": version,
            "node_types": match &node_types {
                Some(kinds) => Value::from(kinds.len()),
                None => Value::String(format!(
                    "unreachable: no {crate_name} node-types.json for version `{version}` under \
                     CARGO_HOME ({}); the corpus directory is {}",
                    cargo_home().display(),
                    dir.display()
                )),
            },
            "kinds_produced_by_corpus": produced.len(),
        }),
        absent_from_grammar,
        unproduced_in_corpus,
        produced: produced.len(),
        declared: declared.len(),
    })
}

/// The grammar crate a language's figures come from, as `Cargo.lock` names it.
fn grammar_crate(language: fathom_analyze::structure::Language) -> Option<&'static str> {
    use fathom_analyze::structure::Language;
    Some(match language {
        Language::Rust => "tree-sitter-rust",
        Language::Python => "tree-sitter-python",
        Language::TypeScript => "tree-sitter-typescript",
        Language::Go => "tree-sitter-go",
        Language::Ruby => "tree-sitter-ruby",
        Language::Java => "tree-sitter-java",
        Language::Unknown => return None,
    })
}

/// The version `Cargo.lock` pins for a crate, from the two lines of its `[[package]]` stanza.
fn lock_version(lock: &str, crate_name: &str) -> Option<String> {
    let mut inside = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            inside = false;
            continue;
        }
        if line == format!("name = \"{crate_name}\"") {
            inside = true;
            continue;
        }
        if inside && let Some(rest) = line.strip_prefix("version = ") {
            return Some(rest.trim_matches('"').to_string());
        }
    }
    None
}

fn cargo_home() -> std::path::PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".cargo"))
                .unwrap_or_else(|| std::path::PathBuf::from("/.cargo"))
        })
}

/// Where a crate keeps the `node-types.json` of the grammar this build links.
///
/// Explicitly, because it is not uniform: the TypeScript crate vendors two grammars
/// (`typescript/` and `tsx/`) and the figures come from the first, so guessing "the only one" would
/// check the wrong grammar for TypeScript and say so by passing.
fn node_types_path(language: fathom_analyze::structure::Language) -> &'static str {
    use fathom_analyze::structure::Language;
    match language {
        Language::TypeScript => "typescript/src/node-types.json",
        _ => "src/node-types.json",
    }
}

/// Every kind a pinned grammar can emit: the top-level named types plus the anonymous tokens its
/// field and child type lists spell out.
fn node_type_kinds(
    crate_name: &str,
    version: &str,
    language: fathom_analyze::structure::Language,
) -> Option<BTreeSet<String>> {
    let relative = format!("{crate_name}-{version}/{}", node_types_path(language));
    let path = registry_candidates(&relative)?.into_iter().next()?;
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let mut out = BTreeSet::new();
    if let Some(entries) = value.as_array() {
        for entry in entries {
            if let Some(kind) = entry["type"].as_str() {
                out.insert(kind.to_string());
            }
        }
    }
    walk_anonymous(&value, &mut out);
    Some(out)
}

/// Collect the anonymous token kinds a grammar mentions anywhere in its `node-types.json`.
fn walk_anonymous(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(kind) = map.get("type").and_then(Value::as_str) {
                let named = map.get("named").and_then(Value::as_bool).unwrap_or(true);
                if !named {
                    out.insert(kind.to_string());
                }
            }
            for child in map.values() {
                walk_anonymous(child, out);
            }
        }
        Value::Array(list) => {
            for child in list {
                walk_anonymous(child, out);
            }
        }
        _ => {}
    }
}

/// The vendored sources of `Cargo`'s registry, one per registry, each holding `<crate>-<version>/`.
fn registry_candidates(relative: &str) -> Option<Vec<std::path::PathBuf>> {
    let src = cargo_home().join("registry/src");
    let mut out = Vec::new();
    for registry in std::fs::read_dir(&src).ok()?.flatten() {
        let candidate = registry.path().join(relative);
        if candidate.exists() {
            out.push(candidate);
        }
    }
    Some(out)
}

/// Whether a declared spelling is a named node kind rather than an operator token.
///
/// Operator tokens (`&&`, `||`, `??`, `&.`) are not listed as types in `node-types.json`, so the
/// grammar half of C-KIND cannot check them — the corpus half checks them, which is the half that
/// matters for a token anyway: a token a corpus file never contains has never been counted.
fn is_named_kind(kind: &str) -> bool {
    kind.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

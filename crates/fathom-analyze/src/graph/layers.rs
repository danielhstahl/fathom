//! The layer tags, from `[layers]`.
//!
//! A layer is not derivable from a file. It is what the repository's owner *asserted* about a folder
//! — which of our directories are library code, which is the door, which is the analysis — and that
//! is why ADR-004 §5.1 lists `layers.` as a `graph.build` config input: the tag is config, so the
//! tag has to move the cache key when the assertion changes.
//!
//! The dialect is deliberately small, because these rules are evaluated once per row of every
//! revision:
//!
//! * a rule with no `*` or `?` is a **path prefix** — `crates/fathom-cli` tags everything under it;
//! * `**` means "anything here and below", `*` means "anything but a separator", `?` is one
//!   character — `crates/**/tests` tags a test directory at any depth;
//! * the **longest matching rule wins**, so a narrow rule can carve a subdirectory out of a broad
//!   one (`crates/fathom-cli/tests` may be tagged apart from `crates/fathom-cli`);
//! * a path no rule claims is **untagged** (`layer = ''`), which is not the same claim as
//!   "has no layer" and is printed differently: an untagged node is a repository that has not said,
//!   not a graph that lost.
//!
//! Ties between equally specific rules go to the alphabetically-first name, so the answer does not
//! depend on the order the table was written in.

use std::collections::BTreeMap;

use toml_edit::Item as TomlItem;
use toml_edit::Table as TomlTable;

/// The configured layer rules.
#[derive(Debug, Clone, Default)]
pub struct Layers {
    /// `(pattern, name)`, sorted by pattern so the match is order-independent.
    entries: Vec<(String, String)>,
    /// Which names came from which pattern, for `fathom config explain`.
    by_name: BTreeMap<String, Vec<String>>,
}

impl Layers {
    /// Build from `(name, pattern)` pairs.
    #[must_use]
    pub fn new(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut sorted: Vec<(String, String)> = Vec::new();
        let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, pattern) in entries {
            by_name
                .entry(name.clone())
                .or_default()
                .push(pattern.clone());
            sorted.push((pattern, name));
        }
        sorted.sort();
        for patterns in by_name.values_mut() {
            patterns.sort();
        }
        Self {
            entries: sorted,
            by_name,
        }
    }

    /// Read the `[layers]` table of a `fathom.toml` document.
    ///
    /// A non-string entry is a notice, not a failure: one typo in one rule must not lose every
    /// layer tag in the repository.
    #[must_use]
    pub fn from_repo_config_text(text: &str) -> (Self, Vec<String>) {
        let mut notices = Vec::new();
        let document = match text.parse::<toml_edit::DocumentMut>() {
            Ok(document) => document,
            Err(err) => {
                notices.push(format!(
                    "fathom.toml does not parse, so no layer tags were applied: {err}"
                ));
                return (Self::default(), notices);
            }
        };
        let Some(table) = document.get("layers").and_then(TomlItem::as_table) else {
            notices.push(
                "fathom.toml declares no [layers], so every node is untagged and the layer \
                 questions answer empty rather than guessing"
                    .to_string(),
            );
            return (Self::default(), notices);
        };
        let (layers, table_notices) = Self::from_table_with_notices(table);
        notices.extend(table_notices);
        (layers, notices)
    }

    /// Build from a parsed `[layers]` table, refusing non-string entries loudly.
    #[must_use]
    pub fn from_table_with_notices(table: &TomlTable) -> (Self, Vec<String>) {
        let mut entries = Vec::new();
        let mut notices = Vec::new();
        for (name, item) in table {
            let Some(list) = item.as_array() else {
                notices.push(format!(
                    "layers.{name} is not a list of paths, so it tagged nothing"
                ));
                continue;
            };
            for entry in list.iter() {
                match entry.as_str() {
                    Some(pattern) => entries.push((name.to_string(), pattern.to_string())),
                    None => notices.push(format!(
                        "layers.{name} has an entry that is not a string; it was skipped"
                    )),
                }
            }
        }
        (Self::new(entries), notices)
    }

    /// The layer a path belongs to, or `""` when nothing claims it.
    #[must_use]
    pub fn layer_of(&self, path: &str) -> String {
        let mut best: Option<(usize, &str)> = None;
        for (pattern, name) in &self.entries {
            let matched = if is_glob(pattern) {
                glob_match(pattern, path)
            } else {
                path.starts_with(pattern.as_str())
            };
            if matched && best.is_none_or(|(len, _)| pattern.len() > len) {
                best = Some((pattern.len(), name));
            }
        }
        best.map(|(_, name)| name.to_string()).unwrap_or_default()
    }

    /// How many rules are configured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no rule is configured, in which case every node is untagged.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The distinct layer names configured, sorted — the list a layer subgraph query iterates.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.by_name.keys().cloned().collect()
    }

    /// The patterns that claim one layer name, for `fathom config explain --path layers.ui`.
    #[must_use]
    pub fn patterns_for(&self, name: &str) -> &[String] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }
}

/// Whether a rule has to be read as a glob.
fn is_glob(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

/// The small glob dialect the docs above describe.
///
/// `globset` is already a dependency of this crate, and this is not an argument for hand-rolling a
/// matcher in general; it is an argument about *this* matcher, which runs once per row per revision
/// against rules whose whole job is "this folder is that layer". Four branches, and the dialect is
/// published above so a rule can be checked by reading it.
#[must_use]
pub fn glob_match(pattern: &str, text: &str) -> bool {
    if let Some((prefix, suffix)) = pattern.split_once("**") {
        let suffix = suffix.trim_start_matches('/');
        return text.starts_with(prefix)
            && (suffix.is_empty()
                || text.ends_with(suffix)
                || text.contains(&format!("/{suffix}")));
    }
    let mut parts = pattern.split('*');
    let Some(head) = parts.next() else {
        return false;
    };
    if !head.is_empty() && !text.starts_with(head) {
        return false;
    }
    let mut rest = &text[head.len()..];
    for part in parts {
        let part = part.replace('?', ".");
        let needle = part.as_str();
        let Some(at) = rest.find(needle) else {
            if needle.is_empty() {
                continue;
            }
            return false;
        };
        rest = &rest[at + needle.len()..];
    }
    true
}

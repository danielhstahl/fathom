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

/// How many files of one language one extraction run may scan. Ruby's cap, and the only language
/// that needs one: `s1` measured the Ruby grammar at under 1 MiB/s against 11–16 MiB/s for every
/// other language, so a warm graph pass over a Ruby repository is the one case where "read every
/// file" is a cost no budget in ADR-004 covers. `extract.ruby.max_files_per_run`.
pub const DEFAULT_RUBY_MAX_FILES_PER_RUN: usize = 4_000;

/// How many bytes of one language one extraction run may scan. Same reasoning as the count cap and
/// the reason both exist: the count cap binds on a tree of many small `def`s (which is what makes
/// Ruby expensive per construct) and the byte cap binds on a few enormous generated files.
/// `extract.ruby.max_bytes_per_run`.
pub const DEFAULT_RUBY_MAX_BYTES_PER_RUN: usize = 24_000_000;

/// The order Ruby files are scanned in, as path prefixes, so that a run which *has* to stop early
/// stops having read the files an entry point reaches. `spec/` and `test/` are last on purpose: a
/// budget spent on a suite is a budget spent on the least architecturally interesting half of the
/// repository. `extract.ruby.priority`.
pub const DEFAULT_RUBY_PRIORITY: &[&str] = &["lib", "app", "ext", "spec", "test"];

/// How far the Java extractor looks for a dependency that has no `import` statement.
/// `extract.java.same_package_scan`.
pub const DEFAULT_JAVA_SAME_PACKAGE_SCAN: &str = "imports_plus_package";

/// The two spellings `extract.java.same_package_scan` takes, and the reason there are two.
///
/// Spike `s1` found 14/14 same-package edges with zero false positives — by scanning *every* type
/// reference in a ten-file corpus. At repository scale that is how `Builder`, `Result` and `Config`
/// get resolved to the wrong package, which is why the shipped default is the scoped scan and why
/// the wide one is still selectable: the report measures both on a 1 000-file corpus so the scoping
/// decision is a number rather than an argument (`e3.1b` acceptance criterion 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanScope {
    /// Only an unqualified type reference this file does not already import by name, resolved
    /// against the package the file declares. The shipped default.
    ImportsPlusPackage,
    /// Every `type_identifier` in the file, resolved against anything in the corpus with that name.
    /// What `s1` ran; kept selectable so the false-positive cost of it stays measurable.
    AllTypeReferences,
}

impl ScanScope {
    /// The printed spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ScanScope::ImportsPlusPackage => "imports_plus_package",
            ScanScope::AllTypeReferences => "all_type_references",
        }
    }

    /// Parse a configured value. `None` means "not a word the extractor knows", which the caller
    /// turns into a notice rather than an error.
    pub fn parse(text: &str) -> Option<ScanScope> {
        match text.trim() {
            "imports_plus_package" => Some(ScanScope::ImportsPlusPackage),
            "all_type_references" => Some(ScanScope::AllTypeReferences),
            _ => None,
        }
    }
}

/// A per-run cap on one language's scan, and the order the cap spends itself in.
///
/// This is the scan policy spike `s1` named as the thing that changes when a grammar is slow
/// (finding 7): the feature gates stay as ADR-002 §6.5 wrote them, and what a slow grammar buys
/// instead is a *bound that is arithmetic*. The caps are per run, files over the cap come back as
/// [`UnavailableReason::ScanBudget`](crate::structure::UnavailableReason::ScanBudget) with their
/// file-level numbers intact, and the priority list decides who gets refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanBudget {
    /// `…max_files_per_run`.
    pub max_files: usize,
    /// `…max_bytes_per_run`.
    pub max_bytes: usize,
    /// Path prefixes, in the order they are scanned. A file matching none is scanned last.
    pub priority: Vec<String>,
}

impl ScanBudget {
    /// Where a path sits in the priority list: lower sorts first, and the length of the list is
    /// "matched nothing".
    pub fn rank(&self, path: &str) -> usize {
        self.priority
            .iter()
            .position(|prefix| {
                let prefix = prefix.trim_matches('/');
                path == prefix
                    || path.starts_with(&format!("{prefix}/"))
                    || path.contains(&format!("/{prefix}/"))
            })
            .unwrap_or(self.priority.len())
    }
}

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
    /// The Ruby scan budget: `extract.ruby.max_files_per_run`, `…max_bytes_per_run`, `…priority`.
    pub ruby_scan: ScanBudget,
    /// `extract.java.same_package_scan`.
    pub java_same_package_scan: ScanScope,
    /// Whether a name the file binds itself (a method type parameter, a local class) is treated as a
    /// reference for the same-package guess. `true` is the shipped rule.
    ///
    /// Not a configuration key, and deliberately not one: an operator has no reason to choose the
    /// worse-sounding side of a rule, and a key is a promise to keep both sides working. The harness
    /// switches it off to *measure* the rule — the report's counterfactual is a third scan, not
    /// arithmetic about a rule someone might later edit out of the code.
    pub java_bound_names: bool,
}

impl ExtractPolicy {
    /// The per-run scan budget for a language, if that language has one.
    ///
    /// Only Ruby does. The mechanism is generic because the *reason* is generic — a grammar whose
    /// cost is per syntactic construct rather than per byte needs a cap expressed in constructs —
    /// and every other grammar measured 11–16 MiB/s, where a cap would be decoration.
    #[must_use]
    pub fn scan_budget(&self, language: crate::structure::Language) -> Option<&ScanBudget> {
        match language {
            crate::structure::Language::Ruby => Some(&self.ruby_scan),
            _ => None,
        }
    }
}

impl Default for ExtractPolicy {
    fn default() -> Self {
        ExtractPolicy {
            max_resolution_passes: DEFAULT_MAX_RESOLUTION_PASSES,
            max_resolution_hops: DEFAULT_MAX_RESOLUTION_HOPS,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_complexity_nodes: DEFAULT_MAX_COMPLEXITY_NODES,
            ruby_scan: ScanBudget {
                max_files: DEFAULT_RUBY_MAX_FILES_PER_RUN,
                max_bytes: DEFAULT_RUBY_MAX_BYTES_PER_RUN,
                priority: DEFAULT_RUBY_PRIORITY
                    .iter()
                    .map(|prefix| (*prefix).to_string())
                    .collect(),
            },
            java_same_package_scan: ScanScope::ImportsPlusPackage,
            java_bound_names: true,
        }
    }
}

impl ExtractPolicy {
    /// The scoped same-package scan with the bound-name rule switched off.
    ///
    /// The harness uses this to *measure* the rule: re-run the same corpus with it off and publish the
    /// rate. See [`ExtractPolicy::java_bound_names`] for why this is not a configuration key.
    #[must_use]
    pub fn with_bound_name_rule_off(mut self) -> Self {
        self.java_bound_names = false;
        self
    }

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
        // The scan policy: the two Ruby caps (integers, same rule as the other bounds), Ruby's
        // priority list (a string list) and Java's scan scope (one of two words). Kept in their own
        // loop rather than folded into `Limit`, which is an integer-slot abstraction and would have
        // to stop being one to carry a list.
        for path in ["ruby.max_files_per_run", "ruby.max_bytes_per_run"] {
            let Some(value) = knob(table, path).and_then(TomlItem::as_value) else {
                continue;
            };
            let Some(number) = value.as_integer() else {
                notices.push(format!(
                    "extract.{path} is not a whole number, so it was not used"
                ));
                continue;
            };
            if number < 0 {
                notices.push(format!(
                    "extract.{path} = {number} is not a quantity; the built-in default stood"
                ));
                continue;
            }
            let slot = usize::try_from(number).unwrap_or(usize::MAX);
            if path.ends_with("max_files_per_run") {
                policy.ruby_scan.max_files = slot;
            } else {
                policy.ruby_scan.max_bytes = slot;
            }
        }
        if let Some(value) = knob(table, "ruby.priority").and_then(TomlItem::as_value) {
            match value.as_array() {
                Some(array) => {
                    let mut prefixes = Vec::new();
                    for entry in array {
                        match entry.as_str() {
                            Some(prefix) => prefixes.push(prefix.to_string()),
                            None => notices.push(
                                "extract.ruby.priority must be a list of path prefixes".to_string(),
                            ),
                        }
                    }
                    policy.ruby_scan.priority = prefixes;
                }
                None => notices.push("extract.ruby.priority is not a list of strings".to_string()),
            }
        }
        if let Some(value) = knob(table, "java.same_package_scan").and_then(TomlItem::as_value) {
            match value.as_str().and_then(ScanScope::parse) {
                Some(scope) => policy.java_same_package_scan = scope,
                None => notices.push(format!(
                    "extract.java.same_package_scan is not `{}` or `{}`, so the scoped default \
                     stood",
                    ScanScope::ImportsPlusPackage.as_str(),
                    ScanScope::AllTypeReferences.as_str()
                )),
            }
        }
        // Every key that was *not* read is named. A limit typed as `max_file_size`, or under the
        // wrong table, is a limit that does not exist — and the difference between "I ignored your
        // typo" and "I never saw your typo" is the difference between a config surface and a rumour.
        let known: std::collections::BTreeSet<&str> = [
            "max_file_bytes",
            "complexity.max_nodes",
            "resolution.max_passes",
            "resolution.max_hops",
            "ruby.max_files_per_run",
            "ruby.max_bytes_per_run",
            "ruby.priority",
            "java.same_package_scan",
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
            "extract.ruby.max_files_per_run": self.ruby_scan.max_files,
            "extract.ruby.max_bytes_per_run": self.ruby_scan.max_bytes,
            "extract.ruby.priority": self.ruby_scan.priority.clone(),
            "extract.java.same_package_scan": self.java_same_package_scan.as_str(),
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

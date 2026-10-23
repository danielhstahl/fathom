//! Reading the facts out of what is already stored: the graph of `e3.2`, `e3.1`'s structural records,
//! and `e3.3`'s churn window (`e3.8`, `reco-wky`).
//!
//! This is the seam the rest of the module is built against, and it is deliberately narrow: one call
//! takes a *materialised* revision's graph, a naming, the per-file structural records for that revision
//! and a churn window, and hands back [`ModuleFacts`]. Nothing in this function reads a store, a file,
//! a clock or an environment variable — so a lane can call it after it has paid for its materialisation,
//! and a test can call it with three hand-written modules and mean the same thing (ADR-002 §6.1 needs
//! the ranking to exist in a build with no SQLite and no grammars linked at all).
//!
//! ```text
//!   graph::Lane::materialise(tree)      → (Graph, Naming)        e3.2
//!   structural records for `tree`       → [FileStructure]        e3.1
//!   graph::Lane::churn_recent(window)   → ChurnReport            e3.3
//!                      │
//!                      └─ measure::facts_of(...)  → [ModuleFacts] → rank::(...)
//! ```
//!
//! # What is counted as one dependent
//!
//! [`crate::graph::query::dependents_of`] already refuses to merge the three kinds of caller, and the
//! facts keep that split rather than flattening it: named, wildcard (contents unknown, `s1` finding 2)
//! and package-granular (`s1` finding 4). The score counts all three, because a wildcard dependency is
//! a dependency whose contents are unknown and not a dependency that does not exist — but each row
//! prints the split in its reason and repeats the caveat, so nobody reads a fan-in of 12 as twelve
//! files that name this one.

use std::collections::BTreeMap;

use crate::graph::query::{dependencies_of, dependents_of};
use crate::graph::{ChurnReport, Graph, Naming, RowKind};
use crate::structure::FileStructure;

use super::model::{ChurnFacts, ComplexityFacts, ModuleFacts};

/// One file's structural facts, as `e3.1` measured them and as the score needs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    /// The path, as the graph keys it.
    pub path: String,
    /// Code lines: comments and blanks excluded, per `e3.1`'s file-level half.
    pub code_lines: usize,
    /// The span proxies, or `None` when the file has no structural model at all — a grammar this build
    /// does not carry, a file the extractor refused, a binary. [`FileStructure::structure`] being
    /// `None` is exactly this case, and it must not arrive as "zero complexity".
    pub complexity: Option<ComplexityFacts>,
    /// Whether its declaration list may have shrunk rather than shrunk (`s1` finding 1).
    pub may_have_shrunk: bool,
    /// Why there is no structure, kept for the report: an unreadable file and an unread one are
    /// different facts and only one of them is a bug.
    pub unavailable: Option<String>,
}

impl FileFacts {
    /// Read one `e3.1` record.
    ///
    /// Complexity is summed over **top-level declarations only**, because `e3.1` counts each
    /// declaration's own subtree: a nested declaration's branches are already inside its enclosing
    /// declaration's count, and summing every declaration would count a method's `if` once as the
    /// method's and once as its `impl`'s.
    #[must_use]
    pub fn of(record: &FileStructure) -> Self {
        let complexity = record.structure.as_ref().map(|structure| {
            let mut facts = ComplexityFacts::default();
            for symbol in structure.symbols.iter().filter(|symbol| symbol.top_level()) {
                let points = symbol.complexity.branches
                    + symbol.complexity.loops
                    + symbol.complexity.returns
                    + symbol.complexity.nesting;
                facts.points += points;
                facts.declarations += 1;
                facts.max_body_lines = facts.max_body_lines.max(symbol.complexity.body_lines);
                facts.max_nesting = facts.max_nesting.max(symbol.complexity.nesting);
            }
            facts
        });
        FileFacts {
            path: record.path.clone(),
            code_lines: record.file_level.code_lines,
            complexity,
            may_have_shrunk: record
                .structure
                .as_ref()
                .is_some_and(|structure| structure.may_have_shrunk().is_some()),
            unavailable: record.reason.as_ref().map(|reason| reason.sentence()),
        }
    }
}

/// Build the facts for every module the graph holds at one revision.
///
/// * `files` — `e3.1`'s records for that revision, in any order; only the paths the graph names are
///   read, and a module with no record gets `None` for size and complexity rather than a zero.
/// * `churn` — `e3.3`'s window, or `None` when there is no earlier revision to measure against. A
///   module the window never touched is *zero churn*, which is a measurement; the absence of a window
///   is not, and the two print differently.
#[must_use]
pub fn facts_of(
    graph: &Graph,
    naming: &Naming,
    files: &[FileFacts],
    churn: Option<&ChurnReport>,
) -> Vec<ModuleFacts> {
    let by_path: BTreeMap<&str, &FileFacts> = files
        .iter()
        .map(|facts| (facts.path.as_str(), facts))
        .collect();
    let mut out = Vec::new();
    let modules: Vec<(i64, String)> = graph
        .nodes()
        .iter()
        .filter(|(_, kind)| **kind == RowKind::Module)
        .filter_map(|(id, _)| naming.name(*id).map(|key| (*id, key.to_string())))
        .collect();
    for (id, module) in modules {
        let in_fan = dependents_of(graph, naming, &module);
        let out_fan = dependencies_of(graph, naming, &module);
        let file = by_path.get(module.as_str()).copied();
        let churn_facts = churn.map(|report| match report.find(&module) {
            Some(churn) => ChurnFacts {
                events: churn.churn,
                revisions: churn.revisions,
                window: churn.window,
            },
            // A window that exists and never named this module is a measurement of *zero*, not the
            // absence of one: "this module is not being reworked" is a fact the ranking may use, and
            // "there is no window to measure" is not. The two print differently in `rank`.
            None => ChurnFacts {
                events: 0,
                revisions: 0,
                window: report.window,
            },
        });
        out.push(ModuleFacts {
            module,
            layer: naming.layer(id).to_string(),
            fan_in: in_fan.named.len(),
            fan_in_wildcard: in_fan.wildcard.len(),
            fan_in_package: in_fan.package_granular.len(),
            fan_out: out_fan.named.len(),
            fan_out_wildcard: out_fan.wildcard.len(),
            fan_out_package: out_fan.package_granular.len(),
            code_lines: file.map(|facts| facts.code_lines),
            complexity: file.and_then(|facts| facts.complexity),
            churn: churn_facts,
            incomplete: naming.incomplete(id) || file.is_some_and(|facts| facts.may_have_shrunk),
        });
    }
    out.sort_by(|a, b| a.module.cmp(&b.module));
    out
}

/// The same, straight from `e3.1` records — the shape a caller holding `FileStructure`s wants.
#[must_use]
pub fn facts_of_records(
    graph: &Graph,
    naming: &Naming,
    records: &[FileStructure],
    churn: Option<&ChurnReport>,
) -> Vec<ModuleFacts> {
    let files: Vec<FileFacts> = records.iter().map(FileFacts::of).collect();
    facts_of(graph, naming, &files, churn)
}

/// Which files the graph names and `e3.1` has no record for — the hole a ranking prints instead of
/// filling with zeroes.
#[must_use]
pub fn missing_structures(graph: &Graph, naming: &Naming, files: &[FileFacts]) -> Vec<String> {
    let known: BTreeMap<&str, &FileFacts> = files
        .iter()
        .map(|facts| (facts.path.as_str(), facts))
        .collect();
    let mut out: Vec<String> = graph
        .nodes()
        .iter()
        .filter(|(_, kind)| **kind == RowKind::Module)
        .filter_map(|(id, _)| naming.name(*id))
        .filter(|key| !known.contains_key(*key))
        .map(str::to_string)
        .collect();
    out.sort();
    out
}

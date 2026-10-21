//! Queries over an extracted corpus.
//!
//! Only one query lives here, and it is the one spike `s1` named as the reason wildcard edges have
//! to exist: **"who depends on this?"** A graph that answers that question with "nobody" because the
//! dependent used a star import is worse than a graph that admits it does not know
//! (`docs/concepts/structure-extraction.md`, finding 2).

use crate::structure::model::{CorpusStructure, Dependents, EdgeKind};
use std::collections::BTreeSet;

/// Who depends on `file`, or on `file`'s `symbol`.
///
/// The two halves are kept apart deliberately. `named` is the confident answer: a file that names
/// the unit (or names it through a barrel it re-exports). `unknown` is a file whose dependency is
/// real but of contents the parser could not name — a star import, a barrel `export *`, a
/// whole-module import, a side-effect import. A surface that prints only `named` is the bug s1
/// named; a surface that merges the two is a subtler version of the same bug.
pub fn dependents_of(corpus: &CorpusStructure, file: &str, symbol: Option<&str>) -> Dependents {
    let mut out = Dependents::default();
    for record in &corpus.files {
        let Some(structure) = &record.structure else {
            continue;
        };
        if record.path == file {
            // A file is not its own dependent; self-edges are already dropped in the engine.
            continue;
        }
        let mut named = false;
        let mut unknown = false;
        let mut package_granular = false;
        for edge in &structure.edges {
            let points_at_unit = edge.target == file;
            let reached_through = edge.via.as_deref() == Some(file);
            match edge.kind {
                EdgeKind::Resolved => {
                    if points_at_unit && symbol.is_none_or(|want| edge.symbol == want) {
                        named = true;
                    } else if reached_through {
                        // The edge was resolved *through* this file, which makes it a dependent of
                        // the forwarder as well as of the origin.
                        named = true;
                    }
                }
                EdgeKind::Wildcard => {
                    if points_at_unit {
                        unknown = true;
                        // Go's wildcard edges are the package-granular ones: the dependent named a
                        // package, not this file, and a caller list that prints them the same way
                        // is the file-level lie s1 finding 4 named.
                        package_granular = edge.is_package_granular();
                    }
                }
                EdgeKind::Unresolved => {}
            }
        }
        if named && !out.named.contains(&record.path) {
            out.named.push(record.path.clone());
        }
        if unknown && !out.unknown.contains(&record.path) {
            out.unknown.push(record.path.clone());
        }
        if package_granular && !out.package_granular.contains(&record.path) {
            out.package_granular.push(record.path.clone());
        }
    }
    out.named.sort();
    out.unknown.sort();
    out.package_granular.sort();
    out
}

/// Which files an entry point pulls in, transitively, over whole-file dependencies.
///
/// This is the Ruby question, and it is the only surface question Ruby can answer honestly
/// (spike `s1`, finding 3: with 8 exports for 67 declarations and no per-symbol access control,
/// *"what does this file's public interface contain"* is not a meaningful query; *"what is loaded
/// when you require this file"* is). It also happens to be the right question for a Go `main` or a
/// Python `__main__`, so it is not named after Ruby.
///
/// `entry` is a corpus path. The answer is every file reachable through wildcard edges — a `require`,
/// a star import, a Go package import, a barrel `export *` — sorted, with the entry itself excluded.
/// The walk is bounded by [`LOAD_HOPS`]: a dependency cycle must not be able to hang a surface, and a
/// load graph deeper than the bound returns what it reached with the depth it stopped at.
pub fn loads(corpus: &CorpusStructure, entry: &str) -> Loads {
    let mut frontier: BTreeSet<String> = BTreeSet::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut depth = 0usize;
    frontier.insert(entry.to_string());
    seen.insert(entry.to_string());
    let mut truncated = false;
    while !frontier.is_empty() {
        if depth >= LOAD_HOPS {
            truncated = true;
            break;
        }
        depth += 1;
        let mut next: BTreeSet<String> = BTreeSet::new();
        for path in &frontier {
            let Some(record) = corpus.record(path) else {
                continue;
            };
            let Some(structure) = &record.structure else {
                continue;
            };
            for edge in structure.wildcard_edges() {
                if edge.target.is_empty() || edge.target == entry || seen.contains(&edge.target) {
                    continue;
                }
                seen.insert(edge.target.clone());
                out.insert(edge.target.clone());
                next.insert(edge.target.clone());
            }
        }
        frontier = next;
    }
    Loads {
        entry: entry.to_string(),
        files: out.into_iter().collect(),
        depth,
        truncated,
    }
}

/// The answer to "what does loading this file pull in".
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Loads {
    /// The file the question was asked about.
    pub entry: String,
    /// Every file reachable through whole-file dependencies, sorted.
    pub files: Vec<String>,
    /// How many hops were walked.
    pub depth: usize,
    /// The bound stopped the walk before it ran out of graph.
    pub truncated: bool,
}

impl Loads {
    /// The sentence a surface prints: the count, and the honest note when the bound bit.
    #[must_use]
    pub fn sentence(&self) -> String {
        let base = format!(
            "loading {} pulls in {} file(s){}",
            self.entry,
            self.files.len(),
            if self.files.is_empty() {
                String::new()
            } else {
                format!(": {}", self.files.join(", "))
            }
        );
        if self.truncated {
            format!("{base} (stopped after {} hops)", self.depth)
        } else {
            base
        }
    }
}

/// How far [`loads`] walks. Twenty files deep is a load graph nobody has; the bound exists so a
/// cycle in a hostile repository costs a stated truncation rather than a hung pane.
const LOAD_HOPS: usize = 20;

/// Which files declare a symbol name anywhere, resolved or not. `e5.6`'s symbol search reads this;
/// `e3.2` will supersede it with the stored graph.
pub fn declaring_files(corpus: &CorpusStructure, symbol: &str) -> Vec<String> {
    let mut out: Vec<String> = corpus
        .files
        .iter()
        .filter(|record| {
            record
                .structure
                .as_ref()
                .is_some_and(|structure| structure.symbols.iter().any(|s| s.name == symbol))
        })
        .map(|record| record.path.clone())
        .collect();
    out.sort();
    out
}

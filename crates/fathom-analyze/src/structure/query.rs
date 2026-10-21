//! Queries over an extracted corpus.
//!
//! Only one query lives here, and it is the one spike `s1` named as the reason wildcard edges have
//! to exist: **"who depends on this?"** A graph that answers that question with "nobody" because the
//! dependent used a star import is worse than a graph that admits it does not know
//! (`docs/concepts/structure-extraction.md`, finding 2).

use crate::structure::model::{CorpusStructure, Dependents, EdgeKind};

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
    }
    out.named.sort();
    out.unknown.sort();
    out
}

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

//! Where a dependency statement actually is, so a violation can point at a line.
//!
//! The architecture delta's rows are `(kind, src, dst)` over interned ids — that is the whole stored
//! shape, and it is why a revision costs 6 KB instead of 4 MB
//! ([`crate::graph::model`]). It is also why a violation has no line of its own: the row says
//! `api.rs → db.rs` and nothing about the statement that made it. The statement is in the structural
//! substrate, in [`crate::structure::Import::span`], and a caller that built the graph already has
//! those records in hand, so this module pairs the two and costs nothing to do it.
//!
//! # The pairing, and what it refuses to guess
//!
//! An [`crate::structure::Edge`] is produced *from* an [`crate::structure::Import`] and carries that
//! statement's `form` and `symbol`, so `(form, symbol)` is the join key. It is exact when it is unique
//! and it is not always unique, so the two failure modes are named instead of smoothed over:
//!
//! * **two statements with the same form and symbol** (two `use` lines importing two different things
//!   that both resolve into the same file): the pair is [`Line::Ambiguous`] and the violation prints
//!   the file with no line and says why. Printing the first candidate would be a coin flip dressed as
//!   a citation, and a reviewer who follows a wrong line once stops trusting the right ones.
//! * **no statement that could have made the edge** — an edge the extractor derived rather than read,
//!   a Java same-package guess, a record from a version of the extractor that did not copy the form:
//!   [`Line::Unrecorded`], which is the truth.
//!
//! When several statements genuinely create the same module-to-module edge (`from x import a` and
//! `from x import b`), the pair is not ambiguous — they are all the same edge in the graph — so the
//! **earliest** line is named. Deterministic, first-in-reading-order, and stated here rather than
//! left as a side effect.

use std::collections::BTreeMap;

use crate::graph::build::ModuleInput;
use crate::graph::delta::module_key;
use crate::structure::EdgeKind;

/// One `(source module, destination module)` pair's answer about where the statement is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// The statement starts at this one-based line.
    At(usize),
    /// Several statements could be the one; none may be printed as *the* citation.
    Ambiguous(usize),
    /// Nothing was handed over that could say.
    Unrecorded,
}

impl Line {
    /// The line to print, if there is one.
    #[must_use]
    pub const fn number(self) -> Option<usize> {
        match self {
            Line::At(line) => Some(line),
            Line::Ambiguous(_) | Line::Unrecorded => None,
        }
    }

    /// Why there is no number, in the words a violation prints. `None` when there is a number.
    #[must_use]
    pub fn note(self) -> Option<String> {
        match self {
            Line::At(_) => None,
            Line::Ambiguous(count) => Some(format!(
                "{count} statements in this file could be this dependency; the line is not printed \
                 because guessing would be worse than saying so"
            )),
            Line::Unrecorded => Some(
                "the structural record does not name a statement for this edge, so only the file \
                 can be cited"
                    .to_string(),
            ),
        }
    }

    /// Whether anything was actually recorded for this pair.
    #[must_use]
    pub const fn is_recorded(self) -> bool {
        matches!(self, Line::At(_))
    }
}

/// The pair→line map, built once per revision from the records the builder was given.
#[derive(Debug, Clone, Default)]
pub struct Locate {
    pairs: BTreeMap<(String, String), Line>,
    /// How many files were read, for the report's cost line.
    pub files: usize,
    /// How many edges got a line.
    pub located: usize,
    /// How many pairs came back ambiguous.
    pub ambiguous: usize,
    /// How many edges had no statement to pair with.
    pub unpaired: usize,
}

impl Locate {
    /// Nothing to say: every violation will cite a file and no line.
    ///
    /// This is the state of a caller that has a stored graph and not the substrate — a legitimate
    /// state, and a stated one, not a failure.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Pair every edge the substrate could account for with the statement that made it.
    #[must_use]
    pub fn from_inputs(inputs: &[ModuleInput<'_>]) -> Self {
        let mut out = Self::default();
        for input in inputs {
            out.files += 1;
            let Some(structure) = input.file.and_then(|record| record.structure.as_ref()) else {
                continue;
            };
            let src = module_key(input.path);
            // (form, symbol) → the lines of the statements that could have produced it.
            let mut statements: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
            for import in &structure.imports {
                statements
                    .entry((import.form.as_str(), import.symbol.as_str()))
                    .or_default()
                    .push(import.span.start_line.max(1));
            }
            for edge in &structure.edges {
                if edge.target.is_empty() || edge.kind == EdgeKind::Unresolved {
                    continue;
                }
                let dst = module_key(&edge.target);
                if dst == src {
                    continue;
                }
                let key = (edge.form.as_str(), edge.symbol.as_str());
                let candidates = statements.get(&key).map_or_else(Vec::new, |lines| {
                    let mut sorted = lines.clone();
                    sorted.sort_unstable();
                    sorted.dedup();
                    sorted
                });
                let line = match candidates.as_slice() {
                    [] => {
                        out.unpaired += 1;
                        Line::Unrecorded
                    }
                    [only] => {
                        out.located += 1;
                        Line::At(*only)
                    }
                    many => {
                        out.ambiguous += 1;
                        Line::Ambiguous(many.len())
                    }
                };
                out.merge(src.clone(), dst, line);
            }
        }
        out
    }

    /// Record one pair by hand — for a caller that knows where a statement is without a structural
    /// record, and for the tests that assert the states rather than deriving them.
    pub fn record(&mut self, src: &str, dst: &str, line: usize) {
        self.merge(module_key(src), module_key(dst), Line::At(line));
    }

    /// The answer for one pair.
    #[must_use]
    pub fn at(&self, src: &str, dst: &str) -> Line {
        self.pairs
            .get(&(src.to_string(), dst.to_string()))
            .copied()
            .unwrap_or(Line::Unrecorded)
    }

    /// How many pairs were recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Whether nothing was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// What the counters say about this evidence.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "files": self.files,
            "located": self.located,
            "ambiguous": self.ambiguous,
            "unpaired": self.unpaired,
        })
    }

    /// Fold one pair in, keeping the strongest claim: a line beats a shrug, and the earliest line of
    /// several that make the same edge is the one a reader meets first.
    fn merge(&mut self, src: String, dst: String, line: Line) {
        let key = (src, dst);
        let keep = match (self.pairs.get(&key).copied(), line) {
            (None, next) => next,
            (Some(Line::At(left)), Line::At(right)) => Line::At(left.min(right)),
            (Some(Line::At(line)), _) => Line::At(line),
            (Some(Line::Ambiguous(left)), Line::Ambiguous(right)) => {
                Line::Ambiguous(left.max(right))
            }
            (Some(Line::Ambiguous(count)), _) => Line::Ambiguous(count),
            (Some(Line::Unrecorded), next) => next,
        };
        self.pairs.insert(key, keep);
    }
}

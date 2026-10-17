//! Spike `s1` — the shapes every part of the harness agrees on: the normalised symbol, the
//! hand-authored truth file, the index the resolvers read, and the scorer.
//!
//! See `docs/concepts/structure-extraction.md` for what each shape means and why.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// The kind vocabulary. Deliberately ten names, not each language's own sixty: the graph
/// `e3.2` builds has one node type per kind, and an extractor that cannot say which of these
/// ten a declaration is has not told `e3.2` anything.
pub const KINDS: [&str; 10] = [
    "module",
    "class",
    "interface",
    "struct",
    "enum",
    "type",
    "fn",
    "method",
    "const",
    "var",
];

/// One normalised definition: `kind` from [`KINDS`], `name` dotted from the file's top level
/// (`Rect.new`, `Demo.Core.Shape.area`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Def {
    pub kind: String,
    pub name: String,
}

impl Def {
    pub fn new(kind: &str, name: impl Into<String>) -> Def {
        Def {
            kind: kind.to_string(),
            name: name.into(),
        }
    }
}

/// A resolved import: `target` is a corpus-relative path, `symbol` is the named symbol or
/// `*` for "the file as a whole".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge {
    pub target: String,
    pub symbol: String,
}

impl Edge {
    pub fn new(target: impl Into<String>, symbol: impl Into<String>) -> Edge {
        Edge {
            target: target.into(),
            symbol: symbol.into(),
        }
    }
}

/// What an extractor says it could not resolve (a specifier) or knowingly could not see
/// (a blind spot, e.g. a macro-generated definition). Diagnostics, never scored.
#[derive(Debug, Default)]
pub struct Scan {
    pub defs: Vec<Def>,
    pub exports: Vec<String>,
    pub imports: Vec<RawImport>,
    pub blind: Vec<String>,
    /// The per-language side channel the resolver and the export rule read: a module path here,
    /// a package name there, a visibility marker somewhere else. Keys are namespaced by the
    /// language module that writes them, and nothing outside that module interprets them.
    pub aux: BTreeMap<String, String>,
}

impl Scan {
    pub fn note(&mut self, kind: &str, detail: &str) {
        self.blind.push(format!("{kind}:{detail}"));
    }
}

/// An import as the source states it, before resolution.
#[derive(Debug, Clone)]
pub struct RawImport {
    /// The specifier as written (`./geo`, `crate::geo::point`, `demo/version`, `com.acme.app.model.*`).
    pub spec: String,
    /// The symbol the statement names, or `*` for a whole-file import.
    pub symbol: String,
    /// The module path the statement sits in, for relative forms (`super`, `.`).
    pub module: Vec<String>,
    /// A short spelling of the statement's form, for the report (`use`, `mod_decl`, `from_import`).
    pub form: &'static str,
    /// True when the statement re-exports what it imports rather than consuming it.
    pub reexport: bool,
}

impl RawImport {
    pub fn new(
        spec: impl Into<String>,
        symbol: impl Into<String>,
        form: &'static str,
    ) -> RawImport {
        RawImport {
            spec: spec.into(),
            symbol: symbol.into(),
            module: Vec::new(),
            form,
            reexport: false,
        }
    }
}

/// What the harness knows about the corpus while it resolves.
#[derive(Debug, Default)]
pub struct Index {
    /// Every source file in the corpus, corpus-relative.
    pub files: BTreeSet<String>,
    /// Symbols declared in each file.
    pub defs: BTreeMap<String, BTreeSet<String>>,
    /// Names each file makes available to importers, including names it re-exports. Filled in
    /// by the fixpoint in the driver, which is what makes re-export chains and barrels work.
    pub surface: BTreeMap<String, BTreeSet<String>>,
    /// Extra lookup tables a language needs (Go's module path, Java's package map, ...).
    pub extra: BTreeMap<String, String>,
}

impl Index {
    pub fn has(&self, path: &str) -> bool {
        self.files.contains(path)
    }

    pub fn defines(&self, path: &str, name: &str) -> bool {
        self.defs
            .get(path)
            .is_some_and(|names| names.contains(name))
    }

    pub fn surfaces(&self, path: &str, name: &str) -> bool {
        self.surface
            .get(path)
            .is_some_and(|names| names.contains(name))
    }
}

// ---------------------------------------------------------------------------
// the hand-authored side
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Truth {
    pub language: String,
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub notes: String,
    pub files: BTreeMap<String, TruthFile>,
}

#[derive(Debug, Deserialize)]
pub struct TruthFile {
    #[serde(default)]
    pub definitions: Vec<TruthDef>,
    #[serde(default)]
    pub exports: Vec<String>,
    #[serde(default)]
    pub imports: Vec<TruthImport>,
    /// Specifiers the hand annotation says are external, kept so the report can prove it did
    /// not invent an edge into the corpus for them.
    #[serde(default)]
    pub external: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TruthDef {
    pub k: String,
    pub n: String,
    #[serde(default)]
    pub t: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TruthImport {
    pub f: String,
    pub s: String,
    #[serde(default)]
    pub t: Vec<String>,
}

// ---------------------------------------------------------------------------
// scoring
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy)]
pub struct Score {
    pub tp: usize,
    pub fp: usize,
    pub miss: usize,
}

impl Score {
    pub fn add(&mut self, other: &Score) {
        self.tp += other.tp;
        self.fp += other.fp;
        self.miss += other.miss;
    }

    pub fn precision(&self) -> f64 {
        if self.tp + self.fp == 0 {
            1.0
        } else {
            self.tp as f64 / (self.tp + self.fp) as f64
        }
    }

    pub fn recall(&self) -> f64 {
        if self.tp + self.miss == 0 {
            1.0
        } else {
            self.tp as f64 / (self.tp + self.miss) as f64
        }
    }

    pub fn f1(&self) -> f64 {
        let p = self.precision();
        let r = self.recall();
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }
}

/// Precision and recall for the three relations, per language.
#[derive(Debug, Default)]
pub struct LangScore {
    pub definitions: Score,
    pub exports: Score,
    pub imports: Score,
    /// Recall broken down by the hand annotation's construct tags, which is how a miss becomes
    /// a named dealbreaker instead of a decimal.
    pub tags: BTreeMap<String, Score>,
    /// The misses and the surprises, verbatim, so the report is auditable.
    pub missing_defs: Vec<String>,
    pub extra_defs: Vec<String>,
    pub missing_edges: Vec<String>,
    pub extra_edges: Vec<String>,
    pub missing_exports: Vec<String>,
    pub extra_exports: Vec<String>,
    pub blind: BTreeMap<String, usize>,
    pub corpus_files: usize,
    pub corpus_bytes: usize,
}

/// One key per scored row. `\u{1}` separates the parts so the key is also the printable form
/// of a miss: the report lists misses verbatim, and a miss that prints as
/// `shapes.rs\u0001fn\u0001describe` is not something a reader can use.
pub fn key(parts: &[&str]) -> String {
    parts.join(" \u{2022} ")
}

fn compare(
    truth: &BTreeSet<String>,
    got: &BTreeSet<String>,
    miss_out: &mut Vec<String>,
    extra_out: &mut Vec<String>,
) -> Score {
    let mut score = Score::default();
    for t in truth {
        if got.contains(t) {
            score.tp += 1;
        } else {
            score.miss += 1;
            miss_out.push(t.clone());
        }
    }
    for g in got {
        if !truth.contains(g) {
            score.fp += 1;
            extra_out.push(g.clone());
        }
    }
    score
}

/// Score one language's extraction against its truth.
#[allow(clippy::type_complexity)]
pub fn score(
    truth: &Truth,
    got: &BTreeMap<String, (BTreeSet<Def>, BTreeSet<String>, BTreeSet<Edge>)>,
) -> LangScore {
    let mut out = LangScore::default();
    // Keyed by file plus the row, so a miss in one file cannot be cancelled by a hit in another.
    let mut want_defs: BTreeSet<String> = BTreeSet::new();
    let mut made_defs: BTreeSet<String> = BTreeSet::new();
    let mut want_exp: BTreeSet<String> = BTreeSet::new();
    let mut made_exp: BTreeSet<String> = BTreeSet::new();
    let mut want_imp: BTreeSet<String> = BTreeSet::new();
    let mut made_imp: BTreeSet<String> = BTreeSet::new();
    // tag -> expected items, and the items that were matched.
    let mut tag_want: BTreeMap<String, usize> = BTreeMap::new();
    let mut tag_hit: BTreeMap<String, usize> = BTreeMap::new();

    for (file, tf) in &truth.files {
        for d in &tf.definitions {
            want_defs.insert(key(&[file, &d.k, &d.n]));
        }
        for e in &tf.exports {
            want_exp.insert(key(&[file, e]));
        }
        for i in &tf.imports {
            want_imp.insert(key(&[file, &i.f, &i.s]));
        }
    }
    for (file, (defs, exports, edges)) in got {
        for d in defs {
            made_defs.insert(key(&[file, &d.kind, &d.name]));
        }
        for e in exports {
            made_exp.insert(key(&[file, e]));
        }
        for e in edges {
            made_imp.insert(key(&[file, &e.target, &e.symbol]));
        }
    }

    out.definitions = compare(
        &want_defs,
        &made_defs,
        &mut out.missing_defs,
        &mut out.extra_defs,
    );
    out.imports = compare(
        &want_imp,
        &made_imp,
        &mut out.missing_edges,
        &mut out.extra_edges,
    );
    out.exports = compare(
        &want_exp,
        &made_exp,
        &mut out.missing_exports,
        &mut out.extra_exports,
    );

    for (file, tf) in &truth.files {
        for d in &tf.definitions {
            let hit = made_defs.contains(&key(&[file, &d.k, &d.n]));
            for tag in &d.t {
                *tag_want.entry(tag.clone()).or_insert(0) += 1;
                if hit {
                    *tag_hit.entry(tag.clone()).or_insert(0) += 1;
                }
            }
        }
        for i in &tf.imports {
            let hit = made_imp.contains(&key(&[file, &i.f, &i.s]));
            for tag in &i.t {
                *tag_want.entry(format!("import:{tag}")).or_insert(0) += 1;
                if hit {
                    *tag_hit.entry(format!("import:{tag}")).or_insert(0) += 1;
                }
            }
        }
        for e in &tf.exports {
            *tag_want.entry("export".to_string()).or_insert(0) += 1;
            if made_exp.contains(&key(&[file, e])) {
                *tag_hit.entry("export".to_string()).or_insert(0) += 1;
            }
        }
    }

    for tag in tag_want.keys() {
        let want = tag_want[tag];
        let hit = tag_hit.get(tag).copied().unwrap_or(0);
        out.tags.insert(
            tag.clone(),
            Score {
                tp: hit,
                fp: 0,
                miss: want - hit,
            },
        );
    }

    out.corpus_files = truth.files.len();
    out
}

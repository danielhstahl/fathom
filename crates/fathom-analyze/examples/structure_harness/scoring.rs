//! Set scoring: hand-authored truth against what the extractor actually returned.
//!
//! Every relation is an exact set comparison, with no near-miss credit. That is a deliberate
//! harshness, inherited from spike `s1`: partial credit is where a harness goes soft, and the
//! questions ADR-004's budgets depend on are answerable only by an extractor that gets the name and
//! the file exactly right. "Close" is a `miss` and a `false positive`, both printed, both a bug.
//!
//! The one place leniency lives is `blind`: categories the annotation records as *not derivable from
//! a file* (a Python `getattr`, a Rust macro arm the extractor cannot expand). Those rows are
//! counted, listed in the report, and never charged against a floor — because charging them would
//! force the extractor to lie about scopes it honestly cannot enumerate, which is the exact failure
//! `synthetic_eligible` exists to prevent.

use crate::truth::Truth;
use fathom_analyze::structure::{CorpusStructure, synthetic::*};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone, Copy)]
pub struct Score {
    pub tp: usize,
    pub fp: usize,
    pub miss: usize,
}

impl Score {
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
        let (p, r) = (self.precision(), self.recall());
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }
}

pub struct LangScore {
    pub definitions: Score,
    pub exports: Score,
    pub imports: Score,
    pub supertypes: Score,
    /// Recall per annotation tag, so a regression that hits only `cfg_conditional` definitions is
    /// visible as itself rather than as a dip in an aggregate nobody looks at.
    pub tags: BTreeMap<String, Score>,
    pub corpus_files: usize,
    pub corpus_bytes: usize,
    pub missing_defs: Vec<String>,
    pub extra_defs: Vec<String>,
    pub missing_exports: Vec<String>,
    pub extra_exports: Vec<String>,
    pub missing_edges: Vec<String>,
    pub extra_edges: Vec<String>,
    pub missing_bases: Vec<String>,
    pub extra_bases: Vec<String>,
    pub missing_synthetic: Vec<String>,
    /// Truth rows whose declared relation kind (`trait` vs `base` vs `interface`) is not the kind the
    /// extractor named. Not scored — the relation is right and the label is a vocabulary question —
    /// but a growing list here means the two vocabularies are drifting.
    pub base_kind_mismatches: Vec<String>,
    /// Rows the annotation declares irreducible, and why. Reported, never scored.
    pub blind: BTreeMap<String, usize>,
    /// How many irreducible rows the extractor found anyway. A bonus, reported as one.
    pub blind_recovered: usize,
    /// The irreducible rows themselves, spelled out, so the report says which macro arm or
    /// monkeypatch each blind-spot count is made of.
    pub irreducible_rows: Vec<String>,
    /// The annotation's own provenance, echoed into the report so a reviewer can see which truth file
    /// produced which number.
    pub root: String,
    pub notes: String,
    pub bases_notes: String,
    pub synthetic_notes: String,
}

fn compare(
    expected: &BTreeSet<String>,
    found: &BTreeSet<String>,
) -> (Score, Vec<String>, Vec<String>) {
    let mut score = Score::default();
    let mut missing = Vec::new();
    let mut extra = Vec::new();
    for key in expected {
        if found.contains(key) {
            score.tp += 1;
        } else {
            score.miss += 1;
            missing.push(key.clone());
        }
    }
    for key in found {
        if !expected.contains(key) {
            score.fp += 1;
            extra.push(key.clone());
        }
    }
    (score, missing, extra)
}

pub fn score(
    annotation: &Truth,
    corpus: &CorpusStructure,
    irreducible: &BTreeSet<String>,
) -> LangScore {
    let mut expected_defs: BTreeSet<String> = BTreeSet::new();
    let mut expected_exports: BTreeSet<String> = BTreeSet::new();
    let mut expected_edges: BTreeSet<String> = BTreeSet::new();
    let mut expected_bases: BTreeSet<String> = BTreeSet::new();
    let mut expected_synthetic: BTreeSet<String> = BTreeSet::new();
    let mut tag_totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut blind: BTreeMap<String, usize> = BTreeMap::new();
    // Definitions the annotation declares irreducible, by file: an export whose name comes from one
    // of those is irreducible too, and scoring it twice would be scoring the same blind spot twice.
    let mut irreducible_names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut irreducible_rows: Vec<String> = Vec::new();
    let mut irreducible_keys: BTreeSet<String> = BTreeSet::new();

    for (path, file) in &annotation.files {
        for definition in &file.definitions {
            let hit_irreducible = definition.t.iter().find(|tag| irreducible.contains(*tag));
            if let Some(tag) = hit_irreducible {
                // Not a miss, and not scored as a row: the declaration is not in the file. The
                // extractor's answer to a macro arm, a monkeypatch or a decorator-written method is
                // to mark the scope unenumerable, which the `synthetic_eligible` mechanism checks.
                irreducible_names
                    .entry(path.clone())
                    .or_default()
                    .insert(definition.n.clone());
                let key = format!("{path} \u{2022} {} \u{2022} {}", definition.k, definition.n);
                irreducible_rows.push(format!("{key} [{tag}]"));
                irreducible_keys.insert(key);
                *blind
                    .entry(format!("irreducible_definition:{tag}"))
                    .or_default() += 1;
                continue;
            }
            expected_defs.insert(format!(
                "{path} \u{2022} {} \u{2022} {}",
                definition.k, definition.n
            ));
            for tag in &definition.t {
                *tag_totals.entry(tag.clone()).or_default() += 1;
            }
        }
        for export in &file.exports {
            if irreducible_names
                .get(path)
                .is_some_and(|names| names.contains(export))
            {
                *blind.entry("irreducible_export".to_string()).or_default() += 1;
                irreducible_rows.push(format!("{path} \u{2022} export \u{2022} {export}"));
                continue;
            }
            expected_exports.insert(format!("{path} \u{2022} {export}"));
        }
        for import in &file.imports {
            expected_edges.insert(format!(
                "{path} \u{2022} {} \u{2022} {}",
                import.f, import.s
            ));
            for tag in &import.t {
                *tag_totals.entry(format!("import:{tag}")).or_default() += 1;
            }
        }
        for base in &file.bases {
            expected_bases.insert(format!("{path} \u{2022} {} \u{2022} {}", base.n, base.b));
        }

        for scope in &file.synthetic {
            expected_synthetic.insert(format!("{path} \u{2022} {}", scope.scope));
            for tag in &scope.t {
                *tag_totals.entry(format!("synthetic:{tag}")).or_default() += 1;
            }
        }
        let _ = &file.external;
    }

    let mut found_defs: BTreeSet<String> = BTreeSet::new();
    let mut found_exports: BTreeSet<String> = BTreeSet::new();
    let mut found_edges: BTreeSet<String> = BTreeSet::new();
    let mut found_bases: BTreeSet<String> = BTreeSet::new();
    let mut found_base_kinds: BTreeMap<String, String> = BTreeMap::new();
    let mut found_synthetic: BTreeSet<String> = BTreeSet::new();
    let mut tag_hits: BTreeMap<String, Score> = BTreeMap::new();

    for record in &corpus.files {
        let Some(structure) = &record.structure else {
            continue;
        };
        let path = record.path.as_str();
        // `Symbol::name` is already the dotted path the annotation uses (`Point.new`), with
        // `container` carrying the same prefix separately for the graph's benefit. Prepending it here
        // would score `Point.Point.new` against `Point.new` and call a correct extractor wrong.
        for symbol in &structure.symbols {
            found_defs.insert(format!(
                "{path} \u{2022} {} \u{2022} {}",
                symbol.kind.as_str(),
                symbol.name
            ));
        }
        for export in &structure.exports {
            found_exports.insert(format!("{path} \u{2022} {export}"));
        }
        for edge in &structure.edges {
            // Only the two kinds that make a *claim* about a corpus file are scored. An `unresolved`
            // edge names something the extractor could not place; whether the annotation agrees is a
            // different question with a different answer, and mixing them would score a stdlib blind
            // spot as a bug.
            match edge.kind {
                fathom_analyze::structure::EdgeKind::Resolved
                | fathom_analyze::structure::EdgeKind::Wildcard => {
                    found_edges.insert(format!(
                        "{path} \u{2022} {} \u{2022} {}",
                        edge.target, edge.symbol
                    ));
                }
                fathom_analyze::structure::EdgeKind::Unresolved => {}
            }
        }
        for base in &structure.supertypes {
            found_bases.insert(format!(
                "{path} \u{2022} {} \u{2022} {}",
                base.of, base.base
            ));
            found_base_kinds.insert(
                format!("{path} \u{2022} {} \u{2022} {}", base.of, base.base),
                base.kind.as_str().to_string(),
            );
        }
        for scope in &structure.synthetic_scopes {
            found_synthetic.insert(format!("{path} \u{2022} {}", scope.scope));
        }
    }

    // Tag recall, computed the only way that is meaningful: a tag's row counts as hit when the exact
    // definition it annotates was found.
    for (path, file) in &annotation.files {
        for definition in &file.definitions {
            let key = format!("{path} \u{2022} {} \u{2022} {}", definition.k, definition.n);
            let hit = found_defs.contains(&key);
            for tag in &definition.t {
                let row = tag_hits.entry(tag.clone()).or_default();
                if hit {
                    row.tp += 1;
                } else {
                    row.miss += 1;
                }
            }
        }
    }

    // An extractor that found an irreducible row anyway is not wrong, and a harness that charged it
    // as a false positive would train the extractor to stop trying. `Shape.describe = lambda ...`
    // written into a class from `dynamic.py` is a definition this file can see even though no file
    // declares it: the row stays out of the scored set, and the recovery is counted separately.
    let mut blind_recovered = 0usize;
    for key in &irreducible_keys {
        if found_defs.remove(key) {
            blind_recovered += 1;
        }
    }

    // Tag recall for the two relations that carry tags besides definitions.
    for (path, file) in &annotation.files {
        for import in &file.imports {
            let key = format!("{path} \u{2022} {} \u{2022} {}", import.f, import.s);
            let hit = found_edges.contains(&key);
            for tag in &import.t {
                let row = tag_hits.entry(format!("import:{tag}")).or_default();
                if hit {
                    row.tp += 1;
                } else {
                    row.miss += 1;
                }
            }
        }
        for scope in &file.synthetic {
            let key = format!("{path} \u{2022} {}", scope.scope);
            let hit = found_synthetic.contains(&key);
            for tag in &scope.t {
                let row = tag_hits.entry(format!("synthetic:{tag}")).or_default();
                if hit {
                    row.tp += 1;
                } else {
                    row.miss += 1;
                }
            }
        }
    }

    let mut base_kind_mismatches = Vec::new();
    for (path, file) in &annotation.files {
        for base in &file.bases {
            let key = format!("{path} \u{2022} {} \u{2022} {}", base.n, base.b);
            if let (false, Some(found)) = (base.k.is_empty(), found_base_kinds.get(&key))
                && *found != base.k
            {
                base_kind_mismatches.push(format!(
                    "{key}: truth says {}, extractor says {found}",
                    base.k
                ));
            }
        }
    }

    let (definitions, missing_defs, extra_defs) = compare(&expected_defs, &found_defs);
    let (exports, missing_exports, extra_exports) = compare(&expected_exports, &found_exports);
    let (imports, missing_edges, extra_edges) = compare(&expected_edges, &found_edges);
    let (supertypes, missing_bases, extra_bases) = compare(&expected_bases, &found_bases);
    let (_synthetic, missing_synthetic, _extra_synthetic) =
        compare(&expected_synthetic, &found_synthetic);

    let mut tags = BTreeMap::new();
    for (tag, total) in &tag_totals {
        let mut row = tag_hits.get(tag).copied().unwrap_or_default();
        if row.tp == 0 && row.miss == 0 {
            // A tag on a non-definition row: counted against its own total so the report's tag table
            // adds up to the corpus, and never against a floor.
            row = Score {
                tp: *total,
                fp: 0,
                miss: 0,
            };
        }
        tags.insert(tag.clone(), row);
    }

    LangScore {
        definitions,
        exports,
        imports,
        supertypes,
        tags,
        corpus_files: 0,
        corpus_bytes: 0,
        missing_defs,
        extra_defs,
        missing_exports,
        extra_exports,
        missing_edges,
        extra_edges,
        missing_bases,
        extra_bases,
        missing_synthetic,
        blind_recovered,
        irreducible_rows,
        base_kind_mismatches,
        blind,
        root: annotation.root.clone().unwrap_or_default(),
        notes: annotation.notes.clone().unwrap_or_default(),
        bases_notes: annotation.bases_notes.clone().unwrap_or_default(),
        synthetic_notes: annotation.synthetic_notes.clone().unwrap_or_default(),
    }
}

/// The reason vocabulary, re-exported so the truth file's reason words can be checked against the
/// extractor's constants rather than against a comment.
#[allow(dead_code)]
pub const KNOWN_REASONS: &[&str] = &[
    MACRO_DEFINITION,
    MACRO_INVOCATION,
    DECORATED,
    MONKEYPATCH,
    DYNAMIC_METHOD,
    SYNTHESISED_ACCESSORS,
    WILDCARD_REEXPORT,
    AMBIENT_MERGE,
    UNKNOWN_SHAPE,
];

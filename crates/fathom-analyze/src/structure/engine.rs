//! The engine: what one language's extractor has to provide, the corpus index resolution reads,
//! and the bounded fixpoint that turns import statements into edges.
//!
//! The split is deliberate. A language module knows how its grammar spells a declaration, an import
//! and a supertype; the engine knows how a corpus is walked, how far resolution is allowed to go,
//! and how the result is ordered so it is byte-stable. Nothing in here knows what a `trait` is.
//!
//! Two rules hold the whole module honest:
//!
//! * **Nothing is dropped.** Every input file comes back as a [`FileStructure`], and one that could
//!   not be read says why. A dependency graph built by an extractor that loses files is a graph that
//!   is confidently wrong about the files it lost.
//! * **Nothing runs unbounded.** Resolution is iterative (s1 finding 6), and every iterative step in
//!   here is bounded by [`ExtractPolicy`] and reports how close it came to the bound.

#[allow(unused_imports)]
pub(crate) use crate::structure::model::LanguageSummary;
pub(crate) use crate::structure::model::{CorpusStructure, SourceFile, Summary};
pub(crate) use crate::structure::model::{Edge, EdgeKind};
use crate::structure::model::{
    FileLevel, FileStructure, Import, Language, Note, Resolution, Span, Structure, Supertype,
    SyntheticScope, UnavailableReason,
};
use crate::structure::policy::ExtractPolicy;
use crate::structure::util;
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language as TsLanguage, Node, Parser};

/// The separator inside an origin key. One character, never printable in a path or a symbol name.
pub(crate) const SEP: char = '\u{1}';

/// An import as the source states it, in the richer shape the resolvers need: `module` is the
/// module path the statement sits in (Rust's `self`/`super` anchors, Python's relative base) and
/// costs nothing to keep.
#[derive(Debug, Clone)]
pub(crate) struct RawImport {
    pub spec: String,
    pub symbol: String,
    pub form: &'static str,
    pub module: Vec<String>,
    pub reexport: bool,
    pub span: Span,
}

impl RawImport {
    pub(crate) fn new(
        spec: impl Into<String>,
        symbol: impl Into<String>,
        form: &'static str,
        span: Span,
    ) -> RawImport {
        RawImport {
            spec: spec.into(),
            symbol: symbol.into(),
            form,
            module: Vec::new(),
            reexport: false,
            span,
        }
    }
}

/// What one language's scan produced, before resolution.
#[derive(Debug, Default, Clone)]
pub(crate) struct Draft {
    pub symbols: Vec<crate::structure::model::Symbol>,
    pub imports: Vec<RawImport>,
    pub supertypes: Vec<Supertype>,
    pub notes: Vec<Note>,
    /// Scopes that cannot be enumerated, keyed by dotted scope name ("" is the file's own scope).
    pub synthetic: BTreeMap<String, Vec<String>>,
    /// The per-language side channel the resolvers and export rules read: a module path here, a
    /// visibility marker there. Keys are namespaced by the language module that writes them, and
    /// nothing outside that module interprets them.
    pub aux: BTreeMap<String, String>,
}

impl Draft {
    pub(crate) fn note(&mut self, kind: &str, detail: &str) {
        self.notes.push(Note {
            kind: kind.to_string(),
            detail: detail.to_string(),
        });
    }

    /// Mark a scope as one whose declaration list cannot be enumerated (s1 finding 1). From here on
    /// the scope is never reported as "declares nothing".
    pub(crate) fn mark_synthetic(&mut self, scope: &str, reason: &str) {
        let entry = self.synthetic.entry(scope.to_string()).or_default();
        if !entry.iter().any(|r| r == reason) {
            entry.push(reason.to_string());
            entry.sort();
        }
    }

    /// Mark the file's own top-level scope.
    pub(crate) fn mark_file_synthetic(&mut self, reason: &str) {
        self.mark_synthetic("", reason);
    }

    pub(crate) fn synthetic_scopes(&self) -> Vec<SyntheticScope> {
        self.synthetic
            .iter()
            .map(|(scope, reasons)| SyntheticScope {
                scope: scope.clone(),
                reasons: reasons.clone(),
            })
            .collect()
    }
}

/// What the engine and the language modules know about the corpus while they resolve.
#[derive(Debug, Default)]
pub(crate) struct Index {
    /// Every source file of this language in the corpus, corpus-relative.
    pub files: BTreeSet<String>,
    /// Symbols each file declares.
    pub defs: BTreeMap<String, BTreeSet<String>>,
    /// `file \u{1} symbol` → the `file \u{1} symbol` that actually *declares* it, filled in by the
    /// fixpoint as re-export hops become known. This is what makes a barrel a hop rather than a dead
    /// end (s1 finding 3: "re-export chains close after several passes").
    pub origin: BTreeMap<String, String>,
    /// Everything else a language needs: its module map, its package table, its derived export
    /// facts. Namespaced by the module that writes each key.
    pub extra: BTreeMap<String, String>,
}

impl Index {
    pub(crate) fn has(&self, path: &str) -> bool {
        self.files.contains(path)
    }

    pub(crate) fn defines(&self, path: &str, name: &str) -> bool {
        self.defs
            .get(path)
            .is_some_and(|names| names.contains(name))
    }

    /// Where a name published by `file` is actually declared. Defaults to the file itself, which is
    /// the common case and the one nobody bothers to publish.
    pub(crate) fn origin_of(&self, file: &str, symbol: &str) -> (String, String) {
        match self.origin.get(&origin_key(file, symbol)) {
            Some(value) => match value.split_once(SEP) {
                Some((f, s)) => (f.to_string(), s.to_string()),
                None => (value.clone(), symbol.to_string()),
            },
            None => (file.to_string(), symbol.to_string()),
        }
    }
}

pub(crate) fn origin_key(file: &str, symbol: &str) -> String {
    format!("{file}{SEP}{symbol}")
}

pub(crate) fn publish_origin(
    index: &mut Index,
    file: &str,
    symbol: &str,
    origin: &str,
    name: &str,
) {
    index
        .origin
        .insert(origin_key(file, symbol), format!("{origin}{SEP}{name}"));
}

/// One answer from [`follow`].
pub(crate) struct Chain {
    pub target: String,
    pub symbol: String,
    /// The file the answer was reached *through*, when that is not the declaring file.
    pub via: Option<String>,
    pub hops: usize,
    /// The chain was still going when it hit [`ExtractPolicy::max_resolution_hops`]: the answer is
    /// a prefix of the truth and the file says so.
    pub truncated: bool,
}

/// Follow a (file, symbol) pair to the file that declares it, bounded per s1 finding 6.
///
/// `None` means the symbol is not in this corpus at all, which is an answer
/// ([`EdgeKind::Unresolved`]) and not an absence.
pub(crate) fn follow(index: &Index, file: &str, symbol: &str, bound: usize) -> Option<Chain> {
    let bound = bound.max(1);
    let mut current_file = file.to_string();
    let mut current_symbol = symbol.to_string();
    for hop in 0..bound {
        if current_symbol == "*" || index.defines(&current_file, &current_symbol) {
            let via = (file != current_file).then(|| file.to_string());
            return Some(Chain {
                target: current_file,
                symbol: current_symbol,
                via,
                hops: hop + 1,
                truncated: false,
            });
        }
        let (next_file, next_symbol) = index.origin_of(&current_file, &current_symbol);
        if next_file == current_file && next_symbol == current_symbol {
            // Nothing more to learn: this file publishes the name and does not declare it, and no
            // further hop exists. That is a miss, not a truncated chain.
            return None;
        }
        current_file = next_file;
        current_symbol = next_symbol;
    }
    let via = (file != current_file).then(|| file.to_string());
    Some(Chain {
        target: current_file,
        symbol: current_symbol,
        via,
        hops: bound,
        truncated: true,
    })
}

/// The corpus as an export rule sees it.
pub(crate) struct CorpusView<'a> {
    pub drafts: &'a BTreeMap<String, Draft>,
    pub edges: &'a BTreeMap<String, Vec<Edge>>,
    pub index: &'a Index,
}

/// A language, as far as the engine is concerned. Every field is a plain `fn` pointer rather than a
/// trait object because there are three implementations, each compiled in or out as a unit.
pub(crate) struct Lang {
    pub language: Language,
    pub grammar: fn() -> TsLanguage,
    /// Read one file's tree into a [`Draft`].
    pub scan: fn(Node, &[u8], &str, &mut Draft, &ExtractPolicy),
    /// Seed corpus-wide lookup tables from one file's draft (run before the first pass).
    pub seed: fn(&str, &Draft, &mut Index),
    /// Turn one file's statements into edges, given what is known so far.
    pub resolve: fn(&str, &Draft, &mut Index, &ExtractPolicy, &mut usize) -> Vec<Edge>,
    /// Record what this file makes available to an importer, so a later pass can follow the hop.
    pub publish: fn(&str, &Draft, &[Edge], &mut Index),
    /// Derive whatever the export rule needs from the whole corpus (once, after resolution).
    /// Derive whatever the export rule needs from the whole corpus (once, after resolution).
    /// Returned rather than written into the index because the view that reads the index and the
    /// index being extended cannot both exist at once, and that is the borrow checker's point.
    pub prepare_exports: fn(&CorpusView) -> BTreeMap<String, String>,
    /// The names this file exports, per the language's own access rules.
    pub export_names: fn(&str, &CorpusView) -> BTreeSet<String>,
}

/// A file that parsed and scanned, waiting for resolution.
struct Scanned {
    path: String,
    language: Language,
    file_level: FileLevel,
    draft: Draft,
}

/// Extract a whole corpus.
///
/// Files with no grammar, a grammar this build was not compiled with, a hostile grammar or a size
/// above the policy all come back as records too — with `structure: None`, a stated reason and their
/// file-level facts filled in. That is the acceptance criterion "unknown or broken language yields
/// structure unavailable plus file-level analysis, never a lost episode", and it is why this
/// function returns a value per input rather than a `Result`.
pub fn extract_all(sources: &[SourceFile], policy: &ExtractPolicy) -> CorpusStructure {
    let mut scanned: Vec<Scanned> = Vec::new();
    let mut records: Vec<FileStructure> = Vec::new();

    for source in sources {
        match scan_one(source, policy) {
            ScannedOrRefused::Scanned(entry) => scanned.push(entry),
            ScannedOrRefused::Refused(record) => records.push(record),
        }
    }

    // Group by language: resolution is a within-language relation (`e3.1` has no cross-language
    // edges, and inventing them would be a different ticket).
    let mut by_language: BTreeMap<Language, BTreeMap<String, &Scanned>> = BTreeMap::new();
    for entry in &scanned {
        by_language
            .entry(entry.language)
            .or_default()
            .insert(entry.path.clone(), entry);
    }

    for (language, group) in by_language {
        let Some(lang) = lang_for(language) else {
            continue;
        };
        let drafts: BTreeMap<String, Draft> = group
            .iter()
            .map(|(path, entry)| (path.clone(), entry.draft.clone()))
            .collect();
        let mut index = Index::default();
        for path in drafts.keys() {
            index.files.insert(path.clone());
        }
        for (path, draft) in &drafts {
            index.defs.insert(
                path.clone(),
                draft.symbols.iter().map(|s| s.name.clone()).collect(),
            );
            (lang.seed)(path, draft, &mut index);
        }

        // The bounded fixpoint. Each pass resolves every file against what the previous pass made
        // known, then publishes what that file now makes available; a re-export chain of length n
        // closes on pass n, and nothing is allowed to take longer than the bound.
        let mut edges: BTreeMap<String, Vec<Edge>> = BTreeMap::new();
        let mut passes_used: BTreeMap<String, usize> =
            drafts.keys().map(|path| (path.clone(), 0usize)).collect();
        let mut quiescent = true;
        let mut last_pass = 0usize;
        let bound = policy.max_resolution_passes.max(1);
        for pass in 1..=bound {
            let mut changed = false;
            for path in drafts.keys() {
                let draft = &drafts[path];
                let mut ignored = 0usize;
                let mut found = (lang.resolve)(path, draft, &mut index, policy, &mut ignored);
                // A module that re-exports its own file is not a dependency on anything.
                found.retain(|edge| {
                    edge.target.as_str() != path.as_str() || edge.kind == EdgeKind::Unresolved
                });
                (lang.publish)(path, draft, &found, &mut index);
                let before = edges.insert(path.clone(), found.clone());
                if before.map(|old| old != found).unwrap_or(true) {
                    changed = true;
                    *passes_used.entry(path.clone()).or_insert(0) = pass;
                }
            }
            last_pass = pass;
            quiescent = !changed;
            if quiescent {
                break;
            }
        }

        // One final resolve, with no publishing, so the numbers reported are the answer rather than
        // the last intermediate state, and so the hop-bound count reflects the answer the caller
        // receives.
        let mut final_edges: BTreeMap<String, Vec<Edge>> = BTreeMap::new();
        let mut truncated: BTreeMap<String, usize> = BTreeMap::new();
        for path in drafts.keys() {
            let draft = &drafts[path];
            let mut truncated_here = 0usize;
            let mut found = (lang.resolve)(path, draft, &mut index, policy, &mut truncated_here);
            found.retain(|edge| {
                edge.target.as_str() != path.as_str() || edge.kind == EdgeKind::Unresolved
            });
            found.sort_by(|a, b| {
                (&a.target, &a.symbol, a.kind, &a.specifier).cmp(&(
                    &b.target,
                    &b.symbol,
                    b.kind,
                    &b.specifier,
                ))
            });
            final_edges.insert(path.clone(), found);
            truncated.insert(path.clone(), truncated_here);
        }
        edges = final_edges;

        let derived = (lang.prepare_exports)(&CorpusView {
            drafts: &drafts,
            edges: &edges,
            index: &index,
        });
        for (key, value) in derived {
            index.extra.insert(key, value);
        }
        let view = CorpusView {
            drafts: &drafts,
            edges: &edges,
            index: &index,
        };

        for (path, entry) in &group {
            let exported = (lang.export_names)(path, &view);
            let used = *passes_used.get(path).unwrap_or(&0);
            let structure = assemble(
                &entry.draft,
                &exported,
                edges.get(path).cloned().unwrap_or_default(),
                &entry.supertypes_resolved(),
                Resolution {
                    passes_used: used,
                    converged: per_file_convergence(used, last_pass, quiescent, bound),
                    bound,
                    truncated_chains: *truncated.get(path).unwrap_or(&0),
                },
            );
            records.push(FileStructure {
                path: path.clone(),
                language,
                file_level: entry.file_level.clone(),
                structure: Some(structure),
                reason: None,
            });
        }
    }

    records.sort_by(|a, b| a.path.cmp(&b.path));
    let summary = summarise(&records, policy);
    CorpusStructure {
        policy: policy.clone(),
        files: records,
        summary,
    }
}

/// Whether *this file* settled inside the bound.
///
/// Corpus-wide convergence is the cheap and wrong version: it says a file did not converge because
/// some *other* file was still changing when the bound ran out, and says nothing at all about the
/// normal case where a quiet pass proved the whole corpus stable. Per file, the honest reading is
/// "did this file have a pass in which it stopped changing". A file that never changed converged on
/// the spot; a file that changed on the final allowed pass never got the quiet pass that would prove
/// it settled, which is what `converged: false` means and why the report names the file rather than
/// shrugging.
fn per_file_convergence(
    last_change: usize,
    last_pass: usize,
    quiescent: bool,
    bound: usize,
) -> bool {
    quiescent || last_change < last_pass || last_change < bound.min(last_pass)
}

/// Resolve a draft's supertypes against the index. Lives on `Scanned` so the call site reads as an
/// accessor; see the impl.
impl Scanned {
    fn supertypes_resolved(&self) -> Vec<Supertype> {
        // Supertype resolution is a same-corpus lookup only: a base declared in this file is
        // resolved to this file, and everything else stays `None` rather than guessing a package.
        // Cross-file base resolution is `e3.2`'s job (it has the whole module graph); `e3.1`'s
        // extractor is per-file by contract, so this is the honest subset.
        self.draft
            .supertypes
            .iter()
            .map(|supertype| Supertype {
                resolved: if self
                    .draft
                    .symbols
                    .iter()
                    .any(|symbol| symbol.name == supertype.base)
                {
                    Some(self.path.clone())
                } else {
                    supertype.resolved.clone()
                },
                ..supertype.clone()
            })
            .collect()
    }
}

/// Scan one file, or say why it cannot be scanned.
fn scan_one(source: &SourceFile, policy: &ExtractPolicy) -> ScannedOrRefused {
    let language = Language::detect(source.path);
    let mut file_level = FileLevel::measure(source.path, source.text);
    let refused = |file_level: &FileLevel, reason: UnavailableReason| {
        ScannedOrRefused::Refused(FileStructure {
            path: source.path.to_string(),
            language,
            file_level: file_level.clone(),
            structure: None,
            reason: Some(reason),
        })
    };
    if source.text.len() > policy.max_file_bytes {
        file_level.refused = true;
        return refused(
            &file_level,
            UnavailableReason::TooLarge {
                limit: policy.max_file_bytes,
            },
        );
    }
    if std::str::from_utf8(source.text).is_err() {
        file_level.refused = true;
        return refused(&file_level, UnavailableReason::NotUtf8);
    }
    if !language.is_known() {
        return refused(&file_level, UnavailableReason::LanguageUnknown);
    }
    let Some(lang) = lang_for(language) else {
        // Distinguish "this binary has no grammar" from "no extractor has been written yet": the
        // first is a rebuild, the second is `e3.1b`, and nobody reading a report can tell them
        // apart otherwise.
        return refused(
            &file_level,
            if grammar_compiled(language) {
                UnavailableReason::ExtractorNotShipped
            } else {
                UnavailableReason::GrammarNotCompiled {
                    feature: language.feature().unwrap_or("langs-all").to_string(),
                }
            },
        );
    };
    let mut parser = Parser::new();
    if parser.set_language(&(lang.grammar)()).is_err() {
        file_level.refused = true;
        return refused(&file_level, UnavailableReason::Parser);
    }
    let Some(tree) = parser.parse(source.text, None) else {
        file_level.refused = true;
        return refused(&file_level, UnavailableReason::Parser);
    };
    let root = tree.root_node();
    file_level.parse_errors = error_nodes(&root, policy.max_complexity_nodes);
    let (error_bytes, _) = util::error_share(&root, policy.max_complexity_nodes.max(4096));
    if error_bytes * 2 > source.text.len() {
        // More error than code: a truncated file, a binary file, or the wrong extension. The
        // structural half is withheld and the file-level half is not, which is the difference
        // between "unknown" and "lost".
        file_level.refused = true;
        return refused(
            &file_level,
            UnavailableReason::ParseFailed {
                errors: file_level.parse_errors,
            },
        );
    }
    let mut draft = Draft::default();
    (lang.scan)(root, source.text, source.path, &mut draft, policy);
    ScannedOrRefused::Scanned(Scanned {
        path: source.path.to_string(),
        language,
        file_level,
        draft,
    })
}

enum ScannedOrRefused {
    Scanned(Scanned),
    Refused(FileStructure),
}

/// Whether the grammar crate for `language` is linked into this build (ADR-002 §6.5).
pub(crate) fn grammar_compiled(language: Language) -> bool {
    match language {
        Language::Rust => cfg!(feature = "lang-rust"),
        Language::Python => cfg!(feature = "lang-python"),
        Language::TypeScript => cfg!(feature = "lang-typescript"),
        Language::Go => cfg!(feature = "lang-go"),
        Language::Ruby => cfg!(feature = "lang-ruby"),
        Language::Java => cfg!(feature = "lang-java"),
        Language::Unknown => false,
    }
}

/// Count ERROR/MISSING nodes, bounded.
fn error_nodes(root: &Node, budget: usize) -> usize {
    let mut count = 0usize;
    let mut visited = 0usize;
    let mut stack = vec![*root];
    while let Some(current) = stack.pop() {
        if visited >= budget {
            break;
        }
        visited += 1;
        if current.is_error() || current.is_missing() {
            count += 1;
        }
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            stack.push(child);
        }
    }
    count
}

/// Turn a draft plus its resolved edges into the stored half of a file's record. Everything is
/// sorted here, once, so the bytes of a stored row depend on the input and on nothing else.
fn assemble(
    draft: &Draft,
    exported: &BTreeSet<String>,
    edges: Vec<Edge>,
    supertypes: &[Supertype],
    resolution: Resolution,
) -> Structure {
    let mut symbols = draft.symbols.clone();
    for symbol in &mut symbols {
        symbol.exported = exported.contains(&symbol.name);
    }
    symbols.sort_by(|a, b| {
        (a.span.start_byte, &a.name, a.kind).cmp(&(b.span.start_byte, &b.name, b.kind))
    });
    let mut imports: Vec<Import> = draft
        .imports
        .iter()
        .map(|raw| Import {
            specifier: raw.spec.clone(),
            symbol: raw.symbol.clone(),
            form: raw.form.to_string(),
            reexport: raw.reexport,
            span: raw.span,
        })
        .collect();
    imports.sort_by(|a, b| {
        (a.span.start_byte, &a.specifier, &a.symbol).cmp(&(
            b.span.start_byte,
            &b.specifier,
            &b.symbol,
        ))
    });
    let mut edges = edges;
    edges.sort_by(|a, b| {
        (&a.target, &a.symbol, a.kind, &a.specifier).cmp(&(
            &b.target,
            &b.symbol,
            b.kind,
            &b.specifier,
        ))
    });
    let mut supertypes = supertypes.to_vec();
    supertypes.sort_by(|a, b| (&a.of, &a.base, a.kind).cmp(&(&b.of, &b.base, b.kind)));
    let mut notes = draft.notes.clone();
    notes.sort_by(|a, b| (&a.kind, &a.detail).cmp(&(&b.kind, &b.detail)));
    Structure {
        symbols,
        exports: exported.iter().cloned().collect(),
        imports,
        edges,
        supertypes,
        synthetic_scopes: draft.synthetic_scopes(),
        notes,
        resolution,
    }
}

fn summarise(records: &[FileStructure], _policy: &ExtractPolicy) -> Summary {
    let mut summary = Summary::default();
    for record in records {
        summary.files += 1;
        let entry = summary
            .by_language
            .entry(record.language.as_str().to_string())
            .or_default();
        entry.files += 1;
        let Some(structure) = &record.structure else {
            summary.unavailable += 1;
            entry.unavailable += 1;
            continue;
        };
        summary.extracted += 1;
        entry.extracted += 1;
        summary.symbols += structure.symbols.len();
        summary.exports += structure.exports.len();
        summary.supertypes += structure.supertypes.len();
        summary.synthetic_scopes += structure.synthetic_scopes.len();
        summary.passes_used_max = summary
            .passes_used_max
            .max(structure.resolution.passes_used);
        if !structure.resolution.converged {
            summary.non_converged += 1;
        }
        summary.truncated_chains += structure.resolution.truncated_chains;
        entry.symbols += structure.symbols.len();
        entry.exports += structure.exports.len();
        entry.supertypes += structure.supertypes.len();
        entry.synthetic_scopes += structure.synthetic_scopes.len();
        for edge in &structure.edges {
            match edge.kind {
                EdgeKind::Resolved => {
                    summary.edges_resolved += 1;
                    entry.edges_resolved += 1;
                }
                EdgeKind::Wildcard => {
                    summary.edges_wildcard += 1;
                    entry.edges_wildcard += 1;
                }
                EdgeKind::Unresolved => {
                    summary.edges_unresolved += 1;
                    entry.edges_unresolved += 1;
                }
            }
        }
    }
    summary
}

/// The languages this build can extract.
//
// Built by pushing rather than by a `vec![]` literal because each entry is behind its own
// ADR-002 §6.5 feature gate, and a literal would have to be re-written per configuration.
#[allow(clippy::vec_init_then_push)]
pub(crate) fn langs() -> Vec<&'static Lang> {
    let mut out: Vec<&'static Lang> = Vec::new();
    #[cfg(feature = "lang-rust")]
    out.push(&crate::structure::rustlang::LANG);
    #[cfg(feature = "lang-python")]
    out.push(&crate::structure::pythonlang::LANG);
    #[cfg(feature = "lang-typescript")]
    out.push(&crate::structure::tslang::LANG);
    out
}

pub(crate) fn lang_for(language: Language) -> Option<&'static Lang> {
    langs().into_iter().find(|lang| lang.language == language)
}

//! `e3.1` (`reco-zoz`) — the per-file structural model, for the three languages `e0.12` put first.
//!
//! ```text
//! source text in, structure out, or a stated reason there is none
//! ```
//!
//! Spike `s1` measured what six tree-sitter grammars can say
//! ([`docs/concepts/structure-extraction.md`](../../../../docs/concepts/structure-extraction.md))
//! and the checkpoint that read it
//! ([`.plan/reconciliation-e0.12.md`](../../../../.plan/reconciliation-e0.12.md)) split the work:
//! this ticket is **Rust, Python and TypeScript plus the harness**, `e3.1b` (`reco-dlp.1`) is the
//! Go, Ruby and Java tail plus the scan-policy knobs. What follows is what `s1` said this module had
//! to get right, as mechanisms rather than as findings:
//!
//! 1. **`synthetic_eligible` is a state, not an absence.** Every language has declarations only the
//!    running program knows — a `macro_rules!` expansion, a `@dataclass` generated `__init__`, a
//!    decorated class, a barrel `export *`. A scope where that can happen is marked
//!    ([`model::SyntheticScope`]) and its declaration list is then never reported as complete;
//!    [`model::Structure::may_have_shrunk`] is the sentence `e3.7` prints instead of "removed".
//! 2. **A wildcard dependency is its own edge kind.** Star import, `pub use *`, barrel `export *`,
//!    whole-module import, side-effect import: "depends on that unit, contents unknown"
//!    ([`model::EdgeKind::Wildcard`]). Stored as nothing, *who depends on `models.py`?* answers
//!    "nobody", which is worse than unknown — see [`query::dependents_of`].
//! 3. **Resolution is iterative and bounded.** Re-export chains close after several passes, the
//!    number of passes is [`ExtractPolicy::max_resolution_passes`], and every file reports how many
//!    it used and whether the bound stopped it ([`model::Resolution`]). It is the only super-linear
//!    step in extraction, which is why it is a config key and not a constant.
//! 4. **A file that cannot be read says so.** Unknown language, grammar not compiled in, oversized
//!    file, truncated file: each is a [`model::UnavailableReason`] beside the file-level facts that
//!    did not need a grammar. Nothing is dropped, because a dropped file is an invisible dependency.
//!
//! # What is *in* the model
//!
//! Per file: definitions with span, kind, visibility and complexity proxies; the exported set, per
//! the language's own access rules; imports as written *and* their resolved edges; implemented
//! interfaces/traits and inherited bases; the cannot-enumerate marks; and the resolution ledger.
//!
//! # Determinism
//!
//! Everything is ordered by a key that depends only on the input, so identical input produces
//! byte-identical output. [`canonical_json`] is the byte-identity contract the snapshot test checks,
//! and it is what lets `e3.2` store a row per revision and diff two of them.
//!
//! # Where the corpus and the numbers live
//!
//! `crates/fathom-analyze/structure/corpus/<language>/` is the corpus inherited from `s1` — same
//! files, same hand-authored `truth.json`, extended with the two annotations only a shipped
//! extractor needs (`bases` and `synthetic`) — and `crates/fathom-analyze/structure/thresholds.json`
//! holds the floors, which are `s1`'s, never lower. The harness is
//! `crates/fathom-analyze/examples/structure_harness.rs`; its committed report is
//! `benchmarks/reports/e3-1-structure-core.json` and `--check` re-measures it in CI.

mod model;
mod policy;
mod query;

#[cfg(feature = "lang-go")]
mod golang;
#[cfg(feature = "lang-java")]
mod javalang;
#[cfg(feature = "lang-python")]
mod pythonlang;
#[cfg(feature = "lang-ruby")]
mod rubylang;
#[cfg(feature = "lang-rust")]
mod rustlang;
#[cfg(feature = "lang-typescript")]
mod tslang;

/// The extractor proper: the corpus walk, the bounded fixpoint and the language dispatch table.
///
/// It exists only when at least one grammar is linked, because `tree_sitter` itself is an optional
/// dependency (ADR-002 §6.5: each grammar is a C build, and the six together are the difference
/// between a thirty-second and a four-minute laptop build). A build with none of them still gets the
/// model, the policy, the queries and a stated reason per file — see [`extract`]'s fallback.
#[cfg(any(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-go",
    feature = "lang-ruby",
    feature = "lang-java"
))]
pub(crate) mod engine;
/// Grammar helpers, gated for the same reason as [`engine`].
#[cfg(any(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-go",
    feature = "lang-ruby",
    feature = "lang-java"
))]
pub(crate) mod util;

pub use crate::structure::model::synthetic;
pub use crate::structure::model::{
    Complexity, CorpusStructure, Dependents, Edge, EdgeKind, FileLevel, FileStructure, Granularity,
    Import, Language, LanguageSummary, Note, Resolution, SourceFile, Span, Structure, Summary,
    Supertype, SupertypeKind, Symbol, SymbolKind, SyntheticScope, UnavailableReason, Visibility,
};
pub use crate::structure::policy::{
    DEFAULT_JAVA_SAME_PACKAGE_SCAN, DEFAULT_MAX_COMPLEXITY_NODES, DEFAULT_MAX_FILE_BYTES,
    DEFAULT_MAX_RESOLUTION_HOPS, DEFAULT_MAX_RESOLUTION_PASSES, DEFAULT_RUBY_MAX_BYTES_PER_RUN,
    DEFAULT_RUBY_MAX_FILES_PER_RUN, DEFAULT_RUBY_PRIORITY, ExtractPolicy, ScanBudget, ScanScope,
};
pub use crate::structure::query::Loads;
pub use crate::structure::query::{declaring_files, dependents_of, loads};

/// The version of *this* structural shape.
///
/// ADR-004 §5 keys a stored analysis row on the analyzer's own version, and this integer is that
/// version for the structure analyzer: a change to what a `Symbol` carries, to a normalisation rule,
/// or to a language module's export rule has to bump it, because every stored row after the bump
/// means something different from every row before it.
///
/// **Version 2** is `e3.1b` (`reco-dlp.1`): every `Edge` gained `granularity` and `package` (Go's
/// dependency is a package, and a stored edge that cannot say so is a file-to-file claim nobody
/// made), every `Structure` gained `package` (a Go package clause, a Java package declaration, a
/// `module-info.java` surface), and `UnavailableReason` gained `scan_budget`. A version-1 row is not
/// wrong — it simply cannot answer "was this edge a package edge?", which is exactly the question a
/// reader must be able to ask of a stored row before re-using it.
pub const SCHEMA_VERSION: u32 = 2;

/// Extract a corpus: one record per input file, always.
///
/// This is the function `e3.2`'s graph builder and `e3.7`'s API-surface diff will call. It is a pure
/// function of (files, policy) — no filesystem, no clock, no environment — which is what makes it
/// usable inside a cache key and inside a property test.
#[cfg(any(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-go",
    feature = "lang-ruby",
    feature = "lang-java"
))]
#[must_use]
pub fn extract(sources: &[SourceFile], policy: &ExtractPolicy) -> CorpusStructure {
    engine::extract_all(sources, policy)
}

/// The no-grammar build's answer: every file measured, every file told the truth about.
///
/// A `--no-default-features --features minimal` binary is a legitimate shipped binary (CI builds it,
/// and ADR-002 §6.5 exists so it can be), so extraction has to *exist* there and say "this build has
/// no grammar" per file rather than fail to compile or return nothing. The file-level half — size,
/// line shape, detected language — costs no grammar and is still delivered.
#[cfg(not(any(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-go",
    feature = "lang-ruby",
    feature = "lang-java"
)))]
#[must_use]
pub fn extract(sources: &[SourceFile], policy: &ExtractPolicy) -> CorpusStructure {
    let mut files: Vec<FileStructure> = Vec::with_capacity(sources.len());
    let mut summary = Summary::default();
    for source in sources {
        let mut file_level = FileLevel::measure(source.path, source.text);
        let language = file_level.language;
        let reason = if source.text.len() > policy.max_file_bytes {
            file_level.refused = true;
            UnavailableReason::TooLarge {
                limit: policy.max_file_bytes,
            }
        } else if std::str::from_utf8(source.text).is_err() {
            file_level.refused = true;
            UnavailableReason::NotUtf8
        } else if !language.is_known() {
            UnavailableReason::LanguageUnknown
        } else {
            UnavailableReason::GrammarNotCompiled {
                feature: language.feature().unwrap_or("langs-all").to_string(),
            }
        };
        summary.unavailable += 1;
        summary.files += 1;
        summary
            .by_language
            .entry(language.as_str().to_string())
            .or_default()
            .files += 1;
        summary
            .by_language
            .entry(language.as_str().to_string())
            .or_default()
            .unavailable += 1;
        files.push(FileStructure {
            path: source.path.to_string(),
            language,
            file_level,
            structure: None,
            reason: Some(reason),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    CorpusStructure {
        policy: policy.clone(),
        files,
        summary,
    }
}

/// Extract one file with no corpus around it. Resolution has nothing to resolve against, so every
/// import comes back `unresolved` or `wildcard` — which is the honest answer, and the shape a
/// single-file review path needs.
#[must_use]
pub fn extract_one(path: &str, text: &[u8], policy: &ExtractPolicy) -> FileStructure {
    let corpus = extract(&[SourceFile::new(path, text)], policy);
    match corpus.files.into_iter().next() {
        Some(record) => record,
        // `extract_all` returns one record per input file; an empty result would be a bug, and the
        // bug is stated rather than unwrapped.
        None => FileStructure {
            path: path.to_string(),
            language: Language::detect(path),
            file_level: FileLevel {
                path: path.to_string(),
                language: Language::detect(path),
                bytes: text.len(),
                newlines: 0,
                blank_lines: 0,
                code_lines: 0,
                parse_errors: 0,
                refused: true,
            },
            structure: None,
            reason: Some(UnavailableReason::Parser),
        },
    }
}

/// The extraction run as bytes: the byte-identity contract.
///
/// Two runs over identical input produce identical bytes, in any order of the input list, on any
/// machine. The snapshot test compares this against a checked-in golden file, and `e3.2`'s store keys
/// rows on a digest of exactly this shape.
pub fn canonical_json(corpus: &CorpusStructure) -> Vec<u8> {
    let document = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "policy": corpus.policy.to_json(),
        "summary": corpus.summary,
        "files": corpus.files,
    });
    let mut text = serde_json::to_string_pretty(&document)
        .unwrap_or_else(|_| "{}".to_string())
        .into_bytes();
    text.push(b'\n');
    text
}

/// Which of the six grammars this build can actually extract, as the shipped spellings.
///
/// A binary built with `--no-default-features` returns an empty list, and a surface reading this can
/// say "structure is unavailable in this build" before it parses anything rather than after.
#[must_use]
pub fn available_languages() -> Vec<&'static str> {
    #[cfg(any(
        feature = "lang-rust",
        feature = "lang-python",
        feature = "lang-typescript",
        feature = "lang-go",
        feature = "lang-ruby",
        feature = "lang-java"
    ))]
    {
        engine::langs()
            .into_iter()
            .map(|lang| lang.language.as_str())
            .collect()
    }
    #[cfg(not(any(
        feature = "lang-rust",
        feature = "lang-python",
        feature = "lang-typescript",
        feature = "lang-go",
        feature = "lang-ruby",
        feature = "lang-java"
    )))]
    {
        Vec::new()
    }
}

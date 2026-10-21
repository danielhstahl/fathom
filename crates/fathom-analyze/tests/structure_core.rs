//! The structural extractor core (`e3.1`, `reco-zoz`): the acceptance criteria as tests.
//!
//! The corpus harness (`crates/fathom-analyze/examples/structure_harness.rs`) owns *accuracy* — the
//! set comparison against hand-authored truth, and the four mechanism checks. This file owns the
//! properties that a harness with a `--check` flag cannot own:
//!
//! * **byte-identical output** (AC 6), pinned by a golden file, because "deterministic enough to
//!   store and diff" is a claim about bytes and nothing else;
//! * **the fallback battery** (AC 2), on inputs the corpus deliberately does not contain;
//! * **the model's promises** — a synthetic scope never asserts emptiness, a wildcard edge is never
//!   reported as a named dependency, an unresolved edge is never silently dropped;
//! * **the policy surface** (AC 5), read out of a `fathom.toml` in the shape the repository will
//!   actually write.
//!
//! Accuracy on the corpus is *not* duplicated here, on purpose. Two copies of a scoring rule drift,
//! and the second one always goes quiet first.
//!
//! ```text
//! FATHOM_GOLDEN=write cargo test -p fathom-analyze --features lang-rust,lang-python,lang-typescript --test structure_core
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use fathom_analyze::structure::{self, ExtractPolicy, Language, SourceFile, SymbolKind, synthetic};
use std::path::{Path, PathBuf};

/// The corpus, in the same order the harness walks it.
const LANGUAGES: [(&str, &[&str]); 3] = [
    ("rust", &["rs"]),
    ("python", &["py"]),
    ("typescript", &["ts"]),
];

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("structure")
        .join("corpus")
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("structure")
        .join("core-canonical.json")
}

fn bless() -> bool {
    std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write")
}

/// Every corpus file of one language, corpus-relative and `/`-separated — the same spelling the
/// harness and the truth files use, because the extractor derives module paths from the path and a
/// prefixed path is a different module.
fn corpus_sources(language: &str, extensions: &[&str]) -> Vec<(String, Vec<u8>)> {
    let root = corpus_root().join(language);
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.clone()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("the corpus directory is readable") {
            let entry = entry.expect("a readable directory entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(&root)
                .expect("a file inside the corpus")
                .to_string_lossy()
                .replace('\\', "/");
            if relative == "truth.json" {
                continue;
            }
            if extensions
                .iter()
                .any(|extension| relative.ends_with(&format!(".{extension}")))
            {
                out.push((
                    relative,
                    std::fs::read(&path).expect("a readable corpus file"),
                ));
            }
        }
    }
    out.sort();
    out
}

/// The three languages in one call. Safe because the extractor builds one index per language group:
/// a `.rs` file and a `.py` file cannot be each other's module.
fn all_corpus() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for (language, extensions) in LANGUAGES {
        out.extend(corpus_sources(language, extensions));
    }
    out
}

fn extract_all(
    sources: &[(String, Vec<u8>)],
    policy: &ExtractPolicy,
) -> structure::CorpusStructure {
    let borrowed: Vec<SourceFile> = sources
        .iter()
        .map(|(path, body)| SourceFile::new(path, body))
        .collect();
    structure::extract(&borrowed, policy)
}

// ---------------------------------------------------------------------------
// AC 6 — deterministic output, in the strict sense: the same bytes
// ---------------------------------------------------------------------------

#[test]
fn extraction_is_byte_identical_across_runs_and_orders_deterministically() {
    let sources = all_corpus();
    let policy = ExtractPolicy::default();
    let first = structure::canonical_json(&extract_all(&sources, &policy));
    let second = structure::canonical_json(&extract_all(&sources, &policy));
    assert_eq!(
        first, second,
        "two extractions of the same bytes differ; nothing downstream may diff them"
    );
    // And a shuffled input order must not move a byte: the ordering is the extractor's contract, not
    // the caller's courtesy.
    let mut shuffled = sources.clone();
    shuffled.rotate_left(sources.len() / 3);
    let shuffled = structure::canonical_json(&extract_all(&shuffled, &policy));
    assert_eq!(
        first, shuffled,
        "output depends on the order the files were handed over"
    );
}

#[test]
fn the_canonical_output_matches_the_golden_bytes() {
    let sources = all_corpus();
    let bytes = structure::canonical_json(&extract_all(&sources, &ExtractPolicy::default()));
    let path = golden_path();
    if bless() {
        std::fs::create_dir_all(path.parent().expect("a golden directory"))
            .expect("create golden dir");
        std::fs::write(&path, &bytes).expect("write golden");
        println!("blessed {}", path.display());
        return;
    }
    let committed = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "{} is missing ({error}); regenerate it with `FATHOM_GOLDEN=write cargo test -p \
             fathom-analyze --features lang-rust,lang-python,lang-typescript --test structure_core`",
            path.display()
        )
    });
    assert_eq!(
        committed.len(),
        bytes.len(),
        "the canonical output changed size; diff it with the harness's --snapshot"
    );
    assert_eq!(
        committed, bytes,
        "the canonical output changed. If the change is intended, bless the golden and say why in \
         the commit message — every byte here is a claim `e3.2` will build a stored graph on."
    );
}

// ---------------------------------------------------------------------------
// AC 2 — unknown or broken input is stated, never lost
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_language_returns_structure_unavailable_with_file_level_numbers() {
    const PROSE: &[u8] = b"prose, not code, 3 lines\nof it\nand one more\n";
    let record =
        structure::extract_one("notes/README.unknown-ext", PROSE, &ExtractPolicy::default());
    assert!(
        record.structure.is_none(),
        "an unknown language must not fabricate structure"
    );
    assert_eq!(record.language, Language::Unknown);
    let reason = record.reason.clone().expect("a reason must be stated");
    assert_eq!(reason, structure::UnavailableReason::LanguageUnknown);
    assert!(
        reason.sentence().starts_with("structure unavailable"),
        "the sentence is what a surface prints: {reason:?}"
    );
    assert_eq!(reason.as_str(), "language_unknown");
    // The file-level half is what makes "unavailable" different from "absent".
    assert_eq!(record.file_level.bytes, PROSE.len());
    assert_eq!(record.file_level.newlines, 3);
    assert!(!record.file_level.refused);
}

#[test]
fn a_file_over_the_size_limit_is_refused_with_its_limit_in_the_reason() {
    let policy = ExtractPolicy {
        max_file_bytes: 32,
        ..ExtractPolicy::default()
    };
    let body = vec![b'x'; 64];
    let record = structure::extract_one("big.rs", &body, &policy);
    assert!(record.structure.is_none());
    assert_eq!(
        record.reason,
        Some(structure::UnavailableReason::TooLarge { limit: 32 })
    );
    assert!(
        record.file_level.refused,
        "a refused file must say it was refused"
    );
    assert_eq!(
        record.file_level.bytes, 64,
        "the measurement survives the refusal"
    );
}

#[test]
fn a_file_that_is_not_utf8_is_refused_rather_than_lossily_decoded() {
    let record = structure::extract_one(
        "binary.rs",
        b"\xff\xfe\x00fn x(){}",
        &ExtractPolicy::default(),
    );
    assert!(record.structure.is_none());
    assert_eq!(record.reason, Some(structure::UnavailableReason::NotUtf8));
    assert!(record.file_level.refused);
}

#[test]
fn a_broken_file_of_a_known_language_is_reported_as_broken_not_as_empty() {
    // tree-sitter recovers from almost anything, so "parse failed" is not guaranteed by garbage;
    // what *is* guaranteed, and what this pins, is that the extractor never returns a silently empty
    // structure without either a reason or the parse's own complaint in `file_level`.
    let record = structure::extract_one(
        "broken.rs",
        b"fn oops( { let x = ; if { }]]] struct ",
        &ExtractPolicy::default(),
    );
    match (&record.structure, &record.reason) {
        (None, Some(_)) => {}
        (Some(_), None) => assert!(
            record.file_level.parse_errors > 0,
            "a structure extracted from a broken file must carry the parse's complaint: {:?}",
            record.file_level
        ),
        (None, None) => panic!("a file with no structure and no reason is a lost file"),
        (Some(_), Some(_)) => panic!("a file may not be both extracted and unavailable"),
    }
}

#[test]
fn every_corpus_file_comes_back_and_none_of_them_is_unavailable() {
    let sources = all_corpus();
    let corpus = extract_all(&sources, &ExtractPolicy::default());
    assert_eq!(corpus.files.len(), sources.len());
    assert_eq!(corpus.summary.unavailable, 0);
    assert_eq!(corpus.summary.extracted, sources.len());
    for record in &corpus.files {
        assert!(
            record.structure.is_some(),
            "{} came back with neither",
            record.path
        );
        assert!(record.reason.is_none());
    }
}

// ---------------------------------------------------------------------------
// AC 4 — `synthetic_eligible` is a state, not an absence
// ---------------------------------------------------------------------------

#[test]
fn a_dynamic_scope_is_marked_and_never_asserts_emptiness() {
    let corpus = extract_all(
        &corpus_sources("python", &["py"]),
        &ExtractPolicy::default(),
    );
    let dynamic = corpus
        .record("dynamic.py")
        .expect("dynamic.py is in the corpus");
    let structure = dynamic.structure.as_ref().expect("dynamic.py extracts");
    assert!(
        structure.is_synthetic(""),
        "a module with a `__getattr__` and a `globals()` write is not enumerable"
    );
    assert!(
        structure.is_synthetic("Slate"),
        "a decorated class is not enumerable"
    );
    let sentence = structure
        .may_have_shrunk()
        .expect("a synthetic structure must be able to say it may have shrunk");
    assert!(
        sentence.contains("may have shrunk") && !sentence.contains("declares nothing"),
        "{sentence}"
    );
    // The reasons are part of the state, not a side note: the surface has to be able to explain
    // itself, and `e3.7`'s pane is the thing that will.
    let reasons: Vec<&str> = structure
        .synthetic_scopes
        .iter()
        .flat_map(|scope| scope.reasons.iter().map(String::as_str))
        .collect();
    assert!(
        reasons.contains(&synthetic::MONKEYPATCH),
        "a monkeypatched class must say so: {reasons:?}"
    );
}

#[test]
fn a_macro_file_marks_its_scope_and_an_ordinary_file_asserts_completeness() {
    let corpus = extract_all(&corpus_sources("rust", &["rs"]), &ExtractPolicy::default());
    let macros = corpus
        .record("macros.rs")
        .expect("macros.rs is in the corpus");
    let structure = macros.structure.as_ref().expect("macros.rs extracts");
    assert!(structure.is_synthetic(""));
    assert!(
        structure
            .synthetic_scopes
            .iter()
            .filter(|scope| scope.scope.is_empty())
            .any(|scope| scope
                .reasons
                .iter()
                .any(|reason| reason == synthetic::MACRO_DEFINITION)),
        "{:?}",
        structure.synthetic_scopes
    );

    let secret = corpus
        .record("secret.rs")
        .expect("secret.rs is in the corpus");
    let secret = secret.structure.as_ref().expect("secret.rs extracts");
    // The other half of the same coin: a plain file may say "these are my declarations", and that
    // claim is what makes a `declares nothing` line meaningful when a file really is empty.
    assert!(
        secret.may_have_shrunk().is_none(),
        "{:?}",
        secret.synthetic_scopes
    );
}

// ---------------------------------------------------------------------------
// AC 3 — a wildcard dependency is an edge of its own, and answers stay honest
// ---------------------------------------------------------------------------

#[test]
fn a_star_import_is_its_own_edge_kind_and_who_depends_on_says_who_does_not_know() {
    let corpus = extract_all(&corpus_sources("rust", &["rs"]), &ExtractPolicy::default());
    let lib = corpus.record("lib.rs").expect("lib.rs is in the corpus");
    let structure = lib.structure.as_ref().expect("lib.rs extracts");
    let wildcards = structure.wildcard_edges();
    assert!(
        wildcards.iter().any(|edge| edge.target == "shapes.rs"),
        "`use crate::shapes::*` must arrive as a wildcard edge: {:?}",
        structure.edges
    );
    for edge in &wildcards {
        assert_eq!(
            edge.symbol, "*",
            "a wildcard edge names no symbol: {edge:?}"
        );
    }

    // The acceptance criterion, spelled as the question it is.
    let answer = corpus.dependents_of("shapes.rs", None);
    assert!(
        answer.unknown.contains(&"lib.rs".to_string()),
        "lib.rs star-imports shapes.rs, so the honest answer is that lib.rs depends on it with \
         contents unknown: {answer:?}"
    );
    assert!(
        answer.sentence().contains("unknown"),
        "{}",
        answer.sentence()
    );
}

#[test]
fn a_wildcard_dependent_is_never_reported_as_though_its_contents_were_named() {
    // A file may be in both lists, and `consumer.ts` is: it names `Circle` from `shapes.ts` through
    // the barrel *and* star-imports the rest. The case that must never be conflated is a dependency
    // that is *only* a wildcard — `geo/index.ts`'s `export * from './distance'`, which names nothing
    // and is therefore not entitled to appear in the confident list.
    let corpus = extract_all(
        &corpus_sources("typescript", &["ts"]),
        &ExtractPolicy::default(),
    );
    let answer = corpus.dependents_of("geo/distance.ts", None);
    assert!(
        answer.unknown.contains(&"geo/index.ts".to_string()),
        "the barrel star-re-exports distance.ts: {answer:?}"
    );
    assert!(
        !answer.named.contains(&"geo/index.ts".to_string()),
        "geo/index.ts names nothing that distance.ts declares: {answer:?}"
    );
    let shapes = corpus.dependents_of("shapes.ts", None);
    assert!(
        shapes.named.contains(&"consumer.ts".to_string())
            && shapes.unknown.contains(&"consumer.ts".to_string()),
        "naming one thing and starring the rest is both claims at once: {shapes:?}"
    );
}

#[test]
fn declaring_files_answers_the_symbol_question_with_a_sorted_list() {
    // The other query in `query.rs`, and the one `e5.6`'s symbol search will grow into. Sorted and
    // deduplicated, because a search result that changes order between runs is a search result that
    // cannot be paged through.
    let corpus = extract_all(&all_corpus(), &ExtractPolicy::default());
    let files = structure::declaring_files(&corpus, "Point");
    assert!(files.contains(&"geo/point.rs".to_string()), "{files:?}");
    assert!(files.contains(&"geo/point.ts".to_string()), "{files:?}");
    let mut sorted = files.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(files, sorted);
}

#[test]
fn an_unresolvable_import_is_kept_as_an_unresolved_edge_rather_than_dropped() {
    let corpus = extract_all(&corpus_sources("rust", &["rs"]), &ExtractPolicy::default());
    let shapes = corpus
        .record("shapes.rs")
        .expect("shapes.rs is in the corpus");
    let structure = shapes.structure.as_ref().expect("shapes.rs extracts");
    assert!(
        structure.edges.iter().any(
            |edge| edge.kind == structure::EdgeKind::Unresolved && edge.specifier == "std::fmt"
        ),
        "std::fmt is outside the corpus and must be said to be: {:?}",
        structure.edges
    );
    assert!(corpus.summary.edges_unresolved > 0);
}

// ---------------------------------------------------------------------------
// AC 5 — resolution is bounded, and the bound is configuration
// ---------------------------------------------------------------------------

#[test]
fn the_resolution_bound_is_reported_per_file_and_bites_when_tightened() {
    let sources = corpus_sources("typescript", &["ts"]);
    let roomy = extract_all(&sources, &ExtractPolicy::default());
    for record in roomy
        .files
        .iter()
        .filter_map(|record| record.structure.as_ref())
    {
        assert!(
            record.resolution.converged,
            "the corpus must converge within the default bound: {:?}",
            record.resolution
        );
        assert!(record.resolution.passes_used <= record.resolution.bound);
    }

    let tight_policy = ExtractPolicy {
        max_resolution_passes: 1,
        max_resolution_hops: 1,
        ..ExtractPolicy::default()
    };
    let tight = extract_all(&sources, &tight_policy);
    assert!(
        tight
            .files
            .iter()
            .filter_map(|record| record.structure.as_ref())
            .any(|structure| !structure.resolution.converged),
        "a bound that cannot bite is not a bound"
    );
    for record in tight
        .files
        .iter()
        .filter_map(|record| record.structure.as_ref())
    {
        assert_eq!(record.resolution.bound, 1);
        assert!(record.resolution.passes_used <= 1);
    }
}

#[test]
fn the_policy_reads_the_repository_config_shape_and_says_what_it_ignored() {
    let (policy, notices) = ExtractPolicy::from_repo_config_text(
        r"
[extract]
max_file_bytes = 4096

[extract.resolution]
max_passes = 7
max_hops = 9

[extract.complexity]
max_nodes = 1234
",
    );
    assert_eq!(policy.max_file_bytes, 4096);
    assert_eq!(policy.max_resolution_passes, 7);
    assert_eq!(policy.max_resolution_hops, 9);
    assert_eq!(policy.max_complexity_nodes, 1234);
    assert!(notices.is_empty(), "{notices:?}");

    let (policy, notices) = ExtractPolicy::from_repo_config_text(
        r"
[extract]
max_file_bytes = 0
nonsense = 3
",
    );
    assert_eq!(
        policy.max_file_bytes,
        ExtractPolicy::default().max_file_bytes,
        "a limit of zero would refuse every file, and a config key may not do that silently"
    );
    assert_eq!(
        notices.len(),
        2,
        "every ignored key must be named: {notices:?}"
    );
}

#[test]
fn a_tighter_complexity_bound_is_a_note_not_a_loss() {
    let source = vec![(
        "registry.py".to_string(),
        std::fs::read(corpus_root().join("python/registry.py")).expect("read fixture"),
    )];
    let roomy = extract_all(&source, &ExtractPolicy::default());
    let tight = extract_all(
        &source,
        &ExtractPolicy {
            max_complexity_nodes: 1,
            ..ExtractPolicy::default()
        },
    );
    // The file is still extracted, with its definitions, when the complexity walk hits its ceiling:
    // the bound protects the extractor from a pathological file, it does not get to hide the file.
    assert!(tight.summary.extracted == roomy.summary.extracted);
    assert_eq!(
        roomy.files[0]
            .structure
            .as_ref()
            .expect("structure")
            .symbols
            .len(),
        tight.files[0]
            .structure
            .as_ref()
            .expect("structure")
            .symbols
            .len()
    );
}

// ---------------------------------------------------------------------------
// AC 1 — the model itself: definitions, spans, complexity, visibility, supertypes
// ---------------------------------------------------------------------------

#[test]
fn definitions_carry_span_visibility_and_a_complexity_proxy() {
    let corpus = extract_all(&corpus_sources("rust", &["rs"]), &ExtractPolicy::default());
    let shapes = corpus
        .record("shapes.rs")
        .expect("shapes.rs is in the corpus");
    let structure = shapes.structure.as_ref().expect("shapes.rs extracts");
    let rect = structure
        .find("Rect")
        .expect("Rect is a definition of shapes.rs");
    assert_eq!(rect.kind, SymbolKind::Struct);
    assert!(rect.span.end_byte > rect.span.start_byte);
    assert!(rect.span.start_line >= 1 && rect.span.end_line >= rect.span.start_line);
    assert!(rect.visibility.is_public());
    let members = structure.members_of("Rect");
    assert!(
        members.iter().any(|member| member.name == "Rect.new"),
        "an impl block's members are attributed to its type: {:?}",
        members
            .iter()
            .map(|member| &member.name)
            .collect::<Vec<_>>()
    );
    // A complexity proxy must be non-zero where there is real code, and zero where there is not:
    // `e3.5`'s risk score reads these as numbers, not as decoration.
    assert!(rect.complexity.members > 0);
    let new = structure.find("Rect.new").expect("Rect::new");
    assert!(
        new.complexity.body_lines > 0,
        "a function with a body has body lines: {:?}",
        new.complexity
    );
}

#[test]
fn visibility_and_export_are_separate_claims_in_every_language() {
    let corpus = extract_all(
        &[
            corpus_sources("rust", &["rs"]),
            corpus_sources("python", &["py"]),
            corpus_sources("typescript", &["ts"]),
        ]
        .concat(),
        &ExtractPolicy::default(),
    );
    // Rust: `pub(crate)` is visible inside the crate and not exported.
    let rust = corpus
        .record("cfg_split.rs")
        .and_then(|record| record.structure.as_ref())
        .expect("cfg_split.rs extracts");
    assert!(rust.exports.iter().any(|name| name == "attributed"));
    // Python: the underscore convention decides export, and `__all__` overrides it.
    let python = corpus
        .record("util.py")
        .and_then(|record| record.structure.as_ref())
        .expect("util.py extracts");
    assert!(python.exports.iter().any(|name| name == "label"));
    assert!(
        !python.exports.iter().any(|name| name == "_private"),
        "an underscore-prefixed name is not exported unless `__all__` says so: {:?}",
        python.exports
    );
    // TypeScript: a declaration with no `export` is not on the surface, whatever its name says.
    let typescript = corpus
        .record("legacy.ts")
        .and_then(|record| record.structure.as_ref())
        .expect("legacy.ts extracts");
    assert!(
        !typescript.exports.iter().any(|name| name == "LocalOnly"),
        "{:?}",
        typescript.exports
    );
    assert!(typescript.exports.iter().any(|name| name == "Config"));
}

#[test]
fn supertypes_are_extracted_per_language_and_distinguish_base_from_contract() {
    let rust = extract_all(&corpus_sources("rust", &["rs"]), &ExtractPolicy::default());
    let shapes = rust
        .record("shapes.rs")
        .and_then(|record| record.structure.as_ref())
        .expect("shapes.rs extracts");
    assert!(
        shapes
            .supertypes
            .iter()
            .any(|base| base.of == "Rect" && base.base == "Shape"),
        "{:?}",
        shapes.supertypes
    );

    let python = extract_all(
        &corpus_sources("python", &["py"]),
        &ExtractPolicy::default(),
    );
    let models = python
        .record("models.py")
        .and_then(|record| record.structure.as_ref())
        .expect("models.py extracts");
    assert!(
        models
            .supertypes
            .iter()
            .any(|base| base.of == "Circle" && base.base == "Shape"),
        "{:?}",
        models.supertypes
    );

    let typescript = extract_all(
        &corpus_sources("typescript", &["ts"]),
        &ExtractPolicy::default(),
    );
    let shapes = typescript
        .record("shapes.ts")
        .and_then(|record| record.structure.as_ref())
        .expect("shapes.ts extracts");
    assert!(
        shapes
            .supertypes
            .iter()
            .any(|base| base.of == "Circle" && base.base == "Shape"),
        "{:?}",
        shapes.supertypes
    );
}

// ---------------------------------------------------------------------------
// the surface the next tickets read
// ---------------------------------------------------------------------------

#[test]
fn language_detection_names_the_three_shipped_languages_and_the_three_named_but_unshipped() {
    assert_eq!(Language::detect("a/b/mod.rs"), Language::Rust);
    assert_eq!(Language::detect("pkg/mod_test.rs"), Language::Rust);
    assert_eq!(Language::detect("pkg/__init__.py"), Language::Python);
    assert_eq!(Language::detect("app.pyi"), Language::Python);
    assert_eq!(Language::detect("src/index.ts"), Language::TypeScript);
    assert_eq!(Language::detect("src/decls.d.ts"), Language::TypeScript);
    assert_eq!(Language::detect("src/tsx.tsx"), Language::TypeScript);
    assert_eq!(Language::detect("main.go"), Language::Go);
    assert_eq!(Language::detect("lib/thing.rb"), Language::Ruby);
    assert_eq!(Language::detect("com/Thing.java"), Language::Java);
    assert_eq!(Language::detect("Makefile"), Language::Unknown);
    assert_eq!(Language::detect("thing.swift"), Language::Unknown);
    // The named-but-unshipped three are detected *and* carry the feature to rebuild with, so a
    // Go repository gets "rebuild with lang-go" rather than "not a language we know".
    assert_eq!(Language::Go.feature(), Some("lang-go"));
    assert_eq!(Language::Rust.feature(), Some("lang-rust"));
    assert!(!Language::Unknown.is_known());
    assert!(Language::Rust.is_known());
}

#[test]
fn the_schema_version_is_stated_because_the_output_is_stored() {
    // Not ceremony: `e3.2` writes these shapes into a stored graph, and a stored shape that cannot
    // say which version of the model produced it is a migration nobody can write afterwards.
    assert_eq!(structure::SCHEMA_VERSION, 1);
    let bytes = structure::canonical_json(&extract_all(
        &corpus_sources("rust", &["rs"]),
        &ExtractPolicy::default(),
    ));
    let parsed: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the canonical output is json");
    assert_eq!(
        parsed["schema_version"],
        serde_json::json!(structure::SCHEMA_VERSION)
    );
}

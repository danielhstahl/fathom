//! The structural extractor tail (`e3.1b`, `reco-dlp.1`): Go, Ruby and Java, and the three
//! scan-policy knobs spike `s1` named.
//!
//! The corpus harness owns *accuracy* for these languages the same way it owns it for `e3.1` — six
//! languages, one report, one `--check`. This file owns what a harness that only compares sets cannot
//! own: the **rules** the three new languages introduced, each stated as a behaviour rather than a
//! count.
//!
//! * **Go's dependency is a package, and every stored edge says so** (s1 finding 4). A Go edge that
//!   looks like a file-to-file fact is a claim nobody made, so the granularity is on the edge, in the
//!   sentence a pane prints, and in who-depends-on's answer.
//! * **Ruby's scan is bounded per run, in a priority order, and a refused file is still a measured
//!   file** (s1 finding 7). The cap is the ticket's answer to a grammar `s1` measured an order of
//!   magnitude off the pack, and a bound nobody can observe is a rumour.
//! * **Java's same-package scan is a guess with a scope, and the scope is a knob** (s1 finding 5).
//!   The scan is why Java can report a dependency with no `import` statement; the scope is why it
//!   does not invent one, and both halves are asserted here rather than published as a rate alone.
//!
//! ```text
//! cargo test -p fathom-analyze --features langs-all --test structure_tail
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use fathom_analyze::structure::{
    self, ExtractPolicy, Granularity, Language, ScanBudget, ScanScope, SourceFile, synthetic,
};
use std::path::{Path, PathBuf};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("structure")
        .join("corpus")
}

/// Every corpus file of one language, corpus-relative — the spelling the extractor derives module
/// paths from, so a prefixed path is a different program.
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

fn extract(sources: &[(String, Vec<u8>)], policy: &ExtractPolicy) -> structure::CorpusStructure {
    let borrowed: Vec<SourceFile> = sources
        .iter()
        .map(|(path, body)| SourceFile::new(path, body))
        .collect();
    structure::extract(&borrowed, policy)
}

fn go_corpus() -> Vec<(String, Vec<u8>)> {
    corpus_sources("go", &["go"])
}

fn ruby_corpus() -> Vec<(String, Vec<u8>)> {
    corpus_sources("ruby", &["rb"])
}

fn java_corpus() -> Vec<(String, Vec<u8>)> {
    corpus_sources("java", &["java"])
}

fn structure_of<'a>(
    corpus: &'a structure::CorpusStructure,
    path: &str,
) -> &'a structure::Structure {
    corpus
        .record(path)
        .unwrap_or_else(|| panic!("{path} did not come back at all"))
        .structure
        .as_ref()
        .unwrap_or_else(|| panic!("{path} came back unavailable"))
}

// ---------------------------------------------------------------------------
// Go: an import is a package, and the edge has to admit it (s1 finding 4)
// ---------------------------------------------------------------------------

#[test]
fn a_go_import_resolves_to_every_file_of_the_package_and_labels_each_edge() {
    let corpus = extract(&go_corpus(), &ExtractPolicy::default());
    let store = structure_of(&corpus, "internal/store/store.go");
    // `import "example.com/app/pkg/shape"` is one statement and three edges, because the importer
    // really does become able to name everything in that directory — and it is *the directory*, not
    // a file, that it named.
    let package_edges = store.package_edges();
    let targets: Vec<&str> = package_edges
        .iter()
        .map(|edge| edge.target.as_str())
        .collect();
    assert_eq!(
        targets,
        [
            "pkg/shape/circle.go",
            "pkg/shape/doc.go",
            "pkg/shape/shape.go"
        ],
        "a package import must fan out to the package's files, in path order"
    );
    assert!(
        !targets.contains(&"pkg/shape/shape_test.go"),
        "a `_test.go` file of the package is not part of the package an importer sees"
    );
    for edge in &package_edges {
        assert_eq!(edge.granularity, Granularity::Package);
        assert!(edge.is_package_granular());
        assert_eq!(
            edge.package.as_deref(),
            Some("example.com/app/pkg/shape"),
            "the edge must carry the import path it came from, or a reader cannot tell a package \
             edge from a file one after the fact"
        );
        assert_eq!(
            edge.symbol, "*",
            "a package edge names no symbol, and must not pretend to"
        );
    }
    // And the file-level edges of the same file must not be swept into the claim.
    assert!(
        store
            .edges
            .iter()
            .any(|edge| edge.kind == structure::EdgeKind::Unresolved && !edge.is_package_granular()),
        "`context` is outside the corpus and has to stay an unresolved file-level edge"
    );
}

#[test]
fn a_package_granular_edge_is_described_as_one_when_anyone_asks() {
    // The pane half is `e3.5`/`e3.6`; what this ticket owes them is the wording, produced by the
    // model rather than typed into a UI.
    let corpus = extract(&go_corpus(), &ExtractPolicy::default());
    let main = structure_of(&corpus, "main.go");
    let sentence = main
        .granularity_sentence()
        .expect("main.go's imports are package-granular, so it must be able to say so");
    assert!(
        sentence.contains("package") && sentence.contains("granularity"),
        "{sentence}"
    );
    assert!(
        !sentence.contains("declares nothing"),
        "a granularity sentence must not double as an emptiness claim: {sentence}"
    );
    // A language whose edges really are file-level has no such sentence to offer, which is what
    // stops the label being decoration.
    let ruby = extract(&ruby_corpus(), &ExtractPolicy::default());
    assert!(
        structure_of(&ruby, "lib/demo.rb")
            .granularity_sentence()
            .is_none(),
        "Ruby's edges are file-granular; labelling them package-granular would be the same lie in \
         the other direction"
    );
}

#[test]
fn who_depends_on_a_go_package_names_the_files_and_says_the_edges_are_package_granular() {
    // s1 finding 4's second half: a wildcard dependent must never be reported as though its contents
    // were named, and the honest answer is the file list plus the granularity, not "unknown".
    let corpus = extract(&go_corpus(), &ExtractPolicy::default());
    let dependents = structure::dependents_of(&corpus, "pkg/shape/shape.go", Some("Shape"));
    assert!(
        dependents.named.is_empty(),
        "nobody imported `shape.go` by name — Go cannot: {:?}",
        dependents.named
    );
    assert!(
        dependents
            .unknown
            .contains(&"internal/store/store.go".to_string())
            && dependents.unknown.contains(&"main.go".to_string()),
        "{:?}",
        dependents.unknown
    );
    assert_eq!(
        dependents.package_granular, dependents.unknown,
        "every dependent of a Go file arrived through a package import, and the answer must say so"
    );
    let sentence = dependents.sentence();
    assert!(
        sentence.contains("package"),
        "the sentence must carry the granularity: {sentence}"
    );
    // A Ruby dependent, by contrast, is an ordinary file-level dependent: the same wildcard kind,
    // no package claim.
    let ruby = extract(&ruby_corpus(), &ExtractPolicy::default());
    let ruby_dependents = structure::dependents_of(&ruby, "lib/demo/version.rb", None);
    assert!(
        ruby_dependents.unknown.contains(&"lib/demo.rb".to_string()),
        "{:?}",
        ruby_dependents.unknown
    );
    assert!(
        ruby_dependents.package_granular.is_empty(),
        "a `require` is a file-level dependency and must not borrow Go's label: {:?}",
        ruby_dependents.package_granular
    );
}

#[test]
fn a_go_file_behind_a_build_constraint_marks_its_scope_instead_of_claiming_the_package_is_whole() {
    // Two files declare `metrics.Init` and exactly one is compiled. A parser does not run the build,
    // so the honest output is "this scope may have shrunk", not two definitions and not one.
    let corpus = extract(&go_corpus(), &ExtractPolicy::default());
    for path in ["pkg/metrics/init_linux.go", "pkg/metrics/init_other.go"] {
        let file = structure_of(&corpus, path);
        assert!(
            file.is_synthetic(""),
            "{path} is behind a `//go:build` line and must mark its file scope"
        );
        assert!(
            file.synthetic_scopes.iter().any(|scope| scope
                .reasons
                .iter()
                .any(|reason| reason == synthetic::BUILD_CONSTRAINT)),
            "the mark has to say *why*: {:?}",
            file.synthetic_scopes
        );
        let sentence = file
            .may_have_shrunk()
            .expect("a synthetic file must be able to say it may have shrunk");
        assert!(!sentence.contains("declares nothing"), "{sentence}");
    }
    // The ordinary file of the same package is *not* marked: a second file in a directory is not a
    // second copy of anything.
    assert!(
        !structure_of(&corpus, "pkg/metrics/metrics.go").is_synthetic(""),
        "the mark is about build constraints, not about sharing a package"
    );
}

#[test]
fn a_go_module_path_is_deduced_from_the_corpus_because_no_one_hands_the_extractor_go_mod() {
    // The spike was *told* the module path. This module refuses, because everything it reads is
    // inside the corpus it was given; so the corpus's own import specs are the only evidence, and an
    // import that fits neither the deduced module nor a corpus-relative path stays unresolved.
    let corpus = extract(&go_corpus(), &ExtractPolicy::default());
    let main = structure_of(&corpus, "main.go");
    let unresolved: Vec<&str> = main
        .edges
        .iter()
        .filter(|edge| edge.kind == structure::EdgeKind::Unresolved)
        .map(|edge| edge.specifier.as_str())
        .collect();
    assert_eq!(
        unresolved,
        ["fmt"],
        "stdlib imports stay unresolved: {:?}",
        unresolved
    );
    assert!(
        main.imports
            .iter()
            .any(|import| import.specifier == "example.com/app/pkg/units"),
        "the dot-import is an import like any other and has to be recorded"
    );
}

// ---------------------------------------------------------------------------
// Ruby: the scan is bounded, in a priority order, and refusal still measures (s1 finding 7)
// ---------------------------------------------------------------------------

#[test]
fn ruby_is_the_only_language_with_a_default_scan_budget_and_the_cap_is_configurable() {
    // One cap, for the one grammar `s1` measured an order of magnitude off the pack. A cap on
    // everything is a cap nobody tunes; a cap on nothing is the spike's finding 7 still unfixed.
    let policy = ExtractPolicy::default();
    assert!(policy.scan_budget(Language::Ruby).is_some());
    for language in [
        Language::Rust,
        Language::Python,
        Language::Go,
        Language::TypeScript,
        Language::Java,
    ] {
        assert!(
            policy.scan_budget(language).is_none(),
            "{language:?} has a scan budget by default; `s1` measured it inside the pack"
        );
    }
    let ruby = policy.scan_budget(Language::Ruby).expect("ruby's budget");
    assert_eq!(ruby.max_files, 4_000);
    assert_eq!(ruby.max_bytes, 24_000_000);
    assert_eq!(ruby.priority, ["lib", "app", "ext", "spec", "test"]);
}

#[test]
fn files_past_the_scan_budget_come_back_refused_and_still_measured() {
    // The cap must behave exactly like `too_large`: the file is refused, and it is not lost. A
    // refused file with no file-level numbers is a hole in the graph wearing a bound's clothes.
    let sources = ruby_corpus();
    let whole = extract(&sources, &ExtractPolicy::default());
    assert_eq!(
        whole.summary.unavailable, 0,
        "the corpus fits inside the default budget"
    );

    let capped = ExtractPolicy {
        ruby_scan: ScanBudget {
            max_files: 2,
            max_bytes: 0,
            priority: ExtractPolicy::default().ruby_scan.priority,
        },
        ..ExtractPolicy::default()
    };
    let squeezed = extract(&sources, &capped);
    let refused: Vec<&str> = squeezed
        .files
        .iter()
        .filter(|record| {
            record
                .reason
                .as_ref()
                .is_some_and(|reason| reason.as_str() == "scan_budget")
        })
        .map(|record| record.path.as_str())
        .collect();
    assert_eq!(
        refused.len(),
        sources.len() - 2,
        "two files allowed, the rest refused"
    );
    for record in squeezed
        .files
        .iter()
        .filter(|record| record.structure.is_none())
    {
        assert!(
            record.file_level.bytes > 0,
            "{} was refused without being measured",
            record.path
        );
        assert!(record.reason.is_some(), "a refusal has to name itself");
    }
    // The reason names the key, so `fathom explain` can point at the config line instead of at a
    // mystery.
    let first_refused = squeezed
        .files
        .iter()
        .find(|record| {
            record
                .reason
                .as_ref()
                .is_some_and(|reason| reason.as_str() == "scan_budget")
        })
        .expect("a refused file");
    let reason = first_refused.reason.as_ref().expect("the refusal reason");
    assert!(
        reason.as_str().contains("scan_budget"),
        "the reason must be a vocabulary word: {reason:?}"
    );
}

#[test]
fn the_priority_order_decides_who_gets_refused_because_alphabetical_order_is_not_a_policy() {
    // A run that has to stop early should stop having read what an entry point reaches. `lib` first
    // is the whole point of the key; without the ordering the cap would keep `ext/` and drop `lib/`.
    let mut owned = ruby_corpus();
    // Two files outside `lib/`, so the ordering is observable rather than accidental.
    owned.push((
        "ext/native/ext_demo.rb".to_string(),
        b"module ExtDemo\n  def self.boot\n      :ok\n  end\nend\n".to_vec(),
    ));
    owned.push((
        "spec/lib_spec.rb".to_string(),
        b"require 'demo'\n\nSpec = Demo\n".to_vec(),
    ));
    owned.sort();
    let capped = ExtractPolicy {
        ruby_scan: ScanBudget {
            max_files: 3,
            max_bytes: 0,
            priority: vec!["spec".to_string(), "lib".to_string()],
        },
        ..ExtractPolicy::default()
    };
    let squeezed = extract(&owned, &capped);
    let kept: Vec<&str> = squeezed
        .files
        .iter()
        .filter(|record| record.structure.is_some())
        .map(|record| record.path.as_str())
        .collect();
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert!(
        kept.contains(&"spec/lib_spec.rb"),
        "`spec` was listed first, so it is scanned first: {kept:?}"
    );
    assert!(
        !kept.contains(&"ext/native/ext_demo.rb"),
        "`ext` matches no listed prefix and comes last: {kept:?}"
    );
    // The byte cap bites on its own too, on a corpus whose files are big enough for it.
    let by_bytes = ExtractPolicy {
        ruby_scan: ScanBudget {
            max_files: 0,
            max_bytes: 1,
            priority: Vec::new(),
        },
        ..ExtractPolicy::default()
    };
    let tight = extract(&owned, &by_bytes);
    assert!(
        tight.files.iter().any(|record| record.structure.is_none()
            && record
                .reason
                .as_ref()
                .is_some_and(|reason| reason.as_str() == "scan_budget")),
        "a byte cap of one must refuse something"
    );
}

#[test]
fn a_ruby_require_resolves_against_the_file_or_the_load_path_and_never_a_guess_between_the_two() {
    let corpus = extract(&ruby_corpus(), &ExtractPolicy::default());
    let demo = structure_of(&corpus, "lib/demo.rb");
    let targets: Vec<&str> = demo
        .edges
        .iter()
        .filter(|edge| edge.kind == structure::EdgeKind::Wildcard)
        .map(|edge| edge.target.as_str())
        .collect();
    assert!(
        targets.contains(&"lib/demo/version.rb"),
        "`require \"demo/version\"` resolves against the corpus's load root (lib/): {targets:?}"
    );
    assert!(
        targets.contains(&"lib/demo/core/shape.rb"),
        "`require_relative \"core/shape\"` resolves against the *file's* directory: {targets:?}"
    );
    // Ruby has no symbol-level import edge at all: a require brings in a file, so every edge is a
    // wildcard and no name is claimed.
    assert!(
        demo.edges
            .iter()
            .filter(|edge| edge.kind == structure::EdgeKind::Wildcard)
            .all(|edge| edge.symbol == "*"),
        "a resolved `require` names no symbol"
    );
    let unresolved: Vec<&str> = demo
        .edges
        .iter()
        .filter(|edge| edge.kind == structure::EdgeKind::Unresolved)
        .map(|edge| edge.specifier.as_str())
        .collect();
    assert!(
        unresolved.contains(&"json"),
        "a gem is not in the corpus and must stay unresolved rather than be aimed at a stranger: \
         {unresolved:?}"
    );
}

#[test]
fn a_ruby_scope_a_program_wrote_into_is_marked_and_never_asserts_emptiness() {
    // Two ways to arrive at the same state: `define_method` over an array (the names exist only at
    // runtime) and `class_eval` on a class this file does not declare.
    let corpus = extract(&ruby_corpus(), &ExtractPolicy::default());
    let shape = structure_of(&corpus, "lib/demo/core/shape.rb");
    assert!(shape.is_synthetic("Demo.Core.Shape"));
    assert!(
        shape.synthetic_scopes.iter().any(|scope| scope
            .reasons
            .iter()
            .any(|reason| reason == synthetic::DYNAMIC_METHOD)),
        "the mark has to say it was a dynamic definition: {:?}",
        shape.synthetic_scopes
    );
    let plugin = structure_of(&corpus, "lib/demo/plugin.rb");
    assert!(plugin.is_synthetic("Demo.Core.Shape"));
    assert!(
        plugin.synthetic_scopes.iter().any(|scope| scope
            .reasons
            .iter()
            .any(|reason| reason == synthetic::MONKEYPATCH)),
        "{:?}",
        plugin.synthetic_scopes
    );
    for file in [shape, plugin] {
        let sentence = file
            .may_have_shrunk()
            .expect("a marked scope can never claim to be whole");
        assert!(!sentence.contains("declares nothing"), "{sentence}");
    }
    // A literal `define_method(:perimeter)` is a declaration in the file and is a definition, not a
    // blind spot: the mark is for what the file cannot enumerate, not for unusual syntax.
    assert!(
        shape
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Demo.Core.Shape.perimeter"),
        "a literal define_method is in the file"
    );
}

#[test]
fn the_question_ruby_can_answer_is_what_an_entry_point_loads() {
    // s1 finding 3: Ruby has no per-symbol access control, so an "exported symbols" list is a
    // degenerate thing; *this* is the question a Ruby surface can answer, and the query has to exist
    // before `e3.7` is asked to draw it.
    let corpus = extract(&ruby_corpus(), &ExtractPolicy::default());
    let loaded = structure::loads(&corpus, "lib/demo.rb");
    assert_eq!(
        loaded.files,
        [
            "lib/demo/core/circle.rb",
            "lib/demo/core/shape.rb",
            "lib/demo/report.rb",
            "lib/demo/version.rb",
        ],
        "the answer is exactly what `require` reaches, transitively"
    );
    assert!(
        !loaded.files.contains(&"lib/demo.rb".to_string()),
        "the entry point is not one of its own loaded files"
    );
    assert!(
        !loaded.files.contains(&"lib/demo/plugin.rb".to_string()),
        "nothing that `demo.rb` loads loads `plugin.rb`, so the query must not list it: the answer \
         is the load graph, not the corpus"
    );
    assert!(
        !loaded.truncated,
        "the corpus's require graph is shallow: {}",
        loaded.depth
    );
    let sentence = loaded.sentence();
    assert!(
        sentence.contains("load") || sentence.contains("require"),
        "the sentence has to be about loading: {sentence}"
    );
    // And the export list says what it is, because the note is the difference between a degenerate
    // answer and a wrong one.
    let demo = structure_of(&corpus, "lib/demo.rb");
    assert!(
        demo.notes
            .iter()
            .any(|note| note.kind == "export_surface_is_the_file"),
        "{:?}",
        demo.notes
    );
}

// ---------------------------------------------------------------------------
// Java: a dependency with no import statement, guessed inside a stated scope (s1 finding 5)
// ---------------------------------------------------------------------------

#[test]
fn a_java_bare_type_reference_resolves_inside_the_declared_package_and_nowhere_else() {
    let corpus = extract(&java_corpus(), &ExtractPolicy::default());
    let circle = structure_of(&corpus, "com/acme/app/model/Circle.java");
    // `Circle implements Shape` and `new Point(...)` carry no import: Shape and Point are *beside*
    // Circle.java, and Java says so without writing it down. A scan that refused to guess would
    // report no edge at all, which is the failure s1 finding 5 is about.
    let same_package: Vec<(&str, &str)> = circle
        .edges
        .iter()
        .filter(|edge| edge.form == "same_package")
        .map(|edge| (edge.target.as_str(), edge.symbol.as_str()))
        .collect();
    assert!(
        same_package.iter().any(
            |(target, symbol)| *target == "com/acme/app/model/Shape.java" && *symbol == "Shape"
        ),
        "{same_package:?}"
    );
    assert!(
        same_package
            .iter()
            .any(|(target, _)| *target == "com/acme/app/model/Point.java"),
        "{same_package:?}"
    );
    // The scope is the whole claim: a same-named type in a *different* package is not this file's
    // dependency, and the corpus contains one (`Tracked` is imported by name from the parent).
    assert!(
        circle
            .edges
            .iter()
            .any(|edge| edge.target == "com/acme/app/Tracked.java" && edge.form == "single_import"),
        "the import statement is still an edge of its own: {:?}",
        circle.edges
    );
    for edge in circle
        .edges
        .iter()
        .filter(|edge| edge.form == "same_package")
    {
        assert_eq!(
            edge.granularity,
            Granularity::File,
            "a same-package edge is a file-to-file fact, unlike a Go package edge"
        );
    }
}

#[test]
fn an_import_of_the_same_spelling_shadows_the_package_neighbour_so_the_edge_is_the_imports() {
    // JLS 6.5.5, and the one case where a naive same-package scan invents an edge: `App.java` says
    // `import com.acme.app.util.Geom.area` and mentions `Geom`; the import wins.
    let corpus = extract(&java_corpus(), &ExtractPolicy::default());
    let app = structure_of(&corpus, "com/acme/app/App.java");
    assert!(
        app.edges
            .iter()
            .any(|edge| edge.target == "com/acme/app/util/Geom.java" && edge.symbol == "area"),
        "the static import is the dependency: {:?}",
        app.edges
    );
    assert!(
        !app.edges
            .iter()
            .any(|edge| edge.form == "same_package" && edge.symbol == "Geom"),
        "an imported name must not also be claimed as a same-package neighbour"
    );
    // A package-qualified head is a package, not a type: a lower-case head never resolves against the
    // file's own package.
    assert!(
        !app.edges
            .iter()
            .any(|edge| edge.form == "same_package" && edge.symbol == "model"),
        "a package-shaped head must not become an edge"
    );
}

#[test]
fn the_same_package_scan_scope_is_a_knob_and_the_wider_scope_claims_more() {
    // The knob exists so the *cost* of the scoping stays measurable: `all_type_references` is the
    // spike's scan, and the report publishes both false-positive rates. Here the cheaper claim is
    // enough: the wide scope claims at least what the scoped one does.
    let sources = java_corpus();
    let scoped = extract(&sources, &ExtractPolicy::default());
    let wide_policy = ExtractPolicy {
        java_same_package_scan: ScanScope::AllTypeReferences,
        ..ExtractPolicy::default()
    };
    let wide = extract(&sources, &wide_policy);
    let count = |corpus: &structure::CorpusStructure| {
        corpus
            .files
            .iter()
            .filter_map(|record| record.structure.as_ref())
            .map(|structure| {
                structure
                    .edges
                    .iter()
                    .filter(|edge| edge.form == "same_package")
                    .count()
            })
            .sum::<usize>()
    };
    assert_eq!(
        ExtractPolicy::default().java_same_package_scan,
        ScanScope::ImportsPlusPackage,
        "the shipped default is the scoped scan; the wide one is a measurement, not a setting"
    );
    assert!(
        count(&wide) >= count(&scoped),
        "wide {} < scoped {} — the wide scope is a superset by definition",
        count(&wide),
        count(&scoped)
    );
    assert!(
        count(&scoped) > 0,
        "the scoped scan must find the corpus's same-package edges"
    );
}

#[test]
fn a_record_component_is_a_note_and_a_marked_scope_never_a_definition_the_file_does_not_contain() {
    let corpus = extract(&java_corpus(), &ExtractPolicy::default());
    let circle = structure_of(&corpus, "com/acme/app/model/Circle.java");
    assert!(
        !circle
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Circle.name" || symbol.name == "Circle.r"),
        "a record's accessors are written by the compiler, so a definition list that includes them \
         is lying about the file"
    );
    assert!(circle.is_synthetic("Circle"));
    assert!(
        circle.synthetic_scopes.iter().any(|scope| scope
            .reasons
            .iter()
            .any(|reason| reason == synthetic::SYNTHESISED_ACCESSORS)),
        "{:?}",
        circle.synthetic_scopes
    );
    assert!(
        circle
            .notes
            .iter()
            .any(|note| note.kind == "record_accessor"),
        "the note is where the two missing accessors are accounted for: {:?}",
        circle.notes
    );
}

#[test]
fn a_java_module_file_decides_what_is_public_and_a_package_outside_it_is_not() {
    // Java's export rule lives in a *different file* than the code it governs, which is why the
    // extractor derives the export surface once, from `module-info.java`, rather than per file.
    let corpus = extract(&java_corpus(), &ExtractPolicy::default());
    let exported = structure_of(&corpus, "com/acme/app/model/Shape.java");
    assert!(
        exported.exports.iter().any(|name| name == "Shape"),
        "{:?}",
        exported.exports
    );
    let legacy = structure_of(&corpus, "com/acme/app/legacy/Old.java");
    assert!(
        legacy.exports.is_empty(),
        "`com.acme.app.legacy` is not exported by the module, so nothing in it is public API: {:?}",
        legacy.exports
    );
}

#[test]
fn a_java_wildcard_import_is_an_edge_on_the_package_not_on_an_invented_symbol() {
    // An on-demand import (`import java.util.*`) is a dependency whose contents nobody named, so it
    // must arrive as a wildcard — or, if the package is outside the corpus, as unresolved. What it
    // must never arrive as is a *resolved, named* edge: that is the lie s1 finding 2 is about, and it
    // is why `EdgeKind` has three members instead of two.
    let corpus = extract(&java_corpus(), &ExtractPolicy::default());
    let wildcard = structure_of(&corpus, "com/acme/app/util/Wildcard.java");
    let stars: Vec<&structure::Edge> = wildcard
        .edges
        .iter()
        .filter(|edge| edge.symbol == "*")
        .collect();
    assert!(
        !stars.is_empty(),
        "the file's on-demand import has to be recorded even when nothing in it is named"
    );
    for edge in &stars {
        assert!(
            matches!(
                edge.kind,
                structure::EdgeKind::Wildcard | structure::EdgeKind::Unresolved
            ),
            "an on-demand import came back as a named dependency: {edge:?}"
        );
        assert_eq!(
            edge.form, "wildcard_import",
            "the form says how the dependency was written"
        );
    }
    assert!(
        stars
            .iter()
            .any(|edge| edge.specifier == "java.util.*" && edge.target.is_empty()),
        "`java.util` is outside the corpus and stays an honest hole: {stars:?}"
    );
}

// ---------------------------------------------------------------------------
// the knobs, as configuration
// ---------------------------------------------------------------------------

#[test]
fn the_scan_policy_keys_read_out_of_a_repository_config_and_the_bad_ones_are_named() {
    let (policy, notices) = ExtractPolicy::from_repo_config_text(
        r#"
[extract.ruby]
max_files_per_run = 250
max_bytes_per_run = 1048576
priority = ["app", "lib"]

[extract.java]
same_package_scan = "all_type_references"
"#,
    );
    assert_eq!(policy.ruby_scan.max_files, 250);
    assert_eq!(policy.ruby_scan.max_bytes, 1_048_576);
    assert_eq!(policy.ruby_scan.priority, ["app", "lib"]);
    assert_eq!(policy.java_same_package_scan, ScanScope::AllTypeReferences);
    assert!(notices.is_empty(), "{notices:?}");

    // A wrong spelling of a scope is a condition on the run, stated: silently falling back to the
    // default would change what the graph claims while leaving the report's wording intact.
    let (policy, notices) = ExtractPolicy::from_repo_config_text(
        r#"
[extract.java]
same_package_scan = "every_reference_i_have_ever_seen"
"#,
    );
    assert_eq!(policy.java_same_package_scan, ScanScope::ImportsPlusPackage);
    assert!(
        notices
            .iter()
            .any(|notice| notice.to_string().contains("same_package_scan")),
        "the ignored key has to be named: {notices:?}"
    );

    // Zero is not a typo for "refuse every file": it is how an operator says *no cap*, and it is the
    // safe direction for a mistake — the failure mode is the pre-`e3.1b` behaviour (scan the lot),
    // not an empty graph. A negative is still nonsense and is named.
    let (policy, notices) = ExtractPolicy::from_repo_config_text(
        r#"
[extract.ruby]
max_files_per_run = 0
max_bytes_per_run = 0
"#,
    );
    assert_eq!(policy.ruby_scan.max_files, 0, "zero means uncapped");
    assert_eq!(policy.ruby_scan.max_bytes, 0, "zero means uncapped");
    assert!(notices.is_empty(), "{notices:?}");
    let (policy, notices) = ExtractPolicy::from_repo_config_text(
        r"
[extract.ruby]
max_files_per_run = -1
",
    );
    assert_eq!(policy.ruby_scan.max_files, 4_000);
    assert!(
        notices
            .iter()
            .any(|notice| notice.to_string().contains("max_files_per_run")),
        "{notices:?}"
    );
}

#[test]
fn the_three_new_languages_are_detected_named_and_their_feature_names_are_stable() {
    // ADR-002 §6.2's rule: a gated-out language is named, not unknown, so the advice says which
    // feature to rebuild with.
    for (path, language) in [
        ("main.go", Language::Go),
        ("lib/demo.rb", Language::Ruby),
        ("com/acme/App.java", Language::Java),
    ] {
        assert_eq!(Language::detect(path), language, "{path}");
        assert!(language.is_known(), "{language:?} must be known");
    }
    assert_eq!(Language::Go.feature(), Some("lang-go"));
    assert_eq!(Language::Ruby.feature(), Some("lang-ruby"));
    assert_eq!(Language::Java.feature(), Some("lang-java"));
}

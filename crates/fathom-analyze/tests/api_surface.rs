//! The public API surface diff (`e3.7`, `reco-p5u`): the acceptance criteria as tests.
//!
//! The ticket asks five things of this module, and each one is a test below with the same words:
//!
//! 1. **breaking and additive fixtures for at least Rust, Python, TypeScript and Go classify
//!    correctly** — one fixture pair per language (plus Ruby, whose surface is a different shape and
//!    needs its own row), each asserting the breaking half, the additive half, and the thing that must
//!    *not* appear;
//! 2. **"no public API change" is detected by a fast path** — asserted at both rungs: the byte rung
//!    that never opens a grammar, and the digest rung that parses and finds nothing;
//! 3. **every change links to a `file:line` and to the episode that made it** — asserted as a property
//!    over every change in every fixture, and then as the episode trail's own question;
//! 4. **the semver recommendation explains which specific change drove it** — asserted by requiring the
//!    driver's name, location and reason code to be *in* the sentence, not merely implied by a bump;
//! 5. the honest limits — a scope that cannot be enumerated says so instead of accusing, a file that
//!    could not be read makes the answer incomplete rather than clean, and an excluded path is counted
//!    in the open.
//!
//! The fixtures are trees, not strings, for one reason: package attribution, the module tree, the barrel
//! that forwards a name and the manifest that names a crate are all *relationships between files*, and a
//! single-string fixture cannot express them. Where a case needs no relationship — a macro's synthetic
//! scope, a policy knob — it is inline.
//!
//! ```text
//! FATHOM_API_DUMP=1 cargo test -p fathom-analyze --features langs-all --test api_surface -- --nocapture
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use fathom_analyze::api::report;
use fathom_analyze::api::{
    Answer, Answered, ApiPolicy, Bump, Change, Class, Diff, Episode, Identity, Inputs, Quick,
    Surface, Trail, extract_surface, fingerprint, quick,
};
use fathom_analyze::structure::{ExtractPolicy, Language, SourceFile, Visibility};
use std::path::{Path, PathBuf};

// ==============================================================================================
// the harness
// ==============================================================================================

/// One revision of a fixture tree, as a [`Surface`].
fn surface(language: &str, revision: &str) -> Surface {
    extract_surface(&inputs(language, revision, &ApiPolicy::default_policy()))
}

fn inputs(language: &str, revision: &str, policy: &ApiPolicy) -> Inputs<'static> {
    let root = corpus(language, revision);
    inputs_of(&files(&root), policy)
}

/// The inputs a caller would hand the module, from a list of `(path, text)`.
///
/// The two vectors the split produces have to outlive the `Inputs` built from them, and a test that
/// declared them in a local scope would fight the borrow checker for a reason that has nothing to do
/// with what is being tested, so they are leaked. A real caller declares them first.
fn inputs_of(files: &[(String, Vec<u8>)], policy: &ApiPolicy) -> Inputs<'static> {
    let files: &'static [(String, Vec<u8>)] = Box::leak(files.to_vec().into_boxed_slice());
    let policy: &'static ApiPolicy = Box::leak(Box::new(policy.clone()));
    let (sources, manifests) = Inputs::split(files);
    let sources: &'static [SourceFile<'static>] = Box::leak(sources.into_boxed_slice());
    let manifests: &'static [SourceFile<'static>] = Box::leak(manifests.into_boxed_slice());
    Inputs {
        sources,
        manifests,
        cargo_metadata: None,
        rustdoc_json: None,
        policy,
        extract: ExtractPolicy::default(),
    }
}

fn corpus(language: &str, revision: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("api")
        .join("corpus")
        .join(language)
        .join(revision)
}

/// Every source and manifest of a fixture tree, corpus-relative. Both halves matter: a caller that
/// handed over only the source files would lose every package name in the report.
fn files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("the fixture directory is readable") {
            let entry = entry.expect("a readable directory entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .expect("under the fixture root")
                .to_string_lossy()
                .replace('\\', "/");
            if Language::detect(&relative).is_known()
                || fathom_analyze::api::package::is_manifest(&relative)
            {
                out.push((
                    relative,
                    std::fs::read(&path).expect("a readable fixture file"),
                ));
            }
        }
    }
    out.sort();
    out
}

fn inline(files: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
    files
        .iter()
        .map(|(path, text)| (path.to_string(), text.to_string().into_bytes()))
        .collect()
}

/// The diff of a fixture pair, with the dump a reader needs when an assertion fails.
fn diff(language: &str, before: &Surface, after: &Surface) -> Diff {
    let diff = fathom_analyze::api::diff::diff(before, after, None);
    if std::env::var("FATHOM_API_DUMP").is_ok() {
        println!("#### {language}");
        for line in report::lines(&diff) {
            println!("  {line}");
        }
    }
    diff
}

/// One change, found by the name a reviewer would search for.
fn change<'a>(diff: &'a Diff, want: &str, name: &str) -> &'a Change {
    diff.packages
        .iter()
        .find(|package| package.name == want)
        .unwrap_or_else(|| panic!("no package {want} in the diff"))
        .changes
        .iter()
        .find(|change| change.item.name == name)
        .unwrap_or_else(|| {
            let all: Vec<String> = diff
                .packages
                .iter()
                .flat_map(|package| &package.changes)
                .map(|change| format!("{} {}", change.class.as_str(), change.item.name))
                .collect();
            panic!(
                "no change to {name} in {want}; the diff says: {}",
                all.join(" | ")
            )
        })
}

fn has_reason(change: &Change, code: &str) -> bool {
    change
        .reasons
        .iter()
        .any(|detail| detail.reason.code() == code)
}

fn names(diff: &Diff, class: Class) -> Vec<String> {
    let mut out: Vec<String> = diff
        .packages
        .iter()
        .flat_map(|package| &package.changes)
        .filter(|change| change.class == class)
        .map(|change| change.item.name.clone())
        .collect();
    out.sort();
    out
}

// ==============================================================================================
// 1. the four (and a half) languages
// ==============================================================================================

/// Rust: the module tree is part of the surface, `pub` is not enough on its own, and a field is a
/// contract.
#[test]
fn rust_breaking_and_additive() {
    let before = surface("rust", "before");
    let after = surface("rust", "after");
    assert_eq!(before.packages.len(), 1, "one crate, one package");
    assert_eq!(before.packages[0].name, "shapes", "named by its Cargo.toml");
    let diff = diff("rust", &before, &after);
    assert_eq!(diff.answered, Answered::Items);

    // Breaking, each for the reason a Rust caller would feel.
    assert!(has_reason(change(&diff, "shapes", "gone"), "item_removed"));
    assert!(has_reason(
        change(&diff, "shapes", "distance"),
        "required_parameter_added"
    ));
    assert!(has_reason(
        change(&diff, "shapes", "Rect.area"),
        "return_type_changed"
    ));
    assert!(has_reason(
        change(&diff, "shapes", "Rect.width"),
        "receiver_changed"
    ));

    // Additive.
    assert!(has_reason(
        change(&diff, "shapes", "perimeter"),
        "item_added"
    ));
    assert!(has_reason(change(&diff, "shapes", "NEW"), "item_added"));
    assert!(has_reason(change(&diff, "shapes", "Rect"), "member_added"));
    assert!(has_reason(
        change(&diff, "shapes", "label"),
        "parameter_type_widened"
    ));

    // Deprecated, with the author's own words.
    let legacy = change(&diff, "shapes", "legacy");
    assert_eq!(legacy.class, Class::Deprecating);
    assert!(
        legacy
            .reasons
            .iter()
            .any(|detail| detail.note.contains("use `label` instead")),
        "the note is the migrator's sentence: {:?}",
        legacy.reasons
    );

    // A constant's *value* changed and its type did not: not an API change, and said as much.
    let sides = change(&diff, "shapes", "SIDES");
    assert_eq!(sides.class, Class::Neutral, "a value is not a signature");
    assert!(has_reason(sides, "value_changed"));

    // The things that must not be in the diff at all.
    for absent in ["helper", "Area", "Shape", "Rect.new", "secret.rs", "lib.rs"] {
        assert!(
            !diff
                .packages
                .iter()
                .flat_map(|package| &package.changes)
                .any(|change| change.item.name == absent),
            "{absent} did not change and must not be reported"
        );
    }

    // A `pub(crate)` method is not on the surface in either revision, so a change inside it is silent.
    assert!(
        !after
            .package("shapes")
            .expect("shapes")
            .items
            .iter()
            .any(|item| item.id.name == "Rect.grow" && item.on_surface),
        "pub(crate) is not public API"
    );
}

/// Python: `__all__` and the import chain are the surface, and dropping a re-export is breaking.
#[test]
fn python_breaking_and_additive() {
    let before = surface("python", "before");
    let after = surface("python", "after");
    let diff = diff("python", &before, &after);
    assert_eq!(diff.answered, Answered::Items);
    assert_eq!(before.packages[0].name, "shapes", "the importable unit");

    assert!(has_reason(change(&diff, "shapes", "gone"), "item_removed"));
    assert!(has_reason(
        change(&diff, "shapes", "Circle.area"),
        "item_removed"
    ));
    assert!(has_reason(
        change(&diff, "shapes", "Shape.describe"),
        "parameter_tightened"
    ));
    // `Circle` is no longer imported by the package root: the name is still declared and still public,
    // and it is still *gone* from where consumers import it.
    let circle = change(&diff, "shapes", "Circle");
    assert_eq!(circle.class, Class::Breaking);
    assert!(has_reason(circle, "reachability_narrowed"));

    assert!(has_reason(
        change(&diff, "shapes", "distance"),
        "optional_parameter_added"
    ));
    assert!(has_reason(
        change(&diff, "shapes", "label"),
        "optional_parameter_added"
    ));
    assert!(has_reason(
        change(&diff, "shapes", "perimeter"),
        "item_added"
    ));
    assert!(has_reason(change(&diff, "shapes", "Shape"), "member_added"));

    // The underscore rule: private names are not surface, and `__all__` is this module's input rather
    // than an item of its own.
    let items: Vec<&str> = after
        .package("shapes")
        .expect("shapes")
        .items
        .iter()
        .filter(|item| item.on_surface)
        .map(|item| item.id.name.as_str())
        .collect();
    assert!(
        !items
            .iter()
            .any(|name| name.starts_with('_') && !name.starts_with("__")),
        "a leading underscore is Python's private marker: {items:?}"
    );
    assert!(
        !items.contains(&"__all__"),
        "the export list is read, not reported as an item: {items:?}"
    );

    // A changed body inside an internal module is not an API change.
    assert!(
        !diff
            .packages
            .iter()
            .flat_map(|package| &package.changes)
            .any(|change| change.item.name == "helper"),
        "util.helper's body changed and its signature did not"
    );
}

/// TypeScript: optionality is the whole game, and a barrel is part of the surface.
#[test]
fn typescript_breaking_and_additive() {
    let before = surface("typescript", "before");
    let after = surface("typescript", "after");
    assert_eq!(
        before.packages[0].name, "@acme/sdk",
        "named by package.json"
    );
    let diff = diff("typescript", &before, &after);

    assert!(has_reason(
        change(&diff, "@acme/sdk", "gone"),
        "item_removed"
    ));
    for name in ["Shape.describe", "Rect.describe", "Circle.describe"] {
        assert!(
            has_reason(change(&diff, "@acme/sdk", name), "parameter_tightened"),
            "`prefix?: string` becoming required breaks every call that omitted it"
        );
    }

    assert!(has_reason(
        change(&diff, "@acme/sdk", "distance"),
        "optional_parameter_added"
    ));
    assert!(has_reason(
        change(&diff, "@acme/sdk", "draw"),
        "parameter_type_widened"
    ));
    assert!(has_reason(
        change(&diff, "@acme/sdk", "Rect"),
        "member_added"
    ));
    assert!(has_reason(
        change(&diff, "@acme/sdk", "perimeter"),
        "item_added"
    ));
    // A declaration file is surface on its own.
    assert!(has_reason(
        change(&diff, "@acme/sdk", "fromDeclaration"),
        "item_added"
    ));

    let legacy = change(&diff, "@acme/sdk", "legacy");
    assert_eq!(legacy.class, Class::Deprecating);
    assert!(has_reason(legacy, "item_added"), "it arrived deprecated");
    assert!(has_reason(legacy, "deprecated"));

    // The barrel's star is stated as context and is not treated as a hole in the read.
    assert_eq!(diff.answered, Answered::Items);
    assert!(
        diff.caveats
            .iter()
            .any(|caveat| caveat.contains("forward names with a star")),
        "the star is said out loud: {:?}",
        diff.caveats
    );
    assert!(
        diff.gaps.is_empty(),
        "a barrel is not an unread file: {:?}",
        diff.gaps
    );
}

/// Go: the unit is the package, so a file move inside it is not a moved name.
#[test]
fn go_breaking_and_additive() {
    let before = surface("go", "before");
    let after = surface("go", "after");
    let package = format!("{}/shape", "example.com/shapes");
    assert_eq!(
        before.packages[0].name, package,
        "go.mod plus the directory"
    );
    let diff = diff("go", &before, &after);

    assert!(has_reason(change(&diff, &package, "Gone"), "item_removed"));
    assert!(has_reason(
        change(&diff, &package, "NewRect"),
        "required_parameter_added"
    ));
    assert!(has_reason(
        change(&diff, &package, "Rect.Area"),
        "receiver_changed"
    ));

    // `eps ...float64`: a list the caller may leave empty.
    assert!(has_reason(
        change(&diff, &package, "Distance"),
        "optional_parameter_added"
    ));
    assert!(has_reason(
        change(&diff, &package, "Perimeter"),
        "item_added"
    ));
    assert!(has_reason(change(&diff, &package, "Rect"), "member_added"));

    // `Draw` moved from `shape.go` to `draw.go` *and* widened its parameter. One row, and it is about
    // the widening: in Go a moved file is the same exported name.
    let draw = change(&diff, &package, "Draw");
    assert_eq!(
        draw.reasons.len(),
        1,
        "the move is not a change, so it must not be reported as one: {:?}",
        draw.reasons
    );
    assert!(has_reason(draw, "parameter_type_widened"));
    assert_eq!(
        draw.location, "shape/draw.go:6",
        "and the location is where the name lives now"
    );

    let name = change(&diff, &package, "Rect.Name");
    assert_eq!(name.class, Class::Deprecating, "the doc comment said so");

    // Unexported names are not surface in either revision.
    assert!(
        !after
            .package(&package)
            .expect("the package")
            .items
            .iter()
            .any(|item| item.id.name == "helper" && item.on_surface),
        "Go's first rune is the access rule"
    );
}

/// Ruby: the file is the surface, and the report says why in the same breath.
#[test]
fn ruby_reports_the_files_not_the_symbols() {
    let before = surface("ruby", "before");
    let after = surface("ruby", "after");
    assert_eq!(
        fathom_analyze::api::extract::access_of(Language::Ruby),
        fathom_analyze::api::Access::File
    );
    let diff = diff("ruby", &before, &after);

    let removed = change(&diff, "(root)", "lib/demo/shape.rb");
    assert_eq!(removed.class, Class::Breaking);
    assert!(has_reason(removed, "entry_file_removed"));

    let added = change(&diff, "(root)", "lib/demo/extra.rb");
    assert_eq!(added.class, Class::Additive);
    assert!(has_reason(added, "entry_file_added"));

    // The entry point's own load set moved both ways, and *fewer* files is not an addition.
    let entry = change(&diff, "(root)", "lib/demo.rb");
    assert!(has_reason(entry, "loads_grew"));
    assert!(has_reason(entry, "loads_shrank"));
    assert_eq!(entry.class, Class::Breaking);

    // The sentence that makes the shape of the answer legible.
    let sentence = &diff.packages[0].surface_sentence;
    assert!(
        sentence.contains("file") && sentence.contains("symbol"),
        "the report has to say why Ruby is reported per file: {sentence}"
    );
}

// ==============================================================================================
// 2. "no public API change", the fast way
// ==============================================================================================

/// The byte rung: two revisions whose surface-bearing files are identical, decided without a parse.
#[test]
fn the_fast_path_answers_before_anything_is_parsed() {
    let policy = ApiPolicy::default_policy();
    let before = files(&corpus("rust", "before"));
    let after = files(&corpus("rust", "before"));
    let (before_sources, _) = Inputs::split(&before);
    let (after_sources, _) = Inputs::split(&after);
    let rung = quick(
        &fingerprint(&before_sources, &policy),
        &fingerprint(&after_sources, &policy),
    );
    match rung {
        Quick::NoPublicApiChange {
            files,
            surface_bearing,
            ignored,
            test_files,
        } => {
            assert_eq!(
                files, 4,
                "the four `.rs` files; Cargo.toml is a manifest, not a source"
            );
            assert_eq!(surface_bearing, 4);
            assert_eq!(ignored, 0);
            assert_eq!(test_files, 0);
        }
        Quick::NeedsSurfaceDiff { changed, .. } => {
            panic!("identical revisions asked for a surface diff: {changed:?}")
        }
    }
    assert!(
        !rung.sentence().is_empty(),
        "the fast path still has something to say"
    );
}

/// A changed `README.md` is not an API question, and the fast path is the reason it costs nothing.
#[test]
fn a_changed_document_is_not_a_surface_change() {
    let policy = ApiPolicy::default_policy();
    let mut after = files(&corpus("rust", "before"));
    after.push(("README.md".to_string(), b"# shapes\n\nreworded\n".to_vec()));
    let before = files(&corpus("rust", "before"));
    let (before_sources, _) = Inputs::split(&before);
    let (after_sources, _) = Inputs::split(&after);
    let rung = quick(
        &fingerprint(&before_sources, &policy),
        &fingerprint(&after_sources, &policy),
    );
    match rung {
        Quick::NoPublicApiChange {
            surface_bearing,
            ignored,
            ..
        } => {
            assert_eq!(surface_bearing, 4);
            assert_eq!(ignored, 0);
        }
        other => panic!("a documentation change asked for a surface diff: {other:?}"),
    }
}

/// The entry point takes the fast path *for* its caller, so no caller can forget to.
#[test]
fn the_entry_point_takes_the_fast_path_itself() {
    let policy = ApiPolicy::default_policy();
    let files = files(&corpus("rust", "before"));
    let before = inputs_of(&files, &policy);
    let after = inputs_of(&files, &policy);
    match fathom_analyze::api::compare(&before, &after, None) {
        Answer::NoPublicApiChange { quick } => {
            assert!(matches!(quick, Quick::NoPublicApiChange { .. }))
        }
        Answer::Changed { .. } => panic!("the revisions were identical"),
    }
}

/// The digest rung: the files changed, the declarations did not. This is the answer that has to exist,
/// because most episodes are comments, bodies and formatting.
#[test]
fn the_digest_rung_parses_and_finds_nothing() {
    let policy = ApiPolicy::default_policy();
    let before = files(&corpus("go", "before"));
    // A comment above a declaration, and a body: both are inside a surface-bearing file, so the byte
    // rung cannot answer, and both are outside every header, so the digest of the surface is identical.
    let after: Vec<(String, Vec<u8>)> = before
        .iter()
        .map(|(path, bytes)| {
            if path.ends_with("shape.go") {
                let text = String::from_utf8(bytes.clone()).expect("utf-8");
                let reworded = text.replace(
                    "// Rect is a rectangle.",
                    "// Rect is a rectangle, drawn and measured and reworded here on purpose.",
                );
                let reworded = reworded.replace("return a - b", "return (a - b).abs()");
                (path.clone(), reworded.into_bytes())
            } else {
                (path.clone(), bytes.clone())
            }
        })
        .collect();
    let before_surface = extract_surface(&inputs_of(&before, &policy));
    let after_surface = extract_surface(&inputs_of(&after, &policy));
    let diff = fathom_analyze::api::diff::diff(&before_surface, &after_surface, None);
    assert_eq!(
        diff.answered,
        Answered::Digest,
        "the byte rung could not answer and the surface digests are equal"
    );
    assert!(diff.no_public_api_change());
    assert!(
        diff.changes().is_empty(),
        "a comment and a body are not a public API change"
    );
    assert_eq!(
        diff.cost.items_identical, diff.cost.items_compared,
        "and the cost line says the comparison happened: {:?}",
        diff.cost
    );
}

/// A file that could not be read makes the answer incomplete, and "no change" is not available to it.
#[test]
fn an_unread_file_makes_the_answer_incomplete_not_clean() {
    let policy = ApiPolicy::default_policy();
    let files = files(&corpus("go", "before"));
    let mut small = policy.clone();
    // The same two revisions, read with a file-size bound that refuses the bigger file. Nothing about
    // the repository changed, and the answer must not be "no public API change".
    small.max_items_per_package = policy.max_items_per_package;
    let mut before = inputs_of(&files, &policy);
    let mut after = inputs_of(&files, &policy);
    // A file too large for the policy is the shape of the case; the extractor records it as unread.
    let extract = ExtractPolicy {
        max_file_bytes: 1,
        ..ExtractPolicy::default()
    };
    before.extract = extract.clone();
    after.extract = extract;
    let diff =
        fathom_analyze::api::diff::diff(&extract_surface(&before), &extract_surface(&after), None);
    assert_eq!(diff.answered, Answered::Incomplete);
    assert!(
        !diff.no_public_api_change(),
        "an answer built from files that could not be read is not a clean bill of health: {:?}",
        diff.gaps
    );
    assert!(!diff.gaps.is_empty(), "and the hole is named");
    let _ = small;
}

// ==============================================================================================
// 3. every change carries a `file:line`, and the episode that made it
// ==============================================================================================

/// The acceptance criterion read literally: no change in any fixture is allowed to be anonymous.
#[test]
fn every_change_carries_a_file_and_line() {
    for language in ["rust", "python", "typescript", "go", "ruby"] {
        let before = surface(language, "before");
        let after = surface(language, "after");
        let diff = diff(language, &before, &after);
        for change in diff.changes() {
            let (path, line) = change
                .location
                .rsplit_once(':')
                .unwrap_or_else(|| panic!("{} has no location", change.item.name));
            assert!(path.contains('/') || path.contains('.'), "{path} is a path");
            let line: usize = line.parse().unwrap_or_else(|_| panic!("{line} is a line"));
            assert!(line > 0, "one-based, straight from the extractor's span");
            // The line is in a file the fixture actually has.
            let file = corpus(language, "after").join(path);
            let existed = file.is_file() || corpus(language, "before").join(path).is_file();
            assert!(existed, "{} points at no fixture file", change.location);
        }
    }
}

/// Which episode made it: the trail answers per item, and an attribution problem is a named row.
#[test]
fn the_episode_that_made_each_change() {
    let policy = ApiPolicy::default_policy();
    let v0 = files(&corpus("rust", "before"));
    let v1 = files(&corpus("rust", "after"));
    let v2 = files(&corpus("typescript", "after"));
    let s0 = extract_surface(&inputs_of(&v0, &policy));
    let s1 = extract_surface(&inputs_of(&v1, &policy));

    let mut trail = Trail::new();
    let episode = Episode::new(
        "ep-1",
        "rev-1",
        &["geo.rs", "shapes.rs", "lib.rs", "secret.rs", "Cargo.toml"],
    )
    .with_base("rev-0");
    let first = trail.record(&s0, &s1, &episode).clone();
    assert_eq!(first.episode.as_deref(), Some("ep-1"));
    for change in first.changes() {
        assert_eq!(
            change.episode.as_deref(),
            Some("ep-1"),
            "every change in an attributed diff carries the episode"
        );
        assert_eq!(change.revision.as_deref(), Some("rev-1"));
    }
    assert!(
        first.attribution_gaps.is_empty(),
        "the episode did touch these files: {:?}",
        first.attribution_gaps
    );
    let gone = change(&first, "shapes", "gone").item.clone();
    assert_eq!(
        trail
            .caused(&gone)
            .iter()
            .map(|episode| episode.id.clone())
            .collect::<Vec<_>>(),
        vec!["ep-1".to_string()],
        "the removal of `gone` is this episode's doing"
    );
    assert_eq!(
        trail
            .since("ep-1")
            .iter()
            .map(|change| change.item.name.clone())
            .collect::<Vec<_>>()
            .len(),
        first.changes().len(),
        "`since` is the same question from the other end"
    );

    // A second episode that touched nothing it could have caused: its own diff is empty, and the
    // earlier change is still the first episode's.
    let s_types = extract_surface(&inputs_of(&v2, &policy));
    let second = Episode::new("ep-2", "rev-2", &["shapes.ts"]);
    let empty = trail.record(&s_types, &s_types, &second).clone();
    assert!(
        empty.changes().is_empty(),
        "the same revision diffed against itself changes nothing"
    );
    assert_eq!(trail.len(), 2);
    assert!(
        trail
            .caused(&gone)
            .iter()
            .all(|episode| episode.id == "ep-1"),
        "an episode that touched nothing cannot cause anything"
    );
    assert!(
        trail
            .caused(&fathom_analyze::api::ItemId {
                package: "nowhere".to_string(),
                path: "nowhere.rs".to_string(),
                name: "unseen".to_string(),
                kind: fathom_analyze::structure::SymbolKind::Fn,
            })
            .is_empty(),
        "an item nothing touched has no episode, which is not the same as a claim"
    );
}

// ==============================================================================================
// 4. the recommendation, with its reason
// ==============================================================================================

/// The bump is not the finding; the sentence is.
#[test]
fn the_recommendation_names_the_change_that_drove_it() {
    let before = surface("rust", "before");
    let after = surface("rust", "after");
    let diff = diff("rust", &before, &after);
    let package = diff
        .packages
        .iter()
        .find(|p| p.name == "shapes")
        .expect("shapes");
    let recommendation = &package.recommendation;
    assert_eq!(recommendation.bump, Bump::Major, "a name was removed");
    let driver = recommendation.driver.as_ref().expect("a driver");
    assert_eq!(
        driver.item, "gone",
        "the worst change drives, and it is named: {driver:?}"
    );
    assert_eq!(driver.location, "geo.rs:11");
    assert_eq!(driver.reason.code(), "item_removed");
    assert!(
        recommendation.why.contains("gone")
            && recommendation.why.contains("geo.rs:11")
            && recommendation.why.contains("item_removed"),
        "the sentence must carry the name, the place and the reason code: {}",
        recommendation.why
    );
    assert!(
        recommendation.smaller.is_some(),
        "a removal can always be made smaller by deprecating first"
    );
    assert_eq!(recommendation.counts, package.counts);

    // The additive-only case: a package whose only changes are additive is a minor, and says which.
    let only_additive = inline(&[
        ("lib.rs", "pub mod a;\npub use crate::a::*;\n"),
        ("a.rs", "pub fn kept() -> u32 { 1 }\n"),
    ]);
    let only_additive_after = inline(&[
        ("lib.rs", "pub mod a;\npub use crate::a::*;\n"),
        (
            "a.rs",
            "pub fn kept() -> u32 { 1 }\n\npub fn added() -> u32 { 2 }\n",
        ),
    ]);
    let policy = ApiPolicy::default_policy();
    let added = fathom_analyze::api::diff::diff(
        &extract_surface(&inputs_of(&only_additive, &policy)),
        &extract_surface(&inputs_of(&only_additive_after, &policy)),
        None,
    );
    let recommendation = &added.packages[0].recommendation;
    assert_eq!(recommendation.bump, Bump::Minor);
    assert!(
        recommendation.why.contains("added") && recommendation.why.contains("item_added"),
        "{}",
        recommendation.why
    );
}

/// A package the repository declared internal gets its changes and no version.
#[test]
fn a_private_package_gets_no_recommendation() {
    let mut policy = ApiPolicy::default_policy();
    policy.private = vec!["shapes".to_string()];
    let before = extract_surface(&inputs_of(&files(&corpus("rust", "before")), &policy));
    let after = extract_surface(&inputs_of(&files(&corpus("rust", "after")), &policy));
    let diff = fathom_analyze::api::diff::diff(&before, &after, None);
    let package = diff
        .packages
        .iter()
        .find(|p| p.name == "shapes")
        .expect("shapes");
    assert_eq!(package.recommendation.bump, Bump::Internal);
    assert!(
        !package.changes.is_empty(),
        "the changes are still reported — private is not invisible"
    );
    assert!(
        package.recommendation.why.contains("private") || package.recommendation.caveat.is_some(),
        "and the report says why no bump was offered"
    );
}

// ==============================================================================================
// 5. the honest limits
// ==============================================================================================

/// A name that vanishes from a scope the extractor cannot enumerate is *not* a removal. This is the
/// acceptance criterion about macros, and it is the one that would be quietly wrong by default.
#[test]
fn a_synthetic_scope_that_shrinks_says_so_instead_of_accusing() {
    let policy = ApiPolicy::default_policy();
    // `@dataclass` writes methods, and `Shape` in the fixture corpus is decorated for exactly that
    // reason. A name that was listed and is not now *may* have been a decorator's doing, and the only
    // honest answer is that this reader cannot tell.
    let before = inline(&[
        (
            "shapes/__init__.py",
            "from .models import Shape\n\n__all__ = [\"Shape\"]\n",
        ),
        (
            "shapes/models.py",
            "from dataclasses import dataclass\n\n\
             @dataclass\n\
             class Shape:\n\
             \x20   name: str\n\n\
             \x20   def area(self) -> float:\n\
             \x20       return 0.0\n\n\
             \x20   def describe(self) -> str:\n\
             \x20       return self.name\n",
        ),
    ]);
    let after = inline(&[
        (
            "shapes/__init__.py",
            "from .models import Shape\n\n__all__ = [\"Shape\"]\n",
        ),
        (
            "shapes/models.py",
            "from dataclasses import dataclass\n\n\
             @dataclass\n\
             class Shape:\n\
             \x20   name: str\n\n\
             \x20   def area(self) -> float:\n\
             \x20       return 0.0\n",
        ),
    ]);
    let before = extract_surface(&inputs_of(&before, &policy));
    let after = extract_surface(&inputs_of(&after, &policy));
    let gone = after
        .package("shapes")
        .expect("shapes")
        .items
        .iter()
        .find(|item| item.id.name == "Shape.area");
    assert!(
        gone.map(|item| item.synthetic).unwrap_or(true),
        "a member of a decorated class is in a scope that cannot be enumerated"
    );
    let diff = fathom_analyze::api::diff::diff(&before, &after, None);
    let missing = diff
        .packages
        .iter()
        .flat_map(|package| &package.changes)
        .find(|change| change.item.name == "Shape.describe")
        .unwrap_or_else(|| {
            panic!(
                "the decorated member was removed from the list and nothing was said: {:?}",
                diff.changes()
                    .iter()
                    .map(|change| change.item.name.clone())
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        missing.synthetic,
        "the extractor said the scope cannot be enumerated"
    );
    assert_eq!(missing.class, Class::Indeterminate);
    assert!(has_reason(missing, "may_have_shrunk"));
    assert!(
        !has_reason(missing, "item_removed"),
        "a name a decorator may still be writing is not a removal"
    );
    assert_eq!(
        diff.packages[0].recommendation.bump,
        Bump::Indeterminate,
        "and the package's bump refuses to accuse"
    );
}

/// The `[api]` policy knobs, each one changing exactly what it says it changes.
#[test]
fn the_policy_knobs_are_honoured() {
    let (policy, notices) = ApiPolicy::from_repo_config_text(
        "[api]\nmode = \"deny\"\nignore = [\"geo.rs\"]\nprivate = [\"shapes\"]\n",
    );
    assert!(
        notices.is_empty(),
        "a well-formed table warns about nothing: {notices:?}"
    );
    assert_eq!(policy.mode, fathom_analyze::api::Mode::Deny);
    let before = extract_surface(&inputs_of(&files(&corpus("rust", "before")), &policy));
    let after = extract_surface(&inputs_of(&files(&corpus("rust", "after")), &policy));
    let diff = fathom_analyze::api::diff::diff(&before, &after, None);
    let names: Vec<String> = names(&diff, Class::Breaking);
    assert!(
        !names
            .iter()
            .any(|name| name == "gone" || name == "distance"),
        "the ignored file's changes are not tracked: {names:?}"
    );
    assert!(
        names.iter().any(|name| name == "Rect.width"),
        "the tracked file's changes still are: {names:?}"
    );
    assert!(
        before.files_ignored > 0 && !before.ignored_paths.is_empty(),
        "and the exclusion is counted in the open"
    );
    assert!(
        !diff.gaps.is_empty(),
        "an exclusion is a hole in the claim, not a silent narrowing"
    );
    assert_eq!(
        diff.packages
            .iter()
            .find(|package| package.name == "shapes")
            .expect("shapes")
            .recommendation
            .bump,
        Bump::Internal
    );

    // A pinned package name wins over the manifest, which is the whole point of the pin: the Go
    // fixture's package would be `example.com/shapes/shape` from `go.mod` plus the directory.
    let (pinned, notices) =
        ApiPolicy::from_repo_config_text("[api]\npackage = [\"shape = pinned-shape\"]\n");
    assert!(notices.is_empty(), "{notices:?}");
    let before = extract_surface(&inputs_of(&files(&corpus("go", "before")), &pinned));
    let names: Vec<String> = before
        .packages
        .iter()
        .map(|package| package.name.clone())
        .collect();
    assert!(
        names.contains(&"pinned-shape".to_string()),
        "the pin renamed the package: {names:?}"
    );
    let pinned_package = before
        .packages
        .iter()
        .find(|package| package.name == "pinned-shape")
        .expect("the pinned package");
    assert_eq!(
        pinned_package.decided_by, "api.package pin",
        "and the report says so"
    );
}

/// A malformed table is a notice and a default, never a silent zero.
#[test]
fn a_broken_table_is_a_notice_not_a_default_silently() {
    let (policy, notices) = ApiPolicy::from_repo_config_text("[api]\nmode = \"shout\"\n");
    assert_eq!(policy.mode, fathom_analyze::api::Mode::Warn);
    assert_eq!(
        policy.max_items_per_package,
        fathom_analyze::api::policy::DEFAULT_MAX_ITEMS_PER_PACKAGE
    );
    assert!(
        notices.iter().any(|notice| notice.contains("mode")),
        "{notices:?}"
    );
}

/// The JSON the future gate will read: the codes are the stable vocabulary, not the sentences.
#[test]
fn the_json_export_carries_the_codes() {
    let diff = diff(
        "rust",
        &surface("rust", "before"),
        &surface("rust", "after"),
    );
    let json = report::to_json(&diff);
    let text = json.to_string();
    for code in [
        "item_removed",
        "required_parameter_added",
        "return_type_changed",
        "receiver_changed",
        "member_added",
        "deprecated",
        "value_changed",
    ] {
        assert!(text.contains(code), "{code} missing from {text}");
    }
    assert_eq!(json["answered"], "items");
    // 2 is `reco-u1i`: test files are not surface, and the shape gained the two counts that say how
    // many paths it declined. A reader that pins this is pinning what a stored row means.
    assert_eq!(json["schema_version"], 2);
    let changes = json["packages"][0]["changes"].as_array().expect("changes");
    assert!(
        changes.iter().all(|change| change["location"].is_string()
            && change["reasons"].is_array()
            && change["class"].is_string()),
        "every change row carries its location, its class and its reasons"
    );
}

/// The report a human reads, in the order a reviewer reads it.
#[test]
fn the_report_leads_with_the_bump_and_ends_with_the_holes() {
    let diff = diff(
        "rust",
        &surface("rust", "before"),
        &surface("rust", "after"),
    );
    let lines = report::lines(&diff);
    let joined = lines.join("\n");
    assert!(joined.contains("major"), "{joined}");
    assert!(joined.contains("geo.rs:11"), "{joined}");
    assert!(joined.contains("would have been smaller") || joined.contains("smaller"));
    let headline = lines.first().expect("a headline");
    assert!(
        headline.contains("cost:"),
        "the cost is printed beside the answer, in the first line: {headline}"
    );
    assert!(headline.contains("package"), "{headline}");
    assert!(
        diff.caveats.is_empty() && diff.gaps.is_empty(),
        "a fully read surface says nothing in the margin: {:?} {:?}",
        diff.caveats,
        diff.gaps
    );

    // And when there *is* something in the margin, it comes after the changes, because a reviewer
    // scanning a bad diff reads the top of it.
    let noisy = fathom_analyze::api::diff::diff(
        &surface("typescript", "before"),
        &surface("typescript", "after"),
        None,
    );
    let noisy = report::lines(&noisy);
    assert!(
        noisy
            .last()
            .is_some_and(|line| line.starts_with("note:") || line.starts_with("gap:")),
        "the caveats come last: {:?}",
        noisy.last()
    );
    let exit = report::exit_code(&diff, &ApiPolicy::default_policy());
    assert_eq!(exit, 0, "warn mode never fails a build");
    let mut deny = ApiPolicy::default_policy();
    deny.mode = fathom_analyze::api::Mode::Deny;
    assert_eq!(
        report::exit_code(&diff, &deny),
        7,
        "breaking changes in deny mode are the violation exit (`e6.6`'s gate will use it)"
    );
}

/// The surface of one revision on its own: the cold-index answer, and what `--surface` would print.
#[test]
fn one_revisions_surface_speaks_for_itself() {
    let surface = surface("rust", "before");
    let lines = report::surface_lines(&surface);
    let joined = lines.join("\n");
    assert!(joined.contains("public API surface"));
    assert!(joined.contains("grammars in this build"));
    assert!(joined.contains("Rust:"));
    let json = report::surface_to_json(&surface);
    assert_eq!(
        json["schema_version"], 2,
        "see `api::surface::SCHEMA_VERSION`"
    );
    assert!(
        json["files_tests"].is_number() && json["test_paths"].is_array(),
        "the surface JSON says how many test paths it declined: {json}"
    );
    assert!(
        json["policy"].is_string(),
        "the policy is echoed as its own sentence: {}",
        json["policy"]
    );
}
/// Java: `public` is the export rule, a package-private type is its own row, and a package-private
/// type that becomes public is additive.
/// Java: `public` is the export rule, a required constructor parameter is breaking, a renamed method
/// is a removal and an addition under package identity, and a package-private class is not surface —
/// the row the report prints instead of pretending Java's default access is private.
/// Java: `public` is the export rule, a package-private type is its own visibility class rather than
/// "private", and a package-private type that becomes public is *additive* — reported once, not as a
/// removal, an addition and an unexplained text change on top.
#[test]
fn java_public_types_and_the_package_private_row() {
    let before = surface("java", "before");
    let after = surface("java", "after");
    let package = "com.acme.pay";
    assert_eq!(
        before.packages[0].name, package,
        "the package clause is the unit"
    );
    let diff = diff("java", &before, &after);

    // A method renamed is a removal and an addition: under unit identity Java does not see `amount()`
    // and `amountCents()` as the same name.
    let gone = change(&diff, package, "Payment.amount");
    assert_eq!(gone.class, Class::Breaking);
    assert!(has_reason(gone, "item_removed"));
    let fresh = change(&diff, package, "Payment.amountCents");
    assert_eq!(fresh.class, Class::Additive);
    assert!(has_reason(fresh, "item_added"));

    // `refund()` now takes a String: every existing call site has to change.
    let refund = change(&diff, package, "Payment.refund");
    assert_eq!(refund.class, Class::Breaking);
    assert!(has_reason(refund, "required_parameter_added"));
    assert!(
        refund
            .location
            .starts_with("src/com/acme/pay/Payment.java:"),
        "{}",
        refund.location
    );

    // `Ledger` was package-private and is now `public`: one additive row, citing the visibility change.
    let ledger = change(&diff, package, "Ledger");
    assert_eq!(
        ledger.class,
        Class::Additive,
        "a widening is not breaking, and the header text repeating the keyword must not make it so: {:?}",
        ledger.reasons
    );
    assert!(has_reason(ledger, "visibility_widened"));
    assert!(
        !has_reason(ledger, "declaration_text_changed"),
        "the access keyword is written once by the author: {:?}",
        ledger.reasons
    );

    // A package-private type is carried as its own visibility class, not folded into "private": it is
    // reachable inside the package, which is the unit here.
    let ledger_before = before
        .package(package)
        .expect("the package")
        .items
        .iter()
        .find(|item| item.id.name == "Ledger")
        .expect("Ledger in the before revision");
    assert_eq!(ledger_before.visibility, Visibility::Scoped);
    assert!(
        !ledger_before.on_surface,
        "a package-private type is not on the surface of a package it was not exported from"
    );

    let package_diff = diff
        .packages
        .iter()
        .find(|pkg| pkg.name == package)
        .expect("the package");
    assert_eq!(package_diff.identity, Identity::Unit);
    assert_eq!(package_diff.recommendation.bump, Bump::Major);
}

// ==============================================================================================
// 6. a test file is not a surface (`reco-u1i`)
// ==============================================================================================
//
// The ticket: `fathom check`'s api check reported a Python test function as a breaking public API
// removal —
//
// ```text
// breaking test_the_gate_can_go_red_twelve_ways tests/test_release_contract.py:78
//   — public item removed
// ```
//
// The decision, written down in [`fathom_analyze::api::testpath`]: a file the language's own test
// convention claims is not on the surface. These tests are that decision, in both directions — the
// test tree is declined, and the three things that must *not* be declined alongside it are not.

/// Before: the test function under the name it had before the rename that started the ticket.
const TEST_BEFORE: &str = "\
def test_the_gate_can_go_red_twelve_ways():
    result = run(\"--self-test\")
    assert result.returncode == 0
";

/// After: the same test, renamed to say what it now covers. Ordinary maintenance.
const TEST_AFTER: &str = "\
def test_the_gate_can_go_red_every_way_it_can():
    result = run(\"--self-test\")
    assert result.returncode == 0
";

fn test_tree_pair(policy: &ApiPolicy) -> (Inputs<'static>, Inputs<'static>) {
    let library = ("src/geo.py", "def area(w, h):\n    return w * h\n");
    (
        inputs_of(
            &inline(&[library, ("tests/test_release_contract.py", TEST_BEFORE)]),
            policy,
        ),
        inputs_of(
            &inline(&[library, ("tests/test_release_contract.py", TEST_AFTER)]),
            policy,
        ),
    )
}

/// The acceptance criterion, as the finding that must never appear again: renaming a test function
/// is not a surface change at all, and the fast path is the one that says so.
#[test]
fn renaming_a_test_function_is_not_a_breaking_removal() {
    let policy = ApiPolicy::default_policy();
    let (before, after) = test_tree_pair(&policy);
    match fathom_analyze::api::compare(&before, &after, None) {
        Answer::NoPublicApiChange { quick } => match quick {
            Quick::NoPublicApiChange {
                surface_bearing,
                test_files,
                ..
            } => {
                assert_eq!(
                    surface_bearing, 1,
                    "the library module is the only surface-bearing file"
                );
                assert_eq!(
                    test_files, 1,
                    "the one test file is declined, and the answer says so"
                );
            }
            other => panic!("the fast path changed its mind: {other:?}"),
        },
        Answer::Changed { diff, .. } => panic!(
            "renaming a test function was reported as a surface change: {:?}",
            names(&diff, Class::Breaking)
        ),
    }
    // …and the same answer from the slow path, so the behaviour does not ride on the fast path alone.
    let surface_diff =
        fathom_analyze::api::diff::diff(&extract_surface(&before), &extract_surface(&after), None);
    assert!(
        surface_diff
            .packages
            .iter()
            .flat_map(|package| &package.changes)
            .all(|change| !change.item.name.starts_with("test_")),
        "the item-level diff still saw the test rename: {:?}",
        names(&surface_diff, Class::Breaking)
    );
}

/// The exclusion is stated, not assumed: counted, named with the rule that claimed the file, and
/// *not* a gap, because a decision about what a surface is does not make the answer incomplete.
#[test]
fn a_declined_test_tree_is_counted_and_named() {
    let policy = ApiPolicy::default_policy();
    let surface = extract_surface(&inputs_of(
        &inline(&[
            ("src/geo.py", "def area(w, h):\n    return w * h\n"),
            ("tests/test_release_contract.py", TEST_BEFORE),
            ("tests/conftest.py", "import pytest\n"),
        ]),
        &policy,
    ));
    assert_eq!(surface.files_tests, 2);
    assert_eq!(surface.test_paths.len(), 2, "{:?}", surface.test_paths);
    assert!(
        surface
            .test_paths
            .iter()
            .any(|path| path == "tests/test_release_contract.py (test_dir:tests)"),
        "the report has to name the rule that declined a path: {:?}",
        surface.test_paths
    );
    assert_eq!(
        surface.public_count(),
        1,
        "only the library's `area` is on the surface"
    );
    assert!(
        surface.gaps.is_empty(),
        "declining a test tree is a decision, not a hole: {:?}",
        surface.gaps
    );
    assert!(surface.complete(), "the answer is still a complete answer");
    assert!(
        surface
            .caveats
            .iter()
            .any(|caveat| caveat.contains("test files and contribute no items")),
        "the caveat is where the decision is told: {:?}",
        surface.caveats
    );
    assert!(
        surface
            .caveats
            .iter()
            .any(|caveat| caveat.contains("api.track_tests")),
        "the way back has to be named where the exclusion is: {:?}",
        surface.caveats
    );
}

/// The way back, tested: a repository whose tests *are* the published artefact gets the finding it
/// asked for, and the report says which key decided it.
#[test]
fn track_tests_puts_the_test_tree_back_on_the_surface() {
    let policy = ApiPolicy {
        track_tests: true,
        ..ApiPolicy::default_policy()
    };
    let (before, after) = test_tree_pair(&policy);
    let surface_before = extract_surface(&before);
    assert_eq!(surface_before.files_tests, 0, "nothing was declined");
    assert!(
        surface_before
            .policy
            .sentence()
            .contains("api.track_tests = true"),
        "{}",
        surface_before.policy.sentence()
    );
    let diff = fathom_analyze::api::diff::diff(&surface_before, &extract_surface(&after), None);
    assert!(
        names(&diff, Class::Breaking).contains(&"test_the_gate_can_go_red_twelve_ways".to_string()),
        "`track_tests = true` asked for the old behaviour and must get it: {:?}",
        names(&diff, Class::Breaking)
    );
}

/// The name convention is consulted only where the runner consults it. A function with a test-shaped
/// name in an ordinary module — which is what a fixture corpus is full of — is a function.
#[test]
fn a_test_shaped_name_in_an_ordinary_module_is_still_surface() {
    let policy = ApiPolicy::default_policy();
    let before = inputs_of(
        &inline(&[(
            "src/recommender/testsurface/corpus.py",
            "def test_standalone():\n    return 1\n",
        )]),
        &policy,
    );
    let after = inputs_of(
        &inline(&[(
            "src/recommender/testsurface/corpus.py",
            "def not_a_case_standalone():\n    return 1\n",
        )]),
        &policy,
    );
    let diff =
        fathom_analyze::api::diff::diff(&extract_surface(&before), &extract_surface(&after), None);
    assert!(
        names(&diff, Class::Breaking).contains(&"test_standalone".to_string()),
        "a module no runner collects is an ordinary module: {:?}",
        names(&diff, Class::Breaking)
    );
}

/// `django.test` is published, supported API. This is the counter-example that decides the table:
/// the plural `tests` is a test tree, the singular `test` is somebody's package.
#[test]
fn a_python_module_named_test_is_published_api() {
    let policy = ApiPolicy::default_policy();
    let before = extract_surface(&inputs_of(
        &inline(&[(
            "django/test/client.py",
            "def get(path):\n    return None\n\n\ndef post(path):\n    return None\n",
        )]),
        &policy,
    ));
    let after = extract_surface(&inputs_of(
        &inline(&[(
            "django/test/client.py",
            "def post(path):\n    return None\n",
        )]),
        &policy,
    ));
    assert_eq!(
        before.files_tests, 0,
        "`test/` is not a test tree in Python"
    );
    let diff = fathom_analyze::api::diff::diff(&before, &after, None);
    assert!(
        names(&diff, Class::Breaking).contains(&"get".to_string()),
        "a removal from a published module is a removal: {:?}",
        names(&diff, Class::Breaking)
    );
}

/// The other languages' conventions, in one place: cargo's `tests/`, Go's `_test.go`, Java's
/// `src/test/java`, Node's `__tests__`. Each is a fact about what a consumer can import, and each
/// leaves the library's own removals reported.
#[test]
fn every_language_owns_its_own_test_convention() {
    let policy = ApiPolicy::default_policy();
    let cases: [(&str, [(&str, &str); 2], &str); 4] = [
        (
            "rust",
            [
                (
                    "shapes/Cargo.toml",
                    "[package]\nname = \"shapes\"\nversion = \"0.1.0\"\n",
                ),
                (
                    "shapes/tests/api.rs",
                    "use shapes::area;\n\n#[test]\nfn area_works() {\n    assert!(area() > 0);\n}\n",
                ),
            ],
            "shapes/tests/api.rs",
        ),
        (
            "go",
            [
                ("store/go.mod", "module example.com/store\n\ngo 1.22\n"),
                (
                    "store/table_test.go",
                    "package store\n\nfunc TestTable() {\n}\n",
                ),
            ],
            "store/table_test.go",
        ),
        (
            "java",
            [
                (
                    "src/test/java/com/acme/WidgetTest.java",
                    "package com.acme;\n\npublic class WidgetTest {\n    public void checks() {\n    }\n}\n",
                ),
                (
                    "src/main/java/com/acme/Widget.java",
                    "package com.acme;\n\npublic class Widget {\n}\n",
                ),
            ],
            "src/test/java/com/acme/WidgetTest.java",
        ),
        (
            "ts",
            [
                (
                    "pkg/package.json",
                    "{ \"name\": \"@acme/pkg\", \"version\": \"1.0.0\" }\n",
                ),
                (
                    "pkg/__tests__/widget.spec.ts",
                    "export function checks(): void {}\n",
                ),
            ],
            "pkg/__tests__/widget.spec.ts",
        ),
    ];
    for (language, files, unread) in cases {
        let surface = extract_surface(&inputs_of(
            &inline(&[
                files[0],
                files[1],
                ("shapes/src/lib.rs", "pub fn area() -> u32 {\n    2\n}\n"),
            ]),
            &policy,
        ));
        assert!(
            surface.items().all(|item| item.id.path != unread),
            "{language}: {unread} contributed items to the surface"
        );
        assert!(
            surface
                .test_paths
                .iter()
                .any(|path| path.starts_with(unread)),
            "{language}: {unread} was not counted as declined: {:?}",
            surface.test_paths
        );
    }
}

/// Declining a language's only test file does not make the language absent. "Languages present" is a
/// fact about the corpus, not about what this reader chose to read — and the missing-grammar gap is
/// computed from that list, so getting this wrong would silence a real hole.
#[test]
fn a_declined_test_file_still_counts_as_its_language_being_present() {
    let policy = ApiPolicy::default_policy();
    let surface = extract_surface(&inputs_of(
        &inline(&[
            ("store/go.mod", "module example.com/store\n\ngo 1.22\n"),
            (
                "store/table_test.go",
                "package store\n\nfunc TestTable() {\n}\n",
            ),
        ]),
        &policy,
    ));
    assert_eq!(surface.files_tests, 1, "the one test file was declined");
    assert_eq!(surface.files_parsed, 0, "and nothing was parsed");
    assert!(
        surface.languages.iter().any(|found| found == "go"),
        "the corpus still contains Go: {:?}",
        surface.languages
    );
}

// ==============================================================================================
// 7. a docstring is prose, and prose is not a signature (`reco-9k3`)
// ==============================================================================================
//
// The ticket: `fathom check --only api` over this repository's own history reported five breaking
// signature removals in a Python file whose signatures had not been touched —
//
// ```text
// breaking prose_units tools/check_ticket_claims.py:226 — parameter removed — position 1
//   (`text: str`) is gone; return type changed — was `list[tuple[int, str]]`, now `(none)`
// ```
//
// The shape of the wrongness said where it came from: the head side carried the item's name and its
// line — both true, both read from the real file — and no parameters and no return type. A header
// parsed to a bare name, paired against a base side parsed in full, is exactly "every parameter
// removed", and it is what the byte scanner produced whenever a Python docstring crossed a newline
// while carrying a quote in its prose: a literal was being ended at its newline, which is right for
// `"short"` and wrong for `"""a docstring"""`, so the rest of the prose went back through the scanner
// as source and the quotes in it blanked the declarations underneath. The scanner is supposed to say
// *unparsed* when it cannot read a header (`Reason::SignatureUnparsed`); here it had read something,
// and that something was empty.
//
// The pair below is the real one, small: the only thing that changes is prose — a paragraph in the
// module docstring, a quoted phrase in the function's own, and a line of the body.

/// Before: a module whose docstring already carries a quote.
const PROSE_BEFORE: &str = "\
\"\"\"What counts as a claim of unbuilt work.

A line that names a plan key **and** says the thing is missing: \"is not built\", \"will fill\".
\"\"\"
from __future__ import annotations


def claims(unit: str, keys: dict[str, str]) -> list[str]:
    \"\"\"Which keys a unit claims.\"\"\"
    return [key for key in keys if key in unit]


def spotlight(unit: str, key: str, width: int = 150) -> str:
    \"\"\"The line around the claim, for the report.\"\"\"
    return unit[:width]
";

/// After: the same two signatures, the same parameters and return types, and three times as much
/// prose — which is the whole edit the ticket's diff contained. The quoted list is load-bearing: a
/// docstring whose prose carries an odd number of quotes across a wrapped line, and a bracket pair
/// split across that wrap, is exactly what the scanner used to misread.
const PROSE_AFTER: &str = "\
\"\"\"What counts as a claim of unbuilt work.

A line that names a plan key **and** says the thing is missing: \"is not built\", \"will fill\".

Widened to the prose pages, which is the review of every hit they carried: \"is not built\", \"not
wired\", \"will
land\" — anywhere on the line (or
paragraph, in prose);
\"\"\"
from __future__ import annotations


def claims(unit: str, keys: dict[str, str]) -> list[str]:
    \"\"\"Which keys a unit claims.\"\"\"
    return [key for key in keys if key in unit and negated(unit)]


def spotlight(unit: str, key: str, width: int = 150) -> str:
    \"\"\"The line around the claim, for the report.\"\"\"
    return unit[:width]
";

/// The acceptance criterion: a function whose signature did not change is not reported as a breaking
/// removal, and the head-side signature still says what the source says.
#[test]
fn a_wider_docstring_is_not_a_signature_removal() {
    let policy = ApiPolicy::default_policy();
    let before = extract_surface(&inputs_of(
        &inline(&[("prose/__init__.py", PROSE_BEFORE)]),
        &policy,
    ));
    let after = extract_surface(&inputs_of(
        &inline(&[("prose/__init__.py", PROSE_AFTER)]),
        &policy,
    ));

    // The head side read the header it was given: two parameters and a return type, not a bare name.
    for (which, surface) in [("before", &before), ("after", &after)] {
        let item = surface
            .package("prose")
            .expect("the package is the module")
            .items
            .iter()
            .find(|item| item.id.name == "claims")
            .unwrap_or_else(|| panic!("{which} declares `claims`"));
        assert!(
            !item.signature.unparsed,
            "{which}: the header decomposed, or it should have said so"
        );
        let params: Vec<&str> = item
            .signature
            .params
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(
            params,
            vec!["unit", "keys"],
            "{which}: the parameters are the ones the source declares"
        );
        assert_eq!(
            item.signature.returns.as_deref(),
            Some("list[str]"),
            "{which}: the return type survived the docstring"
        );
    }

    // And the pair produces no breaking change at all: only prose moved.
    let diff = diff("python", &before, &after);
    let breaking: Vec<String> = diff
        .packages
        .iter()
        .flat_map(|package| &package.changes)
        .filter(|change| change.class == Class::Breaking)
        .map(|change| format!("{} {}", change.class.as_str(), change.item.name))
        .collect();
    assert!(
        breaking.is_empty(),
        "prose is not a signature: {breaking:?}"
    );
}

/// The same file read alone, with the quote that started the ticket sitting in the *function's* docstring
/// rather than the module's: every declaration below a quoted line of prose is still a declaration.
#[test]
fn every_declaration_under_quoted_prose_survives() {
    let surface = extract_surface(&inputs_of(
        &inline(&[("prose/__init__.py", PROSE_AFTER)]),
        &ApiPolicy::default_policy(),
    ));
    let package = surface.package("prose").expect("the module is the package");
    let named: Vec<&str> = package
        .items
        .iter()
        .filter(|item| item.on_surface)
        .map(|item| item.id.name.as_str())
        .collect();
    assert!(
        named.contains(&"claims") && named.contains(&"spotlight"),
        "both declarations are on the surface: {named:?}"
    );
    for item in &package.items {
        assert!(
            !item.signature.unparsed,
            "{}'s header was read, not given up on",
            item.id.name
        );
        assert!(
            !item.signature.params.is_empty(),
            "{} lost its parameters, which is the failure this test exists for: {}",
            item.id.name,
            item.signature.head
        );
    }
}

//! Golden structures, one fixture per ecosystem and per lockfile format (e2.5 / reco-j83, AC 1).
//!
//! Every directory under `tests/golden/deps/` is a tiny repository: the dependency files a real project
//! would have at its root, with their real names. Each has an `expected.json` next to it holding the exact
//! projection (see `mod projection`) of what the scan must say. The test walks the directory rather than
//! listing the cases, so a new fixture is a new directory and nothing else.
//!
//! The fixtures are small on purpose. A 40k-line `pnpm-lock.yaml` would be a more realistic input and a
//! worse golden file: what is being pinned is the *structure* the parser produces — name, version, source,
//! integrity, kind, edges, workspace membership — and every one of those claims is easier to check, and
//! easier to disagree with, in a fixture you can read at a glance. Bulk is covered by `deps_bench.rs`, and
//! malformed input by `deps_tolerance.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

mod projection;

use fathom_analyze::deps::{self, Content, Ecosystem, Format, Input, Options, Recognition};

/// Whether to rewrite the expected files instead of comparing them.
fn bless() -> bool {
    std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write")
}

fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("deps")
}

/// Every fixture case, sorted so a failure names them in the same order every run.
fn cases() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(golden_root())
        .expect("the golden directory exists")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    out.sort();
    out
}

/// Read one case directory into scan inputs, in path order.
fn inputs_for(case: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![case.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("the case directory is readable") {
            let entry = entry.expect("a readable directory entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name().is_some_and(|name| name == "expected.json") {
                continue;
            }
            let relative = path
                .strip_prefix(case)
                .expect("a file inside the case")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((
                relative,
                std::fs::read(&path).expect("a readable fixture file"),
            ));
        }
    }
    files.sort();
    files
}

fn scan_case(case: &Path) -> serde_json::Value {
    let files = inputs_for(case);
    let options = Options::default();
    let inputs = files.iter().map(|(path, bytes)| Input {
        path: path.as_str(),
        content: Content::Bytes(bytes),
    });
    let surface = deps::scan(inputs, &options);
    projection::dump_surface(&surface)
}

#[test]
fn every_fixture_directory_is_scanned_and_matches_its_expected_structure() {
    let mut checked = 0usize;
    for case in cases() {
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        let actual = scan_case(&case);
        let expected_path = case.join("expected.json");
        let actual_text = format!(
            "{}\n",
            serde_json::to_string_pretty(&actual).expect("a projection is always serializable")
        );
        if bless() {
            std::fs::write(&expected_path, &actual_text).expect("an expected file is writable");
            continue;
        }
        let expected_text = std::fs::read_to_string(&expected_path).unwrap_or_else(|_| {
            panic!(
                "`{name}` has no expected.json; run `FATHOM_GOLDEN=write cargo test --test deps_golden` \
                 and read the result before committing it"
            )
        });
        let expected: serde_json::Value = serde_json::from_str(&expected_text)
            .unwrap_or_else(|err| panic!("`{name}/expected.json` is not JSON: {err}"));
        assert_eq!(
            expected, actual,
            "`{name}` no longer parses to its golden structure\n--- expected ---\n{expected_text}\n--- actual ---\n{actual_text}"
        );
        checked += 1;
    }
    assert!(
        bless() || checked >= 10,
        "the ticket asks for golden fixtures across 10+ formats; only {checked} cases are present"
    );
    if bless() {
        // A bless run is a write, not a pass: it has to be followed by reading what it wrote.
        eprintln!("blessed {} golden cases", cases().len());
    }
}

#[test]
fn every_format_the_module_claims_has_a_fixture() {
    // The AC is per *format*, so this counts them: a format that gained a parser and lost its fixture
    // would otherwise be invisible until a repository hit it.
    let mut covered: Vec<&'static str> = Vec::new();
    for case in cases() {
        for (path, _) in inputs_for(&case) {
            if let Recognition::Recognized(format) = deps::recognize(&path) {
                covered.push(format.as_str());
            }
        }
    }
    covered.sort_unstable();
    covered.dedup();
    let claimed: Vec<&str> = [
        Format::PackageJson,
        Format::PackageLockJson,
        Format::YarnLock,
        Format::PnpmLock,
        Format::PnpmWorkspace,
        Format::CargoToml,
        Format::CargoLock,
        Format::PyProjectToml,
        Format::RequirementsTxt,
        Format::PoetryLock,
        Format::UvLock,
        Format::PipfileLock,
        Format::GoMod,
        Format::GoSum,
        Format::GoWork,
        Format::Gemfile,
        Format::GemfileLock,
        Format::MixExs,
        Format::MixLock,
        Format::PomXml,
        Format::BuildGradle,
        Format::GradleLockfile,
        Format::Csproj,
        Format::DirectoryPackagesProps,
        Format::PackagesLockJson,
    ]
    .iter()
    .map(|format| format.as_str())
    .collect();
    let missing: Vec<&str> = claimed
        .iter()
        .copied()
        .filter(|f| !covered.contains(f))
        .collect();
    assert!(
        missing.is_empty(),
        "these formats have no golden fixture: {missing:?}"
    );
}

/// Every `Format` must map to an ecosystem and a role, and no two formats may share a name — a
/// regression here would make two formats print the same string in `--json` and in a golden file.
#[test]
fn format_names_are_unique_and_totalled() {
    static FORMATS: LazyLock<Vec<Format>> = LazyLock::new(|| {
        use Format::*;
        [
            PackageJson,
            PackageLockJson,
            YarnLock,
            PnpmLock,
            PnpmWorkspace,
            CargoToml,
            CargoLock,
            PyProjectToml,
            RequirementsTxt,
            PoetryLock,
            UvLock,
            PipfileLock,
            GoMod,
            GoSum,
            GoWork,
            Gemfile,
            GemfileLock,
            MixExs,
            MixLock,
            PomXml,
            BuildGradle,
            GradleLockfile,
            Csproj,
            DirectoryPackagesProps,
            PackagesLockJson,
        ]
        .to_vec()
    });
    let mut names: Vec<&str> = FORMATS.iter().map(|format| format.as_str()).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), before, "two formats print the same `as_str()`");
    for format in FORMATS.iter() {
        // Each format's ecosystem has to be one of the eight, and its role one of three; both are total
        // functions, so this asserts the table is fully populated rather than the values.
        let ecosystem = format.ecosystem();
        assert!(
            Ecosystem::ALL.contains(&ecosystem),
            "{:?} has an ecosystem outside the table",
            format
        );
        assert!(!format.role().as_str().is_empty());
    }
}

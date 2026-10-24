//! A test file is not a public API surface (`reco-u1i`).
//!
//! # The decision, written down
//!
//! The api check once reported this repository against itself:
//!
//! ```text
//! breaking test_the_gate_can_go_red_twelve_ways tests/test_release_contract.py:78
//!   — public item removed — fn test_the_gate_can_go_red_twelve_ways was last seen at
//!   tests/test_release_contract.py:78 (class: breaking, access: Symbol)
//! ```
//!
//! The claim was false, and false in the way that costs a tool its authority. A test function is not
//! a contract a caller compiles against: nothing outside its file imports it, no build outside the
//! test runner can link it, and renaming one to say what it now tests is the ordinary business of
//! test maintenance. The extractor had been reading `tests/` as an exported package because nothing
//! had ever told it not to — the Python export rule ("module-level, no leading underscore") is
//! correct for a *library* module and says nothing at all about a file pytest collected by name.
//!
//! So the decision, in one line: **a file that the language's own test convention claims is not on
//! the public API surface, and contributes no items to it.**
//!
//! Three arguments, in order of how much they weigh:
//!
//! 1. **Nothing outside the file can name it.** Cargo compiles `crates/x/tests/*.rs` as its own
//!    crate that nothing can `use`; Go compiles `*_test.go` only for `go test`; Maven does not put
//!    `src/test/java` in the published artifact; a `.spec.ts` file is collected by the runner; and
//!    the only importer of `tests/test_release_contract.py` is pytest, which imports it by path in
//!    order to run it, not to link it. Every one of those is a *language* fact, not a convention
//!    this module invented.
//! 2. **Renaming a test is maintenance, not a breaking change.** The finding above was produced by a
//!    rename that made the test's name true again. A check that calls ordinary maintenance a breaking
//!    change does not get ignored politely: it gets suppressed wholesale, `[api] ignore`-ing a
//!    quarter of the repository to silence one class of lie, and then the four real removals in the
//!    library go unreported too.
//! 3. **The exclusion has to be stated, not assumed.** `[api] ignore` is a claim the repository's
//!    owner makes; this is a claim the *language* makes, so it is built in — and, exactly like
//!    `ignore`, it is counted and named in the report (`Surface::test_paths`) rather than quietly
//!    dropping files, because "no public API change" has to say how much of the repository it
//!    declined to look at.
//!
//! # Where the rule stops
//!
//! **The unit is the file, not the declaration.** A test file contributes nothing; a source file
//! contributes its exports as before. No per-symbol guess is made about what "looks like a test".
//!
//! **The name convention is consulted only where the runner consults it.** pytest collects files
//! named `test_*.py` and `*_test.py`; a function named `test_square` in `src/recommender/foo.py` is
//! a function with a test-shaped name in a module no runner will collect, and it is judged like any
//! other name. That is not a gap: this repository keeps sample corpora full of `def test_…`
//! functions in ordinary library modules *on purpose* ([`crate::structure`]'s fixtures), and a rule
//! that called those non-API would be a rule about identifiers rather than about callers.
//!
//! **`tests` is a directory marker; `test` is only sometimes one.** A directory named `tests`,
//! `__tests__` or `specs` is a test tree in every one of the six languages. A directory named
//! `test` is a test tree in Ruby (`test/`), Java (`src/test/java`) and Node (`test/`), and is
//! somebody's published API in Python — `django.test` is documented, supported surface, and a rule
//! that swallowed it would be the *opposite* of this ticket. So Python gets the plural only, and the
//! singular where the language's own layout makes it unambiguous.
//!
//! **Inline test modules ride their language's own rule.** A Rust `mod tests` inside `src/` is
//! already off the surface, because a private module is not a public path from the crate root and
//! the export rule says so; declaring `pub mod tests` and expecting it to be invisible to a caller
//! is a different mistake, and not one this table can catch from a path.
//!
//! # The way back
//!
//! `[api] track_tests = true`. A repository whose tests *are* the published artefact — a compliance
//! suite shipped as a package, a pytest plugin whose fixtures live in its test tree, a language
//! conformance suite other projects link — says so with that one key, and this table stops applying.
//! The default is `false` because that repository is rare and the one above is every repository.

use crate::structure::Language;
use serde::Serialize;

/// Why a file is a test file, in the language that owns the convention.
///
/// Carried as data rather than as a `bool` because the report has to name the rule that excluded a
/// path: "these 14 files are not surface" is a claim, and "because `tests/` is cargo's integration
/// test directory" is the part of it a reviewer can check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "rule")]
pub enum TestRule {
    /// The path has a directory segment the language's test layout owns (`tests/`, `spec/`, …).
    Directory {
        /// The segment that matched.
        segment: String,
    },
    /// The file's own name is the language's test-file name (`test_*.py`, `*_test.go`, `FooTest.java`).
    FileName {
        /// The spelling of the convention that matched, as a glob-shaped pattern.
        pattern: String,
    },
}

impl TestRule {
    /// The stable code a report and a baseline can both carry.
    #[must_use]
    pub fn code(&self) -> String {
        match self {
            TestRule::Directory { segment } => format!("test_dir:{segment}"),
            TestRule::FileName { pattern } => format!("test_name:{pattern}"),
        }
    }

    /// The sentence that says which convention claimed the file.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            TestRule::Directory { segment } => format!(
                "it lives under a `{segment}/` directory, which is where this language puts tests \
                 and where nothing outside the test runner can import from"
            ),
            TestRule::FileName { pattern } => format!(
                "its file name matches the test-file convention `{pattern}`, which is the name the \
                 test runner collects and nothing else loads"
            ),
        }
    }
}

/// Directory segments that mean "everything below here is a test", per language.
///
/// Deliberately not one shared list: `test` is a Ruby, Java and Node test directory and is
/// `django.test` in Python. See the module documentation.
#[must_use]
const fn test_segments(language: Language) -> &'static [&'static str] {
    match language {
        // Cargo's integration tests: `<crate>/tests/*.rs`, compiled as their own crate that
        // nothing can `use`. `examples/` is *not* here: an example is code that ships with the
        // crate, and whether it is a surface is a question this table does not pretend to answer.
        Language::Rust => &["tests"],
        // pytest's usual home, and the plural only — `django.test` is published API.
        Language::Python => &["tests", "__tests__"],
        Language::Go => &["tests"],
        // Minitest's `test/` and RSpec's `spec/`, both conventions a consumer never loads.
        Language::Ruby => &["test", "tests", "spec", "specs"],
        // Node and friends: `test/`, `__tests__/`, `spec/`.
        Language::TypeScript => &["test", "tests", "__tests__", "spec", "specs"],
        // Maven and Gradle: `src/test/java`.
        Language::Java => &["test", "tests"],
        Language::Unknown => &[],
    }
}

/// The rule that claims `path`, if one does.
///
/// Pure function of the path and the language, so it is cheap enough to run on every file in both
/// revisions of a diff — including on the byte fast path, where the whole point is that nothing is
/// parsed.
#[must_use]
pub fn rule(language: Language, path: &str) -> Option<TestRule> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some(segment) = path
        .rsplit('/')
        .skip(1)
        .find(|segment| test_segments(language).contains(segment))
    {
        return Some(TestRule::Directory {
            segment: segment.to_string(),
        });
    }
    file_rule(language, name).map(|pattern| TestRule::FileName {
        pattern: pattern.to_string(),
    })
}

/// Whether `path` is a test file in `language`.
#[must_use]
pub fn is_test_file(language: Language, path: &str) -> bool {
    rule(language, path).is_some()
}

/// The file-name conventions, per language. Each returned pattern is a *description* of the rule,
/// printed in the report; the match itself is spelled out below, because a glob engine that had to
/// run per file per revision would cost more than the rule is worth.
fn file_rule(language: Language, name: &str) -> Option<&'static str> {
    match language {
        Language::Python => {
            // Exactly pytest's default `python_files`, and nothing wider: the runner collects
            // `test_*.py` and `*_test.py`, and a module merely *named* `test` is somebody's API.
            let stem = name
                .strip_suffix(".pyi")
                .or_else(|| name.strip_suffix(".py"))?;
            if stem == "conftest" {
                return Some("conftest.py");
            }
            if stem.starts_with("test_") {
                return Some("test_*.py");
            }
            if stem.ends_with("_test") {
                return Some("*_test.py");
            }
            None
        }
        Language::Go => {
            if name.ends_with("_test.go") {
                Some("*_test.go")
            } else {
                None
            }
        }
        Language::Ruby => {
            if name.ends_with("_test.rb") {
                Some("*_test.rb")
            } else if name.ends_with("_spec.rb") {
                Some("*_spec.rb")
            } else {
                None
            }
        }
        Language::TypeScript => {
            // `foo.test.ts`, `foo.spec.tsx`, `foo.test.mjs`: the runner's own naming rule.
            for infix in [".test.", ".spec."] {
                if let Some(at) = name.find(infix) {
                    let rest = &name[at + infix.len()..];
                    if matches!(
                        rest,
                        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs"
                    ) {
                        return Some(if infix.starts_with(".test") {
                            "*.test.ts"
                        } else {
                            "*.spec.ts"
                        });
                    }
                }
            }
            None
        }
        Language::Java => {
            for suffix in [
                "Test.java",
                "Tests.java",
                "TestCase.java",
                "IT.java",
                "ITest.java",
            ] {
                if name.ends_with(suffix) {
                    return Some("*Test.java");
                }
            }
            None
        }
        // Rust's convention is a `tests/` directory (above) or a private `mod tests` inside a file,
        // and the module's own visibility already handles the second case.
        Language::Rust | Language::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Language::*;

    #[track_caller]
    fn claims(language: Language, path: &str, code: &str) {
        let found = rule(language, path).unwrap_or_else(|| panic!("{path} was not claimed"));
        assert_eq!(found.code(), code, "{path} was claimed by the wrong rule");
    }

    #[track_caller]
    fn leaves(language: Language, path: &str) {
        if let Some(found) = rule(language, path) {
            panic!("{path} was claimed by {}", found.code());
        }
    }

    #[test]
    fn a_python_test_tree_and_the_pytest_file_names_are_tests() {
        claims(Python, "tests/test_release_contract.py", "test_dir:tests");
        claims(
            Python,
            "src/recommender/api/test_surface.py",
            "test_name:test_*.py",
        );
        claims(Python, "tools/check_release_test.py", "test_name:*_test.py");
        claims(Python, "tests/conftest.py", "test_dir:tests");
        claims(Python, "src/conftest.py", "test_name:conftest.py");
    }

    #[test]
    fn a_python_module_named_test_is_not_a_test_tree() {
        // The counter-example that decides the whole table: `django.test` is published, supported
        // API, and swallowing it is the exact opposite of this ticket.
        leaves(Python, "django/test/client.py");
        leaves(Python, "django/testutils/test.py");
        // …while `django.tests` is not.
        claims(Python, "django/tests/test_client.py", "test_dir:tests");
    }

    #[test]
    fn a_test_shaped_name_in_an_ordinary_module_is_a_function() {
        // This repository keeps sample corpora of exactly this shape (reco-u1i's module docs), and
        // the name convention is only consulted where the runner consults it.
        leaves(Python, "src/recommender/testsurface/corpus.py");
        leaves(Python, "src/recommender/change/corpus.py");
        leaves(Python, "lib/testing.py");
        leaves(Python, "src/test.py");
    }

    #[test]
    fn cargo_go_ruby_java_and_node_conventions_are_tests() {
        claims(Rust, "crates/fathom-core/tests/api.rs", "test_dir:tests");
        leaves(Rust, "crates/fathom-core/examples/demo.rs");
        leaves(Rust, "crates/fathom-core/src/lib.rs");
        leaves(Rust, "crates/fathom-core/src/tests.rs");
        claims(Go, "store/table_test.go", "test_name:*_test.go");
        leaves(Go, "store/table.go");
        claims(Ruby, "test/unit/widget_test.rb", "test_dir:test");
        claims(Ruby, "spec/widget_spec.rb", "test_dir:spec");
        claims(Ruby, "lib/widget_spec.rb", "test_name:*_spec.rb");
        claims(
            Java,
            "src/test/java/com/acme/WidgetTest.java",
            "test_dir:test",
        );
        leaves(Java, "src/main/java/com/acme/Widget.java");
        claims(
            TypeScript,
            "packages/sdk/src/widget.test.ts",
            "test_name:*.test.ts",
        );
        claims(
            TypeScript,
            "packages/sdk/__tests__/widget.spec.tsx",
            "test_dir:__tests__",
        );
        leaves(TypeScript, "packages/sdk/src/widget.ts");
        leaves(TypeScript, "packages/sdk/src/testing.ts");
    }

    #[test]
    fn the_rule_needs_a_directory_boundary_not_a_substring() {
        leaves(Python, "tests_helpers/client.py");
        leaves(Python, "mytests/helper.py");
        leaves(TypeScript, "spectator/view.ts");
        // A file whose *own* name is the marker is a file, not a directory.
        leaves(Python, "tests.py");
    }

    #[test]
    fn the_rule_names_itself_because_the_report_has_to() {
        let found = rule(Python, "tests/test_release_contract.py")
            .unwrap_or_else(|| panic!("the rule the report promises to name"));
        assert_eq!(found.code(), "test_dir:tests");
        assert!(
            found.sentence().contains("`tests/`"),
            "{}",
            found.sentence()
        );
    }

    #[test]
    fn unknown_languages_are_never_claimed() {
        leaves(Unknown, "tests/notes.md");
        leaves(Unknown, "anything.test.txt");
    }
}

//! `untested.rs` — new logic arrived and nothing in the episode runs it (`e4.3`).
//!
//! **What it claims.** This episode added real, exported, callable logic to `src/**` and produced no test
//! that could plausibly be the one for it. That is a *question for the reviewer*, not a verdict: the
//! coverage may live in an integration suite that was not touched, and the message says so rather than
//! claiming the behaviour is uncovered.
//!
//! **Where `e2.9`'s boundary is.** The test-surface delta (`reco-6pl`) is the fine instrument: it parses
//! test *cases* per language, pairs them to subjects, and prices a coverage proxy per case. This detector
//! does not re-do that work and does not pretend to. What it adds is that the claim is made **per episode,
//! from the delta model, with a finding shape** — a reason, a jump, a suppression key, in a table an
//! attention queue can rank. Where `e2.9` lands in the tree it is read instead of re-derived: this
//! detector's job is the *absence*, and an absence is something a test parser cannot report.
//!
//! **Three guards, because "no tests" is the easiest false positive in the product.**
//!
//! 1. **new lines, not changed lines** — at least `detectors.untested.min_added_lines` (default 20) of
//!    added lines on the path, so a two-line fix is not asked for a new test;
//! 2. **a public declaration among the added lines** — `pub fn`, `export function`, `def` at module level,
//!    `func NewThing`, `public class`. A private helper, a reordered import block, a config table or a
//!    moved file has no new callable surface and gets nothing. `pub(crate)`, `_name` and `lowercase` names
//!    are not public;
//! 3. **a mirror rule on the test surface** — a test path whose name shares the source file's stem counts
//!    as the plausible cover (`src/parser/grammar.rs` ↔ `tests/grammar.rs`, `grammar_test.rs`,
//!    `test_grammar.py`, `grammar.test.ts`). Silence here means "a test named for this file was touched",
//!    which is a weaker claim than "this file is covered" and is worded that way.
//!
//! **Two confidences, not one.** When the episode touched *no* test at all the claim is stronger: `0.7`.
//! When it touched tests that do not mirror the file the claim is `0.55` and the message names the test
//! paths it saw, because a reviewer who is told "no test covers this" when `tests/api.rs` is in the diff
//! stops trusting the whole product after one miss.
//!
//! **What it cannot see, and says so by not speaking:** a test that already existed and already covers the
//! new branch; a language with no public/private marker it can read (C, Makefile, shell); a test runner
//! whose naming convention this mirror rule does not know — which is why the mirror is a *name* test and
//! not a path template.

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::detectors::text::{self, Dialect};
use crate::risk::Surface;

pub fn detect(episode: &Episode, _corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    let test_paths: Vec<&str> = episode
        .files
        .iter()
        .filter(|file| file.is_test())
        .map(|file| file.path.as_str())
        .collect();

    episode
        .files
        .iter()
        .filter(|file| qualifies(file, policy))
        .filter_map(|file| finding_for(episode, file, &test_paths, policy))
        .collect()
}

/// The three guards, as one predicate: cheap ones first.
fn qualifies(file: &FileChange, policy: &Policy) -> bool {
    !file.binary
        && !file.noisy()
        && !file.is_delete()
        && file.surface() == Surface::Source
        && file.add >= policy.untested_min_added_lines
        && text::dialect(&file.path).is_some_and(Dialect::is_code)
}

fn finding_for(
    episode: &Episode,
    file: &FileChange,
    test_paths: &[&str],
    policy: &Policy,
) -> Option<Finding> {
    let dialect = text::dialect(&file.path)?;
    let added = file.added();
    let declarations: Vec<&str> = added
        .iter()
        .copied()
        .filter(|line| is_public_declaration(line, dialect))
        .collect();
    if declarations.is_empty() {
        return None;
    }

    if test_paths.iter().any(|test| mirrors(&file.path, test)) {
        return None;
    }

    let named = crate::detectors::named(
        &declarations
            .iter()
            .map(|line| line.trim())
            .collect::<Vec<_>>(),
        3,
    );
    let (confidence, test_clause) = if test_paths.is_empty() {
        (
            0.7,
            format!(
                "the episode touched no test file at all ({} test paths, 0 of them named for `{}`)",
                test_paths.len(),
                stem(&file.path)
            ),
        )
    } else {
        (
            0.55,
            format!(
                "the episode touched {}, none of them named for `{}` — an existing suite may already \
                 cover it",
                crate::detectors::named(test_paths, policy.name_cap),
                stem(&file.path)
            ),
        )
    };

    Some(
        Finding::build(
            "untested_new_logic",
            1,
            &episode.id,
            file.path.clone(),
            format!(
                "{} added {} new line(s) including {} public declaration(s) — {named} — and {}",
                file.path,
                file.add,
                declarations.len(),
                test_clause
            ),
        )
        .severity(Severity::Medium)
        .confidence(confidence)
        .evidence(Evidence::observed(
            EvidenceKind::Hunk,
            format!(
                "{} {}",
                file.path,
                file.hunks
                    .first()
                    .map_or_else(String::new, |hunk| hunk.header.clone())
            ),
            crate::detectors::quote(&declarations, policy.evidence_lines),
        ))
        .evidence(Evidence::inferred(
            EvidenceKind::Range,
            format!("episode:{}#files", episode.id),
            format!(
                "{} file rows, test surface touched: {}",
                episode.files.len(),
                !test_paths.is_empty()
            ),
        ))
        .jump_path(&file.path)
        .finish(),
    )
}

/// Is this line declaring something callable *from outside*?
fn is_public_declaration(line: &str, dialect: Dialect) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    match dialect {
        Dialect::Rust => {
            (trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("pub unsafe fn ")
                || trimmed.starts_with("pub const ")
                || trimmed.starts_with("pub static ")
                || trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub trait ")
                || trimmed.starts_with("pub mod ")
                || trimmed.starts_with("pub use "))
                && !trimmed.starts_with("pub(crate)")
        }
        Dialect::Python => {
            (trimmed.starts_with("def ") || trimmed.starts_with("class "))
                && name_after(
                    trimmed,
                    trimmed.split_whitespace().next().unwrap_or_default(),
                )
                .is_some_and(|name| !name.starts_with('_'))
                && !line.starts_with([' ', '\t'])
        }
        Dialect::Ruby => {
            trimmed.starts_with("def ")
                && name_after(trimmed, "def").is_some_and(|name| !name.starts_with('_'))
        }
        Dialect::Go => {
            (trimmed.starts_with("func ") || trimmed.starts_with("type "))
                && name_after(
                    trimmed,
                    trimmed.split_whitespace().next().unwrap_or_default(),
                )
                .is_some_and(|name| name.starts_with(char::is_uppercase))
        }
        Dialect::JavaScriptTypeScript => {
            trimmed.starts_with("export ")
                && (trimmed.contains("function ")
                    || trimmed.contains("class ")
                    || trimmed.contains("const ")
                    || trimmed.contains("async ")
                    || trimmed.contains("interface ")
                    || trimmed.contains("type "))
        }
        Dialect::Java | Dialect::Kotlin | Dialect::CSharp => {
            trimmed.starts_with("public ") && !trimmed.starts_with("public static void main")
        }
        Dialect::Swift => trimmed.starts_with("public ") || trimmed.starts_with("open "),
        Dialect::Php => trimmed.starts_with("public function ") || trimmed.starts_with("function "),
        _ => false,
    }
}

/// The identifier following a keyword, with its original case intact: Python tests for a leading
/// underscore and Go tests for a leading capital, so folding happens nowhere but in the comparison.
fn name_after<'a>(trimmed: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix(keyword)?.trim_start();
    let end = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .unwrap_or(rest.len());
    if end == 0 { None } else { Some(&rest[..end]) }
}

/// The file stem, lower-cased, with a leading `test`/`test_` and a trailing `_test`/`.test` folded away.
fn stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name
        .split_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_ascii_lowercase();
    stem.trim_matches(|ch| ch == '_' || ch == '.').to_owned()
}

/// Does this test path plausibly cover this source path?
///
/// A name test, not a template: `tests/grammar.rs`, `grammar_test.rs`, `test_grammar.py`,
/// `grammar.test.ts` and `src/parser/grammar/tests.rs` all count, and so does `tests/api_grammar.py` —
/// because a stem contained in a longer name is how people actually name a test for one module, and a
/// template that missed those would fire on the biggest, best-tested repositories in the corpus.
fn mirrors(source: &str, test: &str) -> bool {
    let subject = stem(source);
    if subject.len() < 3 || subject == "mod" || subject == "lib" || subject == "init" {
        // A `mod.rs`/`lib.rs` has no stem worth pairing on; every test file "mirrors" it and none does.
        return false;
    }
    let folded = test.to_ascii_lowercase();
    folded.contains(&subject)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::model::{Content, Hunk, change};

    fn file(path: &str, added: &[&str]) -> FileChange {
        FileChange {
            path: path.to_owned(),
            change: change::ADDED.to_owned(),
            add: added.len() as i64,
            hunks: vec![Hunk::new(
                "@@ -0,0 +1,3 @@",
                added.to_vec(),
                Vec::<String>::new(),
            )],
            ..Default::default()
        }
    }

    fn episode(files: Vec<FileChange>) -> Episode {
        Episode {
            id: "ep-1".to_owned(),
            files,
            content: Content::Full,
            ..Episode::new("ep-1")
        }
    }

    fn many<'a>(lines: &'a [&'a str]) -> Vec<&'a str> {
        let mut out: Vec<&str> = lines.to_vec();
        while out.len() < 24 {
            out.push("        self.advance();");
        }
        out
    }

    #[test]
    fn new_public_logic_with_no_test_is_a_finding() {
        let file = file(
            "src/parser/grammar.rs",
            &many(&[
                "pub fn parse_grammar(input: &str) -> Grammar {",
                "pub struct Grammar {",
                "}",
            ]),
        );
        let findings = detect(&episode(vec![file]), &Corpus::default(), &Policy::default());
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "untested_new_logic");
        assert!(
            finding.message.contains("public declaration"),
            "{}",
            finding.message
        );
        assert!(finding.message.contains("no test file at all"));
        assert!((finding.confidence - 0.7).abs() < f64::EPSILON);
        assert_eq!(
            finding.suppression_key,
            "untested_new_logic:src/parser/grammar.rs"
        );
    }

    #[test]
    fn a_test_named_for_the_file_is_enough_to_stay_silent() {
        let files = vec![
            file(
                "src/parser/grammar.rs",
                &many(&["pub fn parse_grammar(input: &str) -> Grammar {", "}"]),
            ),
            file(
                "tests/grammar.rs",
                &many(&["#[test] fn parses_the_grammar() {", "assert!(true);"]),
            ),
        ];
        assert!(detect(&episode(files), &Corpus::default(), &Policy::default()).is_empty());
    }

    #[test]
    fn every_test_naming_style_the_corpus_actually_uses_counts_as_a_mirror() {
        for test in [
            "tests/grammar.rs",
            "src/parser/grammar_test.rs",
            "tests/test_grammar.py",
            "src/grammar.test.ts",
            "tests/api_grammar.py",
        ] {
            assert!(
                mirrors("src/parser/grammar.rs", test),
                "{test} should count as the cover"
            );
        }
        assert!(!mirrors("src/parser/grammar.rs", "tests/api.rs"));
        assert!(
            !mirrors("src/api/mod.rs", "tests/api.rs"),
            "a `mod.rs` has no stem to pair on, so it never claims a mirror"
        );
    }

    #[test]
    fn unrelated_tests_lower_the_confidence_and_say_which_ones_were_touched() {
        let files = vec![
            file(
                "src/parser/grammar.rs",
                &many(&["pub fn parse_grammar() {", "}"]),
            ),
            file(
                "tests/api.rs",
                &many(&["#[test] fn lists() {}", "assert!(true);"]),
            ),
        ];
        let findings = detect(&episode(files), &Corpus::default(), &Policy::default());
        assert_eq!(findings.len(), 1);
        assert!((findings[0].confidence - 0.55).abs() < f64::EPSILON);
        assert!(
            findings[0].message.contains("tests/api.rs"),
            "{}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("may already cover it"),
            "the honest version of the claim: {}",
            findings[0].message
        );
    }

    #[test]
    fn private_or_small_or_noise_or_deleted_changes_are_silent() {
        let cases = vec![
            // Too few lines.
            file("src/api.rs", &["pub fn tiny() {}"]),
            // Public keyword absent: a private helper.
            file("src/parser.rs", &many(&["fn parse_grammar() {}", "}"])),
            // Rust's crate-private is not public API.
            file(
                "src/parser.rs",
                &many(&["pub(crate) fn parse_grammar() {}", "}"]),
            ),
            // A lockfile is not logic.
            FileChange {
                path: "Cargo.lock".to_owned(),
                noise_class: "lockfile".to_owned(),
                add: 400,
                hunks: vec![Hunk::new(
                    "@@ -0,0 +1,400 @@",
                    ["name = \"serde\""],
                    Vec::<String>::new(),
                )],
                ..Default::default()
            },
            // A deletion is not new logic.
            FileChange {
                path: "src/gone.rs".to_owned(),
                change: change::DELETED.to_owned(),
                del: 90,
                add: 0,
                hunks: vec![Hunk::new(
                    "@@ -1,90 +0,0 @@",
                    Vec::<String>::new(),
                    ["pub fn gone() {}"],
                )],
                ..Default::default()
            },
            // Documentation.
            file(
                "docs/grammar.md",
                &many(&["# Grammar", "pub fn parse() {}"]),
            ),
        ];
        for case in cases {
            let findings = detect(&episode(vec![case]), &Corpus::default(), &Policy::default());
            assert!(
                findings.is_empty(),
                "{:?}",
                findings.iter().map(|f| &f.message).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn the_public_marker_is_per_dialect_and_a_leading_underscore_is_not_public() {
        assert!(is_public_declaration("pub fn a() {}", Dialect::Rust));
        assert!(is_public_declaration("def parse(input):", Dialect::Python));
        assert!(
            !is_public_declaration("    def parse(input):", Dialect::Python),
            "indented is not module level"
        );
        assert!(!is_public_declaration(
            "def _parse(input):",
            Dialect::Python
        ));
        assert!(is_public_declaration(
            "func NewCache() *Cache {",
            Dialect::Go
        ));
        assert!(
            !is_public_declaration("func newCache() *Cache {", Dialect::Go),
            "unexported is not API"
        );
        assert!(is_public_declaration(
            "export function parse() {",
            Dialect::JavaScriptTypeScript
        ));
        assert!(!is_public_declaration(
            "function parse() {",
            Dialect::JavaScriptTypeScript
        ));
        assert!(is_public_declaration(
            "public class Parser {",
            Dialect::Java
        ));
        assert!(!is_public_declaration("let x = 1;", Dialect::Rust));
        assert!(
            !is_public_declaration("let x = 1;", Dialect::Unknown),
            "no dialect, no claim about what is public"
        );
    }

    #[test]
    fn the_threshold_is_the_config_knob() {
        let file = file("src/api.rs", &many(&["pub fn a() {}"]));
        let policy = Policy {
            untested_min_added_lines: 10_000,
            ..Default::default()
        };
        assert!(detect(&episode(vec![file]), &Corpus::default(), &policy).is_empty());
    }
}

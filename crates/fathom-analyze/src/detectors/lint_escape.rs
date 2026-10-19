//! `lint_escape.rs` — a check was switched off, and which one is in the diff (`e4.3`).
//!
//! **What it claims.** An added line contains a directive whose whole purpose is to make a checker stop
//! checking: `#[allow(clippy::unwrap_used)]`, `# type: ignore`, `// eslint-disable-next-line`,
//! `@ts-ignore`, `//nolint`, `# noqa`, `@SuppressWarnings`, `#pragma GCC diagnostic ignored`. The claim is
//! a fact about the line — the directive is quoted in the evidence — which is why this detector's
//! confidence is the highest in the catalogue at `0.9`.
//!
//! **#2 is the finding, and it is the reason this is not one line per file.** "A lint was suppressed" is
//! noise; "this episode silenced `clippy::unwrap_used`" is a review question, because the suppressed *id*
//! says what the author did not want to hear. So the message names the id, and when the id is one of the
//! safety-critical set below — `unsafe_code`, `unwrap_used`, `expect_used`, `transmute`, `explicit_into_iter_loop`
//! excluded, `no-explicit-any`, `unchecked` — the severity is `high`, because those are the suppressions
//! that hide a memory bug, a panic path, or a hole in a type system rather than a style preference.
//!
//! **What would make it wrong.**
//!
//! * **a directive that was always there.** Only added lines are read, so a file that has carried
//!   `#[allow(dead_code)]` since 2019 is silent unless the episode adds it again.
//! * **documentation about directives.** Prose is not read, and a Rust *doc* comment (`///`, `//!`) is not
//!   read either, because this repository's own docs quote `#[allow(…)]` as an example. A `//` directive
//!   still is, because in JavaScript and YAML that is where the directive lives.
//! * **generated and vendored code**, where the directive is the generator's or the dependency's and not
//!   this episode's choice.
//! * **a macro parameter that happens to read `#[allow(`.** Exists; priced at the 0.1 that is missing from
//!   the confidence rather than argued away.
//!
//! **Severity, not verdict.** Disabling a lint is sometimes correct and the finding never says otherwise —
//! it says what was disabled, where, and that the *reason* is not in the diff. A reviewer who agrees types
//! the one key in `detectors.suppress`.

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::detectors::text::{self, Dialect};

/// A directive, and the dialects it means something in.
struct Directive {
    /// The substring that identifies it in a line.
    text: &'static str,
    /// What it disables, in the words a reviewer already uses for it.
    what: &'static str,
    dialects: &'static [Dialect],
}

const DIRECTIVES: &[Directive] = &[
    Directive {
        text: "#![allow(",
        what: "an entire crate's lint set",
        dialects: &[Dialect::Rust],
    },
    Directive {
        text: "#[allow(",
        what: "a lint",
        dialects: &[Dialect::Rust],
    },
    Directive {
        text: "#[expect(",
        what: "a lint, with an expectation",
        dialects: &[Dialect::Rust],
    },
    Directive {
        text: "#[rustfmt::skip]",
        what: "formatting",
        dialects: &[Dialect::Rust],
    },
    Directive {
        text: "# type: ignore",
        what: "the type checker on this line",
        dialects: &[Dialect::Python],
    },
    Directive {
        text: "# noqa",
        what: "the linter on this line",
        dialects: &[Dialect::Python],
    },
    Directive {
        text: "ruff: noqa",
        what: "the linter for this file",
        dialects: &[Dialect::Python],
    },
    Directive {
        text: "# pylint: disable",
        what: "pylint",
        dialects: &[Dialect::Python],
    },
    Directive {
        text: "# mypy: ",
        what: "mypy configuration",
        dialects: &[Dialect::Python],
    },
    Directive {
        text: "eslint-disable",
        what: "eslint",
        dialects: &[Dialect::JavaScriptTypeScript],
    },
    Directive {
        text: "@ts-ignore",
        what: "the TypeScript type check on this line",
        dialects: &[Dialect::JavaScriptTypeScript],
    },
    Directive {
        text: "@ts-nocheck",
        what: "the TypeScript type check for this file",
        dialects: &[Dialect::JavaScriptTypeScript],
    },
    Directive {
        text: "@ts-expect-error",
        what: "the TypeScript type check on this line",
        dialects: &[Dialect::JavaScriptTypeScript],
    },
    Directive {
        text: "biome-ignore",
        what: "biome",
        dialects: &[Dialect::JavaScriptTypeScript],
    },
    Directive {
        text: "//nolint",
        what: "golangci-lint",
        dialects: &[Dialect::Go],
    },
    Directive {
        text: "// nolint",
        what: "golangci-lint",
        dialects: &[Dialect::Go],
    },
    Directive {
        text: "lint:ignore",
        what: "go vet",
        dialects: &[Dialect::Go],
    },
    Directive {
        text: "NOLINT",
        what: "clang-tidy",
        dialects: &[Dialect::C],
    },
    Directive {
        text: "#pragma GCC diagnostic ignored",
        what: "a compiler diagnostic",
        dialects: &[Dialect::C],
    },
    Directive {
        text: "@SuppressWarnings",
        what: "the compiler's warnings",
        dialects: &[Dialect::Java, Dialect::Kotlin],
    },
    Directive {
        text: "@Suppress(",
        what: "the IDE's inspections",
        dialects: &[Dialect::Kotlin],
    },
    Directive {
        text: "SuppressMessage(",
        what: "the analyser's warnings",
        dialects: &[Dialect::CSharp],
    },
    Directive {
        text: "markdownlint-disable",
        what: "markdownlint",
        dialects: &[Dialect::Prose, Dialect::Config],
    },
    Directive {
        text: "hadolint ignore=",
        what: "the Dockerfile linter",
        dialects: &[Dialect::Config],
    },
    Directive {
        text: "shellcheck disable=",
        what: "shellcheck",
        dialects: &[Dialect::Shell],
    },
    Directive {
        text: "yamllint disable",
        what: "yamllint",
        dialects: &[Dialect::Config],
    },
];

/// The ids whose suppression hides a class of bug rather than a preference.
const SAFETY_CRITICAL: &[&str] = &[
    "unsafe_code",
    "unsafe_op_in_unsafe_fn",
    "unwrap_used",
    "expect_used",
    "transmute",
    "transmutes",
    "mutable_transmutes",
    "invalid_reference_casting",
    "pointer_format",
    "explicit_iter_loop",
    "no-explicit-any",
    "ts-nocheck",
    "unchecked",
    "rawtypes",
    "forbidden",
    "security",
    "null",
    "unused_must_use",
    "clippy::all",
    "clippy::pedantic",
    "deny",
];

pub fn detect(episode: &Episode, _corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    episode
        .files
        .iter()
        .filter(|file| reads(file))
        .filter_map(|file| {
            let dialect = text::dialect(&file.path)?;
            let added = file.added();
            let hits: Vec<(&str, &'static Directive)> = added
                .iter()
                .copied()
                .filter(|line| !(line.trim_start().starts_with("///") || line.trim_start().starts_with("//!")))
                .filter_map(|line| {
                    DIRECTIVES
                        .iter()
                        .find(|directive| directive.dialects.contains(&dialect) && line.contains(directive.text))
                        .map(|directive| (line, directive))
                })
                .collect();
            if hits.is_empty() {
                return None;
            }
            let quoted: Vec<&str> = hits.iter().map(|(line, _)| line.trim()).collect();
            let whats: Vec<&str> = hits.iter().map(|(_, directive)| directive.what).collect();
            let critical_id: Option<&'static str> = SAFETY_CRITICAL
                .iter()
                .copied()
                .find(|id| hits.iter().any(|(line, _)| line.contains(id)));
            Some(
                Finding::build(
                    "lint_escape",
                    1,
                    &episode.id,
                    file.path.clone(),
                    format!(
                        "{} adds {} check-suppressing directive(s) — {} — and the reason is not in the \
                         diff: {}",
                        file.path,
                        hits.len(),
                        crate::detectors::named(&whats, policy.name_cap),
                        crate::detectors::named(&quoted, 2)
                    ),
                )
                .severity(if critical_id.is_none() { Severity::Medium } else { Severity::High })
                .confidence(0.9)
                .evidence(Evidence::observed(
                    EvidenceKind::Hunk,
                    format!(
                        "{} {}",
                        file.path,
                        file.hunks.first().map_or_else(String::new, |hunk| hunk.header.clone())
                    ),
                    crate::detectors::quote(&quoted, policy.evidence_lines),
                ))
                .evidence(Evidence::inferred(
                    EvidenceKind::ConfigKey,
                    critical_id.unwrap_or("style-suppression").to_owned(),
                    if critical_id.is_none() {
                        "no safety-critical id is named, so this reads as a style suppression".to_owned()
                    } else {
                        "a safety-critical id is named: this class of suppression hides a bug class, not a \
                         preference"
                            .to_owned()
                    },
                ))
                .jump_path(&file.path)
                .finish(),
            )
        })
        .collect()
}

/// Hand-written code and configuration only: generated output carries its generator's directives, vendored
/// code is not this episode's choice, and a Markdown file is where the directive's *documentation* lives.
fn reads(file: &FileChange) -> bool {
    !file.binary
        && !file.noisy()
        && !file.is_delete()
        && text::dialect(&file.path)
            .is_some_and(|dialect| dialect.is_code() || dialect == Dialect::Config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::model::{Content, Hunk, change};

    fn file(path: &str, added: &[&str]) -> FileChange {
        FileChange {
            path: path.to_owned(),
            change: change::MODIFIED.to_owned(),
            add: added.len() as i64,
            hunks: vec![Hunk::new(
                "@@ -1,1 +1,3 @@",
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

    fn findings_for(path: &str, line: &str) -> Vec<Finding> {
        detect(
            &episode(vec![file(path, &[line])]),
            &Corpus::default(),
            &Policy::default(),
        )
    }

    #[test]
    fn a_type_check_disabled_is_a_fact_quoted_in_the_finding() {
        let findings = findings_for("src/checker.py", "value = compute()  # type: ignore");
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "lint_escape");
        assert_eq!(finding.severity, Severity::Medium);
        assert!(
            (finding.confidence - 0.9).abs() < f64::EPSILON,
            "the highest price in the catalogue"
        );
        assert!(
            finding.message.contains("the type checker on this line"),
            "{}",
            finding.message
        );
        assert!(finding.evidence[0].detail.contains("# type: ignore"));
        assert_eq!(finding.suppression_key, "lint_escape:src/checker.py");
    }

    #[test]
    fn a_safety_critical_id_makes_it_high_and_names_it() {
        let findings = findings_for("src/parse.rs", "#[allow(clippy::unwrap_used)]");
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "unwrap_used is a panic path, not a style"
        );
        assert!(findings[0].evidence[1].locator.contains("unwrap_used"));

        let style = findings_for("src/parse.rs", "#[allow(clippy::too_many_arguments)]");
        assert_eq!(
            style[0].severity,
            Severity::Medium,
            "a style suppression stays medium"
        );
    }

    #[test]
    fn every_language_in_the_table_is_recognised_in_its_own_spelling() {
        for (path, line) in [
            ("src/a.rs", "#[allow(dead_code)]"),
            ("src/a.py", "x = 1  # noqa: E501"),
            ("src/a.py", "# ruff: noqa"),
            ("src/a.ts", "// eslint-disable-next-line no-explicit-any"),
            ("src/a.ts", "// @ts-ignore"),
            ("src/a.go", "_ = x //nolint:errcheck"),
            ("src/A.java", "@SuppressWarnings(\"unchecked\")"),
            ("src/a.c", "int x; // NOLINT"),
            (
                ".github/workflows/ci.yml",
                "# yamllint disable-line rule:line-length",
            ),
        ] {
            let findings = findings_for(path, line);
            assert_eq!(findings.len(), 1, "{path}: {line} → {findings:?}");
        }
    }

    #[test]
    fn a_directive_in_a_rust_doc_comment_is_documentation() {
        // This repository's own docs quote `#[allow(...)]` as an example; a detector that read doc
        // comments would flag the manual for using the thing it documents.
        assert!(
            findings_for("src/parse.rs", "/// Use `#[allow(unsafe_code)]` only here").is_empty()
        );
        assert!(findings_for("docs/lints.md", "Add `#[allow(dead_code)]` to skip it.").is_empty());
    }

    #[test]
    fn a_directive_in_the_wrong_language_is_not_a_directive() {
        assert!(findings_for("src/a.go", "value = compute()  # type: ignore").is_empty());
        assert!(findings_for("src/a.rs", "// eslint-disable").is_empty());
    }

    #[test]
    fn generated_and_vendored_files_are_not_this_episodes_choice() {
        for path in ["src/generated/api.rs", "vendor/lib/x.go", "dist/bundle.js"] {
            assert!(
                findings_for(path, "#[allow(dead_code)] // eslint-disable").is_empty(),
                "{path}"
            );
        }
    }

    #[test]
    fn ordinary_code_is_silent() {
        for line in [
            "let allow_unsafe = false;",
            "fn lint_escape() {}",
            "config.allow = true;",
        ] {
            assert!(findings_for("src/a.rs", line).is_empty(), "{line}");
        }
    }

    #[test]
    fn several_directives_in_one_file_are_one_row_naming_them_all() {
        let episode = episode(vec![file(
            "src/parse.rs",
            &[
                "#[allow(unsafe_code)]",
                "#[allow(clippy::missing_safety_doc)]",
            ],
        )]);
        let findings = detect(&episode, &Corpus::default(), &Policy::default());
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0]
                .message
                .contains("2 check-suppressing directive(s)")
        );
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "one safety-critical id is enough"
        );
    }
}

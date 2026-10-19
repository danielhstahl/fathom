//! `debug_leftover.rs` — something written to look at the program survived into the commit (`e4.3`).
//!
//! **Two tiers, because "a print statement" is two different claims.**
//!
//! Tier A is a marker that is never production code: `dbg!`, `debugger`, `pdb.set_trace()`,
//! `breakpoint()`, `binding.pry`, `var_dump(`, `console.debug(`, `@Trace`. Its finding is `medium` at
//! `0.8` — not `0.95`, because a Rust file that defines its own macro called `dbg!`, or a document that
//! quotes one, exists somewhere.
//!
//! Tier B is a print that *may* be the program talking to its user: `println!`, `print!`, `fmt.Println`,
//! `System.out.print`, `console.log`, Python's `print(`, Ruby's `puts`. Its finding is `low` at `0.45`,
//! and it is silent in the places where printing is the job:
//!
//! * an entrypoint — `src/main.rs`, `src/bin/**`, `cmd/**`, `main.py`, `__main__.py`, `cli.py`,
//!   `*/scripts/**`, `bin/**`: a CLI that does not print is broken;
//! * the test surface — a test that prints is untidy, not left-behind;
//! * prose and configuration, where the marker is being *quoted*;
//! * generated and vendored churn, which nobody hand-edited.
//!
//! **Why this ships on while the loop detectors do not.** A `dbg!` in a commit is a fact about a line, and
//! the line is quoted in the finding: there is no judgement between the marker and the claim, which is the
//! property the four disabled detectors lack. The worst this detector does in a repository whose logging
//! idiom it does not know is print a `low` finding once per file, on a path the reviewer can silence with
//! `debug_leftover:src/term/**`.
//!
//! **What it will not do:** invent a line number. The evidence locator is the hunk header the store
//! actually holds, and the quoted lines are the added lines themselves.

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::detectors::text::{self, Dialect};

/// One marker, the dialects it means something in, and whether it is ever legitimate.
struct Marker {
    text: &'static str,
    dialects: Dialects,
}

/// The dialect set a marker applies to. A marker in an unknown dialect is not a marker: `print(` in a
/// file fathom cannot name is a line that could be anything.
struct Dialects(&'static [Dialect]);

impl Dialects {
    fn applies(&self, dialect: Dialect) -> bool {
        self.0.contains(&dialect)
    }
}

const JS: &[Dialect] = &[Dialect::JavaScriptTypeScript];
const PY: &[Dialect] = &[Dialect::Python];
const RB: &[Dialect] = &[Dialect::Ruby];
const RS: &[Dialect] = &[Dialect::Rust];
const GO: &[Dialect] = &[Dialect::Go];
const JVM: &[Dialect] = &[Dialect::Java, Dialect::Kotlin, Dialect::CSharp];

/// Tier A: never production code.
const CERTAIN: &[Marker] = &[
    Marker {
        text: "dbg!",
        dialects: Dialects(RS),
    },
    Marker {
        text: "debugger",
        dialects: Dialects(JS),
    },
    Marker {
        text: "console.debug(",
        dialects: Dialects(JS),
    },
    Marker {
        text: "console.trace(",
        dialects: Dialects(JS),
    },
    Marker {
        text: "pdb.set_trace()",
        dialects: Dialects(PY),
    },
    Marker {
        text: "ipdb.set_trace()",
        dialects: Dialects(PY),
    },
    Marker {
        text: "breakpoint()",
        dialects: Dialects(PY),
    },
    Marker {
        text: "binding.pry",
        dialects: Dialects(RB),
    },
    Marker {
        text: "var_dump(",
        dialects: Dialects(&[Dialect::Php]),
    },
    Marker {
        text: "dd(",
        dialects: Dialects(&[Dialect::Php]),
    },
    Marker {
        text: "TRACE!(",
        dialects: Dialects(&[Dialect::CSharp]),
    },
];

/// Tier B: could be the program, probably is not in this position.
const PROBABLE: &[Marker] = &[
    Marker {
        text: "println!(",
        dialects: Dialects(RS),
    },
    Marker {
        text: "print!(",
        dialects: Dialects(RS),
    },
    Marker {
        text: "eprintln!(",
        dialects: Dialects(RS),
    },
    Marker {
        text: "log::trace!(",
        dialects: Dialects(RS),
    },
    Marker {
        text: "console.log(",
        dialects: Dialects(JS),
    },
    Marker {
        text: "console.info(",
        dialects: Dialects(JS),
    },
    Marker {
        text: "fmt.Println(",
        dialects: Dialects(GO),
    },
    Marker {
        text: "fmt.Printf(",
        dialects: Dialects(GO),
    },
    Marker {
        text: "System.out.print",
        dialects: Dialects(JVM),
    },
    Marker {
        text: "System.err.print",
        dialects: Dialects(JVM),
    },
    Marker {
        text: "print(",
        dialects: Dialects(PY),
    },
    Marker {
        text: "pp(",
        dialects: Dialects(RB),
    },
    Marker {
        text: "p ",
        dialects: Dialects(RB),
    },
    Marker {
        text: "puts ",
        dialects: Dialects(RB),
    },
];

pub fn detect(episode: &Episode, _corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in &episode.files {
        if !reads(file) {
            continue;
        }
        let Some(dialect) = text::dialect(&file.path) else {
            continue;
        };
        let added = file.added();
        if added.is_empty() {
            continue;
        }
        let entrypoint = is_entrypoint(&file.path);
        let mut certain: Vec<&str> = Vec::new();
        let mut probable: Vec<&str> = Vec::new();
        for raw in &added {
            if text::is_blank_or_comment(raw, dialect) {
                continue;
            }
            // A marker inside a trailing comment — `let x = 1; // dbg!(y)` — is a note *about*
            // debugging, not debugging that runs. The scan is on the code half of the line.
            let line = text::strip_inline_comment(raw, dialect);
            if CERTAIN
                .iter()
                .any(|marker| marker.dialects.applies(dialect) && line.contains(marker.text))
            {
                certain.push(line);
            } else if !entrypoint
                && PROBABLE
                    .iter()
                    .any(|marker| marker.dialects.applies(dialect) && line.contains(marker.text))
            {
                probable.push(line);
            }
        }

        for (lines, severity, confidence) in [
            (&certain, Severity::Medium, 0.8),
            (&probable, Severity::Low, 0.45),
        ] {
            if lines.is_empty() {
                continue;
            }
            findings.push(
                Finding::build(
                    "debug_leftover",
                    1,
                    &episode.id,
                    file.path.clone(),
                    format!(
                        "{} added {} debug output line(s) that read as left behind: {}",
                        file.path,
                        lines.len(),
                        crate::detectors::named(
                            &lines.iter().map(|line| line.trim()).collect::<Vec<_>>(),
                            3
                        )
                    ),
                )
                .severity(severity)
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
                    crate::detectors::quote(lines, policy.evidence_lines),
                ))
                .jump_path(&file.path)
                .finish(),
            );
        }
    }
    findings
}

/// Only hand-written code files: prose quotes markers, generated churn was not hand-edited, tests that
/// print are untidy rather than forgotten, and configuration has no statements.
fn reads(file: &FileChange) -> bool {
    !file.binary
        && !file.noisy()
        && !file.is_test()
        && !file.is_delete()
        && text::dialect(&file.path).is_some_and(Dialect::is_code)
}

/// The places where printing to stdout *is* the program.
fn is_entrypoint(path: &str) -> bool {
    const HEADS: &[&str] = &[
        "src/main.rs",
        "src/bin/",
        "cmd/",
        "bin/",
        "scripts/",
        "main.py",
        "__main__.py",
        "cli.py",
    ];
    let folded = path.to_ascii_lowercase();
    HEADS
        .iter()
        .any(|head| folded.starts_with(head) || folded.contains(&format!("/{head}")))
        || folded.ends_with("_cli.rs")
        || folded.ends_with("/cli.ts")
        || folded.ends_with("/cli.js")
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
                "@@ -10,3 +10,5 @@ fn parse(",
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

    #[test]
    fn a_dbg_is_a_medium_finding_that_quotes_the_line() {
        let findings = detect(
            &episode(vec![file(
                "src/parse.rs",
                &["let v = items.pop();", "dbg!(&items);"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "debug_leftover");
        assert_eq!(finding.severity, Severity::Medium);
        assert!((finding.confidence - 0.8).abs() < f64::EPSILON);
        assert!(finding.evidence[0].detail.contains("dbg!(&items);"));
        assert_eq!(finding.suppression_key, "debug_leftover:src/parse.rs");
        assert_eq!(finding.jump.display(), "diff ep-1#src/parse.rs");
    }

    #[test]
    fn every_tier_a_marker_fires_and_every_tier_b_marker_is_only_a_note() {
        let certain_cases = [
            ("src/a.py", "pdb.set_trace()"),
            ("src/a.py", "breakpoint()"),
            ("src/a.rb", "binding.pry"),
            ("src/a.ts", "debugger;"),
            ("src/a.ts", "console.debug('x');"),
        ];
        for (path, marker) in certain_cases {
            let findings = detect(
                &episode(vec![file(path, &[marker])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert_eq!(findings.len(), 1, "{path} {marker}");
            assert_eq!(findings[0].severity, Severity::Medium, "{path} {marker}");
        }
        let probable_cases = [
            ("src/a.rs", "println!(\"{n}\");"),
            ("src/a.go", "fmt.Println(x)"),
            ("src/a.py", "print(x)"),
        ];
        for (path, marker) in probable_cases {
            let findings = detect(
                &episode(vec![file(path, &[marker])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert_eq!(findings.len(), 1, "{path} {marker}");
            assert_eq!(
                findings[0].severity,
                Severity::Low,
                "{path} {marker} must be a note, not a demand"
            );
            assert!(findings[0].confidence <= 0.5);
        }
    }

    #[test]
    fn a_cli_that_prints_is_not_left_behind() {
        for path in [
            "src/main.rs",
            "src/bin/convert.rs",
            "cmd/tool/main.go",
            "tools/cli.py",
        ] {
            let findings = detect(
                &episode(vec![file(path, &["println!(\"done\");"])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert!(findings.is_empty(), "{path} prints on purpose");
        }
    }

    #[test]
    fn the_tiers_are_split_and_a_file_with_both_gets_two_findings() {
        let findings = detect(
            &episode(vec![file(
                "src/parse.rs",
                &["dbg!(x);", "println!(\"hi\");"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert_eq!(
            findings.len(),
            2,
            "one row per tier: a reviewer dismisses them differently"
        );
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[1].severity, Severity::Low);
    }

    #[test]
    fn a_comment_quoting_a_marker_is_not_a_leftover() {
        let findings = detect(
            &episode(vec![file(
                "src/parse.rs",
                &["// remove the dbg! before merging"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn prose_generated_and_vendored_files_are_not_read() {
        for path in [
            "docs/debugging.md",
            "src/generated/api.rs",
            "vendor/lib/print.go",
            "tests/printing.rs",
        ] {
            let case = FileChange {
                path: path.to_owned(),
                add: 1,
                hunks: vec![Hunk::new(
                    "@@ -1,1 +1,2 @@",
                    ["dbg!(x);"],
                    Vec::<String>::new(),
                )],
                ..Default::default()
            };
            let findings = detect(&episode(vec![case]), &Corpus::default(), &Policy::default());
            assert!(
                findings.is_empty(),
                "{path} should not be read: {findings:?}"
            );
        }
    }

    #[test]
    fn an_unknown_extension_claims_nothing() {
        let case = FileChange {
            path: "assets/shader.glsl".to_owned(),
            add: 1,
            hunks: vec![Hunk::new(
                "@@ -1,1 +1,2 @@",
                ["print(x);"],
                Vec::<String>::new(),
            )],
            ..Default::default()
        };
        assert!(detect(&episode(vec![case]), &Corpus::default(), &Policy::default()).is_empty());
    }
}

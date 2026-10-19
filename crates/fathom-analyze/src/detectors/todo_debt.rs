//! `todo_debt.rs` — how much explicitly unfinished work this episode added (`e4.3`).
//!
//! **What it claims.** Someone wrote `TODO` in a line of code that did not exist before this episode. The
//! claim is small and it is exactly as large as the marker: the ticket is not "this code is unfinished"
//! (unknowable here) but "this repository now contains N more promises than it did". A row per file, the
//! markers quoted, and `detectors.todo.min_new` as the threshold a repository with a TODO-per-file culture
//! can raise to zero-nearly-always.
//!
//! **What would make it wrong.** Prose about TODOs, generated files that carry the generator's own
//! `TODO`, a vendored library's markers, and a marker inside a string that quotes one. Prose, generated
//! and vendored paths are not read at all; inside a code file the marker has to be in the *added* lines,
//! which is what makes this a debt statement rather than a census.
//!
//! **Two spellings of the check, on purpose.** `TODO`, `FIXME`, `HACK`, `XXX` and `@todo` are matched
//! case-sensitively, because `todo` in a Python string, a route name or a sentence is an English word and
//! a case-insensitive search for it fires on the clean corpus. `todo!()`, `unimplemented!()`,
//! `NotImplementedError`, `panic!("not implemented")` and `pass  # stub` are matched anywhere: those are
//! syntax, and syntax does not occur by accident.
//!
//! **Severity.** `low` up to four markers in a file, `medium` at five or more — a file with five new
//! markers is a different statement than one, and the escalation is printed as a count rather than an
//! adjective. `0.85`, not `0.95`: the marker is a certainty, the *debt* is a judgement about whether it
//! was ever going to be paid.

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::detectors::text::{self, Dialect};

/// Markers that only count in this exact case, because their lower-case form is an English word or an
/// unrelated identifier.
const CASE_SENSITIVE: &[&str] = &["TODO", "FIXME", "HACK", "XXX", "@todo"];

/// Markers that are syntax: they cannot occur by accident.
const SYNTAX: &[&str] = &[
    "todo!(",
    "todo!()",
    "unimplemented!(",
    "NotImplementedError",
    "not implemented",
    "notimplemented",
    "# stub",
    "raise NotImplementedError",
];

/// At how many markers in one file the finding stops being a note.
const ESCALATE_AT: usize = 5;

pub fn detect(episode: &Episode, _corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    episode
        .files
        .iter()
        .filter(|file| reads(file))
        .filter_map(|file| {
            let dialect = text::dialect(&file.path)?;
            let added = file.added();
            let markers: Vec<&str> = added
                .iter()
                .copied()
                .filter(|line| !line.trim().is_empty())
                .filter(|line| {
                    CASE_SENSITIVE.iter().any(|marker| line.contains(marker))
                        || SYNTAX
                            .iter()
                            .any(|marker| line.to_ascii_lowercase().contains(marker))
                })
                .collect();
            if markers.len() < usize::try_from(policy.todo_min_new.max(0)).unwrap_or(1) {
                return None;
            }
            let severity = if markers.len() >= ESCALATE_AT {
                Severity::Medium
            } else {
                Severity::Low
            };
            Some(
                Finding::build(
                    "todo_debt",
                    1,
                    &episode.id,
                    file.path.clone(),
                    format!(
                        "{} added {} unfinished-work marker(s) ({}): {}",
                        file.path,
                        markers.len(),
                        dialect.as_str(),
                        crate::detectors::named(
                            &markers.iter().map(|line| line.trim()).collect::<Vec<_>>(),
                            3
                        )
                    ),
                )
                .severity(severity)
                .confidence(0.85)
                .evidence(Evidence::observed(
                    EvidenceKind::Hunk,
                    format!(
                        "{} {}",
                        file.path,
                        file.hunks
                            .first()
                            .map_or_else(String::new, |hunk| hunk.header.clone())
                    ),
                    crate::detectors::quote(&markers, policy.evidence_lines),
                ))
                .jump_path(&file.path)
                .finish(),
            )
        })
        .collect()
}

/// Code a person wrote: not prose, not derived, not somebody else's, not gone.
fn reads(file: &FileChange) -> bool {
    !file.binary
        && !file.noisy()
        && !file.is_delete()
        && text::dialect(&file.path).is_some_and(Dialect::is_code)
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

    #[test]
    fn a_new_todo_is_a_note_with_the_marker_quoted() {
        let findings = detect(
            &episode(vec![file(
                "src/api.rs",
                &["// TODO: wire the retry after the merge"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "todo_debt");
        assert_eq!(finding.severity, Severity::Low);
        assert!((finding.confidence - 0.85).abs() < f64::EPSILON);
        assert!(finding.evidence[0].detail.contains("TODO: wire the retry"));
        assert_eq!(finding.suppression_key, "todo_debt:src/api.rs");
    }

    #[test]
    fn a_dozen_of_them_in_one_file_is_not_a_note() {
        let lines: Vec<String> = (0..7).map(|n| format!("// TODO: piece {n}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let findings = detect(
            &episode(vec![file("src/api.rs", &refs)]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert_eq!(findings[0].severity, Severity::Medium);
        assert!(findings[0].message.contains("7 unfinished-work marker(s)"));
    }

    #[test]
    fn syntax_markers_count_whatever_their_case_and_english_words_do_not() {
        let syntax = detect(
            &episode(vec![file(
                "src/api.rs",
                &["return todo!();", "raise notimplementederror()"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert_eq!(syntax.len(), 1);
        assert!(
            syntax[0].message.contains("2 unfinished-work marker"),
            "{}",
            syntax[0].message
        );

        let english = detect(
            &episode(vec![file(
                "src/api.rs",
                &["let route = \"/my-todo-list\";", "let todo = 3;"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert!(
            english.is_empty(),
            "a lower-case `todo` is an English word: {english:?}"
        );
    }

    #[test]
    fn prose_generated_and_vendored_files_are_not_a_census() {
        for path in [
            "docs/plan.md",
            "src/gen/api.py",
            "vendor/lib/x.go",
            "CHANGELOG.md",
        ] {
            let findings = detect(
                &episode(vec![file(path, &["TODO: not this one"])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert!(findings.is_empty(), "{path}: {findings:?}");
        }
    }

    #[test]
    fn the_threshold_is_the_config_knob() {
        let policy = Policy {
            todo_min_new: 3,
            ..Default::default()
        };
        let findings = detect(
            &episode(vec![file("src/api.rs", &["// TODO a", "// TODO b"])]),
            &Corpus::default(),
            &policy,
        );
        assert!(
            findings.is_empty(),
            "two markers is under a threshold of three"
        );
    }

    #[test]
    fn a_removed_todo_is_not_debt() {
        let file = FileChange {
            path: "src/api.rs".to_owned(),
            del: 1,
            hunks: vec![Hunk::new(
                "@@ -1,2 +1,1 @@",
                Vec::<String>::new(),
                ["// TODO: paid off"],
            )],
            ..Default::default()
        };
        assert!(detect(&episode(vec![file]), &Corpus::default(), &Policy::default()).is_empty());
    }
}

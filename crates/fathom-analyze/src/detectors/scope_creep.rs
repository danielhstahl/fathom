//! `scope_creep.rs` — the change went outside what somebody declared (`e4.3`).
//!
//! **What it claims.** Someone said, in a hook payload or a `Scope:` trailer, which paths this turn was
//! about. These paths are not in that set. That is the whole claim, and it is the only pathology detector
//! in the catalogue whose evidence is a *statement the repository made* rather than a pattern this build
//! recognised — which is why it is the one whose evidence line reads like a quotation.
//!
//! **What it cannot claim.** It has no opinion about a change nobody scoped. [`Deviation::Unknown`] is the
//! common state of every repository without hooks (`e4.2` says so in its own words), and an undeclared
//! scope is emphatically not an empty allow-list: read that way, the detector fires on every commit in
//! the ordinary repository, which is the exact way to make a reviewer distrust the whole catalogue. The
//! report says `not examined` only when *content* is missing, and here content is not needed — paths and
//! a declaration are enough, so this detector runs on a spine-only read.
//!
//! **What would make it wrong.** Three things, and each has a rule:
//!
//! * **a move.** A file that was inside the scope, moved, is a file whose old path was declared. Flagging
//!   it would tell the reviewer the agent went somewhere it was not told to, when the diff says it moved
//!   a file it was told to touch. [`rescued`] handles this and says so in the finding's evidence.
//! * **a glob nobody meant.** A declared `src/parser/**` that the author meant to cover the parser's
//!   tests in `tests/parser/` is a real disagreement between two documents, and this detector reports it:
//!   the fix is to widen the declaration, and `detectors.suppress` is the one key for the case where the
//!   declaration is right and the reviewer disagrees.
//! * **a rename of the scope itself.** Out of scope for this detector: moving a directory the scope named
//!   is the same case as the first one, and [`rescued`] covers it because the old path is the one that
//!   was declared.
//!
//! **Confidence: 0.9.** Not 1.0, because the declaration could itself be stale, truncated by
//! `intent.scope_cap`, or written by the agent whose work is under review — `e4.2`'s ladder takes a
//! `Scope:` trailer from the same commit it is describing. The last 0.1 is that possibility, and it is
//! printed as evidence (`who declared it`) rather than hidden.
//!
//! **Severity.** `high` for a surface a revert does not undo — a migration, a pipeline, infrastructure, a
//! container — and `medium` everywhere else. Breadth is not severity: ten outside paths are ten findings,
//! not one big one, so that each can be dismissed on its own key.

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::risk::Surface;
use crate::scope::{Deviation, deviation, matches};

/// One episode's declared scope against its touched paths.
pub fn detect(episode: &Episode, _corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    if episode.declared_scope.is_empty() {
        // The normal state of a repository with no hooks. No opinion, no finding, no gap: this detector
        // asked and the repository declined to answer, which is the report's business and not a finding's.
        return Vec::new();
    }
    let touched: Vec<&str> = episode.paths();
    let Deviation::Outside(outside) = deviation(&episode.declared_scope, &touched) else {
        return Vec::new();
    };

    outside
        .iter()
        .filter_map(|path| {
            let file = episode
                .files
                .iter()
                .find(|candidate| &candidate.path == path);
            if rescued(episode, file?).is_some() {
                // The move case above: the file left the scope's spelling but not its meaning. Said in
                // the report? No — said nowhere, because printing a non-finding per move is how a
                // rename-storm episode becomes noise. The unit test pins that it stays silent.
                return None;
            }
            let surface = file.map_or(Surface::Other, FileChange::surface);
            let severity = if is_operator_surface(surface) {
                Severity::High
            } else {
                Severity::Medium
            };
            let change = file.map_or("", FileChange::change_kind_sentence);
            Some(
                Finding::build(
                    "scope_creep",
                    1,
                    &episode.id,
                    path.clone(),
                    format!(
                        "{path} is outside the declared scope ({}), and {change}",
                        crate::detectors::named(
                            &episode
                                .declared_scope
                                .iter()
                                .map(String::as_str)
                                .collect::<Vec<_>>(),
                            policy.name_cap
                        )
                    ),
                )
                .severity(severity)
                .confidence(0.9)
                .evidence(Evidence::asserted(
                    EvidenceKind::Range,
                    format!("episode:{}#declared_scope", episode.id),
                    format!("declared scope: {}", episode.declared_scope.join(", ")),
                ))
                .evidence(Evidence::inferred(
                    EvidenceKind::Path,
                    path.clone(),
                    format!("touched by this episode as {surface:?}"),
                ))
                .jump_path(path)
                .finish(),
            )
        })
        .collect()
}

/// The path this one moved from was inside the scope, so the scope covered it.
fn rescued<'a>(episode: &Episode, file: &'a FileChange) -> Option<&'a str> {
    file.old_path
        .as_deref()
        .filter(|old| matches(&episode.declared_scope, *old))
}

/// The surfaces where "outside the declared scope" means an operator has to do something, not just read
/// something: a migration is applied, a pipeline runs, infrastructure is live.
fn is_operator_surface(surface: Surface) -> bool {
    matches!(
        surface,
        Surface::Migration | Surface::Ci | Surface::Infra | Surface::Container
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::model::{Content, Hunk, change};

    fn episode(scope: &[&str], files: Vec<FileChange>) -> Episode {
        Episode {
            id: "ep-1".to_owned(),
            declared_scope: scope.iter().map(|glob| (*glob).to_owned()).collect(),
            files,
            content: Content::Full,
            ..Episode::new("ep-1")
        }
    }

    fn file(path: &str) -> FileChange {
        FileChange {
            path: path.to_owned(),
            change: change::MODIFIED.to_owned(),
            add: 4,
            hunks: vec![Hunk::new(
                "@@ -1,1 +1,2 @@",
                ["let x = 1;"],
                Vec::<String>::new(),
            )],
            ..Default::default()
        }
    }

    #[test]
    fn a_path_outside_the_declaration_is_a_finding_with_everything_the_ticket_asks_for() {
        let episode = episode(
            &["src/parser/**"],
            vec![file("src/parser/grammar.rs"), file("src/api.rs")],
        );
        let findings = detect(&episode, &Corpus::default(), &Policy::default());
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "scope_creep");
        assert_eq!(finding.subject, "src/api.rs");
        assert!(
            finding.message.contains("outside the declared scope")
                && finding.message.contains("src/parser/**"),
            "{}",
            finding.message
        );
        assert_eq!(finding.severity, Severity::Medium);
        assert!((finding.confidence - 0.9).abs() < f64::EPSILON);
        assert_eq!(finding.jump.display(), "diff ep-1#src/api.rs");
        assert_eq!(finding.suppression_key, "scope_creep:src/api.rs");
        assert_eq!(
            finding.evidence.len(),
            2,
            "the declaration and the touch are both quoted"
        );
    }

    #[test]
    fn nothing_declared_is_no_opinion_not_an_empty_allow_list() {
        // The state of every repository with no hooks, and the single largest false-positive risk in this
        // catalogue. A finding here would fire on the clean corpus and on every real repository.
        let episode = episode(&[], vec![file("src/parser/grammar.rs"), file("src/api.rs")]);
        assert!(detect(&episode, &Corpus::default(), &Policy::default()).is_empty());
    }

    #[test]
    fn everything_inside_the_declaration_is_silent() {
        let episode = episode(
            &["src/parser/**"],
            vec![file("src/parser/grammar.rs"), file("src/parser/lr.rs")],
        );
        assert!(detect(&episode, &Corpus::default(), &Policy::default()).is_empty());
    }

    #[test]
    fn a_move_of_a_declared_file_is_not_creep() {
        let mut moved = file("src/parser/mod.rs");
        moved.old_path = Some("src/parser/grammar.rs".to_owned());
        moved.change = change::RENAMED.to_owned();
        let episode = episode(&["src/parser/grammar.rs"], vec![moved]);
        assert!(
            detect(&episode, &Corpus::default(), &Policy::default()).is_empty(),
            "moving a file the scope named is not going somewhere it did not"
        );
    }

    #[test]
    fn a_move_from_outside_the_scope_still_counts() {
        let mut moved = file("src/parser/mod.rs");
        moved.old_path = Some("src/elsewhere/thing.rs".to_owned());
        moved.change = change::RENAMED.to_owned();
        let episode = episode(&["src/parser/grammar.rs"], vec![moved]);
        assert_eq!(
            detect(&episode, &Corpus::default(), &Policy::default()).len(),
            1
        );
    }

    #[test]
    fn a_migration_outside_the_scope_outranks_a_document() {
        let episode = episode(
            &["src/parser/**"],
            vec![FileChange {
                path: "db/migrations/0007_add_owner.sql".to_owned(),
                change: change::ADDED.to_owned(),
                add: 3,
                hunks: vec![Hunk::new(
                    "@@ -0,0 +1,3 @@",
                    ["ALTER TABLE t ADD COLUMN owner TEXT;"],
                    Vec::<String>::new(),
                )],
                ..Default::default()
            }],
        );
        let findings = detect(&episode, &Corpus::default(), &Policy::default());
        assert_eq!(
            findings[0].severity,
            Severity::High,
            "a migration is not a document"
        );
    }

    #[test]
    fn many_paths_outside_are_many_findings_each_with_its_own_key() {
        let files: Vec<FileChange> = (0..9)
            .map(|n| file(&format!("src/outside/{n}.rs")))
            .collect();
        let episode = episode(&["src/parser/**"], files);
        let findings = detect(&episode, &Corpus::default(), &Policy::default());
        assert_eq!(findings.len(), 9);
        let keys: Vec<&str> = findings
            .iter()
            .map(|finding| finding.suppression_key.as_str())
            .collect();
        assert_eq!(
            keys.iter().collect::<std::collections::BTreeSet<_>>().len(),
            9
        );
    }

    #[test]
    fn a_long_declaration_is_named_up_to_the_cap_and_says_the_rest() {
        let scope: Vec<String> = (0..9).map(|n| format!("src/area{n}/**")).collect();
        let episode = Episode {
            declared_scope: scope,
            ..episode(&["src/parser/**"], vec![file("src/api.rs")])
        };
        let findings = detect(
            &episode,
            &Corpus::default(),
            &Policy {
                name_cap: 3,
                ..Default::default()
            },
        );
        assert!(
            findings[0].message.contains("and 6 more"),
            "{}",
            findings[0].message
        );
    }
}

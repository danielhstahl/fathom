//! `rework_loop.rs` — this episode is undoing, or re-landing, something that already happened (`e4.3`).
//!
//! **Ships disabled**, on the same `e4.3a` condition as [`crate::detectors::churn_loop`], and for a sharper
//! reason: a revert is a *legitimate act* that an operator performs on purpose, and at line level a revert,
//! a rebase that re-applied a commit twice, a squash-merge that landed the same diff on two branches, and an
//! agent that changed its mind are four names for one shape. The detector can say the shape with confidence;
//! only a disposition set says whether the shape deserved a row.
//!
//! **Two arms, two confidences.**
//!
//! * **revert** (`0.75`) — on a path both episodes touched, this episode adds what the earlier one removed
//!   *and* removes what the earlier one added, each at least `detectors.rework.min_overlap` (default `0.6`)
//!   Jaccard. Both halves are required on purpose: one-directional overlap is a follow-up commit, and the
//!   ticket's word is *revert*.
//! * **repeated hunk** (`0.6`) — the same added lines on the same path arrived in an earlier episode too,
//!   at `0.9` overlap and at least `detectors.copy.min_block_lines` of them. This is a squash-merge or a
//!   badly-resolved rebase landing one change twice, and it is worth a row even when nothing was undone.
//!
//! **Line-set comparison, not string equality.** [`crate::detectors::text::normalise`] folds indentation
//! and runs of whitespace, because the second landing of a hunk is usually re-indented. What is *not*
//! folded: case, and the contents of strings.
//!
//! **Jaccard, both directions for the revert arm.** A three-line episode whose lines all appear inside a
//! nine-hundred-line episode is not that episode undone, and a "share of the smaller side" measure would
//! say it was — see [`crate::detectors::text::overlap`].
//!
//! **Boundary with `churn_loop`.** `churn_loop` needs at least three episodes on one *region* and a put-back
//! somewhere in that chain; this one works on pairs and on whole episodes. An episode can appear in both,
//! which is correct: they answer different questions ("this region keeps moving" versus "this episode is
//! that earlier one, backwards").

use std::collections::BTreeSet;

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::detectors::text;

/// How far back to look for the episode this one is undoing. A rebase train is short; ninety episodes is
/// archaeology.
const LOOK_BACK: usize = 20;

/// The overlap that makes two added-line sets "the same hunk".
const SAME_HUNK: f64 = 0.9;

pub fn detect(episode: &Episode, corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    let mut findings = Vec::new();
    for earlier in corpus.prior(episode, LOOK_BACK) {
        for file in &episode.files {
            let Some(before) = earlier.files.iter().find(|candidate| {
                candidate.path == file.path
                    || candidate.old_path.as_deref() == Some(file.path.as_str())
                    || file
                        .old_path
                        .as_deref()
                        .is_some_and(|old| candidate.path == old)
            }) else {
                continue;
            };
            let (added_now, removed_now) = sides(file);
            let (added_then, removed_then) = sides(before);
            if added_now.is_empty() && removed_now.is_empty() {
                continue;
            }

            let earlier_id = earlier.id.as_str();
            let restoring = text::overlap(&added_now, &removed_then);
            let readding = text::overlap(&removed_now, &added_then);
            let bar = policy.rework_min_overlap;
            // Both directions clear the bar — or one side of the earlier episode was empty and the other
            // direction does. Deleting what the previous episode added *is* its revert when that episode
            // added and removed nothing else; demanding two directions there would ask for a half of the
            // revert that does not exist.
            let reverted = (restoring >= bar && readding >= bar)
                || (removed_then.is_empty() && !added_then.is_empty() && readding >= bar)
                || (added_then.is_empty() && !removed_then.is_empty() && restoring >= bar);
            if reverted {
                findings.push(
                    Finding::build(
                        "rework_loop",
                        1,
                        &episode.id,
                        file.path.clone(),
                        format!(
                            "{} puts {} back: {}",
                            file.path,
                            earlier_id,
                            if removed_then.is_empty() {
                                format!(
                                    "it removes every one of the {} line(s) {earlier_id} added to this path \
                                     ({:.0}% of them gone again)",
                                    added_then.len(),
                                    readding * 100.0
                                )
                            } else {
                                format!(
                                    "it re-adds {:.0}% of what it removed from this path and re-removes \
                                     {:.0}% of what it added there",
                                    restoring * 100.0,
                                    readding * 100.0
                                )
                            }
                        ),
                    )
                    .severity(Severity::Medium)
                    .confidence(0.75)
                    .evidence(Evidence::observed(
                        EvidenceKind::Episode,
                        earlier.id.clone(),
                        format!("its removals on {} reappear here", file.path),
                    ))
                    .evidence(Evidence::inferred(
                        EvidenceKind::Path,
                        file.path.clone(),
                        format!(
                            "both directions of the overlap clear the {:.0}% bar ({} line(s) added here, \
                             {} removed)",
                            bar * 100.0,
                            added_now.len(),
                            removed_now.len()
                        ),
                    ))
                    .jump_path(&file.path)
                    .finish(),
                );
                continue;
            }

            let repeated = text::overlap(&added_now, &added_then);
            let shared = added_now.intersection(&added_then).count();
            if repeated >= SAME_HUNK && shared >= policy.copy_min_block_lines {
                findings.push(
                    Finding::build(
                        "rework_loop",
                        1,
                        &episode.id,
                        file.path.clone(),
                        format!(
                            "the same {} added line(s) landed on {} in {earlier_id} as well — a change that \
                             arrives twice is a rebase or a squash landing one diff on two lines of history",
                            shared, file.path
                        ),
                    )
                    .severity(Severity::Medium)
                    .confidence(0.6)
                    .evidence(Evidence::observed(
                        EvidenceKind::Episode,
                        earlier.id.clone(),
                        crate::detectors::quote(
                            &added_now.iter().map(String::as_str).collect::<Vec<_>>(),
                            policy.evidence_lines,
                        ),
                    ))
                    .evidence(Evidence::inferred(
                        EvidenceKind::Path,
                        file.path.clone(),
                        format!("{shared} of {} added line(s) are identical to {earlier_id}'s", added_now.len()),
                    ))
                    .jump_path(&file.path)
                    .finish(),
                );
            }
        }
    }
    findings.sort_by(|a, b| a.subject.cmp(&b.subject).then(a.message.cmp(&b.message)));
    findings
}

/// The two sides of a file's change, folded and blank-free.
fn sides(file: &FileChange) -> (BTreeSet<String>, BTreeSet<String>) {
    let added: Vec<String> = file.added().into_iter().map(str::to_owned).collect();
    let removed: Vec<String> = file.removed().into_iter().map(str::to_owned).collect();
    (text::line_set(&added), text::line_set(&removed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::model::{Content, Hunk, change};

    fn edit(path: &str, added: &[&str], removed: &[&str]) -> FileChange {
        FileChange {
            path: path.to_owned(),
            change: change::MODIFIED.to_owned(),
            add: added.len() as i64,
            del: removed.len() as i64,
            hunks: vec![Hunk::new(
                "@@ -1,4 +1,4 @@",
                added.to_vec(),
                removed.to_vec(),
            )],
            ..Default::default()
        }
    }

    fn episode(id: &str, seq: i64, files: Vec<FileChange>) -> Episode {
        Episode {
            id: id.to_owned(),
            lineage_ref: "refs/heads/main".to_owned(),
            lineage_seq: seq,
            files,
            content: Content::Full,
            ..Episode::new(id)
        }
    }

    #[test]
    fn a_revert_is_caught_in_both_directions_and_named() {
        let before = episode(
            "ep-add",
            1,
            vec![edit(
                "src/cache.rs",
                &["fn evict() {}", "let ttl = 60;"],
                &["fn old_evict() {}"],
            )],
        );
        let after = episode(
            "ep-revert",
            2,
            vec![edit(
                "src/cache.rs",
                &["fn old_evict() {}"],
                &["fn evict() {}", "let ttl = 60;"],
            )],
        );
        let pair = [before, after.clone()];
        let corpus = Corpus::of(&pair);
        let findings = detect(&after, &corpus, &Policy::all_enabled());
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "rework_loop");
        assert!(
            finding.message.contains("puts ep-add back"),
            "{}",
            finding.message
        );
        assert_eq!(finding.severity, Severity::Medium);
        assert!(finding.confidence > 0.7);
    }

    #[test]
    fn indentation_does_not_make_a_new_change() {
        let before = episode(
            "ep-1",
            1,
            vec![edit("src/a.rs", &["let x = 1;", "let y = 2;"], &[])],
        );
        let after = episode(
            "ep-2",
            2,
            vec![edit("src/a.rs", &[], &["    let x = 1;", "\tlet y = 2;"])],
        );
        let pair = [before, after.clone()];
        let corpus = Corpus::of(&pair);
        let findings = detect(&after, &corpus, &Policy::all_enabled());
        assert_eq!(
            findings.len(),
            1,
            "the removal matches after folding: {findings:?}"
        );
    }

    #[test]
    fn a_followup_that_only_adds_is_not_a_revert() {
        let before = episode("ep-1", 1, vec![edit("src/a.rs", &[], &["let x = 1;"])]);
        let after = episode(
            "ep-2",
            2,
            vec![edit("src/a.rs", &["let y = 2;", "let z = 3;"], &[])],
        );
        let pair = [before, after.clone()];
        let corpus = Corpus::of(&pair);
        assert!(detect(&after, &corpus, &Policy::all_enabled()).is_empty());
    }

    #[test]
    fn a_small_edit_inside_a_huge_episode_is_not_that_episode_reverted() {
        let big_added: Vec<String> = (0..40).map(|n| format!("let big{n} = {n};")).collect();
        let big: Vec<&str> = big_added.iter().map(String::as_str).collect();
        let before = episode(
            "ep-big",
            1,
            vec![edit("src/a.rs", big.as_slice(), &["let x = 1;"])],
        );
        let after = episode("ep-tiny", 2, vec![edit("src/a.rs", &["let x = 1;"], &[])]);
        let pair = [before, after.clone()];
        let corpus = Corpus::of(&pair);
        let findings = detect(&after, &corpus, &Policy::all_enabled());
        assert!(
            findings.is_empty(),
            "one line back is not the forty-line episode undone: {findings:?}"
        );
    }

    #[test]
    fn the_same_hunk_landing_twice_is_caught_even_without_a_revert() {
        let lines: Vec<String> = (0..13).map(|n| format!("fn item{n}() {{}}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let before = episode("ep-1", 1, vec![edit("src/a.rs", &refs, &[])]);
        let after = episode("ep-2", 2, vec![edit("src/a.rs", &refs, &[])]);
        let pair = [before, after.clone()];
        let corpus = Corpus::of(&pair);
        let policy = Policy::all_enabled();
        let findings = detect(&after, &corpus, &policy);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0]
                .message
                .contains("landed on src/a.rs in ep-1 as well"),
            "{}",
            findings[0].message
        );
        assert!(
            findings[0].confidence < 0.7,
            "the repeat arm is priced lower than the revert arm"
        );
    }

    #[test]
    fn a_repeat_shorter_than_the_block_knob_is_not_a_repeat() {
        let before = episode("ep-1", 1, vec![edit("src/a.rs", &["fn a() {}"], &[])]);
        let after = episode("ep-2", 2, vec![edit("src/a.rs", &["fn a() {}"], &[])]);
        let pair = [before, after.clone()];
        let corpus = Corpus::of(&pair);
        let policy = Policy {
            copy_min_block_lines: 4,
            ..Policy::all_enabled()
        };
        assert!(
            detect(&after, &corpus, &policy).is_empty(),
            "one line is a coincidence, not a hunk"
        );
    }

    #[test]
    fn a_revert_across_a_rename_is_still_a_revert() {
        let before = episode(
            "ep-1",
            1,
            vec![edit("src/old.rs", &["let x = 1;", "let y = 2;"], &[])],
        );
        let mut back = edit("src/new.rs", &[], &["let x = 1;", "let y = 2;"]);
        back.old_path = Some("src/old.rs".to_owned());
        back.change = change::RENAMED.to_owned();
        let after = episode("ep-2", 2, vec![back]);
        let pair = [before, after.clone()];
        let corpus = Corpus::of(&pair);
        assert_eq!(detect(&after, &corpus, &Policy::all_enabled()).len(), 1);
    }
}

//! `churn_loop.rs` — the same region, edited across N episodes, without converging (`e4.3`).
//!
//! **Ships disabled.** See [`crate::detectors::CATALOGUE`]: the re-enable condition is the `e4.3a`
//! three-repository disposition set, and the reason it is one is stated below under *the judgement*.
//!
//! **What it claims.** This region — a function, or a 40-line block when git gave no function context —
//! has been edited by at least `detectors.churn.min_episodes` episodes (default 3) in the recent window,
//! **and** at least one of those edits put lines back that an earlier one removed. The second half is what
//! makes it a loop rather than a list: three episodes touching a function is ordinary work on a hard
//! problem, and this detector refuses to call ordinary work a pathology. Putting back what you took out is
//! not ordinary — it is the shape of an agent that removed a guard, found the tests failed, and restored
//! the guard three episodes later.
//!
//! **The judgement it cannot avoid.** "Converging" is a claim about intent. The line-level signature is
//! real (the overlap is computed, not guessed) but a legitimate refactor has the same signature: move code
//! out, discover a caller needed it where it was, move it back. That is a *correct* outcome and this
//! detector reads it as a loop. No threshold fixes that; only a disposition set on real repositories says
//! how often it happens, which is `e4.3a`'s job.
//!
//! **Region identity.** [`crate::detectors::model::Hunk::region`]: git's function context when the hunk
//! header has one, a 40-line block when it does not. Line numbers alone would split a region every time a
//! comment is added above it, and a churn loop that resets when the file gets tidier is not measuring
//! churn.
//!
//! **Confidence `0.5`, severity `medium`.** The count is a fact; the interpretation is the coin flip the
//! calibration exists to price.

use std::collections::BTreeSet;

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, Hunk};
use crate::detectors::policy::Policy;
use crate::detectors::text;

/// How many episodes of look-back the window spans, as a multiple of the threshold. Three is a loop; the
/// history you check for one is four times that, and no further back — a region touched in the same way
/// ninety episodes ago is history, not a loop.
const WINDOW_MULTIPLE: usize = 4;

/// The line overlap that counts as "put back".
const PUT_BACK: f64 = 0.5;

pub fn detect(episode: &Episode, corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    let window = policy
        .churn_min_episodes
        .saturating_mul(WINDOW_MULTIPLE)
        .max(4);
    let history = corpus.prior(episode, window);

    // (path, region) → the episodes that touched it, oldest first with the target last, each with its two
    // line sets.
    let mut chain: Vec<(String, String, Vec<Stitch>)> = Vec::new();
    for file in &episode.files {
        for region in file.regions() {
            let mut stitches: Vec<Stitch> = history
                .iter()
                .rev()
                .filter(|other| {
                    other.files.iter().any(|candidate| {
                        candidate.path == file.path && candidate.regions().contains(&region)
                    })
                })
                .map(|other| stitch_of(other, &file.path, &region))
                .collect();
            stitches.push(stitch_of(episode, &file.path, &region));
            if stitches.len() >= policy.churn_min_episodes {
                chain.push((file.path.clone(), region, stitches));
            }
        }
    }

    let mut findings = Vec::new();
    for (path, region, stitches) in chain {
        let Some((put_back, older_id)) = find_put_back(&stitches) else {
            continue;
        };
        let names: Vec<&str> = stitches
            .iter()
            .map(|stitch| stitch.episode_id.as_str())
            .collect();
        findings.push(
            Finding::build(
                "churn_loop",
                1,
                &episode.id,
                path.clone(),
                format!(
                    "{path} · {region} has been edited by {} episodes ({}) without converging: {} puts \
                     back {:.0}% of the lines {older_id} removed there",
                    stitches.len(),
                    crate::detectors::named(&names, policy.name_cap),
                    episode.id,
                    put_back * 100.0
                ),
            )
            .severity(Severity::Medium)
            .confidence(0.5)
            .evidence(Evidence::inferred(
                EvidenceKind::Path,
                path.clone(),
                format!("{} episodes touched this region within the last {window}: {}", stitches.len(), names.join(", ")),
            ))
            .evidence(Evidence::observed(
                EvidenceKind::Episode,
                older_id.clone(),
                format!("its lines removed here reappear in {}'s added lines", episode.id),
            ))
            .jump_path(&path)
            .finish(),
        );
    }
    findings.sort_by(|a, b| a.subject.cmp(&b.subject).then(a.message.cmp(&b.message)));
    findings
}

/// One episode's lines on one (path, region), folded for comparison.
struct Stitch {
    episode_id: String,
    added: BTreeSet<String>,
    removed: BTreeSet<String>,
}

fn stitch_of(episode: &Episode, path: &str, region: &str) -> Stitch {
    let file = episode
        .files
        .iter()
        .find(|candidate| candidate.path == path);
    let hunks: Vec<&Hunk> = file
        .map(|found| {
            found
                .hunks
                .iter()
                .filter(|hunk| hunk.region() == region)
                .collect()
        })
        .unwrap_or_default();
    let added: Vec<String> = hunks
        .iter()
        .flat_map(|hunk| hunk.added())
        .cloned()
        .collect();
    let removed: Vec<String> = hunks
        .iter()
        .flat_map(|hunk| hunk.removed())
        .cloned()
        .collect();
    Stitch {
        episode_id: episode.id.clone(),
        added: text::line_set(&added),
        removed: text::line_set(&removed),
    }
}

/// The first pair (older, later) in the chain where the later put back at least [`PUT_BACK`] of what the
/// older removed, returning the overlap and the older episode's id.
fn find_put_back(stitches: &[Stitch]) -> Option<(f64, String)> {
    for (index, older) in stitches.iter().enumerate() {
        for later in stitches.iter().skip(index + 1) {
            let ratio = text::overlap(&later.added, &older.removed);
            if ratio >= PUT_BACK {
                return Some((ratio, older.episode_id.clone()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::model::{Content, FileChange, change};

    fn edit(path: &str, region: &str, added: &[&str], removed: &[&str]) -> FileChange {
        FileChange {
            path: path.to_owned(),
            change: change::MODIFIED.to_owned(),
            add: added.len() as i64,
            del: removed.len() as i64,
            hunks: vec![Hunk::new(
                format!("@@ -1,2 +1,3 @@ {region}"),
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
    fn a_region_edited_three_times_that_puts_lines_back_is_a_loop() {
        let episodes = vec![
            episode(
                "ep-1",
                1,
                vec![edit("src/p.rs", "fn parse", &["let a = 1;"], &[])],
            ),
            episode(
                "ep-2",
                2,
                vec![edit("src/p.rs", "fn parse", &[], &["let a = 1;"])],
            ),
            episode(
                "ep-3",
                3,
                vec![edit(
                    "src/p.rs",
                    "fn parse",
                    &["let a = 1;"],
                    &["let b = 2;"],
                )],
            ),
        ];
        let findings = detect(&episodes[2], &Corpus::of(&episodes), &Policy::all_enabled());
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "churn_loop");
        assert!(
            finding.message.contains("3 episodes"),
            "{}",
            finding.message
        );
        assert!(
            finding.message.contains("ep-2"),
            "it names whose removal was undone: {}",
            finding.message
        );
        assert_eq!(finding.severity, Severity::Medium);
        assert!((finding.confidence - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn ordinary_iteration_that_never_reverts_is_silent() {
        let episodes = vec![
            episode(
                "ep-1",
                1,
                vec![edit("src/p.rs", "fn parse", &["let a = 1;"], &[])],
            ),
            episode(
                "ep-2",
                2,
                vec![edit("src/p.rs", "fn parse", &["let b = 2;"], &[])],
            ),
            episode(
                "ep-3",
                3,
                vec![edit("src/p.rs", "fn parse", &["let c = 3;"], &[])],
            ),
        ];
        assert!(
            detect(&episodes[2], &Corpus::of(&episodes), &Policy::all_enabled()).is_empty(),
            "adding three different things to one function is work, not a loop"
        );
    }

    #[test]
    fn two_episodes_are_not_a_loop() {
        let episodes = vec![
            episode(
                "ep-1",
                1,
                vec![edit("src/p.rs", "fn parse", &[], &["let a = 1;"])],
            ),
            episode(
                "ep-2",
                2,
                vec![edit("src/p.rs", "fn parse", &["let a = 1;"], &[])],
            ),
        ];
        assert!(detect(&episodes[1], &Corpus::of(&episodes), &Policy::all_enabled()).is_empty());
    }

    #[test]
    fn the_threshold_is_the_config_knob() {
        let episodes = vec![
            episode(
                "ep-1",
                1,
                vec![edit("src/p.rs", "fn parse", &["let a = 1;"], &[])],
            ),
            episode(
                "ep-2",
                2,
                vec![edit("src/p.rs", "fn parse", &[], &["let a = 1;"])],
            ),
            episode(
                "ep-3",
                3,
                vec![edit("src/p.rs", "fn parse", &["let a = 1;"], &[])],
            ),
            episode(
                "ep-4",
                4,
                vec![edit("src/p.rs", "fn parse", &[], &["let a = 1;"])],
            ),
        ];
        let policy = Policy {
            churn_min_episodes: 5,
            ..Policy::all_enabled()
        };
        assert!(detect(&episodes[3], &Corpus::of(&episodes), &policy).is_empty());
        assert_eq!(
            detect(&episodes[3], &Corpus::of(&episodes), &Policy::all_enabled()).len(),
            1,
            "one finding per region in the loop, whatever pairs it contains"
        );
    }

    #[test]
    fn a_different_region_in_the_same_file_is_a_different_loop() {
        let episodes = vec![
            episode(
                "ep-1",
                1,
                vec![edit("src/p.rs", "fn one", &[], &["let a = 1;"])],
            ),
            episode(
                "ep-2",
                2,
                vec![edit("src/p.rs", "fn two", &["let a = 1;"], &[])],
            ),
            episode(
                "ep-3",
                3,
                vec![edit("src/p.rs", "fn two", &["let a = 1;"], &[])],
            ),
        ];
        let findings = detect(&episodes[2], &Corpus::of(&episodes), &Policy::all_enabled());
        assert!(
            findings.is_empty(),
            "the put-back is in a different region: {findings:?}"
        );
    }

    #[test]
    fn look_back_is_capped_so_an_old_pair_is_not_a_loop() {
        // The put-back pair is at ordinals 1 and 2. At the default window (12 episodes) the loop cannot
        // see them and says nothing; widen the window past them and the same corpus is a loop.
        let mut episodes = vec![
            episode(
                "ep-old-1",
                1,
                vec![edit("src/p.rs", "fn parse", &[], &["let a = 1;"])],
            ),
            episode(
                "ep-old-2",
                2,
                vec![edit("src/p.rs", "fn parse", &["let a = 1;"], &[])],
            ),
        ];
        for n in 3..=30 {
            episodes.push(episode(
                &format!("ep-{n}"),
                n,
                vec![edit("src/p.rs", "fn parse", &["let other = 1;"], &[])],
            ));
        }
        let target = episode(
            "ep-target",
            31,
            vec![edit("src/p.rs", "fn parse", &["let new = 1;"], &[])],
        );
        episodes.push(target.clone());
        let corpus = Corpus::of(&episodes);
        assert!(
            detect(&target, &corpus, &Policy::all_enabled()).is_empty(),
            "the pair is outside the window"
        );
        let wide = Policy {
            churn_min_episodes: 10,
            ..Policy::all_enabled()
        };
        let findings = detect(&target, &corpus, &wide);
        assert_eq!(
            findings.len(),
            1,
            "the same corpus is a loop once the window reaches the pair"
        );
    }
}

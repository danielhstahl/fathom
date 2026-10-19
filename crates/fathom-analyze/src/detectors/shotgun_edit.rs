//! `shotgun_edit.rs` — one episode reaching into places with nothing to do with each other (`e4.3`).
//!
//! **Ships disabled.** Not because the shape is rare — it is the most visible thing an unconstrained agent
//! does — but because *at path level* it is indistinguishable from two legitimate activities: a cross-cutting
//! rename, and a feature that legitimately touches `src/parser`, `src/api`, `crates/fathom-store` and
//! `tools/`. The false-positive gate is `e4.3a`'s, and the real fix is named in the catalogue's `reenable`
//! line: `e3.1`/`e3.2`'s module graph turns "clusters" from directories into actual coupling, and a shotgun
//! measured on edges is a different and much better detector than one measured on path segments.
//!
//! **What it claims now.** The episode touched at least `detectors.shotgun.min_clusters` (default 4)
//! *directory clusters* — the first two path segments of each hand-written, non-test file — with at least
//! twice that many files, and it is not a module move.
//!
//! **Three refusals, all of them attacks on its own false-positive rate.**
//!
//! * **a module move.** If at least half the files arrived by a rename or a copy, this is a relocation: one
//!   act in many places, which is what a relocation *is*. Silent.
//! * **a feature with its tests and its docs.** Test, docs, asset and noise surfaces do not form clusters. A
//!   change to `src/parser`, `src/api` plus `tests/**` and `docs/**` is two clusters, not four, and is the
//!   ordinary shape of good work.
//! * **a small change.** Under twice the cluster count in files there is nothing to say: four files in four
//!   places is a small cross-cutting fix, and printing that is how every reviewer learns to ignore the
//!   queue.
//!
//! **Confidence `0.45`, the lowest in the catalogue**, and it is a ceiling rather than an estimate: a path
//! heuristic that cannot see a single dependency edge deserves no more, and the number is printed with the
//! finding so nobody mistakes it for the graph-based detector it is waiting to become.

use std::collections::BTreeMap;

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::risk::Surface;

/// How many path segments make a cluster. Two reaches `src/parser` and `crates/fathom-store` without
/// reaching individual files, which would make every directory of ten files its own cluster.
const CLUSTER_DEPTH: usize = 2;

pub fn detect(episode: &Episode, _corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    let files: Vec<&FileChange> = episode.files.iter().filter(|file| counts(file)).collect();
    if files.len() < policy.shotgun_min_clusters.saturating_mul(2) {
        return Vec::new();
    }
    let moves = files.iter().filter(|file| file.is_move()).count();
    if moves.saturating_mul(2) >= files.len() {
        return Vec::new();
    }

    let mut clusters: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for file in &files {
        clusters
            .entry(cluster(&file.path))
            .or_default()
            .push(file.path.as_str());
    }
    if clusters.len() < policy.shotgun_min_clusters {
        return Vec::new();
    }

    let named: Vec<&str> = clusters.keys().map(String::as_str).collect();
    let widest = clusters
        .iter()
        .max_by_key(|(_, paths)| (paths.len(), paths.join("/")))
        .map(|(cluster, paths)| format!("{cluster} ({} file(s))", paths.len()))
        .unwrap_or_default();
    vec![
        Finding::build(
            "shotgun_edit",
            1,
            &episode.id,
            // The subject is the shape, not the episode: a key that carried the episode id would have to
            // be retyped after every rebase, which is the one thing a suppression may not do.
            "clusters",
            format!(
                "one episode reaches {} separate clusters ({}), widest {} — a change this spread is \
                 either a cross-cutting fix or a hunt-and-patch, and a path list cannot tell them apart",
                clusters.len(),
                crate::detectors::named(&named, policy.name_cap),
                widest
            ),
        )
        .severity(Severity::Medium)
        .confidence(0.45)
        .evidence(Evidence::inferred(
            EvidenceKind::Path,
            format!("fathom show {} --files", episode.id),
            clusters
                .iter()
                .map(|(cluster, paths)| format!("{cluster}: {}", paths.join(" ")))
                .collect::<Vec<_>>()
                .join(" | "),
        ))
        .evidence(Evidence::absent(
            EvidenceKind::Path,
            "e3.1",
            "no module graph in this build, so 'unrelated' is a directory heuristic and not a measured \
             absence of coupling",
        ))
        .jump(crate::detectors::finding::view::STRUCTURE, episode.id.clone())
        .finish(),
    ]
}

/// Only the surfaces where a person wrote logic. Tests, docs, assets, lockfiles, generated and vendored
/// churn are the ordinary company of any real change and would each add a cluster of their own.
fn counts(file: &FileChange) -> bool {
    if file.binary || file.noisy() || file.is_test() {
        return false;
    }
    matches!(
        file.surface(),
        Surface::Source
            | Surface::Migration
            | Surface::Ci
            | Surface::Infra
            | Surface::Container
            | Surface::Env
            | Surface::Schema
    )
}

/// The first [`CLUSTER_DEPTH`] path segments, or the whole directory when the file sits shallower.
fn cluster(path: &str) -> String {
    let dirs = path
        .rsplit_once('/')
        .map(|(dir, _)| {
            dir.split('/')
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if dirs.is_empty() {
        return ".".to_owned();
    }
    dirs.iter()
        .take(CLUSTER_DEPTH)
        .copied()
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::model::{Content, Hunk, change};

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

    fn episode(files: Vec<FileChange>) -> Episode {
        Episode {
            id: "ep-1".to_owned(),
            files,
            content: Content::Full,
            ..Episode::new("ep-1")
        }
    }

    fn spread(clusters: &[&str], per: usize) -> Vec<FileChange> {
        let mut out = Vec::new();
        for cluster in clusters {
            for n in 0..per {
                out.push(file(&format!("{cluster}/file{n}.rs")));
            }
        }
        out
    }

    #[test]
    fn four_clusters_is_the_shape_it_is_looking_for() {
        let episode = episode(spread(
            &[
                "src/parser",
                "src/api",
                "crates/fathom-store",
                "tools/migrate",
            ],
            2,
        ));
        let findings = detect(&episode, &Corpus::default(), &Policy::all_enabled());
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "shotgun_edit");
        assert!(
            finding.message.contains("4 separate clusters"),
            "{}",
            finding.message
        );
        assert!(
            (finding.confidence - 0.45).abs() < f64::EPSILON,
            "the ceiling, stated"
        );
        assert_eq!(finding.jump.view, "structure");
        assert_eq!(
            finding.suppression_key, "shotgun_edit:clusters",
            "the key is the shape, not the episode: a rebase must not invalidate a suppression"
        );
    }

    #[test]
    fn three_clusters_is_ordinary_work() {
        let episode = episode(spread(&["src/parser", "src/api", "tools/migrate"], 2));
        assert!(detect(&episode, &Corpus::default(), &Policy::all_enabled()).is_empty());
    }

    #[test]
    fn a_cross_cutting_fix_in_three_files_is_too_small_to_claim_anything() {
        let episode = episode(vec![
            file("src/parser/a.rs"),
            file("src/api/b.rs"),
            file("tools/migrate/c.rs"),
            file("crates/fathom-store/d.rs"),
        ]);
        assert!(
            detect(&episode, &Corpus::default(), &Policy::all_enabled()).is_empty(),
            "four files in four places is a small fix, and printing it teaches a reviewer to ignore the queue"
        );
    }

    #[test]
    fn a_module_move_is_not_a_shotgun() {
        let mut files = spread(
            &[
                "src/parser",
                "src/api",
                "crates/fathom-store",
                "tools/migrate",
            ],
            2,
        );
        for (index, moved) in files.iter_mut().enumerate() {
            moved.change = change::RENAMED.to_owned();
            moved.old_path = Some(format!("old/location{index}.rs"));
        }
        assert!(detect(&episode(files), &Corpus::default(), &Policy::all_enabled()).is_empty());
    }

    #[test]
    fn tests_docs_and_generated_churn_do_not_make_clusters() {
        let mut files = spread(&["src/parser", "src/api"], 2);
        files.extend(spread(
            &["tests/api", "docs/guide", "src/generated", "dist"],
            2,
        ));
        assert!(
            detect(&episode(files), &Corpus::default(), &Policy::all_enabled()).is_empty(),
            "a feature with its tests, its docs and its build output is two clusters wearing four"
        );
    }

    #[test]
    fn the_threshold_is_the_config_knob() {
        let episode = episode(spread(&["src/parser", "src/api", "tools/migrate"], 3));
        let policy = Policy {
            shotgun_min_clusters: 3,
            ..Policy::all_enabled()
        };
        assert_eq!(detect(&episode, &Corpus::default(), &policy).len(), 1);
    }

    #[test]
    fn a_cluster_is_two_segments_and_root_files_are_one_cluster() {
        assert_eq!(cluster("Cargo.toml"), ".");
        assert_eq!(
            cluster("README.md"),
            ".",
            "the repository's own front room is one place"
        );
        assert_eq!(cluster("src/parser/grammar.rs"), "src/parser");
        assert_eq!(
            cluster("crates/fathom-store/src/lib.rs"),
            "crates/fathom-store"
        );
        assert_eq!(cluster("tools/migrate.rs"), "tools");
    }

    #[test]
    fn root_files_alone_do_not_make_four_clusters() {
        let episode = episode(vec![
            file("build.rs"),
            file("build2.rs"),
            file("build3.rs"),
            file("build4.rs"),
            file("src/a.rs"),
            file("src/b.rs"),
        ]);
        let findings = detect(&episode, &Corpus::default(), &Policy::all_enabled());
        assert!(
            findings.is_empty(),
            "six files in two clusters is not a spread: {findings:?}"
        );
    }
}

//! Rows for commits that no longer exist (`reco-218`).
//!
//! A rebase, an `--amend` or a force-push replaces commits *by identity*: the replacements are new rows, the
//! old rows stay (the spine is append-only, ADR-004 §6.2), and `git gc --prune=now` then deletes the objects
//! so the repository ends up with **fewer** commits than the index counts. The store's half of that is three
//! promises, and every test below is one of them:
//!
//! 1. the scale the status verbs read counts the history the refs reach, and the rows they do not reach are
//!    counted separately, as a set with a size and a name;
//! 2. where the store *cannot* tell the two apart — no ref tip to walk from, a parent row missing below a
//!    commit that is still live — it says it could not judge instead of guessing in either direction;
//! 3. the pruner removes the provable ones, refuses the rest, and never takes a row a review points at
//!    (ADR-004 §8.2's rule that a prune may not lose a review).
//!
//! As in [`episodes`](episodes.rs), the fixtures are written with `write_commits` rather than with git: as
//! far as this crate is concerned a rebase is "new rows arrive, the ref moves, the old rows stay".
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use fathom_store::episode::{EpisodeRow, ReviewState};
use fathom_store::spine::{CommitRow, FileRow, Store, Trigger};

const T0: i64 = 1_700_000_000_000;

fn oid(n: u8) -> String {
    format!("{:040x}", n)
}

/// A commit row: one file, ten added lines, authored at `T0 + at` seconds.
fn commit(n: u8, parent: Option<u8>, at: i64) -> CommitRow {
    CommitRow {
        oid: oid(n),
        tree_oid: "b".repeat(40),
        parent_oids: parent.map(|p| vec![oid(p)]).unwrap_or_default(),
        author_name: "pi-agent".to_owned(),
        author_email: "pi-agent@example.invalid".to_owned(),
        author_utc: T0 + at * 1_000,
        committer_utc: T0 + at * 1_000,
        committer_name: "pi-agent".to_owned(),
        committer_email: "pi-agent@example.invalid".to_owned(),
        message_head: format!("commit {n}"),
        message_hash: format!("{:064x}", n),
        trailers: BTreeMap::new(),
        n_files: 1,
        n_add: 10,
        n_del: 2,
        n_binary: 0,
        oversized: false,
        files: vec![FileRow {
            path_hash: format!("{:064x}", n),
            new_path: format!("src/f{n}.py"),
            old_path: None,
            change_kind: "modify".to_owned(),
            mode_old: Some(0o100644),
            mode_new: Some(0o100644),
            is_binary: false,
            n_add: 10,
            n_del: 2,
            hunk_headers: vec!["@@ -1 +1 @@".to_owned()],
            blob_old: None,
            blob_new: Some("e".repeat(40)),
            path_class: "source".to_owned(),
            taxonomy_version: 1,
            noise_class: "informative".to_owned(),
        }],
    }
}

/// A store with `rows`, the tip of `refs/heads/main` at `tip`, and `extra` refs on top.
fn seeded(rows: &[CommitRow], tip: &str, extra: &[(&str, &str, &str)]) -> Store {
    let mut store = Store::open_in_memory().expect("open in memory");
    let run = store.begin_run("cold", Trigger::Manual, T0).unwrap();
    store.write_commits(run, T0, rows).unwrap();
    let mut tips: Vec<(String, String, String)> = vec![
        ("HEAD".to_owned(), tip.to_owned(), "head".to_owned()),
        (
            "refs/heads/main".to_owned(),
            tip.to_owned(),
            "branch".to_owned(),
        ),
    ];
    tips.extend(
        extra
            .iter()
            .map(|(name, oid, kind)| ((*name).to_owned(), (*oid).to_owned(), (*kind).to_owned())),
    );
    store
        .replace_ref_tips(
            &tips
                .iter()
                .map(|(name, oid, kind)| (name.clone(), oid.clone(), kind.as_str()))
                .collect::<Vec<_>>(),
            T0,
        )
        .unwrap();
    store
}

/// The five-commit history, then the same five with the top two rewritten: `1←2←3←4←5` becomes
/// `1←2←3←4'←5'`, and 4 and 5 are rows for commits git has already forgotten.
fn rebased() -> Store {
    let rows = vec![
        commit(1, None, 0),
        commit(2, Some(1), 1),
        commit(3, Some(2), 2),
        commit(4, Some(3), 3),
        commit(5, Some(4), 4),
        commit(6, Some(3), 5),
        commit(7, Some(6), 6),
    ];
    seeded(&rows, &oid(7), &[])
}

#[test]
fn a_history_the_refs_reach_has_nothing_dead() {
    let rows = vec![
        commit(1, None, 0),
        commit(2, Some(1), 1),
        commit(3, Some(2), 2),
    ];
    let store = seeded(&rows, &oid(3), &[]);
    let dead = store.dead_rows().unwrap();
    assert_eq!((dead.retained, dead.reachable, dead.dead), (3, 3, 0));
    assert!(dead.is_empty() && dead.judged());
    assert_eq!(store.scale().unwrap().commits, 3);
}

#[test]
fn a_rebase_leaves_rows_behind_and_the_store_names_them() {
    let store = rebased();
    let dead = store.dead_rows().unwrap();
    assert_eq!(dead.retained, 7, "five rewritten plus two replacements");
    assert_eq!(dead.reachable, 5, "the repository git would report");
    assert_eq!(
        dead.dead, 2,
        "commits 4 and 5 are rows for commits that do not exist"
    );
    assert_eq!(dead.dead_files, 2, "each carries its one file row");
    assert_eq!(dead.unjudged, 0);
    assert!(
        dead.sample.iter().any(|id| id == &oid(5)) && dead.sample.iter().any(|id| id == &oid(4)),
        "the sample names the ghosts: {:?}",
        dead.sample
    );
    assert_eq!(
        dead.sample.first().map(String::as_str),
        Some(oid(5).as_str()),
        "newest first, so a report line names the commit the reviewer is most likely to remember"
    );
}

#[test]
fn the_scale_counts_the_reachable_history_not_the_file() {
    let store = rebased();
    let scale = store.scale().unwrap();
    // The bug in one assertion: `fathom stats` printed 7 for a repository with 5 commits.
    assert_eq!(scale.commits, 5);
    assert_eq!(
        scale.files, 5,
        "the dead commits' file rows are not the repository's either"
    );
    assert_eq!(
        scale.lines_added, 50,
        "the churn window is the reachable history's churn"
    );
    assert_eq!(scale.lines_removed, 10);
    assert_eq!(
        scale.noise.get("informative").copied(),
        Some(5),
        "even the taxonomy histogram was counting them: {:?}",
        scale.noise
    );
    let inventory = store.inventory().unwrap();
    assert_eq!(
        inventory.scale.commits + inventory.dead.dead,
        inventory.dead.retained,
        "the two halves of one read add up to the file"
    );
}

#[test]
fn a_store_with_no_ref_tip_to_walk_from_has_no_opinion() {
    // Rows without a tip is a state a store can genuinely be in — a walk that found a ref it could not name,
    // a test that seeded rows directly — and the answer is "I cannot tell", never "everything is dead".
    let rows = vec![commit(1, None, 0), commit(2, Some(1), 1)];
    let mut store = Store::open_in_memory().unwrap();
    let run = store.begin_run("cold", Trigger::Manual, T0).unwrap();
    store.write_commits(run, T0, &rows).unwrap();
    let dead = store.dead_rows().unwrap();
    assert_eq!(dead.dead, 0);
    assert_eq!(dead.unjudged, 2);
    assert_eq!(dead.retained, 2);
    assert!(!dead.judged());
    assert!(dead.reason.is_some(), "and it says why: {:?}", dead.reason);
    assert_eq!(
        store.scale().unwrap().commits,
        2,
        "an unjudgeable store reports its whole scale, not a history that shrank"
    );
}

#[test]
fn a_tip_with_no_commit_row_is_not_a_history_with_no_commits_in_it() {
    // HEAD moved to a commit the walk could not read. The tip is the fact; the absence is the walk's.
    let rows = vec![commit(1, None, 0), commit(2, Some(1), 1)];
    let store = seeded(&rows, &"f".repeat(40), &[]);
    let dead = store.dead_rows().unwrap();
    assert_eq!(dead.dead, 0, "nothing was proved dead");
    assert_eq!(dead.unjudged, 2, "and nothing could be proved live");
    assert_eq!(dead.tips_without_rows, 2, "both HEAD and main name nothing");
    assert_eq!(store.scale().unwrap().commits, 2);
}

#[test]
fn a_break_in_the_stored_chain_is_unprovable_not_dead() {
    // `2` names `9` as its parent and this store has no row for 9 — a shallow graft, or an object the walk
    // could not read. The rows below the break might be its ancestors, so *nothing* can be called dead.
    let rows = vec![
        commit(1, Some(9), 0),
        commit(2, Some(1), 1),
        commit(3, Some(2), 2),
    ];
    let store = seeded(&rows, &oid(3), &[]);
    let dead = store.dead_rows().unwrap();
    assert!(
        dead.judged(),
        "the walk ran; it just hit a wall it can't see past"
    );
    assert_eq!(dead.dead, 0, "an unprovable row is not a dead row");
    assert_eq!(dead.unjudged, 0, "and here every row is reachable anyway");
    assert_eq!(dead.reachable, 3);
}

#[test]
fn a_break_in_the_chain_holds_every_unreachable_row_out_of_the_dead_set() {
    // Same break, plus a row nothing reaches: is it a ghost of a rewrite, or an ancestor below the break?
    // The store cannot tell, so it says `unjudged` and leaves the count of dead at zero.
    let mut rows = vec![commit(1, Some(9), 0), commit(2, Some(1), 1)];
    rows.push(commit(4, Some(3), 3));
    rows.push(commit(3, Some(2), 2));
    let store = seeded(&rows, &oid(2), &[]);
    let dead = store.dead_rows().unwrap();
    assert_eq!(dead.retained, 4);
    assert_eq!(dead.reachable, 2);
    assert_eq!(dead.dead, 0);
    assert_eq!(
        dead.unjudged, 2,
        "commits 3 and 4 might live below the break"
    );
}

#[test]
fn a_tag_pinned_to_a_rewritten_commit_does_not_keep_its_row_alive() {
    // ADR-003's reading, and the one `crate::episode` already applies: the tag records the release that
    // commit *was*. It does not make the row part of the history a reviewer is being asked to size.
    let store = rebased();
    let mut store = store;
    store
        .replace_ref_tips(
            &[
                ("HEAD".to_owned(), oid(7), "head"),
                ("refs/heads/main".to_owned(), oid(7), "branch"),
                ("refs/tags/v1".to_owned(), oid(5), "tag"),
            ],
            T0,
        )
        .unwrap();
    assert_eq!(store.dead_rows().unwrap().dead, 2);
}

#[test]
fn a_commit_a_remote_branch_names_is_not_dead() {
    // Someone else's branch is a fact about the repository, not about this checkout.
    let store = rebased();
    let mut store = store;
    store
        .replace_ref_tips(
            &[
                ("HEAD".to_owned(), oid(7), "head"),
                ("refs/heads/main".to_owned(), oid(7), "branch"),
                ("refs/remotes/origin/topic".to_owned(), oid(4), "remote"),
            ],
            T0,
        )
        .unwrap();
    let dead = store.dead_rows().unwrap();
    assert_eq!(dead.dead, 1, "only commit 5 is unreachable now");
    assert_eq!(dead.sample, vec![oid(5)]);
    assert_eq!(
        dead.reachable, 6,
        "commit 4 is on a branch, so it is not a ghost"
    );
}

// ---------------------------------------------------------------------------
// the prune
// ---------------------------------------------------------------------------

#[test]
fn a_prune_takes_the_rows_no_ref_reaches_and_reports_what_it_took() {
    let mut store = rebased();
    let before = store.scale().unwrap();
    let dry = store.prune_dead_commits(true, T0).unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.commits_removed, 2);
    assert_eq!(dry.files_removed, 2);
    assert!(dry.removed_anything());
    let (_, files) = store.counts().unwrap();
    assert_eq!(
        store.dead_rows().unwrap().dead,
        2,
        "a dry run must not have deleted anything"
    );
    assert_eq!(files, 7, "and the file rows are all still there");

    let run = store.prune_dead_commits(false, T0).unwrap();
    assert!(!run.dry_run);
    assert_eq!((run.commits_removed, run.files_removed), (2, 2));
    assert_eq!(run.sample.len(), 2);
    let dead = store.dead_rows().unwrap();
    assert_eq!((dead.retained, dead.dead), (5, 0), "the ghosts are gone");
    let after = store.scale().unwrap();
    assert_eq!(
        after, before,
        "and the reachable history is exactly what it was: {before:?} -> {after:?}"
    );
    assert_eq!(store.counts().unwrap(), (5, 5));
}

#[test]
fn a_prune_of_a_clean_store_says_so_and_writes_nothing() {
    let rows = vec![commit(1, None, 0), commit(2, Some(1), 1)];
    let mut store = seeded(&rows, &oid(2), &[]);
    let run = store.prune_dead_commits(false, T0).unwrap();
    assert!(run.is_empty());
    assert_eq!(run.commits_removed, 0);
    assert_eq!(store.counts().unwrap(), (2, 2));
}

#[test]
fn a_prune_refuses_when_the_store_cannot_tell() {
    let rows = vec![commit(1, None, 0), commit(2, Some(1), 1)];
    let mut store = Store::open_in_memory().unwrap();
    let run = store.begin_run("cold", Trigger::Manual, T0).unwrap();
    store.write_commits(run, T0, &rows).unwrap();
    let report = store.prune_dead_commits(false, T0).unwrap();
    assert_eq!(report.commits_removed, 0);
    assert_eq!(report.withheld_undecidable, 2);
    assert!(
        report.refusal.is_some(),
        "and it says why: {:?}",
        report.refusal
    );
    assert_eq!(store.counts().unwrap(), (2, 2), "and nothing went");
}

#[test]
fn a_prune_never_loses_a_review() {
    // ADR-004 §8.2 and §8.7: the guard is allowed to be loose, because a false positive costs bytes and a
    // false negative costs a sign-off. One dead commit carries an accepted episode; it stays.
    let mut store = rebased();
    let episode = |id: &str, member: u8| EpisodeRow {
        id: id.to_owned(),
        fingerprint: format!("{id}-fingerprint"),
        kind: "committed".to_owned(),
        lineage_ref: "refs/heads/main".to_owned(),
        created_utc: T0,
        created_source: "index".to_owned(),
        created_confidence: 0.7,
        boundary_rule: "author".to_owned(),
        boundary_confidence: 0.75,
        author_kind: "agent".to_owned(),
        author_confidence: 0.85,
        first_oid: oid(member),
        last_oid: oid(member),
        members: vec![oid(member)],
        author_name: "pi-agent".to_owned(),
        author_email: "pi-agent@example.invalid".to_owned(),
        summary: format!("commit {member}"),
        live: true,
        ..EpisodeRow::default()
    };
    store
        .write_episodes(T0, &[episode("kept", 5), episode("gone", 4)])
        .unwrap();
    store
        .set_review("kept", ReviewState::Accepted, "mira", None, T0)
        .unwrap();

    let report = store.prune_dead_commits(false, T0).unwrap();
    assert_eq!(report.commits_removed, 1, "only the unreviewed ghost went");
    assert_eq!(
        report.withheld_reviewed, 1,
        "and the reviewed one is counted as withheld"
    );
    assert_eq!(
        report.episodes_removed, 1,
        "the episode with no members left was retired with it"
    );
    assert_eq!(
        store.episode_members("kept").unwrap(),
        vec![oid(5)],
        "the decision still has the commit it was written against"
    );
    assert!(store.episode("gone").unwrap().is_none());
    assert!(
        store.review_states().unwrap().contains_key("kept"),
        "the review itself survived the prune"
    );
}

#[test]
fn a_prune_never_loses_an_entry_the_ledger_signed() {
    // The other half of §8.2's rule, and a guard a review cannot cover for: `review_state` is the store's
    // own queue, but the ledger is the artefact a person signed and the mirrored row is the only thing that
    // says which commits that sign-off covered. Losing the commit row loses the ability to answer that —
    // ADR-004 §8.7's reason the dead set may never be deleted on a judgment call.
    let mut store = rebased();
    let episode = |id: &str, member: u8| EpisodeRow {
        id: id.to_owned(),
        fingerprint: format!("{id}-fingerprint"),
        kind: "committed".to_owned(),
        lineage_ref: "refs/heads/main".to_owned(),
        created_utc: T0,
        created_source: "index".to_owned(),
        created_confidence: 0.7,
        boundary_rule: "author".to_owned(),
        boundary_confidence: 0.75,
        author_kind: "agent".to_owned(),
        author_confidence: 0.85,
        first_oid: oid(member),
        last_oid: oid(member),
        members: vec![oid(member)],
        author_name: "pi-agent".to_owned(),
        author_email: "pi-agent@example.invalid".to_owned(),
        summary: format!("commit {member}"),
        live: true,
        ..EpisodeRow::default()
    };
    store
        .write_episodes(T0, &[episode("kept", 5), episode("signed", 4)])
        .unwrap();
    // No review row at all: the only thing pointing at `signed` is the mirrored entry.
    store
        .mirror_ledger(
            T0,
            &[fathom_store::ledger::LedgerRow {
                id: "sg-1".to_owned(),
                file: "REVIEW.md".to_owned(),
                anchor: "sg-1".to_owned(),
                episode_id: Some("signed".to_owned()),
                decision: "accepted".to_owned(),
                actor: "mira".to_owned(),
                actor_kind: "human".to_owned(),
                signed_utc: T0,
                content_sha: "sha".to_owned(),
                link_method: "fingerprint".to_owned(),
                state: "fresh".to_owned(),
                ..Default::default()
            }],
        )
        .unwrap();

    let report = store.prune_dead_commits(false, T0).unwrap();
    assert_eq!(
        report.commits_removed, 1,
        "only the commit nothing was signed about went"
    );
    assert_eq!(
        report.withheld_reviewed, 1,
        "and the one a signed entry resolves to is counted as withheld"
    );
    assert!(
        store
            .ledger_rows()
            .unwrap()
            .iter()
            .any(|row| row.episode_id.as_deref() == Some("signed")),
        "the entry still resolves to something"
    );
}

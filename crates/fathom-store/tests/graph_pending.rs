//! The graph lane's work list (`e3.11`, `reco-dlp`): which revisions `fathom index`'s graph pass is asked
//! to write, and what throwing the lane away costs.
//!
//! `Store::graph_pending_revisions` is the only place that decides "which revisions does the graph owe", and
//! three of its answers are load-bearing for the pass that reads it:
//!
//! * **live means reachable from a stored ref**, not "ever written". A rebased branch leaves commit rows
//!   behind (`reco-6aw`'s prune is the only thing that removes them), and a writer that walked them would
//!   spend the index's budget drawing graphs of a history nobody has checked out;
//! * **a cap takes the newest**, because a reviewer's question is always about today — a writer capped at
//!   the oldest revisions would re-draw the same ancient window in every run and never arrive;
//! * **the order is oldest first**, because a delta needs the base it follows, and a chain written out of
//!   order is a lane full of anchors pretending to be deltas.
//!
//! `Store::graph_reset` is the other half: ADR-004 §9.5's rule that derived data is recomputed rather than
//! migrated, and the count of what went, because `--refresh` has to be able to say what it threw away.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use fathom_store::graph::{DeltaOp, NodeEntry, Op, SnapshotRow};
use fathom_store::spine::{CommitRow, FileRow, Store, Trigger};

/// The instant the fixture's commits are dated from. Ascending per commit, because the work list's order is
/// derived from it and a fixture all dated at once would test the tie-break instead of the rule.
const BASE: i64 = 1_700_000_000_000;

/// A commit row: `n`th in a linear chain, dated `n` seconds after the base so the date order is the
/// history's order.
fn commit(n: usize, parents: &[usize]) -> CommitRow {
    let hex = |value: usize| format!("{value:040x}");
    CommitRow {
        oid: hex(n + 1),
        tree_oid: format!("{:02x}{:038x}", n + 0x10, n),
        parent_oids: parents.iter().map(|p| hex(*p + 1)).collect(),
        author_name: "Ada Lovelace".into(),
        author_email: "ada@example.invalid".into(),
        author_utc: BASE + i64::try_from(n).unwrap(),
        committer_utc: BASE + i64::try_from(n).unwrap(),
        committer_name: "Ada Lovelace".into(),
        committer_email: "ada@example.invalid".into(),
        message_head: format!("commit {n}"),
        message_hash: format!("{:064x}", n),
        trailers: BTreeMap::new(),
        n_files: 1,
        n_add: 1,
        n_del: 0,
        n_binary: 0,
        oversized: false,
        files: vec![FileRow {
            path_hash: format!("{:064x}", n),
            new_path: format!("src/m{n}.rs"),
            old_path: None,
            change_kind: "add".into(),
            mode_old: None,
            mode_new: Some(0o100644),
            is_binary: false,
            n_add: 1,
            n_del: 0,
            hunk_headers: vec![],
            blob_old: None,
            blob_new: Some(hex(n)),
            path_class: "source".into(),
            taxonomy_version: 0,
            noise_class: "unknown".into(),
        }],
    }
}

/// A linear history of `n` commits, each the first child of the last, with `refs/heads/main` at the tip.
fn chain(store: &mut Store, n: usize) -> Vec<String> {
    let run = store.begin_run("cold", Trigger::Commit, BASE).unwrap();
    let rows: Vec<CommitRow> = (0..n)
        .map(|i| {
            let parents: Vec<usize> = if i == 0 { vec![] } else { vec![i - 1] };
            commit(i, &parents)
        })
        .collect();
    store.write_commits(run, BASE, &rows).unwrap();
    store
        .replace_ref_tips(
            &[(
                "refs/heads/main".to_owned(),
                format!("{:040x}", n),
                "branch",
            )],
            BASE,
        )
        .unwrap();
    rows.iter().map(|row| row.tree_oid.clone()).collect()
}

#[test]
fn an_empty_lane_owes_every_live_revision_and_lists_them_oldest_first() {
    let mut store = Store::open_in_memory().expect("open store");
    let trees = chain(&mut store, 3);
    let work = store.graph_pending_revisions(10).expect("work list");
    assert_eq!(work.owed, 3, "{work:?}");
    assert_eq!(
        work.revisions
            .iter()
            .map(|rev| rev.tree_oid.clone())
            .collect::<Vec<_>>(),
        trees,
        "the chain in the order a delta chain is written in"
    );
    assert_eq!(
        work.revisions[0].first_parent_tree, None,
        "a root commit has no base, which is the lane's own answer for 'this one is an anchor'"
    );
    assert_eq!(
        work.revisions[1].first_parent_tree.as_deref(),
        Some(trees[0].as_str()),
        "and every later revision names the tree its first parent held"
    );
    assert!(
        work.revisions[0].when <= work.revisions[2].when,
        "oldest first, because the writer stamps with the commit's own date"
    );
}

#[test]
fn a_revision_that_already_has_a_row_is_not_owed_again() {
    let mut store = Store::open_in_memory().expect("open store");
    let trees = chain(&mut store, 3);
    store
        .graph_write_snapshot(&SnapshotRow::anchor(&trees[1]))
        .expect("write a row for the middle revision");
    let work = store.graph_pending_revisions(10).expect("work list");
    assert_eq!(work.owed, 2, "{work:?}");
    assert_eq!(work.revisions.len(), 2);
    assert!(
        work.revisions.iter().all(|rev| rev.tree_oid != trees[1]),
        "the keyed thing is the tree, so a second commit sharing a tree is just as done: {:?}",
        work.revisions
    );
}

#[test]
fn a_capped_pass_takes_the_newest_and_says_how_much_is_left() {
    let mut store = Store::open_in_memory().expect("open store");
    let trees = chain(&mut store, 5);
    let work = store.graph_pending_revisions(2).expect("work list");
    assert_eq!(
        work.owed, 5,
        "the caller needs to be able to say what it left behind"
    );
    assert_eq!(
        work.revisions
            .iter()
            .map(|rev| rev.tree_oid.clone())
            .collect::<Vec<_>>(),
        vec![trees[3].clone(), trees[4].clone()],
        "the two newest, still oldest-first: a writer capped at the oldest window would never catch up"
    );
    assert_eq!(
        work.revisions[0].first_parent_tree.as_deref(),
        Some(trees[2].as_str()),
        "the base of the first revision a capped pass writes is a revision it was not given — the writer \
         must treat that as 'no base in hand', not as a chain it can claim"
    );
}

#[test]
fn commits_that_share_a_committer_date_still_come_out_in_chain_order() {
    // Three commits all dated at the same instant — what a rebase, a bot applying a series inside one second,
    // or any fixture that pins `GIT_COMMITTER_DATE` produces. A date order here is an arbitrary order, and a
    // writer handed the revisions in an arbitrary order has to anchor every one of them.
    let mut store = Store::open_in_memory().expect("open store");
    let run = store.begin_run("cold", Trigger::Commit, BASE).unwrap();
    let mut rows: Vec<CommitRow> = Vec::new();
    let parents: Vec<Vec<usize>> = vec![vec![], vec![0], vec![1]];
    for (n, parents) in parents.iter().enumerate() {
        let mut row = commit(n, parents);
        row.committer_utc = BASE;
        row.author_utc = BASE;
        rows.push(row);
    }
    // Written newest-first, the way a walk from HEAD writes them, so the rowids are not the answer either.
    rows.reverse();
    store.write_commits(run, BASE, &rows).expect("commits");
    store
        .replace_ref_tips(
            &[(
                "refs/heads/main".to_owned(),
                format!("{:040x}", 3),
                "branch",
            )],
            BASE,
        )
        .expect("ref tips");

    let work = store.graph_pending_revisions(10).expect("work list");
    let trees: Vec<String> = rows.iter().rev().map(|row| row.tree_oid.clone()).collect();
    assert_eq!(
        work.revisions
            .iter()
            .map(|rev| rev.tree_oid.clone())
            .collect::<Vec<_>>(),
        trees,
        "the first-parent links, not the clock, decide the order a chain is written in"
    );
}

#[test]
fn a_revision_no_ref_reaches_is_not_owed() {
    let mut store = Store::open_in_memory().expect("open store");
    let run = store.begin_run("cold", Trigger::Commit, BASE).unwrap();
    // main: c0 -> c1. side: c0 -> c2, and no stored ref reaches c2 — what a rebase leaves behind.
    store
        .write_commits(
            run,
            BASE,
            &[commit(0, &[]), commit(1, &[0]), commit(2, &[0])],
        )
        .expect("commits");
    store
        .replace_ref_tips(
            &[(
                "refs/heads/main".to_owned(),
                format!("{:040x}", 2),
                "branch",
            )],
            BASE,
        )
        .expect("ref tips");
    let work = store.graph_pending_revisions(10).expect("work list");
    assert_eq!(
        work.owed, 2,
        "the dead branch's commit gets no graph of its own: {work:?}"
    );

    // No ref tips at all is not "nothing is live". A store opened for the first time, or one whose refs were
    // read by an older build, still owes every revision it holds — answering "nothing owed" here would
    // leave the lane empty forever with nothing refusing.
    let mut bare = Store::open_in_memory().expect("open store");
    let run = bare.begin_run("cold", Trigger::Commit, BASE).unwrap();
    bare.write_commits(run, BASE, &[commit(0, &[]), commit(1, &[0])])
        .expect("commits");
    assert_eq!(
        bare.graph_pending_revisions(10).expect("work list").owed,
        2,
        "no roots to walk from means fall back to everything, not to nothing"
    );
}

#[test]
fn a_reset_empties_the_lane_and_counts_what_it_took() {
    let mut store = Store::open_in_memory().expect("open store");
    let trees = chain(&mut store, 2);
    let dictionary = store
        .graph_intern(&[
            NodeEntry::module("src/m0.rs", Some(0), "l0".to_string(), false),
            NodeEntry::module("src/m1.rs", Some(1), "l0".to_string(), false),
        ])
        .expect("intern");
    assert_eq!(dictionary.len(), 2);
    store
        .graph_write_snapshot(&SnapshotRow::anchor(&trees[0]))
        .expect("anchor");
    store
        .graph_write_ops(
            &trees[0],
            &[DeltaOp {
                op: Op::Add,
                kind: "module".into(),
                src: "1".into(),
                dst: String::new(),
                src_layer: "l0".into(),
                dst_layer: "l0".into(),
            }],
        )
        .expect("ops");

    let removed = store.graph_reset().expect("reset");
    assert_eq!(
        removed.get("graph_snapshot"),
        Some(&1),
        "the one revision this fixture wrote is the one that went: {removed:?}"
    );
    assert!(removed.get("graph_delta") >= Some(&1), "{removed:?}");
    assert!(removed.get("graph_node") >= Some(&2), "{removed:?}");

    let counts = store.graph_row_counts().expect("counts");
    for table in [
        "graph_snapshot",
        "graph_delta",
        "graph_chunk",
        "graph_chunk_link",
        "graph_node",
    ] {
        assert_eq!(counts.get(table), Some(&0), "{table} survived: {counts:?}");
    }
    // And the work list is the whole history again, which is what makes `--refresh` a re-derivation rather
    // than a deletion: the next pass is told to write all of it, not to patch what is left.
    assert_eq!(
        store.graph_pending_revisions(10).expect("work list").owed,
        2,
        "the lane is empty, so every live revision is owed again"
    );
}

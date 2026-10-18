//! The episode half of the store: writing derived episodes, filtering them, and the review state that has
//! to survive the rewrite of what it was written about (ADR-003's stability clause, `e1.3`).
//!
//! Segmentation itself is not tested here — that is [`fathom_ingest::segment`]'s own work, and the index
//! run's integration test covers the wiring. What this file insists on is the store's three promises: a
//! rewrite does not lose a decision, a filter composes as an `AND`, and nothing that nobody reviewed and
//! nobody can reach stays in the table.
//!
//! The fixtures are built with `write_commits` rather than with git because the store's contract is about
//! rows: a rebase, as far as this crate is concerned, is "four new commit rows arrive, the ref moves, and
//! the old rows stay where they were".
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use fathom_store::episode::{EpisodeQuery, EpisodeRow, ReviewState};
use fathom_store::spine::{CommitRow, FileRow, Store, Trigger};

/// A fixed instant; every row in every test is stamped relative to it.
const T0: i64 = 1_700_000_000_000;

/// The nth fixture oid — 40 hex characters, as git writes them.
fn oid(n: u8) -> String {
    format!("{:040x}", n)
}

/// A commit row touching `paths`, authored by `author` at `T0 + at` seconds.
fn commit(
    n: u8,
    parent: Option<u8>,
    author: &str,
    at: i64,
    subject: &str,
    paths: &[&str],
) -> CommitRow {
    CommitRow {
        oid: oid(n),
        tree_oid: "b".repeat(40),
        parent_oids: parent.map(|p| vec![oid(p)]).unwrap_or_default(),
        author_name: author.to_owned(),
        author_email: format!("{author}@example.invalid"),
        author_utc: T0 + at * 1_000,
        committer_utc: T0 + at * 1_000,
        committer_name: author.to_owned(),
        committer_email: format!("{author}@example.invalid"),
        message_head: subject.to_owned(),
        message_hash: format!("{:064x}", n),
        trailers: BTreeMap::new(),
        n_files: paths.len() as i64,
        n_add: 10,
        n_del: 2,
        n_binary: 0,
        oversized: false,
        files: paths
            .iter()
            .enumerate()
            .map(|(index, path)| FileRow {
                path_hash: format!("{:064x}", index + usize::from(n) * 100),
                new_path: (*path).to_owned(),
                old_path: None,
                change_kind: "modify".to_owned(),
                mode_old: Some(0o100644),
                mode_new: Some(0o100644),
                is_binary: false,
                n_add: 10,
                n_del: 2,
                hunk_headers: vec!["@@ -1,2 +1,4 @@".to_owned()],
                blob_old: None,
                blob_new: Some("e".repeat(40)),
                path_class: "source".to_owned(),
                taxonomy_version: 1,
                noise_class: "informative".to_owned(),
            })
            .collect(),
    }
}

/// A store holding `rows` as the history of `refs/heads/main`.
fn seeded(rows: &[CommitRow]) -> Store {
    let mut store = Store::open_in_memory().expect("open in memory");
    let run = store.begin_run("cold", Trigger::Manual, T0).unwrap();
    store.write_commits(run, T0, rows).unwrap();
    let tip = rows
        .last()
        .map(|row| row.oid.clone())
        .unwrap_or_else(|| "0".repeat(40));
    store
        .replace_ref_tips(
            &[
                ("HEAD".to_owned(), tip.clone(), "head"),
                ("refs/heads/main".to_owned(), tip, "branch"),
            ],
            T0,
        )
        .unwrap();
    store
}

/// An episode row with only the fields a store read needs filled in; the rest is `Default`.
fn episode(id: &str, fingerprint: &str, fallback: &str, members: &[u8]) -> EpisodeRow {
    let members: Vec<String> = members.iter().map(|n| oid(*n)).collect();
    EpisodeRow {
        id: id.to_owned(),
        fingerprint: fingerprint.to_owned(),
        fallback_fingerprint: fallback.to_owned(),
        kind: "committed".to_owned(),
        lineage_ref: "refs/heads/main".to_owned(),
        lineage_seq: 0,
        created_utc: T0,
        created_source: "index".to_owned(),
        created_confidence: 0.7,
        boundary_rule: "author".to_owned(),
        boundary_confidence: 0.75,
        author_kind: "agent".to_owned(),
        author_confidence: 0.85,
        first_oid: members.first().cloned().unwrap_or_default(),
        last_oid: members.last().cloned().unwrap_or_default(),
        members,
        n_files: 1,
        n_add: 10,
        n_del: 2,
        first_utc: T0,
        last_utc: T0,
        author_name: "pi-agent".to_owned(),
        author_email: "pi-agent@example.invalid".to_owned(),
        summary: "feat: the thing".to_owned(),
        live: true,
        ..EpisodeRow::default()
    }
}

#[test]
fn writing_the_same_episodes_twice_changes_nothing_and_claims_nothing() {
    let mut store = seeded(&[
        commit(1, None, "pi-agent", 0, "feat: one", &["src/a.rs"]),
        commit(2, Some(1), "pi-agent", 60, "feat: two", &["src/b.rs"]),
    ]);
    let rows = vec![
        episode("ep-one", "f1", "fb1", &[1]),
        episode("ep-two", "f2", "fb2", &[2]),
    ];

    let first = store.write_episodes(T0, &rows).unwrap();
    assert_eq!((first.inserted, first.updated, first.members), (2, 0, 2));
    assert_eq!(
        first.relinked, 0,
        "nothing was rewritten, so nothing to relink"
    );
    assert_eq!(first.retired, 0);

    let second = store.write_episodes(T0 + 1_000, &rows).unwrap();
    assert_eq!(
        (second.inserted, second.updated),
        (0, 2),
        "a rerun refreshes the derived columns; it does not create episodes twice"
    );
    assert_eq!(
        store.count_episodes(&EpisodeQuery::default()).unwrap(),
        2,
        "and the count says so too"
    );
}

#[test]
fn the_filter_composes_as_an_and_and_never_as_an_or() {
    let mut store = seeded(&[
        commit(1, None, "pi-agent", 0, "feat: store", &["src/store.rs"]),
        commit(2, Some(1), "daniel", 60, "fix: by hand", &["src/store.rs"]),
        commit(3, Some(2), "pi-agent", 120, "feat: cli", &["src/cli.rs"]),
    ]);
    let mut human_in_store = episode("ep-human-store", "f2", "fb2", &[2]);
    human_in_store.author_kind = "human".to_owned();
    human_in_store.author_name = "daniel".to_owned();
    store
        .write_episodes(
            T0,
            &[
                episode("ep-agent-store", "f1", "fb1", &[1]),
                human_in_store,
                episode("ep-agent-cli", "f3", "fb3", &[3]),
            ],
        )
        .unwrap();
    store
        .set_review("ep-agent-store", ReviewState::Accepted, "daniel", None, T0)
        .unwrap();

    let all = store.episodes(&EpisodeQuery::default()).unwrap();
    assert_eq!(all.len(), 3, "no filter is every live episode");

    let by_kind = store
        .episodes(&EpisodeQuery {
            kinds: vec!["human".to_owned()],
            ..EpisodeQuery::default()
        })
        .unwrap();
    assert_eq!(
        ids(&by_kind),
        vec!["ep-human-store"],
        "author_kind is the classification, not the author's name"
    );

    let by_author = store
        .episodes(&EpisodeQuery {
            authors: vec!["daniel".to_owned()],
            ..EpisodeQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&by_author), vec!["ep-human-store"]);

    let unread = store
        .episodes(&EpisodeQuery {
            state: Some(ReviewState::Unread),
            ..EpisodeQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&unread), vec!["ep-agent-cli", "ep-human-store"]);

    // The acceptance criterion's composition: author AND path AND unread.
    let composed = store
        .episodes(&EpisodeQuery {
            kinds: vec!["agent".to_owned()],
            path: Some("src/store.rs".to_owned()),
            state: Some(ReviewState::Unread),
            ..EpisodeQuery::default()
        })
        .unwrap();
    assert!(
        composed.is_empty(),
        "the only agent episode touching src/store.rs is accepted: {:?}",
        ids(&composed)
    );

    let same_composed_but_read = store
        .episodes(&EpisodeQuery {
            kinds: vec!["agent".to_owned()],
            path: Some("src/store.rs".to_owned()),
            ..EpisodeQuery::default()
        })
        .unwrap();
    assert_eq!(ids(&same_composed_but_read), vec!["ep-agent-store"]);
    assert_eq!(
        ids(&store
            .episodes(&EpisodeQuery {
                offset: 1,
                limit: 1,
                ..EpisodeQuery::default()
            })
            .unwrap()),
        vec!["ep-agent-store"],
        "and paging is the middle row of the same ordering"
    );
    assert_eq!(
        same_composed_but_read[0].members,
        vec![oid(1)],
        "members come back with the row, in order"
    );
}

#[test]
fn a_rewrite_that_keeps_the_digest_keeps_the_review_state_by_doing_nothing() {
    // A plain rebase: same author, same subject, same paths, so the same id. The new row is written under
    // the same primary key, which is why the review row — keyed by episode id — is still attached when the
    // run finishes. This test exists because that is the common case, and the design must be *shown* to
    // handle the common case rather than only the clever one.
    let mut store = seeded(&[
        commit(1, None, "pi-agent", 0, "feat: epoch", &["src/a.rs"]),
        commit(11, None, "pi-agent", 0, "feat: epoch", &["src/a.rs"]),
    ]);
    store
        .write_episodes(T0, &[episode("ep-x", "same", "fb", &[1])])
        .unwrap();
    store
        .set_review("ep-x", ReviewState::Flagged, "daniel", Some("page 2"), T0)
        .unwrap();
    store
        .write_episodes(T0 + 1_000, &[episode("ep-x", "same", "fb", &[11])])
        .unwrap();

    assert_eq!(store.review_of("ep-x").unwrap(), ReviewState::Flagged);
    let live = store.episodes(&EpisodeQuery::default()).unwrap();
    assert_eq!(
        ids(&live),
        vec!["ep-x"],
        "one episode, not a ghost and its replacement"
    );
    assert_eq!(
        live[0].members,
        vec![oid(11)],
        "and it points at the commits that exist now"
    );
    assert!(
        store.episode_rewrites().unwrap().is_empty(),
        "nothing was relinked, so the ledger says nothing"
    );
}

#[test]
fn a_reword_moves_the_id_and_the_fallback_digest_carries_the_decision() {
    // The case identity cannot solve: the subject changed, so the strong digest — and so the id — moved.
    // The fallback digest (author + subject only) is what the review state rides across on, and the
    // rewrite is written down so "did my sign-off survive?" is a query.
    let mut store = seeded(&[
        commit(
            1,
            None,
            "pi-agent",
            0,
            "feat: the old subject",
            &["src/a.rs"],
        ),
        commit(
            11,
            None,
            "pi-agent",
            0,
            "feat: the new subject",
            &["src/a.rs"],
        ),
    ]);
    store
        .write_episodes(T0, &[episode("ep-old", "f-strong-old", "fb-shared", &[1])])
        .unwrap();
    store
        .set_review(
            "ep-old",
            ReviewState::Accepted,
            "daniel",
            Some("ship it"),
            T0,
        )
        .unwrap();

    let written = store
        .write_episodes(
            T0 + 1_000,
            &[episode("ep-new", "f-strong-new", "fb-shared", &[11])],
        )
        .unwrap();
    assert_eq!(written.relinked, 1);
    assert_eq!(store.review_of("ep-new").unwrap(), ReviewState::Accepted);

    let ledger = store.episode_rewrites().unwrap();
    assert_eq!(
        ledger
            .first()
            .map(|entry| (entry.0.as_str(), entry.1.as_str(), entry.3.as_str())),
        Some(("ep-old", "ep-new", "fingerprint")),
        "the ledger names both ends and the evidence that joined them"
    );
    assert_eq!(
        store.episode("ep-old").unwrap().map(|row| row.live),
        Some(false),
        "the dead episode is kept, because somebody made a decision about it"
    );
    assert_eq!(store.review_of("ep-old").unwrap(), ReviewState::Accepted);
}

#[test]
fn a_reword_moves_the_subject_and_the_shape_digest_carries_the_decision() {
    // The reword: author and files unchanged, subject changed. The fallback digest (author + subject)
    // cannot see this one at all, which is why there is a third digest over author and paths and a third
    // relink tier to use it.
    let mut store = seeded(&[
        commit(1, None, "pi-agent", 0, "feat: first subject", &["src/a.rs"]),
        commit(
            11,
            None,
            "pi-agent",
            0,
            "feat: second subject",
            &["src/a.rs"],
        ),
    ]);
    let mut before = episode("ep-before", "f-before", "fb-before", &[1]);
    before.shape_fingerprint = "fb-shared".to_owned();
    store.write_episodes(T0, &[before]).unwrap();
    store
        .set_review("ep-before", ReviewState::Read, "daniel", None, T0)
        .unwrap();

    let mut after = episode("ep-after", "f-after", "fb-after", &[11]);
    after.shape_fingerprint = "fb-shared".to_owned();
    let written = store.write_episodes(T0 + 1_000, &[after]).unwrap();

    assert_eq!(
        written.relinked, 1,
        "the subject-only digest did not match, so the shape tier did"
    );
    assert_eq!(store.review_of("ep-after").unwrap(), ReviewState::Read);
    assert_eq!(
        store.episode_rewrites().unwrap().first().map(|entry| (
            entry.2.clone(),
            entry.3.clone(),
            entry.0.clone(),
            entry.1.clone()
        )),
        Some((
            "rebase".to_owned(),
            "shape".to_owned(),
            "ep-before".to_owned(),
            "ep-after".to_owned()
        )),
        "recorded as a rebase carried by the shape digest, which is the weaker claim and says so"
    );
}

#[test]
fn a_squash_relinks_on_a_shared_commit_and_a_prune_removes_what_nobody_reviewed() {
    let mut store = seeded(&[
        commit(1, None, "pi-agent", 0, "feat: part one", &["src/a.rs"]),
        commit(2, Some(1), "pi-agent", 60, "feat: part two", &["src/b.rs"]),
        commit(
            12,
            Some(1),
            "pi-agent",
            60,
            "feat: both parts",
            &["src/a.rs", "src/b.rs"],
        ),
    ]);
    // Two episodes, one reviewed and one nobody looked at.
    store
        .write_episodes(
            T0,
            &[
                episode("ep-read", "f-read", "fb-read", &[2]),
                episode("ep-blind", "f-blind", "fb-blind", &[1]),
            ],
        )
        .unwrap();
    store
        .set_review("ep-read", ReviewState::Dismissed, "daniel", None, T0)
        .unwrap();

    // The squash: one episode covering both commits, whose id matches neither of the old digests. Its
    // member list overlaps the reviewed episode's, which is the weaker of the two relink evidences.
    let written = store
        .write_episodes(
            T0 + 1_000,
            &[episode("ep-squashed", "f-squash", "fb-squash", &[1, 2])],
        )
        .unwrap();
    assert_eq!(written.relinked, 1, "the reviewed one was carried forward");
    assert_eq!(
        written.retired, 1,
        "the unreviewed one was deleted, not ghosted"
    );
    assert_eq!(
        store.review_of("ep-squashed").unwrap(),
        ReviewState::Dismissed
    );
    assert!(
        store.episode("ep-blind").unwrap().is_none(),
        "pruned entirely"
    );
    assert_eq!(
        store
            .episode_rewrites()
            .unwrap()
            .first()
            .map(|entry| (entry.2.as_str(), entry.3.as_str())),
        Some(("squash", "member-set")),
        "and the weaker evidence is named as the weaker evidence"
    );
}

#[test]
fn episode_inputs_follow_the_refs_and_drop_what_a_rebase_left_behind() {
    // The store is append-only, so the rebased-away commit is still a row. Segmentation must not see it,
    // or a force-push shows the reviewer the same change twice.
    let store = seeded(&[
        commit(1, None, "pi-agent", 0, "feat: kept", &["src/a.rs"]),
        commit(
            2,
            Some(1),
            "pi-agent",
            60,
            "feat: thrown away",
            &["src/b.rs"],
        ),
        commit(
            3,
            Some(1),
            "pi-agent",
            90,
            "feat: the replacement",
            &["src/c.rs"],
        ),
    ]);
    let inputs = store.episode_inputs().unwrap();
    let oids: Vec<&str> = inputs.iter().map(|input| input.oid.as_str()).collect();
    assert_eq!(oids, vec![oid(1), oid(3)], "the tip is 3; 2 is unreachable");
    assert_eq!(
        inputs[1].paths,
        vec!["src/c.rs".to_owned()],
        "paths come along, because the boundary rules and the identity digest both need them"
    );
    assert_eq!(
        store
            .ref_tip_oids()
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        vec![oid(3)],
        "HEAD and the branch name the same commit, and the set is of oids, not of refs"
    );
}

#[test]
fn episode_inputs_are_topologically_ordered_when_the_clock_lied() {
    // A commit dated before its own parent: git allows it (committer dates are what git orders by), and a
    // segmentation that walked in date order would evaluate a boundary against the wrong neighbour.
    let early_child = commit(2, Some(1), "pi-agent", 10, "feat: child", &["src/b.rs"]);
    let late_parent = commit(1, None, "pi-agent", 500, "feat: parent", &["src/a.rs"]);
    let store = seeded(&[late_parent, early_child]);
    let inputs = store.episode_inputs().unwrap();
    assert_eq!(
        inputs.iter().map(|i| i.oid.as_str()).collect::<Vec<_>>(),
        vec![oid(1), oid(2)],
        "parents before children, whatever the author dates say"
    );
}

#[test]
fn review_state_defaults_to_unread_and_the_counts_cover_the_unrowed() {
    let mut store = seeded(&[commit(1, None, "pi-agent", 0, "feat: one", &["src/a.rs"])]);
    store
        .write_episodes(T0, &[episode("ep-a", "f1", "fb1", &[1])])
        .unwrap();
    assert_eq!(store.review_of("ep-a").unwrap(), ReviewState::Unread);
    assert!(
        store.review_states().unwrap().is_empty(),
        "no row is not the same as a row saying unread, and the map only reports rows"
    );
    assert_eq!(
        store.review_counts().unwrap().get("unread"),
        Some(&1),
        "but the counts, which are what the status line reads, include the unrowed episode"
    );
}

#[test]
fn session_state_round_trips_and_a_none_deletes_it() {
    let mut store = seeded(&[commit(1, None, "pi-agent", 0, "feat: one", &["src/a.rs"])]);
    store
        .set_ui_state("tui.pane_sizes", Some("62,38"), T0)
        .unwrap();
    store
        .set_ui_state("queue.filter", Some("unread"), T0)
        .unwrap();
    assert_eq!(
        store.ui_state("tui.pane_sizes").unwrap().as_deref(),
        Some("62,38")
    );
    assert_eq!(
        store.ui_state_all("tui.").unwrap(),
        vec![("tui.pane_sizes".to_owned(), "62,38".to_owned())]
    );
    store.set_ui_state("tui.pane_sizes", None, T0).unwrap();
    assert_eq!(store.ui_state("tui.pane_sizes").unwrap(), None);
    assert_eq!(
        store.ui_state_all("").unwrap().len(),
        1,
        "only the filter is left"
    );
}

#[test]
fn a_session_state_write_lands_only_on_the_value_the_writer_read() {
    // `e1.11`'s concurrent-session rule, at the one layer that can enforce it: a `session_state` write
    // states the value it believes is there, and a row that moved in between is not touched.
    let mut store = seeded(&[commit(1, None, "pi-agent", 0, "feat: one", &["src/a.rs"])]);
    assert!(
        store
            .set_ui_state_if("timeline.filter", None, Some("state=unread"), T0)
            .unwrap(),
        "a row that was absent and is still absent is ours to write"
    );
    assert!(
        !store
            .set_ui_state_if("timeline.filter", None, Some("group=day"), T0)
            .unwrap(),
        "a second writer that read no row must not create one over the first one's"
    );
    assert_eq!(
        store.ui_state("timeline.filter").unwrap().as_deref(),
        Some("state=unread")
    );
    assert!(
        store
            .set_ui_state_if(
                "timeline.filter",
                Some("state=unread"),
                Some("state=unread;group=session"),
                T0
            )
            .unwrap(),
        "the writer that read the current value may replace it"
    );
    assert!(
        !store
            .set_ui_state_if(
                "timeline.filter",
                Some("state=unread"),
                Some("author=pi"),
                T0
            )
            .unwrap(),
        "and a writer still holding the old read is refused rather than last-writer-wins"
    );
    assert_eq!(
        store.ui_state("timeline.filter").unwrap().as_deref(),
        Some("state=unread;group=session"),
        "the loser wrote nothing"
    );
    assert!(
        !store
            .set_ui_state_if("timeline.filter", Some("state=unread"), None, T0)
            .unwrap(),
        "a delete only deletes what it read"
    );
    assert!(
        store
            .set_ui_state_if(
                "timeline.filter",
                Some("state=unread;group=session"),
                None,
                T0
            )
            .unwrap(),
        "and deletes it when nothing moved"
    );
    assert_eq!(store.ui_state("timeline.filter").unwrap(), None);
    store
        .set_ui_state("tui.view", Some("timeline"), T0 + 5_000)
        .unwrap();
    assert_eq!(
        store.ui_state_rows("tui.").unwrap(),
        vec![("tui.view".to_owned(), "timeline".to_owned(), T0 + 5_000)],
        "the stamp is what lets a resumed session say when a row was written"
    );
}

/// The ids of a page of episodes, for assertions that read as a list of names.
fn ids(rows: &[EpisodeRow]) -> Vec<String> {
    rows.iter().map(|row| row.id.clone()).collect()
}

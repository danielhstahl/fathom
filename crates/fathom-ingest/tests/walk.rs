//! The commit walk: order, pruning, grafts, and what a history that is not whole reports (e1.1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_core::error::Diagnosable;
use fathom_core::error::ErrorCode;
use fathom_core::time::Timestamp;
use fathom_ingest::error::{Error, TruncationCause, Unavailable};
use fathom_ingest::graph::{Detail, MergeFilter, Notice, Truncation, WalkOptions, WalkOrder};
use fathom_ingest::oid::Oid;
use fathom_ingest::repo::Repository;
use std::collections::HashSet;
use std::path::PathBuf;

fn open(path: &std::path::Path) -> Repository {
    Repository::discover(path).expect("discover")
}

fn hexes(entries: &[fathom_ingest::graph::LogEntry]) -> Vec<String> {
    entries.iter().map(|entry| entry.oid.hex()).collect()
}

/// A fixture with a merge in it:
///
/// ```text
/// main:   A ─── B ──────── D   (merge: first parent B, second parent C)
///         └─ C ──┘
/// ```
struct MergeFixture {
    // Kept alive so the temporary directory outlives the assertions.
    _fixture: Fixture,
    root: PathBuf,
    a: String,
    b: String,
    c: String,
    d: String,
}

fn merge_fixture() -> MergeFixture {
    let fixture = Fixture::new("merge");
    let root = fixture.init();
    let a = fixture.commit(&root, "A: initial", &[("one.txt", "one\n")]);
    let b = fixture.commit(&root, "B: main work", &[("two.txt", "two\n")]);
    git(&root, &["checkout", "-q", "-b", "side", &a]);
    let c = fixture.commit(&root, "C: side work", &[("three.txt", "three\n")]);
    git(&root, &["checkout", "-q", "main"]);
    let d = fixture.merge(&root, &c, "D: merge side");
    MergeFixture {
        _fixture: fixture,
        root,
        a,
        b,
        c,
        d,
    }
}

fn linear_fixture(count: usize) -> (Fixture, PathBuf) {
    let fixture = Fixture::new("linear");
    let root = fixture.init();
    for index in 0..count {
        fixture.commit(
            &root,
            &format!("commit {index}"),
            &[("file.txt", &format!("content {index}\n"))],
        );
    }
    (fixture, root)
}

#[test]
fn a_linear_walk_matches_git_exactly() {
    let (_fixture, root) = linear_fixture(12);
    let repo = open(&root);
    let outcome = repo.log(WalkOptions::head()).finish().expect("walk");
    assert_eq!(
        hexes(&outcome.entries),
        git_rev_list_first_parent(&root),
        "a linear history has one order, and it matches `git rev-list`"
    );
    assert_eq!(outcome.len(), 12);
    assert!(!outcome.truncated());
    assert!(outcome.notices.is_empty());
}

#[test]
fn a_first_parent_walk_matches_git_first_parent() {
    let fixture = merge_fixture();
    let repo = open(&fixture.root);
    let outcome = repo
        .log(WalkOptions::head().first_parent())
        .finish()
        .expect("walk");
    assert_eq!(
        hexes(&outcome.entries),
        git_rev_list_first_parent(&fixture.root),
        "the mainline is D, B, A — C is on the other parent"
    );
    assert_eq!(
        hexes(&outcome.entries),
        vec![fixture.d.clone(), fixture.b.clone(), fixture.a.clone()]
    );
}

#[test]
fn a_full_walk_sees_every_commit_exactly_once() {
    let fixture = merge_fixture();
    let repo = open(&fixture.root);
    let outcome = repo.log(WalkOptions::head()).finish().expect("walk");
    let ours: HashSet<String> = hexes(&outcome.entries).into_iter().collect();
    let theirs: HashSet<String> = git_rev_list_all(&fixture.root).into_iter().collect();
    assert_eq!(ours, theirs, "A, B, C and D, once each");
    assert_eq!(outcome.len(), 4);
    // Ancestry-first emits the merge, then the mainline, then the side branch: the property the graph
    // lane relies on is that the mainline arrives before the side history.
    let order = hexes(&outcome.entries);
    assert_eq!(order[0], fixture.d);
    let position_of_b = order.iter().position(|oid| *oid == fixture.b).expect("B");
    let position_of_c = order.iter().position(|oid| *oid == fixture.c).expect("C");
    assert!(
        position_of_b < position_of_c,
        "first parent before the second: {order:?}"
    );
    // Depth is distance from the start, which is what a graph lane draws.
    let depth_of = |oid: &str| -> Option<usize> {
        outcome
            .entries
            .iter()
            .find(|entry| entry.oid.hex() == oid)
            .map(|entry| entry.depth)
    };
    assert_eq!(depth_of(&fixture.d), Some(0));
    assert_eq!(depth_of(&fixture.b), Some(1));
    assert_eq!(depth_of(&fixture.c), Some(1));
    assert_eq!(
        depth_of(&fixture.a),
        Some(2),
        "A is two hops up the mainline"
    );
}

#[test]
fn hiding_a_commit_prunes_it_and_what_only_it_reaches() {
    let fixture = merge_fixture();
    let repo = open(&fixture.root);
    let tip: Oid = fixture.d.parse().expect("oid");
    let outcome = repo
        .log(WalkOptions::head().hiding([tip]))
        .finish()
        .expect("hiding the tip hides everything");
    assert!(outcome.is_empty(), "{outcome:?}");

    let c: Oid = fixture.c.parse().expect("oid");
    let a: Oid = fixture.a.parse().expect("oid");
    let partial = repo
        .log(WalkOptions::head().starting_at([c]).hiding([a]))
        .finish()
        .expect("walk");
    assert_eq!(hexes(&partial.entries), vec![fixture.c.clone()]);
    assert_eq!(
        git(
            &fixture.root,
            &["rev-list", &fixture.c, &format!("^{}", fixture.a)]
        ),
        fixture.c,
        "git's own `A ^B` says the same thing"
    );
}

#[test]
fn date_orders_are_ordered_and_agree_on_the_endpoints() {
    let fixture = Fixture::new("dates");
    let root = fixture.init();
    // A merge whose parents and grandparent are dated so that the frontier holds a real choice:
    // date order is "no parent before its children, newest among what is available", which is exactly
    // what `git log --date-order` promises — and which a linear history cannot distinguish from the
    // creation order.
    fixture.commit_at(
        &root,
        "A: root",
        &[("one.txt", "a\n")],
        "2024-05-01T00:00:00+00:00",
    );
    git(&root, &["checkout", "-q", "-b", "side"]);
    fixture.commit_at(
        &root,
        "C: side",
        &[("two.txt", "c\n")],
        "2024-06-01T00:00:00+00:00",
    );
    git(&root, &["checkout", "-q", "main"]);
    fixture.commit_at(
        &root,
        "B: main",
        &[("three.txt", "b\n")],
        "2024-06-03T00:00:00+00:00",
    );
    // The merge commit itself gets the fixture's pinned date, which is later than every commit above,
    // so the tip is also the newest commit and the order has a single right answer.
    git(
        &root,
        &["merge", "-q", "--no-ff", "-m", "D: merge side", "side"],
    );
    let repo = open(&root);

    let mut newest = WalkOptions::head();
    newest.order = WalkOrder::DateNewestFirst;
    let newest = repo.log(newest).finish().expect("walk");
    let times: Vec<Timestamp> = newest
        .entries
        .iter()
        .map(|entry| entry.detail.as_ref().expect("header").when())
        .collect();
    assert!(
        times.windows(2).all(|pair| pair[0] >= pair[1]),
        "newest first: {times:?}"
    );
    assert_eq!(times.len(), 4, "A, B, C and the merge");

    let mut oldest = WalkOptions::head();
    oldest.order = WalkOrder::DateOldestFirst;
    let oldest = repo.log(oldest).finish().expect("walk");
    let times: Vec<Timestamp> = oldest
        .entries
        .iter()
        .map(|entry| entry.detail.as_ref().expect("header").when())
        .collect();
    assert!(
        times.windows(2).all(|pair| pair[0] <= pair[1]),
        "oldest first: {times:?}"
    );
    assert_eq!(
        oldest.entries.first().expect("one").oid.hex(),
        git_rev_parse(&root, "main~2"),
        "the oldest commit is the root"
    );
    assert_eq!(
        oldest.entries.last().expect("one").oid.hex(),
        git_head(&root),
        "and the newest is HEAD"
    );
    assert_eq!(
        hexes(&newest.entries),
        git_rev_list_all(&root),
        "date order is `git log --date-order`'s order"
    );
    assert_eq!(
        hexes(&oldest.entries),
        hexes(&newest.entries).into_iter().rev().collect::<Vec<_>>(),
        "the two orders are the same set in opposite directions"
    );
}

#[test]
fn limit_and_skip_page_without_overlapping() {
    let (_fixture, root) = linear_fixture(10);
    let repo = open(&root);
    let page = repo
        .log(WalkOptions::head().limit(3))
        .finish()
        .expect("walk");
    assert_eq!(page.len(), 3);
    assert!(
        page.notices.contains(&Notice::LimitReached { emitted: 3 }),
        "a list that stopped early says it did: {:?}",
        page.notices
    );

    let mut second = WalkOptions::head().limit(3);
    second.skip = 3;
    let second_page = repo.log(second).finish().expect("walk");
    assert_eq!(second_page.len(), 3);
    let ids: Vec<String> = [hexes(&page.entries), hexes(&second_page.entries)].concat();
    let unique: HashSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), 6, "the two pages do not overlap: {ids:?}");
    assert_eq!(
        hexes(&second_page.entries).first().expect("one"),
        &git_rev_list_first_parent(&root)[3],
        "skip is `git log --skip`: pagination, not filtering"
    );
}

#[test]
fn since_and_until_filter_on_the_committer_date() {
    let fixture = Fixture::new("window");
    let root = fixture.init();
    // 2024-01-01, 2024-02-01, 2024-03-01 (seconds since the epoch, computed by hand so the test does
    // not need a date library).
    let stamps = [1_704_067_200_i64, 1_706_745_600, 1_709_251_200];
    let dates = [
        "2024-01-01T00:00:00+00:00",
        "2024-02-01T00:00:00+00:00",
        "2024-03-01T00:00:00+00:00",
    ];
    for (index, date) in dates.iter().enumerate() {
        fixture.commit_at(
            &root,
            &format!("commit {index}"),
            &[("file.txt", &format!("{index}\n"))],
            date,
        );
    }
    let repo = open(&root);
    let mut since = WalkOptions::head();
    since.since = Some(fathom_ingest::revspec::timestamp_from_unix_seconds(
        stamps[1],
    ));
    let since = repo.log(since).finish().expect("walk");
    assert_eq!(since.len(), 2, "the February and March commits");
    assert_eq!(
        since.entries.first().expect("newest").oid.hex(),
        git_head(&root)
    );

    let mut window = WalkOptions::head();
    window.since = Some(fathom_ingest::revspec::timestamp_from_unix_seconds(
        stamps[1],
    ));
    window.until = Some(fathom_ingest::revspec::timestamp_from_unix_seconds(
        stamps[2] - 1,
    ));
    let window = repo.log(window).finish().expect("walk");
    assert_eq!(window.len(), 1, "only February");

    // The filtering shows what is *shown*; the walk still crossed the excluded commits to get there.
    let mut merges = WalkOptions::head();
    merges.merges = MergeFilter::NoMerges;
    let merges = repo.log(merges).finish().expect("walk");
    assert_eq!(merges.len(), 3, "a linear history has no merges to hide");
}

#[test]
fn merges_are_filtered_the_way_git_filters_them() {
    let fixture = merge_fixture();
    let repo = open(&fixture.root);

    let mut only = WalkOptions::head();
    only.merges = MergeFilter::OnlyMerges;
    let only = repo.log(only).finish().expect("walk");
    assert_eq!(hexes(&only.entries), vec![fixture.d.clone()]);
    assert_eq!(hexes(&only.entries), git_merge_commits(&fixture.root));

    let mut none = WalkOptions::head();
    none.merges = MergeFilter::NoMerges;
    let none = repo.log(none).finish().expect("walk");
    let ours: HashSet<String> = hexes(&none.entries).into_iter().collect();
    let theirs: HashSet<String> = git_rev_list_all(&fixture.root)
        .into_iter()
        .filter(|oid| oid != &fixture.d)
        .collect();
    assert_eq!(ours, theirs);
}

#[test]
fn a_header_carries_what_a_timeline_row_needs() {
    let fixture = Fixture::new("header");
    let root = fixture.init();
    fixture.commit(&root, "setup", &[("README.md", "before\n")]);
    fixture.commit_with_trailer(
        &root,
        "Refactor the ingest door",
        "Co-authored-by: Pair <pair@fathom.invalid>",
        &[("one.txt", "one\n")],
    );
    let repo = open(&root);
    let outcome = repo.log(WalkOptions::head()).finish().expect("walk");
    let entry = outcome.entries.first().expect("one entry");
    let header = entry.detail.as_ref().expect("headers by default");
    assert_eq!(header.subject, "Refactor the ingest door");
    assert_eq!(header.parents.len(), 1);
    assert!(!header.is_merge());
    assert_eq!(header.author.name, "Fathom Fixture");
    assert_eq!(header.author.email, "fixture@fathom.invalid");
    assert_eq!(header.author.offset_minutes, 0);
    assert!(header.message.starts_with(b"Refactor"));
    assert_eq!(
        header.trailer("co-authored-by").as_deref(),
        Some("Pair <pair@fathom.invalid>"),
        "trailer lookup is case-insensitive and reads the last paragraph"
    );
    assert_eq!(header.trailer("signed-off-by"), None);
    assert_eq!(
        header.tree.hex().len(),
        40,
        "the tree id needs no second read"
    );
    assert_eq!(header.oid, entry.oid);
}

#[test]
fn a_subject_that_looks_like_a_trailer_answers_no_trailer() {
    // The pair to `a_header_carries_what_a_timeline_row_needs` above (`reco-2ss`): a conventional-commit
    // subject is shaped exactly like `Key: value`, so a lookup that reads the *first* paragraph as a
    // trailer block answered every key a repository's commit style happens to use.
    let fixture = Fixture::new("subject-trailer");
    let root = fixture.init();
    fixture.commit(&root, "setup", &[("README.md", "before\n")]);
    fixture.commit(
        &root,
        "fix: the parser read the subject as a trailer",
        &[("one.txt", "one\n")],
    );
    let repo = open(&root);
    let outcome = repo.log(WalkOptions::head()).finish().expect("walk");
    let header = outcome
        .entries
        .first()
        .expect("one entry")
        .detail
        .as_ref()
        .expect("headers by default");
    assert_eq!(
        header.subject,
        "fix: the parser read the subject as a trailer"
    );
    assert_eq!(
        header.trailer("fix"),
        None,
        "a one-paragraph message is a subject, and has no trailer block at all"
    );
    assert_eq!(header.trailer("co-authored-by"), None);
}

#[test]
fn ids_only_reads_no_headers_and_changes_nothing_else() {
    let fixture = merge_fixture();
    let repo = open(&fixture.root);
    let full = repo.log(WalkOptions::head()).finish().expect("walk");
    let cheap = repo
        .log(WalkOptions::head().ids_only())
        .finish()
        .expect("walk");
    assert_eq!(hexes(&full.entries), hexes(&cheap.entries));
    assert!(cheap.entries.iter().all(|entry| entry.detail.is_none()));
    assert!(full.entries.iter().all(|entry| entry.detail.is_some()));
    assert_eq!(Detail::Ids, Detail::Ids);
}

#[test]
fn a_shallow_clone_walks_what_it_has_and_says_where_history_stops() {
    let fixture = Fixture::new("shallow-walk");
    let source = fixture.init();
    for index in 0..6 {
        fixture.commit(
            &source,
            &format!("commit {index}"),
            &[("file.txt", &format!("{index}\n"))],
        );
    }
    let clone = fixture.clone_shallow(&source, "clone", 2);
    let repo = open(&clone);
    let boundary: Vec<Oid> = repo
        .shallow_boundary()
        .expect("shallow file")
        .unwrap_or_default();
    assert_eq!(boundary.len(), 1, "one graft for a depth-2 clone");

    let outcome = repo
        .log(WalkOptions::head())
        .finish()
        .expect("degraded walk");
    assert_eq!(
        outcome.len(),
        2,
        "a depth-2 clone has two commits, and shows both"
    );
    assert_eq!(
        outcome.notices,
        vec![Notice::ShallowBoundary { at: boundary[0] }],
        "the walk names the graft it stopped at"
    );
    assert_eq!(
        outcome.truncation,
        Some(Truncation::Shallow {
            boundary: boundary.clone()
        }),
        "a graft is reported as a graft, not as damage"
    );
    assert!(outcome.truncated());
    assert!(
        outcome
            .truncation
            .as_ref()
            .expect("truncated")
            .hint()
            .contains("never fetches"),
        "the hint says who can fix it: not fathom"
    );

    // `strict` is the same walk with the opposite policy: the partial answer becomes an error that names
    // the boundary, because a caller that presents a history must not present half of one.
    let error = repo
        .log(WalkOptions::head().strict())
        .finish()
        .expect_err("a shallow history is not a whole one");
    match error {
        Error::HistoryTruncated {
            boundary: named,
            cause,
        } => {
            assert_eq!(named, boundary);
            assert_eq!(cause, TruncationCause::Shallow { boundary });
            assert!(cause.hint().contains("--unshallow"));
        }
        other => panic!("expected a truncation error, got {other:?}"),
    }
    assert_eq!(
        fathom_ingest::error::Error::HistoryTruncated {
            boundary: vec![],
            cause: TruncationCause::Shallow { boundary: vec![] },
        }
        .code(),
        ErrorCode::HistoryTruncated
    );
}

#[test]
fn a_missing_commit_is_a_notice_by_default_and_the_error_itself_when_strict() {
    let fixture = Fixture::new("missing-walk");
    let root = fixture.init();
    let base = fixture.commit(&root, "base", &[("one.txt", "one\n")]);
    fixture.commit(&root, "tip", &[("two.txt", "two\n")]);
    let parent_oid: Oid = base.parse().expect("oid");
    std::fs::remove_file(root.join(".git/objects").join(&base[..2]).join(&base[2..]))
        .expect("delete the root commit object");
    let repo = open(&root);
    assert!(!repo.has_object(parent_oid));

    let outcome = repo.log(WalkOptions::head()).finish().expect("walk");
    assert_eq!(outcome.len(), 1, "the tip is still shown");
    assert_eq!(
        outcome.notices,
        vec![Notice::ObjectUnavailable {
            oid: parent_oid,
            why: Unavailable::Missing
        }],
        "the skipped commit is named, with the explanation the repository can give"
    );
    match outcome.truncation {
        Some(Truncation::Unreadable {
            skipped,
            examples,
            filter,
        }) => {
            assert_eq!(skipped, 1);
            assert_eq!(examples, vec![parent_oid]);
            assert_eq!(filter, None, "nothing in the config explains this one");
        }
        other => panic!("expected an unreadable truncation, got {other:?}"),
    }

    let error = repo
        .log(WalkOptions::head().strict())
        .finish()
        .expect_err("strict refuses a partial history");
    assert_eq!(error.code(), ErrorCode::ObjectUnavailable);
    assert!(
        error.hint().is_some_and(|hint| hint.contains("git fsck")),
        "damage points at the tool that diagnoses damage: {:?}",
        error.hint()
    );
}

#[test]
fn an_unborn_head_walks_nothing_and_says_why() {
    let fixture = Fixture::new("unborn-walk");
    let root = fixture.init();
    let repo = open(&root);
    let outcome = repo.log(WalkOptions::head()).finish().expect("walk");
    assert!(outcome.is_empty());
    assert_eq!(
        outcome.notices,
        vec![Notice::UnbornHead {
            branch: "main".into()
        }]
    );
}

#[test]
fn a_start_point_that_is_not_a_commit_is_a_notice_not_a_panic() {
    let fixture = Fixture::new("not-a-commit");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "contents\n")]);
    let repo = open(&root);
    let head = repo.head().expect("head").oid().expect("born");
    let header = repo.commit(head).expect("commit").expect("commit");
    let outcome = repo
        .log(WalkOptions::head().starting_at([header.tree]))
        .finish()
        .expect("a tree is not a commit");
    assert!(outcome.is_empty());
    assert_eq!(
        outcome.notices,
        vec![Notice::NotACommit { oid: header.tree }]
    );
    let blob = repo
        .tree_entries(header.tree)
        .expect("entries")
        .remove(0)
        .oid;
    let outcome = repo
        .log(WalkOptions::head().starting_at([blob]))
        .finish()
        .expect("a blob is not a commit either");
    assert_eq!(outcome.notices, vec![Notice::NotACommit { oid: blob }]);
}

#[test]
fn a_start_point_that_is_absent_is_named_per_start() {
    let fixture = Fixture::new("unknown-start");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    let repo = open(&root);
    let absent: Oid = "0123456789abcdef0123456789abcdef01234567"
        .parse()
        .expect("oid");
    let head = repo.head().expect("head").oid().expect("born");
    let outcome = repo
        .log(WalkOptions::head().starting_at([absent, head]))
        .finish()
        .expect("an unreadable start is not a crash");
    assert_eq!(outcome.len(), 1, "the start that exists is still walked");
    assert_eq!(
        outcome.notices,
        vec![Notice::ObjectUnavailable {
            oid: absent,
            why: Unavailable::Missing
        }]
    );
}

#[test]
fn the_walk_is_lazy_so_a_page_reads_a_page() {
    let (_fixture, root) = linear_fixture(50);
    let repo = open(&root);
    let mut log = repo.log(WalkOptions::head());
    let first_two: Vec<String> = (&mut log)
        .take(2)
        .map(|entry| entry.expect("ok").oid.hex())
        .collect();
    assert_eq!(first_two.len(), 2);
    // Pulling two items off a 50-commit walk must not have produced the other 48.
    assert!(log.emitted() <= 3, "emitted {} items", log.emitted());
}

#[test]
fn an_annotated_tag_start_peels_through_the_revspec_door() {
    let fixture = Fixture::new("tag-walk");
    let root = fixture.init();
    let commit = fixture.commit(&root, "release me", &[("one.txt", "one\n")]);
    fixture.tag(&root, "v1.0");
    let repo = open(&root);
    let tag_target = repo.resolve("v1.0").expect("the tag peels to a commit");
    assert_eq!(
        tag_target.hex(),
        commit,
        "`v1.0` names the commit, not the tag object"
    );
    let outcome = repo
        .log(WalkOptions::head().starting_at([tag_target]))
        .finish()
        .expect("walk");
    assert_eq!(outcome.len(), 1);
}

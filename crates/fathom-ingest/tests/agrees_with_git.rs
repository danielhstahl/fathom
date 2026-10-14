//! The claim that matters most in this ticket: where the git binary is available, fathom's answers are
//! *git's* answers, not merely self-consistent ones (e1.1).
//!
//! Each test builds one history, asks `git rev-list` for a list, asks fathom for the equivalent, and
//! compares the two id-for-id and in order. Where the comparison is deliberately looser — it is, twice —
//! the reason is written next to the assertion, because "we match git except here" is only useful if the
//! "except here" is written down.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_ingest::error::Unavailable;
use fathom_ingest::graph::{CommitLog, MergeFilter, Notice, WalkOptions, WalkOrder};
use fathom_ingest::oid::Oid;
use fathom_ingest::repo::Repository;
use fathom_ingest::revspec::timestamp_from_unix_seconds;
use std::path::Path;
use std::str::FromStr;

fn open(path: &Path) -> Repository {
    Repository::discover(path).expect("discover")
}

fn ids(log: CommitLog<'_>) -> Vec<String> {
    log.map(|entry| entry.expect("walk").oid.hex()).collect()
}

fn parsed(output: &str) -> Vec<String> {
    output.lines().map(str::to_owned).collect()
}

/// A history with real shape: two branches, a merge of a merge, a branch reachable only through a merge,
/// and an annotated tag on the tip.
///
/// Commit dates increase along ancestry, which is what makes git's date-ordered output exactly comparable
/// with fathom's. The last test in this file is about what happens when that is *not* true.
struct Dag {
    // Kept alive so the temporary directory outlives the assertions.
    _fixture: Fixture,
    root: std::path::PathBuf,
}

impl Dag {
    fn root(&self) -> &Path {
        &self.root
    }
}

fn dag() -> Dag {
    let fixture = Fixture::new("dag");
    let root = fixture.init();
    let mut step = 0_i64;
    let mut next = || {
        step += 10;
        date_at(step)
    };
    fixture.commit_at(&root, "gen 0", &[("seed.txt", "0\n")], &next());
    fixture.commit_at(&root, "main 1", &[("main-1.txt", "1\n")], &next());
    fixture.branch(&root, "feature");
    git(&root, &["checkout", "-q", "feature"]);
    fixture.commit_at(&root, "feature 1", &[("feature-1.txt", "1\n")], &next());
    fixture.commit_at(&root, "feature 2", &[("feature-2.txt", "2\n")], &next());
    git(&root, &["checkout", "-q", "main"]);
    fixture.commit_at(&root, "main 2", &[("main-2.txt", "2\n")], &next());
    git(&root, &["checkout", "-q", "-b", "sprint", "feature~1"]);
    fixture.commit_at(&root, "sprint 1", &[("sprint-1.txt", "1\n")], &next());
    git(&root, &["checkout", "-q", "feature"]);
    fixture.merge_at(&root, "sprint", "merge sprint", &next());
    git(&root, &["checkout", "-q", "main"]);
    fixture.merge_at(&root, "feature", "merge feature", &next());
    fixture.commit_at(&root, "main 3", &[("main-3.txt", "3\n")], &next());
    git(&root, &["tag", "-a", "v2.0", "-m", "second release"]);
    Dag {
        _fixture: fixture,
        root,
    }
}

#[test]
fn a_first_parent_walk_is_git_rev_list_first_parent() {
    let dag = dag();
    let repo = open(dag.root());
    let expected = git(dag.root(), &["rev-list", "--first-parent", "main"]);
    assert!(
        expected.lines().count() > 4,
        "the fixture must be non-trivial: {expected}"
    );
    assert_eq!(
        ids(repo.log(WalkOptions::head().first_parent())),
        parsed(&expected),
        "first-parent must match line for line"
    );
}

#[test]
fn a_full_walk_is_git_rev_list() {
    let dag = dag();
    let repo = open(dag.root());
    let expected = git(dag.root(), &["rev-list", "--date-order", "main"]);
    let ours = ids(repo.log(WalkOptions::head().order(WalkOrder::DateNewestFirst)));
    assert_eq!(
        ours,
        parsed(&expected),
        "git's `--date-order` is newest-commit-first, which is `WalkOrder::DateNewestFirst`"
    );
}

#[test]
fn the_default_walk_covers_exactly_what_git_topo_order_covers() {
    let dag = dag();
    let repo = open(dag.root());
    let expected = git(dag.root(), &["rev-list", "--topo-order", "main"]);
    let ours = ids(repo.log(WalkOptions::head()));
    // The set must match git's exactly: same reachable commits, nothing extra, nothing missed. The
    // *sequence* need not — see `the_default_order_walks_the_first_parent_chain_first` for what fathom's
    // default order promises instead, and why it is not git's `--topo-order`.
    let mut ours_sorted = ours.clone();
    ours_sorted.sort();
    let mut expected_sorted = parsed(&expected);
    expected_sorted.sort();
    assert_eq!(
        ours_sorted, expected_sorted,
        "the same commits, however ordered"
    );
    assert_eq!(
        ours.first().expect("non-empty"),
        expected.lines().next().expect("non-empty").trim(),
        "both start at the same commit"
    );
}

#[test]
fn the_default_order_walks_the_first_parent_chain_first() {
    let dag = dag();
    let repo = open(dag.root());
    let ours = ids(repo.log(WalkOptions::head()));
    // What the default order *does* promise: the mainline is emitted as one run, tip down to the root,
    // before any side branch — so `git log --first-parent`'s list is a prefix of it. That is the property
    // a timeline or a bisect UI wants, and it is what keeps the order flat in memory.
    let first_parent = ids(repo.log(WalkOptions::head().first_parent()));
    assert_eq!(
        ours[..first_parent.len()],
        first_parent[..],
        "the first-parent chain comes out first, in order"
    );
    // And the consequence, pinned rather than hidden: because the mainline is followed to the root before
    // a side branch is opened, a commit can be emitted before one of its own children that hangs off a
    // later branch. `feature 1`'s parent is on the mainline, so the mainline commit is emitted first.
    // The other answer needs the whole reachable set read first — git's `--topo-order` — and that is
    // deliberately not this order's job.
    let feature_one = git(dag.root(), &["rev-parse", "feature~2"]);
    let its_parent = git(dag.root(), &["rev-parse", "feature~2^1"]);
    assert!(
        position(&ours, &its_parent) < position(&ours, &feature_one),
        "documented behaviour: the mainline commit precedes the side-branch commit that has it as a parent"
    );
    // The order that *can* promise a parent after its children, in the ordinary case of commit dates that
    // increase along ancestry, is the date order — and that is what the two date tests compare with git.
    let by_date = ids(repo.log(WalkOptions::head().order(WalkOrder::DateNewestFirst)));
    assert!(
        position(&by_date, &feature_one) < position(&by_date, &its_parent),
        "date order emits the child first when dates are monotone"
    );
}

fn position(list: &[String], oid: &str) -> usize {
    list.iter()
        .position(|candidate| candidate == oid)
        .unwrap_or_else(|| panic!("{oid} is not in the walk"))
}

#[test]
fn a_date_ascending_walk_is_git_rev_list_reverse() {
    let dag = dag();
    let repo = open(dag.root());
    let expected = git(
        dag.root(),
        &["rev-list", "--reverse", "--date-order", "main"],
    );
    assert_eq!(
        ids(repo.log(WalkOptions::head().order(WalkOrder::DateOldestFirst))),
        parsed(&expected),
        "`git rev-list --reverse --date-order`"
    );
}

#[test]
fn counts_match_rev_list_count_through_every_filter() {
    let dag = dag();
    let repo = open(dag.root());
    let count = |args: &[&str]| {
        let mut full = vec!["rev-list", "--count"];
        full.extend_from_slice(args);
        full.push("main");
        git(dag.root(), &full).parse::<usize>().expect("a count")
    };
    assert_eq!(
        repo.log(WalkOptions::head().order(WalkOrder::DateNewestFirst))
            .count(),
        count(&[]),
        "the whole history"
    );
    assert_eq!(
        repo.log(WalkOptions::head().first_parent()).count(),
        count(&["--first-parent"]),
        "first-parent only"
    );
    assert_eq!(
        repo.log(
            WalkOptions::head()
                .first_parent()
                .merges(MergeFilter::NoMerges)
        )
        .count(),
        count(&["--first-parent", "--no-merges"]),
        "merge commits excluded"
    );
    assert_eq!(
        repo.log(WalkOptions::head().merges(MergeFilter::OnlyMerges))
            .count(),
        count(&["--merges"]),
        "only merges — the shape of a timeline of integrations"
    );
    assert!(count(&["--merges"]) >= 2, "the fixture has two merges");
}

#[test]
fn a_date_window_matches_git_since_and_until() {
    let dag = dag();
    let repo = open(dag.root());
    // The fixture spaces its commits ten seconds apart, so these are two real cuts: one mid-history and
    // one that keeps everything.
    for (since, until) in [
        (epoch_at(35), epoch_at(65)),
        (epoch_at(5), i64::MAX),
        (epoch_at(0), epoch_at(0)),
    ] {
        // `@<secs>` is git's own exact timestamp form: no calendar arithmetic, no ambiguity about what a
        // bare number means.
        let since_arg = format!("--since=@{since}");
        let until_arg = format!("--until=@{until}");
        let expected = git(
            dag.root(),
            &["rev-list", "--date-order", &since_arg, &until_arg, "main"],
        );
        let options = WalkOptions::head()
            .order(WalkOrder::DateNewestFirst)
            .since(timestamp_from_unix_seconds(since))
            .until(timestamp_from_unix_seconds(until));
        assert_eq!(
            ids(repo.log(options)),
            parsed(&expected),
            "`{since_arg} {until_arg}`"
        );
    }
}

#[test]
fn an_excluded_ancestor_matches_the_two_dot_form() {
    let dag = dag();
    let repo = open(dag.root());
    let base = git(dag.root(), &["rev-parse", "main~1"]);
    let expected = git(
        dag.root(),
        &["rev-list", "--first-parent", &format!("{base}..main")],
    );
    let ours = ids(repo.log(
        WalkOptions::head()
            .first_parent()
            .hiding([Oid::from_str(&base).expect("oid")]),
    ));
    assert_eq!(ours, parsed(&expected));
    assert_eq!(ours.len(), 1, "only the tip is newer than its own parent");
}

#[test]
fn limits_match_git_max_count_and_skip() {
    let dag = dag();
    let repo = open(dag.root());
    for (limit, skip) in [(1usize, 0usize), (3, 0), (3, 2), (100, 4), (0, 0)] {
        let limit_arg = format!("--max-count={limit}");
        let skip_arg = format!("--skip={skip}");
        let expected = git(
            dag.root(),
            &["rev-list", "--first-parent", &limit_arg, &skip_arg, "main"],
        );
        let ours = ids(repo.log(WalkOptions {
            skip,
            ..WalkOptions::head().first_parent().limit(limit)
        }));
        assert_eq!(ours, parsed(&expected), "limit {limit}, skip {skip}");
    }
}

#[test]
fn the_merge_structure_itself_matches_git() {
    let dag = dag();
    let repo = open(dag.root());
    let ours: Vec<(String, Vec<String>)> = repo
        .log(WalkOptions::head().order(WalkOrder::DateNewestFirst))
        .map(|entry| {
            let entry = entry.expect("walk");
            (
                entry.oid.hex(),
                entry.parents.iter().map(Oid::hex).collect(),
            )
        })
        .collect();
    let expected = git(dag.root(), &["rev-list", "--parents", "main"]);
    assert!(
        expected
            .lines()
            .any(|line| line.split_whitespace().count() == 3),
        "the fixture must contain a merge: {expected}"
    );
    for line in expected.lines() {
        let mut fields = line.split_whitespace().map(str::to_owned);
        let id = fields.next().expect("an id");
        let parents: Vec<String> = fields.collect();
        let (_, ours_parents) = ours
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .unwrap_or_else(|| panic!("{id} is missing from fathom's walk"));
        assert_eq!(*ours_parents, parents, "parents of {id}");
    }
}

#[test]
fn a_start_point_from_a_ref_or_tag_is_the_commit_git_starts_from() {
    let dag = dag();
    let repo = open(dag.root());
    for spec in ["v2.0", "main", "HEAD", "feature"] {
        let resolved = repo.resolve(spec).expect(spec);
        assert_eq!(
            resolved.hex(),
            git_rev_parse(dag.root(), &format!("{spec}^{{commit}}")),
            "`{spec}`"
        );
        let ours = ids(repo.log(WalkOptions::head().starting_at([resolved]).first_parent()));
        assert_eq!(
            ours.first().expect("non-empty"),
            &resolved.hex(),
            "the walk starts where it was told to"
        );
    }
}

#[test]
fn an_octopus_merge_keeps_every_parent_in_git_order() {
    let fixture = Fixture::new("octopus");
    let root = fixture.init();
    fixture.commit(&root, "gen 0", &[("seed.txt", "0\n")]);
    for name in ["one", "two", "three"] {
        fixture.branch(&root, name);
        git(&root, &["checkout", "-q", name]);
        let file = format!("{name}.txt");
        fixture.commit(&root, &format!("{name} work"), &[(&file, "x\n")]);
        git(&root, &["checkout", "-q", "main"]);
    }
    git(
        &root,
        &["merge", "--no-ff", "-m", "octopus", "one", "two", "three"],
    );
    let repo = open(&root);
    assert_eq!(
        ids(repo.log(WalkOptions::head().first_parent())),
        parsed(&git(&root, &["rev-list", "--first-parent", "main"]))
    );
    let tip = repo
        .log(WalkOptions::head().limit(1))
        .next()
        .expect("a commit")
        .expect("readable");
    assert_eq!(
        tip.parents.iter().map(Oid::hex).collect::<Vec<_>>(),
        git(
            &root,
            &["rev-parse", "main^1", "main^2", "main^3", "main^4"]
        )
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>(),
        "an octopus merge keeps all four parents, in git's order"
    );
}

#[test]
fn a_parent_dated_newer_than_its_child_lands_where_git_puts_it() {
    // The interesting case for a date-ordered walk: a commit that is *older* than its own parent. Both
    // git and fathom can only emit a commit once it has been discovered through a child, so a backdated
    // commit appears where its discovery point is, not where its date would put it. This pins the
    // agreement down rather than discovering it as a surprise later.
    let fixture = Fixture::new("late-date");
    let root = fixture.init();
    fixture.commit_at(&root, "gen 0", &[("seed.txt", "0\n")], &date_at(0));
    fixture.commit_at(&root, "gen 1", &[("one.txt", "1\n")], &date_at(100));
    let young = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["checkout", "-q", "-b", "backdated", "HEAD~1"]);
    fixture.commit_at(&root, "backdated past", &[("old.txt", "x\n")], &date_at(5));
    git(&root, &["checkout", "-q", "main"]);
    git(
        &root,
        &[
            "-c",
            "advice.resolveConflict=false",
            "merge",
            "--no-ff",
            "-m",
            "merge backdated",
            "backdated",
        ],
    );
    let repo = open(&root);
    let ours = ids(repo.log(WalkOptions::head().order(WalkOrder::DateNewestFirst)));
    assert_eq!(
        ours,
        parsed(&git(&root, &["rev-list", "--date-order", "main"])),
        "the backdated commit is emitted in the same place git puts it"
    );
    assert_eq!(
        ours.first().expect("non-empty").as_str(),
        git_head(&root).as_str(),
        "the tip is first: newest by date as well as by position"
    );
    assert!(
        ours.iter().position(|id| *id == young).expect("found") > 0,
        "the walk ordered by date, not by depth"
    );
}

#[test]
fn a_walk_from_a_missing_start_is_a_typed_error_not_an_empty_iterator() {
    let dag = dag();
    let repo = open(dag.root());
    let missing_hex = "0123456789abcdef0123456789abcdef01234567";
    let missing = Oid::from_str(missing_hex).expect("oid");
    // `git rev-list` refuses a start it cannot resolve, and so surfaces the failure as a hard error.
    let status = git_status(dag.root(), &["rev-list", "--count", missing_hex]);
    assert!(
        status.never_succeeded(),
        "git refuses it too: {}",
        status.stderr()
    );
    // fathom's policy (set in `graph`) is different on purpose: name the unreadable start as a notice and
    // walk what it can, so a walk asked for from four refs is not lost because one id was garbage.
    let outcome = repo
        .log(WalkOptions::head().starting_at([missing]))
        .finish()
        .expect("not a crash");
    assert!(outcome.is_empty(), "nothing is reachable from it");
    assert_eq!(
        outcome.notices,
        vec![Notice::ObjectUnavailable {
            oid: missing,
            why: Unavailable::Missing
        }],
        "and the walk says which start it could not use"
    );
}

//! The changed-path set, checked against `git` itself (`e1.4` / reco-f28).
//!
//! ADR-004 §6.6 R1 promises the analysis layer an honest `changed_paths` and `blob_of`, and R7 promises
//! they are cheap. Cheap is a benchmark's business (`fathom-analyze`'s warm-index bench); this file is
//! about *correct*: every claim [`fathom_ingest::diff`] makes about what moved is compared with what
//! `git diff --raw`, `git ls-tree` and `git rev-parse tree:path` say, on fixtures built by the same
//! helper that builds every other `fathom-ingest` fixture.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_ingest::diff::{PathChange, PathChangeKind};
use fathom_ingest::oid::Oid;
use fathom_ingest::paths::RepoPath;
use fathom_ingest::record::{Limits, Unclassified};
use fathom_ingest::repo::Repository;
use std::collections::BTreeSet;
use std::path::Path;
use std::str::FromStr as _;

/// The tree a commit points at, taken from `git rev-parse` rather than from fathom: a wrong tree then
/// fails loudly here instead of quietly in the walk.
fn tree_of(cwd: &Path, rev: &str) -> Oid {
    oid_of(&git_rev_parse(cwd, &format!("{rev}^{{tree}}")))
}

fn oid_of(text: &str) -> Oid {
    Oid::from_str(text.trim()).expect("an oid")
}

/// What `git diff --raw --no-renames` says moved, as `(letter, path)`.
fn git_raw(cwd: &Path, from: &str, to: &str) -> BTreeSet<String> {
    git_bytes(
        cwd,
        &[
            "diff",
            "--raw",
            "--no-renames",
            "-M0",
            "--no-ext-diff",
            from,
            to,
        ],
    )
    .split(|byte| *byte == b'\n')
    .filter(|line| !line.is_empty())
    .map(|line| {
        let text = String::from_utf8_lossy(line).into_owned();
        let (_colon, rest) = text.split_once(':').unwrap_or(("", &text));
        // git's raw record is `:<old mode> <new mode> <old oid> <new oid> <status>\t<path>`, with the
        // status space-attached to the oids — and for a rename, `\t<old>\t<new>`, which `--no-renames`
        // rules out here.
        let fields: Vec<&str> = rest.split('\t').filter(|field| !field.is_empty()).collect();
        let status = fields
            .first()
            .and_then(|head| head.split_whitespace().last())
            .unwrap_or_default();
        format!("{status}\t{}", fields.last().copied().unwrap_or_default())
    })
    .collect()
}

/// What fathom says moved, in the same `(letter, path)` shape so the two can be compared directly.
fn fathom_raw(changes: &[PathChange]) -> BTreeSet<String> {
    changes
        .iter()
        .map(|change| {
            let letter = match change.kind {
                PathChangeKind::Added => "A",
                PathChangeKind::Deleted => "D",
                PathChangeKind::TypeChanged => "T",
                PathChangeKind::Modified | PathChangeKind::ModeChanged => "M",
            };
            format!("{letter}\t{}", change.path.to_string_lossy())
        })
        .collect()
}

#[test]
fn a_deep_single_file_edit_is_exactly_the_changed_set_git_reports() {
    let fixture = Fixture::new("changed-deep");
    let repo_dir = fixture.init();
    fixture.commit(
        &repo_dir,
        "seed",
        &[
            ("src/app/core/mod.rs", "fn a() {}\n"),
            ("src/app/util/strings.rs", "pub fn s() {}\n"),
            ("docs/README.md", "# hi\n"),
            ("tests/it.rs", "#[test] fn t() {}\n"),
        ],
    );
    let before = git_head(&repo_dir);
    write(&repo_dir, "src/app/util/strings.rs", "pub fn s() { 1 }\n");
    fixture.commit(&repo_dir, "edit one deep file", &[]);
    let after = git_head(&repo_dir);

    let repo = Repository::discover(&repo_dir).expect("discover");
    let changed = repo
        .changed_paths(
            Some(tree_of(&repo_dir, &before)),
            Some(tree_of(&repo_dir, &after)),
        )
        .expect("changed_paths");
    assert_eq!(
        changed.len(),
        1,
        "one file changed, so exactly one path is reported"
    );
    assert_eq!(changed[0].path.to_string_lossy(), "src/app/util/strings.rs");
    assert_eq!(changed[0].kind, PathChangeKind::Modified);
    assert_ne!(
        changed[0].old.expect("old side").oid,
        changed[0].new.expect("new side").oid,
        "a modified path names two different blobs"
    );
    assert_eq!(
        fathom_raw(&changed),
        git_raw(&repo_dir, &before, &after),
        "git and fathom agree about what moved"
    );
}

#[test]
fn every_kind_of_change_is_reported_and_matches_git() {
    let fixture = Fixture::new("changed-kinds");
    let repo_dir = fixture.init();
    fixture.commit(
        &repo_dir,
        "seed",
        &[
            ("keep.txt", "same\n"),
            ("modify.txt", "one\n"),
            ("drop.txt", "gone\n"),
            ("script.sh", "echo hi\n"),
            ("nested/deep/leaf.txt", "leaf\n"),
        ],
    );
    let before = git_head(&repo_dir);
    write(&repo_dir, "modify.txt", "two\n");
    write(&repo_dir, "added.txt", "new\n");
    write(&repo_dir, "nested/deep/other.txt", "another\n");
    std::fs::remove_file(fixture.path("drop.txt")).expect("remove");
    make_executable(&repo_dir, "script.sh");
    fixture.commit(&repo_dir, "five kinds of change", &[]);
    let after = git_head(&repo_dir);

    let repo = Repository::discover(&repo_dir).expect("discover");
    let changed = repo
        .changed_paths(
            Some(tree_of(&repo_dir, &before)),
            Some(tree_of(&repo_dir, &after)),
        )
        .expect("changed_paths");
    let seen: BTreeSet<String> = changed
        .iter()
        .map(|change| format!("add?\t{}", change.path.to_string_lossy()))
        .collect();
    for path in [
        "modify.txt",
        "added.txt",
        "drop.txt",
        "script.sh",
        "nested/deep/other.txt",
    ] {
        assert!(
            seen.iter().any(|row| row.ends_with(path)),
            "expected {path} in {seen:?}"
        );
    }
    assert!(
        !seen.iter().any(|row| row.ends_with("\tkeep.txt")),
        "an unchanged file must not appear: {seen:?}"
    );

    // A mode-only change is a change. git spells it `M` with the same blob on both sides, and fathom
    // says `modify` with equal blob ids — the pair of facts a reviewer needs in order to know it was a
    // chmod and not a content edit.
    let mode = changed
        .iter()
        .find(|change| change.path.to_string_lossy() == "script.sh")
        .expect("the chmod is reported");
    assert_eq!(mode.kind, PathChangeKind::ModeChanged);
    assert_eq!(mode.old.expect("old").oid, mode.new.expect("new").oid);
    assert_ne!(mode.old.expect("old").mode, mode.new.expect("new").mode);

    assert_eq!(fathom_raw(&changed), git_raw(&repo_dir, &before, &after));
}

#[test]
fn a_directory_replaced_by_a_file_is_the_file_plus_every_path_inside_it() {
    let fixture = Fixture::new("changed-type");
    let repo_dir = fixture.init();
    fixture.commit(
        &repo_dir,
        "seed",
        &[
            ("pkg/a.txt", "a\n"),
            ("pkg/b.txt", "b\n"),
            ("pkg/sub/c.txt", "c\n"),
        ],
    );
    let before = git_head(&repo_dir);
    std::fs::remove_dir_all(fixture.path("pkg")).expect("remove the directory");
    write(&repo_dir, "pkg", "now a file\n");
    fixture.commit(&repo_dir, "directory became a file", &[]);
    let after = git_head(&repo_dir);

    let repo = Repository::discover(&repo_dir).expect("discover");
    let changed = repo
        .changed_paths(
            Some(tree_of(&repo_dir, &before)),
            Some(tree_of(&repo_dir, &after)),
        )
        .expect("changed_paths");
    // git's own answer is the new file as `A` and every path that was inside the old directory as `D`,
    // and that is the set a warm path must analyse, so it is the set this walk owes.
    assert_eq!(
        fathom_raw(&changed),
        git_raw(&repo_dir, &before, &after),
        "a subtree that vanished is every path inside it, exactly as git counts them"
    );
    assert_eq!(
        changed.len(),
        4,
        "`pkg` arrived; three paths inside it went"
    );
    let arrived = changed
        .iter()
        .find(|change| change.path.to_string_lossy() == "pkg")
        .expect("the new file is reported");
    assert_eq!(arrived.kind, PathChangeKind::Added);
    assert!(arrived.old.is_none(), "git reports it with no old side");
    assert!(!arrived.new.expect("a new side").is_tree());
    assert!(
        changed
            .iter()
            .filter(|change| change.kind == PathChangeKind::Deleted)
            .all(|change| change.new.is_none()),
        "a deletion names only what it deleted"
    );
}

#[test]
fn the_root_commit_reports_every_file_and_a_missing_side_is_none() {
    let fixture = Fixture::new("changed-root");
    let repo_dir = fixture.init();
    fixture.commit(
        &repo_dir,
        "first",
        &[
            ("a.txt", "a\n"),
            ("dir/b.txt", "b\n"),
            ("dir/deep/c.txt", "c\n"),
        ],
    );
    let repo = Repository::discover(&repo_dir).expect("discover");
    let tree = tree_of(&repo_dir, "HEAD");
    let changed = repo
        .changed_paths(None, Some(tree))
        .expect("a diff against no tree at all");
    let paths: Vec<String> = changed
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(paths, vec!["a.txt", "dir/b.txt", "dir/deep/c.txt"]);
    assert!(
        changed
            .iter()
            .all(|change| change.kind == PathChangeKind::Added && change.old.is_none()),
        "there was no before, and the rows say so"
    );

    // And the other direction: everything is a deletion, which is what a tree that vanished looks like.
    let reversed = repo.changed_paths(Some(tree), None).expect("reverse");
    assert_eq!(reversed.len(), 3);
    assert!(
        reversed
            .iter()
            .all(|change| change.kind == PathChangeKind::Deleted)
    );
}

#[test]
fn blob_of_agrees_with_git_rev_parse_and_answers_none_for_paths_that_are_not_there() {
    let fixture = Fixture::new("changed-blob-of");
    let repo_dir = fixture.init();
    fixture.commit(
        &repo_dir,
        "seed",
        &[
            ("x.txt", "x\n"),
            ("dir/nested/y.txt", "y\n"),
            ("bin.dat", "a\0b\n"),
        ],
    );
    let tree = tree_of(&repo_dir, "HEAD");
    let repo = Repository::discover(&repo_dir).expect("discover");

    for path in ["x.txt", "dir/nested/y.txt", "bin.dat"] {
        let expected = oid_of(&git(&repo_dir, &["rev-parse", &format!("HEAD:{path}")]));
        let found = repo
            .blob_of(tree, &RepoPath::new(path))
            .expect("blob_of")
            .expect("present");
        assert_eq!(
            found.oid, expected,
            "git and fathom name the same blob for {path}"
        );
        assert_eq!(found.mode & 0o170000, 0o100000, "{path} is a regular file");
    }

    for absent in [
        "nope.txt",
        "dir/nope.txt",
        "dir",          // a directory is not a file
        "x.txt/inside", // a file cannot hold a path
        "",             // the empty path is the tree itself
    ] {
        assert_eq!(
            repo.blob_of(tree, &RepoPath::new(absent))
                .expect("not an error"),
            None,
            "{absent} is not a file in this tree"
        );
    }
}

#[test]
fn tree_files_enumerates_what_git_ls_tree_lists() {
    let fixture = Fixture::new("changed-ls");
    let repo_dir = fixture.init();
    fixture.commit(
        &repo_dir,
        "seed",
        &[
            ("a.txt", "aaa\n"),
            ("dir/b.txt", "bb\n"),
            ("dir/deep/c.txt", "c\n"),
            ("bin.dat", "\0\0\n"),
        ],
    );
    fixture.commit(&repo_dir, "second", &[("a.txt", "aaa\naaa\n")]);
    let tree = tree_of(&repo_dir, "HEAD");
    let repo = Repository::discover(&repo_dir).expect("discover");

    let listing = repo.tree_files(tree, None).expect("tree_files");
    assert!(listing.complete());
    let mine: Vec<(String, String, u64)> = listing
        .files
        .iter()
        .map(|file| {
            (
                file.path.to_string_lossy().into_owned(),
                file.oid.hex(),
                file.size,
            )
        })
        .collect();
    let theirs: Vec<(String, String, u64)> = git(&repo_dir, &["ls-tree", "-r", "-l", &tree.hex()])
        .lines()
        .map(|line| {
            // `<mode> <type> <oid> <size>\t<path>` — `-l` pads the size, so split on whitespace runs.
            let (meta, path) = line.split_once('\t').expect("ls-tree line");
            let fields: Vec<&str> = meta.split_whitespace().collect();
            (
                path.to_owned(),
                fields[2].to_owned(),
                fields
                    .get(3)
                    .and_then(|size| size.parse().ok())
                    .unwrap_or(0),
            )
        })
        .collect();
    assert_eq!(
        mine, theirs,
        "byte order over paths, and every blob's id and size"
    );
    assert_eq!(
        listing.total_bytes(),
        theirs.iter().map(|(_, _, size)| size).sum::<u64>()
    );
    assert_eq!(
        listing.directories, 3,
        "the root, `dir/` and `dir/deep/` were the tree objects that had to be read"
    );

    // The cap is a stated partial, not a silent one.
    let capped = repo.tree_files(tree, Some(2)).expect("capped");
    assert!(capped.truncated && capped.files.len() == 2);
    assert!(!capped.complete());
}

#[test]
fn a_rename_is_two_paths_here_and_one_row_in_the_spine() {
    // The documented split, asserted rather than hoped for: the warm changed-set walk reports what the
    // tree says (one path gone, one arrived), and rename pairing belongs to the spine's normalizer and
    // to `diff.rename_detect` (`e2.4`). Both halves in one test is what keeps the split from being
    // "discovered" the day the rename analyzer lands.
    let fixture = Fixture::new("changed-rename");
    let repo_dir = fixture.init();
    let body = "line one\nline two\nline three\nline four\n";
    fixture.commit(&repo_dir, "seed", &[("old/name.txt", body)]);
    write(&repo_dir, "new/name.txt", body);
    std::fs::remove_file(fixture.path("old/name.txt")).expect("remove the old name");
    fixture.commit(&repo_dir, "moved it", &[]);

    let repo = Repository::discover(&repo_dir).expect("discover");
    let commit = oid_of(&git_head(&repo_dir));
    let header = repo.commit(commit).expect("commit").expect("present");
    let changed = repo
        .changed_paths(
            Some(
                repo.commit(
                    *header
                        .parents
                        .first()
                        .expect("a parent on a non-root commit"),
                )
                .expect("parent")
                .expect("present")
                .tree,
            ),
            Some(header.tree),
        )
        .expect("changed_paths");
    assert_eq!(changed.len(), 2, "a raw tree diff: one gone, one arrived");
    assert!(changed.iter().any(|c| c.kind == PathChangeKind::Deleted));
    assert!(changed.iter().any(|c| c.kind == PathChangeKind::Added));
    let blobs: Vec<Oid> = changed.iter().filter_map(PathChange::blob).collect();
    assert_eq!(
        blobs.first().copied(),
        blobs.get(1).copied(),
        "the same content moved, which is exactly the fact a rename analyzer exists to report"
    );

    // The spine, by contrast, pairs them into one rename row — the difference this split is built on.
    let normalizer = fathom_ingest::normalize::Normalizer::new(
        &repo,
        &Unclassified,
        fathom_ingest::normalize::Options::default(),
    );
    let record = normalizer.record(commit).expect("record");
    assert_eq!(record.files.len(), 1, "one row, not two");
    assert_eq!(
        record.files[0].change,
        fathom_ingest::record::ChangeKind::Rename,
        "the spine sees a rename; the warm walk sees two paths, and both are stated"
    );
}

#[test]
fn measuring_one_pair_is_the_same_number_the_spine_records() {
    // The per-file analysis cache is only honest if a cached measurement and a spine row are the same
    // computation. Same bytes in, same counts out — asserted, not assumed.
    let fixture = Fixture::new("changed-measure");
    let repo_dir = fixture.init();
    fixture.commit(&repo_dir, "seed", &[("f.txt", "1\n2\n3\n4\n5\n")]);
    write(&repo_dir, "f.txt", "1\nTWO\n3\n4\nFIVE\n");
    fixture.commit(&repo_dir, "two lines", &[]);

    let repo = Repository::discover(&repo_dir).expect("discover");
    let commit = oid_of(&git_head(&repo_dir));
    let header = repo.commit(commit).expect("commit").expect("present");
    let parent = repo
        .commit(
            *header
                .parents
                .first()
                .expect("a parent on a non-root commit"),
        )
        .expect("parent")
        .expect("present");
    let changed = repo
        .changed_paths(Some(parent.tree), Some(header.tree))
        .expect("changed");
    let change = changed.first().expect("one change");
    let limits = Limits::default();
    let measured = fathom_ingest::diff::measure_blobs(
        &repo,
        change.old.map(|entry| entry.oid),
        change.new.map(|entry| entry.oid),
        &limits,
    )
    .expect("measure");
    assert_eq!((measured.added, measured.removed), (2, 2));
    // The hunk count is git's, computed with the same context the spine's default uses: two edits three
    // lines apart share their context and therefore share a hunk, which is the difference between git's
    // headers and a raw change list's.
    let git_hunks = git(
        &repo_dir,
        &["diff", "--unified=3", "HEAD~1", "HEAD", "--", "f.txt"],
    )
    .lines()
    .filter(|line| line.starts_with("@@"))
    .count();
    assert_eq!(
        measured.hunk_total, git_hunks,
        "the hunk list is git's, not an approximation of it"
    );
    assert_eq!(measured.hunks.len(), git_hunks);

    let normalizer = fathom_ingest::normalize::Normalizer::new(
        &repo,
        &Unclassified,
        fathom_ingest::normalize::Options::default(),
    );
    let row = normalizer
        .record(commit)
        .expect("record")
        .files
        .first()
        .expect("one file row")
        .clone();
    assert_eq!((row.added, row.removed), (measured.added, measured.removed));
    assert_eq!(row.hunk_total, measured.hunk_total);
    assert_eq!(
        row.hunks, measured.hunks,
        "the same `@@` headers, from one code path"
    );
}

#[test]
fn an_identical_tree_reports_nothing() {
    // The property `warm.reindex.noop.ms` rests on: comparing a tree with itself is answered from the
    // root entry, and nothing below it is read.
    let fixture = Fixture::new("changed-noop");
    let repo_dir = fixture.init();
    for index in 0..12 {
        let name = format!("f{index}.txt");
        fixture.commit(
            &repo_dir,
            &format!("seed {index}"),
            &[(name.as_str(), "content\n")],
        );
    }
    let repo = Repository::discover(&repo_dir).expect("discover");
    let tree = tree_of(&repo_dir, "HEAD");
    assert!(
        repo.changed_paths(Some(tree), Some(tree))
            .expect("the same tree against itself")
            .is_empty(),
        "nothing moved, so nothing is reported"
    );
    let listing = repo.tree_files(tree, None).expect("the full enumeration");
    assert_eq!(
        listing.files.len(),
        12,
        "which is what the warm path avoids"
    );
}

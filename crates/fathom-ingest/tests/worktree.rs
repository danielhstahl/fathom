//! The working tree as files: what a bounded walk hands over, what it refuses to enter, and what one read
//! costs (`reco-p41`).
//!
//! These are the promises `fathom_analyze::search::symbols` is built on now that the walk lives in the door:
//! one order, one depth rule, one skip list, and a read that says which of "empty" and "unreadable" it means.
//! The analysis half — the caps, the extractor, what a declaration is — is pinned in
//! `crates/fathom-analyze/tests/search_corpus.rs`, and deliberately not duplicated here.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_ingest::worktree::{self, WalkPolicy};
use std::path::{Path, PathBuf};

/// A tree written to a directory no other test is using, from `(relative path, contents)` pairs.
fn tree(files: &[(&str, &str)]) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "fathom-worktree-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    for (path, text) in files {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().expect("a directory to write into"))
            .expect("create the fixture directory");
        std::fs::write(full, text).expect("write the fixture file");
    }
    root
}

/// Drop the fixture: a test that fails has already told the reviewer, and a leftover tree in `/tmp` tells
/// them nothing.
fn cleanup(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
}

/// Every path of one walk, in the order the walk returned them.
fn paths(root: &Path, policy: &WalkPolicy, wants: &dyn Fn(&str) -> bool) -> Vec<String> {
    worktree::find(root, policy, wants)
        .into_iter()
        .map(|entry| entry.path)
        .collect()
}

/// The predicate these tests use instead of a language table: "a `.rs` file is the interesting one".
fn rust(path: &str) -> bool {
    path.ends_with(".rs")
}

#[test]
fn a_walk_hands_over_repository_relative_paths_in_one_order() {
    let root = tree(&[
        ("b.rs", "fn b() {}\n"),
        ("a.rs", "fn a() {}\n"),
        ("notes/prose.txt", "not a source file\n"),
        ("src/lib.rs", "mod inner;\n"),
        ("src/inner.rs", "pub fn inner() {}\n"),
    ]);
    let policy = WalkPolicy::default();

    let found = paths(&root, &policy, &rust);
    assert_eq!(
        found,
        ["a.rs", "b.rs", "src/inner.rs", "src/lib.rs"],
        "paths are repository-relative, `/`-separated and sorted, so a caller's caps are spent the same way \
         twice",
    );

    // The same walk twice is the same list: the extractor's input order, and therefore a golden test's
    // fixture, may not depend on what `readdir` felt like.
    assert_eq!(found, paths(&root, &policy, &rust), "one tree, one order");

    // The absolute path is the one that opens, and it is under the root that was handed in.
    let entries = worktree::find(&root, &policy, &rust);
    for entry in &entries {
        assert!(
            entry.absolute.starts_with(&root),
            "the read path is under the root the caller named: {:?}",
            entry.absolute
        );
        assert!(
            entry.size > 0,
            "a file the walk wrote has a size, and the caller spends its caps on it: {}",
            entry.path
        );
        assert!(
            entry.modified > 0,
            "a file written a moment ago has a last touch: {}",
            entry.path
        );
        assert_eq!(
            entry.read().expect("read the fixture file"),
            std::fs::read(&entry.absolute).expect("read it directly too"),
            "the door's read and a direct read are the same bytes"
        );
    }

    // A file the predicate refused is not in the list at all, and neither is a directory.
    assert!(!found.iter().any(|path| path.ends_with(".txt")));
    cleanup(&root);
}

#[test]
fn depth_is_a_rule_the_walk_keeps_to_itself() {
    let root = tree(&[
        ("one.rs", "// 1\n"),
        ("a/two.rs", "// 2\n"),
        ("a/b/three.rs", "// 3\n"),
        ("a/b/c/four.rs", "// 4\n"),
    ]);

    let one_deep = paths(
        &root,
        &WalkPolicy {
            max_depth: 1,
            ..WalkPolicy::default()
        },
        &rust,
    );
    assert_eq!(
        one_deep,
        ["a/two.rs", "one.rs"],
        "depth 1 means one directory below the root and no further"
    );

    let three_deep = paths(
        &root,
        &WalkPolicy {
            max_depth: 3,
            ..WalkPolicy::default()
        },
        &rust,
    );
    assert_eq!(
        three_deep,
        ["a/b/c/four.rs", "a/b/three.rs", "a/two.rs", "one.rs"],
        "the leaf a shallow walk lost is the only difference a deeper walk finds"
    );

    // The default is deep enough for the case it was sized against, and `find` does not descend past the
    // depth it was given even where a directory exists to descend into.
    assert_eq!(
        three_deep,
        paths(&root, &WalkPolicy::default(), &rust),
        "the default depth reaches the deepest level this tree has, which is the point of six"
    );
    cleanup(&root);
}

#[test]
fn a_directory_the_walk_skips_is_not_walked_at_all() {
    let root = tree(&[
        ("src/lib.rs", "mod ours;\n"),
        ("src/vendor/lib.rs", "fn not_ours() {}\n"),
        ("node_modules/left-pad/index.rs", "fn not_reached() {}\n"),
        (".git/objects/ab/cdef.rs", "fn nothing_here() {}\n"),
        ("build/out.rs", "fn compiled_output() {}\n"),
        (
            "me/third-party/keep.rs",
            "fn a_directory_somebodys_config_names() {}\n",
        ),
    ]);
    let policy = WalkPolicy::default();

    let found = paths(&root, &policy, &rust);
    assert_eq!(
        found,
        ["me/third-party/keep.rs", "src/lib.rs"],
        "the build and vendor names stop the walk where they are, at any depth, and a hidden directory is \
         never entered"
    );

    // `skips` is the same judgment the walk applied, exposed: a caller that is deciding about a path it
    // already has gets the same answer the walk gave while descending.
    assert!(
        policy.skips("node_modules") && policy.skips("target") && policy.skips(".git"),
        "the door's list and the door's judgment are one list"
    );
    assert!(!policy.skips("src"));

    // A caller's list is added to the door's, not substituted for it — the ignore list in a repository's
    // config does not get to make `node_modules` interesting again.
    let mine = WalkPolicy {
        max_depth: WalkPolicy::default().max_depth,
        skip_dirs: vec!["Third-Party".to_owned()],
    };
    assert_eq!(
        paths(&root, &mine, &rust),
        ["src/lib.rs"],
        "a caller's names are matched case-insensitively, like the door's, and the door's own names still \
         stand"
    );
    cleanup(&root);
}

#[test]
fn a_read_that_cannot_happen_says_so_rather_than_returning_nothing() {
    let root = tree(&[("here.rs", "fn here() {}\n")]);
    let entries = worktree::find(&root, &WalkPolicy::default(), &rust);
    let one = entries.first().expect("the one file");
    assert_eq!(one.path, "here.rs");
    assert!(!one.read().expect("read it").is_empty());

    // The failure shape a caller has to be able to count: a path the tree no longer has. `fathom-analyze`'s
    // scan turns a tally of these into a note, which is the whole reason the door returns an `io::Result`
    // instead of an empty `Vec` — an empty body and a missing file are different answers (ADR-001 §7.2).
    let missing = root.join("gone.rs");
    assert_eq!(
        worktree::read(&missing).err().map(|why| why.kind()),
        Some(std::io::ErrorKind::NotFound),
        "a file that is not there is a NotFound, not an empty file"
    );

    // A directory is not a file, and reading one is an error rather than a hang or a zero-length answer.
    assert!(worktree::read(root.join("here.rs").parent().expect("the root")).is_err());
    cleanup(&root);
}

#[test]
fn a_tree_that_is_not_there_walks_as_an_empty_tree() {
    // The door never fails a walk: a root that does not exist, or is a file, or is a directory the process
    // cannot read, hands back nothing and lets the caller — which knows what it was looking for — say so.
    let absent =
        std::env::temp_dir().join(format!("fathom-worktree-absent-{}", std::process::id()));
    assert!(
        worktree::find(&absent, &WalkPolicy::default(), &rust).is_empty(),
        "a root that is not there is a tree with nothing in it"
    );

    let root = tree(&[("a.rs", "fn a() {}\n")]);
    let file = root.join("a.rs");
    assert!(
        worktree::find(&file, &WalkPolicy::default(), &rust).is_empty(),
        "a root that is a file has no children to hand over"
    );
    cleanup(&root);
}

#[test]
fn a_symlink_is_neither_the_file_nor_the_directory_it_points_at() {
    // The property that keeps a walk bounded: a tree with a link back at one of its own ancestors is a walk
    // that never ends if links are followed, and a link into a home directory is a walk that leaves the
    // repository the caller named.
    #[cfg(unix)]
    {
        let root = tree(&[("a.rs", "fn a() {}\n"), ("real/b.rs", "fn b() {}\n")]);
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).expect("make the symlink");
        std::os::unix::fs::symlink(root.join("a.rs"), root.join("link.rs")).expect("link a file");

        let found = paths(&root, &WalkPolicy::default(), &rust);
        assert_eq!(
            found,
            ["a.rs", "real/b.rs"],
            "neither a symlinked directory nor a symlinked file is handed over"
        );
        cleanup(&root);
    }
}

#[test]
fn the_default_lists_are_the_ones_the_walk_uses() {
    // A constant a caller quotes and a constant the walk uses are only one fact if they agree; this is the
    // pin that keeps them one fact.
    assert!(worktree::DEFAULT_SKIPPED_DIRS.contains(&"node_modules"));
    assert!(worktree::DEFAULT_SKIPPED_DIRS.contains(&"target"));
    assert!(!worktree::DEFAULT_SKIPPED_DIRS.contains(&"src"));
    let policy = WalkPolicy::default();
    for skipped in worktree::DEFAULT_SKIPPED_DIRS {
        assert!(
            policy.skips(skipped),
            "{skipped} is in both lists or in neither"
        );
    }
    assert_eq!(
        WalkPolicy::default().max_depth,
        worktree::DEFAULT_MAX_DEPTH,
        "the default policy walks the default depth"
    );
}

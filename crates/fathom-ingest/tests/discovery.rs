//! Repository discovery: the acceptance criteria of `reco-cey` that are about *finding* a repository.
//!
//! Every fixture here is built by the real `git` binary (see `tests/common/mod.rs`), and every claim is
//! double-checked against what `git` itself says, so a test that passes because `gix` and `fathom` agree
//! with each other and disagree with `git` cannot happen.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_core::error::{Diagnosable, ErrorCode};
use fathom_core::exit::ExitCode;
use fathom_ingest::error::Error;
use fathom_ingest::repo::{
    DiscoverOptions, HealthNotice, IndexPresence, RefFilter, RefKind, RefTarget, Repository,
    RepositoryKind, RepositoryKind as Kind, Shallow, WorktreePresence,
};

fn open(path: &std::path::Path) -> Repository {
    Repository::discover(path).expect("discover fixture repository")
}

#[test]
fn finds_a_repository_in_the_current_directory() {
    let fixture = Fixture::new("discovery");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);

    let repo = open(&root);
    assert_eq!(repo.kind(), RepositoryKind::WorkTree);
    assert!(!repo.is_bare());
    assert_eq!(
        repo.identity().display_name,
        root.file_name().expect("a name").to_string_lossy(),
        "the display name is the working tree's directory name"
    );
    assert_eq!(repo.git_dir(), root.join(".git"));
    assert_eq!(repo.common_dir(), root.join(".git"));
    assert_eq!(repo.work_dir(), Some(root.as_path()));
    assert_eq!(
        repo.head().expect("head").oid().map(|o| o.hex()),
        Some(git_head(&root))
    );
}

#[test]
fn searches_upward_from_a_subdirectory() {
    let fixture = Fixture::new("upward");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("nested/deep/file.txt", "x\n")]);
    let deep = fixture.path("nested/deep/more");
    std::fs::create_dir_all(&deep).expect("mkdir");

    let repo = open(&deep);
    assert_eq!(repo.work_dir(), Some(root.as_path()));
    // ...and the walk back up is not confused by the subdirectory being inside the work tree.
    assert_eq!(
        repo.identity().display_name,
        root.file_name().expect("a name").to_string_lossy()
    );
}

#[test]
fn a_directory_that_is_not_a_repository_is_a_typed_error_with_a_remedy() {
    let fixture = Fixture::new("not-a-repo");
    let error = Repository::discover(fixture.root()).expect_err("there is no repository here");
    match &error {
        Error::NotARepository { root } => assert!(
            root.contains("not-a-repo"),
            "the error must name the path the user typed: {root}"
        ),
        other => panic!("expected NotARepository, got {other:?}"),
    }
    assert_eq!(error.code(), ErrorCode::NotARepository);
    assert_eq!(error.code().exit(), ExitCode::NotARepository);
    let hint = error.hint().expect("a user-facing error carries a remedy");
    assert!(
        hint.contains("never fetches"),
        "the remedy must say what fathom will not do: {hint}"
    );
}

#[test]
fn a_ceiling_stops_the_search_and_says_so() {
    let fixture = Fixture::new("ceiling");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    let deep = fixture.path("a/b/c");
    std::fs::create_dir_all(&deep).expect("mkdir");

    let options = DiscoverOptions {
        ceiling: Some(fixture.path("a/b")),
        ..DiscoverOptions::default()
    };
    // The repository is above the ceiling, so the search that starts below it must not reach it.
    let error =
        Repository::discover_with(&deep, &options).expect_err("ceiling hides the repository");
    assert_eq!(error.code(), ErrorCode::NotARepository, "{error:?}");
}

#[test]
fn an_open_at_does_not_search_upwards() {
    let fixture = Fixture::new("open-at");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    let deep = fixture.path("sub/dir");
    std::fs::create_dir_all(&deep).expect("mkdir");

    let options = DiscoverOptions::default();
    assert!(
        Repository::open_at(&deep, &options).is_err(),
        "opening a path that is not a repository must not silently find the one above it"
    );
    let repo = Repository::open_at(&root, &options).expect("the repository root opens directly");
    assert_eq!(repo.work_dir(), Some(root.as_path()));
}

#[test]
fn a_bare_repository_is_a_repository_too() {
    let fixture = Fixture::new("bare");
    let root = fixture.init_bare();
    // A bare repository needs an object in it to be interesting: clone from a work tree.
    let seed = Fixture::new("bare-seed");
    let seed_root = seed.init();
    seed.commit(&seed_root, "seed commit", &[("one.txt", "one\n")]);
    git(
        &root,
        &[
            "-c",
            "protocol.file.allow=always",
            "fetch",
            "-q",
            &seed_root.display().to_string(),
            "refs/heads/main:refs/heads/main",
        ],
    );

    let repo = open(&root);
    assert_eq!(repo.kind(), RepositoryKind::Bare);
    assert!(repo.is_bare());
    assert_eq!(repo.work_dir(), None);
    let health = repo.health().expect("health");
    assert_eq!(health.worktree, WorktreePresence::Bare);
    assert!(
        health.notices.contains(&HealthNotice::Bare),
        "a bare repository must say it has no working tree: {:?}",
        health.notices
    );
    assert_eq!(
        repo.head().expect("bare head").oid().map(|o| o.hex()),
        Some(git_head(&root))
    );
}

#[test]
fn a_detached_head_is_a_state_not_an_error() {
    let fixture = Fixture::new("detached");
    let root = fixture.init();
    let first = fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    fixture.commit(&root, "second", &[("two.txt", "two\n")]);
    fixture.checkout_detached(&root, &first);

    let repo = open(&root);
    let head = repo.head().expect("head");
    assert!(head.is_detached());
    assert_eq!(head.branch_name(), None);
    assert_eq!(head.oid().expect("detached head has a commit").hex(), first);
    assert_eq!(head.summary(), format!("detached@{}", &first[..7]));
    let health = repo.health().expect("health");

    assert!(health.notices.contains(&HealthNotice::DetachedHead {
        at: first.parse().expect("oid")
    }));
    // The git binary agrees about what state this is.
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), first);
    assert!(
        git_status(&root, &["symbolic-ref", "-q", "HEAD"]).never_succeeded(),
        "git agrees there is no symbolic branch"
    );
}

#[test]
fn an_unborn_head_is_reported_as_unborn() {
    let fixture = Fixture::new("unborn");
    let root = fixture.init();

    let repo = open(&root);
    let head = repo.head().expect("head of a fresh repository");
    assert!(head.is_unborn(), "{head:?}");
    assert_eq!(head.branch_name(), Some("main"));
    assert_eq!(head.oid(), None);
    assert_eq!(head.summary(), "main (no commits yet)");
    let health = repo.health().expect("health");
    assert!(health.notices.contains(&HealthNotice::UnbornHead {
        branch: "main".into()
    }));
}

#[test]
fn a_submodule_is_recognised_and_names_its_superproject() {
    let fixture = Fixture::new("submodule-host");
    let source = Fixture::new("submodule-source");
    let source_root = source.init();
    source.commit(
        &source_root,
        "library commit",
        &[("lib.rs", "pub fn f() {}\n")],
    );

    let host = fixture.init();
    fixture.add_submodule(&host, &source_root, "vendor/lib");

    let checkout = fixture.path("vendor/lib");
    let sub = Repository::open_at(&checkout, &DiscoverOptions::default())
        .expect("the submodule checkout is a repository");
    let kind = sub.kind();
    match &kind {
        RepositoryKind::Submodule { superproject } => {
            assert_eq!(
                superproject,
                &host.join(".git"),
                "the superproject is named, not implied"
            );
        }
        other => panic!("expected a submodule, got {other:?}"),
    }
    assert_eq!(sub.superproject(), Some(host.join(".git").as_path()));

    // The superproject sees the submodule, with the commit it records.
    let host_repo = open(&host);
    let submodules = host_repo.submodules().expect("submodule listing");
    assert_eq!(submodules.len(), 1);
    let declared = &submodules[0];
    assert_eq!(declared.name, "vendor/lib");
    assert_eq!(declared.path.as_bytes(), b"vendor/lib");
    assert!(declared.url.is_some(), ".gitmodules records the URL");
    assert!(
        declared.recorded.is_some(),
        "the gitlink in HEAD is the recorded commit"
    );
    assert_eq!(
        declared.recorded.map(|oid| oid.hex()),
        Some(git_head(&source_root)),
        "the recorded gitlink is the source repository's commit"
    );
    assert_eq!(
        git(&checkout, &["rev-parse", "HEAD"]),
        declared.recorded.expect("recorded").hex()
    );
}

#[test]
fn a_linked_worktree_shares_the_object_database() {
    let fixture = Fixture::new("worktree");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    let other = fixture.path("wt-side");
    fixture.add_worktree_new_branch(&root, &other, "side");

    let main = open(&root);
    let listed = main.linked_worktrees().expect("worktree list");
    assert_eq!(listed.len(), 2, "main plus the linked worktree: {listed:?}");
    assert!(listed[0].is_main);
    let side = listed.iter().find(|w| !w.is_main).expect("linked worktree");
    assert_eq!(side.path, other);
    assert!(!side.missing);
    assert_eq!(
        side.head.as_ref().expect("head").branch_name(),
        Some("side")
    );

    let side_repo =
        Repository::open_at(&other, &DiscoverOptions::default()).expect("open worktree");
    match side_repo.kind() {
        RepositoryKind::LinkedWorkTree { main_repository } => {
            assert_eq!(main_repository, root.join(".git"));
        }
        other => panic!("expected a linked worktree, got {other:?}"),
    }
    // Objects and refs are shared, which is what `common_dir` is for.
    assert_eq!(side_repo.common_dir(), root.join(".git"));
    assert_ne!(side_repo.git_dir(), root.join(".git"));
    assert_eq!(
        side_repo.head().expect("head").oid().map(|o| o.hex()),
        Some(git_head(&other))
    );

    // A worktree whose directory is gone is still listed, and says it is missing.
    std::fs::remove_dir_all(&other).expect("remove worktree directory");
    let after = open(&root)
        .linked_worktrees()
        .expect("worktree list after removal");
    let gone = after
        .iter()
        .find(|w| !w.is_main)
        .expect("the prunable entry");
    assert!(
        gone.missing,
        "a deleted worktree directory is reported, not skipped"
    );
}

#[test]
fn a_shallow_clone_reports_its_boundary_and_refuses_to_pretend() {
    let fixture = Fixture::new("shallow");
    let source = fixture.init();
    for index in 0..5 {
        fixture.commit(
            &source,
            &format!("commit {index}"),
            &[("file.txt", &format!("content {index}\n"))],
        );
    }
    let clone = fixture.clone_shallow(&source, "shallow-clone", 2);

    let repo = open(&clone);
    let shallow = Shallow {
        is_shallow: true,
        boundary: repo
            .shallow_boundary()
            .expect("shallow file")
            .unwrap_or_default(),
    };
    assert!(shallow.is_shallow);
    assert!(!shallow.boundary.is_empty(), "a depth-2 clone has a graft");
    assert!(repo.is_shallow().expect("is_shallow"));
    // The boundary is exactly what git's own shallow file lists.
    let git_shallow: Vec<String> = std::fs::read_to_string(clone.join(".git/shallow"))
        .expect("shallow file")
        .lines()
        .map(str::to_owned)
        .collect();
    let mut ours: Vec<String> = shallow.boundary.iter().map(|oid| oid.hex()).collect();
    ours.sort();
    let mut theirs = git_shallow;
    theirs.sort();
    assert_eq!(
        ours, theirs,
        "the graft list must match git's byte for byte"
    );

    let health = repo.health().expect("health");
    assert_eq!(health.shallow.boundary.len(), shallow.boundary.len());
    assert!(health.notices.contains(&HealthNotice::Shallow {
        boundary: shallow.boundary.len()
    }));
}

#[test]
fn a_filtered_clone_is_reported_as_partial_and_its_missing_blobs_are_explained() {
    let fixture = Fixture::new("partial");
    let source = fixture.init();
    fixture.commit(&source, "first", &[("one.txt", "the first blob\n")]);
    fixture.allow_filter(&source);
    let clone = fixture.clone_filtered(&source, "partial-clone", "blob:none");

    let repo = open(&clone);
    let partial = repo.partial_clone();
    assert!(partial.is_partial, "remote.origin.promisor is set");
    assert_eq!(partial.filter.as_deref(), Some("blob:none"));
    assert_eq!(partial.remote.as_deref(), Some("origin"));
    assert_eq!(partial.promisor_remotes, vec!["origin".to_owned()]);

    // The tree is there; the blob it names is not. That is the degraded case, and it must be classified
    // rather than panic (the ticket's last line).
    let head = repo.head().expect("head").oid().expect("born");
    let header = repo.commit(head).expect("commit").expect("a commit");
    let entries = repo.tree_entries(header.tree).expect("the tree is present");
    let blob = entries
        .iter()
        .find(|entry| entry.name.as_bytes() == b"one.txt")
        .expect("one.txt");
    assert!(
        !repo.has_object(blob.oid),
        "the filter never fetched the blob"
    );
    let error = repo.blob(blob.oid, 1024).expect_err("the blob is not here");
    match &error {
        Error::ObjectUnavailable { oid, why } => {
            assert_eq!(*oid, blob.oid);
            match why {
                fathom_ingest::error::Unavailable::PartialClone { filter, remote } => {
                    assert_eq!(filter.as_deref(), Some("blob:none"));
                    assert_eq!(remote.as_deref(), Some("origin"));
                }
                other => panic!("expected a partial-clone explanation, got {other:?}"),
            }
        }
        other => panic!("expected ObjectUnavailable, got {other:?}"),
    }
    assert_eq!(error.code(), ErrorCode::ObjectUnavailable);
    assert!(
        error
            .hint()
            .is_some_and(|hint| hint.contains("never fetches")),
        "the remedy names the filter, and says fathom will not fetch: {:?}",
        error.hint()
    );
}

#[test]
fn an_object_that_is_simply_gone_says_the_repository_may_be_damaged() {
    let fixture = Fixture::new("missing");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "only copy\n")]);
    let blob = {
        let repo = open(&root);
        let head = repo.head().expect("head").oid().expect("born");
        let header = repo.commit(head).expect("commit").expect("commit");
        repo.tree_entries(header.tree).expect("tree").remove(0).oid
    };
    // A freshly committed object is stored loose, so removing the file makes it genuinely absent with
    // nothing in the repository to explain it — the case that must not be reported as a shallow graft or
    // a filter omission.
    std::fs::remove_file(
        root.join(".git/objects")
            .join(&blob.hex()[..2])
            .join(&blob.hex()[2..]),
    )
    .expect("delete the loose object");

    let repo = open(&root);
    assert!(!repo.has_object(blob));
    assert!(repo.partial_clone().filter.is_none());
    assert_eq!(
        repo.why_unavailable(blob),
        fathom_ingest::error::Unavailable::Missing
    );
    let error = repo.blob(blob, 1024).expect_err("gone");
    assert!(
        error.hint().is_some_and(|hint| hint.contains("git fsck")),
        "an unexplained missing object points at `git fsck`: {:?}",
        error.hint()
    );
}

#[test]
fn unicode_paths_survive_the_round_trip() {
    let fixture = Fixture::new("unicode");
    let root = fixture.init();
    fixture.commit(
        &root,
        "unicode names",
        &[
            ("docs/über view.txt", "umlaut\n"),
            ("中文/文件.txt", "CJK\n"),
            ("☕ coffee/☂ umbrella.md", "symbols\n"),
        ],
    );
    // A name that is valid on the filesystem and not valid UTF-8 — the case a `String` path type turns
    // into a `?`, and the reason `RepoPath` keeps bytes.
    write_bytes(&root, b"latin1/caf\xe9.txt", b"not utf-8\n");
    git(&root, &["add", "--all"]);
    git(&root, &["commit", "-q", "-m", "non-utf8 name"]);

    let repo = open(&root);
    let head = repo.head().expect("head").oid().expect("born");
    let header = repo.commit(head).expect("commit").expect("commit");
    let mut paths: Vec<Vec<u8>> = flatten_tree_bytes(&repo, header.tree);
    paths.sort();
    let as_text: Vec<String> = paths
        .iter()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect();
    assert!(
        as_text.iter().any(|p| p == "docs/über view.txt"),
        "{as_text:?}"
    );
    assert!(as_text.iter().any(|p| p == "中文/文件.txt"), "{as_text:?}");
    assert!(as_text.iter().any(|p| p == "☕ coffee/☂ umbrella.md"));
    // The invalid-UTF-8 name keeps its exact bytes; git stores bytes, so fathom must too.
    assert!(
        paths
            .iter()
            .any(|bytes| bytes == &b"latin1/caf\xe9.txt".to_vec()),
        "the raw bytes round-trip: {paths:?}"
    );
    // ...and git itself lists the same bytes, which is the half that proves it is not a `gix` quirk.
    let listing = git_bytes(&root, &["ls-tree", "-r", "--name-only", "-z", "HEAD"]);
    let git_paths: Vec<&[u8]> = listing
        .split(|byte| *byte == b'\0')
        .filter(|s| !s.is_empty())
        .collect();
    assert!(
        git_paths.contains(&&b"latin1/caf\xe9.txt"[..]),
        "{git_paths:?}"
    );
    assert!(git_paths.contains(&"docs/über view.txt".as_bytes()));
    assert_eq!(
        git_paths.len(),
        paths.len(),
        "fathom saw every path git stored, no more and no less"
    );
}

/// Flatten a tree through the public API only, so the test exercises the door and not its internals.
fn flatten_tree_bytes(repo: &Repository, root: fathom_ingest::oid::Oid) -> Vec<Vec<u8>> {
    use fathom_ingest::repo::TreeEntryKind;
    let mut out = Vec::new();
    let mut stack: Vec<(fathom_ingest::oid::Oid, Vec<u8>)> = vec![(root, Vec::new())];
    while let Some((tree, prefix)) = stack.pop() {
        for entry in repo.tree_entries(tree).expect("tree") {
            let mut full = prefix.clone();
            if !full.is_empty() {
                full.push(b'/');
            }
            full.extend_from_slice(entry.name.as_bytes());
            if entry.kind == TreeEntryKind::Tree {
                stack.push((entry.oid, full));
            } else {
                out.push(full);
            }
        }
    }
    out
}

#[test]
fn refs_are_listed_and_classified() {
    let fixture = Fixture::new("refs");
    let root = fixture.init();
    let first = fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    fixture.branch(&root, "topic");
    fixture.tag(&root, "v1.0");
    fixture.lightweight_tag(&root, "v0.9");
    fixture.commit(&root, "second", &[("two.txt", "two\n")]);
    // A remote-tracking ref, written the way a fetch would leave it.
    std::fs::create_dir_all(root.join(".git/refs/remotes/origin")).expect("mkdir");
    std::fs::write(
        root.join(".git/refs/remotes/origin/main"),
        format!("{first}\n"),
    )
    .expect("write ref");
    fixture.stash(&root, "one.txt", "stashed\n");

    let repo = open(&root);
    let all = repo.refs(RefFilter::All).expect("refs");
    let named = |kind: RefKind| -> Vec<String> {
        all.iter()
            .filter(|reference| reference.kind == kind)
            .map(|reference| reference.name.clone())
            .collect()
    };
    assert_eq!(
        named(RefKind::Branch),
        ["refs/heads/main", "refs/heads/topic"]
    );
    assert_eq!(named(RefKind::Tag), ["refs/tags/v0.9", "refs/tags/v1.0"]);
    assert_eq!(named(RefKind::RemoteBranch), ["refs/remotes/origin/main"]);
    assert_eq!(named(RefKind::Stash), ["refs/stash"]);

    // An annotated tag is a tag object whose peel is the commit; a lightweight tag points straight at it.
    let annotated = all
        .iter()
        .find(|reference| reference.name == "refs/tags/v1.0")
        .expect("tag");
    assert_eq!(
        annotated.peeled.map(|oid| oid.hex()),
        Some(first.clone()),
        "a tag ref carries the commit it names, whether the ref store recorded the peel or not"
    );
    let RefTarget::Direct { oid: tag_object } = annotated.target else {
        panic!("an annotated tag is a direct ref to a tag object")
    };
    assert_ne!(tag_object.hex(), first, "the tag object is not the commit");
    assert_eq!(
        repo.object_header(tag_object)
            .expect("header")
            .expect("present")
            .kind,
        fathom_ingest::repo::ObjectKind::Tag
    );
    assert_eq!(annotated.short_name(), "v1.0");
    let _ = Kind::WorkTree;

    // The filters filter.
    assert_eq!(repo.refs(RefFilter::Tags).expect("tags").len(), 2);
    assert_eq!(
        repo.refs(RefFilter::LocalBranches).expect("branches").len(),
        2
    );
    assert_eq!(
        repo.refs(RefFilter::RemoteBranches).expect("remotes").len(),
        1
    );
}

#[test]
fn a_broken_symbolic_ref_is_reported_and_not_followed_into_nothing() {
    let fixture = Fixture::new("broken-ref");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    // `git branch x refs/heads/nope` refuses, so write the loose ref the way a half-finished
    // operation would leave it: a symbolic ref whose target does not exist.
    std::fs::create_dir_all(root.join(".git/refs/heads")).expect("mkdir");
    std::fs::write(
        root.join(".git/refs/heads/pointless"),
        "ref: refs/heads/nope\n",
    )
    .expect("write the symbolic ref");

    let repo = open(&root);
    let health = repo.health().expect("health");
    assert!(
        health
            .notices
            .iter()
            .any(|notice| matches!(notice, HealthNotice::BrokenSymbolicRef { name, .. } if name == "refs/heads/pointless")),
        "a symbolic ref to nothing is a notice: {:?}",
        health.notices
    );
}

#[test]
fn the_health_report_names_the_index_and_the_ref_counts() {
    let fixture = Fixture::new("health");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    fixture.commit(&root, "second", &[("two.txt", "two\n")]);

    let repo = open(&root);
    let health = repo.health().expect("health");
    assert_eq!(
        health.worktree,
        WorktreePresence::Present { path: root.clone() }
    );
    assert!(
        matches!(health.index, IndexPresence::Present { .. }),
        "a checked-out repository has an index: {:?}",
        health.index
    );
    assert_eq!(health.refs.branches, 1);
    assert_eq!(health.refs.total(), 1);
    assert_eq!(health.submodules, 0);
    assert!(
        health.notices.is_empty(),
        "a clean ordinary repository has nothing to warn about: {:?}",
        health.notices
    );
}

#[test]
fn a_worktree_directory_that_is_gone_is_a_typed_error_with_the_path_it_missed() {
    let fixture = Fixture::new("no-worktree");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    let work = fixture.path("elsewhere");
    git(
        &root,
        &["config", "core.worktree", &work.display().to_string()],
    );

    let repo = open(&root);
    match repo.working_state(&Default::default()) {
        Err(Error::WorkingTreeUnavailable { why, root: named }) => {
            assert_eq!(
                why,
                fathom_ingest::error::WorkingTreeUnavailable::WorktreeMissing {
                    expected: work.display().to_string()
                },
                "the error names the directory it looked for"
            );
            assert_eq!(named, "elsewhere");
        }
        other => panic!("expected a missing-worktree error, got {other:?}"),
    }
    // The commits are still readable: only the working tree is gone.
    assert!(repo.head().expect("head").oid().is_some());
}

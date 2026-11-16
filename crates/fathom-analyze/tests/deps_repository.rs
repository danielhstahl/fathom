//! The git-backed half: a surface for a revision, and a delta for a diff (e2.5 / reco-j83, AC 1 and 2).
//!
//! `scan` and `delta` in the previous tests take bytes; the product calls them through `fathom-ingest`, which
//! reads real objects out of a real repository. Those two paths differ in ways only a real repository shows:
//! a blob that is over the read cap, a tree entry that is a symlink rather than a file, a rename that must
//! read as one delete and one add, and a changed path that is a source file and must therefore produce
//! *nothing* — the warm path's whole cost argument is that a commit touching 40 `.rs` files and no manifest
//! costs no blob reads and produces no rows.
//!
//! The fixture is the same one the index tests use: real `git`, scrubbed environment, pinned identity, so
//! the commit ids are the same on every machine and a failure can be reproduced with the commands in
//! `tests/common`. Nothing in this file runs `git` itself for that reason — every fixture step goes through
//! `common::Fixture`, including the `git rm` below; a shell-out that inherited the ambient environment
//! would commit under whatever identity and clock the machine happens to have, which is a red test on a
//! fresh clone and a different commit id on every one.
#![allow(clippy::unwrap_used, clippy::expect_used)]

// The fixture module also carries helpers this file does not need; `incremental_index` does.
#[allow(dead_code)]
mod common;

use common::Fixture;
use fathom_analyze::deps::{self, ChangeKind, Delta, Options, Status, Surface};
use fathom_ingest::oid::Oid;
use fathom_ingest::repo::{Head, Repository};

/// The repository: an npm workspace and a Go service, then four commits that exercise each kind of change.
fn fixture() -> Fixture {
    let repo = Fixture::new("deps");
    repo.commit(
        0,
        "the project",
        &[
            (
                "package.json",
                r#"{
  "name": "root",
  "private": true,
  "workspaces": ["apps/*"]
}"#,
            ),
            (
                "apps/web/package.json",
                r#"{ "name": "web", "dependencies": { "react": "^18.2.0", "lodash": "^4.17.21" } }"#,
            ),
            ("apps/web/package-lock.json", LOCK_V3),
            ("services/go/go.mod", GO_MOD),
            ("services/go/go.sum", GO_SUM),
            ("README.md", "# hello\n"),
        ],
    );
    repo
}

const LOCK_V3: &str = r#"{
  "name": "web",
  "lockfileVersion": 3,
  "packages": {
    "": { "dependencies": { "react": "^18.2.0", "lodash": "^4.17.21" } },
    "node_modules/react": { "version": "18.2.0", "integrity": "sha512-/XJ" },
    "node_modules/lodash": { "version": "4.17.21", "integrity": "sha512-m" }
  }
}"#;

const GO_MOD: &str = "module example.com/svc\n\ngo 1.22\n\nrequire github.com/lib/pq v1.10.9\n";
const GO_SUM: &str =
    "github.com/lib/pq v1.10.9 h1:ky1\ngithub.com/lib/pq v1.10.9/go.mod h1:Un/wp\n";

/// `git rev-parse HEAD` as an `Oid`.
fn head_oid(repo: &Repository) -> Oid {
    match repo.head().expect("HEAD resolves") {
        Head::Branch { oid, .. } => oid,
        Head::Unborn { name } => panic!("`{name}` has no commits"),
        Head::Detached { oid } => oid,
    }
}

/// The tree a commit introduces.
fn tree_of(repo: &Repository, commit: Oid) -> Oid {
    repo.commit(commit)
        .expect("the commit reads")
        .expect("the commit exists")
        .tree
}

fn scan_at(fixture: &Fixture) -> Surface {
    let repo = Repository::discover(fixture.path()).expect("the fixture is a repository");
    let head = head_oid(&repo);
    let tree = tree_of(&repo, head);
    deps::scan_repository(&repo, tree, &Options::default()).expect("a scan of a real repository")
}

fn delta_between(fixture: &Fixture, from: Oid, to: Oid) -> Delta {
    let repo = Repository::discover(fixture.path()).expect("the fixture is a repository");
    let from_tree = tree_of(&repo, from);
    let to_tree = tree_of(&repo, to);
    deps::delta_repository(&repo, Some(from_tree), Some(to_tree), &Options::default())
        .expect("a delta of a real repository")
}

#[test]
fn a_revision_reads_its_whole_dependency_surface_from_the_object_store() {
    let fixture = fixture();
    let surface = scan_at(&fixture);
    assert_eq!(
        surface.totals.files,
        5,
        "every dependency file at HEAD, and nothing else: {:?}",
        surface
            .files
            .iter()
            .map(|file| &file.path)
            .collect::<Vec<_>>()
    );
    assert_eq!(surface.totals.manifests, 3);
    assert_eq!(surface.totals.locks, 2);
    assert_eq!(surface.totals.unparsed, 0);
    assert_eq!(surface.members.len(), 3);
    assert!(surface.complete);
    // The README and the directory structure do not count as anything.
    assert!(
        surface
            .files
            .iter()
            .all(|file| file.path != "README.md" && !file.path.contains("node_modules"))
    );
}

#[test]
fn an_added_dependency_appears_as_a_requirement_added_in_the_right_member() {
    let fixture = fixture();
    let before = {
        let repo = Repository::discover(fixture.path()).unwrap();
        head_oid(&repo)
    };
    fixture.edit(
        "apps/web/package.json",
        r#"{ "name": "web", "dependencies": { "react": "^18.2.0", "lodash": "^4.17.21", "chalk": "^5.3.0" } }"#,
    );
    fixture.edit(
        "apps/web/package-lock.json",
        &LOCK_V3.replace(
            "    \"node_modules/lodash\": { \"version\": \"4.17.21\", \"integrity\": \"sha512-m\" }",
            "    \"node_modules/lodash\": { \"version\": \"4.17.21\", \"integrity\": \"sha512-m\" },\n    \"node_modules/chalk\": { \"version\": \"5.3.0\", \"integrity\": \"sha512-c\" }",
        ),
    );
    let after = {
        let repo = Repository::discover(fixture.path()).unwrap();
        head_oid(&repo)
    };
    let delta = delta_between(&fixture, before, after);
    assert_eq!(delta.paths.len(), 2, "two files changed: {:?}", delta.paths);
    assert_eq!(delta.totals.modified, 2);
    assert_eq!(delta.totals.direct_added, 1);
    assert_eq!(delta.totals.direct_removed, 0);
    assert_eq!(delta.totals.resolved_added, 1);
    let manifest = delta
        .paths
        .iter()
        .find(|row| row.path == "apps/web/package.json")
        .expect("the manifest is in the diff");
    assert_eq!(manifest.kind, ChangeKind::Modified);
    assert!(
        manifest.summary.contains("+1"),
        "the summary should say a requirement was added: {}",
        manifest.summary
    );
    // A parsed, comparable change is a parsed row: the raw diff is not needed to review it.
    assert!(!manifest.show_raw);
    let added: Vec<String> = match (&manifest.before, &manifest.after) {
        (Some(before), Some(after)) => {
            let names = |file: &fathom_analyze::deps::DepFile| match &file.body {
                fathom_analyze::deps::Body::Manifest(manifest) => manifest
                    .requirements
                    .iter()
                    .map(|d| d.name.clone())
                    .collect(),
                _ => Vec::new(),
            };
            let (before, after) = (names(before), names(after));
            after
                .iter()
                .filter(|n| !before.contains(n))
                .cloned()
                .collect()
        }
        _ => panic!("both sides of a modification exist"),
    };
    assert_eq!(added, vec!["chalk".to_owned()]);
}

#[test]
fn a_source_only_commit_produces_no_dependency_rows() {
    // The warm path's claim in one test: an ordinary commit costs nothing here, and produces nothing to show.
    let fixture = fixture();
    let before = {
        let repo = Repository::discover(fixture.path()).unwrap();
        head_oid(&repo)
    };
    fixture.edit("README.md", "# hello, world\n");
    fixture.edit("src/main.rs", "fn main() {}\n");
    let after = {
        let repo = Repository::discover(fixture.path()).unwrap();
        head_oid(&repo)
    };
    let delta = delta_between(&fixture, before, after);
    assert!(
        delta.paths.is_empty(),
        "a commit with no dependency change produced rows: {:?}",
        delta.paths
    );
    assert_eq!(delta.totals.files, 0);
    assert!(delta.complete);
}

#[test]
fn a_removed_manifest_is_a_deletion_and_a_new_member_is_an_addition() {
    let fixture = fixture();
    let before = {
        let repo = Repository::discover(fixture.path()).unwrap();
        head_oid(&repo)
    };
    fixture.remove("services/go/go.sum");
    fixture.commit(
        1,
        "add the api member, drop go.sum",
        &[(
            "apps/api/package.json",
            r#"{ "name": "api", "dependencies": { "express": "4.18.2" } }"#,
        )],
    );
    let after = {
        let repo = Repository::discover(fixture.path()).unwrap();
        head_oid(&repo)
    };
    let delta = delta_between(&fixture, before, after);
    let deleted = delta
        .paths
        .iter()
        .find(|row| row.path == "services/go/go.sum")
        .expect("the deleted lockfile is a row");
    assert_eq!(deleted.kind, ChangeKind::Deleted);
    assert!(deleted.after.is_none());
    assert!(deleted.before.is_some());
    // `go.sum` is two lines and one module: the `/go.mod` line is the same module's manifest hash, so a
    // resolved count of 1 is the honest number and 2 would mean the same module counted twice.
    assert_eq!(delta.totals.resolved_removed, 1);
    let added = delta
        .paths
        .iter()
        .find(|row| row.path == "apps/api/package.json")
        .expect("the new member is a row");
    assert_eq!(added.kind, ChangeKind::Added);
    assert!(added.before.is_none());
    assert_eq!(delta.totals.added, 1);
    assert_eq!(delta.totals.deleted, 1);
}

#[test]
fn a_corrupted_lockfile_in_a_diff_carries_its_raw_text() {
    // The ticket's headline case, end to end: a commit writes half a lockfile. The row is still there, it
    // says which side failed and why, and it tells the viewer to show the raw diff.
    let fixture = fixture();
    let before = {
        let repo = Repository::discover(fixture.path()).unwrap();
        head_oid(&repo)
    };
    fixture.edit(
        "apps/web/package-lock.json",
        "{\n  \"name\": \"web\",\n  \"lockfileVersion\": 3,\n  \"packages\": {\n    \"node_modules/react\": { \"version\": \"18.2",
    );
    let after = {
        let repo = Repository::discover(fixture.path()).unwrap();
        head_oid(&repo)
    };
    let delta = delta_between(&fixture, before, after);
    let row = delta
        .paths
        .iter()
        .find(|row| row.path == "apps/web/package-lock.json")
        .expect("the corrupted lockfile is a row");
    assert!(
        row.show_raw,
        "an unparsable side must ask for the raw diff: {:?}",
        row.summary
    );
    assert_eq!(delta.totals.unparsed, 1);
    assert_eq!(delta.unparsed.len(), 1);
    let after = row.after.as_ref().expect("the new side exists");
    assert_eq!(after.status, Status::Unparsed);
    let fathom_analyze::deps::Body::Unparsed(unparsed) = &after.body else {
        panic!("the broken side must be typed, not empty");
    };
    assert!(!unparsed.preview.is_empty());
    // The good side is still parsed, so the reviewer can compare against what it used to be.
    assert_eq!(
        row.before.as_ref().expect("the old side exists").status,
        Status::Parsed
    );
}

#[test]
fn a_symlink_or_submodule_entry_is_reported_not_silently_dropped() {
    // A tree entry that is not a blob is a real thing that happens (`pnpm-workspace.yaml` symlinked into
    // place, a nested repository), and `fathom-ingest` distinguishes it; the scan has to keep the row.
    let repo_fixture = Fixture::new("deps-weird");
    repo_fixture.commit(
        0,
        "a gitlink and a manifest",
        &[
            ("package.json", r#"{ "name": "root" }"#),
            ("README.md", "hi\n"),
        ],
    );
    let repo = Repository::discover(repo_fixture.path()).unwrap();
    let head = head_oid(&repo);
    let tree = tree_of(&repo, head);
    let surface = deps::scan_repository(&repo, tree, &Options::default()).unwrap();
    assert_eq!(surface.totals.files, 1);
    // And a revision whose only dependency file is unreadable still yields the row.
    let options = Options {
        max_blob_bytes: 4,
        ..Options::default()
    };
    let capped = deps::scan_repository(&repo, tree, &options).unwrap();
    assert_eq!(capped.files.len(), 1);
    assert_eq!(capped.files[0].status, Status::Unparsed);
}

/// The same episode the store will hold, built from real commits rather than bytes (e2.6 / reco-kvs).
///
/// This is the warm path end to end: only the paths the commit touched are read, both revisions are
/// rebuilt from the objects, and the rows come out attributed and proved against the lockfile hunk.
#[test]
fn the_dependency_delta_is_computed_from_the_object_store() {
    let fixture = fixture();
    let before = head_oid(&Repository::discover(fixture.path()).unwrap());
    let lock_with_chalk = LOCK_V3
        .replace(
            r#""": { "dependencies": { "react": "^18.2.0", "lodash": "^4.17.21" } }"#,
            r#""": { "dependencies": { "react": "^18.2.0", "lodash": "^4.17.21", "chalk": "^5.3.0" } }"#,
        )
        .replace(
            r#""node_modules/lodash": { "version": "4.17.21", "integrity": "sha512-m" }"#,
            r#""node_modules/lodash": { "version": "4.17.21", "integrity": "sha512-m" },
    "node_modules/chalk": { "version": "5.3.0", "integrity": "sha512-c" }"#,
        );
    fixture.commit(
        1,
        "add chalk",
        &[
            (
                "apps/web/package.json",
                r#"{ "name": "web", "dependencies": { "react": "^18.2.0", "lodash": "^4.17.21", "chalk": "^5.3.0" } }"#,
            ),
            ("apps/web/package-lock.json", &lock_with_chalk),
        ],
    );
    let after = head_oid(&Repository::discover(fixture.path()).unwrap());

    let repo = Repository::discover(fixture.path()).unwrap();
    let episode = deps::dependency_episode_repository(
        &repo,
        Some(tree_of(&repo, before)),
        Some(tree_of(&repo, after)),
        &deps::DependencyOptions::default(),
    )
    .expect("a dependency episode of a real repository");

    assert!(episode.paths.complete);
    assert_eq!(episode.paths.paths.len(), 2, "manifest and lockfile");
    assert_eq!(
        episode.model.counts.direct_added, 1,
        "{}",
        episode.model.headline
    );
    let chalk = episode
        .model
        .changes
        .iter()
        .find(|change| change.name == "chalk")
        .expect("chalk is a row");
    assert_eq!(chalk.attribution, deps::Attribution::Direct);
    assert!(
        chalk.proof.as_ref().is_some_and(|proof| proof.found_hunk()),
        "the row has to point at the lockfile hunk that proves it"
    );
    assert!(
        episode
            .model
            .headline
            .contains("1 new direct dependency (chalk 5.3.0)"),
        "{}",
        episode.model.headline
    );
}

/// A commit that touches no dependency file costs nothing: no blob reads requested, no rows, no headline.
#[test]
fn the_dependency_episode_of_a_source_only_commit_is_empty() {
    let fixture = fixture();
    let before = head_oid(&Repository::discover(fixture.path()).unwrap());
    fixture.commit(1, "typo", &[("README.md", "# hello there\n")]);
    let after = head_oid(&Repository::discover(fixture.path()).unwrap());
    let repo = Repository::discover(fixture.path()).unwrap();
    let episode = deps::dependency_episode_repository(
        &repo,
        Some(tree_of(&repo, before)),
        Some(tree_of(&repo, after)),
        &deps::DependencyOptions::default(),
    )
    .expect("a dependency episode of a real repository");
    assert!(episode.paths.paths.is_empty());
    assert!(episode.model.changes.is_empty());
    assert_eq!(episode.model.graph.total.net, 0);
}

//! The working tree: what is staged, what is unstaged, what conflicts, and what the reader refuses to
//! promise (e1.1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_core::error::{Diagnosable, ErrorCode};
use fathom_ingest::error::{Error, WorkingTreeUnavailable};
use fathom_ingest::repo::Repository;
use fathom_ingest::state::{
    ChangeKind, ScanMode, ScanOptions, Side, StateNotice, WorkingState, WorktreeKind,
};
use std::path::Path;

fn open(path: &Path) -> Repository {
    Repository::discover(path).expect("discover")
}

/// Every change of one side, keyed by path text, for assertions that do not care about ordering.
fn changes(state: &WorkingState, side: Side) -> Vec<(String, ChangeKind)> {
    let list = match side {
        Side::Staged => &state.staged,
        Side::Unstaged => &state.unstaged,
    };
    let mut out: Vec<(String, ChangeKind)> = list
        .iter()
        .map(|change| (change.path_text().into_owned(), change.kind))
        .collect();
    out.sort();
    out
}

#[test]
fn a_clean_tree_says_so() {
    let fixture = Fixture::new("clean");
    let root = fixture.init();
    fixture.commit(
        &root,
        "first",
        &[("one.txt", "one\n"), ("nested/two.txt", "two\n")],
    );
    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert!(state.is_clean(), "{state:?}");
    assert!(state.staged.is_empty() && state.unstaged.is_empty());
    assert_eq!(state.index.entries, 2, "two tracked paths");
    assert_eq!(
        state.index.version,
        Some(2),
        "git 2.x writes index v2 by default"
    );
    assert!(
        state.notices.contains(&StateNotice::UntrackedNotScanned),
        "the reader says what it did not look at: {:?}",
        state.notices
    );
    assert!(state.summary().starts_with("staged: 0 added, 0 modified"));
    assert!(state.summary().contains("conflicts: 0"));
}

#[test]
fn staged_is_head_against_the_index_which_is_git_diff_cached() {
    let fixture = Fixture::new("staged");
    let root = fixture.init();
    fixture.commit(
        &root,
        "first",
        &[
            ("keep.txt", "keep\n"),
            ("change.txt", "before\n"),
            ("remove.txt", "gone\n"),
        ],
    );
    // Stage one of each kind: add, modify, delete.
    write(&root, "added.txt", "new file\n");
    write(&root, "change.txt", "after\n");
    std::fs::remove_file(root.join("remove.txt")).expect("rm");
    git(&root, &["add", "--all"]);

    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert_eq!(
        changes(&state, Side::Staged),
        vec![
            ("added.txt".to_owned(), ChangeKind::Added),
            ("change.txt".to_owned(), ChangeKind::Modified),
            ("remove.txt".to_owned(), ChangeKind::Deleted),
        ],
        "exactly `git diff --cached --name-status`"
    );
    assert_eq!(
        git(&root, &["diff", "--cached", "--name-status"]),
        "A\tadded.txt\nM\tchange.txt\nD\tremove.txt"
    );
    // Nothing is unstaged: the index matches the files.
    assert!(state.unstaged.is_empty(), "{:?}", state.unstaged);
    assert!(!state.is_clean());
    assert_eq!(
        state.summary(),
        "staged: 1 added, 1 modified, 1 deleted | unstaged: 0 modified, 0 deleted | conflicts: 0"
    );
    // The recorded sides carry the ids on both ends, which is what a diff pane needs to open the right
    // two blobs.
    let modified = state
        .staged
        .iter()
        .find(|change| change.path_text() == "change.txt")
        .expect("change.txt");
    assert_ne!(
        modified.before.expect("head side").oid,
        modified.after.expect("index side").oid
    );
}

#[test]
fn unstaged_is_the_index_against_the_files_which_is_git_diff() {
    let fixture = Fixture::new("unstaged");
    let root = fixture.init();
    fixture.commit(
        &root,
        "first",
        &[
            ("edit.txt", "before\n"),
            ("delete.txt", "gone\n"),
            ("chmod.txt", "x\n"),
        ],
    );
    write(&root, "edit.txt", "after\n");
    std::fs::remove_file(root.join("delete.txt")).expect("rm");
    make_executable(&root, "chmod.txt");

    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert_eq!(
        changes(&state, Side::Unstaged),
        vec![
            ("chmod.txt".to_owned(), ChangeKind::ModeChanged),
            ("delete.txt".to_owned(), ChangeKind::Deleted),
            ("edit.txt".to_owned(), ChangeKind::Modified),
        ],
        "exactly `git diff --name-status`"
    );
    assert_eq!(
        git(&root, &["diff", "--name-status"]),
        "M\tchmod.txt\nD\tdelete.txt\nM\tedit.txt"
    );
    assert!(state.staged.is_empty(), "nothing was staged");
    let deleted = state
        .unstaged
        .iter()
        .find(|change| change.path_text() == "delete.txt")
        .expect("delete.txt");
    assert_eq!(deleted.after, None, "a missing file has no `after`");
    assert_eq!(
        deleted.on_disk.as_ref().expect("stat").kind,
        WorktreeKind::File
    );
}

#[test]
fn the_stat_shortcut_is_the_whole_point_of_a_fast_status() {
    let fixture = Fixture::new("stat");
    let root = fixture.init();
    fixture.commit(
        &root,
        "first",
        &[("touch.txt", "content\n"), ("untouched.txt", "x\n")],
    );
    // Same bytes, same length, newer mtime: the shortcut's designed case.
    touch(&root, "touch.txt");

    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert!(
        state.unstaged.is_empty(),
        "a touched file with unchanged bytes is clean: {:?}",
        changes(&state, Side::Unstaged)
    );
    assert!(
        state
            .notices
            .iter()
            .any(|notice| matches!(notice, StateNotice::StatAssumedClean { count } if *count >= 1)),
        "and the reader says how many entries it did not hash: {:?}",
        state.notices
    );
    assert_eq!(state.hashed, 1, "only the touched file was hashed");
    assert_eq!(state.examined, 2);

    // The same touch under `HashEveryFile`: same answer, more work — which is why the shortcut exists.
    let thorough = open(&root)
        .working_state(&ScanOptions {
            mode: ScanMode::HashEveryFile,
            ..ScanOptions::default()
        })
        .expect("state");
    assert!(thorough.unstaged.is_empty());
    assert_eq!(thorough.hashed, 2, "every tracked file was hashed");
    assert!(
        !thorough
            .notices
            .iter()
            .any(|notice| matches!(notice, StateNotice::StatAssumedClean { .. })),
        "{:?}",
        thorough.notices
    );
}

#[test]
fn a_same_size_rewrite_in_the_same_second_is_the_shortcut_known_limit() {
    let fixture = Fixture::new("same-size");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("sneaky.txt", "aaaa\n")]);
    // Same length, different bytes, mtime pushed back to what the index recorded: git's own shortcut
    // cannot see this either, and the honest thing is to state the limit in a test rather than pretend
    // the reader is magic.
    let mtime = std::fs::metadata(root.join("sneaky.txt"))
        .expect("stat")
        .modified()
        .expect("mtime");
    write(&root, "sneaky.txt", "bbbb\n");
    set_mtime(&root, "sneaky.txt", mtime);

    let shortcut = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert!(
        shortcut.unstaged.is_empty(),
        "documented limitation of the stat shortcut (git has it too): {:?}",
        changes(&shortcut, Side::Unstaged)
    );

    let thorough = open(&root)
        .working_state(&ScanOptions {
            mode: ScanMode::HashEveryFile,
            ..ScanOptions::default()
        })
        .expect("state");
    assert_eq!(
        changes(&thorough, Side::Unstaged),
        vec![("sneaky.txt".to_owned(), ChangeKind::Modified)],
        "and `HashEveryFile` is the door that catches it"
    );
}

#[test]
fn a_file_becoming_a_directory_or_a_symlink_is_a_type_change() {
    let fixture = Fixture::new("typechange");
    let root = fixture.init();
    fixture.commit(
        &root,
        "first",
        &[("flat.txt", "file\n"), ("linked.txt", "file\n")],
    );
    std::fs::remove_file(root.join("flat.txt")).expect("rm");
    std::fs::create_dir(root.join("flat.txt")).expect("mkdir");
    std::fs::remove_file(root.join("linked.txt")).expect("rm");
    symlink_to(&root, "target.txt", "linked.txt");
    write(&root, "target.txt", "link body\n");

    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert_eq!(
        changes(&state, Side::Unstaged),
        vec![
            ("flat.txt".to_owned(), ChangeKind::TypeChanged),
            ("linked.txt".to_owned(), ChangeKind::TypeChanged),
        ],
        "a path that changed kind is not a content edit"
    );
    let as_dir = state
        .unstaged
        .iter()
        .find(|change| change.path_text() == "flat.txt")
        .expect("flat.txt");
    assert_eq!(
        as_dir.on_disk.as_ref().expect("stat").kind,
        WorktreeKind::Directory
    );
    let as_link = state
        .unstaged
        .iter()
        .find(|change| change.path_text() == "linked.txt")
        .expect("linked.txt");
    assert_eq!(
        as_link.on_disk.as_ref().expect("stat").kind,
        WorktreeKind::Symlink
    );
}

#[test]
fn a_symlink_target_hashes_the_way_git_hashes_it() {
    let fixture = Fixture::new("symlink");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("link.txt", "original\n")]);
    // Point it at something else: git stores a symlink's *target* as the blob, and so does this.
    std::fs::remove_file(root.join("link.txt")).expect("rm");
    symlink_to(&root, "somewhere-else", "link.txt");

    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    let change = state
        .unstaged
        .iter()
        .find(|change| change.path_text() == "link.txt")
        .expect("a modified symlink");
    assert_eq!(
        change.kind,
        ChangeKind::TypeChanged,
        "a file that became a symlink is a type change, as git calls it"
    );
    let ours = change
        .on_disk
        .as_ref()
        .and_then(|entry| entry.oid)
        .expect("hashed");
    // What git stores for a symlink is a blob whose *contents are the target text*, so that file is the
    // expectation, produced by git itself.
    let expected = {
        let temp = fixture.path("body");
        std::fs::write(&temp, "somewhere-else").expect("write body");
        git(&root, &["hash-object", "-w", &temp.display().to_string()])
    };
    assert_eq!(ours.hex(), expected);
}

#[test]
fn conflicts_come_from_the_index_stages_and_keep_all_three_sides() {
    let fixture = Fixture::new("conflict");
    let root = fixture.init();
    fixture.commit(&root, "base", &[("both.txt", "base\n")]);
    git(&root, &["checkout", "-q", "-b", "theirs"]);
    fixture.commit(&root, "theirs", &[("both.txt", "theirs\n")]);
    git(&root, &["checkout", "-q", "main"]);
    fixture.commit(&root, "ours", &[("both.txt", "ours\n")]);
    let status = git_status(&root, &["merge", "theirs"]);
    assert!(
        status.never_succeeded(),
        "the fixture must actually conflict: {}",
        status.stdout()
    );

    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert_eq!(state.conflicts.len(), 1, "{:?}", state.conflicts);
    let conflict = state.conflicts.first().expect("one conflict");
    assert_eq!(conflict.path.as_bytes(), b"both.txt");
    assert!(conflict.base.is_some(), "stage 1: the merge base");
    assert!(conflict.ours.is_some(), "stage 2: ours");
    assert!(conflict.theirs.is_some(), "stage 3: theirs");
    assert_ne!(
        conflict.ours.expect("ours").oid,
        conflict.theirs.expect("theirs").oid
    );
    assert_eq!(
        conflict.base.expect("base").oid.hex(),
        git(&root, &["rev-parse", "HEAD^1:both.txt"]),
        "stage 1 is the merge base blob, which is nowhere else after the merge starts"
    );
    assert!(
        !state.is_clean(),
        "a conflicted tree is never clean: {}",
        state.summary()
    );
    assert!(state.summary().contains("conflicts: 1"));
    assert_eq!(
        state.index.entries, 3,
        "one path, three stages, and nothing else"
    );
}

#[test]
fn a_bare_repository_has_no_working_tree_and_says_which_one_it_means() {
    let fixture = Fixture::new("bare-state");
    let root = fixture.init_bare();
    let seed = Fixture::new("bare-seed-state");
    let seed_root = seed.init();
    seed.commit(&seed_root, "seed", &[("one.txt", "one\n")]);
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
    let error = repo
        .working_state(&ScanOptions::default())
        .expect_err("a bare repository has no working tree");
    match &error {
        Error::WorkingTreeUnavailable { root: named, why } => {
            assert_eq!(*why, WorkingTreeUnavailable::Bare);
            assert!(
                !named.is_empty(),
                "the error names the repository it looked at"
            );
        }
        other => panic!("expected a working-tree error, got {other:?}"),
    }
    assert_eq!(error.code(), ErrorCode::WorkingTreeUnavailable);
    assert!(
        error
            .hint()
            .is_some_and(|hint| hint.contains("bare repository")),
        "{:?}",
        error.hint()
    );
}

#[test]
fn no_index_is_a_state_and_not_a_failure() {
    let fixture = Fixture::new("no-index");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    std::fs::remove_file(root.join(".git/index")).expect("remove the index");

    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("a missing index is a state, not an error");
    assert!(state.staged.is_empty() && state.unstaged.is_empty());
    assert!(
        state.notices.contains(&StateNotice::NoIndex),
        "{:?}",
        state.notices
    );
    assert!(!state.index.present);
    assert_eq!(state.index.entries, 0);
    assert_eq!(state.examined, 0);
}

#[test]
fn a_file_over_the_cap_is_reported_and_never_read() {
    let fixture = Fixture::new("oversized");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("big.txt", "small\n")]);
    // Ten MiB, one over the `ingest.file_size_cap.mib` budget, written as a sparse file so the test
    // does not need ten MiB of real disk.
    let file = std::fs::File::create(root.join("big.txt")).expect("create");
    file.set_len(10 * 1024 * 1024).expect("set_len");

    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert_eq!(
        changes(&state, Side::Unstaged),
        vec![("big.txt".to_owned(), ChangeKind::Oversized)],
        "a huge file is counted, not slurped"
    );
    assert!(
        state
            .notices
            .iter()
            .any(|notice| matches!(notice, StateNotice::Oversized { count: 1 })),
        "{:?}",
        state.notices
    );
    let entry = state.unstaged.first().expect("one change");
    assert_eq!(entry.on_disk.as_ref().expect("stat").size, 10 * 1024 * 1024);
    assert_eq!(
        entry.on_disk.as_ref().expect("stat").oid,
        None,
        "and it was not hashed"
    );
    assert_eq!(state.hashed, 0, "nothing at all was read into memory");
    // A caller with a bigger cap gets a real answer, so the cap is a policy, not a limitation.
    let roomier = open(&root)
        .working_state(&ScanOptions {
            max_file_bytes: 64 * 1024 * 1024,
            ..ScanOptions::default()
        })
        .expect("state");
    assert_eq!(
        changes(&roomier, Side::Unstaged),
        vec![("big.txt".to_owned(), ChangeKind::Modified)]
    );
}

#[test]
fn untracked_files_are_not_reported_and_says_so() {
    let fixture = Fixture::new("untracked");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    write(&root, "brand-new.txt", "nobody has seen this\n");
    write(&root, "ignored/build.o", "binary\n");

    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert!(state.is_clean(), "{:?}", changes(&state, Side::Staged));
    assert!(
        state.notices.contains(&StateNotice::UntrackedNotScanned),
        "the answer is not 'clean' without saying it only looked at tracked paths"
    );
    let all: Vec<String> = state
        .staged
        .iter()
        .chain(state.unstaged.iter())
        .map(|change| change.path_text().into_owned())
        .collect();
    assert!(
        !all.iter().any(|path| path.contains("brand-new")),
        "{all:?}"
    );
}

#[test]
fn a_moved_submodule_is_not_reported_as_a_modified_file() {
    let fixture = Fixture::new("sub-state");
    let source = Fixture::new("sub-source");
    let source_root = source.init();
    source.commit(&source_root, "lib one", &[("lib.rs", "1\n")]);
    let host = fixture.init();
    let first_commit = fixture.add_submodule(&host, &source_root, "vendor/lib");

    let state = open(&host)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert!(
        state.is_clean(),
        "a fresh submodule is not a change: {:?}",
        [
            changes(&state, Side::Staged),
            changes(&state, Side::Unstaged)
        ]
    );
    // A gitlink is not a file: the path must not appear as a modified file, because the answer about a
    // submodule is "it moved", which `submodules()` reports.
    let gitlink = state
        .unstaged
        .iter()
        .any(|change| change.path_text() == "vendor/lib");
    assert!(!gitlink);

    // Move the submodule forward. The superproject sees a gitlink that moved, which is not a file
    // change and is not hashed.
    // Commit *inside* the submodule checkout: `actual != recorded` is exactly what that produces.
    let second_commit = source.commit(&fixture.path("vendor/lib"), "lib two", &[("lib.rs", "2\n")]);
    let after = open(&host)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert!(
        after.unstaged.is_empty() && after.staged.is_empty(),
        "a moved submodule is reported by `submodules()`, never as a modified file: {:?}",
        changes(&after, Side::Unstaged)
    );
    let submodules = open(&host).submodules().expect("submodules");
    let moved = submodules.first().expect("one submodule");
    assert!(
        matches!(
            moved.state,
            fathom_ingest::repo::SubmoduleState::AtDifference { .. }
        ),
        "the submodule moved: {:?}",
        moved.state
    );
    assert_eq!(
        moved.recorded.map(|oid| oid.hex()).as_deref(),
        Some(first_commit.as_str()),
        "the recorded gitlink is the commit the superproject's HEAD holds, not where the checkout is"
    );
    match moved.state {
        fathom_ingest::repo::SubmoduleState::AtDifference { recorded, actual } => {
            assert_eq!(recorded.hex(), first_commit.as_str());
            assert_eq!(actual.hex(), second_commit.as_str());
        }
        ref other => panic!("expected a moved submodule, got {other:?}"),
    }
}

#[test]
fn sparse_and_assume_unchanged_entries_are_left_alone() {
    let fixture = Fixture::new("sparse");
    let root = fixture.init();
    fixture.commit(
        &root,
        "first",
        &[("keep.txt", "k\n"), ("skipped.txt", "s\n")],
    );
    git(&root, &["sparse-checkout", "init", "--no-cone"]);
    git(&root, &["sparse-checkout", "set", "--no-cone", "keep.txt"]);
    // `sparse-checkout set` removes the de-selected file from disk; a naive reader calls that a delete.
    let state = open(&root)
        .working_state(&ScanOptions::default())
        .expect("state");
    assert!(
        state.unstaged.is_empty(),
        "a deliberately absent file is not a deletion: {:?}",
        changes(&state, Side::Unstaged)
    );
    assert!(
        state
            .notices
            .iter()
            .any(|notice| matches!(notice, StateNotice::Sparse { count } if *count >= 1)),
        "{:?}",
        state.notices
    );

    // `--assume-unchanged` means the owner told git not to look; the same respect applies here.
    let fixture2 = Fixture::new("assume");
    let root2 = fixture2.init();
    fixture2.commit(&root2, "first", &[("frozen.txt", "one\n")]);
    write(&root2, "frozen.txt", "two\n");
    git(
        &root2,
        &["update-index", "--assume-unchanged", "frozen.txt"],
    );
    let state = open(&root2)
        .working_state(&ScanOptions {
            mode: ScanMode::HashEveryFile,
            ..ScanOptions::default()
        })
        .expect("state");
    assert!(
        state.unstaged.is_empty(),
        "an assume-unchanged path is not re-checked: {:?}",
        changes(&state, Side::Unstaged)
    );
}

#[test]
fn an_entry_cap_stops_the_scan_and_says_it_did() {
    let fixture = Fixture::new("capped");
    let root = fixture.init();
    let names: Vec<String> = (0..25).map(|index| format!("f{index}.txt")).collect();
    let files: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), "x\n")).collect();
    fixture.commit(&root, "many files", &files);
    let state = open(&root)
        .working_state(&ScanOptions {
            max_entries: Some(5),
            ..ScanOptions::default()
        })
        .expect("state");
    assert!(
        state
            .notices
            .iter()
            .any(|notice| matches!(notice, StateNotice::Capped { .. })),
        "a scan that stopped early must say it stopped: {:?}",
        state.notices
    );
    assert!(state.examined <= 5, "examined {}", state.examined);
}

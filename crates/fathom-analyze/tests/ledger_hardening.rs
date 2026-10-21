//! The ledger reader's refusal to read what a clone points it at (`e7.8`, `reco-41a`).
//!
//! [`fathom_analyze::ledger::format::parse`] is the only place the ledger's files are opened, and it is
//! called by `review status`, `review verify`, the TUI's rail and the export — everything that answers
//! "what has this reviewer already decided". Two refusals are asserted here, both because the ledger is
//! *markdown a person trusts* and the bytes it is made of come from a directory a clone controls
//! ([threat model F-1, F-7](../../../docs/security/threat-model.md)):
//!
//! * a file that does not resolve inside the repository — `.fathom/reviews/notes.md -> ~/.ssh/id_rsa` is
//!   ordinary clone content, and reading it would put somebody's private file into a review report;
//! * a file that is not a ledger-shaped object at all: a directory, a device, or something enormous.
//!
//! Both have to be a [`Problem`] rather than a silence, because "the ledger is empty" and "the ledger was
//! refused" reach a reviewer as the same sentence otherwise, and one of them is an attack.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::File;
use std::path::Path;

use fathom_analyze::ledger::format::{MAX_LEDGER_BYTES, default_path, parse};

/// A repository root that is a real directory, unique per test so parallel tests cannot collide.
fn root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fathom-ledger-hardening-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(default_path(&dir).parent().expect("the ledger directory")).unwrap();
    dir
}

/// A file a clone could have shipped: a symlink from inside `.fathom/reviews/` to somewhere else.
#[cfg(unix)]
fn link_within_reviews(root: &Path, name: &str, target: &Path) -> std::path::PathBuf {
    let path = root.join(".fathom").join("reviews").join(name);
    std::os::unix::fs::symlink(target, &path).expect("create the symlink");
    path
}

#[cfg(not(unix))]
fn link_within_reviews(root: &Path, name: &str, target: &Path) -> std::path::PathBuf {
    let path = root.join(".fathom").join("reviews").join(name);
    std::os::windows::fs::symlink_file(target, &path).expect("create the link");
    path
}

fn complaints(path: &Path, root: &Path) -> Vec<String> {
    let parsed = parse(path, root);
    assert_eq!(
        parsed.entries.len(),
        0,
        "a refused ledger must contribute no entries: {:?}",
        parsed.entries
    );
    assert_eq!(parsed.problems.len(), 1, "one refusal, named");
    parsed.problems.iter().map(|p| p.what.clone()).collect()
}

#[test]
fn a_ledger_that_resolves_outside_the_repository_is_not_read() {
    let base = std::env::temp_dir().join(format!("fathom-ledger-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let repo = root("outside");
    let elsewhere = base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    // The forged ledger: a well-formed-looking file that is simply not in the repository.
    let forged = elsewhere.join("somebody-elses.md");
    std::fs::write(&forged, "# Fathom review ledger\n\nnot yours\n").unwrap();

    let path = link_within_reviews(&repo, "notes.md", &forged);
    let what = complaints(&path, &repo).remove(0);
    assert!(
        what.contains("outside the repository"),
        "the refusal has to say why: {what}"
    );

    std::fs::remove_dir_all(&base).ok();
    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn a_symlink_that_stays_inside_the_repository_is_still_a_ledger() {
    // The refusal is about *leaving*, not about symlinks: a store moved within the working tree with a link
    // is something people do on purpose, and refusing it would be refusing the repository.
    let repo = root("inside");
    let real = repo.join(".fathom").join("reviews").join("real.md");
    std::fs::write(
        &real,
        "# Fathom review ledger\n\n<!-- format: fathom-review/1 -->\n",
    )
    .unwrap();
    let path = link_within_reviews(&repo, "alias.md", &real);
    let parsed = parse(&path, &repo);
    assert!(
        parsed.problems.is_empty(),
        "an in-repository link is not a refusal: {:?}",
        parsed.problems
    );
    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn a_ledger_over_the_size_cap_is_refused() {
    // Sparse: the file's *length* is what the reader has to refuse, and writing 32 MiB of content to prove
    // it would only make the suite slow.
    let repo = root("size");
    let path = repo.join(".fathom").join("reviews").join("huge.md");
    let file = File::create(&path).unwrap();
    file.set_len(MAX_LEDGER_BYTES + 1).unwrap();
    let what = complaints(&path, &repo).remove(0);
    assert!(
        what.contains("over the"),
        "the size refusal names the cap: {what}"
    );
    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn a_directory_named_like_a_ledger_is_refused() {
    // `mkfifo` is not portable; a directory exercises the same "not a regular file" arm without needing a
    // device node or a shell.
    let repo = root("directory");
    let path = repo.join(".fathom").join("reviews").join("x.md");
    std::fs::create_dir_all(&path).unwrap();
    let what = complaints(&path, &repo).remove(0);
    assert!(
        what.contains("not a regular file"),
        "the refusal says what it is: {what}"
    );
    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn a_ledger_that_is_plain_valid_markdown_is_still_read() {
    // The control: every refusal above must not be a way of refusing the normal case.
    let repo = root("normal");
    let path = default_path(&repo);
    std::fs::write(
        &path,
        "# Fathom review ledger\n\n<!-- format: fathom-review/1 -->\n\nno entries yet\n",
    )
    .unwrap();
    let parsed = parse(&path, &repo);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    assert_eq!(
        parsed.entries.len(),
        0,
        "an empty ledger is zero entries, not a problem"
    );
    std::fs::remove_dir_all(&repo).ok();
}

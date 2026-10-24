//! `fathom analyze` answers "what does this episode now depend on?" — `e2.6` through `e2.11`'s seam.
//!
//! Epic 2's exit criterion is that every episode answers WHERE the change landed and WHAT it now depends
//! on, with every number traceable to its evidence. The `where` half has been answered since `e2.11`; this
//! file is the `what it depends on` half, and it is tested end to end for the reason `e2.11`'s own fixtures
//! could not do it: the answer is not a shape the model can invent, it is an `e2.6` measurement read out of
//! a real object database.
//!
//! Four claims, one per test:
//!
//! 1. An episode that moves a manifest **measures** the dependency delta: the section is `measured`, it
//!    carries the direct movement, and its question is gone from the gap list.
//! 2. An episode that touches no dependency file is still `measured` — an empty answer is an answer — and
//!    says the graph was not read rather than printing `0 → 0 packages` as though it had been.
//! 3. Every figure in every section of a real document carries evidence, and the document verifies itself.
//!    That is the clause of the exit criterion that says *every number*, not some of them.
//! 4. The dependency figures name a command the reviewer can run to check them, and it is one this binary
//!    answers.
//!
//! The fixture is three commits by three authors, so the spine splits one episode per commit instead of
//! guessing on a time gap: a test whose episode boundary depends on the clock is a test that fails on a
//! loaded CI runner.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

/// One commit of the fixture: who wrote it, when, and the message the episode is named by.
struct Turn {
    name: &'static str,
    email: &'static str,
    date: &'static str,
    message: &'static str,
}

const TURNS: [Turn; 3] = [
    Turn {
        name: "Ada",
        email: "ada@example.invalid",
        date: "1700000000 +0000",
        message: "add the crate: one observation and the store",
    },
    Turn {
        name: "Bob",
        email: "bob@example.invalid",
        date: "1700003600 +0000",
        message: "bump serde to 1.0.190 for the flatten fix",
    },
    Turn {
        name: "Cy",
        email: "cy@example.invalid",
        date: "1700007200 +0000",
        message: "add the second observation",
    },
];

const LOCK_BEFORE: &str = "version = 3\n\n[[package]]\nname = \"signal\"\nversion = \"0.1.0\"\n";
const LOCK_AFTER: &str = "version = 3\n\n[[package]]\nname = \"signal\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.190\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"7761121fc46e22defff54d2e3a0dbf23e3d6f53a62b4d348bc7bdee92ecc01f0\"\n";
const MANIFEST_BEFORE: &str =
    "[package]\nname = \"signal\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n";
const MANIFEST_AFTER: &str = "[package]\nname = \"signal\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1.0.190\"\n";

/// The three-commit fixture: a crate that depends on nothing, the commit that adds `serde`, and a
/// source-only commit that changes nothing dependency-shaped.
#[cfg(feature = "sqlite-bundled")]
fn repository() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-analyze-deps-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"], None);

    write(&dir, "Cargo.toml", MANIFEST_BEFORE);
    write(&dir, "Cargo.lock", LOCK_BEFORE);
    write(&dir, "src/lib.rs", "pub fn one() -> u32 { 1 }\n");
    commit(&dir, 0);

    write(&dir, "Cargo.toml", MANIFEST_AFTER);
    write(&dir, "Cargo.lock", LOCK_AFTER);
    commit(&dir, 1);

    write(
        &dir,
        "src/lib.rs",
        "pub fn one() -> u32 { 1 }\npub fn two() -> u32 { 2 }\n",
    );
    commit(&dir, 2);
    dir
}

/// Every episode the index holds, as `(id, summary)`, newest first — the order the rail prints them.
#[cfg(feature = "sqlite-bundled")]
fn episodes(root: &str) -> Vec<(String, String)> {
    let out = run(&["episodes", "--root", root, "--json"]);
    assert_eq!(0, code(&out), "episodes: {}", stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("`episodes --json` is JSON");
    document["episodes"]
        .as_array()
        .expect("an episode list")
        .iter()
        .map(|row| {
            (
                row["id"].as_str().unwrap_or_default().to_owned(),
                row["summary"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

/// One episode's delta document, from the verb every surface is the headless form of.
#[cfg(feature = "sqlite-bundled")]
fn delta(root: &str, episode: &str) -> serde_json::Value {
    let out = run(&["analyze", episode, "--root", root, "--json"]);
    assert_eq!(
        0,
        code(&out),
        "`fathom analyze {episode}`: {}",
        stderr(&out)
    );
    serde_json::from_str(&stdout(&out)).expect("`fathom analyze --json` is one JSON document")
}

/// The section with this id, or a panic: a missing section is a missing question, which is its own failure.
#[cfg(feature = "sqlite-bundled")]
fn section<'a>(document: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    document["sections"]
        .as_array()
        .expect("sections")
        .iter()
        .find(|section| section["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("no `{id}` section in the document"))
}

/// The figure with this label inside a section.
#[cfg(feature = "sqlite-bundled")]
fn figure<'a>(section: &'a serde_json::Value, label: &str) -> Option<&'a serde_json::Value> {
    section["figures"]
        .as_array()?
        .iter()
        .find(|figure| figure["label"].as_str() == Some(label))
}

/// The episode whose summary contains this word.
#[cfg(feature = "sqlite-bundled")]
fn episode_about(root: &str, word: &str) -> String {
    episodes(root)
        .into_iter()
        .find(|(_, summary)| summary.contains(word))
        .unwrap_or_else(|| panic!("no episode about `{word}`"))
        .0
}

#[cfg(feature = "sqlite-bundled")]
fn write(dir: &std::path::Path, path: &str, text: &str) {
    let full = dir.join(path);
    std::fs::create_dir_all(full.parent().expect("parent directory")).expect("create the parent");
    std::fs::write(full, text).expect("write the fixture file");
}

#[cfg(feature = "sqlite-bundled")]
fn commit(dir: &std::path::Path, which: usize) {
    git(dir, &["add", "-A"], Some(which));
    git(
        dir,
        &["commit", "-q", "-m", TURNS[which].message],
        Some(which),
    );
}

/// `git`, with no ambient configuration and — when `which` names a turn — that turn's author and date.
#[cfg(feature = "sqlite-bundled")]
fn git(cwd: &std::path::Path, args: &[&str], which: Option<usize>) {
    let mut command = Command::new("git");
    command
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1");
    if let Some(which) = which {
        let turn = &TURNS[which];
        command
            .env("GIT_AUTHOR_NAME", turn.name)
            .env("GIT_AUTHOR_EMAIL", turn.email)
            .env("GIT_COMMITTER_NAME", turn.name)
            .env("GIT_COMMITTER_EMAIL", turn.email)
            .env("GIT_AUTHOR_DATE", turn.date)
            .env("GIT_COMMITTER_DATE", turn.date);
    }
    let out = command.args(args).output().expect("git is on PATH");
    assert!(out.status.success(), "git {args:?}: {}", stderr_bytes(&out));
}

#[cfg(feature = "sqlite-bundled")]
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .output()
        .expect("the built fathom binary is runnable")
}

#[cfg(feature = "sqlite-bundled")]
fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[cfg(feature = "sqlite-bundled")]
fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[cfg(feature = "sqlite-bundled")]
fn stderr_bytes(out: &std::process::Output) -> String {
    stderr(out)
}

#[cfg(feature = "sqlite-bundled")]
fn code(out: &std::process::Output) -> i32 {
    out.status.code().expect("fathom exits with a code")
}

/// Claim 1: the episode that moved the manifest answers what the tree now depends on.
#[cfg(all(feature = "sqlite-bundled", feature = "deps-all"))]
#[test]
fn the_episode_that_moved_a_manifest_measures_what_the_tree_now_depends_on() {
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    let indexed = run(&["index", root]);
    assert_eq!(0, code(&indexed), "index: {}", stderr(&indexed));

    let id = episode_about(&root, "serde");
    let document = delta(&root, &id);
    let section = section(&document, "dependencies");
    assert_eq!(
        section["state"].as_str(),
        Some("measured"),
        "the dependency section answers from the two trees: {section}"
    );
    let known = section["figures"]
        .as_array()
        .expect("figures")
        .iter()
        .filter(|figure| figure["known"].as_bool() == Some(true))
        .count();
    assert!(known >= 4, "a measured section shows its work: {section}");

    let headline = figure(section, "headline").expect("the owning analysis' own sentence");
    let text = headline["value"].as_str().unwrap_or_default();
    assert!(
        text.contains("serde"),
        "the sentence names the package the manifest named: {text}"
    );
    assert_eq!(
        figure(section, "direct").expect("the direct count")["value"].as_str(),
        Some("1 added"),
        "one declared dependency appeared, and the section says exactly that"
    );
    let grown = figure(section, "graph").expect("the graph size");
    assert!(
        grown["value"]
            .as_str()
            .unwrap_or_default()
            .contains("net +1"),
        "the resolved graph grew by exactly the package the manifest asked for: {grown}"
    );
    assert!(
        !document["gaps"]
            .as_array()
            .expect("gaps")
            .iter()
            .any(|gap| gap["section"].as_str() == Some("dependencies")),
        "an answered question leaves the gap list: {}",
        document["gaps"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Claim 2: an episode with no dependency change answers *that*, and does not dress an unread graph up as
/// a zero.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn an_episode_with_nothing_dependency_shaped_says_so_instead_of_printing_a_zero() {
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    assert_eq!(0, code(&run(&["index", root])));

    let id = episode_about(&root, "second observation");
    let document = delta(&root, &id);
    let section = section(&document, "dependencies");
    assert_eq!(
        section["state"].as_str(),
        Some("measured"),
        "\"nothing dependency-shaped changed\" is an answer, not a gap: {section}"
    );
    let graph = figure(section, "graph").expect("the graph figure");
    assert!(
        graph["value"]
            .as_str()
            .unwrap_or_default()
            .contains("not read"),
        "an unread graph is reported as unread, never as `0 → 0`: {graph}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Claim 3: every figure of every section of a real document carries evidence, and the document verifies
/// itself — the exit criterion's *every number*, not some of them.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn every_figure_of_a_real_document_carries_its_evidence() {
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    assert_eq!(0, code(&run(&["index", root])));

    let all = episodes(&root);
    assert_eq!(3, all.len(), "three turns, three episodes: {all:?}");
    for (id, _) in all {
        let document = delta(&root, &id);
        assert_eq!(
            document["integrity"]["verified"].as_bool(),
            Some(true),
            "{id} does not verify itself: {}",
            document["integrity"]["problems"]
        );
        assert_eq!(
            section(&document, "dependencies")["state"].as_str(),
            Some("measured"),
            "{id}: the epic's criterion is *every* episode answers what it now depends on"
        );
        for section in document["sections"].as_array().expect("sections") {
            for figure in section["figures"].as_array().expect("figures") {
                let evidence = &figure["evidence"];
                let named = evidence["pointer"].is_string()
                    || evidence["run"].is_string()
                    || evidence["owner"].is_string();
                assert!(
                    named,
                    "{id}: `{}` carries no evidence at all: {figure}",
                    figure["label"].as_str().unwrap_or("?")
                );
                // An unknown figure is allowed — `blast` is one, until `e3.2`'s graph reaches this read —
                // but it may never be a blank: the reason is what separates "not measured" from
                // "nothing to measure" (ADR-001 §7.7).
                if figure["known"].as_bool() == Some(false) {
                    assert!(
                        figure["reason"]
                            .as_str()
                            .is_some_and(|reason| !reason.trim().is_empty()),
                        "{id}: an unknown figure with no reason is a blank: {figure}"
                    );
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Claim 4: the dependency figures name a command, and the command is one this binary answers.
#[cfg(all(feature = "sqlite-bundled", feature = "deps-all"))]
#[test]
fn the_dependency_figures_point_at_a_command_that_runs() {
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    assert_eq!(0, code(&run(&["index", root])));

    let id = episode_about(&root, "serde");
    let document = delta(&root, &id);
    let section = section(&document, "dependencies");
    let named = figure(section, "direct").expect("the direct count")["evidence"]["run"]
        .as_str()
        .expect("a hand-over figure names the command that reproduces it")
        .to_owned();
    let words: Vec<&str> = named.split(' ').skip(1).collect();
    let mut command = Command::new(env!("CARGO_BIN_EXE_fathom"));
    let out = command
        .args(&words)
        .arg("--root")
        .arg(root)
        .output()
        .expect("the named command is a real one");
    // A gate finding is exit 7 and a usage error is exit 2: what is asserted here is that the pointer is a
    // verb, not that the episode is clean.
    assert!(
        code(&out) != 2,
        "the evidence named a command this binary refuses: {named}\n{}",
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("dependencies"),
        "the named command is the dependency check: {named}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

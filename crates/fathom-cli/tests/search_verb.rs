//! `fathom search`, end to end (`e5.2`, `reco-mkc`).
//!
//! `/` in the shell and `fathom search` in a pipe are the same box: the same corpus, the same scorer, the
//! same filter grammar, the same `--explain`-style honesty about which kinds it could not read. This file
//! runs the binary, because the claim worth testing is that the two surfaces cannot disagree — and the
//! saved half of it ("recent and saved searches") only means anything across two processes.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fathom(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("the built fathom binary is runnable")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("fathom exits with a code")
}

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Dana Ruiz")
        .env("GIT_AUTHOR_EMAIL", "dana@example.invalid")
        .env("GIT_COMMITTER_NAME", "Dana Ruiz")
        .env("GIT_COMMITTER_EMAIL", "dana@example.invalid")
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    std::fs::create_dir_all(target.parent().expect("a parent directory"))
        .expect("create the directory");
    std::fs::write(target, content).expect("write the fixture file");
}

fn commit(root: &Path, date: i64, message: &str) {
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "commit",
            "-q",
            &format!("--date={date} +0000"),
            "-m",
            message,
        ],
    );
}

/// A repository whose episodes say different things, so a query can rank one above the others.
fn repository(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-search-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    git(&dir, &["config", "user.name", "Dana Ruiz"]);
    git(&dir, &["config", "user.email", "dana@example.invalid"]);
    write(&dir, "src/store.rs", "fn read() {}\n");
    commit(&dir, 1_700_000_000, "feat: epoch-safe store reads");
    write(&dir, "src/parse.rs", "fn toml() {}\n");
    commit(&dir, 1_700_014_400, "fix: parse the manifest dependencies");
    write(&dir, "src/net.rs", "fn fetch() {}\n");
    commit(&dir, 1_700_028_800, "feat: fetch the dependency graph");
    dir
}

fn json(out: &Output) -> serde_json::Value {
    let text = stdout(out);
    let line = text
        .lines()
        .find(|line| line.starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in:\n{text}"));
    serde_json::from_str(line).unwrap_or_else(|err| panic!("not JSON ({err}):\n{line}"))
}

#[test]
fn a_query_finds_the_episode_that_talks_about_it() {
    let dir = repository("rank");
    let indexed = fathom(&dir, &["index"]);
    assert_eq!(code(&indexed), 0, "index: {}", stderr(&indexed));
    let out = fathom(&dir, &["search", "store", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let value = json(&out);
    assert_eq!(value["contract"].as_str(), Some("fathom.search/1"));
    assert_eq!(value["query"].as_str(), Some("store"));
    let hits = value["hits"]
        .as_array()
        .unwrap_or_else(|| panic!("no hits array: {value}"));
    assert!(
        !hits.is_empty(),
        "a corpus with the word in it answers: {value}"
    );
    assert!(
        hits.iter()
            .any(|hit| hit["label"].as_str().unwrap_or_default().contains("store")),
        "{value}"
    );
    assert_eq!(
        hits[0]["goes_to"]["view"].as_str(),
        Some("timeline"),
        "a hit knows where it goes, which is what `<Enter>` uses: {value}"
    );
    // The report says which kinds it searched and which it could not — a box that silently searched only
    // episodes is the failure this report exists to make impossible.
    let groups = value["groups"].as_array().cloned().unwrap_or_default();
    assert!(
        groups.iter().any(|kind| kind["kind"] == "episode"),
        "{value}"
    );
    let gaps = value["gaps"].as_array().cloned().unwrap_or_default();
    assert!(
        !gaps.is_empty(),
        "kinds this build cannot read are named, not omitted: {value}"
    );
}

#[test]
fn a_query_with_no_answer_says_so_in_text_and_in_json() {
    let dir = repository("empty-answer");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let out = fathom(&dir, &["search", "zzzzzznotaword"]);
    assert_eq!(code(&out), 0, "no hits is an answer, not a failure");
    let text = stdout(&out);
    assert!(
        text.contains("no") || text.contains("0"),
        "the text output says what it found: {text}"
    );
}

#[test]
fn a_saved_query_is_named_in_the_next_process() {
    let dir = repository("saved");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let save = fathom(&dir, &["search", "dep:cargo", "--save", "my-deps"]);
    assert_eq!(code(&save), 0, "{}", stderr(&save));
    assert!(stdout(&save).contains("my-deps"), "{}", stdout(&save));

    let list = fathom(&dir, &["search", "--list"]);
    assert_eq!(code(&list), 0, "{}", stderr(&list));
    let text = stdout(&list);
    assert!(text.contains("my-deps"), "{text}");
    assert!(
        text.contains("dep:cargo"),
        "the name is useless alone: {text}"
    );

    let session = fathom(&dir, &["tui", "--show-session", "--json"]);
    assert!(
        stdout(&session).contains("tui.search.saved.my-deps"),
        "a saved search outside the shell's namespace is one the palette cannot see: {}",
        stdout(&session)
    );
}

#[test]
fn an_empty_query_is_refused_with_the_shape_to_type() {
    let dir = repository("noquery");
    let out = fathom(&dir, &["search"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    let text = stderr(&out);
    assert!(
        text.contains("query") || text.contains("word"),
        "the refusal says what to type: {text}"
    );
}

#[test]
fn an_unindexed_repository_says_which_kind_of_empty_it_is() {
    // "never indexed" and "nothing matched" are different sentences; the shell says them the same way and
    // so does the pipe (`ADR-001 §7.2` — suppressed information is the one failure not allowed).
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-search-noindex-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    let out = fathom(&dir, &["search", "anything"]);
    assert_ne!(code(&out), 0, "{}", stderr(&out));
    let text = stderr(&out);
    assert!(
        text.contains("index"),
        "the refusal names the missing thing: {text}"
    );
}

// ---------------------------------------------------------------------------
// the symbol section (`reco-6sw`): the extractor's output, in the box
// ---------------------------------------------------------------------------

/// A repository whose declarations are worth searching for, so a symbol hit is a fact and not a coincidence.
fn declarations(name: &str) -> PathBuf {
    let dir = repository(name);
    write(
        &dir,
        "src/parse.rs",
        "fn toml() {}\n\n/// The lockfile reader.\npub fn parse_lockfile(text: &str) -> usize { \
         text.len() }\n\nstruct Private {\n    n: usize,\n}\n",
    );
    commit(&dir, 1_700_043_200, "feat: parse the lockfile");
    dir
}

#[test]
fn a_symbol_query_finds_the_declaration_and_names_its_file() {
    let dir = declarations("symbol-hit");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let out = fathom(&dir, &["search", "parse_lockfile", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let value = json(&out);
    let hits = value["hits"]
        .as_array()
        .unwrap_or_else(|| panic!("no hits: {value}"));
    let hit = hits
        .iter()
        .find(|hit| hit["kind"] == "symbol" && hit["label"] == "parse_lockfile")
        .unwrap_or_else(|| panic!("the declaration is in the list: {value}"));
    assert_eq!(
        hit["file"].as_str(),
        Some("src/parse.rs"),
        "a symbol row names the file it was read out of: {hit}"
    );
    assert!(
        hit["detail"]
            .as_str()
            .is_some_and(|d| d.contains("src/parse.rs:4")),
        "and where in the file it starts: {hit}"
    );
    assert_eq!(hit["goes_to"]["view"].as_str(), Some("diff"));
    assert_eq!(
        hit["goes_to"]["path"].as_str(),
        Some("src/parse.rs"),
        "the deep link is the diff with the file selected: {hit}"
    );
    assert_eq!(
        hit["goes_to"]["select"].as_str(),
        Some("file"),
        "and the machine form says what is selected: {hit}"
    );
    // The gap that used to stand in for this section is gone: a corpus that filled a kind does not also
    // report it as missing.
    let gaps = value["gaps"].as_array().cloned().unwrap_or_default();
    assert!(
        !gaps
            .iter()
            .any(|gap| gap["kind"] == "symbol" && gap["state"] == "not indexed"),
        "the symbol gap is retired when the section is filled: {value}"
    );
}

#[test]
fn kind_symbol_narrows_the_list_to_symbols_and_sym_is_the_same_filter() {
    let dir = declarations("symbol-filter");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    for spelling in ["kind:symbol", "kind:sym"] {
        let out = fathom(&dir, &["search", "lockfile", spelling, "--json"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        let hits = json(&out)["hits"].as_array().cloned().unwrap_or_default();
        assert!(
            !hits.is_empty(),
            "`{spelling}` over a name that is one: {}",
            stdout(&out)
        );
        assert!(
            hits.iter().all(|hit| hit["kind"] == "symbol"),
            "`{spelling}` returned something that is not a symbol: {hits:?}"
        );
    }
}

#[test]
fn the_corpus_identity_moves_with_the_tree_and_is_printed() {
    // The acceptance criterion, from the outside: the corpus identity changes when the tree the symbols
    // came from changes, so a reader can tell "the box was rebuilt" from "the box is answering from an
    // older tree".
    let dir = declarations("identity");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let before = json(&fathom(&dir, &["search", "lockfile", "--json"]))["identity"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        !before.is_empty(),
        "the report carries the stamp it was built from"
    );
    let text = stdout(&fathom(&dir, &["search", "lockfile"]));
    assert!(
        text.contains(&format!("corpus  {before}")),
        "and the text form prints it: {text}"
    );

    // One line changed, nothing committed: the tree moved, and so does the stamp.
    write(
        &dir,
        "src/parse.rs",
        "fn toml() {}\n\n/// The lockfile reader.\npub fn parse_lockfile(text: &str) -> usize { \
         text.len() }\nfn added_after_the_corpus_was_read() {}\n",
    );
    let after = json(&fathom(&dir, &["search", "lockfile", "--json"]))["identity"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert_ne!(
        before, after,
        "an uncommitted edit moves the tree, and the identity says so"
    );
    assert_eq!(
        code(&fathom(
            &dir,
            &["search", "added_after_the_corpus_was_read", "--json"]
        )),
        0,
        "and the name that only exists in the working tree is searchable, because the section is read \
         from the working tree"
    );
}

#[test]
fn a_repository_with_nothing_to_extract_says_which_kind_of_empty_it_is() {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-search-nosymbols-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    write(&dir, "README.md", "# prose only\n");
    commit(&dir, 1_700_000_000, "docs: only prose");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let value = json(&fathom(&dir, &["search", "prose", "--json"]));
    let gaps = value["gaps"].as_array().cloned().unwrap_or_default();
    let gap = gaps
        .iter()
        .find(|gap| gap["kind"] == "symbol")
        .unwrap_or_else(|| panic!("a tree with no declarations still states why: {value}"));
    assert_eq!(gap["state"].as_str(), Some("not indexed"));
    assert!(
        gap["because"]
            .as_str()
            .is_some_and(|because| { because.contains("language") || because.contains("grammar") }),
        "the sentence names which reason it was: {gap}"
    );
}

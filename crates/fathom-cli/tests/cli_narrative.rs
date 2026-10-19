//! `fathom narrative`, driven as a binary (`e4.7`, `reco-2rx`).
//!
//! `crates/fathom-analyze/tests/narrative.rs` proves the composer's rules. A verb owns four different
//! things, and this file is where each of them is checked at the process boundary:
//!
//! * **the handle grammar** — an episode id, a rev, an oid prefix, and the two refusals when the name is
//!   nobody's (exit 1) or the index has not seen it (exit 12 with a remedy);
//! * **the machine contract** — `--json` puts exactly one parseable value on stdout, carrying
//!   `fathom-narrative/1`, and every claim in it cites something that exists in the same payload;
//! * **the three renderings agree** — text, `--markdown` and `--json` describe one composition, and the
//!   watermark and the self-check verdict are printed in all three;
//! * **the LLM answer is a fact about the build** — `--attribution` needs no index, prints the decision,
//!   and refuses the polish path by name.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The agent trailer that lets the intent ladder record something without a harness. It goes in the *same*
/// trailer block as `Agent-Intent:` — git reads the last paragraph as the trailer block, and a blank line
/// between the two is the difference between a recorded intent and a silent miss.
const AGENT_TRAILER: &str = "Co-authored-by: looprs <looprs@example.invalid>";

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
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

/// A repository with one agent turn of three commits and one human commit, indexed.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-narrative-cli-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"], &[]);
        git(&dir, &["config", "user.name", "Pi Agent"], &[]);
        git(
            &dir,
            &["config", "user.email", "agent@example.invalid"],
            &[],
        );
        let repo = Repo { dir };
        repo.commit(
            "chore: initialise harbour",
            &[("README.md", "# harbour\n")],
            0,
        );
        // One agent turn: three commits minutes apart, an intent trailer and a declared scope, which is the
        // trailer fallback `e4.2` documents and the narrative quotes. The turn starts three hours after the
        // initial commit because three commits ten minutes apart by one author are *one episode* — that is
        // ADR-003's rule, and the fixture needs it.
        for (minute, subject) in [
            (180, "feat: split the worker pool"),
            (186, "feat: route the clone off the UI thread"),
            (192, "test: cover the starvation case"),
        ] {
            repo.commit(
                &format!(
                    "{subject}\n\nAgent-Intent: split the worker pool so a slow clone cannot starve the rail\nAgent-Scope: src/pool/**, tests/pool/**\n{AGENT_TRAILER}"
                ),
                &[(
                    "src/pool/worker.rs",
                    &format!("// {subject}\nfn worker() {{}}\n"),
                )],
                minute,
            );
        }
        let out = fathom(&repo.dir, &["index", "."]);
        assert_eq!(code(&out), 0, "indexing failed: {}", stderr(&out));
        repo
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn write(&self, files: &[(&str, &str)]) {
        for (path, body) in files {
            let full = self.dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create the file's directory");
            }
            std::fs::write(full, body).expect("write the fixture file");
        }
    }

    fn commit(&self, message: &str, files: &[(&str, &str)], minute: i64) {
        self.write(files);
        for (path, _) in files {
            git(self.path(), &["add", "--", path], &[]);
        }
        git(
            self.path(),
            &["commit", "-q", "-m", message],
            &[("MINUTE", &minute.to_string())],
        );
    }
}

fn git(dir: &Path, args: &[&str], extra_env: &[(&str, &str)]) {
    let minute: i64 = extra_env
        .iter()
        .find(|(key, _)| *key == "MINUTE")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let date = format!("@{} +0000", 1_700_000_000 + minute * 60);
    let mut command = Command::new("git");
    command.args(args).current_dir(dir);
    // A pinned clock and no inherited identity: the same scrubbing the library fixtures use, so an episode
    // id here is the same on every machine.
    command.env("GIT_AUTHOR_DATE", &date);
    command.env("GIT_COMMITTER_DATE", &date);
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("HOME", dir);
    command.env("LC_ALL", "C");
    let out = command.output().expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_narrative_prints_and_verifies_itself() {
    let repo = Repo::new("print");
    let out = fathom(repo.path(), &["narrative", "HEAD"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("Read this first"), "{text}");
    assert!(
        text.contains("split the worker pool so a slow clone cannot starve the rail"),
        "the asker's own words are the narrative's, quoted: {text}"
    );
    assert!(text.contains("You cannot tell"), "{text}");
    assert!(text.contains("fathom-template/1"), "{text}");
    assert!(
        text.contains("verified:"),
        "the verdict is printed with the prose: {text}"
    );
    assert!(
        !text.contains("UNVERIFIED"),
        "a narrative that failed its own audit reached a user: {text}"
    );
}

#[test]
fn every_claim_in_the_json_cites_something_that_exists() {
    let repo = Repo::new("json");
    let out = fathom(repo.path(), &["narrative", "HEAD", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "`--json` prints one value on stdout");
    let payload: serde_json::Value = serde_json::from_str(lines[0]).expect("one JSON value");
    assert_eq!(payload["contract"], "fathom-narrative/1");
    assert_eq!(payload["generator"], "fathom-template/1");
    assert_eq!(payload["source_contract"], "fathom-delta/1");
    assert_eq!(payload["integrity"]["verified"], true, "{payload}");
    let claims = payload["claims"].as_array().expect("claims");
    let citations = payload["citations"].as_array().expect("citations");
    assert!(!claims.is_empty());
    let ids: Vec<&str> = citations
        .iter()
        .map(|row| row["id"].as_str().unwrap_or_default())
        .collect();
    for claim in claims {
        let cites = claim["cites"].as_array().expect("a claim cites");
        assert!(!cites.is_empty(), "an unattached assertion: {claim}");
        for cite in cites {
            assert!(
                ids.contains(&cite.as_str().unwrap_or_default()),
                "{claim} cites {cite}, which is not in the table"
            );
        }
        assert!(claim["weight"].is_number(), "every claim is priced");
    }
    // The citation table is contiguous — the same rule the library enforces, checked through the wire.
    for (index, id) in ids.iter().enumerate() {
        assert_eq!(*id, format!("c{}", index + 1), "ids drifted: {ids:?}");
    }
}

#[test]
fn the_markdown_is_the_artefact_body_and_names_its_evidence() {
    let repo = Repo::new("markdown");
    let out = fathom(repo.path(), &["narrative", "HEAD", "--markdown"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("## "), "{text}");
    assert!(text.contains("Evidence"), "{text}");
    assert!(text.contains("`fathom-delta/1`"), "{text}");
    // Every bracketed citation in the prose has a row in the table — the AC's "resolvable", checked on
    // the bytes a user would paste into a PR.
    for index in 1..=60 {
        let marker = format!("[e{index}]");
        let row = format!("`{marker}`");
        let cited = text.contains(&marker);
        let listed = text.contains(&row);
        let shape = if cited {
            "cited but not listed"
        } else {
            "listed but not cited"
        };
        assert_eq!(cited, listed, "`{marker}` is {shape}:\n{text}");
    }
}

#[test]
fn explain_prints_the_ranking_beside_the_prose_not_inside_it() {
    let repo = Repo::new("explain");
    let plain = fathom(repo.path(), &["narrative", "HEAD"]);
    let explained = fathom(repo.path(), &["narrative", "HEAD", "--explain"]);
    assert_eq!(code(&plain), 0, "{}", stderr(&plain));
    assert_eq!(code(&explained), 0, "{}", stderr(&explained));
    let plain_text = stdout(&plain);
    let explained_text = stdout(&explained);
    assert!(
        explained_text.contains("Ranking — why each sentence sits where it does"),
        "{explained_text}"
    );
    assert!(
        explained_text.starts_with(&plain_text),
        "`--explain` must add to the same narrative, not rewrite it"
    );
}

#[test]
fn an_unknown_handle_and_a_missing_index_are_different_refusals() {
    let repo = Repo::new("refuse");
    // Names nothing in the repository at all: the caller's typo, so exit 1.
    let typo = fathom(repo.path(), &["narrative", "not-a-rev-nor-an-id"]);
    assert_eq!(code(&typo), 1, "{}", stderr(&typo));
    assert!(
        stderr(&typo).contains("names no episode"),
        "{}",
        stderr(&typo)
    );
    // The grammar is printed with the refusal, because the reviewer is mid-task.
    assert!(
        stderr(&typo).contains("the grammar is"),
        "{}",
        stderr(&typo)
    );
    // Nothing named at all: a usage error, exit 2, with the verb that answers instead.
    let bare = fathom(repo.path(), &["narrative"]);
    assert_eq!(code(&bare), 2, "{}", stderr(&bare));
    assert!(stderr(&bare).contains("--attribution"), "{}", stderr(&bare));
}

#[test]
fn the_llm_answer_needs_no_index_and_refuses_by_name() {
    // `--attribution` answers a question about the build, so it runs anywhere — including a directory
    // that is not a repository at all, which is what a privacy audit runs it in.
    let outside = std::env::temp_dir().join(format!(
        "fathom-narrative-not-a-repo-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).expect("create the scratch directory");
    let out = fathom(&outside, &["narrative", "--attribution"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("llm_narrative      not shipped"), "{text}");
    assert!(text.contains("net-llm"), "{text}");
    for gate in 1..=5 {
        assert!(
            text.contains(&format!("  {gate}. ")),
            "gate {gate} of the decision is not printed:\n{text}"
        );
    }
    let json = fathom(&outside, &["narrative", "--attribution", "--json"]);
    assert_eq!(code(&json), 0, "{}", stderr(&json));
    let payload: serde_json::Value = serde_json::from_str(stdout(&json).trim()).expect("one value");
    assert_eq!(payload["shipped"], false);
    assert_eq!(payload["gates"].as_array().expect("gates").len(), 5);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn the_verb_is_advertised_where_a_person_would_look_for_it() {
    let help = fathom(std::path::Path::new("."), &["--help"]);
    assert_eq!(code(&help), 0, "{}", stderr(&help));
    assert!(
        stdout(&help).contains("narrative"),
        "the verb is not in --help:\n{}",
        stdout(&help)
    );
    let detail = fathom(std::path::Path::new("."), &["narrative", "--help"]);
    assert_eq!(code(&detail), 0, "{}", stderr(&detail));
    for flag in [
        "--json",
        "--markdown",
        "--explain",
        "--top",
        "--attribution",
    ] {
        assert!(
            stdout(&detail).contains(flag),
            "{flag} is not in the verb's help:\n{}",
            stdout(&detail)
        );
    }
}

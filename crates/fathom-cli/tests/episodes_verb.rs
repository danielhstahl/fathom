//! `fathom episodes`, driven as a binary (`reco-11b`).
//!
//! The defect was that nothing enumerated episodes headlessly at all: `fathom show` takes one revspec and
//! answers about one episode, so "the rail shows 213 episodes" was a sentence no script could check and the
//! acceptance run could only say it by watching a screen. ADR-006 §6.7 says every view has a headless
//! equivalent, and the rail — the reviewer's main list, the first thing the product draws — did not have one.
//!
//! So these tests run the binary, and what they assert is the shape of a *list*:
//!
//! * **the columns the ticket names** — id, time, author kind, `+add`/`−del`, path count, review state,
//!   session/turn — in the text form a human reads and the `--json` form a script reads, from one read;
//! * **the three counts that are not one count** — what the index holds, what the filters select, what the
//!   page printed — with the truncation said out loud instead of a silently short list;
//! * **the filters the rail filters with**, because a filtered count that only the shell can compute is a
//!   second reality;
//! * **the exit codes**, including the one that is *not* an error: a filter that matches nothing is an empty
//!   answer at exit 0, where a repository with no index is exit 12 with a remedy.
//!
//! The `--json` document's shape is `schemas/episodes.schema.json`, checked by the round trip in
//! `tests/schemas.rs`; this file is about what the verb decides, not about whether it validates.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Every test in this file is store-backed: the fixture builds its index with `fathom index`, and `fathom episodes`
// reads that index back — both halves of that are compiled only when a SQLite backend is on (ADR-002
// §6.1, §6.2), so on the headless `minimal` build the fixture cannot be built at all and the verb answers
// `FEATURE_ABSENT` (9) instead of answering. Gated in the spelling `tests/check_verb.rs` and
// `tests/show_trailers.rs` use; on every store-backed build the whole suite still runs.
#![cfg(feature = "sqlite-bundled")]

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
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The three episodes: two agent turns in one harness session (so a turn id and a session appear), and one
/// human commit with neither (so the *absence* is printed too, and is not a blank).
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(label: &str) -> Repo {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-episodes-cli-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"]);
        // A real reviewer's repository has an identity, and `fathom review sign` refuses to sign without
        // one — so the fixture has one rather than passing --identity at every call.
        git(&dir, &["config", "user.name", "Rue Reviewer"]);
        git(&dir, &["config", "user.email", "rue@example.invalid"]);
        // `.fathom/` is ignored in a real repository (ADR-004 §12), and a fixture without that ignores it
        // too happily: `git add -A` starts committing the store.
        let exclude = dir.join(".git").join("info").join("exclude");
        std::fs::create_dir_all(exclude.parent().expect("git made info/")).expect("info/");
        std::fs::write(&exclude, ".fathom/\n").expect("write the local exclude");

        commit(
            &dir,
            "Dana Ruiz",
            "dana@example.invalid",
            "2026-06-01T10:00:00+00:00",
            "human: add the reader",
            None,
            "reader",
        );
        commit(
            &dir,
            "Ada Agent",
            "ada-agent@example.invalid",
            "2026-06-02T10:00:00+00:00",
            "agent: wire the rail",
            Some(("sess-7", "2")),
            "wire",
        );
        commit(
            &dir,
            "Ada Agent",
            "ada-agent@example.invalid",
            "2026-06-03T10:00:00+00:00",
            "agent: widen the rail",
            Some(("sess-7", "3")),
            "widen",
        );
        let out = fathom(&dir, &["index", "."]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        Repo { dir }
    }

    fn episodes(&self, args: &[&str]) -> Output {
        let mut all = vec!["episodes"];
        all.extend_from_slice(args);
        fathom(&self.dir, &all)
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let out = self.episodes(args);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        serde_json::from_str(stdout(&out).trim())
            .unwrap_or_else(|err| panic!("stdout was one JSON value: {err}\n{}", stdout(&out)))
    }

    fn ids(&self, args: &[&str]) -> Vec<String> {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        self.json(&all)["episodes"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| row["id"].as_str().expect("an id").to_owned())
            .collect()
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn commit(
    dir: &Path,
    name: &str,
    email: &str,
    date: &str,
    subject: &str,
    turn: Option<(&str, &str)>,
    file: &str,
) {
    std::fs::write(
        dir.join(format!("{file}.rs")),
        format!("fn {file}() {{}}\n"),
    )
    .expect("write");
    git(dir, &["add", "-A"]);
    let message = match turn {
        Some((session, turn)) => {
            format!("{subject}\n\nAgent-Session: {session}\nAgent-Turn: {turn}")
        }
        None => subject.to_owned(),
    };
    let out = Command::new("git")
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_COMMITTER_NAME", name)
        .env("GIT_COMMITTER_EMAIL", email)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .args(["commit", "-q", "-m", &message])
        .output()
        .expect("git commit runs");
    assert!(
        out.status.success(),
        "commit {subject}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The text report: the columns the ticket names, in one row per episode, in the rail's order.
///
/// Parsed out of the shipped `fathom episodes` rather than out of a library function, because the thing the
/// ticket said was missing is a surface a person can read — "the rail shows 213 episodes" has to become a
/// line a person can point at.
#[test]
fn the_text_report_prints_every_column_the_ticket_names() {
    let repo = Repo::new("text");
    let out = repo.episodes(&[]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    let header = text
        .lines()
        .find(|line| line.starts_with("ID"))
        .unwrap_or_else(|| panic!("no column header:\n{text}"));
    for column in [
        "ID",
        "WHEN",
        "KIND",
        "+ADD",
        "-DEL",
        "FILES",
        "STATE",
        "SESSION/TURN",
        "SUMMARY",
    ] {
        assert!(
            header.contains(column),
            "no {column} column in the header: {header}"
        );
    }
    // One row per episode, newest first, and each row carries the row's whole story.
    let rows: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("ep-"))
        .collect();
    assert_eq!(rows.len(), 3, "three commits, three episodes:\n{text}");
    assert!(
        rows[0].contains("agent: widen the rail"),
        "the newest episode leads:\n{}",
        rows[0]
    );
    assert!(
        rows[0].contains("agent") && rows[0].contains("+1") && rows[0].contains("-0"),
        "author kind and the churn on one line: {}",
        rows[0]
    );
    assert!(
        rows[0].contains("sess-7:3"),
        "the turn id is session-qualified (ADR-003 §18.2 D5): {}",
        rows[0]
    );
    assert!(
        rows[0].contains("2026-06-03 10:00 UTC") && rows[0].contains("unread"),
        "the time and the review state are on the row: {}",
        rows[0]
    );
    // The human commit has no harness session, and that is printed as a word, not left blank.
    assert!(
        rows[2].contains("human") && rows[2].contains("no-session"),
        "an absence is stated: {}",
        rows[2]
    );
    // The header block answers "what did this read?" before any row appears.
    assert!(
        text.contains("3 episode(s)") && text.contains("selected   3 of 3"),
        "the scale and the page come first:\n{text}"
    );
}

/// Text and `--json` are one read rendered twice: same rows, same order, same numbers.
#[test]
fn the_json_rows_are_the_text_rows() {
    let repo = Repo::new("twice");
    let text = stdout(&repo.episodes(&[]));
    let report = repo.json(&["--json"]);
    let ids: Vec<String> = report["episodes"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| row["id"].as_str().expect("an id").to_owned())
        .collect();
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("ep-"))
        .collect();
    assert_eq!(lines.len(), ids.len(), "one line per row:\n{text}");
    for (line, id) in lines.iter().zip(&ids) {
        assert!(line.starts_with(id.as_str()), "{line} is not {id}");
    }
    // The columns the acceptance criteria name, on the machine row — asserted as present *and* typed, not
    // as `is_some()`, so a null where a number belongs fails here instead of in somebody's `jq`.
    let row = &report["episodes"][0];
    assert_eq!(row["author_kind"].as_str(), Some("agent"));
    assert_eq!(row["n_add"].as_i64(), Some(1));
    assert_eq!(row["n_del"].as_i64(), Some(0));
    assert_eq!(row["n_files"].as_i64(), Some(1));
    assert_eq!(row["paths"].as_array().map(Vec::len), Some(1));
    assert_eq!(row["review_state"].as_str(), Some("unread"));
    assert_eq!(row["session"].as_str(), Some("sess-7"));
    assert_eq!(row["turn"].as_str(), Some("sess-7:3"));
    assert_eq!(row["turn_id"].as_str(), Some("sess-7:3"));
    assert_eq!(row["last"].as_str(), Some("2026-06-03 10:00 UTC"));
    assert_eq!(row["created_utc_ms"].as_i64(), Some(1_780_480_800_000));
    assert_eq!(row["n_commits"].as_i64(), Some(1));
    assert_eq!(row["live"].as_bool(), Some(true));
    // The human commit states its absences as nulls, which is what a script can branch on.
    let human = &report["episodes"][2];
    assert_eq!(human["author_kind"].as_str(), Some("human"));
    assert!(human["session"].is_null() && human["turn"].is_null() && human["turn_id"].is_null());
    // The counts, the paging and the order are their own sections, not a guess from the row list.
    assert_eq!(report["index"]["episodes"].as_i64(), Some(3));
    assert_eq!(report["page"]["order"].as_str(), Some("last_utc desc, id"));
    assert_eq!(report["review_counts"]["unread"].as_i64(), Some(3));
    assert!(
        report["notes"].as_array().is_some_and(Vec::is_empty),
        "{report}"
    );
}

/// Three counts, three questions: the index holds, the filters select, the page printed.
///
/// The bug this exists to prevent is the quiet one — a list shorter than the index with nothing saying so,
/// and a script concluding the repository was quiet.
#[test]
fn a_page_that_is_not_the_whole_selection_says_so() {
    let repo = Repo::new("paging");
    let out = repo.episodes(&["--limit", "2"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(
        text.lines().filter(|line| line.starts_with("ep-")).count(),
        2
    );
    assert!(
        text.contains("! the page is what was printed: 3 selected, 2 shown"),
        "a short page is a stated short page:\n{text}"
    );
    let report = repo.json(&["--json", "--limit", "2"]);
    assert_eq!(report["page"]["returned"].as_i64(), Some(2));
    assert_eq!(report["page"]["matched"].as_i64(), Some(3));
    assert_eq!(report["page"]["truncated"].as_bool(), Some(true));
    assert_eq!(report["index"]["episodes"].as_i64(), Some(3));

    // Offsetting past the end is a different no, and gets its own sentence.
    let past = repo.episodes(&["--offset", "9"]);
    assert_eq!(code(&past), 0, "{}", stderr(&past));
    assert!(
        stdout(&past).contains("--offset 9 is past the end: the filters select 3"),
        "{}",
        stdout(&past)
    );
    // `--limit 0` is "the rest", not "none" — the store's own convention, kept.
    assert_eq!(
        repo.json(&["--json", "--limit", "0"])["page"]["returned"].as_i64(),
        Some(3)
    );
    // Paging is exhaustive: offset walks the whole selection without repeats or gaps.
    let mut walked: Vec<String> = repo.ids(&["--limit", "1", "--offset", "0"]);
    walked.extend(repo.ids(&["--limit", "1", "--offset", "1"]));
    walked.extend(repo.ids(&["--limit", "1", "--offset", "2"]));
    let whole = repo.ids(&[]);
    assert_eq!(walked, whole, "one at a time must equal all at once");
}

/// The filters are the rail's own — author, kind, state, path, date range — so a count taken here is the
/// count the shell drew, from the same `EpisodeQuery`.
#[test]
fn the_filters_are_the_rail_owns() {
    let repo = Repo::new("filters");
    assert_eq!(
        repo.ids(&["--kind", "human"]),
        vec![repo.ids(&[])[2].clone()]
    );
    assert_eq!(repo.ids(&["--author", "Dana Ruiz"]).len(), 1);
    assert_eq!(repo.ids(&["--path", "wire.rs"]).len(), 1);
    // The date grammar is `fathom search since:`'s, resolved to an instant and echoed both ways.
    let report = repo.json(&["--json", "--since", "2026-06-02", "--until", "2026-06-04"]);
    assert_eq!(report["page"]["matched"].as_i64(), Some(2), "{report}");
    assert_eq!(report["filter"]["since"].as_str(), Some("2026-06-02"));
    assert!(
        report["filter"]["since_utc_ms"].as_i64().is_some(),
        "the resolved bound is printed too: {report}"
    );
    assert_eq!(
        report["filter"]["text"].as_str(),
        Some("since=2026-06-02 until=2026-06-04")
    );
    // A filter that excludes is reported as excluded, not as an empty repository.
    let text = stdout(&repo.episodes(&["--kind", "human"]));
    assert!(
        text.contains("! the filters excluded 2 of the 3 episodes the index holds"),
        "{text}"
    );
    // And a filter that selects nothing is exit 0 with a reason: a script must be able to ask "how many
    // flagged episodes?" of a repository with none.
    let none = repo.episodes(&["--path", "nowhere.rs"]);
    assert_eq!(code(&none), 0, "{}", stderr(&none));
    assert_eq!(
        repo.json(&["--json", "--path", "nowhere.rs"])["page"]["matched"].as_i64(),
        Some(0)
    );
    assert!(stdout(&none).contains("no episode matches the filters"));
}

/// A review state the reviewer set is a state this verb lists — and the breakdown is printed even for the
/// states the filter excluded, which is the only way `--state unread` tells you what it left out.
#[test]
fn a_reviewed_episode_moves_state_and_the_breakdown_shows_it() {
    let repo = Repo::new("states");
    let ids = repo.ids(&[]);
    {
        let path = repo.path().join(".fathom").join("index.sqlite3");
        let mut store = fathom_store::spine::Store::open(&path).expect("the store opens");
        // `x` in the shell writes exactly this row (`ReviewState::Accepted`), so the CLI and the rail are
        // reading one table and cannot disagree about who has decided what.
        store
            .set_review(
                &ids[0],
                fathom_store::episode::ReviewState::Accepted,
                "Rue Reviewer",
                None,
                1,
            )
            .expect("stamping a marker works");
        store
            .set_review(
                &ids[1],
                fathom_store::episode::ReviewState::Read,
                "Rue Reviewer",
                None,
                1,
            )
            .expect("stamping a marker works");
    }
    let report = repo.json(&["--json"]);
    let states: Vec<&str> = report["episodes"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| row["review_state"].as_str().expect("a state"))
        .collect();
    assert_eq!(states, vec!["accepted", "read", "unread"], "{report}");
    assert_eq!(report["review_counts"]["accepted"].as_i64(), Some(1));
    assert_eq!(report["review_counts"]["read"].as_i64(), Some(1));
    assert_eq!(report["review_counts"]["unread"].as_i64(), Some(1));
    assert_eq!(
        report["review_counts"]["flagged"].as_i64(),
        Some(0),
        "all six keys, always"
    );

    let accepted = repo.json(&["--json", "--state", "accepted"]);
    assert_eq!(accepted["page"]["matched"].as_i64(), Some(1));
    assert_eq!(
        accepted["episodes"][0]["id"].as_str(),
        Some(ids[0].as_str())
    );
    // The breakdown survives its own filter: `--state accepted` still prints the split it filtered to get
    // one row, which is what makes the number auditable.
    assert_eq!(accepted["review_counts"]["unread"].as_i64(), Some(1));
    assert!(
        stdout(&repo.episodes(&["--state", "accepted"])).contains("1 unread · 1 read · 1 accepted"),
        "the states line prints the whole split"
    );
}

/// A flag value that names nothing is the reviewer's typo, and it is refused before anything is read.
#[test]
fn a_value_that_names_nothing_is_a_usage_error_with_the_names() {
    let repo = Repo::new("usage");
    for (args, needle) in [
        (vec!["--state", "never"], "unread, read, accepted"),
        (vec!["--kind", "sideways"], "agent, human, unknown"),
        (vec!["--since", "someday"], "is not a date"),
    ] {
        let out = repo.episodes(&args);
        assert_eq!(code(&out), 2, "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(needle),
            "{args:?} said {} — it should name what it wanted",
            stderr(&out)
        );
        assert!(
            stdout(&out).is_empty(),
            "a refusal prints no rows: {args:?}"
        );
    }
    // `--help` lists the flags, because the verb is only a contract if it can be read.
    let help = fathom(repo.path(), &["episodes", "--help"]);
    assert_eq!(code(&help), 0);
    for flag in [
        "--json",
        "--limit",
        "--offset",
        "--author",
        "--kind",
        "--state",
        "--path",
        "--since",
        "--until",
        "--include-dead",
    ] {
        assert!(stdout(&help).contains(flag), "{flag} missing from --help");
    }
}

/// No index is exit 12 with the remedy, a plain directory is exit 3, and neither prints a payload.
#[test]
fn an_index_that_is_not_there_refuses_with_its_code() {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let cold =
        std::env::temp_dir().join(format!("fathom-episodes-cold-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cold);
    std::fs::create_dir_all(&cold).expect("create the cold directory");
    git(&cold, &["init", "-q", "--initial-branch=main"]);
    let out = fathom(&cold, &["episodes", "--json"]);
    assert_eq!(code(&out), 12, "{}", stderr(&out));
    assert!(stdout(&out).is_empty(), "a refusal prints nothing to pipe");
    assert!(
        stderr(&out).contains("fathom index"),
        "the remedy is on the line after the error: {}",
        stderr(&out)
    );
    std::fs::remove_dir_all(&cold).ok();

    let plain = std::env::temp_dir().join(format!(
        "fathom-episodes-notrepo-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&plain);
    std::fs::create_dir_all(&plain).expect("create the plain directory");
    let out = fathom(&plain, &["episodes"]);
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    std::fs::remove_dir_all(&plain).ok();
}

/// `--include-dead` is the store's `include_dead`, and it is honest both ways: ghosts when there are
/// ghosts, and the sentence that says there are none when the pruner already cleared them.
#[test]
fn dead_episodes_appear_only_when_asked_for() {
    let repo = Repo::new("dead");
    // Nothing here was rewritten away, so asking for dead rows must not invent any — the empty case is the
    // one a CI script meets most often, and a verb that implied ghosts would be worse than one that says no.
    let report = repo.json(&["--json", "--include-dead"]);
    assert_eq!(report["filter"]["include_dead"].as_bool(), Some(true));
    assert_eq!(report["page"]["matched"].as_i64(), Some(3));
    assert!(
        report["episodes"]
            .as_array()
            .expect("rows")
            .iter()
            .all(|row| row["live"].as_bool() == Some(true)),
        "no ghost on a clean history: {report}"
    );
    assert!(
        report["notes"]
            .as_array()
            .is_some_and(|notes| notes.iter().any(|note| note
                .as_str()
                .is_some_and(|note| note.contains("no row belongs to no live commit")))),
        "the absence is stated: {report}"
    );
    assert!(
        !repo.json(&["--json"])["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().is_some_and(|n| n.contains("live commit"))),
        "a list that did not ask about ghosts should not talk about them"
    );
}

//! The ranking, out of the shipped binary (`reco-c05`).
//!
//! `reco-glv`'s acceptance step (b) — "find and read the three riskiest episodes" — is a question
//! about a screen, so it is tested as a screen: this file builds a repository, runs `fathom index`,
//! and asks `fathom --plain` (the same renderer the terminal gets, ADR-006 §6.7) what its queue looks
//! like. Nothing here hands a fixture to a view: the rows, the order, the numbers and the reasons all
//! come out of the store the indexer wrote, through the same read `fathom tui` uses.
//!
//! The repository is small on purpose. `tui_data.rs` already proves the 500-commit scale; what is
//! under test is *ordering*, and ordering is visible in twenty-one commits. Eighteen of them are the
//! ordinary filler of an agent's week; three are the shapes `tools/agent_fixture.py` plants — a
//! three-line workflow change, a short migration, and a big turn that deleted its own test file. The
//! fourth shape that has to lose is the 400-line documentation edit, which is the largest single-file
//! addition in the fixture and must not outrank any of them.
//!
//! The view is chosen through `session_state` (`tui.view`, `e1.11`), the same row `fathom tui` writes
//! when a reviewer presses `q` and quits, because `--plain` has no keystrokes.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// One row of `session_state`, written the way the shell writes it.
#[cfg(feature = "sqlite-bundled")]
fn seed(dir: &Path, rows: &[(&str, &str)]) {
    let path = dir.join(".fathom").join("index.sqlite3");
    let mut store = fathom_store::spine::Store::open(&path)
        .unwrap_or_else(|err| panic!("the store `fathom index` just wrote opens: {err}"));
    for (key, value) in rows {
        store
            .set_ui_state(key, Some(value), 1_700_000_000_000)
            .unwrap_or_else(|err| panic!("writing {key}: {err}"));
    }
}

/// One file in one commit: keep this many of its current lines, then add this many new ones. Keeping
/// fewer lines than it has *is* the deletion — fast-import diffs the tree, so a fixture that wants
/// `n_del` to mean something has to remove lines from a file that had them.
#[cfg(feature = "sqlite-bundled")]
#[derive(Clone, Copy)]
struct Op {
    path: &'static str,
    keep: usize,
    add: usize,
}

/// One commit, and therefore one episode: an hour apart, one hour is past the 30-minute gap rule
/// (ADR-003 §4), so each of these is its own episode rather than one merged turn.
#[cfg(feature = "sqlite-bundled")]
struct Shape {
    subject: &'static str,
    agent: bool,
    files: &'static [Op],
}

/// The fixture. See [`repository`].
#[cfg(feature = "sqlite-bundled")]
const SHAPES: &[Shape] = &[
    // Filler: the ordinary turns an agent lands in a week. None of them touches anything but source,
    // and none of them is the thing a reviewer would stop for.
    shape(
        "feat: narrow the booking query",
        &[op("src/harbour/store.py", 0, 24)],
    ),
    shape(
        "feat: cache the availability calendar",
        &[op("src/harbour/avail.py", 0, 18)],
    ),
    shape("fix: trim the footer", &[op("web/src/footer.tsx", 0, 6)]),
    shape(
        "feat: rate-limit headers",
        &[op("src/harbour/api.py", 0, 30)],
    ),
    // The largest single-file addition in the fixture, and the one that must not be mistaken for risk:
    // prose, written by a named human, with nothing to execute.
    human(
        "docs: a 400-line guide for the new hire",
        &[op("docs/onboarding.md", 0, 400)],
    ),
    shape(
        "feat: webhook validation",
        &[op("src/harbour/hooks.py", 0, 40)],
    ),
    shape(
        "feat: pricing rounding",
        &[op("src/harbour/pricing.py", 0, 22)],
    ),
    shape(
        "refactor: extract the worker",
        &[op("src/harbour/worker.py", 0, 34)],
    ),
    // The three-line workflow change. This is the row `reco-c05` exists to lift.
    shape(
        "ci: publish release artefacts with the live key",
        &[op(".github/workflows/release.yml", 0, 3)],
    ),
    shape(
        "feat: booking windows",
        &[op("src/harbour/booking.py", 0, 26)],
    ),
    shape("chore: tidy imports", &[op("src/harbour/api.py", 18, 4)]),
    shape(
        "feat: retry notification delivery",
        &[op("src/harbour/notify.py", 0, 28)],
    ),
    shape(
        "fix: report timezone",
        &[op("src/harbour/report.py", 0, 16)],
    ),
    shape(
        "feat: settlement ledger",
        &[op("src/harbour/ledger.py", 0, 45)],
    ),
    // The suite, so that there is a test surface to remove later: 300 lines, all of them real.
    shape(
        "test: the availability suite",
        &[op("tests/test_avail.py", 0, 300)],
    ),
    // The big turn that deleted its own test file: 520 lines of source for 300 lines of removed
    // coverage. The aggregate `n_del` the rail prints cannot tell this from a refactor; the test
    // surface can, and prints it as a reason.
    shape(
        "perf: rewrite the availability path",
        &[
            op("src/harbour/avail.py", 18, 520),
            op("tests/test_avail.py", 0, 8),
        ],
    ),
    shape("feat: export csv", &[op("src/harbour/export.py", 0, 20)]),
    shape("fix: tooltip copy", &[op("web/src/tooltip.tsx", 0, 5)]),
    // The short migration: twelve lines, one file, and the only row about the database in the whole
    // fixture. It has no down file beside it, but nothing in the spine can see that (reco-c05 prints
    // that as a blind spot rather than pretending to rank on it).
    shape(
        "migrations: drop customers.email_backup",
        &[op("db/migrations/0038_drop_email_backup.sql", 0, 12)],
    ),
    shape("feat: venue search", &[op("src/harbour/search.py", 0, 30)]),
    shape("feat: dashboard tiles", &[op("web/src/tiles.tsx", 0, 26)]),
];

#[cfg(feature = "sqlite-bundled")]
const fn op(path: &'static str, keep: usize, add: usize) -> Op {
    Op { path, keep, add }
}

#[cfg(feature = "sqlite-bundled")]
const fn shape(subject: &'static str, files: &'static [Op]) -> Shape {
    Shape {
        subject,
        agent: true,
        files,
    }
}

#[cfg(feature = "sqlite-bundled")]
const fn human(subject: &'static str, files: &'static [Op]) -> Shape {
    Shape {
        subject,
        agent: false,
        files,
    }
}

/// A repository holding [`SHAPES`], one commit per shape, an hour apart.
#[cfg(feature = "sqlite-bundled")]
fn repository() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-cli-risk-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(dir.as_path(), &["init", "-q", "--initial-branch=main", "."]);
    let mut child = Command::new("git")
        .args(["fast-import", "--done"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git fast-import is on PATH, as git itself is");
    child
        .stdin
        .take()
        .expect("a piped stdin")
        .write_all(&stream())
        .expect("fast-import takes the stream");
    let done = child.wait_with_output().expect("fast-import finishes");
    assert!(
        done.status.success(),
        "fast-import: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    let count = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(&dir)
        .output()
        .expect("git rev-list runs");
    assert_eq!(
        String::from_utf8_lossy(&count.stdout).trim(),
        SHAPES.len().to_string(),
        "one commit per shape, or the ranking is being tested against a different fixture"
    );
    dir
}

/// The fast-import stream. Three agent identities, each name carrying an `agent_markers` marker so
/// the classifier calls them agents (ADR-005); the documentation shape is the human's. Content is
/// filler — a spine ranking reads length and path, not meaning — but the *lengths* are the fixture.
#[cfg(feature = "sqlite-bundled")]
fn stream() -> Vec<u8> {
    const AGENTS: [(&str, &str); 3] = [
        ("claude", "claude@harness.invalid"),
        ("codex-bot", "codex@harness.invalid"),
        ("copilot", "copilot@harness.invalid"),
    ];
    let start: i64 = 1_700_000_000;
    let mut out = Vec::new();
    let mut mark: i64 = 0;
    let mut parent: Option<i64> = None;
    // The tree as fast-import has left it, so a later `keep` can mean "delete the rest".
    let mut tree: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (index, shape) in SHAPES.iter().enumerate() {
        let (name, email) = if shape.agent {
            AGENTS[index % AGENTS.len()]
        } else {
            ("Ada", "ada@example.invalid")
        };
        let when = start + i64::try_from(index).unwrap_or(i64::MAX) * 3_600;
        // One blob per path: a shape that touches two files must produce two diffs, or the fixture's
        // per-file numbers are fiction.
        let mut marks = Vec::with_capacity(shape.files.len());
        for file in shape.files {
            let had = tree.get(file.path).copied().unwrap_or(0);
            let keep = file.keep.min(had);
            let body: String = (0..keep + file.add)
                .map(|line| format!("line {line}\n"))
                .collect();
            tree.insert(file.path, keep + file.add);
            mark += 1;
            out.extend(format!("blob\nmark :{mark}\ndata {}\n", body.len()).into_bytes());
            out.extend(body.into_bytes());
            out.push(b'\n');
            marks.push((mark, file.path));
        }
        mark += 1;
        let commit = mark;
        out.extend(
            format!(
                "commit refs/heads/main\nmark :{commit}\ncommitter {name} <{email}> {when} +0000\n"
            )
            .into_bytes(),
        );
        let message = if shape.agent {
            format!(
                "{}\n\nAgent-Session: s-{}\nAgent-Turn: {index}\n",
                shape.subject, index
            )
        } else {
            format!("{}\n", shape.subject)
        };
        out.extend(format!("data {}\n", message.len()).into_bytes());
        out.extend(message.into_bytes());
        if let Some(parent) = parent {
            out.extend(format!("from :{parent}\n").into_bytes());
        }
        for (blob, path) in marks {
            out.extend(format!("M 100644 :{blob} {path}\n").into_bytes());
        }
        parent = Some(commit);
    }
    out.extend(b"done\n");
    out
}

/// One `git` call with the environment pinned, as `tui_data.rs` does it.
#[cfg(feature = "sqlite-bundled")]
fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(feature = "sqlite-bundled")]
fn fathom_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the built fathom binary is runnable")
}

#[cfg(feature = "sqlite-bundled")]
fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[cfg(feature = "sqlite-bundled")]
fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[cfg(feature = "sqlite-bundled")]
fn code(out: &Output) -> Option<i32> {
    out.status.code()
}

/// The queue pane's body out of a drawn frame: the first column below the pane rule.
#[cfg(feature = "sqlite-bundled")]
fn worklist(frame: &str) -> Vec<String> {
    frame
        .lines()
        .skip_while(|line| !line.starts_with('┌'))
        .skip(1)
        .map(|line| {
            line.split(['│', '┬'])
                .nth(1)
                .unwrap_or("")
                .trim_end()
                .to_owned()
        })
        .filter(|line| !line.is_empty() && !line.starts_with('└') && !line.starts_with('─'))
        .collect()
}

/// `(rank, id, risk)` for every row the queue drew, in the order it drew them.
#[cfg(feature = "sqlite-bundled")]
fn ranked_rows(frame: &str) -> Vec<(Option<u32>, String, Option<u32>)> {
    let mut rows: Vec<(Option<u32>, String, Option<u32>)> = Vec::new();
    for line in worklist(frame) {
        let Some(body) = line.strip_prefix('▸').or_else(|| line.strip_prefix(' ')) else {
            continue;
        };
        let mut fields = body.split_whitespace();
        let (Some(rank), Some(id)) = (fields.next(), fields.next()) else {
            continue;
        };
        // A row is `  1 ep-bdc agent …`; a reason line is `     · …`, and the pane's rule and strip
        // have no episode id in the second field at all.
        if id.len() != 6 || !id.starts_with("ep-") {
            continue;
        }
        let risk = body
            .split("risk ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.parse::<u32>().ok());
        rows.push((rank.parse::<u32>().ok(), id.to_owned(), risk));
    }
    rows
}

/// The acceptance question, asked of the binary: the queue is a ranked worklist, and its first three
/// rows are an answer a reviewer can act on.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_queue_answers_which_three_episodes_are_the_riskiest() {
    let dir = repository();
    let indexed = fathom_in(&dir, &["index", "."]);
    assert_eq!(code(&indexed), Some(0), "{}", stderr(&indexed));
    seed(&dir, &[("tui.view", "queue"), ("tui.size", "140x60")]);
    let frame = stdout(&fathom_in(&dir, &["--plain"]));
    let rows = ranked_rows(&frame);
    let text = worklist(&frame).join("\n");

    // One row per episode, and every one of them carries a number: the shipped build printed `?`.
    assert_eq!(
        rows.len(),
        SHAPES.len(),
        "one row per shape, all of them on one screen — the queue drew {} rows for {} commits:\n{frame}",
        rows.len(),
        SHAPES.len()
    );
    for (_, id, risk) in &rows {
        assert!(
            risk.is_some(),
            "{id} drew no risk number — every episode here touched files:\n{text}"
        );
    }
    let scored: Vec<u32> = rows.iter().filter_map(|(_, _, risk)| *risk).collect();
    assert!(
        !scored.contains(&0),
        "every episode here touched files, so nothing is unranked:\n{text}"
    );
    assert!(
        !frame.contains("risk ?"),
        "the rail's `?` column is answered now:\n{frame}"
    );
    let mut descending = scored.clone();
    descending.sort_unstable_by(|left, right| right.cmp(left));
    assert_eq!(
        scored, descending,
        "the queue is in descending risk order, not the store's chronological order"
    );
    // The rank column agrees with the order, which is the only way "which three" is readable.
    let ranks: Vec<Option<u32>> = rows.iter().map(|(rank, _, _)| *rank).collect();
    assert_eq!(
        ranks,
        (1..=u32::try_from(SHAPES.len()).unwrap())
            .map(Some)
            .collect::<Vec<_>>(),
        "rank 1, 2, 3 … down the pane"
    );

    // The three that matter are at the top, and each one's reason cites the file that put it there.
    let top_three = text.lines().take(6).collect::<Vec<_>>().join("\n");
    let line_of = |needle: &str| {
        text.lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("nothing on the queue mentions {needle}:\n{text}"))
    };
    assert!(
        top_three.contains("release.yml"),
        "the three-line CI change is in the first three rows:\n{text}"
    );
    assert!(
        top_three.contains("0038_drop_email_backup.sql"),
        "and so is the twelve-line migration:\n{text}"
    );
    assert!(
        top_three.contains("test surface shrank"),
        "and the turn that deleted 300 lines of tests behind a 520-line rewrite:\n{text}"
    );
    // The 400-line documentation edit is the largest addition in the fixture and is not in the top
    // three — which is the whole distinction this ticket was filed to make.
    assert!(
        !top_three.contains("onboarding.md"),
        "a 400-line documentation edit does not belong in the top three:\n{text}"
    );
    assert!(
        line_of("release.yml") < line_of("+400"),
        "the three-line workflow change is ahead of the 400-line documentation edit — that is the          whole distinction this ticket was filed to make:\n{text}"
    );

    // And the screen says what the screen is: the ranking named, its ceiling, and what it is not.
    assert!(
        frame.contains("order: spine-1"),
        "the strip names the ranking function:\n{frame}"
    );
    assert!(
        frame.contains("ranked of 21 held"),
        "and how many rows carry it:\n{frame}"
    );
    assert!(
        frame.contains("cannot see 7 things"),
        "and how much of the question it cannot answer:\n{frame}"
    );
    assert!(
        frame.contains(&format!("queue {} open", SHAPES.len())),
        "the status line counts the queue:\n{frame}"
    );
}

/// The same numbers on the timeline: the rail's risk column is the ranking's, not a placeholder.
///
/// `reco-glv` named both panes — the queue had no rows and the rail printed `?`. The rail is the view
/// a reviewer presses `t` for, so the column has to be filled there too, and the ordering of the
/// *rail* stays chronological on purpose (`e1.6`): the rail is a calendar, the queue is the triage.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_rail_prints_the_number_instead_of_the_placeholder() {
    let dir = repository();
    assert_eq!(code(&fathom_in(&dir, &["index", "."])), Some(0));
    seed(&dir, &[("tui.view", "timeline"), ("tui.size", "140x46")]);
    let frame = stdout(&fathom_in(&dir, &["--plain"]));
    assert!(!frame.contains("?  ⚑"), "no `?` risk column:\n{frame}");
    let numbered = frame
        .lines()
        .filter(|line| line.contains("risk ") || line.contains('●'))
        .filter(|line| line.chars().any(|c| c.is_ascii_digit()))
        .count();
    assert!(
        numbered > 5,
        "the rail drew risk numbers beside its rows:\n{frame}"
    );
}

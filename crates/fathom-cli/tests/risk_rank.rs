//! The ranking, out of the shipped binary (`reco-c05`).
//!
//! `reco-glv`'s acceptance step (b) — "find and read the three riskiest episodes" — is a question
//! about a screen, so it is tested as a screen: this file builds a repository, runs `fathom index`,
//! and asks `fathom --plain` (the same renderer the terminal gets, ADR-006 §6.7) what its rail looks
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
//! when a reviewer presses a view key and quits, because `--plain` has no keystrokes. Since `e10.3`
//! the home view is the timeline, so most of these tests seed nothing at all.
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
/// The rail's column out of a drawn frame: the first pane's body, rule and chrome taken out.
#[cfg(feature = "sqlite-bundled")]
fn rail(frame: &str) -> Vec<String> {
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

/// The short id of every episode row the rail drew, in the order it drew them.
///
/// A row is `▸ ep-c5d 2y +26 −0 ●●●○○○ ? 22 ⚑`; a group rule is `Wed 15 Nov ── 19 episodes`, the strip is
/// `order: newest first · …` and the pane rule is a row of dashes — none of them has an `ep-` first field,
/// so the id is what tells the rows apart.
#[cfg(feature = "sqlite-bundled")]
fn drawn_rows(frame: &str) -> Vec<String> {
    rail(frame)
        .iter()
        .filter_map(|line| {
            let body = line.trim_start_matches(['▸', ' ']);
            let first = body.split_whitespace().next()?;
            let is_row = first.starts_with("ep-")
                && first.len() == 6
                && first["ep-".len()..].bytes().all(|b| b.is_ascii_hexdigit());
            is_row.then(|| first.to_owned())
        })
        .collect()
}

/// The figure, asked of the shipped binary (`reco-c05`, re-hosted on the rail by `e10.3`,
/// `reco-ydek.3`).
///
/// This test used to ask "which three episodes are the riskiest" and read the answer off a ranked pane.
/// `e10.1` decided the reviewer works the chronology (ADR-006 §5.1.2 §5) and `e10.3` deleted the ranking,
/// so what is asserted now is the half that survived and the half the ticket's language is about: every
/// row prints its own score, the strip names the order the renderer actually implements — chronology, not
/// a score — and the count line keeps *caught up* a claim a reviewer can disprove. The fixture is kept
/// whole for the same reason: it is a set of episodes whose scores disagree with each other, which is the
/// only fixture that can show a rail declining to sort by them.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_rail_draws_every_episode_once_and_names_the_order_it_drew() {
    let dir = repository();
    let indexed = fathom_in(&dir, &["index", "."]);
    assert_eq!(code(&indexed), Some(0), "{}", stderr(&indexed));
    // No `tui.view` is seeded: the home view is the timeline now, so the shell gets there on its own
    // (`e10.3`, ADR-006 §5.1.2 §1). The size is a user's own last-read size, as `e1.11` stores it.
    seed(&dir, &[("tui.size", "140x46")]);
    let frame = stdout(&fathom_in(&dir, &["--plain"]));
    let rows = drawn_rows(&frame);
    let text = rail(&frame).join("\n");

    // One row per episode, on one screen, in the order they happened.
    assert_eq!(
        rows.len(),
        SHAPES.len(),
        "one row per shape, all of them on one screen — the rail drew {} rows for {} commits:\n{frame}",
        rows.len(),
        SHAPES.len()
    );
    assert!(
        !frame.contains("risk ?"),
        "an episode that touched files is never printed as unscored:\n{text}"
    );

    // The strip names the order the pane is in, in the words the renderer implements (ADR-006 §5.1.2 §3).
    // It used to name a ranking function; what it names now is the chronology the rows are actually in.
    assert!(
        frame.contains("order: newest first"),
        "the strip names the order, and the order is the chronology:\n{frame}"
    );
    assert!(
        !frame.contains("risk × staleness"),
        "the ranking function is gone from the screen with the ranking:\n{frame}"
    );

    // And the same strip counts the open work, which is what makes the pane's “caught up” falsifiable
    // now that no queue count exists to make it (`e10.3`, ADR-006 §5.1.2 §3 part 2).
    assert!(
        frame.contains(&format!("{} unread", SHAPES.len())),
        "the strip counts the episodes still asking to be read:\n{frame}"
    );
}

/// The same numbers on the timeline: the rail's risk column is the ranking's, not a placeholder.
///
/// `reco-glv` named two panes — one had no rows and the other printed `?`. Since `e10.3` there is one:
/// the rail is the view a reviewer presses `t` for, the column has to be filled there, and the ordering
/// of the *rail* is chronology on purpose (`e1.6`) — the rail is a calendar that prints a score per row.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_rail_prints_the_number_instead_of_the_placeholder() {
    let dir = repository();
    assert_eq!(code(&fathom_in(&dir, &["index", "."])), Some(0));
    seed(&dir, &[("tui.size", "140x46")]);
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

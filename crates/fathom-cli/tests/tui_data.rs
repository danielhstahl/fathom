//! The store→frame path, from outside the process (`reco-wzt`).
//!
//! The defect was that no pane ever received an episode: `fathom-cli` built every frame with
//! `episodes: None`, so a reviewer on an indexed repository was told in six panes that nothing was
//! available, and the only test that seemed to cover it handed a fixture to the view — which is true
//! of a rail and says nothing about whether the binary can fill one.
//!
//! So these tests run the binary. They build a real repository with `git fast-import`, index it with
//! `fathom index`, and ask the shipped `fathom --plain` (the same renderer the terminal gets, ADR-006
//! §6.7) what it draws. The fixture is 500 commits because that is the number Epic 1's exit criterion
//! and `reco-glv`'s walkthrough both use.
//!
//! One thing has to be arranged rather than typed: `--plain` has no keystrokes, and the shell opens on
//! the queue. The arrangement is a user's own — `tui.view` in `session_state` is what `fathom tui`
//! writes when a reviewer presses `t` and quits — so the test writes that row through the store's
//! `set_ui_state`, the same call `fathom tui`'s exit path uses, and then asks the binary what it draws.
//! The stored `tui.size` is the same trick (`e1.11`: a piped frame draws the layout the reviewer last
//! read at) and it is what lets the rail's strip print whole, which 80 columns truncates.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The fixture's size: Epic 1's own number, so what this asserts is what the epic promises.
#[cfg(feature = "sqlite-bundled")]
const COMMITS: usize = 500;

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

/// A repository with `count` commits, built by `git fast-import`.
///
/// fast-import rather than `count` `git commit` calls for the same reason `tools/agent_fixture.py`
/// uses it: a fixture that takes a minute to make is a fixture that gets skipped, and this one is
/// fixed-clock, fixed-author and fixed-path, so two runs of this file index the same repository and
/// the episode count is a constant rather than a flake.
///
/// The shape is the acceptance fixture's in miniature: three agent identities (each name carries an
/// `agent_markers` marker, so `e4.1`'s classifier calls them agents) and two human ones, one commit per
/// hour so the 30-minute gap rule opens an episode per commit, and `Agent-Session`/`Agent-Turn`
/// trailers on the agent commits, because a turn the harness named is the row the card has to show.
#[cfg(feature = "sqlite-bundled")]
fn repository(count: usize) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-cli-rail-{}-{}",
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
        .write_all(&stream(count))
        .expect("fast-import takes the stream");
    let done = child.wait_with_output().expect("fast-import finishes");
    assert!(
        done.status.success(),
        "fast-import: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    let log = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(&dir)
        .output()
        .expect("git rev-list runs");
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        count.to_string(),
        "the fixture is the size the acceptance run's is"
    );
    dir
}

/// The fast-import stream: `count` single-file commits, one hour apart, alternating three agents and
/// two humans (every fifth commit is human).
#[cfg(feature = "sqlite-bundled")]
fn stream(count: usize) -> Vec<u8> {
    const AGENTS: [(&str, &str); 3] = [
        ("claude", "claude@harness.invalid"),
        ("codex-bot", "codex@harness.invalid"),
        ("copilot", "copilot@harness.invalid"),
    ];
    const HUMANS: [(&str, &str); 2] = [
        ("Ada", "ada@example.invalid"),
        ("Grace", "grace@example.invalid"),
    ];
    // A fixed clock, so the day headers and the ages are the same in every run.
    let start: i64 = 1_700_000_000;
    let mut out = Vec::new();
    let mut mark: i64 = 0;
    let mut parent: Option<i64> = None;
    for i in 0..count {
        let agent = i % 5 != 4;
        let (name, email) = if agent {
            AGENTS[i % AGENTS.len()]
        } else {
            HUMANS[(i / 5) % HUMANS.len()]
        };
        let when = start + i64::try_from(i).unwrap_or(i64::MAX) * 3_600;
        mark += 1;
        let blob = mark;
        let body = format!("change {i}\n");
        out.extend(format!("blob\nmark :{blob}\ndata {}\n", body.len()).into_bytes());
        out.extend(body.into_bytes());
        out.push(b'\n');
        mark += 1;
        let commit = mark;
        out.extend(
            format!(
                "commit refs/heads/main\nmark :{commit}\ncommitter {name} <{email}> {when} +0000\n"
            )
            .into_bytes(),
        );
        // An agent commit carries the harness's own trailers (ADR-005): the session and the turn the
        // card prints, and the evidence `e1.3`'s `trailer` boundary rule reads.
        let message = if agent {
            format!(
                "feat: turn {i} for module {}\n\nAgent-Session: s-{}\nAgent-Turn: {i}\n",
                i % 20,
                i % 40
            )
        } else {
            format!("docs: note {i}\n")
        };
        out.extend(format!("data {}\n", message.len()).into_bytes());
        out.extend(message.into_bytes());
        if let Some(parent) = parent {
            out.extend(format!("from :{parent}\n").into_bytes());
        }
        out.extend(format!("M 100644 :{blob} src/module{}/change{i}.txt\n", i % 20).into_bytes());
        parent = Some(commit);
    }
    out.extend(b"done\n");
    out
}

/// One `git` call with the environment pinned, as `tests/cli.rs` does it.
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

/// The rail's rows, counted by the only thing only a row draws: the sparkline.
#[cfg(feature = "sqlite-bundled")]
fn rail_rows(frame: &str) -> usize {
    frame
        .lines()
        .filter(|line| line.contains('●') || line.contains("bin "))
        .count()
}

/// The acceptance criterion, from the outside: on a 500-commit indexed repository `fathom tui` shows
/// the episode rows in the rail, with no further work.
///
/// Every clause below is a fact the shipped binary had to produce by reading its own store — the row,
/// the author badge, the day header, the strip's count, and the card's `where` line, whose paths come
/// from the join this ticket added. Nothing here hands a fixture to the view.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_rail_lists_the_episodes_the_index_holds() {
    let dir = repository(COMMITS);
    let indexed = fathom_in(&dir, &["index", "."]);
    assert_eq!(code(&indexed), Some(0), "{}", stderr(&indexed));
    assert!(
        stdout(&indexed).contains("500 commit(s) walked, 500 new"),
        "{}",
        stdout(&indexed)
    );

    // "the reviewer pressed `t`, quit, and came back": see the module header.
    seed(&dir, &[("tui.view", "timeline"), ("tui.size", "200x60")]);
    let frame = fathom_in(&dir, &["--plain"]);
    assert_eq!(code(&frame), Some(0), "{}", stderr(&frame));
    let frame = stdout(&frame);

    // The rows. At 60 rows tall the window shows a good many of them, and every one is a store row.
    let rows = rail_rows(&frame);
    assert!(
        rows >= 30,
        "the rail drew {rows} rows over an index of 500 episodes:\n{frame}"
    );
    assert!(
        frame.contains(" agent ") && frame.contains(" human "),
        "the author kind is a store column, and the rail should show both it knows:\n{frame}"
    );
    assert!(
        frame
            .lines()
            .any(|line| line.contains("──") && line.contains("episodes")),
        "the rail groups by day, and a group header is a count of real rows:\n{frame}"
    );
    // The strip's count. Parsed rather than matched literally, because how many episodes 500 commits
    // segment into is the boundary rules' business and not this test's; what this test owns is the
    // promise that the rail holds every episode the index has, and says the same number twice doing it.
    let strip = frame
        .lines()
        .find(|line| line.contains("filter:") && line.contains(" of "))
        .unwrap_or_else(|| panic!("the strip carries the filter and the count:\n{frame}"));
    let (kept, held) = strip
        .split('·')
        .map(|bit| bit.split('│').next().unwrap_or(bit))
        .find_map(|bit| {
            let (kept, held) = bit.split_once(" of ")?;
            let number = |text: &str| text.trim().replace(' ', "").parse::<u64>().ok();
            Some((number(kept)?, number(held)?))
        })
        .unwrap_or_else(|| panic!("the strip should read `… · N of N …`: {strip}"));
    assert!(
        held >= 40,
        "the fixture should segment into a railful of episodes: {strip}"
    );
    assert_eq!(
        kept, held,
        "the rail holds {kept} of the index's {held}, and the strip should say both in one breath"
    );
    assert!(
        frame.contains("index epoch 1 (ready)"),
        "the batch names the epoch its rows came from:\n{frame}"
    );

    // The card, for the row under the cursor: the newest episode's subject, scale, a path, and the
    // member SHA — none of which the shell could have invented.
    assert!(
        frame.contains("ep-") && frame.contains("docs: note 499"),
        "the card describes the episode the cursor is on:\n{frame}"
    );
    assert!(
        frame.contains("where") && frame.contains("src/module"),
        "the `where` line is the paths the index recorded for that episode:\n{frame}"
    );
    assert!(
        frame.contains("members") && frame.contains("1 commit"),
        "the card states the member commits:\n{frame}"
    );

    // And the sentence that was true before this ticket is gone: `reco-1nu`'s rule is that a pane never
    // prints a remedy that is not real, and "nothing in this build can list them" stopped being real.
    for forbidden in [
        "nothing in this build can list them",
        "the episode read is e2.11's",
        "run `fathom index`",
    ] {
        assert!(
            !frame.contains(forbidden),
            "the frame still says {forbidden:?} while holding 500 episodes:\n{frame}"
        );
    }

    // And the delta half of the card is no longer a stated absence: `reco-tt4` made every rail row carry
    // an `EpisodeDelta`, so the card prints the model's seven sections and signs the contract it read.
    // That is the assertion a reviewer can audit: a frame that claims `fathom-delta/1` is a frame whose
    // numbers came from the one model, and the self-check beside it says the document agreed with itself.
    assert!(
        frame.contains("fathom-delta/1") && frame.contains("self-check"),
        "the card names the contract it rendered and the model's own verdict on itself:\n{frame}"
    );
    assert!(
        !frame.contains("not in this build"),
        "the scaffolded refusal is gone now that the model exists:\n{frame}"
    );
    std::fs::remove_dir_all(&dir).expect("clean up the fixture");
}

/// The cursor is addressed by identity (ADR-003 §2), so a selection read back out of the store lands on
/// the row it names — and the card then prints that episode's harness session, which only the read
/// could have supplied.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_selection_restores_onto_the_row_the_store_named() {
    let dir = repository(40);
    assert_eq!(
        code(&fathom_in(&dir, &["index", "."])),
        Some(0),
        "a small repository indexes as happily as a large one"
    );
    let wanted = {
        let path = dir.join(".fathom").join("index.sqlite3");
        let store = fathom_store::spine::Store::open(&path).expect("the store opens");
        store
            .episodes(&fathom_store::episode::EpisodeQuery::default())
            .expect("the episode read works")
            .into_iter()
            .find(|row| row.session_id.is_some())
            .expect("an agent turn in the fixture carries the session trailer it was written with")
    };
    seed(
        &dir,
        &[
            ("tui.view", "timeline"),
            ("tui.size", "200x60"),
            ("timeline.selection", &wanted.id),
        ],
    );
    let frame = stdout(&fathom_in(&dir, &["--plain"]));
    let (session, turn) = (
        wanted.session_id.as_deref().expect("checked above"),
        wanted
            .turn_id
            .as_deref()
            .expect("the same trailer block wrote both"),
    );
    assert!(
        frame.contains(&format!("session  session {session} · turn {turn}")),
        "the card should print the session and turn the harness named for the episode under the \
         cursor ({session}/{turn}):\n{frame}"
    );
    assert!(
        frame.contains(&wanted.summary)
            && frame.contains(&wanted.id.chars().take(6).collect::<String>()),
        "and it is that episode's card, not the newest one's:\n{frame}"
    );
    std::fs::remove_dir_all(&dir).expect("clean up the fixture");
}

/// A finished index with nothing in it gets its own sentence, now that "the timeline lists them" is a
/// remedy this build can honour — and one it should only offer when there is something to list.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn an_index_with_nothing_to_list_says_so_rather_than_pointing_at_the_timeline() {
    let dir = std::env::temp_dir().join(format!("fathom-cli-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    git(&dir, &["init", "-q", "--initial-branch=main", "."]);
    let out = fathom_in(&dir, &["--plain", "--fresh"]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    // Not even indexed yet: `fathom index` is still the honest remedy, and says so.
    assert!(
        stdout(&out).contains("run `fathom index` here"),
        "{}",
        stdout(&out)
    );
    let indexed = fathom_in(&dir, &["index", "."]);
    assert_eq!(code(&indexed), Some(0), "{}", stderr(&indexed));
    let frame = stdout(&fathom_in(&dir, &["--plain", "--fresh"]));
    assert!(
        frame.contains("index epoch 1 ready · 0 commits · 0 eps"),
        "the empty repository's frame states its own numbers:\n{frame}"
    );
    assert!(
        !frame.contains("`t` opens the timeline"),
        "there is nothing on the timeline to list:\n{frame}"
    );
    assert!(
        frame.contains("`fathom doctor`"),
        "zero over a finished index is a fact to check, not a quiet repository:\n{frame}"
    );
    std::fs::remove_dir_all(&dir).expect("clean up the fixture");
}

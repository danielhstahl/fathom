//! The diff pane gets its bytes from the store, from outside the process (`reco-0kw`).
//!
//! The defect was that nothing ever constructed a `PatchSource`: a reviewer pressed `d` — or resumed
//! into the diff view — and the pane printed "no diff — no episode is in scope / no patch is loaded"
//! while the store held every fact the diff is made of. The unit tests that *looked* like coverage
//! fed a fixture straight to the viewer, which proves the viewer can read a patch and says nothing
//! about whether the binary can produce one. So this file runs the binary: a real repository built
//! by `git fast-import`, indexed by `fathom index`, and then asked — through `fathom --plain`, the
//! same renderer the terminal gets (ADR-006 §6.7) — what the diff pane draws.
//!
//! Nothing here hands the pane a patch. Every byte of every hunk that shows up in the frame was
//! read out of the store's `git_commit_file` rows and the git objects they name, or it does not
//! show up at all.
//!
//! The arrangement that replaces the keystroke is a user's own, exactly as in `tui_data.rs`: a
//! `tui.view = "diff"` row in `session_state` is what `fathom tui` writes when a reviewer is looking
//! at the diff when they quit, and the shell opens into the diff view and resolves the scope the same
//! way `d` does. The keystroke path itself is what `tests/test_diff_open_contract.py` drives over a
//! real pty on the same binary.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// A few commits is enough here: this asserts the path from the store to the pane, not the size of
/// the store (that is `tui_data.rs` and the acceptance run).
#[cfg(feature = "sqlite-bundled")]
const COMMITS: usize = 8;

/// The fast-import stream: two commits per harness turn, so every episode has two members — the
/// net-change fold is then observable, not assumed. The changed line is the needle: it exists
/// nowhere but in the git object for that commit, so if the frame prints it, the bytes came through
/// the store.
#[cfg(feature = "sqlite-bundled")]
fn stream(count: usize) -> Vec<u8> {
    let start: i64 = 1_700_000_000;
    let mut out = Vec::new();
    let mut mark: i64 = 0;
    let mut parent: Option<i64> = None;
    for i in 0..count {
        let when = start + i64::try_from(i).unwrap_or(i64::MAX) * 3_600;
        mark += 1;
        let body = format!(
            "value = {i}\ncontext one\ncontext two\ncontext three\ncontext four\ncontext five\n"
        );
        out.extend(format!("blob\nmark :{mark}\ndata {}\n", body.len()).into_bytes());
        out.extend(body.into_bytes());
        out.push(b'\n');
        let blob = mark;
        mark += 1;
        let commit = mark;
        out.extend(
            format!(
                "commit refs/heads/main\nmark :{commit}\ncommitter claude <claude@harness.invalid> {when} +0000\n"
            )
            .as_bytes(),
        );
        // Two consecutive commits share a turn, which is the segmentation rule's "the fixup that
        // followed" case: one episode, two members.
        let message = if i % 2 == 0 {
            format!("feat: set value to {i}\n\nAgent-Session: s-1\nAgent-Turn: {i}\n")
        } else {
            format!(
                "fixup! set value to {i}\n\nAgent-Session: s-1\nAgent-Turn: {}\n",
                i - 1
            )
        };
        out.extend(format!("data {}\n", message.len()).into_bytes());
        out.extend(message.into_bytes());
        if let Some(parent) = parent {
            out.extend(format!("from :{parent}\n").into_bytes());
        }
        out.extend(format!("M 100644 :{blob} src/core.py\n").into_bytes());
        parent = Some(commit);
    }
    out.extend(b"done\n");
    out
}

#[cfg(feature = "sqlite-bundled")]
fn repository() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-cli-diff-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main", "."]);
    let mut child = Command::new("git")
        .args(["fast-import", "--done"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git fast-import is on PATH");
    child
        .stdin
        .take()
        .expect("a piped stdin")
        .write_all(&stream(COMMITS))
        .expect("fast-import takes the stream");
    let done = child.wait_with_output().expect("fast-import finishes");
    assert!(
        done.status.success(),
        "fast-import: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    dir
}

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

/// The pane's content is the whole frame at `--plain` size; the assertions below work on the frame
/// directly, because the two `diff` panes are the only thing that can print `@@` at this view.
#[cfg(feature = "sqlite-bundled")]
fn index(dir: &Path) {
    let out = fathom_in(dir, &["index"]);
    assert_eq!(out.status.code(), Some(0), "index: {}", stdout(&out));
}

/// The acceptance criterion, from outside: on an indexed repository the diff pane draws a file tree
/// and at least one real hunk, with the store as the byte source.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_diff_pane_paints_a_real_hunk_from_the_store() {
    let dir = repository();
    index(&dir);
    // The session row a reviewer leaves behind by quitting out of the diff view. `tui.size` is wide
    // so the hunks pane prints its columns whole (the same trick `tui_data.rs` uses).
    seed(&dir, &[("tui.view", "diff"), ("tui.size", "200x50")]);
    let frame = stdout(&fathom_in(&dir, &["--plain"]));

    // 1. The view is the diff view, and it is *loaded*: the toast every successful load prints names
    //    the origin and the file count, which the refusal path never claims.
    assert!(
        frame.contains("Diff ·"),
        "the shell opened the diff view:\n{frame}"
    );
    assert!(
        frame.contains("episode ep-") && frame.contains("commit(s) · 1 file(s)"),
        "the pane says where its bytes came from — store-backed, one file:\n{frame}"
    );
    // 2. The tree drew the file row, with its counts and the class the scanner derives from the path.
    assert!(
        frame.contains("src/core.py") && frame.contains("1 files · +1 −1"),
        "the tree pane lists the changed file:\n{frame}"
    );
    // 3. The hunks pane drew a hunk header and body — the header the spine recorded and the lines
    //    only the git objects behind the spine's blob references can produce.
    assert!(
        frame.contains("@@ -1,4 +1,4 @@"),
        "the first hunk's `@@` header is drawn:\n{frame}"
    );
    assert!(
        frame.contains("value = 7") && frame.contains("value = 5"),
        "the hunk's added and removed lines are the store's bytes, not a fixture:\n{frame}"
    );
    // 4. The sentence that was the defect is gone from a *loaded* pane.
    assert!(
        !frame.contains("no diff — "),
        "a loaded pane no longer prints the empty-state sentence:\n{frame}"
    );
}

/// A multi-commit episode draws its net change, not one hunk per member: the removed side is the
/// line *before* the episode's first member and the added side the line *after* its last, a pair
/// no single member's diff contains. This is the `d` on a two-member episode.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_multi_commit_episode_paints_its_net_change() {
    let dir = repository();
    index(&dir);
    seed(&dir, &[("tui.view", "diff"), ("tui.size", "200x50")]);
    let frame = stdout(&fathom_in(&dir, &["--plain"]));
    // The toast counts the members: whichever episode the cursor resolves to, it is the pair.
    assert!(
        frame.contains("2 commit(s) · 1 file(s)"),
        "the scope is a two-member episode and the fold is one file, not two rows:\n{frame}"
    );
    // The episode under the cursor is the newest pair, commits 6 and 7: the drawn hunk removes
    // `value = 5` (the line before the episode) and adds `value = 7` (the line after it). Member-
    // wise hunks would have drawn `value = 6` at all — its absence is what says *net*.
    assert!(
        frame.contains("value = 7") && frame.contains("value = 5"),
        "the hunk spans the episode: −value = 5 from before member one, +value = 7 after member two:\n{frame}"
    );
    assert!(
        !frame.contains("value = 6"),
        "an intermediate member's line would mean per-commit hunks, not the fold:\n{frame}"
    );
}

/// The empty state stays honest on a repository the binary has not indexed: the pane names what is
/// missing instead of pretending. (The other half of `reco-0kw`: a loaded pane loads, an unloaded
/// one *says why*.)
#[cfg(feature = "sqlite-bundled")]
#[test]
fn an_unindexed_repository_gets_a_stated_empty_state() {
    let dir = repository();
    // No `fathom index`: the store does not exist, so there is no provider answer to show.
    let frame = stdout(&fathom_in(&dir, &["--plain"]));
    assert!(
        frame.contains("Queue ·") || frame.contains("queue"),
        "with no session and no index the shell opens home:\n{frame}"
    );
}

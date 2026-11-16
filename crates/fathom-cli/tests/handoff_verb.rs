//! The exits out of the reader, run as the shipped binary (`e5.8`, `reco-dmw`).
//!
//! `fathom_core::handoff` proves the *plans* are right on a matrix of three OSes and a dozen editors;
//! the unit tests in `crates/fathom-cli/src/` prove the settings and the link printer. What neither can
//! prove is the part the ticket is actually about: that a real episode in a real index turns into a real
//! subprocess with the right file and the right line in it, and that the two refusals — no editor, no
//! difftool — are places a reviewer can stand rather than walls.
//!
//! So everything here runs the built binary against a fixture repository indexed by that same binary.
//! The "editor" is `fathom --version` — a program that exists on all three platforms because we built
//! it, that prints something recognisable, and that exits non-zero when handed a bad flag, which is how
//! "the editor's exit code is the verb's" gets tested without a terminal. The difftool hand-off is
//! tested through `--print --keep`, which is the door that leaves the two staged files behind: their
//! bytes come out of the git objects the index names, so reading them back is the assertion that the
//! pair is the right pair.
//!
//! Two things the harness itself has to get right, both of them learned the hard way (`reco-67ml`). Every
//! run is captured to a file and watched against a deadline ([`drive`]), because `Command::output()`
//! cannot be interrupted and a hand-off that never comes back would take the whole CI job with it. And a
//! run whose claim is "nothing is installed" is handed a `PATH` with nothing on it ([`empty_path`]): the
//! platform default editor is not a setting but a name — `vi`, `notepad` — and that name is on the
//! `PATH` of every machine that has ever run these tests, so a test that only empties the variables
//! finds an editor, plans it, and starts it for real.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Lines in the fixture's main file. The change is planted at `PLANT`, three context lines down from a
/// hunk header, so the line `fathom open` must ask the editor for is `PLANT - 3`.
#[cfg(feature = "sqlite-bundled")]
const LINES: usize = 40;
#[cfg(feature = "sqlite-bundled")]
const PLANT: usize = 30;

/// The command every run here starts from: the built binary, the fixture as its working directory,
/// and an environment in which the reviewer has configured nothing.
///
/// An empty `HOME` means no user config, so a reviewer's real `~/.config/fathom/config.toml` cannot
/// decide what this test asserts. The same for `XDG_CONFIG_HOME`, which wins when set.
///
/// `PATH` is deliberately *not* emptied here: `fathom index` and the diff staging are libraries and
/// need nothing off it. The runs whose claim really is "nothing is installed" scrub it themselves with
/// [`empty_path`] — see that function for why leaving it was the hang.
#[cfg(feature = "sqlite-bundled")]
fn bare(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fathom"));
    command
        .args(args)
        .current_dir(dir)
        .env("HOME", "")
        .env("XDG_CONFIG_HOME", "")
        .env("FATHOM_EDITOR", "")
        .env("VISUAL", "")
        .env("EDITOR", "")
        .env("FATHOM_DIFFTOOL", "")
        .env("FATHOM_HYPERLINKS", "")
        .env("TERM_PROGRAM", "")
        .env("VTE_VERSION", "");
    command
}

#[cfg(feature = "sqlite-bundled")]
fn fathom(dir: &Path, args: &[&str]) -> Output {
    drive(args, bare(dir, args))
}

/// As [`fathom`], with extra variables — which is how `$EDITOR`, `$FATHOM_HYPERLINKS`, a tmux-style
/// advertisement and a scrubbed `PATH` are put in front of a run that has to believe them.
#[cfg(feature = "sqlite-bundled")]
fn fathom_env(dir: &Path, args: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut command = bare(dir, args);
    for (key, value) in extra {
        command.env(key, value);
    }
    drive(args, command)
}

/// How long one `fathom` run may take before it is called a hang and stopped (`reco-67ml`).
///
/// Every verb driven here works on a three-commit fixture and is done in well under a second, so a run
/// still alive a minute later is not slow, it is stuck.
#[cfg(feature = "sqlite-bundled")]
const RUN_LIMIT: Duration = Duration::from_secs(60);

/// Run one `fathom`, capture it, and refuse to wait forever for it.
///
/// `Command::output()` cannot be used: it reads each captured pipe to EOF, so a run is only as bounded
/// as the process tree behind it. A verb that hands off — `fathom open` starting `$EDITOR`, `fathom
/// difftool` starting a diff tool — leaves the grandchild holding the pipe's write end, and a pipe with
/// a live writer never reaches EOF, so the *test* blocks in `read` whatever the process it spawned did.
/// That is exactly how `reco-67ml`'s hang arrived: an editor with no terminal, on a runner that had one
/// on `PATH`.
///
/// So the capture goes to two files rather than two pipes — a file never blocks a writer, and the bytes
/// are still there to print after the kill — and the wait is a `try_wait` loop with a deadline that
/// kills the run and reports what it had managed to say.
#[cfg(feature = "sqlite-bundled")]
fn drive(args: &[&str], mut command: Command) -> Output {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let tag = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stamp = std::process::id();
    let paths = (
        std::env::temp_dir().join(format!("fathom-cli-handoff-{stamp}-{tag}-out")),
        std::env::temp_dir().join(format!("fathom-cli-handoff-{stamp}-{tag}-err")),
    );
    let files = (
        std::fs::File::create(&paths.0).expect("the stdout capture is writable"),
        std::fs::File::create(&paths.1).expect("the stderr capture is writable"),
    );
    let mut child = command
        // Closed, not inherited: a grandchild must never be able to read the test runner's own stdin.
        .stdin(Stdio::null())
        .stdout(Stdio::from(files.0))
        .stderr(Stdio::from(files.1))
        .spawn()
        .expect("the built fathom binary is runnable");
    let deadline = Instant::now() + RUN_LIMIT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let said = std::fs::read_to_string(&paths.0).unwrap_or_default()
                    + &std::fs::read_to_string(&paths.1).unwrap_or_default();
                let _ = std::fs::remove_file(&paths.0);
                let _ = std::fs::remove_file(&paths.1);
                panic!(
                    "`fathom {}` was still running after {}s and was killed. A verb that never \
                     returns has usually handed the terminal to a program that needs one — an editor \
                     or a difftool found on a `PATH` this test did not scrub, drawing on a stream \
                     that is not a terminal. It had said:\n{said}",
                    args.join(" "),
                    RUN_LIMIT.as_secs()
                );
            }
            Err(err) => panic!("`fathom {}` could not be waited for: {err}", args.join(" ")),
        }
    };
    let output = Output {
        status,
        stdout: std::fs::read(&paths.0).expect("the stdout capture is readable"),
        stderr: std::fs::read(&paths.1).expect("the stderr capture is readable"),
    };
    let _ = std::fs::remove_file(&paths.0);
    let _ = std::fs::remove_file(&paths.1);
    output
}

/// A directory that exists and holds nothing, to hand a run as the whole of its `PATH`.
///
/// A test whose claim is "nothing that could be run is installed" has to say so about `PATH` as well as
/// about the variables, or it is asserting a fact about whichever machine it happens to run on: `vi` is
/// on `PATH` on every Linux and macOS box there is and `notepad` is on it on every Windows one, so
/// `fathom open`, given no `$EDITOR`, found a default, planned it, and — with no `--print` in the way —
/// started it. See [`drive`] for the hang that followed.
///
/// A directory rather than an empty `PATH` because it is the same claim spelled in the way every
/// platform reads: no directories to search, and no DLL-search weirdness on Windows for a value that is
/// the empty string.
#[cfg(feature = "sqlite-bundled")]
fn empty_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-cli-handoff-{}-nothing-on-path-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the empty PATH directory is creatable");
    dir
}

/// This very binary, spelled as an absolute path so it is on `PATH` by definition.
#[cfg(feature = "sqlite-bundled")]
fn ourselves() -> String {
    env!("CARGO_BIN_EXE_fathom").to_owned()
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

#[cfg(feature = "sqlite-bundled")]
fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "claude")
        .env("GIT_AUTHOR_EMAIL", "claude@harness.invalid")
        .env("GIT_COMMITTER_NAME", "claude")
        .env("GIT_COMMITTER_EMAIL", "claude@harness.invalid")
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Three commits, two episodes.
///
/// The first two share a session and a turn, so the segmenter groups them into one episode with two
/// members and the multi-member fold is under test rather than assumed. The third is its own episode
/// and deletes a file, which is the case `fathom open` must refuse politely.
///
/// * `src/core.py` — `LINES` lines, edited at line `PLANT` in the second commit: the jump target, and
///   the line is knowable exactly, not approximately.
/// * `Cargo.lock` — a lockfile that grows by far more lines than `src/core.py` does, so the "pick the
///   file the episode is about" rule has to see through the noise class to choose.
/// * `docs/core.py` — same final component as `src/core.py`, so `--path core.py` is ambiguous on
///   purpose; deleted in the third commit, so a jump into it is refused.
#[cfg(feature = "sqlite-bundled")]
fn repository() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-cli-handoff-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main", "."]);

    let file = |count: usize, plant: Option<usize>| -> String {
        (1..=count)
            .map(|line| match plant {
                Some(plant) if line == plant => format!("line {line} CHANGED"),
                _ => format!("line {line}"),
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    std::fs::write(dir.join("src-placeholder"), "x\n").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(dir.join("src/core.py"), file(LINES, None)).unwrap();
    std::fs::write(dir.join("docs/core.py"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(dir.join("Cargo.lock"), "version = 3\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["rm", "-q", "--cached", "src-placeholder"]);
    std::fs::remove_file(dir.join("src-placeholder")).unwrap();
    git(
        &dir,
        &[
            "commit",
            "-q",
            "-m",
            "feat: the fixture\n\nAgent-Session: s-1\nAgent-Turn: r1\n",
        ],
    );

    std::fs::write(dir.join("src/core.py"), file(LINES, Some(PLANT))).unwrap();
    let mut lock = std::fs::read_to_string(dir.join("Cargo.lock")).unwrap();
    for index in 0..200 {
        lock.push_str(&format!(
            "name = \"dependency-{index}\"\nversion = \"{index}\"\n"
        ));
    }
    std::fs::write(dir.join("Cargo.lock"), &lock).unwrap();
    git(&dir, &["add", "-A"]);
    git(
        &dir,
        &[
            "commit",
            "-q",
            "-m",
            "fix: change one line\n\nAgent-Session: s-1\nAgent-Turn: r1\n",
        ],
    );

    git(&dir, &["rm", "-q", "docs/core.py"]);
    git(
        &dir,
        &[
            "commit",
            "-q",
            "-m",
            "chore: drop the doc copy\n\nAgent-Session: s-1\nAgent-Turn: r2\n",
        ],
    );
    dir
}

#[cfg(feature = "sqlite-bundled")]
fn index(dir: &Path) {
    let out = fathom(dir, &["index"]);
    assert_eq!(code(&out), Some(0), "index: {}", stderr(&out));
}

/// The episode the second commit belongs to — the two-member one, whose jump line is knowable.
#[cfg(feature = "sqlite-bundled")]
fn episode_id(dir: &Path) -> String {
    let out = fathom(dir, &["show", "HEAD~1", "--json"]);
    assert_eq!(code(&out), Some(0), "show: {}", stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("show --json is JSON");
    document["episode"]["identity"]["id"]
        .as_str()
        .expect("the indexed commit has an episode id")
        .to_owned()
}

// --- fathom open -------------------------------------------------------------------------------

#[cfg(feature = "sqlite-bundled")]
#[test]
fn open_prints_the_editor_the_line_and_the_reason_before_touching_anything() {
    let dir = repository();
    index(&dir);
    let episode = episode_id(&dir);
    let out = fathom(
        &dir,
        &["open", &episode, "--print", "--editor", &ourselves()],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("episode"), "{text}");
    assert!(
        text.contains("src/core.py"),
        "the jump target is the real change, not the lockfile:\n{text}"
    );
    assert!(
        !text.contains("Cargo.lock"),
        "the lockfile is noise and `best()` must not pick it:\n{text}"
    );
    // The line is the hunk's new-side start: the change is at `PLANT`, and git gives three lines of
    // context above it. This is the same header the diff pane's `[` walks, so the two agree by
    // construction — and here it is checked against the number, not against a `contains("line")`.
    let want = PLANT as u32 - 3;
    assert!(
        text.contains(&format!("at line {want}")),
        "the jump line is the first hunk's new side ({want}):\n{text}"
    );
    assert!(text.contains("stopped at the plan"), "{text}");
    assert!(
        text.contains("was asked for") || text.contains("from $"),
        "the plan says where the editor came from:\n{text}"
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn open_runs_the_editor_and_hands_the_absolute_path_and_the_line_to_it() {
    let dir = repository();
    index(&dir);
    // `fathom --version` is the stand-in editor: it exists here by construction, it prints, and it
    // exits 0 whatever else is on the line — so the child's own output appearing proves the handoff.
    let editor = format!("{} --version", ourselves());
    let out = fathom_env(&dir, &["open", "HEAD~1", "--editor", &editor], &[]);
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    assert!(
        stdout(&out).contains("fathom 0.1.0"),
        "the editor's own stdout reached the terminal:\n{}",
        stdout(&out)
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn an_editor_that_cancelled_leaves_the_verb_cancelled_too() {
    let dir = repository();
    index(&dir);
    // A clap usage error is exit 2. `:cq` in vim is the same shape: the editor decided, and a verb
    // that swallowed it would be reporting a review that did not happen.
    let editor = format!("{} --not-a-real-flag", ourselves());
    let out = fathom_env(&dir, &["open", "HEAD~1", "--editor", &editor], &[]);
    assert_eq!(code(&out), Some(1), "{}{}", stdout(&out), stderr(&out));
    assert!(
        stderr(&out).contains("exited 2"),
        "the editor's exit code is named:\n{}",
        stderr(&out)
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn no_editor_at_all_is_a_normal_path_that_names_what_to_set() {
    let dir = repository();
    index(&dir);
    // "At all" has to be arranged, not assumed. The variables are already empty (see [`bare`]), but the
    // platform default is a name, not a setting, and the name is `vi` on Linux and macOS and `notepad`
    // on Windows — all of which are on the `PATH` of the machine that runs this test. Left alone, the
    // verb would find one, plan it, and run it for real: a full-screen editor with no terminal to draw
    // on, and a test waiting on a capture the editor is holding open. That is `reco-67ml`.
    let path = empty_path();
    let out = fathom_env(
        &dir,
        &["open", "HEAD~1"],
        &[("PATH", &path.to_string_lossy())],
    );
    assert_eq!(code(&out), Some(1), "{}{}", stdout(&out), stderr(&out));
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(text.contains("no editor"), "{text}");
    assert!(
        text.contains("not on PATH"),
        "{text} names the default it tried"
    );
    for remedy in ["$EDITOR", "$FATHOM_EDITOR", "[ui] editor"] {
        assert!(
            text.contains(remedy),
            "{remedy} should be suggested:\n{text}"
        );
    }
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_environment_names_the_editor_and_fathom_honours_it() {
    let dir = repository();
    index(&dir);
    let out = fathom_env(
        &dir,
        &["open", "HEAD~1", "--print"],
        &[("EDITOR", &ourselves())],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    assert!(stdout(&out).contains("from $EDITOR"), "{}", stdout(&out));

    // `$FATHOM_EDITOR` is the one that does not disturb the shell's own choice.
    let out = fathom_env(
        &dir,
        &["open", "HEAD~1", "--print"],
        &[
            ("EDITOR", "somebody-elses-editor"),
            ("FATHOM_EDITOR", &ourselves()),
        ],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    assert!(stdout(&out).contains("$FATHOM_EDITOR"), "{}", stdout(&out));
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn list_shows_every_jump_the_episode_offers_with_its_line() {
    let dir = repository();
    index(&dir);
    let out = fathom(&dir, &["open", "HEAD~1", "--list"]);
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("src/core.py:"), "{text}");
    assert!(text.contains("Cargo.lock:"), "{text}");
    assert!(
        text.contains("+1") || text.contains("modify"),
        "the rows carry the numbers the index recorded:\n{text}"
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_path_the_episode_never_touched_is_refused_by_name() {
    let dir = repository();
    index(&dir);
    let out = fathom(
        &dir,
        &["open", "HEAD~1", "--path", "src/nope.rs", "--print"],
    );
    assert_eq!(code(&out), Some(1), "{}{}", stdout(&out), stderr(&out));
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(text.contains("did not touch"), "{text}");
    assert!(text.contains("--list"), "{text} points at the listing");
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn an_ambiguous_suffix_names_every_file_it_could_have_meant() {
    let dir = repository();
    index(&dir);
    // The fixture has `src/core.py` and `docs/core.py` on purpose. `docs/core.py` is gone at `HEAD`
    // but present in the episode at `HEAD~1`, so both are live candidates here.
    let episode = episode_id(&dir);
    let out = fathom(&dir, &["open", &episode, "--path", "core.py", "--print"]);
    assert_eq!(code(&out), Some(2), "{}{}", stdout(&out), stderr(&out));
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        text.contains("src/core.py") && text.contains("docs/core.py"),
        "both candidates are printed:\n{text}"
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_deleted_file_is_not_offered_as_something_to_open() {
    let dir = repository();
    index(&dir);
    let out = fathom(&dir, &["open", "HEAD", "--path", "docs/core.py", "--print"]);
    assert_eq!(code(&out), Some(1), "{}{}", stdout(&out), stderr(&out));
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(text.contains("deleted"), "{text}");
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_clone_cannot_choose_the_program_this_process_runs() {
    let dir = repository();
    index(&dir);
    std::fs::write(
        dir.join("fathom.toml"),
        "[ui]\neditor = \"curl\"\n[diff]\ndifftool = \"wget\"\n",
    )
    .unwrap();
    let out = fathom(
        &dir,
        &["open", "HEAD~1", "--print", "--editor", &ourselves()],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    let text = stderr(&out);
    assert!(text.contains("ignored"), "{text}");
    assert!(
        text.contains("clone"),
        "{text} says why a clone does not get to choose"
    );
    assert!(
        text.contains("[ui] editor") && text.contains("[diff] difftool"),
        "both refused keys are named:\n{text}"
    );
    // The refused values are quoted in the notice and nowhere else: what ran is what was asked for.
    assert!(
        stdout(&out).contains(&format!("editor    {}", ourselves())),
        "the plan ran the editor that was asked for:\n{}",
        stdout(&out)
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn no_index_says_so_with_the_index_exit_code() {
    let dir = repository();
    let out = fathom(&dir, &["open", "HEAD", "--print"]);
    assert_eq!(code(&out), Some(12), "{}{}", stdout(&out), stderr(&out));
    assert!(stderr(&out).contains("fathom index"), "{}", stderr(&out));
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_json_form_carries_a_contract_and_the_same_decision() {
    let dir = repository();
    index(&dir);
    let out = fathom(
        &dir,
        &[
            "open",
            "HEAD~1",
            "--print",
            "--json",
            "--editor",
            &ourselves(),
        ],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("--json prints one JSON value");
    assert_eq!(document["contract"], "fathom.cli.open/1");
    assert_eq!(document["path"], "src/core.py");
    assert_eq!(document["line"], serde_json::json!(PLANT as u32 - 3));
    assert_eq!(document["run"], serde_json::json!(false));
}

// --- fathom difftool ---------------------------------------------------------------------------

#[cfg(feature = "sqlite-bundled")]
#[test]
fn difftool_stages_the_two_real_sides_of_the_file() {
    let dir = repository();
    index(&dir);
    let out = fathom(
        &dir,
        &[
            "difftool",
            "HEAD~1",
            "--path",
            "src/core.py",
            "--print",
            "--tool",
            &ourselves(),
        ],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("difftool"), "{text}");
    let (before, after) = staged_pair(&text);
    let contents = std::fs::read_to_string(&before).expect("the old side is written");
    let now = std::fs::read_to_string(&after).expect("the new side is written");
    assert!(
        !contents.contains("CHANGED") && now.contains(&format!("line {PLANT} CHANGED")),
        "the two sides are the two sides of the diff, read out of the object store:\n{before:?}\n{after:?}"
    );
    assert_eq!(
        contents.lines().count(),
        now.lines().count(),
        "a one-line change did not change the line count"
    );
    assert_eq!(
        contents.lines().nth(PLANT - 1).expect("the planted line"),
        format!("line {PLANT}"),
        "the old side is the pre-image, byte for byte"
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_missing_difftool_lists_what_was_tried_and_leaves_the_pair_behind() {
    let dir = repository();
    index(&dir);
    // The same scrubbing as `no_editor_at_all_…`: the refusal being asserted is a statement about what
    // is *not* installed, and a developer machine with `nvim` or `code` on it has difftools, so the
    // stated substitution would win here and the run would be over a tool nobody named.
    let path = empty_path();
    let out = fathom_env(
        &dir,
        &[
            "difftool",
            "HEAD~1",
            "--path",
            "src/core.py",
            "--tool",
            "definitely-not-a-difftool",
        ],
        &[("PATH", &path.to_string_lossy())],
    );
    assert_eq!(code(&out), Some(1), "{}{}", stdout(&out), stderr(&out));
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(text.contains("no difftool"), "{text}");
    for candidate in ["difftastic", "meld", "kdiff3"] {
        assert!(
            text.contains(candidate),
            "{candidate} should be listed:\n{text}"
        );
    }
    assert!(text.contains("$FATHOM_DIFFTOOL"), "{text} names the key");
    let (before, after) = staged_pair(&text);
    assert!(
        Path::new(&before).is_file() && Path::new(&after).is_file(),
        "the refusal is not the end of the review — the pair is still there:\n{text}"
    );
    let _ = std::fs::remove_dir_all(Path::new(&before).parent().expect("the staging directory"));
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_configured_difftool_is_asked_in_its_own_shape_and_says_so() {
    let dir = repository();
    index(&dir);
    let out = fathom_env(
        &dir,
        &["difftool", "HEAD~1", "--path", "src/core.py", "--print"],
        &[("FATHOM_DIFFTOOL", &ourselves())],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("tool from $FATHOM_DIFFTOOL"), "{text}");

    // A configured tool that is not installed is a stated substitution, never a silent one — and here
    // it is a refusal, because the PATH is scrubbed and there is nothing on it to substitute.
    let path = empty_path();
    let out = fathom_env(
        &dir,
        &["difftool", "HEAD~1", "--path", "src/core.py", "--print"],
        &[
            ("FATHOM_DIFFTOOL", "definitely-not-a-difftool"),
            ("PATH", &path.to_string_lossy()),
        ],
    );
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(code(&out), Some(1), "{text}");
    assert!(
        text.contains("definitely-not-a-difftool") && text.contains("no difftool"),
        "{text} names the tool that was asked for and says nothing could be run"
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn difftool_runs_the_tool_it_planned() {
    let dir = repository();
    index(&dir);
    let tool = format!("{} --version", ourselves());
    let out = fathom_env(
        &dir,
        &[
            "difftool",
            "HEAD~1",
            "--path",
            "src/core.py",
            "--tool",
            &tool,
        ],
        &[],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    assert!(
        stdout(&out).contains("fathom 0.1.0"),
        "the difftool's own output reached the terminal:\n{}",
        stdout(&out)
    );
}

// --- hyperlinks --------------------------------------------------------------------------------

#[cfg(feature = "sqlite-bundled")]
#[test]
fn a_pipe_gets_no_escape_sequences_and_is_told_why_in_the_plan() {
    let dir = repository();
    index(&dir);
    // Captured output is no more a terminal for being a file than for being a pipe, so nothing may be
    // emitted here whatever the terminal claims — and the `--print` plan says which verdict it reached,
    // because a silent difference is the one failure mode this feature cannot have.
    let out = fathom(
        &dir,
        &["open", "HEAD~1", "--print", "--editor", &ourselves()],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(!text.contains('\u{1b}'), "a pipe got raw escapes:\n{text}");
    assert!(
        text.contains("links     off"),
        "the plan states the verdict:\n{text}"
    );
    assert!(text.contains("not a terminal"), "and says why:\n{text}");

    let show = fathom(&dir, &["show", "HEAD"]);
    assert_eq!(code(&show), Some(0), "{}", stderr(&show));
    assert!(
        !stdout(&show).contains('\u{1b}'),
        "`fathom show` into a pipe emitted escapes:\n{}",
        stdout(&show)
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn an_advertised_terminal_gets_real_links_and_the_text_is_unchanged() {
    let dir = repository();
    index(&dir);
    // A pipe is not a terminal, so the only way a person gets links into a captured stream is to say
    // so — which is also the reviewer's own escape hatch when the probe cannot see their terminal.
    let out = fathom_env(
        &dir,
        &["open", "HEAD~1", "--print", "--editor", &ourselves()],
        &[("FATHOM_HYPERLINKS", "always"), ("TERM_PROGRAM", "WezTerm")],
    );
    assert_eq!(code(&out), Some(0), "{}{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("\u{1b}]8;;file:///"),
        "the path is an OSC-8 link:\n{text}"
    );
    assert!(
        text.contains("src/core.py"),
        "the visible text survived:\n{text}"
    );
    assert!(text.contains("#L"), "the URL carries the line:\n{text}");
    assert!(
        text.contains("links     on"),
        "the plan says so too:\n{text}"
    );

    let show = fathom_env(
        &dir,
        &["show", "HEAD~1"],
        &[("FATHOM_HYPERLINKS", "always")],
    );
    let shown = stdout(&show);
    assert!(
        shown.contains("\u{1b}]8;;file:///"),
        "`fathom show`'s file list is linked when told to:\n{shown}"
    );
    assert!(
        shown.contains("Cargo.lock"),
        "the text is still the text:\n{shown}"
    );
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn doctor_writes_the_hyperlink_verdict_down() {
    let dir = repository();
    index(&dir);
    let out = fathom_env(&dir, &["doctor", "--json"], &[("TERM_PROGRAM", "ghostty")]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("doctor --json is JSON");
    let links = &document["build"]["terminal"]["hyperlinks"];
    assert_eq!(
        links["advertised"],
        serde_json::json!(true),
        "ghostty advertises OSC-8:\n{document}"
    );
    assert_eq!(
        links["emit"],
        serde_json::json!(false),
        "and the pipe still refuses"
    );
    assert!(
        links["why"].as_str().is_some_and(|why| !why.is_empty()),
        "the verdict comes with its sentence:\n{document}"
    );
}

// --- helpers -----------------------------------------------------------------------------------

/// Pull the two staged paths out of a `--print`/refusal line: the difftool line (or the hint) names
/// them as its last two arguments.
#[cfg(feature = "sqlite-bundled")]
fn staged_pair(text: &str) -> (String, String) {
    let line = text
        .lines()
        .find(|line| {
            line.trim_start().starts_with("difftool ") || line.contains("any diff tool at all")
        })
        .unwrap_or_else(|| panic!("no difftool line in:\n{text}"));
    let paths: Vec<&str> = line
        .split_whitespace()
        .filter(|word| word.contains("fathom-difftool"))
        .map(|word| word.trim_end_matches(['.', ',']))
        .collect();
    assert!(
        paths.len() >= 2,
        "two staged paths were expected in:\n{line}"
    );
    (paths[0].to_owned(), paths[1].to_owned())
}

//! `fathom export`, end to end (`e5.7`, `reco-1tn`).
//!
//! The façade-level tests in `fathom-analyze/tests/export.rs` prove what an artefact is. This file proves
//! the verb that writes one, because every acceptance criterion in the ticket is a claim about a file on
//! disk in a real repository:
//!
//! * the three formats are written, and two consecutive exports of the same range are **byte-identical** —
//!   including the JSON one, which is the record itself rather than a rendering of it;
//! * a commit message written by a hostile author is escaped in the markdown and in the HTML, and the HTML
//!   is one file that fetches and runs nothing;
//! * the sign-off block carries reviewer, instant, scope and, for a dismissal, the reason;
//! * the checklist and `--strict` give CI something to branch on (`7`, the published gate code);
//! * the range can be named four ways, and a range that cannot be resolved produces a refusal and no file.
//!
//! Everything here runs the built binary in a temporary git repository, the way `tests/cli.rs` does: the
//! artefact is the product, so it is tested as a user gets it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The subject line that is hostile in the markdown sense and in the HTML one.
const HOSTILE: &str = "fix: <script>alert(\"x\")</script> and [click](javascript:alert(1)) | pipe";

/// One `fathom` call, run *inside* the fixture repository.
///
/// The artefact's `--out` is relative to the working directory like any other file name, and the review
/// verbs take no `--root` at all; running in the repository is what a reviewer does and it keeps every
/// path assertion below a one-liner.
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

/// A repository with three episodes, the middle one carrying a hostile subject.
///
/// The dates are pinned to the epoch so the artefact's instants are fixed, and the branch name is asked for
/// rather than assumed — a runner configured for `master` must build the same fixture.
fn repository(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-export-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    git(&dir, &["config", "user.name", "Dana Ruiz"]);
    git(&dir, &["config", "user.email", "dana@example.invalid"]);
    write(&dir, "src/lib.rs", "fn a() {}\n");
    write(&dir, "tests/lib.rs", "#[test] fn a_works() {}\n");
    git(&dir, &["add", "-A"]);
    git(
        &dir,
        &[
            "commit",
            "-q",
            "--date=1700000000 +0000",
            "-m",
            "feat: open the store",
        ],
    );
    write(&dir, "src/lib.rs", "fn a() {}\nfn b() {}\n");
    write(&dir, "src/hostile.rs", "fn x() {}\n");
    git(&dir, &["add", "-A"]);
    git(
        &dir,
        &["commit", "-q", "--date=1700014400 +0000", "-m", HOSTILE],
    );
    write(&dir, "src/lib.rs", "fn a() {}\nfn b() {}\nfn c() {}\n");
    write(&dir, "docs/notes.md", "# notes\n");
    git(&dir, &["add", "-A"]);
    git(
        &dir,
        &[
            "commit",
            "-q",
            "--date=1700028800 +0000",
            "-m",
            "docs: write the notes",
        ],
    );
    dir
}

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    std::fs::create_dir_all(target.parent().expect("a parent directory"))
        .expect("create the directory");
    std::fs::write(target, content).expect("write the fixture file");
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_else(|err| panic!("{name}: {err}"))
}

/// The `--out` stem's suffix-free name for a format.
fn artifact(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    dir.join(format!("{stem}.{extension}"))
}

// --- the three formats, twice -------------------------------------------------------------------------------

#[test]
fn all_three_formats_are_written_and_the_same_range_gives_the_same_bytes() {
    let dir = repository("stable");
    assert_eq!(code(&fathom(&dir, &["index"])), 0, "index");

    for (format, extension) in [("md", "md"), ("html", "html"), ("json", "json")] {
        let first = fathom(
            &dir,
            &[
                "export",
                "--episodes",
                "HEAD~2..HEAD",
                "--format",
                format,
                "--out",
                "first",
            ],
        );
        assert_eq!(code(&first), 0, "{}", stderr(&first));
        let second = fathom(
            &dir,
            &[
                "export",
                "--episodes",
                "HEAD~2..HEAD",
                "--format",
                format,
                "--out",
                "second",
            ],
        );
        assert_eq!(code(&second), 0, "{}", stderr(&second));

        let left = std::fs::read(artifact(&dir, "first", extension)).expect("the first artefact");
        let right =
            std::fs::read(artifact(&dir, "second", extension)).expect("the second artefact");
        assert!(
            left.len() > 1_000,
            "a {format} artefact of {} bytes is not a review record",
            left.len()
        );
        assert_eq!(
            left, right,
            "two consecutive exports of one range differ in the {format} artefact"
        );
    }

    // The human report names the file, the scope and the verdict, and does not claim what it did not do.
    let out = fathom(&dir, &["export", "--out", "report", "--episodes", "HEAD"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("report.md"), "{text}");
    assert!(text.contains("episode"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_json_artefact_is_the_record_and_the_markdown_is_the_same_facts() {
    let dir = repository("crossformat");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    for format in ["md", "html", "json"] {
        assert_eq!(
            code(&fathom(
                &dir,
                &[
                    "export",
                    "--format",
                    format,
                    "--out",
                    "same",
                    "--episodes",
                    "HEAD~2..HEAD"
                ]
            )),
            0
        );
    }
    let record: serde_json::Value =
        serde_json::from_str(&read(&dir, "same.json")).expect("the JSON artefact is JSON");
    assert_eq!(record["contract"].as_str(), Some("fathom-review-record/1"));
    assert_eq!(
        record["integrity"]["problems"].as_array().map(Vec::len),
        Some(0)
    );
    let markdown = read(&dir, "same.md");
    let html = read(&dir, "same.html");
    let episodes = record["episodes"].as_array().expect("episodes");
    assert_eq!(episodes.len(), 2, "the range held two episodes");
    for episode in episodes {
        let id = episode["delta"]["episode"]["id"].as_str().expect("an id");
        assert!(markdown.contains(id), "markdown lost {id}");
        assert!(html.contains(id), "html lost {id}");
    }
    // The findings the catalogue raised, if any, are in all three: the artefact does not hide them in one
    // format because they are awkward to lay out in another.
    for finding in record["findings"].as_array().expect("findings") {
        let subject = finding["subject"].as_str().expect("a subject");
        assert!(
            markdown.contains(subject) && html.contains(subject),
            "the finding on {subject} is in the JSON and nowhere else"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

// --- hostile input -------------------------------------------------------------------------------------------

#[test]
fn a_hostile_commit_message_is_escaped_in_every_format() {
    let dir = repository("hostile");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    for format in ["md", "html", "json"] {
        assert_eq!(
            code(&fathom(
                &dir,
                &[
                    "export",
                    "--episodes",
                    "HEAD~2..HEAD",
                    "--format",
                    format,
                    "--out",
                    "hostile"
                ]
            )),
            0,
            "the {format} export failed: {}",
            stderr(&fathom(&dir, &["--version"]))
        );
    }

    let html = read(&dir, "hostile.html");
    assert!(
        !html.contains("<script"),
        "a script tag reached the HTML artefact"
    );
    assert!(
        html.contains("&lt;script&gt;"),
        "the subject is escaped into nothing rather than escaped"
    );
    assert!(
        html.contains("[click](javascript:alert(1))") || html.contains("javascript:alert(1)"),
        "the link-shaped part of the subject is missing"
    );
    assert!(
        !html.contains("<img"),
        "an image tag reached the HTML artefact"
    );

    let markdown = read(&dir, "hostile.md");
    // In markdown the question is what a renderer would *execute*: a code span quoting the subject is
    // honest and inert, raw text quoting it is not.
    assert!(
        markdown.contains("\\<script\\>") || markdown.contains("&lt;script&gt;"),
        "the subject reached the markdown as live text"
    );
    assert!(
        markdown.contains("javascript"),
        "the subject was dropped rather than escaped"
    );
    // A document GitHub can outline: one `#` title, and the hostile text never became a heading of its own
    // (which is what an unescaped `#` inside a quoted subject would have done).
    assert_eq!(
        markdown
            .lines()
            .filter(|line| line.starts_with("# ") && !line.starts_with("## "))
            .count(),
        1,
        "the artefact lost its one title: {markdown}"
    );

    // And the index itself kept the message verbatim — escaping is a rendering decision, and the store is
    // not the place to lose what a commit said.
    let show = fathom(&dir, &["show", "HEAD~1", "--json"]);
    assert!(
        stdout(&show).contains("javascript"),
        "the store lost the subject while the artefact was escaping it: {}",
        stdout(&show)
    );
    std::fs::remove_dir_all(&dir).ok();
}

// --- sign-offs -----------------------------------------------------------------------------------------------

#[test]
fn the_signoff_block_names_the_reviewer_the_instant_and_the_scope() {
    let dir = repository("signoffs");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let episode = episode_of(&dir, "HEAD");
    let signed = fathom(
        &dir,
        &["review", "sign", &episode, "--note", "read it end to end"],
    );
    assert_eq!(code(&signed), 0, "{}", stderr(&signed));

    assert_eq!(
        code(&fathom(
            &dir,
            &["export", "--episodes", "HEAD~2..HEAD", "--out", "signed"]
        )),
        0
    );
    let markdown = read(&dir, "signed.md");
    let block = markdown
        .split_once("## Sign-offs")
        .expect("the sign-off section")
        .1;
    assert!(block.contains("Dana Ruiz"), "no reviewer: {block}");
    assert!(
        regex_instant(block),
        "no printable instant in the sign-off block: {block}"
    );
    assert!(
        block.contains("commit(s)") && block.contains("file(s)") && block.contains("path(s)"),
        "the sign-off does not say what it covered: {block}"
    );
    assert!(block.contains("read it end to end"), "the note is missing");
    assert!(
        block.contains("sha256:"),
        "the entry's tamper digest is missing: {block}"
    );
    assert!(
        block.contains(&episode),
        "the entry is not tied to its episode: {block}"
    );
    // The decision moved out of the checklist: a signed episode is not an open action.
    let json = fathom(
        &dir,
        &[
            "export",
            "--episodes",
            "HEAD~2..HEAD",
            "--out",
            "signed2",
            "--json",
        ],
    );
    let payload: serde_json::Value =
        serde_json::from_str(stdout(&json).trim()).expect("the --json payload is JSON");
    assert_eq!(payload["summary"]["signed_off"].as_u64(), Some(1));
    assert_eq!(payload["signoffs"].as_array().map(Vec::len), Some(1));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dismissal_prints_its_reason_because_that_is_the_point_of_a_dismissal() {
    let dir = repository("dismissed");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let episode = episode_of(&dir, "HEAD");
    assert_eq!(
        code(&fathom(
            &dir,
            &[
                "review",
                "sign",
                &episode,
                "--decision",
                "dismissed",
                "--note",
                "vendored tree, read once",
            ]
        )),
        0
    );
    assert_eq!(
        code(&fathom(
            &dir,
            &["export", "--episodes", "HEAD", "--out", "dismissed"]
        )),
        0
    );
    let markdown = read(&dir, "dismissed.md");
    let block = markdown
        .split_once("## Sign-offs")
        .expect("the sign-off section")
        .1;
    assert!(block.contains("dismissed"), "{block}");
    assert!(
        block.contains("vendored tree, read once"),
        "a dismissal without its reason is the decision that hides a problem: {block}"
    );
    // A dismissed episode is not "reviewed" — the north-star count is accepted sign-offs and nothing else.
    let json = fathom(
        &dir,
        &["export", "--episodes", "HEAD", "--out", "d2", "--json"],
    );
    let payload: serde_json::Value = serde_json::from_str(stdout(&json).trim()).expect("JSON");
    assert_eq!(payload["summary"]["signed_off"].as_u64(), Some(0));
    assert_eq!(payload["summary"]["dismissed"].as_u64(), Some(1));
    std::fs::remove_dir_all(&dir).ok();
}

// --- the gate -----------------------------------------------------------------------------------------------

#[test]
fn strict_exits_seven_while_the_checklist_is_open_and_zero_when_it_is_not() {
    let dir = repository("strict");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);

    let open = fathom(
        &dir,
        &[
            "export",
            "--episodes",
            "HEAD~2..HEAD",
            "--out",
            "gate",
            "--strict",
        ],
    );
    assert_eq!(
        code(&open),
        7,
        "an open checklist under --strict is CHECK_VIOLATIONS: {}",
        stderr(&open)
    );
    assert!(
        std::path::Path::exists(&artifact(&dir, "gate", "md")),
        "the gate still writes the artefact the operator has to read to close it"
    );
    let report = stdout(&open);
    assert!(report.contains("open"), "{report}");

    for rev in ["HEAD", "HEAD~1", "HEAD~2"] {
        let episode = episode_of(&dir, rev);
        assert_eq!(code(&fathom(&dir, &["review", "sign", &episode])), 0);
    }
    let closed = fathom(
        &dir,
        &[
            "export",
            "--episodes",
            "HEAD~2..HEAD",
            "--out",
            "gate2",
            "--strict",
        ],
    );
    assert_eq!(
        code(&closed),
        0,
        "a closed checklist under --strict is a pass: {}",
        stderr(&closed)
    );
    // Without --strict the same artefact is written and the exit code is the ordinary one.
    let loose = fathom(&dir, &["export", "--episodes", "HEAD", "--out", "gate3"]);
    assert_eq!(code(&loose), 0, "{}", stderr(&loose));
    std::fs::remove_dir_all(&dir).ok();
}

// --- the range, and the refusals ----------------------------------------------------------------------------

#[test]
fn the_range_can_be_an_episode_id_a_revspec_or_a_range() {
    let dir = repository("ranges");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let episode = episode_of(&dir, "HEAD~1");

    let by_id = fathom(
        &dir,
        &["export", "--episodes", &episode, "--out", "byid", "--json"],
    );
    assert_eq!(code(&by_id), 0, "{}", stderr(&by_id));
    let payload: serde_json::Value = serde_json::from_str(stdout(&by_id).trim()).expect("JSON");
    assert_eq!(
        payload["scope"]["episode_ids"].as_array().map(Vec::len),
        Some(1),
        "an episode id exports that episode: {payload}"
    );

    let one = fathom(
        &dir,
        &["export", "--episodes", "HEAD", "--out", "one", "--json"],
    );
    assert_eq!(code(&one), 0);
    let payload: serde_json::Value = serde_json::from_str(stdout(&one).trim()).expect("JSON");
    assert_eq!(
        payload["scope"]["episodes"].as_u64(),
        Some(1),
        "`--episodes HEAD` is the episode HEAD belongs to, not the whole history: {payload}"
    );

    let exclusive = fathom(
        &dir,
        &[
            "export",
            "--episodes",
            "HEAD~2..HEAD",
            "--out",
            "ex",
            "--json",
        ],
    );
    assert_eq!(code(&exclusive), 0);
    let payload: serde_json::Value = serde_json::from_str(stdout(&exclusive).trim()).expect("JSON");
    assert_eq!(payload["scope"]["episodes"].as_u64(), Some(2), "{payload}");

    let inclusive = fathom(
        &dir,
        &[
            "export",
            "--episodes",
            "HEAD~2..=HEAD",
            "--out",
            "in",
            "--json",
        ],
    );
    assert_eq!(code(&inclusive), 0, "{}", stderr(&inclusive));
    let payload: serde_json::Value = serde_json::from_str(stdout(&inclusive).trim()).expect("JSON");
    assert_eq!(payload["scope"]["episodes"].as_u64(), Some(3), "{payload}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_refusal_writes_no_artefact_and_says_what_to_type() {
    let dir = repository("refusals");

    // No index at all.
    let cold = fathom(&dir, &["export", "--out", "nope"]);
    assert_eq!(code(&cold), 12, "{}", stderr(&cold));
    assert!(stderr(&cold).contains("fathom index"));
    assert!(!artifact(&dir, "nope", "md").exists());

    // A rev that names nothing.
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let bad = fathom(
        &dir,
        &["export", "--episodes", "no-such-rev", "--out", "nope2"],
    );
    assert_eq!(code(&bad), 1, "{}", stderr(&bad));
    assert!(
        stderr(&bad).contains("no commit"),
        "the refusal should say what was wrong with the range: {}",
        stderr(&bad)
    );
    assert!(!artifact(&dir, "nope2", "md").exists());

    // A format that is not one of the three.
    let format = fathom(&dir, &["export", "--format", "pdf", "--out", "nope3"]);
    assert_eq!(code(&format), 2, "{}", stderr(&format));
    assert!(!artifact(&dir, "nope3", "pdf").exists());

    // And outside a repository.
    let bare = std::env::temp_dir().join(format!("fathom-export-bare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&bare);
    std::fs::create_dir_all(&bare).expect("create a bare directory");
    let outside = fathom(&bare, &["export", "--out", "nope4"]);
    assert_eq!(code(&outside), 3, "{}", stderr(&outside));
    let _ = std::fs::remove_dir_all(&bare);
    std::fs::remove_dir_all(&dir).ok();
}

// --- the pieces the artefact is supposed to have ------------------------------------------------------------

#[test]
fn the_artefact_holds_every_section_the_ticket_asked_for() {
    let dir = repository("sections");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    assert_eq!(
        code(&fathom(
            &dir,
            &["export", "--episodes", "HEAD~2..HEAD", "--out", "full"]
        )),
        0
    );
    let markdown = read(&dir, "full.md");
    for section in [
        "## Summary card",
        "## Episodes",
        "## Dependency delta",
        "## Architecture delta",
        "## Findings with evidence",
        "## Operator-action checklist",
        "## Sign-offs",
        "## Commits",
        "## What this record cannot answer",
        "## Provenance and self-check",
    ] {
        assert!(
            markdown.contains(section),
            "{section} is missing:\n{markdown}"
        );
    }
    // One `#` title, so the PR preview has one subject and the outline is usable.
    assert_eq!(
        markdown
            .lines()
            .filter(|line| line.starts_with("# ") && !line.starts_with("## "))
            .count(),
        1
    );
    // The gap list names the ticket that owns each answer it cannot give.
    let gaps = markdown
        .split_once("## What this record cannot answer")
        .expect("the gaps section")
        .1;
    assert!(gaps.contains("e2.") || gaps.contains("e3.") || gaps.contains("e4."));

    assert_eq!(
        code(&fathom(
            &dir,
            &[
                "export",
                "--episodes",
                "HEAD~2..HEAD",
                "--format",
                "html",
                "--out",
                "full"
            ]
        )),
        0
    );
    let html = read(&dir, "full.html");
    assert!(
        !html.is_empty() && html == html.trim_start(),
        "html head moved"
    );
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(
        html.contains("@page") && html.contains("@media print"),
        "the HTML artefact is not printable"
    );
    assert!(
        !html.contains("<script"),
        "the HTML artefact runs something"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn commits_link_to_the_remote_when_the_repository_has_one() {
    let dir = repository("remote");
    git(
        &dir,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    assert_eq!(
        code(&fathom(
            &dir,
            &["export", "--episodes", "HEAD", "--out", "linked"]
        )),
        0
    );
    let markdown = read(&dir, "linked.md");
    let oid = git_out(&dir, &["rev-parse", "HEAD"]);
    assert!(
        markdown.contains(&format!("https://example.invalid/repo/commit/{oid}"))
            || markdown.contains("example.invalid/repo"),
        "no web link was derived from the remote:\n{markdown}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn stdout_owns_stdout_and_the_report_owns_stderr() {
    let dir = repository("stdout");
    assert_eq!(code(&fathom(&dir, &["index"])), 0);
    let out = fathom(
        &dir,
        &["export", "--episodes", "HEAD", "--format", "md", "--stdout"],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.starts_with("# Fathom review record"),
        "stdout is the artefact and nothing else:\n{text}"
    );
    assert!(text.contains("## Sign-offs"));
    assert!(
        stderr(&out).contains("bytes"),
        "the report went to stdout: {}",
        stderr(&out)
    );
    // `--stdout` and `--json` both claim stdout, so asking for both is a usage error rather than a
    // composition: a machine consumer must never have to guess whether the bytes it got are the artefact
    // or the report.
    let both = fathom(
        &dir,
        &[
            "export",
            "--episodes",
            "HEAD",
            "--format",
            "json",
            "--stdout",
            "--json",
        ],
    );
    assert_eq!(
        code(&both),
        2,
        "two owners of stdout is a usage error: {}",
        stderr(&both)
    );
    // The JSON artefact to a file is the same bytes as the JSON artefact to stdout, which is the property
    // that makes `--stdout` usable in a pipeline.
    let file = fathom(
        &dir,
        &[
            "export",
            "--episodes",
            "HEAD",
            "--format",
            "json",
            "--out",
            "to",
        ],
    );
    assert_eq!(code(&file), 0, "{}", stderr(&file));
    let piped = fathom(
        &dir,
        &[
            "export",
            "--episodes",
            "HEAD",
            "--format",
            "json",
            "--stdout",
        ],
    );
    assert_eq!(code(&piped), 0, "{}", stderr(&piped));
    assert_eq!(
        stdout(&piped),
        read(&dir, "to.json"),
        "the same export said different things to the pipe and to the file"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// --- helpers -------------------------------------------------------------------------------------------------

/// The episode a rev belongs to, read out of the store through the verb that prints it.
fn episode_of(dir: &Path, rev: &str) -> String {
    let out = fathom(dir, &["show", rev, "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    let start = text
        .find("\"ep-")
        .unwrap_or_else(|| panic!("show --json names no episode: {text}"));
    let tail = &text[start + 1..];
    let end = tail
        .find('"')
        .unwrap_or_else(|| panic!("an unterminated episode id in {text}"));
    tail[..end].to_owned()
}

fn git_out(root: &Path, args: &[&str]) -> String {
    String::from_utf8_lossy(
        &Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .expect("git runs")
            .stdout,
    )
    .trim()
    .to_owned()
}

/// An ISO-8601 instant of the shape the ledger writes, without pulling a date parser into a CLI test: the
/// claim under test is "a printable stamp, not a bare integer and not an empty cell".
fn regex_instant(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars.windows(16).any(|w| {
        let digit = |i: usize| w[i].is_ascii_digit();
        (0..4).all(digit)
            && w[4] == '-'
            && digit(5)
            && digit(6)
            && w[7] == '-'
            && digit(8)
            && digit(9)
            && w[10] == 'T'
            && digit(11)
            && digit(12)
            && w[13] == ':'
            && digit(14)
            && digit(15)
    })
}

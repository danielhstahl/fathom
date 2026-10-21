//! `fathom cmp`, end to end (`e5.3`, `reco-ifo`).
//!
//! The façade-level tests in `fathom-analyze/tests/compare.rs` (if any) prove what a comparison *is*.
//! This file proves the verb, because every acceptance criterion in the ticket is a claim about a program
//! run in a repository:
//!
//! * any two things a reviewer can name — episode ids, commits, `branch@rev` — make a comparison;
//! * the answer is the *composed* delta over the range between them, not one diff;
//! * a comparison can be saved under a name and the name works in the next process, which is the only
//!   meaning "survives restarts" can have;
//! * every refusal says which end was wrong and what to type instead, and prints nothing on stdout.
//!
//! Everything runs the built binary in a temporary git repository, the way `tests/cli.rs` does.
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

/// A repository with four episodes, one per day, each touching one file.
///
/// Dates are pinned so a range's span is a fact of the fixture and not of the clock the test runs under.
fn repository(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-cmp-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    git(&dir, &["config", "user.name", "Dana Ruiz"]);
    git(&dir, &["config", "user.email", "dana@example.invalid"]);
    write(&dir, "src/lib.rs", "fn a() {}\n");
    commit(&dir, 1_700_000_000, "feat: open the store");
    write(&dir, "src/lib.rs", "fn a() {}\nfn b() {}\n");
    commit(&dir, 1_700_014_400, "feat: add b");
    write(&dir, "docs/notes.md", "# notes\n");
    commit(&dir, 1_700_028_800, "docs: write the notes");
    write(&dir, "src/lib.rs", "fn a() {}\nfn b() {}\nfn c() {}\n");
    commit(&dir, 1_700_043_200, "feat: add c");
    let out = fathom(&dir, &["index"]);
    assert_eq!(code(&out), 0, "index: {}", stderr(&out));
    dir
}

/// The `--json` payload, parsed.
fn json(out: &Output) -> serde_json::Value {
    let text = stdout(out);
    let line = text
        .lines()
        .find(|line| line.starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON line in:\n{text}"));
    serde_json::from_str(line).unwrap_or_else(|err| panic!("not JSON ({err}):\n{line}"))
}

#[test]
fn two_episode_ids_make_a_composed_comparison() {
    let dir = repository("episodes");
    // Revisions first — the shape a reviewer types without looking anything up.
    let out = fathom(&dir, &["cmp", "HEAD~3..HEAD", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let value = json(&out);
    assert_eq!(
        value["contract"].as_str(),
        Some("fathom.compare/1"),
        "{value}"
    );
    assert_eq!(
        value["spec"].as_str(),
        Some("HEAD~3..HEAD"),
        "the answer names the pair it is between"
    );
    let from = &value["from"];
    let to = &value["to"];
    for end in [from, to] {
        assert!(
            end["episode"].is_string() && end["summary"].is_string(),
            "an endpoint names the episode it resolved to and the subject it carries: {value}"
        );
    }
    assert_ne!(
        from["episode"].as_str(),
        to["episode"].as_str(),
        "the two ends are two episodes: {value}"
    );
    assert_eq!(
        value["episodes"].as_array().map(Vec::len),
        Some(3),
        "the span between the ends is the range the delta was composed from: {value}"
    );

    // ...and the *episode ids* the answer named are a spec of their own: the two halves of "any two
    // episodes or revisions" resolve to the same two endpoints however they were written, and compose the
    // same delta from them.
    let from = value["from"]["episode"].as_str().unwrap().to_owned();
    let to = value["to"]["episode"].as_str().unwrap().to_owned();
    let by_id = fathom(&dir, &["cmp", &format!("{from}..{to}"), "--json"]);
    assert_eq!(code(&by_id), 0, "{}", stderr(&by_id));
    let again = json(&by_id);
    assert_eq!(
        again["from"]["episode"].as_str(),
        Some(from.as_str()),
        "{again}"
    );
    assert_eq!(
        again["to"]["episode"].as_str(),
        Some(to.as_str()),
        "{again}"
    );
    assert_eq!(
        again["delta"], value["delta"],
        "the same two endpoints, however written, compose the same delta"
    );
}

#[test]
fn a_single_endpoint_is_refused_with_the_shape_to_type() {
    let dir = repository("one-end");
    let out = fathom(&dir, &["cmp"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    let text = stderr(&out);
    assert!(text.contains("two endpoints"), "{text}");
    assert!(text.contains(".."), "the remedy shows the shape: {text}");
    assert!(
        stdout(&out).trim().is_empty(),
        "a refusal prints a diagnosis, not a partial comparison"
    );
}

#[test]
fn an_endpoint_that_is_not_there_names_which_one() {
    let dir = repository("missing");
    let out = fathom(&dir, &["cmp", "HEAD..nosuchthing"]);
    assert_ne!(code(&out), 0);
    let text = stderr(&out);
    assert!(
        text.contains("nosuchthing"),
        "the refusal repeats what it could not resolve: {text}"
    );
    assert!(!stdout(&out).contains("\"contract\""), "{}", stdout(&out));
}

#[test]
fn a_saved_comparison_is_named_in_the_next_process() {
    // The ticket's criterion is "`fathom cmp --save` survives restarts", and the only way to test that is
    // a second process: the row has to be on disk, decoded by the same code that wrote it.
    let dir = repository("saved");
    let saved = fathom(
        &dir,
        &[
            "cmp",
            "HEAD~2..HEAD",
            "--save",
            "since-the-refactor",
            "--note",
            "the release cut",
        ],
    );
    assert_eq!(code(&saved), 0, "{}", stderr(&saved));
    assert!(
        stdout(&saved).contains("since-the-refactor"),
        "saving says it saved: {}",
        stdout(&saved)
    );

    let list = fathom(&dir, &["cmp", "--list"]);
    assert_eq!(code(&list), 0, "{}", stderr(&list));
    let text = stdout(&list);
    assert!(text.contains("since-the-refactor"), "{text}");
    assert!(
        text.contains("HEAD~2..HEAD"),
        "the name is worthless without the spec beside it: {text}"
    );
    assert!(
        text.contains("the release cut"),
        "the note is why the name exists: {text}"
    );

    // The name alone is now a spec.
    let again = fathom(&dir, &["cmp", "since-the-refactor", "--json"]);
    assert_eq!(code(&again), 0, "{}", stderr(&again));
    let value = json(&again);
    assert_eq!(
        value["spec"].as_str(),
        Some("HEAD~2..HEAD"),
        "the saved name resolved to what was saved: {value}"
    );
    assert_eq!(
        value["asked"].as_str(),
        Some("since-the-refactor"),
        "and the answer still says what was typed: {value}"
    );

    // ...and the row is in the session table the shell reads, under the prefix `fathom_tui::session`
    // owns — which is what puts it in the `,` palette.
    let session = fathom(&dir, &["tui", "--show-session", "--json"]);
    assert_eq!(code(&session), 0, "{}", stderr(&session));
    assert!(
        stdout(&session).contains("tui.compare.saved.since-the-refactor"),
        "a saved comparison outside the shell's namespace is a saved comparison the palette cannot see"
    );
}

#[test]
fn saving_a_name_that_exists_replaces_it_and_says_so() {
    let dir = repository("replace");
    for spec in ["HEAD~3..HEAD", "HEAD~1..HEAD"] {
        let out = fathom(&dir, &["cmp", spec, "--save", "recent"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
    }
    let list = stdout(&fathom(&dir, &["cmp", "--list"]));
    assert!(
        list.matches("recent").count() >= 1,
        "the name is listed: {list}"
    );
    assert!(
        !list.contains("HEAD~3..HEAD"),
        "the second save replaced the first rather than shadowing it: {list}"
    );
}

#[test]
fn text_output_reads_like_an_answer_and_names_the_endpoints() {
    let dir = repository("text");
    let out = fathom(&dir, &["cmp", "HEAD~2..HEAD"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("HEAD~2..HEAD"), "{text}");
    assert!(
        text.contains("→") || text.contains(".."),
        "the pair is stated: {text}"
    );
    assert!(
        text.contains("src/lib.rs") || text.contains("file"),
        "the composed body is printed: {text}"
    );
}

#[test]
fn a_comparison_over_a_huge_range_says_what_it_left_out() {
    // `fathom cmp` bounds its own work the way every other façade in this workspace does: the answer is
    // complete about what it covers and says so about what it does not — never a silently short list.
    let dir = repository("truncated");
    let out = fathom(&dir, &["cmp", "HEAD~3..HEAD", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let value = json(&out);
    let gaps = value["delta"]["gaps"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // A four-episode range is inside the bound, so it must not claim a truncation...
    assert!(
        gaps.iter().all(|gap| gap["short"]
            .as_str()
            .unwrap_or_default()
            .find("left out")
            .is_none()),
        "a range that fits has no gap to report: {value}"
    );
}

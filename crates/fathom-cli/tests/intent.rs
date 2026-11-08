//! The intent surfaces, driven as a binary (`e4.2`, `reco-c54`).
//!
//! The ladder is [`fathom_ingest::intent`]'s test, the wiring is `fathom-analyze`'s, and the payload shape
//! is `schemas.rs`. What only a binary test can prove is what a person actually sees:
//!
//! * `fathom index` says what it captured and from where — including the run that captured nothing;
//! * `fathom show` prints the intent, the plan and the declared scope, and prints *something* when there is
//!   none, because a blank field is a field the reader has to guess the meaning of;
//! * `[intent]` in the repository's own `fathom.toml` changes what the ladder is allowed to read;
//! * an unreadable stream is said out loud instead of being reported as "no stream".

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Every test in this file is store-backed: the fixture builds its index with `fathom index`, and `fathom intent`
// reads that index back — both halves of that are compiled only when a SQLite backend is on (ADR-002
// §6.1, §6.2), so on the headless `minimal` build the fixture cannot be built at all and the verb answers
// `FEATURE_ABSENT` (9) instead of answering. Gated in the spelling `tests/check_verb.rs` and
// `tests/show_trailers.rs` use; on every store-backed build the whole suite still runs.
#![cfg(feature = "sqlite-bundled")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The instant every commit date and event timestamp is measured from, in seconds.
const BASE: i64 = 1_770_000_000;

fn fathom(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .current_dir(cwd)
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

/// A repository with one agent turn in it: a root commit, then a commit inside a harness turn whose stream
/// is written after the commits so it stays out of the history it describes.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("fathom-cli-intent-{label}-{index}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"], &[]);
        let repo = Repo { dir };
        repo.commit(
            "daniel",
            0,
            "chore: initialise",
            &[("README.md", "# fathom\n")],
            &[],
        );
        repo
    }

    fn root(&self) -> &Path {
        &self.dir
    }

    fn commit(
        &self,
        author: &str,
        minute: i64,
        subject: &str,
        files: &[(&str, &str)],
        trailers: &[(&str, &str)],
    ) {
        for (path, body) in files {
            let full = self.dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create the source directory");
            }
            std::fs::write(full, body).expect("writing a fixture file");
        }
        git(self.root(), &["add", "-A"], &[]);
        let block = trailers
            .iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        let message = if block.is_empty() {
            subject.to_owned()
        } else {
            format!("{subject}\n\n{block}")
        };
        git(
            self.root(),
            &["commit", "-q", "-m", &message],
            &[("AUTHOR", author), ("MINUTE", &minute.to_string())],
        );
    }

    /// Append the harness's lines. Written after the commits so the stream is not in the history it describes.
    fn write_stream(&self, lines: &[(&str, i64, &str)]) {
        let fathom_dir = self.dir.join(".fathom");
        std::fs::create_dir_all(&fathom_dir).expect("create .fathom");
        let body = lines
            .iter()
            .enumerate()
            .map(|(index, (source, minute, payload))| {
                format!(
                    r#"{{"cwd":"{}","event_id":"e{seq}","harness":"pi_json","payload":{payload},"schema_version":1,"seq":{seq},"session_id":"s1","source":"{source}","ts":"{ts}"}}"#,
                    self.dir.display().to_string().replace('\\', "/"),
                    seq = index + 1,
                    ts = ts(*minute * 60),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(fathom_dir.join("events.jsonl"), format!("{body}\n"))
            .expect("writing the stream");
    }

    fn write_config(&self, text: &str) {
        std::fs::write(self.dir.join("fathom.toml"), text).expect("writing fathom.toml");
    }

    fn index(&self) -> Output {
        fathom(&["index", "."], self.root())
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(cwd: &Path, args: &[&str], extra: &[(&str, &str)]) {
    let author = extra
        .iter()
        .find(|(key, _)| *key == "AUTHOR")
        .map_or("pi-agent", |(_, value)| *value);
    let minute: i64 = extra
        .iter()
        .find(|(key, _)| *key == "MINUTE")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let date = format!("@{} +0000", BASE + minute * 60);
    let out = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", author)
        .env("GIT_AUTHOR_EMAIL", format!("{author}@example.invalid"))
        .env("GIT_COMMITTER_NAME", author)
        .env("GIT_COMMITTER_EMAIL", format!("{author}@example.invalid"))
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A timestamp `seconds` after [`BASE`], in the shape vendors send.
fn ts(seconds: i64) -> String {
    let total = BASE + seconds;
    let days = total.div_euclid(86_400);
    let secs_of_day = total.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// One turn's worth of events, with a prompt, a plan and a declared scope.
fn turn_events() -> Vec<(&'static str, i64, &'static str)> {
    vec![
        (
            "UserPromptSubmit",
            58,
            r#"{"prompt":"make the card pane scroll","plan":["pin the cursor","golden-test it"],"declared_scope":["crates/fathom-tui/**"]}"#,
        ),
        (
            "file.edited",
            59,
            r#"{"path":"src/card.rs","change":"update"}"#,
        ),
        ("Stop", 60, "{}"),
    ]
}

#[test]
fn index_says_what_it_captured_and_from_where() {
    let repo = Repo::new("tally");
    repo.commit(
        "pi-agent",
        59,
        "feat: the card scrolls",
        &[("src/card.rs", "// s\n")],
        &[],
    );
    repo.write_stream(&turn_events());
    let out = repo.index();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("intent: 1 recorded (1 from the harness), 0 digest-only, 1 none recorded"),
        "the run reports every bucket, including the empty one:\n{text}"
    );
    assert!(
        text.contains("events.jsonl"),
        "and names the stream it read:\n{text}"
    );
}

#[test]
fn index_reports_no_stream_as_a_state_not_a_failure() {
    let repo = Repo::new("nostream");
    repo.commit("daniel", 60, "chore: tidy", &[("docs/tidy.md", "t\n")], &[]);
    let out = repo.index();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).contains("no stream at"),
        "absence is printed as absence:\n{}",
        stdout(&out)
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&fathom(&["index", ".", "--json"], repo.root())))
            .expect("json");
    assert_eq!(json["intent"]["none"], 2);
    assert_eq!(json["intent"]["recorded"], 0);
    assert_eq!(json["intent"]["harness"]["exists"], false);
    assert_eq!(
        json["intent"]["harness"]["notices"]
            .as_array()
            .map(Vec::len),
        Some(0),
        "no stream is not a fault, so there is nothing to notice: {json}"
    );
}

#[test]
fn an_unreadable_stream_says_so_instead_of_claiming_there_is_none() {
    // "no stream" and "unreadable stream" are different reasons for the same empty tally, and only one of
    // them is the repository's fault. The index must not print the reassuring one.
    let repo = Repo::new("broken");
    repo.commit(
        "pi-agent",
        60,
        "feat: the thing",
        &[("src/a.rs", "// a\n")],
        &[],
    );
    std::fs::create_dir_all(repo.root().join(".fathom")).expect("create .fathom");
    std::fs::write(
        repo.root().join(".fathom/events.jsonl"),
        "\u{ff}\u{ff}not json at all\n",
    )
    .expect("writing junk");
    let out = repo.index();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("unreadable stream at"),
        "the run says the file is there and unreadable, not that it is absent:\n{text}"
    );
    assert!(
        !text.contains("no stream at"),
        "and does not print the reassuring version:\n{text}"
    );
    assert!(
        text.contains("stream:"),
        "with the reader's own reason on the next line:\n{text}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&fathom(&["index", ".", "--json"], repo.root())))
            .expect("json");
    assert_eq!(json["intent"]["harness"]["exists"], true);
    assert_eq!(json["intent"]["harness"]["turns"], 0);
    assert!(
        json["intent"]["harness"]["refused"].as_u64().unwrap_or(0) >= 1,
        "counted, not swallowed: {json}"
    );
}

#[test]
fn show_prints_the_intent_the_plan_and_the_declared_scope() {
    let repo = Repo::new("show");
    repo.commit(
        "pi-agent",
        59,
        "feat: the card scrolls",
        &[("src/card.rs", "// s\n")],
        &[],
    );
    repo.write_stream(&turn_events());
    assert_eq!(code(&repo.index()), 0);
    let out = fathom(&["show", "HEAD", "--root", "."], repo.root());
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("make the card pane scroll"),
        "the request, in the asker's words:\n{text}"
    );
    assert!(
        text.contains("hook-prompt"),
        "and which rung decided:\n{text}"
    );
    assert!(
        text.contains("pin the cursor"),
        "and the plan item nobody chose as the text is still printed:\n{text}"
    );
    assert!(
        text.contains("crates/fathom-tui/**"),
        "and the scope that was declared:\n{text}"
    );
}

#[test]
fn show_says_no_recorded_intent_rather_than_printing_nothing() {
    let repo = Repo::new("silent");
    repo.commit("daniel", 60, "chore: tidy", &[("docs/tidy.md", "t\n")], &[]);
    assert_eq!(code(&repo.index()), 0);
    let out = fathom(&["show", "HEAD", "--root", "."], repo.root());
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("no recorded intent"),
        "the absence is shown, with what was looked for:\n{text}"
    );
    assert!(text.contains("no harness events"), "{text}");
}

#[test]
fn show_json_carries_the_whole_intent_document() {
    let repo = Repo::new("json");
    repo.commit(
        "pi-agent",
        60,
        "feat: parse the header",
        &[("src/parser/header.rs", "// h\n")],
        &[
            ("Intent", "accept the new header format"),
            ("Scope", "src/parser/**"),
        ],
    );
    assert_eq!(code(&repo.index()), 0);
    let out = fathom(&["show", "HEAD", "--json", "--root", "."], repo.root());
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let payload: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("one JSON value");
    let intent = &payload["episode"]["identity"]["intent"];
    assert_eq!(intent["state"], "recorded");
    assert_eq!(intent["source"], "agent-intent-trailer");
    assert_eq!(intent["text"], "accept the new header format");
    assert_eq!(intent["schema"], "fathom-intent/1");
    assert_eq!(intent["scope"][0], "src/parser/**");
    assert_eq!(intent["confidence"], 0.9);
    assert!(
        intent.get("prompt").is_none(),
        "a trailer-only episode has no prompt to report: {intent}"
    );
}

#[test]
fn the_repository_can_switch_the_prompt_off() {
    // `[intent] sources` replaces the list, and the only interesting use of it is to refuse to quote the
    // harness. The stream is still there and still read — the trailer is what the card prints.
    let repo = Repo::new("configured");
    repo.commit(
        "pi-agent",
        59,
        "feat: the card scrolls",
        &[("src/card.rs", "// s\n")],
        &[("Intent", "the trailer's own words")],
    );
    repo.write_stream(&turn_events());
    repo.write_config("[intent]\nsources = [\"agent-intent-trailer\"]\n");
    assert_eq!(code(&repo.index()), 0, "{}", stderr(&repo.index()));
    let text = stdout(&fathom(&["show", "HEAD", "--root", "."], repo.root()));
    assert!(
        text.contains("the trailer's own words"),
        "the prompt was read and not used:\n{text}"
    );
    assert!(
        !text.contains("make the card pane scroll"),
        "and the prompt is not quoted:\n{text}"
    );
    // But it is not hidden either: the evidence row says the prompt exists and this repository refused it,
    // which is a different finding from a turn that said nothing.
    let payload: serde_json::Value = serde_json::from_str(&stdout(&fathom(
        &["show", "HEAD", "--json", "--root", "."],
        repo.root(),
    )))
    .expect("one JSON value");
    let evidence = payload["episode"]["identity"]["intent"]["evidence"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        evidence.iter().any(|item| {
            item["source"] == "hook-prompt"
                && item["detail"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("not used")
        }),
        "{evidence:?}"
    );
}

#[test]
fn digest_only_can_be_turned_off_too() {
    // `[intent] digest_counts = false` — a repository that would rather print `no recorded intent` than a
    // hash gets exactly that.
    let repo = Repo::new("digest");
    repo.commit(
        "pi-agent",
        59,
        "feat: the thing",
        &[("src/a.rs", "// a\n")],
        &[],
    );
    repo.write_stream(&[
        (
            "UserPromptSubmit",
            58,
            r#"{"prompt_sha256":"aa","prompt_chars":40}"#,
        ),
        (
            "file.edited",
            59,
            r#"{"path":"src/a.rs","change":"update"}"#,
        ),
        ("Stop", 60, "{}"),
    ]);
    repo.write_config("[intent]\ndigest_counts = false\n");
    assert_eq!(code(&repo.index()), 0);
    assert!(
        stdout(&fathom(&["show", "HEAD", "--root", "."], repo.root()))
            .contains("no recorded intent"),
    );
    // And with the default it is a state with a digest beside it.
    std::fs::remove_file(repo.root().join("fathom.toml")).expect("remove the config");
    assert_eq!(code(&repo.index()), 0);
    let text = stdout(&fathom(&["show", "HEAD", "--root", "."], repo.root()));
    assert!(
        text.contains("no text captured"),
        "the digest is a state, and it says which:\n{text}"
    );
}

#[test]
fn a_bad_intent_key_is_a_notice_and_not_a_failure() {
    // The config surface refuses to guess: a key of the wrong shape is reported and the default stands.
    let repo = Repo::new("badconfig");
    repo.commit("daniel", 60, "chore: tidy", &[("docs/tidy.md", "t\n")], &[]);
    repo.write_config("[intent]\nsources = [\"review-note\"]\nplan_cap = 0\n");
    let out = repo.index();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&fathom(&["index", ".", "--json"], repo.root())))
            .expect("json");
    assert_eq!(
        json["intent"]["none"], 2,
        "and the intent still resolved: {json}"
    );
}

#[test]
fn correcting_an_author_does_not_lose_what_the_harness_said() {
    // `fathom attribution correct` re-segments the whole repository, and a rebuild recomputes every
    // episode's intent. If that rebuild ran without the event stream it would answer the same question
    // differently — the trailer's words, or `no recorded intent` — and a correction to an *author* would
    // silently cost the reviewer the *intent*. One answer per question, whichever command asked it.
    let repo = Repo::new("attribution");
    repo.commit(
        "pi-agent",
        59,
        "feat: the card scrolls",
        &[("src/card.rs", "// s\n")],
        &[],
    );
    repo.write_stream(&turn_events());
    assert_eq!(code(&repo.index()), 0);
    let shown = |repo: &Repo| -> serde_json::Value {
        let out = fathom(&["show", "HEAD", "--json", "--root", "."], repo.root());
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        serde_json::from_str(&stdout(&out)).expect("one JSON value")
    };
    let before = shown(&repo);
    assert_eq!(
        before["episode"]["identity"]["intent"]["source"],
        "hook-prompt"
    );

    let out = fathom(
        &[
            "attribution",
            "correct",
            "HEAD",
            "--kind",
            "human",
            "--note",
            "that was me",
            "--root",
            ".",
        ],
        repo.root(),
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let after = shown(&repo);
    assert_eq!(
        after["episode"]["identity"]["author_kind"], "human",
        "the correction took"
    );
    assert_eq!(
        after["episode"]["identity"]["intent"], before["episode"]["identity"]["intent"],
        "and nothing else about the intent moved"
    );
}

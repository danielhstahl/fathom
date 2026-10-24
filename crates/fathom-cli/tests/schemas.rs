//! The `--json` contract, checked against the checked-in schemas — acceptance criterion 1 of `e1.10`:
//! "each verb has a JSON schema under `schemas/` and a round-trip test".
//!
//! The round trip is: run the real binary, parse stdout as one JSON value, validate it against the
//! checked-in schema, re-serialise it, re-parse it, and validate again. The last two steps are not
//! theatre — a payload that validates only before it goes through a string is a payload whose consumer
//! (a shell, a `jq` filter, a CI step) gets a different shape than the test saw.
//!
//! Why a hand-written validator instead of a crate: the workspace's dependency policy (ADR-002 §6.1) makes
//! every dependency a feature-and-audit decision, and this validator covers the keyword subset the checked-in
//! schemas actually use (`type`, `required`, `properties`, `additionalProperties`, `items`, `enum`, `const`,
//! `minimum`, `maximum`, `oneOf`, `$ref` across files). A `jsonschema` crate would be a heavier dependency
//! than the subset is worth, and a test that skips when its dependency is missing is a test that does not
//! run (`tests/conftest.py`'s argument, applied to Rust).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Output;

/// The validator, shared with `tests/delta_schema.rs` — see that file's header for why it is a `#[path]`
/// include and not a copy.
#[path = "validator.rs"]
mod validator;

use validator::*;

use serde_json::Value;

/// The one runner: a throwaway repository, three commits, the verbs pointed at it.
mod fixture {
    use std::process::{Command, Output};

    pub fn repository(tag: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "fathom-cli-schemas-{}-{tag}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"]);
        // A real repository ignores `.fathom/` (ADR-004 §12); a fixture that does not turns a watcher's or a
        // daemon's atomic rename into a `git add -A` that died on a path it had just listed. Local exclude, so
        // no commit content changes.
        let exclude = dir.join(".git").join("info").join("exclude");
        std::fs::create_dir_all(exclude.parent().expect("git created info/")).expect("info/");
        std::fs::write(&exclude, ".fathom/\n").expect("write the local exclude");
        for n in 0..3 {
            std::fs::write(
                dir.join(format!("src{n}.rs")),
                format!("fn f{n}() -> u32 {{ {n} }}\n"),
            )
            .expect("write a fixture file");
            git(&dir, &["add", "-A"]);
            git(&dir, &["commit", "-q", "-m", &format!("commit {n}")]);
        }
        dir
    }

    pub fn git(cwd: &std::path::Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(cwd)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.invalid")
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "ada@example.invalid")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .args(args)
            .output()
            .expect("git is on PATH");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub fn fathom(args: &[&str], cwd: &std::path::Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fathom"))
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("the built fathom binary is runnable")
    }

    pub fn stdout(out: &Output) -> String {
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    pub fn stderr(out: &Output) -> String {
        String::from_utf8_lossy(&out.stderr).to_string()
    }

    pub fn code(out: &Output) -> i32 {
        out.status.code().expect("fathom exits with a code")
    }
}

/// Parse stdout as exactly one JSON value, validate it, re-parse the re-serialisation, validate again.
fn round_trip(schema_name: &str, out: &Output) -> Value {
    let schema = Schema::load(schema_name);
    open(&["common.schema.json", schema_name]);

    let text = fixture::stdout(out).trim().to_owned();
    let value: Value = serde_json::from_str(&text).unwrap_or_else(|err| {
        panic!(
            "stdout was not one JSON value: {err}\n--- stdout ---\n{text}\n--- stderr ---\n{}",
            fixture::stderr(out)
        )
    });
    let problems = validate(&schema.document, &value, "$");
    assert!(
        problems.is_empty(),
        "{schema_name}: {problems:#?}\npayload: {value:#}"
    );

    // The round trip proper: bytes out, bytes back in, same verdict.
    let again: Value = serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
    assert_eq!(again, value, "re-serialising changed the payload");
    let problems = validate(&schema.document, &again, "$");
    assert!(
        problems.is_empty(),
        "{schema_name} after a round trip: {problems:#?}"
    );
    value
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn index_json_conforms_to_its_schema() {
    let dir = fixture::repository("index");
    let out = fixture::fathom(&["index", ".", "--json"], &dir);
    assert_eq!(fixture::code(&out), 0, "{}", fixture::stderr(&out));
    let value = round_trip("index.schema.json", &out);
    assert_eq!(value["commits"]["new"].as_u64(), Some(3));
    assert_eq!(value["rebuild"].as_bool(), Some(false));
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn show_stats_and_doctor_conform_to_their_schemas() {
    let dir = fixture::repository("verbs");
    let root = dir.to_str().unwrap();
    assert_eq!(fixture::code(&fixture::fathom(&["index", root], &dir)), 0);

    let shown = fixture::fathom(&["show", "HEAD~1", "--json", "--root", root], &dir);
    assert_eq!(fixture::code(&shown), 0, "{}", fixture::stderr(&shown));
    let episode = round_trip("show.schema.json", &shown);
    assert_eq!(
        episode["episode"]["message_head"].as_str(),
        Some("commit 1")
    );
    assert_eq!(
        episode["episode"]["parents"].as_array().map(Vec::len),
        Some(1),
        "the parent is printed, not elided: {episode}"
    );

    let stats = fixture::fathom(&["stats", root, "--json"], &dir);
    assert_eq!(fixture::code(&stats), 0, "{}", fixture::stderr(&stats));
    let report = round_trip("stats.schema.json", &stats);
    assert_eq!(report["degraded"].as_bool(), Some(false));
    assert_eq!(
        report["index"]["commits"].as_u64(),
        Some(3),
        "repo scale is in the report: {report}"
    );

    let doctor = fixture::fathom(&["doctor", "--root", root, "--json"], &dir);
    assert_eq!(fixture::code(&doctor), 0, "{}", fixture::stderr(&doctor));
    let report = round_trip("doctor.schema.json", &doctor);
    assert_eq!(report["strict"].as_bool(), Some(false));
    assert!(
        report["sqlite"]["capabilities"]["STRICT"].as_bool() == Some(true),
        "the capability probe ran: {report}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// `fathom episodes --json` against `schemas/episodes.schema.json` (`reco-11b`): the reviewer's main list,
/// which had no headless form at all until this verb.
///
/// The fixture's three same-dated commits by one author segment into **one** episode, which is the point of
/// asserting the three counts separately: `index.episodes` is what the index holds, `page.matched` is that
/// filtered and `page.returned` is that paged. A script that asserts on the wrong one is asserting on a
/// number that moves when somebody else commits, and a list shorter than the index with nothing saying so is
/// the confusion the schema's three fields exist to prevent.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn episodes_json_conforms_to_its_schema() {
    let dir = fixture::repository("episodes");
    let root = dir.to_str().unwrap();
    assert_eq!(fixture::code(&fixture::fathom(&["index", root], &dir)), 0);

    let out = fixture::fathom(&["episodes", "--root", root, "--json"], &dir);
    assert_eq!(fixture::code(&out), 0, "{}", fixture::stderr(&out));
    let report = round_trip("episodes.schema.json", &out);
    assert_eq!(report["contract"], "fathom.cli.episodes/1");
    assert_eq!(report["index"]["episodes"].as_u64(), Some(1));
    assert_eq!(report["page"]["matched"].as_u64(), Some(1));
    assert_eq!(report["page"]["returned"].as_u64(), Some(1));
    assert_eq!(report["page"]["limit"].as_u64(), Some(500));
    assert_eq!(report["page"]["truncated"].as_bool(), Some(false));
    assert_eq!(report["page"]["order"].as_str(), Some("last_utc desc, id"));
    assert_eq!(
        report["review_counts"]["unread"].as_u64(),
        Some(1),
        "nothing has been read, and the breakdown says so: {report}"
    );
    assert_eq!(
        report["review_counts"]["accepted"].as_u64(),
        Some(0),
        "all six states are keyed, always: {report}"
    );
    let rows = report["episodes"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    // The acceptance criterion's columns, on the row rather than in a fixture somewhere — asserted as
    // *keys*, because a stated absence (`session: null`) is a column too, and a script keys off the name.
    let row = &rows[0];
    for column in [
        "id",
        "last",
        "author_kind",
        "n_add",
        "n_del",
        "n_files",
        "review_state",
        "session",
        "turn",
    ] {
        assert!(row.get(column).is_some(), "row has no {column}: {row}");
    }
    assert_eq!(row["summary"].as_str(), Some("commit 0"));
    assert_eq!(
        row["n_commits"].as_u64(),
        Some(3),
        "one episode, three commits"
    );
    assert_eq!(row["n_files"].as_u64(), Some(3));
    assert_eq!(row["n_add"].as_u64(), Some(3));
    assert_eq!(row["paths"].as_array().map(Vec::len), Some(3));
    assert_eq!(row["author_kind"].as_str(), Some("human"));
    assert_eq!(row["review_state"].as_str(), Some("unread"));
    assert!(
        row["session"].is_null() && row["turn"].is_null(),
        "no harness trailer named a session, and that is a null and not a blank: {row}"
    );

    // `--limit 0` is "every row the filters select", printed as `null` because a ceiling that does not
    // exist is not the same statement as a ceiling of zero.
    let rest = fixture::fathom(
        &["episodes", "--root", root, "--json", "--limit", "0"],
        &dir,
    );
    assert_eq!(fixture::code(&rest), 0, "{}", fixture::stderr(&rest));
    let report = round_trip("episodes.schema.json", &rest);
    assert!(report["page"]["limit"].is_null(), "{report}");
    assert_eq!(report["page"]["returned"].as_u64(), Some(1));

    // The filter echoes itself both ways: as typed, and as the instant the query used.
    let filtered = fixture::fathom(
        &[
            "episodes",
            "--root",
            root,
            "--json",
            "--state",
            "unread",
            "--since",
            "2023-01-01",
        ],
        &dir,
    );
    assert_eq!(
        fixture::code(&filtered),
        0,
        "{}",
        fixture::stderr(&filtered)
    );
    let report = round_trip("episodes.schema.json", &filtered);
    assert_eq!(report["filter"]["state"].as_str(), Some("unread"));
    assert_eq!(report["filter"]["since"].as_str(), Some("2023-01-01"));
    assert!(report["filter"]["since_utc_ms"].as_i64().is_some());
    assert_eq!(
        report["filter"]["text"].as_str(),
        Some("state=unread since=2023-01-01")
    );
    assert_eq!(report["page"]["matched"].as_u64(), Some(1));

    // An empty selection is exit 0 with a note, not an error: a script must be able to ask "how many
    // flagged episodes?" of a repository with none.
    let none = fixture::fathom(
        &["episodes", "--root", root, "--json", "--state", "flagged"],
        &dir,
    );
    assert_eq!(fixture::code(&none), 0, "{}", fixture::stderr(&none));
    let report = round_trip("episodes.schema.json", &none);
    assert_eq!(report["episodes"].as_array().map(Vec::len), Some(0));
    assert_eq!(report["page"]["matched"].as_u64(), Some(0));
    assert!(
        report["notes"]
            .as_array()
            .is_some_and(|notes| !notes.is_empty()),
        "an empty answer says why: {report}"
    );

    // The junk this schema has to refuse: a row with its identity removed, and a key nobody declared.
    let schema = Schema::load("episodes.schema.json");
    let mut value: Value =
        serde_json::from_str(fixture::stdout(&out).trim()).expect("the payload is JSON");
    value["episodes"][0]
        .as_object_mut()
        .expect("a row")
        .remove("id");
    assert!(
        !validate(&schema.document, &value, "$").is_empty(),
        "a row with no id validated clean"
    );
    let mut value: Value =
        serde_json::from_str(fixture::stdout(&out).trim()).expect("the payload is JSON");
    value["not_a_field"] = serde_json::json!(1);
    assert!(
        !validate(&schema.document, &value, "$").is_empty(),
        "an undeclared key validated clean against episodes.schema.json"
    );

    // No index: the refusal is an exit code and a remedy, and stdout stays clean enough to pipe.
    let cold = fixture::repository("episodes-cold");
    let refused = fixture::fathom(&["episodes", "--json"], &cold);
    assert_eq!(fixture::code(&refused), 12);
    assert!(
        fixture::stdout(&refused).trim().is_empty(),
        "a refusal prints no payload: {}",
        fixture::stdout(&refused)
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&cold).ok();
}

#[cfg(feature = "sqlite-bundled")]
#[test]
fn status_json_conforms_to_its_schema() {
    let dir = fixture::repository("status");
    let root = dir.to_str().unwrap();
    assert_eq!(fixture::code(&fixture::fathom(&["index", root], &dir)), 0);

    let out = fixture::fathom(&["status", root, "--json"], &dir);
    assert_eq!(fixture::code(&out), 0, "{}", fixture::stderr(&out));
    let report = round_trip("status.schema.json", &out);
    assert_eq!(report["daemon"]["source"].as_str(), Some("none"));
    assert_eq!(report["daemon"]["running"].as_bool(), Some(false));
    assert!(
        report["daemon"]["lease"]["path"].is_string(),
        "the lease is reported even when it is free: {report}"
    );
    assert!(
        report["diagnostics"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "an absent daemon is a finding with a remedy, not a blank: {report}"
    );
    assert!(
        report["diagnostics"][0]["remedy"]
            .as_str()
            .is_some_and(|remedy| remedy.contains("fathom daemon start")),
        "the remedy names the verb that starts one: {report}"
    );
    assert_eq!(
        report["index"]["commits"].as_u64(),
        Some(3),
        "the index half comes from the same gatherer `fathom stats` uses: {report}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn export_json_conforms_to_its_schema() {
    // `e5.7`: the verb writes an artefact and reports the run. The payload is the machine line about the
    // write — scope, contract, verdict, the sign-offs it carried — and the artefact itself is a file, so
    // this test checks the shape of the report and that the file it describes really exists.
    let dir = fixture::repository("export");
    let root = dir.to_str().expect("a temporary path in utf-8");
    assert_eq!(fixture::code(&fixture::fathom(&["index", root], &dir)), 0);
    let out = fixture::fathom(
        &[
            "export",
            "--json",
            "--root",
            ".",
            "--episodes",
            "HEAD~2..HEAD",
        ],
        &dir,
    );
    let value = round_trip("export.schema.json", &out);
    assert_eq!(
        fixture::code(&out),
        0,
        "an export that wrote its file exits clean: {}",
        fixture::stderr(&out)
    );
    assert_eq!(value["status"].as_str(), Some("written"));
    assert_eq!(value["writes_file"].as_bool(), Some(true));
    assert_eq!(
        value["record"]["contract"].as_str(),
        Some("fathom-review-record/1")
    );
    assert_eq!(
        value["record"]["integrity"]["verified"].as_bool(),
        Some(true)
    );
    let artifact = value["artifact"].as_str().expect("an artifact name");
    let path = dir.join(artifact);
    assert!(path.is_file(), "{artifact} was reported and not written");
    assert_eq!(
        value["bytes"].as_u64(),
        Some(std::fs::read(&path).expect("read the artefact").len() as u64),
        "the report's byte count is the file's"
    );
    // The refusal shape: no index, no payload, and the exit code that says why.
    let cold = fixture::repository("export-cold");
    let refused = fixture::fathom(&["export", "--json", "--root", "."], &cold);
    assert_eq!(fixture::code(&refused), 12);
    assert!(
        fixture::stdout(&refused).trim().is_empty(),
        "a refusal prints no payload: {}",
        fixture::stdout(&refused)
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&cold).ok();
}

/// `fathom hooks --json` against `schemas/hooks.schema.json`, with a payload that must fail.
///
/// The fixture stream is damaged on purpose — a torn tail, a refused line, a path that escapes — because the
/// refused-line diagnostics are the part of the payload most likely to be hand-rolled and least likely to be
/// exercised by a happy-path fixture.
#[test]
fn hooks_json_conforms_to_its_schema() {
    let dir = fixture::repository("hooks");
    let events = dir.join(".fathom");
    std::fs::create_dir_all(&events).expect("create .fathom");
    std::fs::write(
        events.join("events.jsonl"),
        concat!(
            r#"{"harness":"pi_json","schema_version":1,"seq":1,"session_id":"s7","source":"turn_start","ts":"2026-01-01T00:00:00.000Z"}"#,
            "\n",
            r#"{"harness":"pi_json","payload":{"path":"../escape"},"schema_version":1,"seq":2,"session_id":"s7","source":"file.edited","ts":"2026-01-01T00:00:01.000Z"}"#,
            "\n",
            "half a line, no newline",
        ),
    )
    .expect("write the events file");
    let root = dir.to_str().unwrap();

    for action in [vec!["--check"], vec!["--tail"], vec!["--replay"]] {
        let mut args = vec!["hooks", root];
        args.extend(action.iter().copied());
        args.push("--json");
        let out = fixture::fathom(&args, &dir);
        assert_eq!(fixture::code(&out), 0, "{}", fixture::stderr(&out));
        let report = round_trip("hooks.schema.json", &out);
        assert_eq!(report["contract"], "fathom.cli.hooks/1");
        assert_eq!(report["stream"]["protocol"], "fathom-hooks/1");
        assert_eq!(
            report["stream"]["turns"]["all_session_qualified"], true,
            "every turn id this build emits is session-qualified (ADR-003 §18.2 D4)"
        );
    }

    let trailer = fixture::fathom(&["hooks", root, "--trailer", "s7:3", "--json"], &dir);
    assert_eq!(fixture::code(&trailer), 0, "{}", fixture::stderr(&trailer));
    let report = round_trip("hooks.schema.json", &trailer);
    assert_eq!(report["action"], "trailer");
    assert_eq!(report["trailer"]["turn_id"], "s7:3");
    assert!(report["stream"].is_null());

    // The junk value that must fail, as every schema test must have: a stream report with its subject
    // removed, and a payload key the schema never declared.
    let mut hollow = fixture::stdout(&fixture::fathom(
        &["hooks", root, "--check", "--json"],
        &dir,
    ))
    .trim()
    .to_owned();
    let mut value: Value = serde_json::from_str(&hollow).expect("the payload is JSON");
    value.as_object_mut().expect("an object").remove("stream");
    hollow = serde_json::to_string(&value).unwrap();
    let schema = Schema::load("hooks.schema.json");
    let problems = validate(&schema.document, &value, "$");
    assert!(
        !problems.is_empty(),
        "a stream report with no `stream` validated clean:\n{problems:#?}\n{hollow}"
    );
    let mut extra = fixture::fathom(&["hooks", root, "--check", "--json"], &dir);
    let text = fixture::stdout(&extra).trim().to_owned();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    value["not_a_field"] = serde_json::json!(1);
    let problems = validate(&schema.document, &value, "$");
    assert!(
        !problems.is_empty(),
        "an undeclared key validated clean against hooks.schema.json"
    );
    let _ = round_trip;
    let _ = &mut extra;
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn every_schema_is_a_well_formed_document_with_documented_exit_codes() {
    let known: Vec<i32> = fathom_core::exit::ExitCode::ALL
        .iter()
        .map(|code| code.as_i32())
        .collect();
    for name in [
        "common.schema.json",
        "architecture.schema.json",
        "check.schema.json",
        "episodes.schema.json",
        "index.schema.json",
        "show.schema.json",
        "stats.schema.json",
        "doctor.schema.json",
        "export.schema.json",
        "hooks.schema.json",
        "ratchet.schema.json",
        "status.schema.json",
    ] {
        let schema = Schema::load(name);
        let document = &schema.document;
        assert_eq!(
            document["$schema"].as_str(),
            Some("https://json-schema.org/draft/2020-12/schema"),
            "{name} does not declare its draft"
        );
        assert!(
            document["$id"]
                .as_str()
                .is_some_and(|id| id.starts_with("https://"))
        );
        assert!(document["title"].as_str().is_some_and(|t| !t.is_empty()));
        assert!(
            document["description"]
                .as_str()
                .is_some_and(|t| !t.is_empty())
        );
        if name == "common.schema.json" {
            assert!(document["$defs"].is_object(), "the shared defs live here");
            continue;
        }
        let codes = document["x-fathom-exit-codes"]
            .as_array()
            .unwrap_or_else(|| panic!("{name} documents no exit codes"));
        assert!(!codes.is_empty());
        for row in codes {
            let code = row["code"].as_i64().expect("an exit code is a number") as i32;
            let name_of = row["name"].as_str().expect("an exit code has a name");
            assert!(
                known.contains(&code),
                "{name} documents code {code}, which fathom_core::exit does not define"
            );
            let expected = fathom_core::exit::ExitCode::try_from(code)
                .expect("a known code")
                .name();
            assert_eq!(
                name_of, expected,
                "{name} calls code {code} {name_of:?}, the contract calls it {expected:?}"
            );
            assert!(row["when"].as_str().is_some_and(|when| !when.is_empty()));
        }
    }
}

/// The schemas name the same store path the verbs print, and the contract id each payload carries.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_contract_id_in_a_payload_is_the_id_its_schema_declares() {
    let dir = fixture::repository("contract");
    let root = dir.to_str().unwrap();
    fixture::fathom(&["index", root], &dir);
    for (verb, schema_name, args) in [
        ("index", "index.schema.json", vec!["index", ".", "--json"]),
        ("show", "show.schema.json", vec!["show", "HEAD", "--json"]),
        ("stats", "stats.schema.json", vec!["stats", "."]),
    ] {
        let args: Vec<&str> = if verb == "stats" {
            vec!["stats", ".", "--json"]
        } else {
            args
        };
        let out = fixture::fathom(&args, &dir);
        let payload: Value = serde_json::from_str(fixture::stdout(&out).trim())
            .unwrap_or_else(|err| panic!("{verb} --json: {err}"));
        let schema = Schema::load(schema_name);
        assert_eq!(
            payload["contract"].as_str(),
            schema.document["x-fathom-contract"].as_str(),
            "{verb}'s payload and schema disagree about the contract id"
        );
    }
    let _ = root;
    std::fs::remove_dir_all(&dir).ok();
}

/// A schema that is not valid JSON is caught by `Schema::load`; this catches a schema file going missing.
///
/// One file per verb, plus one file per *model*: `episode-delta.schema.json` describes the delta document
/// `fathom analyze` publishes rather than a verb's wrapper, and `plugin-finding.schema.json` describes one
/// finding a detector outside this repository hands back, and they are in the same directory because they
/// are the same kind of promise — a checked-in contract a consumer pins, generated rather than hand-edited,
/// and version-governed (`tools/check_delta_compat.py`). It is listed here so a fifth verb's schema cannot go
/// missing and these two cannot go missing either.
///
/// `fathom-analysis-events.schema.json` is held out of this list for a different reason and says so in the
/// file's own `$comment`: it describes one *line* of a `.jsonl` stream rather than a payload envelope, and
/// `crates/fathom-core/tests/events.rs` is what diffs it against the types that own it.
#[test]
fn the_schema_directory_holds_one_file_per_verb() {
    let dir = schemas_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| name.ends_with(".schema.json") && name != "fathom.schema.json")
        .filter(|name| name != "fathom-analysis-events.schema.json")
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "architecture.schema.json",
            "check.schema.json",
            "common.schema.json",
            "doctor.schema.json",
            "episode-delta.schema.json",
            "episodes.schema.json",
            "export.schema.json",
            "hooks.schema.json",
            "index.schema.json",
            "plugin-finding.schema.json",
            "ratchet.schema.json",
            "show.schema.json",
            "stats.schema.json",
            "status.schema.json",
        ]
    );
}

/// The validator is itself tested, or every schema test below it could pass vacuously.
#[test]
fn the_validator_validates() {
    let schema: Value = serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["a", "b"],
        "properties": {
            "a": {"type": "integer", "minimum": 1},
            "b": {"type": ["string", "null"]},
            "c": {"type": "array", "items": {"type": "string"}},
        },
    });
    assert!(validate(&schema, &serde_json::json!({"a": 2, "b": null}), "$").is_empty());
    assert!(!validate(&schema, &serde_json::json!({"a": 0, "b": ""}), "$").is_empty());
    assert!(!validate(&schema, &serde_json::json!({"a": 2}), "$").is_empty());
    assert!(!validate(&schema, &serde_json::json!({"a": 2, "b": "", "d": 1}), "$").is_empty());
    assert!(
        !validate(
            &schema,
            &serde_json::json!({"a": 2, "b": "", "c": [1]}),
            "$"
        )
        .is_empty()
    );
}

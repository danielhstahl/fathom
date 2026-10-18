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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Output;

/// The schemas live at the workspace root, next to the config schema `e0.8` generates.
fn schemas_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/fathom-cli is two levels below the workspace root")
        .join("schemas")
}

/// A schema document plus the directory it came from, so `$ref` to a sibling file resolves.
#[derive(Clone)]
struct Schema {
    document: serde_json::Value,
}

impl Schema {
    fn load(name: &str) -> Schema {
        let dir = schemas_dir();
        let text =
            std::fs::read_to_string(dir.join(name)).unwrap_or_else(|err| panic!("{name}: {err}"));
        let _ = dir;
        Schema {
            document: serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("{name} is not valid JSON: {err}")),
        }
    }
}

/// Validate `value` against `schema`, returning every problem found (empty means it conforms).
fn validate(schema: &serde_json::Value, value: &serde_json::Value, at: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(object) = schema.as_object() else {
        return problems;
    };

    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        let resolved = resolve_ref(reference, schema_root(schema));
        return validate(&resolved, value, at);
    }
    // A `$ref` alongside siblings is legal in 2020-12 but is not what these schemas do; keep the simpler
    // rule and let the schema-well-formedness test complain if it ever changes.
    if let Some(types) = object.get("type")
        && !type_matches(types, value)
    {
        problems.push(format!(
            "{at}: expected type {types}, got {}",
            kind_of(value)
        ));
        return problems;
    }
    if let Some(allowed) = object.get("enum").and_then(Value::as_array)
        && !allowed.iter().any(|candidate| candidate == value)
    {
        problems.push(format!("{at}: {value} is not one of {allowed:?}"));
    }
    if let Some(wanted) = object.get("const")
        && wanted != value
    {
        problems.push(format!("{at}: expected the constant {wanted}, got {value}"));
    }
    if let Some(minimum) = object.get("minimum").and_then(Value::as_f64)
        && let Some(number) = value.as_f64()
        && number < minimum
    {
        problems.push(format!("{at}: {number} is below the minimum {minimum}"));
    }
    if let Some(maximum) = object.get("maximum").and_then(Value::as_f64)
        && let Some(number) = value.as_f64()
        && number > maximum
    {
        problems.push(format!("{at}: {number} is above the maximum {maximum}"));
    }
    if let Some(options) = object.get("oneOf").and_then(Value::as_array) {
        let matches = options
            .iter()
            .filter(|option| validate(option, value, at).is_empty())
            .count();
        if matches != 1 {
            problems.push(format!(
                "{at}: matched {matches} of the {} oneOf options",
                options.len()
            ));
        }
    }

    if let Some(array) = value.as_array()
        && let Some(items) = object.get("items")
    {
        for (index, entry) in array.iter().enumerate() {
            problems.extend(validate(items, entry, &format!("{at}[{index}]")));
        }
    }

    if let Some(object_value) = value.as_object() {
        for key in object
            .get("required")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let name = key.as_str().unwrap_or_default();
            if !object_value.contains_key(name) {
                problems.push(format!("{at}: required property {name:?} is missing"));
            }
        }
        let properties = object.get("properties").and_then(Value::as_object);
        let additional = object.get("additionalProperties");
        for (key, entry) in object_value {
            let path = format!("{at}.{key}");
            if let Some(properties) = properties
                && let Some(sub) = properties.get(key)
            {
                problems.extend(validate(sub, entry, &path));
                continue;
            }
            match additional {
                Some(Value::Bool(false)) => {
                    problems.push(format!(
                        "{path}: not in the schema, and additionalProperties is false"
                    ));
                }
                Some(sub) if !sub.is_boolean() => {
                    problems.extend(validate(sub, entry, &path));
                }
                _ => {}
            }
        }
    }
    problems
}

/// Where the current schema came from, so a `$ref` to a sibling file can be found. Threaded through by a
/// side channel rather than a parameter because only `$ref` needs it.
fn schema_root(_schema: &serde_json::Value) -> PathBuf {
    schemas_dir()
}

/// `{"$ref": "common.schema.json#/$defs/finding"}` — a sibling file and a JSON pointer into it.
fn resolve_ref(reference: &str, root: PathBuf) -> serde_json::Value {
    let (file, pointer) = match reference.split_once('#') {
        Some((file, pointer)) => (file, pointer),
        None => (reference, ""),
    };
    let document: serde_json::Value = if file.is_empty() {
        // A local `$ref` is resolved by the caller's own document; the only local refs used are
        // `#/$defs/...` inside the file being validated, whose root the validator walks anyway.
        return local_ref(pointer);
    } else {
        let path = root.join(file);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        serde_json::from_str(&text).expect("the schema file is valid JSON")
    };
    follow(pointer, &document)
}

/// The document the current `$ref` is relative to. The validator is called with one file open at a time;
/// `SCHEMAS` holds them all so a cross-file ref resolves without re-reading from disk mid-walk.
static SCHEMAS: std::sync::OnceLock<BTreeMap<String, serde_json::Value>> =
    std::sync::OnceLock::new();

fn local_ref(pointer: &str) -> serde_json::Value {
    let schemas = SCHEMAS
        .get()
        .expect("a schema was loaded before a local $ref");
    // A local `$ref` is looked up in every open schema; the keys are unique across them in practice, and a
    // miss is a hard failure rather than a silent pass.
    for document in schemas.values() {
        let found = follow(pointer, document);
        if !found.is_null() {
            return found;
        }
    }
    panic!("local $ref {pointer} resolved in none of the loaded schemas");
}

fn follow(pointer: &str, document: &serde_json::Value) -> serde_json::Value {
    let mut current = document.clone();
    for segment in pointer
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        let key = segment.replace("~1", "/").replace("~0", "~");
        current = match current {
            serde_json::Value::Object(map) => map.get(&key).cloned().unwrap_or_default(),
            _ => return serde_json::Value::Null,
        };
    }
    current
}

fn type_matches(types: &serde_json::Value, value: &serde_json::Value) -> bool {
    let candidates: Vec<&str> = match types {
        serde_json::Value::String(name) => vec![name.as_str()],
        serde_json::Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
        _ => return true,
    };
    candidates.iter().any(|name| match *name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    })
}

fn kind_of(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

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
    SCHEMAS
        .set(
            ["common.schema.json", schema_name]
                .into_iter()
                .map(|name| {
                    let text =
                        std::fs::read_to_string(schemas_dir().join(name)).expect("schema file");
                    (
                        name.to_owned(),
                        serde_json::from_str::<Value>(&text).expect("schema is JSON"),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        )
        .ok();

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

#[test]
fn export_json_conforms_to_its_schema() {
    let dir = fixture::repository("export");
    let out = fixture::fathom(&["export", "--json", "--root", "."], &dir);
    let value = round_trip("export.schema.json", &out);
    assert_eq!(
        fixture::code(&out),
        1,
        "the skeleton exits ERROR: {}",
        fixture::stderr(&out)
    );
    assert_eq!(value["status"].as_str(), Some("not_implemented"));
    assert_eq!(value["owner_ticket"].as_str(), Some("e5.7"));
    std::fs::remove_dir_all(&dir).ok();
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
        "index.schema.json",
        "show.schema.json",
        "stats.schema.json",
        "doctor.schema.json",
        "export.schema.json",
        "hooks.schema.json",
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
#[test]
fn the_schema_directory_holds_one_file_per_verb() {
    let dir = schemas_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| name.ends_with(".schema.json") && name != "fathom.schema.json")
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "common.schema.json",
            "doctor.schema.json",
            "export.schema.json",
            "hooks.schema.json",
            "index.schema.json",
            "show.schema.json",
            "stats.schema.json",
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

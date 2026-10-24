//! The delta contract: `schemas/episode-delta.schema.json`, the model that generates it, and every shape
//! the model can emit — acceptance criteria 1 and 2 of `reco-tt4`.
//!
//! Four claims, each one a failure mode that has already happened somewhere else:
//!
//! 1. **The checked-in schema is the generated one.** A hand-edited schema is a second source of truth
//!    with a nicer file name. `fathom analyze --schema --write` is the only blessed way to change it.
//! 2. **Every delta this build can emit conforms.** [`corpus`] walks the shapes the model produces — no
//!    rows, rollup rows, a rename with a unicode path, a binary, generated noise, a lent dependency
//!    block, a capped read, a ranking with its components — and each is serialised and validated.
//! 3. **Every key a document writes is declared.** `additionalProperties` is `true` on purpose (an
//!    additive change must validate against the older schema it is still versioned as), so the schema
//!    cannot enforce closedness — this test does it from the other direction instead.
//! 4. **The gate bites.** A document missing a required key, or with a key of the wrong type, fails.
//!
//! The version policy — additive keeps the version, breaking bumps it — is `tools/check_delta_compat.py`,
//! which compares against the archived baseline of the previous version in the CI job that has no Rust.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::process::Command;

use fathom_analyze::insight::{
    self, EpisodeDelta, EpisodeRef, Evidence, FileChange, Finding, Input, Provenance, RiskView,
    Signals, Supplied, SuppliedBlock, classify, rank,
};
use serde_json::Value;

#[path = "validator.rs"]
mod validator;

use validator::{Schema, validate};

/// The published schema, from the checked-in file and not from the generator: this file's job is to catch
/// the two disagreeing, so it may not read the generator's output as its input.
fn schema() -> Schema {
    validator::open(&["episode-delta.schema.json"]);
    Schema::load("episode-delta.schema.json")
}

/// One shape the model can be in, with the name it is reported under.
fn corpus() -> Vec<(&'static str, EpisodeDelta)> {
    let mut corpus: Vec<(&'static str, EpisodeDelta)> = Vec::new();

    // The floor: an episode with no rows at all. Every section is `unavailable` here, and each gap names
    // the plan key that owns the answer — which is the whole reason the model exists.
    corpus.push((
        "no-rows",
        EpisodeDelta::sparse("e-aaaa1111", "backfill the archive index", None),
    ));

    // A bare score, which is what a caller that has a number and nothing else can lend.
    corpus.push((
        "score-only",
        EpisodeDelta::sparse(
            "e-bbbb2222",
            "vendor the TLS handshake",
            Some(RiskView::scored(41)),
        ),
    ));

    // A real ranking: components, vector, surfaces, blind spots, and a rank within a set.
    // An array, not a `Vec`: it is borrowed and iterated, never grown, and the row count is the fixture.
    let files = [
        ("src/store/epoch.rs", 412, 90, false, "source", "none"),
        ("tests/store/epoch.rs", 88, 12, false, "test", "none"),
        ("migrations/0007.sql", 40, 0, false, "migration", "none"),
        ("assets/blob.bin", 0, 0, true, "binary", "generated"),
        ("src/café/naïve.rs", 12, 3, false, "source", "none"),
    ];
    let changes: Vec<FileChange<'_>> = files
        .iter()
        .map(|(path, add, del, binary, class, noise)| FileChange {
            path,
            add: *add,
            del: *del,
            binary: *binary,
            path_class: class,
            noise_class: noise,
        })
        .collect();
    let ranking = rank(&Signals {
        files: &changes,
        n_add: 552,
        n_del: 105,
        n_files: 5,
        n_binary: 1,
        n_commits: 4,
        author: "agent",
        partial_files: false,
    });
    corpus.push((
        "ranked",
        EpisodeDelta::sparse(
            "e-cccc3333",
            "make the store epoch-safe",
            Some(RiskView::of(&ranking, Some(1), Some(3))),
        ),
    ));

    // Rows, so the sections have something to measure: a rename, a unicode path, a binary, generated
    // noise, and the totals the episode rollup recorded.
    let mut input = Input::new(EpisodeRef::sparse(
        "e-dddd4444",
        "split the ingest worker pool",
    ));
    input.totals.n_files = 5;
    input.totals.n_add = 552;
    input.totals.n_del = 105;
    input.totals.n_binary = 1;
    input.episode.n_commits = 4;
    input.episode.author_kind = "agent".to_owned();
    input.files = files
        .iter()
        .enumerate()
        .map(
            |(index, (path, add, del, binary, class, noise))| fathom_analyze::insight::FileDelta {
                path: (*path).to_owned(),
                old_path: (index == 4).then(|| "src/cafe/naive.rs".to_owned()),
                change: if index == 4 { "renamed" } else { "modified" }.to_owned(),
                add: i64::try_from(*add).unwrap_or(i64::MAX),
                del: i64::try_from(*del).unwrap_or(i64::MAX),
                binary: *binary,
                commits: 1,
                path_class: (*class).to_owned(),
                noise_class: (*noise).to_owned(),
                surface: classify(path, class, noise).as_str().to_owned(),
                basis: Provenance::DETAIL_FULL.to_owned(),
            },
        )
        .collect();
    input.risk = Some(RiskView::of(&ranking, Some(2), Some(9)));
    corpus.push(("measured", insight::compose(&input)));

    // A capped read: the rows seen are fewer than the episode counted, and `reconciles` has to say
    // whether the sums still add up rather than quietly pretending they do.
    let mut capped = input.clone();
    capped.totals.n_files = 400;
    capped.totals.files_truncated = true;
    capped.provenance.files_detail = Provenance::DETAIL_ROLLUP.to_owned();
    corpus.push(("truncated-read", insight::compose(&capped)));

    // Lent analyses: the sections that are `unavailable` to this build become `measured` because a
    // caller that owns them handed them over. This is the seam `e2.6`/`e2.9`/`e3.2`/`e4.3` arrive through,
    // and it is the reason the TUI can gain a figure without gaining a computation.
    let mut lent = input.clone();
    lent.supplied = Supplied {
        dependencies: Some(SuppliedBlock {
            headline: "2 added, 1 removed, 3 bumped".to_owned(),
            basis: "resolved imports".to_owned(),
            figures: BTreeMap::from([
                ("added".to_owned(), "2".to_owned()),
                ("bumped".to_owned(), "3".to_owned()),
            ]),
            confidence: Some(0.9),
            owner: "e2.6".to_owned(),
            run: None,
        }),
        tests: Some(SuppliedBlock {
            headline: "1 case deleted, none added".to_owned(),
            basis: "test names".to_owned(),
            figures: BTreeMap::from([("deleted".to_owned(), "1".to_owned())]),
            confidence: None,
            owner: "e2.9".to_owned(),
            run: None,
        }),
        blast: None,
        surface: None,
        structure: None,
        findings: vec![Finding {
            kind: "test-deleted-without-coverage".to_owned(),
            subject: "tests/store/epoch.rs".to_owned(),
            message: "a case was deleted and nothing was added in its place".to_owned(),
            severity: "high".to_owned(),
            evidence: Evidence::inside("#/files"),
            owner: "e4.3".to_owned(),
        }],
    };
    corpus.push(("supplied", insight::compose(&lent)));

    // Everything lent at once, which is the shape the day every analysis exists; the sections must not
    // overflow their caps, and the remainders must be counted.
    let mut full = lent.clone();
    full.supplied.blast = Some(SuppliedBlock {
        headline: "14 modules reachable".to_owned(),
        basis: "module graph".to_owned(),
        figures: BTreeMap::from([("reachable".to_owned(), "14".to_owned())]),
        confidence: Some(0.7),
        owner: "e3.2".to_owned(),
        run: None,
    });
    full.supplied.surface = Some(SuppliedBlock {
        headline: "docs touched, no api change".to_owned(),
        basis: "path classes".to_owned(),
        figures: BTreeMap::new(),
        confidence: None,
        owner: "e2.8".to_owned(),
        run: None,
    });
    full.supplied.structure = Some(SuppliedBlock {
        headline: "one module split, churn band low".to_owned(),
        basis: "path prefixes".to_owned(),
        figures: BTreeMap::from([("bands".to_owned(), "low".to_owned())]),
        confidence: None,
        owner: "e2.3".to_owned(),
        run: None,
    });
    corpus.push(("fully-lent", insight::compose(&full)));

    corpus
}

/// Validate one document against the published schema, and fail with the document in the message.
fn conforms(name: &str, delta: &EpisodeDelta) {
    let value = delta.to_value();
    let problems = validate(&schema().document, &value, "$");
    assert!(
        problems.is_empty(),
        "{name} does not conform to {}: {}\n{}",
        insight::SCHEMA_FILE,
        problems.join("\n"),
        delta.to_json_pretty()
    );
}

/// Every object key the document writes, as `<parent>.<key>` pairs.
fn keys(value: &Value, at: &str, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                out.insert(format!("{at}.{key}"), kind(child).to_owned());
                keys(child, &format!("{at}.{key}"), out);
            }
        }
        Value::Array(items) => {
            for item in items {
                keys(item, &format!("{at}[]"), out);
            }
        }
        other => {
            let _ = other;
        }
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        // `integer` and `number` are one JSON type and two schema types; conflating them here is how a
        // schema that says `integer` passes while the model writes `1.5`.
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Every property name the schema declares, anywhere, with the type it declares for it.
fn collect_declared(value: &Value, out: &mut BTreeMap<String, Vec<String>>) {
    match value {
        Value::Object(map) => {
            if let Some(properties) = map.get("properties").and_then(Value::as_object) {
                for (key, child) in properties {
                    // A property declared without a `type` — a `$defs` placeholder, an untyped object —
                    // declares *shape* but not *kind*, so it accepts whatever the model writes there.
                    // Recorded as `any` rather than skipped: one untyped declaration of a name must not be
                    // silenced by a second, typed declaration of the same leaf name somewhere else in the
                    // document (`evidence` is a figure's evidence object and an attribution's evidence
                    // array at once, and both are correct where they live).
                    let types = child.get("type").map_or_else(
                        || vec!["any".to_owned()],
                        |t| match t {
                            Value::String(one) => vec![one.clone()],
                            Value::Array(many) => many
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect(),
                            other => vec![other.to_string()],
                        },
                    );
                    out.entry(key.clone()).or_default().extend(types);
                }
            }
            for child in map.values() {
                collect_declared(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_declared(item, out);
            }
        }
        other => {
            let _ = other;
        }
    }
}

#[test]
fn the_checked_in_schema_is_what_the_generator_writes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/fathom-cli is two levels below the workspace root")
        .join(insight::SCHEMA_FILE);
    let checked_in =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    assert_eq!(
        checked_in,
        insight::schema_to_text(),
        "{} is stale. Regenerate with `fathom analyze --schema --write`; if the change is breaking, \
         bump `fathom_analyze::insight::VERSION` first and let tools/check_delta_compat.py bless it.",
        path.display()
    );
}

#[test]
fn the_schema_declares_the_version_the_model_writes() {
    let document = schema().document;
    assert_eq!(
        document["x-fathom"]["version"].as_u64(),
        Some(u64::from(insight::SCHEMA_VERSION)),
        "the schema's declared version and the model's must be one number"
    );
    assert_eq!(
        document["x-fathom"]["contract"].as_str(),
        Some(insight::CONTRACT),
        "the schema's contract id and the payload's must be one string"
    );
    assert_eq!(
        document["properties"]["schema_version"]["const"].as_u64(),
        Some(u64::from(insight::SCHEMA_VERSION)),
        "every payload repeats `schema_version`; the schema pins it, so a consumer can refuse what it \
         cannot read before it reads it"
    );
    assert_eq!(
        document["properties"]["contract"]["const"].as_str(),
        Some(insight::CONTRACT)
    );
    assert_eq!(
        document["additionalProperties"].as_bool(),
        Some(true),
        "an additive change must validate against the schema it predates, or `additive` is a word \
         with no meaning"
    );
}

#[test]
fn every_delta_in_the_corpus_validates_and_verifies() {
    for (name, delta) in corpus() {
        let problems = delta.verify();
        assert!(
            problems.is_empty(),
            "{name} does not verify against its own model: {problems:?}"
        );
        conforms(name, &delta);

        // The round trip is the contract's other half: an artefact written today has to read back as the
        // same document in a year, and an MCP tool result is the same bytes through a JSON round trip.
        let text = delta.to_json();
        let again = EpisodeDelta::from_json(&text).expect("{name} must re-read its own JSON");
        assert_eq!(again, delta, "{name} does not survive its own JSON");
        assert!(
            validate(&schema().document, &again.to_value(), "$").is_empty(),
            "{name} conforms before the round trip and not after"
        );
    }
}

#[test]
fn every_key_a_document_writes_is_declared_by_the_schema() {
    let published = schema().document;
    let mut declared = BTreeMap::new();
    collect_declared(&published, &mut declared);

    let mut written = BTreeMap::new();
    for (name, delta) in corpus() {
        let before = written.len();
        keys(&delta.to_value(), "$", &mut written);
        let _ = (name, before);
    }
    let mut missing: Vec<String> = Vec::new();
    for (path, kind) in &written {
        // Two maps have keys the *producer* invents — a figure's `raw` integers and `provenance.models`
        // — and they are declared by `additionalProperties`, not by a name per key. Matching a free-form
        // key against a property of the same name elsewhere in the schema would only ever produce a false
        // alarm (a `raw.binary` integer is not the `binary` boolean on a file row).
        if path.contains(".raw.") || path.contains(".models.") {
            continue;
        }
        let key = path.rsplit('.').next().unwrap_or(path);
        match declared.get(key) {
            None => missing.push(format!("{path} ({kind}) is written and never declared")),
            Some(types) if types.iter().any(|t| t == kind || t == "any") => {}
            // A property declared with no `type` (a `$defs` placeholder, say) is a declaration the
            // test cannot check the type of, and saying so is better than pretending it matched.
            Some(types) if types.is_empty() => {}
            Some(types) => missing.push(format!(
                "{path} is written as {kind} and declared as {}",
                types.join("|")
            )),
        }
    }
    assert!(
        missing.is_empty(),
        "the published schema does not describe the model:\n{}",
        missing.join("\n")
    );
}

/// The forward-compatibility promise, tested rather than asserted: a document carrying a field this
/// build has never heard of is a document from tomorrow's v1, and it must validate today.
#[test]
fn a_document_with_a_field_this_build_does_not_know_still_validates() {
    let mut value = EpisodeDelta::sparse("e-eeee5555", "from tomorrow", None).to_value();
    value["figure_from_the_future"] = serde_json::json!({"known": true, "value": "9"});
    value["episode"]["trust"] = serde_json::json!("human-confirmed");
    let problems = validate(&schema().document, &value, "$");
    assert!(
        problems.is_empty(),
        "an additive change must not break a v1 reader: {problems:?}"
    );

    // …and the v1 reader must ignore it, not reject the document or lose the rest of it.
    let read =
        EpisodeDelta::from_json(&value.to_string()).expect("an unknown field is not an error");
    assert_eq!(read.episode.id, "e-eeee5555");
    assert!(
        read.verify().is_empty(),
        "a borrowed document still self-checks: {:?}",
        read.verify()
    );
}

/// The gate has to reject something, or it is decoration.
#[test]
fn the_schema_rejects_a_document_that_is_not_the_model() {
    let document = schema().document;
    let good = EpisodeDelta::sparse("e-ffff6666", "one episode", None).to_value();
    assert!(
        validate(&document, &good, "$").is_empty(),
        "the fixture must be valid"
    );

    // A required key gone: `gaps` is redundant with the section gaps and required anyway, because two
    // lists that can disagree is the bug, not the design.
    let mut holes = good.clone();
    holes.as_object_mut().expect("an object").remove("gaps");
    assert!(
        !validate(&document, &holes, "$").is_empty(),
        "a document with no `gaps` list is not the model, and the schema must say so"
    );

    // A version this build does not speak: `schema_version` is a `const`, so v2 is a type error.
    let mut future = good.clone();
    future["schema_version"] = serde_json::json!(insight::SCHEMA_VERSION + 1);
    assert!(
        !validate(&document, &future, "$").is_empty(),
        "a v{} document must not validate against the v{} schema — that is what the version is for",
        insight::SCHEMA_VERSION + 1,
        insight::SCHEMA_VERSION
    );

    // A wrong type where the model is the only thing that could have produced it.
    let mut wrong = good.clone();
    wrong["totals"]["n_files"] = serde_json::json!("5");
    assert!(!validate(&document, &wrong, "$").is_empty());

    // An enum member that is not one of the three states.
    let mut state = good.clone();
    state["sections"][0]["state"] = serde_json::json!("probably-fine");
    assert!(
        !validate(&document, &state, "$").is_empty(),
        "three states, spelled exactly: `probably-fine` is not one of them"
    );
}

/// The seven sections are the contract: every document carries all seven, in the order the model
/// publishes, and the state vocabulary is exactly the three spellings.
///
/// The section *id* is deliberately not an `enum` in the schema — a reader that hard-fails on an id it
/// has not met turns an appended eighth section into an outage, which is the opposite of the policy. The
/// ordering and completeness are the model's job ([`EpisodeDelta::verify`] checks them on every document
/// it emits), so they are asserted here against the corpus rather than in the schema.
#[test]
fn the_seven_sections_are_pinned_in_number_and_order() {
    let document = schema().document;
    assert_eq!(
        document["$defs"]["sectionState"]["enum"]
            .as_array()
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["measured", "partial", "unavailable"]),
        "three states and three spellings: a fourth is a breaking change, not a string"
    );
    for (name, delta) in corpus() {
        let ids: Vec<&str> = delta
            .sections
            .iter()
            .map(|section| section.id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec![
                "what",
                "where",
                "dependencies",
                "surface",
                "tests",
                "blast",
                "attention"
            ],
            "{name} does not carry the seven sections, in order"
        );
    }
}

/// The end to end that the ticket is actually about: the verb, the store, the schema, one line of JSON.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_analyze_verb_prints_one_schema_valid_document() {
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    let indexed = run(&["index", root]);
    assert_eq!(code(&indexed), 0, "{}", stderr(&indexed));

    let text = stdout(&run(&["analyze", "HEAD", "--root", root, "--json"]));
    assert!(
        text.lines().count() == 1,
        "`--json` is one line on stdout and nothing else there: {text}"
    );
    let value: Value = serde_json::from_str(text.trim()).expect("`--json` prints JSON");
    let problems = validate(&schema().document, &value, "$");
    assert!(
        problems.is_empty(),
        "`fathom analyze --json` does not conform: {}\n{text}",
        problems.join("\n")
    );

    // The document the verb prints is the model's, not a re-serialisation of it: parse it back and the
    // self-check has to be the one the model ran.
    let delta = EpisodeDelta::from_json(text.trim()).expect("the verb prints the model");
    assert!(
        delta.verify().is_empty(),
        "the verb printed a document that fails its own check"
    );
    assert_eq!(
        delta.integrity.verified,
        delta.verify().is_empty(),
        "`integrity` must be the check's result and not a copy of last run's"
    );

    // The text rendering is the same document's words, and the card reads the same sections the CLI
    // printed — one model, three surfaces, no second computation.
    let rendered = stdout(&run(&["analyze", "HEAD", "--root", root]));
    assert!(rendered.contains("what"), "{rendered}");
    assert!(rendered.contains("fathom-delta/1"), "{rendered}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The artefact is a rendering of the one model, not a second one: `e5.7`'s JSON export carries the
/// delta documents themselves, and `--json` reports the same seven sections the model states.
///
/// This is the ADR-001 §7.10 claim made checkable at the process boundary. An artefact that quietly
/// invented its own section list, or re-worded a state, would be the second reality the narrow waist
/// exists to prevent — so the check is that the artefact's facts are the model's facts, byte for byte,
/// including when the model has nothing: a stated absence with the owner named, never an empty list that
/// reads as "no gaps".
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_export_is_a_rendering_of_the_one_model() {
    let dir = repository();
    let root = dir.to_str().expect("a temporary path in utf-8");
    assert_eq!(code(&run(&["index", root])), 0);

    // The machine report: seven sections, in the model's order, with the contract they came from.
    let out = run(&[
        "export",
        "--json",
        "--root",
        root,
        "--episodes",
        "HEAD~1..HEAD",
        "--out",
        dir.join("artefact").to_str().expect("a utf-8 path"),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let value: Value =
        serde_json::from_str(stdout(&out).trim()).expect("export --json prints JSON");
    let sections = value["record"]["sections"]
        .as_array()
        .expect("the report carries the sections");
    assert_eq!(
        sections.len(),
        7,
        "all seven sections, in the model's order, with each one's state: {value}"
    );
    assert_eq!(
        sections
            .iter()
            .map(|section| section["id"].as_str().unwrap_or(""))
            .collect::<Vec<_>>(),
        vec![
            "what",
            "where",
            "dependencies",
            "surface",
            "tests",
            "blast",
            "attention"
        ],
        "the artefact asks the model's seven questions, in the model's order"
    );
    assert_eq!(
        value["record"]["contract"].as_str(),
        Some("fathom-review-record/1")
    );

    // The artefact itself: the delta documents, verbatim, with the model's own contract and version. The
    // JSON format *is* the record, which is what makes this a check rather than a screenshot.
    let written = run(&[
        "export",
        "--root",
        root,
        "--format",
        "json",
        "--episodes",
        "HEAD~1..HEAD",
        "--out",
        dir.join("artefact").to_str().expect("a utf-8 path"),
    ]);
    assert_eq!(code(&written), 0, "{}", stderr(&written));

    let artefact =
        std::fs::read_to_string(dir.join("artefact.json")).expect("the artefact was written");
    let record: Value = serde_json::from_str(&artefact).expect("the JSON artefact is JSON");
    assert_eq!(record["contract"].as_str(), Some("fathom-review-record/1"));
    assert_eq!(
        record["scope"]["delta_contract"].as_str(),
        Some(insight::CONTRACT),
        "the artefact names the contract its figures came from: {record}"
    );
    assert_eq!(
        record["scope"]["delta_version"].as_u64(),
        Some(u64::from(insight::SCHEMA_VERSION))
    );
    let episodes = record["episodes"]
        .as_array()
        .expect("one entry per episode");
    assert!(!episodes.is_empty(), "the range held an episode");
    for episode in episodes {
        let delta = &episode["delta"];
        assert_eq!(delta["contract"].as_str(), Some(insight::CONTRACT));
        assert_eq!(
            delta["integrity"]["verified"].as_bool(),
            Some(true),
            "the artefact carries a document that verifies: {delta}"
        );
        assert!(
            !episode["narrative_markdown"]
                .as_str()
                .unwrap_or_default()
                .is_empty(),
            "the narrative travels with the record"
        );
    }

    // And with no index, the verb refuses with a remedy instead of writing an empty artefact.
    let bare = repository();
    let bare_root = bare.to_str().expect("a temporary path in utf-8");
    let cold = run(&[
        "export",
        "--json",
        "--root",
        bare_root,
        "--episodes",
        "HEAD",
    ]);
    assert_eq!(
        code(&cold),
        12,
        "no index is INDEX_DEGRADED, not an empty artefact: {}",
        stdout(&cold)
    );
    assert!(
        stderr(&cold).contains("fathom index"),
        "the absence names what to run: {}",
        stderr(&cold)
    );
    assert!(
        !dir.join("no-artefact.md").exists(),
        "a refusal writes nothing"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&bare);
}

#[cfg(feature = "sqlite-bundled")]
fn repository() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-delta-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    std::fs::write(dir.join("lib.rs"), "fn a() {}\n").expect("write the fixture file");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "add the module"]);
    std::fs::write(dir.join("lib.rs"), "fn a() {}\nfn b() {}\n").expect("write the fixture file");
    std::fs::write(dir.join("tests/one.rs"), "#[test] fn t() {}\n").ok();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "add b and its test"]);
    dir
}

#[cfg(feature = "sqlite-bundled")]
fn git(cwd: &std::path::Path, args: &[&str]) {
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
    assert!(out.status.success(), "git {args:?}: {}", stderr_bytes(&out));
}

#[cfg(feature = "sqlite-bundled")]
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .output()
        .expect("the built fathom binary is runnable")
}

#[cfg(feature = "sqlite-bundled")]
fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[cfg(feature = "sqlite-bundled")]
fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[cfg(feature = "sqlite-bundled")]
fn stderr_bytes(out: &std::process::Output) -> String {
    stderr(out)
}

#[cfg(feature = "sqlite-bundled")]
fn code(out: &std::process::Output) -> i32 {
    out.status.code().expect("fathom exits with a code")
}

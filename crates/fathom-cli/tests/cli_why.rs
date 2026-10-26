//! `fathom why <episode>`, out of the shipped binary (`reco-884`, `e4.4`).
//!
//! The first acceptance criterion is a sentence about a command, so it is tested as a command: this file
//! builds a repository, runs `fathom index`, and asks the binary to explain one episode's score. Nothing
//! here imports the scorer — a test that calls the library would pass even if the verb printed something
//! else, and the verb is the deliverable.
//!
//! The repository is eight commits, four of them the shapes the fixture generator plants (a migration, a
//! CI permission change, a turn that deleted its own test file, an untested public API) and four of them
//! ordinary filler. The distinction that has to survive all the way to the printed page is the one
//! `reco-c05` drew for the queue: a 400-line documentation edit is not the riskiest thing in the room.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// One file in one commit: keep this many of its current lines, then add this many new ones.
#[derive(Clone, Copy)]
struct Op {
    path: &'static str,
    keep: usize,
    add: usize,
}

/// One commit, and therefore one episode: an hour apart, past the 30-minute gap rule (ADR-003 §4).
struct Shape {
    subject: &'static str,
    agent: bool,
    files: &'static [Op],
}

const fn op(path: &'static str, keep: usize, add: usize) -> Op {
    Op { path, keep, add }
}

const fn shape(subject: &'static str, files: &'static [Op]) -> Shape {
    Shape {
        subject,
        agent: true,
        files,
    }
}

const fn human(subject: &'static str, files: &'static [Op]) -> Shape {
    Shape {
        subject,
        agent: false,
        files,
    }
}

const SHAPES: &[Shape] = &[
    shape(
        "feat: narrow the booking query",
        &[op("src/harbour/store.py", 0, 24)],
    ),
    // The largest addition in the fixture, and the one that must not be mistaken for risk: prose, by a
    // named human, with nothing to execute and no finding to its name.
    human(
        "docs: a 400-line guide for the new hire",
        &[op("docs/onboarding.md", 0, 400)],
    ),
    // The destructive migration: twelve lines, the only database path in the fixture, and the shape the
    // `migration-destructive` detector exists for (e4.3).
    shape(
        "migrations: drop customers.email_backup",
        &[
            op("db/migrations/0038_drop_email_backup.sql", 0, 12),
            op("src/harbour/schema.py", 30, 6),
        ],
    ),
    shape(
        "feat: pricing rounding",
        &[op("src/harbour/pricing.py", 0, 22)],
    ),
    // The public API with no test beside it: 180 lines of exported surface, and the suite unchanged.
    shape(
        "feat: export the settlement ledger",
        &[op("src/harbour/ledger.py", 0, 180)],
    ),
    // The turn that deleted its own test file.
    shape(
        "perf: rewrite the availability path",
        &[
            op("src/harbour/avail.py", 18, 520),
            op("tests/test_avail.py", 0, 4),
        ],
    ),
    shape("feat: venue search", &[op("src/harbour/search.py", 0, 30)]),
    shape(
        "test: the availability suite",
        &[op("tests/test_avail.py", 0, 300)],
    ),
];

/// A repository holding [`SHAPES`], one commit per shape, an hour apart.
fn repository() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-cli-why-{}-{}",
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
    dir
}

/// The fast-import stream. Content is filler; the *paths* and *lengths* are the fixture. Three agent
/// identities carry a trailer so the classifier calls them agents (ADR-005).
fn stream() -> Vec<u8> {
    const AGENTS: [(&str, &str); 2] = [
        ("claude", "claude@harness.invalid"),
        ("codex-bot", "codex@harness.invalid"),
    ];
    let start: i64 = 1_700_000_000;
    let mut out = Vec::new();
    let mut mark: i64 = 0;
    let mut parent: Option<i64> = None;
    let mut tree: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (index, shape) in SHAPES.iter().enumerate() {
        let (name, email) = if shape.agent {
            AGENTS[index % AGENTS.len()]
        } else {
            ("Ada", "ada@example.invalid")
        };
        let when = start + i64::try_from(index).unwrap_or(i64::MAX) * 3_600;
        let mut marks = Vec::with_capacity(shape.files.len());
        for file in shape.files {
            let had = tree.get(file.path).copied().unwrap_or(0);
            let keep = file.keep.min(had);
            let body: String = match file.path {
                // The shapes the detectors read need content that means what its name says: the
                // destructive detector looks for the verb, the test detector for the file name.
                "db/migrations/0038_drop_email_backup.sql" => {
                    "ALTER TABLE customers DROP COLUMN email_backup;\n".repeat(6)
                }
                "tests/test_avail.py" => (0..keep + file.add)
                    .map(|line| format!("def test_case_{line}():\n    assert True\n"))
                    .collect(),
                "src/harbour/ledger.py" => (0..keep + file.add)
                    .map(|line| {
                        format!(
                            "def public_api_{line}(self, amount, currency):\n    return amount\n"
                        )
                    })
                    .collect(),
                _ => (0..keep + file.add)
                    .map(|line| format!("line {line}\n"))
                    .collect(),
            };
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

fn fathom_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the built fathom binary is runnable")
}

/// A repository with an index, and the revspec of each shape (`HEAD~n`, newest last).
fn indexed() -> PathBuf {
    let dir = repository();
    let out = fathom_in(&dir, &["index", "."]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    dir
}

/// The revspec of shape `index`, counting from the end of the log.
fn rev(index: usize) -> String {
    format!("HEAD~{}", SHAPES.len() - 1 - index)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> Option<i32> {
    out.status.code()
}

/// The twelve feature names the verb has to account for, in the order it prints them.
const FEATURES: [&str; 12] = [
    "findings",
    "surface",
    "migration",
    "blast",
    "test_ratio",
    "size",
    "dependency",
    "deletions",
    "noise",
    "breadth",
    "attribution",
    "authorship",
];

/// The feature table as printed: the name, then the rest of the row.
///
/// The parser is deliberately dumb — the header line opens the table, the first column is the feature, the
/// `—` separator row is skipped — because the test's job is to notice when the table stops naming every
/// feature, not to re-render it.
fn table(body: &str) -> BTreeMap<String, String> {
    let mut rows = BTreeMap::new();
    let mut inside = false;
    for line in body.lines() {
        if line.starts_with("feature ") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        let Some((name, rest)) = line.split_once(char::is_whitespace) else {
            break;
        };
        if name != "—" {
            rows.insert(name.to_owned(), rest.trim_start().to_owned());
        }
    }
    rows
}

/// The acceptance criterion, read off the screen: **every feature, its weight, and its contribution**.
#[test]
fn why_prints_every_feature_its_weight_and_its_contribution() {
    let dir = indexed();
    // The migration is shape 2 (0-based).
    let out = fathom_in(&dir, &["why", &rev(2)]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    eprintln!("{text}");

    // The header: which episode, and from which model.
    assert!(
        text.contains("features-1"),
        "the line says which model produced the number:\n{text}"
    );
    assert!(
        text.contains("features-1 ") && text.contains(" = "),
        "the arithmetic is printed as a sum a human can check:\n{text}"
    );
    assert!(
        text.contains("weight denominator"),
        "and the denominator it divided by:\n{text}"
    );
    assert!(
        text.contains("why, in order"),
        "and the reasons, in order:\n{text}"
    );
    assert!(
        text.contains("cannot see"),
        "and what the model could not see at all:\n{text}"
    );

    // Every feature is accounted for: a row with its weight and its points, or an absence with the
    // ticket that owns the missing input.
    let rows = table(&text);
    let mut names: Vec<&str> = rows.keys().map(String::as_str).collect();
    names.sort_unstable();
    let mut expected = FEATURES.to_vec();
    expected.sort_unstable();
    assert_eq!(
        names, expected,
        "every one of the twelve is on the page, scored or declared absent:\n{text}"
    );
    for (name, row) in &rows {
        let fields: Vec<&str> = row.split_whitespace().collect();
        assert!(
            fields.len() >= 5,
            "`{name}` printed a row with no weight or no points: {row}"
        );
        assert!(
            row.contains("—") || row.contains('/'),
            "a row either carries `points/max` or says it measured nothing: {row}"
        );
    }
    // The migration axis is scored at its shipped weight, and its reason quotes the line.
    assert!(
        rows.contains_key("migration") && rows["migration"].contains("0.09"),
        "the migration axis carries its weight:\n{text}"
    );
    assert!(
        text.contains("ALTER TABLE customers DROP COLUMN"),
        "and a reason quotes the SQL that caused it:\n{text}"
    );
}

/// A `human` verdict the classifier *fell through to* is not a measurement of authorship (`reco-0er`).
///
/// Shape 1 is Ada's docs commit: a person-shaped author string, no harness turn, no claim trailer, no
/// registry entry — the whole of the evidence is that nothing said an agent. Crediting "a person wrote
/// this, and is named as having done so" on that is the confident 0.00 this ticket is about, in a
/// repository an agent wrote end to end. The axis leaves the arithmetic instead, says why, and names what
/// would fill it; the episode beside it, which *does* carry turn evidence, is still measured.
#[test]
fn a_human_the_classifier_guessed_leaves_the_authorship_axis_unmeasured() {
    let dir = indexed();
    let out = fathom_in(&dir, &["why", &rev(1)]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    eprintln!("{text}");

    let rows = table(&text);
    let authorship = &rows["authorship"];
    assert!(
        authorship.starts_with('—'),
        "a verdict that is only the absence of evidence is not scored: {authorship}"
    );
    assert!(
        authorship.contains("identity"),
        "the absence names the rung it came from: {authorship}"
    );
    assert!(
        authorship.contains("owned by the harness"),
        "and says who can put a number on it: {authorship}"
    );
    assert!(
        text.contains("the `authorship` axis"),
        "so the scope statement under the score carries it too:\n{text}"
    );
    // The doubt axis is a different question and keeps its number: `attribution` measures how sure the
    // credit is, and "not sure" is a real answer, unlike "a person".
    assert!(
        rows["attribution"].contains("0.50"),
        "the confidence is still printed as the doubt it is: {}",
        rows["attribution"]
    );

    // …and the axis is not simply switched off: the turn-carrying episode beside it is measured.
    let agent = stdout(&fathom_in(&dir, &["why", &rev(0)]));
    let agent_rows = table(&agent);
    assert!(
        agent_rows["authorship"].contains('/') && agent_rows["authorship"].contains("0.70"),
        "an episode the harness spoke for is scored on the axis:\n{agent}"
    );

    // The machine's form says the same thing, with the owner attached.
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&fathom_in(&dir, &["why", &rev(1), "--json"])))
            .expect("--json is JSON");
    let absent = json["absent"]
        .as_array()
        .expect("an absent list")
        .iter()
        .find(|axis| axis["name"] == "authorship")
        .expect("authorship is absent on this episode");
    assert_eq!(absent["owner"].as_str(), Some("the harness"));
    assert!(
        absent["why"]
            .as_str()
            .unwrap_or_default()
            .contains("attribution.agents")
    );
}

/// The same episode, twice, and again: byte-identical, with nothing written to disk in between (criterion
/// two). The vector carries a path and a time nowhere in its printed form, so a run in a copied directory
/// agrees with the original.
#[test]
fn the_same_episode_scores_the_same_way_twice_and_says_so_in_the_same_words() {
    let dir = indexed();
    let first = stdout(&fathom_in(&dir, &["why", &rev(2)]));
    let second = stdout(&fathom_in(&dir, &["why", &rev(2)]));
    assert_eq!(
        first, second,
        "the same episode printed different words on two runs"
    );
    let json_first = stdout(&fathom_in(&dir, &["why", &rev(2), "--json"]));
    let json_second = stdout(&fathom_in(&dir, &["why", &rev(2), "--json"]));
    assert_eq!(
        json_first, json_second,
        "and the same for the machine's form"
    );
    let payload: serde_json::Value = serde_json::from_str(&json_first).expect("--json is JSON");
    assert_eq!(
        payload["contract"].as_str(),
        Some("fathom.cli.why/1"),
        "the envelope carries a contract id, as every published verb's does"
    );
    assert_eq!(payload["model"].as_str(), Some("features-1"));
    let terms = payload["terms"].as_array().expect("a term list");
    let total = payload["total"].as_i64().expect("a total");
    let weights = payload["weights"].as_array().expect("the weight set");
    assert_eq!(
        weights.len(),
        FEATURES.len(),
        "the payload names all twelve weights"
    );
    for weight in weights {
        assert!(
            weight["value"].is_number() && weight["origin"].is_string(),
            "each weight names its value and where it came from: {weight}"
        );
    }
    let sum: i64 = terms
        .iter()
        .map(|term| term["points"].as_i64().expect("a term's points"))
        .sum();
    assert_eq!(sum, total, "the printed terms are the total");
    assert_eq!(
        terms.len(),
        payload["measured"].as_u64().expect("a measured count") as usize,
        "every measured feature is printed, and no absent one is"
    );
    for term in terms {
        assert!(
            term["weight"].is_number() && term["feature"].is_string(),
            "each term names its feature and its weight: {term}"
        );
    }
    assert!(
        payload["score"].as_u64().unwrap() <= 100,
        "the score is a percentage"
    );
}

/// The score *means* something: the migration and the deleted test file outrank the 400-line prose edit,
/// and each says which axis did it.
#[test]
fn what_outranks_what_is_the_distinction_the_ticket_was_filed_for() {
    let dir = indexed();
    let score_of = |shape: usize| -> u64 {
        let out = fathom_in(&dir, &["why", &rev(shape), "--json"]);
        assert_eq!(code(&out), Some(0), "{}", stderr(&out));
        serde_json::from_str::<serde_json::Value>(&stdout(&out)).expect("json")["score"]
            .as_u64()
            .expect("a score")
    };
    let docs = score_of(1);
    let migration = score_of(2);
    let deleted_tests = score_of(5);
    assert!(
        migration > docs,
        "a destructive migration beats 400 lines of prose ({migration} vs {docs})"
    );
    assert!(
        deleted_tests > docs,
        "a rewrite that deleted its own test file beats prose ({deleted_tests} vs {docs})"
    );
    // And the docs episode is not scored 0: it is measured, and it is simply low. A zero would read as
    // "safe", which is the misreading the whole design exists to prevent.
    assert!(
        docs > 0,
        "a documentation edit still scores its real small amount"
    );
}

/// Weights come from `fathom.toml`, and the score says which ones came from where (criterion three).
/// One episode read alone must be measured the way the same episode is measured in a page read.
///
/// `fathom why` and `fathom export` read one episode; the rail, the queue and the Guardrail 3 harness
/// read a page. Both go through `risk::collect::vector`, but the single-episode read did not fold the
/// index's per-path rollup into the episode, so every axis that reads per-path churn — `size`, `noise`,
/// `test_ratio`, `deletions` — was measured against zeros. Shape 5 added 520 lines of logic beside four
/// lines of test, and the verb printed "neither logic nor tests arrived" about it while the queue
/// measured the coverage, which is one episode carrying two numbers depending on which door asked. The
/// fix is the rollup; this is the claim that says it stayed fixed.
#[test]
fn an_episode_read_alone_is_measured_the_way_a_page_read_measures_it() {
    let dir = indexed();
    // Shape 5: `perf: rewrite the availability path` — src/harbour/avail.py +520 and tests/test_avail.py +4.
    let out = fathom_in(&dir, &["why", &rev(5)]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let rows = table(&stdout(&out));

    let ratio = rows
        .get("test_ratio")
        .expect("the table accounts for test_ratio either way");
    assert!(
        ratio.contains("coverage share"),
        "logic and tests both arrived, so coverage is measured and not shrugged at:\n{ratio}"
    );
    let size = rows.get("size").expect("the table accounts for size");
    assert!(
        size.contains("reviewable lines") && !size.contains("0.00 reviewable lines"),
        "the churn a reviewer reads is the episode's own, not zero:\n{size}"
    );

    // And the honest absence is still an absence: shape 1 is prose only, so there is nothing new to cover
    // and the axis stays a stated hole rather than a zero.
    let out = fathom_in(&dir, &["why", &rev(1)]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let rows = table(&stdout(&out));
    let ratio = rows
        .get("test_ratio")
        .expect("test_ratio is still accounted for");
    assert!(
        ratio.contains("neither logic nor tests arrived"),
        "a docs-only episode has nothing new to cover, and says so:\n{ratio}"
    );
}

#[test]
fn a_weight_in_the_toml_is_a_weight_the_answer_admits_to() {
    let dir = indexed();
    std::fs::write(
        dir.join("fathom.toml"),
        "[risk]\ntau = 60\n\n[risk.weights]\nfindings = 0.40\nsize = 0.0\n",
    )
    .expect("write fathom.toml");
    let out = fathom_in(&dir, &["why", &rev(2), "--json"]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let payload: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("json");
    let findings = payload["weights"]
        .as_array()
        .expect("a weight list")
        .iter()
        .find(|w| w["feature"].as_str() == Some("findings"))
        .expect("findings is listed");
    assert_eq!(findings["value"].as_f64(), Some(0.40));
    assert_eq!(findings["origin"].as_str(), Some("repo"));
    assert_eq!(
        payload["tau"].as_u64(),
        Some(60),
        "risk.tau comes from the file too"
    );

    // A flag overrides it for one run, and the notices say so rather than leaving a silent difference.
    let overridden = fathom_in(
        &dir,
        &["why", &rev(2), "--weight", "findings=0.05", "--json"],
    );
    assert_eq!(code(&overridden), Some(0), "{}", stderr(&overridden));
    let text = stdout(&overridden);
    assert!(
        text.contains("0.05") && text.contains("--weight findings = 0.05"),
        "the run admits the override: {text}"
    );
    assert!(
        !text.contains("\"origin\":\"repo\"},\"weight\":0.4")
            || text.contains("overrides findings"),
        "an overridden weight is not silently still marked `repo`:\n{text}"
    );
}

/// A weight the build does not know is refused out loud. A silent fallback to the default would be a
/// wrong score with no notice attached.
#[test]
fn a_weight_that_is_not_a_feature_is_refused() {
    let dir = indexed();
    for bad in ["sideways=1", "findings=banana", "0.5"] {
        let out = fathom_in(&dir, &["why", &rev(2), "--weight", bad]);
        assert_eq!(
            code(&out),
            Some(2),
            "`--weight {bad}` has to be refused:\n{}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("findings") || stderr(&out).contains("weight"),
            "and the refusal names what it wanted:\n{}",
            stderr(&out)
        );
    }
    // A weight outside `0.0..=1.0` is clamped and *says* it was, which is the same rule the file follows:
    // a value that means "a lot" is not a typo to die over, and it is not a silent one either.
    for wild in ["findings=2", "findings=-1"] {
        let out = fathom_in(&dir, &["why", &rev(2), "--weight", wild]);
        assert_eq!(code(&out), Some(0), "{}", stderr(&out));
        assert!(
            stdout(&out).contains("outside 0.0..=1.0"),
            "`--weight {wild}` is clamped out loud:\n{}",
            stdout(&out)
        );
    }
    // A typo in the file is a notice, not a wrong score: the run still answers, with the notice printed.
    std::fs::write(dir.join("fathom.toml"), "[risk.weights]\nfinding = 0.4\n")
        .expect("write fathom.toml");
    let out = fathom_in(&dir, &["why", &rev(2)]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("finding"),
        "the typo is printed as a notice:\n{}",
        stdout(&out)
    );
}

/// `--calibrate` answers the question a weight change actually turns on: what did the top of the queue
/// contain before the change, and after it (criterion four).
#[test]
fn calibrate_prints_the_before_and_after_of_a_weight_change() {
    let dir = indexed();
    let out = fathom_in(&dir, &["why", "--calibrate"]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    eprintln!("{text}");
    for needle in [
        "shipped",
        "retuned",
        "precision",
        "recall",
        "F1",
        "[risk.weights]",
    ] {
        assert!(text.contains(needle), "`{needle}` is missing:\n{text}");
    }
    assert!(
        text.contains("40 cases") && text.contains("6 labelled must-read"),
        "the corpus it measured is named:\n{text}"
    );

    // A repository can measure its own dispositions instead, and a set that does not parse is refused.
    let other = dir.join("mine.json");
    std::fs::write(
        &other,
        r#"{"cases":[
            {"id":"a","summary":"migrations: drop the column","label":true,"features":{"migration":1.0,"findings":3.0,"deletions":0.0,"noise":0.0}},
            {"id":"b","summary":"docs: a sentence","label":false,"features":{"migration":0.0,"findings":0.0,"deletions":0.0,"noise":0.0}}
        ]}"#,
    )
    .expect("write the set");
    let mine = fathom_in(&dir, &["why", "--calibrate", &other.to_string_lossy()]);
    assert_eq!(code(&mine), Some(0), "{}", stderr(&mine));
    assert!(
        stdout(&mine).contains("2 cases"),
        "the repository's own corpus is measured:\n{}",
        stdout(&mine)
    );
    let broken = dir.join("broken.json");
    std::fs::write(&broken, "{\"cases\":[{\"id\":\"x\"}]}").expect("write the broken set");
    let bad = fathom_in(&dir, &["why", "--calibrate", &broken.to_string_lossy()]);
    assert_eq!(code(&bad), Some(1), "a set with no label is refused");
}

/// Discoverability: the names, units and rulers are printed rather than being something you have to read
/// Rust for.
#[test]
fn the_feature_table_is_printable() {
    let dir = indexed();
    let out = fathom_in(&dir, &["why", "--weights"]);
    assert_eq!(code(&out), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    for feature in FEATURES {
        assert!(
            text.contains(feature),
            "`{feature}` is not in the table:\n{text}"
        );
    }
    assert!(
        text.contains("[risk.weights]"),
        "and the table says where the knobs live:\n{text}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&fathom_in(&dir, &["why", "--weights", "--json"])))
            .expect("--weights --json is JSON");
    assert_eq!(
        json["features"].as_array().map(Vec::len),
        Some(FEATURES.len())
    );
}

/// An episode with no features measured at all is a refusal, not a zero; and no index is a refusal with a
/// remedy attached.
#[test]
fn a_score_with_no_denominator_is_refused_not_printed_as_zero() {
    let empty = std::env::temp_dir().join(format!("fathom-cli-why-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).expect("create the directory");
    git(
        empty.as_path(),
        &["init", "-q", "--initial-branch=main", "."],
    );
    std::fs::write(empty.join("README.md"), "nothing indexed here\n").expect("write a file");
    git(empty.as_path(), &["add", "-A"]);
    git(
        empty.as_path(),
        &[
            "-c",
            "user.name=Ada",
            "-c",
            "user.email=ada@example.invalid",
            "commit",
            "-q",
            "-m",
            "docs: a README",
        ],
    );
    let out = fathom_in(&empty, &["why", "HEAD"]);
    assert_eq!(
        code(&out),
        Some(12),
        "no index is exit 12, the code the manual already means by it:\n{}",
        stdout(&out)
    );
    assert!(
        stderr(&out).to_lowercase().contains("index"),
        "and the message says what is missing:\n{}",
        stderr(&out)
    );
}

//! `fathom ratchet`, driven as a binary (`e3.10`, `reco-5ht`).
//!
//! The ticket's acceptance criteria are all about a file's behaviour over successive runs, so these tests run
//! the binary and read the file back:
//!
//! * **CI passes with the old violations baselined and fails on a new one** — `ratchet.mode = deny` is the
//!   switch, and the exit code is 7 exactly when a violation has no live allowance;
//! * **fixing a baselined violation takes its row with it** — the next bare run rewrites the file, unprompted;
//! * **raising the baseline needs `--update-baseline` plus a reason**, and the reason, the owner and the day
//!   are in the file, in the printed diff and in the review memory;
//! * **an expiring allowance is seen before it lapses** — in the report and in `--json`, while it still has
//!   days left.
//!
//! Graphs are seeded through `fathom_analyze::graph::Lane::record` against the store the indexer just wrote,
//! keyed by the repository's own tree oids — the shape `e3.2`'s writer produces and the shape this verb reads.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Every test in this file is store-backed, in the same three ways `tests/architecture_verb.rs` is: the
// fixture indexes with `fathom index`, records its graphs through `fathom_analyze::graph::Lane` (ADR-002
// §6.2 compiles the lane only with a SQLite backend), and drives `fathom ratchet`, which a binary built
// without a store refuses with `FEATURE_ABSENT` (9). Gated in the spelling `tests/check_verb.rs` and
// `tests/show_trailers.rs` use.
#![cfg(feature = "sqlite-bundled")]

/// The checked-in schema validator, shared with the other verb contracts (`e1.10`).
#[path = "validator.rs"]
mod validator;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The `fathom.toml` the fixture writes: two layers, `no_upward` in force, and a ratchet that fails CI on a
/// violation nobody forgave.
const CONFIG: &str = "\
[layers]
l0 = [\"src/l0\"]
l1 = [\"src/l1\"]

[layering]
order = [\"l0\", \"l1\"]
no_upward = true

[ratchet]
mode = \"deny\"
baseline = \".fathom/ratchet.toml\"
";

/// The violation key this fixture produces: an `l0` file reaching up to an `l1` file.
const UPWARD: &str = "no_upward::src/l0/base.rs->src/l1/up.rs";
/// A second one, introduced later, so "new" has something to mean.
const UPWARD_TWO: &str = "no_upward::src/l0/two.rs->src/l1/up.rs";

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
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

fn git(root: &Path, args: &[&str], date: &str) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// One module's structural record: exports and dependency edges, nothing else.
fn module(path: &str, exports: &[&str], deps: &[&str]) -> fathom_analyze::structure::FileStructure {
    use fathom_analyze::structure::{
        Complexity, Edge, EdgeKind, FileLevel, FileStructure, Granularity, Language, Resolution,
        Span, Structure, Symbol, SymbolKind, Visibility,
    };
    let span = Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    let structure = Structure {
        symbols: exports
            .iter()
            .map(|name| Symbol {
                kind: SymbolKind::Fn,
                name: (*name).to_string(),
                container: String::new(),
                span,
                visibility: Visibility::Public,
                exported: true,
                complexity: Complexity {
                    body_lines: 1,
                    branches: 0,
                    loops: 0,
                    params: 0,
                    returns: 0,
                    members: 0,
                    nesting: 0,
                },
                modifiers: Vec::new(),
            })
            .collect(),
        exports: exports.iter().map(|name| (*name).to_string()).collect(),
        imports: Vec::new(),
        edges: deps
            .iter()
            .map(|target| Edge {
                target: (*target).to_string(),
                symbol: "thing".to_string(),
                kind: EdgeKind::Resolved,
                specifier: String::new(),
                via: None,
                form: String::new(),
                passes: 1,
                granularity: Granularity::File,
                package: None,
            })
            .collect(),
        supertypes: Vec::new(),
        synthetic_scopes: Vec::new(),
        notes: Vec::new(),
        resolution: Resolution {
            passes_used: 1,
            converged: true,
            bound: 4,
            truncated_chains: 0,
        },
        package: None,
    };
    FileStructure {
        path: path.to_string(),
        language: Language::Rust,
        file_level: FileLevel {
            path: path.to_string(),
            language: Language::Rust,
            bytes: 10,
            newlines: 1,
            blank_lines: 0,
            code_lines: 1,
            parse_errors: 0,
            refused: false,
        },
        structure: Some(structure),
        reason: None,
    }
}

/// One module in a graph plan: its path and what it reaches.
type Node<'a> = (&'a str, Vec<&'a str>);

/// A repository with an index and a graph snapshot per commit.
struct Repo {
    dir: PathBuf,
    /// The tree oid of each commit, oldest first.
    trees: Vec<String>,
    step: usize,
}

impl Repo {
    /// Two commits, one clean graph and one with a single upward edge, both recorded.
    fn new(label: &str) -> Repo {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-ratchet-cli-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        let start = "2026-06-01T09:00:00+00:00";
        git(&dir, &["init", "-q", "--initial-branch=main"], start);
        git(&dir, &["config", "user.name", "Rue Reviewer"], start);
        git(
            &dir,
            &["config", "user.email", "rue@example.invalid"],
            start,
        );
        std::fs::write(dir.join("fathom.toml"), CONFIG).expect("write the config");
        std::fs::create_dir_all(dir.join(".git").join("info")).expect("info/");
        std::fs::write(dir.join(".git").join("info").join("exclude"), ".fathom/\n")
            .expect("write the local exclude");

        let mut repo = Repo {
            dir,
            trees: Vec::new(),
            step: 0,
        };
        std::fs::create_dir_all(repo.dir.join("src/l0")).expect("src/l0");
        std::fs::create_dir_all(repo.dir.join("src/l1")).expect("src/l1");
        repo.commit(
            "add the two layers",
            &[
                ("src/l0/base.rs", "pub fn base() -> u32 { 1 }\n"),
                ("src/l1/up.rs", "pub fn up() -> u32 { 2 }\n"),
            ],
        );
        let out = fathom(&repo.dir, &["index", "."]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        // Revision one is clean; revision two is the mess somebody has to baseline.
        repo.record(&[("src/l0/base.rs", vec![]), ("src/l1/up.rs", vec![])]);
        repo.turn(
            "let the vocabulary reach up into the door",
            &[
                ("src/l0/base.rs", vec!["src/l1/up.rs"]),
                ("src/l1/up.rs", vec![]),
            ],
        );
        repo
    }

    fn commit(&mut self, subject: &str, files: &[(&str, &str)]) -> String {
        self.step += 1;
        for (path, body) in files {
            let full = self.dir.join(path);
            std::fs::create_dir_all(full.parent().expect("a parent")).expect("dir");
            std::fs::write(full, body).expect("write the file");
        }
        // One line that only this step has, so every commit has something to commit and every tree is
        // distinct — two commits with the same tree would be one graph revision.
        let log = self.dir.join("LOG.md");
        let mut log_text = std::fs::read_to_string(&log).unwrap_or_default();
        log_text.push_str(&format!("step {}\n", self.step));
        std::fs::write(&log, log_text).expect("write the log");
        let date = format!("2026-06-0{}T10:00:00+00:00", self.step);
        git(&self.dir, &["add", "-A"], &date);
        git(&self.dir, &["commit", "-q", "-m", subject], &date);
        let tree = git(&self.dir, &["rev-parse", "HEAD^{tree}"], &date);
        self.trees.push(tree.clone());
        tree
    }

    /// A new commit, and the graph it produced.
    fn turn(&mut self, subject: &str, nodes: &[Node<'_>]) {
        self.commit(subject, &[("src/notes.md", "# a turn\n")]);
        self.record(nodes);
    }

    /// Record the head graph from a module plan.
    fn record(&mut self, nodes: &[Node<'_>]) {
        use fathom_analyze::graph::ModuleInput;
        let tree = self
            .trees
            .get(self.step - 1)
            .expect("a commit to record against")
            .clone();
        let files: Vec<fathom_analyze::structure::FileStructure> = nodes
            .iter()
            .map(|(path, deps)| module(path, &["thing"], deps))
            .collect();
        let blobs: Vec<String> = files
            .iter()
            .map(|f| {
                format!(
                    "blob-{}:{}",
                    f.path,
                    f.structure.as_ref().map_or_else(
                        || "none".to_owned(),
                        |s| format!("{}|{}", s.exports.join(","), s.edges.len())
                    )
                )
            })
            .collect();
        let given: Vec<ModuleInput<'_>> = files
            .iter()
            .zip(&blobs)
            .map(|(f, blob)| ModuleInput {
                path: f.path.as_str(),
                blob: blob.as_str(),
                file: Some(f),
            })
            .collect();
        let mut store = self.open();
        {
            // Two things a fixture must not get wrong. The lane's builder carries `[layers]`, because the
            // layer tags are denormalised into the rows as they are written and a builder that never saw the
            // config records a graph that cannot be judged at all. And every revision is recorded as its own
            // **anchor** — a lane opened fresh per revision has no cursor to diff against, so its rows are
            // all adds, and a delta of adds inherited from a base would carry the base's edges forward and
            // say a paid-off violation is still there.
            let builder = fathom_analyze::graph::builder_from_config_text(CONFIG).0;
            let mut lane = fathom_analyze::graph::Lane::new(&mut store, builder)
                .expect("a lane over a real store");
            lane.record(
                &tree,
                None,
                &given,
                1_780_000_000_000_000 + i64::try_from(self.step).unwrap(),
            )
            .expect("record a graph snapshot");
            lane.flush_interner().expect("flush the interner");
        }
    }

    fn open(&self) -> fathom_store::spine::Store {
        fathom_store::spine::Store::open(&self.store_path()).expect("the store opens")
    }

    fn store_path(&self) -> PathBuf {
        self.dir.join(".fathom").join("index.sqlite3")
    }

    fn ratchet(&self, args: &[&str]) -> Output {
        let mut all = vec!["ratchet"];
        all.extend_from_slice(args);
        fathom(&self.dir, &all)
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let out = self.ratchet(&all);
        let text = stdout(&out);
        assert!(
            [0, 7].contains(&code(&out)),
            "{}: {}",
            all.join(" "),
            stderr(&out)
        );
        assert!(
            !text.trim().contains('\n'),
            "one JSON line for a pipeline to pipe:\n{text}"
        );
        serde_json::from_str(text.trim()).unwrap_or_else(|err| panic!("not JSON: {err}\n{text}"))
    }

    /// The baseline file as it stands on disk.
    fn baseline(&self) -> Option<String> {
        std::fs::read_to_string(self.dir.join(".fathom/ratchet.toml")).ok()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// **Acceptance criterion 1, first half.** A violation with no allowance fails the run, and the report names
/// it. A bare run does not write: creating a baseline is the deliberate half of the feature.
#[test]
fn a_violation_with_no_allowance_fails_and_writes_nothing() {
    let repo = Repo::new("uncovered");
    let out = repo.ratchet(&[]);
    assert_eq!(code(&out), 7, "{}: {}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("mode deny"), "{text}");
    assert!(text.contains(UPWARD), "the report names it: {text}");
    assert!(
        text.contains("--update-baseline"),
        "and says what to type: {text}"
    );
    assert!(
        repo.baseline().is_none(),
        "and it wrote nothing: the file is the deliberate act"
    );
    // The machine form says the same thing, and refuses to pretend the ledger was empty by accident.
    let out = repo.ratchet(&["--json"]);
    assert_eq!(code(&out), 7);
    let payload: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("one JSON line");
    assert_eq!(payload["contract"], "fathom-ratchet/1");
    assert_eq!(payload["exit"], 7);
    assert_eq!(payload["baselined"], serde_json::json!(false));
    assert_eq!(payload["uncovered"].as_array().unwrap().len(), 1);
    assert_eq!(payload["uncovered"][0]["key"], UPWARD);
    assert_eq!(payload["wrote"], serde_json::json!(false));
}

/// A repository with nothing to say about its architecture still has to be told why the verb said nothing.
#[test]
fn an_index_with_no_graph_is_a_refusal_and_not_an_empty_ledger() {
    let dir =
        std::env::temp_dir().join(format!("fathom-ratchet-cli-nograph-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the directory");
    git(
        &dir,
        &["init", "-q", "--initial-branch=main"],
        "2026-06-01T10:00:00+00:00",
    );
    git(
        &dir,
        &["config", "user.name", "Rue Reviewer"],
        "2026-06-01T10:00:00+00:00",
    );
    git(
        &dir,
        &["config", "user.email", "rue@example.invalid"],
        "2026-06-01T10:00:00+00:00",
    );
    std::fs::write(dir.join("a.rs"), "pub fn a() {}\n").expect("a file");
    git(&dir, &["add", "-A"], "2026-06-01T10:00:00+00:00");
    git(
        &dir,
        &["commit", "-q", "-m", "a"],
        "2026-06-01T10:00:00+00:00",
    );
    let out = fathom(&dir, &["ratchet", "--root", "."]);
    assert_eq!(code(&out), 12, "{}", stderr(&out));
    assert!(stdout(&out).is_empty(), "a refusal prints nothing to pipe");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Acceptance criterion 3, the refusal half.** `--update-baseline` without a reason is a usage error, and
/// nothing was loosened by the attempt.
#[test]
fn raising_the_baseline_without_a_reason_is_refused() {
    let repo = Repo::new("noreason");
    let out = repo.ratchet(&["--update-baseline"]);
    assert_eq!(code(&out), 2, "clap's own usage code: {}", stderr(&out));
    assert!(repo.baseline().is_none());
    // An empty reason is the same refusal by another road, because `--reason ""` is what a script types when
    // it has not thought of a reason.
    let out = repo.ratchet(&["--update-baseline", "--reason", " "]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(repo.baseline().is_none());
    // And a reason on its own is the same mistake seen from the other side.
    let out = repo.ratchet(&["--reason", "just thinking ahead"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(repo.baseline().is_none());
    // The expiry is parsed before anything is written, and a day it cannot read says so in the shape it wanted
    // rather than exiting 1 on a TOML error half a file later.
    for bad in ["31-12-2026", "2026-13-01", "soon", "+quarter"] {
        let out = repo.ratchet(&[
            "--update-baseline",
            "--reason",
            "a date I cannot type",
            "--expires",
            bad,
        ]);
        assert_eq!(code(&out), 2, "{bad}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("YYYY-MM-DD") || stderr(&out).contains("never"),
            "`--expires {bad}` was refused without naming the shape it wanted: {}",
            stderr(&out)
        );
        assert!(repo.baseline().is_none(), "{bad} wrote a file");
    }
}

/// **Acceptance criteria 1 and 3, the act.** The flag plus a reason writes the row — with its owner, its day
/// and its reason — and the next bare run passes, because the violation is debt now and not news.
#[test]
fn the_act_lands_in_the_file_and_the_next_run_passes() {
    let repo = Repo::new("act");
    let out = repo.ratchet(&[
        "--update-baseline",
        "--reason",
        "the ranking inverts with the storage rewrite",
        "--owner",
        "team-storage",
        "--granted-by",
        "Rue Reviewer",
        "--expires",
        "2099-01-31",
    ]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("+ layer "),
        "the diff shows the addition: {text}"
    );
    assert!(
        text.contains("reason: the ranking inverts with the storage rewrite"),
        "the diff line says who and why: {text}"
    );
    let file = repo.baseline().expect("the file was written");
    for expect in [
        "kind = \"layer\"",
        &format!("key = \"{UPWARD}\""),
        "owner = \"team-storage\"",
        "expires = \"2099-01-31\"",
        "reason = \"the ranking inverts with the storage rewrite\"",
        "granted_by = \"Rue Reviewer\"",
    ] {
        assert!(file.contains(expect), "{expect} missing from:\n{file}");
    }

    // The next run: same violation, live allowance, green. This is the acceptance criterion CI cares about.
    let out = repo.ratchet(&[]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    assert!(
        stdout(&out).contains("1 allowance(s) live"),
        "{}",
        stdout(&out)
    );
    let payload = repo.json(&[]);
    assert!(payload["uncovered"].as_array().unwrap().is_empty());
    assert_eq!(payload["allowances"].as_array().unwrap().len(), 1);
    assert_eq!(payload["allowances"][0]["owner"], "team-storage");
    assert_eq!(
        payload["allowances"][0]["granted_by"], "Rue Reviewer",
        "the who survives the round trip through the file"
    );
    assert_eq!(payload["changed"], serde_json::json!(false));
    assert_eq!(
        repo.json(&[])["changed"].as_bool(),
        Some(false),
        "a run that changes nothing leaves the file alone"
    );
}

/// **Acceptance criterion 4, the CLI half.** An allowance with days left is named while it still has them,
/// in the report and in the machine form, so it is seen before it lapses.
#[test]
fn an_allowance_is_warned_about_while_it_still_has_days() {
    let repo = Repo::new("expiring");
    let out = repo.ratchet(&[
        "--update-baseline",
        "--reason",
        "five days, then we talk",
        "--expires",
        "+5",
    ]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    let out = repo.ratchet(&[]);
    assert_eq!(code(&out), 0, "it is still an allowance: {}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("expiring within 30 day(s)"), "{text}");
    assert!(text.contains("day(s) left"), "{text}");
    let payload = repo.json(&[]);
    let expiring = payload["expiring"].as_array().unwrap();
    assert_eq!(expiring.len(), 1);
    let days = expiring[0]["days_left"].as_i64().unwrap();
    assert!((3..=5).contains(&days), "five days out, printed as {days}");
    assert_eq!(expiring[0]["owner"], expiring[0]["owner"].clone());
    assert_eq!(payload["back_due"].as_array().unwrap().len(), 0);
}

/// A row whose day has gone by is debt back due: news again, with its date and its reason named, and the row
/// stays in the file until it is paid off or deliberately re-granted. The file is written here rather than
/// granted, because a grant that has already expired is its own story.
#[test]
fn an_allowance_that_lapsed_is_back_due_and_says_so() {
    let repo = Repo::new("lapsed");
    std::fs::write(
        repo.dir.join(".fathom/ratchet.toml"),
        format!(
            "[[allowance]]\nkind = \"layer\"\nkey = \"{UPWARD}\"\nowner = \"team-storage\"\nexpires = \"\
             2020-01-01\"\nreason = \"a date that has gone by\"\n"
        ),
    )
    .expect("write a baseline whose day has passed");
    let out = repo.ratchet(&[]);
    assert_eq!(
        code(&out),
        7,
        "an expired allowance forgives nothing: {}",
        stdout(&out)
    );
    let text = stdout(&out);
    assert!(text.contains("back due (1)"), "{text}");
    assert!(text.contains("expired 2020-01-01"), "{text}");
    assert!(text.contains("a date that has gone by"), "{text}");
    let payload = repo.json(&[]);
    assert_eq!(payload["back_due"].as_array().unwrap().len(), 1);
    assert_eq!(payload["uncovered"].as_array().unwrap().len(), 1);
    // Re-granting replaces the row rather than adding a second one for the same fact.
    let out = repo.ratchet(&["--update-baseline", "--reason", "it slipped again"]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    let file = repo.baseline().expect("a file");
    assert_eq!(
        file.matches("[[allowance]]").count(),
        1,
        "one row for one violation:\n{file}"
    );
    assert!(
        !file.contains("a date that has gone by"),
        "the lapsed row is gone, not stacked under: {file}"
    );
    assert_eq!(
        code(&repo.ratchet(&[])),
        0,
        "and that was a fresh allowance"
    );
}

/// **Acceptance criterion 2.** Fix a baselined violation and the next bare run takes the row out of the
/// file — the ratchet turns one way by itself, and says that it did.
#[test]
fn a_paid_off_violation_leaves_the_file_on_the_next_run() {
    let mut repo = Repo::new("tighten");
    let out = repo.ratchet(&[
        "--update-baseline",
        "--reason",
        "paying this off in July",
        "--expires",
        "2099-01-31",
    ]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    assert!(repo.baseline().unwrap().contains(UPWARD));

    // The debt is paid: the upward edge is gone from the graph the next run reads.
    repo.turn(
        "invert the dependency for good",
        &[("src/l0/base.rs", vec![]), ("src/l1/up.rs", vec![])],
    );
    let out = repo.ratchet(&[]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("removed as the violations went away"),
        "{text}"
    );
    let file = repo.baseline().expect("the file is still there");
    assert!(
        !file.contains(UPWARD),
        "the row is gone from the file:\n{file}"
    );
    assert!(
        !file.contains("[[allowance]]"),
        "and it is empty now:\n{file}"
    );
    assert!(
        file.contains("# Fathom ratchet baseline"),
        "the header stays: the file is still a baseline:\n{file}"
    );
    assert!(
        repo.json(&[])["changed"].as_bool() == Some(false),
        "and a second run has nothing left to tighten"
    );
}

/// **Acceptance criterion 1, both halves in one run sequence.** Baselining what the repository already owed
/// is what makes the next violation legible: the old one is debt, the new one is the whole report.
#[test]
fn a_new_violation_is_the_news_while_the_old_one_is_debt() {
    let mut repo = Repo::new("news");
    let out = repo.ratchet(&["--update-baseline", "--reason", "the July rewrite"]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    assert_eq!(
        code(&repo.ratchet(&[])),
        0,
        "baselined, so the run is green"
    );

    repo.turn(
        "and a second upward edge, five minutes later",
        &[
            ("src/l0/base.rs", vec!["src/l1/up.rs"]),
            ("src/l0/two.rs", vec!["src/l1/up.rs"]),
            ("src/l1/up.rs", vec![]),
        ],
    );
    let out = repo.ratchet(&[]);
    assert_eq!(code(&out), 7, "{}: {}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(text.contains(UPWARD_TWO), "the new one is named: {text}");
    let payload = repo.json(&[]);
    let uncovered = payload["uncovered"].as_array().unwrap();
    assert_eq!(uncovered.len(), 1, "and only the new one: {text}");
    assert_eq!(uncovered[0]["key"], UPWARD_TWO);
    assert_eq!(payload["allowances"].as_array().unwrap().len(), 1);
    assert_eq!(
        repo.baseline().unwrap().matches("[[allowance]]").count(),
        1,
        "a report is not an act: the file still holds one row"
    );
}

/// `--dry-run` is the flag for "show me, change nothing" — including for the automatic half.
#[test]
fn a_dry_run_watches_and_touches_nothing() {
    let mut repo = Repo::new("dryrun");
    repo.ratchet(&[
        "--update-baseline",
        "--reason",
        "paying this off in July",
        "--expires",
        "2099-01-31",
    ]);
    let before = repo.baseline().expect("a baseline");
    repo.turn(
        "invert the dependency for good",
        &[("src/l0/base.rs", vec![]), ("src/l1/up.rs", vec![])],
    );
    let out = repo.ratchet(&["--dry-run"]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    assert!(stdout(&out).contains("nothing written"), "{}", stdout(&out));
    assert_eq!(
        repo.baseline().as_deref(),
        Some(before.as_str()),
        "and the file is byte-for-byte what it was"
    );
}

/// A `--key` that matches nothing is refused in words, not written as a row that forgives a hypothetical.
#[test]
fn a_key_that_matches_nothing_is_refused_in_words() {
    let repo = Repo::new("badkey");
    let out = repo.ratchet(&[
        "--update-baseline",
        "--reason",
        "one real, one typo",
        "--key",
        UPWARD,
        "--key",
        "no_upward::src/l0/ghost.rs->src/l1/up.rs",
    ]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("refused:"), "{text}");
    let file = repo.baseline().expect("a file");
    assert!(file.contains(UPWARD), "{file}");
    assert!(!file.contains("ghost"), "the typo wrote nothing:\n{file}");
}

/// **Acceptance criterion 3, third place the reason lands.** The review memory gets a decision note, because
/// a TOML row cannot carry why a cycle was allowed and `e6.5` says that is the part nobody can recover.
#[test]
fn the_act_is_written_in_the_review_memory_too() {
    let repo = Repo::new("memory");
    let out = repo.ratchet(&[
        "--update-baseline",
        "--reason",
        "the split lands with the ledger rewrite",
        "--owner",
        "team-payments",
    ]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    assert!(
        stdout(&out).contains("decision note"),
        "and it says so: {}",
        stdout(&out)
    );
    let notes = std::fs::read_to_string(repo.dir.join(".fathom/reviews/notes.md"))
        .expect("the memory file was written");
    assert!(
        notes.contains("the split lands with the ledger rewrite"),
        "{notes}"
    );
    assert!(notes.contains(UPWARD), "{notes}");
    assert!(notes.contains("team-payments"), "{notes}");
    assert!(notes.contains("decision"), "{notes}");

    // The opt-out is honoured: the file, not the note, is what enforces anything.
    let quiet = Repo::new("memoryoff");
    let out = quiet.ratchet(&[
        "--update-baseline",
        "--reason",
        "no note this time",
        "--no-note",
    ]);
    assert_eq!(code(&out), 0, "{}: {}", stdout(&out), stderr(&out));
    assert!(!stdout(&out).contains("decision note"));
    assert!(
        !quiet.dir.join(".fathom/reviews/notes.md").exists(),
        "and nothing was written to the memory"
    );
}

/// **The `--json` contract is checked in (`e1.10`'s rule for every verb).** The payload is validated against
/// `schemas/ratchet.schema.json`, re-serialised, validated again, and refused when it grows a key the schema
/// does not declare — a consumer pins this shape from a CI box, and a shape that only exists in the source
/// is a shape nobody pinned.
#[test]
fn the_json_payload_is_the_checked_in_contract() {
    // `$ref`s inside the file resolve out of the set the validator has open, so it has to be opened first.
    validator::open(&["ratchet.schema.json", "common.schema.json"]);
    let schema = validator::Schema::load("ratchet.schema.json");
    let repo = Repo::new("contract");

    // First the uncovered state, then a granted one: the two shapes a consumer sees, with the row lists empty
    // and with them full.
    for args in [
        vec!["--json"],
        vec!["--update-baseline", "--reason", "on purpose", "--json"],
    ] {
        let out = repo.ratchet(&args);
        assert!(
            [0, 7].contains(&code(&out)),
            "{}: {}",
            args.join(" "),
            stderr(&out)
        );
        let text = stdout(&out).trim().to_owned();
        assert!(!text.contains('\n'), "one JSON line: {text}");
        let value: serde_json::Value = serde_json::from_str(&text).expect("one JSON value");
        let problems = validator::validate(&schema.document, &value, "$");
        assert!(problems.is_empty(), "{args:?}: {problems:#?}\n{text}");
        assert_eq!(
            value["contract"].as_str(),
            schema.document["x-fathom-contract"].as_str(),
            "the payload and its schema must name the same contract"
        );
        let again: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(again, value, "re-serialising changed the payload");
        assert!(
            validator::validate(&schema.document, &again, "$").is_empty(),
            "{args:?} validated before the round trip and not after"
        );
    }

    // And the schema is not decorative: a key it does not declare is a problem it reports.
    let out = repo.ratchet(&["--json"]);
    let mut value: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("JSON");
    value["not_a_field"] = serde_json::json!(1);
    assert!(
        !validator::validate(&schema.document, &value, "$").is_empty(),
        "an undeclared key validated clean against ratchet.schema.json"
    );
}

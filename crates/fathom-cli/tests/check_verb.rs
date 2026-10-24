//! `fathom check`, driven as a binary (`e6.6`, `reco-ngk`).
//!
//! The decision layer has its own suite (`crates/fathom-analyze/tests/check_gate.rs`); what has to be proven
//! here is the *gathering* — that the verb reads a real repository, a real index, real git objects and a real
//! ratchet ledger, and that the five formats are real files. The ticket's criteria, test by test:
//!
//! * **evaluates the six checks over a `base..head` range** — each check is fired in a repository built to trip
//!   exactly it, and the JSON says which rule fired and where;
//! * **ratchet baselines, so existing debt does not block** — the same violation, baselined, stops failing
//!   the run while still being counted in the row's note;
//! * **the formats** — `--json` validates against `schemas/check.schema.json`, `--format sarif` is a log
//!   whose rules are all declared and whose locations are all present at the revision under scan, `--out`
//!   writes it, `--format github` annotates, `--format markdown --summary` appends;
//! * **no silent pass** — a missing base revision, an index with no graph and an unreadable policy each have a
//!   stated state and an exit code of their own, and `--require` turns "could not run" into a failure;
//! * **a shallow clone** — four of the six checks answer from the index alone, and the two that need the base
//!   say so instead of inventing an answer.
//!
//! Graph snapshots are seeded through `fathom_analyze::graph::Lane::record` against the store the indexer
//! just wrote, keyed by the repository's own tree oids — the same shape `crates/fathom-cli/tests/ratchet_verb.rs`
//! builds, and the shape `e3.2`'s writer will produce.
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The checked-in schema validator, shared with the other verb contracts (`e1.10`).
#[path = "validator.rs"]
mod validator;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The layering rules, and the gate's policy: deny at warning, with the two thresholds low enough that a
/// four-line fixture can trip the test-coverage check.
const CONFIG: &str = "\
[layers]
l0 = [\"src/l0\"]
l1 = [\"src/l1\"]

[layering]
order = [\"l0\", \"l1\"]
no_upward = true

[ratchet]
mode = \"warn\"
baseline = \".fathom/ratchet.toml\"

[check]
version = 1
mode = \"deny\"
fail_on = \"warning\"

[check.tests]
min_new_lines = 8
min_tested_ratio = 0.5

[check.risk]
threshold = 90
";

/// The violation key this fixture produces: an `l0` file reaching up into an `l1` file.
const UPWARD: &str = "no_upward::src/l0/base.rs->src/l1/up.rs";

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

/// A repository with an index, a graph snapshot per commit, and `[check]` in its config.
struct Repo {
    dir: PathBuf,
    trees: Vec<String>,
    step: usize,
}

impl Repo {
    /// One clean commit, indexed, with a clean graph recorded.
    fn new(label: &str) -> Repo {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-check-cli-{label}-{}-{index}",
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
        repo.commit(
            "add the two layers",
            &[
                ("src/l0/base.rs", "pub fn base() -> u32 {\n    1\n}\n"),
                ("src/l1/up.rs", "pub fn up() -> u32 {\n    2\n}\n"),
            ],
        );
        let out = fathom(&repo.dir, &["index", "."]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        repo.record(&[("src/l0/base.rs", vec![]), ("src/l1/up.rs", vec![])]);
        repo
    }

    fn commit(&mut self, subject: &str, files: &[(&str, &str)]) -> String {
        self.step += 1;
        for (path, body) in files {
            let full = self.dir.join(path);
            std::fs::create_dir_all(full.parent().expect("a parent")).expect("dir");
            std::fs::write(full, body).expect("write the file");
        }
        // One line only this step has, so every commit has something to commit and every tree is distinct —
        // two commits with the same tree would be one graph revision.
        let log = self.dir.join("LOG.md");
        let mut log_text = std::fs::read_to_string(&log).unwrap_or_default();
        log_text.push_str(&format!("step {}\n", self.step));
        std::fs::write(&log, log_text).expect("write the log");
        let day = self.step + 1;
        let date = format!(
            "2026-{:02}-{:02}T10:00:00+00:00",
            day / 28 + 1,
            day % 28 + 1
        );
        git(&self.dir, &["add", "-A"], &date);
        git(&self.dir, &["commit", "-q", "-m", subject], &date);
        let tree = git(&self.dir, &["rev-parse", "HEAD^{tree}"], &date);
        self.trees.push(tree.clone());
        // The episode-shaped checks read the index, so a commit the index has never seen is a commit the
        // gate cannot judge. Once there is a store, every commit goes through it — which is also what a real
        // developer's working tree does between the two `fathom check` runs.
        if self.store_path().is_file() {
            self.reindex();
        }
        tree
    }

    /// The head commit's oid.
    fn head(&self) -> String {
        git(
            &self.dir,
            &["rev-parse", "HEAD"],
            "2026-06-01T00:00:00+00:00",
        )
    }

    /// The oid of the commit at `offset` from the front, as a revision `--base` can name. `revision(1)` is
    /// the fixture's first commit; `revision(0)` would be HEAD.
    fn revision(&self, offset: usize) -> String {
        git(
            &self.dir,
            &["rev-parse", &format!("HEAD~{}", self.step - offset)],
            "2026-06-01T00:00:00+00:00",
        )
    }

    /// A new commit, and the graph it produced.
    fn turn(&mut self, subject: &str, files: &[(&str, &str)], nodes: &[Node<'_>]) {
        self.commit(subject, files);
        self.record(nodes);
        self.reindex();
    }

    fn reindex(&self) {
        // `--no-graph` because every graph these tests read is one they wrote themselves with
        // `Repo::record_graph`: a store that already holds a revision's real graph would keep it and ignore
        // the synthetic one, and the synthetic one is the whole point — it is the only way a three-commit
        // fixture can say "this turn added a cycle" (`reco-dlp` made `fathom index` write the graph).
        let out = fathom(&self.dir, &["index", ".", "--no-graph"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
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
            // The lane's builder carries `[layers]`, because the layer tags are denormalised into the rows
            // as they are written and a builder that never saw the config records a graph that cannot be
            // judged at all.
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

    /// `fathom check …`, with the range pinned to the fixture's own endpoints.
    fn check(&self, args: &[&str]) -> Output {
        let mut all = vec!["check", "--root", "."];
        all.extend_from_slice(args);
        fathom(&self.dir, &all)
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut all: Vec<String> = vec!["--format=json".to_owned()];
        all.extend(args.iter().map(|s| (*s).to_owned()));
        let all: Vec<&str> = all.iter().map(String::as_str).collect();
        let out = self.check(&all);
        let text = stdout(&out);
        serde_json::from_str(text.trim()).unwrap_or_else(|e| {
            panic!(
                "check --format json is not JSON ({e}) after {args:?}\nstdout: {text}\nstderr: {}",
                stderr(&out)
            )
        })
    }

    /// The `checks[]` row for one check.
    fn row<'a>(value: &'a serde_json::Value, check: &str) -> &'a serde_json::Value {
        value["checks"]
            .as_array()
            .expect("a row per check")
            .iter()
            .find(|row| row["check"] == serde_json::json!(check))
            .unwrap_or_else(|| panic!("no row for {check}: {}", value["checks"]))
    }

    fn rules(value: &serde_json::Value, check: &str) -> Vec<String> {
        Self::row(value, check)["findings"]
            .as_array()
            .expect("a findings list")
            .iter()
            .map(|finding| finding["rule"].as_str().expect("a rule").to_owned())
            .collect()
    }

    /// Write `.fathom/ratchet.toml` by asking the verb that owns it, with the reason it demands.
    fn baseline(&self, keys: &[&str]) {
        let mut args = vec![
            "ratchet",
            "--update-baseline",
            "--reason",
            "baselined by the gate test",
        ];
        for key in keys {
            args.push("--allow");
            args.push(key);
        }
        let out = fathom(&self.dir, &args);
        assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
    }
}

// ---------------------------------------------------------------------------
// the six checks, each in a repository built to trip exactly it
// ---------------------------------------------------------------------------

#[test]
fn a_layering_violation_this_range_introduced_fails_the_run_and_names_the_edge() {
    let mut repo = Repo::new("layering");
    // The turn that reaches upward: `src/l0/base.rs` now depends on `src/l1/up.rs`.
    repo.turn(
        "let the vocabulary reach up into the door",
        &[(
            "src/l0/base.rs",
            "use crate::l1::up;\npub fn base() -> u32 {\n    up()\n}\n",
        )],
        &[
            ("src/l0/base.rs", vec!["src/l1/up.rs"]),
            ("src/l1/up.rs", vec![]),
        ],
    );
    let value = repo.json(&["--base", &repo.revision(1), "--head", "HEAD"]);
    assert_eq!(value["outcome"], "fail");
    let row = Repo::row(&value, "layering");
    assert_eq!(row["status"], "ran");
    assert_eq!(
        row["volume"], "error",
        "a structural fact blocks by default"
    );
    assert_eq!(Repo::rules(&value, "layering"), vec![UPWARD]);
    let finding = &row["findings"][0];
    assert_eq!(finding["severity"], "error");
    // The evidence names the file and how the claim was established, which is what makes it checkable.
    assert_eq!(finding["evidence"][0]["path"], "src/l0/base.rs");
    assert!(
        finding["evidence"][0]["source"]
            .as_str()
            .unwrap()
            .starts_with("graph:layering"),
        "{}",
        finding["evidence"][0]
    );

    // The log's one sentence names the same thing, and the exit code is the gate's own `7`.
    let out = repo.check(&["--base", &repo.revision(1), "--head", "HEAD"]);
    assert_eq!(code(&out), 7);
    assert!(
        stdout(&out).contains(&format!("`{UPWARD}`")),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_baselined_violation_stops_blocking_and_says_it_was_forgiven() {
    // *"using ratchet baselines so existing debt does not block"* — and the gate has no ledger of its own:
    // `fathom ratchet` writes `.fathom/ratchet.toml`, and the layering engine reads it before the gate ever
    // sees a count. Two acts, two owners, and the run states in writing that four findings were forgiven.
    let mut repo = Repo::new("ratchet");
    repo.turn(
        "let the vocabulary reach up into the door",
        &[(
            "src/l0/base.rs",
            "use crate::l1::up;\npub fn base() -> u32 {\n    up()\n}\n",
        )],
        &[
            ("src/l0/base.rs", vec!["src/l1/up.rs"]),
            ("src/l1/up.rs", vec![]),
        ],
    );
    let base = repo.revision(1);
    assert_eq!(code(&repo.check(&["--base", &base])), 7, "before");

    repo.baseline(&[]);
    let value = repo.json(&["--base", &base]);
    assert_eq!(value["outcome"], "pass", "{value}");
    assert_eq!(Repo::rules(&value, "layering"), Vec::<String>::new());
    assert_eq!(code(&repo.check(&["--base", &base])), 0);

    // And the row says where the debt went, because "nothing found" and "nothing forgivable" must not look
    // the same in a log a stranger is reading.
    let notes = Repo::row(&value, "layering")["notes"]
        .as_array()
        .expect("a notes list")
        .iter()
        .map(|s| s.as_str().expect("a note").to_owned())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(notes.contains("baselined"), "{notes}");
    assert!(notes.contains(".fathom/ratchet.toml"), "{notes}");
}

#[test]
fn a_cycle_this_range_introduced_is_a_finding_and_an_existing_one_is_not() {
    let mut repo = Repo::new("cycles");
    repo.turn(
        "the billing module and the vault module now know each other",
        &[("src/l1/cycle.rs", "pub fn cycle() -> u32 {\n    1\n}\n")],
        &[
            ("src/l0/base.rs", vec![]),
            ("src/l1/up.rs", vec![]),
            ("src/l1/cycle.rs", vec![]),
        ],
    );
    let base = repo.revision(1);
    // A clean graph on both ends has nothing to report, and says it ran rather than that it is blocked.
    let value = repo.json(&["--base", &base]);
    assert_eq!(Repo::row(&value, "cycles")["status"], "ran");
    assert_eq!(Repo::rules(&value, "cycles"), Vec::<String>::new());

    // Now the cycle: two modules that reach each other, introduced by this range.
    repo.turn(
        "and now they reach each other",
        &[(
            "src/l0/base.rs",
            "pub fn base() -> u32 {\n    crate::l1::cycle::cycle()\n}\n",
        )],
        &[
            ("src/l0/base.rs", vec!["src/l1/cycle.rs"]),
            ("src/l1/cycle.rs", vec!["src/l0/base.rs"]),
            ("src/l1/up.rs", vec![]),
        ],
    );
    let value = repo.json(&["--base", &base]);
    let cycles = Repo::rules(&value, "cycles");
    assert_eq!(cycles.len(), 1, "{cycles:?}");
    assert!(
        cycles[0].contains("src/l0/base.rs") && cycles[0].contains("src/l1/cycle.rs"),
        "the rule is the cycle's own key: {cycles:?}"
    );
    // Layering also objects (l0 reaching l1 twice over), so the run is red for two families at once.
    assert_eq!(code(&repo.check(&["--base", &base])), 7);
}

#[test]
fn removing_public_api_is_breaking_and_adding_it_is_not() {
    // The gate asks the api engine, which scopes a Rust surface by crate root: `Cargo.toml` and a root
    // `lib.rs` are what make a package whose items are a contract. A file under a layer directory is a
    // module, not a surface — the layering check is the one that has an opinion about it.
    let mut repo = Repo::new("api");
    repo.commit(
        "the crate, and the surface it publishes",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
            ),
            (
                "lib.rs",
                "pub fn keep() -> u32 {\n    1\n}\npub fn drop_me() -> u32 {\n    2\n}\n",
            ),
        ],
    );
    // The crate exists as of this commit; the comparison runs from here, so the api row is about the one
    // change under test rather than about the fixture's own history.
    let base = repo.head();

    repo.commit(
        "take a public item away",
        &[("lib.rs", "pub fn keep() -> u32 {\n    1\n}\n")],
    );
    let value = repo.json(&["--base", &base]);
    let rules = Repo::rules(&value, "api");
    assert_eq!(rules, vec!["breaking"], "{rules:?}");
    let finding = &Repo::row(&value, "api")["findings"][0];
    assert_eq!(finding["severity"], "error");
    assert_eq!(finding["evidence"][0]["path"], "lib.rs");
    assert!(
        finding["message"].as_str().unwrap().contains("drop_me"),
        "{}",
        finding["message"]
    );
    assert!(
        finding["detail"]
            .as_array()
            .expect("detail")
            .iter()
            .any(|line| line.as_str().unwrap_or_default().contains("breaking")),
        "{finding}"
    );
    assert_eq!(code(&repo.check(&["--base", &base])), 7);
}

#[test]
fn a_new_dependency_is_a_finding_of_the_policies_volume() {
    let mut repo = Repo::new("deps");
    repo.commit(
        "adopt a package nobody had adopted",
        &[(
            "package.json",
            "{\n  \"dependencies\": {\n    \"acme-sdk\": \"^1.2.0\"\n  }\n}\n",
        )],
    );
    let value = repo.json(&["--base", &repo.revision(1)]);
    assert_eq!(Repo::row(&value, "dependencies")["status"], "ran");
    let rules = Repo::rules(&value, "dependencies");
    assert_eq!(rules, vec!["new-direct"], "{rules:?}");
    let finding = &Repo::row(&value, "dependencies")["findings"][0];
    assert_eq!(
        finding["severity"], "warning",
        "a judgement call, not a fact"
    );
    assert!(finding["message"].as_str().unwrap().contains("acme-sdk"));

    // `forbid` is the one dependency policy that is a fact: the repository named the package.
    std::fs::write(
        repo.dir.join("fathom.toml"),
        format!("{CONFIG}\n[check.dependencies]\nforbid = [\"acme-sdk\"]\n"),
    )
    .expect("rewrite the config");
    let value = repo.json(&["--base", &repo.revision(1)]);
    let rules = Repo::rules(&value, "dependencies");
    assert!(
        rules.iter().any(|r| r.starts_with("forbidden")),
        "{rules:?}"
    );
    assert_eq!(
        Repo::row(&value, "dependencies")["findings"]
            .as_array()
            .expect("list")
            .iter()
            .find(|f| f["rule"].as_str().unwrap().starts_with("forbidden"))
            .map(|f| f["severity"].as_str().unwrap())
            .unwrap(),
        "error"
    );
}

#[test]
fn new_logic_without_tests_fires_above_the_threshold_and_not_below_it() {
    // *"untested new logic above a size threshold"*: the size floor is the config key, and the ratio is the
    // other one. A fixture under the floor must be silent, because a check that fires on four lines is a
    // check that gets switched off.
    let mut repo = Repo::new("tests");
    let base = repo.head();
    repo.commit(
        "four lines and no test",
        &[("src/l0/small.rs", "pub fn a() -> u32 {\n    1\n}\n")],
    );
    let value = repo.json(&["--base", &base]);
    assert_eq!(Repo::row(&value, "tests")["status"], "ran");
    assert_eq!(
        Repo::rules(&value, "tests"),
        Vec::<String>::new(),
        "{value}"
    );

    let lines = "pub fn b() -> u32 {\n    1\n}\n".repeat(10);
    repo.commit("forty lines and no test", &[("src/l0/big.rs", &lines)]);
    let value = repo.json(&["--base", &base]);
    let rules = Repo::rules(&value, "tests");
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert!(rules[0].starts_with("untested-logic"), "{rules:?}");
    let finding = &Repo::row(&value, "tests")["findings"][0];
    assert!(
        finding["message"].as_str().unwrap().contains("30 lines"),
        "{}",
        finding["message"]
    );

    // The same bulk arriving *with* its tests is not the same finding — and "with" means the same
    // episode, because the episode is the unit the ratio is measured over. A separate "add the tests
    // later" commit does not retroactively cover the one that arrived without them, and saying so is the
    // point of keying the check to the episode.
    let tests = "fn t() {\n    assert!(true);\n}\n".repeat(20);
    let mut other = Repo::new("tests-with");
    let with_base = other.head();
    other.commit(
        "forty lines and their tests",
        &[("src/l0/big.rs", &lines), ("tests/big.rs", &tests)],
    );
    let value = other.json(&["--base", &with_base]);
    assert_eq!(
        Repo::rules(&value, "tests"),
        Vec::<String>::new(),
        "{value}"
    );
}

#[test]
fn a_high_risk_episode_without_a_sign_off_fires_and_a_signed_one_does_not() {
    // *"[check.risk] threshold"* is a conjunction in the gate: high score **and** no sign-off. The fixture
    // sets `threshold = 0` so every episode is "high risk", which makes the second half of the conjunction
    // the only variable, and the sign-off the whole difference.
    let mut repo = Repo::new("signoff");
    // Every episode is "high risk" for the duration of this test, which makes the *sign-off* half of the
    // conjunction the only variable.
    std::fs::write(
        repo.dir.join("fathom.toml"),
        CONFIG.replace("threshold = 90", "threshold = 0"),
    )
    .expect("rewrite the config");
    let base = repo.head();
    repo.commit(
        "a turn nobody reviewed",
        &[("src/l0/x.rs", "pub fn x() -> u32 {\n    1\n}\n")],
    );
    let value = repo.json(&["--base", &base]);
    let rules = Repo::rules(&value, "sign-off");
    assert_eq!(rules.len(), 1, "{rules:?}");
    assert_eq!(
        Repo::row(&value, "sign-off")["findings"][0]["severity"],
        "warning"
    );

    // The ledger is the fix, and the gate reads it rather than a flag: `fathom review sign` is the act.
    // Every episode the findings named — the finding's own evidence carries the id it scored.
    let ids: Vec<String> = Repo::row(&value, "sign-off")["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter_map(|finding| finding["evidence"][0]["source"].as_str())
        .filter_map(|source| source.strip_prefix("index:episode "))
        .map(str::to_owned)
        .collect();
    assert!(!ids.is_empty(), "{value}");
    for id in &ids {
        let signed = fathom(
            &repo.dir,
            &[
                "review",
                "sign",
                id,
                "--decision",
                "accepted",
                "--identity",
                "Rue Reviewer",
            ],
        );
        assert_eq!(code(&signed), 0, "{}\n{}", stdout(&signed), stderr(&signed));
    }

    let value = repo.json(&["--base", &base]);
    assert_eq!(
        Repo::rules(&value, "sign-off"),
        Vec::<String>::new(),
        "{value}"
    );
}

// ---------------------------------------------------------------------------
// what makes a run red, and what a run that cannot answer says
// ---------------------------------------------------------------------------

#[test]
fn the_default_is_to_report_without_blocking_and_deny_is_the_flag_that_blocks() {
    // `mode = warn` is the shipped default for a reason: adopting a gate should cost nothing on day one. The
    // same findings, the same notes, exit `0` — and the sentence says which of the two the mode decided.
    let mut repo = Repo::new("mode");
    repo.turn(
        "reach upward",
        &[(
            "src/l0/base.rs",
            "use crate::l1::up;\npub fn base() -> u32 {\n    up()\n}\n",
        )],
        &[
            ("src/l0/base.rs", vec!["src/l1/up.rs"]),
            ("src/l1/up.rs", vec![]),
        ],
    );
    let base = repo.revision(1);
    std::fs::write(
        repo.dir.join("fathom.toml"),
        CONFIG.replace("mode = \"deny\"", "mode = \"warn\""),
    )
    .expect("rewrite the config");
    let out = repo.check(&["--base", &base]);
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    assert!(
        stdout(&out).contains("reported and not enforced"),
        "{}",
        stdout(&out)
    );

    // `--fail-on` out loud is a request to enforce, and it says so in the notices it prints.
    let out = repo.check(&["--base", &base, "--fail-on", "warning"]);
    assert_eq!(code(&out), 7, "{}", stdout(&out));
    assert!(
        stdout(&out).contains("`--fail-on` enforces"),
        "{}",
        stdout(&out)
    );

    // And `mode = off` is the kill switch: no threshold applies, whatever the flags would have said.
    std::fs::write(
        repo.dir.join("fathom.toml"),
        CONFIG.replace("mode = \"deny\"", "mode = \"off\""),
    )
    .expect("rewrite the config");
    let out = repo.check(&["--base", &base, "--fail-on", "warning"]);
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    assert!(stdout(&out).contains("decided nothing"), "{}", stdout(&out));
}

#[test]
fn a_check_that_cannot_run_says_so_and_a_required_one_fails_the_run() {
    // No graph snapshots recorded at all: the two graph checks report `blocked`, the four others answer, and
    // the run is only red if the operator asked for the graph.
    let repo = Repo::new("blocked");
    let base = repo.head();
    std::fs::remove_file(repo.store_path()).expect("remove the index");
    let value = repo.json(&["--base", &base]);
    for check in ["layering", "cycles"] {
        let row = Repo::row(&value, check);
        assert_eq!(row["status"], "blocked", "{check}: {row}");
        assert!(
            row["remedy"].is_string(),
            "{check} must give a remedy: {row}"
        );
        assert!(
            row["examined"].is_null(),
            "a blocked row examined nothing: {row}"
        );
    }
    // A row is `off` only when the policy switched it off; a missing index never makes one.
    assert_eq!(Repo::row(&value, "tests")["status"], "blocked");
    assert_eq!(value["outcome"], "pass", "nothing was required: {value}");
    assert_eq!(code(&repo.check(&["--base", &base])), 0);

    // Required, and blocked: the run is red, and the one sentence is the reason.
    let out = repo.check(&["--base", &base, "--require", "layering"]);
    assert_eq!(code(&out), 7, "{}", stdout(&out));
    assert!(
        stdout(&out).contains("`layering` is required and could not run"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_repository_with_no_index_at_all_still_answers_in_the_formats_that_need_no_store() {
    // A fresh clone with no `.fathom/` is the common case in CI before the index step. `fathom check` answers
    // rather than crashing — the store-shaped checks are blocked, and the exit code follows the policy.
    let repo = Repo::new("noindex");
    std::fs::remove_dir_all(repo.dir.join(".fathom")).expect("remove the store directory");
    let out = repo.check(&["--format", "markdown", "--summary", "/dev/stdout"]);
    // `--summary` to /dev/stdout is the job-summary case; the run's own exit code is the verdict either way.
    assert!(code(&out) == 0 || code(&out) == 7, "{}", stderr(&out));
    assert!(stdout(&out).contains("fathom"), "{}", stdout(&out));
}

#[test]
fn a_base_revision_outside_the_clone_blocks_the_two_checks_that_need_it() {
    // *"without a full history fetch"*. A shallow clone where the base is not present: the graph and episode
    // checks work from the index and answer; the two that read the base tree report blocked with the command
    // that fixes it, and nothing guesses.
    let mut repo = Repo::new("shallow");
    repo.turn(
        "reach upward",
        &[(
            "src/l0/base.rs",
            "use crate::l1::up;\npub fn base() -> u32 {\n    up()\n}\n",
        )],
        &[
            ("src/l0/base.rs", vec!["src/l1/up.rs"]),
            ("src/l1/up.rs", vec![]),
        ],
    );
    // A clone of depth 1: HEAD only. The base commit does not exist in it.
    let shallow = repo.dir.with_extension("shallow");
    let _ = std::fs::remove_dir_all(&shallow);
    // `--no-local` matters: git's local-clone shortcut copies the whole object store and ignores
    // `--depth` entirely, which would make this fixture a full clone wearing a trench coat.
    git(
        repo.dir.parent().expect("a parent"),
        &[
            "clone",
            "--no-local",
            "--depth",
            "1",
            "-q",
            &format!("file://{}", repo.dir.display()),
            &shallow.display().to_string(),
        ],
        "2026-06-01T00:00:00+00:00",
    );
    std::fs::write(shallow.join("fathom.toml"), CONFIG).expect("the policy travels with the run");
    let base = repo.revision(1);
    let out = fathom(
        &shallow,
        &[
            "check",
            "--base",
            &base,
            "--require",
            "api",
            "--format",
            "json",
        ],
    );
    let text = stdout(&out) + &stderr(&out);
    assert_eq!(code(&out), 7, "{text}");
    let value: serde_json::Value = serde_json::from_str(stdout(&out).trim()).expect("json");
    assert_eq!(
        value["why"],
        format!(
            "fathom check failed: `api` is required and could not run — the base revision `{base}` is not \
             in this clone — it is a shallow clone"
        ),
        "{value}"
    );

    // The two tree-shaped checks say what they could not do, with the fetch command that fixes it. The four
    // that read the index are blocked for a different reason (no index here yet) and stay out of the way.
    let api = Repo::row(&value, "api");
    assert_eq!(api["status"], "blocked", "{api}");
    assert!(api["reason"].as_str().unwrap().contains("clone"), "{api}");
    assert!(
        api["remedy"].as_str().unwrap().contains("fetch-depth: 0"),
        "{api}"
    );
    assert_eq!(Repo::row(&value, "dependencies")["status"], "blocked");
    let required = value["summary"]["blocked_required"]
        .as_array()
        .expect("a list");
    assert_eq!(required.len(), 1);
    assert_eq!(required[0], "api");

    // And the same command without `--require` is a *report*: a shallow clone is a state of the pipeline,
    // not an objection, and the run that knows the difference is the difference between a red build and a
    // yellow note.
    let lenient = fathom(&shallow, &["check", "--base", &base, "--format", "json"]);
    let value: serde_json::Value = serde_json::from_str(stdout(&lenient).trim()).expect("json");
    assert!(
        value["summary"]["blocked_required"]
            .as_array()
            .expect("a list")
            .is_empty(),
        "nothing was required: {value}"
    );
    let _ = std::fs::remove_dir_all(&shallow);
}

// ---------------------------------------------------------------------------
// the five formats
// ---------------------------------------------------------------------------

#[test]
fn the_json_document_validates_against_its_schema() {
    let mut repo = Repo::new("json");
    repo.turn(
        "reach upward",
        &[(
            "src/l0/base.rs",
            "use crate::l1::up;\npub fn base() -> u32 {\n    up()\n}\n",
        )],
        &[
            ("src/l0/base.rs", vec!["src/l1/up.rs"]),
            ("src/l1/up.rs", vec![]),
        ],
    );
    let value = repo.json(&["--base", &repo.revision(1)]);
    let schema = validator::Schema::load("check.schema.json");
    let problems = validator::validate(&schema.document, &value, "$");
    assert!(problems.is_empty(), "not schema-shaped: {problems:#?}");
    // The schema is the contract, and the contract is named in the document it describes.
    assert_eq!(value["contract"], "fathom-check/1");
}

#[test]
fn every_output_format_produces_what_its_consumer_needs() {
    let mut repo = Repo::new("formats");
    repo.turn(
        "reach upward",
        &[(
            "src/l0/base.rs",
            "use crate::l1::up;\npub fn base() -> u32 {\n    up()\n}\n",
        )],
        &[
            ("src/l0/base.rs", vec!["src/l1/up.rs"]),
            ("src/l1/up.rs", vec![]),
        ],
    );
    let base = repo.revision(1);

    // --- SARIF, to a file the uploader can read.
    let sarif_path = repo.dir.join("fathom.sarif");
    let out = repo.check(&[
        "--base",
        &base,
        "--format",
        "sarif",
        "--out",
        sarif_path.to_str().unwrap(),
    ]);
    assert_eq!(
        code(&out),
        7,
        "the verdict travels regardless of format: {}",
        stdout(&out)
    );
    let sarif: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&sarif_path).expect("--out wrote a file"))
            .expect("SARIF is JSON");
    assert_eq!(sarif["version"], "2.1.0");
    let driver = &sarif["runs"][0]["tool"]["driver"];
    let ids: Vec<&str> = driver["rules"]
        .as_array()
        .expect("rules")
        .iter()
        .map(|rule| rule["id"].as_str().expect("an id"))
        .collect();
    let results = sarif["runs"][0]["results"].as_array().expect("results");
    assert!(!results.is_empty());
    for result in results {
        assert!(
            ids.contains(&result["ruleId"].as_str().unwrap()),
            "{} is used without being declared",
            result["ruleId"]
        );
        assert_eq!(
            result["kind"], "fail",
            "Fathom reports, it does not recommend"
        );
        assert!(
            result["partialFingerprints"]["primaryLocationLineHash"]
                .as_str()
                .is_some_and(|hash| hash.len() == 16)
        );
    }
    // The one sentence is the automation details' description, so the upload's banner and the exit code
    // that produced it cannot tell different stories.
    assert!(
        sarif["runs"][0]["automationDetails"]["description"]["text"]
            .as_str()
            .unwrap()
            .starts_with("fathom check failed:"),
        "{sarif}"
    );

    // --- GitHub step annotations: the failing findings, as workflow commands.
    let out = repo.check(&["--base", &base, "--format", "github"]);
    let annotations = stdout(&out);
    assert!(annotations.starts_with("::error "), "{annotations}");
    assert!(annotations.contains("file=src/l0/base.rs"), "{annotations}");
    assert!(
        !annotations.contains("api"),
        "a clean check annotates nothing: {annotations}"
    );

    // --- The job summary, appended to a file the runner will render.
    let summary = repo.dir.join("step-summary.md");
    std::fs::write(&summary, "# the pipeline's own heading\n\n").expect("write the summary");
    let out = repo.check(&[
        "--base",
        &base,
        "--format",
        "markdown",
        "--summary",
        summary.to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 7);
    let text = std::fs::read_to_string(&summary).expect("the summary is readable");
    assert!(text.starts_with("# the pipeline's own heading"), "{text}");
    assert!(text.contains("## fathom check"), "{text}");
    assert!(text.contains("| `layering` |"), "{text}");

    // --- Human, to stdout, with the table and the sentence.
    let out = repo.check(&["--base", &base]);
    let log = stdout(&out);
    assert!(log.contains("check         volume   status"), "{log}");
    assert!(log.contains("layering      error    ran"), "{log}");
    assert!(log.contains("fathom check failed:"), "{log}");
}

#[test]
fn annotations_are_optional_and_the_summary_appends_rather_than_replaces() {
    let repo = Repo::new("formats2");
    let base = repo.head();
    let out = repo.check(&["--base", &base, "--format", "github", "--no-annotations"]);
    assert!(
        stdout(&out).is_empty() || !stdout(&out).contains("::"),
        "{}",
        stdout(&out)
    );

    let summary = repo.dir.join("sum.md");
    for _ in 0..2 {
        let out = repo.check(&[
            "--format",
            "markdown",
            "--summary",
            summary.to_str().unwrap(),
        ]);
        assert_eq!(code(&out), 0, "{}", stdout(&out));
    }
    let text = std::fs::read_to_string(&summary).expect("the summary exists");
    assert_eq!(
        text.matches("## fathom check").count(),
        2,
        "two runs, two blocks: {text}"
    );
}

// ---------------------------------------------------------------------------
// policy failures are the operator's, not the gate's
// ---------------------------------------------------------------------------

#[test]
fn an_unreadable_policy_is_exit_five_with_the_key_and_the_remedy() {
    let repo = Repo::new("policy");
    std::fs::write(
        repo.dir.join("fathom.toml"),
        CONFIG.replace("mode = \"deny\"", "mode = \"loud\""),
    )
    .expect("rewrite the config");
    let out = repo.check(&[]);
    assert_eq!(code(&out), 5, "{}\n{}", stdout(&out), stderr(&out));
    let text = stderr(&out);
    assert!(text.contains("check.mode"), "{text}");
    assert!(text.contains("fathom.toml"), "{text}");
}

#[test]
fn explain_prints_the_policy_in_force_and_exits_zero_without_running() {
    // The answer to "why did that check not fire", and the thing to paste into a review comment. It must not
    // need an index: the policy is a file.
    let repo = Repo::new("explain");
    std::fs::remove_dir_all(repo.dir.join(".fathom")).expect("remove the store");
    let out = repo.check(&["--explain"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("policy v1"), "{text}");
    assert!(text.contains("mode deny"), "{text}");
    assert!(text.contains("fail at warning"), "{text}");
    assert!(text.contains("layering"), "{text}");
    assert!(text.contains("min_new_lines"), "{text}");
}

#[test]
fn ignore_and_only_and_severity_overrides_apply_to_the_run() {
    let mut repo = Repo::new("overrides");
    repo.turn(
        "reach upward",
        &[(
            "src/l0/base.rs",
            "use crate::l1::up;\npub fn base() -> u32 {\n    up()\n}\n",
        )],
        &[
            ("src/l0/base.rs", vec!["src/l1/up.rs"]),
            ("src/l1/up.rs", vec![]),
        ],
    );
    let base = repo.revision(1);
    assert_eq!(code(&repo.check(&["--base", &base])), 7);

    // `--skip` is a stated choice: the check reports itself off and the run has nothing left to fail on.
    let value = repo.json(&["--base", &base, "--skip", "layering"]);
    assert_eq!(Repo::row(&value, "layering")["status"], "off");
    assert_eq!(value["outcome"], "pass");
    assert_eq!(
        code(&repo.check(&["--base", &base, "--skip", "layering"])),
        0
    );

    // `--severity layering=note` keeps the finding and drops its volume below the threshold.
    let value = repo.json(&["--base", &base, "--severity", "layering=note"]);
    assert_eq!(Repo::rules(&value, "layering"), vec![UPWARD], "still found");
    assert_eq!(
        Repo::row(&value, "layering")["findings"][0]["severity"],
        "note"
    );
    assert_eq!(
        code(&repo.check(&["--base", &base, "--severity", "layering=note"])),
        0
    );

    // `--ignore` silences it and says what it silenced.
    let value = repo.json(&["--base", &base, "--ignore", &format!("layering:{UPWARD}")]);
    assert_eq!(Repo::rules(&value, "layering"), Vec::<String>::new());
    assert_eq!(value["summary"]["silenced"], 1, "{value}");
    assert!(
        value["silenced"][0]["pattern"]
            .as_str()
            .unwrap()
            .contains("layering:")
    );

    // A flag with a nonsense value is usage (2), not a policy refusal (5).
    assert_eq!(code(&repo.check(&["--severity", "layerings=off"])), 2);
    assert_eq!(code(&repo.check(&["--fail-on", "sometimes"])), 2);
    assert_eq!(code(&repo.check(&["--range", "nonsense"])), 2);
}

// ---------------------------------------------------------------------------
// the budget, and the shape of a real run
// ---------------------------------------------------------------------------

#[test]
fn a_run_over_a_repo_with_forty_commits_finishes_in_well_under_a_minute() {
    // *"under 60 seconds on a medium repo with a shallow clone"* is an acceptance criterion, and the honest
    // version of it lives in `ci.yml`, where it is measured on `factspantry`. This is the local half: a
    // fixture with real episodes and real graph revisions, timed, against the same budget — so a change that
    // makes the gate quadratically slower is caught here as well as there.
    let mut repo = Repo::new("budget");
    for i in 0..40 {
        repo.turn(
            &format!("step {i}"),
            &[(
                &format!("src/l0/mod{i}.rs"),
                "pub fn thing() -> u32 {\n    1\n}\n",
            )],
            &[
                ("src/l0/base.rs", vec![]),
                ("src/l1/up.rs", vec![]),
                ("src/l0/extra.rs", vec![]),
            ],
        );
    }
    let started = std::time::Instant::now();
    let out = repo.check(&["--base", "HEAD~20", "--head", "HEAD"]);
    let took = started.elapsed();
    assert_eq!(code(&out), 0, "{}", stdout(&out));
    assert!(
        took.as_secs() < 60,
        "a 40-commit range took {took:?}; the budget is 60s"
    );
}

/// A store the indexer filled but that has no module graph — which, since `reco-dlp`, takes an explicit
/// `--no-graph` or a `graph.enabled = false` to arrange. The remedy may now be `fathom index`, because that
/// command really does write the lane; what it must also do is say *which* knob could make the index decline,
/// so the reader is not left re-running a command that refuses for a reason the gate never mentioned.
#[test]
fn an_index_with_no_module_graph_names_the_lane_and_the_knobs_that_bound_it() {
    let dir = std::env::temp_dir().join(format!("fathom-check-cli-nograph-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let start = "2026-06-01T09:00:00+00:00";
    git(&dir, &["init", "-q", "--initial-branch=main"], start);
    git(&dir, &["config", "user.name", "Rue Reviewer"], start);
    git(
        &dir,
        &["config", "user.email", "rue@example.invalid"],
        start,
    );
    std::fs::write(dir.join("fathom.toml"), CONFIG).expect("config");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("src").join("a.rs"),
        "pub fn a() -> u32 {\n    1\n}\n",
    )
    .expect("a.rs");
    git(&dir, &["add", "-A"], start);
    git(&dir, &["commit", "-q", "-m", "one"], start);
    std::fs::write(
        dir.join("src").join("b.rs"),
        "pub fn b() -> u32 {\n    2\n}\n",
    )
    .expect("b.rs");
    git(&dir, &["add", "-A"], "2026-06-02T10:00:00+00:00");
    git(
        &dir,
        &["commit", "-q", "-m", "two"],
        "2026-06-02T10:00:00+00:00",
    );

    // Indexed with the graph pass held back, which is the only way left to arrive at "a store the indexer
    // has filled and that still has no module graph" (`reco-dlp`).
    let out = fathom(&dir, &["index", ".", "--no-graph"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let out = fathom(
        &dir,
        &[
            "check", "--base", "HEAD~1", "--head", "HEAD", "--format", "json",
        ],
    );
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("the gate prints JSON");
    for name in ["layering", "cycles"] {
        let row = value["checks"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|row| row["check"] == name)
            .expect("the row");
        assert_eq!(row["status"], "blocked", "{name} has nothing to read");
        let remedy = row["remedy"].as_str().expect("a remedy");
        // `fathom index` is the remedy now, and it is allowed to be — but only alongside the reason it might
        // not help: the pass caps itself, and a reader who is told "run the indexer" and gets a refusal back
        // has been sent somewhere without being told what to look for there.
        assert!(
            remedy.starts_with("run `fathom index"),
            "{name} should send the reader to the pass that writes the lane: {remedy}"
        );
        for knob in ["graph.window", "graph.max_corpus_mib"] {
            assert!(
                remedy.contains(knob),
                "{name}'s remedy does not name {knob}, the cap that would make that run decline: {remedy}"
            );
        }
        assert!(
            remedy.contains("module graph"),
            "{name} says which lane is missing, not just that something is: {remedy}"
        );
    }
    // And the four checks that do not need the graph still ran — the gate degrades, it does not fold.
    let ran = value["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["status"] == "ran")
        .count();
    assert_eq!(
        ran, 4,
        "api, dependencies, tests and sign-off do not need a module graph"
    );
    assert_eq!(value["summary"]["blocked"], 2);
}

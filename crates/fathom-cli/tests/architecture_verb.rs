//! `fathom architecture`, driven as a binary (`e3.9`, `reco-al1`).
//!
//! The ticket's deliverable is a *surface* — "a five-dimension scorecard … and `fathom architecture --json`
//! for CI and dashboards" — and a surface is only real if a process can run it. So these tests execute the
//! binary and assert on what comes out, the way `tests/episodes_verb.rs` does for the episode list:
//!
//! * **the refusal is a first-class answer.** An index with no module graph is exit 12 with a remedy and
//!   nothing on stdout, because a CI script that pipes `--json` must never receive a scorecard-shaped object
//!   built from an empty graph;
//! * **text and `--json` are one read rendered twice** — same score, same five dimensions, same trend, and
//!   the `fathom-architecture/1` contract named on the wire;
//! * **scoring writes, and `--read-only` does not**, because a dashboard polling every minute must not be
//!   the thing that decides what the index believes;
//! * **a row is written once and `--refresh` is the recovery** when the rules or the model change — the
//!   honest fix for a derived fact is to re-derive it, not to patch it;
//! * **and "since my last review" is the reviewer's own act**, taken from `review_state`, not from a
//!   timestamp someone guessed at.
//!
//! The graphs are seeded through `fathom_analyze::graph::Lane::record` against the store the indexer just
//! wrote, keyed by the repository's own tree oids — the same shape `e3.2`'s writer produces and the shape
//! this verb reads.
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
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
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
fn module(
    path: &str,
    exports: &[&str],
    deps: &[(&str, &str)],
) -> fathom_analyze::structure::FileStructure {
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
            .map(|(target, symbol)| Edge {
                target: (*target).to_string(),
                symbol: (*symbol).to_string(),
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

/// A repository with three commits, an index, and (when asked) a graph snapshot per commit.
struct Repo {
    dir: PathBuf,
    /// The tree oid of each commit, oldest first.
    trees: Vec<String>,
}

impl Repo {
    fn new(label: &str) -> Repo {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fathom-arch-cli-{label}-{}-{index}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the fixture directory");
        git(&dir, &["init", "-q", "--initial-branch=main"]);
        git(&dir, &["config", "user.name", "Rue Reviewer"]);
        git(&dir, &["config", "user.email", "rue@example.invalid"]);
        let exclude = dir.join(".git").join("info").join("exclude");
        std::fs::create_dir_all(exclude.parent().expect("git made info/")).expect("info/");
        std::fs::write(&exclude, ".fathom/\n").expect("write the local exclude");

        let mut trees = Vec::new();
        for (step, file) in ["reader", "writer", "wider"].iter().enumerate() {
            std::fs::write(
                dir.join(format!("src_{file}.rs")),
                format!("pub fn {file}() -> u32 {{ {}\n}}\n", step + 1),
            )
            .expect("write the file");
            git(&dir, &["add", "-A"]);
            commit(&dir, &format!("step {step}: add {file}"), step);
            trees.push(git(&dir, &["rev-parse", "HEAD^{tree}"]));
        }
        let out = fathom(&dir, &["index", "."]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        Repo { dir, trees }
    }

    /// Record a module graph for each of this repository's commit trees, oldest first.
    ///
    /// The graphs get worse as they go: the last revision closes a cycle, so a backfilled trend has a
    /// direction and a since-review delta has something to find.
    fn seed_graphs(&self) {
        let mut store = self.open();
        {
            let mut lane = fathom_analyze::graph::Lane::new(
                &mut store,
                fathom_analyze::graph::Builder::default(),
            )
            .expect("a lane over a real store");
            let mut base: Option<String> = None;
            for (index, tree) in self.trees.iter().enumerate() {
                let files = if index + 1 == self.trees.len() {
                    vec![
                        module("src_a.rs", &["a"], &[("src_b.rs", "b")]),
                        module("src_b.rs", &["b"], &[("src_a.rs", "a")]),
                    ]
                } else {
                    vec![
                        module("src_a.rs", &["a"], &[]),
                        module("src_b.rs", &["b"], &[("src_a.rs", "a")]),
                    ]
                };
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
                let given: Vec<fathom_analyze::graph::ModuleInput<'_>> = files
                    .iter()
                    .zip(&blobs)
                    .map(|(f, blob)| fathom_analyze::graph::ModuleInput {
                        path: f.path.as_str(),
                        blob: blob.as_str(),
                        file: Some(f),
                    })
                    .collect();
                lane.record(
                    tree,
                    base.as_deref(),
                    &given,
                    1_780_000_000_000_000 + i64::try_from(index).unwrap(),
                )
                .expect("record a graph snapshot");
                base = Some(tree.clone());
            }
            lane.flush_interner().expect("flush the interner");
        }
        drop(store);
    }

    fn open(&self) -> fathom_store::spine::Store {
        fathom_store::spine::Store::open(&self.store_path()).expect("the store opens")
    }

    fn store_path(&self) -> PathBuf {
        self.dir.join(".fathom").join("index.sqlite3")
    }

    fn architecture(&self, args: &[&str]) -> Output {
        let mut all = vec!["architecture"];
        all.extend_from_slice(args);
        fathom(&self.dir, &all)
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let out = self.architecture(&all);
        assert_eq!(code(&out), 0, "{}: {}", all.join(" "), stderr(&out));
        let text = stdout(&out);
        assert!(
            !text.trim().contains('\n'),
            "one JSON line for a pipeline to pipe:\n{text}"
        );
        serde_json::from_str(text.trim()).unwrap_or_else(|err| panic!("not JSON: {err}\n{text}"))
    }

    fn scored(&self) -> i64 {
        self.open().scorecard_count().expect("count")
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn commit(dir: &Path, subject: &str, step: usize) {
    let date = format!("2026-06-0{}T10:00:00+00:00", step + 1);
    let out = Command::new("git")
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .args(["commit", "-q", "-m", subject])
        .output()
        .expect("git commit runs");
    assert!(
        out.status.success(),
        "commit {subject}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// **The refusal.** No graph is the index's problem (exit 12), a plain directory is the caller's (exit 3),
/// and neither prints a payload for a script to mistake for an answer.
#[test]
fn an_index_with_no_graph_refuses_with_its_code_and_a_remedy() {
    let repo = Repo::new("nograph");
    let out = repo.architecture(&[]);
    assert_eq!(code(&out), 12, "{}", stderr(&out));
    assert!(stdout(&out).is_empty(), "a refusal prints nothing to pipe");
    let text = stderr(&out);
    assert!(text.contains("no module graph"), "{text}");
    assert!(
        text.contains("fathom index"),
        "the remedy is on the line: {text}"
    );

    let json = repo.architecture(&["--json"]);
    assert_eq!(code(&json), 12);
    assert!(stdout(&json).is_empty());

    // Not a repository at all is a different code, because it is a different failure.
    let plain = std::env::temp_dir().join(format!("fathom-arch-plain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&plain);
    std::fs::create_dir_all(&plain).expect("create the plain directory");
    let out = fathom(&plain, &["architecture"]);
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    std::fs::remove_dir_all(&plain).ok();
}

/// The head is scored, printed, and stored — and the second run reads its own row rather than re-scoring.
#[test]
fn the_head_is_scored_printed_and_written_once() {
    let repo = Repo::new("head");
    repo.seed_graphs();
    assert_eq!(repo.scored(), 0, "seeding a graph writes no scorecard");

    let out = repo.architecture(&[]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    // The five dimensions, by name, each with its number, in the report's own words.
    for dimension in ["boundaries", "coupling", "acyclicity", "api", "hotspots"] {
        assert!(text.contains(dimension), "no {dimension} line:\n{text}");
    }
    for section in ["scorecard", "trend", "knobs"] {
        assert!(text.contains(section), "no {section} section:\n{text}");
    }
    assert!(text.contains("architecture-1"), "{text}");
    assert!(text.contains("architecture"), "{text}");
    // The head revision is named, and the graph came from the repository's own tree.
    assert!(
        text.contains(&repo.trees[2][..7]),
        "the head is the newest graph revision:\n{text}"
    );
    assert_eq!(repo.scored(), 1, "one row was written, for the head");

    // A second run reads the row it wrote. The notice says so, because a reader is entitled to know whether
    // the number in front of them was computed now or stored earlier.
    let again = stdout(&repo.architecture(&[]));
    assert_eq!(repo.scored(), 1, "a row is written once");
    assert!(
        again.contains("--refresh") || again.contains("stored"),
        "the report says where the head's numbers came from:\n{again}"
    );
    let first_score = text
        .lines()
        .find(|line| line.contains("/100"))
        .unwrap_or_default()
        .to_owned();
    let second_score = again
        .lines()
        .find(|line| line.contains("/100"))
        .unwrap_or_default()
        .to_owned();
    assert_eq!(
        first_score, second_score,
        "the same revision scores the same"
    );
}

/// `--json` is the contract a CI job and a dashboard read, and it agrees with the text line for line.
#[test]
fn the_json_document_is_the_contract_and_agrees_with_the_text() {
    let repo = Repo::new("json");
    repo.seed_graphs();
    let text = stdout(&repo.architecture(&[]));
    let report = repo.json(&[]);
    assert_eq!(report["contract"], "fathom-architecture/1");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["scorecard"]["model"], "architecture-1");
    let score = report["scorecard"]["score"].as_u64().expect("a score");
    assert!(
        report["scorecard"]["scored"].as_bool().unwrap(),
        "a score exists"
    );
    assert!(score <= 100, "a score is a percentage: {score}");
    // The five dimensions, each with the question it answers and the arithmetic that answered it.
    let terms = report["scorecard"]["terms"].as_array().expect("five terms");
    assert_eq!(terms.len(), 5);
    let ids: Vec<&str> = terms
        .iter()
        .map(|term| term["dimension"].as_str().expect("an id"))
        .collect();
    assert_eq!(
        ids,
        vec!["boundaries", "coupling", "acyclicity", "api", "hotspots"],
        "a fixed order, so a dashboard can index by position"
    );
    for term in terms {
        assert!(term["question"].is_string());
        assert!(term["source"].as_str().unwrap().starts_with('e'));
        assert!(term["points"].is_i64());
        assert!(term["weight"].as_f64().unwrap() > 0.0);
    }
    // The index block answers "what did this read?" — the same question every other headless verb answers.
    assert_eq!(report["index"]["revisions"].as_i64(), Some(3));
    assert_eq!(report["index"]["scored"].as_i64(), Some(1));
    assert_eq!(report["index"]["unscored"].as_i64(), Some(2));
    assert_eq!(
        report["index"]["head"].as_str(),
        Some(repo.trees[2].as_str())
    );
    // The policy is exported with the score, so a stored artefact can be read a year later.
    assert_eq!(
        report["policy"]["weights"].as_array().map(Vec::len),
        Some(5)
    );
    assert_eq!(report["policy"]["untouched"], true);
    assert_eq!(report["policy"]["alert_below"], 50);
    // The trend, including what it refused.
    assert_eq!(report["trend"]["rows_read"].as_i64(), Some(1));
    assert_eq!(report["trend"]["rows_foreign"].as_i64(), Some(0));
    assert_eq!(report["trend"]["rows_unreadable"].as_i64(), Some(0));
    // The review block is present even when nobody has reviewed: its absence is a stated absence.
    assert_eq!(report["since_last_review"]["anchor"]["kind"], "none");
    // And the score the text printed is the score the payload carries.
    assert!(
        text.contains(&format!("{score}")),
        "the shell prints the same number: {text}"
    );
    assert!(text.contains("scorecard"), "{text}");
}

/// `--backfill` pays for the history, `--read-only` refuses to pay at all, and `--history` bounds the read.
#[test]
fn backfill_writes_the_history_and_read_only_writes_nothing() {
    let repo = Repo::new("backfill");
    repo.seed_graphs();
    let report = repo.json(&["--backfill", "5"]);
    assert_eq!(repo.scored(), 3, "every graph revision now has a row");
    assert_eq!(report["index"]["scored"].as_i64(), Some(3));
    assert_eq!(report["index"]["unscored"].as_i64(), Some(0));
    assert_eq!(report["trend"]["rows_read"].as_i64(), Some(3));
    assert_eq!(report["trend"]["series"].as_array().map(Vec::len), Some(3));
    // The oldest revision has no API comparison, and the newest one does: the per-revision "before" is the
    // revision before it in the index, not a guess.
    let scores: Vec<u64> = report["trend"]["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|point| point["score"].as_u64().expect("a score"))
        .collect();
    assert_eq!(scores.len(), 3);
    assert_eq!(
        scores[0],
        report["scorecard"]["score"].as_u64().expect("a score"),
        "newest first"
    );
    // The cycle in the newest revision costs it: the trend is not flat, which is the whole point of drawing
    // one.
    assert!(
        scores[0] < scores[2],
        "the newest revision closed a cycle: {scores:?}"
    );

    // `--read-only` is the dashboard's flag: the same document, no writes.
    let before = repo.scored();
    let fresh = std::env::temp_dir().join(format!("fathom-arch-ro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(&fresh).expect("create");
    // Copy the repository, including its store, then score a read-only run on it.
    copy_dir(&repo.dir, &fresh);
    let out = fathom(
        &fresh,
        &[
            "architecture",
            "--read-only",
            "--backfill",
            "5",
            "--refresh",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(
        fathom_store::spine::Store::open(&fresh.join(".fathom").join("index.sqlite3"))
            .expect("open")
            .scorecard_count()
            .expect("count"),
        before,
        "a read-only run neither writes nor clears"
    );
    std::fs::remove_dir_all(&fresh).ok();

    // `--history 2` reads the two newest rows and says the trend is shorter than the index.
    let bounded = repo.json(&["--history", "2"]);
    assert_eq!(bounded["trend"]["rows_read"].as_i64(), Some(2));
    assert_eq!(bounded["index"]["revisions"].as_i64(), Some(3));
}

/// `--refresh` is the recovery path for a row that no longer matches the rules: drop and re-derive.
#[test]
fn refresh_re_derives_rows_a_model_change_or_a_bad_row_made_untrustworthy() {
    let repo = Repo::new("refresh");
    repo.seed_graphs();
    repo.architecture(&["--backfill", "5"]);
    assert_eq!(repo.scored(), 3);
    // A row from another model version: not corrupt, just not comparable, and the report must say which.
    {
        let mut store = repo.open();
        let head = repo.trees[2].clone();
        let mut row = store
            .scorecard_get(&head)
            .expect("get")
            .expect("the head has a row");
        row.dims = row.dims.replace("\"architecture-1\"", "\"architecture-0\"");
        store.scorecard_clear().expect("clear");
        store.scorecard_write(&row).expect("write the foreign row");
    }
    let report = repo.json(&[]);
    assert_eq!(
        report["trend"]["rows_foreign"].as_i64(),
        Some(1),
        "{report}"
    );
    let text = stdout(&repo.architecture(&[]));
    assert!(
        text.contains("architecture-0") && text.contains("--refresh"),
        "the foreign row is named and the remedy is offered:\n{text}"
    );
    // The head cannot be overwritten in place — a row is written once — so the fix is the refresh, and the
    // refresh re-derives everything the run needs.
    let refreshed = repo.json(&["--refresh", "--backfill", "5"]);
    assert_eq!(refreshed["trend"]["rows_foreign"].as_i64(), Some(0));
    assert_eq!(repo.scored(), 3, "and the rows are all there again");
    assert!(
        !serde_json::to_string(&refreshed["scorecard"])
            .unwrap()
            .contains("architecture-0"),
        "no foreign model survives a refresh"
    );
}

/// The reviewer's own review, taken from `review_state`, is what "since my last review" means.
#[test]
fn the_delta_is_since_the_reviewers_own_last_review() {
    let repo = Repo::new("review");
    repo.seed_graphs();
    // Nobody has reviewed: the report says who is missing rather than inventing a starting point.
    let cold = repo.json(&["--backfill", "5"]);
    assert_eq!(cold["since_last_review"]["anchor"]["kind"], "none");
    assert!(
        cold["since_last_review"]["anchor"]["why"]
            .as_str()
            .unwrap_or_default()
            .contains("nobody has reviewed"),
        "{cold}"
    );

    // The reviewer accepts the *first* episode — the one whose revision is the oldest scored row. That is
    // the review `fathom review x` would write, and the scorecard reads the same table the shell does.
    let ids: Vec<String> = {
        let out = fathom(&repo.dir, &["episodes", "--json"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        serde_json::from_str::<serde_json::Value>(&stdout(&out)).expect("the episode list is JSON")
            ["episodes"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| row["id"].as_str().expect("an id").to_owned())
            .collect()
    };
    assert_eq!(ids.len(), 3, "three commits, three episodes");
    {
        let mut store = repo.open();
        store
            .set_review(
                &ids[2],
                fathom_store::episode::ReviewState::Accepted,
                "Rue Reviewer <rue@example.invalid>",
                None,
                1,
            )
            .expect("stamping a marker is the same write the shell makes");
    }
    let report = repo.json(&["--backfill", "5"]);
    let since = &report["since_last_review"];
    // The reviewed episode's own commit is one of the scored revisions, so this is rung 1 of the ladder —
    // the exact answer, not an inference from a timestamp — and the revision it names is a revision this
    // index actually scored.
    let kind = since["anchor"]["kind"].as_str().unwrap_or_default();
    assert!(
        matches!(kind, "revision" | "episode"),
        "a review attached to a scored revision resolves exactly: {report}"
    );
    let anchor_tree = since["anchor"]["revision"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        repo.trees.iter().any(|tree| tree == &anchor_tree),
        "the anchor is a revision of this repository: {anchor_tree}"
    );
    assert_eq!(since["anchor"]["review"]["state"], "accepted");
    assert_eq!(
        since["anchor"]["review"]["actor"], "Rue Reviewer <rue@example.invalid>",
        "the reviewer is named, not inferred"
    );
    // The delta spans the revisions since that episode, counted over the series the report itself printed:
    // the anchor's position from the newest end, inclusive. That is the claim a reader checks with their own
    // calendar — "two revisions since I looked" — so it is asserted against the printed series, not against
    // a count this test keeps separately.
    let series = report["trend"]["series"].as_array().expect("a series");
    let at = series
        .iter()
        .position(|point| point["revision_full"].as_str() == Some(anchor_tree.as_str()))
        .expect("the anchor is in the series");
    assert_eq!(
        since["revisions"].as_i64(),
        Some(i64::try_from(at).unwrap() + 1),
        "{report}"
    );
    assert!(
        at >= 1,
        "the reviewed revision is not the head: {anchor_tree}"
    );
    // And it is the news: the newest revision closed a cycle, so the score went down and the cycle dimension
    // is what moved.
    assert_eq!(since["direction"].as_str(), Some("declined"));
    assert!(since["score_delta"].as_i64().unwrap() < 0, "{since}");
    assert!(
        since["worst"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("acyclicity")),
        "{since}"
    );
    // And the same run writes the answer down. A reader next month — or one whose review state has since
    // been cleared, or whose episode has been re-segmented out from under the anchor — can still ask what
    // "since your last review" meant on the day this revision was scored, and get the recorded answer
    // rather than a recomputation that no longer has an anchor to recompute from.
    let head_tree = repo.trees[repo.trees.len() - 1].clone();
    let stored = repo
        .open()
        .scorecard_get(&head_tree)
        .expect("read the head row")
        .expect("the head has a row")
        .since
        .expect("and the companion was written in the same run that learned the anchor");
    let recorded: serde_json::Value = serde_json::from_str(&stored).expect("the companion is JSON");
    assert_eq!(
        recorded["anchor_kind"].as_str(),
        Some("revision"),
        "{recorded}"
    );
    assert_eq!(
        recorded["anchor_revision"].as_str().unwrap_or_default(),
        anchor_tree,
        "the recorded anchor is the one the delta used"
    );
    assert_eq!(
        recorded["revisions"], since["revisions"],
        "and the same count the report printed"
    );
    assert_eq!(
        report["since_recorded"], recorded,
        "the report prints what the row holds, not a second computation of it"
    );
    assert!(
        text_has_no_companion_before_a_review(&repo.dir),
        "a run with no review writes no companion — an absence is not a zero"
    );
    let text = stdout(&repo.architecture(&["--backfill", "0"]));
    assert!(
        text.contains("since your last review"),
        "the shell leads with the reviewer's question:\n{text}"
    );
    assert!(text.contains("accepted"), "and names what they did: {text}");
}

/// The knobs the report prints are the knobs in force, and `--explain` prints them without a graph at all.
#[test]
fn the_knobs_are_reported_and_explainable() {
    let repo = Repo::new("knobs");
    // `--explain` needs no graph, so it answers on a repository the scorecard cannot score yet.
    let out = repo.architecture(&["--explain"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    for knob in [
        "weight boundaries",
        "weight coupling",
        "weight acyclicity",
        "weight api",
        "weight hotspots",
        "window",
        "flat within",
        "trend cap",
        "coupling_ceiling",
        "api_knee",
        "hotspot_bar",
        "alert_below",
    ] {
        assert!(text.contains(knob), "no {knob} in --explain:\n{text}");
    }
    // Every dimension's full question text, not just a slug: an owner tuning a weight has to know what they
    // are tuning, and reading five identifiers off a screen is not knowing.
    for sentence in [
        "does this revision respect the layering rules this repository wrote down?",
        "how much of the graph does an average module reach?",
        "how much of the graph is inside a loop that nobody budgeted?",
        "what did this episode do to the contracts other code compiles against?",
        "how many modules are expensive to change and widely felt?",
    ] {
        assert!(
            text.contains(sentence),
            "--explain does not spell out {sentence:?}:\n{text}"
        );
    }
    // And the curve of each, with the constant it divides by — four shares and one knee — plus the total's
    // arithmetic, so a reader holding the exported JSON can redo the score on paper. This is the ticket's
    // "documented formula", shipped as program output rather than as prose that can drift.
    for formula in [
        "curve: share: violations / checked dependency edges",
        "curve: share: dependency edges / (modules x 8.00 edges per module)",
        "curve: share: modules inside an unaccepted cycle / modules",
        "curve: knee: n / (n + 4.00), n = exports a module stopped exporting",
        "curve: share: modules scoring 70 or above / scored modules",
        "points = round(weight / measured weight sum x health), summed as hundredths",
        "score = points / 100",
    ] {
        assert!(
            text.contains(formula),
            "--explain does not print {formula:?}:\n{text}"
        );
    }
    // A `[scorecard]` table in the repository's `fathom.toml` is in force, is printed as tuned, and says
    // which way it was bent from the built-in.
    std::fs::write(
        repo.dir.join("fathom.toml"),
        "[scorecard]\nalert_below = 75\n[scorecard.weight]\nboundaries = 0.5\n",
    )
    .expect("write fathom.toml");
    let tuned = stdout(&repo.architecture(&["--explain"]));
    assert!(
        tuned.contains("boundaries —") && tuned.contains("weight: 0.5 (default 0.3)"),
        "a tuned weight is printed with the default it replaced:\n{tuned}"
    );
    assert!(
        tuned.contains("alert_below 75"),
        "the tuned alert bar is printed:\n{tuned}"
    );
    repo.seed_graphs();
    let report = repo.json(&[]);
    assert_eq!(report["policy"]["alert_below"], 75);
    assert_eq!(report["policy"]["untouched"], false);
    let boundaries = report["policy"]["weights"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["dimension"] == "boundaries")
        .expect("boundaries");
    assert_eq!(boundaries["weight"].as_f64(), Some(0.5));
    assert_eq!(boundaries["origin"].as_str(), Some("fathom.toml"));
    let text = stdout(&repo.architecture(&[]));
    assert!(
        text.contains("0.50"),
        "the tuned weight is printed:\n{text}"
    );
}

/// A flag that names nothing is the reviewer's typo, refused with exit 2 and no payload.
#[test]
fn a_revision_that_is_not_in_the_index_is_a_usage_error() {
    let repo = Repo::new("usage");
    repo.seed_graphs();
    let out = repo.architecture(&["--revision", "deadbeef"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(stdout(&out).is_empty());
    assert!(
        stderr(&out).contains("--revision"),
        "the message names the flag: {}",
        stderr(&out)
    );
    // An unambiguous prefix of a real revision works, and scores that revision rather than the head.
    let prefix = &repo.trees[0][..8];
    let out = repo.architecture(&["--revision", prefix, "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let report: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("JSON");
    assert_eq!(
        report["index"]["head"].as_str(),
        Some(repo.trees[0].as_str())
    );
    // `--help` lists the flags, because a verb is only a contract if it can be read.
    let help = fathom(&repo.dir, &["architecture", "--help"]);
    assert_eq!(code(&help), 0);
    for flag in [
        "--json",
        "--revision",
        "--backfill",
        "--history",
        "--refresh",
        "--read-only",
        "--explain",
        "--root",
    ] {
        assert!(stdout(&help).contains(flag), "{flag} missing from --help");
    }
}

/// A small recursive copy, so the read-only case can run on a store without touching the original.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create the copy");
    for entry in std::fs::read_dir(from).expect("read the source") {
        let entry = entry.expect("an entry");
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            std::fs::copy(&path, &target).expect("copy a file");
        }
    }
}

/// The payload against the checked-in schema, through the same round trip `tests/schemas.rs` runs for every
/// other verb — run here rather than there because this payload only exists once a module graph does, and
/// the shared fixture builds none. Re-serialising and re-validating is not theatre: a consumer gets the
/// bytes, and a payload that validates only before it goes through a string is a different promise than the
/// one the test checked. Two payloads are checked, because the flags change which sections are populated —
/// a bare head run, and a backfilled one with a trend, a previous revision and a review to anchor on.
#[cfg(feature = "sqlite-bundled")]
#[test]
fn the_json_payload_validates_against_the_checked_in_schema() {
    #[path = "validator.rs"]
    mod validator;

    let repo = Repo::new("schema");
    repo.seed_graphs();
    let ids: Vec<String> = {
        let out = fathom(&repo.dir, &["episodes", "--json"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        serde_json::from_str::<serde_json::Value>(&stdout(&out)).expect("the episode list is JSON")
            ["episodes"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| row["id"].as_str().expect("an id").to_owned())
            .collect()
    };
    {
        let mut store = repo.open();
        store
            .set_review(
                &ids[ids.len() - 1],
                fathom_store::episode::ReviewState::Accepted,
                "Rue Reviewer <rue@example.invalid>",
                None,
                1,
            )
            .expect("stamping a marker is the same write the shell makes");
    }

    let schema = validator::Schema::load("architecture.schema.json");
    for args in [
        vec!["--json"],
        vec!["--backfill", "0", "--history", "--json"],
    ] {
        let out = repo.architecture(&args);
        assert_eq!(code(&out), 0, "{}: {}", args.join(" "), stderr(&out));
        let text = stdout(&out).trim().to_owned();
        assert!(!text.contains('\n'), "one JSON line: {text}");
        let value: serde_json::Value = serde_json::from_str(&text).expect("one JSON value");
        let problems = validator::validate(&schema.document, &value, "$");
        assert!(problems.is_empty(), "{args:?}: {problems:#?}\n{text}");
        assert_eq!(
            value["contract"].as_str(),
            Some("fathom-architecture/1"),
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
}

/// `--explain` is the scale, not the measurement, so it answers in a repository nobody has indexed yet —
/// which is where the question "what would you score, and how?" actually gets asked. The score itself still
/// refuses without an index, and the two refusals are different: one is a print, the other is a finding.
#[test]
fn explain_answers_before_there_is_an_index() {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-arch-cli-explain-only-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    git(&dir, &["config", "user.name", "Rue Reviewer"]);
    git(&dir, &["config", "user.email", "rue@example.invalid"]);
    std::fs::write(dir.join("a.rs"), "pub fn a() -> u32 { 1 }\n").expect("write a file");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);

    let out = fathom(&dir, &["architecture", "--explain"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("weight: 0.3") && text.contains("score = points / 100"),
        "the built-in scale, printed:\\n{text}"
    );

    std::fs::write(
        dir.join("fathom.toml"),
        "[scorecard]\nalert_below = 75\n[scorecard.weight]\nboundaries = 0.5\n",
    )
    .expect("write fathom.toml");
    let tuned = stdout(&fathom(&dir, &["architecture", "--explain"]));
    assert!(
        tuned.contains("weight: 0.5 (default 0.3)") && tuned.contains("alert_below 75"),
        "the repository's own tuning, with the default it replaced:\n{tuned}"
    );

    // And the score, which is a measurement, says what is missing instead of printing a zero.
    let scored = fathom(&dir, &["architecture"]);
    assert_eq!(code(&scored), 12);
    assert!(
        stderr(&scored).contains("fathom index"),
        "the remedy is the missing step:\n{}",
        stderr(&scored)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The first run of this fixture scored its revisions before anyone had reviewed, and none of those rows may
/// carry a companion: "no anchor yet" has to stay an absence in the stored state, not a `{"revisions":0}`
/// that a later reader would take for a delta of zero.
#[cfg(feature = "sqlite-bundled")]
fn text_has_no_companion_before_a_review(dir: &Path) -> bool {
    let store = fathom_store::spine::Store::open(&dir.join(".fathom").join("index.sqlite3"))
        .expect("open the store");
    // The rows this fixture scored before the review existed are the ones with no `since` — and after the
    // review run, exactly one row (the head) has one. The count is the whole check.
    let with_companion = store
        .scorecard_rows(0)
        .expect("rows")
        .iter()
        .filter(|row| row.since.is_some())
        .count();
    with_companion <= 1
}

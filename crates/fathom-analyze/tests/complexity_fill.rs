//! The index-time complexity fill (`e8.4`, `reco-l9bz.4`), measured against real repositories.
//!
//! The ticket's acceptance criteria are four sentences and three of them are about *absence*: where the
//! measurement runs, what stops it, and what a reviewer is told when it did not happen. Everything here
//! goes through [`index::run`] — the entry point the CLI uses — against a real store, because a ceiling
//! that only bites in a unit test of a helper is a ceiling nobody can rely on, and because the difference
//! between "the analyzer could not" and "the index would not pay for it" lives in the ordering of the pass
//! and not in any single function.
//!
//! What is asserted, in order:
//!
//! * a file that *can* be counted is counted, on both sides, with the worst unit beside it — `e8.3`'s
//!   columns, filled at last;
//! * every absence the pass can meet answers with its ADR-007 §7.1 code: `absent_revision`, `binary`,
//!   `language_unknown`, `too_large`, `parse_failed`;
//! * each of the three ceilings produces `unknown` + reason + a count, and — the property that makes a
//!   ceiling survivable rather than a silent hole — a refused row stays **owed**, so raising the ceiling and
//!   re-indexing fills it;
//! * the base side is read back from the parent's row, which is what makes §9.2's telescoping identity a
//!   property of the code instead of a reconciliation step;
//! * a warm run reads nothing it has already read, because the filled column *is* the memo; and
//! * the progress row is told when a ceiling stopped the pass, instead of printing `100 % · done` over a
//!   column of `?`s.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod common;

use common::Fixture;
use fathom_analyze::fill::Declined;
use fathom_analyze::index::{self, Phase, ProgressSink, Scope, Tick};
use fathom_analyze::structure::ExtractPolicy;
use fathom_analyze::structure::complexity::BASIS;
use fathom_ingest::record::Unclassified;
use fathom_ingest::repo::Repository;
use fathom_store::complexity::{BUDGET_REASON, FileComplexity, SIZE_REASON, SideWrite};
use fathom_store::spine::Store;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The instant every fixture run stamps with. Any fixed value works; this one is a Tuesday.
const NOW: i64 = 1_735_689_600_000;

/// A repository beside a store file, which is the pair every assertion below is about.
struct Run {
    fixture: Fixture,
    store: PathBuf,
}

impl Run {
    /// Index with the shipped ceilings.
    fn index(&self) -> index::Report {
        self.index_with(|_| {})
    }

    /// Index, overriding one of the `[extract.complexity]` ceilings.
    fn index_with(&self, policy: impl FnOnce(&mut ExtractPolicy)) -> index::Report {
        self.index_options(&self.options(policy))
    }

    fn index_options(&self, options: &index::Options) -> index::Report {
        let repository =
            Repository::discover(self.fixture.path()).expect("the fixture is a repository");
        let mut store = Store::open(&self.store).expect("the store opens");
        index::run(&repository, &mut store, &Unclassified, options).expect("the index run succeeds")
    }

    /// Options at the shipped ceilings plus an override, assembled the way the CLI assembles them.
    fn options(&self, policy: impl FnOnce(&mut ExtractPolicy)) -> index::Options {
        let mut extract = ExtractPolicy::default();
        policy(&mut extract);
        index::Options {
            now: Some(NOW),
            extract,
            ..index::Options::head()
        }
    }

    /// The stored complexity row for a path in the commit at `head`.
    fn row(&self, head: &str, path: &str) -> FileComplexity {
        let store = Store::open(&self.store).expect("reopen");
        let files = store.read_files(head).expect("the commit's file rows");
        let file = files
            .iter()
            .find(|file| file.new_path == path)
            .unwrap_or_else(|| panic!("no file row for {path} in {head}"));
        store
            .file_complexity(head, &file.path_hash)
            .expect("the read works")
            .unwrap_or_else(|| panic!("no complexity row written for {path}"))
    }

    /// The commit `back` steps from the tip, newest first.
    ///
    /// Asked of `git` rather than of the store, because `Store::all_oids` is an unordered set and the
    /// question here is about ancestry.
    fn nth_commit(&self, back: usize) -> String {
        let walk = git(self.fixture.path(), &["rev-list", "HEAD"]);
        walk.lines()
            .nth(back)
            .unwrap_or_else(|| panic!("the fixture has no commit {back} back"))
            .to_owned()
    }

    fn counters(&self) -> std::collections::BTreeMap<String, i64> {
        Store::open(&self.store)
            .expect("reopen")
            .counters("complexity.")
            .expect("counters read")
    }

    fn coverage(&self) -> fathom_store::complexity::ComplexityCoverage {
        Store::open(&self.store)
            .expect("reopen")
            .complexity_coverage()
            .expect("coverage")
    }
}

/// A fresh run pair: a repository with one countable Rust file, one path no grammar reads, and one binary.
fn run(label: &str) -> Run {
    let fixture = Fixture::new(label);
    fixture.commit(
        0,
        "add the shapes",
        &[
            (
                "src/shapes.rs",
                "pub fn area(w: u32, h: u32) -> u32 {\n    if w == 0 || h == 0 { 0 } else { w * h }\n}\n",
            ),
            ("notes/readme.md", "# notes\n"),
            ("assets/logo.bin", "\u{0}\u{0}\u{0}binary bytes\n"),
            ("data/config.json", "{\n  \"a\": 1\n}\n"),
        ],
    );
    let store = PathBuf::from(format!("{}.store", fixture.path().display()));
    std::fs::create_dir_all(&store).expect("create the sibling store directory");
    Run {
        fixture,
        store: store.join("index.sqlite3"),
    }
}

/// The same repository one commit later: the Rust file gains a branch.
fn grown(label: &str) -> Run {
    let run = run(label);
    run.fixture.edit(
        "src/shapes.rs",
        "pub fn area(w: u32, h: u32) -> u32 {\n    if w == 0 || h == 0 { 0 } else if w == h { w * w } else { w * h }\n}\n",
    );
    run
}

/// A `git` read for the test's own bookkeeping (the fixture's own `git` helper is private to it).
fn git(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A counted file carries a figure on both sides of a modify, and the head side's worst unit beside it.
///
/// This is the whole reason `e8.4` exists: `e8.3` built the columns and nothing wrote them, so every review
/// question about complexity had to answer `?`.
#[test]
fn a_countable_file_is_counted_on_both_sides() {
    let run = grown("fill-measured");
    let report = run.index();
    assert!(report.complexity.rows > 0, "the fill ran");
    assert_eq!(report.complexity.declined, None, "at the shipped ceilings");
    assert_eq!(
        report.complexity.missing, 0,
        "every write landed on a file row"
    );

    let head = run.fixture.head();
    let row = run.row(&head, "src/shapes.rs");
    assert_eq!(
        row.basis, BASIS,
        "every figure names the rules that counted it"
    );
    // `if w == 0 || h == 0 { … } else if w == h { … } else { … }`: two arms, three decision points, one base
    // body — and ADR-007 §4.1 counts `||` as a point of its own. The parent's version was one simpler.
    assert_eq!(
        row.after.figure(),
        Some(4),
        "the head side counts the branch this commit added: {}",
        row.after.printed()
    );
    assert_eq!(
        row.after.reason(),
        "",
        "an ordinary figure carries no reason"
    );
    assert_eq!(
        row.before.figure(),
        Some(3),
        "the base side counts what the parent had: {}",
        row.before.printed()
    );
    assert_eq!(
        row.max,
        Some(4),
        "the worst unit on the head side travels with it"
    );
}

/// An add has no base side and a delete no head side; both are measured zeros with §7.1's reason, never a
/// `?` — the distinction §9.4 exists for, because "0" is a complexity claim and "?" is not.
#[test]
fn a_side_that_does_not_exist_is_a_measured_zero() {
    let run = run("fill-absent-sides");
    run.fixture.edit("gone.rs", "fn gone() {}\n");
    run.fixture.remove("gone.rs");
    run.index();

    let head = run.fixture.head();
    let row = run.row(&head, "gone.rs");
    assert_eq!(
        row.after.reason(),
        "absent_revision",
        "a deleted path's head side is a zero with §7.1's reason, not an unknown"
    );
    assert_eq!(row.after.figure(), Some(0));
    assert_eq!(
        row.before.figure(),
        Some(1),
        "and its base side was counted"
    );
}

/// Git says binary, so nothing is inflated, and the code that says so is `binary` rather than `not_utf8` —
/// a different layer already answered this one, and §7.1 keeps the two apart for that reason.
#[test]
fn a_binary_row_answers_binary_without_reading_anything() {
    let run = run("fill-binary");
    let report = run.index();
    let head = run.fixture.head();
    let row = run.row(&head, "assets/logo.bin");
    // The base side answers `absent_revision` rather than `binary` because the path did not exist before the
    // commit that added it, and that is the cheaper fact to state — the cost order in `fill::Fill::side`
    // spends nothing on a side that is not there.
    assert_eq!(row.before.reason(), "absent_revision");
    assert_eq!(row.after.reason(), "binary");
    assert_eq!(
        row.after.figure(),
        None,
        "and there is no figure to mistake it for"
    );
    // The refusal is cheap by construction, and the byte ledger is how that is checked rather than asserted:
    // everything the pass inflated must be inside the one file this fixture can count. A run that opened the
    // binary, the Markdown or the JSON to find out would show up here.
    let counted_file =
        std::fs::read(run.fixture.path().join("src/shapes.rs")).expect("the fixture file");
    assert_eq!(
        report.complexity.bytes_read,
        u64::try_from(counted_file.len()).expect("a usize fits a u64"),
        "the only content this run inflated was the file it could count: {:?}",
        report.complexity
    );
}

/// A path no grammar reads is refused on its *name*, before the object database is asked for it — so an
/// unsupported language costs nothing, which is the property the `…langs-all` feature list is priced in.
#[test]
fn an_unsupported_language_is_refused_before_it_is_read() {
    let run = run("fill-language");
    let report = run.index_with(|policy| {
        // A ceiling small enough that one *unnecessary* read would trip it and name itself in the ledger:
        // if the pass opened `notes/readme.md` to discover it cannot parse Markdown, this run would report
        // a spent budget rather than a refusal it paid nothing for.
        policy.complexity_max_run_bytes = 64;
    });
    let head = run.fixture.head();
    assert_eq!(
        run.row(&head, "notes/readme.md").after.reason(),
        "language_unknown"
    );
    assert_eq!(
        run.row(&head, "data/config.json").after.reason(),
        "language_unknown"
    );
    assert_eq!(
        report.complexity.declined,
        None,
        "nothing was refused *by a budget*: the languages were refused first — {}",
        report.complexity.line()
    );
}

/// The per-file ceiling is decided from the object header, so the outcome is `too_large` and the cost of a
/// file over the cap is a header read rather than an inflation.
#[test]
fn a_file_over_the_per_file_ceiling_is_too_large() {
    let run = grown("fill-too-large");
    run.index_with(|policy| {
        policy.complexity_max_file_bytes = 1;
    });
    let head = run.fixture.head();
    let row = run.row(&head, "src/shapes.rs");
    assert_eq!(
        row.after.reason(),
        SIZE_REASON,
        "the per-file ceiling answers `too_large`, naming itself and not a run budget"
    );
    assert_eq!(row.after.figure(), None);
}

/// A spent per-run byte budget: `?` with `scan_budget`, the refusal counted, and the row left **owed** —
/// then the ceiling is raised, and the same store answers. This is the difference between a ceiling and a
/// hole in the index.
#[test]
fn a_spent_byte_budget_refuses_the_rest_and_leaves_it_owed() {
    let run = grown("fill-bytes");
    let first = run.index_with(|policy| {
        policy.complexity_max_run_bytes = 1;
    });
    assert_eq!(first.complexity.declined, Some(Declined::Bytes));
    assert!(
        first.complexity.refused.contains_key(BUDGET_REASON),
        "the refusal is counted: {:?}",
        first.complexity.refused
    );

    let head = run.fixture.head();
    let refused = run.row(&head, "src/shapes.rs");
    assert_eq!(refused.after.reason(), BUDGET_REASON);
    assert_eq!(refused.after.figure(), None);
    // The base side was still answered, because answering it cost a store probe and no read: a budget that
    // stopped the *reads* does not stop the pass from using what the store already knows.
    assert_eq!(
        refused.before.figure(),
        Some(3),
        "the side the store could still answer was answered from the parent's row: {}",
        refused.before.printed()
    );
    assert!(
        run.coverage()
            .reasons
            .iter()
            .any(|(code, _)| code == BUDGET_REASON),
        "and the coverage roll-up sees it: {:?}",
        run.coverage().reasons
    );

    // Same store, same rows, one number raised. This is the entire argument for a budget refusal being
    // re-openable: a `?` cached as if it were a fact would have answered the same way forever, and ADR-007
    // §9.6's "the series of budgeted runs converges" would have been a sentence with nothing behind it.
    let second = run.index_with(|policy| {
        policy.complexity_max_run_bytes = 1_000_000;
    });
    assert_eq!(second.complexity.declined, None);
    let mut options = run.options(|policy| policy.complexity_max_run_bytes = 1_000_000);
    options.scope = Scope::Everything;
    let third = run.index_options(&options);
    assert_eq!(third.complexity.declined, None);
    let filled = run.row(&head, "src/shapes.rs");
    assert_eq!(
        filled.after.figure(),
        Some(4),
        "the row a budget refused is measurable once the budget is raised: {:?}",
        filled.after.printed()
    );
    assert_eq!(filled.basis, BASIS);
}

/// A spent time budget names the *time* key, because its remedy is a different number — the ceiling's
/// config name is part of what the operator is told, not an implementation detail.
#[test]
fn a_spent_time_budget_names_the_time_key() {
    let run = grown("fill-millis");
    let report = run.index_with(|policy| {
        policy.complexity_max_run_ms = 0;
    });
    assert_eq!(report.complexity.declined, Some(Declined::Millis));
    assert_eq!(
        report.complexity.declined.map(Declined::key),
        Some("extract.complexity.max_run_ms"),
        "the line names the key the operator has to raise"
    );
    let head = run.fixture.head();
    assert_eq!(
        run.row(&head, "src/shapes.rs").after.reason(),
        BUDGET_REASON
    );
    assert!(
        report.complexity.line().contains("max_run_ms"),
        "{}",
        report.complexity.line()
    );
}

/// The base side is read back from the parent's stored row rather than recomputed, and the two columns
/// agree because they are the same read — §9.2's telescoping identity, held by the only order of operations
/// that can hold it.
#[test]
fn the_base_side_is_borrowed_from_the_parent_row() {
    let run = grown("fill-borrowed");
    let report = run.index();
    assert!(
        report.complexity.base_reused > 0,
        "the second commit's base side came from the first one's head side: {:?}",
        report.complexity
    );
    let head = run.fixture.head();
    let parent = run.nth_commit(1);
    let second = run.row(&head, "src/shapes.rs");
    let first = run.row(&parent, "src/shapes.rs");
    assert_eq!(
        second.before.figure(),
        first.after.figure(),
        "the two sides are the same number because they are the same read"
    );
    assert_eq!(
        first.before.reason(),
        "absent_revision",
        "the first commit's base side has no parent to borrow from: {}",
        first.before.printed()
    );
}

/// A rename measures its base side from the blob the spine recorded, and borrows nothing: the parent's
/// figure for a renamed path lives under the *old* path hash, which this row does not carry, so a borrow
/// would put one path's complexity on another path's row.
#[test]
fn a_renamed_path_borrows_nothing() {
    let run = run("fill-rename");
    run.fixture.rename("src/shapes.rs", "src/geometry.rs");
    let report = run.index();
    let head = run.fixture.head();
    let row = run.row(&head, "src/geometry.rs");
    assert_eq!(
        report.complexity.base_reused, 0,
        "nothing was borrowed on a run whose only commit was a rename: {:?}",
        report.complexity
    );
    assert!(
        row.before.measured() && row.after.measured(),
        "both sides are counted, the base side from the pre-rename blob: {} {}",
        row.before.printed(),
        row.after.printed()
    );
    assert_eq!(
        row.before.figure(),
        row.after.figure(),
        "and they agree, because a rename moved the file without changing it"
    );
}

/// A warm run reads what is new and nothing else: the filled column *is* the memo, which is what makes the
/// pass affordable on a watcher rather than only on a first index.
#[test]
fn a_warm_run_reads_only_what_is_new() {
    let run = run("fill-warm");
    let cold = run.index();
    let cold_bytes = cold.complexity.bytes_read;
    assert!(cold_bytes > 0, "the cold run read the file it could count");

    run.fixture.edit(
        "src/shapes.rs",
        "pub fn area(w: u32, h: u32) -> u32 {\n    if w == 0 || h == 0 { 0 } else if w > h { w } else { h }\n}\n",
    );
    let warm = run.index();
    assert_eq!(warm.commits_new, 1, "one commit of work to do");
    assert_eq!(
        warm.complexity.rows, 1,
        "and one file row to read, not the whole history again: {:?}",
        warm.complexity
    );
    assert!(
        warm.complexity.bytes_read < cold_bytes * 2,
        "one small file: {:?}",
        warm.complexity
    );

    // And a run with nothing new reads nothing at all.
    let again = run.index();
    assert!(
        again.quiet || again.complexity.rows == 0,
        "{:?}",
        again.complexity
    );
    assert_eq!(again.complexity.bytes_read, 0);
}

/// A `--refresh` over a filled store is a probe, not a re-parse: it asks the owed question, is told nothing
/// is owed, and inflates nothing.
#[test]
fn a_refresh_re_reads_only_the_rows_that_owe_a_figure() {
    let run = grown("fill-refresh");
    run.index();
    let mut options = run.options(|_| {});
    options.scope = Scope::Everything;
    let refresh = run.index_options(&options);
    assert_eq!(
        refresh.complexity.rows, 0,
        "a filled store owes nothing, so a refresh reads nothing: {:?}",
        refresh.complexity
    );
    assert_eq!(refresh.complexity.bytes_read, 0);
}

/// A file the parse refused is `parse_failed`, and a file that parses to nothing countable is `no_bodies`:
/// two sentences, because only one of them is fixed by editing the file.
#[test]
fn an_unparseable_file_says_which_half_failed() {
    let run = run("fill-parse");
    run.fixture.edit("src/broken.rs", "fn broken( {\n");
    run.index();
    let head = run.fixture.head();
    let broken = run.row(&head, "src/broken.rs");
    assert_eq!(
        broken.after.figure(),
        None,
        "a file the parse refused has no figure, never a `0` that reads as \"simple\""
    );
    assert!(
        matches!(broken.after.reason(), "parse_failed" | "no_bodies"),
        "and it names which failure this was: {}",
        broken.after.reason()
    );
}

/// The progress row is told. A run whose pass was stopped by a ceiling must not reach `100 % · done` without
/// saying what it did not do — ADR-006 §10.3's row is the only thing most operators ever read.
#[test]
fn the_progress_row_is_told_when_a_ceiling_stops_the_pass() {
    let run = grown("fill-progress");
    let ticks: Arc<Mutex<Vec<Tick>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = Arc::clone(&ticks);
    let mut options = run.options(|policy| {
        policy.complexity_max_run_bytes = 1;
    });
    options.progress = Some(ProgressSink::new(move |tick: &Tick| {
        collector.lock().expect("lock").push(*tick);
    }));
    let report = run.index_options(&options);
    assert_eq!(report.complexity.declined, Some(Declined::Bytes));

    let seen = ticks.lock().expect("lock").clone();
    assert!(
        seen.iter().any(|tick| tick.note.is_some()),
        "a tick carried the note the row appends: {seen:?}"
    );
    assert!(
        seen.iter()
            .filter(|tick| tick.note.is_some())
            .all(|tick| tick.phase == Phase::Analysing),
        "the note belongs to the phase it describes: {seen:?}"
    );
    let last_analysing = seen
        .iter()
        .rev()
        .find(|tick| tick.phase == Phase::Analysing)
        .expect("the analyses phase reported");
    assert!(
        last_analysing.note.is_some(),
        "the tick that reaches 100 % is the tick that has to say what was declined: {last_analysing:?}"
    );
    assert!(
        last_analysing.note.expect("noted").contains("budget"),
        "and it says so in words a log line keeps: {:?}",
        last_analysing.note
    );
}

/// The counters land, because `fathom doctor` answers "has this store ever run out of complexity budget?"
/// from the store and not from a run report that died with its process.
#[test]
fn the_run_counts_itself_into_the_store() {
    let run = grown("fill-counters");
    run.index_with(|policy| {
        policy.complexity_max_run_bytes = 1;
    });
    let counters = run.counters();
    let names: Vec<&String> = counters.keys().collect();
    assert!(
        counters.contains_key("complexity.rows_measured"),
        "{names:?}"
    );
    assert!(
        counters.contains_key(&format!("complexity.refused.{BUDGET_REASON}")),
        "a refusal is counted in the store as well as in the column: {names:?}"
    );
    assert!(
        counters.contains_key("complexity.budget_spent.extract.complexity.max_run_bytes"),
        "and so is the fact that a ceiling stopped the pass: {names:?}"
    );
}

/// A figure counted under another basis token is owed again: §9.5's version bump is a fill condition, not
/// a migration, and the fill is what a basis bump leaves behind for.
#[test]
fn a_figure_counted_under_another_basis_is_owed_again() {
    let run = grown("fill-basis");
    run.index();
    let head = run.fixture.head();

    // Stamp every row back to an older token — the state a basis bump produces, written through the store's
    // own door so the test cannot invent a row shape production cannot produce.
    {
        let store = Store::open(&run.store).expect("reopen");
        let files = store.read_files(&head).expect("file rows");
        drop(store);
        let mut store = Store::open(&run.store).expect("reopen again");
        let writes: Vec<_> = files
            .iter()
            .map(|file| fathom_store::complexity::ComplexityWrite {
                commit: &head,
                path_hash: &file.path_hash,
                basis: "mccabe-0",
                before: SideWrite::Measured {
                    figure: 7,
                    reason: "",
                },
                after: SideWrite::Measured {
                    figure: 7,
                    reason: "",
                },
                max: Some(7),
            })
            .collect();
        store
            .record_complexity(&writes)
            .expect("the stamp goes through the store's own CHECK");
    }
    assert_eq!(
        run.row(&head, "src/shapes.rs").basis,
        "mccabe-0",
        "the stale figure is what a reader would see now"
    );

    let mut options = run.options(|_| {});
    options.scope = Scope::Everything;
    let refresh = run.index_options(&options);
    assert!(
        refresh.complexity.rows > 0,
        "a stale basis is owed work: {:?}",
        refresh.complexity
    );
    let row = run.row(&head, "src/shapes.rs");
    assert_eq!(
        row.basis, BASIS,
        "and the fill re-counted it under this build's token"
    );
    assert_eq!(
        row.after.figure(),
        Some(4),
        "with this build's figures, not the stamp's 7"
    );
}

// --- ceilings are stated, not implied -----------------------------------------------

#[test]
fn the_ledger_reports_the_ceilings_it_obeyed() {
    // A reader of `fathom index --json` decides from these three numbers whether the run was limited and
    // whether raising a key would buy anything, so the JSON must carry the values the pass compared against
    // — not the defaults it could have looked up, and not the text line's rounding of them.
    let run = run("ledger-ceilings");
    let report = run.index_with(|policy| policy.complexity_max_run_ms = 7_500);
    let json = report.complexity.to_json();
    assert_eq!(json["ceilings"]["max_run_ms"], 7_500);
    assert_eq!(json["ceilings"]["max_run_bytes"], 128_000_000);
    assert_eq!(json["ceilings"]["max_file_bytes"], 2_000_000);
    assert!(
        report.complexity.declined.is_none(),
        "a 7.5 s budget is not spent by four small files: {:?}",
        report.complexity
    );
}

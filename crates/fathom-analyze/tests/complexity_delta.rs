//! Complexity as a primitive of the delta document (`e8.5`, `reco-l9bz.5`).
//!
//! [ADR-007 §5.4](../../../../docs/adr/0007-complexity.md) names the grain — *this change* — and insists
//! the figure "enters the single `EpisodeDelta` model, not a card-only calculation", because four consumers
//! that each sum the same columns are four consumers that will disagree. So everything here is about the
//! one field and the honesty rules attached to it:
//!
//! | Criterion | Test |
//! |---|---|
//! | the figure sits in `what`, beside the counters it qualifies, carrying delta, absolute, coverage and codes | `a_measured_roll_up_prints_one_figure_beside_the_line_counts` |
//! | every figure is built by the evidence-carrying constructor and `verify()` passes with it | every test composes through [`verified`], and `integrity` carries a check of its own (`CHECKS`' last line) |
//! | a partial figure says what it is a floor of | `a_partial_roll_up_prints_what_it_left_out` |
//! | the unmeasurable case is `measured: false` with a reason, an owner and a gap that fits the card | `nothing_measurable_is_an_unknown_with_a_reason_and_an_owner_never_a_zero`, `every_reason_fits_the_card_with_its_owner` |
//! | the same figure on every surface, so no consumer derives its own | the store section: the bridge's figure, the block and `provenance.models` agree |
//! | a roll-up that was never read is absent, not zero | `a_read_that_made_no_roll_up_prints_no_figure` |
//!
//! The store half runs a real index, because the difference between "the analyzer could not" and "the
//! index has not filled the column yet" is a fact about the store and cannot be shown from a struct.

#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod common;

use common::Fixture;
use fathom_analyze::index;
use fathom_analyze::insight::{
    self, CARD_COLUMNS, ComplexityView, EpisodeDelta, EpisodeRef, FileDelta, Gap, Input,
    REFUSAL_CODES, SectionId, Totals,
};
use fathom_analyze::structure::UnavailableReason;
use fathom_ingest::record::Unclassified;
use fathom_ingest::repo::Repository;
use fathom_store::episode::EpisodeQuery;
use fathom_store::spine::Store;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The instant every fixture run stamps with. Any fixed value works; this one is a Tuesday.
const NOW: i64 = 1_735_689_600_000;

/// The `raw` keys a *measured* complexity figure carries. Asserted as a set because the ticket names the
/// list — delta, absolute, coverage, codes — and a figure that drops one leaves its consumer guessing.
const MEASURED_RAW: &[&str] = &[
    "absolute",
    "coverage_measured",
    "coverage_total",
    "delta",
    "max",
    "measured",
    "unmeasurable",
];

/// Compose, and fail loudly when the document does not pass its own [`EpisodeDelta::verify`] — which is
/// the ticket's "every figure is built by the constructor that carries its evidence", checked where the
/// figures are built instead of trusted.
fn verified(delta: &EpisodeDelta) {
    let problems = delta.verify();
    assert!(
        problems.is_empty(),
        "the document fails its own integrity check: {problems:?}\n{}",
        delta.to_json_pretty()
    );
}

/// The figure named `complexity`, wherever the sections put it.
fn figure<'a>(delta: &'a EpisodeDelta, label: &str) -> Option<&'a insight::Figure> {
    delta
        .section(SectionId::What)
        .expect("`what` is always present")
        .figures
        .iter()
        .find(|figure| figure.label == label)
}

/// The `what` section's figure labels, in the order a card prints them.
fn labels(delta: &EpisodeDelta) -> Vec<String> {
    delta
        .section(SectionId::What)
        .expect("`what` is always present")
        .figures
        .iter()
        .map(|figure| figure.label.clone())
        .collect()
}

/// A two-path episode with the line counters every test below puts its figure beside.
fn input() -> Input {
    let mut input = Input::new(EpisodeRef::sparse("ep-1", "widen the shapes"));
    input.files = vec![
        FileDelta {
            path: "src/shapes.rs".to_string(),
            old_path: None,
            change: "modified".to_string(),
            add: 40,
            del: 5,
            binary: false,
            commits: 1,
            path_class: "source".to_string(),
            noise_class: "content".to_string(),
            surface: "code".to_string(),
            basis: "full".to_string(),
        },
        FileDelta {
            path: "assets/logo.bin".to_string(),
            old_path: None,
            change: "modified".to_string(),
            add: 0,
            del: 0,
            binary: true,
            commits: 1,
            path_class: "other".to_string(),
            noise_class: "generated".to_string(),
            surface: "other".to_string(),
            basis: "full".to_string(),
        },
    ];
    input.totals = Totals {
        n_files: 2,
        n_add: 40,
        n_del: 5,
        n_binary: 1,
        files_read: 2,
        ..Totals::default()
    };
    input
}

/// A fully countable roll-up: one path, six lines of branch added, nothing refused.
fn measured() -> ComplexityView {
    ComplexityView {
        basis: vec!["mccabe-1".to_string()],
        delta: Some(6),
        absolute: Some(9),
        max: Some(5),
        touched: 1,
        measured_paths: 1,
        unmeasurable_paths: 0,
        reasons: BTreeMap::new(),
        measured: false,
        state: String::new(),
        coverage: None,
        gap: None,
    }
}

/// The same episode with nothing countable — the shape ADR-007 §5.4 refuses to print as `0`.
fn unmeasurable(code: &str, paths: i64) -> ComplexityView {
    let mut reasons = BTreeMap::new();
    reasons.insert(code.to_string(), paths);
    ComplexityView {
        basis: Vec::new(),
        delta: None,
        absolute: None,
        max: None,
        touched: paths,
        measured_paths: 0,
        unmeasurable_paths: paths,
        reasons,
        measured: false,
        state: String::new(),
        coverage: None,
        gap: None,
    }
}

// ---------------------------------------------------------------------------
// The figure, beside the counters it qualifies
// ---------------------------------------------------------------------------

#[test]
fn a_measured_roll_up_prints_one_figure_beside_the_line_counts() {
    let mut input = input();
    input.complexity = Some(measured());
    let delta = insight::compose(&input);
    verified(&delta);

    let figure = figure(&delta, "complexity").expect("`what` carries a complexity figure");
    assert!(figure.known);
    assert_eq!(figure.value, "+6", "ADR-007 §10.1's signed form");
    // Evidence, or it is a recommendation (ADR-001 §7.9): the pointer resolves inside the document, which
    // is what lets `verify` check it and the export link it.
    assert_eq!(figure.evidence.pointer.as_deref(), Some("#/complexity"));
    let keys: Vec<&str> = figure.raw.keys().map(String::as_str).collect();
    assert_eq!(
        keys, MEASURED_RAW,
        "the §5.2 keys, named rather than assumed"
    );
    assert_eq!(figure.raw["delta"], 6);
    assert_eq!(figure.raw["absolute"], 9);
    // Coverage travels as a pair so a reader can see the denominator a percentage would have hidden.
    assert_eq!(figure.raw["coverage_measured"], 1);
    assert_eq!(figure.raw["coverage_total"], 1);
    assert_eq!(figure.raw["unmeasurable"], 0);

    let block = delta
        .complexity
        .as_ref()
        .expect("the block travels with the figure");
    assert_eq!(block.delta, Some(6));
    assert_eq!(block.absolute, Some(9));
    assert_eq!(block.max, Some(5));
    assert!(block.measured);
    assert_eq!(block.state, insight::STATE_MEASURED);
    assert_eq!(block.coverage, Some(1.0));
    assert!(block.gap.is_none(), "there is no silence to explain");

    // Beside the churn it qualifies, and nothing the card needs was pushed out of the section.
    let labels = labels(&delta);
    assert!(
        labels
            .windows(2)
            .any(|pair| pair == ["reviewable churn", "complexity"]),
        "complexity belongs beside the churn it qualifies: {labels:?}"
    );
    for keep in ["lines", "files", "churn", "reviewable churn"] {
        assert!(
            labels.contains(&keep.to_string()),
            "{keep} was pushed out: {labels:?}"
        );
    }
}

#[test]
fn a_partial_roll_up_prints_what_it_left_out() {
    let mut input = input();
    input.complexity = Some(ComplexityView {
        basis: vec!["mccabe-1".to_string()],
        delta: Some(6),
        absolute: Some(9),
        max: Some(5),
        touched: 5,
        measured_paths: 2,
        unmeasurable_paths: 3,
        reasons: BTreeMap::from([("language_unknown".to_string(), 3)]),
        ..ComplexityView::default()
    });
    let delta = insight::compose(&input);
    verified(&delta);

    let figure = figure(&delta, "complexity").expect("the figure");
    assert!(
        figure.known,
        "two paths were counted, so the figure is a number"
    );
    assert_eq!(
        figure.value, "~+6 (2 of 5 measured · 3 unmeasured: language_unknown 3)",
        "the floor names its own denominator and what is not in it"
    );
    assert_eq!(figure.raw["coverage_measured"], 2);
    assert_eq!(figure.raw["coverage_total"], 5);

    let block = delta.complexity.as_ref().expect("the block");
    assert_eq!(block.state, insight::STATE_PARTIAL);
    assert_eq!(block.coverage, Some(0.4));
    assert_eq!(
        block.reasons,
        BTreeMap::from([("language_unknown".to_string(), 3)])
    );
    // A floor still prints, so there is no gap: a gap is for the case where the figure is not a number.
    assert!(block.gap.is_none());
    assert!(
        !delta
            .gaps
            .iter()
            .any(|gap| gap.short.contains("complexity")),
        "a partial figure is a floor that prints, not a section that failed: {:?}",
        delta.gaps
    );
}

#[test]
fn nothing_measurable_is_an_unknown_with_a_reason_and_an_owner_never_a_zero() {
    let mut input = input();
    input.complexity = Some(unmeasurable("column_not_filled", 5));
    let delta = insight::compose(&input);
    verified(&delta);

    // The row stays — the question was asked — but it prints `?` with its reason, never `0`.
    let figure = figure(&delta, "complexity").expect("the figure is still there");
    assert!(!figure.known);
    assert_eq!(
        figure.value, "",
        "an unknown figure carries no number to mistake for zero"
    );
    let reason = figure
        .reason
        .as_deref()
        .expect("an unknown without a reason is a blank");
    assert!(
        reason.contains("column_not_filled") && reason.contains("5 of 5"),
        "the reason is a §7.1 code with its count: {reason}"
    );
    assert!(
        reason.contains("fathom index"),
        "a shrug is not a remedy (ADR-006 §10.3): {reason}"
    );
    assert!(
        figure.raw.is_empty(),
        "an unknown figure has no integers to compare: {:?}",
        figure.raw
    );

    let block = delta.complexity.as_ref().expect("the block");
    assert!(!block.measured, "the ticket's `measured = false` section");
    assert_eq!(block.state, insight::STATE_UNAVAILABLE);
    assert!(
        block.delta.is_none(),
        "no path could be subtracted, so there is no delta"
    );
    assert_eq!(
        block.coverage,
        Some(0.0),
        "0 of 5 is a coverage, not a missing one"
    );

    let json = serde_json::to_value(&delta).expect("the document serializes");
    assert_eq!(json["complexity"]["measured"], false);
    assert!(
        json["complexity"].get("delta").is_none(),
        "a `measured: false` block wearing `delta: 0` is the exact lie §5.4 forbids: {json}"
    );

    // The gap: one line, filed against the section the figure sits in, an owner that is a plan key, and
    // short enough that the card can print it with the owner attached.
    let gap = block
        .gap
        .clone()
        .expect("an unmeasured figure carries its gap");
    assert_eq!(gap.section, SectionId::What.as_str());
    assert_eq!(
        gap.owner, "e8.3",
        "the column that is not filled is `e8.3`'s answer"
    );
    assert_eq!(gap.remedy.as_deref(), Some("fathom index"));
    assert!(
        printed_gap_columns(&gap) <= CARD_COLUMNS,
        "`{} ({})` is {} columns, over the card's {CARD_COLUMNS}",
        gap.short,
        gap.owner,
        printed_gap_columns(&gap)
    );
    assert!(
        gap.short.len() < gap.reason.len(),
        "the one-line form and the full form are two forms: {:?} / {:?}",
        gap.short,
        gap.reason
    );
}

/// How wide the card's gap line is: the short form, and the plan key it blames, in one row.
fn printed_gap_columns(gap: &Gap) -> usize {
    format!("{} ({})", gap.short, gap.owner).chars().count()
}

#[test]
fn every_reason_fits_the_card_with_its_owner() {
    // The gap line is generated from the dominant code, so *every* code in the shared vocabulary owes a
    // card-sized sentence — not just the common one. The table in `delta.rs` is checked against the
    // vocabulary itself, so a code cannot arrive without a line of its own.
    let table: Vec<&str> = {
        let mut codes = REFUSAL_CODES.to_vec();
        codes.sort_unstable();
        codes
    };
    let vocabulary: Vec<&str> = {
        let all = UnavailableReason::vocabulary();
        let mut codes: Vec<&str> = all.iter().map(|reason| reason.as_str()).collect();
        codes.sort_unstable();
        codes
    };
    assert_eq!(
        table, vocabulary,
        "the refusal table and ADR-007 §7.1's vocabulary must hold the same codes"
    );

    for code in REFUSAL_CODES {
        let mut input = input();
        input.complexity = Some(unmeasurable(code, 1_234));
        let delta = insight::compose(&input);
        verified(&delta);

        let gap = delta
            .complexity
            .as_ref()
            .and_then(|block| block.gap.clone())
            .unwrap_or_else(|| panic!("`{code}` produced no gap"));
        let line = format!("{} ({})", gap.short, gap.owner);
        assert!(
            line.chars().count() <= CARD_COLUMNS,
            "`{code}` prints {line:?} — {} columns, over the card's {CARD_COLUMNS}",
            line.chars().count()
        );
        assert!(
            gap.reason.contains(code),
            "`{code}` printed a reason that does not name it: {:?}",
            gap.reason
        );
        assert!(
            gap.owner.starts_with('e') && gap.owner.contains('.'),
            "`{code}` blames {:?}, which is not a plan key",
            gap.owner
        );
    }
}

#[test]
fn a_read_that_made_no_roll_up_prints_no_figure() {
    // The other half of §5.4's distinction: a read that made no roll-up is not the same claim as a
    // roll-up that measured nothing, and it must not borrow the `?` row.
    let delta = insight::compose(&input());
    verified(&delta);
    assert!(
        figure(&delta, "complexity").is_none(),
        "an absent roll-up is an absent row, never a `0` and never a `?`"
    );
    assert!(
        serde_json::to_value(&delta)
            .expect("the document serializes")
            .get("complexity")
            .is_none(),
        "the key is absent when the read made no roll-up"
    );
    assert!(
        !delta
            .gaps
            .iter()
            .any(|gap| gap.short.contains("complexity")),
        "an unasked question is nobody's gap: {:?}",
        delta.gaps
    );
}

#[test]
fn the_basis_is_named_where_a_mixed_basis_shows_itself() {
    // ADR-007 §9.5: an episode whose paths were counted under two bases has no defensible sum, and the
    // place a reader finds that out is the place the sum is printed.
    let mut input = input();
    input.complexity = Some(ComplexityView {
        basis: vec!["cc-shannon-1".to_string(), "mccabe-1".to_string()],
        delta: Some(6),
        absolute: Some(9),
        max: Some(5),
        touched: 2,
        measured_paths: 2,
        unmeasurable_paths: 0,
        ..ComplexityView::default()
    });
    let delta = insight::compose(&input);
    verified(&delta);

    assert_eq!(
        delta
            .provenance
            .models
            .get("complexity")
            .expect("the model map names it"),
        "structure::complexity (cc-shannon-1+mccabe-1)",
        "two basis tokens in one episode, stated where the sum is"
    );
    assert_eq!(
        delta.complexity.as_ref().expect("block").state,
        insight::STATE_PARTIAL,
        "one sum over two rule sets is a partial answer, not a measured one (ADR-007 §9.5)"
    );
    let value = &figure(&delta, "complexity").expect("figure").value;
    assert!(
        value.starts_with('~') && value.contains("cc-shannon-1") && value.contains("mccabe-1"),
        "the sum carries its own warning: {value}"
    );
}

// ---------------------------------------------------------------------------
// What `integrity` refuses
// ---------------------------------------------------------------------------

/// Find the `what` section mutably.
fn what_mut(delta: &mut EpisodeDelta) -> &mut insight::Section {
    delta
        .sections
        .iter_mut()
        .find(|section| section.id == SectionId::What.as_str())
        .expect("`what` is always present")
}

#[test]
fn integrity_refuses_a_figure_that_disagrees_with_the_block() {
    let mut input = input();
    input.complexity = Some(measured());
    let mut delta = insight::compose(&input);
    verified(&delta);

    // A surface that "reformats" the figure by writing its own number is the failure ADR-001 §7.10 names.
    what_mut(&mut delta)
        .figures
        .iter_mut()
        .find(|figure| figure.label == "complexity")
        .expect("the figure")
        .value = "+99".to_string();

    let problems = delta.verify();
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("complexity")
                && problem.contains("formatted the number")),
        "a figure that disagrees with its block has to say so: {problems:?}"
    );
}

#[test]
fn integrity_refuses_an_unmeasured_figure_wearing_a_zero() {
    // Hand-built on purpose: `compose` cannot produce this, and `verify` has to refuse it anyway, because
    // the documents a consumer reads were composed by some other build of this crate.
    let mut input = input();
    input.complexity = Some(measured());
    let mut delta = insight::compose(&input);
    delta.complexity.as_mut().expect("the block").measured = false;

    let problems = delta.verify();
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("the figure says a measurement")),
        "a printed number the roll-up disowns is the lie the check exists for: {problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("silent zero")),
        "and it is also a gap with nothing in it: {problems:?}"
    );
}

#[test]
fn integrity_refuses_a_gap_line_too_long_for_the_card() {
    let mut input = input();
    input.complexity = Some(unmeasurable("column_not_filled", 5));
    let mut delta = insight::compose(&input);
    delta
        .complexity
        .as_mut()
        .and_then(|block| block.gap.as_mut())
        .expect("the gap")
        .short = "complexity: not measured, and here is a second thought".to_string();

    let problems = delta.verify();
    assert!(
        problems.iter().any(|problem| problem.contains("columns")),
        "an over-long gap line is a row the card will cut: {problems:?}"
    );
}

#[test]
fn integrity_refuses_a_figure_with_no_block_and_vice_versa() {
    // The pair is the contract: either half alone is a surface inventing a figure or a roll-up nobody was
    // told about.
    let mut input = input();
    input.complexity = Some(measured());
    let mut delta = insight::compose(&input);
    what_mut(&mut delta)
        .figures
        .retain(|figure| figure.label != "complexity");
    assert!(
        delta
            .verify()
            .iter()
            .any(|problem| problem.contains("no `complexity` figure")),
        "a block with no figure is a roll-up no surface prints"
    );

    let mut other = insight::compose(&input);
    other.complexity = None;
    assert!(
        other
            .verify()
            .iter()
            .any(|problem| problem.contains("with no `complexity` block")),
        "a figure with no block is a number whose evidence is nowhere in the document"
    );
}

// ---------------------------------------------------------------------------
// The bridge: a real index, and a store that has not filled the column
// ---------------------------------------------------------------------------

/// A repository whose second commit adds a branch — the smallest change that moves a complexity figure —
/// with its store beside it rather than inside it (`git add -A` must not commit the index).
struct Repo {
    fixture: Fixture,
    store: PathBuf,
}

fn repo(label: &str) -> Repo {
    let fixture = Fixture::new(label);
    fixture.commit(
        0,
        "add the shapes",
        &[(
            "src/shapes.rs",
            "pub fn area(w: u32, h: u32) -> u32 {\n    if w == 0 || h == 0 { 0 } else { w * h }\n}\n",
        )],
    );
    let store = PathBuf::from(format!("{}.store", fixture.path().display()));
    std::fs::create_dir_all(&store).expect("create the sibling store directory");
    Repo {
        fixture,
        store: store.join("index.sqlite3"),
    }
}

impl Repo {
    /// The commit that makes the file harder to follow: one more branch, so one more path through it.
    fn grow(&self) {
        self.fixture.commit(
            1,
            "widen the shapes",
            &[(
                "src/shapes.rs",
                "pub fn area(w: u32, h: u32) -> u32 {\n    if w == 0 || h == 0 { 0 }\n    else if w == h { w * w }\n    else { w * h }\n}\n",
            )],
        );
    }

    /// The run `fathom index` performs, ceilings and all.
    fn index(&self) {
        let repository =
            Repository::discover(self.fixture.path()).expect("the fixture is a repository");
        let mut store = Store::open(&self.store).expect("the store opens");
        index::run(
            &repository,
            &mut store,
            &Unclassified,
            &index::Options {
                now: Some(NOW),
                ..index::Options::head()
            },
        )
        .expect("the index run succeeds");
    }

    /// Drop the complexity columns, which is the state a store an older fathom wrote is in.
    fn clear_complexity(&self) {
        let store = Store::open(&self.store).expect("reopen");
        store
            .connection()
            .execute(
                "UPDATE git_commit_file
                 SET complexity_basis = '', complexity_before = NULL, complexity_after = NULL,
                     complexity_before_reason = '', complexity_after_reason = '', complexity_max = NULL",
                [],
            )
            .expect("clear the complexity columns");
    }

    /// The delta the surfaces read for the episode that grew the file.
    fn delta(&self) -> EpisodeDelta {
        let store = Store::open(&self.store).expect("reopen");
        let mut episodes = store
            .episodes(&EpisodeQuery::default())
            .expect("episodes read");
        episodes.reverse();
        let episode = episodes
            .iter()
            .find(|row| row.summary.contains("widen the shapes"))
            .or_else(|| episodes.first())
            .expect("the run segmented at least one episode");
        insight::for_episode(&store, &episode.id)
            .expect("the delta builds")
            .expect("the episode is in the store")
    }
}

#[test]
fn a_real_index_makes_the_figure_measured_and_self_consistent() {
    let repo = repo("measured");
    repo.grow();
    repo.index();
    let delta = repo.delta();
    verified(&delta);

    let block = delta
        .complexity
        .as_ref()
        .expect("a read over a filled store makes a roll-up");
    let figure = figure(&delta, "complexity").expect("and prints it in `what`");
    assert!(
        block.measured,
        "the index filled the column, so the figure is a number: {block:?}"
    );
    assert!(
        figure.value.starts_with('+'),
        "an added branch is complexity added: {:?}",
        figure.value
    );
    assert_eq!(
        delta
            .provenance
            .models
            .get("complexity")
            .expect("named in the model map"),
        "structure::complexity (mccabe-1)",
        "the basis is printed where the sum is, on every surface that prints the model map"
    );
    // The reason tally accounts for the coverage exactly: one path, one code, none invented.
    let total: i64 = block.reasons.values().sum();
    assert_eq!(
        total, block.unmeasurable_paths,
        "reasons are counted once per path"
    );
    assert_eq!(
        block.measured_paths + block.unmeasurable_paths,
        block.touched,
        "the coverage pair is the episode's paths, not a sample of them"
    );
    for code in block.reasons.keys() {
        assert!(
            REFUSAL_CODES.contains(&code.as_str()),
            "`{code}` is not ADR-007 §7.1's vocabulary"
        );
    }
}

#[test]
fn a_store_that_has_not_filled_the_column_says_who_owns_it() {
    let repo = repo("unfilled");
    repo.grow();
    repo.index();
    repo.clear_complexity();

    let delta = repo.delta();
    verified(&delta);
    let block = delta.complexity.as_ref().expect("the roll-up was read");
    assert!(
        !block.measured,
        "the analyzer could have measured and the index had not filled the column"
    );
    assert_eq!(
        block.reasons.keys().collect::<Vec<_>>(),
        vec!["column_not_filled"],
        "the honest code is `column_not_filled`, not `parse_failed`"
    );
    let gap = block.gap.clone().expect("and it names its owner");
    assert_eq!(gap.owner, "e8.3");
    assert_eq!(gap.section, SectionId::What.as_str());
    assert!(
        printed_gap_columns(&gap) <= CARD_COLUMNS,
        "`{} ({})` is {} columns, over the card's {CARD_COLUMNS}",
        gap.short,
        gap.owner,
        printed_gap_columns(&gap)
    );

    let figure = figure(&delta, "complexity").expect("the row is still there");
    assert!(!figure.known);
    assert!(
        !figure.value.contains('0'),
        "the unknown form prints nothing that reads as zero: {:?}",
        figure.value
    );
    // The document still round-trips through its own parser with the figure in it — the export and the MCP
    // copy these bytes rather than re-deriving them, so a lost field would show up here first.
    let again = EpisodeDelta::from_json(&delta.to_json()).expect("the document re-reads");
    assert_eq!(again, delta);
}

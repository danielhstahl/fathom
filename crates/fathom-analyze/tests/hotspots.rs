//! Hotspots (`e3.8`, `reco-wky`): the seeded god module, the dead-code candidates that admit how they
//! could be wrong, the empty state a new repository gets, and the weights a repository can retune.
//!
//! Four things are asserted the hard way, one per acceptance criterion:
//!
//! * **the ranking, through the real builder.** The corpus is a small repository built by `e3.2`'s
//!   [`Builder`] over six revisions, so fan-in and fan-out are the answers
//!   [`fathom_analyze::graph::query`] gives — not hand-written counts a test could get wrong on
//!   purpose. The seeded god module is big, complex, depended on by fourteen modules and re-pointed in
//!   every revision of the window, which is what makes it first rather than what makes the test
//!   convenient.
//! * **every metric shows its contribution.** Not "a score and a shrug": the parts sum to the printed
//!   total in exact hundredths, each part prints its raw value, the knee it saturated against, its
//!   weight and its ceiling, and the reasons name the counts they came from.
//! * **uncertainty is a field, not a footnote.** A dead-code candidate states its age against the bar
//!   that made it a candidate, and lists the mechanism that could make it wrong — a wildcard that can
//!   name a symbol invisibly, a scope that cannot be enumerated, a window too short to judge.
//! * **zero data is a state, not a crash.** An empty graph, a store with one revision and a policy with
//!   every weight at zero each produce a stated refusal instead of a table of zeroes.
//!
//! Like `tests/layers.rs`, everything here needs no store: the judgment is a pure function of a graph,
//! a naming, `e3.1`'s records and `e3.3`'s churn window, which is what ADR-002 §6.1 requires a
//! `--no-default-features` build to be able to say.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::graph::{
    BuildOutcome, Builder, ChurnReport, Graph, GraphPolicy, Interner, Layers, ModuleInput, Naming,
    RevisionOps, ops_between,
};
use fathom_analyze::hotspots::measure::facts_of_records;
use fathom_analyze::hotspots::{
    self, CandidateKind, Confidence, DeadCodeInput, FileFacts, Hotspot, Metric, ModuleFacts,
    Origin, Policy, Quadrant, Shape, dead_code_from,
    deadcode::{AddedLines, NewSymbol, Source, gates},
    facts_of, policy_from_config_text, rank, render, render_dead, render_row, report,
};
use fathom_analyze::structure::{
    Complexity, Edge, EdgeKind, FileLevel, FileStructure, Granularity, Language, Resolution, Span,
    Structure, Symbol, SymbolKind, SyntheticScope, Visibility,
};

// --------------------------------------------------------------------------------------------
// The corpus: a small repository with one module that has become the problem.
// --------------------------------------------------------------------------------------------

/// One file of the corpus: its dependency edges, its size and its declarations.
#[derive(Clone)]
struct File {
    path: String,
    deps: Vec<String>,
    wildcards: Vec<String>,
    /// `(name, branches, loops, returns, nesting, body_lines)` per top-level declaration — the `e3.1`
    /// proxies the score reads, stated as numbers a reviewer could check in a source file.
    decls: Vec<(String, usize, usize, usize, usize, usize)>,
    code_lines: usize,
    synthetic: bool,
}

/// An ordinary module: two small declarations, forty lines, and its dependencies as `(target)` pairs.
fn file(path: &str, deps: &[&str]) -> File {
    File {
        path: path.to_string(),
        deps: deps.iter().map(|target| (*target).to_string()).collect(),
        wildcards: Vec::new(),
        decls: vec![
            (String::from("run"), 2, 0, 1, 1, 14),
            (String::from("help"), 0, 0, 1, 0, 4),
        ],
        code_lines: 40,
        synthetic: false,
    }
}

/// A file built from a list of dependency paths.
fn many(path: &str, deps: &[String]) -> File {
    File {
        deps: deps.to_vec(),
        ..file(path, &[])
    }
}

impl File {
    /// A dependency whose contents nobody read: a star import (`s1` finding 2).
    fn star(mut self, target: &str) -> Self {
        self.wildcards.push(target.to_string());
        self
    }

    /// Grow it into the god module: fourteen declarations, 1 400 lines, and a name for every branch a
    /// reviewer has to hold — 28 proxies each, 392 across the module, against a knee of 120.
    fn bloated(mut self) -> Self {
        self.code_lines = 1_400;
        self.decls = (0..14)
            .map(|index| (format!("handle_{index}"), 10, 3, 5, 10, 100 + index))
            .collect();
        self
    }

    fn synthetic(mut self) -> Self {
        self.synthetic = true;
        self
    }

    fn structure(&self) -> FileStructure {
        let span = Span {
            start_byte: 0,
            end_byte: 1,
            start_line: 1,
            end_line: self.code_lines,
        };
        let exports: Vec<String> = self.decls.iter().map(|decl| decl.0.clone()).collect();
        let symbols = self
            .decls
            .iter()
            .map(|(name, branches, loops, returns, nesting, body)| Symbol {
                kind: SymbolKind::Fn,
                name: name.clone(),
                container: String::new(),
                span,
                visibility: Visibility::Public,
                exported: true,
                complexity: Complexity {
                    body_lines: *body,
                    branches: *branches,
                    loops: *loops,
                    params: 1,
                    returns: *returns,
                    members: 0,
                    nesting: *nesting,
                },
                modifiers: Vec::new(),
            })
            .collect();
        // Every dependency names the target's `run`, which is what makes a symbol-level `uses` edge
        // appear in the graph and what the dead-code sweep reads.
        let named = "run";
        let edges = self
            .deps
            .iter()
            .map(|target| Edge {
                target: target.clone(),
                symbol: named.to_string(),
                kind: EdgeKind::Resolved,
                specifier: String::new(),
                via: None,
                form: String::new(),
                passes: 1,
                granularity: Granularity::File,
                package: None,
            })
            .chain(self.wildcards.iter().map(|target| Edge {
                target: target.clone(),
                symbol: "*".to_string(),
                kind: EdgeKind::Wildcard,
                specifier: String::new(),
                via: None,
                form: String::new(),
                passes: 1,
                granularity: Granularity::File,
                package: None,
            }))
            .collect();
        FileStructure {
            path: self.path.clone(),
            language: Language::Rust,
            file_level: FileLevel {
                path: self.path.clone(),
                language: Language::Rust,
                bytes: self.code_lines * 32,
                newlines: self.code_lines,
                blank_lines: 0,
                code_lines: self.code_lines,
                parse_errors: 0,
                refused: false,
            },
            structure: Some(Structure {
                symbols,
                exports,
                imports: Vec::new(),
                edges,
                supertypes: Vec::new(),
                synthetic_scopes: if self.synthetic {
                    vec![SyntheticScope {
                        scope: "file".to_string(),
                        reasons: vec![String::from("macro_rules!")],
                    }]
                } else {
                    Vec::new()
                },
                notes: Vec::new(),
                resolution: Resolution::none(),
                package: None,
            }),
            reason: None,
        }
    }

    fn blob(&self) -> String {
        let mut parts: Vec<String> = self.deps.clone();
        parts.extend(self.wildcards.iter().cloned());
        parts.push(self.code_lines.to_string());
        format!("blob:{}:{}", self.path, parts.join(","))
    }
}

/// One revision of the corpus.
struct Revision(Vec<File>);

impl Revision {
    fn build(&self, interner: &mut Interner) -> BuildOutcome {
        let structures = self.0.iter().map(File::structure).collect::<Vec<_>>();
        let blobs = self.0.iter().map(File::blob).collect::<Vec<_>>();
        let inputs: Vec<ModuleInput<'_>> = self
            .0
            .iter()
            .zip(structures.iter())
            .zip(blobs.iter())
            .map(|((file, structure), blob)| ModuleInput {
                path: file.path.as_str(),
                blob: blob.as_str(),
                file: Some(structure),
            })
            .collect();
        builder().build(&inputs, interner)
    }

    fn records(&self) -> Vec<FileStructure> {
        self.0.iter().map(File::structure).collect()
    }
}

fn builder() -> Builder {
    Builder {
        policy: GraphPolicy::default(),
        // `(name, pattern)`, the order `Layers::new` documents: a layer name and the path prefix that
        // claims it, so `crates/ledger/**` is `l1_domain` and the consumers are `l2_app`.
        layers: Layers::new([
            (String::from("l1_domain"), String::from("crates/ledger")),
            (String::from("l2_app"), String::from("crates/consumer")),
        ]),
    }
}

fn range(prefix: &str, count: usize, suffix: &str) -> Vec<String> {
    (0..count)
        .map(|index| format!("{prefix}{index}{suffix}"))
        .collect()
}

/// The corpus, one revision:
///
/// * `crates/ledger/src/engine.rs` — **the seeded god module**: 1 400 lines, 392 complexity points,
///   fourteen consumers in `l2_app` depending on it, and four modules it reaches into.
/// * `crates/common/src/names.rs` — the hub: thirteen callers, forty lines, four proxies.
/// * `crates/tools/src/main.rs` — the sink: thirteen dependencies, no caller.
/// * `crates/legacy/src/big.rs` — the giant: as big and complex as the god module, one caller, no churn.
/// * everything else — ordinary modules, the majority of any repository.
fn corpus() -> Revision {
    let sides = range("crates/ledger/src/side", 6, ".rs");
    let mut files = vec![
        file(
            "crates/ledger/src/engine.rs",
            &["crates/ledger/src/format.rs", "crates/ledger/src/keys.rs"],
        )
        .bloated(),
        file("crates/ledger/src/keys.rs", &[]),
        file("crates/ledger/src/format.rs", &[]),
    ];
    for side in &sides {
        files.push(file(side, &["crates/ledger/src/format.rs"]));
    }
    for app in range("crates/consumer/src/app", 14, ".rs") {
        files.push(file(&app, &["crates/ledger/src/engine.rs"]));
    }
    files.push(file("crates/common/src/names.rs", &[]));
    for user in range("crates/user/src/u", 13, ".rs") {
        files.push(file(&user, &["crates/common/src/names.rs"]));
    }
    // The sink's thirteen dependencies: six side modules and seven application ones.
    let mut sink_deps = sides.clone();
    sink_deps.extend(range("crates/consumer/src/app", 7, ".rs"));
    files.push(many("crates/tools/src/main.rs", &sink_deps));
    files.push(file("crates/legacy/src/big.rs", &["crates/ledger/src/keys.rs"]).bloated());
    files.push(file(
        "crates/api/src/router.rs",
        &["crates/legacy/src/big.rs"],
    ));
    Revision(files)
}

/// Six revisions of the corpus, with the god module re-pointing two of its dependencies in each one —
/// the churn that separates "changed once, in a big commit" from "keeps coming back". The last revision
/// is the one the ranking reads, and it points at `side4` and `side5`.
fn history() -> (Vec<BuildOutcome>, Naming, ChurnReport) {
    let mut revisions = vec![corpus()];
    for step in 0..5 {
        let mut files = revisions.last().expect("the base revision").0.clone();
        for slot in &mut files {
            if slot.path != "crates/ledger/src/engine.rs" {
                continue;
            }
            slot.deps.retain(|target| !target.contains("/side"));
            slot.deps
                .push(format!("crates/ledger/src/side{}.rs", step % 6));
            slot.deps
                .push(format!("crates/ledger/src/side{}.rs", (step + 1) % 6));
        }
        revisions.push(Revision(files));
    }
    let mut interner = Interner::default();
    let outcomes: Vec<BuildOutcome> = revisions
        .iter()
        .map(|revision| revision.build(&mut interner))
        .collect();
    let view = outcomes.iter().fold(Naming::default(), |view, outcome| {
        Naming::merged(&view, &outcome.naming())
    });
    let steps: Vec<RevisionOps> = outcomes
        .windows(2)
        .enumerate()
        .map(|(index, pair)| {
            RevisionOps::new(
                format!("r{index}"),
                ops_between(&pair[0].graph, &pair[1].graph, &view),
            )
        })
        .collect();
    let churn = ChurnReport::measure(&steps, &view, outcomes.last().map(|last| &last.graph));
    (outcomes, view, churn)
}

/// The corpus as facts: the newest revision's graph, its records and its churn window.
fn seeded_facts() -> Vec<ModuleFacts> {
    let (outcomes, view, churn) = history();
    let newest = outcomes.last().expect("six revisions");
    facts_of_records(&newest.graph, &view, &corpus().records(), Some(&churn))
}

fn find<'a>(table: &'a fathom_analyze::hotspots::Table, suffix: &str) -> &'a Hotspot {
    table
        .rows
        .iter()
        .find(|row| row.module.ends_with(suffix))
        .unwrap_or_else(|| panic!("no row ending in {suffix}: {:?}", table.rows))
}

fn item(row: &Hotspot, metric: Metric) -> &fathom_analyze::hotspots::Measurement {
    row.contributions
        .iter()
        .find(|item| item.metric == metric)
        .expect("every metric is listed, measured or not")
}

fn value(row: &Hotspot, metric: Metric) -> f64 {
    item(row, metric).value
}

// --------------------------------------------------------------------------------------------
// AC 1 — the table ranks the seeded god module first, and explains why, per metric.
// --------------------------------------------------------------------------------------------

#[test]
fn the_seeded_god_module_ranks_first_and_is_called_a_god_module() {
    let table = rank("head", &seeded_facts(), &Policy::default(), &[]);
    assert!(table.ranked, "{:?}", table.empty_reason);
    let first = find(&table, "engine.rs");
    assert_eq!(first.rank, 1);
    assert_eq!(first.module, "crates/ledger/src/engine.rs");
    assert_eq!(first.shape, Shape::God, "{}", first.shape_why);
    assert_eq!(first.quadrant, Quadrant::HotComplex);
    assert!(
        first.score > 50,
        "a god module is not a middling row: {}",
        first.score
    );
    // The shape sentence names the axes that made the call, so a reviewer can disagree about them.
    for axis in ["fan-in", "size", "complexity", "churn"] {
        assert!(
            first.shape_why.contains(axis),
            "the god-module sentence must name {axis}: {}",
            first.shape_why
        );
    }
    // And the other shapes in the corpus are each their own shape: the ranking distinguishes them.
    for (suffix, want) in [
        ("common/src/names.rs", Shape::Hub),
        ("tools/src/main.rs", Shape::Sink),
        ("legacy/src/big.rs", Shape::Giant),
        ("consumer/src/app8.rs", Shape::None),
    ] {
        let row = find(&table, suffix);
        assert_eq!(row.shape, want, "{suffix}: {:?}", row.shape_why);
    }
    // The giant is big and complex but quiet and barely used, so it sits below the god module.
    assert!(find(&table, "legacy/src/big.rs").score < first.score);
}

#[test]
fn every_metric_prints_its_contribution_and_the_parts_add_up_exactly() {
    let table = rank("head", &seeded_facts(), &Policy::default(), &[]);
    let row = find(&table, "engine.rs");
    assert_eq!(row.contributions.len(), Metric::ALL.len());
    let sum: i64 = row.contributions.iter().map(|item| item.points.0).sum();
    assert_eq!(
        sum, row.total.0,
        "the printed parts sum to the printed total"
    );
    assert_eq!(
        row.score,
        u8::try_from(((row.total.0 + 50) / 100).clamp(0, 100)).expect("0..=100"),
        "the score is the exact sum, rounded once"
    );
    assert_eq!(report::check_sum(row).0, row.total.0);
    let denominator: f64 = row
        .contributions
        .iter()
        .filter(|item| item.measured)
        .map(|item| item.weight)
        .sum();
    assert!(
        (denominator - row.denominator).abs() < 1e-9,
        "the denominator is the measured weight sum"
    );
    for item in &row.contributions {
        assert!(item.measured, "{item:?} is measurable in the corpus");
        let expected = (10_000.0 * item.weight * item.value / row.denominator).round() as i64;
        assert_eq!(item.points.0, expected, "{item:?}");
        assert!(
            item.points <= item.cap,
            "a part never exceeds its own ceiling: {item:?}"
        );
        assert!(
            !item.reason.is_empty() && !item.basis.is_empty(),
            "{item:?}"
        );
    }
    // Each raw number is what the corpus put there, and each reason says where it came from.
    assert_eq!(
        value(row, Metric::FanIn),
        14.0 / 26.0,
        "14 callers against a knee of 12"
    );
    assert_eq!(item(row, Metric::Complexity).raw, Some(392.0));
    assert_eq!(item(row, Metric::Size).raw, Some(1_400.0));
    assert_eq!(
        item(row, Metric::Size).raw_shown,
        "1 400 lines",
        "the raw number is printed with its unit"
    );
    assert!(
        item(row, Metric::Instability)
            .reason
            .contains("points outward")
    );
    assert!(
        item(row, Metric::Complexity)
            .reason
            .contains("14 declaration")
    );
    assert!(item(row, Metric::Complexity).reason.contains("longest 113"));
    assert!(
        item(row, Metric::Churn)
            .reason
            .contains("of the last 5 revision")
    );
    let rendered = render_row(row).join("\n");
    assert!(
        rendered.contains("hotspots-1"),
        "the vector line names the score"
    );
    for metric in Metric::ALL {
        assert!(
            rendered.contains(metric.id()),
            "the row must print {}: {rendered}",
            metric.id()
        );
    }
}

#[test]
fn the_rendered_table_names_the_weights_and_the_knees_it_used() {
    let policy = Policy::default();
    let table = rank("head", &seeded_facts(), &policy, &[]);
    let lines = render(&table, &policy).join("\n");
    assert!(lines.contains("weights: fan_in 0.25 (default)"));
    assert!(lines.contains("knees: fan_in 12"));
    assert!(lines.contains("crates/ledger/src/engine.rs"));
    assert!(lines.contains("shape calls"));
    assert!(lines.contains("quadrant"));
    assert!(report::summary(&table).contains("first is crates/ledger/src/engine.rs"));
    // The god module gets its full arithmetic printed, not just a row in the grid.
    assert!(
        lines.contains("denominator 1.00"),
        "the row's denominator travels with it"
    );
}

#[test]
fn an_unmeasured_metric_is_a_sentence_and_not_a_zero() {
    // A module the graph knows about that `e3.1` has no record for: its size and complexity are
    // unknown, and unknown is not "tiny and simple".
    let (outcomes, view, churn) = history();
    let newest = outcomes.last().expect("six revisions");
    let mut records = corpus().records();
    records.retain(|record| !record.path.ends_with("engine.rs"));
    let facts = facts_of_records(&newest.graph, &view, &records, Some(&churn));
    let table = rank("head", &facts, &Policy::default(), &[]);
    let row = find(&table, "engine.rs");
    assert_eq!(row.measured, 4, "{:?}", row.contributions);
    for metric in [Metric::Size, Metric::Complexity] {
        let item = item(row, metric);
        assert!(!item.measured);
        assert_eq!(item.points.0, 0);
        assert!(
            item.unmeasured
                .as_deref()
                .is_some_and(|why| why.contains("no structural record")),
            "{item:?}"
        );
    }
    assert!(
        row.caveats
            .iter()
            .any(|caveat| caveat.starts_with("size was not measured")),
        "{:?}",
        row.caveats
    );
    // The four remaining weights are the whole denominator: the missing share is redistributed and
    // printed, not quietly dropped.
    assert!((row.denominator - 0.65).abs() < 1e-9, "{}", row.denominator);
    assert_eq!(
        item(row, Metric::FanIn).cap.0,
        3_846,
        "0.25 of 0.65, in hundredths"
    );
}

#[test]
fn a_wildcard_dependent_is_counted_and_says_so() {
    let mut interner = Interner::default();
    let revision = Revision(vec![
        file("core/target.rs", &[]),
        file("core/star.rs", &[]).star("core/target.rs"),
    ]);
    let outcome = revision.build(&mut interner);
    let naming = outcome.naming();
    let facts = facts_of_records(&outcome.graph, &naming, &revision.records(), None);
    let table = rank("head", &facts, &Policy::default(), &[]);
    let row = find(&table, "core/target.rs");
    let fan_in = item(row, Metric::FanIn);
    assert_eq!(
        fan_in.raw,
        Some(1.0),
        "a wildcard is a dependency, not an absence of one"
    );
    assert!(
        fan_in
            .reason
            .contains("wildcard whose contents are unknown")
    );
    assert!(
        row.caveats
            .iter()
            .any(|caveat| caveat.contains("edge(s) are wildcards")),
        "{:?}",
        row.caveats
    );
}

#[test]
fn a_module_whose_scope_cannot_be_enumerated_is_caveated_as_a_lower_bound() {
    let mut interner = Interner::default();
    let revision = Revision(vec![file("core/macro.rs", &[]).synthetic()]);
    let outcome = revision.build(&mut interner);
    let naming = outcome.naming();
    let facts = facts_of_records(&outcome.graph, &naming, &revision.records(), None);
    assert!(facts.first().expect("one module").incomplete);
    let table = rank("head", &facts, &Policy::default(), &[]);
    let row = find(&table, "core/macro.rs");
    assert!(
        row.caveats
            .iter()
            .any(|caveat| caveat.contains("may have shrunk rather than shrunk")),
        "{:?}",
        row.caveats
    );
}

// --------------------------------------------------------------------------------------------
// AC 4 — the weights are configuration, and the same file produces the same ranking.
// --------------------------------------------------------------------------------------------

const WEIGHTS_TOML: &str = r#"
[hotspots]
window = 12
min_history = 2
top = 5
dead_code_after = 2

[hotspots.weights]
fan_in = 1.0
complexity = 1.0
fan_out = 0.0
instability = 0.0
size = 0.0
churn = 0.0

[hotspots.knee]
fan_in = 4
"#;

#[test]
fn the_weights_come_from_fathom_toml_and_move_the_ranking() {
    let facts = seeded_facts();
    let (policy, notices) = policy_from_config_text(WEIGHTS_TOML);
    assert_eq!(policy.window, 12);
    assert_eq!(policy.top, 5);
    assert_eq!(policy.dead_code_after, 2);
    assert_eq!(policy.weight(Metric::FanIn), 1.0);
    assert_eq!(policy.weight(Metric::Size), 0.0);
    assert_eq!(policy.origin(Metric::FanIn), Origin::Repo);
    assert_eq!(
        policy.origin(Metric::Instability),
        Origin::Repo,
        "an explicit zero is the repository's own line, not a default"
    );
    let (untouched, _) = policy_from_config_text("[hotspots]\ntop = 3\n");
    assert_eq!(untouched.origin(Metric::FanIn), Origin::Default);
    assert_eq!(untouched.knee(Metric::FanIn), Some(12.0));
    assert_eq!(policy.knee(Metric::FanIn), Some(4.0));
    assert!(notices.is_empty(), "{notices:?}");

    let table = rank("head", &facts, &policy, &[]);
    assert_eq!(
        table.rows.len(),
        5,
        "hotspots.top = 5, and the drop is counted"
    );
    assert_eq!(table.rows_dropped, table.modules_seen - 5);
    // With only fan-in and complexity weighted, the order is exactly the order those two values put
    // them in: the ranking followed the configuration and nothing else.
    for pair in table.rows.windows(2) {
        let left = value(&pair[0], Metric::FanIn) + value(&pair[0], Metric::Complexity);
        let right = value(&pair[1], Metric::FanIn) + value(&pair[1], Metric::Complexity);
        assert!(
            left + 1e-9 >= right,
            "{} weighted above {}: {left} < {right}",
            pair[0].module,
            pair[1].module
        );
    }
    for row in &table.rows {
        for metric in [
            Metric::Size,
            Metric::Churn,
            Metric::FanOut,
            Metric::Instability,
        ] {
            assert_eq!(
                item(row, metric).points.0,
                0,
                "{metric:?} is weighted 0 and must contribute 0"
            );
            assert!(item(row, metric).measured, "weighted 0 is still measured");
        }
    }
    // The god module still leads — it leads on fan-in too — but its score is now the two metrics'
    // average and the size of its complexity contribution says so.
    let god = find(&table, "engine.rs");
    assert!(
        god.score > 50,
        "two strong metrics, each worth half: {}",
        god.score
    );
    assert_eq!(
        item(god, Metric::Complexity).cap.0,
        5_000,
        "half the score, in hundredths"
    );
}

#[test]
fn the_same_input_and_the_same_file_produce_the_same_bytes_twice() {
    let facts = seeded_facts();
    let (policy, _) = policy_from_config_text(WEIGHTS_TOML);
    let once = rank("head", &facts, &policy, &[]);
    let twice = rank("head", &facts, &policy, &[]);
    assert_eq!(once, twice);
    assert_eq!(
        serde_json::to_string(&once).expect("serialisable"),
        serde_json::to_string(&twice).expect("serialisable")
    );
    assert_eq!(once.contract, "fathom.hotspots/1");
    // …and a reordered input list cannot change it either: the ranking is a function of the facts.
    let mut shuffled = facts;
    shuffled.reverse();
    assert_eq!(rank("head", &shuffled, &policy, &[]).rows, once.rows);
}

#[test]
fn raising_a_weight_never_lowers_a_score() {
    // Monotonicity, the property that makes retuning a deliberate act rather than a lottery.
    let facts = seeded_facts();
    let light = Policy::default();
    let mut heavy = Policy::default();
    for slot in &mut heavy.weights {
        if slot.metric == Metric::Churn {
            slot.value = 0.6;
        }
    }
    let before = rank("head", &facts, &light, &[]);
    let after = rank("head", &facts, &heavy, &[]);
    let god = |table: &fathom_analyze::hotspots::Table| find(table, "engine.rs").score;
    assert!(
        god(&after) >= god(&before),
        "a heavier churn weight lowered the god module: {} → {}",
        god(&before),
        god(&after)
    );
    // The quiet giant, whose churn is zero, cannot gain from a churn weight going up either: it loses
    // share to the metric that got heavier.
    let giant_before = find(&before, "legacy/src/big.rs").score;
    let giant_after = find(&after, "legacy/src/big.rs").score;
    assert!(
        giant_after <= giant_before,
        "{giant_before} → {giant_after}"
    );
}

#[test]
fn a_bad_weight_is_a_notice_and_a_default_never_a_silent_no_op() {
    let (policy, notices) = policy_from_config_text(
        r#"
[hotspots.weights]
fanin = 0.9
size = 3.0
"#,
    );
    assert!(
        notices
            .iter()
            .any(|line| line.contains("hotspots.weights.fanin") && line.contains("the six are")),
        "{notices:?}"
    );
    assert_eq!(
        policy.weight(Metric::FanIn),
        0.25,
        "the typo did not rename anything"
    );
    let clamped = policy
        .weights
        .iter()
        .find(|weight| weight.metric == Metric::Size)
        .expect("size");
    assert_eq!(
        clamped.value, 1.0,
        "clamped, not renormalised behind the reader"
    );
    assert_eq!(
        clamped.clamped_from,
        Some(3.0),
        "and the clamp is printed, not applied quietly"
    );
}

#[test]
fn a_knee_on_a_ratio_is_refused_because_there_is_no_curve_to_set() {
    let (policy, notices) = policy_from_config_text(
        r#"
[hotspots.knee]
instability = 20
fan_in = 0
"#,
    );
    assert!(
        notices
            .iter()
            .any(|line| line.contains("already a 0.0..=1.0 ratio")),
        "{notices:?}"
    );
    assert!(
        notices
            .iter()
            .any(|line| line.contains("hotspots.knee.fan_in is 0")),
        "{notices:?}"
    );
    assert_eq!(policy.knee(Metric::Instability), None);
    assert_eq!(
        policy.knee(Metric::FanIn),
        Some(12.0),
        "the built-in knee stands"
    );
    assert_eq!(hotspots::default_knee(Metric::Churn), Some(2.0));
}

#[test]
fn a_file_that_does_not_parse_is_a_notice_and_the_defaults() {
    let (policy, notices) = policy_from_config_text("this is not toml at all [[");
    assert_eq!(policy, Policy::default());
    assert!(
        notices
            .iter()
            .any(|line| line.contains("does not parse") && line.contains("built-in default")),
        "{notices:?}"
    );
    // A file that says nothing about hotspots is not an error either.
    let (quiet, notices) = policy_from_config_text("[noise]\nglobs = []\n");
    assert_eq!(quiet, Policy::default());
    assert!(notices.is_empty(), "{notices:?}");
}

#[test]
fn a_window_shorter_than_min_history_leaves_churn_unmeasured() {
    let facts = seeded_facts();
    let (policy, notices) = policy_from_config_text(
        r#"
[hotspots]
window = 1
min_history = 8
"#,
    );
    assert!(
        notices
            .iter()
            .any(|line| line.contains("hotspots.window (1) is below hotspots.min_history (8)")),
        "{notices:?}"
    );
    let table = rank("head", &facts, &policy, &[]);
    let churn = item(find(&table, "engine.rs"), Metric::Churn);
    assert!(!churn.measured);
    assert!(
        churn
            .unmeasured
            .as_deref()
            .is_some_and(|why| why.contains("velocity needs a window")),
        "{churn:?}"
    );
}

#[test]
fn the_policy_round_trips_the_tuning_it_was_given() {
    let (policy, _) = policy_from_config_text(WEIGHTS_TOML);
    let written = fathom_analyze::hotspots::policy::to_toml(&policy);
    assert!(written.contains("window = 12"), "{written}");
    assert!(written.contains("fan_in = 1.00"), "{written}");
    assert!(written.contains("[hotspots.knee]"), "{written}");
    assert!(
        written.contains("fan_out = 0.00"),
        "an explicit zero is the repository's own line and stays: {written}"
    );
    let (quiet, _) = policy_from_config_text("");
    assert!(
        fathom_analyze::hotspots::policy::to_toml(&quiet).contains("built-in default"),
        "a policy that changed nothing writes no keys"
    );
}

// --------------------------------------------------------------------------------------------
// AC 3 — zero data is an empty state, not a crash and not a ranking of zeroes.
// --------------------------------------------------------------------------------------------

#[test]
fn an_empty_graph_ranks_nothing_and_says_why() {
    let table = rank("fresh-repo", &[], &Policy::default(), &[]);
    assert!(!table.ranked);
    assert!(table.rows.is_empty());
    assert_eq!(table.modules_seen, 0);
    assert_eq!(table.modules_scored, 0);
    assert!(
        table.empty_reason.as_deref().is_some_and(
            |why| why.contains("no modules") && why.contains("not a ranking of zeroes")
        ),
        "{:?}",
        table.empty_reason
    );
    // And the renderer prints that refusal instead of a header with nothing under it.
    let lines = render(&table, &Policy::default());
    assert!(lines[1].starts_with("  the module graph"));
    assert!(!lines.iter().any(|line| line.trim_start().starts_with("1 ")));
    assert!(report::summary(&table).starts_with("no hotspot ranking"));
}

#[test]
fn a_repository_with_one_revision_has_no_velocity_and_still_ranks() {
    // The cold index: a graph, structure, and no earlier revision to measure against.
    let mut interner = Interner::default();
    let revision = Revision(vec![
        file("core/a.rs", &["core/b.rs"]),
        file("core/b.rs", &[]),
    ]);
    let outcome = revision.build(&mut interner);
    let naming = outcome.naming();
    let facts = facts_of_records(&outcome.graph, &naming, &revision.records(), None);
    let table = rank("r1", &facts, &Policy::default(), &[]);
    assert!(table.ranked, "structure is a fact even with no history");
    let row = table.first().expect("two modules, one ranked first");
    let churn = item(row, Metric::Churn);
    assert!(!churn.measured);
    assert!(
        churn
            .unmeasured
            .as_deref()
            .is_some_and(|why| why.contains("no earlier revision")),
        "{churn:?}"
    );
    assert_eq!(row.measured, 5, "five of six, and the row says which");
}

#[test]
fn every_weight_at_zero_refuses_to_rank() {
    let (policy, notices) = policy_from_config_text(
        r#"
[hotspots.weights]
fan_in = 0
fan_out = 0
instability = 0
size = 0
complexity = 0
churn = 0
"#,
    );
    assert!(
        notices
            .iter()
            .any(|line| line.contains("every hotspots.weights entry is 0")),
        "{notices:?}"
    );
    let table = rank("head", &seeded_facts(), &policy, &notices);
    assert!(!table.ranked);
    assert!(table.rows.is_empty());
    assert!(
        table
            .empty_reason
            .as_deref()
            .is_some_and(|why| why.contains("weighted 0")),
        "{:?}",
        table.empty_reason
    );
}

#[test]
fn a_module_with_nothing_known_about_it_is_listed_as_unmeasured_not_as_zero() {
    // Instability with no edges is undefined, not `0.0`: "stable" and "nothing is known" differ.
    let table = rank(
        "head",
        &[ModuleFacts::blank("orphan.rs")],
        &Policy::default(),
        &[],
    );
    let row = table.first().expect("one row");
    assert_eq!(row.measured, 2, "only the two fan numbers are measurable");
    assert_eq!(value(row, Metric::Instability), 0.0);
    assert!(
        item(row, Metric::Instability)
            .unmeasured
            .as_deref()
            .unwrap()
            .contains("undefined rather than zero")
    );
    assert!(
        item(row, Metric::Churn)
            .unmeasured
            .as_deref()
            .unwrap()
            .contains("no churn window")
    );
}

// --------------------------------------------------------------------------------------------
// AC 2 — dead-code candidates: how long, and how they could be wrong.
// --------------------------------------------------------------------------------------------

/// Five modules: one that names another, one nothing names, one reached only through a star import, and
/// one whose scope cannot be enumerated.
fn symbols_revision() -> (BuildOutcome, Naming) {
    let mut interner = Interner::default();
    let revision = Revision(vec![
        file("core/uses.rs", &["core/extra.rs"]),
        file("core/extra.rs", &[]),
        file("core/orphan.rs", &[]),
        file("core/star.rs", &[]).star("core/orphan.rs"),
        file("core/macro.rs", &[]).synthetic(),
    ]);
    let outcome = revision.build(&mut interner);
    let naming = outcome.naming();
    (outcome, naming)
}

fn fresh(key: &str, episodes: usize, revision: Option<&str>) -> NewSymbol {
    NewSymbol {
        key: key.to_string(),
        added_episodes_ago: episodes,
        revision: revision.map(str::to_string),
    }
}

fn unreferenced(new: &[NewSymbol]) -> hotspots::DeadCode {
    let (outcome, naming) = symbols_revision();
    let policy = Policy::default();
    hotspots::unreferenced_report(
        "head",
        &outcome.graph,
        &naming,
        new,
        policy.window,
        &policy,
        Vec::new(),
    )
}

#[test]
fn an_unreferenced_symbol_states_its_age_and_the_bar_that_made_it_a_candidate() {
    let report = unreferenced(&[
        fresh("core/orphan.rs::run", 4, Some("abc1234")),
        fresh("core/extra.rs::run", 9, Some("def5678")),
    ]);
    // The referenced symbol is not a candidate at all — the sweep ran on both and only one survived.
    assert_eq!(report.new_symbols, 2);
    assert_eq!(report.candidates.len(), 1, "{:?}", report.candidates);
    let candidate = &report.candidates[0];
    assert_eq!(candidate.kind, CandidateKind::Unreferenced);
    assert_eq!(candidate.subject, "core/orphan.rs::run");
    assert_eq!(
        candidate.verdict,
        Confidence::Uncertain,
        "a star import reaches it"
    );
    let age = candidate
        .age
        .clone()
        .expect("an unreferenced candidate has an age");
    assert_eq!(age.episodes, 4);
    assert!(age.due);
    assert_eq!(age.after, 3);
    assert_eq!(
        age.window, 30,
        "and the window it was judged over is printed too"
    );
    let since = candidate.since.clone().expect("the age as a sentence");
    assert!(
        since.contains("unreferenced for 4 episode(s) since it appeared in abc1234")
            && since.contains("past the `dead_code_after` bar of 3"),
        "{since}"
    );
    assert!(
        candidate
            .why
            .contains("no module other than `core/orphan.rs` names it")
    );
    assert_eq!(
        candidate.decided_by.as_deref(),
        Some("hotspots.dead_code_after = 3")
    );
    assert_eq!(report.symbols_checked, 2);
    assert!(
        !report.limits.is_empty(),
        "a dead-code report carries its limits"
    );
    assert_eq!(report.contract, "fathom.dead-code/1");
}

#[test]
fn a_wildcard_or_a_macro_scope_downgrades_a_candidate_and_says_which() {
    let report = unreferenced(&[
        fresh("core/orphan.rs::run", 4, None),
        fresh("core/macro.rs::run", 6, None),
    ]);
    assert_eq!(report.candidates.len(), 2);
    let helper = report
        .candidates
        .iter()
        .find(|candidate| candidate.subject.contains("orphan"))
        .expect("listed");
    assert_eq!(helper.verdict, Confidence::Uncertain);
    assert!(
        helper
            .uncertainty
            .iter()
            .any(|line| line.contains("reach `core/orphan.rs`")),
        "{:?}",
        helper.uncertainty
    );
    let generated = report
        .candidates
        .iter()
        .find(|candidate| candidate.subject.contains("macro"))
        .expect("listed");
    assert_eq!(generated.verdict, Confidence::Uncertain);
    assert!(
        generated
            .uncertainty
            .iter()
            .any(|line| line.contains("may have shrunk rather than shrunk")),
        "{:?}",
        generated.uncertainty
    );
    assert_eq!(report.uncertain, 2);
    assert_eq!(report.firm, 0, "nothing in this corpus is firm");
}

#[test]
fn a_symbol_that_just_arrived_is_not_due_yet_and_is_not_called_dead() {
    let report = unreferenced(&[fresh("core/orphan.rs::run", 1, Some("9f8e7d6"))]);
    let candidate = report.candidates.first().expect("listed as unverified");
    assert_eq!(candidate.verdict, Confidence::Unverified);
    assert!(!candidate.age.clone().expect("age").due);
    let since = candidate.since.clone().expect("the age as a sentence");
    assert!(
        since.contains("unreferenced for 1 episode(s)") && since.contains("too new to judge"),
        "{since}"
    );
    assert_eq!(report.unverified, 1);
    assert_eq!(report.firm, 0);
    // The renderer prints the counts separately, so a surface cannot quote the total as findings.
    let lines = render_dead(&report).join("\n");
    assert!(lines.contains("0 firm, 0 uncertain, 1 unverified"));
    assert!(lines.contains("[unverified]"));
    assert!(lines.contains("could be wrong because:"));
    assert!(lines.contains("what no static read of this repository can see"));
    assert_eq!(
        report::dead_summary(&report),
        "0 firm · 0 uncertain · 1 unverified"
    );
}

#[test]
fn a_sweep_with_nothing_new_says_that_rather_than_nothing_found() {
    let report = unreferenced(&[]);
    assert!(report.is_empty());
    assert!(
        report
            .empty_reason
            .as_deref()
            .is_some_and(|why| why.contains("no symbol appeared in the last")),
        "{:?}",
        report.empty_reason
    );
}

#[test]
fn a_symbol_that_is_not_in_the_graph_is_not_invented_as_a_candidate() {
    // A key the dictionary never minted — deleted, renamed, or never built — is not a dead symbol; it
    // is a symbol that is not there, and the sweep says nothing rather than something wrong.
    let report = unreferenced(&[fresh("core/gone.rs::Whisper", 9, None)]);
    assert!(report.candidates.is_empty());
    assert_eq!(report.new_symbols, 1, "and the denominator still counts it");
}

// ---- the flag sweep ---------------------------------------------------------------------------

const GATED_RS: &str = r#"
pub fn always() {}

#[cfg(feature = "postgres")]
pub fn connect() {}

#[cfg(feature = "sqlite")]
pub fn connect_lite() {}

#[cfg(test)]
mod tests {
    fn only_in_tests() {}
}
"#;

#[test]
fn a_gate_is_recognised_per_language_and_classified_by_whose_knob_it_is() {
    let rust: Vec<(String, String)> = gates(GATED_RS)
        .into_iter()
        .map(|gate| (gate.flag, gate.kind.as_str().to_string()))
        .collect();
    assert_eq!(
        rust,
        vec![
            (String::from("postgres"), String::from("repository")),
            (String::from("sqlite"), String::from("repository")),
            (String::from("test"), String::from("build")),
        ]
    );
    let go = gates("//go:build !js\npackage main\n");
    assert_eq!(go.first().expect("a build tag").kind.as_str(), "build");
    let python = gates("if TYPE_CHECKING:\n    from x import Y\n");
    assert_eq!(
        python.first().expect("a type-checking gate").flag,
        "TYPE_CHECKING"
    );
    let ts = gates("if (process.env.LAKE_PROXY) {\n  run();\n}\n");
    assert_eq!(
        ts.first().expect("an env gate").flag,
        "process.env.LAKE_PROXY"
    );
    // A plain line is not a gate: the scan matches lines that *are* gates, not anything nearby.
    assert!(gates("pub fn ordinary() {}\n").is_empty());
}

#[test]
fn gated_code_is_a_candidate_only_while_the_repository_has_not_said_which_flags_are_on() {
    let file = Source {
        path: "src/db.rs",
        text: GATED_RS,
    };
    let nobody_said = Policy::default();
    let report = hotspots::gated("head", &[file], &nobody_said);
    assert_eq!(
        report.candidates.len(),
        2,
        "postgres and sqlite, not cfg(test)"
    );
    assert!(
        report
            .candidates
            .iter()
            .all(|candidate| candidate.verdict == Confidence::Uncertain)
    );
    assert!(
        report.candidates[0]
            .uncertainty
            .iter()
            .any(|line| line.contains("declares no flags as on"))
    );
    // cfg(test) was counted and not reported: it is the build's knob, not the repository's.
    assert_eq!(report.gated_not_reported, 1);

    let (policy, notices) = policy_from_config_text(
        r#"
[hotspots]
flags = ["postgres"]
"#,
    );
    assert_eq!(policy.flags, vec![String::from("postgres")]);
    assert!(notices.is_empty(), "{notices:?}");
    assert!(policy.flag_on("postgres") && !policy.flag_on("sqlite"));
    let report = hotspots::gated("head", &[file], &policy);
    let only = report.candidates.first().expect("sqlite remains");
    assert!(only.subject.starts_with("src/db.rs:"));
    assert!(only.why.contains("the repository declares 1 flag(s) on"));
    assert!(
        only.uncertainty
            .iter()
            .any(|line| line.contains("[postgres]") && line.contains("not a resolved build"))
    );
    assert_eq!(
        report.gated_not_reported, 2,
        "postgres moved into the not-reported count"
    );
    assert!(
        render_dead(&report)
            .join("\n")
            .contains("gated group(s) not reported")
    );
}

// ---- the duplication sweep --------------------------------------------------------------------

/// Twelve substantive lines a reviewer would call a copy.
fn block(tag: &str) -> Vec<String> {
    (0..12)
        .map(|index| format!("let value_{tag}_{index} = compute_the_thing(index) * multiplier;"))
        .collect()
}

#[test]
fn a_block_that_arrived_twice_in_one_episode_is_one_candidate() {
    let policy = Policy::default();
    let lines = block("a");
    let files = [
        AddedLines {
            path: "src/a.rs",
            lines: &lines,
        },
        AddedLines {
            path: "src/b.rs",
            lines: &lines,
        },
    ];
    let report = hotspots::duplicated_growth("head", &files, &policy);
    assert_eq!(
        report.candidates.len(),
        1,
        "overlapping windows are one block: {:?}",
        report.candidates
    );
    let candidate = &report.candidates[0];
    assert_eq!(candidate.kind, CandidateKind::Duplicated);
    assert_eq!(candidate.verdict, Confidence::Uncertain);
    assert!(candidate.why.contains("src/a.rs"));
    assert!(candidate.why.contains("src/b.rs"));
    assert!(
        candidate
            .why
            .contains("both copies arrived in this episode")
    );
    assert!(
        candidate
            .uncertainty
            .iter()
            .any(|line| line.contains("table-driven test"))
    );
    assert!(
        candidate
            .decided_by
            .as_deref()
            .is_some_and(|line| line.contains("duplicate_block_lines = 12"))
    );
}

#[test]
fn a_longer_copy_is_still_one_block_and_not_a_run_of_blocks() {
    let policy = Policy::default();
    let mut lines = block("a");
    for index in 12..20 {
        lines.push(format!("let extra_{index} = another_call(index);"));
    }
    let files = [
        AddedLines {
            path: "src/a.rs",
            lines: &lines,
        },
        AddedLines {
            path: "src/b.rs",
            lines: &lines,
        },
    ];
    let report = hotspots::duplicated_growth("head", &files, &policy);
    assert_eq!(report.candidates.len(), 1, "{:?}", report.candidates);
    assert!(
        report.candidates[0]
            .why
            .contains("a block of 20 added lines"),
        "the copy's own span is printed: {}",
        report.candidates[0].why
    );
}

#[test]
fn the_duplication_sweep_refuses_what_would_only_look_like_a_copy() {
    let policy = Policy::default();
    // A test file: repetition by design, and `e2.9` owns what a test case is.
    let lines = block("t");
    let tests = [
        AddedLines {
            path: "tests/a.rs",
            lines: &lines,
        },
        AddedLines {
            path: "tests/b.rs",
            lines: &lines,
        },
    ];
    let report = hotspots::duplicated_growth("head", &tests, &policy);
    assert!(report.candidates.is_empty());
    assert!(
        report
            .notices
            .iter()
            .any(|line| line.contains("table-driven test is repetition by design")),
        "{:?}",
        report.notices
    );
    // A twelve-line import block: what the top of a big file looks like.
    let imports: Vec<String> = (0..12)
        .map(|index| format!("use crate::very_long_module_name_{index}::Thing;"))
        .collect();
    let files = [
        AddedLines {
            path: "src/a.rs",
            lines: &imports,
        },
        AddedLines {
            path: "src/b.rs",
            lines: &imports,
        },
    ];
    assert!(
        hotspots::duplicated_growth("head", &files, &policy)
            .candidates
            .is_empty(),
        "two files with the same imports is not duplicated structure"
    );
    // One line, twice, is not a block.
    let short = vec![String::from(
        "let one = two_three_four_five_six_seven_eight_nine();",
    )];
    let files = [
        AddedLines {
            path: "src/a.rs",
            lines: &short,
        },
        AddedLines {
            path: "src/b.rs",
            lines: &short,
        },
    ];
    assert!(
        hotspots::duplicated_growth("head", &files, &policy)
            .candidates
            .is_empty()
    );
}

#[test]
fn the_three_sweeps_arrive_as_one_report_with_honest_counts() {
    let (outcome, naming) = symbols_revision();
    let policy = Policy::default();
    let gated = [Source {
        path: "src/db.rs",
        text: GATED_RS,
    }];
    let lines = block("c");
    let blocks = [
        AddedLines {
            path: "src/a.rs",
            lines: &lines,
        },
        AddedLines {
            path: "src/b.rs",
            lines: &lines,
        },
    ];
    let new = [fresh("core/orphan.rs::run", 4, None)];
    let input = DeadCodeInput {
        revision: "head",
        graph: &outcome.graph,
        naming: &naming,
        new_symbols: &new,
        gated: &gated,
        added: &blocks,
        policy: &policy,
        notices: &[],
    };
    let report = dead_code_from(input);
    assert_eq!(report.candidates.len(), 4, "{:?}", report.candidates);
    assert_eq!(
        report.firm + report.uncertain + report.unverified,
        report.candidates.len(),
        "the three verdicts are a partition, not a suggestion"
    );
    assert_eq!(report.of_kind(CandidateKind::Unreferenced).len(), 1);
    assert_eq!(report.of_kind(CandidateKind::Gated).len(), 2);
    assert_eq!(report.of_kind(CandidateKind::Duplicated).len(), 1);
    assert!(!report.limits.is_empty());
    // Deterministic: the same inputs print in the same order.
    let again = dead_code_from(DeadCodeInput {
        revision: "head",
        graph: &outcome.graph,
        naming: &naming,
        new_symbols: &new,
        gated: &gated,
        added: &blocks,
        policy: &policy,
        notices: &[],
    });
    assert_eq!(report, again);
    assert_eq!(
        report::dead_to_json(&report)["contract"],
        "fathom.dead-code/1"
    );
}

#[test]
fn an_ignored_path_is_excluded_counted_and_named() {
    let (policy, notices) = policy_from_config_text(
        r#"
[hotspots]
ignore = ["src/generated/**", "vendor"]
"#,
    );
    assert!(notices.is_empty(), "{notices:?}");
    assert!(policy.ignores("src/generated/api.rs"));
    assert!(policy.ignores("vendor/lib.rs"));
    assert!(!policy.ignores("src/lib.rs"));
    let report = hotspots::gated(
        "head",
        &[Source {
            path: "src/generated/api.rs",
            text: GATED_RS,
        }],
        &policy,
    );
    assert!(report.candidates.is_empty());
    assert_eq!(report.ignored_paths, 1);
    assert_eq!(
        report.ignored_patterns,
        vec![String::from("src/generated/**"), String::from("vendor")],
        "the report names every pattern that could have swallowed a path"
    );
    // A pattern that is not a valid glob is a notice and matches nothing, never a silent "nothing
    // matches" — the refusal names the pattern that pretends to exclude something.
    let (broken, _) = policy_from_config_text("[hotspots]\nignore = [\"src/**[\"]\n");
    assert!(
        broken
            .notices
            .iter()
            .any(|line| line.contains("not a valid glob") && line.contains("src/**[")),
        "{:?}",
        broken.notices
    );
    assert!(
        fathom_analyze::hotspots::policy::notices_of(&broken)
            .iter()
            .any(|line| line.contains("not a valid glob")),
        "the checker repeats it for a policy handed over from elsewhere"
    );
    assert!(
        !broken.ignores("src/[unclosed/x.rs"),
        "a broken pattern excludes nothing"
    );
}

#[test]
fn the_facts_layer_reads_the_graph_and_e3_1_records_together() {
    // A check on the seam itself: the fan numbers in the facts are the answers `e3.2`'s queries give,
    // and `e3.1`'s spans arrive as complexity points rather than as a guess.
    let (outcomes, view, churn) = history();
    let newest = outcomes.last().expect("six revisions");
    let records = corpus().records();
    let facts = facts_of_records(&newest.graph, &view, &records, Some(&churn));
    let god = facts
        .iter()
        .find(|facts| facts.module.ends_with("engine.rs"))
        .expect("the god module is in the graph");
    assert_eq!(god.fan_in, 14, "one per consumer module");
    assert_eq!(god.fan_in_wildcard, 0);
    assert_eq!(
        god.fan_out, 4,
        "format, keys, and the two side modules of the last revision"
    );
    let complexity = god.complexity.expect("it has a structural record");
    assert_eq!(complexity.points, 14 * 28);
    assert_eq!(complexity.declarations, 14);
    assert_eq!(complexity.max_body_lines, 113);
    assert_eq!(complexity.max_nesting, 10);
    assert_eq!(god.code_lines, Some(1_400));
    let churn_facts = god
        .churn
        .expect("it churns in every revision of the window");
    assert!(churn_facts.events > 0);
    assert_eq!(
        churn_facts.revisions, 5,
        "every transition of the window touched it"
    );
    assert_eq!(churn_facts.window, 5, "the denominator the rate prints");
    assert_eq!(
        god.layer, "l1_domain",
        "and the tag rides along, printed and never scored"
    );
    // The same facts come out whether they are built from records or from `FileFacts`, which is what a
    // lane would hand the seam after reading its own records.
    let direct: Vec<FileFacts> = records.iter().map(FileFacts::of).collect();
    let graph: &Graph = &newest.graph;
    assert_eq!(
        facts_of(graph, &view, &direct, Some(&churn)),
        facts,
        "the two entrances to the seam agree"
    );
    // And the hole is nameable: everything here has a record, so the list is empty and says so.
    assert!(fathom_analyze::hotspots::missing_structures(graph, &view, &direct).is_empty());
}

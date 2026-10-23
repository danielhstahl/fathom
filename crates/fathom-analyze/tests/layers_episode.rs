//! The per-episode layering answer, over the real graph lane (`e3.6`, `reco-cfp`).
//!
//! `tests/layers.rs` proves the judgment against a configured repository. This proves the *seam*: two
//! revisions recorded through [`Lane::record`], and one episode's layering answer read out of the store
//! without materialising the world twice. The claims here are the ones the ticket's "crucially, the DELTA"
//! clause rests on:
//!
//! * the after-revision graph comes from the stored `graph_delta` rows, not from a second chain walk;
//! * the violation set that graph produces is the set a full materialisation of the same revision
//!   produces — [`verify`] is the guard, and a disagreement is a lane bug, not a state of the repository;
//! * a violation introduced in this episode is news, and the same violation in the revision before it was
//!   not, which is the whole reason the delta exists;
//! * and a violation whose file was *deleted* leaves with it, as a resolution rather than a mystery.
//!
//! The line citation survives the store: a surface has the working tree in hand, so it passes a
//! [`Locate`] built from the revision it is reviewing, and the violation still names a line.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::cycles::debt::Ratchet;
use fathom_analyze::graph::layers::Layers;
use fathom_analyze::graph::{self, GraphPolicy, Lane, ModuleInput};
use fathom_analyze::layering::{self, Debt, Locate, Rules};
use fathom_analyze::structure::{
    Complexity, Edge, EdgeKind, FileLevel, FileStructure, Granularity, Import, Language,
    Resolution, Span, Structure, Symbol, SymbolKind, Visibility,
};
use fathom_core::time::Timestamp;
use fathom_store::spine::Store;

/// The instant every revision is stamped with; expiry arithmetic is caller-supplied on purpose.
const NOW: i64 = 2_000_000_000_000;

/// The same four-layer system as `tests/layers.rs`, kept small because this file is about the seam.
const CONFIG: &str = r#"
[layers]
l1_store = ["crates/store"]
l2_core = ["crates/core"]
l3_api = ["crates/api"]

[layering]
order = ["l1_store", "l2_core", "l3_api"]
no_upward = true
"#;

/// A module: its path and what it depends on, with the line each statement sits on.
#[derive(Debug, Clone)]
struct Fixture {
    path: String,
    deps: Vec<(&'static str, usize)>,
}

fn fixture(path: &str, deps: &[(&'static str, usize)]) -> Fixture {
    Fixture {
        path: path.to_string(),
        deps: deps.to_vec(),
    }
}

/// One file's structural record: one export, one import per dependency, with the span that carries.
fn structure_of(file: &Fixture) -> FileStructure {
    let span = Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    let stem = file
        .path
        .rsplit('/')
        .next()
        .unwrap_or(file.path.as_str())
        .trim_end_matches(".rs")
        .to_string();
    let symbol = Symbol {
        kind: SymbolKind::Fn,
        name: stem.clone(),
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
    };
    let structure = Structure {
        symbols: vec![symbol],
        exports: vec![stem],
        imports: file
            .deps
            .iter()
            .map(|(target, line)| Import {
                specifier: format!("{target}::Thing"),
                symbol: "Thing".to_string(),
                form: "use".to_string(),
                reexport: false,
                span: Span {
                    start_byte: *line * 10,
                    end_byte: *line * 10 + 9,
                    start_line: *line,
                    end_line: *line,
                },
            })
            .collect(),
        edges: file
            .deps
            .iter()
            .map(|(target, _)| Edge {
                target: (*target).to_string(),
                symbol: "Thing".to_string(),
                kind: EdgeKind::Resolved,
                specifier: String::new(),
                via: None,
                form: "use".to_string(),
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
        path: file.path.clone(),
        language: Language::Rust,
        file_level: FileLevel {
            path: file.path.clone(),
            language: Language::Rust,
            bytes: 12,
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

/// One revision's live file list, owned so the hand-off can borrow from it.
struct Revision {
    files: Vec<Fixture>,
    structures: Vec<FileStructure>,
    blobs: Vec<String>,
}

impl Revision {
    fn new(files: Vec<Fixture>) -> Self {
        let structures = files.iter().map(structure_of).collect();
        let blobs = files
            .iter()
            .map(|file| {
                format!(
                    "blob:{}:{}",
                    file.path,
                    file.deps
                        .iter()
                        .map(|(target, _)| (*target).to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .collect();
        Self {
            files,
            structures,
            blobs,
        }
    }

    fn inputs(&self) -> Vec<ModuleInput<'_>> {
        self.files
            .iter()
            .zip(self.structures.iter())
            .zip(self.blobs.iter())
            .map(|((file, structure), blob)| ModuleInput {
                path: file.path.as_str(),
                blob: blob.as_str(),
                file: Some(structure),
            })
            .collect()
    }

    /// The line evidence for this revision — what a surface holding the working tree would pass.
    fn locate(&self) -> Locate {
        Locate::from_inputs(&self.inputs())
    }
}

/// The rules, read out of the same text the CLI would read, with the tags the lane's builder needs.
fn rules() -> (Rules, Layers) {
    let (layers, notices) = Layers::from_repo_config_text(CONFIG);
    assert!(notices.is_empty(), "{notices:?}");
    let (rules, notices) = layering::load(&layers, CONFIG);
    assert_eq!(rules.refused, 0, "{notices:?}");
    (rules, layers)
}

/// A lane whose builder carries the layer tags, so every stored row arrives already tagged.
fn lane(store: &mut Store, layers: Layers) -> Lane<'_> {
    Lane::new(
        store,
        graph::Builder {
            policy: GraphPolicy {
                chain_full_every: 4,
                max_chain_walk: 4,
                ..GraphPolicy::default()
            },
            layers,
        },
    )
    .expect("a lane over a fresh store")
}

fn store_for() -> Store {
    Store::open_in_memory().expect("open the store")
}

fn now() -> Timestamp {
    Timestamp::from_unix_micros(NOW)
}

/// A repository with nothing wrong in it: every reference points down the order.
fn clean() -> Revision {
    Revision::new(vec![
        fixture("crates/store/src/sink.rs", &[]),
        fixture(
            "crates/core/src/engine.rs",
            &[("crates/store/src/sink.rs", 3)],
        ),
        fixture(
            "crates/api/src/router.rs",
            &[("crates/core/src/engine.rs", 4)],
        ),
    ])
}

/// The same repository with one upward reference added, on a line this test chooses.
fn upward() -> Revision {
    Revision::new(vec![
        fixture(
            "crates/store/src/sink.rs",
            &[("crates/core/src/engine.rs", 5)],
        ),
        fixture(
            "crates/core/src/engine.rs",
            &[("crates/store/src/sink.rs", 3)],
        ),
        fixture(
            "crates/api/src/router.rs",
            &[("crates/core/src/engine.rs", 4)],
        ),
    ])
}

const UPWARD_KEY: &str = "no_upward::crates/store/src/sink.rs->crates/core/src/engine.rs";

#[test]
fn an_episode_reports_the_violation_it_introduced_and_nothing_that_was_already_there() {
    let (rules, layers) = rules();
    let mut store = store_for();
    let mut lane = lane(&mut store, layers);
    let before = clean();
    lane.record("tree1", None, &before.inputs(), NOW)
        .expect("record the first revision");
    let after = upward();
    lane.record("tree2", Some("tree1"), &after.inputs(), NOW + 1)
        .expect("record the second revision");

    let report = layering::episode(
        &lane,
        "tree1",
        "tree2",
        &rules,
        &after.locate(),
        &Ratchet::empty(),
        now(),
    )
    .expect("the episode answer");

    assert_eq!(report.delta.introduced, vec![UPWARD_KEY.to_string()]);
    assert!(report.delta.resolved.is_empty());
    assert_eq!(
        report.new_violations(),
        1,
        "one violation, and this turn caused it"
    );
    assert_eq!(report.violations_count(), 1, "and nothing was already owed");
    let entry = &report.verdict.violations[0];
    assert!(entry.introduced, "the news is labelled as news");
    // The line evidence came from the revision being reviewed, through the store side of the seam.
    assert_eq!(
        entry.violation.line,
        Some(5),
        "{}",
        entry.violation.location()
    );
    assert_eq!(entry.violation.location(), "crates/store/src/sink.rs:5");
    assert!(
        report.exit_code().is_none(),
        "warn does not decide the exit"
    );
    assert!(
        report.counters.checked > 0,
        "rows were examined: {:?}",
        report.counters
    );
}

#[test]
fn the_stepped_graph_agrees_with_a_fresh_materialisation_of_the_same_revision() {
    // The claim `episode` buys its complexity budget with: applying the stored delta rows to the graph
    // in hand produces the violations of the revision the lane would have walked the chain to build.
    let (rules, layers) = rules();
    let mut store = store_for();
    let mut lane = lane(&mut store, layers);
    let history = [
        clean(),
        upward(),
        // A second upward reference, from a file that did not exist before: an *addition*.
        Revision::new(vec![
            fixture(
                "crates/store/src/sink.rs",
                &[("crates/core/src/engine.rs", 5)],
            ),
            fixture(
                "crates/store/src/read.rs",
                &[("crates/core/src/engine.rs", 7)],
            ),
            fixture(
                "crates/core/src/engine.rs",
                &[("crates/store/src/sink.rs", 3)],
            ),
            fixture(
                "crates/api/src/router.rs",
                &[("crates/core/src/engine.rs", 4)],
            ),
        ]),
        // And the repair, which is a modification *and* a deletion in one revision.
        Revision::new(vec![
            fixture("crates/store/src/sink.rs", &[]),
            fixture(
                "crates/core/src/engine.rs",
                &[("crates/store/src/sink.rs", 3)],
            ),
            fixture(
                "crates/api/src/router.rs",
                &[("crates/core/src/engine.rs", 4)],
            ),
        ]),
    ];
    let mut previous: Option<String> = None;
    let mut oids = Vec::new();
    for (index, revision) in history.iter().enumerate() {
        let oid = format!("tree{index}");
        lane.record(
            &oid,
            previous.as_deref(),
            &revision.inputs(),
            NOW + i64::try_from(index).unwrap_or(0),
        )
        .expect("record a revision");
        oids.push(oid);
        previous = Some(oids[index].clone());
    }

    for step in 1..oids.len() {
        let from = &oids[step - 1];
        let to = &oids[step];
        assert!(
            layering::verify(&lane, from, to, &rules, &Locate::empty())
                .expect("the two graphs are comparable"),
            "the stepped graph and a fresh materialisation of {to} disagree"
        );
        let stepped = layering::episode(
            &lane,
            from,
            to,
            &rules,
            &Locate::empty(),
            &Ratchet::empty(),
            now(),
        )
        .expect("the episode answer");
        // The same answer a caller who materialised both ends would get, which is the whole point of
        // spending the delta rather than the walk.
        let whole = layering::episode(
            &lane,
            to,
            to,
            &rules,
            &Locate::empty(),
            &Ratchet::empty(),
            now(),
        )
        .expect("the same revision against itself");
        assert_eq!(
            stepped.violations_count(),
            whole.violations_count(),
            "{from} → {to}"
        );
    }
}

#[test]
fn an_episode_that_repairs_a_violation_reports_it_resolved() {
    let (rules, layers) = rules();
    let mut store = store_for();
    let mut lane = lane(&mut store, layers);
    let broken = upward();
    lane.record("tree1", None, &broken.inputs(), NOW)
        .expect("record the broken revision");
    let fixed = clean();
    lane.record("tree2", Some("tree1"), &fixed.inputs(), NOW + 1)
        .expect("record the repair");

    let report = layering::episode(
        &lane,
        "tree1",
        "tree2",
        &rules,
        &fixed.locate(),
        &Ratchet::empty(),
        now(),
    )
    .expect("the episode answer");

    assert_eq!(report.delta.resolved, vec![UPWARD_KEY.to_string()]);
    assert_eq!(report.new_violations(), 0);
    assert_eq!(report.resolved_violations(), 1);
    assert!(report.verdict.violations.is_empty());
}

#[test]
fn a_baselined_violation_is_debt_in_an_episode_and_never_news() {
    let (rules, layers) = rules();
    let mut store = store_for();
    let mut lane = lane(&mut store, layers);
    lane.record("tree1", None, &clean().inputs(), NOW)
        .expect("record the first revision");
    lane.record("tree2", Some("tree1"), &upward().inputs(), NOW + 1)
        .expect("record the second revision");

    let day = Timestamp::from_unix_micros(NOW + 30 * 86_400_000_000);
    let (year, month, date) = day.civil();
    let ratchet = Ratchet::from_text(
        &format!(
            "[[allowance]]\nkind = \"layer\"\nkey = \"{UPWARD_KEY}\"\nowner = \"team-store\"\n\
             expires = \"{year:04}-{month:02}-{date:02}\"\nreason = \"the door is being rebuilt\"\n"
        ),
        "ratchet.toml",
    );
    let report = layering::episode(
        &lane,
        "tree1",
        "tree2",
        &rules,
        &Locate::empty(),
        &ratchet,
        now(),
    )
    .expect("the episode answer");

    assert_eq!(report.new_violations(), 0, "it is introduced and forgiven");
    assert_eq!(report.baselined_violations(), 1);
    let debt: Vec<&Debt> = report.verdict.debt.iter().collect();
    assert_eq!(debt.len(), 1);
    assert_eq!(debt[0].key, UPWARD_KEY);
    assert_eq!(debt[0].owner, "team-store");
    assert_eq!(debt[0].days_left, Some(30));
    // And the delta still says what moved, because the delta is about the graph, not about the ledger.
    assert_eq!(report.delta.introduced, vec![UPWARD_KEY.to_string()]);
}

#[test]
fn a_cold_index_labels_nothing_as_new() {
    // No earlier revision is not a clean slate: a report that called every violation *introduced* would
    // be the noise the ratchet exists to prevent.
    let (rules, layers) = rules();
    let mut store = store_for();
    let mut lane = lane(&mut store, layers);
    let broken = upward();
    lane.record("tree1", None, &broken.inputs(), NOW)
        .expect("record the only revision");

    let report = layering::snapshot(
        &lane,
        "tree1",
        &rules,
        &broken.locate(),
        &Ratchet::empty(),
        now(),
    )
    .expect("the snapshot answer");

    assert_eq!(report.violations_count(), 1);
    assert_eq!(report.new_violations(), 0, "nothing to be new against");
    assert!(
        report
            .verdict
            .violations
            .iter()
            .all(|entry| !entry.introduced),
        "{:?}",
        report.verdict.violations
    );
    assert!(
        report
            .delta
            .sentence()
            .contains("no earlier revision to compare to"),
        "{}",
        report.delta.sentence()
    );
    assert_eq!(
        report.to_json()["delta"]["episode"].as_bool(),
        Some(false),
        "the export says there was no episode, rather than leaving it to be inferred from two zeros"
    );
}

#[test]
fn a_violation_in_an_untagged_revision_still_needs_no_tags_to_be_wrong() {
    // The lane's builder carries the tags here, but a repository that has not declared any is a real
    // state: `no_upward` has nothing to be upward relative to, so the honest answer is that nothing was
    // judged — not a clean bill.
    let (rules, _) = rules();
    let mut store = store_for();
    let mut lane = lane(&mut store, Layers::default());
    lane.record("tree1", None, &clean().inputs(), NOW)
        .expect("record the first revision");
    lane.record("tree2", Some("tree1"), &upward().inputs(), NOW + 1)
        .expect("record the second revision");

    let report = layering::episode(
        &lane,
        "tree1",
        "tree2",
        &rules,
        &Locate::empty(),
        &Ratchet::empty(),
        now(),
    )
    .expect("the episode answer");

    assert!(
        report.verdict.violations.is_empty(),
        "an untagged graph has no upward references: {:?}",
        report.verdict.violations
    );
    assert!(report.sentence().contains("untagged") || report.counters.untagged > 0);
}

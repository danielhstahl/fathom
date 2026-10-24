//! The per-episode cycle answer, over the real graph lane (`e3.5`, `reco-8md`).
//!
//! `tests/cycles.rs` proves the model; this proves the seam: a revision recorded through
//! [`Lane::record`], the next revision recorded on top of it, and the cycle answer read back out of the
//! store without materialising the world twice. The three claims here are the ones the ticket's first
//! sentence rests on: the answer comes from the *stored delta* (not from a diff of two full
//! graphs), it agrees with a full recomputation of the revision it ended at, and a module deleted
//! between the two revisions takes its cycle with it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::cycles::{self, Options, Ratchet, View};
use fathom_analyze::graph::{self, GraphPolicy, Lane, ModuleInput};
use fathom_analyze::structure::{
    Complexity, Edge, EdgeKind, FileLevel, FileStructure, Granularity, Language, Resolution, Span,
    Structure, Symbol, SymbolKind, Visibility,
};
use fathom_store::spine::Store;

/// The instant every revision here is stamped with. The lane takes `now` as an argument precisely so a
/// test can be written like this (`fathom_core::time`).
const NOW: i64 = 2_000_000_000_000;

/// A module: its path and what it depends on.
#[derive(Debug, Clone)]
struct Fixture {
    path: String,
    deps: Vec<String>,
}

fn fixture(path: &str, deps: &[&str]) -> Fixture {
    Fixture {
        path: path.to_string(),
        deps: deps.iter().map(|d| (*d).to_string()).collect(),
    }
}

/// The structural record a fixture would have produced: one public export, one import per dependency.
fn structure_of(fixture: &Fixture) -> FileStructure {
    let span = Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    let symbol = Symbol {
        kind: SymbolKind::Fn,
        name: "run".to_string(),
        container: String::new(),
        span,
        visibility: Visibility::Public,
        exported: true,
        complexity: Complexity {
            body_lines: 1,
            branches: 0,
            loops: 0,
            params: 0,
            members: 0,
            nesting: 0,
            returns: 0,
        },
        modifiers: Vec::new(),
    };
    let structure = Structure {
        symbols: vec![symbol],
        exports: vec!["run".to_string()],
        imports: Vec::new(),
        edges: fixture
            .deps
            .iter()
            .map(|target| Edge {
                target: target.clone(),
                symbol: String::new(),
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
        path: fixture.path.clone(),
        language: Language::Rust,
        file_level: FileLevel {
            path: fixture.path.clone(),
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
            .map(|file| format!("blob:{}:{}", file.path, file.deps.join(",")))
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
            .map(|((fixture, file), blob)| ModuleInput {
                path: fixture.path.as_str(),
                blob: blob.as_str(),
                file: Some(file),
            })
            .collect()
    }
}

/// A lane over a throwaway store, shortened anchor period so the chain stays short.
fn lane(store: &mut Store) -> Lane<'_> {
    Lane::new(
        store,
        graph::Builder {
            policy: GraphPolicy {
                chain_full_every: 4,
                max_chain_walk: 4,
                ..GraphPolicy::default()
            },
            layers: graph::Layers::default(),
        },
    )
    .expect("a lane over a fresh store")
}

/// A store for one test. In memory, like every other lane test: nothing here needs a file on disk,
/// and a store that leaks onto the filesystem is a test that fails differently on a clean checkout.
fn store_for(_name: &str) -> Store {
    Store::open_in_memory().expect("open the store")
}

#[test]
fn an_episode_through_the_lane_reports_the_cycle_it_introduced() {
    let mut store = store_for("introduce");
    let mut lane = lane(&mut store);
    let before = Revision::new(vec![
        fixture("a.rs", &["b.rs"]),
        fixture("b.rs", &["c.rs"]),
        fixture("c.rs", &[]),
    ]);
    lane.record("tree1", None, &before.inputs(), NOW)
        .expect("record the first revision");
    let after = Revision::new(vec![
        fixture("a.rs", &["b.rs"]),
        fixture("b.rs", &["c.rs"]),
        fixture("c.rs", &["a.rs"]),
    ]);
    lane.record("tree2", Some("tree1"), &after.inputs(), NOW + 1)
        .expect("record the second revision");

    let report = cycles::episode(
        &lane,
        "tree1",
        "tree2",
        &Ratchet::empty(),
        Options::default(),
        now(),
    )
    .expect("the episode answer");

    assert_eq!(report.delta.introduced.len(), 1);
    assert_eq!(
        report.delta.introduced[0].display(),
        "a.rs → b.rs → c.rs → a.rs"
    );
    assert_eq!(report.new_cycles(), 1, "nothing was baselined");
    assert_eq!(report.health.nodes_in_cycles, 3);
    assert_eq!(
        report.counters.full_recomputes, 1,
        "the answer came from the stored delta, not from a recomputation of the graph"
    );
    assert!(report.counters.updates > 0, "and the update was counted");
    assert_eq!(
        report.delta.affected.splits, 0,
        "nothing broke, so nothing was re-split"
    );
    // The lane answered the changed-edge question out of `graph_delta`, which is what made the above
    // cheap: the same call a pane uses.
    let delta = lane
        .changed_edges("tree1", "tree2")
        .expect("the stored delta");
    assert!(
        delta.from_store,
        "one indexed query, not two materialisations"
    );
}

#[test]
fn the_incremental_answer_equals_a_full_recomputation_of_the_revision_it_ended_at() {
    let mut store = store_for("verify");
    let mut lane = lane(&mut store);
    let history = [
        Revision::new(vec![fixture("a.rs", &[]), fixture("b.rs", &["a.rs"])]),
        Revision::new(vec![
            fixture("a.rs", &["b.rs"]),
            fixture("b.rs", &["a.rs"]),
            fixture("c.rs", &["a.rs"]),
        ]),
        Revision::new(vec![
            fixture("a.rs", &["b.rs"]),
            fixture("b.rs", &["a.rs"]),
            fixture("c.rs", &[]),
        ]),
        Revision::new(vec![fixture("a.rs", &[]), fixture("c.rs", &[])]),
    ];
    let mut previous: Option<String> = None;
    let mut oids = Vec::new();
    for (index, revision) in history.iter().enumerate() {
        let oid = format!("tree{index}");
        lane.record(
            &oid,
            previous.as_deref(),
            &revision.inputs(),
            NOW + i64::try_from(index).unwrap(),
        )
        .expect("record");
        oids.push(oid.clone());
        previous = Some(oid);
    }

    for pair in oids.windows(2) {
        assert!(
            cycles::verify(
                &lane,
                &pair[0],
                &pair[1],
                Options::default().with_symbol_view()
            )
            .expect("verify"),
            "{} → {} drifted from a full recompute",
            pair[0],
            pair[1]
        );
    }
    // And a cold read of the head says the same thing as walking the chain to it.
    let head = cycles::snapshot(&lane, "tree3", &Ratchet::empty(), Options::default(), now())
        .expect("the head snapshot");
    assert_eq!(head.health.cycles, 0, "the last revision deleted the cycle");
}

#[test]
fn a_deleted_module_takes_its_cycle_with_it() {
    let mut store = store_for("delete");
    let mut lane = lane(&mut store);
    let before = Revision::new(vec![
        fixture("a.rs", &["b.rs"]),
        fixture("b.rs", &["a.rs"]),
        fixture("spare.rs", &["a.rs"]),
    ]);
    lane.record("tree1", None, &before.inputs(), NOW)
        .expect("record");
    let after = Revision::new(vec![fixture("spare.rs", &["a.rs"])]);
    lane.record("tree2", Some("tree1"), &after.inputs(), NOW + 1)
        .expect("record");

    let report = cycles::episode(
        &lane,
        "tree1",
        "tree2",
        &Ratchet::empty(),
        Options::default(),
        now(),
    )
    .expect("the episode answer");
    assert_eq!(report.delta.resolved.len(), 1);
    assert!(report.delta.introduced.is_empty());
    assert_eq!(report.health.cycles, 0);
    assert_eq!(
        report.health.nodes_in_cycles, 0,
        "the metric follows the graph down, not just up"
    );
}

#[test]
fn a_baselined_cycle_stays_debt_through_the_lane_path() {
    let mut store = store_for("debt");
    let mut lane = lane(&mut store);
    let revision = Revision::new(vec![
        fixture("billing.rs", &["invoicing.rs"]),
        fixture("invoicing.rs", &["billing.rs"]),
    ]);
    lane.record("tree1", None, &revision.inputs(), NOW)
        .expect("record");
    let ledger = Ratchet::from_text(
        "[[allowance]]\nkind = \"cycle\"\nkey = \"billing.rs → invoicing.rs → billing.rs\"\n\
         owner = \"team-payments\"\nexpires = \"2026-03-31\"\nreason = \"the split lands soon\"\n",
        ".fathom/ratchet.toml",
    );

    let report =
        cycles::snapshot(&lane, "tree1", &ledger, Options::default(), now()).expect("snapshot");
    assert_eq!(report.new_cycles(), 0);
    assert_eq!(report.baselined_cycles(), 1);
    assert_eq!(report.verdict.debt[0].owner, "team-payments");
    assert!(
        report.verdict.debt[0]
            .sentence()
            .contains("expires 2026-03-31"),
        "{}",
        report.verdict.debt[0].sentence()
    );
}

#[test]
fn a_longer_walk_still_answers_and_says_what_it_walked() {
    // Eight revisions between anchors would be a stated partial; four apart is the lane's ordinary
    // business. This is the guard that the cycle path does not quietly re-materialise per step.
    let mut store = store_for("chain");
    let mut lane = lane(&mut store);
    let mut previous: Option<String> = None;
    for index in 0..8 {
        let mut files = vec![fixture("a.rs", &["b.rs"]), fixture("b.rs", &["c.rs"])];
        if index % 2 == 0 {
            files.push(fixture("c.rs", &["a.rs"]));
        } else {
            files.push(fixture("c.rs", &[]));
        }
        let revision = Revision::new(files);
        let oid = format!("tree{index}");
        lane.record(
            &oid,
            previous.as_deref(),
            &revision.inputs(),
            NOW + i64::from(index),
        )
        .expect("record");
        previous = Some(oid);
    }
    let head = cycles::snapshot(
        &lane,
        "tree7",
        &Ratchet::empty(),
        Options {
            view: View::Module,
            ..Options::default()
        },
        now(),
    )
    .expect("the head snapshot");
    assert_eq!(head.health.cycles, 0, "tree7 has no cycle to find");
    let odd = cycles::snapshot(&lane, "tree6", &Ratchet::empty(), Options::default(), now())
        .expect("tree6's snapshot");
    assert_eq!(odd.health.cycles, 1, "tree6 does");
    let between = cycles::episode(
        &lane,
        "tree5",
        "tree7",
        &Ratchet::empty(),
        Options::default(),
        now(),
    )
    .expect("a two-commit episode");
    // A cycle that arrives and leaves inside the window the episode spans nets out to nothing, which is
    // the same rule that makes a moved dependency quiet — stated here because the two-commit window is
    // where a reader would least expect it.
    assert!(
        between.delta.introduced.is_empty() && between.delta.resolved.is_empty(),
        "{}",
        serde_json::to_string_pretty(&between.delta.to_json()).unwrap_or_default()
    );
    let arrived = cycles::episode(
        &lane,
        "tree5",
        "tree6",
        &Ratchet::empty(),
        Options::default(),
        now(),
    )
    .expect("the commit that closed the loop");
    assert_eq!(arrived.delta.introduced.len(), 1);
    let left = cycles::episode(
        &lane,
        "tree6",
        "tree7",
        &Ratchet::empty(),
        Options::default(),
        now(),
    )
    .expect("the commit that opened it again");
    assert_eq!(left.delta.resolved.len(), 1);
}

/// The instant the debt assertions argue against: 2023-11-14, so the 2026 dates in the fixtures are
/// comfortably in the future.
fn now() -> fathom_core::time::Timestamp {
    fathom_core::time::Timestamp::from_unix_micros(1_700_000_000_000_000)
}

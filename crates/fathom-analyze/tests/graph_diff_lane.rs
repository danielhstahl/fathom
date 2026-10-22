//! The architecture delta over a real store: the four rungs, the constant-time empty answer, and the
//! claim that every rung says the same thing (`e3.3`, `reco-8wg`).
//!
//! `s3`'s policy is worth nothing if the cheap answer and the expensive answer disagree, and the
//! ticket's measurement claim — a SELECT over stored delta rows where a rebuilding representation
//! pays two materialisations — is worth nothing if the cheap answer is a worse answer. So this file
//! asserts three things about the same store:
//!
//! 1. **each rung answers at the cost it claims** — one delta row set for an adjacent pair, a run of
//!    them for a span, no chunk decoded unless the pair straddles an anchor;
//! 2. **the constant-time "no architectural change" is constant** — one snapshot row, zero delta
//!    rows, zero materialisations, on a repository the size of a real one;
//! 3. **every rung produces the same document** for the same pair — the six findings identical, with
//!    the one exception that only a rung which read a revision may state that an edge was dead or
//!    that two arrows now point at each other.
//!
//! Plus the churn window, which is the one question in this ticket a stored chain answers better
//! than a snapshot store could: a count over rows, one SELECT per revision, no reconstruction.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::graph::{
    self, ArchitectureDelta, Band, Builder, GraphPolicy, ModuleInput, Rung,
};
use fathom_analyze::structure::{
    Complexity, Edge, EdgeKind, FileLevel, FileStructure, Granularity, Language, Resolution, Span,
    Structure, Symbol, SymbolKind, Visibility,
};
use fathom_store::spine::Store;

/// A fixture module: path, exports, dependencies as `(target, symbol)`.
#[derive(Debug, Clone)]
struct Fixture {
    path: String,
    exports: Vec<String>,
    deps: Vec<(String, String)>,
    /// A note that changes the blob and nothing the graph can see.
    noise: String,
}

fn fixture(path: &str, exports: &[&str], deps: &[(&str, &str)]) -> Fixture {
    Fixture {
        path: path.to_string(),
        exports: exports.iter().map(|name| (*name).to_string()).collect(),
        deps: deps
            .iter()
            .map(|(target, symbol)| (target.to_string(), symbol.to_string()))
            .collect(),
        noise: String::new(),
    }
}

impl Fixture {
    fn blob(&self) -> String {
        let mut parts: Vec<String> = self.exports.clone();
        parts.extend(self.deps.iter().map(|(t, s)| format!("{t}:{s}")));
        parts.push(self.noise.clone());
        format!("blob:{}", parts.join(","))
    }
}

fn structure_of(fixture: &Fixture) -> FileStructure {
    let span = Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    FileStructure {
        path: fixture.path.clone(),
        language: Language::Rust,
        file_level: FileLevel {
            path: fixture.path.clone(),
            language: Language::Rust,
            bytes: 10,
            newlines: 1,
            blank_lines: 0,
            code_lines: 1,
            parse_errors: 0,
            refused: false,
        },
        structure: Some(Structure {
            symbols: fixture
                .exports
                .iter()
                .map(|name| Symbol {
                    kind: SymbolKind::Fn,
                    name: name.clone(),
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
            exports: fixture.exports.clone(),
            imports: Vec::new(),
            edges: fixture
                .deps
                .iter()
                .map(|(target, symbol)| Edge {
                    target: target.clone(),
                    symbol: symbol.clone(),
                    kind: if symbol == "*" {
                        EdgeKind::Wildcard
                    } else {
                        EdgeKind::Resolved
                    },
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
        }),
        reason: None,
    }
}

/// A revision's live file list, owned so the borrow ends where the statement does.
#[derive(Debug, Clone)]
struct Revision {
    files: Vec<Fixture>,
    structures: Vec<FileStructure>,
    blobs: Vec<String>,
}

impl Revision {
    fn new(files: Vec<Fixture>) -> Self {
        let structures = files.iter().map(structure_of).collect();
        let blobs = files.iter().map(Fixture::blob).collect();
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

    fn changed(mut self, path: &str, f: impl FnOnce(Fixture) -> Fixture) -> Self {
        if let Some(index) = self.files.iter().position(|slot| slot.path == path) {
            let fresh = f(self.files[index].clone());
            self.files[index] = fresh;
        }
        self.rebuild()
    }

    fn minus(mut self, path: &str) -> Self {
        self.files.retain(|file| file.path != path);
        self.rebuild()
    }

    fn plus(mut self, extra: Fixture) -> Self {
        self.files.push(extra);
        self.rebuild()
    }

    /// A commit that touched a body and nothing the graph can see: new blob, same rows.
    fn reworded(mut self, path: &str) -> Self {
        for slot in &mut self.files {
            if slot.path == path {
                slot.noise = format!("reworded-{}", slot.exports.len());
            }
        }
        self.rebuild()
    }

    fn rebuild(mut self) -> Self {
        self.structures = self.files.iter().map(structure_of).collect();
        self.blobs = self.files.iter().map(Fixture::blob).collect();
        self
    }
}

const NOW: i64 = 2_000_000_000_000;

/// The anchor period shortened to 4, so a test can reach every rung in eight revisions.
fn policy() -> GraphPolicy {
    GraphPolicy {
        chain_full_every: 4,
        max_chain_walk: 4,
        ..GraphPolicy::default()
    }
}

fn builder() -> Builder {
    Builder {
        policy: policy(),
        layers: graph::Layers::new([
            (String::from("core"), String::from("l0")),
            (String::from("state"), String::from("l1")),
            (String::from("app"), String::from("l2")),
        ]),
    }
}

fn base() -> Revision {
    Revision::new(vec![
        fixture("core/shape.rs", &["Shape"], &[]),
        fixture("core/circle.rs", &["Circle"], &[("core/shape.rs", "Shape")]),
        fixture(
            "state/store.rs",
            &["Store"],
            &[("core/circle.rs", "Circle")],
        ),
        fixture("app/main.rs", &["main"], &[("state/store.rs", "Store")]),
    ])
}

/// Nine revisions, one commit each: an export appearing, a module going, a module arriving, a body
/// reworded twice, and a symbol moving — the shapes a real history is made of. The two reworded
/// steps matter: one of them lands on an anchor and one on a delta row, and the cheap "did this
/// commit change the architecture" answer can only speak about the second.
fn history() -> Vec<Revision> {
    let mut out = vec![base()];
    let steps: Vec<fn(Revision) -> Revision> = vec![
        |r| r.changed("core/circle.rs", |f| f),
        |r| {
            r.changed("core/shape.rs", |_| {
                fixture("core/shape.rs", &["Shape", "Area"], &[])
            })
        },
        |r| {
            r.changed("app/main.rs", |_| {
                fixture("app/main.rs", &["main"], &[("state/store.rs", "Store")])
            })
        },
        |r| r.reworded("core/circle.rs"),
        |r| {
            r.minus("state/store.rs")
                .changed("app/main.rs", |_| fixture("app/main.rs", &["main"], &[]))
        },
        |r| {
            r.plus(fixture(
                "core/geo.rs",
                &["Circle"],
                &[("core/shape.rs", "Shape")],
            ))
            .changed("core/circle.rs", |_| fixture("core/circle.rs", &[], &[]))
        },
        |r| {
            r.changed("core/geo.rs", |_| {
                fixture("core/geo.rs", &["Circle"], &[("core/shape.rs", "*")])
            })
        },
        |r| {
            r.plus(fixture(
                "state/cache.rs",
                &["Cache"],
                &[("core/geo.rs", "Circle")],
            ))
        },
        // A reword with no anchor to hide behind: revision 9 is a delta row (the period is 4, so 0,
        // 4 and 8 are the anchors), and it moves not one graph row.
        |r| r.reworded("core/shape.rs"),
    ];
    for step in steps {
        let previous = out.last().cloned().expect("non-empty history");
        out.push(step(previous));
    }
    out
}

/// Replay a history into a fresh store; returns the store and each revision's tree oid.
fn replay(history: &[Revision]) -> (Store, Vec<String>) {
    let mut store = Store::open_in_memory().expect("open store");
    let mut oids: Vec<String> = Vec::new();
    {
        let mut lane = graph::Lane::new(&mut store, builder()).expect("lane");
        for (index, revision) in history.iter().enumerate() {
            let oid = format!("tree{index:04}");
            let base = if index == 0 {
                None
            } else {
                Some(oids[index - 1].as_str())
            };
            lane.record(
                &oid,
                base,
                &revision.inputs(),
                NOW + i64::try_from(index).unwrap_or(0),
            )
            .expect("record");
            oids.push(oid);
        }
        drop(lane);
    }
    (store, oids)
}

/// A lane over a store that already holds a history, with no cursor in hand — the cold reader, which
/// is what an MCP call or a CLI verb is.
fn lane(store: &mut Store) -> graph::Lane<'_> {
    graph::Lane::new(store, builder()).expect("lane")
}

/// A store with a directory of its own.
///
/// The only kind that can hold an oversized payload: a 160-module anchor manifest is a file-class
/// blob, and an in-memory store refuses one rather than pretending it fit. `Store::open` beside a
/// working tree is what the CLI actually does, so this is also the more realistic of the two; the
/// directory is removed again on the way out because a test that litters `/tmp` is a test that
/// confuses the next one.
fn on_disk_store(label: &str) -> (Store, std::path::PathBuf) {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-graph-diff-{label}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the store directory");
    let store = Store::open(&dir.join("index.sqlite3")).expect("the store opens");
    (store, dir)
}

/// The same document from two different costs, with the things allowed to differ taken out: the
/// cost itself, and the extra knowledge only a rung that read a revision has.
fn comparable(mut delta: ArchitectureDelta) -> ArchitectureDelta {
    delta.cost = graph::Cost::in_hand();
    delta.adjacent = false;
    delta.notices.clear();
    for edge in &mut delta.edges_added {
        edge.dead = None;
        edge.dangling = None;
    }
    for edge in &mut delta.edges_removed {
        edge.dead = None;
        edge.dangling = None;
    }
    for reversal in &mut delta.reversals {
        reversal.mutual = None;
    }
    for row in &mut delta.churn {
        // `edges_now` is the churn row's denominator, and it is a fact about a *revision*, not
        // about a set of ops: the rungs that read stored rows have not seen one, and an answer
        // that invented the number would be inventing the one thing the band depends on.
        row.edges_now = None;
    }
    delta
}

/// The endpoint document, computed from two materialisations, which every rung must match.
fn endpoint(lane: &graph::Lane<'_>, from: &str, to: &str) -> ArchitectureDelta {
    let (before, _) = lane.materialise(from).expect("materialise from");
    let (after, _) = lane.materialise(to).expect("materialise to");
    let view = lane.naming().expect("naming");
    ArchitectureDelta::between(&before, &after, &view, from, to)
}

// ---------------------------------------------------------------------------------------------
// Each rung answers at the cost it claims.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_adjacent_pair_is_answered_by_one_stored_delta() {
    let history = history();
    let (mut store, oids) = replay(&history);
    let lane = lane(&mut store);
    let answer = lane
        .architecture_delta(&oids[1], &oids[2])
        .expect("adjacent delta");
    assert_eq!(answer.cost.rung, Rung::Adjacent);
    assert!(answer.adjacent, "the store says `to`'s base is `from`");
    assert_eq!(answer.cost.materialisations, 0);
    assert_eq!(answer.cost.deltas, 1, "one delta row set, one question");
    assert!(
        !answer.is_empty(),
        "revision 2 added an export: {:?}",
        answer.counts()
    );
    assert!(
        answer
            .surface
            .iter()
            .any(|change| change.module == "core/shape.rs" && change.added == vec!["Area"]),
        "{:?}",
        answer.surface
    );
    assert_eq!(
        comparable(answer.clone()),
        comparable(endpoint(&lane, &oids[1], &oids[2])),
        "the cheap answer is not a different answer"
    );
}

#[test]
fn a_span_inside_one_chain_is_composed_from_stored_deltas_without_a_rebuild() {
    let history = history();
    let (mut store, oids) = replay(&history);
    let lane = lane(&mut store);
    // 1 → 3: two stored deltas between them, well inside the anchor period.
    let answer = lane
        .architecture_delta(&oids[1], &oids[3])
        .expect("span delta");
    assert_eq!(answer.cost.rung, Rung::Chain, "{:?}", answer.cost);
    assert_eq!(answer.cost.materialisations, 0, "no chunk was decoded");
    assert_eq!(
        answer.cost.deltas, 2,
        "the two deltas between the endpoints"
    );
    assert_eq!(
        comparable(answer),
        comparable(endpoint(&lane, &oids[1], &oids[3])),
        "the composition must be the span, not an approximation of it"
    );
}

#[test]
fn two_revisions_on_one_anchor_are_answered_without_materialising_either() {
    let history = history();
    let (mut store, oids) = replay(&history);
    let lane = lane(&mut store);
    // Backwards: 3 → 1. `from` is not on `to`'s chain at all, but they share an anchor, so the
    // answer is that anchor-to-`from` run inverted, composed with the anchor-to-`to` run.
    let answer = lane
        .architecture_delta(&oids[3], &oids[1])
        .expect("backwards pair");
    assert_eq!(answer.cost.rung, Rung::SharedAnchor, "{:?}", answer.cost);
    assert_eq!(answer.cost.materialisations, 0);
    assert!(
        answer
            .notices
            .iter()
            .any(|notice| notice.contains("neither revision was materialised")),
        "{:?}",
        answer.notices
    );
    assert_eq!(
        comparable(answer),
        comparable(endpoint(&lane, &oids[3], &oids[1])),
        "and the inversion composed to the same span"
    );
}

#[test]
fn a_pair_that_straddles_an_anchor_pays_two_materialisations_and_says_so() {
    let history = history();
    let (mut store, oids) = replay(&history);
    let lane = lane(&mut store);
    // 1 → 5: 1 sits under anchor 0, 5 under anchor 4. Nothing stored connects them, and the answer
    // admits the price rather than pretending otherwise.
    let answer = lane
        .architecture_delta(&oids[1], &oids[5])
        .expect("straddling pair");
    assert_eq!(answer.cost.rung, Rung::Materialised, "{:?}", answer.cost);
    assert_eq!(answer.cost.materialisations, 2);
    assert!(
        answer.sentence().contains("materialised"),
        "{}",
        answer.sentence()
    );
    // And the expensive rung is the one allowed to make the extra claims, because it read both
    // revisions: dead edges and mutual arrows are decidable only from a graph.
    assert!(
        answer.edges_removed.iter().all(|edge| edge.dead.is_some())
            && !answer.edges_removed.is_empty(),
        "a rung that read the revision can say whether each removed edge was live: {:?}",
        answer.edges_removed
    );
    let cheap = lane
        .architecture_delta(&oids[4], &oids[5])
        .expect("adjacent");
    assert!(
        cheap
            .notices
            .iter()
            .any(|notice| notice.contains("is not claimed")),
        "and the cheap rung has to say what it will not guess: {:?}",
        cheap.notices
    );
}

#[test]
fn every_rung_disagrees_about_the_cost_and_agrees_about_the_facts() {
    let history = history();
    let (mut store, oids) = replay(&history);
    let lane = lane(&mut store);
    for (from, to) in [
        (1_usize, 2_usize),
        (1, 3),
        (3, 1),
        (0, 2),
        (1, 5),
        (2, 6),
        (0, 7),
    ] {
        let answer = lane
            .architecture_delta(&oids[from], &oids[to])
            .expect("delta");
        let endpoint = endpoint(&lane, &oids[from], &oids[to]);
        assert_eq!(
            comparable(answer),
            comparable(endpoint),
            "rung mismatch on {}→{}",
            oids[from],
            oids[to]
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The constant-time empty answer.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_commit_that_changed_no_structure_is_named_unchanged_at_a_constant_cost() {
    let history = history();
    let (mut store, oids) = replay(&history);
    // The last revision reworded `core/shape.rs`: a new blob, and not one graph row moved. It lands
    // on a delta row rather than on an anchor, and that is the whole point — an anchor writes no
    // delta rows, so the constant-time test has nothing to read about one (asserted below).
    let from = &oids[8];
    let to = &oids[9];
    let lane = lane(&mut store);
    assert_eq!(
        lane.architectural_change(from, to).expect("cheap test"),
        Some(false),
        "the cheap test knows"
    );
    let snap = lane
        .read()
        .graph_snapshot(to)
        .expect("read")
        .expect("written");
    assert_eq!(
        snap.bytes, 0,
        "the writer recorded that it staged no rows — that column is the whole trick"
    );
    assert_eq!(
        lane.read().graph_ops(to).expect("ops").len(),
        0,
        "and there really are no rows"
    );
    let answer = lane.architecture_delta(from, to).expect("empty delta");
    assert_eq!(answer.cost.rung, Rung::Empty);
    assert_eq!(answer.cost.delta_rows, 0, "the delta was not read");
    assert_eq!(answer.cost.materialisations, 0, "and neither was the graph");
    assert!(answer.is_empty());
    assert!(
        answer.sentence().contains("nothing architectural changed"),
        "{}",
        answer.sentence()
    );

    // The other side of the same test: a revision that did move something answers `true`, and does
    // it without reading a delta row either.
    assert_eq!(
        lane.architectural_change(&oids[1], &oids[2])
            .expect("cheap test"),
        Some(true)
    );
    assert!(
        lane.read()
            .graph_snapshot(&oids[2])
            .expect("read")
            .expect("written")
            .bytes
            > 0,
        "the byte column is what lets the cheap answer skip the delta entirely"
    );
    // A non-adjacent pair is not an answer of "no".
    assert_eq!(
        lane.architectural_change(&oids[0], &oids[3])
            .expect("cheap test"),
        None,
        "the cheap test only speaks about adjacent revisions"
    );
    // An anchor has no base at all, so it cannot be "unchanged from" anything.
    assert_eq!(
        lane.architectural_change(&oids[0], &oids[0])
            .expect("same revision"),
        None
    );
    let same = lane
        .architecture_delta(&oids[2], &oids[2])
        .expect("same revision delta");
    assert!(same.is_empty() && !same.adjacent);
}

#[test]
fn the_empty_answer_costs_the_same_on_a_big_graph_as_on_a_small_one() {
    // "Constant time" has to mean something beyond a stopwatch in a test: the empty answer must read
    // no delta rows and no chunks, and must not touch the graph at all, so there is nothing in it
    // that could scale. The same no-structure commit is run on a four-module store and on a store
    // forty times the size, and the recorded cost is identical. The measured version of the claim —
    // a p95 against `query.graph_delta_adjacent.ms` — is in the harness report.
    let (small_cost, small_nodes) = {
        let mut store = Store::open_in_memory().expect("store");
        let one = Revision::new(vec![fixture("core/a.rs", &["A"], &[])]);
        {
            let mut lane = graph::Lane::new(&mut store, builder()).expect("lane");
            lane.record("small0", None, &one.inputs(), NOW)
                .expect("record");
            let two = one.reworded("core/a.rs");
            lane.record("small1", Some("small0"), &two.inputs(), NOW + 1)
                .expect("record");
        }
        let lane = lane(&mut store);
        let delta = lane.architecture_delta("small0", "small1").expect("delta");
        let nodes = store
            .graph_snapshot("small0")
            .expect("read")
            .expect("written")
            .node_count;
        (delta.cost, nodes)
    };
    let (big_cost, big_nodes) = {
        // A directory, not a memory: this store's anchor manifest is bigger than the inline limit,
        // and "the empty answer does not scale with the graph" has to hold on the store shape that
        // actually gets a 160-module repository, which is the one on disk.
        let (mut store, dir) = on_disk_store("big");
        let files: Vec<Fixture> = (0usize..160)
            .map(|index| {
                let path = format!("core/m{index}.rs");
                let symbol = format!("S{index}");
                let previous = format!("core/m{}.rs", index.saturating_sub(1));
                fixture(&path, &[symbol.as_str()], &[(previous.as_str(), "S0")])
            })
            .collect();
        let one = Revision::new(files);
        {
            let mut lane = graph::Lane::new(&mut store, builder()).expect("lane");
            lane.record("big0", None, &one.inputs(), NOW)
                .expect("record");
            let two = one.reworded("core/m42.rs");
            lane.record("big1", Some("big0"), &two.inputs(), NOW + 1)
                .expect("record");
        }
        let delta = {
            let lane = lane(&mut store);
            let delta = lane.architecture_delta("big0", "big1").expect("delta");
            drop(lane);
            delta
        };
        let nodes = store
            .graph_snapshot("big0")
            .expect("read")
            .expect("written")
            .node_count;
        // The directory goes only after every handle to it is closed; a leftover file is worse than
        // a skipped cleanup, so the drop is written rather than left to the end of the block.
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
        (delta.cost, nodes)
    };
    assert!(
        big_nodes > small_nodes * 20,
        "the second store really is far bigger: {small_nodes} against {big_nodes}"
    );
    assert_eq!(
        small_cost, big_cost,
        "and the empty answer paid the same in both: {small_cost:?} against {big_cost:?}"
    );
    assert_eq!(big_cost, graph::Cost::empty());
    assert_eq!(
        big_cost.delta_rows, 0,
        "not one delta row read, so there is nothing here to scale"
    );
    assert_eq!(
        big_cost.materialisations, 0,
        "and no graph, which is the only other thing that could"
    );
}

// ---------------------------------------------------------------------------------------------
// Churn, from stored rows.
// ---------------------------------------------------------------------------------------------

#[test]
fn churn_over_a_window_names_the_module_that_keeps_getting_reworked() {
    // One wire re-cut in four revisions of a window, and one module whose edges all move at once in
    // a fifth: the same number of edge events, opposite findings.
    let (mut store, oids) = replay(&history_reworked());
    let lane = lane(&mut store);
    let report = lane.churn_recent(oids.len()).expect("churn recent");
    assert_eq!(report.window, oids.len(), "the denominator is the window");
    assert_eq!(
        report.revisions.len(),
        oids.len(),
        "and it is reproducible from the report"
    );
    let hot = report
        .find("core/touched.rs")
        .expect("the module on the receiving end");
    assert_eq!(hot.band, Band::Reworked, "{hot:?}");
    assert!(hot.revisions >= 3, "touched in most of the window: {hot:?}");
    assert_eq!(
        hot.window, report.window,
        "every row carries its own denominator"
    );
    // The list a pane leads with is ordered by churn and then by name, and the tie here is the point:
    // `core/hot.rs` and `core/touched.rs` moved exactly as many edges across the window, because they
    // are the two ends of one wire that was re-cut four times. What separates this pair from
    // `core/onceloud.rs` is repetition, not size — so the module that moved once, loudly, and moved
    // more edges than either of them in a single commit, is what this window leads with *last*.
    assert!(
        report
            .hot
            .iter()
            .filter(|lead| **lead == "core/hot.rs" || **lead == "core/touched.rs")
            .count()
            == 2,
        "both ends of the re-cut wire are the finding: {}",
        report.sentence()
    );
    assert_eq!(
        report.hot.last().map(String::as_str),
        Some("core/onceloud.rs"),
        "the list a pane leads with does not lead with the one big commit: {}",
        report.sentence()
    );
    let onceloud = report
        .find("core/onceloud.rs")
        .expect("the module that moved once, loudly");
    assert_ne!(
        onceloud.band,
        Band::Reworked,
        "one commit is not a habit: {onceloud:?}"
    );
    assert!(
        onceloud.churn >= 2,
        "it did move edges, which is why the band is about repetition and not size: {onceloud:?}"
    );
    // A window is a slice: asking about the last three revisions must not report the whole history.
    let tail: Vec<String> = oids[oids.len() - 3..].to_vec();
    let short = lane.churn(&tail).expect("short window");
    assert_eq!(short.window, 3);
    assert!(
        short.revisions == tail,
        "and it names the revisions it measured"
    );
    assert!(
        short
            .per_module
            .iter()
            .all(|row| row.revisions <= short.window),
        "no module can be touched more often than the window has revisions"
    );
    // The document form carries the bands, so an exporter never re-derives them.
    let read = graph::ChurnReport::from_json(&report.to_json()).expect("round trip");
    assert_eq!(read, report);
    assert!(
        report.sentence().contains("reworked"),
        "{}",
        report.sentence()
    );
}

/// The same history the churn test needs, spelled out: four revisions re-cutting one wire.
fn history_reworked() -> Vec<Revision> {
    let mut out = vec![Revision::new(vec![
        fixture("core/hot.rs", &["Hot"], &[]),
        fixture("core/touched.rs", &["Touched"], &[]),
        fixture("core/onceloud.rs", &["OnceLoud"], &[]),
    ])];
    for step in 0..4 {
        let previous = out.last().cloned().expect("non-empty");
        let next = if step % 2 == 0 {
            previous.changed("core/hot.rs", |_| {
                fixture("core/hot.rs", &["Hot"], &[("core/touched.rs", "Touched")])
            })
        } else {
            previous.changed("core/hot.rs", |_| fixture("core/hot.rs", &["Hot"], &[]))
        };
        out.push(next);
    }
    // One revision that moves a great many edges at once, in a module nobody else reworks.
    out.push(
        out.last()
            .cloned()
            .expect("non-empty")
            .changed("core/onceloud.rs", |_| {
                fixture(
                    "core/onceloud.rs",
                    &["OnceLoud"],
                    &[("core/hot.rs", "Hot"), ("core/touched.rs", "Touched")],
                )
            }),
    );
    out
}

#[test]
fn a_store_that_has_never_seen_a_revision_is_refused_rather_than_answered_as_empty() {
    let (mut store, oids) = replay(&history());
    let lane = lane(&mut store);
    let missing = lane.architecture_delta(&oids[0], "tree9999");
    assert!(
        missing.is_err(),
        "no such revision, and no empty delta pretending to be one"
    );
    let partial = missing.expect_err("the partial");
    assert!(partial.code().starts_with("graph.chain"), "{partial}");
}

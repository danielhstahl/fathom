//! `e3.2` (`reco-8s4`) — the graph chain's measurement harness.
//!
//! The ticket's storage policy is a number: one delta per revision plus a shared anchor every
//! `graph.chain_full_every` was chosen because spike `s3` measured it at 22 335 B/revision against a
//! 32 KiB budget (`docs/concepts/graph-storage.md`, ADR-004 §13.7). A policy adopted from a spike and
//! then never re-measured is a policy that quietly stops being true, and `s3` measured a *prototype*
//! in Python with ids not interned — the `e3.2` half of the same claim (node-id interning, "expected
//! to bring the real figure near 8 KiB") had never been spent, let alone measured.
//!
//! ```text
//! # measure the shipped lane and write the committed report
//! cargo run --release -p fathom-analyze --features sqlite-bundled --example graph_harness -- \
//!     --report benchmarks/reports/e3-2-graph-chain.json
//!
//! # the gate: the committed report still matches a fresh run (bytes and chain shape, never timings)
//! cargo run --release -p fathom-analyze --features sqlite-bundled --example graph_harness -- \
//!     --report benchmarks/reports/e3-2-graph-chain.json --check
//!
//! # the repository shape `s3` measured against
//! cargo run --release -p fathom-analyze --features sqlite-bundled --example graph_harness -- \
//!     --modules 2300 --revisions 1000
//! ```
//!
//! # Why the corpus is synthetic
//!
//! `s3`'s own corpus was synthetic for the same reason: the quantity under test is bytes per revision
//! as a function of *repository shape* — modules, exports, fan-out, files touched per commit — and a
//! real repository confounds that shape with whatever its history happened to do. The generator takes
//! the same four parameters `s3` swept, so a number here is comparable to a number there. Timings are
//! reported with their budget verdict and are never blessed by `--check`, for the reason
//! `structure_harness` gives: a busy runner is not a regression.
//!
//! Bytes are the payload basis (row text plus blob payloads), which is the basis `s3` measured and
//! therefore the basis `store.bytes_per_commit_graph.*` means. Store pages are a different quantity
//! and are reported beside it, labelled.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::unwrap_used,
    clippy::expect_used
)]

use fathom_analyze::graph::{
    self, Builder, GraphPolicy, Lane, ModuleInput, dependencies_of, dependents_of,
    path as path_query,
};
use fathom_analyze::structure::{
    Complexity, Edge, EdgeKind, FileLevel, FileStructure, Language, Resolution, Span, Structure,
    Symbol, SymbolKind, Visibility,
};
use fathom_store::spine::Store;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The default corpus: `s3`'s reference shape, a 2 300-module repository.
const DEFAULT_MODULES: usize = 2_300;
/// The default history length, in revisions.
const DEFAULT_REVISIONS: usize = 500;
/// How many times each query is repeated for a percentile, per sampled revision.
const QUERY_REPEATS: usize = 20;
/// How many revisions the timing pass samples.
const TIMED_REVISIONS: usize = 40;

/// A deterministic generator: a xorshift over `usize`.
///
/// `rand` is not a dependency of this crate and a measurement corpus must not be random between runs
/// anyway — `--check` compares a committed report, and a corpus that drifted would make the gate
/// compare two different repositories.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).unwrap_or(1)).unwrap_or(0)
    }
}

/// One module of the synthetic repository.
#[derive(Debug, Clone)]
struct Module {
    path: String,
    exports: Vec<String>,
    /// `(target path, symbol)`; the symbol is `*` for the wildcard share.
    deps: Vec<(String, String)>,
}

/// The corpus: modules, and the history of revisions over them.
struct Corpus {
    modules: Vec<Module>,
    /// Each revision is the full live file list, as the substrate would hand it over.
    history: Vec<Vec<Module>>,
    shape: Value,
}

/// The layer tags the synthetic repository is arranged in, mirroring a real Fathom-shaped tree.
const LAYERS: [(&str, &str); 5] = [
    ("l0", "core"),
    ("l1", "util"),
    ("l2", "state"),
    ("l3", "api"),
    ("tests", "core/tests"),
];

/// Build the corpus.
fn generate(modules: usize, revisions: usize, fan_out: usize, seed: u64) -> Corpus {
    let mut rng = Lcg(seed | 1);
    let mut corpus: Vec<Module> = Vec::with_capacity(modules);
    for index in 0..modules {
        // A directory per layer, and a tenth of the tree carved out as tests, so a layer subgraph has
        // a boundary that is not the whole tree.
        let layer = if index % 10 == 9 {
            LAYERS[4]
        } else {
            LAYERS[index % 4]
        };
        let path = format!("{}/module{index}.rs", layer.1);
        let exports: Vec<String> = (0..1 + rng.below(3))
            .map(|s| format!("S{index}_{s}"))
            .collect();
        // Dependencies point backwards, so the graph is a DAG and a path query has something to find.
        let mut deps = Vec::new();
        for _ in 0..fan_out.min(index) {
            let target = rng.below(index);
            let symbol = if rng.below(10) == 0 {
                String::from("*")
            } else {
                corpus[target]
                    .exports
                    .get(rng.below(corpus[target].exports.len()))
                    .cloned()
                    .unwrap_or_default()
            };
            deps.push((corpus[target].path.clone(), symbol));
        }
        corpus.push(Module {
            path,
            exports,
            deps,
        });
    }

    let mut history = Vec::with_capacity(revisions);
    let mut live = corpus.clone();
    history.push(live.clone());
    for step in 1..revisions {
        // One to three files per commit, which is the shape of a real commit and the unit the
        // marginal-cost budget is written against.
        let touches = 1 + rng.below(3);
        for _ in 0..touches {
            let index = rng.below(live.len().max(1));
            if live.is_empty() {
                break;
            }
            match rng.below(3) {
                0 => {
                    let name = format!("Extra{step}");
                    live[index].exports.push(name);
                }
                1 => {
                    let target = rng.below(index.max(1));
                    // Read both halves before writing either: the target of the new edge may be the
                    // module being edited, and `Vec` will not hand out two mutable slices of itself.
                    let path = live[target].path.clone();
                    let symbol = live[target]
                        .exports
                        .get(rng.below(live[target].exports.len()))
                        .cloned()
                        .unwrap_or_default();
                    let module = &mut live[index];
                    module.deps.push((path, symbol));
                }
                _ => {
                    // A deletion, which is the only way a removal op ever reaches the chain.
                    if live.len() > modules / 2 {
                        live.remove(index);
                    }
                }
            }
        }
        history.push(live.clone());
    }

    let shape = json!({
        "modules": modules,
        "revisions": revisions,
        "fan_out": fan_out,
        "seed": seed,
        "layers": LAYERS.iter().map(|(name, pattern)| json!([name, pattern])).collect::<Vec<_>>(),
    });
    Corpus {
        modules: corpus,
        history,
        shape,
    }
}

/// The structural record one synthetic module would have produced.
fn file_of(module: &Module) -> FileStructure {
    let span = Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    FileStructure {
        path: module.path.clone(),
        language: Language::Rust,
        file_level: FileLevel {
            path: module.path.clone(),
            language: Language::Rust,
            bytes: 120,
            newlines: 12,
            blank_lines: 2,
            code_lines: 10,
            parse_errors: 0,
            refused: false,
        },
        structure: Some(Structure {
            symbols: module
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
                        body_lines: 4,
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
            exports: module.exports.clone(),
            imports: Vec::new(),
            edges: module
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
                    granularity: fathom_analyze::structure::Granularity::File,
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

/// One revision's hand-off to the lane, owned so the inputs can borrow it.
struct Revision {
    modules: Vec<Module>,
    files: Vec<FileStructure>,
    blobs: Vec<String>,
}

impl Revision {
    fn new(modules: &[Module]) -> Self {
        let files = modules.iter().map(file_of).collect();
        let blobs = modules
            .iter()
            .map(|m| format!("blob:{}|{}|{}", m.path, m.exports.join(","), m.deps.len()))
            .collect();
        Self {
            modules: modules.to_vec(),
            files,
            blobs,
        }
    }

    fn inputs(&self) -> Vec<ModuleInput<'_>> {
        self.modules
            .iter()
            .zip(self.files.iter())
            .zip(self.blobs.iter())
            .map(|((module, file), blob)| ModuleInput {
                path: module.path.as_str(),
                blob: blob.as_str(),
                file: Some(file),
            })
            .collect()
    }
}

/// The percentiles a budget is written against.
#[derive(Debug, Clone)]
struct Timing {
    p50: f64,
    p95: f64,
    max: f64,
    samples: usize,
}

impl Timing {
    fn of(mut samples: Vec<f64>) -> Self {
        if samples.is_empty() {
            return Self {
                p50: 0.0,
                p95: 0.0,
                max: 0.0,
                samples: 0,
            };
        }
        samples.sort_by(f64::total_cmp);
        let pick = |q: f64| {
            let index = ((samples.len() as f64 - 1.0) * q).round() as usize;
            samples[index.min(samples.len() - 1)]
        };
        Self {
            p50: pick(0.50),
            p95: pick(0.95),
            max: *samples.last().unwrap_or(&0.0),
            samples: samples.len(),
        }
    }

    fn json(&self) -> Value {
        json!({
            "p50_ms": (self.p50 * 1000.0).round() / 1000.0,
            "p95_ms": (self.p95 * 1000.0).round() / 1000.0,
            "max_ms": (self.max * 1000.0).round() / 1000.0,
            "samples": self.samples,
        })
    }
}

/// The budgets this harness can be checked against, read from `benchmarks/budgets.json` rather than
/// repeated here — one place for a budget to live.
fn budgets(path: &Path) -> BTreeMap<String, f64> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let mut out = BTreeMap::new();
    if let Some(list) = parsed.get("budgets").and_then(Value::as_array) {
        for entry in list {
            let id = entry.get("id").and_then(Value::as_str);
            let budget = entry.get("budget").and_then(Value::as_f64);
            if let (Some(id), Some(budget)) = (id, budget) {
                out.insert(id.to_string(), budget);
            }
        }
    }
    out
}

/// One verdict line.
fn verdict(id: &str, budget: Option<&f64>, measured: f64, unit: &str) -> Value {
    json!({
        "id": id,
        "budget": budget,
        "measured": (measured * 1000.0).round() / 1000.0,
        "unit": unit,
        "verdict": match budget {
            None => "no budget on record",
            Some(limit) if measured <= *limit => "within",
            Some(_) => "OVER",
        },
    })
}

#[allow(clippy::too_many_lines)]
fn measure(corpus: &Corpus, policy: &GraphPolicy, directory: &Path) -> Value {
    let store_path = directory.join("index.sqlite3");
    let _ = std::fs::remove_dir_all(directory);
    std::fs::create_dir_all(directory).expect("create the harness store directory");
    let mut store = Store::open(&store_path).expect("open the store");
    let builder = Builder {
        policy: policy.clone(),
        layers: graph::Layers::new(
            LAYERS
                .iter()
                .map(|(name, pattern)| (name.to_string(), pattern.to_string())),
        ),
    };
    let mut lane = Lane::new(&mut store, builder).expect("lane");

    // -- the write pass: every revision, exactly as the lane would see it on a cold index ---------
    let mut write_ms = Vec::new();
    let mut anchors = 0_usize;
    let mut ops_written = 0_usize;
    let mut chunks_written = 0_usize;
    let mut previous: Option<String> = None;
    let mut oids = Vec::with_capacity(corpus.history.len());
    for (index, revision) in corpus.history.iter().enumerate() {
        let oid = format!("tree{index:09}");
        let revision = Revision::new(revision);
        let started = Instant::now();
        let summary = lane
            .record(
                &oid,
                previous.as_deref(),
                &revision.inputs(),
                1_700_000_000_000 + i64::try_from(index).unwrap(),
            )
            .expect("record");
        write_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
        anchors += usize::from(summary.is_anchor);
        ops_written += summary.ops;
        chunks_written += summary.new_chunks;
        previous = Some(oid.clone());
        oids.push(oid);
    }
    let bytes = lane.graph_bytes().expect("bytes");
    let store_bytes = lane.usage().expect("store bytes");
    let revisions = i64::try_from(corpus.history.len()).unwrap_or(1);
    let bytes_per_revision = bytes.total() / revisions.max(1);
    let modules_live = lane.naming().expect("naming").len();

    // -- the read pass: materialise, step, and the five queries ---------------------------------
    let step = (corpus.history.len() / TIMED_REVISIONS).max(1);
    let sampled: Vec<usize> = (0..corpus.history.len())
        .step_by(step)
        .take(TIMED_REVISIONS)
        .collect();
    let mut materialise_ms = Vec::new();
    let mut walk_steps = Vec::new();
    let mut undecoded = 0_usize;
    for &index in &sampled {
        let started = Instant::now();
        let (graph, report) = lane.materialise(&oids[index]).expect("materialise");
        materialise_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
        walk_steps.push(report.steps);
        undecoded += report.undecoded;
        drop(graph);
    }

    // The warm path, measured as the ticket defines it: one commit, on a graph already in hand. Each
    // sample materialises a revision — that cost is charged to `materialise`, above — and then steps
    // exactly one commit forward, which is what a keystroke on the cursor pays.
    let mut step_ms = Vec::new();
    let mut stepped = 0_usize;
    let mut onto_anchor = 0_usize;
    for &index in &sampled {
        if index + 1 >= oids.len() {
            continue;
        }
        let (graph, _) = lane
            .materialise(&oids[index])
            .expect("materialise for step");
        let cursor = graph::Cursor {
            tree_oid: oids[index].clone(),
            modules: BTreeMap::new(),
            graph,
        };
        let started = Instant::now();
        let (_, moved) = lane.step(&cursor, &oids[index + 1]).expect("step");
        // One commit forward is a delta unless that commit is an anchor, in which case the honest
        // thing the lane did was load it; the sample counts only the delta cases, because they are
        // what `warm.graph.marginal.ms` is a budget for.
        if moved.kind == graph::StepKind::DeltaApplied {
            step_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
            stepped += 1;
        } else {
            onto_anchor += 1;
        }
    }
    assert!(
        stepped > 0,
        "no sample stepped onto a delta, so the chain shape is not the one being measured"
    );

    let naming = lane.naming().expect("naming");
    let head = oids.last().expect("at least one revision");
    let (head_graph, _) = lane.materialise(head).expect("materialise head");
    let subjects: Vec<String> = naming
        .module_ids("")
        .into_iter()
        .chain(
            corpus
                .modules
                .iter()
                .take(64)
                .map(|m| naming.id(&m.path).unwrap_or(0))
                .filter(|id| *id != 0),
        )
        .map(|id| naming.name(id).unwrap_or_default().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    let subjects: Vec<String> = if subjects.is_empty() {
        corpus
            .modules
            .iter()
            .map(|m| m.path.clone())
            .take(32)
            .collect()
    } else {
        subjects
    };

    let mut deps_ms = Vec::new();
    let mut fans_ms = Vec::new();
    for _ in 0..QUERY_REPEATS {
        for subject in subjects.iter().take(20) {
            let started = Instant::now();
            let _ = dependencies_of(&head_graph, &naming, subject);
            deps_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
            let started = Instant::now();
            let _ = dependents_of(&head_graph, &naming, subject);
            fans_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
        }
    }

    let mut path_ms = Vec::new();
    for pair in subjects
        .iter()
        .take(20)
        .zip(subjects.iter().skip(1).take(20))
    {
        for _ in 0..QUERY_REPEATS {
            let started = Instant::now();
            let _ = path_query(&head_graph, &naming, pair.0, pair.1, policy.path_max_hops);
            path_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
        }
    }

    let mut layer_ms = Vec::new();
    for layer in LAYERS.iter().map(|(name, _)| *name) {
        for _ in 0..QUERY_REPEATS {
            let started = Instant::now();
            let _ = graph::layer_subgraph(&head_graph, &naming, layer);
            layer_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
        }
    }

    // Adjacent deltas are the only changed-edge answer that has a budget of its own, and the only one
    // that is cheap — but an anchor revision has no delta of its own to read, so the sample is the
    // pairs the chain actually stored as deltas.
    let delta_pairs: Vec<(String, String)> = oids
        .windows(2)
        .filter(|pair| {
            lane.read()
                .graph_snapshot(&pair[1])
                .expect("read")
                .is_some_and(|snap| snap.kind == "delta")
        })
        .take(TIMED_REVISIONS)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect();
    let mut delta_ms = Vec::new();
    let mut from_store = true;
    for pair in &delta_pairs {
        for _ in 0..QUERY_REPEATS {
            let started = Instant::now();
            let changed = lane.changed_edges(&pair.0, &pair.1).expect("changed");
            delta_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
            from_store &= changed.from_store;
        }
    }

    let loaded = budgets(Path::new("benchmarks/budgets.json"));
    let verdicts = vec![
        verdict(
            "store.bytes_per_commit_graph.small",
            loaded.get("store.bytes_per_commit_graph.small"),
            bytes_per_revision as f64,
            "bytes",
        ),
        verdict(
            "warm.graph.marginal.ms.small",
            loaded.get("warm.graph.marginal.ms.small"),
            Timing::of(write_ms.clone()).p95,
            "ms",
        ),
        verdict(
            "graph.materialise_revision.ms.small",
            loaded.get("graph.materialise_revision.ms.small"),
            Timing::of(materialise_ms.clone()).p95,
            "ms",
        ),
        verdict(
            "warm.graph.marginal.ms.small (step)",
            loaded.get("warm.graph.marginal.ms.small"),
            Timing::of(step_ms.clone()).p95,
            "ms",
        ),
        verdict(
            "query.graph_dependents.ms.small",
            loaded.get("query.graph_dependents.ms.small"),
            Timing::of(fans_ms.clone()).p95,
            "ms",
        ),
        verdict(
            "query.graph_path.ms.small",
            loaded.get("query.graph_path.ms.small"),
            Timing::of(path_ms.clone()).p95,
            "ms",
        ),
        verdict(
            "query.graph_delta_adjacent.ms.small",
            loaded.get("query.graph_delta_adjacent.ms.small"),
            Timing::of(delta_ms.clone()).p95,
            "ms",
        ),
    ];

    json!({
        "corpus": corpus.shape,
        "policy": {
            "chain_full_every": policy.chain_full_every,
            "max_chain_walk": policy.max_chain_walk,
            "path_max_hops": policy.path_max_hops,
        },
        "write": {
            "per_revision": Timing::of(write_ms).json(),
            "anchors": anchors,
            "delta_ops": ops_written,
            "chunk_blobs": chunks_written,
            "max_deltas_between_anchors": (corpus.history.len() / usize::max(anchors, 1))
                .saturating_sub(1),
        },
        "bytes": {
            "basis": "row text plus blob payload, the basis s3 measured and budgets.json means",
            "per_revision": bytes_per_revision,
            "total": bytes.total(),
            "delta_rows": bytes.delta_rows,
            "delta_payload": bytes.delta_payload,
            "chunk_blobs": bytes.chunk_rows,
            "chunk_bytes": bytes.chunk_bytes,
            "manifest_blobs": bytes.manifest_rows,
            "manifest_bytes": bytes.manifest_bytes,
            "dictionary_rows": bytes.dictionary_rows,
            "dictionary_payload": bytes.dictionary_payload,
            "dictionary_vertices": modules_live,
            "store_total": store_bytes.total,
            "store_basis": store_bytes.basis,
        },
        "read": {
            "materialise": Timing::of(materialise_ms).json(),
            "step": Timing::of(step_ms.clone()).json(),
            "step_samples": stepped,
            "step_samples_onto_anchors": onto_anchor,
            "dependencies_of": Timing::of(deps_ms).json(),
            "dependents_of": Timing::of(fans_ms).json(),
            "path": Timing::of(path_ms).json(),
            "layer_subgraph": Timing::of(layer_ms).json(),
            "changed_edges_adjacent": Timing::of(delta_ms.clone()).json(),
            "changed_edges_came_from_the_store": from_store,
            "chain_walk_steps_max": walk_steps.iter().max().copied().unwrap_or(0),
            "undecoded_lines": undecoded,
        },
        "verdicts": verdicts,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argument = |name: &str, default: &str| -> String {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
            .unwrap_or_else(|| default.to_string())
    };
    let flag = |name: &str| args.iter().any(|a| a == name);

    let modules: usize = argument("--modules", &DEFAULT_MODULES.to_string())
        .parse()
        .expect("--modules");
    let revisions: usize = argument("--revisions", &DEFAULT_REVISIONS.to_string())
        .parse()
        .expect("--revisions");
    let fan_out: usize = argument("--fan-out", "6").parse().expect("--fan-out");
    let seed: u64 = argument("--seed", "20260729").parse().expect("--seed");
    let chain_full_every: usize = argument(
        "--chain-full-every",
        &GraphPolicy::default().chain_full_every.to_string(),
    )
    .parse()
    .expect("--chain-full-every");
    let report = argument("--report", "");
    let directory = argument(
        "--store-dir",
        &std::env::temp_dir()
            .join(format!("fathom-graph-harness-{}", std::process::id()))
            .display()
            .to_string(),
    );
    let policy = GraphPolicy {
        chain_full_every,
        max_chain_walk: chain_full_every,
        ..GraphPolicy::default()
    };
    for notice in policy.notices() {
        eprintln!("note: {notice}");
    }

    println!(
        "generating {modules} modules x {revisions} revisions (fan-out {fan_out}, seed {seed})"
    );
    let started = Instant::now();
    let corpus = generate(modules, revisions, fan_out, seed);
    println!(
        "generated in {:.1} s; recording the chain",
        started.elapsed().as_secs_f64()
    );
    let started = Instant::now();
    let report_value = measure(&corpus, &policy, Path::new(&directory));
    println!("measured in {:.1} s", started.elapsed().as_secs_f64());

    let bytes = &report_value["bytes"];
    println!(
        "{} B/revision over {} revisions ({} anchors, {} delta rows, {} chunk blobs); dictionary {} rows",
        bytes["per_revision"].as_i64().unwrap_or(0),
        revisions,
        report_value["write"]["anchors"].as_u64().unwrap_or(0),
        bytes["delta_rows"].as_i64().unwrap_or(0),
        bytes["chunk_blobs"].as_i64().unwrap_or(0),
        bytes["dictionary_rows"].as_i64().unwrap_or(0),
    );
    println!("{}", bytes["basis"].as_str().unwrap_or_default());
    println!(
        "materialise p95 {} ms, step p95 {} ms, dependents p95 {} ms, path p95 {} ms, adjacent delta p95 {} ms",
        report_value["read"]["materialise"]["p95_ms"],
        report_value["read"]["step"]["p95_ms"],
        report_value["read"]["dependents_of"]["p95_ms"],
        report_value["read"]["path"]["p95_ms"],
        report_value["read"]["changed_edges_adjacent"]["p95_ms"],
    );
    for line in report_value["verdicts"].as_array().unwrap_or(&Vec::new()) {
        println!(
            "  {:<48} {:>10} {} (budget {:>10}) {}",
            line["id"].as_str().unwrap_or_default(),
            line["measured"],
            line["unit"].as_str().unwrap_or_default(),
            line["budget"],
            line["verdict"].as_str().unwrap_or_default()
        );
    }

    if !report.is_empty() {
        let path = PathBuf::from(&report);
        let payload = serde_json::to_string_pretty(&report_value).expect("serialize");
        if flag("--check") {
            let before = std::fs::read_to_string(&path).unwrap_or_default();
            let previous: Value = serde_json::from_str(&before).unwrap_or(Value::Null);
            // Bytes and chain shape are deterministic on a deterministic corpus; timings are not, so
            // they are not compared — the same line `structure_harness --check` draws. The byte
            // verdict is in, because it is a byte budget and nothing else.
            let comparable = |value: &Value| {
                let byte_verdicts: Vec<Value> = value["verdicts"]
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter(|line| {
                                line["id"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .starts_with("store.bytes_")
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                json!({
                    "bytes": value["bytes"],
                    "write": {
                        "anchors": value["write"]["anchors"],
                        "delta_ops": value["write"]["delta_ops"],
                        "chunk_blobs": value["write"]["chunk_blobs"],
                    },
                    "corpus": value["corpus"],
                    "policy": value["policy"],
                    "undecoded": value["read"]["undecoded_lines"],
                    "verdicts": byte_verdicts,
                })
            };
            if comparable(&previous) == comparable(&report_value) {
                println!("check: {report} reproduces");
            } else {
                eprintln!("check: {report} does NOT reproduce\n{payload}");
                std::process::exit(1);
            }
        } else {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&path, payload).expect("write the report");
            println!("wrote {report}");
        }
    }
}

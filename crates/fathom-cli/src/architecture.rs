//! The architecture map's read (`e3.4`, `reco-w3w`).
//!
//! ADR-006 §5.5 gives the graph view four jobs — the structure map, the dependency map, the per-episode
//! delta overlay, and the file-and-line citation behind every arrow — and every one of them is a store read
//! plus a chain walk. The shell may not make either (ADR-002 §5.2, lint `E01`), so this module makes them
//! here and hands the result over as one [`graph::Hand`], in the same shape as [`crate::episodes`],
//! [`crate::patches`] and [`crate::search`]: the composition root pulls, the shell draws what it was given.
//!
//! Three rules:
//!
//! * **The read happens once, at startup.** A chain walk per keystroke is a chain walk the reviewer pays
//!   for with a frozen frame, and [`graph::Hand`] is deliberately a value: nodes, edges, heat and the
//!   episode's delta, all already spelled in the view's own nouns.
//! * **`None` is an answer with a reason.** No store, no graph lane, a chain that reaches no anchor: each
//!   is said out loud on stderr, and the canvas keeps its published placeholder. An empty architecture pane
//!   that says nothing is the failure mode ADR-006 §6.3 was written against.
//! * **The citation is lazy.** Naming the line an arrow came from means reading that file's structural
//!   record, which for a whole map is a thousand reads nobody asked for. So it is a provider, asked for the
//!   one arrow under the cursor — the same inversion `d` uses for patch bytes.

use std::path::{Path, PathBuf};

use fathom_tui::graph::hand::{Cell as HandCell, CellKind, Link as HandLink};
use fathom_tui::graph::{self, Line, Mark, Op, Subject};

/// The churn window the heat map is drawn from, in revisions.
///
/// Thirty is the number `e3.5`'s churn measure already uses for a "recent" window and the number the
/// hot-file ladder in the queue reads from; a second window length would be two answers to one question.
const CHURN_WINDOW: usize = 30;

/// How many delta rows the hand carries.
///
/// The canvas aggregates by *module*, so a row per symbol past a few hundred says nothing a pane can print;
/// the counts the hand still carries are the full ones, and the truncation is declared in
/// [`graph::Hand::truncated`] rather than hidden.
const DELTA_ROW_CAP: usize = 400;

/// The map the `g` view draws, or `None` with the reason printed.
///
/// Called once, by `fathom tui`'s option build, before the first frame.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
#[must_use]
pub fn hand(root: &Path) -> Option<graph::Hand> {
    use fathom_analyze::graph::Lane;

    let path = crate::session::store_path(root);
    if !path.is_file() {
        // No index at all. The canvas's own placeholder already names `fathom index`, so this line is for
        // the person who piped the shell's stderr into a log and cannot see the frame.
        eprintln!(
            "fathom: no index at {}, so the architecture map has nothing to draw",
            path.display()
        );
        return None;
    }
    let mut store = match fathom_store::spine::Store::open(&path) {
        Ok(store) => store,
        Err(why) => {
            eprintln!("fathom: the index would not open for the architecture read: {why}");
            return None;
        }
    };
    let revisions = match store.graph_revisions() {
        Ok(revisions) => revisions,
        Err(why) => {
            eprintln!("fathom: the graph lane would not be listed: {why}");
            return None;
        }
    };
    if revisions.is_empty() {
        eprintln!(
            "fathom: this index holds no module graph, so `g` draws its placeholder — `fathom index` \
             writes graph snapshots"
        );
        return None;
    }
    let (builder, mut notices) = builder(root);
    let lane = match Lane::new(&mut store, builder) {
        Ok(lane) => lane,
        Err(why) => {
            eprintln!("fathom: the graph lane would not open: {why}");
            return None;
        }
    };
    // Newest first, which is the order `graph_revisions` is written in (`built_utc DESC`).
    let to = revisions[0].clone();
    let from = revisions.get(1).cloned();

    let (graph, report) = match lane.materialise(&to) {
        Ok(pair) => pair,
        Err(partial) => {
            eprintln!(
                "fathom: the architecture map could not be reconstructed: {}",
                partial.sentence()
            );
            return None;
        }
    };
    // The walk's own cost is a fact about the picture, not a log line: a graph that arrived from an anchor
    // eight deltas back is a graph whose hot modules are older news than the strip implies.
    notices.push(report.sentence());
    if report.undecoded > 0 {
        notices.push(format!(
            "{} stored chunk line(s) did not decode, so this map may be smaller than the revision's graph",
            report.undecoded
        ));
    }

    // The node dictionary, once: the graph itself stores interned ids, and every name a pane prints comes
    // from this map.
    let dictionary = match lane.read().graph_nodes() {
        Ok(nodes) => nodes,
        Err(why) => {
            eprintln!("fathom: the node dictionary would not be read: {why}");
            return None;
        }
    };
    let layers: Vec<String> = lane
        .layers()
        .values()
        .filter(|name| !name.is_empty())
        .cloned()
        .collect::<std::collections::BTreeSet<String>>()
        .into_iter()
        .collect();

    // Cells: module nodes, plus any symbol an edge actually names. A symbol nobody points at is a declaration
    // list, not a node in a dependency map, and drawing 40 000 of them would be a map of nothing.
    let mut cells: Vec<HandCell> = Vec::new();
    let mut named: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for &(_kind, src, dst) in graph.edges() {
        named.insert(src);
        named.insert(dst);
    }
    let mut skipped_kinds: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut links: Vec<HandLink> = Vec::new();
    for &(kind_stored, src_id, dst_id) in graph.edges() {
        let kind_text = kind_stored.as_str();
        let Some(kind) = graph::LinkKind::parse(kind_text) else {
            // `contains`, and anything a future analyzer writes that this build does not know: counted,
            // never drawn as a kind it is not.
            if kind_text != "contains" {
                *skipped_kinds.entry(kind_text.to_owned()).or_default() += 1;
            }
            continue;
        };
        let (Some(src), Some(dst)) = (dictionary.get(&src_id), dictionary.get(&dst_id)) else {
            // A row whose end the dictionary cannot spell is a pruned or foreign vertex. The delta still
            // reports it (`e3.3` calls it class R), so counting it here is the honest half.
            *skipped_kinds
                .entry("unnamed endpoint".to_owned())
                .or_default() += 1;
            continue;
        };
        if src.kind != "module" || dst.kind != "module" {
            // Symbol-granular arrows are drawn by aggregating them onto their modules: two arrows between
            // the same pair of files is one arrow in a map whose unit is the file, and the row count says
            // how many statements it stands for.
            links.push(HandLink::new(
                module_of(&src.key),
                module_of(&dst.key),
                kind,
                module_of(&src.key),
                Line::Unrecorded,
            ));
            continue;
        }
        links.push(HandLink::new(
            &src.key,
            &dst.key,
            kind,
            &src.key,
            Line::Unrecorded,
        ));
    }
    let link_total = links.len();
    links.sort_by(|a, b| {
        a.src
            .cmp(&b.src)
            .then(a.dst.cmp(&b.dst))
            .then(a.kind.cmp(&b.kind))
    });
    links.dedup_by(|a, b| a.src == b.src && a.dst == b.dst && a.kind == b.kind);
    let link_rows = link_total;
    let link_arrows = links.len();

    for (id, kind) in graph.nodes() {
        let Some(node) = dictionary.get(id) else {
            continue;
        };
        let module = node.kind == "module";
        if !module && !named.contains(id) {
            continue;
        }
        cells.push(HandCell {
            key: node.key.clone(),
            kind: if module {
                CellKind::Module
            } else {
                CellKind::Symbol
            },
            layer: if node.layer.is_empty() {
                None
            } else {
                Some(node.layer.clone())
            },
            may_have_shrunk: false,
            present: true,
            evidence: None,
        });
        let _ = kind;
    }
    cells.sort_by(|a, b| a.key.cmp(&b.key));
    cells.dedup_by(|a, b| a.key == b.key);

    let mut gaps: Vec<String> = notices
        .iter()
        .map(|note| format!("graph: {note}"))
        .collect();
    for (kind, count) in &skipped_kinds {
        gaps.push(format!(
            "{count} rows of kind `{kind}` are not dependencies this map can draw"
        ));
    }

    // Heat: churn is measured; complexity is not, and says so rather than drawing a cold map.
    let heat = match lane.churn_recent(CHURN_WINDOW) {
        Ok(report) => graph::Heat {
            churn: heat_series(&report),
            complexity: graph::Series::unavailable(
                "structural complexity is not computed at startup — `fathom structure` measures it",
            ),
        },
        Err(partial) => {
            gaps.push(format!("heat: {}", partial.sentence()));
            graph::Heat {
                churn: graph::Series::unavailable("the churn window could not be read"),
                complexity: graph::Series::unavailable(
                    "structural complexity is not computed at startup",
                ),
            }
        }
    };

    let mut hand = graph::Hand::new(short(&to), cells, links);
    hand.layers = layers;
    hand.debt = debt(root);
    hand.heat = heat;
    hand.gaps = gaps;
    hand.cost = Some(format!(
        "{} · {} modules · {} dependencies",
        short(&to),
        hand.cells.len(),
        link_rows
    ));
    if link_rows > link_arrows {
        hand.truncated = Some(graph::Truncated {
            cells_handed: hand.cells.len(),
            cells_total: hand.cells.len(),
            links_handed: link_arrows,
            links_total: link_rows,
            reason:
                "arrows between the same pair of modules were merged into one, and the row count \
                     beside each arrow says how many statements it stands for"
                    .to_owned(),
        });
    }

    // The overlay: what this episode did. `from` is the previous indexed revision, which is the only span
    // this build can call "this episode" without an episode id in play; when the shell is opened with a
    // focused episode the caller narrows it (`Episode::delta_span`).
    if let Some(from) = from {
        match lane.architecture_delta(&from, &to) {
            Ok(delta) => hand.delta = delta_of(&delta),
            Err(partial) => hand.gaps.push(format!("overlay: {}", partial.sentence())),
        }
    } else {
        hand.gaps.push(
            "overlay: this index holds one graph revision, so there is no previous revision to \
                   diff this one against"
                .to_owned(),
        );
    }

    match hand.checked() {
        Ok(hand) => Some(hand),
        Err(why) => {
            eprintln!("fathom: the architecture hand was refused: {why}");
            None
        }
    }
}

/// No graph in a build with no store backend, said once.
#[cfg(not(all(feature = "tui", feature = "sqlite-bundled")))]
#[must_use]
pub fn hand(_root: &Path) -> Option<graph::Hand> {
    None
}

/// The committed allowance ledger as the map shows it (`e3.10`, `reco-5ht`).
///
/// Only the file and the clock, which is the whole point of reading it here: the map has no judged revision
/// in front of it and must not pretend to know what is still owed. What it can know — what it would be
/// careless not to show beside a vertex drawn as debt — is that the allowance forgiving it runs out on a
/// date, and that the date is in the file the reviewer already committed.
#[must_use]
fn debt(root: &Path) -> graph::Debt {
    use fathom_analyze::ratchet::{Ledger, Ratchet};
    use fathom_core::time::Timestamp;

    let config = std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default();
    let (ratchet, _notices) = Ratchet::read(root, &config);
    let now = Timestamp::from_unix_micros(crate::clock::now_millis() * 1_000);
    let ledger = Ledger::of(&ratchet, now);
    graph::Debt {
        source: ledger.source.clone(),
        read: ledger.read,
        live: ledger.live,
        expiring: ledger.expiring_lines(),
        back_due: ledger.lapsed_lines(),
        // "There is no baseline file" is the verb's sentence to say, not the map's; a malformed one is
        // different, because the file is there and the map is showing a ledger it could not read.
        why: if ratchet.loaded {
            ledger.notices.clone()
        } else {
            Vec::new()
        },
    }
}

/// Who names the statement an arrow came from: the declaring file's own structural record, read at the
/// revision the map is drawn from (`e3.4`).
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
#[must_use]
pub fn citations(root: &Path) -> Option<graph::CitationSource> {
    Some(graph::CitationSource::new(RecordCitations {
        root: root.to_path_buf(),
        revision: "HEAD".to_owned(),
    }))
}

/// A build with no backend has no record to read, and the pane says the line is not recorded rather than
/// printing a path as though it were a citation.
#[cfg(not(all(feature = "tui", feature = "sqlite-bundled")))]
#[must_use]
pub fn citations(_root: &Path) -> Option<graph::CitationSource> {
    None
}

/// The provider, which opens the store and the object store per request and holds nothing between them —
/// the same discipline [`crate::patches::StorePatches`] follows, for the same reason.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
pub struct RecordCitations {
    /// The repository under review.
    pub root: PathBuf,
    /// The revision the map was materialised at, as a name the object store resolves.
    pub revision: String,
}

#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
impl graph::Citations for RecordCitations {
    fn cite(&self, src: &str, dst: &str) -> Option<graph::Evidence> {
        let (path, line) = cite_statement(&self.root, &self.revision, src, dst)?;
        let mut evidence = graph::Evidence::file(path, line);
        evidence.run = Some(format!("fathom structure {}", src));
        Some(evidence)
    }
}

/// The statement in `src` that depends on `dst`, as file and line.
///
/// One import per line is the normal case, and the normal case is the case worth printing. Where a file
/// names its target twice — two symbols out of one module, imported on two lines — the candidates are
/// counted and printed as candidates rather than guessed at, because a wrong line number in a citation is
/// worse than no line number (`e3.4`'s honesty rule, the same one `s1` finding 2 left behind).
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn cite_statement(root: &Path, revision: &str, src: &str, dst: &str) -> Option<(String, Line)> {
    use fathom_analyze::structure::{ExtractPolicy, extract_one};
    use fathom_ingest::repo::Repository;

    let repo = Repository::discover(root).ok()?;
    let oid = repo.resolve(revision).ok()?;
    let commit = repo.commit(oid).ok()??;
    let text = read_path(&repo, commit.tree, src)?;
    let record = extract_one(src, &text, &ExtractPolicy::default());
    let structure = record.structure?;

    // The edges the extractor resolved, and the import statements that produced them: an `Edge` copies the
    // `form` and the `symbol` of the `Import` it came from, which is the only link back to a line number
    // the record keeps.
    let target = module_of(dst);
    let forms: Vec<(String, String)> = structure
        .edges
        .iter()
        .filter(|edge| module_of(&edge.target) == target)
        .map(|edge| (edge.form.clone(), edge.symbol.clone()))
        .collect();
    if forms.is_empty() {
        return Some((src.to_owned(), Line::Unrecorded));
    }
    let lines: Vec<u32> = structure
        .imports
        .iter()
        .filter(|import| {
            forms
                .iter()
                .any(|(form, symbol)| form == &import.form && symbol == &import.symbol)
        })
        .map(|import| u32::try_from(import.span.start_line).unwrap_or(u32::MAX))
        .collect();
    let lines: std::collections::BTreeSet<u32> = lines.into_iter().collect();
    let line = match lines.len() {
        0 => Line::Unrecorded,
        1 => Line::At(*lines.iter().next()?),
        count => Line::Ambiguous(count),
    };
    Some((src.to_owned(), line))
}

/// Walk a tree to one path and read the blob at it.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn read_path(
    repo: &fathom_ingest::repo::Repository,
    tree: fathom_ingest::oid::Oid,
    path: &str,
) -> Option<Vec<u8>> {
    use fathom_ingest::repo::{BlobRead, TreeEntryKind};

    let mut current = tree;
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    for (index, part) in parts.iter().enumerate() {
        let entries = repo.tree_entries(current).ok()?;
        let entry = entries
            .iter()
            .find(|entry| entry.name_text().as_ref() == *part)?;
        if index + 1 == parts.len() {
            if !matches!(entry.kind, TreeEntryKind::File | TreeEntryKind::Executable) {
                return None;
            }
            return match repo.blob(entry.oid, 2 << 20).ok()? {
                BlobRead::Present { bytes, .. } => Some(bytes),
                _ => None,
            };
        }
        current = entry.oid;
    }
    None
}

/// The graph lane's builder and layer tags, read out of the repository's own `fathom.toml`.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn builder(root: &Path) -> (fathom_analyze::graph::Builder, Vec<String>) {
    match std::fs::read_to_string(root.join("fathom.toml")) {
        Ok(text) => fathom_analyze::graph::builder_from_config_text(&text),
        Err(_) => (
            fathom_analyze::graph::Builder::default(),
            vec![
                "no fathom.toml, so no [layers] table was read and every module is untagged"
                    .to_owned(),
            ],
        ),
    }
}

/// The delta, flattened into the three marks a canvas can draw.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn delta_of(delta: &fathom_analyze::graph::ArchitectureDelta) -> graph::Delta {
    let mut out = graph::Delta::empty(short(&delta.from), short(&delta.to));
    out.rung = delta.cost.rung.as_str().to_owned();
    out.counts = delta
        .counts()
        .into_iter()
        .map(|(words, count)| (words.to_owned(), count))
        .collect();
    out.notices = delta.notices.clone();

    let mut ops: Vec<Op> = Vec::new();
    for fact in &delta.nodes_added {
        ops.push(node_op(
            "node_added",
            &fact.key,
            Mark::Added,
            node_why(fact),
        ));
    }
    for fact in &delta.nodes_removed {
        ops.push(node_op(
            "node_removed",
            &fact.key,
            Mark::Removed,
            node_why(fact),
        ));
    }
    for fact in &delta.edges_added {
        ops.push(edge_op("edges_added", fact, Mark::Added));
    }
    for fact in &delta.edges_removed {
        ops.push(edge_op("edges_removed", fact, Mark::Removed));
    }
    for fact in &delta.repointed {
        // Same endpoints, a different kind of knowledge: the dependency did not move, what the file says
        // about it did — a change, and not an addition.
        ops.push(Op {
            finding: "repointed".to_owned(),
            subject: Subject::Edge {
                src: module_of(&fact.src),
                dst: module_of(&fact.dst),
                kind: graph::LinkKind::parse(&fact.to_kind),
            },
            mark: Mark::Changed,
            why: Some(format!("{} \u{2192} {}", fact.from_kind, fact.to_kind)),
            evidence: Some(graph::Evidence::file(
                module_of(&fact.module),
                Line::Unrecorded,
            )),
        });
    }
    for fact in &delta.reversals {
        ops.push(Op {
            finding: "reversals".to_owned(),
            subject: Subject::Edge {
                src: module_of(&fact.a),
                dst: module_of(&fact.b),
                kind: None,
            },
            mark: Mark::Changed,
            why: Some(if fact.mutual.unwrap_or(false) {
                "both directions now exist: this is a cycle, not a reversal".to_owned()
            } else {
                "this arrow now points the other way".to_owned()
            }),
            evidence: None,
        });
    }
    let total = ops.len();
    if total > DELTA_ROW_CAP {
        ops.truncate(DELTA_ROW_CAP);
        out.notices.push(format!(
            "the overlay prints {DELTA_ROW_CAP} of {total} delta rows; the counts beside them are the \
             full ones"
        ));
    }
    out.ops = ops;
    out.violations = delta.layer_drift.len();
    out
}

/// Why a node row moved, in the delta's own facts: a nameless vertex and a possibly-shrunk declaration
/// list are two different reasons and a pane may not print them as one.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn node_why(fact: &fathom_analyze::graph::NodeFact) -> Option<String> {
    if fact.unnamed {
        return Some("the dictionary cannot spell this node — a pruned or foreign row".to_owned());
    }
    if fact.may_have_shrunk {
        return Some(
            "this module's declaration list may have shrunk rather than shrunk".to_owned(),
        );
    }
    None
}

/// A node row, in the hand's nouns.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn node_op(finding: &str, key: &str, mark: Mark, why: Option<String>) -> Op {
    let mut op = Op {
        finding: finding.to_owned(),
        subject: Subject::Node {
            key: key.to_owned(),
        },
        mark,
        why,
        evidence: None,
    };
    op.evidence = Some(graph::Evidence::file(module_of(key), Line::Unrecorded));
    op
}

/// An edge row, in the hand's nouns.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn edge_op(finding: &str, fact: &fathom_analyze::graph::EdgeFact, mark: Mark) -> Op {
    let mut op = Op {
        finding: finding.to_owned(),
        subject: Subject::Edge {
            src: module_of(&fact.src),
            dst: module_of(&fact.dst),
            kind: graph::LinkKind::parse(&fact.kind),
        },
        mark,
        why: if fact.why.is_empty() {
            None
        } else {
            Some(fact.why.clone())
        },
        evidence: None,
    };
    if !fact.module.is_empty() {
        op.evidence = Some(graph::Evidence::file(
            module_of(&fact.module),
            Line::Unrecorded,
        ));
    }
    op
}

/// A symbol key's owning module: `<path>::<Symbol>` is the graph's spelling of a declaration inside a file.
#[must_use]
pub fn module_of(key: &str) -> String {
    match key.find("::") {
        Some(at) => key[..at].to_owned(),
        None => key.to_owned(),
    }
}

/// A short revision name for a pane: the first ten characters of a tree oid is still unique in a session
/// and fits in a strip that is already carrying four other facts.
#[must_use]
pub fn short(revision: &str) -> String {
    if revision.len() <= 10 || revision.chars().any(|c| !(c.is_ascii_hexdigit())) {
        return revision.to_owned();
    }
    let cut: String = revision.chars().take(10).collect();
    format!("{cut}…")
}

/// The churn report, normalised into the heat series the canvas bands.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn heat_series(report: &fathom_analyze::graph::ChurnReport) -> graph::Series {
    let Some(top) = report.per_module.iter().map(|row| row.churn).max() else {
        return graph::Series::unavailable("no revision in the window changed a dependency");
    };
    if top == 0 {
        return graph::Series::unavailable("no revision in the window changed a dependency");
    }
    let values: Vec<(String, f64)> = report
        .per_module
        .iter()
        .map(|row| {
            (
                module_of(&row.module),
                f64::from(row.churn as u32) / f64::from(top as u32),
            )
        })
        .collect();
    graph::Series::measured(values, format!("over the last {} revisions", report.window))
}

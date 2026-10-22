//! The cycles (`e3.5`, `reco-8md`): the shortest offender path, the maintained components, and the
//! debt ledger.
//!
//! The store-facing half — two revisions in a lane, one episode's answer — is `tests/cycles_episode.rs`.
//! What lives here needs no store, which is the point: cycle detection is a pure function of a graph and
//! a naming, and ADR-002 §6.1 says a `--no-default-features` build has to be able to say so.
//!
//! The graphs here are hand-built from rows rather than run through the builder, for two reasons. The
//! assertions are about the *cycle model* — a minimal path, a package claim, a wildcard count — and
//! running them through the extractor would be testing the extractor twice. And a few of the rows a
//! cycle detector has to survive (a self-import, an endpoint no dictionary knows) are ones the builder
//! refuses to write, so they can only be made by hand; the test is then honest about that, because a
//! defensive path that is never exercised is a path nobody has checked.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use fathom_analyze::cycles::{
    self, Allowance, Change, Cycle, CycleDelta, Mode, Options, Ratchet, Tracker, View,
    normalize_key, parse_date,
};
use fathom_analyze::graph::model::{Graph, Op, Row, RowKind, Vertex};
use fathom_analyze::graph::query::Naming;
use fathom_core::time::Timestamp;

/// Microseconds in a second: the epoch the debt tests argue against is in seconds.
const MICROS_PER_SEC: i64 = 1_000_000;

/// A hand-built revision: interned keys, the rows they make, and the layer tags to go with them.
///
/// Ids are minted in call order, which is the same contract the store's dictionary has — and nothing here
/// depends on the numbers, only on the keys, exactly as the cycle model does not.
#[derive(Default)]
struct Model {
    ids: BTreeMap<String, i64>,
    layers: BTreeMap<String, String>,
    rows: Vec<Row>,
    next: i64,
}

impl Model {
    fn id(&mut self, key: &str) -> i64 {
        if let Some(id) = self.ids.get(key) {
            return *id;
        }
        let id = self.next;
        self.next += 1;
        self.ids.insert(key.to_string(), id);
        id
    }

    /// A module node, optionally tagged with a layer.
    fn module(&mut self, path: &str, layer: &str) -> i64 {
        let id = self.id(path);
        self.rows.push((RowKind::Module, id, 0));
        if !layer.is_empty() {
            self.layers.insert(path.to_string(), layer.to_string());
        }
        id
    }

    /// An exported symbol node, and the `contains` row that puts it in its module.
    fn symbol(&mut self, path: &str, name: &str) -> i64 {
        let module = self.id(path);
        let id = self.id(&format!("{path}::{name}"));
        self.rows.push((RowKind::Symbol, id, 0));
        self.rows.push((RowKind::Contains, module, id));
        id
    }

    /// One dependency row.
    fn edge(&mut self, kind: RowKind, from: &str, to: &str) {
        let (src, dst) = (self.id(from), self.id(to));
        self.rows.push((kind, src, dst));
    }

    /// The graph these rows make.
    fn graph(&self) -> Graph {
        Graph::from_rows(self.rows.iter().copied())
    }

    /// The dictionary, as the lane would hand it back after a write.
    fn naming(&self) -> Naming {
        Naming::from_vertices(self.ids.iter().map(|(key, id)| Vertex {
            id: *id,
            key: key.clone(),
            kind: if key.contains("::") {
                RowKind::Symbol
            } else {
                RowKind::Module
            },
            layer: self.layers.get(key).cloned().unwrap_or_default(),
            incomplete: false,
        }))
    }

    /// The ops that add rows this model has and a previous graph does not.
    fn added_against(&self, previous: &Graph) -> Vec<Op> {
        let after: Vec<Row> = self.rows.clone();
        let before: Vec<Row> = previous.rows().into_iter().collect();
        let mut ops: Vec<Op> = after
            .iter()
            .filter(|row| !before.contains(row))
            .copied()
            .map(Op::add)
            .collect();
        ops.extend(
            before
                .iter()
                .filter(|row| !after.contains(row))
                .copied()
                .map(Op::remove),
        );
        ops.sort_by_key(|op| (op.add, op.row));
        ops
    }
}

/// The three-module cycle every reviewer pictures: `a → b → c → a`.
fn triangle() -> Model {
    let mut model = Model::default();
    for path in ["a.rs", "b.rs", "c.rs"] {
        model.module(path, "");
    }
    model.edge(RowKind::Imports, "a.rs", "b.rs");
    model.edge(RowKind::Imports, "b.rs", "c.rs");
    model.edge(RowKind::Imports, "c.rs", "a.rs");
    model
}

fn tracker(model: &Model, options: Options) -> Tracker {
    Tracker::build(&model.graph(), &model.naming(), options)
}

/// The cycles of a model, computed cold in the module view — the shape an assertion starts from.
fn detect(model: &Model) -> Vec<Cycle> {
    cycles::detect(&model.graph(), &model.naming(), Options::default())
}

fn one(cycles: Vec<Cycle>) -> Cycle {
    assert_eq!(
        cycles.len(),
        1,
        "expected exactly one cycle, got {cycles:?}"
    );
    cycles.into_iter().next().expect("checked above")
}

// ---------------------------------------------------------------------------
// the shortest offender path, exactly
// ---------------------------------------------------------------------------

#[test]
fn two_modules_that_import_each_other_are_one_cycle_of_two() {
    let mut model = Model::default();
    model.module("billing.rs", "domain");
    model.module("invoicing.rs", "domain");
    model.edge(RowKind::Imports, "billing.rs", "invoicing.rs");
    model.edge(RowKind::Imports, "invoicing.rs", "billing.rs");

    let cycles = cycles::detect(&model.graph(), &model.naming(), Options::default());
    let cycle = one(cycles);
    assert_eq!(cycle.members, vec!["billing.rs", "invoicing.rs"]);
    // The minimal path, starting at the alphabetically-first member and closing the loop.
    assert_eq!(cycle.path, vec!["billing.rs", "invoicing.rs"]);
    assert_eq!(cycle.display(), "billing.rs → invoicing.rs → billing.rs");
    assert!(
        cycle.exact,
        "a two-member component is searched exhaustively"
    );
    assert_eq!(cycle.depth(), 2);
    assert_eq!(cycle.key(), "billing.rs|invoicing.rs");
    assert!(
        cycle.sentence().contains("2 module member(s), 2 deep"),
        "{}",
        cycle.sentence()
    );
    assert!(
        cycle.sentence().contains("layers domain"),
        "the layer tags ride along: {}",
        cycle.sentence()
    );
}

#[test]
fn the_shortest_path_wins_when_there_is_a_long_way_round() {
    // One component, two routes back: the short one is the offender path, and the long one's modules
    // are still members of the same cycle.
    let mut model = Model::default();
    for path in ["a.rs", "b.rs", "c.rs", "x.rs", "y.rs", "z.rs"] {
        model.module(path, "");
    }
    model.edge(RowKind::Imports, "a.rs", "b.rs");
    model.edge(RowKind::Imports, "b.rs", "c.rs");
    model.edge(RowKind::Imports, "c.rs", "a.rs");
    model.edge(RowKind::Imports, "a.rs", "x.rs");
    model.edge(RowKind::Imports, "x.rs", "y.rs");
    model.edge(RowKind::Imports, "y.rs", "z.rs");
    model.edge(RowKind::Imports, "z.rs", "a.rs");

    let cycles = cycles::detect(&model.graph(), &model.naming(), Options::default());
    let cycle = one(cycles);
    assert_eq!(cycle.members.len(), 6, "one component, not two cycles");
    assert_eq!(cycle.depth(), 3, "the offender path is the short way round");
    assert_eq!(cycle.display(), "a.rs → b.rs → c.rs → a.rs");
    assert!(cycle.exact);
}

#[test]
fn a_component_beyond_the_witness_cap_reports_its_witness_as_inexact() {
    // Forty modules in one ring. Searching every member for the shortest cycle is superlinear in the
    // component, so above the cap the walk runs once, from the canonical member, and says so rather
    // than claiming minimality it did not check.
    let mut model = Model::default();
    let names: Vec<String> = (0..40).map(|index| format!("m{index:02}.rs")).collect();
    for name in &names {
        model.module(name, "");
    }
    for index in 0..names.len() {
        model.edge(
            RowKind::Imports,
            &names[index],
            &names[(index + 1) % names.len()],
        );
    }

    let cycles = cycles::detect(&model.graph(), &model.naming(), Options::default());
    let cycle = one(cycles);
    assert_eq!(cycle.members.len(), 40);
    assert!(
        !cycle.exact,
        "past the cap the witness is a fact about one member"
    );
    assert!(cycle.sentence().contains("not proven shortest"));
}

#[test]
fn a_self_import_is_a_cycle_of_one_and_not_a_rounding_error() {
    // The extractor drops self-edges today (`build.rs`), so this row can only arrive hand-made — and
    // that is exactly why the detector is the place it must not silently vanish.
    let mut model = Model::default();
    model.module("mirror.rs", "");
    model.edge(RowKind::Imports, "mirror.rs", "mirror.rs");

    let cycles = cycles::detect(&model.graph(), &model.naming(), Options::default());
    let cycle = one(cycles);
    assert_eq!(cycle.members, vec!["mirror.rs"]);
    assert_eq!(cycle.display(), "mirror.rs → mirror.rs");
    assert!(cycle.exact);
}

#[test]
fn a_dependency_on_an_unknown_endpoint_is_counted_not_invented() {
    let mut model = Model::default();
    model.module("a.rs", "");
    model.module("b.rs", "");
    model.edge(RowKind::Imports, "a.rs", "b.rs");
    model.edge(RowKind::Imports, "b.rs", "a.rs");
    // A row whose destination the dictionary never saw: named by the row, unknown to the graph.
    let known = model.id("a.rs");
    model.rows.push((RowKind::Uses, known, 9_999));

    let health = cycles::health(&model.graph(), &model.naming(), Options::default());
    assert_eq!(health.cycles, 1, "the cycle is still found");
    assert_eq!(health.nodes_in_cycles, 2);
    assert!(!health.complete(), "and the answer says it is not complete");
    assert!(health.sentence().contains("not claimed complete"));
}

// ---------------------------------------------------------------------------
// what the granularity of the edges says about the cycle
// ---------------------------------------------------------------------------

#[test]
fn a_package_granular_cycle_is_reported_as_a_package_claim() {
    // Go: the dependency is between packages, recorded per file because that is the granularity the
    // store keys on (`s1` finding 4). A pane that drew `pay.go → bill.go → pay.go` would be reporting
    // a cycle the Go language does not have.
    let mut model = Model::default();
    model.module("internal/billing/pay.go", "billing");
    model.module("internal/invoicing/bill.go", "invoicing");
    model.edge(
        RowKind::ImportsPackage,
        "internal/billing/pay.go",
        "internal/invoicing/bill.go",
    );
    model.edge(
        RowKind::ImportsPackage,
        "internal/invoicing/bill.go",
        "internal/billing/pay.go",
    );

    let cycles = cycles::detect(&model.graph(), &model.naming(), Options::default());
    let cycle = one(cycles);
    assert_eq!(cycle.claim, cycles::Claim::Package);
    assert_eq!(cycle.unit, "package");
    assert_eq!(
        cycle.members,
        vec!["internal/billing", "internal/invoicing"],
        "the claim is about packages, so the members are packages"
    );
    assert_eq!(cycle.key(), "internal/billing|internal/invoicing");
    assert_eq!(cycle.package_edges, 2);
    assert!(cycle.sentence().contains("package-level"));
}

#[test]
fn a_package_claim_that_collapses_to_one_package_stays_a_file_claim() {
    // Two files, one package, a package-granular edge between them: Go cannot import itself, so this
    // is a file-level fact and printing it as "one package is a cycle around itself" would be nonsense.
    let mut model = Model::default();
    model.module("internal/billing/pay.go", "");
    model.module("internal/billing/round.go", "");
    model.edge(
        RowKind::ImportsPackage,
        "internal/billing/pay.go",
        "internal/billing/round.go",
    );
    model.edge(
        RowKind::ImportsPackage,
        "internal/billing/round.go",
        "internal/billing/pay.go",
    );

    let cycle = one(cycles::detect(
        &model.graph(),
        &model.naming(),
        Options::default(),
    ));
    assert_eq!(cycle.claim, cycles::Claim::File);
    assert_eq!(cycle.members.len(), 2);
    assert!(cycle.sentence().contains("stayed inside one package"));
}

#[test]
fn a_wildcard_edge_is_counted_on_the_cycle_it_makes() {
    let mut model = Model::default();
    model.module("models.py", "");
    model.module("service.py", "");
    model.edge(RowKind::Imports, "service.py", "models.py");
    model.edge(RowKind::Wildcard, "models.py", "service.py");

    let cycle = one(cycles::detect(
        &model.graph(),
        &model.naming(),
        Options::default(),
    ));
    assert_eq!(cycle.unknown_edges, 1);
    assert_eq!(cycle.members.len(), 2);
    assert!(
        cycle.sentence().contains("unread contents"),
        "the wildcard has to arrive with the finding: {}",
        cycle.sentence()
    );
    assert_eq!(
        cycles::health(&model.graph(), &model.naming(), Options::default()).wildcard_claimed,
        1
    );
}

#[test]
fn the_opt_in_symbol_view_names_the_declarations_that_close_the_loop() {
    // `impl` in both directions between two modules: at module level it is one cycle between two files,
    // and the symbol view is the answer to "which declarations are responsible".
    let mut model = Model::default();
    model.module("a.rs", "");
    model.module("b.rs", "");
    model.symbol("a.rs", "Alpha");
    model.symbol("b.rs", "Beta");
    model.edge(RowKind::Inherits, "a.rs::Alpha", "b.rs::Beta");
    model.edge(RowKind::Inherits, "b.rs::Beta", "a.rs::Alpha");

    let module_view = Tracker::build(
        &model.graph(),
        &model.naming(),
        Options {
            view: View::Module,
            ..Options::default()
        },
    );
    let cycle = one(module_view.cycles());
    assert_eq!(cycle.members, vec!["a.rs", "b.rs"]);

    let symbol_view = Tracker::build(
        &model.graph(),
        &model.naming(),
        Options {
            view: View::Symbol,
            ..Options::default()
        },
    );
    let cycle = one(symbol_view.cycles());
    assert_eq!(cycle.members, vec!["a.rs::Alpha", "b.rs::Beta"]);
    assert_eq!(cycle.unit, "symbol");
    assert_eq!(
        cycle.display(),
        "a.rs::Alpha → b.rs::Beta → a.rs::Alpha",
        "the opt-in view answers the opt-in question"
    );

    // And one tracker can maintain both at once, which is what the report's `symbol` half is for: the
    // module answer stays primary and the symbol answer rides along only when it was asked for.
    let mut before = Model::default();
    before.module("a.rs", "");
    before.module("b.rs", "");
    before.symbol("a.rs", "Alpha");
    before.symbol("b.rs", "Beta");
    before.edge(RowKind::Inherits, "a.rs::Alpha", "b.rs::Beta");
    let mut both = tracker(&before, Options::default().with_symbol_view());
    let ops = model.added_against(&before.graph());
    assert_eq!(
        ops.len(),
        1,
        "one row: the second impl, which closes both loops"
    );
    let report = both.episode(&ops, &model.naming(), &Ratchet::empty(), now());
    assert_eq!(
        report.delta.introduced[0].members,
        vec!["a.rs", "b.rs"],
        "the module view is still the primary answer"
    );
    let symbol = report.symbol.expect("the symbol view was asked for");
    assert_eq!(symbol.introduced.len(), 1);
    assert_eq!(
        symbol.introduced[0].members,
        vec!["a.rs::Alpha", "b.rs::Beta"],
        "and it names the declarations, not just the files"
    );
}

// ---------------------------------------------------------------------------
// the delta: what an episode introduced, resolved, or grew
// ---------------------------------------------------------------------------

/// The instant the debt tests argue against. Nothing here reads a clock (`fathom_core::time`).
fn now() -> Timestamp {
    Timestamp::from_unix_micros(1_700_000_000 * MICROS_PER_SEC)
}

/// A date `days` after [`now`], as the baseline file would write it.
fn date_in(days: i64) -> String {
    let stamp = now().shift_days(days);
    let (year, month, day) = stamp.civil();
    format!("{year:04}-{month:02}-{day:02}")
}

#[test]
fn an_episode_that_closes_a_loop_introduces_exactly_that_cycle() {
    let mut before = Model::default();
    for path in ["a.rs", "b.rs", "c.rs"] {
        before.module(path, "");
    }
    before.edge(RowKind::Imports, "a.rs", "b.rs");
    before.edge(RowKind::Imports, "b.rs", "c.rs");

    let mut after = Model::default();
    for path in ["a.rs", "b.rs", "c.rs"] {
        after.module(path, "");
    }
    after.edge(RowKind::Imports, "a.rs", "b.rs");
    after.edge(RowKind::Imports, "b.rs", "c.rs");
    after.edge(RowKind::Imports, "c.rs", "a.rs");

    let mut tracker = tracker(&before, Options::default());
    let ops = after.added_against(&before.graph());
    assert_eq!(ops.len(), 1, "one row: the one the commit added");
    let report = tracker.episode(&ops, &after.naming(), &Ratchet::empty(), now());

    assert_eq!(report.delta.introduced.len(), 1);
    assert!(report.delta.resolved.is_empty());
    assert_eq!(
        report.delta.introduced[0].display(),
        "a.rs → b.rs → c.rs → a.rs",
        "the exact minimal path, in the order the cycle runs"
    );
    assert_eq!(report.delta.unchanged, 0);
    assert_eq!(report.introduced_by_episode(), 1);
    assert_eq!(report.new_cycles(), 1, "nothing was ever baselined");
    assert_eq!(
        report.health.nodes_in_cycles, 3,
        "and the health metric moved with it"
    );
}

#[test]
fn an_episode_that_breaks_a_loop_resolves_it_and_the_components_split() {
    let before = triangle();
    let mut after = Model::default();
    for path in ["a.rs", "b.rs", "c.rs"] {
        after.module(path, "");
    }
    after.edge(RowKind::Imports, "a.rs", "b.rs");
    after.edge(RowKind::Imports, "b.rs", "c.rs");

    let mut tracker = tracker(&before, Options::default());
    let ops = after.added_against(&before.graph());
    let report = tracker.episode(&ops, &after.naming(), &Ratchet::empty(), now());

    assert_eq!(report.delta.resolved.len(), 1);
    assert_eq!(
        report.delta.resolved[0].display(),
        "a.rs → b.rs → c.rs → a.rs",
        "a resolved cycle is reported with the path it had, so the reviewer can check it is the right one"
    );
    assert!(report.delta.introduced.is_empty());
    assert_eq!(report.health.cycles, 0);
    assert_eq!(report.health.nodes_in_cycles, 0);
}

#[test]
fn a_cycle_that_grew_is_changed_and_never_counted_as_resolved() {
    // The distinction the ratchet depends on: `a ↔ b` becoming `a → b → c → a` is more debt, not one
    // cycle paid off and another taken on.
    let mut before = Model::default();
    for path in ["a.rs", "b.rs"] {
        before.module(path, "");
    }
    before.edge(RowKind::Imports, "a.rs", "b.rs");
    before.edge(RowKind::Imports, "b.rs", "a.rs");

    let mut after = Model::default();
    for path in ["a.rs", "b.rs", "c.rs"] {
        after.module(path, "");
    }
    after.edge(RowKind::Imports, "a.rs", "b.rs");
    after.edge(RowKind::Imports, "b.rs", "c.rs");
    after.edge(RowKind::Imports, "c.rs", "a.rs");

    let mut tracker = tracker(&before, Options::default());
    let ops = after.added_against(&before.graph());
    let report = tracker.episode(&ops, &after.naming(), &Ratchet::empty(), now());

    let changed: Vec<&Change> = report.delta.changed.iter().collect();
    assert_eq!(changed.len(), 1, "one cycle, reshaped: {:?}", report.delta);
    assert!(changed[0].grew());
    assert_eq!(changed[0].was.members.len(), 2);
    assert_eq!(changed[0].now.members.len(), 3);
    assert!(
        report.delta.resolved.is_empty(),
        "the old key did not get paid off, it got refinanced"
    );
    assert!(changed[0].sentence().starts_with("cycle grew"));
}

#[test]
fn two_unrelated_cycles_do_not_pair_up_as_a_change() {
    // A resolved cycle here and an introduced cycle there must stay two findings: the overlap pairing
    // is for growth, not an excuse.
    let mut before = Model::default();
    for path in ["old-a.rs", "old-b.rs", "filler.rs"] {
        before.module(path, "");
    }
    before.edge(RowKind::Imports, "old-a.rs", "old-b.rs");
    before.edge(RowKind::Imports, "old-b.rs", "old-a.rs");

    let mut after = before.clone_shapes();
    after.drop_module("old-a.rs");
    after.drop_module("old-b.rs");
    after.module("new-a.rs", "");
    after.module("new-b.rs", "");
    after.edge(RowKind::Imports, "new-a.rs", "new-b.rs");
    after.edge(RowKind::Imports, "new-b.rs", "new-a.rs");

    let mut tracker = tracker(&before, Options::default());
    let ops = after.added_against(&before.graph());
    let report = tracker.episode(&ops, &after.naming(), &Ratchet::empty(), now());
    assert_eq!(report.delta.resolved.len(), 1);
    assert_eq!(report.delta.introduced.len(), 1);
    assert!(report.delta.changed.is_empty());
}

#[test]
fn the_maintained_partition_matches_a_full_recomputation_after_every_revision() {
    // The acceptance criterion behind the whole module: maintained, not recomputed — and maintained
    // *correctly*, which is only checkable against the slow answer. Scripted, not random: a failure has
    // to reproduce on the machine that printed it.
    let mut current = Model::default();
    for index in 0..12 {
        current.module(&format!("m{index}.rs"), "");
    }
    let mut tracker: Option<Tracker> = None;
    let mut revisions = 0_usize;

    for step in 0..20 {
        let mut next = current.clone_shapes();
        let from = (step * 5) % 12;
        let to = (step * 7 + 1) % 12;
        if step % 3 == 2 {
            next.drop_edge(
                RowKind::Imports,
                &format!("m{from}.rs"),
                &format!("m{to}.rs"),
            );
        } else {
            next.edge(
                RowKind::Imports,
                &format!("m{from}.rs"),
                &format!("m{to}.rs"),
            );
        }
        let graph_before = current.graph();
        let ops = next.added_against(&graph_before);
        let naming = next.naming();
        let entry = tracker
            .get_or_insert_with(|| Tracker::build(&graph_before, &naming, Options::default()));
        entry.episode(&ops, &naming, &Ratchet::empty(), now());
        assert!(
            entry.verify(&next.graph(), &naming),
            "step {step}: the maintained partition disagrees with a full recompute"
        );
        current = next;
        revisions += 1;
    }
    assert_eq!(revisions, 20);
    let state = tracker
        .as_ref()
        .and_then(|tracker| tracker.state(View::Module))
        .expect("a module view");
    assert_eq!(
        state.full_recomputes(),
        1,
        "one full computation, twenty episodes later"
    );
    assert!(state.nodes_searched() > 0, "and real work in between");
}

impl Model {
    /// A copy that can be edited further, for the scripted history above.
    fn clone_shapes(&self) -> Self {
        Self {
            ids: self.ids.clone(),
            layers: self.layers.clone(),
            rows: self.rows.clone(),
            next: self.next,
        }
    }

    /// Take a module out of the revision, the way a deleted file would: its node row and every row
    /// that mentions it.
    fn drop_module(&mut self, path: &str) {
        let id = self.id(path);
        self.rows
            .retain(|row| row.1 != id && (row.0.is_node() || row.2 != id));
        self.rows.retain(|row| row.1 != id);
    }

    fn drop_edge(&mut self, kind: RowKind, from: &str, to: &str) {
        let (src, dst) = (self.id(from), self.id(to));
        self.rows.retain(|row| *row != (kind, src, dst));
    }
}

#[test]
fn a_cycle_the_extractor_added_and_removed_in_one_revision_is_quiet() {
    // Every moved dependency looks like this: a row out here, a row in there, the cycles unchanged.
    // The edge that moves is outside the cycle here, which is the common case in a real commit.
    let mut before = triangle();
    before.module("util.rs", "");
    before.edge(RowKind::Imports, "a.rs", "util.rs");
    let mut after = before.clone_shapes();
    after.drop_edge(RowKind::Imports, "a.rs", "util.rs");
    after.edge(RowKind::Imports, "b.rs", "util.rs");

    let mut tracker = tracker(&before, Options::default());
    let ops = after.added_against(&before.graph());
    let report = tracker.episode(&ops, &after.naming(), &Ratchet::empty(), now());
    assert!(report.delta.is_empty(), "{:?}", report.delta);
    assert_eq!(report.health.cycles, 1);
    assert!(report.delta.sentence().contains("no cycle change"));
}

#[test]
fn a_report_bound_says_it_stopped_counting() {
    let mut model = Model::default();
    for index in 0..8 {
        model.module(&format!("p{index}.rs"), "");
        model.module(&format!("q{index}.rs"), "");
        model.edge(
            RowKind::Imports,
            &format!("p{index}.rs"),
            &format!("q{index}.rs"),
        );
        model.edge(
            RowKind::Imports,
            &format!("q{index}.rs"),
            &format!("p{index}.rs"),
        );
    }
    let empty = Graph::default();
    let mut tracker = Tracker::build(
        &empty,
        &Naming::default(),
        Options {
            max_reported: 2,
            ..Options::default()
        },
    );
    let ops = model.added_against(&empty);
    let report = tracker.episode(&ops, &model.naming(), &Ratchet::empty(), now());
    assert_eq!(report.health.cycles, 8, "the count is never truncated");
    assert_eq!(report.delta.introduced.len(), 2, "the list is");
    assert!(report.truncated);
    assert!(report.delta.sentence().contains("list capped"));
}

// ---------------------------------------------------------------------------
// the debt ledger
// ---------------------------------------------------------------------------

/// A baseline document in the published shape, with one allowance.
fn baseline(key: &str, owner: &str, expires: &str) -> String {
    format!(
        "[[allowance]]\nkind = \"cycle\"\nkey = \"{key}\"\nowner = \"{owner}\"\nexpires = \
         \"{expires}\"\nreason = \"the split lands next quarter\"\n"
    )
}

#[test]
fn a_baselined_cycle_is_debt_with_its_owner_and_expiry_and_never_a_violation() {
    let model = triangle();
    let ledger = Ratchet::from_text(
        &baseline("c.rs → a.rs → b.rs → c.rs", "team-payments", &date_in(40)),
        "ratchet.toml",
    );
    let tracker = tracker(&model, Options::default());
    let report = tracker.snapshot(&ledger, now());

    assert_eq!(report.new_cycles(), 0, "a baselined cycle is never news");
    assert!(report.verdict.violations.is_empty());
    assert!(!report.alarms());
    assert_eq!(report.baselined_cycles(), 1);
    let debt = &report.verdict.debt[0];
    assert_eq!(debt.owner, "team-payments");
    assert_eq!(debt.days_left, Some(40));
    assert!(debt.sentence().contains("team-payments"));
    assert!(debt.sentence().contains("40 day(s) left"));
    assert!(report.verdict.sentence().contains("baselined as debt"));
}

#[test]
fn an_allowance_written_as_a_set_matches_the_same_cycle_as_one_written_as_a_path() {
    let model = triangle();
    for spelling in [
        "a.rs → b.rs → c.rs → a.rs",
        "b.rs → c.rs → a.rs → b.rs",
        "c.rs → b.rs → a.rs → c.rs",
        "a.rs, b.rs, c.rs",
        "a.rs|b.rs|c.rs",
        "c.rs -> a.rs -> b.rs -> c.rs",
    ] {
        let ledger = Ratchet::from_text(&baseline(spelling, "team", &date_in(10)), "ratchet.toml");
        let report = tracker(&model, Options::default()).snapshot(&ledger, now());
        assert_eq!(
            report.new_cycles(),
            0,
            "the spelling {spelling:?} did not match the cycle"
        );
    }
    assert_eq!(
        normalize_key("a.rs → b.rs → a.rs"),
        normalize_key("b.rs|a.rs"),
        "and the two spellings are one key"
    );
}

#[test]
fn an_expired_allowance_brings_the_cycle_back_as_news_with_its_date() {
    let model = triangle();
    let ledger = Ratchet::from_text(
        &baseline("a.rs → b.rs → c.rs → a.rs", "team-payments", &date_in(-3)),
        "ratchet.toml",
    );
    let report = tracker(&model, Options::default()).snapshot(&ledger, now());

    assert_eq!(report.new_cycles(), 1, "the forgiveness lapsed");
    assert_eq!(report.verdict.back.len(), 1, "and it says why");
    let violation = &report.verdict.violations[0];
    assert!(violation.why.contains("expired"), "{}", violation.why);
    assert!(violation.why.contains("team-payments"));
    assert!(report.verdict.sentence().contains("came back due"));
}

#[test]
fn an_allowance_is_live_for_the_whole_of_its_expiry_day() {
    let model = triangle();
    let ledger = Ratchet::from_text(
        &baseline("a.rs → b.rs → c.rs → a.rs", "team", &date_in(0)),
        "ratchet.toml",
    );
    let report = tracker(&model, Options::default()).snapshot(&ledger, now());
    assert_eq!(
        report.new_cycles(),
        0,
        "a reviewer who wrote today's date gets today"
    );
    let later = tracker(&model, Options::default()).snapshot(&ledger, now().shift_days(1));
    assert_eq!(later.new_cycles(), 1, "and not tomorrow");
}

#[test]
fn an_allowance_for_a_cycle_that_is_gone_is_offered_as_a_tightening() {
    let mut model = Model::default();
    model.module("lonely.rs", "");
    let ledger = Ratchet::from_text(
        &baseline("gone-a.rs → gone-b.rs → gone-a.rs", "team", &date_in(10)),
        "ratchet.toml",
    );
    let report = tracker(&model, Options::default()).snapshot(&ledger, now());
    assert_eq!(report.verdict.stale.len(), 1);
    assert_eq!(report.verdict.stale[0].owner, "team");
    assert!(report.verdict.sentence().contains("could be removed"));
    // Offered, never applied: the file is the record of a decision, and this module does not delete it.
    assert_eq!(ledger.allowances().len(), 1);
}

#[test]
fn an_uncovered_cycle_is_news_at_warn_and_a_failure_at_deny() {
    let model = triangle();
    for (mode, alarms, exit) in [
        ("off", false, None),
        ("warn", true, None),
        ("deny", true, Some(7)),
    ] {
        let ledger = Ratchet::from_text(
            &format!(
                "[[allowance]]\nkind = \"layer\"\nkey = \"other\"\nowner = \"x\"\nreason = \"y\"\n"
            ),
            "ratchet.toml",
        );
        let mut ledger = ledger;
        ledger.mode = Mode::parse(mode).expect("a mode");
        let report = tracker(&model, Options::default()).snapshot(&ledger, now());
        assert_eq!(
            report.new_cycles(),
            1,
            "{mode} suppresses nothing but its own alarm"
        );
        assert_eq!(report.alarms(), alarms, "{mode}");
        assert_eq!(report.exit_code().map(|code| code.as_i32()), exit, "{mode}");
    }
    assert_eq!(
        Ratchet::config("[ratchet]\nmode = \"deny\"\nbaseline = \"etc/ok.toml\"\n").0,
        Mode::Deny,
        "and the config keys are read, not assumed"
    );
    let (_, baseline_path, _) = Ratchet::config("[ratchet]\nmode = \"squint\"\nbaseline = \"x\"\n");
    assert_eq!(baseline_path, "x");
    let (_, _, notices) = Ratchet::config("[ratchet]\nmode = \"squint\"\n");
    assert!(
        notices[0].contains("not `off`, `warn` or `deny`"),
        "{notices:?}"
    );
    assert_eq!(
        Ratchet::config("").0,
        Mode::Warn,
        "no [ratchet] table is the documented default"
    );
}

#[test]
fn a_baseline_file_that_is_not_there_is_stated_rather_than_fabricated() {
    let (ledger, notices) = Ratchet::read(std::path::Path::new("/nonexistent-here"), "");
    assert!(!ledger.loaded);
    assert_eq!(ledger.cycle_allowances(), 0);
    assert!(
        notices
            .iter()
            .any(|notice| notice.contains("every cycle in this revision counts as new")),
        "{notices:?}"
    );
}

#[test]
fn a_baseline_reads_the_published_shape_and_says_what_it_refused() {
    let text = r#"
[[allowance]]
kind = "cycle"
key = "billing → invoicing → billing"
owner = "team-payments"
expires = "2026-03-31"
reason = "split lands with the ledger rewrite"

[[allowance]]
kind = "layer"
key = "api → core → api"
owner = "team-core"
expires = "2026-04-30"
reason = "not this module's business"

[[allowance]]
owner = "nobody"
reason = "no key, so nothing is forgiven"
"#;
    let ledger = Ratchet::from_text(text, ".fathom/ratchet.toml");
    assert_eq!(ledger.cycle_allowances(), 1);
    assert_eq!(
        ledger.other_kinds, 1,
        "the file is shared, and the other kinds are left alone"
    );
    assert_eq!(
        ledger.refused, 1,
        "and a row with no key is refused out loud"
    );
    assert_eq!(ledger.allowances()[0].owner, "team-payments");
    assert_eq!(
        ledger.allowances()[0].expires,
        parse_date("2026-03-31"),
        "the date is the first instant after the day, so the day itself is covered"
    );
    assert!(
        ledger.notices.iter().any(|n| n.contains("no key")),
        "{:?}",
        ledger.notices
    );
}

#[test]
fn a_rendered_baseline_round_trips() {
    let ledger = Ratchet::from_text(
        &baseline("a.rs → b.rs → a.rs", "team", &date_in(20)),
        "ratchet.toml",
    );
    let written = ledger.render();
    let reread = Ratchet::from_text(&written, "ratchet.toml");
    let strip = |rows: &[Allowance]| {
        rows.iter()
            .map(|row| {
                let mut copy = row.clone();
                copy.line = 0; // the line number is where the parser found it, not part of the row
                copy
            })
            .collect::<Vec<Allowance>>()
    };
    assert_eq!(strip(reread.allowances()), strip(ledger.allowances()));
    assert_eq!(reread.cycle_allowances(), 1);
}

#[test]
fn the_allowance_a_writer_would_emit_is_the_canonical_key() {
    // `e3.10` owns the verb; this is the half it needs, and it has to agree with the matcher above.
    let model = triangle();
    let cycle = one(cycles::detect(
        &model.graph(),
        &model.naming(),
        Options::default(),
    ));
    // Written from a date, as a writer would: `to_toml` prints the last whole day an allowance covers,
    // so an expiry that came from a date round-trips exactly and an arbitrary instant does not.
    let expiry = parse_date(&date_in(20)).expect("a date this test wrote");
    let allowance = Allowance::for_cycle(&cycle, "me", Some(expiry), "because");
    assert_eq!(allowance.pattern, cycle.key());
    let ledger = Ratchet::from_text(&allowance.to_toml(), "ratchet.toml");
    let report = tracker(&model, Options::default()).snapshot(&ledger, now());
    assert_eq!(
        report.new_cycles(),
        0,
        "what the writer writes is what the matcher matches"
    );
}

// ---------------------------------------------------------------------------
// the exported metric
// ---------------------------------------------------------------------------

#[test]
fn the_health_metric_counts_nodes_inside_cycles_and_exports_the_four_numbers() {
    // Two disjoint cycles of two and three, plus three modules outside any cycle.
    let mut model = Model::default();
    for path in [
        "a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "free1.rs", "free2.rs", "free3.rs",
    ] {
        model.module(path, "");
    }
    model.edge(RowKind::Imports, "a.rs", "b.rs");
    model.edge(RowKind::Imports, "b.rs", "a.rs");
    model.edge(RowKind::Imports, "c.rs", "d.rs");
    model.edge(RowKind::Imports, "d.rs", "e.rs");
    model.edge(RowKind::Imports, "e.rs", "c.rs");

    let health = cycles::health(&model.graph(), &model.naming(), Options::default());
    assert_eq!(health.cycles, 2);
    assert_eq!(
        health.nodes_in_cycles, 5,
        "the trendable number is nodes, not cycles"
    );
    assert_eq!(health.nodes, 8);
    assert_eq!(health.largest_members, 3);
    assert!((health.share() - 0.625).abs() < 1e-9);
    assert!(health.complete());
    assert!(health.sentence().contains("5 of 8 nodes (62.5%)"));

    let tracker = tracker(&model, Options::default());
    let json = tracker.snapshot(&Ratchet::empty(), now()).to_json();
    for key in [
        "cycles",
        "nodes_in_cycles",
        "new_cycles",
        "baselined_cycles",
    ] {
        assert!(json.get(key).is_some(), "{key} is not exported");
    }
    assert_eq!(json["cycles"].as_u64(), Some(2));
    assert_eq!(json["nodes_in_cycles"].as_u64(), Some(5));
    assert_eq!(json["new_cycles"].as_u64(), Some(2));
    assert_eq!(json["baselined_cycles"].as_u64(), Some(0));
    assert_eq!(
        json["health"]["nodes_in_cycles_share"].as_f64(),
        Some(0.625)
    );
    assert_eq!(json["cycle_list"].as_array().map(Vec::len), Some(2));
}

#[test]
fn an_empty_graph_answers_zero_and_does_not_divide_by_it() {
    let empty = Graph::default();
    let naming = Naming::default();
    let health = cycles::health(&empty, &naming, Options::default());
    assert_eq!(health.cycles, 0);
    assert_eq!(health.share(), 0.0);
    assert!(health.sentence().contains("no modules to ask about"));
}

#[test]
fn a_dag_reports_no_cycles_at_all() {
    let mut model = Model::default();
    for path in ["cli.rs", "analyze.rs", "store.rs"] {
        model.module(path, "");
    }
    model.edge(RowKind::Imports, "cli.rs", "analyze.rs");
    model.edge(RowKind::Imports, "analyze.rs", "store.rs");

    let report = tracker(&model, Options::default()).snapshot(&Ratchet::empty(), now());
    assert_eq!(report.health.cycles, 0);
    assert_eq!(report.delta.unchanged, 0);
    assert!(
        report
            .verdict
            .sentence()
            .contains("no cycles in this revision")
    );
}

#[test]
fn a_delta_with_no_cycle_is_still_a_delta_with_a_shape() {
    // The JSON a surface consumes must not depend on there being something to show.
    let mut before = Model::default();
    before.module("x.rs", "");
    before.module("y.rs", "");
    before.edge(RowKind::Imports, "x.rs", "y.rs");
    let mut after = before.clone_shapes();
    after.module("z.rs", "");
    after.edge(RowKind::Imports, "y.rs", "z.rs");

    let mut tracker = tracker(&before, Options::default());
    let ops = after.added_against(&before.graph());
    let report = tracker.episode(&ops, &after.naming(), &Ratchet::empty(), now());
    let json = report.to_json();
    assert_eq!(
        json["delta"]["introduced"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(json["counters"]["full_recomputes"].as_u64(), Some(1));
    assert_eq!(json["counters"]["views"].as_u64(), Some(1));
    assert!(
        json["counters"]["updates"].as_u64().unwrap_or(0) > 0,
        "the update happened, and it was counted"
    );
}

#[test]
fn a_wide_dependency_graph_costs_the_region_and_not_the_graph() {
    // 500 modules in a chain, one cycle closed at the far end. Closing a loop near the front must touch
    // the region between the endpoints and nothing else; the bench measures the same claim in ms, and
    // this asserts it in nodes so a regression is a failure and not a slower CI run.
    let mut model = Model::default();
    for index in 0..500 {
        model.module(&format!("n{index:03}.rs"), "");
    }
    for index in 0..499 {
        model.edge(
            RowKind::Imports,
            &format!("n{index:03}.rs"),
            &format!("n{:03}.rs", index + 1),
        );
    }
    let before = model.clone_shapes();
    model.edge(RowKind::Imports, "n004.rs", "n002.rs");

    let mut tracker = tracker(&before, Options::default());
    let before_nodes = tracker
        .state(View::Module)
        .map_or(0, |state| state.nodes_searched());
    let ops = model.added_against(&before.graph());
    tracker.episode(&ops, &model.naming(), &Ratchet::empty(), now());
    let after_nodes = tracker
        .state(View::Module)
        .map_or(0, |state| state.nodes_searched());
    let searched = after_nodes - before_nodes;
    assert!(
        searched < 100,
        "closing one loop searched {searched} nodes; the region was supposed to be local"
    );
    let report = tracker.snapshot(&Ratchet::empty(), now());
    assert_eq!(report.health.cycles, 1);
    assert_eq!(report.health.nodes_in_cycles, 3);
}

#[test]
fn a_delta_carries_the_counts_that_explain_its_own_shape() {
    let before = triangle();
    let mut after = before.clone_shapes();
    after.module("d.rs", "");
    after.edge(RowKind::Imports, "c.rs", "d.rs");
    after.edge(RowKind::Imports, "d.rs", "a.rs");

    let mut tracker = tracker(&before, Options::default());
    let ops = after.added_against(&before.graph());
    let report = tracker.episode(&ops, &after.naming(), &Ratchet::empty(), now());
    let delta: &CycleDelta = &report.delta;
    assert_eq!(
        delta.affected.merges, 1,
        "the new module merged into the cycle: that is what a growth is, structurally"
    );
    assert_eq!(delta.changed.len(), 1, "and it reads as one changed cycle");
    let json = delta.to_json();
    assert_eq!(json["view"].as_str(), Some("module"));
    assert_eq!(
        json["unchanged"].as_u64(),
        Some(0),
        "the key moved, so nothing is unchanged — and `changed` is where the story is"
    );
    assert_eq!(json["merges"].as_u64(), Some(1));
    assert_eq!(json["truncated"].as_bool(), Some(false));
}

#[test]
fn two_rows_onto_one_module_edge_leave_the_edge_until_the_last_one_goes() {
    // The module view is a projection, so several symbol rows can be one module edge. That multiplicity
    // is where an incremental implementation breaks most quietly: if the backward adjacency gives up the
    // pair on the first row while the forward side waits for the last, the split search still sees an
    // edge that is gone and the cycle never resolves. It looks like a detector that missed a deletion,
    // which is a far worse bug than a crash.
    let mut before = Model::default();
    for path in ["a.rs", "b.rs"] {
        before.module(path, "");
    }
    before.symbol("a.rs", "first");
    before.symbol("a.rs", "second");
    before.symbol("b.rs", "target");
    // Two symbol rows, one module edge: a.rs → b.rs.
    before.edge(RowKind::Uses, "a.rs::first", "b.rs::target");
    before.edge(RowKind::Uses, "a.rs::second", "b.rs::target");
    // And the way back, at module grain.
    before.edge(RowKind::Imports, "b.rs", "a.rs");
    assert_eq!(detect(&before).len(), 1, "a.rs ↔ b.rs");

    // Drop one of the two symbol rows. The module edge stands, so the cycle stands.
    let mut half = before.clone_shapes();
    half.drop_edge(RowKind::Uses, "a.rs::second", "b.rs::target");
    let mut tracker = tracker(&before, Options::default());
    let ops = half.added_against(&before.graph());
    assert_eq!(ops.len(), 1, "one row goes");
    let report = tracker.episode(&ops, &half.naming(), &Ratchet::empty(), now());
    assert!(
        report.delta.resolved.is_empty() && report.delta.introduced.is_empty(),
        "one of two rows is not the edge going away: {:?}",
        report.delta
    );
    assert_eq!(report.health.cycles, 1);
    assert!(
        tracker.verify(&half.graph(), &half.naming()),
        "and it half-way too"
    );

    // Drop the other. Now the module edge is gone and so is the cycle.
    let mut gone = half.clone_shapes();
    gone.drop_edge(RowKind::Uses, "a.rs::first", "b.rs::target");
    let ops = gone.added_against(&half.graph());
    let report = tracker.episode(&ops, &half.naming(), &Ratchet::empty(), now());
    assert_eq!(
        report.delta.resolved.len(),
        1,
        "the last row was the edge, so the cycle is owed"
    );
    assert_eq!(report.health.cycles, 0);
    assert_eq!(report.health.nodes_in_cycles, 0);
    assert!(
        tracker.verify(&gone.graph(), &gone.naming()),
        "and the maintained components agree with a cold computation"
    );
}

#[test]
fn an_incremental_update_on_five_thousand_nodes_answers_inside_its_budget() {
    // The acceptance criterion, made a test: a graph the size of a real repository, one episode, and the
    // wall clock watching. `--release` is where the number is meaningful, so the search bound is asserted
    // unconditionally and the clock is asserted with room to spare — a debug build is not the deal, but a
    // hundredfold regression in either number must fail here either way.
    const NODES: usize = 5_000;
    let mut model = Model::default();
    for index in 0..NODES {
        model.module(&format!("m{index:05}.rs"), "");
    }
    // A spine, a fan, and one cycle parked at the far end of the graph — the shape a real dependency
    // graph has, where a loop sits in one corner and everything else flows past it.
    for index in 0..NODES - 1 {
        model.edge(
            RowKind::Imports,
            &format!("m{index:05}.rs"),
            &format!("m{:05}.rs", index + 1),
        );
    }
    for index in (0..NODES - 7).step_by(7) {
        model.edge(
            RowKind::Imports,
            &format!("m{index:05}.rs"),
            &format!("m{:05}.rs", index + 6),
        );
    }
    model.edge(RowKind::Imports, "m04997.rs", "m04995.rs");
    let before = model.clone_shapes();
    assert_eq!(detect(&before).len(), 1, "the parked cycle is found cold");

    // The episode: one row near the front, closing a different loop three nodes away.
    model.edge(RowKind::Imports, "m00003.rs", "m00001.rs");

    let mut tracker = tracker(&before, Options::default());
    let searched_before = tracker
        .state(View::Module)
        .map_or(0, |state| state.nodes_searched());
    let ops = model.added_against(&before.graph());
    let started = std::time::Instant::now();
    let report = tracker.episode(&ops, &model.naming(), &Ratchet::empty(), now());
    let elapsed = started.elapsed();
    let searched = tracker
        .state(View::Module)
        .map_or(0, |state| state.nodes_searched())
        - searched_before;

    assert_eq!(report.delta.introduced.len(), 1);
    assert_eq!(report.health.cycles, 2, "the parked one and the new one");
    assert_eq!(report.health.nodes_in_cycles, 6, "two loops of three");
    assert!(
        searched < 4 * NODES,
        "the update searched {searched} nodes on a {NODES}-node graph"
    );
    assert_eq!(
        tracker
            .state(View::Module)
            .map_or(0, |state| state.full_recomputes()),
        1,
        "one whole-graph pass, at build, and no more"
    );
    assert!(
        tracker.verify(&model.graph(), &model.naming()),
        "and the maintained answer is the answer"
    );
    let millis = elapsed.as_millis();
    assert!(
        millis < 100,
        "the episode took {millis} ms on a {NODES}-node graph; the budget is 100 ms"
    );
}

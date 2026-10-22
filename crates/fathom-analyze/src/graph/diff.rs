//! The architecture delta: what one revision — or a whole span of them — did to the graph
//! (`e3.3`, `reco-8wg`).
//!
//! [`crate::graph::delta`] answers "which rows differ". This answers the question a reviewer
//! actually asks, which is a different question: *"what happened to the architecture?"* A reviewer
//! does not read a list of 1 431 rows; they read "the `Store` type moved from `store.rs` to
//! `store/persist.rs`, `api.rs` and `core.rs` now point at each other, and `models.py`'s public
//! surface gained one function". So this module takes the row difference and turns it into the six
//! findings the ticket names, each of which is a *different fact* about the same rows:
//!
//! | Finding | Why it is not just two rows |
//! |---|---|
//! | nodes and edges added, removed | the raw difference, kept in full so nothing is lost |
//! | a symbol that **moved** module | delete + add is the wrong answer: it charges the old module for a loss it did not suffer and hides that the dependency survived |
//! | an edge **re-pointed** or re-kinded | the dependency is the same one; what changed is how well it is known |
//! | a **dependency inversion** | `A→B` removed and `B→A` added in one revision is one event, not two unrelated churns |
//! | a file whose **public surface** changed | the export list moved, which is the half an API question needs |
//! | per-module **edge churn** | "this module keeps getting reworked" is a rate over history, not a property of one revision |
//!
//! # `changed` is four things, not one
//!
//! The ticket says "nodes and edges added/removed/changed". A graph row has no field to change:
//! `(kind, src, dst)` is the whole row and it is immutable. What *looks* like a change decomposes
//! into four things that each mean something different, and this module keeps them apart for the same
//! reason [`crate::graph::query::Fan`] keeps named, wildcard and package-granular callers apart — a
//! folded answer is a lost fact:
//!
//! 1. a **move** — the same symbol, a different module ([`Move`]);
//! 2. a **re-point** — the same two endpoints, a different kind ([`Repoint`]: a named import that
//!    became a star import got *less certain*, which is the opposite of what an added/removed pair
//!    reads like);
//! 3. a **reversal** — the arrow between two modules turned around ([`Reversal`]);
//! 4. a **re-point that a move caused** — a third module's `uses old::X` became `uses new::X`; that
//!    is the move's invoice, not the third module changing its mind ([`Move::retargets`]).
//!
//! One thing genuinely is not here, and says so: a `[layers]` re-tag of a module nobody edited writes
//! **no rows at all** (the tag is denormalised onto rows the revision rewrote), so a re-tag cannot be
//! seen in a delta. It is visible in the dictionary and in `e3.4`'s layering rules, and
//! [`ArchitectureDelta::layer_drift`] is the place that admits the tags a revision stamped are not
//! what the config says now.
//!
//! # The cost of an answer is part of the answer
//!
//! `s3` measured the two ends of this at **0.194 ms p95** for a SELECT over stored delta rows and
//! **272 ms p95** for a strategy that has to rebuild — a thousand-fold difference on the same
//! question, which is the whole reason [`crate::graph::policy`] stores deltas. So every
//! [`ArchitectureDelta`] carries its [`Cost`], and there are five rungs for a pair the store can
//! connect, plus one for a caller that already holds both graphs:
//!
//! | Rung | Work | When |
//! |---|---|---|
//! | [`Rung::Empty`] | one `graph_snapshot` row, zero delta rows, zero chunks | the honest "nothing architectural changed", and it is O(1) |
//! | [`Rung::Adjacent`] | one SELECT over `graph_delta` for `to` | `to.base_tree == from` — the common case, and the one ADR-004 §11 budgets |
//! | [`Rung::Chain`] | k SELECTs, no chunk decoded | `from` is on `to`'s delta chain inside `graph.query.max_chain_walk` |
//! | [`Rung::SharedAnchor`] | two SELECT runs to one anchor, one of them inverted | both revisions sit on the same anchor — two episodes in one anchor window |
//! | [`Rung::Materialised`] | two full reconstructions | anything else, and it says so |
//! | [`Rung::InHand`] | a set difference, nothing read | the caller already holds both graphs |
//!
//! [`Rung::Chain`] and [`Rung::SharedAnchor`] exist because "adjacent or else two materialisations"
//! is a false dichotomy: a stored delta chain can answer a *span* by composing, which is what the
//! third acceptance criterion asserts and what [`compose`] implements. Composition is cancellation,
//! not concatenation — a row removed and re-added inside the span is not a change, and a chain that
//! reported it as one would disagree with the direct diff of the same pair.
//!
//! # Determinism, because this is a document
//!
//! Nothing here reads a clock, a filesystem or an environment variable; every list is sorted by the
//! key it is named by, every map is a `BTreeMap`, and the JSON form is produced from that order, so
//! the same pair produces the same bytes in the same process, in a second process, and whichever
//! order the rows arrived in. That is what makes the form safe to put in an exported document or in
//! an MCP response: two agents asking the same question must not see two documents.
//!
//! # What it is not allowed to claim
//!
//! * A **move** is only asserted where the pairing is derivable from the revision. Two modules that
//!   both lost `parse` and two that both gained it do not make a pairing, and
//!   [`Move::confident`] is false for the four-way case — the rows stay in
//!   [`ArchitectureDelta::nodes_added`] and [`ArchitectureDelta::nodes_removed`] where a reader can
//!   see both halves, exactly as `e2.4` refuses to call a 1→N split a rename pair.
//! * A **shrunken export list** on a module whose declaration list *may* have shrunk rather than
//!   shrunk ([`s1` finding 1](crate::graph::query)) is reported with that flag on it, never as a
//!   removal.
//! * Whether a removed edge was **live** or already **dead** — pointing at a target nobody declared —
//!   is only knowable from the revision it was read against. [`EdgeFact::dead`] is an `Option` and
//!   the answer says which of the two it is; a cheap store-backed answer does not guess.
//!
//! [`s1` finding 1]: crate::graph::query

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::graph::model::{Graph, Op, Row, RowKind};
use crate::graph::query::{Naming, module_of};

/// The contract id a machine-readable delta carries, so a reader knows which shape it is holding.
pub const CONTRACT: &str = "fathom.graph-delta/1";

/// A module touched in fewer than this many revisions of a window is not "reworked".
const ACTIVE_MIN_REVISIONS: usize = 2;
/// A module touched in at least this many revisions, and enough edges, is reworked.
const REWORK_MIN_REVISIONS: usize = 3;
/// The edge-events floor that goes with [`REWORK_MIN_REVISIONS`].
const REWORK_MIN_EDGES: usize = 8;
/// How many modules a churn report names as its hot list.
pub const HOT_MODULES: usize = 3;

/// How an answer was obtained. The variant *is* the measurement claim: a surface that prints a
/// number from an answer has to print which rung produced it, because a 0.02 ms answer and a 45 ms
/// answer are the same shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rung {
    /// One `graph_snapshot` row said the revision wrote no graph rows at all: no architectural
    /// change, stated without reading the delta, the chunks, or the graph.
    Empty,
    /// One indexed SELECT over `graph_delta` for `to`. The common case, and the one
    /// `query.graph_delta_adjacent.ms` budgets at 15 ms p95.
    Adjacent,
    /// A run of stored deltas, composed. Bounded by `graph.query.max_chain_walk`.
    Chain,
    /// Two runs of stored deltas meeting at one anchor, one of them inverted: no chunk is decoded,
    /// which is what makes a pair of episodes inside one anchor window cheap.
    SharedAnchor,
    /// Two full reconstructions. The expensive answer, and the one every representation without
    /// stored deltas is forced into for every question.
    Materialised,
    /// Both graphs were already in the caller's hand, so the work was a set difference.
    InHand,
}

impl Rung {
    /// The stored-ish spelling, for JSON and for a pane.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Rung::Empty => "empty",
            Rung::Adjacent => "adjacent",
            Rung::Chain => "chain",
            Rung::SharedAnchor => "shared-anchor",
            Rung::Materialised => "materialised",
            Rung::InHand => "in-hand",
        }
    }

    /// Whether this rung read stored deltas instead of rebuilding. The claim `s3`'s policy exists to
    /// earn, stated as a predicate rather than as a comment.
    #[must_use]
    pub const fn from_store(self) -> bool {
        matches!(
            self,
            Rung::Empty | Rung::Adjacent | Rung::Chain | Rung::SharedAnchor
        )
    }

    /// The sentence for the pane, which names the cost rather than hiding it.
    #[must_use]
    pub fn sentence(self) -> String {
        match self {
            Rung::Empty => "nothing architectural changed between the two".to_string(),
            Rung::Adjacent => "read as one stored delta".to_string(),
            Rung::Chain => "read as a composed run of stored deltas".to_string(),
            Rung::SharedAnchor => {
                "read as two runs of stored deltas to a shared anchor".to_string()
            }
            Rung::Materialised => {
                "both revisions had to be materialised: this pair straddles an anchor".to_string()
            }
            Rung::InHand => "diffed from two graphs already in hand".to_string(),
        }
    }
}

/// What an answer paid. Kept beside the answer, not in a log line, because the same question asked at
/// two different revisions legitimately costs two different amounts here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cost {
    /// Which rung answered.
    pub rung: Rung,
    /// `graph_delta` rows read.
    #[serde(default)]
    pub delta_rows: usize,
    /// Stored deltas read, which is the number the chain rungs bound.
    #[serde(default)]
    pub deltas: usize,
    /// Revisions reconstructed from an anchor plus deltas.
    #[serde(default)]
    pub materialisations: usize,
}

impl Default for Cost {
    /// A cost that read nothing: the neutral default for a delta built by hand.
    fn default() -> Self {
        Self::in_hand()
    }
}

impl Cost {
    /// The cost of the constant-time "nothing changed" answer.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            rung: Rung::Empty,
            delta_rows: 0,
            deltas: 0,
            materialisations: 0,
        }
    }

    /// The cost of one adjacent delta read.
    #[must_use]
    pub const fn adjacent(delta_rows: usize) -> Self {
        Self {
            rung: Rung::Adjacent,
            delta_rows,
            deltas: 1,
            materialisations: 0,
        }
    }

    /// The cost of a composed run of `deltas` stored deltas.
    #[must_use]
    pub const fn chain(delta_rows: usize, deltas: usize) -> Self {
        Self {
            rung: Rung::Chain,
            delta_rows,
            deltas,
            materialisations: 0,
        }
    }

    /// The cost of two runs meeting at an anchor.
    #[must_use]
    pub const fn shared_anchor(delta_rows: usize, deltas: usize) -> Self {
        Self {
            rung: Rung::SharedAnchor,
            delta_rows,
            deltas,
            materialisations: 0,
        }
    }

    /// The cost of two materialisations, plus any delta rows read on the way.
    #[must_use]
    pub const fn materialised(delta_rows: usize, deltas: usize) -> Self {
        Self {
            rung: Rung::Materialised,
            delta_rows,
            deltas,
            materialisations: 2,
        }
    }

    /// The cost of a difference taken between two graphs the caller already held.
    #[must_use]
    pub const fn in_hand() -> Self {
        Self {
            rung: Rung::InHand,
            delta_rows: 0,
            deltas: 0,
            materialisations: 0,
        }
    }

    /// Whether the answer read the graph at all: the cheap rungs read delta rows or nothing.
    #[must_use]
    pub const fn touched_graph(self) -> bool {
        self.materialisations > 0
    }

    /// The same sentence as [`Rung::sentence`], with the numbers after it.
    #[must_use]
    pub fn sentence(self) -> String {
        let base = self.rung.sentence();
        match self.rung {
            Rung::Empty | Rung::InHand => base,
            Rung::Adjacent => format!("{base}: {} row(s)", self.delta_rows),
            _ => format!(
                "{base}: {} delta(s), {} row(s), {} materialisation(s)",
                self.deltas, self.delta_rows, self.materialisations
            ),
        }
    }
}

/// One operation, with the layer tags the revision stamped on it.
///
/// The model's [`Op`] is `(add, kind, src, dst)` and that is right for applying it to a graph. A
/// *delta document* also has to say what the revision believed about the endpoints' layers, which is
/// why `graph_delta` carries `src_layer` and `dst_layer` at all; this is the carrier that keeps them
/// long enough to be reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedOp {
    /// The row and its direction.
    pub op: Op,
    /// The layer tag of the source as the revision stamped it; empty when untagged.
    pub src_layer: String,
    /// The layer tag of the destination; empty for a node op, where there is no destination.
    pub dst_layer: String,
}

impl TaggedOp {
    /// An op whose endpoint tags come from the naming view rather than from stored rows.
    #[must_use]
    pub fn tagged(op: Op, src_layer: impl Into<String>, dst_layer: impl Into<String>) -> Self {
        Self {
            op,
            src_layer: src_layer.into(),
            dst_layer: dst_layer.into(),
        }
    }

    /// The op with its endpoints' tags read out of a dictionary.
    #[must_use]
    pub fn from_op(op: Op, view: &Naming) -> Self {
        let (kind, src, dst) = op.row;
        Self {
            src_layer: view.layer(src).to_string(),
            dst_layer: if kind.is_node() {
                String::new()
            } else {
                view.layer(dst).to_string()
            },
            op,
        }
    }

    /// The same op pointed the other way. The inverse of a minimal delta is minimal, which is what
    /// lets a shared-anchor pair be composed without a rebuild.
    #[must_use]
    pub fn inverted(&self) -> Self {
        Self {
            op: Op {
                add: !self.op.add,
                row: self.op.row,
            },
            src_layer: self.src_layer.clone(),
            dst_layer: self.dst_layer.clone(),
        }
    }

    /// The row this op moves.
    #[must_use]
    pub fn row(&self) -> Row {
        self.op.row
    }

    /// Whether this op adds rather than removes.
    #[must_use]
    pub const fn added(&self) -> bool {
        self.op.add
    }
}

/// Compose runs of ops into the single minimal run that does the same work.
///
/// Cancellation, not concatenation. Each input run is the exact set difference between two
/// consecutive revisions, so applying them in order is the transform from the first revision to the
/// last, and the *minimal* description of that transform is what remains after every row that came
/// back to where it started cancels itself out: `remove r` then `add r` is nothing, and `add r` then
/// `remove r` is nothing. That is the whole content of the third acceptance criterion — the delta
/// over a span **is** the composition of the deltas in it — and it holds because a stored delta is
/// minimal by construction, not because this function is clever.
///
/// The output is ordered the same way [`crate::graph::delta::module_ops`] orders a single revision:
/// removals first, then additions, each by `(kind, src, dst)`, so a composed span and a single
/// revision can be compared row for row.
#[must_use]
pub fn compose<'a>(runs: impl IntoIterator<Item = &'a [TaggedOp]>) -> Vec<TaggedOp> {
    // Keyed by the row plus the *first* direction it moved in this span: a row can only ever be in
    // one of the three net states (untouched, net-added, net-removed), and a later opposite op
    // returns it to untouched.
    let mut net: BTreeMap<Row, TaggedOp> = BTreeMap::new();
    for run in runs {
        for op in run {
            match net.get(&op.row()) {
                // The row is back where it started: the pair of ops cancels.
                Some(prior) if prior.added() != op.added() => {
                    net.remove(&op.row());
                }
                // The same direction twice: the later tags win, because they are the revision
                // nearest the answer's `to`.
                Some(_) => {}
                None => {
                    net.insert(op.row(), op.clone());
                }
            }
        }
    }
    let mut out: Vec<TaggedOp> = net.into_values().collect();
    out.sort_by(|a, b| a.added().cmp(&b.added()).then(a.row().cmp(&b.row())));
    out
}

/// The ops that turn `from` into `to`, tagged from `view`.
///
/// This is the shape a caller holding two materialised graphs uses, and it is the same difference
/// [`crate::graph::EdgeDelta::between`] takes — this function just keeps the tags and the direction
/// in the form the rest of this module reads.
#[must_use]
pub fn ops_between(from: &Graph, to: &Graph, view: &Naming) -> Vec<TaggedOp> {
    let before: BTreeSet<Row> = from.rows().into_iter().collect();
    let after: BTreeSet<Row> = to.rows().into_iter().collect();
    let mut ops: Vec<TaggedOp> = before
        .difference(&after)
        .map(|row| TaggedOp::from_op(Op::remove(*row), view))
        .chain(
            after
                .difference(&before)
                .map(|row| TaggedOp::from_op(Op::add(*row), view)),
        )
        .collect();
    ops.sort_by(|a, b| a.added().cmp(&b.added()).then(a.row().cmp(&b.row())));
    ops
}

/// One endpoint of a delta, spelled back as a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeFact {
    /// The node's key: a module path, or `<path>::<symbol>`.
    pub key: String,
    /// `module` or `symbol`.
    pub kind: String,
    /// The module that owns it, which for a module node is itself.
    pub module: String,
    /// The layer tag this revision stamped, empty when untagged.
    pub layer: String,
    /// Whether its (or its module's) declaration list may have shrunk rather than shrunk.
    pub may_have_shrunk: bool,
    /// Whether the delta could not name this node — an id the dictionary never saw, which is a
    /// foreign row and is reported rather than renamed.
    pub unnamed: bool,
}

/// One edge of a delta, spelled back as a path pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeFact {
    /// The edge kind's stored spelling (`imports`, `uses`, `wildcard`, …).
    pub kind: String,
    /// The declaring module's key.
    pub src: String,
    /// The target's key — a module, a symbol, or a name nobody declared.
    pub dst: String,
    /// The module that owns the row, which is `src` with a symbol source resolved to its file.
    pub module: String,
    /// The source's layer tag in this revision.
    pub src_layer: String,
    /// The destination's layer tag in this revision.
    pub dst_layer: String,
    /// Why this row moved, when the delta can say. Three removals, three meanings, and a reader
    /// who is told only "removed" cannot tell them apart:
    ///
    /// * `with-target` — the thing it pointed at went away in the same revision;
    /// * `unwired` — the target is still there and the declaring module stopped declaring it, which
    ///   is a decision somebody made;
    /// * `dropped` — the row was cleaned up on a target that was already gone, so nothing deleted
    ///   either end in this revision; [`EdgeFact::dead`] is the flag that says so.
    ///
    /// Additions say `new`, or `new-target` when the target arrived in the same revision.
    pub why: String,
    /// Whether this edge was dead — pointing at a target the revision never declared. `None` when
    /// the answer could not know: a cheap store-backed answer has not read either revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead: Option<bool>,
    /// True for an added edge whose target is not declared in `to`. Same `Option` rule as
    /// [`EdgeFact::dead`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dangling: Option<bool>,
    /// Whether an endpoint of this row is an id the dictionary could not spell — a pruned or
    /// rebuilt `graph_node`, which is class R and may be dropped, or a foreign row. The row is still
    /// reported, with the name it has, because a silently smaller delta is a wrong answer.
    pub unnamed: bool,
}

impl EdgeFact {
    /// Whether this row moved because its own target moved or went away.
    #[must_use]
    pub fn went_with_target(&self) -> bool {
        self.why == "with-target"
    }

    /// Whether this row is here because somebody took a live dependency away, as opposed to the
    /// graph tidying up a row whose target had already gone.
    #[must_use]
    pub fn unwired(&self) -> bool {
        self.why == "unwired"
    }
}

/// The same dependency, a different kind of knowledge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repoint {
    /// The declaring module.
    pub src: String,
    /// The target.
    pub dst: String,
    /// The kind it was.
    pub from_kind: String,
    /// The kind it became.
    pub to_kind: String,
    /// The module that owns the row.
    pub module: String,
    /// What the change means, in the one sentence the two kinds imply: becoming a wildcard is a loss
    /// of knowledge, not a new dependency.
    pub meaning: String,
}

/// The shape of a symbol move, which is the difference between a rename and a split (`e2.4`'s
/// lesson, applied to the graph: a 1→N move is one grouping, never N renames).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveShape {
    /// One module out, one module in.
    Moved,
    /// One module out, several in.
    Split,
    /// Several modules out, one in.
    Fold,
    /// Several on each side: the pairing is not derivable from one revision, so this is a
    /// *candidate* and the raw rows stay visible.
    Unresolved,
}

impl MoveShape {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            MoveShape::Moved => "moved",
            MoveShape::Split => "split",
            MoveShape::Fold => "fold",
            MoveShape::Unresolved => "unresolved",
        }
    }
}

/// A symbol that changed module — reported as one event, which is the ticket's second acceptance
/// criterion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    /// The symbol's own name.
    pub name: String,
    /// The module keys it left, sorted.
    pub out: Vec<String>,
    /// The module keys it arrived in, sorted.
    pub into: Vec<String>,
    /// The shape: moved, split, folded, or unresolvable.
    pub shape: MoveShape,
    /// Whether the pairing is derived rather than guessed. False only for
    /// [`MoveShape::Unresolved`], where the raw add and remove stay in the delta as well.
    pub confident: bool,
    /// How many third-party dependencies moved with it — the `uses old::X` rows that became
    /// `uses new::X`, billed to the move rather than to the depending module.
    pub retargets: usize,
    /// The keys this move accounts for, so a reader can see which rows are no longer in
    /// [`ArchitectureDelta::nodes_added`] and [`ArchitectureDelta::nodes_removed`].
    pub keys: Vec<String>,
    /// Why a candidate is not a fact, when it is one.
    pub reason: String,
}

/// A file whose export list moved. This is the API-surface half — what `e3.7` will ask about and
/// what a reviewer reads as "did anyone's door change".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceChange {
    /// The module, sorted by key.
    pub module: String,
    /// Exports that appeared.
    pub added: Vec<String>,
    /// Exports that disappeared, and that the next field may disqualify as claims.
    pub removed: Vec<String>,
    /// Whether this module's declaration list may have shrunk rather than shrunk: a `macro`-heavy
    /// or monkeypatched file's disappeared export is a *wider* statement than a removal (`s1`
    /// finding 1).
    pub may_have_shrunk: bool,
    /// Whether the answer may call this a complete description of the surface: false whenever
    /// [`SurfaceChange::may_have_shrunk`] is true.
    pub complete: bool,
}

impl SurfaceChange {
    /// Whether the door moved at all.
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty()
    }
}

/// Two modules whose arrow turned around in one revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reversal {
    /// The module whose outbound edge went away, which is also the one that gained one.
    pub a: String,
    /// The other side.
    pub b: String,
    /// The kind that was removed, `a → b`.
    pub removed_kind: String,
    /// The kind that arrived, `b → a`.
    pub added_kind: String,
    /// Whether both arrows now exist — a two-module cycle. `None` when the answer could not know,
    /// which is every answer that did not read the `to` revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutual: Option<bool>,
}

/// A `[layers]` tag this revision stamped that the dictionary now spells differently — the honest
/// half of the re-tag question: a delta can report the tags it carries, never a re-tag of a module
/// whose rows nobody rewrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerDrift {
    /// The node the tag belongs to.
    pub subject: String,
    /// The tag this revision's rows carried.
    pub then: String,
    /// The tag the dictionary holds now.
    pub now: String,
}

/// How much one module's wiring moved over a window of revisions.
///
/// This is the "this module keeps getting reworked" metric, and it is a *rate over history*, not a
/// property of one revision: a single revision's churn is the same measure with a window of one.
/// Edges are counted on both sides — a module that five other modules keep re-pointing at is being
/// reworked as surely as one that keeps changing its own imports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleChurn {
    /// The module, sorted by key.
    pub module: String,
    /// Edge rows it declared or pointed at that appeared.
    pub added: usize,
    /// Edge rows that went away.
    pub removed: usize,
    /// `added + removed`, the headline number.
    pub churn: usize,
    /// How many revisions of the window touched it — the number that separates one big commit from
    /// a module that keeps coming back.
    pub revisions: usize,
    /// Revisions in the window, printed beside it so the rate has a denominator.
    pub window: usize,
    /// Edge events on the module's own side (it declared them).
    pub declared: usize,
    /// Edge events on the other side (something pointed at it).
    pub targeted: usize,
    /// Node events under it: an export appearing or a module going away.
    pub nodes: usize,
    /// Edges it holds in the newest revision of the window, when known: the denominator that tells
    /// a reader whether 12 events is a reshuffle or a rewrite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges_now: Option<usize>,
    /// Churn per revision of the window, to three decimals, so the JSON is stable text.
    pub per_revision: f64,
    /// The band the numbers put it in.
    pub band: Band,
}

/// The churn bands. Deliberately constants rather than configuration: the denominator is the window
/// the caller chose, and a pane that tunes both the window and the bands can make any module look
/// stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    /// Touched once, or barely.
    Stable,
    /// More than one revision, enough to be worth a look.
    Active,
    /// More than one revision *and* enough edges: this module keeps getting reworked.
    Reworked,
}

impl Band {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Band::Stable => "stable",
            Band::Active => "active",
            Band::Reworked => "reworked",
        }
    }
}

/// The band a set of numbers puts a module in.
fn band_of(revisions: usize, churn: usize) -> Band {
    if revisions >= REWORK_MIN_REVISIONS && churn >= REWORK_MIN_EDGES {
        Band::Reworked
    } else if revisions >= ACTIVE_MIN_REVISIONS || churn >= REWORK_MIN_EDGES / 2 {
        Band::Active
    } else {
        Band::Stable
    }
}

/// The architecture delta between two revisions.
///
/// Build it with [`ArchitectureDelta::of`] from ops the caller already has, with
/// [`ArchitectureDelta::between`] from two graphs in hand, or with
/// [`crate::graph::lane::Lane::architecture_delta`], which picks the cheapest rung the store can
/// answer at and is the only path that gets to report [`Rung::Empty`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArchitectureDelta {
    /// The shape tag a machine reads first.
    pub contract: String,
    /// The revision the answer starts at, as its caller named it.
    pub from: String,
    /// The revision it ends at.
    pub to: String,
    /// Whether `to`'s stored base is `from` — the common case, and the one that is cheap.
    pub adjacent: bool,
    /// What reading it cost.
    pub cost: Cost,
    /// Nodes the span added, minus the ones a confident move already accounts for.
    pub nodes_added: Vec<NodeFact>,
    /// Nodes the span removed, same rule.
    pub nodes_removed: Vec<NodeFact>,
    /// Edges the span added.
    pub edges_added: Vec<EdgeFact>,
    /// Edges the span removed.
    pub edges_removed: Vec<EdgeFact>,
    /// Same endpoints, different kind: the dependency is the same one and the knowledge about it
    /// changed.
    pub repointed: Vec<Repoint>,
    /// Symbols that changed module, including the splits and folds that are not renames.
    pub moves: Vec<Move>,
    /// Arrows that turned around.
    pub reversals: Vec<Reversal>,
    /// Files whose export list moved.
    pub surface: Vec<SurfaceChange>,
    /// Per-module churn of this span: the same measure a history window uses, with a window of one.
    pub churn: Vec<ModuleChurn>,
    /// Layer tags this revision's rows carry that the dictionary now spells differently.
    pub layer_drift: Vec<LayerDrift>,
    /// What this answer could not do, in the answer.
    pub notices: Vec<String>,
}

/// One revision's ops, as a churn window sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionOps {
    /// The revision, named as its caller names it.
    pub revision: String,
    /// Its ops, tagged.
    pub ops: Vec<TaggedOp>,
}

impl ArchitectureDelta {
    /// The delta a set of ops describes.
    ///
    /// `before` and `after` are the two revisions' graphs, when the caller has them: they are what
    /// turns "this edge went away" into "this edge went away *and its target was already gone*", and
    /// an answer that did not read them says so rather than guessing.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn of(
        from: &str,
        to: &str,
        ops: &[TaggedOp],
        view: &Naming,
        before: Option<&Graph>,
        after: Option<&Graph>,
        cost: Cost,
    ) -> Self {
        let key_of = |id: i64| -> (String, bool) {
            (
                view.name(id)
                    .map_or_else(|| format!("#unnamed:{id}"), str::to_string),
                view.vertex(id).is_none(),
            )
        };
        let layer_of = |id: i64, tagged: &str| -> String {
            if tagged.is_empty() {
                view.layer(id).to_string()
            } else {
                tagged.to_string()
            }
        };

        // -- 1. the rows, in two passes: nodes first, then edges --------------------------------
        // Two passes, deliberately. Whether an edge's target moved in the same revision is a
        // question about the *whole* revision, and answering it while walking the rows once would
        // make the answer depend on the order the rows arrived in — which is exactly the kind of
        // "right answer, wrong reason" the determinism criterion exists to catch.
        let mut added_nodes: BTreeMap<i64, NodeFact> = BTreeMap::new();
        let mut removed_nodes: BTreeMap<i64, NodeFact> = BTreeMap::new();
        for op in ops {
            let (kind, src, _dst) = op.row();
            if !kind.is_node() {
                continue;
            }
            let (key, unnamed) = key_of(src);
            let module = module_of(&key);
            let fact = NodeFact {
                layer: layer_of(src, &op.src_layer),
                key,
                kind: kind.as_str().to_string(),
                module: module.clone(),
                may_have_shrunk: view.id(&module).is_some_and(|id| view.incomplete(id))
                    || view.incomplete(src),
                unnamed,
            };
            if op.added() {
                added_nodes.insert(src, fact);
            } else {
                removed_nodes.insert(src, fact);
            }
        }
        // The modules whose own node went away: an edge naming any of them went with its target, and
        // did not get dropped by anyone.
        let gone_modules: BTreeSet<String> = removed_nodes
            .values()
            .filter(|node| node.kind == RowKind::Module.as_str())
            .map(|node| node.key.clone())
            .collect();
        let arrived_nodes: BTreeSet<i64> = added_nodes.keys().copied().collect();

        let mut added_edges: BTreeMap<Row, EdgeFact> = BTreeMap::new();
        let mut removed_edges: BTreeMap<Row, EdgeFact> = BTreeMap::new();
        for op in ops {
            let (kind, src, dst) = op.row();
            if kind.is_node() {
                continue;
            }
            let (key, src_unnamed) = key_of(src);
            let module = module_of(&key);
            let (dst_key, dst_unnamed) = key_of(dst);
            let dst_module = module_of(&dst_key);
            let target_gone =
                removed_nodes.contains_key(&dst) || gone_modules.contains(dst_module.as_str());
            let why = if op.added() {
                if arrived_nodes.contains(&dst) {
                    "new-target"
                } else {
                    "new"
                }
            } else if target_gone {
                "with-target"
            } else if before.is_some_and(|graph| graph.nodes().contains_key(&dst)) {
                // The target was a live node in the revision this delta starts from, so the only
                // thing that happened here is that the declaring module stopped declaring it.
                // Saying `dropped` for both this and the clean-up of a row whose target went two
                // revisions ago is how a changelog says "the dependency went away" about two
                // opposite events.
                "unwired"
            } else {
                "dropped"
            };
            let fact = EdgeFact {
                dead: if op.added() || before.is_none() {
                    None
                } else {
                    Some(!before.is_some_and(|graph| graph.nodes().contains_key(&dst)))
                },
                dangling: if op.added() {
                    after.map(|graph| !graph.nodes().contains_key(&dst))
                } else {
                    None
                },
                kind: kind.as_str().to_string(),
                src: key,
                dst: dst_key,
                module,
                src_layer: layer_of(src, &op.src_layer),
                dst_layer: layer_of(dst, &op.dst_layer),
                why: why.to_string(),
                unnamed: src_unnamed || dst_unnamed,
            };
            if op.added() {
                added_edges.insert((kind, src, dst), fact);
            } else {
                removed_edges.insert((kind, src, dst), fact);
            }
        }

        // -- 2. moves: the same symbol, another module -----------------------------------------
        let mut moves: Vec<Move> = Vec::new();
        let mut moved_symbol_ids: BTreeSet<i64> = BTreeSet::new();
        for name in symbol_names(&removed_nodes, &added_nodes) {
            let lost: BTreeSet<String> = removed_nodes
                .values()
                .filter(|node| node.kind == RowKind::Symbol.as_str() && node_symbol(node) == name)
                .map(|node| node.module.clone())
                .collect();
            let gained: BTreeSet<String> = added_nodes
                .values()
                .filter(|node| node.kind == RowKind::Symbol.as_str() && node_symbol(node) == name)
                .map(|node| node.module.clone())
                .collect();
            // A module that lost and gained the same name kept it, and a module that kept it cannot
            // be either end of a move.
            let kept: BTreeSet<String> = lost.intersection(&gained).cloned().collect();
            let out: Vec<String> = lost.difference(&kept).cloned().collect();
            let into: Vec<String> = gained.difference(&kept).cloned().collect();
            if out.is_empty() || into.is_empty() {
                continue;
            }
            let shape = match (out.len(), into.len()) {
                (1, 1) => MoveShape::Moved,
                (1, _) => MoveShape::Split,
                (_, 1) => MoveShape::Fold,
                _ => MoveShape::Unresolved,
            };
            let confident = shape != MoveShape::Unresolved;
            let parties: BTreeSet<String> = out.iter().chain(into.iter()).cloned().collect();
            let keys: Vec<String> = removed_nodes
                .values()
                .chain(added_nodes.values())
                .filter(|node| {
                    node.kind == RowKind::Symbol.as_str()
                        && node_symbol(node) == name
                        && parties.contains(&node.module)
                })
                .map(|node| node.key.clone())
                .collect::<BTreeSet<String>>()
                .into_iter()
                .collect();
            let mut retargets = 0_usize;
            if shape == MoveShape::Moved {
                let (src_id, dst_id) = (
                    out.first()
                        .and_then(|module| view.id(&format!("{module}::{name}"))),
                    into.first()
                        .and_then(|module| view.id(&format!("{module}::{name}"))),
                );
                if let (Some(src_id), Some(dst_id)) = (src_id, dst_id) {
                    // Somebody else's `uses old::X` became `uses new::X`. That is the move's
                    // invoice, not that module changing its mind.
                    let dependents: Vec<i64> = removed_edges
                        .iter()
                        .filter(|((kind, src, dst), _)| {
                            *kind == RowKind::Uses && *dst == src_id && *src != dst_id
                        })
                        .map(|((_, src, _), _)| *src)
                        .collect();
                    for dependent in dependents {
                        let row = (RowKind::Uses, dependent, dst_id);
                        if removed_edges
                            .remove(&(RowKind::Uses, dependent, src_id))
                            .is_some()
                            && added_edges.remove(&row).is_some()
                        {
                            retargets += 1;
                        }
                    }
                }
            }
            // The rows this move accounts for stop being a deletion and an addition. They are
            // still in the document — inside the move — where a reader can see them.
            let moved: BTreeSet<String> = keys.iter().cloned().collect();
            moves.push(Move {
                name: name.to_string(),
                reason: if confident {
                    String::new()
                } else {
                    format!(
                        "{out} lost `{name}` and {into} gained it in the same revision; which went \
                         where is not derivable from one revision, so the removal and the addition \
                         are both still listed",
                        out = out.len(),
                        into = into.len()
                    )
                },
                out,
                into,
                shape,
                confident,
                retargets,
                keys: moved.iter().cloned().collect(),
            });
            if confident {
                for (id, fact) in removed_nodes.iter().chain(added_nodes.iter()) {
                    if moved.contains(&fact.key) {
                        moved_symbol_ids.insert(*id);
                    }
                }
            }
        }
        for id in &moved_symbol_ids {
            removed_nodes.remove(id);
            added_nodes.remove(id);
        }
        // A moved symbol's `contains` rows moved with it too: they are the move's bookkeeping, not
        // this revision's wiring.
        let listed = added_edges.len() + removed_edges.len();
        added_edges
            .retain(|key, _| !(key.0 == RowKind::Contains && moved_symbol_ids.contains(&key.2)));
        removed_edges
            .retain(|key, _| !(key.0 == RowKind::Contains && moved_symbol_ids.contains(&key.2)));
        let dropped_contains = listed - (added_edges.len() + removed_edges.len());

        // -- 3. re-points: the same two endpoints, a different kind -----------------------------
        let repointed = take_repoints(&mut added_edges, &mut removed_edges);
        // -- 4. inversions: an arrow that turned around -----------------------------------------
        let reversals = take_reversals(&added_edges, &removed_edges, after, view);

        // -- 5. the public surface, including what a move did to two doors ----------------------
        let surface = surface_of(&added_nodes, &removed_nodes, &moves, view);

        // -- 6. churn, and the two ways this answer can be weaker than it looks -----------------
        let mut tally = ChurnTally::default();
        for op in ops {
            tally.absorb(op, 0, view);
        }
        let churn = tally.finish(1, view, after);

        let mut notices = Vec::new();
        if !moves.iter().any(|mv| mv.confident) && !moves.is_empty() {
            notices.push(format!(
                "{} symbol{} moved in a shape this revision cannot pair; the removals and the \
                 additions are both listed",
                moves.len(),
                if moves.len() == 1 { "" } else { "s" }
            ));
        }
        if dropped_contains > 0 {
            notices.push(format!(
                "{dropped_contains} `contains` row(s) moved with a symbol rather than being \
                 counted as this revision's wiring"
            ));
        }
        if before.is_none() && !removed_edges.is_empty() {
            notices.push(String::from(
                "whether a removed edge was still live is not claimed: that needs the revision it \
                 was read against, which is a materialisation this answer did not pay",
            ));
        }
        let unnamed_rows: usize = added_edges
            .values()
            .chain(removed_edges.values())
            .filter(|fact| fact.unnamed)
            .count();
        if unnamed_rows > 0 {
            notices.push(format!(
                "{unnamed_rows} row(s) name a node this dictionary cannot spell, so they are \
                 reported by their id: `graph_node` is derived and re-derivable, and a rebuilt \
                 dictionary is not a smaller change"
            ));
        }
        let layer_drift = drift_of(ops, view, key_of);
        if !layer_drift.is_empty() {
            notices.push(format!(
                "{} node(s) here carry a layer tag this revision stamped that the dictionary now \
                 spells differently: the `[layers]` config moved after this revision was written",
                layer_drift.len()
            ));
        }
        if ops
            .iter()
            .any(|op| !op.src_layer.is_empty() || !op.dst_layer.is_empty())
        {
            notices.push(String::from(
                "a `[layers]` re-tag of a module this revision did not rewrite writes no rows, so \
                 no delta can see it: the layering a delta states is the layering of the modules \
                 that moved",
            ));
        }

        let mut delta = Self {
            contract: CONTRACT.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            adjacent: cost.rung == Rung::Adjacent,
            cost,
            nodes_added: fact_vec(added_nodes),
            nodes_removed: fact_vec(removed_nodes),
            edges_added: edge_vec(added_edges),
            edges_removed: edge_vec(removed_edges),
            repointed,
            moves,
            reversals,
            surface,
            churn,
            layer_drift,
            notices,
        };
        delta.sort();
        delta
    }

    /// The delta between two graphs the caller already holds, with everything that implies.
    ///
    /// This is the honest, complete answer: both revisions are in hand, so dead and dangling edges
    /// and two-way cycles are all decidable. It is also the expensive shape of the question — the one
    /// every representation without stored deltas pays for every question — so it reports
    /// [`Rung::InHand`] and a lane will not reach for it unless the store cannot answer cheaper.
    #[must_use]
    pub fn between(
        from: &Graph,
        to: &Graph,
        view: &Naming,
        from_label: &str,
        to_label: &str,
    ) -> Self {
        let ops = ops_between(from, to, view);
        Self::of(
            from_label,
            to_label,
            &ops,
            view,
            Some(from),
            Some(to),
            Cost::in_hand(),
        )
    }

    /// The delta that says nothing happened, with the cost that claim deserves: one snapshot row.
    #[must_use]
    pub fn empty(from: &str, to: &str) -> Self {
        Self {
            contract: CONTRACT.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            adjacent: true,
            cost: Cost::empty(),
            notices: vec![String::from(
                "no architectural change: the revision wrote no graph rows at all",
            )],
            ..Self::default()
        }
    }

    /// Put every list in its canonical order, so the document is a function of the facts.
    pub(crate) fn sort(&mut self) {
        self.nodes_added.sort_by(|a, b| a.key.cmp(&b.key));
        self.nodes_removed.sort_by(|a, b| a.key.cmp(&b.key));
        self.edges_added
            .sort_by(|a, b| (&a.kind, &a.src, &a.dst).cmp(&(&b.kind, &b.src, &b.dst)));
        self.edges_removed
            .sort_by(|a, b| (&a.kind, &a.src, &a.dst).cmp(&(&b.kind, &b.src, &b.dst)));
        self.repointed
            .sort_by(|a, b| (&a.src, &a.dst).cmp(&(&b.src, &b.dst)));
        self.moves.sort_by(|a, b| a.name.cmp(&b.name));
        self.reversals
            .sort_by(|a, b| (&a.a, &a.b).cmp(&(&b.a, &b.b)));
        self.surface.sort_by(|a, b| a.module.cmp(&b.module));
        self.churn.sort_by(|a, b| a.module.cmp(&b.module));
        self.layer_drift.sort_by(|a, b| a.subject.cmp(&b.subject));
        self.notices.sort();
        self.notices.dedup();
    }

    /// Whether the architecture did not move.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many facts the answer states.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes_added.len()
            + self.nodes_removed.len()
            + self.edges_added.len()
            + self.edges_removed.len()
            + self.repointed.len()
            + self.moves.len()
            + self.reversals.len()
            + self.surface.len()
    }

    /// The counts, labelled and sorted, which is what a summary line is built from.
    #[must_use]
    pub fn counts(&self) -> Vec<(&'static str, usize)> {
        let mut out = vec![
            ("nodes added", self.nodes_added.len()),
            ("nodes removed", self.nodes_removed.len()),
            ("edges added", self.edges_added.len()),
            ("edges removed", self.edges_removed.len()),
            ("symbols moved", self.moves.len()),
            ("dependencies re-pointed", self.repointed.len()),
            ("dependencies inverted", self.reversals.len()),
            ("files with a new surface", self.surface.len()),
        ];
        out.sort_by(|a, b| a.0.cmp(b.0));
        out
    }

    /// Every module the answer names, sorted.
    #[must_use]
    pub fn modules(&self) -> Vec<String> {
        let mut out: Vec<String> = self.churn.iter().map(|row| row.module.clone()).collect();
        out.sort();
        out.dedup();
        out
    }

    /// The one-line answer, cost included, which is the sentence a pane prints.
    #[must_use]
    pub fn sentence(&self) -> String {
        if self.is_empty() {
            return format!("{} — nothing architectural changed", self.cost.sentence());
        }
        let parts: Vec<String> = self
            .counts()
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(label, n)| format!("{n} {label}"))
            .collect();
        format!("{}: {}", parts.join(", "), self.cost.sentence())
    }

    /// The lines a card prints, one per fact, in a fixed order.
    ///
    /// Capping is the card's business; this produces every line, deterministically, so two callers
    /// capping at two different depths get the same first two lines.
    #[must_use]
    pub fn summary_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for mv in &self.moves {
            out.push(format!(
                "{} `{}`: {} → {}{}",
                match mv.shape {
                    MoveShape::Moved => "moved",
                    MoveShape::Split => "split",
                    MoveShape::Fold => "folded",
                    MoveShape::Unresolved => "move candidate",
                },
                mv.name,
                mv.out.join(", "),
                mv.into.join(", "),
                if mv.retargets > 0 {
                    format!(
                        " ({} dependenc{} re-pointed)",
                        mv.retargets,
                        if mv.retargets == 1 { "y" } else { "ies" }
                    )
                } else {
                    String::new()
                }
            ));
        }
        for rev in &self.reversals {
            out.push(format!(
                "inverted: {} → {} became {} → {}{}",
                rev.a,
                rev.b,
                rev.b,
                rev.a,
                match rev.mutual {
                    Some(true) => " — and both arrows still exist",
                    Some(false) => "",
                    None => "",
                }
            ));
        }
        for repoint in &self.repointed {
            out.push(format!(
                "re-pointed: {} → {} is now {} ({})",
                repoint.src, repoint.dst, repoint.to_kind, repoint.meaning
            ));
        }
        for change in &self.surface {
            out.push(format!(
                "surface: {} +{} −{}{}",
                change.module,
                change.added.len(),
                change.removed.len(),
                if change.may_have_shrunk {
                    " — the list may have shrunk rather than shrunk"
                } else {
                    ""
                }
            ));
        }
        for node in &self.nodes_added {
            out.push(format!("node added: {}", node.key));
        }
        for node in &self.nodes_removed {
            out.push(format!("node removed: {}", node.key));
        }
        for edge in &self.edges_added {
            out.push(format!(
                "{}: {} → {}{}",
                edge.kind,
                edge.src,
                edge.dst,
                if edge.why == "new-target" {
                    " (new target)"
                } else {
                    ""
                }
            ));
        }
        for edge in &self.edges_removed {
            out.push(format!(
                "{} −: {} → {} ({})",
                edge.kind,
                edge.src,
                edge.dst,
                match edge.dead {
                    Some(true) => "already dead",
                    Some(false) if edge.why == "with-target" => "went with its target",
                    Some(false) => "dropped",
                    None if edge.why == "with-target" => "went with its target",
                    None => "dropped",
                }
            ));
        }
        for drift in &self.layer_drift {
            out.push(format!(
                "layer as this revision stamped it: {} was `{}`, the dictionary now says `{}`",
                drift.subject, drift.then, drift.now
            ));
        }
        for notice in &self.notices {
            out.push(format!("note: {notice}"));
        }
        out
    }

    /// The document form the exporter and the MCP server hand out.
    ///
    /// Deterministic by construction: every list was sorted when the answer was built, and serde
    /// writes struct fields in declaration order.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Read a document back. The error is the sentence a CLI prints.
    ///
    /// The JSON a reviewer pastes is an input like any other: a truncated file or a different
    /// shape is refused with a reason, never half-read.
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|err| format!("not a graph delta document: {err}"))
    }
}

/// The symbol half of a node key: `src/parse.rs::parse` → `parse`.
fn node_symbol(node: &NodeFact) -> &str {
    node.key
        .find("::")
        .map_or(node.key.as_str(), |at| &node.key[at + 2..])
}

/// The symbol names that appear on both sides of a difference: the only candidates for a move.
fn symbol_names<'a>(
    removed: &'a BTreeMap<i64, NodeFact>,
    added: &'a BTreeMap<i64, NodeFact>,
) -> Vec<&'a str> {
    let here = |nodes: &'a BTreeMap<i64, NodeFact>| -> BTreeSet<&'a str> {
        nodes
            .values()
            .filter(|node| node.kind == RowKind::Symbol.as_str())
            .map(node_symbol)
            .collect()
    };
    here(removed).intersection(&here(added)).copied().collect()
}

fn fact_vec(nodes: BTreeMap<i64, NodeFact>) -> Vec<NodeFact> {
    nodes.into_values().collect()
}

fn edge_vec(edges: BTreeMap<Row, EdgeFact>) -> Vec<EdgeFact> {
    edges.into_values().collect()
}

/// Take the rows whose two endpoints are the same but whose kind differs out of the added and
/// removed lists, and say what the change means.
fn take_repoints(
    added: &mut BTreeMap<Row, EdgeFact>,
    removed: &mut BTreeMap<Row, EdgeFact>,
) -> Vec<Repoint> {
    let mut out = Vec::new();
    let mut pairs: BTreeMap<(i64, i64), (Option<Row>, Option<Row>)> = BTreeMap::new();
    for row in removed.keys().chain(added.keys()) {
        let entry = pairs.entry((row.1, row.2)).or_default();
        if removed.contains_key(row) {
            entry.0 = Some(*row);
        } else {
            entry.1 = Some(*row);
        }
    }
    for ((src, dst), (old, new)) in pairs {
        // Both halves present and the kinds differ: the same dependency, differently known.
        let (Some(old), Some(new)) = (old, new) else {
            continue;
        };
        if old == new {
            continue;
        }
        let (Some(from_fact), Some(to_fact)) = (removed.remove(&old), added.remove(&new)) else {
            continue;
        };
        out.push(Repoint {
            src: from_fact.src.clone(),
            dst: to_fact.dst.clone(),
            from_kind: from_fact.kind.clone(),
            to_kind: to_fact.kind.clone(),
            module: from_fact.module.clone(),
            meaning: repoint_meaning(&from_fact.kind, &to_fact.kind).to_string(),
            // `dst` is in the key map above, so nothing is lost by not naming it again here.
        });
        let _ = (src, dst);
    }
    out
}

/// The one sentence two kinds imply about the same dependency.
fn repoint_meaning(from: &str, to: &str) -> &'static str {
    match (from, to) {
        (f, t) if t == RowKind::Wildcard.as_str() && f != t => {
            "the dependency is the same and it is now known less precisely: a star import names a \
             module, not a unit"
        }
        (f, t) if f == RowKind::Wildcard.as_str() && t != f => {
            "the dependency is now named: what was a star import resolves to a unit"
        }
        (f, t) if t == RowKind::ImportsPackage.as_str() && f != t => {
            "the dependency is now package-granular, so it must not be read as a file-to-file claim"
        }
        (f, t) if f == RowKind::ImportsPackage.as_str() && t != f => {
            "the dependency is file-granular again"
        }
        _ => "the same two modules, a different kind of relationship",
    }
}

/// Does the graph still carry a dependency from module `from` to module `to`, at whichever
/// granularity the rows happen to be?
///
/// The reviewer's question — "is there still an arrow the other way?" — is a module question, and
/// answering it at row granularity is how a graph says an arrow is gone while the symbol-granular
/// row that still carries it (`impl Shape for Rect` is a module-to-module fact too) sits in the
/// same store. So every node of the module is asked, and every node of the target counts.
fn module_arrow(graph: &Graph, view: &Naming, from: &str, to: &str) -> bool {
    let targets: BTreeSet<i64> = view.module_ids(to).into_iter().collect();
    if targets.is_empty() {
        return false;
    }
    view.module_ids(from).into_iter().any(|id| {
        graph
            .reached(id, RowKind::is_dependency)
            .into_iter()
            .any(|(_, dst)| targets.contains(&dst))
    })
}

/// The arrows that turned around: `a → b` removed and `b → a` added in the same span.
fn take_reversals(
    added: &BTreeMap<Row, EdgeFact>,
    removed: &BTreeMap<Row, EdgeFact>,
    after: Option<&Graph>,
    view: &Naming,
) -> Vec<Reversal> {
    let mut out = Vec::new();
    for (row, fact) in removed {
        let (kind, src, dst) = *row;
        if !kind.is_dependency() {
            continue;
        }
        for (new_row, new_fact) in added.iter() {
            let (new_kind, new_src, new_dst) = *new_row;
            if !new_kind.is_dependency() || new_src != dst || new_dst != src {
                continue;
            }
            out.push(Reversal {
                a: fact.src.clone(),
                b: new_fact.src.clone(),
                removed_kind: fact.kind.clone(),
                added_kind: new_fact.kind.clone(),
                mutual: after.and_then(|graph| {
                    // Both ends folded onto their module: the arrow that turned around is named by
                    // two modules, and the arrow that survived may be spelled at either end's
                    // symbols. A dictionary that cannot spell one of them cannot answer, and an
                    // answer that cannot be given is `None`, not a confident `false`.
                    let (a, b) = (
                        view.name(src).map(module_of)?,
                        view.name(dst).map(module_of)?,
                    );
                    Some(module_arrow(graph, view, &a, &b) && module_arrow(graph, view, &b, &a))
                }),
            });
        }
    }
    out
}

/// The export lists that moved, including the two doors a move changes.
fn surface_of(
    added: &BTreeMap<i64, NodeFact>,
    removed: &BTreeMap<i64, NodeFact>,
    moves: &[Move],
    view: &Naming,
) -> Vec<SurfaceChange> {
    let mut by_module: BTreeMap<String, SurfaceChange> = BTreeMap::new();
    for node in added.values() {
        if node.kind == RowKind::Symbol.as_str() {
            surface_entry(&mut by_module, &node.module, view, added, removed)
                .added
                .push(node_symbol(node).to_string());
        }
    }
    for node in removed.values() {
        if node.kind == RowKind::Symbol.as_str() {
            surface_entry(&mut by_module, &node.module, view, added, removed)
                .removed
                .push(node_symbol(node).to_string());
        }
    }
    // A move is a loss at one door and a gain at the other, and both doors really changed. The
    // symbol nodes themselves were consumed by the move, so nothing here double-counts them.
    for mv in moves {
        if !mv.confident {
            continue;
        }
        for module in &mv.out {
            surface_entry(&mut by_module, module, view, added, removed)
                .removed
                .push(mv.name.clone());
        }
        for module in &mv.into {
            surface_entry(&mut by_module, module, view, added, removed)
                .added
                .push(mv.name.clone());
        }
    }
    let mut out: Vec<SurfaceChange> = by_module
        .into_values()
        .filter(SurfaceChange::changed)
        .map(|mut change| {
            change.added.sort();
            change.added.dedup();
            change.removed.sort();
            change.removed.dedup();
            change
        })
        .collect();
    out.sort_by(|a, b| a.module.cmp(&b.module));
    out
}

/// One module's surface row, created with its two honesty flags attached.
fn surface_entry<'a>(
    by: &'a mut BTreeMap<String, SurfaceChange>,
    module: &str,
    view: &Naming,
    added: &BTreeMap<i64, NodeFact>,
    removed: &BTreeMap<i64, NodeFact>,
) -> &'a mut SurfaceChange {
    by.entry(module.to_string()).or_insert_with(|| {
        // The dictionary's flag is the authoritative one; a node fact carries the same flag its
        // module was built with, for the case where the dictionary has not been told yet.
        let may_have_shrunk = view.id(module).is_some_and(|id| view.incomplete(id))
            || removed
                .values()
                .chain(added.values())
                .any(|node| node.module == module && node.may_have_shrunk);
        SurfaceChange {
            module: module.to_string(),
            added: Vec::new(),
            removed: Vec::new(),
            may_have_shrunk,
            complete: !may_have_shrunk,
        }
    })
}

/// Where a revision's own layer tags disagree with the dictionary's current ones.
fn drift_of(
    ops: &[TaggedOp],
    view: &Naming,
    key_of: impl Fn(i64) -> (String, bool),
) -> Vec<LayerDrift> {
    let mut out: BTreeSet<(String, String, String)> = BTreeSet::new();
    for op in ops {
        let (kind, src, dst) = op.row();
        for (id, then) in [(src, op.src_layer.as_str())]
            .into_iter()
            .chain((!kind.is_node()).then_some((dst, op.dst_layer.as_str())))
        {
            if view.vertex(id).is_none() {
                continue;
            }
            let now = view.layer(id);
            if now == then {
                continue;
            }
            out.insert((key_of(id).0, then.to_string(), now.to_string()));
        }
    }
    out.into_iter()
        .map(|(subject, then, now)| LayerDrift { subject, then, now })
        .collect()
}

/// The accumulator behind both churn surfaces: one revision's wiring, or a hundred.
#[derive(Default)]
struct ChurnTally {
    declared: BTreeMap<String, (usize, usize)>,
    targeted: BTreeMap<String, (usize, usize)>,
    nodes: BTreeMap<String, usize>,
    touched: BTreeMap<String, BTreeSet<usize>>,
}

impl ChurnTally {
    /// Charge one op to the modules it touches.
    fn absorb(&mut self, op: &TaggedOp, revision: usize, view: &Naming) {
        let (kind, src, dst) = op.row();
        let side = |id: i64| -> String {
            module_of(
                view.name(id)
                    .map_or_else(|| format!("#unnamed:{id}"), str::to_string)
                    .as_str(),
            )
        };
        let src_module = side(src);
        if kind.is_node() || kind == RowKind::Contains {
            *self.nodes.entry(src_module.clone()).or_default() += 1;
            self.touched.entry(src_module).or_default().insert(revision);
            return;
        }
        let dst_module = side(dst);
        let slot = self.declared.entry(src_module.clone()).or_default();
        if op.added() {
            slot.0 += 1;
        } else {
            slot.1 += 1;
        }
        let slot = self.targeted.entry(dst_module.clone()).or_default();
        if op.added() {
            slot.0 += 1;
        } else {
            slot.1 += 1;
        }
        for module in [src_module, dst_module] {
            self.touched.entry(module).or_default().insert(revision);
        }
    }

    /// Finish the tally: one row per module, sorted, with the band and the rate attached.
    fn finish(self, window: usize, view: &Naming, newest: Option<&Graph>) -> Vec<ModuleChurn> {
        // "how many edges is this module in now", so a 12-event module with 3 edges and a 12-event
        // module with 300 are not the same finding.
        let mut in_graph: BTreeMap<String, usize> = BTreeMap::new();
        if let Some(graph) = newest {
            for (_kind, src, dst) in graph.edges() {
                let side = |id: i64| -> String {
                    module_of(
                        view.name(id)
                            .map_or_else(|| format!("#unnamed:{id}"), str::to_string)
                            .as_str(),
                    )
                };
                *in_graph.entry(side(*src)).or_default() += 1;
                *in_graph.entry(side(*dst)).or_default() += 1;
            }
        }
        let mut out = Vec::new();
        for module in self.touched.keys() {
            let (da, dr) = self.declared.get(module).copied().unwrap_or((0, 0));
            let (ta, tr) = self.targeted.get(module).copied().unwrap_or((0, 0));
            let nodes = self.nodes.get(module).copied().unwrap_or(0);
            let churn = da + dr + ta + tr;
            let revisions = self
                .touched
                .get(module)
                .map_or(0, <BTreeSet<usize>>::len)
                .max(usize::from(nodes > 0));
            let per_revision = (f64::from(u32::try_from(churn).unwrap_or(u32::MAX)) * 1_000.0
                / f64::from(u32::try_from(window.max(1)).unwrap_or(u32::MAX)))
            .round()
                / 1_000.0;
            out.push(ModuleChurn {
                module: module.clone(),
                added: da + ta,
                removed: dr + tr,
                churn,
                revisions,
                window,
                declared: da + dr,
                targeted: ta + tr,
                nodes,
                edges_now: in_graph.get(module).copied(),
                per_revision,
                band: band_of(revisions, churn),
            });
        }
        out.sort_by(|a, b| a.module.cmp(&b.module));
        out
    }
}

/// One revision's ops, as a churn window reads them.
impl RevisionOps {
    /// A window entry from stored or computed ops.
    #[must_use]
    pub fn new(revision: impl Into<String>, ops: Vec<TaggedOp>) -> Self {
        Self {
            revision: revision.into(),
            ops,
        }
    }
}

/// Per-module churn over a window of revisions: the "this module keeps getting reworked" answer.
///
/// The window is the caller's, and it is printed on every row: a module reworked in 4 of 4 revisions
/// and one reworked in 4 of 200 are different findings, and a report that hides its denominator is
/// how a churn metric becomes decoration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChurnReport {
    /// The shape tag.
    pub contract: String,
    /// Revisions in the window.
    pub window: usize,
    /// The revisions, oldest first, so the window is reproducible from the report.
    pub revisions: Vec<String>,
    /// One row per touched module, sorted by module.
    pub per_module: Vec<ModuleChurn>,
    /// The hottest modules, by churn then by name — the short list a pane leads with.
    pub hot: Vec<String>,
    /// What the window could not see.
    pub notices: Vec<String>,
}

impl ChurnReport {
    /// Measure a window.
    ///
    /// `newest` is the newest revision's graph, when the caller has one: it supplies each module's
    /// current edge count, which is the denominator that separates a reshuffle from a rewrite.
    #[must_use]
    pub fn measure(steps: &[RevisionOps], view: &Naming, newest: Option<&Graph>) -> Self {
        let mut tally = ChurnTally::default();
        for (index, step) in steps.iter().enumerate() {
            for op in &step.ops {
                tally.absorb(op, index, view);
            }
        }
        let per_module = tally.finish(steps.len(), view, newest);
        let mut hot: Vec<(String, usize)> = per_module
            .iter()
            .map(|row| (row.module.clone(), row.churn))
            .collect();
        hot.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let notices: Vec<String> = if steps.is_empty() {
            vec![String::from(
                "an empty window: nothing was measured, and no module is stable either",
            )]
        } else if per_module.iter().any(|row| row.band == Band::Reworked) {
            vec![String::from(
                "a reworked module is a module whose edges moved in several revisions of this \
                 window, which is a statement about the window, not about the module's history",
            )]
        } else {
            Vec::new()
        };
        Self {
            contract: CONTRACT.to_string(),
            window: steps.len(),
            revisions: steps.iter().map(|step| step.revision.clone()).collect(),
            per_module,
            hot: hot
                .into_iter()
                .take(HOT_MODULES)
                .map(|(module, _)| module)
                .collect(),
            notices,
        }
    }

    /// One module's row.
    #[must_use]
    pub fn find(&self, module: &str) -> Option<&ModuleChurn> {
        self.per_module.iter().find(|row| row.module == module)
    }

    /// The modules the report would flag as reworked.
    #[must_use]
    pub fn reworked(&self) -> Vec<&ModuleChurn> {
        self.per_module
            .iter()
            .filter(|row| row.band == Band::Reworked)
            .collect()
    }

    /// Whether the window named anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.per_module.is_empty()
    }

    /// How many modules the window touched.
    #[must_use]
    pub fn len(&self) -> usize {
        self.per_module.len()
    }

    /// The sentence a pane leads with.
    #[must_use]
    pub fn sentence(&self) -> String {
        if self.per_module.is_empty() {
            return format!("no module's edges moved across {} revision(s)", self.window);
        }
        let reworked = self.reworked().len();
        format!(
            "{} module(s) moved across {} revision(s); {} reworked, hottest {}",
            self.per_module.len(),
            self.window,
            reworked,
            self.hot
                .first()
                .map_or_else(|| "none".to_string(), String::clone)
        )
    }

    /// The document form, for the exporter and the MCP server.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Read a document back.
    ///
    /// # Errors
    /// A one-line reason the document is not one.
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|err| format!("not a graph churn document: {err}"))
    }
}

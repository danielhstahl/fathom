//! `fathom_tui::graph::hand` — the one thing the architecture map is allowed to know.
//!
//! The shell holds no store handle and may not go and get one (ADR-002 §5.2, lint `E01`), and it may not
//! compute an architectural fact from its own reading of a repository (lint `M02`, and ADR-001 §7.10,
//! which is the same rule with a severity attached: a number the human and the MCP server disagree about
//! is a bug). So everything the map draws arrives as one [`Hand`], built by the composition root out of
//! [`fathom_analyze::graph`] — the materialised revision ([`Graph`]), its [`Naming`], the churn report and
//! the architecture delta of `e3.3`. This module is the *shape* of that hand-over and nothing else: it
//! parses no store, runs no grammar and answers no architectural question.
//!
//! Three rules the shapes below enforce:
//!
//! * **A missing figure is a `Series` with a reason, never a zero** (ADR-001 §7.7). A hand with no churn
//!   window prints `heat ? — no churn window in this index`, and the map still draws: the graph itself is
//!   a measured fact even when the heat on top of it is not.
//! * **Every edge carries its own citation.** [`Line`] is `e3.6`'s [`fathom_analyze::layering::locate`]
//!   answer restated in the shell's own nouns — a line, several candidate lines, or *no line recorded* —
//!   because "the file-and-line that produced this edge" has three honest answers and only one of them
//!   prints a number.
//! * **A hand says what it left out.** [`Hand::truncated`] and [`Hand::gaps`] are printed on the canvas, so
//!   "214 of 1 902 shown" is a fact about the drawing and "the hand carried 214 of 1 902 modules" is a
//!   fact about the hand-over, and the two are never conflated.
//!
//! [`Graph`]: fathom_analyze::graph::model::Graph
//! [`Naming`]: fathom_analyze::graph::Naming

use std::collections::BTreeMap;

/// The largest node list a hand may carry.
///
/// The map aggregates long before this number is reached, so the cap is not a layout limit but a memory
/// limit: the façade owns the memory budget (ADR-002 §5.3), and a composition root that hands the shell
/// four hundred thousand modules is a composition root that OOMs the terminal instead of answering the
/// question. Past the cap the hand says so in [`Hand::truncated`].
pub const CELL_CAP: usize = 20_000;

/// The largest edge list a hand may carry, for the same reason as [`CELL_CAP`].
pub const LINK_CAP: usize = 60_000;

/// What a canvas cell is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// A module — the granularity the graph is drawn at by default.
    Module,
    /// An exported symbol, which the extractor keeps as its own node (`e3.2`).
    Symbol,
    /// Several modules collapsed into one cell by aggregation ([`crate::graph::Level`]).
    Aggregate,
}

impl CellKind {
    /// The stored/registry spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CellKind::Module => "module",
            CellKind::Symbol => "symbol",
            CellKind::Aggregate => "aggregate",
        }
    }

    /// The one-character class badge the canvas prints when a cell is too narrow for its name.
    #[must_use]
    pub const fn badge(self) -> char {
        match self {
            CellKind::Module => '·',
            CellKind::Symbol => 'ƒ',
            CellKind::Aggregate => '▦',
        }
    }
}

/// Where the statement that produced an edge starts.
///
/// The three cases are `e3.6`'s [`fathom_analyze::layering::locate::Line`] restated for a pane: a number,
/// several candidates that must not be guessed between, or a record that can only name the file. Collapsing
/// the last two into "no line" would print a citation that is a guess, which is the one thing an evidence
/// pointer may not be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Line {
    /// The statement starts at this one-based line.
    At(u32),
    /// Several statements could be this dependency; none may be printed as *the* citation.
    Ambiguous(usize),
    /// Nothing in the record could say.
    #[default]
    Unrecorded,
}

impl Line {
    /// The number to print, if there is one.
    #[must_use]
    pub const fn number(self) -> Option<u32> {
        match self {
            Line::At(line) => Some(line),
            Line::Ambiguous(_) | Line::Unrecorded => None,
        }
    }

    /// Why there is no number. `None` when there is one.
    #[must_use]
    pub fn note(self) -> Option<String> {
        match self {
            Line::At(_) => None,
            // The actionable half comes first: these notes are printed inside a 54-column pane and the
            // tail of a clipped sentence is a fact nobody reads.
            Line::Ambiguous(count) => Some(format!(
                "{count} lines could be this dependency — `fathom structure` prints them all; guessing \
                 one would be worse"
            )),
            Line::Unrecorded => Some(
                "no recorded line — only the file can be cited; `fathom structure <file>` prints the \
                 statement"
                    .to_owned(),
            ),
        }
    }

    /// Whether a line was actually recorded.
    #[must_use]
    pub const fn is_recorded(self) -> bool {
        matches!(self, Line::At(_))
    }
}

/// A citation a reviewer can go and read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    /// The file, corpus-relative, as the graph names it.
    pub file: String,
    /// Where the statement is, or why it cannot be said.
    pub line: Line,
    /// The row's own reason, when it has one: `unwired`, `with-target`, `new-target`, `dropped`.
    pub why: Option<String>,
    /// A command that shows the same evidence outside the shell, when one exists.
    pub run: Option<String>,
}

impl Evidence {
    /// A citation with a line.
    #[must_use]
    pub fn at(file: impl Into<String>, line: u32) -> Self {
        Evidence {
            file: file.into(),
            line: Line::At(line),
            why: None,
            run: None,
        }
    }

    /// A citation that can only name the file, with the reason the caller has.
    #[must_use]
    pub fn file(file: impl Into<String>, line: Line) -> Self {
        Evidence {
            file: file.into(),
            line,
            why: None,
            run: None,
        }
    }

    /// The `path:line` spelling, or the bare path when there is no line.
    #[must_use]
    pub fn location(&self) -> String {
        match self.line.number() {
            Some(line) => format!("{}:{}", self.file, line),
            None => self.file.clone(),
        }
    }

    /// The citation as one line of a pane, with the reason for a missing line printed after it.
    #[must_use]
    pub fn line_of(&self) -> String {
        let mut out = format!("evidence {}", self.location());
        if let Some(reason) = self.line.note() {
            out.push_str(" — ");
            out.push_str(&reason);
        }
        out
    }
}

/// One node, as the composition root spelled it out of the graph's dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The module key, or the `path::Symbol` spelling.
    pub key: String,
    /// Module or symbol.
    pub kind: CellKind,
    /// The `[layers]` tag; `None` when the module is untagged, which is printed, not hidden.
    pub layer: Option<String>,
    /// Whether the declaration list *may have shrunk* rather than shrunk (`s1` finding 1).
    pub may_have_shrunk: bool,
    /// `false` for a node the delta names but this revision does not have — a removed node, drawn as a
    /// ghost because a delta that silently drops its own removed end is a delta that denies having removed
    /// anything.
    pub present: bool,
    /// Where a removed node's own statement was, when the delta still knows.
    pub evidence: Option<Evidence>,
}

impl Cell {
    /// A node this revision declares.
    #[must_use]
    pub fn module(key: impl Into<String>, layer: Option<String>) -> Self {
        Cell {
            key: key.into(),
            kind: CellKind::Module,
            layer,
            may_have_shrunk: false,
            present: true,
            evidence: None,
        }
    }

    /// The module path this cell belongs to: a symbol's owning file, or a module itself.
    #[must_use]
    pub fn module_key(&self) -> &str {
        match self.kind {
            CellKind::Symbol => match self.key.rfind("::") {
                Some(at) => &self.key[..at],
                None => &self.key,
            },
            CellKind::Module | CellKind::Aggregate => &self.key,
        }
    }
}

/// The kind of a dependency, in the graph's own vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkKind {
    /// A file-granular import.
    Imports,
    /// A package-granular dependency recorded per file (Go), which is *not* a file-to-file edge.
    ImportsPackage,
    /// A use of a named symbol.
    Uses,
    /// A contract: `impl Shape for Rect`.
    Implements,
    /// An ancestry: `class Circle(Shape)`.
    Inherits,
    /// A dependency whose unit is known and whose contents are not — a star import, a barrel.
    Wildcard,
}

impl LinkKind {
    /// The stored spelling, as `graph_delta.kind` writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LinkKind::Imports => "imports",
            LinkKind::ImportsPackage => "imports_package",
            LinkKind::Uses => "uses",
            LinkKind::Implements => "implements",
            LinkKind::Inherits => "inherits",
            LinkKind::Wildcard => "wildcard",
        }
    }

    /// The stored spelling, or `None` for anything unrecognised: a foreign kind is refused, not
    /// reinterpreted as something plausible.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "imports" => Some(LinkKind::Imports),
            "imports_package" => Some(LinkKind::ImportsPackage),
            "uses" => Some(LinkKind::Uses),
            "implements" => Some(LinkKind::Implements),
            "inherits" => Some(LinkKind::Inherits),
            "wildcard" => Some(LinkKind::Wildcard),
            _ => None,
        }
    }

    /// How the canvas draws it, in one glyph (a box-to-box connector carries no more than that).
    #[must_use]
    pub const fn glyph(self) -> char {
        match self {
            LinkKind::Imports => '─',
            LinkKind::ImportsPackage => '≡',
            LinkKind::Uses => '┄',
            LinkKind::Implements => '┈',
            LinkKind::Inherits => '┅',
            LinkKind::Wildcard => '√',
        }
    }

    /// The words a pane prints beside the glyph. Package-granular edges say so, because drawing a Go
    /// package dependency as a file-to-file arrow claims a dependency the language never declared.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            LinkKind::Imports => "imports",
            LinkKind::ImportsPackage => "imports (package-granular, drawn per file)",
            LinkKind::Uses => "uses",
            LinkKind::Implements => "implements",
            LinkKind::Inherits => "inherits",
            LinkKind::Wildcard => "wildcard — unit known, contents not",
        }
    }
}

/// One dependency, as the graph declared it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The declaring module.
    pub src: String,
    /// The target: a module, or a `path::Symbol`.
    pub dst: String,
    /// The kind.
    pub kind: LinkKind,
    /// The file the declaring statement lives in — the declaring module, except when the edge was reached
    /// *through* a barrel, in which case the forwarding file is the citation (`e3.7`'s blame question).
    pub file: String,
    /// Where the statement is.
    pub line: Line,
    /// How many graph rows this one arrow stands for, when aggregation merged them.
    pub rows: u32,
    /// True when the merged rows were of more than one kind, which the pane prints rather than picking one.
    pub mixed: bool,
}

impl Link {
    /// A single-row dependency with its citation.
    #[must_use]
    pub fn new(
        src: impl Into<String>,
        dst: impl Into<String>,
        kind: LinkKind,
        file: impl Into<String>,
        line: Line,
    ) -> Self {
        let file = file.into();
        Link {
            src: src.into(),
            dst: dst.into(),
            kind,
            line,
            file,
            rows: 1,
            mixed: false,
        }
    }

    /// The citation for this edge alone.
    #[must_use]
    pub fn evidence(&self) -> Evidence {
        Evidence {
            file: self.file.clone(),
            line: self.line,
            why: None,
            run: None,
        }
    }

    /// The two-cell identity the canvas and the delta both key on.
    #[must_use]
    pub fn pair(&self) -> (String, String) {
        (self.src.clone(), self.dst.clone())
    }
}

/// One heat basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HeatBasis {
    /// Edge churn per module over a window of revisions — "is the tree hottest where it already was?"
    Churn,
    /// Structural complexity per module, which is a property of the revision rather than of its history.
    Complexity,
}

impl HeatBasis {
    /// All bases, in the order `H` cycles them.
    pub const ALL: [HeatBasis; 2] = [HeatBasis::Churn, HeatBasis::Complexity];

    /// The registry spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            HeatBasis::Churn => "churn",
            HeatBasis::Complexity => "complexity",
        }
    }

    /// The next basis, which is what `H` does.
    #[must_use]
    pub const fn next(self) -> HeatBasis {
        match self {
            HeatBasis::Churn => HeatBasis::Complexity,
            HeatBasis::Complexity => HeatBasis::Churn,
        }
    }

    /// What the number means, printed beside it so a heat map cannot be read as a cost map.
    #[must_use]
    pub const fn sentence(self) -> &'static str {
        match self {
            HeatBasis::Churn => "heat = edge churn per module over the window",
            HeatBasis::Complexity => "heat = structural complexity of the module itself",
        }
    }
}

/// The five heat bands, drawn as dots so heat survives a monochrome terminal (§10.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    /// Nothing was measured. Printed `?`, never `○○○○○`, which reads as "clean".
    Unknown,
    /// `·`.
    Nil,
    /// `●`.
    Low,
    /// `●●`.
    Mid,
    /// `●●●`.
    High,
}

impl Band {
    /// The dots. Four cells wide at most, so a heat column has a width a layout can budget for.
    #[must_use]
    pub const fn dots(self) -> &'static str {
        match self {
            Band::Unknown => "?",
            Band::Nil => "·",
            Band::Low => "●",
            Band::Mid => "●●",
            Band::High => "●●●",
        }
    }

    /// The word, for a strip that has room for it.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Band::Unknown => "unknown",
            Band::Nil => "cold",
            Band::Low => "warm",
            Band::Mid => "hot",
            Band::High => "burning",
        }
    }

    /// Which band a normalised `0.0..=1.0` value falls in. The cut points are here rather than in a
    /// spreadsheet so a test can name them.
    #[must_use]
    pub const fn of(value: f64) -> Band {
        if value <= 0.0 {
            Band::Nil
        } else if value < 0.25 {
            Band::Low
        } else if value < 0.6 {
            Band::Mid
        } else {
            Band::High
        }
    }
}

/// One basis's values, keyed by module, with the window they were measured over.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Series {
    /// Normalised `0.0..=1.0` values, keyed by module key.
    pub values: BTreeMap<String, f64>,
    /// The window or the basis detail: `over the last 30 revisions`.
    pub window: Option<String>,
    /// Why there are no values, when there are none. Required in the same sense as [`Figure::reason`]:
    /// an empty series without a reason is a heat map that claims everything is cold.
    ///
    /// [`Figure::reason`]: fathom_analyze::insight::Figure::reason
    pub missing: Option<String>,
}

impl Series {
    /// A measured series.
    #[must_use]
    pub fn measured(values: Vec<(String, f64)>, window: impl Into<String>) -> Self {
        Series {
            values: values.into_iter().collect(),
            window: Some(window.into()),
            missing: None,
        }
    }

    /// A series this build could not produce, with the reason.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Series {
            values: BTreeMap::new(),
            window: None,
            missing: Some(reason.into()),
        }
    }

    /// Was anything measured?
    #[must_use]
    pub fn known(&self) -> bool {
        !self.values.is_empty()
    }

    /// The value for one module, if it was measured.
    #[must_use]
    pub fn of(&self, key: &str) -> Option<f64> {
        self.values.get(key).copied()
    }

    /// The band for one module.
    #[must_use]
    pub fn band(&self, key: &str) -> Band {
        self.of(key).map_or(Band::Unknown, Band::of)
    }

    /// The mean of a set of keys, which is how an aggregated cell gets its heat. A cell whose members were
    /// not measured is `Unknown`, not `Nil` — averaging an empty set to zero would call an unmeasured
    /// subtree cold.
    #[must_use]
    pub fn mean<'a>(&self, keys: impl Iterator<Item = &'a str>) -> Band {
        let mut total = 0.0;
        let mut seen = 0usize;
        for key in keys {
            if let Some(value) = self.of(key) {
                total += value;
                seen += 1;
            }
        }
        if seen == 0 {
            Band::Unknown
        } else {
            Band::of(total / seen as f64)
        }
    }

    /// The strip's `heat churn ●●● over the last 30 revisions` half, or the refusal.
    #[must_use]
    pub fn sentence(&self, basis: HeatBasis) -> String {
        if let Some(reason) = self.missing.clone() {
            return format!("heat {} ? — {}", basis.as_str(), reason);
        }
        match self.window.clone() {
            Some(window) => format!("heat {} {}", basis.as_str(), window),
            None => format!("heat {}", basis.as_str()),
        }
    }
}

/// Both bases. The hand carries both so the shell can switch without a second read, and so the basis it
/// is *not* showing is still named in the header.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Heat {
    /// Edge churn.
    pub churn: Series,
    /// Structural complexity.
    pub complexity: Series,
}

impl Heat {
    /// The series for one basis.
    #[must_use]
    pub fn series(&self, basis: HeatBasis) -> &Series {
        match basis {
            HeatBasis::Churn => &self.churn,
            HeatBasis::Complexity => &self.complexity,
        }
    }

    /// The basis to open on: the first one that was actually measured, churn first when both are. A hand
    /// with only complexity does not open on a cold-looking churn map.
    #[must_use]
    pub fn default_basis(&self) -> HeatBasis {
        if self.churn.known() || !self.complexity.known() {
            HeatBasis::Churn
        } else {
            HeatBasis::Complexity
        }
    }
}

/// What one episode did to one node or edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    /// The node or edge appeared in this episode.
    Added,
    /// It went away.
    Removed,
    /// It is still there and something about it changed — a re-point, a kind change, a layer move.
    Changed,
}

impl Mark {
    /// The registry spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Mark::Added => "added",
            Mark::Removed => "removed",
            Mark::Changed => "changed",
        }
    }

    /// The canvas glyph: `╳` new, `⌀` gone, `✱` changed. Colour never carries the fact alone (§10.2), so
    /// every mark is a glyph first and a face second.
    #[must_use]
    pub const fn glyph(self) -> char {
        match self {
            Mark::Added => '╳',
            Mark::Removed => '⌀',
            Mark::Changed => '✱',
        }
    }
}

/// What an [`Op`] is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// A dependency, named by its pair. The kind is carried when the delta distinguishes it.
    Edge {
        /// The declaring module.
        src: String,
        /// The target.
        dst: String,
        /// The kind, when the delta named one.
        kind: Option<LinkKind>,
    },
    /// A node.
    Node {
        /// The node key.
        key: String,
    },
}

/// One row of the per-episode delta, in the shell's own nouns.
///
/// This is [`fathom_analyze::graph::diff::ArchitectureDelta`]'s six findings flattened into the three
/// marks a canvas can draw, because the overlay is a *presentation* of the delta (ADR-001 §4.6) and not a
/// second analysis of it. What the flattening keeps: the `why`, the citation and the finding it came from,
/// so a reader who wants the difference between `unwired` and `dropped` can still get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    /// The finding this row came from, verbatim: `edges_added`, `edges_removed`, `repointed`, `moves`,
    /// `reversals`, `surface_change`, `layer_drift`, `node_added`, `node_removed`.
    pub finding: String,
    /// What it is about.
    pub subject: Subject,
    /// The mark to draw.
    pub mark: Mark,
    /// The delta's own reason, when it had one.
    pub why: Option<String>,
    /// The citation, when the delta had one.
    pub evidence: Option<Evidence>,
}

impl Op {
    /// An edge row.
    #[must_use]
    pub fn edge(
        finding: impl Into<String>,
        src: impl Into<String>,
        dst: impl Into<String>,
        mark: Mark,
    ) -> Self {
        Op {
            finding: finding.into(),
            subject: Subject::Edge {
                src: src.into(),
                dst: dst.into(),
                kind: None,
            },
            mark,
            why: None,
            evidence: None,
        }
    }

    /// A node row.
    #[must_use]
    pub fn node(finding: impl Into<String>, key: impl Into<String>, mark: Mark) -> Self {
        Op {
            finding: finding.into(),
            subject: Subject::Node { key: key.into() },
            mark,
            why: None,
            evidence: None,
        }
    }

    /// The module the row is *anchored* to — the declaring end for an edge, the node itself for a node.
    /// Hop limits and `!` filtering are measured from this, because a reviewer asks "what did this episode
    /// touch, and what is near it", not "which unordered pair moved".
    #[must_use]
    pub fn anchor(&self) -> &str {
        match &self.subject {
            Subject::Edge { src, .. } => src,
            Subject::Node { key } => key,
        }
    }
}

/// The per-episode delta the overlay draws.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    /// The revision it starts at, as the reviewer names it.
    pub from: String,
    /// The revision it ends at.
    pub to: String,
    /// The rung the answer came from (`e3.3`): `adjacent`, `chain`, `shared_anchor`, `materialised`,
    /// `empty`. Printed, because a cheap rung cannot see a reversal and the canvas must not imply it did.
    pub rung: String,
    /// The rows.
    pub ops: Vec<Op>,
    /// Every count the delta claims, including the rows this hand did not carry. The canvas prints these
    /// when the overlay has more to say than there are cells to say it in.
    pub counts: Vec<(String, usize)>,
    /// New cycles the delta found, each as the module path round-trip.
    pub cycles: Vec<Vec<String>>,
    /// Cycles the baseline already allowed — debt with a date, which is *not* news this episode made.
    pub cycle_debt: usize,
    /// New layering violations this episode made.
    pub violations: usize,
    /// What the cheap rungs refuse to claim, in their own words.
    pub notices: Vec<String>,
}

impl Delta {
    /// A delta between two revisions with no rows — "nothing architectural changed", which is a measured
    /// answer rather than an absence.
    #[must_use]
    pub fn empty(from: impl Into<String>, to: impl Into<String>) -> Self {
        Delta {
            from: from.into(),
            to: to.into(),
            rung: "empty".to_owned(),
            ops: Vec::new(),
            counts: Vec::new(),
            cycles: Vec::new(),
            cycle_debt: 0,
            violations: 0,
            notices: Vec::new(),
        }
    }

    /// Did anything change?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty() && self.cycles.is_empty() && self.violations == 0
    }

    /// The rows touching one module, in the order the delta gave them.
    #[must_use]
    pub fn touching(&self, module: &str) -> Vec<&Op> {
        self.ops
            .iter()
            .filter(|op| match &op.subject {
                Subject::Edge { src, dst, .. } => src == module || dst == module,
                Subject::Node { key } => key == module,
            })
            .collect()
    }

    /// The modules this episode touched, sorted and deduplicated — the seed set for hop-limited filtering.
    #[must_use]
    pub fn touched(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .ops
            .iter()
            .map(Op::anchor)
            .chain(self.ops.iter().filter_map(|op| match &op.subject {
                Subject::Edge { dst, .. } => Some(dst.as_str()),
                Subject::Node { .. } => None,
            }))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The `edges +1 −1 · cycles +1 · layering +0 · debt 0` line the canvas strip prints, from the delta's
    /// own counts where it has them and from its rows where it does not.
    #[must_use]
    pub fn counts_line(&self) -> String {
        let count = |name: &str, fallback: usize| -> usize {
            self.counts
                .iter()
                .find(|(key, _)| key == name)
                .map_or(fallback, |(_, value)| *value)
        };
        let added = self
            .ops
            .iter()
            .filter(|op| matches!(op.subject, Subject::Edge { .. }) && op.mark == Mark::Added)
            .count();
        let removed = self
            .ops
            .iter()
            .filter(|op| matches!(op.subject, Subject::Edge { .. }) && op.mark == Mark::Removed)
            .count();
        format!(
            "edges +{} −{} · cycles +{} · layering +{} · debt {}",
            count("edges_added", added),
            count("edges_removed", removed),
            count("cycles", self.cycles.len()),
            count("violations", self.violations),
            self.cycle_debt
        )
    }
}

/// The statement that a hand is not the whole graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Truncated {
    /// How many modules the index holds.
    pub cells_total: usize,
    /// How many modules were handed over.
    pub cells_handed: usize,
    /// How many dependencies the index holds.
    pub links_total: usize,
    /// How many dependencies were handed over.
    pub links_handed: usize,
    /// Why the hand stopped: a byte budget, a page limit, a fan-out cap.
    pub reason: String,
}

impl Truncated {
    /// The canvas line, which says *which* of the two totals it is talking about — the hand's or the
    /// drawing's. The two are different numbers and the pane prints both.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "the hand carries {} of {} modules and {} of {} dependencies — {}",
            self.cells_handed, self.cells_total, self.links_handed, self.links_total, self.reason
        )
    }
}

/// Everything the architecture map is allowed to know.
#[derive(Debug, Clone, PartialEq)]
pub struct Hand {
    /// The revision the graph is materialised at, as the reviewer names it.
    pub revision: String,
    /// The nodes.
    pub cells: Vec<Cell>,
    /// The dependencies between them.
    pub links: Vec<Link>,
    /// The declared layers, in the order `[layers]` lists them: a lower index is a lower layer, which is
    /// what the layered layout bands by.
    pub layers: Vec<String>,
    /// The heat, both bases.
    pub heat: Heat,
    /// The episode's delta, which is the overlay.
    pub delta: Delta,
    /// Why a figure is missing, in the analysis' own words.
    pub gaps: Vec<String>,
    /// The statement that this hand is a subset, when it is.
    pub truncated: Option<Truncated>,
    /// The cost of the answer, as the rung named it: `rung adjacent · 0.19 ms`.
    pub cost: Option<String>,
}

impl Hand {
    /// A hand over a revision with a node list and an edge list.
    #[must_use]
    pub fn new(revision: impl Into<String>, cells: Vec<Cell>, links: Vec<Link>) -> Self {
        Hand {
            revision: revision.into(),
            cells,
            links,
            layers: Vec::new(),
            heat: Heat::default(),
            delta: Delta::empty("", ""),
            gaps: Vec::new(),
            truncated: None,
            cost: None,
        }
    }

    /// Did the hand carry more than [`CELL_CAP`] nodes or [`LINK_CAP`] edges?
    ///
    /// Refused rather than truncated in place: a hand that quietly became smaller is a map that lies about
    /// the shape of the system, and the composition root is the party that knows *why* it stopped.
    pub fn checked(self) -> Result<Self, String> {
        if self.cells.len() > CELL_CAP {
            return Err(format!(
                "the hand carries {} modules, over this view's {} cap — hand a subset and say so in \
                 `truncated`",
                self.cells.len(),
                CELL_CAP
            ));
        }
        if self.links.len() > LINK_CAP {
            return Err(format!(
                "the hand carries {} dependencies, over this view's {} cap — hand a subset and say so \
                 in `truncated`",
                self.links.len(),
                LINK_CAP
            ));
        }
        Ok(self)
    }

    /// How deep the deepest module path is, which is the level past which `]` would move a number and
    /// change nothing (`e3.4`).
    #[must_use]
    pub fn path_depth(&self) -> usize {
        self.cells
            .iter()
            .map(|cell| {
                cell.module_key()
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .count()
            })
            .max()
            .unwrap_or(1)
    }

    /// Look up a cell by key.
    #[must_use]
    pub fn cell(&self, key: &str) -> Option<&Cell> {
        self.cells.iter().find(|cell| cell.key == key)
    }

    /// The declared layer index of a key, `None` when untagged or unknown.
    #[must_use]
    pub fn layer_of(&self, key: &str) -> Option<usize> {
        self.cell(key)
            .and_then(|cell| cell.layer.as_deref())
            .and_then(|layer| self.layers.iter().position(|name| name == layer))
    }

    /// Every layer the hand's cells name, including tags missing from [`Hand::layers`] — an undeclared tag
    /// is a fact about the revision (`e3.3`'s layer drift) and is drawn in its own band rather than sorted
    /// into `unlayered`.
    #[must_use]
    pub fn layers_present(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .cells
            .iter()
            .filter_map(|cell| cell.layer.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The fan-in and fan-out of one module over the *whole hand*, not over what is drawn. A detail pane
    /// that counted only visible arrows would report a different fan-in at 80 columns than at 200, which is
    /// the definition of a figure that depends on the window.
    #[must_use]
    pub fn fan(&self, module: &str) -> Fan {
        let mut fan = Fan::default();
        for link in &self.links {
            if link.src == module {
                fan.out += 1;
                fan.rows_out += link.rows as usize;
                if link.dst == module {
                    fan.self_rows += link.rows as usize;
                }
            }
            if link.dst == module {
                fan.in_ += 1;
                fan.rows_in += link.rows as usize;
            }
        }
        fan
    }

    /// The dependencies leaving one module.
    #[must_use]
    pub fn outgoing(&self, module: &str) -> Vec<&Link> {
        self.links
            .iter()
            .filter(|link| link.src == module)
            .collect()
    }

    /// The dependencies arriving at one module.
    #[must_use]
    pub fn incoming(&self, module: &str) -> Vec<&Link> {
        self.links
            .iter()
            .filter(|link| link.dst == module)
            .collect()
    }
}

/// A module's fan-in and fan-out, with the row counts behind them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fan {
    /// Modules this one points at.
    pub out: usize,
    /// Modules that point at this one.
    pub in_: usize,
    /// Graph rows behind the outgoing arrows, which is more than `out` once aggregation merges kinds.
    pub rows_out: usize,
    /// Graph rows behind the incoming arrows.
    pub rows_in: usize,
    /// Rows that pointed at the module itself, counted rather than dropped.
    pub self_rows: usize,
}

impl Fan {
    /// `fan-in 4 (+1 this episode)` for one side of the pair.
    #[must_use]
    pub fn side(self, added_in: usize, added_out: usize) -> [String; 2] {
        [
            format!("fan-in   {} (+{})", self.in_, added_in),
            format!("fan-out  {} (+{})", self.out, added_out),
        ]
    }
}

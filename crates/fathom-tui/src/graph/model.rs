//! `fathom_tui::graph::model` — the drawable set: aggregation, filtering, hop reach, and the counts of
//! what is not drawn.
//!
//! This is where the ticket's one hard number lives. A hand of 3 000 modules cannot be drawn in a 54×16
//! canvas, and the two ways out are clipping and aggregation. ADR-006 §5.4 names the second one this view's
//! contract — *degrades by aggregation, not by clipping* — so **every path through this module ends in a set
//! small enough to draw**, plus a statement of what was folded to get there:
//!
//! ```text
//! nodes 214 of 1 902 shown · aggregated at L2 (auto) · 2 786 folded (`]`)
//! ```
//!
//! The knobs are the published keys and nothing else (ADR-006 §5.5): `[`/`]` aggregation level, `<`/`>` hop
//! limit, `!` show-only-what-this-episode-touched, and the layer/module filter the palette sets.
//!
//! Four rules hold it honest:
//!
//! * **The level the reviewer asked for is honoured, and the drawing says when it was overridden.** An
//!   explicit `L5` that still holds more cells than the canvas has room for is *also* folded, and the header
//!   prints `(auto)`. A picture that looks like what `]` asked for when it is not is the lie this whole
//!   module exists to stop.
//! * **Aggregation keeps the architectural part.** The fold sorts by heat and then by size and keeps those,
//!   so what lands in the counted `… other` cell is the coldest residue. Clipping would have dropped the
//!   same cells in path order and said nothing.
//! * **Counts come from the hand, not from the drawing.** [`Totals::cells_total`] is what the hand holds,
//!   [`Totals::cells_drawn`] is what fits, and the fan-in in the detail pane is the hand's — otherwise the
//!   same module would report a different fan-in at 80 columns than at 200.
//! * **A delta row that cannot be drawn is counted, not skipped** ([`Totals::ops_undrawn`]). "This episode
//!   touched 41 things and the canvas drew 9 of them" is the answer the reviewer asked for.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::hand::{self, Band, CellKind, HeatBasis, LinkKind, Mark, Subject};

/// The separator the module keys use.
///
/// Corpus-relative POSIX paths, which is what `e3.2`'s dictionary stores. The aggregator splits on this and
/// on nothing else, so a `path::Symbol` key stays inside its owning directory's cell.
const SEP: char = '/';

/// The label of the fold cell.
pub const FOLD_LABEL: &str = "… other";

/// How deep the module path is collapsed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Whatever depth fits the canvas at this size. This is the default, and the only default that keeps the
    /// 3 000-node acceptance line true on a 40-column terminal as well as on a 200-column one.
    Auto,
    /// An explicit depth: `1` is one cell per top-level directory, [`usize::MAX`] is every module.
    Depth(usize),
}

impl Level {
    /// The header's `L2` / `Lauto` spelling.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Level::Auto => "Lauto".to_owned(),
            Level::Depth(depth) => format!("L{depth}"),
        }
    }

    /// The depth as a number, with auto's own default printed as the depth auto opens at.
    #[must_use]
    pub const fn depth(self) -> usize {
        match self {
            Level::Auto => 3,
            Level::Depth(depth) => depth,
        }
    }
}

/// The view's knobs, as data, so a frame is a pure function of `(hand, knobs, size)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Knobs {
    /// Aggregation level.
    pub level: Level,
    /// Blast-radius hop limit, for highlighting and for `!`.
    pub hops: usize,
    /// `!` — draw only what this episode touched, plus its hop neighbourhood.
    pub only_touched: bool,
    /// Which heat basis the cells are coloured by.
    pub heat: HeatBasis,
}

impl Default for Knobs {
    /// The published defaults (ADR-006 §5.5): auto-aggregation, hop 2, the whole map, churn heat.
    fn default() -> Self {
        Knobs {
            level: Level::Auto,
            hops: 2,
            only_touched: false,
            heat: HeatBasis::Churn,
        }
    }
}

/// The layer/module filter. It is separate from [`Knobs`] because it is not motion: the palette sets it, and
/// it is the one thing allowed to make the canvas smaller *in meaning* rather than in size.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    /// Restrict to one layer tag.
    pub layer: Option<String>,
    /// Restrict to modules whose key contains this text.
    pub module: Option<String>,
}

impl Filter {
    /// Is anything filtered?
    #[must_use]
    pub fn active(&self) -> bool {
        self.layer.is_some() || self.module.is_some()
    }

    /// The header's clause: `layer=pilot · module~store`, or `no filter`.
    #[must_use]
    pub fn clause(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(layer) = self.layer.clone() {
            parts.push(format!("layer={layer}"));
        }
        if let Some(module) = self.module.clone() {
            parts.push(format!("module~{module}"));
        }
        if parts.is_empty() {
            "no filter".to_owned()
        } else {
            parts.join(" · ")
        }
    }
}

/// One drawable cell: one module at full depth, several of them collapsed, or the fold.
#[derive(Debug, Clone)]
pub struct Cell {
    /// The cell's key: a module key, a directory prefix, or the fold label.
    pub key: String,
    /// What kind of cell this is now.
    pub kind: CellKind,
    /// The module keys folded into it. Empty only for the fold cell, whose members are the residue.
    pub members: Vec<String>,
    /// How many modules are in here, including the fold cell's.
    pub total: usize,
    /// The single layer tag every member shares, when they do.
    pub layer: Option<String>,
    /// Whether members disagree about their layer — layer drift is a fact a cell must not swallow.
    pub mixed_layer: bool,
    /// Whether the episode added, removed or changed any member.
    pub mark: Option<Mark>,
    /// The delta findings behind [`Cell::mark`], for the detail pane.
    pub findings: Vec<String>,
    /// Whether every member is gone from this revision (a ghost cell).
    pub ghost: bool,
    /// The heat band, from the basis in [`Knobs::heat`].
    pub band: Band,
    /// The mean heat value, when any member was measured.
    pub heat: Option<f64>,
    /// Whether a member's declaration list may have shrunk rather than shrunk.
    pub may_have_shrunk: bool,
    /// Whether this cell is in the selected cell's hop neighbourhood.
    pub near: bool,
    /// Whether this is the fold cell.
    pub folded: bool,
    /// The citation any member's delta row carried.
    pub evidence: Option<hand::Evidence>,
}

impl Cell {
    /// The label, clipped to `width` display cells with the cut marked.
    #[must_use]
    pub fn label(&self, width: usize) -> String {
        clip(&self.key, width)
    }

    /// The first module key inside the cell, which is what a detail pane seeds from.
    #[must_use]
    pub fn seed(&self) -> &str {
        self.members
            .first()
            .map(String::as_str)
            .unwrap_or(&self.key)
    }

    /// Does this cell hold the given module key?
    #[must_use]
    pub fn holds(&self, key: &str) -> bool {
        !self.folded && (self.key == key || self.members.iter().any(|member| member == key))
    }
}

/// One drawable dependency: possibly several graph rows merged by aggregation.
#[derive(Debug, Clone)]
pub struct Link {
    /// The source cell's key.
    pub src: String,
    /// The target cell's key.
    pub dst: String,
    /// The kind, or `None` when merged rows disagreed — printed as *mixed*, never as one of them.
    pub kind: Option<LinkKind>,
    /// How many graph rows this arrow stands for.
    pub rows: u32,
    /// The episode's mark on it, if it has one.
    pub mark: Option<Mark>,
    /// The delta findings behind [`Link::mark`].
    pub findings: Vec<String>,
    /// The citation to print: a marked row's own if it had one, else the first hand row's.
    pub evidence: Option<hand::Evidence>,
    /// Whether either end is in the selected cell's hop neighbourhood.
    pub near: bool,
}

impl Link {
    /// The kind's words, with the merged case spelled out instead of guessed.
    #[must_use]
    pub fn kind_words(&self) -> String {
        match self.kind {
            Some(kind) if self.rows > 1 => format!("{} ×{}", kind.words(), self.rows),
            Some(kind) => kind.words().to_owned(),
            None => format!("mixed kinds ({} rows)", self.rows),
        }
    }
}

/// What is drawn, and what the drawing stands for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Totals {
    /// Cells in the hand.
    pub cells_total: usize,
    /// Cells that survived the filter and the `!` reach, before aggregation.
    pub cells_filtered: usize,
    /// Cells the canvas draws.
    pub cells_drawn: usize,
    /// Modules folded into the `… other` cell.
    pub cells_folded: usize,
    /// Links in the hand.
    pub links_total: usize,
    /// Links the canvas draws.
    pub links_drawn: usize,
    /// Links that became cell-to-itself arrows when aggregation merged their ends.
    pub links_internal: usize,
    /// Links dropped because an end was filtered out.
    pub links_undrawn: usize,
    /// Delta rows whose end is not on the canvas.
    pub ops_undrawn: usize,
    /// Cells the filter dropped.
    pub dropped_by_filter: usize,
    /// The depth actually drawn.
    pub depth: usize,
    /// Whether the drawn depth is the canvas's choice rather than the reviewer's.
    pub auto: bool,
}

/// The scene: the drawable set, the reach sets, and the statements the build made about itself.
#[derive(Debug, Clone)]
pub struct Scene {
    /// The cells, in aggregation order.
    pub cells: Vec<Cell>,
    /// The links between them.
    pub links: Vec<Link>,
    /// The counts.
    pub totals: Totals,
    /// The module keys the episode touched.
    pub touched: BTreeSet<String>,
    /// Which cell key holds each module.
    pub cell_of: HashMap<String, String>,
    /// Statements the build made about itself: an auto-aggregation, a fold, undrawn delta rows.
    pub notices: Vec<String>,
}

impl Scene {
    /// An empty scene: what a caller with nothing handed draws. Never a blank canvas — the pane prints
    /// [`crate::graph::Graph::empty_statement`] over it.
    #[must_use]
    pub fn empty() -> Scene {
        Scene {
            cells: Vec::new(),
            links: Vec::new(),
            totals: Totals::default(),
            touched: BTreeSet::new(),
            cell_of: HashMap::new(),
            notices: Vec::new(),
        }
    }

    /// Build the drawable set for one hand, one set of knobs, one filter and one canvas capacity.
    #[must_use]
    pub fn build(source: &hand::Hand, knobs: &Knobs, filter: &Filter, capacity: usize) -> Scene {
        let capacity = capacity.max(1);
        let heat = source.heat.series(knobs.heat);
        let touched: BTreeSet<String> = source
            .delta
            .touched()
            .into_iter()
            .map(|key| key.to_owned())
            .collect();
        let adjacency = adjacency(source);
        // The hop reach is walked over the *hand*, not over the drawing: a hop limit whose meaning changed
        // when the canvas got narrower would be answering a different question at every width.
        let reach: BTreeSet<String> = if knobs.only_touched {
            within(
                &adjacency,
                &touched.iter().map(String::as_str).collect::<Vec<_>>(),
                knobs.hops,
            )
        } else {
            BTreeSet::new()
        };

        // Nodes the delta names but the revision does not have still get a cell: a ghost is the only place a
        // "removed" arrow can point, and a delta whose removed end vanished has denied making the removal.
        let ghosts = ghosts(source, filter, &touched);
        let mut live: Vec<&hand::Cell> = source
            .cells
            .iter()
            .filter(|cell| passes(cell, filter, knobs.only_touched.then_some(&reach)))
            .collect();
        let dropped = source.cells.len() - live.len();
        live.extend(ghosts.iter());
        let filtered = live.len();

        // Depth: the reviewer's, or the shallowest that fits.
        let deepest = live
            .iter()
            .map(|cell| depth_of(cell.module_key()))
            .max()
            .unwrap_or(1);
        let mut auto = matches!(knobs.level, Level::Auto);
        let depth = match knobs.level {
            Level::Auto => {
                let mut depth = 1usize;
                while depth < deepest && group_count(&live, depth) > capacity {
                    depth += 1;
                }
                depth
            }
            Level::Depth(asked) => asked.clamp(1, deepest),
        };
        let mut groups = group(&live, depth, source, heat, &touched);
        let mut notices: Vec<String> = Vec::new();
        let mut folded = 0usize;
        if groups.len() > capacity {
            // Aggregation's last rung, and still aggregation: keep the hot and the big, fold the rest into
            // one counted cell rather than dropping the tail of a sorted list.
            groups.sort_by(|a, b| {
                b.heat
                    .partial_cmp(&a.heat)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.members_total.cmp(&a.members_total))
                    .then_with(|| a.key.cmp(&b.key))
            });
            let keep = capacity.saturating_sub(1).max(1);
            let rest: Vec<Group> = groups.split_off(keep.min(groups.len()));
            folded = rest.iter().map(|group| group.members_total).sum();
            notices.push(format!(
                "{} modules folded into `{FOLD_LABEL}` — the coldest at this size; `]` goes one level down",
                folded
            ));
            let marks: Vec<Mark> = rest.iter().filter_map(|group| group.mark).collect();
            let shrunk = rest.iter().any(|group| group.may_have_shrunk);
            groups.push(Group {
                key: format!("{FOLD_LABEL} ({folded})"),
                members: Vec::new(),
                members_total: folded,
                layer: None,
                mixed_layer: false,
                mark: marks.first().copied(),
                findings: Vec::new(),
                ghost: false,
                heat: None,
                may_have_shrunk: shrunk,
                folded: true,
                evidence: None,
            });
            auto = true;
        }

        let mut cell_of: HashMap<String, String> = HashMap::with_capacity(filtered);
        let mut cells: Vec<Cell> = Vec::with_capacity(groups.len());
        for group in groups {
            for member in &group.members {
                cell_of.insert(member.clone(), group.key.clone());
            }
            if group.folded {
                cell_of.insert(group.key.clone(), group.key.clone());
            }
            cells.push(Cell {
                kind: if group.members_total > 1 {
                    CellKind::Aggregate
                } else {
                    source
                        .cell(&group.key)
                        .map(|cell| cell.kind)
                        .unwrap_or(CellKind::Module)
                },
                key: group.key.clone(),
                members: group.members,
                total: group.members_total,
                layer: group.layer,
                mixed_layer: group.mixed_layer,
                mark: group.mark,
                findings: group.findings,
                ghost: group.ghost,
                band: group.heat.map_or(Band::Unknown, Band::of),
                heat: group.heat,
                may_have_shrunk: group.may_have_shrunk,
                near: false,
                folded: group.folded,
                evidence: group.evidence,
            });
        }

        // Links, mapped through the cell map and merged. An endpoint that is not in the map was filtered
        // away, and is counted rather than dropped.
        let mut merged: BTreeMap<(String, String), Link> = BTreeMap::new();
        let mut internal = 0usize;
        let mut undrawn = 0usize;
        for link in &source.links {
            let src = cell_of.get(&link.src).cloned();
            let dst = cell_of.get(&link.dst).cloned();
            let (Some(src), Some(dst)) = (src, dst) else {
                undrawn += 1;
                continue;
            };
            if src == dst {
                internal += 1;
                continue;
            }
            let entry = merged
                .entry((src.clone(), dst.clone()))
                .or_insert_with(|| Link {
                    src,
                    dst,
                    kind: None,
                    rows: 0,
                    mark: None,
                    findings: Vec::new(),
                    evidence: None,
                    near: false,
                });
            match entry.kind {
                None => entry.kind = Some(link.kind),
                Some(seen) if seen != link.kind => entry.kind = None,
                _ => {}
            }
            entry.rows = entry.rows.saturating_add(link.rows);
            if entry.evidence.is_none() {
                entry.evidence = Some(link.evidence());
            }
        }

        // The overlay: marks onto cells and arrows, counting what the canvas has no room for.
        let mut ops_undrawn = 0usize;
        for op in &source.delta.ops {
            match &op.subject {
                Subject::Edge { src, dst, kind } => {
                    let key = (
                        cell_of.get(src).cloned().unwrap_or_default(),
                        cell_of.get(dst).cloned().unwrap_or_default(),
                    );
                    match merged.get_mut(&key) {
                        Some(entry) if !key.0.is_empty() && !key.1.is_empty() => {
                            entry.mark = Some(op.mark);
                            entry.findings.push(op.finding.clone());
                            if op.evidence.is_some() {
                                entry.evidence = op.evidence.clone();
                            }
                            if let Some(kind) = kind {
                                entry.kind = Some(*kind);
                            }
                        }
                        _ => ops_undrawn += 1,
                    }
                }
                Subject::Node { key } => {
                    let target = cell_of
                        .get(key)
                        .and_then(|cell| cells.iter().position(|c| &c.key == cell))
                        .or_else(|| cells.iter().position(|cell| cell.key == *key));
                    match target {
                        Some(at) => {
                            cells[at].mark = Some(op.mark);
                            cells[at].findings.push(op.finding.clone());
                            if op.evidence.is_some() {
                                cells[at].evidence = op.evidence.clone();
                            }
                        }
                        None => ops_undrawn += 1,
                    }
                }
            }
        }
        if ops_undrawn > 0 {
            notices.push(format!(
                "{} delta rows name a module this canvas is not showing — the detail pane lists them all",
                ops_undrawn
            ));
        }

        let mut totals = Totals {
            cells_total: source.cells.len(),
            cells_filtered: filtered,
            cells_drawn: cells.len(),
            cells_folded: folded,
            links_total: source.links.len(),
            links_drawn: merged.len(),
            links_internal: internal,
            links_undrawn: undrawn,
            ops_undrawn,
            depth,
            auto,
            dropped_by_filter: dropped,
        };
        totals.cells_total = totals.cells_total.max(totals.cells_filtered);
        if totals.auto {
            notices.insert(
                0,
                format!(
                    "aggregated to L{} to fit {} of {} modules in this canvas — `]` goes deeper",
                    totals.depth, totals.cells_drawn, totals.cells_filtered
                ),
            );
        }
        Scene {
            cells,
            links: merged.into_values().collect(),
            totals,
            touched,
            cell_of,
            notices,
        }
    }

    /// Mark the selection's hop neighbourhood.
    ///
    /// Separate from [`Scene::build`] because the selection is a frame fact rather than a data fact: the
    /// drawn set must not depend on where the cursor is, or `<`/`>` would look like a filter.
    pub fn highlight(&mut self, source: &hand::Hand, selection: Option<&str>, hops: usize) {
        for cell in &mut self.cells {
            cell.near = false;
        }
        for link in &mut self.links {
            link.near = false;
        }
        let Some(seed) = selection else { return };
        let seeds = vec![seed];
        let reach = within(&adjacency(source), &seeds, hops);
        for cell in &mut self.cells {
            cell.near = cell.members.iter().any(|member| reach.contains(member))
                || reach.contains(&cell.key);
        }
        for link in &mut self.links {
            link.near = self
                .cells
                .iter()
                .any(|cell| cell.near && (cell.key == link.src || cell.key == link.dst));
        }
    }

    /// The cell holding a module key, if one is drawn.
    #[must_use]
    pub fn cell_holding(&self, key: &str) -> Option<&Cell> {
        self.cell_of
            .get(key)
            .and_then(|cell| self.cells.iter().find(|candidate| candidate.key == *cell))
            .or_else(|| self.cells.iter().find(|cell| cell.key == key))
    }

    /// The index of the cell holding a module key.
    #[must_use]
    pub fn index_holding(&self, key: &str) -> Option<usize> {
        self.cells
            .iter()
            .position(|cell| cell.holds(key) || cell.key == key)
    }

    /// The links touching one cell: outgoing first, then incoming, each in aggregation order.
    #[must_use]
    pub fn touching(&self, key: &str) -> (Vec<&Link>, Vec<&Link>) {
        // A link's end names a *cell*, and a folded cell answers to any module inside it: asking "what
        // crosses this file?" of a file the canvas drew inside a directory cell has to be answered with the
        // directory's arrow, or the answer is "nothing", which is a lie about the picture.
        let holds = |end: &str| {
            self.cells.iter().any(|cell| {
                cell.key == end || cell.seed() == end || cell.members.iter().any(|m| m == end)
            })
        };
        let name = |cell: &Cell| cell.key.clone();
        let cell_key = |end: &str| -> String {
            self.cells
                .iter()
                .find(|cell| {
                    cell.key == end || cell.seed() == end || cell.members.iter().any(|m| m == end)
                })
                .map(name)
                .unwrap_or_else(|| end.to_owned())
        };
        let want = cell_key(key);
        (
            self.links
                .iter()
                .filter(|link| {
                    (link.src == want || holds(&link.src)) && cell_key(&link.src) == want
                })
                .collect(),
            self.links
                .iter()
                .filter(|link| {
                    (link.dst == want || holds(&link.dst)) && cell_key(&link.dst) == want
                })
                .collect(),
        )
    }

    /// Is anything drawn at all? A blank canvas is the one answer this view may never give, which is why the
    /// caller checks this rather than drawing the emptiness.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The `nodes 214 of 1 902 shown · aggregated at L2` line — the acceptance criterion's own wording, and
    /// the pane's promise that nothing was clipped.
    #[must_use]
    pub fn counts_line(&self) -> String {
        let mut line = format!(
            "nodes {} of {} shown · aggregated at L{}{}",
            self.totals.cells_drawn,
            self.totals.cells_total,
            self.totals.depth,
            if self.totals.auto { " (auto)" } else { "" }
        );
        if self.totals.cells_folded > 0 {
            line.push_str(&format!(" · {} folded (`]`)", self.totals.cells_folded));
        }
        if self.totals.links_drawn < self.totals.links_total {
            line.push_str(&format!(
                " · edges {} of {}",
                self.totals.links_drawn, self.totals.links_total
            ));
        }
        if self.totals.links_internal > 0 {
            line.push_str(&format!(" · {} inside cells", self.totals.links_internal));
        }
        line
    }
}

/// Does a cell survive the filter and the `!` reach?
fn passes(cell: &hand::Cell, filter: &Filter, reach: Option<&BTreeSet<String>>) -> bool {
    if let Some(reach) = reach {
        let module = cell.module_key();
        if !reach.iter().any(|key| key == module) {
            return false;
        }
    }
    if let Some(layer) = filter.layer.as_deref()
        && cell.layer.as_deref() != Some(layer)
    {
        return false;
    }
    if let Some(needle) = filter.module.as_deref()
        && !cell.key.contains(needle)
    {
        return false;
    }
    true
}

/// The delta's own nodes that this revision does not have, as ghost cells.
///
/// A ghost under a layer filter is dropped — it has no layer to match, and inventing one would be a claim
/// about a node the revision does not contain. The delta row that named it is then counted in
/// [`Totals::ops_undrawn`] rather than disappearing.
fn ghosts(source: &hand::Hand, filter: &Filter, touched: &BTreeSet<String>) -> Vec<hand::Cell> {
    let mut out: Vec<hand::Cell> = Vec::new();
    // Only the revision's own nodes count as "seen": a touched key is exactly what a delta names, so
    // seeding from `touched` would suppress the ghost of every node the episode removed — the one case
    // a ghost exists for.
    let _ = touched;
    let mut seen: BTreeSet<String> = source.cells.iter().map(|cell| cell.key.clone()).collect();
    for op in &source.delta.ops {
        for key in ends(&op.subject) {
            if !seen.insert(key.to_owned()) {
                continue;
            }
            if source.cell(key).is_some() {
                continue;
            }
            if filter.layer.is_some() {
                continue;
            }
            if let Some(needle) = filter.module.as_deref()
                && !key.contains(needle)
            {
                continue;
            }
            out.push(hand::Cell {
                key: key.to_owned(),
                kind: if key.contains("::") {
                    CellKind::Symbol
                } else {
                    CellKind::Module
                },
                layer: None,
                may_have_shrunk: false,
                present: false,
                evidence: op.evidence.clone(),
            });
        }
    }
    out
}

/// Both ends of a delta subject, as keys.
fn ends(subject: &Subject) -> Vec<&str> {
    match subject {
        Subject::Edge { src, dst, .. } => vec![src.as_str(), dst.as_str()],
        Subject::Node { key } => vec![key.as_str()],
    }
}

/// The directory depth of a module key: `src/store/persist.rs` is three deep.
#[must_use]
fn depth_of(key: &str) -> usize {
    key.split(SEP).count()
}

/// Take a key to a directory depth: depth 2 of `src/store/persist.rs` is `src/store`, depth 3 is the whole
/// key. At a depth one short of the whole key the file stem is kept, so `src/store.rs` and a `src/store/`
/// directory cannot collapse into one cell and claim to be the same module.
#[must_use]
pub fn prefix(key: &str, depth: usize) -> String {
    let parts: Vec<&str> = key.split(SEP).collect();
    if parts.len() <= depth {
        return key.to_owned();
    }
    let mut out = parts[..depth].join("/");
    if depth + 1 == parts.len()
        && let Some(stem) = parts.last().and_then(|last| last.split('.').next())
    {
        out.push('/');
        out.push_str(stem);
    }
    out
}

/// How many cells depth `depth` would produce.
fn group_count(live: &[&hand::Cell], depth: usize) -> usize {
    let deepest = live
        .iter()
        .map(|cell| depth_of(cell.module_key()))
        .max()
        .unwrap_or(1);
    if depth >= deepest {
        return live.len();
    }
    let mut keys: Vec<String> = live
        .iter()
        .map(|cell| prefix(cell.module_key(), depth))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys.len()
}

/// One aggregation group, before the fold.
struct Group {
    key: String,
    members: Vec<String>,
    members_total: usize,
    layer: Option<String>,
    mixed_layer: bool,
    mark: Option<Mark>,
    findings: Vec<String>,
    ghost: bool,
    heat: Option<f64>,
    may_have_shrunk: bool,
    folded: bool,
    evidence: Option<hand::Evidence>,
}

/// Group cells by their prefix at `depth`, carrying each group's heat, layer and delta marks.
///
/// Heat and marks are computed here rather than in a second pass because the walk over the delta is the
/// expensive part and the group boundaries are what decide it: one pass, one answer, and the fold that
/// follows sorts on a number that is already attached.
fn group(
    live: &[&hand::Cell],
    depth: usize,
    source: &hand::Hand,
    heat: &hand::Series,
    touched: &BTreeSet<String>,
) -> Vec<Group> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Group> = HashMap::new();
    for cell in live {
        let whole = depth >= depth_of(cell.module_key());
        let key = if whole {
            cell.key.clone()
        } else {
            prefix(cell.module_key(), depth)
        };
        let fresh = !groups.contains_key(&key);
        if fresh {
            order.push(key.clone());
            groups.insert(
                key.clone(),
                Group {
                    key: key.clone(),
                    members: Vec::new(),
                    members_total: 0,
                    layer: None,
                    mixed_layer: false,
                    mark: None,
                    findings: Vec::new(),
                    ghost: true,
                    heat: None,
                    may_have_shrunk: false,
                    folded: false,
                    evidence: None,
                },
            );
        }
        let Some(entry) = groups.get_mut(&key) else {
            continue; // unreachable: the entry exists by construction
        };
        entry.members.push(cell.key.clone());
        entry.members_total += 1;
        match (&entry.layer, &cell.layer) {
            (None, Some(layer)) => entry.layer = Some(layer.clone()),
            (Some(have), Some(also)) if have != also => {
                entry.mixed_layer = true;
                entry.layer = None;
            }
            _ => {}
        }
        entry.may_have_shrunk |= cell.may_have_shrunk;
        entry.ghost &= !cell.present;
        if let Some(evidence) = &cell.evidence
            && entry.evidence.is_none()
        {
            entry.evidence = Some(evidence.clone());
        }
    }
    for key in &order {
        let Some(entry) = groups.get_mut(key) else {
            continue;
        };
        entry.members.sort();
        entry.members.dedup();
        entry.heat = heat_mean(
            entry
                .members
                .iter()
                .filter_map(|member| heat.of(member))
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let marks: Vec<(Mark, String)> = entry
            .members
            .iter()
            .flat_map(|member| source.delta.touching(member))
            .map(|op| (op.mark, op.finding.clone()))
            .collect();
        entry.mark = marks.first().map(|(mark, _)| *mark);
        entry.findings = marks.into_iter().map(|(_, finding)| finding).collect();
        if entry.mark.is_none()
            && entry
                .members
                .iter()
                .any(|member| touched.contains(member.as_str()))
        {
            entry.mark = Some(Mark::Changed);
        }
    }
    order.iter().filter_map(|key| groups.remove(key)).collect()
}

/// The mean of a non-empty list. `None` for the empty one, which is *unknown* and not zero.
#[must_use]
fn heat_mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

/// The module-level adjacency of the hand, both directions, for the hop walk.
fn adjacency(source: &hand::Hand) -> BTreeMap<&str, Vec<&str>> {
    let mut out: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for link in &source.links {
        out.entry(link.src.as_str())
            .or_default()
            .push(link.dst.as_str());
        out.entry(link.dst.as_str())
            .or_default()
            .push(link.src.as_str());
    }
    out
}

/// Every module within `hops` of any seed, breadth-first, seeds included.
fn within<'a>(
    adjacency: &BTreeMap<&str, Vec<&'a str>>,
    seeds: &[&'a str],
    hops: usize,
) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = seeds.iter().map(|seed| (*seed).to_owned()).collect();
    let mut queue: VecDeque<(&str, usize)> = seeds.iter().map(|seed| (*seed, 0usize)).collect();
    while let Some((here, depth)) = queue.pop_front() {
        if depth >= hops {
            continue;
        }
        for next in adjacency.get(here).into_iter().flatten() {
            if seen.insert((*next).to_owned()) {
                queue.push_back((next, depth + 1));
            }
        }
    }
    seen
}

/// Clip to `width` display cells, marking the cut. Same rule as the rest of the shell: a clipped line says
/// it was clipped.
#[must_use]
pub fn clip(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    if width <= 1 {
        return "…".to_owned();
    }
    let mut out = String::new();
    let mut cells = 0usize;
    for glyph in text.graphemes(true) {
        let wide = UnicodeWidthStr::width(glyph);
        if cells + wide + 1 > width {
            break;
        }
        out.push_str(glyph);
        cells += wide;
    }
    out.push('…');
    out
}

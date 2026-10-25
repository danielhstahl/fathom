//! `fathom-tui::graph` — the architecture map: two coordinated views over one graph model, with the
//! per-episode delta drawn on top of both. **This is ADR-006 §5.5 in Rust**, owned by `reco-w3w` (`e3.4`).
//!
//! The question the view answers is the one no other view can: *did this turn make the shape of the system
//! worse?* Three maps answer it, all of them the same [`Scene`] under different geometry —
//!
//! * the **structure map** ([`Layout::Treemap`]), a hierarchical treemap of the module tree with heat on
//!   top, where heat is churn or complexity and `H` switches between them;
//! * the **dependency map** in its two published layouts — [`Layout::Layered`], columns by `[layers]` tag or
//!   by longest-path depth, and [`Layout::Circular`], one ring for reading clusters;
//! * the **aggregated node table** ([`Layout::Table`]), what the canvas falls back to when `graph-layout` is
//!   not compiled in — a table of the same cells, never a blank pane.
//!
//! On top of all three sits the **delta overlay**: added edges solid green, removed edges *ghosted* — dashed
//! and red, because ADR-006 §10.2 forbids a colour carrying a fact alone — changed nodes outlined in a
//! double rule, new and removed nodes marked `╳` and `⌀`, all of it filtered by hop limit and by `!` (show
//! only what this episode touched). The overlay is a layer over the same graph and never a second rendering
//! of the delta (ADR-001 §4.6), which is why it is marks on [`model::Cell`] and [`model::Link`] rather than a
//! list beside the picture.
//!
//! Four properties, one per acceptance criterion:
//!
//! * **3 000 nodes render in under 100 ms, by aggregating** ([`Scene::build`], [`Layout::capacity`]). The
//!   canvas has a cell budget derived from the pane's size; past it the view folds a directory level instead
//!   of drawing 3 000 boxes into 864 cells, and the fold is counted and named on the canvas.
//!   `crates/fathom-tui/tests/graph_bench.rs` times the real call against a 3 000-module hand.
//! * **Legible at 80×24, degrading by aggregation and never by clipping.** Every cell not drawn is counted in
//!   the canvas's own lines, and every arrow that cannot be routed is counted in
//!   [`layout::Drawing::arrows_undrawn`] rather than drawn through a name.
//! * **Keyboard-only on the published keymap** (ADR-006 §5.5, §7): `[`/`]` aggregation, `<`/`>` hop limit,
//!   `!` touched-only, `L` layout cycle, `H` heat basis, `j`/`k` over the cells, `<Enter>` to read a cell's
//!   arrows and their citations, `<Tab>` to the detail pane, `<Esc>` out. The mouse issues the same actions:
//!   a click resolves through [`Graph::hit`], which is the same list the cursor walks (ADR-006 §10.1).
//! * **Any edge reveals the file-and-line evidence that produced it** ([`Evidence`], [`Line`]). A line,
//!   several candidate lines, or no line recorded are three honest answers; the pane prints the one the
//!   record supports and says why when it is not a number.
//!
//! What this module does **not** do is compute anything about the repository. It holds no store handle and
//! opens no file (ADR-002 §5.2, lint `E01`), and it names `fathom_analyze` only through the façade (lint
//! `M02`): the graph, the heat and the delta arrive as one [`Hand`] built by the composition root, and every
//! number on the canvas is cited to it.

pub mod hand;
mod layout;
mod model;
mod plate;

pub use hand::{
    Band, Cell as SourceCell, CellKind, Debt, Delta, Evidence, Fan, Hand, Heat, HeatBasis, Line,
    Link as SourceLink, LinkKind, Mark, Op, Series, Subject, Truncated,
};
pub use layout::{Drawing, Layout};
pub use model::{Cell, Filter, Knobs, Level, Link, Scene, Totals, clip};

use std::cell::RefCell;
use std::fmt;

use crate::style::{Face, StyledLine};

/// The widest blast radius this canvas walks.
///
/// Six hops is where "what does this change touch?" stops being a question about a neighbourhood and
/// becomes a question about the whole graph — which is what the aggregation levels are for. The key refuses
/// there rather than wrapping, so `>` cannot be pressed past the state the reviewer asked for (`e3.4`).
const MAX_HOPS: usize = 6;

/// Who can name the statement an arrow came from, if anyone (`e3.4`).
///
/// A trait-object provider rather than a field on [`Hand`], for the same reason the diff pane's bytes are a
/// provider and not a `Vec`: naming the line means reading the file's structural record, which for a whole
/// map is a thousand reads nobody asked for. The canvas asks for one arrow at a time — the one the cursor is
/// on — and the composition root answers from the store it holds and the shell may not (`ADR-002 §5.2`).
pub trait Citations: Send + Sync {
    /// The citation for the dependency `src -> dst`, or `None` when this build has no answer.
    fn cite(&self, src: &str, dst: &str) -> Option<Evidence>;
}

impl<F> Citations for F
where
    F: for<'a, 'b> Fn(&'a str, &'b str) -> Option<Evidence> + Send + Sync,
{
    fn cite(&self, src: &str, dst: &str) -> Option<Evidence> {
        self(src, dst)
    }
}

/// The boxed provider, held by [`crate::frame::Options::citations`] the same way the patch provider is held.
#[derive(Clone)]
pub struct CitationSource(pub std::sync::Arc<dyn Citations>);

impl CitationSource {
    /// Box a closure as a provider.
    #[must_use]
    pub fn new(citations: impl Citations + 'static) -> Self {
        CitationSource(std::sync::Arc::new(citations))
    }
}

impl fmt::Debug for CitationSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CitationSource").finish_non_exhaustive()
    }
}

/// What one `(hand, knobs, filter, layout, size, selection)` produced, so a frame that changed nothing draws
/// from the answer it already had.
struct Cache {
    key: (String, Knobs, Filter, Layout, usize, usize, Option<usize>),
    scene: Scene,
    drawing: Drawing,
    cols: usize,
    rows: usize,
}

impl fmt::Debug for Cache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cache")
            .field("cells", &self.scene.cells.len())
            .field("links", &self.scene.links.len())
            .finish_non_exhaustive()
    }
}

/// The graph view's own state, held by [`crate::app::App`] beside the timeline's and the diff's.
#[derive(Debug)]
pub struct Graph {
    /// What the composition root handed over. `None` is a repository with no module graph in this index, or
    /// a build with no store to read, and the pane says which.
    hand: Option<Hand>,
    knobs: Knobs,
    filter: Filter,
    layout: Layout,
    /// The selected cell, as an index into [`Scene::cells`].
    cell: Option<usize>,
    /// The detail pane's cursor, which walks the selected cell's arrows.
    detail_row: usize,
    /// The last thing the view decided, for the toast line.
    pub notice: Option<String>,
    /// Who names an arrow's statement, installed by the composition root (`e3.4`). `None` is a build with
    /// no structural record to read, and the detail pane says so instead of printing a bare path.
    citations: Option<CitationSource>,
    cache: RefCell<Option<Cache>>,
}

impl Default for Graph {
    fn default() -> Self {
        Graph::new()
    }
}

impl Graph {
    /// An empty view with nothing handed, at the published defaults: auto-aggregation, hop 2, whole map.
    #[must_use]
    pub fn new() -> Self {
        Graph {
            hand: None,
            knobs: Knobs {
                level: Level::Auto,
                hops: 2,
                only_touched: false,
                heat: HeatBasis::Churn,
            },
            filter: Filter::default(),
            layout: Layout::preferred(),
            cell: None,
            detail_row: 0,
            notice: None,
            citations: None,
            cache: RefCell::new(None),
        }
    }

    // --- the hand-over -------------------------------------------------------------------------

    /// Install what the composition root read, and say what it means.
    pub fn install(&mut self, hand: Hand) -> String {
        self.hand = Some(hand);
        self.cell = None;
        self.detail_row = 0;
        self.cache.borrow_mut().take();
        self.handed()
    }

    /// Is there a graph to draw at all?
    #[must_use]
    pub fn has_data(&self) -> bool {
        self.hand.is_some()
    }

    /// Install the party that can name an arrow's statement (`e3.4`).
    pub fn set_citations(&mut self, citations: Option<CitationSource>) {
        self.citations = citations;
    }

    /// The sentence about the hand: what revision, what it cost, what it left out.
    #[must_use]
    pub fn handed(&self) -> String {
        let Some(hand) = self.hand.as_ref() else {
            return "no module graph handed to this shell".to_owned();
        };
        let mut parts = vec![format!(
            "graph at {} · {} modules · {} dependencies",
            hand.revision,
            hand.cells.len(),
            hand.links.len()
        )];
        if let Some(cost) = hand.cost.clone() {
            parts.push(cost);
        }
        if let Some(truncated) = hand.truncated.clone() {
            parts.push(truncated.sentence());
        }
        parts.join(" · ")
    }

    /// The half of [`Self::handed`] that is a notice rather than a fact: what the read **left out**.
    ///
    /// `None` when the hand is whole. The whole sentence — revision, module count, cost — stays in the
    /// pane's own detail, because it is a fact about the picture; counting it as a notice would light the
    /// status strip's `⚠` badge on every repository whose index holds a module graph, which since `e3.11`
    /// is every repository that has been indexed, and a badge that is always lit is a badge nobody reads.
    /// What the read could not fit is different: it is the reason the map is smaller than the repository,
    /// and the reviewer is owed it before they conclude the codebase is simple.
    #[must_use]
    pub fn handed_truncation(&self) -> Option<String> {
        let hand = self.hand.as_ref()?;
        hand.truncated
            .as_ref()
            .map(|truncated| format!("module graph: {}", truncated.sentence()))
    }

    /// Why the pane has nothing, in the words ADR-006 §10.3 requires: a fact and a next action.
    #[must_use]
    pub fn empty_statement(&self) -> String {
        "no module graph in this index — the indexer writes graph snapshots (`e3.2`); `fathom doctor` \
         says whether this build has a store at all"
            .to_owned()
    }

    // --- knobs ---------------------------------------------------------------------------------

    /// The current knobs.
    #[must_use]
    pub const fn knobs(&self) -> Knobs {
        self.knobs
    }

    /// Put the aggregation level there directly.
    ///
    /// The keys are the ordinary way to move this (`[` and `]`), and they refuse at the bounds like any
    /// other key; this is the same state for a caller that has a level to restore rather than a keystroke
    /// to simulate — the session file, and a test that wants one level and not a walk to it.
    pub fn set_level(&mut self, level: Level) {
        self.knobs.level = level;
        self.cell = None;
    }

    /// The current filter.
    #[must_use]
    pub fn filter(&self) -> &Filter {
        &self.filter
    }

    /// The current layout.
    #[must_use]
    pub const fn layout(&self) -> Layout {
        self.layout
    }

    /// The heat basis in force.
    #[must_use]
    pub const fn heat(&self) -> HeatBasis {
        self.knobs.heat
    }

    /// Filter to one layer tag. `None` clears it.
    pub fn filter_layer(&mut self, layer: Option<String>) {
        self.filter.layer = layer;
        self.cell = None;
    }

    /// Filter to modules whose key contains `needle`. `None` clears it.
    pub fn filter_module(&mut self, needle: Option<String>) {
        self.filter.module = needle;
        self.cell = None;
    }

    /// Drop both filters, which is what `filter.clear` means for this view.
    pub fn clear_filter(&mut self) {
        self.filter = Filter::default();
        self.cell = None;
    }

    // --- the frame's question: what does the canvas hold? --------------------------------------

    /// Build the scene and the drawing for a canvas of `cols` × `rows`, honouring the current knobs.
    fn build(&self, hand: &Hand, cols: usize, rows: usize) -> (Scene, Drawing) {
        let layout = self.layout.fallback();
        let capacity = layout.capacity(cols, picture_rows(rows));
        let mut scene = Scene::build(hand, &self.knobs, &self.filter, capacity);
        let selected = self
            .cell
            .and_then(|index| scene.cells.get(index))
            .map(|cell| cell.seed().to_owned());
        scene.highlight(hand, selected.as_deref(), self.knobs.hops);
        let drawing = layout::draw(layout, &scene, hand, cols, picture_rows(rows), self.cell);
        (scene, drawing)
    }

    /// The scene and drawing, rebuilt only when something they depend on changed.
    fn frame(&self, cols: usize, rows: usize) -> Option<(Scene, Drawing)> {
        let hand = self.hand.as_ref()?;
        let key = (
            hand.revision.clone(),
            self.knobs,
            self.filter.clone(),
            self.layout,
            cols,
            rows,
            self.cell,
        );
        {
            let borrow = self.cache.borrow();
            if let Some(cache) = borrow.as_ref()
                && cache.key == key
                && cache.cols == cols
                && cache.rows == rows
            {
                return Some((cache.scene.clone(), cache.drawing.clone()));
            }
        }
        let (scene, drawing) = self.build(hand, cols, rows);
        *self.cache.borrow_mut() = Some(Cache {
            key,
            scene: scene.clone(),
            drawing: drawing.clone(),
            cols,
            rows,
        });
        Some((scene, drawing))
    }

    /// The last frame's canvas size, as recorded by the last draw. The detail pane is drawn at a different
    /// width from the canvas, so it asks for the *canvas's* scene rather than rebuilding one at its own.
    fn last_size(&self) -> (usize, usize) {
        self.cache
            .borrow()
            .as_ref()
            .map(|cache| (cache.cols, cache.rows))
            .unwrap_or((54, 14))
    }

    /// The scene the canvas would draw at this size, for a caller that has to *count* rather than look.
    ///
    /// The ADR's rule — degrade by aggregation, never by clipping — is an arithmetic claim about this
    /// structure (drawn + folded = handed), and an arithmetic claim cannot be checked from a screenshot.
    #[must_use]
    pub fn scene(&self, cols: usize, rows: usize) -> Option<Scene> {
        self.frame(cols, rows).map(|(scene, _)| scene)
    }

    /// The module key the cursor sits on, as the hand names it.
    #[must_use]
    pub fn selected_key(&self) -> Option<String> {
        let (cols, rows) = self.last_size();
        let (scene, _) = self.frame(cols, rows)?;
        self.cell
            .and_then(|index| scene.cells.get(index))
            .map(|cell| cell.seed().to_owned())
    }

    /// The selected cell index.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.cell
    }

    /// Put the cursor on a cell index, clamped by the caller. This is what `j`/`k`, a click and a
    /// `H`/`M`/`L` all funnel through, so the pointer cannot select something the keyboard cannot (`e3.4`).
    pub fn set_cell(&mut self, index: usize) {
        let count = self.cell_count();
        self.cell = if count == 0 {
            None
        } else {
            Some(index.min(count.saturating_sub(1)))
        };
        self.detail_row = 0;
    }

    /// How many cells the canvas can select at the last drawn size.
    #[must_use]
    pub fn cell_count(&self) -> usize {
        let (cols, rows) = self.last_size();
        self.frame(cols, rows)
            .map_or(0, |(scene, _)| scene.cells.len())
    }

    /// Focus the cell holding a module key — how `/`'s path hit and a drill land on a cell.
    pub fn focus(&mut self, key: &str) -> bool {
        let (cols, rows) = self.last_size();
        let at = self
            .frame(cols, rows)
            .and_then(|(scene, _)| scene.index_holding(key));
        match at {
            Some(at) => {
                self.cell = Some(at);
                self.detail_row = 0;
                true
            }
            None => false,
        }
    }

    /// The detail pane's cursor.
    #[must_use]
    pub const fn detail_row(&self) -> usize {
        self.detail_row
    }

    /// Move the detail pane's cursor.
    pub fn set_detail_row(&mut self, row: usize) {
        self.detail_row = row;
    }

    /// Resolve a canvas body row to a cell index — the mouse's only question, answered from the same list
    /// the keyboard walks (ADR-006 §10.1).
    #[must_use]
    pub fn hit(&self, row: usize) -> Option<usize> {
        let (cols, rows) = self.last_size();
        let (_, drawing) = self.frame(cols, rows)?;
        drawing.cell_at_row(row.saturating_sub(header_lines(rows)))
    }

    // --- keystrokes ----------------------------------------------------------------------------

    /// Apply one published action. Returns `true` when the action belonged to this view.
    pub fn dispatch(&mut self, action: &str) -> bool {
        match action {
            "graph.level_up" => {
                let depth = match self.knobs.level {
                    Level::Auto => 1,
                    Level::Depth(depth) => depth.saturating_sub(1).max(1),
                };
                if depth == self.knobs.level.depth() {
                    self.notice = Some(
                        "L1 is one cell per top-level directory — `]` goes back down".to_owned(),
                    );
                    return false;
                }
                self.knobs.level = Level::Depth(depth);
                self.notice = Some(format!("aggregated to L{depth} — `[` folds, `]` expands"));
                true
            }
            "graph.level_down" => {
                let depth = match self.knobs.level {
                    Level::Auto => 3,
                    Level::Depth(depth) => depth.saturating_add(1),
                };
                // Past the deepest path in the hand, `]` would be a key that moves a number and changes
                // nothing, so it refuses and says what it is already showing instead.
                let deepest = self.hand.as_ref().map_or(0, |hand| hand.path_depth());
                if depth > deepest {
                    self.notice = Some(format!(
                        "L{} is already one module per cell — the hand has no deeper path",
                        self.knobs.level.depth()
                    ));
                    return false;
                }
                self.knobs.level = Level::Depth(depth);
                self.notice = Some(format!(
                    "one module per cell out to L{depth} — the canvas still folds what it cannot draw"
                ));
                true
            }
            "graph.hop_less" => {
                if self.knobs.hops == 0 {
                    self.notice =
                        Some("hop 0 is the cell itself — `>` widens the blast radius".to_owned());
                    return false;
                }
                self.knobs.hops -= 1;
                self.notice = Some(format!(
                    "hop limit {} — `>` widens the blast radius",
                    self.knobs.hops
                ));
                true
            }
            "graph.hop_more" => {
                if self.knobs.hops >= MAX_HOPS {
                    self.notice = Some(format!(
                        "hop {MAX_HOPS} is as far as this canvas walks — the rest of the map is `[`/`]`"
                    ));
                    return false;
                }
                self.knobs.hops += 1;
                self.notice = Some(format!("hop limit {}", self.knobs.hops));
                true
            }
            "graph.overlay_only" => {
                self.knobs.only_touched = !self.knobs.only_touched;
                self.notice = Some(if self.knobs.only_touched {
                    "showing only what this episode touched, plus its hop neighbourhood — `!` again for \
                     the whole map"
                        .to_owned()
                } else {
                    "showing the whole map".to_owned()
                });
                true
            }
            "graph.layout_cycle" => {
                self.layout = self.layout.next();
                self.notice = Some(format!(
                    "{} — {}",
                    self.layout.name(),
                    self.layout.sentence()
                ));
                true
            }
            "graph.heat_cycle" => {
                self.knobs.heat = self.knobs.heat.next();
                self.notice = Some(
                    self.hand
                        .as_ref()
                        .map(|hand| hand.heat.series(self.knobs.heat).sentence(self.knobs.heat))
                        .unwrap_or_else(|| "no heat series handed".to_owned()),
                );
                true
            }
            "graph.filter_layer" => {
                let layer = self
                    .selected_key()
                    .as_ref()
                    .and_then(|key| self.hand.as_ref()?.cell(key)?.layer.clone());
                match layer {
                    Some(layer) => {
                        let same = self.filter.layer.as_deref() == Some(layer.as_str());
                        self.filter_layer(if same { None } else { Some(layer.clone()) });
                        self.notice = Some(if same {
                            "layer filter cleared".to_owned()
                        } else {
                            format!("filtered to layer `{layer}`")
                        });
                    }
                    None => {
                        self.filter_layer(None);
                        self.notice = Some(
                            "no layer to filter to — select a tagged cell, or `filter.clear`"
                                .to_owned(),
                        );
                    }
                }
                true
            }
            "graph.filter_clear" => {
                self.clear_filter();
                self.notice = Some("graph filter cleared".to_owned());
                true
            }
            "list.down" | "list.scroll_down" => {
                self.move_cell(1);
                true
            }
            "list.up" | "list.scroll_up" => {
                self.move_cell(-1);
                true
            }
            "list.page_down" | "list.half_down" => {
                self.move_cell(4);
                true
            }
            "list.page_up" | "list.half_up" => {
                self.move_cell(-4);
                true
            }
            "list.first" => {
                self.cell = Some(0);
                self.detail_row = 0;
                true
            }
            "list.last" => {
                let last = self.cell_count().saturating_sub(1);
                self.cell = Some(last);
                self.detail_row = 0;
                true
            }
            _ => false,
        }
    }

    /// Move the cell cursor. In a canvas `j`/`k` move the *selection*, not a window: the picture is exactly
    /// the size of the pane, so there is nothing to scroll and everything to select.
    fn move_cell(&mut self, step: i32) {
        let count = self.cell_count();
        if count == 0 {
            self.cell = None;
            return;
        }
        let here = self.cell.unwrap_or(0);
        let next = if step >= 0 {
            here.saturating_add(step as usize).min(count - 1)
        } else {
            here.saturating_sub(step.unsigned_abs() as usize)
        };
        self.cell = Some(next);
        self.detail_row = 0;
    }

    // --- the canvas ----------------------------------------------------------------------------

    /// The canvas: header, the delta's counts, the canvas's own counts, and as much picture as is left.
    #[must_use]
    pub fn canvas(&self, cols: usize, rows: usize) -> Vec<StyledLine> {
        let rows = rows.max(5);
        let Some(hand) = self.hand.as_ref() else {
            return prose(&[&self.empty_statement()], cols, rows);
        };
        let Some((scene, _)) = self.frame(cols, rows) else {
            return prose(&[&self.empty_statement()], cols, rows);
        };
        let level = format!(
            "L{}{}",
            scene.totals.depth,
            if scene.totals.auto { " (auto)" } else { "" }
        );
        let mut out: Vec<StyledLine> = Vec::with_capacity(rows);
        out.push(StyledLine::with_face(
            model::clip(&self.header(&level), cols),
            Face::Meta,
        ));
        let facts = self.facts(hand, &scene, cols);
        let wanted = header_lines(rows);
        for line in facts.iter().take(wanted) {
            out.push(line.clone());
        }
        while out.len() < wanted {
            out.push(StyledLine::plain(""));
        }
        let picture = layout::draw(
            self.layout.fallback(),
            &scene,
            hand,
            cols,
            picture_rows(rows),
            self.cell
                .and_then(|index| scene.cells.get(index).map(|_| index)),
        );
        out.extend(picture.lines);
        out.push(StyledLine::with_face(
            model::clip(&scene.counts_line(), cols),
            Face::Muted,
        ));
        while out.len() < rows {
            out.push(StyledLine::plain(""));
        }
        out.truncate(rows);
        out
    }

    /// The canvas header, in the ADR's own shape:
    /// ``layered · hop 2 · delta overlay ON `!` · L2 `[`/`]``.
    fn header(&self, level: &str) -> String {
        format!(
            "{} · hop {} · delta overlay {} `!` · {level} `[`/`]`",
            self.layout.fallback().name(),
            self.knobs.hops,
            if self.knobs.only_touched { "ON" } else { "off" },
        )
    }

    /// The lines the canvas always carries: what the episode did, what the heat is, and what the canvas is
    /// showing.
    ///
    /// Three lines rather than one long one, because a clipped sentence loses the fact at its end — and the
    /// fact at the end is the one that says whether a number was measured at all (ADR-006 §6.1: a figure
    /// without its basis is a figure that will be misread).
    fn facts(&self, hand: &Hand, scene: &Scene, cols: usize) -> Vec<StyledLine> {
        let mut out = vec![StyledLine::with_face(
            model::clip(
                &format!("{} · rung {}", hand.delta.counts_line(), hand.delta.rung),
                cols,
            ),
            Face::Meta,
        )];
        // The heat's own basis, and the reason when there is no heat: an unmeasured series must not read as
        // a cold one.
        out.push(StyledLine::with_face(
            model::clip(
                &hand.heat.series(self.knobs.heat).sentence(self.knobs.heat),
                cols,
            ),
            Face::Meta,
        ));
        let mut second: Vec<String> = Vec::new();
        if scene.totals.ops_undrawn > 0 {
            second.push(format!("{} delta rows undrawn", scene.totals.ops_undrawn));
        }
        if let Some(truncated) = hand.truncated.clone() {
            second.push(truncated.sentence());
        }
        if let Some(notice) = scene.notices.first() {
            second.push(notice.clone());
        }
        for notice in hand.gaps.iter().take(1) {
            second.push(notice.clone());
        }
        // Wrapped rather than clipped: the sentence that says the map is a *subset* is the one sentence a
        // reader must not miss, and it is the longest of them.
        for line in wrap(&second.join(" · "), cols) {
            out.push(StyledLine::with_face(line, Face::Warning));
        }
        out
    }

    /// The canvas footer strip.
    #[must_use]
    pub fn canvas_strip(&self, cols: usize) -> String {
        let Some(hand) = self.hand.as_ref() else {
            return "no graph handed to this pane · `fathom index` writes the lane it draws from"
                .to_owned();
        };
        let (cols_seen, rows_seen) = self.last_size();
        let (scene, drawing) = self
            .frame(cols_seen, rows_seen)
            .unwrap_or_else(|| (Scene::empty(), Drawing::empty()));
        let layout = self.layout.fallback();
        let basis = drawing
            .basis
            .clone()
            .unwrap_or_else(|| layout.sentence().to_owned());
        // The ledger's dates ride at the *front* of the strip, because the strip is clipped at the end and
        // the strip is the one line of this shell that is true whether or not a cell is selected: a row about
        // to lapse is a finding, and a finding that gets clipped off an 80-column terminal is a finding
        // nobody sees (`e3.10`, `reco-5ht`).
        let debt = if hand.debt.speaks() {
            format!("{} · ", hand.debt.strip())
        } else {
            String::new()
        };
        model::clip(
            &format!(
                "{debt}{} · {} · {} · Δ {} rows · {}",
                layout.sentence(),
                basis,
                self.filter.clause(),
                hand.delta.ops.len(),
                if scene.totals.auto {
                    "aggregation is the canvas's (`]`)"
                } else {
                    "aggregation is yours (`[`/`]`)"
                }
            ),
            cols.max(12),
        )
    }

    // --- the detail pane -----------------------------------------------------------------------

    /// What the detail pane draws: the cell's facts, its arrows with their citations, and the citation of
    /// whichever arrow the cursor is on. With nothing selected it draws the whole map's facts, which is what
    /// a reviewer reads first.
    #[must_use]
    pub fn detail(&self, cols: usize, rows: usize) -> Vec<StyledLine> {
        let Some(hand) = self.hand.as_ref() else {
            return prose(&[&self.empty_statement()], cols, rows);
        };
        let (canvas_cols, canvas_rows) = self.last_size();
        let Some((scene, _)) = self.frame(canvas_cols, canvas_rows) else {
            return prose(&[&self.empty_statement()], cols, rows);
        };
        let cell = self
            .cell
            .and_then(|index| scene.cells.get(index))
            .filter(|cell| !cell.key.is_empty());
        let Some(cell) = cell else {
            return self.summary(hand, &scene, cols, rows);
        };
        let mut out: Vec<StyledLine> = Vec::new();
        let push = |out: &mut Vec<StyledLine>, text: String, face: Face| {
            out.push(StyledLine::with_face(model::clip(&text, cols), face));
        };
        push(&mut out, format!("{} · selected", cell.key), Face::Emphasis);
        let mut kind = format!(
            "{} · {}",
            cell.kind.as_str(),
            if cell.total > 1 {
                format!("{} modules in this cell", cell.total)
            } else {
                "one module".to_owned()
            }
        );
        if cell.ghost {
            kind.push_str(" · gone this episode");
        }
        push(&mut out, kind, Face::Muted);
        push(
            &mut out,
            match (&cell.layer, cell.mixed_layer) {
                (Some(layer), false) => format!("layer    {layer}"),
                (_, true) => "layer    mixed — this episode moved one of these".to_owned(),
                (None, false) => {
                    "layer    untagged — `[layers]` does not name this path".to_owned()
                }
            },
            Face::Muted,
        );
        let series = hand.heat.series(self.knobs.heat);
        push(
            &mut out,
            match cell.heat {
                Some(value) => format!(
                    "heat     {:.2} {} · {}",
                    value,
                    cell.band.dots(),
                    series.sentence(self.knobs.heat)
                ),
                None => series.sentence(self.knobs.heat),
            },
            Face::Muted,
        );
        if cell.may_have_shrunk {
            push(
                &mut out,
                "surface   may have shrunk, not shrunk (e3.7)".to_owned(),
                Face::Warning,
            );
        }
        let fan = hand.fan(cell.seed());
        let rows_of = hand.delta.touching(cell.seed());
        let added = rows_of.iter().filter(|op| op.mark == Mark::Added).count();
        let removed = rows_of.iter().filter(|op| op.mark == Mark::Removed).count();
        for line in fan.side(removed, added) {
            push(&mut out, line, Face::Muted);
        }
        if !hand.delta.cycles.is_empty() || hand.delta.cycle_debt > 0 {
            push(
                &mut out,
                format!(
                    "cycles   {} new · debt {}",
                    hand.delta.cycles.len(),
                    hand.delta.cycle_debt
                ),
                Face::Muted,
            );
            for cycle in hand.delta.cycles.iter().take(2) {
                push(
                    &mut out,
                    format!("         {}", cycle.join(" → ")),
                    Face::Warning,
                );
            }
        }
        if hand.delta.violations > 0 {
            push(
                &mut out,
                format!(
                    "layering {} new violation(s) this episode",
                    hand.delta.violations
                ),
                Face::Warning,
            );
        }
        // The allowance ledger's dates, in the engine's words (`e3.10`, `reco-5ht`). A baselined vertex is
        // quiet because someone wrote it down, and the row that wrote it down has an expiry on it.
        for line in hand.debt.lines() {
            push(&mut out, line, Face::Warning);
        }
        let ops: Vec<&Op> = hand
            .delta
            .ops
            .iter()
            .filter(|op| match &op.subject {
                Subject::Edge { src, dst, .. } => cell.holds(src) || cell.holds(dst),
                Subject::Node { key } => cell.holds(key),
            })
            .collect();
        if !ops.is_empty() {
            push(
                &mut out,
                format!(
                    "this episode · {} row(s) · rung {}",
                    ops.len(),
                    hand.delta.rung
                ),
                Face::Meta,
            );
            for op in ops.iter().take(3) {
                let subject = match &op.subject {
                    Subject::Edge { src, dst, .. } => format!("{src} → {dst}"),
                    Subject::Node { key } => key.clone(),
                };
                // The delta's reason goes *before* the subject, because the subject is the long part and a
                // clipped line would lose the reason entirely: "removed, and here is why" has to survive
                // a 54-column pane (ADR-006 §6.1).
                let why = op
                    .why
                    .as_deref()
                    .map(|why| format!(" ({why})"))
                    .unwrap_or_default();
                push(
                    &mut out,
                    format!(
                        "  {} {}{} {} — {}",
                        op.mark.glyph(),
                        op.mark.as_str(),
                        why,
                        subject,
                        op.finding
                    ),
                    Face::Meta,
                );
            }
        }
        let (out_links, in_links) = scene.touching(cell.seed());
        let links: Vec<&Link> = out_links.iter().chain(in_links.iter()).copied().collect();
        if links.is_empty() {
            push(
                &mut out,
                if scene.totals.links_drawn == 0 {
                    "no arrows between the cells this canvas draws".to_owned()
                } else {
                    "no arrows on this cell — its dependencies are inside another cell".to_owned()
                },
                Face::Muted,
            );
        } else {
            push(
                &mut out,
                format!(
                    "arrows {} ({} out · {} in)",
                    links.len(),
                    out_links.len(),
                    in_links.len()
                ),
                Face::Meta,
            );
            for (row, link) in links.iter().enumerate() {
                let arrow = if link.src == cell.key || link.src == cell.seed() {
                    format!("→ {}", link.dst)
                } else {
                    format!("← {}", link.src)
                };
                let mark = link.mark.map_or_else(String::new, |mark| {
                    format!(" {} {}", mark.glyph(), mark.as_str())
                });
                let face = match link.mark {
                    Some(Mark::Added) => Face::Added,
                    Some(Mark::Removed) => Face::Removed,
                    Some(Mark::Changed) => Face::Warning,
                    None => Face::Text,
                };
                let cursor = if row == self.detail_row { '▸' } else { ' ' };
                push(
                    &mut out,
                    format!(
                        "{cursor}{arrow}{mark} · {} · {} rows",
                        link.kind_words(),
                        link.rows
                    ),
                    face,
                );
            }
            // The criterion: the edge under the cursor says which file and line produced it. The hand's own
            // citation wins when it carried one (a delta row names its statement at the moment it is
            // computed); otherwise the provider is asked for this one arrow, which is the only arrow the
            // reviewer is reading.
            let here = links
                .get(self.detail_row)
                .and_then(|link| link.evidence.clone())
                .or_else(|| {
                    let link = links.get(self.detail_row).or_else(|| links.first())?;
                    self.citations
                        .as_ref()
                        .and_then(|source| source.0.cite(link.src.as_str(), link.dst.as_str()))
                })
                .or_else(|| links.first().and_then(|link| link.evidence.clone()));
            match here {
                Some(evidence) => {
                    push(&mut out, evidence.line_of(), Face::Path);
                    if let Some(run) = evidence.run.clone() {
                        push(&mut out, run, Face::Muted);
                    }
                }
                None => push(
                    &mut out,
                    "evidence ? — this arrow merges rows that carried no file, so no single citation \
                     covers it"
                        .to_owned(),
                    Face::Warning,
                ),
            }
        }
        for notice in scene.notices.iter().take(2) {
            push(&mut out, notice.clone(), Face::Warning);
        }
        while out.len() < rows {
            out.push(StyledLine::plain(""));
        }
        out.truncate(rows);
        out
    }

    /// The detail pane with nothing selected: the whole map's facts.
    fn summary(&self, hand: &Hand, scene: &Scene, cols: usize, rows: usize) -> Vec<StyledLine> {
        let mut out = vec![
            StyledLine::with_face(model::clip(&self.handed(), cols), Face::Emphasis),
            StyledLine::with_face(
                model::clip(
                    &format!(
                        "delta {} → {} · rung {}",
                        if hand.delta.from.is_empty() {
                            "previous"
                        } else {
                            &hand.delta.from
                        },
                        if hand.delta.to.is_empty() {
                            "this revision"
                        } else {
                            &hand.delta.to
                        },
                        hand.delta.rung
                    ),
                    cols,
                ),
                Face::Meta,
            ),
            StyledLine::with_face(model::clip(&hand.delta.counts_line(), cols), Face::Meta),
            StyledLine::with_face(
                model::clip(
                    &hand.heat.series(self.knobs.heat).sentence(self.knobs.heat),
                    cols,
                ),
                Face::Muted,
            ),
            StyledLine::with_face(model::clip(&scene.counts_line(), cols), Face::Muted),
        ];
        if !hand.layers.is_empty() {
            out.push(StyledLine::with_face(
                model::clip(&format!("layers  {}", hand.layers.join(" ")), cols),
                Face::Muted,
            ));
        }
        if hand.heat.churn.known() && hand.heat.complexity.known() {
            out.push(StyledLine::with_face(
                "heat basis `H` — both series handed",
                Face::Muted,
            ));
        }
        for notice in scene.notices.iter().take(3) {
            out.push(StyledLine::with_face(
                model::clip(notice, cols),
                Face::Warning,
            ));
        }
        for notice in hand.delta.notices.iter().take(2) {
            out.push(StyledLine::with_face(
                model::clip(notice, cols),
                Face::Warning,
            ));
        }
        for gap in hand.gaps.iter().take(2) {
            out.push(StyledLine::with_face(model::clip(gap, cols), Face::Warning));
        }
        // The whole-map pane is where the ledger's dates belong as well as the per-cell pane: a row that is
        // running out is a fact about the repository, not about the cell under the cursor (`e3.10`,
        // `reco-5ht`). Same lines, same wording, one fewer thing for a reviewer to have to know to look for.
        for line in hand.debt.lines() {
            out.push(StyledLine::with_face(
                model::clip(&line, cols),
                Face::Warning,
            ));
        }
        if hand.delta.violations > 0 {
            out.push(StyledLine::with_face(
                format!(
                    "layering +{} new violation(s) · debt {}",
                    hand.delta.violations, hand.delta.cycle_debt
                ),
                Face::Warning,
            ));
        }
        out.push(StyledLine::with_face(
            "j/k pick a cell · Enter reads its arrows · Tab to this pane",
            Face::Muted,
        ));
        while out.len() < rows {
            out.push(StyledLine::plain(""));
        }
        out.truncate(rows);
        out
    }

    /// How many rows the detail pane has, which is what its cursor is clamped to.
    #[must_use]
    pub fn detail_rows(&self) -> usize {
        if self.hand.is_none() {
            return 1;
        }
        let (canvas_cols, canvas_rows) = self.last_size();
        if self.frame(canvas_cols, canvas_rows).is_none() {
            return 1;
        }
        self.detail(canvas_cols, 64)
            .iter()
            .filter(|line| !line.text().is_empty())
            .count()
            .max(1)
    }

    /// The canvas as plain text, for `--plain` and for the golden files.
    #[must_use]
    pub fn canvas_text(&self, cols: usize, rows: usize) -> Vec<String> {
        self.canvas(cols, rows)
            .iter()
            .map(|line| line.text().to_owned())
            .collect()
    }
}

/// How many lines the canvas spends on its own chrome above the picture: the header, and the two fact
/// lines it always has room for at a normal size. Fixed arithmetic, because `hit()` has to agree with
/// `canvas()` about which row the picture starts on or a click selects the wrong module.
#[must_use]
fn header_lines(rows: usize) -> usize {
    if rows >= 16 {
        5
    } else if rows >= 8 {
        3
    } else {
        2
    }
}

/// Break a sentence into pane-width lines on its ` · ` seams, so a fact is never lost to a clip.
///
/// A width is a hard edge and a sentence is not: the alternative is a strip that ends mid-fact, which is how
/// "the hand carries 40 of 900…" turns into "the hand is a subset" with the numbers gone.
fn wrap(text: &str, width: usize) -> Vec<String> {
    // Atoms and the glue between them: ` · ` is a seam worth breaking on, a space is the fallback when one
    // clause alone is wider than the pane.
    let mut atoms: Vec<(String, &str)> = Vec::new();
    let parts: Vec<&str> = text.split(" · ").collect();
    for (index, part) in parts.iter().enumerate() {
        let seam = if index + 1 < parts.len() { " · " } else { "" };
        if crate::text::width(part) <= width {
            atoms.push(((*part).to_owned(), seam));
            continue;
        }
        let words: Vec<&str> = part.split(' ').collect();
        for (at, word) in words.iter().enumerate() {
            let glue = if at + 1 == words.len() { seam } else { " " };
            atoms.push(((*word).to_owned(), glue));
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for (atom, seam) in atoms {
        if !line.is_empty()
            && crate::text::width(&line) + crate::text::width(&atom) + crate::text::width(seam)
                > width
        {
            out.push(std::mem::take(&mut line));
        } else if !line.is_empty() {
            line.push_str(seam);
        }
        line.push_str(&atom);
    }
    if !line.is_empty() {
        out.push(line);
    }
    // An atom longer than the pane on its own is still clipped, and `clip` keeps the ellipsis honest.
    out.iter_mut().for_each(|line| {
        *line = model::clip(line, width);
    });
    out
}

/// How many rows the picture gets at a canvas of `rows`.
#[must_use]
fn picture_rows(rows: usize) -> usize {
    rows.saturating_sub(header_lines(rows) + 1).max(1)
}

/// A plain statement filling a pane, used when there is nothing to draw. The pane is never blank.
fn prose(lines: &[&str], cols: usize, rows: usize) -> Vec<StyledLine> {
    let mut out: Vec<StyledLine> = lines
        .iter()
        .map(|line| StyledLine::with_face(model::clip(line, cols), Face::Muted))
        .collect();
    while out.len() < rows {
        out.push(StyledLine::plain(""));
    }
    out.truncate(rows.max(1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::plate::Plate;

    #[test]
    fn a_plate_never_prints_an_arrow_through_a_label() {
        let mut plate = Plate::new(10, 3);
        plate.write(2, 1, "label", Face::Text);
        assert!(!plate.run(0, 9, 1, Face::Added, '─'));
        assert!(plate.run(0, 9, 0, Face::Added, '─'));
    }
}

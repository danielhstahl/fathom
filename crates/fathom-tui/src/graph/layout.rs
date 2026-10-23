//! `fathom-tui::graph::layout` — where the boxes go, and how an arrow gets from one to the other.
//!
//! ADR-006 §5.5 promises two coordinated maps on one canvas: the **dependency map**, in a layered or a
//! circular layout, and the **structure map**, a hierarchical treemap of the same module tree with heat on
//! top. They are three [`Layout`]s over one [`Scene`], which is what *coordinated* means in practice: the
//! same aggregation level, the same filter, the same delta marks, the same selection. `L` cycles them; the
//! canvas never goes blank between two of them, and no two of them show a different set of cells.
//!
//! ```text
//! Layered   ┌ fathom-cli ┐      ┌ fathom-analyze ┐    columns are `[layers]`, or longest-path
//!           │  main.rs   │─────▶│ insight        │    depth when the index declares no layers
//!           └─────┬──────┘      └───────┬────────┘
//!                 │  ╳                    │
//!           ┌─────▼──────┐      ┌────────▼───────┐
//!           │fathom-store│╌╌╌╌╌╌│  fathom-core   │    a ghosted run is a removed edge
//!           └────────────┘  ⌀                   │
//! ```
//!
//! Three rules:
//!
//! * **Everything is drawn or counted.** Every layout reports the cells and arrows it could not place at
//!   this size and the canvas prints the numbers. Nothing here drops a cell in path order and says nothing,
//!   which is the difference between degrading by aggregation and clipping.
//! * **A glyph carries the fact, the colour decorates it** (ADR-006 §10.2). A removed edge is a *dashed*
//!   run, a changed node is a *double* rule, and both are marked `⌀`/`✱` as well; a terminal that prints no
//!   colour still reads.
//! * **`graph-layout` is a real feature, not a decoration.** ADR-002 §6.2 makes it non-default, and the
//!   layered layout is where its `petgraph` build earns its keep: folding a cycle into one rung is
//!   `tarjan_scc`, and a cycle drawn as a layering loop is a layering with no answer. With the feature
//!   compiled out [`Layout::fallback`] draws the aggregated node table and names the feature, which is the
//!   promise ADR-006 §5.5 makes about that pane — the canvas is never blank.

use std::collections::{BTreeMap, HashMap};

use crate::style::{Face, StyledLine};

use super::hand::{self, Band, Mark};
use super::model::{self, Cell, Scene};
use super::plate::Plate;

/// The gap between two columns of boxes, in cells. One cell would make every arrow touch a box, and a
/// touching arrow reads as a box border.
const LANE: usize = 2;
/// The rows a layered box costs: rule, label, rule, gap.
const BOX_ROWS: usize = 4;
/// The narrowest box worth a label. Below this the layout has more columns than the canvas has words.
const BOX_MIN_COLS: usize = 6;
/// The smallest treemap tile worth a box.
const TILE_MIN_COLS: usize = 5;
/// The shortest treemap tile worth a box.
const TILE_MIN_ROWS: usize = 2;

/// Which map the canvas is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layout {
    /// Columns left-to-right in dependency order; the map a reviewer reads for direction.
    Layered,
    /// One ring; the map a reviewer reads for clusters and for what sits in the middle.
    Circular,
    /// The hierarchical treemap — the structure map, with heat as the fill.
    Treemap,
    /// The aggregated node table: what the canvas draws when the map layouts are not compiled in.
    Table,
}

impl Layout {
    /// The layouts this binary can draw, in `L`'s cycle order.
    ///
    /// With `graph-layout` compiled in the cycle is the ADR's own — layered, then circular, then the
    /// structure map. Without it the table is the first stop, and the header names the feature that would
    /// buy the rest.
    #[must_use]
    pub const fn cycle() -> &'static [Layout] {
        if cfg!(feature = "graph-layout") {
            &[Layout::Layered, Layout::Circular, Layout::Treemap]
        } else {
            &[Layout::Table, Layout::Treemap]
        }
    }

    /// The layout to open on.
    #[must_use]
    pub const fn preferred() -> Layout {
        if cfg!(feature = "graph-layout") {
            Layout::Layered
        } else {
            Layout::Table
        }
    }

    /// The next layout in the cycle, which is what `L` does.
    #[must_use]
    pub fn next(self) -> Layout {
        let cycle = Self::cycle();
        let at = cycle.iter().position(|other| *other == self).unwrap_or(0);
        cycle[(at + 1) % cycle.len()]
    }

    /// The registry spelling, and what the header prints.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Layout::Layered => "layered",
            Layout::Circular => "circular",
            Layout::Treemap => "treemap",
            Layout::Table => "table",
        }
    }

    /// The header's clause about the layout, including the refusal when the feature is out.
    #[must_use]
    pub const fn sentence(self) -> &'static str {
        match self {
            Layout::Layered => "columns are layers",
            Layout::Circular => "clusters read as arcs",
            Layout::Treemap => "structure map: treemap, heat as fill",
            Layout::Table => "aggregated node table — `graph-layout` is not compiled in",
        }
    }

    /// Is this layout available in this binary?
    #[must_use]
    pub const fn available(self) -> bool {
        match self {
            Layout::Layered | Layout::Circular => cfg!(feature = "graph-layout"),
            Layout::Treemap | Layout::Table => true,
        }
    }

    /// The layout to use when this one is not compiled in.
    #[must_use]
    pub const fn fallback(self) -> Layout {
        if self.available() {
            self
        } else {
            Layout::Table
        }
    }

    /// How many cells this canvas can hold at `cols` × `rows`.
    ///
    /// This is the *zoom threshold* the acceptance line is about: past this many cells the view aggregates
    /// rather than clips, so what a keystroke costs is bounded by the size of the terminal instead of by the
    /// size of the repository.
    #[must_use]
    pub fn capacity(self, cols: usize, rows: usize) -> usize {
        match self {
            Layout::Layered => {
                let columns = if cols >= BOX_MIN_COLS + LANE {
                    (cols + LANE) / (BOX_MIN_COLS + LANE)
                } else {
                    1
                };
                columns * (rows / BOX_ROWS).max(1)
            }
            // A ring holds as many labels as its circumference can keep apart: a dozen at 54×16, before the
            // labels start touching and the picture becomes texture.
            Layout::Circular => {
                let room = cols.saturating_mul(rows);
                if room >= 720 {
                    12
                } else if room >= 320 {
                    8
                } else {
                    5
                }
            }
            Layout::Treemap => {
                let across = if cols >= TILE_MIN_COLS {
                    cols / TILE_MIN_COLS
                } else {
                    1
                };
                let down = if rows >= TILE_MIN_ROWS {
                    (rows + 1) / TILE_MIN_ROWS
                } else {
                    1
                };
                across * down
            }
            // The table costs one row per group and one per cell, so it holds what the pane has room for.
            Layout::Table => rows.saturating_mul(2).max(1),
        }
    }
}

/// What a layout produced: the picture, and the row ranges each cell occupies for hit-testing.
#[derive(Debug, Clone)]
pub struct Drawing {
    /// The finished rows, exactly `cols` cells wide.
    pub lines: Vec<StyledLine>,
    /// The body rows each drawn cell occupies, as `(cell index, first row, last row)`.
    pub rows_of: Vec<(usize, usize, usize)>,
    /// Cells the layout could not place at this size.
    pub cells_undrawn: usize,
    /// Arrows the layout could not route at this size.
    pub arrows_undrawn: usize,
    /// Arrows it did route.
    pub arrows_drawn: usize,
    /// A statement the layout wants on the canvas — too-small tiles, an unavailable feature.
    pub notice: Option<String>,
    /// How the columns (or ring order) were chosen, for the header.
    pub basis: Option<String>,
}

impl Drawing {
    /// The drawing of nothing at all, for a pane with no graph handed.
    #[must_use]
    pub fn empty() -> Drawing {
        Drawing {
            lines: Vec::new(),
            rows_of: Vec::new(),
            cells_undrawn: 0,
            arrows_undrawn: 0,
            arrows_drawn: 0,
            notice: None,
            basis: None,
        }
    }

    /// Which drawn cell occupies this body row, if any. This is the mouse's whole question, and the
    /// keyboard's cursor moves through the same list, which is what makes the pointer additive (ADR-006
    /// §10.1).
    #[must_use]
    pub fn cell_at_row(&self, row: usize) -> Option<usize> {
        self.rows_of
            .iter()
            .find(|(_, low, high)| *low <= row && row <= *high)
            .map(|(index, _, _)| *index)
    }
}

/// Draw one scene with one layout into a `cols` × `rows` picture.
#[must_use]
pub fn draw(
    layout: Layout,
    scene: &Scene,
    source: &hand::Hand,
    cols: usize,
    rows: usize,
    selection: Option<usize>,
) -> Drawing {
    let mut plate = Plate::new(cols, rows);
    let (rows_of, cells_undrawn, notice, basis) = match layout.fallback() {
        Layout::Layered => layered(&mut plate, scene, cols, rows, selection),
        Layout::Circular => circular(&mut plate, scene, cols, rows, selection),
        Layout::Treemap => treemap(&mut plate, scene, cols, rows, selection),
        Layout::Table => table(&mut plate, scene, source, cols, rows, selection),
    };
    Drawing {
        lines: plate.lines(),
        rows_of,
        cells_undrawn,
        arrows_undrawn: plate.undrawn,
        arrows_drawn: plate.drawn,
        notice,
        basis,
    }
}

// --- the layered map ---------------------------------------------------------------------------

/// One placed box, in plate coordinates.
struct Box0 {
    key: String,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Box0 {
    const fn mid_y(&self) -> usize {
        self.y + self.h / 2
    }
}

/// The map a reviewer reads for direction: columns left-to-right in dependency order, boxes stacked in
/// each, arrows routed through the lanes between them.
/// What a layout engine reports: one `(col, row, height)` slot per cell index, how many cells did not fit,
/// and the two basis sentences — *why* these columns, *why* this order (ADR-006 §5.5: a picture whose
/// grouping rule is not on screen is a picture nobody can check).
type Place = (
    Vec<(usize, usize, usize)>,
    usize,
    Option<String>,
    Option<String>,
);

fn layered(
    plate: &mut Plate,
    scene: &Scene,
    cols: usize,
    rows: usize,
    selection: Option<usize>,
) -> Place {
    if scene.cells.is_empty() {
        return (Vec::new(), 0, None, None);
    }
    let (columns, basis) = columns_of(scene);
    let mut by_column: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..scene.cells.len() {
        by_column
            .entry(columns.get(index).copied().unwrap_or(0))
            .or_default()
            .push(index);
    }
    // Order within a column by the mean position of the cell's drawn neighbours, which keeps arrows short;
    // the key is the tie-break and the stable answer when there are no neighbours.
    for list in by_column.values_mut() {
        list.sort_by(|a, b| {
            order_key(&scene.cells[*a], scene)
                .partial_cmp(&order_key(&scene.cells[*b], scene))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| scene.cells[*a].key.cmp(&scene.cells[*b].key))
        });
    }
    let ncols = by_column.keys().last().map_or(1, |last| last + 1);
    let per_column = (rows / BOX_ROWS).max(1);
    let max_columns = if cols >= BOX_MIN_COLS + LANE {
        (cols + LANE) / (BOX_MIN_COLS + LANE)
    } else {
        1
    };
    let used_columns = ncols.min(max_columns).max(1);
    let box_w = ((cols + LANE) / used_columns).saturating_sub(LANE).max(3);
    let mut placed: Vec<Box0> = Vec::with_capacity(scene.cells.len());
    let mut rows_of: Vec<(usize, usize, usize)> = Vec::new();
    let mut undrawn = 0usize;
    let mut x = 0usize;
    for column in 0..ncols {
        let Some(list) = by_column.get(&column) else {
            x += box_w + LANE;
            continue;
        };
        if column >= used_columns {
            // Columns past the canvas's width are not drawn and are counted; the level above fits.
            undrawn += list.len();
            x += box_w + LANE;
            continue;
        }
        for (slot, index) in list.iter().enumerate() {
            if slot >= per_column {
                undrawn += 1;
                continue;
            }
            let cell = &scene.cells[*index];
            let y = (slot * BOX_ROWS).min(rows.saturating_sub(3));
            draw_box(plate, cell, x, y, box_w, Some(*index) == selection);
            rows_of.push((*index, y, y + 2));
            placed.push(Box0 {
                key: cell.key.clone(),
                x,
                y,
                w: box_w,
                h: 3.min(rows.saturating_sub(y)),
            });
        }
        x += box_w + LANE;
    }
    let index_of: BTreeMap<&str, &Box0> = placed.iter().map(|b| (b.key.as_str(), b)).collect();
    route(plate, scene, &index_of);
    let notice = if undrawn > 0 {
        Some(format!(
            "{undrawn} cells have no box at this size — `[` folds one level up"
        ))
    } else {
        None
    };
    (rows_of, undrawn, notice, Some(basis))
}

/// Which column each cell sits in, and the statement about which rule produced it.
///
/// Layers first: when `[layers]` declares an order, the column *is* the layer, because that is the
/// architecture the repository says it has and a layout algorithm does not get to overrule it. When it does
/// not, the column is the longest-path depth with cycles folded — folded by [`petgraph::algo::tarjan_scc`]
/// with `graph-layout`, and by a bounded relaxation without it, which agrees on an acyclic graph and is a
/// stated weaker answer on a cyclic one.
fn columns_of(scene: &Scene) -> (Vec<usize>, String) {
    let cells = &scene.cells;
    let mut layers: Vec<String> = cells.iter().filter_map(|cell| cell.layer.clone()).collect();
    layers.sort();
    layers.dedup();
    if !layers.is_empty() {
        let out: Vec<usize> = cells
            .iter()
            .map(|cell| {
                cell.layer.as_ref().map_or(layers.len(), |layer| {
                    layers
                        .iter()
                        .position(|name| name == layer)
                        .unwrap_or(layers.len())
                })
            })
            .collect();
        return (out, "columns are `[layers]` tags".to_owned());
    }
    #[cfg(feature = "graph-layout")]
    {
        let (out, folded) = longest_path_condensed(scene);
        let basis = if folded > 0 {
            format!("columns are longest-path depth; {folded} cycle members share a rung")
        } else {
            "columns are longest-path depth".to_owned()
        };
        (out, basis)
    }
    #[cfg(not(feature = "graph-layout"))]
    {
        longest_path_relaxed(scene)
    }
}

/// Longest-path depth by bounded relaxation, which is the answer without `petgraph`: correct on an acyclic
/// set, and on a cyclic one it settles at the rung the relaxation stops moving at, which the statement says
/// out loud rather than leaving the reader to spot the spiral.
/// Longest-path depth by bounded relaxation, which is the answer without `graph-layout`: correct on an acyclic
/// set, and on a cyclic one it settles at the rung the relaxation stops moving at, which the statement says
/// out loud rather than leaving the reader to spot the spiral.
#[cfg(not(feature = "graph-layout"))]
fn longest_path_relaxed(scene: &Scene) -> (Vec<usize>, String) {
    let cells = &scene.cells;
    let mut depth: BTreeMap<&str, usize> = cells
        .iter()
        .map(|cell| (cell.key.as_str(), 0usize))
        .collect();
    for _ in 0..cells.len().clamp(1, 64) {
        let mut moved = false;
        for link in &scene.links {
            let Some(here) = depth.get(link.src.as_str()).copied() else {
                continue;
            };
            let want = here + 1;
            if let Some(there) = depth.get_mut(link.dst.as_str())
                && *there < want
            {
                *there = want;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let out: Vec<usize> = cells
        .iter()
        .map(|cell| depth.get(cell.key.as_str()).copied().unwrap_or(0))
        .collect();
    (
        out,
        "columns are longest-path depth; cycles are not folded without `graph-layout`".to_owned(),
    )
}

/// Longest-path depth over the *condensation*, which is what `graph-layout` buys: a strongly-connected
/// block becomes one rung, so a cycle reads as one box in one column instead of a spiral of arrows.
#[cfg(feature = "graph-layout")]
fn longest_path_condensed(scene: &Scene) -> (Vec<usize>, usize) {
    use petgraph::algo::tarjan_scc;
    use petgraph::graph::DiGraph;

    let cells = &scene.cells;
    let mut graph = DiGraph::<&str, ()>::new();
    let mut node_of: HashMap<&str, petgraph::graph::NodeIndex> = HashMap::new();
    for cell in cells {
        node_of.insert(cell.key.as_str(), graph.add_node(cell.key.as_str()));
    }
    for link in &scene.links {
        if let (Some(a), Some(b)) = (
            node_of.get(link.src.as_str()),
            node_of.get(link.dst.as_str()),
        ) && a != b
        {
            graph.add_edge(*a, *b, ());
        }
    }
    let components = tarjan_scc(&graph);
    // Component id per cell index, then a longest-path pass over the component DAG.
    let mut component: Vec<usize> = vec![0usize; cells.len()];
    let mut folded = 0usize;
    for (id, members) in components.iter().enumerate() {
        if members.len() > 1 {
            folded += members.len();
        }
        for member in members {
            if let Some(cell) = cells.get(member.index()) {
                let _ = cell;
                component[member.index()] = id;
            }
        }
    }
    let count = components.len().max(1);
    let mut depth: Vec<usize> = vec![0usize; count];
    for _ in 0..count {
        let mut moved = false;
        for link in &scene.links {
            let (Some(a), Some(b)) = (
                node_of.get(link.src.as_str()),
                node_of.get(link.dst.as_str()),
            ) else {
                continue;
            };
            let (from, to) = (component[a.index()], component[b.index()]);
            if from != to && depth[to] < depth[from] + 1 {
                depth[to] = depth[from] + 1;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    (
        component
            .iter()
            .map(|id| depth.get(*id).copied().unwrap_or(0))
            .collect(),
        folded,
    )
}

/// A cell's vertical ordering hint: the mean scene position of its drawn neighbours. `None` sorts last, so
/// a cell with no drawn neighbours cannot push the ones that do out of the way.
fn order_key(cell: &Cell, scene: &Scene) -> Option<f64> {
    let mut total = 0f64;
    let mut seen = 0usize;
    for link in &scene.links {
        let other = if link.src == cell.key {
            link.dst.as_str()
        } else if link.dst == cell.key {
            link.src.as_str()
        } else {
            continue;
        };
        if let Some(hold) = scene
            .cells
            .iter()
            .position(|candidate| candidate.key == other)
        {
            total += hold as f64;
            seen += 1;
        }
    }
    if seen == 0 {
        None
    } else {
        Some(total / seen as f64)
    }
}

/// Draw one box: rule, label, rule, with the delta mark and the heat inside it.
fn draw_box(plate: &mut Plate, cell: &Cell, x: usize, y: usize, w: usize, selected: bool) {
    let face = box_face(cell, selected);
    let double = cell.mark == Some(Mark::Changed);
    let (corner, rule, side) = if double {
        ('╔', '═', '║')
    } else if cell.ghost {
        ('┌', '┄', '┊')
    } else {
        ('┌', '─', '│')
    };
    let (top_right, bot_right) = if double {
        ('╗', '╝')
    } else if cell.ghost {
        ('┓', '┛')
    } else {
        ('┐', '┘')
    };
    let right = x + w.saturating_sub(1);
    // Top rule, with the mark glyph printed over it so a new edge and a new node are both a glyph.
    plate.solid(x, y, corner, face);
    for col in (x + 1)..right {
        plate.solid(col, y, rule, face);
    }
    plate.solid(right, y, top_right, face);
    if let Some(mark) = cell.mark {
        plate.solid(x + w.saturating_sub(2), y, mark.glyph(), face);
    }
    // Label row: name on the left, heat dots on the right.
    if y + 1 < plate.rows() {
        plate.solid(x, y + 1, side, face);
        for col in (x + 1)..right {
            plate.solid(col, y + 1, ' ', Face::Text);
        }
        plate.solid(right, y + 1, side, face);
        let dots = cell.band.dots();
        let dots_at = right.saturating_sub(dots.chars().count() + 1);
        let label_room = dots_at.saturating_sub(x + 1);
        plate.write(x + 1, y + 1, &model::clip(&cell.key, label_room), face);
        if dots_at > x {
            plate.write(dots_at, y + 1, dots, heat_face(cell.band));
        }
    }
    // Bottom rule.
    if y + 2 < plate.rows() {
        plate.solid(x, y + 2, if double { '╚' } else { '└' }, face);
        for col in (x + 1)..right {
            plate.solid(col, y + 2, rule, face);
        }
        plate.solid(right, y + 2, bot_right, face);
    }
}

/// The box's face: the selection, then the mark, then the fact that it is a box.
fn box_face(cell: &Cell, selected: bool) -> Face {
    if selected {
        Face::Emphasis
    } else if cell.ghost {
        Face::Faint
    } else {
        match cell.mark {
            Some(Mark::Added) => Face::Added,
            Some(Mark::Removed) => Face::Removed,
            Some(Mark::Changed) => Face::Warning,
            None => Face::Text,
        }
    }
}

/// The heat column's face. The band is glyphed as well as coloured (`e5.6`), so this is emphasis and not the
/// whole fact.
const fn heat_face(band: Band) -> Face {
    match band {
        Band::High => Face::Failure,
        Band::Mid => Face::Warning,
        Band::Low => Face::Number,
        Band::Nil => Face::Faint,
        Band::Unknown => Face::Muted,
    }
}

/// The face of an arrow: added solid green, removed ghosted red, changed emphasised, and everything else
/// by whether it is in the selected neighbourhood.
fn arrow_face(mark: Option<Mark>, near: bool) -> Face {
    match mark {
        Some(Mark::Added) => Face::Added,
        Some(Mark::Removed) => Face::Removed,
        Some(Mark::Changed) => Face::Warning,
        None if near => Face::Emphasis,
        None => Face::Muted,
    }
}

/// The line style of an arrow. A removed edge is *ghosted*, which is a dashed run and not a red one: the
/// glyph is what survives `NO_COLOR` and a black-and-white printout (ADR-006 §10.2).
const fn arrow_body(mark: Option<Mark>) -> char {
    match mark {
        Some(Mark::Removed) => '╌',
        Some(Mark::Changed) => '┄',
        Some(Mark::Added) | None => '─',
    }
}

/// Route every arrow between placed boxes: out of the source's near edge, across a lane, into the target's
/// near edge, with a vertical jog when the rows differ. An arrow whose path crosses a box is counted
/// undrawn rather than drawn through a name.
fn route(plate: &mut Plate, scene: &Scene, boxes: &BTreeMap<&str, &Box0>) {
    for link in &scene.links {
        let (Some(from), Some(to)) = (boxes.get(link.src.as_str()), boxes.get(link.dst.as_str()))
        else {
            plate.undrawn += 1;
            continue;
        };
        let face = arrow_face(link.mark, link.near);
        let body = arrow_body(link.mark);
        let (x0, y0) = (from.x + from.w, from.mid_y());
        let (x1, y1) = (to.x.saturating_sub(1), to.mid_y());
        let ok = if x0 < x1 {
            // Forward: straight across when the rows agree, otherwise out, down the lane, in.
            if y0 == y1 {
                plate.run(x0, x1, y0, face, body)
            } else {
                plate.run(x0, x1, y0, face, body) && plate.drop(x1, y0, y1, face, body)
            }
        } else {
            // Backward, which is what a cycle looks like: out of the left edge, down the gutter, in.
            let gutter = to.x.saturating_sub(1);
            gutter < from.x
                && plate.run(from.x.saturating_sub(1), gutter, y0, face, body)
                && plate.drop(gutter, y0, y1, face, body)
        };
        if ok {
            plate.drawn += 1;
        } else {
            plate.undrawn += 1;
        }
    }
}

// --- the circular map --------------------------------------------------------------------------

/// One ring, read for clusters, with the arrows as straight raster chords stopped short of every label.
fn circular(
    plate: &mut Plate,
    scene: &Scene,
    cols: usize,
    rows: usize,
    selection: Option<usize>,
) -> Place {
    let count = scene.cells.len();
    if count == 0 {
        return (Vec::new(), 0, None, None);
    }
    let radius_x = (cols.saturating_sub(8) / 2).max(1) as f64;
    let radius_y = (rows.saturating_sub(3) / 2).max(1) as f64;
    let centre_x = cols as f64 / 2.0;
    let centre_y = rows as f64 / 2.0;
    let mut spots: Vec<(usize, usize)> = Vec::with_capacity(count);
    let mut rows_of: Vec<(usize, usize, usize)> = Vec::with_capacity(count);
    for (index, cell) in scene.cells.iter().enumerate() {
        let angle = -std::f64::consts::FRAC_PI_2
            + f64::from(u32::try_from(index).unwrap_or(u32::MAX))
                * (std::f64::consts::TAU / count.max(1) as f64);
        let x = (centre_x + radius_x * angle.cos())
            .round()
            .max(0.0)
            .min((cols.saturating_sub(1)) as f64) as usize;
        let y = (centre_y + radius_y * angle.sin())
            .round()
            .max(0.0)
            .min((rows.saturating_sub(1)) as f64) as usize;
        let label = model::clip(&cell.key, 11);
        let face = box_face(cell, Some(index) == selection);
        let start = x.saturating_sub(label.chars().count() / 2);
        plate.write(start, y, &label, face);
        if let Some(mark) = cell.mark {
            plate.solid(
                start.saturating_sub(1),
                y,
                mark.glyph(),
                box_face(cell, Some(index) == selection),
            );
        }
        plate.solid(
            x,
            y,
            if cell.ghost { '◌' } else { '●' },
            heat_face(cell.band),
        );
        rows_of.push((index, y, y));
        spots.push((x, y));
    }
    for link in &scene.links {
        let positions = (
            scene.cells.iter().position(|c| c.key == link.src),
            scene.cells.iter().position(|c| c.key == link.dst),
        );
        let (Some(a), Some(b)) = positions else {
            plate.undrawn += 1;
            continue;
        };
        let (x0, y0) = spots[a];
        let (x1, y1) = spots[b];
        if line(
            plate,
            x0,
            y0,
            x1,
            y1,
            arrow_face(link.mark, link.near),
            arrow_body(link.mark),
        ) {
            plate.drawn += 1;
        } else {
            plate.undrawn += 1;
        }
    }
    let notice = if count > Layout::Circular.capacity(cols, rows) {
        Some(format!(
            "{count} labels on a ring this size overlap — `[` folds one level up"
        ))
    } else {
        None
    };
    (
        rows_of,
        0,
        notice,
        Some("one ring, drawn in scene order".to_owned()),
    )
}

/// A chord of arrow glyphs from `(x0,y0)` to `(x1,y1)`, skipping the two end cells so a line never prints
/// through a label it is attached to. `false` when a solid cell sits on the path.
fn line(
    plate: &mut Plate,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    face: Face,
    body: char,
) -> bool {
    let dx = x1 as i64 - x0 as i64;
    let dy = y1 as i64 - y0 as i64;
    let steps = dx.abs().max(dy.abs()) as usize;
    if steps < 2 {
        return false;
    }
    let mut path: Vec<(usize, usize)> = Vec::with_capacity(steps + 1);
    for step in 1..steps {
        let t = step as f64 / steps as f64;
        let x = (x0 as f64 + dx as f64 * t).round().max(0.0) as usize;
        let y = (y0 as f64 + dy as f64 * t).round().max(0.0) as usize;
        path.push((x, y));
    }
    for (x, y) in &path {
        if !plate.arrow_fits(*x, *y) {
            return false;
        }
    }
    for (step, (x, y)) in path.iter().enumerate() {
        let last = step + 1 == path.len();
        let ch = if last {
            head(dx, dy)
        } else if body != '─' {
            body
        } else if dy.abs() > dx.abs() {
            '│'
        } else if dx * dy < 0 {
            '╱'
        } else {
            '╲'
        };
        plate.arrow(*x, *y, ch, face);
    }
    true
}

/// The arrowhead glyph for a chord's direction.
const fn head(dx: i64, dy: i64) -> char {
    if dy.abs() > dx.abs() {
        if dy > 0 { '▼' } else { '▲' }
    } else if dx >= 0 {
        '▶'
    } else {
        '◀'
    }
}

// --- the structure map -------------------------------------------------------------------------

/// The hierarchical treemap: nested boxes sized by weight, heat as the fill, drilled with `<Enter>`.
///
/// Squarified rather than slice-and-dice, because the whole point is comparing areas by eye and
/// slice-and-dice's long thin strips are unreadable at 54 columns (Bruls, Huizing & van Wijk's rule: keep
/// the worst aspect ratio in a row from getting worse). A tile too small for a box prints one glyph and is
/// counted in the notice, which is the same aggregation-not-clipping rule one level down.
fn treemap(
    plate: &mut Plate,
    scene: &Scene,
    cols: usize,
    rows: usize,
    selection: Option<usize>,
) -> Place {
    if scene.cells.is_empty() || cols < 3 || rows < 2 {
        return (Vec::new(), scene.cells.len(), None, None);
    }
    let arena = Arena::build(scene);
    let roots: Vec<usize> = arena.roots.clone();
    let mut carried = Carried::default();
    squarify(
        &arena,
        &roots,
        scene,
        plate,
        Rect {
            x: 0,
            y: 0,
            w: cols,
            h: rows,
        },
        selection,
        &mut carried,
    );
    let rows_of = carried.rows_of;
    let too_small = carried.too_small;
    let notice = if too_small > 0 {
        Some(format!(
            "{too_small} cells are too small for a box at this size — `[` folds one level up"
        ))
    } else {
        None
    };
    (
        rows_of,
        too_small,
        notice,
        Some("treemap of the module tree, area = modules per tile".to_owned()),
    )
}

/// A rectangle in plate coordinates.
#[derive(Debug, Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Rect {
    const fn inset(self, by: usize) -> Rect {
        Rect {
            x: self.x + by,
            y: self.y + by,
            w: self.w.saturating_sub(by * 2),
            h: self.h.saturating_sub(by * 2),
        }
    }
}

/// One node of the treemap's tree.
#[derive(Debug, Default, Clone)]
struct Tile {
    name: String,
    /// The scene index this tile draws, when the tile *is* a module rather than a directory.
    cell: Option<usize>,
    children: Vec<usize>,
    weight: f64,
}

/// The tile tree, in an arena, so the layout code passes indices instead of fighting the borrow checker.
#[derive(Debug, Default)]
struct Arena {
    tiles: Vec<Tile>,
    roots: Vec<usize>,
}

impl Arena {
    /// Build the directory tree over the scene's cells. A cell that *is* a directory prefix gets one tile
    /// that is both, which is what `src/store.rs` beside a `src/store/` directory looks like.
    fn build(scene: &Scene) -> Arena {
        let mut arena = Arena::default();
        let mut by_path: HashMap<String, usize> = HashMap::new();
        for (index, cell) in scene.cells.iter().enumerate() {
            let parts: Vec<&str> = cell.key.split('/').collect();
            let mut path: Vec<&str> = Vec::new();
            for (depth, part) in parts.iter().enumerate() {
                path.push(*part);
                let whole = path.join("/");
                let at = if let Some(existing) = by_path.get(&whole) {
                    *existing
                } else {
                    let fresh = arena.tiles.len();
                    arena.tiles.push(Tile {
                        name: (*part).to_owned(),
                        cell: None,
                        children: Vec::new(),
                        weight: 0.0,
                    });
                    by_path.insert(whole, fresh);
                    if path.len() == 1 {
                        arena.roots.push(fresh);
                    } else if let Some(parent) = by_path.get(&path[..path.len() - 1].join("/")) {
                        arena.tiles[*parent].children.push(fresh);
                    }
                    fresh
                };
                if depth + 1 == parts.len() {
                    arena.tiles[at].cell = Some(index);
                }
            }
        }
        let weights: Vec<f64> = (0..arena.tiles.len())
            .map(|at| {
                arena.tiles[at]
                    .cell
                    .and_then(|index| scene.cells.get(index))
                    .map_or(0.05, |cell| f64::from(cell.total.max(1) as u32))
                    .max(0.05)
            })
            .collect();
        for (tile, weight) in arena.tiles.iter_mut().zip(weights) {
            tile.weight = weight;
        }
        // Bottom-up by index: a parent tile is always created before its children, because the path is
        // walked top-down, so walking the arena backwards visits every child before its parent.
        for at in (0..arena.tiles.len()).rev() {
            let children = arena.tiles[at].children.clone();
            let sum: f64 = children
                .iter()
                .map(|child| arena.tiles[*child].weight)
                .sum();
            arena.tiles[at].weight = arena.tiles[at].weight.max(sum);
        }
        arena
    }
}

/// What the squarify walk writes back: the rows each cell drew, how many tiles were too small to name, and
/// how many were drawn at all.
#[derive(Debug, Default)]
struct Carried {
    rows_of: Vec<(usize, usize, usize)>,
    too_small: usize,
    placed: usize,
}

/// The squarify walk.
fn squarify(
    arena: &Arena,
    tiles: &[usize],
    scene: &Scene,
    plate: &mut Plate,
    rect: Rect,
    selection: Option<usize>,
    carried: &mut Carried,
) {
    if rect.w == 0 || rect.h == 0 || tiles.is_empty() {
        return;
    }
    let mut order: Vec<usize> = tiles.to_vec();
    order.sort_by(|a, b| {
        arena.tiles[*b]
            .weight
            .partial_cmp(&arena.tiles[*a].weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| arena.tiles[*a].name.cmp(&arena.tiles[*b].name))
    });
    let total: f64 = order
        .iter()
        .map(|at| arena.tiles[*at].weight)
        .sum::<f64>()
        .max(0.05);
    let scale = (rect.w * rect.h) as f64 / total;
    let mut rect = rect;
    // A cursor into `order`, so no slice borrow fights the loop that rewrites the rectangle.
    let mut taken = 0usize;
    while taken < order.len() && rect.w > 0 && rect.h > 0 {
        let rest = &order[taken..];
        let wide = rect.w >= rect.h;
        let length = if wide { rect.h } else { rect.w } as f64;
        // Take the longest prefix of `rest` whose worst aspect ratio does not get worse — the squarify rule.
        let mut row: Vec<usize> = Vec::new();
        let mut index = 0usize;
        while index < rest.len() {
            let mut candidate = row.clone();
            candidate.push(rest[index]);
            if row.is_empty() || worst(arena, &candidate, length) <= worst(arena, &row, length) {
                row = candidate;
                index += 1;
            } else {
                break;
            }
        }
        if row.is_empty() {
            break;
        }
        let row_weight: f64 = row
            .iter()
            .map(|at| arena.tiles[*at].weight)
            .sum::<f64>()
            .max(0.05);
        let thick = (((row_weight * scale) / length).round() as usize)
            .clamp(1, if wide { rect.w } else { rect.h });
        let mut cursor = if wide { rect.x } else { rect.y };
        for at in &row {
            let tile = &arena.tiles[*at];
            let extent = (((tile.weight * scale) / thick.max(1) as f64).round() as usize).max(1);
            let here = if wide {
                Rect {
                    x: cursor,
                    y: rect.y,
                    w: thick.min(rect.w),
                    h: extent.min(rect.h),
                }
            } else {
                Rect {
                    x: rect.x,
                    y: cursor,
                    w: extent.min(rect.w),
                    h: thick.min(rect.h),
                }
            };
            cursor += extent;
            draw_tile(arena, tile, scene, plate, here, selection, carried);
        }
        if wide {
            rect.x += thick;
            rect.w = rect.w.saturating_sub(thick);
        } else {
            rect.y += thick;
            rect.h = rect.h.saturating_sub(thick);
        }
        taken += index;
    }
}

/// The worst aspect ratio in a row laid along `length` — the number squarify minimises.
fn worst(arena: &Arena, row: &[usize], length: f64) -> f64 {
    if row.is_empty() || length <= 0.0 {
        return f64::MAX;
    }
    let sum: f64 = row
        .iter()
        .map(|at| arena.tiles[*at].weight)
        .sum::<f64>()
        .max(0.05);
    let mut worst = 0.0f64;
    for at in row {
        let weight = arena.tiles[*at].weight.max(0.05);
        let long = (length * length * weight) / (sum * sum);
        let short = (sum * sum) / (length * length * weight);
        worst = worst.max(long.max(short));
    }
    worst
}

/// Draw one tile: a box with a name and heat, or one glyph when there is no room for a box, then recurse
/// into its children inside the box.
fn draw_tile(
    arena: &Arena,
    tile: &Tile,
    scene: &Scene,
    plate: &mut Plate,
    rect: Rect,
    selection: Option<usize>,
    carried: &mut Carried,
) {
    let cell = tile.cell.and_then(|index| scene.cells.get(index));
    let face = cell.map_or(Face::Muted, |cell| box_face(cell, tile.cell == selection));
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    if rect.w < TILE_MIN_COLS || rect.h < TILE_MIN_ROWS {
        if cell.is_some() {
            carried.too_small += 1;
        }
        if cell.is_some() {
            plate.solid(
                rect.x,
                rect.y,
                cell.and_then(|cell| cell.mark)
                    .map_or('▒', |mark| mark.glyph()),
                cell.map_or(Face::Muted, |cell| heat_face(cell.band)),
            );
            if let Some(index) = tile.cell {
                carried.rows_of.push((index, rect.y, rect.y));
            }
        }
        return;
    }
    carried.placed += 1;
    let right = rect.x + rect.w.saturating_sub(1);
    let bottom = rect.y + rect.h.saturating_sub(1);
    plate.solid(rect.x, rect.y, '┌', face);
    for col in (rect.x + 1)..right {
        plate.solid(col, rect.y, '─', face);
    }
    plate.solid(right, rect.y, '┐', face);
    let label = model::clip(&tile.name, rect.w.saturating_sub(6).max(1));
    plate.solid(rect.x, rect.y + 1, '│', face);
    plate.write(rect.x + 2, rect.y + 1, &label, face);
    if let Some(cell) = cell {
        let dots = cell.band.dots();
        let at = right.saturating_sub(dots.chars().count() + 1);
        if at > rect.x + 1 + label.chars().count() {
            plate.write(at, rect.y + 1, dots, heat_face(cell.band));
        }
        if let Some(mark) = cell.mark {
            plate.solid(
                right.saturating_sub(1).max(rect.x),
                rect.y,
                mark.glyph(),
                face,
            );
        }
        if cell.ghost {
            plate.write(rect.x + 2, (rect.y + 2).min(bottom), "[gone]", Face::Faint);
        }
    }
    if rect.h >= 3 {
        plate.solid(rect.x, bottom, '└', face);
        for col in (rect.x + 1)..right {
            plate.solid(col, bottom, '─', face);
        }
        plate.solid(right, bottom, '┘', face);
    }
    if let Some(index) = tile.cell {
        carried.rows_of.push((index, rect.y, bottom));
    }
    if !tile.children.is_empty() {
        let inner = rect.inset(1);
        if inner.w >= 2 && inner.h >= 2 {
            let children = tile.children.clone();
            squarify(arena, &children, scene, plate, inner, selection, carried);
        }
    }
}

// --- the fallback table ------------------------------------------------------------------------

/// The aggregated node table: what the canvas draws when the map layouts are not compiled in, and the
/// shape it keeps when a filter has left one layer's worth of cells to list.
///
/// This is not a lesser answer. It is the same scene, the same counts and the same marks in the one shape
/// that stays legible at 36 columns with no layout engine, and the first line names the feature that would
/// buy the picture.
fn table(
    plate: &mut Plate,
    scene: &Scene,
    source: &hand::Hand,
    cols: usize,
    rows: usize,
    selection: Option<usize>,
) -> Place {
    let mut rows_of: Vec<(usize, usize, usize)> = Vec::new();
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, cell) in scene.cells.iter().enumerate() {
        let group = cell
            .layer
            .clone()
            .unwrap_or_else(|| cell.key.split('/').next().unwrap_or(&cell.key).to_owned());
        groups.entry(group).or_default().push(index);
    }
    let mut row = 0usize;
    let mut undrawn = 0usize;
    for (group, members) in groups {
        if row >= rows {
            undrawn += members.len();
            continue;
        }
        let hottest = members
            .iter()
            .filter_map(|index| scene.cells.get(*index))
            .map(|cell| cell.band)
            .max()
            .unwrap_or(Band::Unknown);
        let head = format!(
            "{} · {} modules · heat {}",
            model::clip(&group, cols.saturating_sub(20).max(4)),
            members.len(),
            hottest.dots()
        );
        plate.write(0, row, &model::clip(&head, cols), Face::Dir);
        row += 1;
        for index in members {
            if row >= rows {
                undrawn += 1;
                continue;
            }
            let cell = &scene.cells[index];
            let mark = cell.mark.map_or_else(String::new, |mark| {
                format!(" {} {}", mark.glyph(), mark.as_str())
            });
            let fan = source.fan(cell.seed());
            let name = model::clip(&cell.key, cols.saturating_sub(26).max(6));
            let line = format!(
                "  {} {}{} · in {} out {}",
                cell.kind.badge(),
                name,
                mark,
                fan.in_,
                fan.out
            );
            plate.write(
                0,
                row,
                &model::clip(&line, cols),
                box_face(cell, Some(index) == selection),
            );
            rows_of.push((index, row, row));
            row += 1;
        }
    }
    let notice = if undrawn > 0 {
        Some(format!(
            "{undrawn} cells below the end of this table — `[` folds one level up"
        ))
    } else {
        None
    };
    (
        rows_of,
        undrawn,
        notice,
        Some("aggregated node table by layer".to_owned()),
    )
}

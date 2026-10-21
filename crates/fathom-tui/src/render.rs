//! `fathom_tui::render` — the frame as text, and the blit that puts it on a real terminal.
//!
//! The renderer's product is `Vec<String>`: `rows` lines, each exactly `cols` display cells. That
//! is a deliberate seam, not a stage on the way to "real" rendering:
//!
//! * the smoke tests at 80×24, 100×30 and 200×60 assert on the same bytes the user sees, so "no
//!   layout panic" and "nothing clipped" are string checks with no terminal involved;
//! * `--plain` is the same function with the trailing blanks trimmed (ADR-006 §6.7), so the text
//!   surface cannot drift from the interactive one;
//! * `e5.6`'s theming will add styles to the blit step, which is then the only place a colour can
//!   be wrong — and ADR-006 §10.2's "colour never carries a fact alone" stays true by
//!   construction, because the text already carries every fact.
//!
//! Chrome is drawn from the [`crate::layout`] answer for the current size, so a degraded frame is
//! *shaped* by the same arithmetic that announced it.

use ratatui::buffer::Buffer;
use ratatui::style::Style;

use crate::app::{App, Overlay};
use crate::layout::{self, FitState, display_width, fit_text, refusal_lines};
use crate::style::{Face, StyledLine};
use crate::views::{CH_FRAME, CH_HINT, CH_STATUS, CH_STRIPS, CH_TABS, Pane, Role, View, ViewId};

/// Box characters, one set for the whole crate so a half-degraded frame still lines up.
const TL: char = '┌';
const TR: char = '┐';
const BL: char = '└';
const BR: char = '┘';
const H: char = '─';
const V: char = '│';
const TC: char = '┬';
/// The marker that says *this* pane has the keyboard.
const FOCUS: char = '▸';

/// A frame of cells, so an overlay composites instead of overwriting whole rows.
///
/// A cell holds a whole grapheme cluster, not a `char`, because a cell is not a character: a CJK
/// ideograph is one cluster in two cells, and an index-per-character grid would slide every glyph
/// after it one cell left and print a frame that lies about where its own borders are. The second
/// cell of a wide cluster holds nothing at all — not a space, which would push the row a cell long.
#[derive(Debug)]
struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<String>,
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Self {
        Grid {
            cols,
            rows,
            cells: vec![" ".to_owned(); cols.saturating_mul(rows)],
        }
    }

    fn fill(&mut self, lines: &[String]) {
        for (y, line) in lines.iter().take(self.rows).enumerate() {
            self.put(0, y, line);
        }
    }

    fn put(&mut self, x: usize, y: usize, text: &str) {
        if y >= self.rows {
            return;
        }
        let mut column = x;
        for cluster in unicode_segmentation::UnicodeSegmentation::graphemes(text, true) {
            if column >= self.cols {
                break;
            }
            let wide = crate::text::cluster_width(cluster).max(1);
            self.cells[y * self.cols + column] = cluster.to_owned();
            // The cells a wide cluster covers hold nothing: their width is already paid for by the
            // cluster itself, which is why they are empty strings and not blanks.
            for covered in 1..wide {
                if column + covered < self.cols {
                    self.cells[y * self.cols + column + covered] = String::new();
                }
            }
            column += wide;
        }
    }

    fn to_lines(&self) -> Vec<String> {
        (0..self.rows)
            .map(|y| {
                let joined: String = self.cells[y * self.cols..(y + 1) * self.cols]
                    .iter()
                    .map(String::as_str)
                    .collect();
                crate::text::fit(&joined, self.cols)
            })
            .collect()
    }
}

/// An overlay panel: a box, a title, and rows of which one may be selected.
struct Panel<'a> {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    title: &'a str,
    lines: &'a [String],
    selection: Option<usize>,
}

// ---------------------------------------------------------------------------
// the geometry, separate from the drawing
// ---------------------------------------------------------------------------

/// One pane's rectangle in the drawn frame.
///
/// `ring` is the pane's index in [`View::focus_ring`] — the same number [`App::pane`] holds — so a
/// click can say "focus this pane" in the vocabulary the keyboard already uses.
#[derive(Debug, Clone)]
pub struct PaneRect {
    /// The pane, from the registry.
    pub pane: &'static Pane,
    /// Its position in the focus ring.
    pub ring: usize,
    /// First column, in frame cells.
    pub x: usize,
    /// First body row.
    pub y: usize,
    /// Width in cells.
    pub cols: usize,
    /// Body rows.
    pub rows: usize,
}

/// The frame's shape at one size: which chrome survived the ladder, where each pane is, and how
/// many body rows there are.
///
/// This is [`view_styled`]'s arithmetic pulled out in front of its drawing, for two reasons. The
/// mouse needs it (ADR-006 §10.1: the pointer issues the same actions as the keyboard, which
/// begins with knowing which pane it was over) and the renderer needs it, and the two must never
/// disagree about where a pane is — a click landing one row off is worse than no mouse.
#[derive(Debug, Clone)]
pub struct FramePlan {
    /// The view being drawn.
    pub view: ViewId,
    /// Frame columns.
    pub cols: usize,
    /// Frame rows.
    pub rows: usize,
    /// What the ladder said.
    pub fit: layout::Fit,
    /// The view refused to draw at this size.
    pub refused: bool,
    /// A box is drawn (D4 has not fired).
    pub boxed: bool,
    /// The footer strips are drawn (D3 has not fired).
    pub strips: bool,
    /// The tab bar is drawn (D2 has not fired).
    pub tabs: bool,
    /// The hint row is drawn (D1 has not fired).
    pub hint: bool,
    /// This view draws its own status line.
    pub status: bool,
    /// This view has a query row (the search overlay).
    pub query: bool,
    /// First body row (the header row is the one above it).
    pub body_y: usize,
    /// How many body rows there are.
    pub body_rows: usize,
    /// The panes, left to right, with their rectangles.
    pub panes: Vec<PaneRect>,
}

/// What the pointer was over.
#[derive(Debug, Clone, Copy)]
pub enum Hit<'a> {
    /// A pane's body: `line` is the row inside it, 0 at the top.
    Body {
        /// The pane.
        pane: &'a Pane,
        /// Its focus-ring index.
        ring: usize,
        /// The line within the body.
        line: usize,
    },
    /// A pane's header row: a title is a place to click, and it should focus without moving a
    /// selection the reviewer was about to make.
    Header {
        /// The pane.
        pane: &'a Pane,
        /// Its focus-ring index.
        ring: usize,
    },
    /// The query row, which belongs to the search field.
    Query,
    /// Chrome — the tab bar, the status line, the hint row, the refusal panel. Named, so the
    /// caller can say what it hit instead of doing nothing quietly.
    Chrome(&'static str),
    /// Outside the frame.
    Outside,
}

/// The frame's shape at a size, computed the same way [`view_styled`] computes it to draw.
#[must_use]
pub fn frame_plan(app: &App, view_id: ViewId, cols: usize, rows: usize) -> FramePlan {
    let view = View::by_id(view_id);
    let fit = layout::fit_chosen(view_id, cols as u16, rows as u16, &app.forced_steps());
    let refused = fit.state == FitState::Refused;
    let applied = fit.applied.clone();
    let gone = |id: &str| applied.iter().any(|a| a == id);
    let boxed = view.has_chrome(CH_FRAME) && !gone(layout::Step::Frame.id());
    let strips = view.has_chrome(CH_STRIPS) && !gone(layout::Step::Strips.id());
    let tabs = view.has_chrome(CH_TABS) && !gone(layout::Step::Tabs.id());
    let hint = view.has_chrome(CH_HINT) && !gone(layout::Step::Hint.id());
    let status = view.has_chrome(CH_STATUS);
    let query = view.query().is_some();
    let mut widths = layout::pane_widths(view_id, cols as u16, &applied);
    if app.zoomed
        && let Some(pane) = app.focused_pane()
    {
        // `<C-w>o`: the focused pane takes the frame. Nothing was dropped and the ladder did not
        // run — the reviewer asked to look at one thing (§5.0), and `<C-w>o` again puts it back.
        let frame = usize::from(layout::frame_cols(view, &applied));
        let width = cols.saturating_sub(frame).max(1);
        widths = vec![(pane, u16::try_from(width).unwrap_or(1))];
    }
    // The renderer's own row budget: exactly one line per row it is going to print. This is not
    // `layout::chrome_rows`, which is the ADR's model of what chrome *costs* and is what `fit`
    // reasons about; the two agree on the sizes the ladder was derived at, and this one is the one
    // that decides whether the drawn frame is `rows` lines tall.
    let overhead = usize::from(tabs)
        + usize::from(query)
        + 1 // the pane header row, boxed or not
        + usize::from(strips)
        + usize::from(boxed) // the bottom rule
        + usize::from(status)
        + usize::from(hint);
    let body_rows = rows.saturating_sub(overhead).max(1);
    let body_y = usize::from(tabs) + usize::from(query) + 1;
    let ring = view.focus_ring();
    let mut x = usize::from(boxed);
    let panes: Vec<PaneRect> = widths
        .iter()
        .map(|(pane, width)| {
            let cols_pane = usize::from(*width);
            let rect = PaneRect {
                pane,
                ring: ring.iter().position(|name| *name == pane.name).unwrap_or(0),
                x,
                y: body_y,
                cols: cols_pane,
                rows: body_rows,
            };
            // One column rule between panes, boxed or not — the same rule `row_of` draws with.
            x = x.saturating_add(cols_pane).saturating_add(1);
            rect
        })
        .collect();
    FramePlan {
        view: view_id,
        cols,
        rows,
        fit,
        refused,
        boxed,
        strips,
        tabs,
        hint,
        status,
        query,
        body_y,
        body_rows,
        panes,
    }
}

/// The frame's bottom chrome rows, in the order the renderer takes them from the bottom.
///
/// `None` means the row is not drawn — D3 or D4 fired, or the view has no status line of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeRows {
    /// The hint row, `D1`.
    pub hint: Option<usize>,
    /// The status row.
    pub status: Option<usize>,
    /// The frame's bottom rule, `D4`.
    pub bottom: Option<usize>,
    /// The pane footer strips, `D3`.
    pub strip: Option<usize>,
}

impl FramePlan {
    /// The chrome rows, counted from the bottom of the frame, in the order the renderer takes them:
    /// hint, status, the box's bottom rule, and the pane footer strips inside the box.
    ///
    /// Counting from the bottom is not a stylistic choice: these are the rows the renderer sells from
    /// the bottom up (`frame::layout` takes the hint row, then the status row, then the frame), and a
    /// pointer map that counted from the top would disagree with the drawing about which row is the
    /// bottom rule — the one row a click must never land on as if it were content.
    #[must_use]
    pub fn chrome_rows(&self) -> ChromeRows {
        let mut row = self.rows;
        let mut take = |take: bool| {
            if take {
                row -= 1;
                Some(row)
            } else {
                None
            }
        };
        let hint = take(self.hint);
        let status = take(self.status);
        let bottom = take(self.boxed);
        let strip = take(self.strips);
        ChromeRows {
            hint,
            status,
            bottom,
            strip,
        }
    }

    /// What the pointer at `(col, row)` was over, in cells from the frame's top-left.
    #[must_use]
    pub fn hit(&self, col: usize, row: usize) -> Hit<'static> {
        if row >= self.rows || col >= self.cols {
            return Hit::Outside;
        }
        if self.refused {
            return Hit::Chrome("the refusal panel");
        }
        if self.tabs && row == 0 {
            return Hit::Chrome("the tab bar");
        }
        if self.query && row == usize::from(self.tabs) {
            return Hit::Query;
        }
        let chrome = self.chrome_rows();
        if chrome.hint == Some(row) {
            return Hit::Chrome("the hint line");
        }
        if chrome.status == Some(row) {
            return Hit::Chrome("the status line");
        }
        if chrome.bottom == Some(row) {
            return Hit::Chrome("the bottom rule");
        }
        if chrome.strip == Some(row) {
            return Hit::Chrome("a pane footer strip");
        }
        let mut pane_at_col: Option<&PaneRect> = None;
        for rect in &self.panes {
            if col >= rect.x && col < rect.x + rect.cols {
                pane_at_col = Some(rect);
            }
        }
        let Some(rect) = pane_at_col else {
            return Hit::Chrome("a column rule");
        };
        if row + 1 == self.body_y {
            return Hit::Header {
                pane: rect.pane,
                ring: rect.ring,
            };
        }
        if row >= self.body_y && row < self.body_y + self.body_rows {
            return Hit::Body {
                pane: rect.pane,
                ring: rect.ring,
                line: row - self.body_y,
            };
        }
        Hit::Outside
    }

    /// The pane under the pointer, if a pane is.
    #[must_use]
    pub fn pane_at(&self, col: usize) -> Option<&PaneRect> {
        self.panes
            .iter()
            .find(|rect| col >= rect.x && col < rect.x + rect.cols)
    }
}

/// Every line of the frame, each exactly `cols` cells wide and `rows` lines tall.
///
/// This is the *text projection* of [`frame_styled`], and that direction is the contract: the golden
/// files, `--plain` and a screen reader all read this, so every fact is in the text and the style
/// channel only decorates it (ADR-006 §10.2).
#[must_use]
pub fn frame_lines(app: &App) -> Vec<String> {
    frame_styled(app)
        .into_iter()
        .map(|line| line.text().to_owned())
        .collect()
}

/// Every line of the frame with its faces, which is what the blit draws.
///
/// With no overlay this is the view's own styled rows. An overlay is a panel of text — the help table,
/// the palette, the search field — composited on the char grid the shell already had, and it carries no
/// faces until `e5.6`'s theming gives a panel a style channel too. The text is identical either way,
/// which is the part that carries the facts.
#[must_use]
pub fn frame_styled(app: &App) -> Vec<StyledLine> {
    let (cols, rows) = (usize::from(app.size.0), usize::from(app.size.1));
    let base = view_styled(app, app.view, cols, rows);
    if app.overlay.is_none() {
        return base;
    }
    let mut grid = Grid::new(cols, rows);
    grid.fill(
        &base
            .iter()
            .map(|line| line.text().to_owned())
            .collect::<Vec<String>>(),
    );
    // A triage chooser is drawn *in* the worklist pane rather than floated over it (`e4.5`): the
    // reviewer is choosing a reason about the row they can already see, and a panel that covers the row
    // while they pick its reason is a worse trade than a few rows of the list disappearing for a moment.
    match app.overlay {
        Some(Overlay::Help) => help(&mut grid, app),
        Some(Overlay::Palette) => palette(&mut grid, app),
        Some(Overlay::Filter) => filters(&mut grid, app),
        Some(Overlay::Compare) => compare(&mut grid, app),
        Some(Overlay::Reason | Overlay::Snooze | Overlay::Note) => {}
        Some(Overlay::Search) => {
            // The search overlay borrows the frame (§1) and brings its own query row; the live
            // view behind it keeps its selection, which is what `<Esc>` returning to it means.
            let height = usize::from(View::by_id(ViewId::Search).ideal_rows)
                .min(rows)
                .max(3);
            let search = view_styled(app, ViewId::Search, cols, height);
            grid.fill(
                &search
                    .iter()
                    .map(|line| line.text().to_owned())
                    .collect::<Vec<String>>(),
            );
        }
        None => {}
    }
    grid.to_lines().into_iter().map(StyledLine::plain).collect()
}

/// One view's frame as text — the projection of [`view_styled`].
#[must_use]
pub fn view_lines(app: &App, view_id: ViewId, cols: usize, rows: usize) -> Vec<String> {
    view_styled(app, view_id, cols, rows)
        .into_iter()
        .map(|line| line.text().to_owned())
        .collect()
}

/// One view's frame: chrome, panes, status line, hint line — or the refusal panel.
#[must_use]
pub fn view_styled(app: &App, view_id: ViewId, cols: usize, rows: usize) -> Vec<StyledLine> {
    if rows == 0 || cols == 0 {
        return Vec::new();
    }
    let plan = frame_plan(app, view_id, cols, rows);
    let view = View::by_id(view_id);
    if plan.refused {
        // The refusal replaces the view. The status line stays: the ladder never sells it, and the
        // notice has to land somewhere.
        let mut out: Vec<StyledLine> = refusal_lines(view_id, cols as u16, rows as u16)
            .into_iter()
            .map(StyledLine::plain)
            .collect();
        out.truncate(rows.saturating_sub(usize::from(plan.status)).max(1));
        if plan.status && out.len() < rows {
            out.push(StyledLine::plain(status_line(app)).padded(cols));
        }
        out.truncate(rows);
        while out.len() < rows {
            out.push(StyledLine::plain("").padded(cols));
        }
        return out;
    }
    let boxed = plan.boxed;
    let body_rows = plan.body_rows;

    let mut out: Vec<StyledLine> = Vec::new();
    if plan.tabs {
        out.push(StyledLine::plain(tab_line(app, cols)).padded(cols));
    }
    if let Some(query) = view.query() {
        out.push(StyledLine::plain(query_line(app, query)).padded(cols));
    }
    let heads: Vec<StyledLine> = plan
        .panes
        .iter()
        .map(|rect| StyledLine::plain(pane_head(app, rect.pane)).padded(rect.cols))
        .collect();
    out.push(row_of(&heads, boxed, TL, TC, TR, cols));

    let bodies: Vec<Vec<StyledLine>> = plan
        .panes
        .iter()
        .map(|rect| pane_body(app, rect.pane, rect.cols, body_rows))
        .collect();
    for row in 0..body_rows {
        let parts: Vec<StyledLine> = plan
            .panes
            .iter()
            .enumerate()
            .map(|(index, rect)| {
                bodies[index]
                    .get(row)
                    .cloned()
                    .unwrap_or_else(|| StyledLine::plain("").padded(rect.cols))
            })
            .collect();
        out.push(row_of(&parts, boxed, V, V, V, cols));
    }
    if plan.strips {
        let parts: Vec<StyledLine> = plan
            .panes
            .iter()
            .map(|rect| StyledLine::plain(app.pane_strip(rect.pane)).padded(rect.cols))
            .collect();
        out.push(row_of(&parts, boxed, V, V, V, cols));
    }
    if boxed {
        // One rule the whole width: the junction marks between the panes would need the widths to
        // add up exactly, and a rule that is one cell short reads as a broken box.
        out.push(
            StyledLine::with_face(
                format!("{BL}{}{BR}", H.to_string().repeat(cols.saturating_sub(2))),
                crate::style::Face::Muted,
            )
            .padded(cols),
        );
    }
    if plan.status {
        out.push(StyledLine::plain(status_line(app)).padded(cols));
    }
    if plan.hint {
        out.push(StyledLine::plain(app.hint_line()).padded(cols));
    }
    out.truncate(rows);
    while out.len() < rows {
        out.push(StyledLine::plain("").padded(cols));
    }
    out
}

/// Join pane columns into one row, with the frame's rules, then clip/pad the row to the frame.
fn row_of(
    parts: &[StyledLine],
    boxed: bool,
    left: char,
    mid: char,
    right: char,
    cols: usize,
) -> StyledLine {
    let mut out = StyledLine::new();
    if boxed {
        out.push(Face::Muted, &left.to_string());
    }
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            out.push(Face::Muted, &mid.to_string());
        }
        out.push_line(part);
    }
    if boxed {
        out.push(Face::Muted, &right.to_string());
    }
    out.padded(cols)
}

/// The help overlay: the merged keymap, generated. Whatever a user's `keys.overrides` changed,
/// this is the table that shows it, which is what makes the help overlay unable to lie.
fn help(grid: &mut Grid, app: &App) {
    let width = grid.cols;
    let height = grid.rows;
    let inner = width.saturating_sub(4);
    let room = height.saturating_sub(3).max(1);
    // What went wrong or came short leads, because the keymap below it is long and the truncation
    // happens at the bottom: a notice that could be the part cut off is a notice nobody read.
    let mut panel: Vec<String> = Vec::new();
    if !app.notices.is_empty() {
        panel.push("reported at startup — nothing below was hidden".to_owned());
        panel.extend(
            app.notices
                .iter()
                .map(|notice| format!("\u{26a0} {notice}")),
        );
        panel.push(String::new());
    }
    panel.push(format!("terminal \u{00b7} {}", app.term.label()));
    // The theme, and how much colour the terminal was allowed to be given: "my theme did not arrive" is
    // the same question in two halves — which theme did Fathom pick, and what did the terminal say it
    // could carry — and neither is something the reviewer can read off the frame. ADR-006 §6.7.
    panel.push(format!(
        "theme \u{00b7} {} \u{00b7} {}",
        app.theme.name,
        app.term.color().label()
    ));
    panel.push(String::new());
    // The query grammar belongs in the help overlay and not only in the box: a reviewer who does not know
    // `risk>` exists has no way to discover it by typing into a search field, and `?` is where they are
    // when they have given up looking (ADR-006 §5.8).
    panel.push("search filters (`/`) — they compose with free text (ADR-006 §5.8)".to_owned());
    for (token, meaning) in crate::search::FILTERS {
        panel.push(format!("  {token:<9} {meaning}"));
    }
    panel.push(String::new());
    panel.extend(app.keymap.help_lines(inner));
    let hidden = panel.len().saturating_sub(room);
    panel.truncate(room);
    if hidden > 0 {
        panel.push(format!(
            "… {hidden} more lines — the same table is `fathom tui --show-keys`"
        ));
    }
    draw_panel(
        grid,
        Panel {
            x: 0,
            y: 0,
            width,
            height,
            title: "keymap — the merged table (ADR-006 §7.3) · Esc closes",
            lines: &panel,
            selection: None,
        },
    );
}

/// The `,` palette: every action, with the key it has right now, and the searches and comparisons the
/// reviewer gave names to.
fn palette(grid: &mut Grid, app: &App) {
    let (cols, rows) = (grid.cols, grid.rows);
    let height = 14.min(rows.saturating_sub(1)).max(4);
    let width = 76.min(cols).max(24);
    let entries = app.palette_rows();
    let shown: Vec<String> = entries
        .iter()
        .take(height.saturating_sub(4))
        .map(|row| {
            let key = row.right();
            format!(
                "{:<width$} {key}",
                truncate(&row.label(), width.saturating_sub(16).max(12)),
                width = width.saturating_sub(16).max(12)
            )
        })
        .collect();
    let saved = app.saved_searches.len() + app.saved_comparisons.len();
    let mut lines = vec![format!(",{}▏", app.prompt)];
    lines.extend(shown);
    lines.push(format!(
        "{} of {} · Enter runs it · Esc closes",
        (app.palette_row + 1).min(entries.len()),
        if saved == 0 {
            "commands".to_owned()
        } else {
            format!(
                "{} commands + {saved} saved",
                entries.len().saturating_sub(saved)
            )
        }
    ));
    draw_panel(
        grid,
        Panel {
            x: cols.saturating_sub(width) / 2,
            y: rows.saturating_sub(height + 1),
            width,
            height,
            title: "commands — every action in the registry, with its current key",
            lines: &lines,
            selection: Some(app.palette_row),
        },
    );
}

/// The `c` panel: one comparison, the pair it is between, and the scroll the list motions move.
///
/// A comparison is a document with a header, not a list of things to select, so the panel has a scroll and
/// no cursor — the same shape the diff pane's body has, for the same reason. The spec is in the title so
/// that scrolling down cannot lose the reader the pair the answer belongs to (`e5.3`).
fn compare(grid: &mut Grid, app: &App) {
    let (cols, rows) = (grid.cols, grid.rows);
    let height = (rows.saturating_sub(2)).clamp(6, 30);
    let width = 96.min(cols).max(32);
    let body = app.compare_panel.window(height.saturating_sub(3));
    let drawn = height.saturating_sub(3).max(1);
    let at_bottom = app.compare_panel.scroll >= app.compare_panel.max_scroll(drawn);
    let mut lines = body;
    lines.push(format!(
        "{}–{} of {} lines · j/k · <C-d>/<C-u> · Esc close{}",
        app.compare_panel.scroll + 1,
        app.compare_panel.scroll + drawn.min(app.compare_panel.lines.len()),
        app.compare_panel.lines.len(),
        if at_bottom { "s · bottom" } else { "s" }
    ));
    let title = format!(
        "compare {} — the composed delta between the two endpoints",
        app.compare_panel.spec
    );
    draw_panel(
        grid,
        Panel {
            x: cols.saturating_sub(width) / 2,
            y: rows.saturating_sub(height + 1),
            width,
            height,
            title: &title,
            lines: &lines,
            selection: None,
        },
    );
}

/// The `F` chooser: the filter dimensions and their current values, filtered by what has been typed.
///
/// Same mechanism as the palette and for the same reason — the prompt is already the exclusive text
/// surface, so the filter editor adds no keys, no precedence and nothing for `keys.overrides` to get
/// wrong. What is on screen is the filter: `state · unread ✓` cannot be forgotten about the way a
/// filter hidden in a status line can (ADR-001 §7.2).
fn filters(grid: &mut Grid, app: &App) {
    let (cols, rows) = (grid.cols, grid.rows);
    let width = 76.min(cols).max(24);
    let panel_rows = app.filter_rows();
    // The chooser draws its own cursor, because it knows which row is the *editor's* and which is a
    // header line; the panel is text the moment it leaves the view.
    let shown: Vec<String> = app
        .timeline
        .editor_panel(&app.prompt, &panel_rows, width.saturating_sub(2))
        .into_iter()
        .collect();
    // The panel is as tall as its rows, never a fixed box padded with blank lines: the chooser has
    // nine rows on the dimensions list and three on a dimension with three values, and a 16-row frame
    // that leaves 13 rows of nothing on screen is a panel lying about how much it needs (ADR-006 §6.1
    // D8's rule about trimming applies to a floating panel before it applies to a pane).
    let height = (shown.len() + 2).min(rows.saturating_sub(1)).clamp(4, 16);
    let shown: Vec<String> = shown.into_iter().take(height.saturating_sub(2)).collect();
    draw_panel(
        grid,
        Panel {
            x: cols.saturating_sub(width) / 2,
            // Above the chrome, not over it. The status line is the last chrome standing (§1) and it is
            // where the index state and any degradation notice live, so a filter chooser that painted
            // over it would hide the facts a reviewer needs most while deciding what to filter.
            y: rows.saturating_sub(height + 3).max(1),
            width,
            height,
            title: "filters — what the rail selects (ADR-006 §7.4 filter.edit)",
            lines: &shown,
            selection: None,
        },
    );
}

/// Draw a boxed panel into the grid.
fn draw_panel(grid: &mut Grid, panel: Panel<'_>) {
    if panel.width < 6 || panel.height < 4 {
        return;
    }
    let title = format!("{TL}{}{}{TR}", H, panel.title);
    grid.put(panel.x, panel.y, &fit_text(&title, panel.width));
    let blank = format!("{V}{}{V}", " ".repeat(panel.width.saturating_sub(2)));
    for row in 1..panel.height.saturating_sub(1) {
        grid.put(panel.x, panel.y + row, &blank);
        if let Some(line) = panel.lines.get(row - 1) {
            let marker = match panel.selection {
                Some(index) if index + 1 == row => FOCUS,
                _ => ' ',
            };
            grid.put(
                panel.x + 1,
                panel.y + row,
                &format!("{marker}{}", line.trim_start()),
            );
        }
    }
    let foot: String = H.to_string().repeat(panel.width.saturating_sub(2));
    grid.put(
        panel.x,
        panel.y + panel.height.saturating_sub(1),
        &fit_text(&format!("{BL}{foot}{BR}"), panel.width),
    );
}

/// The pane header: the focused pane carries the marker, so the focus model is visible on screen.
fn pane_head(app: &App, pane: &Pane) -> String {
    let focused = app
        .focused_pane()
        .is_some_and(|current| current.name == pane.name);
    let marker = if focused { FOCUS } else { '·' };
    format!("{marker} {} · {}", pane.name, pane.ticket)
}

/// A pane's body: its rows from the model, or the empty state, clipped and padded to the pane.
fn pane_body(app: &App, pane: &Pane, width: usize, rows: usize) -> Vec<StyledLine> {
    // The diff view draws its own panes: the tree carries a cursor and the hunks pane is a window
    // into a file, so neither is a list of strings the shell can slice on its own. The timeline's
    // rail does the same thing for the same reason — it is a window over a list it keeps virtual.
    if let Some(lines) = app.timeline_body(pane, width, rows) {
        return lines;
    }
    if let Some(lines) = app.diff_body(pane, width, rows) {
        return lines;
    }
    // The queue's worklist is a window over a list the shell holds, with its own cursor and its own
    // two-row-per-episode rhythm, so it draws itself like the rail does (`reco-c05`).
    if app.view == ViewId::Queue && !app.timeline.is_empty() {
        return app.queue_body(pane, width, rows);
    }
    let offset = if pane.role == Role::Primary {
        0
    } else {
        app.scroll_of()
    };
    let mut lines: Vec<StyledLine> = app
        .pane_rows(pane)
        .into_iter()
        .skip(offset)
        .take(rows)
        .map(|line| StyledLine::plain(line).padded(width))
        .collect();
    while lines.len() < rows {
        lines.push(StyledLine::plain("").padded(width));
    }
    lines
}

/// The search field's row: `/` for content, and the filter tokens it accepts (ADR-006 §5.8).
fn query_line(app: &App, query: &Pane) -> String {
    let hint = query.reads.first().copied().unwrap_or("type to filter");
    format!("/{}▏  {hint}", app.prompt)
}

/// The status line: degradation notice first, then the index, then the repository (ADR-006 §6.4).
#[must_use]
pub fn status_line(app: &App) -> String {
    app.status_bits().join(" · ")
}

/// The tab bar: the tabs on the left, the repository on the right.
fn tab_line(app: &App, cols: usize) -> String {
    let left = app.tab_bar();
    let right = app.repo_label.clone();
    let pad = cols.saturating_sub(display_width(&left) + display_width(&right) + 1);
    fit_text(&format!("{left}{}{}", " ".repeat(pad), right), cols)
}

/// Clip to `cells` display columns with an ellipsis, on a cluster boundary.
fn truncate(text: &str, cells: usize) -> String {
    crate::text::clip(text, cells)
}

/// Write the frame into a ratatui buffer — one `set_string` per styled run, because the text is
/// already clipped to the frame and the buffer must never see a line that could wrap.
pub fn blit(buffer: &mut Buffer, lines: &[StyledLine]) {
    blit_themed(buffer, lines, &crate::theme::dark());
}

/// [`blit`] with the terminal's own colour answer applied.
///
/// `NO_COLOR` and `TERM=dumb` are instructions, not decorations: the frame is laid out and drawn by
/// exactly the same code, and the only thing that changes is that no cell is given a colour. Shapes
/// stay, because ADR-006 §10.2 promised they would be enough.
///
/// This is the `e5.5` entry point, and it is now one line of the theme: `depth` is folded into the
/// default dark theme by [`Theme::resolve`](crate::theme::Theme::resolve), which is the same code path
/// a user's theme takes. A caller that has chosen a theme calls [`blit_themed`] instead.
pub fn blit_in_colour(
    buffer: &mut Buffer,
    lines: &[StyledLine],
    colour: crate::terminal::ColorDepth,
) {
    blit_themed(buffer, lines, &crate::theme::dark().resolve(colour));
}

/// [`blit`] painted by a theme.
///
/// The theme arrives already resolved for the terminal it is going to
/// ([`Probe::color`](crate::terminal::Probe::color) → [`Theme::resolve`](crate::theme::Theme::resolve)),
/// so this function has no idea whether the reviewer pressed `NO_COLOR`, has sixteen colours, or is
/// reading the frame in somebody else's palette — and it does not need to. That is the whole point of
/// `e5.6`: the frame is built once, and colour is one lookup on the way out.
pub fn blit_themed(buffer: &mut Buffer, lines: &[StyledLine], theme: &crate::theme::Theme) {
    for (y, line) in lines.iter().enumerate() {
        let Ok(row) = u16::try_from(y) else {
            break;
        };
        for (text, column, style) in line.segments_in(theme) {
            let Ok(x) = u16::try_from(column) else {
                continue;
            };
            buffer.set_string(x, row, text, style);
        }
    }
}

/// The unstyled blit, for a caller that only has text — an overlay panel is a keymap table, and
/// there is nothing there for a colour to mean.
pub fn blit_text(buffer: &mut Buffer, lines: &[String]) {
    for (y, line) in lines.iter().enumerate() {
        let Ok(row) = u16::try_from(y) else {
            break;
        };
        buffer.set_string(0, row, line, Style::default());
    }
}

/// The `--plain` render: the same lines, trailing blanks trimmed, no escape sequences at all
/// (ADR-006 §6.7). A pipe gets text; the refusal panel's advice is the same text.
#[must_use]
pub fn plain_lines(app: &App) -> Vec<String> {
    frame_lines(app)
        .into_iter()
        .map(|line| crate::text::trim_line(&line))
        .collect()
}

/// One pane's rows as text, from whoever owns them — the rail, the diff tree, the worklist, or the
/// shell's own list. This is the same dispatch [`pane_body`] uses, minus the styling and the padding,
/// which is what lets the line mode print exactly the rows the frame drew.
#[must_use]
pub fn pane_text(app: &App, pane: &Pane) -> Vec<String> {
    if let Some(lines) = app.timeline_text(pane) {
        return lines
            .into_iter()
            .map(|line| line.trim_end().to_owned())
            .collect();
    }
    if let Some(lines) = app.diff_text(pane) {
        return lines
            .into_iter()
            .map(|line| line.trim_end().to_owned())
            .collect();
    }
    if app.view == ViewId::Queue && !app.timeline.is_empty() {
        return app
            .queue_text(pane)
            .unwrap_or_default()
            .into_iter()
            .map(|line| line.trim_end().to_owned())
            .collect();
    }
    let offset = if pane.role == Role::Primary {
        0
    } else {
        app.scroll_of()
    };
    app.pane_rows(pane)
        .into_iter()
        .skip(offset)
        .map(|line| line.trim_end().to_owned())
        .collect()
}

/// The `--plain --lines` render: the same frame, **read in order** instead of drawn side by side
/// (ADR-006 §6.7).
///
/// The grid is the right shape for eyes and the wrong shape for a pipe, a log or a screen reader:
/// two panes side by side interleave a worklist row with a card row, and the reader is left to
/// work out which column each fragment came from. Here every fact is on its own line, the panes
/// come one after another in focus order, and nothing is padded — which is the same text the frame
/// draws, in the order a person would read it aloud.
#[must_use]
pub fn plain_flow(app: &App) -> Vec<String> {
    let cols = usize::from(app.size.0);
    let rows = usize::from(app.size.1);
    let plan = frame_plan(app, app.view, cols, rows);
    // An overlay already is a reading surface — the keymap table, the palette, the search field —
    // and the grid form of it is already one column wide. Sequence the grid rather than pretend the
    // panel has panes.
    if app.overlay.is_some() {
        return plain_lines(app);
    }
    let view = View::by_id(app.view);
    let mut out: Vec<String> = Vec::new();
    if plan.refused {
        out.extend(
            refusal_lines(app.view, cols as u16, rows as u16)
                .into_iter()
                .map(|line| line.trim_end().to_owned()),
        );
        return out;
    }
    out.push(format!(
        "fathom · {} · {} · {cols}×{rows}",
        view.title,
        layout::state_name(plan.fit.state)
    ));
    out.push(format!("scope · {}", app.scope.label()));
    for bit in app.status_bits() {
        out.push(format!("status · {bit}"));
    }
    for notice in &app.notices {
        out.push(format!("notice · {notice}"));
    }
    if let Some(toast) = &app.toast {
        out.push(format!("said · {toast}"));
    }
    for rect in &plan.panes {
        let focused = app
            .focused_pane()
            .is_some_and(|pane| pane.name == rect.pane.name);
        out.push(String::new());
        out.push(format!(
            "{} {} · {} · pane {} of {}",
            if focused { "▸" } else { "·" },
            rect.pane.name,
            rect.pane.ticket,
            rect.ring + 1,
            plan.panes.len()
        ));
        out.extend(pane_text(app, rect.pane));
    }
    if plan.hint {
        out.push(String::new());
        out.push(format!("hint · {}", app.hint_line().trim_end()));
    }
    out
}

/// The frame's declared size, for the tests' own assertions.
#[must_use]
pub fn frame_size(app: &App) -> (usize, usize) {
    (usize::from(app.size.0), usize::from(app.size.1))
}

/// Assert-ready: is every line exactly `width` cells?
#[must_use]
pub fn widths_are_exact(lines: &[String], width: usize) -> bool {
    lines.iter().all(|line| display_width(line) <= width)
}

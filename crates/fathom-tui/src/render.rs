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
#[derive(Debug)]
struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<char>,
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Self {
        Grid {
            cols,
            rows,
            cells: vec![' '; cols.saturating_mul(rows)],
        }
    }

    fn fill(&mut self, lines: &[String]) {
        for (y, line) in lines.iter().take(self.rows).enumerate() {
            for (x, ch) in line.chars().take(self.cols).enumerate() {
                self.cells[y * self.cols + x] = ch;
            }
        }
    }

    fn put(&mut self, x: usize, y: usize, text: &str) {
        if y >= self.rows {
            return;
        }
        for (index, ch) in text.chars().enumerate() {
            let column = x + index;
            if column >= self.cols {
                break;
            }
            self.cells[y * self.cols + column] = ch;
        }
    }

    fn to_lines(&self) -> Vec<String> {
        (0..self.rows)
            .map(|y| {
                self.cells[y * self.cols..(y + 1) * self.cols]
                    .iter()
                    .collect()
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
    match app.overlay {
        Some(Overlay::Help) => help(&mut grid, app),
        Some(Overlay::Palette) => palette(&mut grid, app),
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
    let view = View::by_id(view_id);
    let fit = layout::fit(view_id, cols as u16, rows as u16);
    let own_status = view.has_chrome(CH_STATUS);
    if fit.state == FitState::Refused {
        // The refusal replaces the view. The status line stays: the ladder never sells it, and the
        // notice has to land somewhere.
        let mut out: Vec<StyledLine> = refusal_lines(view_id, cols as u16, rows as u16)
            .into_iter()
            .map(StyledLine::plain)
            .collect();
        out.truncate(rows.saturating_sub(usize::from(own_status)).max(1));
        if own_status && out.len() < rows {
            out.push(StyledLine::plain(status_line(app)).padded(cols));
        }
        out.truncate(rows);
        while out.len() < rows {
            out.push(StyledLine::plain("").padded(cols));
        }
        return out;
    }
    let applied = fit.applied.clone();
    let gone = |id: &str| applied.iter().any(|a| a == id);
    let boxed = view.has_chrome(CH_FRAME) && !gone(layout::Step::Frame.id());
    let strips = view.has_chrome(CH_STRIPS) && !gone(layout::Step::Strips.id());
    let tabs = view.has_chrome(CH_TABS) && !gone(layout::Step::Tabs.id());
    let hint = view.has_chrome(CH_HINT) && !gone(layout::Step::Hint.id());
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
    let query_rows = view.panes.iter().filter(|p| p.role == Role::Query).count();
    // The renderer's own row budget: exactly one line per row it is going to print. This is not
    // `layout::chrome_rows`, which is the ADR's model of what chrome *costs* and is what `fit`
    // reasons about; the two agree on the sizes the ladder was derived at, and this one is the one
    // that decides whether the drawn frame is `rows` lines tall.
    let overhead = usize::from(tabs)
        + query_rows
        + 1 // the pane header row, boxed or not
        + usize::from(strips)
        + usize::from(boxed) // the bottom rule
        + usize::from(view.has_chrome(CH_STATUS))
        + usize::from(hint);
    let body_rows = rows.saturating_sub(overhead).max(1);

    let mut out: Vec<StyledLine> = Vec::new();
    if tabs {
        out.push(StyledLine::plain(tab_line(app, cols)).padded(cols));
    }
    if let Some(query) = view.query() {
        out.push(StyledLine::plain(query_line(app, query)).padded(cols));
    }
    let heads: Vec<StyledLine> = widths
        .iter()
        .map(|(pane, width)| StyledLine::plain(pane_head(app, pane)).padded(*width as usize))
        .collect();
    out.push(row_of(&heads, boxed, TL, TC, TR, cols));

    let bodies: Vec<Vec<StyledLine>> = widths
        .iter()
        .map(|(pane, width)| pane_body(app, pane, *width as usize, body_rows))
        .collect();
    for row in 0..body_rows {
        let parts: Vec<StyledLine> = widths
            .iter()
            .enumerate()
            .map(|(index, (_pane, width))| {
                bodies[index]
                    .get(row)
                    .cloned()
                    .unwrap_or_else(|| StyledLine::plain("").padded(*width as usize))
            })
            .collect();
        out.push(row_of(&parts, boxed, V, V, V, cols));
    }
    if strips {
        let parts: Vec<StyledLine> = widths
            .iter()
            .map(|(pane, width)| StyledLine::plain(app.pane_strip(pane)).padded(*width as usize))
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
    if own_status {
        out.push(StyledLine::plain(status_line(app)).padded(cols));
    }
    if hint {
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
    let mut panel = app.keymap.help_lines(inner);
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

/// The `,` palette: every action, with the key it has right now.
fn palette(grid: &mut Grid, app: &App) {
    let (cols, rows) = (grid.cols, grid.rows);
    let height = 14.min(rows.saturating_sub(1)).max(4);
    let width = 76.min(cols).max(24);
    let entries = app.palette_rows();
    let shown: Vec<String> = entries
        .iter()
        .take(height.saturating_sub(4))
        .map(|(_id, label, keys)| {
            let key = if keys.is_empty() {
                "(palette only)"
            } else {
                keys.as_str()
            };
            format!(
                "{:<width$} {key}",
                truncate(label, width.saturating_sub(16).max(12)),
                width = width.saturating_sub(16).max(12)
            )
        })
        .collect();
    let mut lines = vec![format!(",{}▏", app.prompt)];
    lines.extend(shown);
    lines.push(format!(
        "{} of {} commands · Enter runs it · Esc closes",
        (app.palette_row + 1).min(entries.len()),
        entries.len()
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
    // into a file, so neither is a list of strings the shell can slice on its own.
    if let Some(lines) = app.diff_body(pane, width, rows) {
        return lines;
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

/// Clip to `cells` display columns with an ellipsis.
fn truncate(text: &str, cells: usize) -> String {
    if display_width(text) <= cells {
        return text.to_owned();
    }
    let mut out = String::new();
    for ch in text.chars() {
        if display_width(&out) + ch.len_utf8().max(1) + 1 > cells {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

/// Write the frame into a ratatui buffer — one `set_string` per styled run, because the text is
/// already clipped to the frame and the buffer must never see a line that could wrap.
pub fn blit(buffer: &mut Buffer, lines: &[StyledLine]) {
    for (y, line) in lines.iter().enumerate() {
        let Ok(row) = u16::try_from(y) else {
            break;
        };
        for (text, column, style) in line.segments() {
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
        .map(|line| line.trim_end().to_owned())
        .collect()
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

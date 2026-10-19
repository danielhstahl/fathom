//! `fathom_tui::layout` — the sizing contract and **the degradation ladder**.
//!
//! ADR-006 §3 fixes four numbers per view and §6 fixes one ladder; this module *derives* all of
//! them from the pane table in [`crate::views`], which is the difference between a size contract
//! and a paragraph about one. `tests/layout.rs` asserts the derived numbers against the ones
//! ADR-006 §3 and §6.2 publish, so a widened pane cannot leave the published minimum lying.
//!
//! The ladder is one ordered sequence applied to every view (ADR-006 §6.1):
//!
//! ```text
//! D1 hint line     D2 tab bar        D3 pane strips
//! D4 box frame     D5 secondary pane -> strip   D6 panes to their hard minimum
//! D7 compact rows  D8 trimmable body rows
//! D9 single-pane fallback (secondary panes leave the frame)
//! D10 refuse, and say so
//! ```
//!
//! Two rules hold this together (ADR-001 §7.2): every step that removes content is **announced**
//! on the status line, and nothing is ever clipped below a pane's hard minimum — a worklist that
//! cannot show `sha · age · author · +n −n · risk` is not a narrow worklist, it is an ellipsis
//! with a header, and the honest answer is the refusal panel in [`refusal_lines`].

use crate::views::{CH_FRAME, CH_HINT, CH_STATUS, CH_STRIPS, CH_TABS, Pane, Role, View, ViewId};

/// One cell of a pane border.
const FRAME_COLS: u16 = 1;
/// Columns between two panes — one column rule, boxed or not.
const GUTTER: u16 = 1;
/// Ambient has no frame to sell, so its refusal floor is its own line budget: the longest line
/// it may emit is 40 columns, which is what ADR-001 §4.1's "fits a 40×3 tmux pane" promise costs.
const AMBIENT_REFUSE_COLS: u16 = 40;
/// Columns a status line must have before it can afford to spell the degradation notice out in
/// full. Below this the notice counts the drops rather than being clipped mid-word (ADR-006 §6.4).
pub const NOTICE_COLS: usize = 96;

// ---------------------------------------------------------------------------
// display width
// ---------------------------------------------------------------------------
/// Display cells a string occupies. Combining marks are free and wide characters cost two, which
/// is the part the layout engine may not get wrong: a box one cell wider than the terminal wraps,
/// and a wrapped box is the whole design shifted by one.
#[must_use]
pub fn display_width(text: &str) -> usize {
    crate::text::width(text)
}

/// Truncate to `width` cells without splitting a grapheme run, then pad to exactly `width`.
///
/// The ellipsis is one cell, and the padding is why every drawn line is exactly the frame width:
/// it makes "did this clip?" a string comparison instead of a screenshot review. Cutting on a
/// cluster boundary is what stops `é` (two characters, one cluster) and a ZWJ emoji family (five
/// characters, one cluster) from being sliced into a different character and an early ellipsis.
///
/// The arithmetic itself lives in [`crate::text`], one place for the whole crate, so a measured
/// width and the string it measured can never disagree.
#[must_use]
pub fn fit_text(text: &str, width: usize) -> String {
    crate::text::fit(text, width)
}

// ---------------------------------------------------------------------------
// the ladder
// ---------------------------------------------------------------------------
/// One rung of the ladder, with the rows and columns it buys back for a given view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Sell the hint line.
    Hint,
    /// Sell the tab bar.
    Tabs,
    /// Sell the pane footer strips.
    Strips,
    /// Sell the box frame.
    Frame,
    /// Squeeze the secondary panes to their headline strip width.
    SecondaryStrip,
    /// Push panes below their documented minimum, down to their hard minimum.
    ColsHard,
    /// Compact rows in the primary list.
    CompactRows,
    /// Drop trimmable body rows (glance card, previews).
    ContentTrim,
}

impl Step {
    /// The step's id as the ADR and the per-view table print it.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        match self {
            Step::Hint => "D1.hint",
            Step::Tabs => "D2.tabs",
            Step::Strips => "D3.strips",
            Step::Frame => "D4.frame",
            Step::SecondaryStrip => "D5.secondary_strip",
            Step::ColsHard => "D6.cols_hard",
            Step::CompactRows => "D7.compact_rows",
            Step::ContentTrim => "D8.content_trim",
        }
    }

    /// What leaves the frame, in the ADR's words (ADR-006 §6.1).
    #[must_use]
    pub const fn what(&self) -> &'static str {
        match self {
            Step::Hint => "hint line (the next-action reminder)",
            Step::Tabs => "tab bar",
            Step::Strips => "pane footer strips (legend, counts, ordering explanation)",
            Step::Frame => "box frame: side borders and the bottom rule",
            Step::SecondaryStrip => "secondary panes squeezed to their strip width",
            Step::ColsHard => "panes below their documented minimum, to the hard minimum",
            Step::CompactRows => "compact rows in the primary list (one line per item)",
            Step::ContentTrim => "trimmable body lines (glance mode, previews)",
        }
    }

    /// Why this step, in this place — the ADR's justification column, kept next to the number.
    #[must_use]
    pub const fn why(&self) -> &'static str {
        match self {
            Step::Hint => {
                "It is the only line a learned user stopped reading, and the one line \
                           that carries no data."
            }
            Step::Tabs => "`J`/`K` and `<C-w>1..9` still reach every tab, so the bar is a label.",
            Step::Strips => {
                "Legends are learnable once; the strip re-appears with `?` and in the \
                             export."
            }
            Step::Frame => {
                "Titles plus a column rule keep panes distinguishable without a full \
                            box."
            }
            Step::SecondaryStrip => {
                "The secondary pane answers a follow-up question; a headline \
                                     strip keeps it answerable, and `<C-w>l` gives it the frame."
            }
            Step::ColsHard => {
                "Between ideal and minimum a pane simply narrows; below minimum it \
                               starts dropping columns of its own."
            }
            Step::CompactRows => {
                "Buys three more visible episodes; the sparkline and relative \
                                  time give way first."
            }
            Step::ContentTrim => {
                "Only panes that declare trimmable rows may use this: a glance \
                                  card can show four facts instead of seven."
            }
        }
    }

    /// The ticket that owns this step's behaviour on screen.
    #[must_use]
    pub const fn ticket(&self) -> &'static str {
        match self {
            Step::Hint => "e5.4",
            Step::Tabs => "e1.5",
            Step::Strips
            | Step::Frame
            | Step::SecondaryStrip
            | Step::ColsHard
            | Step::CompactRows => "e5.5",
            Step::ContentTrim => "e5.1",
        }
    }

    /// `(columns, rows)` this step buys back for `view`, in frame cells.
    ///
    /// Every number is a fact about the chrome or pane table, never a guess: `D3` is worth one
    /// row because the footer strips of both panes are drawn side by side, and a borderless view
    /// cannot sell what it never had — which is why ambient's ladder is one step long.
    #[must_use]
    pub fn recover(&self, view: &View) -> (u16, u16) {
        match self {
            Step::Hint => (0, u16::from(view.has_chrome(CH_HINT))),
            Step::Tabs => (0, u16::from(view.has_chrome(CH_TABS))),
            Step::Strips => {
                if !view.has_chrome(CH_STRIPS) || !view.panes.iter().any(|p| p.strip) {
                    (0, 0)
                } else {
                    (0, 1)
                }
            }
            Step::Frame => {
                if view.has_chrome(CH_FRAME) {
                    (2, 1)
                } else {
                    (0, 0)
                }
            }
            Step::SecondaryStrip => (
                view.secondaries()
                    .iter()
                    .map(|p| p.min_cols.saturating_sub(p.strip_width))
                    .sum(),
                0,
            ),
            Step::ColsHard => (
                view.panes
                    .iter()
                    .filter(|p| p.role != Role::Query)
                    .map(|p| p.min_cols.saturating_sub(p.hard_min))
                    .sum(),
                0,
            ),
            Step::CompactRows => (0, 3),
            Step::ContentTrim => (0, view.panes.iter().map(|p| p.trim_rows).max().unwrap_or(0)),
        }
    }

    /// This step's `what`, specialised to the view: `D5`'s strip width is each pane's own, so one
    /// shared string would be a made-up number (ADR-006 §6.2 prints the real one).
    #[must_use]
    pub fn what_for(&self, view: &View) -> String {
        if *self == Step::SecondaryStrip {
            let mut widths: Vec<u16> = view.secondaries().iter().map(|p| p.strip_width).collect();
            if widths.is_empty() {
                return "no secondary pane: this step cannot fire here".to_owned();
            }
            widths.sort_unstable();
            widths.dedup();
            return format!(
                "secondary pane{} squeezed to a {}-column strip",
                if widths.len() > 1 { "s" } else { "" },
                widths
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join("/")
            );
        }
        (*self).what().to_owned()
    }
}

/// The order, fixed once so no view can invent a friendlier one for itself (ADR-006 §6.1).
pub const LADDER: [Step; 8] = [
    Step::Hint,
    Step::Tabs,
    Step::Strips,
    Step::Frame,
    Step::SecondaryStrip,
    Step::ColsHard,
    Step::CompactRows,
    Step::ContentTrim,
];

/// The single-pane fallback: a state, not a size step, and a loud one (ADR-006 §6.1 D9).
pub const FALLBACK: &str = "D9.single_pane";
/// The refusal: below this the view is not drawn at all (ADR-006 §6.1 D10).
pub const REFUSED: &str = "D10.refuse";

/// What the ladder decided about a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitState {
    /// Every documented pane at its documented minimum.
    Complete,
    /// Some chrome or width was sold; the status line says what (ADR-006 §6.4).
    Degraded,
    /// The secondary panes left the frame; `<C-w>l` still reaches them.
    Single,
    /// Not drawn at all: the refusal panel replaces it (ADR-006 §6.6).
    Refused,
}

/// The outcome of asking the ladder whether a view fits a size.
#[derive(Debug, Clone)]
pub struct Fit {
    /// The view that was asked.
    pub view: ViewId,
    /// Columns it was given.
    pub cols: u16,
    /// Rows it was given.
    pub rows: u16,
    /// Step ids that fired, in ladder order, with [`FALLBACK`] and [`REFUSED`] appended when
    /// either state was reached.
    pub applied: Vec<String>,
    /// The state the ladder landed in.
    pub state: FitState,
    /// Columns and rows still short after everything — a stated partial, not a clipped frame.
    pub deficit: (u16, u16),
}

impl Fit {
    /// What left the frame, in the order the ladder sold it, in this view's own numbers.
    #[must_use]
    pub fn dropped(&self) -> Vec<String> {
        let view = crate::views::view(self.view);
        let mut out: Vec<String> = LADDER
            .iter()
            .filter(|s| self.applied.iter().any(|a| a == s.id()))
            .map(|s| s.what_for(view))
            .collect();
        if self.applied.iter().any(|a| a == FALLBACK) {
            out.push("single-pane fallback: secondary panes dropped".to_owned());
        }
        out
    }

    /// The status-line notice. ADR-001 §7.2: suppressed information is the one failure a review
    /// tool is not allowed, so nothing the ladder sells may go unsaid.
    #[must_use]
    pub fn notice(&self) -> String {
        let view = crate::views::view(self.view);
        match self.state {
            FitState::Complete => String::new(),
            FitState::Refused => format!(
                "LAYOUT REFUSED: {} needs {}×{} minimum, got {}×{} — `a` ambient, `q` queue, \
                 or `fathom --plain`",
                view.title,
                refuse_below_cols(view),
                refuse_below_rows(view),
                self.cols,
                self.rows
            ),
            _ => format!(
                "LAYOUT {}: {} · <C-w>o",
                state_name(self.state),
                self.dropped().join(", ")
            ),
        }
    }

    /// One-line summary for the tab bar and the tests.
    #[must_use]
    pub fn summary(&self) -> String {
        let view = crate::views::view(self.view);
        match self.state {
            FitState::Complete => "complete".to_owned(),
            FitState::Refused => format!(
                "refused below {}×{}",
                refuse_below_cols(view),
                refuse_below_rows(view)
            ),
            _ => format!("{} — {}", state_name(self.state), self.dropped().join(", ")),
        }
    }

    /// Is the view drawn at all?
    #[must_use]
    pub fn is_drawn(&self) -> bool {
        self.state != FitState::Refused
    }
}

/// Columns the frame itself costs right now: two while a box is drawn, zero once the box is sold
/// — and zero always for a borderless view, which never had them.
#[must_use]
pub fn frame_cols(view: &View, applied: &[String]) -> u16 {
    if !view.has_chrome(CH_FRAME) || applied.iter().any(|a| a == Step::Frame.id()) {
        0
    } else {
        2 * FRAME_COLS
    }
}

/// Chrome rows still standing after the listed steps (ADR-006 §5's chrome column).
///
/// The status line is never a ladder candidate: it is where the degradation notice lives. A boxed
/// frame is a top rule and a bottom rule; after `D4` the top rule survives as the pane header,
/// which is the frame's one remaining row.
#[must_use]
pub fn chrome_rows(view: &View, applied: &[String]) -> u16 {
    let gone = |id: &str| applied.iter().any(|a| a == id);
    let mut rows = 0;
    if view.has_chrome(CH_FRAME) {
        rows += if gone(Step::Frame.id()) { 1 } else { 2 };
    }
    if view.has_chrome(CH_TABS) && !gone(Step::Tabs.id()) {
        rows += 1;
    }
    if view.has_chrome(CH_STRIPS) && !gone(Step::Strips.id()) {
        rows += 1;
    }
    if view.has_chrome(CH_HINT) && !gone(Step::Hint.id()) {
        rows += 1;
    }
    if view.has_chrome(CH_STATUS) {
        rows += 1;
    }
    rows
}

/// The width at which the pane set still renders *as documented*: every pane at its documented
/// minimum. Derived, never declared, and honest about the frame (§3).
#[must_use]
pub fn min_cols_view(view: &View) -> u16 {
    let panes: Vec<&Pane> = view
        .panes
        .iter()
        .filter(|p| p.role != Role::Query)
        .collect();
    let sum: u16 = panes.iter().map(|p| p.min_cols).sum();
    sum + frame_cols(view, &[]) + ((panes.len().saturating_sub(1)) as u16) * GUTTER
}

/// Below this width the view refuses: the primary pane's hard minimum, taken **without** its
/// frame, because the frame was sold two steps earlier on the ladder (ADR-006 §6.6).
#[must_use]
pub fn refuse_below_cols(view: &View) -> u16 {
    if view.id == "ambient" {
        return AMBIENT_REFUSE_COLS;
    }
    view.primary().hard_min
}

/// Below this many frame rows the view refuses: the chrome left standing after the ladder (the
/// status line, one row) plus the view's own floor for content.
#[must_use]
pub fn refuse_below_rows(view: &View) -> u16 {
    1 + view.min_content_rows
}

/// The narrowest width at which the documented pane set still renders, `D9` not needed.
#[must_use]
pub fn floor_cols(view: &View) -> u16 {
    let sold: u16 = LADDER.iter().map(|s| s.recover(view).0).sum();
    min_cols_view(view)
        .saturating_sub(sold)
        .max(refuse_below_cols(view) + 1)
}

/// The shortest height at which the documented pane set still renders.
#[must_use]
pub fn floor_rows(view: &View) -> u16 {
    let sold: u16 = LADDER.iter().map(|s| s.recover(view).1).sum();
    view.min_rows
        .saturating_sub(sold)
        .max(refuse_below_rows(view))
}

/// Ask the ladder, in order, until the view fits. `e1.5` calls exactly this on every resize, and
/// ADR-006 §6.2's per-view table is its output.
#[must_use]
pub fn fit(view_id: ViewId, cols: u16, rows: u16) -> Fit {
    let view = crate::views::view(view_id);
    let mut out = Fit {
        view: view_id,
        cols,
        rows,
        applied: Vec::new(),
        state: FitState::Complete,
        deficit: (0, 0),
    };
    let need_c = i32::from(min_cols_view(view)) - i32::from(cols);
    let need_r = i32::from(view.min_rows) - i32::from(rows);
    if need_c <= 0 && need_r <= 0 {
        return out;
    }
    let mut need_c = need_c;
    let mut need_r = need_r;
    for step in &LADDER {
        if need_c <= 0 && need_r <= 0 {
            break;
        }
        let (saved_c, saved_r) = step.recover(view);
        if (saved_c > 0 && need_c > 0) || (saved_r > 0 && need_r > 0) {
            out.applied.push(step.id().to_owned());
            need_c -= i32::from(saved_c);
            need_r -= i32::from(saved_r);
        }
    }
    if need_c > 0 || need_r > 0 {
        let saved_c: i32 = view
            .secondaries()
            .iter()
            .map(|p| i32::from(p.strip_width) + i32::from(GUTTER))
            .sum();
        if saved_c > 0 {
            out.applied.push(FALLBACK.to_owned());
            need_c -= saved_c;
            out.state = FitState::Single;
        }
    }
    let avail_c = i32::from(cols) - i32::from(frame_cols(view, &out.applied));
    let avail_r = i32::from(rows) - i32::from(chrome_rows(view, &out.applied));
    out.deficit = (
        u16::try_from(need_c.max(0)).unwrap_or(u16::MAX),
        u16::try_from(need_r.max(0)).unwrap_or(u16::MAX),
    );
    if avail_c < i32::from(view.primary().hard_min) || avail_r < i32::from(view.min_content_rows) {
        out.state = FitState::Refused;
        out.applied.push(REFUSED.to_owned());
    } else if out.state != FitState::Single {
        out.state = FitState::Degraded;
    }
    out
}

/// The size at which each step first fires, computed by asking [`fit`] — which is the only way
/// nine views' worth of "rows ≤ 17" stays true (ADR-006 §6.2).
#[must_use]
pub fn ladder_boundary(view_id: ViewId) -> Vec<(String, String)> {
    let view = crate::views::view(view_id);
    let mut out = Vec::new();
    for step in &LADDER {
        let at_c = first_by_cols(view_id, step.id());
        let at_r = first_by_rows(view_id, step.id());
        let mut bits = Vec::new();
        if let Some(c) = at_c {
            bits.push(format!("cols ≤ {c}"));
        }
        if let Some(r) = at_r {
            bits.push(format!("rows ≤ {r}"));
        }
        out.push((
            step.id().to_owned(),
            if bits.is_empty() {
                "—".to_owned()
            } else {
                bits.join(", ")
            },
        ));
    }
    out.push((
        FALLBACK.to_owned(),
        match first_by_cols(view_id, FALLBACK) {
            Some(c) => format!("cols ≤ {c}"),
            None => "—".to_owned(),
        },
    ));
    debug_assert!(!view.panes.is_empty());
    out
}

/// The largest width at which `step_id` is part of the answer, holding height at the ideal.
#[must_use]
pub fn first_by_cols(view_id: ViewId, step_id: &str) -> Option<u16> {
    let view = crate::views::view(view_id);
    (refuse_below_cols(view)..=view.ideal_cols).rev().find(|c| {
        fit(view_id, *c, view.ideal_rows)
            .applied
            .iter()
            .any(|a| a == step_id)
    })
}

/// The largest height at which `step_id` is part of the answer, holding width at the ideal.
#[must_use]
pub fn first_by_rows(view_id: ViewId, step_id: &str) -> Option<u16> {
    let view = crate::views::view(view_id);
    (refuse_below_rows(view)..=view.ideal_rows).rev().find(|r| {
        fit(view_id, view.ideal_cols, *r)
            .applied
            .iter()
            .any(|a| a == step_id)
    })
}

/// Pane widths at a size, given the steps that fired.
///
/// Shrink order is secondaries first, then the primary; growth follows ADR-006 §3: spare columns
/// go to the primary until it is twice the widest secondary, then split evenly. "It just
/// stretches" is not a rule, and at 240 columns the difference is the whole design.
#[must_use]
pub fn pane_widths(view: ViewId, cols: u16, applied: &[String]) -> Vec<(&'static Pane, u16)> {
    let view = crate::views::view(view);
    let panes: Vec<&'static Pane> = view
        .panes
        .iter()
        .filter(|p| p.role != Role::Query)
        .collect();
    let frame = frame_cols(view, applied);
    if applied.iter().any(|a| a == FALLBACK) {
        return vec![(panes[0], cols.saturating_sub(frame).max(1))];
    }
    let avail = i32::from(cols)
        - i32::from(frame)
        - i32::from((panes.len().saturating_sub(1)) as u16) * i32::from(GUTTER);
    let hard = applied.iter().any(|a| a == Step::ColsHard.id());
    let strip = applied.iter().any(|a| a == Step::SecondaryStrip.id());
    let mut targets: Vec<i32> = panes
        .iter()
        .map(|p| {
            let mut t = i32::from(p.ideal_cols);
            if strip && p.role == Role::Secondary {
                t = t.min(i32::from(p.strip_width));
            }
            t.min(i32::from(p.ideal_cols))
        })
        .collect();
    let floors: Vec<i32> = panes
        .iter()
        .map(|p| {
            if strip && p.role == Role::Secondary {
                i32::from(p.strip_width)
            } else if hard {
                i32::from(p.hard_min)
            } else {
                i32::from(p.min_cols)
            }
        })
        .collect();
    // Shrink: the widest pane that is still above its floor gives up one column at a time,
    // secondaries before the primary.
    let mut guard = 0;
    while targets.iter().sum::<i32>() > avail && guard < 4096 {
        guard += 1;
        let order = order_indices(&panes, &targets);
        let mut moved = false;
        for i in order {
            if targets[i] > floors[i] {
                targets[i] -= 1;
                moved = true;
                break;
            }
        }
        if !moved {
            break;
        }
    }
    // Last resort: the floors themselves do not fit (a 40-column Diff frame wants a 24-column strip
    // and an 18-column tree, and the frame is 39 wide). Something has to give, and the ADR's rule is
    // that the rightmost pane gives first, because that is the one the eye is not on, and a cramped
    // frame is better than a `Refused` one (§6.1). Never below one column.
    let mut guard = 0;
    while targets.iter().sum::<i32>() > avail && guard < 4096 {
        guard += 1;
        let mut shrunk = false;
        for i in (0..targets.len()).rev() {
            if targets[i] > 1 {
                targets[i] -= 1;
                shrunk = true;
                break;
            }
        }
        if !shrunk {
            break;
        }
    }
    // Grow.
    let mut guard = 0;
    while targets.iter().sum::<i32>() < avail && guard < 4096 {
        guard += 1;
        let widest = panes
            .iter()
            .zip(&targets)
            .filter(|(p, _)| p.role != Role::Primary)
            .map(|(_, t)| *t)
            .max()
            .unwrap_or(0);
        let primary = panes
            .iter()
            .position(|p| p.role == Role::Primary)
            .unwrap_or(0);
        if widest > 0 && targets[primary] < 2 * widest {
            targets[primary] += 1;
        } else {
            // Split the remainder evenly, one column at a time, left to right.
            let count = targets.len().max(1);
            let mut i = 0;
            while targets.iter().sum::<i32>() < avail {
                targets[i % count] += 1;
                i += 1;
            }
        }
    }
    panes
        .iter()
        .zip(targets)
        .map(|(pane, t)| (*pane, u16::try_from(t.max(1)).unwrap_or(1)))
        .collect()
}

/// Shrink order: secondaries first (widest first), then the primary (widest first).
fn order_indices(panes: &[&Pane], targets: &[i32]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..panes.len()).collect();
    idx.sort_by_key(|i| {
        (
            if panes[*i].role == Role::Secondary {
                0
            } else {
                1
            },
            -targets[*i],
        )
    });
    idx
}

/// The refusal panel (ADR-006 §6.4, §6.6): the size it needs, the size it got, and the two ways
/// out. Short lines on purpose — a refusal that truncates its own advice is worse than none.
#[must_use]
pub fn refusal_lines(view_id: ViewId, cols: u16, rows: u16) -> Vec<String> {
    let view = crate::views::view(view_id);
    let ambient = crate::views::view(ViewId::Ambient);
    let lines = [
        format!("too small for {}", view.title),
        format!(
            "need {}×{}, have {}×{}",
            refuse_below_cols(view),
            refuse_below_rows(view),
            cols,
            rows
        ),
        String::new(),
        format!(
            "`a` ambient ({}×{})",
            refuse_below_cols(ambient),
            refuse_below_rows(ambient)
        ),
        "`q` queue · `,` commands".to_owned(),
        "`fathom --plain` for text".to_owned(),
        String::new(),
        "nothing was clipped.".to_owned(),
    ];
    let mut out: Vec<String> = lines
        .iter()
        .take(usize::from(rows.max(1)))
        .map(|l| fit_text(l, usize::from(cols)))
        .collect();
    while out.len() < usize::from(rows) {
        out.push(fit_text("", usize::from(cols)));
    }
    out
}

/// The degraded status-line notice, abbreviated when the width cannot afford the list
/// (ADR-006 §6.4: the notice leads the status line, because the status line is the chrome that
/// outlives every other and the first thing a naive renderer would clip).
#[must_use]
pub fn degraded_notice(fit: &Fit, cols: usize) -> String {
    if fit.state == FitState::Complete {
        return String::new();
    }
    if fit.state == FitState::Refused {
        return fit.notice();
    }
    let full = format!(
        "LAYOUT {}: {} · <C-w>o, ?",
        state_name(fit.state),
        fit.dropped().join(", ")
    );
    if display_width(&full) + 20 <= cols {
        full
    } else {
        format!(
            "LAYOUT {} · {} dropped · <C-w>o, ?",
            state_name(fit.state),
            fit.applied.len()
        )
    }
}

/// `FitState` printed the way the ADR's matrix spells it (`complete | degraded | single | refused`).
#[must_use]
pub const fn state_name(state: FitState) -> &'static str {
    match state {
        FitState::Complete => "complete",
        FitState::Degraded => "degraded",
        FitState::Single => "single",
        FitState::Refused => "refused",
    }
}

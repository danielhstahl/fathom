//! The degradation ladder, the refusal floors and the width arithmetic.
//!
//! ADR-006 §6 is a set of *numbers*, and the ADR publishes them as generated tables. The tests here
//! read those tables out of `docs/adr/0006-tui.md` and hold the Rust ladder to them: if a pane's
//! `min_cols` changes, the ladder boundary moves, and this file is where the mismatch surfaces
//! rather than in a screenshot review.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use fathom_tui::layout::{
    self, FitState, LADDER, Step, chrome_rows, degraded_notice, display_width, fit, fit_text,
    refusal_lines,
};
use fathom_tui::views::{self, Role, ViewId};

fn adr_block(name: &str) -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/adr/0006-tui.md");
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{path} is not readable: {err}"));
    let begin = format!("<!-- BEGIN GENERATED: {name} -->");
    let end = format!("<!-- END GENERATED: {name} -->");
    let start = text
        .find(&begin)
        .unwrap_or_else(|| panic!("{path} has no {begin}"));
    let stop = text[start..]
        .find(&end)
        .map(|at| start + at)
        .unwrap_or_else(|| panic!("{path} has no {end}"));
    text[start + begin.len()..stop].trim().to_owned()
}

/// The `**bold**` column that names a view in every generated table.
fn view_id(title: &str) -> ViewId {
    let want = title.trim_matches('*').to_lowercase();
    views::VIEWS
        .iter()
        .find(|v| v.title.eq_ignore_ascii_case(&want))
        .map(|v| {
            ViewId::ALL
                .iter()
                .copied()
                .find(|id| format!("{id:?}").eq_ignore_ascii_case(v.id))
                .unwrap_or_else(|| panic!("no ViewId for {}", v.id))
        })
        .unwrap_or_else(|| panic!("no view named {title:?}"))
}

/// `|`-separated table rows as cells, header and separator skipped.
fn rows(block: &str) -> Vec<Vec<String>> {
    block
        .lines()
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.split('|')
                .skip(1)
                .map(|cell| cell.trim().to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn the_ladder_is_the_adrs_ladder_in_the_adrs_order() {
    let table = rows(&adr_block("degradation-ladder"));
    assert_eq!(
        table.len(),
        LADDER.len() + 2,
        "the ADR lists D1-D8 plus the D9 fallback and the D10 refusal"
    );
    let expected: Vec<(&str, &str)> = table
        .iter()
        .map(|cells| (cells[0].trim_matches('*'), cells[1].as_str()))
        .collect();
    let rust: Vec<(&str, &str)> = LADDER
        .iter()
        .map(|step| (step.id(), step.what()))
        .chain(std::iter::once((
            layout::FALLBACK,
            "single-pane fallback: secondary panes dropped",
        )))
        .chain(std::iter::once((
            layout::REFUSED,
            "refuse to draw, and say so",
        )))
        .collect();
    assert_eq!(rust, expected);
    // The recovery a step buys is the ADR's cols/rows column, not a number in a Rust match arm.
    for cells in rows(&adr_block("degradation-ladder")) {
        let Some(step) = LADDER.iter().find(|s| s.id() == cells[0].trim_matches('*')) else {
            continue;
        };
        let (cols, rows_bought) = step.recover(views::view(ViewId::Queue));
        let want_cols = cells[2]
            .strip_prefix('+')
            .and_then(|c| c.parse::<u16>().ok())
            .unwrap_or(0);
        let want_rows = cells[3]
            .strip_prefix('+')
            .and_then(|c| c.parse::<u16>().ok())
            .unwrap_or(0);
        assert_eq!(
            (cols, rows_bought),
            (want_cols, want_rows),
            "{} recovers a different amount than the ADR's table says",
            step.id()
        );
    }
}

#[test]
fn every_ladder_boundary_is_where_the_adr_says_it_is() {
    for cells in rows(&adr_block("degradation-per-view")) {
        let id = view_id(&cells[0]);
        let step = cells[1].trim_matches('*').to_owned();
        let published = cells[3].clone();
        let computed = layout::ladder_boundary(id)
            .into_iter()
            .find(|(name, _)| *name == step)
            .unwrap_or_else(|| panic!("{id:?} has no ladder entry for {step}"))
            .1;
        assert_eq!(
            computed, published,
            "{id:?} {step}: the Rust ladder fires at a different size than the ADR publishes"
        );
    }
}

#[test]
fn every_cell_of_the_size_matrix_is_the_state_the_adr_publishes() {
    // §6.6's matrix: nine views × six terminal sizes, state and the exact step list.
    for cells in rows(&adr_block("layout-matrix-steps")) {
        let id = view_id(&cells[0]);
        let mut wh = cells[1].split('×');
        let cols: u16 = wh.next().unwrap_or("80").parse().expect("cols");
        let rowcount: u16 = wh.next().unwrap_or("24").parse().expect("rows");
        let state = cells[2].as_str();
        let steps = cells[3].as_str();
        let fit = fit(id, cols, rowcount);
        assert_eq!(
            layout::state_name(fit.state),
            state,
            "{id:?} at {cols}×{rowcount}"
        );
        let joined = fit.applied.join(", ");
        assert_eq!(
            joined,
            if steps == "—" { "" } else { steps },
            "{id:?} at {cols}×{rowcount} applies a different ladder than the ADR"
        );
    }
}

#[test]
fn chrome_rows_is_the_chrome_the_view_declared() {
    // The model of what chrome costs: a boxed frame pays two rows for its rule pair, everything
    // else pays one. The renderer's own budget is allowed to differ (it draws a header row for a
    // borderless view too); this is the number `fit` reasons with.
    for id in ViewId::ALL {
        let view = views::view(id);
        let declared = usize::from(view.has_chrome(views::CH_FRAME)) * 2
            + usize::from(view.has_chrome(views::CH_TABS))
            + usize::from(view.has_chrome(views::CH_STRIPS))
            + usize::from(view.has_chrome(views::CH_STATUS))
            + usize::from(view.has_chrome(views::CH_HINT));
        assert_eq!(
            usize::from(chrome_rows(view, &[])),
            declared,
            "{id:?} charges a different chrome rent than it declares"
        );
    }
}

#[test]
fn a_view_that_cannot_draw_says_what_it_needs_and_what_it_got() {
    // §6.4's refusal panel, verbatim in shape.
    let queue = views::view(ViewId::Queue);
    assert_eq!(layout::refuse_below_cols(queue), 32);
    assert_eq!(layout::refuse_below_rows(queue), 4);
    let lines = refusal_lines(ViewId::Queue, 30, 8);
    assert_eq!(lines[0].trim(), "too small for Queue");
    assert_eq!(lines[1].trim(), "need 32×4, have 30×8");
    assert_eq!(lines[2].trim(), "");
    assert!(lines[3].trim().starts_with("`a` ambient"), "{lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("`fathom --plain`")),
        "the way out is on the panel: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("nothing was clipped")),
        "the ADR's last line: {lines:?}"
    );
    // Every view refuses below the size its own panes need, and the refusal is never a blank frame.
    for id in ViewId::ALL {
        let view = views::view(id);
        let fit = fit(id, 1, 1);
        assert_eq!(fit.state, FitState::Refused, "{id:?} should refuse 1×1");
        let panel = refusal_lines(id, 30, 10);
        assert!(
            panel.iter().any(|l| l.contains("too small for")),
            "{id:?} refused without saying so"
        );
        assert!(
            layout::refuse_below_cols(view) >= 1 && layout::refuse_below_rows(view) >= 1,
            "{id:?} has no refusal floor"
        );
    }
}

#[test]
fn the_ladder_is_applied_in_its_own_order_and_the_frame_is_never_emptied() {
    // §6.1's contract: the order is the ladder's, not whichever step is roomiest, and the primary
    // pane is the last thing standing.
    let mut seen_order = Vec::new();
    for id in ViewId::ALL {
        let fit = fit(id, 42, 8);
        let rank: Vec<usize> = fit
            .applied
            .iter()
            .map(|id| {
                LADDER
                    .iter()
                    .position(|s| s.id() == id)
                    .unwrap_or(LADDER.len())
            })
            .collect();
        let mut sorted = rank.clone();
        sorted.sort_unstable();
        assert_eq!(rank, sorted, "{id:?} applied its steps out of ladder order");
        assert!(
            !fit.dropped().is_empty() || fit.state == FitState::Complete,
            "{id:?} changed shape without saying what it dropped"
        );
        seen_order.push(fit.state);
        // D9 is the last thing to fire, and it keeps the primary pane.
        if fit.applied.iter().any(|a| a == layout::FALLBACK) {
            let widths = layout::pane_widths(id, 42, &fit.applied);
            assert_eq!(
                widths.len(),
                1,
                "{id:?} dropped everything instead of falling back to one pane"
            );
            assert_eq!(widths[0].0.role, Role::Primary);
        }
    }
    assert!(seen_order.contains(&FitState::Single));
}

#[test]
fn pane_widths_always_add_up_to_the_room_the_frame_has() {
    for id in ViewId::ALL {
        for (cols, rowcount) in [
            (80u16, 24u16),
            (100, 30),
            (200, 60),
            (64, 14),
            (52, 12),
            (40, 10),
        ] {
            let fit = fit(id, cols, rowcount);
            if fit.state == FitState::Refused {
                continue;
            }
            let widths = layout::pane_widths(id, cols, &fit.applied);
            let gutters = widths.len().saturating_sub(1)
                + usize::from(layout::frame_cols(views::view(id), &fit.applied) > 0) * 2;
            let total: usize = widths.iter().map(|(_, w)| usize::from(*w)).sum::<usize>() + gutters;
            assert_eq!(
                total,
                usize::from(cols),
                "{id:?} at {cols}×{rowcount}: pane widths do not fill the frame ({widths:?})"
            );
            for (pane, width) in &widths {
                if fit.applied.iter().any(|a| a == Step::SecondaryStrip.id())
                    && pane.role == Role::Secondary
                {
                    // D5's whole point: the secondary becomes a headline strip, which is *narrower*
                    // than the pane's hard minimum on purpose — a strip is a different object.
                    assert!(
                        *width <= pane.strip_width,
                        "{id:?} `{}` should be no wider than its strip width at \
                         {cols}×{rowcount}",
                        pane.name
                    );
                    if *width < pane.strip_width {
                        // Less than the strip means the frame cannot afford anybody's floor, which
                        // is the last-resort pass, not a D5 that forgot its number.
                        assert!(
                            cols < pane.strip_width + pane.hard_min,
                            "{id:?} shaved `{}` below its strip width for no reason",
                            pane.name
                        );
                    }
                } else {
                    assert!(
                        *width >= pane.hard_min.min(cols),
                        "{id:?} squeezed `{}` below its hard minimum at {cols}×{rowcount}",
                        pane.name
                    );
                }
            }
        }
    }
}

#[test]
fn the_secondary_pane_is_squeezed_before_the_primary_and_the_frame_last() {
    // §6.1's ordering claim, checked against the arithmetic instead of the prose: at the size where
    // D5 fires the secondary is at strip width and the primary still has room; D9 is the only step
    // that removes the secondary outright.
    let queue = views::view(ViewId::Queue);
    let at_d5 = {
        let mut cols = 80u16;
        while cols > 30 {
            let fit = fit(ViewId::Queue, cols, 24);
            if fit.applied.iter().any(|a| a == Step::SecondaryStrip.id()) {
                break;
            }
            cols -= 1;
        }
        cols
    };
    let fit = fit(ViewId::Queue, at_d5, 24);
    let widths = layout::pane_widths(ViewId::Queue, at_d5, &fit.applied);
    let secondary = queue.panes[1];
    let (_, width) = widths
        .iter()
        .find(|(pane, _)| pane.name == secondary.name)
        .expect("queue has a secondary pane");
    assert_eq!(
        *width, queue.panes[1].strip_width,
        "D5 should hold the secondary at exactly its strip width"
    );
    assert!(widths[0].1 > *width, "the primary gave up width first");
    assert!(
        !fit.applied.iter().any(|a| a == layout::FALLBACK),
        "D9 must not be what a squeezed secondary looks like"
    );
}

#[test]
fn the_degradation_notice_fits_the_width_it_is_printed_in() {
    // §6.4: at a narrow width the notice counts the drops; at a wide one it lists them.
    let queue14 = fit(ViewId::Queue, 64, 14);
    let narrow = degraded_notice(&queue14, 64);
    assert!(
        narrow.contains("dropped") && narrow.contains("<C-w>o"),
        "narrow notice should count: {narrow}"
    );
    assert!(
        display_width(&narrow) + 20 <= 64,
        "the narrow notice is the thing that must not be the clipped line: {narrow}"
    );
    let wide = degraded_notice(&queue14, 240);
    assert!(
        wide.contains("hint line") && wide.contains("box frame"),
        "a frame wide enough to afford the list should spell it out: {wide}"
    );
    assert!(
        wide.starts_with("LAYOUT degraded: "),
        "the long form is the one with the list: {wide}"
    );
    assert_eq!(degraded_notice(&fit(ViewId::Queue, 200, 60), 200), "");
}

#[test]
fn fitting_text_counts_cells_and_never_breaks_a_grapheme() {
    assert_eq!(display_width("queue"), 5);
    assert_eq!(display_width("段落"), 4, "CJK is two cells wide");
    assert_eq!(display_width("👀"), 2, "an emoji is two cells wide");
    assert_eq!(
        display_width("e\u{301}"),
        1,
        "a combining mark adds no cell"
    );
    assert_eq!(
        display_width("a\u{200d}b"),
        2,
        "a ZWJ join is not two cells, and it must not be cut between the halves"
    );
    assert_eq!(fit_text("queue", 10), "queue     ");
    assert_eq!(fit_text("queue", 5), "queue");
    assert_eq!(fit_text("queue", 4), "que…");
    assert_eq!(display_width(&fit_text("👀👀👀", 5)), 5);
    assert_eq!(fit_text("👀👀👀", 5), "👀👀…");
    assert_eq!(
        fit_text("e\u{301}x", 2),
        "e\u{301}x",
        "a combining mark travels with its base character"
    );
}

//! The layout matrix: the sizes the acceptance criteria name, every view, and the state each one
//! lands in — documented in ADR-006 §6.5 and pinned here as screenshots.
//!
//! This file is the second half of "`e5.5`: responsive layouts". ADR-006 §6.5 publishes a matrix of
//! `40×10 · 80×24 · 100×30 · 120×40 · 160×50 · 240×60` (the ticket's "ultrawide") for the nine views,
//! with a state column and a steps column, and it is generated from the Python ladder in
//! `src/recommender/tui/views.py`. The Rust ladder in [`fathom_tui::layout`] is a second
//! implementation of the same contract, and an unverified second implementation is a second opinion
//! waiting to diverge. So:
//!
//! 1. [`the_rust_ladder_agrees_with_the_published_matrix`] derives the matrix from Rust `fit()` and
//!    diffs it against the ADR's two generated blocks — a widened pane cannot leave the published
//!    table lying;
//! 2. [`every_view_has_a_screenshot_at_every_matrix_size`] renders every view at every size into
//!    `tests/golden/layout/` and compares, so a change to the drawing — not only to the arithmetic —
//!    has to be blessed by a human reading a diff;
//! 3. the width and height assertions in [`every_screenshot_frame_is_exactly_what_it_claimed`] are
//!    the part a screenshot review cannot check: a frame line that is one cell wide is a frame that
//!    wraps, and below the floor the view must refuse in words rather than draw badly.
//!
//! Re-bake after an intentional change with:
//!
//! ```text
//! FATHOM_GOLDEN=write cargo test -p fathom-tui --test layout_matrix
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::path::{Path, PathBuf};

use fathom_tui::app::{App, Model};
use fathom_tui::index::Facts;
use fathom_tui::keymap::Keymap;
use fathom_tui::layout::{self, state_name};
use fathom_tui::render::{frame_lines, frame_plan, pane_text, plain_flow};
use fathom_tui::text::width;
use fathom_tui::timeline::Batch;
use fathom_tui::views::{self, ViewId};

#[path = "timeline_corpus/mod.rs"]
mod corpus;

use corpus::{NOW, mock};

/// The six sizes the acceptance criteria name. `240×60` is the ticket's "ultrawide" and the same
/// number ADR-006 §6.5 publishes; `40×10` is the one below every view's floor except ambient's.
const MATRIX: [(u16, u16); 6] = [
    (40, 10),
    (80, 24),
    (100, 30),
    (120, 40),
    (160, 50),
    (240, 60),
];

/// The ADR, as text — the same file `tests/layout.rs` reads for the ladder tables.
fn adr() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/adr/0006-tui.md");
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{path} is not readable: {err}"))
}

fn block(name: &str) -> String {
    let text = adr();
    let begin = format!("<!-- BEGIN GENERATED: {name} -->");
    let end = format!("<!-- END GENERATED: {name} -->");
    let start = text
        .find(&begin)
        .unwrap_or_else(|| panic!("the ADR has no {begin} block"));
    let stop = text[start..]
        .find(&end)
        .map(|at| start + at)
        .unwrap_or_else(|| panic!("the ADR has no {end} block"));
    text[start + begin.len()..stop].trim().to_owned()
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

/// The ADR's state glyphs, read back into the words the Rust ladder uses.
fn state_of(cell: &str) -> &str {
    let cell = cell.trim();
    if cell.contains("complete") {
        "complete"
    } else if cell.contains("single") {
        "single"
    } else if cell.contains("refuse") {
        "refused"
    } else if cell.contains("steps") || cell.contains("degraded") {
        "degraded"
    } else {
        cell
    }
}

/// The matrix, as Rust derives it: `(view, size) → state`.
fn rust_state(view: ViewId, size: (u16, u16)) -> String {
    state_name(layout::fit(view, size.0, size.1).state).to_owned()
}

/// The matrix, as Rust derives it: `(view, size) → "D1.hint, D9.single_pane"`.
fn rust_steps(view: ViewId, size: (u16, u16)) -> String {
    let fit = layout::fit(view, size.0, size.1);
    if fit.applied.is_empty() {
        "—".to_owned()
    } else {
        fit.applied.join(", ")
    }
}

#[test]
fn the_matrix_names_exactly_the_sizes_the_ticket_names() {
    // The table header is the acceptance criterion in its own words; if the ADR ever changes the sizes,
    // this fails before the screenshot test can quietly stop testing them.
    let header = rows(&block("layout-matrix"))
        .first()
        .cloned()
        .unwrap_or_default();
    assert!(
        !header.is_empty(),
        "the matrix table has a header row to read"
    );
    let cells = block("layout-matrix");
    for (cols, rows) in MATRIX {
        let label = format!("{cols}×{rows}");
        assert!(
            cells.contains(&label),
            "ADR-006 §6.5 must publish a column for {label}"
        );
    }
}

#[test]
fn the_rust_ladder_agrees_with_the_published_matrix() {
    // §6.5's state column, cell by cell, from the ADR's own rows.
    let table = rows(&block("layout-matrix"));
    assert_eq!(
        table.len(),
        views::VIEWS.len(),
        "one row per view in §6.5's matrix"
    );
    let mut mismatched: Vec<String> = Vec::new();
    for row in &table {
        let title = row[0].trim_matches('*');
        let view = views::VIEWS
            .iter()
            .find(|view| view.title.eq_ignore_ascii_case(title))
            .unwrap_or_else(|| panic!("no view named {title:?} in the registry"));
        let id = ViewId::ALL
            .iter()
            .copied()
            .find(|id| id.name() == view.id)
            .unwrap_or_else(|| panic!("no ViewId for {}", view.id));
        for (index, (cols, height)) in MATRIX.iter().enumerate() {
            let cell = row
                .get(index + 1)
                .unwrap_or_else(|| panic!("{} has no cell for {cols}×{height}", view.id));
            let want = state_of(cell);
            let got = rust_state(id, (*cols, *height));
            if want != got {
                mismatched.push(format!(
                    "{title} at {cols}×{height}: the ADR says {want:?}, the Rust ladder says {got:?}"
                ));
            }
        }
    }
    assert!(
        mismatched.is_empty(),
        "the Rust ladder and ADR-006 §6.5 disagree:\n{}",
        mismatched.join("\n")
    );
}

#[test]
fn the_rust_ladder_applies_the_published_steps() {
    // §6.5's second table: the rungs, in order, for the same cells. The ADR joins them with `D1.hint,
    // D2.tabs`; `applied` is the same list in the same order, so the comparison is a string compare.
    let mut mismatched: Vec<String> = Vec::new();
    for row in rows(&block("layout-matrix-steps")) {
        let title = row[0].trim_matches('*');
        let Some(size) = row.get(1) else { continue };
        let Some((cols, height)) = size.split_once('×') else {
            continue;
        };
        let (Ok(cols), Ok(height)) = (cols.trim().parse::<u16>(), height.trim().parse::<u16>())
        else {
            continue;
        };
        let Some(view) = views::VIEWS
            .iter()
            .find(|view| view.title.eq_ignore_ascii_case(title))
        else {
            continue;
        };
        let id = ViewId::ALL
            .iter()
            .copied()
            .find(|id| id.name() == view.id)
            .expect("every view has an id");
        let want = row[3].trim();
        let got = rust_steps(id, (cols, height));
        // The ADR spells the refused state's steps out too; `applied` ends with `D10.refuse`, which the
        // published column abbreviates as `↓ … refuse` in the narrow matrix and lists in full here.
        if want != got && !(want.contains("refuse") && got.contains(layout::REFUSED)) {
            mismatched.push(format!(
                "{title} at {cols}×{height}: the ADR lists {want:?}, the Rust ladder applies {got:?}"
            ));
        }
    }
    assert!(
        mismatched.is_empty(),
        "the rungs the ADR publishes are not the rungs the ladder applies:\n{}",
        mismatched.join("\n")
    );
}

/// An app with rows in it: a matrix screenshot of an empty shell would prove the ladder at exactly
/// the moment it has the least to do.
fn app(view: ViewId, size: (u16, u16)) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        size,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.set_index_facts(&Facts::present(
        Some(1),
        Some("ready".to_owned()),
        Some(500),
        Some(48),
    ));
    app.load_episodes(Batch::new("fixture index", mock()));
    if view == ViewId::Diff {
        // The diff pane has no provider here, and the refusal it prints instead is part of what the
        // screenshot is for: a narrow Diff frame is the narrowest frame the shell has to cope with.
        app.set_view(ViewId::Diff);
        app.open_diff();
    } else {
        app.set_view(view);
    }
    app
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("layout")
}

fn blessing() -> bool {
    std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write")
}

/// One screenshot: a header that says what the ladder answered, then the frame as the terminal would
/// have drawn it.
fn render(view: ViewId, size: (u16, u16)) -> String {
    let app = app(view, size);
    let fit = layout::fit(view, size.0, size.1);
    let mut out = String::new();
    out.push_str(&format!(
        "# {} at {}×{} — {}\n",
        views::view(view).title,
        size.0,
        size.1,
        fit.summary()
    ));
    if !fit.applied.is_empty() {
        out.push_str(&format!("# steps · {}\n", fit.applied.join(", ")));
        out.push_str(&format!("# lost · {}\n", fit.dropped().join("; ")));
    }
    if let Some((cols, height)) = (fit.deficit.0 > 0 || fit.deficit.1 > 0).then_some(fit.deficit) {
        out.push_str(&format!("# deficit · {cols} cols, {height} rows short\n"));
    }
    out.push('\n');
    for line in frame_lines(&app) {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[test]
fn every_view_has_a_screenshot_at_every_matrix_size() {
    let dir = golden_dir();
    if blessing() {
        std::fs::create_dir_all(&dir).expect("the golden directory can be made");
    }
    let mut missing: Vec<String> = Vec::new();
    for view in ViewId::ALL {
        for size in MATRIX {
            let text = render(view, size);
            let path = dir.join(format!("{}_{}x{}.txt", view.name(), size.0, size.1));
            if blessing() {
                std::fs::write(&path, &text).expect("a golden can be written");
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(want) => assert_eq!(
                    want,
                    text,
                    "{} differs. If the change is intended, re-bake with FATHOM_GOLDEN=write and \
                     read the diff.",
                    path.display()
                ),
                Err(_) => missing.push(path.display().to_string()),
            }
        }
    }
    assert!(
        missing.is_empty(),
        "no screenshot for: {}",
        missing.join("\n")
    );
}

#[test]
fn every_screenshot_frame_is_exactly_what_it_claimed() {
    // The invariant a screenshot review is worst at checking: every drawn line is exactly the frame
    // width in *cells*, and there are exactly `rows` of them. Wide characters, emoji and combining
    // marks are what makes this hard, and the fixture rows carry all three (`unicode.rs` adds the
    // hostile set to the same renderer).
    for view in ViewId::ALL {
        for size in MATRIX {
            let lines = frame_lines(&app(view, size));
            assert_eq!(
                lines.len(),
                usize::from(size.1),
                "{} at {}×{} drew {} lines",
                view.name(),
                size.0,
                size.1,
                lines.len()
            );
            for (index, line) in lines.iter().enumerate() {
                assert_eq!(
                    width(line),
                    usize::from(size.0),
                    "{} at {}×{} line {} is {} cells, not the frame width: {line:?}",
                    view.name(),
                    size.0,
                    size.1,
                    index,
                    width(line)
                );
            }
        }
    }
}

#[test]
fn below_the_floor_a_view_refuses_in_words_and_says_what_it_needs() {
    // 40×10 is the ticket's "smaller than a phone" size. Anything that will not draw there says so
    // and states the size it wants (ADR-006 §6.6); nothing draws a clipped version of itself.
    // 20×4 is below every view's own floor, including ambient's, which is the highest one in the
    // registry (ADR-006 §6.6: the smallest view has the largest floor, because it has the least room
    // to give up).
    for view in ViewId::ALL {
        let tiny = layout::fit(view, 20, 4);
        assert_eq!(
            tiny.state,
            layout::FitState::Refused,
            "{} at 20×4 must refuse, not clip: {:?}",
            view.name(),
            tiny.applied
        );
        let notice = tiny.notice();
        assert!(
            notice.contains("LAYOUT REFUSED") || notice.contains("need"),
            "a refusal states the size it needs: {notice}"
        );
        let lines = frame_lines(&app(view, (20, 4)));
        assert_eq!(
            lines.len(),
            4,
            "a refusal still fills the frame it was given"
        );
        for line in &lines {
            assert_eq!(width(line), 20, "a refusal panel lines up too: {line:?}");
        }
    }
}

#[test]
fn the_plan_the_mouse_hits_is_the_plan_the_frame_drew() {
    // The pointer and the renderer may not have two opinions about where a pane is. The check is
    // arithmetic, not a screenshot: every pane rectangle must sit inside the frame, and the last one
    // must end where the frame does.
    for view in ViewId::ALL {
        for size in MATRIX {
            let app = app(view, size);
            let plan = frame_plan(&app, view, usize::from(size.0), usize::from(size.1));
            if plan.refused {
                continue;
            }
            assert!(
                !plan.panes.is_empty(),
                "{} at {}×{} has no panes to hit",
                view.name(),
                size.0,
                size.1
            );
            for rect in &plan.panes {
                assert!(
                    rect.x + rect.cols <= usize::from(size.0) + 1,
                    "{} `{}` at {}×{} runs off the frame: x {} + {} > {}",
                    view.name(),
                    rect.pane.name,
                    size.0,
                    size.1,
                    rect.x,
                    rect.cols,
                    size.0
                );
                assert!(
                    rect.y + rect.rows <= usize::from(size.1),
                    "{} `{}` at {}×{} runs below the frame",
                    view.name(),
                    rect.pane.name,
                    size.0,
                    size.1
                );
            }
        }
    }
}

#[test]
fn the_plain_line_mode_carries_every_fact_the_frame_does() {
    // ADR-006 §6.7: the line mode is the same frame, read in order. The check that makes that a
    // statement rather than a hope is that every visible character of the frame appears in the text
    // render — nothing is dropped because it lived in a column the renderer did not print.
    for view in [ViewId::Queue, ViewId::Timeline, ViewId::Diff] {
        let size = (100, 30);
        let mut app = app(view, size);
        app.notices.push("a notice worth reading".to_owned());
        let flow = plain_flow(&app);
        assert!(
            flow.len() >= 4,
            "{view:?} line mode printed almost nothing: {flow:?}"
        );
        for line in &flow {
            assert!(
                !line.contains('\u{1b}'),
                "the line mode writes no escape sequences: {line:?}"
            );
        }
        assert!(
            flow.iter()
                .any(|line| line.contains("a notice worth reading")),
            "the line mode prints the notices: {flow:?}"
        );
        // The focused pane's first row survives the trip from grid to lines.
        let pane = app.focused_pane().expect("a focused pane");
        let rows = pane_text(&app, pane);
        if let Some(first) = rows.first()
            && !first.trim().is_empty()
        {
            assert!(
                flow.iter().any(|line| line.trim_end() == first.trim_end()),
                "{view:?}'s first `{}` row is missing from the line mode: {flow:?}",
                pane.name
            );
        }
    }
}

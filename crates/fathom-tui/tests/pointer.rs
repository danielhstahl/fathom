//! The pointer (`e5.5`, ADR-006 §10.1): the mouse is additive, and "additive" is a testable claim.
//!
//! §10.1's sentence is *"nothing available only on the mouse"*. Read as a design note it says "put
//! the mouse in, keep the keyboard first". Read as a test it says something much stronger and much
//! more checkable: **every gesture has a keystroke that leaves the application in the same state**.
//! [`every_gesture_has_a_keystroke_that_does_the_same_thing`] is that reading — it drives the pointer
//! and the keymap through the same moves and compares the resulting state, instead of asserting that
//! a click "did something".
//!
//! The rest of the file is the edges a reviewer meets first: a click on a title, on chrome, on the
//! other pane, a wheel over a document, the second press of a double-click, and the buttons that are
//! bound to nothing and say so in words.
//!
//! Capture is off unless `fathom --mouse` was asked for, because capture takes the terminal's own
//! text selection away — [`capture_is_asked_for_only_when_it_was_asked_for`] and
//! [`the_restore_gives_the_pointer_back`] pin both halves of that trade.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use fathom_tui::app::{App, Model, Overlay};
use fathom_tui::event::{Button, Message, MouseMsg, Scroll};
use fathom_tui::frame::{self, Options, render_plain};
use fathom_tui::keymap::Keymap;
use fathom_tui::render::{FramePlan, Hit, frame_plan};
use fathom_tui::terminal::TerminalRestore;
use fathom_tui::timeline::Batch;
use fathom_tui::views::ViewId;

#[path = "timeline_corpus/mod.rs"]
mod corpus;

/// The diff fixtures, under a name that does not collide with the timeline corpus.
#[path = "corpus/mod.rs"]
mod diffcorpus;

use corpus::{NOW, mock};

const SIZE: (u16, u16) = (80, 24);

fn shell(view: ViewId) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        SIZE,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("fixture index", mock()));
    app.set_view(view);
    app.mouse = true;
    // Draw it once. A list pane's window — how many rows fit, and so which slot a line of the drawing
    // is — is a fact about the frame, and a pointer test that never drew a frame is asking the rail
    // about a window it was never given.
    let _ = fathom_tui::render::frame_lines(&app);
    app
}

/// The body cell whose drawn line names `name`, in the terminal's 1-based coordinates.
///
/// The alternative — recomputing which slot a line is in the test — would only ever agree with itself.
/// Reading the pane's own text and clicking the line that says what we asked for is the check that the
/// pointer and the renderer agree.
fn cell_naming(app: &App, pane: &str, name: &str) -> (u16, u16) {
    let frame = plan(app);
    let rect = frame
        .panes
        .iter()
        .find(|rect| rect.pane.name == pane)
        .unwrap_or_else(|| panic!("no {pane} pane in {:?}", app.view));
    let lines = fathom_tui::render::pane_text(app, rect.pane);
    for (line, text) in lines.iter().enumerate() {
        if text.contains(name) {
            return at(rect.x + 1, frame.body_y + line);
        }
    }
    panic!(
        "no line of `{pane}` names {name:?}; the pane drew {:?}",
        lines.iter().take(4).collect::<Vec<_>>()
    );
}

/// The frame's own geometry, at the size the app was built at.
fn plan(app: &App) -> FramePlan {
    frame_plan(app, app.view, usize::from(SIZE.0), usize::from(SIZE.1))
}

/// Every body cell of a named pane, top to bottom — asked of the plan rather than computed, so the
/// test cannot drift from the frame: whether a box is drawn, where the body starts and how wide the
/// gutter is are all the ladder's business, not the test's.
fn body_cells(app: &App, pane: &str) -> Vec<(u16, u16)> {
    let plan = plan(app);
    let mut out = Vec::new();
    for row in 0..plan.rows {
        for col in 0..plan.cols {
            if matches!(plan.hit(col, row), Hit::Body { pane: p, .. } if p.name == pane) {
                out.push(at(col, row));
                break;
            }
        }
    }
    assert!(
        out.len() > 4,
        "{pane} has {} body rows to click at {SIZE:?}",
        out.len()
    );
    out
}

/// The `n`th body cell of a pane.
fn body_cell(app: &App, pane: &str, n: usize) -> (u16, u16) {
    *body_cells(app, pane)
        .get(n)
        .unwrap_or_else(|| panic!("{pane} has no body row #{n}"))
}

/// The terminal's own coordinate system: crossterm reports row and column **1-based**, and
/// `App::pointer` converts to the frame's 0-based cells before asking the plan. Every helper here
/// addresses cells the way `FramePlan` does — so the conversion happens here, in one place, and a test
/// that got it wrong would be clicking one cell up and left of the cell it names.
fn at(col: usize, row: usize) -> (u16, u16) {
    (
        u16::try_from(col + 1).expect("a cell is a column the terminal can name"),
        u16::try_from(row + 1).expect("a cell is a row the terminal can name"),
    )
}

/// Press the left button at a cell the terminal named.
fn press_at(app: &mut App, cell: (u16, u16)) {
    press(app, cell.0, cell.1);
}

/// Press the left button, through the same entry point the event loop uses.
fn press(app: &mut App, col: u16, row: u16) {
    frame::apply(
        app,
        Message::Mouse(MouseMsg::Press {
            col,
            row,
            button: Button::Left,
            double: false,
        }),
    );
}

fn wheel(app: &mut App, col: u16, row: u16, direction: Scroll) {
    frame::apply(
        app,
        Message::Mouse(MouseMsg::Scroll {
            col,
            row,
            direction,
        }),
    );
}

fn key(app: &mut App, code: KeyCode) {
    frame::apply(app, Message::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// The pointer's own entry point refuses to be reached without capture: `App::mouse` is the flag the
/// loop sets when capture succeeded, and every gesture checks it.
#[test]
fn a_pointer_without_capture_does_nothing_at_all() {
    let mut app = shell(ViewId::Timeline);
    app.mouse = false;
    let before = app.timeline.cursor();
    let (col, row) = body_cell(&app, "rail", 4);
    press(&mut app, col, row);
    assert_eq!(
        app.timeline.cursor(),
        before,
        "a click moved the cursor although nothing asked for the mouse"
    );
}

#[test]
fn the_row_under_the_pointer_is_the_row_that_gets_selected() {
    // The claim a reviewer actually makes — "I clicked the row I meant" — checked by reading the drawn
    // row back: the line under the pointer has to name the episode that ended up selected. Doing this
    // by recomputing the window arithmetic in the test would only ever agree with itself, so the test
    // reads the pane's own text and the corpus's own ids instead.
    let ids: Vec<String> = mock().iter().map(|row| row.id.clone()).collect();
    let mut app = shell(ViewId::Timeline);
    let frame = plan(&app);
    let rail = frame
        .panes
        .iter()
        .find(|rect| rect.pane.name == "rail")
        .expect("the rail");
    let lines = fathom_tui::render::pane_text(&app, rail.pane);
    let mut checked = 0;
    for (line, drawn) in lines.iter().take(rail.rows).enumerate() {
        let Some(want) = ids.iter().find(|id| drawn.contains(*id)) else {
            continue; // a group header, a `··· n more`, or a blank row: nothing to select there
        };
        press_at(&mut app, at(rail.x + 1, frame.body_y + line));
        let got = app
            .timeline
            .selected()
            .unwrap_or_else(|| panic!("a click on line {line} selected nothing"))
            .id
            .clone();
        assert_eq!(
            got, *want,
            "the rail's line {line} reads {drawn:?}, so the click should have selected {want:?}"
        );
        checked += 1;
    }
    assert!(
        checked > 2,
        "only {checked} rail rows could be checked at {SIZE:?}"
    );

    // The worklist draws two lines per row, so the arithmetic a click has to agree with is different
    // in kind: two neighbouring cells are one row, and the cell after them is the next one down.
    // The worklist's rows are the ranking's, and since `reco-c05` each one is drawn on two lines — an
    // odd shape for a list, and the one place the pointer's arithmetic could go quietly wrong.
    let queue_plan = plan(&shell(ViewId::Queue));
    let work = queue_plan
        .panes
        .iter()
        .find(|rect| rect.pane.name == "worklist")
        .expect("the worklist");
    let cell = |line: usize| at(work.x + 1, queue_plan.body_y + line);
    let clicked = |line: usize| {
        let mut app = shell(ViewId::Queue);
        let (col, row) = cell(line);
        press(&mut app, col, row);
        app.selected_row()
    };
    assert_eq!(
        clicked(0),
        clicked(1),
        "the first two lines of a worklist row are the same row"
    );
    assert_eq!(
        clicked(2),
        clicked(0) + 1,
        "and the third line is the next row down"
    );

    // The diff tree: the file under the pointer is the file that opens. A real patch, because a tree
    // with nothing in it would pass this test by checking nothing.
    let mut app = diffcorpus::app_with("mixed", (110, 30));
    app.mouse = true;
    let _ = fathom_tui::render::frame_lines(&app);
    let frame = frame_plan(&app, ViewId::Diff, 110, 30);
    let tree = frame
        .panes
        .iter()
        .find(|rect| rect.pane.name == "tree")
        .expect("the tree");
    let lines = fathom_tui::render::pane_text(&app, tree.pane);
    // The check is not "the tree's text contains the path" — a 24-column tree elides a long path in the
    // middle, and a test that insisted on the whole path would be a test of the elider. It is "the line
    // I clicked names the file that opened": the hunk pane's header names the file it is showing, and
    // if the click had landed a row off, that header would name a different file.
    let hunks_pane = frame
        .panes
        .iter()
        .find(|rect| rect.pane.name == "hunks")
        .expect("the hunk pane")
        .pane;
    let mut checked = 0;
    for (line, raw) in lines.iter().take(tree.rows).enumerate() {
        let drawn = raw.trim_end();
        if drawn.trim().is_empty() || !drawn.contains('.') {
            continue; // a group header or a blank row: nothing to open
        }
        press_at(&mut app, at(tree.x + 1, frame.body_y + line));
        let Some(index) = app.diff.file else {
            continue;
        };
        let opened = app
            .diff
            .file_at(index)
            .unwrap_or_else(|| panic!("the click on tree line {line} selected no file"));
        let name = opened.path.rsplit('/').next().unwrap_or(&opened.path);
        let header = fathom_tui::render::pane_text(&app, hunks_pane)
            .first()
            .cloned()
            .unwrap_or_default();
        assert!(
            drawn.contains(name) && header.contains(name),
            "tree line {line} reads {drawn:?}, and the hunk pane opened {} which the header              {header:?} should name",
            opened.path
        );
        checked += 1;
    }
    assert!(
        checked > 1,
        "only {checked} tree rows could be checked: {lines:?}"
    );
}

#[test]
fn a_click_selects_the_row_under_it_and_a_click_below_is_a_row_below() {
    let ids: Vec<String> = mock().iter().map(|row| row.id.clone()).collect();
    let mut app = shell(ViewId::Timeline);
    let (col, row) = cell_naming(&app, "rail", &ids[2]);
    press(&mut app, col, row);
    assert_eq!(
        app.timeline.selected().map(|row| row.id.clone()),
        Some(ids[2].clone()),
        "the click selected the row it was on"
    );
    let below = app.timeline.cursor();
    let (col, row) = cell_naming(&app, "rail", &ids[0]);
    press(&mut app, col, row);
    assert!(
        app.timeline.cursor() < below,
        "clicking a row higher up moves the cursor up, not sideways: {below} → {}",
        app.timeline.cursor()
    );
    // A click on a day header is a click on a row the reviewer reads but cannot act on: the pane
    // takes the focus, and the cursor stays where it was.
    let frame = plan(&app);
    let rail = frame
        .panes
        .iter()
        .find(|rect| rect.pane.name == "rail")
        .expect("the rail");
    let header = at(
        rail.x + 1,
        frame.body_y, // the rail's first line is the day header
    );
    press(&mut app, header.0, header.1);
    assert_eq!(
        app.timeline.selected().map(|row| row.id.clone()),
        Some(ids[0].clone()),
        "clicking the day header did not move the cursor off the episode it was on"
    );
}

#[test]
fn a_click_focuses_the_pane_it_landed_in() {
    let mut app = shell(ViewId::Timeline);
    assert_eq!(app.pane, 0, "the rail starts focused");
    let (col, row) = body_cell(&app, "card", 1);
    press(&mut app, col, row);
    assert_eq!(app.pane, 1, "the card is focused now");
    let (col, row) = body_cell(&app, "rail", 1);
    press(&mut app, col, row);
    assert_eq!(app.pane, 0, "and back");
}

#[test]
fn a_click_on_a_title_focuses_the_pane_without_moving_the_cursor() {
    // A title is a place to click, and a reviewer will click it. It must not move the selection out
    // from under them while doing so.
    let mut app = shell(ViewId::Timeline);
    let plan = plan(&app);
    let card = plan
        .panes
        .iter()
        .find(|rect| rect.pane.name == "card")
        .expect("the timeline has a card pane");
    let (col, row) = at(card.x + 1, plan.body_y.saturating_sub(1));
    let before = app.timeline.cursor();
    press(&mut app, col, row);
    assert_eq!(app.pane, 1, "the click focused the card");
    assert_eq!(
        app.timeline.cursor(),
        before,
        "and did not move the selection"
    );
}

#[test]
fn a_click_on_chrome_answers_in_words() {
    let mut app = shell(ViewId::Timeline);
    let plan = plan(&app);
    let row = (0..usize::from(SIZE.1))
        .find(|row| matches!(plan.hit(4, *row), Hit::Chrome(_)))
        .expect("the frame has a row of chrome to click");
    press(&mut app, 4, u16::try_from(row).unwrap());
    let toast = app.toast.clone().unwrap_or_default();
    assert!(
        toast.contains("chrome"),
        "the click was answered rather than swallowed: {toast:?}"
    );
}

#[test]
fn the_wheel_moves_the_pane_under_it_and_no_other() {
    // The wheel is `j`/`k` where the pointer is, so what it may not do is move anything else. Over the
    // rail it moves the rail's cursor one row and leaves the card's own scroll alone; over a document
    // it scrolls the document and leaves the list's selection alone.
    let mut app = shell(ViewId::Timeline);
    let rail_was = app.timeline.cursor();
    let cell = cell_naming(&app, "rail", &mock()[1].id);
    wheel(&mut app, cell.0, cell.1, Scroll::Down);
    assert_eq!(
        app.timeline.cursor(),
        rail_was + 1,
        "over a list the wheel moves one row"
    );
    assert_eq!(
        app.pane, 0,
        "and the wheel was over the rail, so the rail is focused"
    );
    wheel(&mut app, cell.0, cell.1, Scroll::Up);
    assert_eq!(app.timeline.cursor(), rail_was, "and back");

    // A document: the hunk pane, with a patch in it, which is the one pane in this shell with a scroll
    // of its own that is not a cursor.
    let mut app = diffcorpus::app_with("mixed", (110, 30));
    app.mouse = true;
    let _ = fathom_tui::render::frame_lines(&app);
    let tree_was = app.diff.file;
    let cell = {
        let frame = frame_plan(&app, ViewId::Diff, 110, 30);
        let hunks = frame
            .panes
            .iter()
            .find(|rect| rect.pane.name == "hunks")
            .expect("the hunk pane");
        at(hunks.x + 1, frame.body_y + 1)
    };
    let before = app.scroll_of();
    wheel(&mut app, cell.0, cell.1, Scroll::Down);
    assert_eq!(
        app.scroll_of(),
        before + 1,
        "the wheel scrolled the document under it by one line"
    );
    assert_eq!(
        app.diff.file, tree_was,
        "and did not move the tree's selection while doing it"
    );
}

#[test]
fn a_second_click_in_the_same_cell_is_enter_on_that_row() {
    let target = mock()[1].id.clone();
    // Keyboard first: select the row, press `<Enter>`, and write down what changed.
    let mut typed = shell(ViewId::Timeline);
    let cell = cell_naming(&typed, "rail", &target);
    press(&mut typed, cell.0, cell.1);
    let before = (typed.view, typed.pane, typed.scope.episode.clone());
    key(&mut typed, KeyCode::Enter);
    let after = (typed.view, typed.pane, typed.scope.episode.clone());
    assert_ne!(
        before, after,
        "`<Enter>` on a row has to do something, or this proves nothing"
    );

    // Pointer: the same row, pressed twice in the same cell.
    let mut clicked = shell(ViewId::Timeline);
    let cell = cell_naming(&clicked, "rail", &target);
    press(&mut clicked, cell.0, cell.1);
    assert_eq!(
        clicked.view, before.0,
        "a single click selects the row and does not drill"
    );
    assert_eq!(
        clicked.scope.episode,
        Some(target.clone()),
        "the first click selected the row under the pointer"
    );
    press(&mut clicked, cell.0, cell.1);
    assert_eq!(
        (clicked.view, clicked.pane, clicked.scope.episode.clone()),
        after,
        "the second click in the same cell is `<Enter>` on that row, and nothing else"
    );
}

#[test]
fn two_clicks_far_apart_in_time_are_two_clicks_not_one_double() {
    // The terminal reports two presses and no double flag, so the interval is ours (`DOUBLE_CLICK`).
    // Two clicks a minute apart on the same row are two selections, and the row is already selected by
    // the first, so the second must not drill.
    let target = mock()[1].id.clone();
    let mut app = shell(ViewId::Timeline);
    let cell = cell_naming(&app, "rail", &target);
    press(&mut app, cell.0, cell.1);
    let after_one = (app.view, app.pane);
    // A press long after the double-click window: the same cell, a minute later.
    frame::apply(
        &mut app,
        Message::Mouse(MouseMsg::Press {
            col: cell.0,
            row: cell.1,
            button: Button::Left,
            double: false,
        }),
    );
    app.click = Some((cell.0, cell.1, Instant::now() - Duration::from_secs(60)));
    frame::apply(
        &mut app,
        Message::Mouse(MouseMsg::Press {
            col: cell.0,
            row: cell.1,
            button: Button::Left,
            double: false,
        }),
    );
    assert_eq!(
        (app.view, app.pane),
        after_one,
        "a second press outside the double-click window is not a double-click"
    );
}

#[test]
fn the_buttons_that_are_bound_to_nothing_say_so() {
    for (button, want) in [
        (Button::Right, "right button"),
        (Button::Middle, "middle button"),
        (Button::Drag, "drag"),
        (Button::Other, "bound to nothing"),
    ] {
        let mut app = shell(ViewId::Timeline);
        let before = app.timeline.cursor();
        let (col, row) = body_cell(&app, "rail", 1);
        frame::apply(
            &mut app,
            Message::Mouse(MouseMsg::Press {
                col,
                row,
                button,
                double: false,
            }),
        );
        let toast = app.toast.clone().unwrap_or_default();
        assert!(
            toast.contains(want),
            "{button:?} answered {toast:?}, which does not mention {want:?}"
        );
        assert_eq!(
            app.timeline.cursor(),
            before,
            "{button:?} must not also move the selection"
        );
    }
}

#[test]
fn a_click_while_an_overlay_is_open_only_closes_it() {
    // An overlay owns the keyboard (§7.1). A click must not act *through* a prompt that is holding
    // half-typed text, so it does the one thing `Esc` does.
    let mut app = shell(ViewId::Timeline);
    app.timeline.open_editor();
    app.overlay = Some(Overlay::Filter);
    let before = app.timeline.cursor();
    let (col, row) = body_cell(&app, "rail", 4);
    press(&mut app, col, row);
    assert!(app.overlay.is_none(), "the overlay closed");
    assert_eq!(
        app.timeline.cursor(),
        before,
        "and nothing behind it was selected"
    );
}

#[test]
fn every_gesture_has_a_keystroke_that_does_the_same_thing() {
    // §10.1 as a property: for each gesture, the keyboard's answer and the pointer's answer leave the
    // same state. Anything else would be a feature only the pointer has.
    let mut typed = shell(ViewId::Timeline);
    let mut reached: Vec<String> = Vec::new();
    for _ in 0..6 {
        key(&mut typed, KeyCode::Char('j'));
        // `j` steps over a day header the same way it steps onto a row: the cursor is only ever left
        // on something a reviewer can act on, and the parity check is about those.
        if let Some(id) = typed.timeline.selected().map(|row| row.id.clone())
            && !reached.contains(&id)
        {
            reached.push(id);
        }
    }
    assert!(
        reached.len() >= 3,
        "`j` reached only {reached:?} in six presses"
    );
    for want in reached {
        // The pointer's answer: click the line that names that same episode.
        let mut clicked = shell(ViewId::Timeline);
        let (col, row) = cell_naming(&clicked, "rail", &want);
        press(&mut clicked, col, row);
        assert_eq!(
            clicked.timeline.selected().map(|row| row.id.clone()),
            Some(want.clone()),
            "`j` and a click on the row that names {want:?} are not the same move"
        );
        assert_eq!(
            clicked.pane, typed.pane,
            "and they leave the focus in the same place"
        );
    }

    // One wheel notch against `j`, from the same place.
    let mut typed = shell(ViewId::Timeline);
    key(&mut typed, KeyCode::Char('j'));
    let from = typed
        .timeline
        .selected()
        .expect("a fresh timeline has a row to be on")
        .id
        .clone();
    // The wheel moves the *pane*, not the pointer's neighbourhood — exactly what `j` does, which is
    // the claim under test. So from the same starting state, one notch lands where one `j` lands.
    let mut wheeled = shell(ViewId::Timeline);
    let (col, row) = cell_naming(&wheeled, "rail", &from);
    wheel(&mut wheeled, col, row, Scroll::Down);
    assert_eq!(
        wheeled.timeline.selected().map(|row| row.id.clone()),
        Some(from),
        "one wheel notch is one row, which is `j`"
    );

    // The double-click against `<Enter>`.
    let mut typed = shell(ViewId::Timeline);
    key(&mut typed, KeyCode::Char('j'));
    key(&mut typed, KeyCode::Enter);
    let mut doubled = shell(ViewId::Timeline);
    let target = mock()[1].id.clone();
    let (col, row) = cell_naming(&doubled, "rail", &target);
    press(&mut doubled, col, row);
    press(&mut doubled, col, row);
    assert_eq!(
        doubled.view, typed.view,
        "the double-click drills to the view `<Enter>` drills to"
    );
    assert_eq!(
        doubled.scope.episode, typed.scope.episode,
        "and drills into the same episode"
    );
}

#[test]
fn a_hit_map_covers_every_cell_of_a_bordered_frame() {
    // A cell that belongs to no pane and no chrome is a cell the pointer cannot answer for, and a
    // reviewer will click it eventually. Ambient is exempt: it draws one card into an empty frame on
    // purpose, so "outside" is its ordinary answer rather than a hole.
    for view in ViewId::ALL {
        if view == ViewId::Ambient {
            continue;
        }
        let app = shell(view);
        let plan = plan(&app);
        for row in 0..usize::from(SIZE.1) {
            for col in 0..usize::from(SIZE.0) {
                assert!(
                    !matches!(plan.hit(col, row), Hit::Outside),
                    "{view:?} at {col},{row} belongs to nothing: {:?}",
                    plan.panes
                        .iter()
                        .map(|r| (r.pane.name, r.x, r.y, r.cols, r.rows))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}

#[test]
fn capture_is_asked_for_only_when_it_was_asked_for() {
    // The frame must not take the mouse, and must not give it back, unless `--mouse` said so.
    // What "asked for" is made of. Capture is taken by the terminal session and never by a text
    // render: a pipe has no pointer to report, and a `--plain` run that asked for one would be
    // switching on a mode with nothing to read it.
    assert!(
        fathom_tui::terminal::ENTER_MOUSE.contains("[?1000h")
            && fathom_tui::terminal::ENTER_MOUSE.contains("[?1006h"),
        "capture asks for button reports and SGR coordinates"
    );
    assert!(
        fathom_tui::terminal::LEAVE_MOUSE.contains("[?1000l")
            && fathom_tui::terminal::LEAVE_MOUSE.contains("[?1006l"),
        "and there is a matching leave for every enter"
    );
    assert!(
        !render_plain(&Options {
            mouse: true,
            ..Options::default()
        })
        .first()
        .cloned()
        .unwrap_or_default()
        .contains("[?1000h"),
        "the text render never takes the pointer"
    );

    let mut off = shell(ViewId::Queue);
    off.mouse = false;
    assert!(
        !off.status_bits().join(" · ").contains("mouse"),
        "with capture off the status line says nothing about it: {}",
        off.status_bits().join(" · ")
    );
    let mut on = shell(ViewId::Queue);
    on.mouse = true;
    let status = on.status_bits().join(" · ");
    assert!(
        status.contains("mouse on"),
        "capture is visible in the status line, because it costs the reviewer their text \
         selection: {status}"
    );
}

#[test]
fn the_restore_gives_the_pointer_back() {
    // The ADR's rule for everything the shell takes over: normal exit, panic and signal are one code
    // path, and the sequence is written even when the frame did not get to say goodbye.
    let restore = TerminalRestore::with_sink(Box::new(Vec::new()));
    restore.note(true, true);
    restore.enable_mouse().expect("the sink takes the sequence");
    assert!(restore.mouse(), "capture is recorded as taken");
    assert!(
        restore.owns_screen(),
        "and it counts as owning the terminal"
    );

    let mut out = Vec::new();
    restore.restore_to(&mut out).expect("the restore writes");
    let text = String::from_utf8(out).expect("sequences are ASCII");
    assert!(
        text.contains("[?1000l") && text.contains("[?1006l"),
        "the pointer is given back before the screen is: {text:?}"
    );
    assert!(
        text.find("[?1000l") < text.find("[?1049l"),
        "mouse off first, alt-screen leave after: {text:?}"
    );
    assert!(!restore.owns_screen(), "and nothing is left claimed");

    let mut again = Vec::new();
    restore
        .restore_to(&mut again)
        .expect("a second restore is fine");
    assert!(
        again.is_empty(),
        "the second restore writes nothing, as §8.1 requires of the alt-screen sequence too"
    );
}

#[test]
fn a_resize_recomputes_the_grammar_and_says_so() {
    // ADR-006 §6.4's promise, at the moment it matters: a resize that fired the ladder names the
    // rungs that fired, and a resize that did not tells them nothing.
    let mut app = shell(ViewId::Timeline);
    frame::apply(&mut app, Message::Resize { cols: 80, rows: 24 });
    assert!(
        !app.status_bits().join(" · ").contains("LAYOUT"),
        "80×24 is complete: {}",
        app.status_bits().join(" · ")
    );
    frame::apply(&mut app, Message::Resize { cols: 46, rows: 20 });
    let left = app.status_bits().join(" · ");
    assert!(
        left.contains("LAYOUT"),
        "a degraded frame says it is degraded: {left}"
    );
    let lines = fathom_tui::render::view_lines(&app, app.view, 46, 20);
    assert_eq!(lines.len(), 20, "and still fills the frame it was given");
    frame::apply(
        &mut app,
        Message::Resize {
            cols: 120,
            rows: 40,
        },
    );
    assert!(
        !app.status_bits().join(" · ").contains("LAYOUT"),
        "120×40 is complete again: {}",
        app.status_bits().join(" · ")
    );
}

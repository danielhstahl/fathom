//! The shell: what the frame looks like, what a keystroke changes, and how the loop ends.
//!
//! The ticket's first acceptance criterion lives here — an interactive smoke pass at 80×24, 100×30
//! and 200×60 with no layout panic — plus the focus model, the tab bar, the drill edges, the three
//! overlays and the two ways a prompt can swallow a keystroke. Every test draws through the real
//! renderer (`frame_lines`) rather than a mock, because "the frame is 24 lines of 80 cells" is only
//! worth asserting if it is the same renderer the terminal gets.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};

use fathom_tui::app::{App, Model};
use fathom_tui::frame::{HeadlessSink, Options, render_at, render_plain, run_loop};
use fathom_tui::keymap::{Keymap, Mode, Override};
use fathom_tui::render::frame_lines;
use fathom_tui::views::{self, ViewId};

/// The three sizes the ticket names, plus the ones the ladder was derived at.
const SIZES: [(u16, u16); 3] = [(80, 24), (100, 30), (200, 60)];

fn app(size: (u16, u16)) -> App {
    App::new(
        Keymap::defaults(),
        Model::from_facade(),
        size,
        "fathom/pilot · main",
    )
}

/// An app whose lists have rows in them, because motion needs something to move over. `rows` are
/// fed in as plain strings — the shell only ever holds the index, never the row (ADR-006 §5.4).
fn app_with_rows(size: (u16, u16), rows: usize) -> App {
    let mut app = app(size);
    let lines: Vec<String> = (0..rows)
        .map(|i| format!("e{i:04x} 0:12 agent  +{i} −1   risk {}", 90 - i))
        .collect();
    for view in [ViewId::Queue, ViewId::Timeline] {
        for pane in views::view(view).panes {
            app.model
                .rows
                .insert((view, pane.name.to_owned()), lines.clone());
        }
    }
    app
}

fn cell_width(line: &str) -> usize {
    fathom_tui::layout::display_width(line)
}

fn assert_frame(lines: &[String], size: (u16, u16), what: &str) {
    assert_eq!(
        lines.len(),
        usize::from(size.1),
        "{what}: {} rows, want {}",
        lines.len(),
        size.1
    );
    for (index, line) in lines.iter().enumerate() {
        assert_eq!(
            cell_width(line),
            usize::from(size.0),
            "{what}: row {index} is {} cells, want {} — {line:?}",
            cell_width(line),
            size.0
        );
    }
}

// --- criterion 1: the smoke pass -----------------------------------------------------------

#[test]
fn every_view_draws_an_exact_frame_at_the_three_promised_sizes() {
    let options = Options::default();
    for size in SIZES {
        for view in ViewId::ALL {
            let lines = render_at(&options, view, size);
            assert_frame(&lines, size, &format!("{view:?} at {}×{}", size.0, size.1));
        }
    }
}

#[test]
fn the_frame_survives_being_shrunk_and_grown_across_the_whole_ladder() {
    // 200×60 → 20×5 → back, drawing at every step. The ladder has to degrade, refuse and recover
    // without a panic and without ever emitting a ragged frame (§6.1).
    let mut app = app((200, 60));
    let sizes: Vec<(u16, u16)> = (5u16..=60)
        .map(|rows| (rows.saturating_mul(3), rows))
        .chain((5u16..=60).rev().map(|rows| (rows.saturating_mul(4), rows)))
        .collect();
    for size in sizes {
        app.resize(size.0, size.1);
        let lines = frame_lines(&app);
        assert_frame(&lines, size, &format!("queue at {}×{}", size.0, size.1));
    }
}

#[test]
fn every_view_still_draws_a_frame_when_it_refuses() {
    // A refused view is a frame with a panel in it, not an empty screen (§6.4).
    let options = Options::default();
    for view in ViewId::ALL {
        let declared = views::view(view);
        let size = (
            fathom_tui::layout::refuse_below_cols(declared) - 1,
            fathom_tui::layout::refuse_below_rows(declared) - 1,
        );
        let lines = render_at(&options, view, size);
        assert_frame(&lines, size, &format!("{view:?} refused panel"));
        assert!(
            lines.iter().any(|l| l.contains("too small for")),
            "{view:?} refused without saying so: {lines:?}"
        );
    }
}

#[test]
fn a_plain_frame_is_the_same_frame_with_no_terminal_in_it() {
    let options = Options::default();
    let lines = render_plain(&options);
    assert!(!lines.is_empty());
    assert!(
        lines.iter().all(|l| !l.contains('\x1b')),
        "no escape sequences in --plain"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("queue") || l.contains("Queue")),
        "{lines:?}"
    );
}

// --- the focus model -----------------------------------------------------------------------

#[test]
fn focus_walks_the_declared_ring_and_wraps() {
    for view in ViewId::ALL {
        let mut app = app((100, 30));
        app.set_view(view);
        let ring = app.ring();
        assert!(
            !ring.is_empty(),
            "{view:?} has no focus ring: every view must have a pane 1"
        );
        assert_eq!(
            app.focused_pane().map(|p| p.name),
            Some(ring[0]),
            "{view:?} must start focused on pane 1"
        );
        for step in 1..=ring.len() {
            app.dispatch("pane.focus_next");
            let want = ring[step % ring.len()];
            assert_eq!(
                app.focused_pane().map(|p| p.name),
                Some(want),
                "{view:?} focus_next step {step}"
            );
        }
        for step in 1..=ring.len() {
            app.dispatch("pane.focus_prev");
            let want = ring[(ring.len() - step % ring.len()) % ring.len()];
            assert_eq!(
                app.focused_pane().map(|p| p.name),
                Some(want),
                "{view:?} focus_prev step {step}"
            );
        }
    }
}

#[test]
fn zoom_takes_the_focused_pane_to_the_whole_frame_and_gives_it_back() {
    let mut app = app((100, 30));
    app.dispatch("pane.focus_next");
    let focused = app.focused_pane().map(|p| p.name);
    app.dispatch("pane.zoom");
    assert!(app.zoomed, "<C-w>o should zoom");
    assert_eq!(
        app.ring().len(),
        1,
        "a zoomed frame has one pane in the ring, and `<Tab>` cannot reach the other"
    );
    assert_eq!(app.focused_pane().map(|p| p.name), focused);
    let lines = frame_lines(&app);
    assert!(
        !lines.iter().any(|l| l.contains('┬')),
        "a zoomed frame has no column rule: {lines:?}"
    );
    app.dispatch("pane.zoom");
    assert!(!app.zoomed);
    assert!(app.ring().len() > 1, "the other pane is back in the ring");
}

#[test]
fn the_interaction_mode_follows_the_pane_that_has_focus() {
    // ADR-006 §7.1: `list` mode where the focused pane is a list, `body` where it is a document.
    for view in views::VIEWS.iter().filter(|v| v.kind == "view") {
        let mut app = app((100, 30));
        app.set_view(
            ViewId::ALL
                .iter()
                .copied()
                .find(|id| format!("{id:?}").eq_ignore_ascii_case(view.id))
                .unwrap(),
        );
        assert_eq!(
            app.interaction(),
            view.entry_interaction(),
            "{} should open in its primary pane's mode",
            view.id
        );
        app.dispatch("pane.focus_next");
        if let Some(second) = view.panes.get(1) {
            let want = second.kind.interaction();
            assert_eq!(
                app.interaction(),
                want,
                "{}'s second pane is a {:?} surface",
                view.id,
                second.kind
            );
        }
    }
}

#[test]
fn switching_views_lands_on_pane_one_and_keeps_the_row_selected() {
    let mut app = app_with_rows((100, 30), 20);
    app.set_selection(7);
    app.dispatch("view.timeline");
    assert_eq!(app.view, ViewId::Timeline);
    assert_eq!(app.pane, 0, "a new view starts on pane 1 (§5.2)");
    app.dispatch("view.queue");
    assert_eq!(
        app.selected_row(),
        7,
        "the selection is per (view, pane) and survives a view switch (§5.0)"
    );
}

// --- tabs ----------------------------------------------------------------------------------

#[test]
fn a_pinned_tab_is_kept_and_the_switch_happens_beside_it() {
    let mut app = app((100, 30));
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].view, ViewId::Queue);
    assert!(
        !app.tabs[0].pinned,
        "the context you start in is the working one, not a pinned tab"
    );
    let bar = app.tab_bar();
    assert!(
        bar.contains("1 ▣ Queue") && bar.contains("(T pins this view)"),
        "{bar}"
    );
    // A view switch with nothing pinned replaces the working context: one tab, new view.
    app.dispatch("view.timeline");
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].view, ViewId::Timeline);
    // `T` pins it, and the hint becomes a statement.
    app.dispatch("tab.pin");
    assert!(app.tabs[0].pinned);
    assert!(app.tab_bar().contains("(pinned)"), "{}", app.tab_bar());
    // Now a switch has to open a context beside it — which is how §5's three tabs exist.
    app.dispatch("view.queue");
    assert_eq!(app.tabs.len(), 2, "a pinned tab is never overwritten");
    assert_eq!(app.tabs[0].view, ViewId::Timeline);
    assert_eq!(app.view, ViewId::Queue);
    assert_eq!(app.active_tab, 1);
}

#[test]
fn cycling_tabs_moves_the_live_context_and_closing_respects_the_pin() {
    let mut app = app((100, 30));
    app.dispatch("tab.pin");
    app.dispatch("view.timeline");
    app.dispatch("tab.pin");
    assert_eq!(app.tabs.len(), 2);
    app.dispatch("tab.next");
    assert_eq!(app.view, ViewId::Queue, "`J` cycles to the other tab");
    app.dispatch("tab.close");
    assert_eq!(app.tabs.len(), 2, "a pinned tab is not closed by `<C-w>c`");
    assert!(
        app.toast.as_deref().is_some_and(|t| t.contains("pinned")),
        "{:?}",
        app.toast
    );
    app.dispatch("tab.pin");
    assert!(!app.tabs[app.active_tab].pinned);
    app.dispatch("tab.close");
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.view, ViewId::Timeline);
    // The one remaining tab is pinned, so it is refused on that ground; unpin it and it is refused
    // because it is the last one standing.
    app.dispatch("tab.close");
    assert!(
        app.toast.as_deref().is_some_and(|t| t.contains("pinned")),
        "{:?}",
        app.toast
    );
    app.dispatch("tab.pin");
    app.dispatch("tab.close");
    assert_eq!(app.tabs.len(), 1);
    assert!(
        app.toast
            .as_deref()
            .is_some_and(|t| t.contains("the last tab stays")),
        "{:?}",
        app.toast
    );
}

#[test]
fn select_tab_reaches_the_ninth_tab_and_beyond_the_end_is_a_no_op() {
    let mut app = app((120, 40));
    app.dispatch("tab.pin");
    for view in [ViewId::Diff, ViewId::Graph, ViewId::Deps] {
        app.set_view(view);
        app.dispatch("tab.pin");
    }
    assert_eq!(app.tabs.len(), 4);
    app.feed_key("<C-w>3", Instant::now());
    assert_eq!(app.view, app.tabs[2].view, "<C-w>3 selects the third tab");
    app.feed_key("<C-w>9", Instant::now());
    assert_eq!(
        app.view, app.tabs[2].view,
        "a tab that does not exist must not move the live view"
    );
    assert!(
        app.toast.as_deref().is_some_and(|t| t.contains("no tab 9")),
        "and it says so: {:?}",
        app.toast
    );
}

// --- drill edges ---------------------------------------------------------------------------

#[test]
fn drill_in_pushes_focus_into_the_next_pane_and_esc_pops_it_back() {
    let mut app = app((100, 30));
    assert_eq!(app.pane, 0);
    app.dispatch("drill.in");
    assert_eq!(
        app.pane, 1,
        "`<Enter>` on the worklist drills to the card (§5.1)"
    );
    assert_eq!(app.drill.len(), 1);
    app.dispatch("drill.in");
    assert!(
        !app.drill.is_empty(),
        "drilling again does not lose the stack"
    );
    app.dispatch("drill.out");
    assert_eq!(app.pane, 0, "<Esc> pops back to the list");
    app.dispatch("drill.out");
    assert!(
        app.view == ViewId::Queue,
        "popping out of the last level stays in the view, it does not quit"
    );
    assert!(app.quit.is_none(), "nothing about <Esc> quits Fathom");
}

#[test]
fn history_back_walks_the_places_the_reviewer_has_been() {
    let mut app = app((100, 30));
    app.dispatch("view.graph");
    app.dispatch("view.deps");
    app.dispatch("history.back");
    assert_eq!(
        app.view,
        ViewId::Graph,
        "<C-o> returns to the previous place"
    );
    app.dispatch("history.back");
    assert_eq!(app.view, ViewId::Queue);
    app.dispatch("history.back");
    assert_eq!(
        app.view,
        ViewId::Queue,
        "back past the beginning stays at the beginning and says so"
    );
    assert!(
        app.toast
            .as_deref()
            .is_some_and(|t| t.contains("no earlier place")),
        "{:?}",
        app.toast
    );
}

// --- overlays and prompts ------------------------------------------------------------------

#[test]
fn the_help_overlay_draws_the_keymap_and_esc_leaves_the_view_as_it_was() {
    let mut app = app((100, 30));
    app.dispatch("help.open");
    assert!(app.overlay.is_some());
    let lines = frame_lines(&app);
    let text = lines.join("\n");
    // The overlay is the merged table, and a 30-row frame cannot show all of it: what has to be
    // true is that the overlay *is* that table from the top, and that the frame says how to leave.
    assert!(
        text.contains("keymap — the merged table") && text.contains("Esc closes"),
        "the overlay is the merged table: {text}"
    );
    assert!(
        text.contains("the three tasks") && text.contains("T1"),
        "the first thing the overlay answers is the three tasks:\n{text}"
    );
    assert!(
        app.keymap
            .help_lines(100)
            .iter()
            .any(|line| line.contains("view.queue")),
        "the table underneath has every action"
    );
    app.dispatch("overlay.close");
    assert_eq!(app.overlay, None);
    assert_eq!(app.view, ViewId::Queue);
}

#[test]
fn the_help_overlay_shows_the_override_instead_of_the_default() {
    // Criterion 3, seen through the frame rather than through the keymap object.
    let keymap = Keymap::merged(&[Override {
        mode: Mode::Global,
        chord: "x".to_owned(),
        action: Some("view.timeline".to_owned()),
        source: None,
    }]);
    let mut app = App::new(
        keymap,
        Model::from_facade(),
        (120, 40),
        "fathom/pilot · main",
    );
    app.dispatch("help.open");
    let text = frame_lines(&app).join("\n");
    assert!(
        text.contains("x -> view.timeline"),
        "the overlay must show what was applied:\n{text}"
    );
    // …and the hint line the user looks at without opening anything agrees with it.
    app.overlay = None;
    let hint = app.hint_line();
    assert!(
        !hint.contains("Mark reviewed"),
        "the hint line must not still advertise `x` as review.accept: {hint}"
    );
}

#[test]
fn the_palette_filters_and_runs_the_row_under_the_cursor() {
    let mut app = app((120, 40));
    app.dispatch("palette.open");
    assert!(app.overlay.is_some());
    let all = app.palette_rows().len();
    for ch in "timelin".chars() {
        app.feed_key(&ch.to_string(), Instant::now());
    }
    let filtered = app.palette_rows();
    assert!(
        filtered.len() < all && !filtered.is_empty(),
        "the filter should narrow {} rows to fewer",
        all
    );
    assert!(
        filtered
            .iter()
            .all(|row| row.label().to_lowercase().contains("timelin")),
        "{filtered:?}"
    );
    app.dispatch("prompt.submit");
    assert_eq!(app.view, ViewId::Timeline, "Enter runs the selected row");
    assert_eq!(app.overlay, None);
    assert!(app.prompt.is_empty());
}

#[test]
fn the_search_field_owns_the_keyboard_and_does_not_move_the_list() {
    let mut app = app((120, 40));
    let before = app.selected_row();
    app.dispatch("search.open");
    assert_eq!(
        app.interaction(),
        Mode::Prompt,
        "an exclusive interaction mode replaces the chain (§7.1)"
    );
    for ch in "jqk".chars() {
        app.feed_key(&ch.to_string(), Instant::now());
    }
    assert_eq!(
        app.prompt, "jqk",
        "in the field, `j` and `k` are letters, not motion"
    );
    assert_eq!(app.selected_row(), before, "and the list did not move");
    app.dispatch("prompt.backspace");
    assert_eq!(app.prompt, "jq");
    app.dispatch("prompt.submit");
    assert_eq!(app.overlay, None);
    assert!(
        app.toast.as_deref().is_some_and(|toast| {
            let _ = toast;
            toast.contains("no index") || toast.contains("fathom index")
        }),
        "a box with nothing to search says what would make it answer: {:?}",
        app.toast
    );
    app.feed_key("<Esc>", Instant::now());
    assert_eq!(app.overlay, None);
}

/// Type into a field the way a keyboard does it: one key event per character, so the space bar
/// arrives as the chord `<Space>` (`keymap::chord_of`) rather than as a text-shaped shortcut that
/// only the test suite speaks.
fn type_text(app: &mut App, text: &str) {
    use crossterm::event::KeyEvent;
    for ch in text.chars() {
        app.feed_key_event(
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            Instant::now(),
        );
    }
}

#[test]
fn the_space_bar_is_text_in_every_prompt() {
    // ADR-006 §7.1: an exclusive mode owns the keyboard and everything it does not bind is text.
    // The space bar is the keystroke that was easy to lose — the terminal spells it `<Space>` — and
    // losing it is not cosmetic: every query §5.2 documents is multi-token, so a field that eats
    // the space cannot express one filter token, let alone the free text that composes with them.
    let query = "author:mira path:src/harbour";

    // `/` — the search box.
    let mut search = app((120, 40));
    search.dispatch("search.open");
    type_text(&mut search, query);
    assert_eq!(
        search.prompt, query,
        "a two-token query survives the keystrokes that typed it"
    );
    assert!(
        !search
            .toast
            .as_deref()
            .is_some_and(|toast| toast.contains("<Space>")),
        "the space bar is not an unbound key in a field: {:?}",
        search.toast
    );

    // `,` — the palette filter, whose matcher already splits the query on whitespace and so has
    // always assumed a space a reviewer could not type.
    let mut palette = app((120, 40));
    palette.dispatch("palette.open");
    type_text(&mut palette, "focus next");
    assert_eq!(palette.prompt, "focus next");
    let rows = palette.palette_rows();
    assert!(
        !rows.is_empty()
            && rows.iter().all(|row| {
                let label = row.label().to_lowercase();
                label.contains("focus") && label.contains("next")
            }),
        "both tokens filter, the space included: {:?}",
        rows.iter().map(|row| row.label()).collect::<Vec<_>>()
    );

    // `F` — the filter chooser, `filter.edit`: a prompt with no keys of its own (§7.4).
    let mut filter = app((120, 40));
    filter.dispatch("filter.edit");
    type_text(&mut filter, "epoch store");
    assert_eq!(
        filter.prompt, "epoch store",
        "and the `F` field is a field too"
    );

    // The published chord spelling arrives at the same place as the keystroke.
    let mut spelled = app((120, 40));
    spelled.dispatch("search.open");
    for chord in ["a", "<Space>", "b"] {
        spelled.feed_key(chord, Instant::now());
    }
    assert_eq!(spelled.prompt, "a b");

    // And the field can take the space back: `<BS>` deletes it like any other character.
    spelled.dispatch("prompt.backspace");
    assert_eq!(
        spelled.prompt, "a ",
        "one keystroke deletes one character, space or not"
    );
    spelled.dispatch("prompt.backspace");
    assert_eq!(spelled.prompt, "a");
}

#[test]
fn an_unbound_key_toasts_instead_of_doing_nothing_silently() {
    let mut app = app((100, 30));
    app.feed_key("\u{1}", Instant::now()); // <C-a>, reserved for the user and unbound
    assert!(
        app.toast
            .as_deref()
            .is_some_and(|t| t.contains("not bound") || t.contains("unbound")),
        "{:?}",
        app.toast
    );
    let frames = frame_lines(&app).join("\n");
    assert!(
        frames.contains("not bound") || frames.contains("unbound"),
        "the toast reaches the frame: {frames}"
    );
}

// --- how the loop ends ---------------------------------------------------------------------

#[test]
fn control_c_quits_through_the_keymap_and_restores_in_order() {
    // With crossterm's raw mode ISIG is off (ADR-006 §8.1 measured it), so Ctrl-C arrives as the
    // byte 0x03 and `app.quit` is reached as a binding; the signal path is the other door.
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut shell = app((100, 30));
    shell.feed_key_event(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        Instant::now(),
    );
    assert_eq!(shell.quit.as_deref(), Some("app.quit"));
    let mut again = app((100, 30));
    again.feed_key("<C-c>", Instant::now());
    assert_eq!(again.quit.as_deref(), Some("app.quit"));
}

#[test]
fn the_quit_chord_survives_the_modes_that_own_the_keyboard() {
    // §1.1 hands an exclusive mode the whole keyboard, and §8.1 measured a terminal in which
    // `<C-c>` is a signal and never a keystroke, so an overlay swallowing it was invisible on
    // paper. On crossterm's raw mode it *is* a byte — measured on a real pty while writing this —
    // and "Ctrl-C never leaves the terminal in alt-screen" is worth nothing if the help overlay or
    // the search box eats the only quit chord.
    let mut help = app((100, 30));
    help.feed_key("?", Instant::now());
    assert!(help.overlay.is_some(), "the overlay is open");
    help.feed_key("<C-c>", Instant::now());
    assert_eq!(
        help.quit.as_deref(),
        Some("app.quit"),
        "quit outranks the overlay"
    );

    let mut search = app((100, 30));
    search.dispatch("search.open");
    search.feed_key("x", Instant::now());
    assert_eq!(search.prompt, "x", "a printable key is still the field's");
    search.feed_key("<C-c>", Instant::now());
    assert_eq!(
        search.quit.as_deref(),
        Some("app.quit"),
        "and quit still outranks the field"
    );
}

#[test]
fn a_batch_of_keystrokes_draws_one_frame_and_a_held_key_costs_one_frame_per_batch() {
    let mut app = app((100, 30));
    let bus = fathom_tui::event::Bus::new();
    for ch in "jjjjkkk".chars() {
        bus.send(fathom_tui::event::Message::Key(key(ch)));
    }
    bus.send(fathom_tui::event::Message::Resize { cols: 90, rows: 28 });
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: None,
    };
    let report = run_loop(&mut app, &bus, &mut sink, None, Some(1)).expect("loop");
    assert_eq!(report.end, fathom_tui::frame::LoopEnd::FrameLimit);
    assert_eq!(
        sink.frames.len(),
        1,
        "seven keys and a resize in one batch is one frame (ADR-006 §0.1)"
    );
    assert_eq!(report.events, 8);
    assert_eq!(app.size, (90, 28), "the resize in the batch was applied");
}

/// A plain keystroke, the way the input reader would deliver it.
fn key(ch: char) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
}

#[test]
fn the_loop_applies_every_key_in_order_before_drawing() {
    let mut app = app_with_rows((100, 30), 20);
    let bus = fathom_tui::event::Bus::new();
    for ch in "jjjj".chars() {
        bus.send(fathom_tui::event::Message::Key(key(ch)));
    }
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: None,
    };
    run_loop(&mut app, &bus, &mut sink, None, Some(1)).expect("loop");
    assert_eq!(app.selected_row(), 4, "four `j` presses, applied in order");
}

#[test]
#[allow(clippy::print_stderr)] // the criterion is a number, and a passing assert that hides it teaches nobody the margin
fn a_held_key_stays_under_the_frame_budget_with_an_empty_model() {
    // Criterion 4: p99 under 33 ms with nothing in the model. The measurement is the renderer's,
    // so this is the number that matters before any pane owns real content.
    let mut app = app((100, 30));
    let bus = fathom_tui::event::Bus::new();
    let sender = bus.sender();
    let writer = std::thread::spawn(move || {
        for batch in 0..40u32 {
            for ch in "jjjj".chars() {
                if sender
                    .send(fathom_tui::event::Message::Key(key(ch)))
                    .is_err()
                {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(u64::from(2 + batch % 3)));
        }
    });
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: None,
    };
    // A frame limit rather than "run forever": the bus keeps a sender of its own, so the loop only
    // ends on a quit, a signal or the limit (`run_loop`'s own contract).
    let report = run_loop(&mut app, &bus, &mut sink, None, Some(40)).expect("loop");
    writer.join().expect("writer");
    eprintln!(
        "frame budget: {} frames, {} events, p99 {:.2} ms, worst {:.2} ms, over-budget {}",
        report.frames,
        report.events,
        f64::from(report.p99.as_micros() as f32) / 1000.0,
        f64::from(report.worst.as_micros() as f32) / 1000.0,
        report.over_budget
    );
    assert!(
        report.frames >= 5,
        "the run should have drawn several frames: {report:?}"
    );
    assert!(
        report.p99 < std::time::Duration::from_millis(33),
        "p99 {} ms over {} frames: {report:?}",
        f64::from(report.p99.as_micros() as f32) / 1000.0,
        report.frames
    );
    assert!(report.events >= 120, "{report:?}");
    assert!(
        !app.model.has_rows(),
        "an empty model is what this measures"
    );
}

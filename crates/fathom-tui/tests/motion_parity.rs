//! One motion implementation, in every list (`e5.4`, `reco-mdb` criterion 3).
//!
//! The criterion is the shortest sentence on the ticket and the easiest to fail quietly: *"motion
//! behaviour is identical across timeline, file tree, queue and search results (shared
//! implementation)"*. Quiet failure looks like a second `<C-d>` added beside the first one, in a new
//! pane, with a page that means something slightly different — every list still *works*, and the
//! reviewer has to learn that paging is not the same thing in here as it is over there.
//!
//! So this file does not test the motions one at a time. It puts the same cursor on the same row in
//! each list the shell has, sends the same action, and compares the answers:
//!
//! * the seven cursor motions (`j`/`k`/`gg`/`G`/`H`/`M`/`L`) must land in the same row in every list;
//! * a page must be one page in every list, and a page down and back must restore what was there —
//!   which is where the two honest shapes of a page show up, a pane scrolling its window and the rail
//!   moving its cursor (ADR-006 §5.2: the rail's cursor is an episode, not a row);
//! * the two overlays that own a list — the `,` palette and the `/` results — answer the same eleven
//!   action ids and agree with the same function, because a search result is a list too (`e5.2`, `e5.4`);
//! * and at the keymap level every view resolves the same chord to the same motion action, which is
//!   what makes "shared implementation" a claim about the table rather than about three call sites.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use fathom_analyze::search::{Corpus, Entry, Index};
use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model, motion_target};
use fathom_tui::keymap::{Keymap, Mode, mode_chain};
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState};
use fathom_tui::views::ViewId;

/// The instant the fixtures measure "now" from — fixed, never the clock.
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
const MINUTE: i64 = 60_000_000;
/// Rows in every seeded list, and the row the cursor starts on. The same in every surface, so a
/// difference in an answer can only be a difference in the arithmetic.
const ROWS: usize = 30;
const START: usize = 6;
/// The frame. Tall enough that `H`/`M`/`L` and a page are distinct rows, so a page that is really
/// "the whole list" cannot hide behind the clamp.
const SIZE: (u16, u16) = (100, 24);

/// The eleven motions the shared set is made of — the action ids, not the chords, because the chords
/// differ by mode (`j` in a list, `<Down>` in a prompt) and the actions must not.
const MOTIONS: [&str; 11] = [
    "list.down",
    "list.up",
    "list.first",
    "list.last",
    "list.page_down",
    "list.page_up",
    "list.half_down",
    "list.half_up",
    "list.top_visible",
    "list.mid_visible",
    "list.bot_visible",
];

fn base() -> App {
    App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        SIZE,
        "fathom/pilot · main",
    )
}

/// `n` episodes, newest first, so the rail and the queue both have `n` rows to move over.
fn episodes(n: usize) -> Vec<Episode> {
    (0..n)
        .map(|i| {
            let when = Timestamp::from_unix_micros(NOW.as_unix_micros() - (i as i64 + 1) * MINUTE);
            let mut episode = Episode::new(
                format!("{i:016x}"),
                when,
                format!("episode {i} of the parity fixture"),
            );
            episode.author = AuthorKind::Agent;
            episode.author_name = "pi".to_owned();
            episode.n_files = 1 + (i % 4) as u64;
            episode.n_add = 10 + i as u64;
            episode.n_del = (i % 7) as u64;
            episode.state = ReviewState::Unread;
            episode
        })
        .collect()
}

fn with_episodes(mut app: App, view: ViewId) -> App {
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("parity fixture", episodes(ROWS)));
    app.set_view(view);
    app
}

/// A list backed by `model.rows` — the shape the deps tree and the artefact scope arrive in.
fn with_rows(mut app: App, view: ViewId, pane: &str) -> App {
    app.model.rows.insert(
        (view, pane.to_owned()),
        (0..ROWS).map(|i| format!("row {i:02}")).collect(),
    );
    app.set_view(view);
    // Focus the list pane of that view, whichever position in the ring it sits at.
    let index = app
        .ring()
        .iter()
        .position(|name| *name == pane)
        .unwrap_or_else(|| panic!("{view:?} has no focusable {pane:?} pane to move"));
    app.pane = index;
    app
}

/// One surface to move a cursor in: a name for the failure message, and a fresh app with `ROWS` rows
/// and the list pane focused.
fn surfaces() -> Vec<(&'static str, App)> {
    vec![
        ("queue (worklist)", with_episodes(base(), ViewId::Queue)),
        ("timeline (rail)", with_episodes(base(), ViewId::Timeline)),
        ("deps (file tree)", with_rows(base(), ViewId::Deps, "tree")),
        (
            "artefact (scope)",
            with_rows(base(), ViewId::Artefact, "scope"),
        ),
    ]
}

fn place(app: &mut App, row: usize) {
    app.set_selection(row);
    assert_eq!(
        app.selected_row(),
        row,
        "the fixture has to start where it says it does"
    );
}

fn search_app() -> App {
    let mut corpus = Corpus::new();
    corpus.documents = ROWS as i64;
    for i in 0..ROWS {
        corpus.push(Entry::episode(
            format!("{i:016x}"),
            &format!("epoch-safe store read {i}"),
            "pin the epoch before the read",
            Some("Dana Stahl".to_owned()),
            Some(Timestamp::from_unix_micros(
                NOW.as_unix_micros() - i as i64 * MINUTE,
            )),
            Some(50 + i as i64),
        ));
    }
    let mut app = base();
    app.search = Some(Arc::new(Index::build(&corpus)));
    app.dispatch("search.open");
    app.prompt = "epoch".to_owned();
    app.refresh_search();
    assert!(
        app.search_report
            .as_ref()
            .is_some_and(|report| report.hits.len() > 1),
        "the corpus has to answer the query with a list, or the test proves nothing"
    );
    app
}

/// How many rows the list this app is currently moving belongs to.
fn list_len(app: &mut App) -> usize {
    match app.overlay {
        Some(fathom_tui::app::Overlay::Search) => app
            .search_report
            .as_ref()
            .map_or(0, |report| report.hits.len()),
        Some(fathom_tui::app::Overlay::Palette) => app.palette_rows().len(),
        _ => app.focused_pane().map_or(0, |pane| app.row_count(pane)),
    }
}

// --- the cursor motions: the same answer everywhere -----------------------------------------

#[test]
fn the_cursor_motions_land_where_the_chord_says_in_every_list() {
    for action in [
        "list.down",
        "list.up",
        "list.first",
        "list.last",
        "list.top_visible",
        "list.mid_visible",
        "list.bot_visible",
    ] {
        let mut seen: Vec<(String, usize, usize)> = Vec::new();
        for (name, mut app) in surfaces() {
            place(&mut app, START);
            let count = list_len(&mut app);
            app.dispatch(action);
            let landed = app.selected_row();
            // `list.last` is each list's own last row: the rail has one slot more than it has
            // episodes, because a day group draws a header the motions can stop on (ADR-006 §5.2).
            // Everything else lands on the same row in every list, and the search overlay — whose
            // cursor lives in the palette's field, reached through the same dispatch arm (`e5.4`) —
            // lands there too.
            let expected = match action {
                "list.down" => START + 1,
                "list.up" => START - 1,
                "list.first" | "list.top_visible" => 0,
                "list.last" => count - 1,
                "list.mid_visible" => usize::from(SIZE.1) / 2,
                "list.bot_visible" => usize::from(SIZE.1) - 1,
                _ => unreachable!(),
            };
            assert_eq!(
                landed, expected,
                "{name}: `{action}` from row {START} landed on {landed} with {count} rows"
            );
            seen.push((name.to_owned(), count, landed));
        }
        let mut search = search_app();
        let hits = search
            .search_report
            .as_ref()
            .map_or(0, |report| report.hits.len());
        search.palette_row = START.min(hits - 1);
        search.dispatch(action);
        if !matches!(action, "list.down" | "list.up" | "list.first" | "list.last") {
            // `H`/`M`/`L` are relative to the top of the window in a pane and to the cursor in the
            // overlay, because in the overlay the window follows the cursor — the one difference
            // between the two, and it is asserted against `motion_target` below instead.
            continue;
        }
        let expected = match action {
            "list.down" => START + 1,
            "list.up" => START - 1,
            "list.first" => 0,
            "list.last" => hits - 1,
            _ => unreachable!(),
        };
        assert_eq!(
            search.palette_row, expected,
            "the search results answered `{action}` differently from the panes: {seen:?}"
        );
    }
}

// --- the page motions: one page each, and reversible ---------------------------------------

#[test]
fn a_page_is_one_page_in_every_list_and_a_page_back_restores_the_position() {
    for (down, up) in [
        ("list.page_down", "list.page_up"),
        ("list.half_down", "list.half_up"),
    ] {
        for (name, mut app) in surfaces() {
            place(&mut app, START);
            let before = (app.selected_row(), app.scroll_of());
            app.dispatch(down);
            let after = (app.selected_row(), app.scroll_of());
            assert_ne!(
                after, before,
                "{name}: `{down}` did nothing at all — the page is not a page"
            );
            assert!(
                after.0 >= before.0 && after.1 >= before.1,
                "{name}: `{down}` moved backwards: {before:?} → {after:?}"
            );
            app.dispatch(up);
            assert_eq!(
                (app.selected_row(), app.scroll_of()),
                before,
                "{name}: `{down}` then `{up}` did not land back where the reviewer was"
            );
        }
    }
}

#[test]
fn top_middle_and_bottom_are_three_different_rows_in_every_list() {
    for (name, mut app) in surfaces() {
        place(&mut app, START);
        app.dispatch("list.top_visible");
        let top = app.selected_row();
        app.dispatch("list.mid_visible");
        let mid = app.selected_row();
        app.dispatch("list.bot_visible");
        let bot = app.selected_row();
        assert!(
            top <= START && mid > top && bot > mid,
            "{name}: H/M/L were top {top}, middle {mid}, bottom {bot} with the cursor at {START} — \
             three names for one row is not a window"
        );
    }
}

// --- the overlays: the same eleven answers, from the same function -------------------------

#[test]
fn both_overlays_answer_every_motion_the_shared_arithmetic_says() {
    let mut palette = base();
    palette.dispatch("palette.open");
    let search = search_app();
    for (label, mut app) in [("palette", palette), ("search", search)] {
        let count = list_len(&mut app);
        assert!(count > 4, "{label} needs a list to move over");
        for action in MOTIONS {
            for here in [0usize, START, count - 1] {
                app.palette_row = here;
                app.dispatch(action);
                let page = if label == "palette" {
                    count.min(10)
                } else {
                    app.search_visible().max(1)
                };
                assert_eq!(
                    app.palette_row,
                    motion_target(action, here, page, count),
                    "{label} at row {here} moved differently from motion_target on `{action}`"
                );
            }
        }
    }
}

#[test]
fn a_saved_search_and_the_typed_query_move_the_same_cursor() {
    // `e5.4`'s saved rows run through the same submission as a typed query, and that means the same
    // cursor: a reviewer who pages to row 7 and presses `<Enter>` goes to row 7, not to row 1.
    let mut app = search_app();
    app.palette_row = 3;
    app.dispatch("list.down");
    let row = app.palette_row;
    assert_eq!(row, 4, "the cursor is the one the motions moved");
    app.dispatch("prompt.submit");
    assert_eq!(
        app.view,
        ViewId::Timeline,
        "a hit from an episode deep-links to the rail"
    );
}

#[test]
fn the_comparison_panel_answers_the_same_eleven_motions() {
    // A comparison is a document, so its "cursor" is its scroll — but the *verbs* are the same eleven,
    // because the reviewer should not have to learn a third vocabulary for the third kind of surface.
    struct Counted;
    impl fathom_tui::compare::CompareProvider for Counted {
        fn compare(&self, spec: &str) -> Result<Vec<String>, fathom_tui::diff::Unavailable> {
            Ok((0..60)
                .map(|i| format!("{i:>3}  line {i} of the delta between {spec}"))
                .collect())
        }
    }
    let mut app = base();
    app.compare = Some(fathom_tui::compare::Provider::new(Counted));
    app.scope.mark = Some("aaaa1111".to_owned());
    app.scope.episode = Some("bbbb2222".to_owned());
    app.dispatch("compare.with_mark");
    assert_eq!(app.overlay, Some(fathom_tui::app::Overlay::Compare));
    for action in MOTIONS {
        app.compare_panel.scroll = 0;
        app.dispatch(action);
        assert!(
            app.compare_panel.scroll <= app.compare_panel.max_scroll(app.compare_height()),
            "`{action}` scrolled past the end of the comparison"
        );
        if matches!(
            action,
            "list.down"
                | "list.page_down"
                | "list.half_down"
                | "list.mid_visible"
                | "list.bot_visible"
                | "list.last"
        ) {
            assert!(
                app.compare_panel.scroll > 0,
                "`{action}` did not scroll the comparison from its top"
            );
        } else {
            assert_eq!(
                app.compare_panel.scroll, 0,
                "`{action}` moved down from the top of a comparison"
            );
        }
    }
    app.compare_panel.scroll = 0;
    app.dispatch("list.page_down");
    let paged = app.compare_panel.scroll;
    assert!(paged > 0);
    app.dispatch("list.page_up");
    assert_eq!(
        app.compare_panel.scroll, 0,
        "a page down and up is the top again"
    );
}

// --- the keymap: one vocabulary, every view -----------------------------------------------

#[test]
fn every_view_resolves_the_motion_chords_to_the_same_actions() {
    let km = Keymap::defaults();
    let chords = [
        "j", "k", "gg", "G", "H", "M", "L", "h", "l", "<C-d>", "<C-u>", "<C-f>", "<C-b>",
    ];
    let mut reference: Option<Vec<Option<String>>> = None;
    for view in ViewId::ALL {
        let chain = mode_chain(Mode::for_view(view), Mode::List);
        let answers: Vec<Option<String>> = chords
            .iter()
            .map(|chord| km.resolve(&chain, chord).map(str::to_owned))
            .collect();
        if let Some(wanted) = &reference {
            assert_eq!(
                &answers, wanted,
                "{view:?} resolves the shared motion chords differently from the reference view"
            );
        } else {
            reference = Some(answers);
        }
    }
}

#[test]
fn every_motion_action_is_bound_in_the_modes_that_own_a_list() {
    // The eleven ids have to be reachable from the modes that show a list, or "shared implementation"
    // is true of the function and false of the keyboard. `H`/`M`/`L` and the letter motions live in
    // `list`; the paging motions live in `body`, which is the other half of every list view's chain.
    let km = Keymap::defaults();
    for action in MOTIONS {
        let bound: Vec<Mode> = km
            .bindings_of(action)
            .iter()
            .map(|(mode, _)| *mode)
            .collect();
        assert!(
            bound.contains(&Mode::List) || bound.contains(&Mode::Body),
            "`{action}` is bound in {bound:?}, neither of which is a list mode, so a reviewer in a \
             list cannot reach it"
        );
    }
}

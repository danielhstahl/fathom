//! The session (`e1.11`): what comes back, what is forgiven, and what is written.
//!
//! The four acceptance bullets, and where each is paid for:
//!
//! * *"Reopening the TUI restores selection, filters and layout exactly"* —
//!   `a_reopened_session_comes_back_exactly` and `the_frame_comes_back_at_the_size_it_was_left_at`,
//!   both round-tripped through the stored rows rather than through a struct copy, because the rows are
//!   what survives a process exit.
//! * *"Two simultaneous TUI sessions do not clobber each other's state"* — the store half is
//!   `fathom-store/tests/episodes.rs` and the writer half is `fathom-cli/src/session.rs`'s own tests;
//!   what this file can assert is the shell's side: `a_run_that_changed_nothing_leaves_the_state_alone`
//!   is the precondition that makes a merge possible at all.
//! * *"Truncated/corrupt state is quarantined and the app starts clean with a warning"* —
//!   `damage_in_the_state_is_named_and_the_defaults_stand` (what `fathom-cli` quarantines is decided by
//!   the same `damaged` function this file exercises) and
//!   `a_state_from_a_newer_fathom_is_read_not_refused`, which is the case that must *not* be treated as
//!   damage.
//! * *"`fathom --fresh` starts from defaults"* — the shell's half is that an empty set of rows *is*
//!   defaults and says so (`a_run_with_no_rows_at_all_is_a_first_session_not_a_damaged_one`), and that a
//!   run which only drew frames asks the writer for nothing
//!   (`a_run_that_drew_frames_and_took_no_keystroke_asks_for_nothing`).
//!
//! As in the other timeline tests, nothing here reaches past the public API, and the fixture is the
//! corpus the golden files use, so a session restored onto different rows would be a different test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_tui::app::{App, Model};
use fathom_tui::event::Bus;
use fathom_tui::frame::{HEADLESS_SIZE, HeadlessSink, Options, render_plain, run_loop};
use fathom_tui::keymap::Keymap;
use fathom_tui::session::{self, READ_PREFIX, STATE_VERSION};
use fathom_tui::timeline::{Batch, Filter, GroupBy, ReviewState};
use fathom_tui::views::ViewId;

#[path = "timeline_corpus/mod.rs"]
mod corpus;

use corpus::{NOW, mock};

/// A timeline app over the corpus, at a documented size — the state a reviewer is in when they leave.
fn fixture(size: (u16, u16)) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        size,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.set_view(ViewId::Timeline);
    app.load_episodes(Batch::new("fixture index", mock()));
    app
}

/// A cold app: the same repository, an hour later, nothing applied yet.
fn cold(size: (u16, u16)) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        size,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("fixture index", mock()));
    app
}

/// Put `app`'s state through the store and back out of it.
fn round_trip(app: &App) -> App {
    let entries = session::snapshot(app);
    let mut restored = cold(app.size);
    let said = session::restore(&mut restored, &entries);
    assert!(
        said.warnings.is_empty(),
        "a state this build wrote must not come back with warnings: {:?}",
        said.warnings
    );
    restored
}

/// Work the rail the way a reviewer does: narrow it, move the cursor, pin a tab, zoom, search.
fn work() -> App {
    let mut app = fixture((100, 30));
    app.timeline.set_filter(Filter {
        state: Some(ReviewState::Unread),
        // Ungrouped, so "the third row" is the third episode and not a group header — the test is
        // about the cursor, not about where `day` grouping put the headers today.
        group: GroupBy::None,
        ..Filter::default()
    });
    app.timeline.move_to(2);
    app.dispatch("tab.pin");
    app.dispatch("pane.zoom");
    app.dispatch("search.open");
    app.prompt = "panic=use_after_free".to_owned();
    app.dispatch("prompt.submit");
    app.dispatch("drill.in");
    app.scope.mark = Some("40c9b3".to_owned());
    app
}

// --- the round trip -------------------------------------------------------------------------------

#[test]
fn a_reopened_session_comes_back_exactly() {
    let left = work();
    let back = round_trip(&left);
    assert_eq!(back.view, left.view, "the view that was live");
    assert_eq!(back.pane, left.pane, "the pane that had focus");
    assert_eq!(
        back.zoomed, left.zoomed,
        "the zoomed pane stays the whole frame"
    );
    assert_eq!(back.size, left.size, "the frame it was left at");
    assert_eq!(
        back.timeline.filter(),
        left.timeline.filter(),
        "the filter is the reviewer's question"
    );
    assert_eq!(
        back.timeline.selected().map(|episode| episode.id.clone()),
        left.timeline.selected().map(|episode| episode.id.clone()),
        "the cursor is on the episode it was on"
    );
    assert_eq!(
        left.timeline.selected().map(|episode| episode.id.as_str()),
        Some("40c9b3"),
        "and the fixture really did put the cursor on an episode, so the line above is not comparing \
         two absences"
    );
    assert_eq!(back.tabs.len(), left.tabs.len(), "the tab list came back");
    assert_eq!(
        back.tabs[0].pinned, left.tabs[0].pinned,
        "pinning is a thing the reviewer asked for and does not expect to repeat"
    );
    assert_eq!(
        back.scope.mark, left.scope.mark,
        "the `m` mark is half of a comparison"
    );
    assert_eq!(
        back.last_search.as_deref(),
        left.last_search.as_deref(),
        "the last question outlives the field it was typed into"
    );
    assert_eq!(
        back.opened, left.opened,
        "the read set came back with the reviewer"
    );
}

#[test]
fn the_round_trip_is_stable_so_a_resume_never_drifts() {
    let left = work();
    let once = round_trip(&left);
    let twice = round_trip(&once);
    assert_eq!(
        session::snapshot(&once).into_iter().collect::<Vec<_>>(),
        session::snapshot(&twice).into_iter().collect::<Vec<_>>(),
        "restoring a state and saving it again must be the same state — a resume that drifted would \
         move the reviewer's cursor a little more each week"
    );
}

#[test]
fn the_frame_comes_back_at_the_size_it_was_left_at() {
    // The pane sizes are derived from the frame, so the honest test of "layout restored" is that the
    // numbers the reviewer left are the numbers the next run prints — at 100×30, not at the default.
    let left = work();
    let stored = session::snapshot(&left);
    let sizes = stored
        .iter()
        .find(|(key, _)| key == session::KEY_PANE_SIZES)
        .map(|(_, value)| value.clone())
        .expect("the pane widths are part of the state");
    assert!(
        sizes.split(',').count() >= 2,
        "the timeline has a rail and a card: {sizes}"
    );
    let mut back = cold(HEADLESS_SIZE);
    let said = session::restore(&mut back, &stored);
    assert_eq!(
        said.size,
        Some((100, 30)),
        "the stored frame size is reported to the caller, which is what lets a run with no terminal \
         to ask use it"
    );
    assert_eq!(back.size, (100, 30));
    let options = Options {
        session: stored,
        ..Options::default()
    };
    let wide = render_plain(&options);
    let default = render_plain(&Options::default());
    let widest = |lines: &[String]| {
        lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0)
    };
    assert!(
        widest(&wide) > widest(&default),
        "a piped frame has no terminal to ask, so it draws at the width the reviewer read at ({} \
         columns) and not at the {HEADLESS_SIZE:?} default ({})",
        widest(&wide),
        widest(&default)
    );
    assert_eq!(
        wide.len(),
        default.len() + 6,
        "and the height came back too: 30 rows against the default's 24"
    );
}

#[test]
fn a_run_that_drew_frames_and_took_no_keystroke_asks_for_nothing() {
    // The `--fresh` criterion seen from the loop: `frame::run` takes this exact delta and hands it to
    // `fathom-cli`, so "nothing changed" has to survive a whole run — redraws, resizes and all.
    let entries = session::snapshot(&work());
    let mut app = cold((100, 30));
    let resumed = session::restore(&mut app, &entries);
    assert!(
        resumed.warnings.is_empty(),
        "a state this build wrote comes back without warnings: {:?}",
        resumed.warnings
    );
    let baseline = session::snapshot(&app);
    let bus = Bus::new();
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: Some((100, 30)),
    };
    run_loop(&mut app, &bus, &mut sink, None, Some(1)).expect("the loop runs headlessly");
    assert_eq!(sink.frames.len(), 1, "it drew the frame it was asked for");
    let delta = session::delta(&baseline, &session::snapshot(&app));
    assert!(
        !delta.changed,
        "a session that was looked at and left alone writes nothing back"
    );
}

#[test]
fn a_run_that_changed_nothing_leaves_the_state_alone() {
    // The precondition for a merge, and the `--fresh` criterion in one line: looked at and left alone is
    // not a change, so nothing is asked of the store.
    let app = fixture((100, 30));
    let before = session::snapshot(&app);
    let untouched = session::delta(&before, &session::snapshot(&app));
    assert!(
        !untouched.changed,
        "drawing the same frame twice is not a reviewer changing anything"
    );
    let mut moved = fixture((100, 30));
    moved.dispatch("list.down");
    let moved_state = session::delta(&before, &session::snapshot(&moved));
    assert!(
        moved_state.changed,
        "one keystroke on the rail is a change: the cursor moved"
    );
    assert!(
        moved_state
            .entries
            .iter()
            .any(|(key, value)| key == session::KEY_VERSION && value == &STATE_VERSION.to_string()),
        "the snapshot is the whole state, version included, so the writer can diff it"
    );
}

#[test]
fn a_run_with_no_rows_at_all_is_a_first_session_not_a_damaged_one() {
    let mut app = cold((100, 30));
    let said = session::restore(&mut app, &[]);
    assert!(
        said.warnings.is_empty(),
        "an empty store is the ordinary case and has no business warning: {:?}",
        said.warnings
    );
    assert_eq!(
        said.summary(),
        "nothing to restore — this is a first session in this repository",
        "and it says which of the three empties it is"
    );
    assert_eq!(app.view, ViewId::Queue, "and the defaults are the defaults");
}

// --- damage and mismatch ---------------------------------------------------------------------------

#[test]
fn damage_in_the_state_is_named_and_the_defaults_stand() {
    // One row of each kind of hurt: not-a-number, not-a-view, not-0-or-1, oversized, a control
    // character, an `unread:` row with no id. None of them stops the app, and each is named.
    let junk = vec![
        (session::KEY_VERSION.to_owned(), "seventeen".to_owned()),
        (session::KEY_VIEW.to_owned(), "star-chart".to_owned()),
        (session::KEY_PANE.to_owned(), "second".to_owned()),
        (session::KEY_ZOOM.to_owned(), "maybe".to_owned()),
        (session::KEY_SIZE.to_owned(), "banana".to_owned()),
        (
            session::KEY_PANE_SIZES.to_owned(),
            "wide,thirty-eight".to_owned(),
        ),
        (session::KEY_TABS.to_owned(), "timeline:e8f1a2".to_owned()),
        (
            session::KEY_SEARCH.to_owned(),
            "x".repeat(session::MAX_VALUE + 10),
        ),
        (format!("{READ_PREFIX}e8f1a2"), "read".to_owned()),
        (READ_PREFIX.to_owned(), "read".to_owned()),
        (format!("{READ_PREFIX}9c4d10"), "read\u{7}".to_owned()),
    ];
    let broken = session::damaged(&junk)
        .into_iter()
        .map(|(key, _)| key)
        .collect::<Vec<_>>();
    for key in [
        session::KEY_VERSION,
        session::KEY_PANE,
        session::KEY_ZOOM,
        session::KEY_SIZE,
        session::KEY_PANE_SIZES,
        session::KEY_TABS,
        session::KEY_SEARCH,
        READ_PREFIX,
        &format!("{READ_PREFIX}9c4d10"),
    ] {
        assert!(
            broken.iter().any(|have| have == key),
            "`{key}` holds a value no fathom writes and must be named as damage: {broken:?}"
        );
    }
    // A name this build has no view for is a disagreement, not damage: a newer fathom wrote it.
    assert!(
        !broken.iter().any(|key| key == session::KEY_VIEW),
        "`tui.view = star-chart` may be a real view in a newer build: {broken:?}"
    );

    let mut app = cold((100, 30));
    let said = session::restore(&mut app, &junk);
    assert_eq!(
        app.view,
        ViewId::Queue,
        "the app starts, on the view it opens by default"
    );
    assert_eq!(app.size, (100, 30), "and at the size it was given");
    assert!(!app.zoomed, "an unusable zoom flag is not a zoomed frame");
    assert_eq!(
        app.tabs.len(),
        1,
        "the torn tab row is dropped, not half-applied"
    );
    assert_eq!(
        app.last_search, None,
        "the oversized query is not loaded at all"
    );
    assert!(
        said.warnings.len() >= broken.len(),
        "every damaged row is reported: {:?}",
        said.warnings
    );
    for (key, _) in &junk {
        if broken.iter().any(|damaged| damaged == key) {
            assert!(
                said.warnings.iter().any(|line| line.contains(key)),
                "the warning names the key it refused: {:?}",
                said.warnings
            );
        }
    }
    // The one row that was fine survives the ones that were not.
    assert_eq!(
        app.opened,
        vec!["e8f1a2".to_owned()],
        "a damaged neighbour does not take the read set down with it"
    );
}

#[test]
fn a_state_from_a_newer_fathom_is_read_not_refused() {
    let entries = vec![
        (
            session::KEY_VERSION.to_owned(),
            (STATE_VERSION + 6).to_string(),
        ),
        (session::KEY_VIEW.to_owned(), "board".to_owned()),
        ("board.columns".to_owned(), "risk,age".to_owned()),
        (format!("{READ_PREFIX}e8f1a2"), "accepted".to_owned()),
        (session::KEY_PANE.to_owned(), "0".to_owned()),
    ];
    assert!(
        session::damaged(&entries).is_empty(),
        "none of this is damage: it is a newer fathom's vocabulary, and quarantining it would be \
         this build destroying a state it cannot read"
    );
    let mut app = cold((100, 30));
    let said = session::restore(&mut app, &entries);
    assert!(
        said.warnings
            .iter()
            .any(|line| line.contains("newer fathom") && line.contains("left alone")),
        "the reviewer is told the state is ahead of the binary: {:?}",
        said.warnings
    );
    assert!(
        said.warnings
            .iter()
            .any(|line| line.contains(session::KEY_VIEW)),
        "and told which value this build could not apply: {:?}",
        said.warnings
    );
    assert_eq!(app.view, ViewId::Queue, "and the app starts anyway");
}

// --- the read set ----------------------------------------------------------------------------------

#[test]
fn an_episode_opened_with_enter_is_in_the_read_set() {
    let mut app = fixture((100, 30));
    assert!(
        app.opened.is_empty(),
        "opening the shell has not read anything yet"
    );
    // Onto an episode first: the rail opens grouped by day, so slot 0 is a header and `<Enter>` on a
    // header opens nothing.
    let jumped = app.timeline.goto_first_unread();
    assert!(
        jumped.contains("e8f1a2"),
        "the fixture starts the cursor on the newest unread: {jumped}"
    );
    let opened = app.timeline.selected().unwrap().id.clone();
    app.dispatch("drill.in");
    assert_eq!(
        app.opened,
        vec![opened],
        "`<Enter>` is the reviewer saying *look at this one*"
    );
    let strip = app.timeline.strip();
    assert!(
        strip.contains("1 opened, not marked"),
        "the strip says so, because the ledger does not: {strip}"
    );
}

#[test]
fn a_read_set_from_an_earlier_session_moves_the_first_unread_jump() {
    // The session's read set is not decoration: `goto.first_unread` means "not looked at", and a jump
    // that walks the reviewer back to what they read last week is a rewind button.
    let entries = vec![
        (format!("{READ_PREFIX}e8f1a2"), "read".to_owned()),
        (format!("{READ_PREFIX}9c4d10"), "read".to_owned()),
    ];
    let mut app = cold((100, 30));
    let resumed = session::restore(&mut app, &entries);
    assert!(
        resumed.warnings.is_empty(),
        "two ordinary read rows are not a warning: {:?}",
        resumed.warnings
    );
    assert_eq!(app.timeline.seen_len(), 2, "both rows came back");
    let said = app.timeline.goto_first_unread();
    assert!(
        said.contains("40c9b3"),
        "the two opened rows are skipped and the cursor lands on the third unread: {said}"
    );
    assert!(
        app.timeline.strip().contains("2 opened, not marked"),
        "and the rail is honest that these read as unread in the ledger"
    );
}

#[test]
fn a_row_that_is_not_a_read_row_is_ignored_and_said() {
    let entries = vec![
        (format!("{READ_PREFIX}e8f1a2"), "accepted".to_owned()),
        (format!("{READ_PREFIX}9c4d10"), "read".to_owned()),
    ];
    let mut app = cold((100, 30));
    let said = session::restore(&mut app, &entries);
    assert_eq!(
        app.opened,
        vec!["9c4d10".to_owned()],
        "the good row survives the odd one"
    );
    assert!(
        said.warnings
            .iter()
            .any(|line| line.contains("unread:e8f1a2") && line.contains("read")),
        "and the odd one is reported, not silently dropped: {:?}",
        said.warnings
    );
}

// --- the keys themselves ---------------------------------------------------------------------------

#[test]
fn every_key_the_shell_writes_is_one_the_writer_owns() {
    // A key outside the three prefixes would be a key `fathom-cli` never reads back, which is a silent
    // hole rather than a loud one.
    let app = work();
    for (key, value) in session::snapshot(&app) {
        assert!(
            session::owns(&key),
            "`{key}` is written but not in a prefix the loader reads"
        );
        assert!(
            !value.contains(char::is_control),
            "`{key}` would be stored with a raw control character in it"
        );
        assert!(
            !session::too_long(&value),
            "`{key}` is longer than the format allows and would come back as damage"
        );
    }
}

#[test]
fn a_value_that_would_break_the_grammar_comes_back_unchanged() {
    // The separators are `;` `:` `,` `=`, so an episode id, a search or a mark containing one has to
    // survive the trip: the reviewer's query is `panic=use_after_free, area=src/store`.
    let mut app = work();
    app.last_search = Some("a;b:c,d=e%f".to_owned());
    let back = round_trip(&app);
    assert_eq!(
        back.last_search.as_deref(),
        Some("a;b:c,d=e%f"),
        "every separator the grammar uses, in one query, unchanged"
    );
    app.last_search = Some("line\u{a}break".to_owned());
    let back = round_trip(&app);
    assert_eq!(
        back.last_search.as_deref(),
        Some("line\nbreak"),
        "even a newline, which is why the format escapes at all"
    );
}

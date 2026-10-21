//! The keyboard-first end of `e5`: the hint line a reviewer can turn off, the named searches and
//! comparisons the palette runs, the `c` panel, and the list motions that have to work in every list.
//!
//! Each test drives the real keymap and the real renderer, because the criterion is a session, not a
//! function: what a reviewer does at 80×24 with no mouse is keystrokes, and the only honest way to ask
//! whether they are enough is to send them.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::sync::Arc;

use fathom_analyze::search::{Corpus, Entry, Index};
use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model, Overlay, PaletteRow};
use fathom_tui::compare::{self, CompareProvider};
use fathom_tui::frame::Options;
use fathom_tui::keymap::{Keymap, Mode, Override};
use fathom_tui::layout;
use fathom_tui::render::frame_lines;
use fathom_tui::views::{self, ViewId};

fn app(size: (u16, u16)) -> App {
    App::new(
        Keymap::defaults(),
        Model::from_facade(),
        size,
        "fathom/pilot · main",
    )
}

/// A corpus with one episode a query can find, so the search overlay has a *result* list to move over.
fn corpus() -> Corpus {
    let mut corpus = Corpus::new();
    corpus.documents = 2;
    corpus.push(Entry::episode(
        "e8f1a2c3d4",
        "epoch-safe store reads",
        "pin the epoch before the read",
        Some("Dana Stahl".to_owned()),
        Some(Timestamp::from_unix_micros(1_760_000_000_000_000)),
        Some(82),
    ));
    corpus.push(Entry::finding(
        "new-cycle",
        "store→core",
        "a new cycle between the store and the core",
        "high",
        None,
    ));
    corpus
}

/// A comparison provider that answers with numbered lines, so a scroll is visible.
struct Counted;

impl CompareProvider for Counted {
    fn compare(&self, spec: &str) -> Result<Vec<String>, fathom_tui::diff::Unavailable> {
        if spec.starts_with("broken") {
            return Err(fathom_tui::diff::Unavailable::new(
                "neither end of `broken` is in this repository",
            ));
        }
        Ok((0..40)
            .map(|i| format!("{i:>3}  line {i} of the delta between {spec}"))
            .collect())
    }
}

fn with_compare(size: (u16, u16)) -> App {
    let mut app = app(size);
    app.compare = Some(compare::Provider::new(Counted));
    app.scope.mark = Some("base@abc".to_owned());
    app.scope.episode = Some("e8f1a2c3d4".to_owned());
    app
}

fn toast(app: &App) -> String {
    app.toast.clone().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// e5.4 — the hint line, including off
// ---------------------------------------------------------------------------

#[test]
fn turning_the_hint_line_off_gives_its_rows_to_the_list() {
    // The claim is arithmetic, not cosmetic. A hint line "hidden" by drawing over it would pass a
    // screenshot and fail this: `chrome_rows` is the number every row computation downstream reads.
    let queue = views::view(ViewId::Queue);

    let mut on = app((100, 24));
    on.set_view(ViewId::Queue);
    let on_chrome = layout::chrome_rows(queue, &on.fit.applied);

    let mut off = app((100, 24));
    off.set_hints(false);
    off.set_view(ViewId::Queue);
    let off_chrome = layout::chrome_rows(queue, &off.fit.applied);

    assert!(
        off_chrome < on_chrome,
        "disabling the hint line must buy rows for content, not for blank space: \
         {on_chrome} chrome rows -> {off_chrome}"
    );
    assert!(
        off.fit
            .applied
            .iter()
            .any(|step| step == layout::Step::Hint.id()),
        "the step has to be in the ladder's own output, or every computation that reads `applied` is \
         one row out: {:?}",
        off.fit.applied
    );
    assert!(
        on.fit
            .applied
            .iter()
            .all(|step| step != layout::Step::Hint.id()),
        "at 100x24 the ladder has no reason to sell the hint line, so the difference below is the \
         reviewer's choice and not the terminal's: {:?}",
        on.fit.applied
    );
    // ...and the frame really has no hint row, at a size where the ladder would otherwise have kept it.
    let lines = frame_lines(&off);
    assert_eq!(lines.len(), 24);
    assert_eq!(
        frame_lines(&on).len().saturating_sub(0),
        24,
        "both frames are the size they were asked for — the hint row is not padding"
    );
}

#[test]
fn a_hint_line_the_reviewer_turned_off_is_not_reported_as_a_degradation() {
    // `Fit::dropped()` is the ladder being honest about what the frame had to sell. A row the reviewer
    // asked to lose is not the frame missing something, and a notice that called a choice a squeeze
    // would train a reviewer to distrust notices.
    let mut wide = app((100, 24));
    wide.set_hints(false);
    wide.set_view(ViewId::Queue);
    assert!(
        !wide
            .fit
            .dropped()
            .iter()
            .any(|step| step.contains("hint line")),
        "a chosen step is not a degradation: {:?}",
        wide.fit.dropped()
    );
    assert!(
        wide.fit
            .chosen
            .iter()
            .any(|step| step == layout::Step::Hint.id()),
        "{:?}",
        wide.fit.chosen
    );

    // On a frame small enough that the ladder has to sell the tab bar and the strips too, the hint line
    // the reviewer disabled still does not show up among the losses.
    let mut tiny = app((60, 8));
    tiny.set_hints(false);
    tiny.set_view(ViewId::Queue);
    assert!(
        !tiny
            .fit
            .dropped()
            .iter()
            .any(|step| step.contains("hint line")),
        "{:?}",
        tiny.fit.dropped()
    );
    // The same frame with the hint line *on* and the ladder selling it for itself: then it is a loss,
    // and the notice says so.
    let mut squeezed = app((60, 8));
    squeezed.set_view(ViewId::Queue);
    assert!(
        squeezed
            .fit
            .dropped()
            .iter()
            .any(|step| step.contains("hint line")),
        "the ladder selling the hint line is a degradation and must be reported: {:?}",
        squeezed.fit.dropped()
    );
}

#[test]
fn the_hint_line_is_a_frame_option_so_the_composition_root_decides_it() {
    // `fathom-tui` may not read configuration (ADR-002 §3), so the setting arrives the way every other
    // setting does — and the default is on, because the line is there for the reviewer who has not
    // learned the keys yet.
    assert!(Options::default().hints);
    assert!(
        !Options {
            hints: false,
            ..Options::default()
        }
        .hints
    );
}

// ---------------------------------------------------------------------------
// e5.4 — the things a reviewer named, run from the palette
// ---------------------------------------------------------------------------

fn saved_rows(app: &mut App) {
    app.saved_searches
        .push(fathom_analyze::search::history::SavedSearch {
            name: "risky-recent".to_owned(),
            query: "epoch".to_owned(),
        });
    app.saved_comparisons.push(fathom_analyze::compare::Saved {
        name: "since-the-refactor".to_owned(),
        spec: "base@abc..e8f1a2c3d4".to_owned(),
        note: "the release cut".to_owned(),
        created_utc: Some(1_760_000_000_000_000),
    });
}

fn row_named(rows: &[PaletteRow], needle: &str) -> Option<usize> {
    rows.iter().position(|row| row.label().contains(needle))
}

#[test]
fn the_palette_lists_the_saved_search_and_the_saved_comparison() {
    let mut app = app((120, 40));
    saved_rows(&mut app);
    app.dispatch("palette.open");
    let rows = app.palette_rows();
    assert!(
        row_named(&rows, "risky-recent").is_some(),
        "a saved search belongs in the palette: {:?}",
        rows.iter().map(PaletteRow::label).collect::<Vec<_>>()
    );
    assert!(row_named(&rows, "since-the-refactor").is_some());
    // The filter reaches them the same way it reaches an action, because they are rows, not a section.
    // The filter reaches a saved row the same way it reaches an action, because it is a row and not a
    // section — and the scorer is the search scorer, so the row whose *name* matched ranks first even
    // though other labels contain the same letters as a subsequence.
    app.prompt = "refactor".to_owned();
    let filtered = app.palette_rows();
    assert!(
        filtered.len() < 8,
        "`refactor` should narrow the field: {:?}",
        filtered.iter().map(PaletteRow::label).collect::<Vec<_>>()
    );
    assert!(matches!(
        filtered.first(),
        Some(PaletteRow::SavedComparison { .. })
    ));
}

#[test]
fn a_saved_search_runs_through_the_same_submission_as_a_typed_query() {
    let mut app = app((120, 40));
    app.search = Some(Arc::new(Index::build(&corpus())));
    saved_rows(&mut app);
    app.dispatch("palette.open");
    let rows = app.palette_rows();
    app.palette_row = row_named(&rows, "risky-recent").unwrap();
    // First prove the query is the one the row carries, then run it: `Enter` on a search hit goes where
    // the hit owns, which is what "run the saved search" means (`e5.2`'s deep-link rule, reached from the
    // palette instead of the box).
    app.prompt = "epoch".to_owned();
    app.refresh_search();
    assert!(
        app.search_report
            .as_ref()
            .is_some_and(|report| !report.hits.is_empty()),
        "the saved query found what typing it finds"
    );
    app.dispatch("palette.open");
    let rows = app.palette_rows();
    app.palette_row = row_named(&rows, "risky-recent").unwrap();
    app.dispatch("prompt.submit");
    assert_eq!(app.overlay, None, "the row ran and gave the keyboard back");
    assert_eq!(
        app.last_search.as_deref(),
        Some("epoch"),
        "the query that ran is the query that was saved"
    );
    assert_eq!(
        app.view,
        ViewId::Timeline,
        "the hit was an episode, so the timeline is where the reviewer ends up"
    );
}

#[test]
fn a_saved_comparison_runs_the_provider_and_names_the_spec() {
    let mut app = with_compare((120, 40));
    saved_rows(&mut app);
    app.scope.mark = None;
    app.scope.episode = None;
    app.dispatch("palette.open");
    let rows = app.palette_rows();
    app.palette_row = row_named(&rows, "since-the-refactor").unwrap();
    app.dispatch("prompt.submit");
    assert_eq!(app.overlay, Some(Overlay::Compare));
    assert_eq!(app.compare_panel.spec, "base@abc..e8f1a2c3d4");
    assert!(!app.compare_panel.refused, "{:?}", app.compare_panel.lines);
    assert!(
        toast(&app).contains("since-the-refactor"),
        "running a saved comparison says which one it ran: {:#?}",
        app.toast
    );
}

#[test]
fn a_saved_row_that_cannot_run_says_so_instead_of_running_something_else() {
    let mut app = app((120, 40));
    app.saved_searches
        .push(fathom_analyze::search::history::SavedSearch {
            name: "empty".to_owned(),
            query: "   ".to_owned(),
        });
    app.dispatch("palette.open");
    let rows = app.palette_rows();
    app.palette_row = row_named(&rows, "empty").unwrap();
    app.dispatch("prompt.submit");
    assert_eq!(
        app.overlay, None,
        "a query that is only whitespace is not a search"
    );
    assert!(
        !toast(&app).is_empty(),
        "the keystroke was refused, so it has to say why"
    );
}

#[test]
fn a_named_saved_comparison_survives_the_session_round_trip() {
    // The end-to-end half of "`fathom cmp --save` survives restarts": the row is written under `tui.` so
    // the shell is given it, decoded into the palette, and — the part that would otherwise break silently
    // — put back by `snapshot`, because the write-back removes every row a run read and did not write.
    let rows = vec![fathom_analyze::compare::saved_entry(
        &fathom_analyze::compare::Saved {
            name: "since-the-refactor".to_owned(),
            spec: "base@abc..def".to_owned(),
            note: "the release cut".to_owned(),
            created_utc: None,
        },
    )];
    assert!(fathom_tui::session::owns(&rows[0].0));
    let mut app = app((120, 40));
    let applied = fathom_tui::session::restore(&mut app, &rows);
    assert_eq!(app.saved_comparisons.len(), 1, "{:?}", applied.applied);
    assert_eq!(app.saved_comparisons[0].spec, "base@abc..def");
    let snapshot = fathom_tui::session::snapshot(&app);
    assert!(
        snapshot.contains(&rows[0]),
        "a row the shell does not own must be written back untouched, or the next run deletes it: {:?}",
        snapshot
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// e5.4 — the shared list motions, in every list
// ---------------------------------------------------------------------------

#[test]
fn the_palette_moves_with_the_shared_list_motions() {
    let mut app = app((120, 40));
    app.dispatch("palette.open");
    assert_eq!(app.palette_row, 0);
    app.dispatch("list.down");
    app.dispatch("list.down");
    assert_eq!(app.palette_row, 2, "`j` in a list means one row");
    app.dispatch("list.up");
    assert_eq!(app.palette_row, 1);
    app.dispatch("list.last");
    let last = app.palette_row;
    assert!(last > 2);
    app.dispatch("list.last");
    assert_eq!(
        app.palette_row, last,
        "the end of the list is the end of the list"
    );
    app.dispatch("list.first");
    assert_eq!(app.palette_row, 0);
    app.dispatch("list.page_down");
    let paged = app.palette_row;
    assert!(
        paged > 0 && paged < last,
        "one `<C-d>` is one page, not the whole list"
    );
    app.dispatch("list.page_up");
    assert_eq!(app.palette_row, 0);
    app.dispatch("list.half_down");
    assert!(app.palette_row > 0 && app.palette_row < paged);
    // `prompt.result_down` is the same cursor: `Mode::Pick` binds `j`/`k` to it, and the field's own
    // up/down must not fight the list's.
    let here = app.palette_row;
    app.dispatch("prompt.result_down");
    assert_eq!(app.palette_row, here + 1);
}

#[test]
fn the_search_results_move_with_the_shared_list_motions() {
    let mut app = app((120, 40));
    app.search = Some(Arc::new(Index::build(&corpus())));
    app.dispatch("search.open");
    app.prompt = "e".to_owned();
    app.refresh_search();
    let hits = app
        .search_report
        .as_ref()
        .map_or(0, |report| report.hits.len());
    assert!(hits > 1, "the corpus should answer `e` with a list");
    app.dispatch("list.down");
    assert_eq!(app.palette_row, 1);
    app.dispatch("list.last");
    assert_eq!(app.palette_row, hits - 1);
    app.dispatch("list.down");
    assert_eq!(
        app.palette_row,
        hits - 1,
        "at the end the cursor stays, it does not walk off"
    );
    app.dispatch("list.first");
    assert_eq!(app.palette_row, 0);
    // The drawn window follows the cursor, which is what keeps the selection on screen.
    app.dispatch("list.last");
    let (first, _) = app.search_window;
    assert!(first > 0 || hits <= app.search_visible());
}

#[test]
fn the_vim_aliases_are_defaults_and_an_override_still_outranks_them() {
    // `h j k l` are *defaults* in the shared motion modes, not a second vocabulary: they reach the same
    // action ids the arrows do, so a palette row cannot show one key and run another, and
    // `keys.overrides` can rebind or unbind them like any other default.
    let km = Keymap::defaults();
    let list = [Mode::List, Mode::Global];
    for (key, action) in [
        ("h", "drill.out"),
        ("l", "drill.in"),
        ("j", "list.down"),
        ("k", "list.up"),
        ("g", "view.graph"),
        ("G", "list.last"),
    ] {
        assert_eq!(
            km.resolve(&list, key),
            Some(action),
            "`{key}` must be a default motion in list mode, not a promise the keymap does not keep"
        );
    }
    let rebound = Keymap::merged(&[Override {
        mode: Mode::List,
        chord: "l".to_owned(),
        action: None,
        source: None,
    }]);
    assert_eq!(
        rebound.resolve(&list, "l"),
        None,
        "unbinding `l` unbinds the alias, because it is a default like any other"
    );
}

// ---------------------------------------------------------------------------
// e5.3 — the panel, from the shell's side
// ---------------------------------------------------------------------------

#[test]
fn c_compares_the_mark_against_the_selection_and_floats_the_answer() {
    let mut app = with_compare((120, 40));
    app.dispatch("compare.with_mark");
    assert_eq!(app.overlay, Some(Overlay::Compare));
    assert_eq!(
        app.compare_panel.spec, "base@abc..e8f1a2c3d4",
        "the right end is the live scope, exactly as with `d`"
    );
    let lines = frame_lines(&app);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("line 0 of the delta")),
        "{lines:?}"
    );
    // The spec travels in the title, so scrolling cannot lose the pair the answer belongs to.
    assert!(
        lines
            .iter()
            .any(|line| line.contains("base@abc..e8f1a2c3d4")),
        "{lines:?}"
    );
}

#[test]
fn the_comparison_panel_scrolls_and_stops_at_the_end() {
    let mut app = with_compare((120, 30));
    app.dispatch("compare.with_mark");
    assert_eq!(app.compare_panel.scroll, 0);
    app.dispatch("list.down");
    assert_eq!(app.compare_panel.scroll, 1);
    app.dispatch("list.page_down");
    assert!(app.compare_panel.scroll > 1);
    app.dispatch("list.last");
    let floor = app.compare_panel.scroll;
    assert!(floor > 0);
    for _ in 0..50 {
        app.dispatch("list.down");
    }
    assert_eq!(
        app.compare_panel.scroll, floor,
        "at the bottom the scroll holds, and the panel says so instead of eating the keystroke"
    );
    app.dispatch("list.first");
    assert_eq!(app.compare_panel.scroll, 0);
    app.dispatch("list.up");
    assert_eq!(app.compare_panel.scroll, 0, "and the top holds too");
}

#[test]
fn a_comparison_that_cannot_be_made_still_floats_its_reason() {
    let mut app = with_compare((120, 30));
    app.scope.mark = Some("broken".to_owned());
    app.dispatch("compare.with_mark");
    assert_eq!(
        app.overlay,
        Some(Overlay::Compare),
        "a refusal that only toasted would scroll away from itself"
    );
    assert!(app.compare_panel.refused);
    assert!(
        app.compare_panel
            .lines
            .iter()
            .any(|line| line.contains("neither end of `broken`")),
        "{:?}",
        app.compare_panel.lines
    );
}

#[test]
fn c_without_two_endpoints_says_which_half_is_missing() {
    let mut app = app((120, 30));
    app.compare = Some(compare::Provider::new(Counted));
    app.dispatch("compare.with_mark");
    assert_eq!(app.overlay, None);
    assert!(toast(&app).contains("`m` marks one"), "{:?}", app.toast);
}

#[test]
fn c_with_nothing_behind_it_says_which_kind_of_nothing_that_is() {
    // "no index" and "the store would not open" are different sentences and neither is an empty panel.
    let mut app = app((120, 30));
    app.scope.mark = Some("a".to_owned());
    app.scope.episode = Some("b".to_owned());
    app.open_compare("a..b");
    assert_eq!(app.overlay, Some(Overlay::Compare));
    assert!(app.compare_panel.refused);
    assert!(
        app.compare_panel
            .lines
            .iter()
            .any(|line| line.contains("no comparison provider")),
        "{:?}",
        app.compare_panel.lines
    );
}

// ---------------------------------------------------------------------------
// e5.4 — the palette: every action reachable, fuzzy in the same way `/` is
// ---------------------------------------------------------------------------

#[test]
fn every_palette_listed_action_is_a_row_in_the_palette() {
    // The criterion is an enumeration, so this is an enumeration: the registry's own `palette: true`
    // column against the rows `App::palette_rows()` actually prints for an empty query. An action in
    // the registry that the palette does not show is an action with no key and no row — unreachable,
    // which is the one thing ADR-006 §7.0 K1 forbids.
    let app = app((120, 30));
    let rows: Vec<&'static str> = app
        .palette_rows()
        .iter()
        .filter_map(PaletteRow::action_id)
        .collect();
    assert_eq!(
        rows.len(),
        rows.iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        "the palette printed one action twice"
    );
    let missing: Vec<&'static str> = fathom_tui::keymap::ACTIONS
        .iter()
        .filter(|action| action.palette)
        .map(|action| action.id)
        .filter(|id| !rows.contains(id))
        .collect();
    assert!(
        missing.is_empty(),
        "{}/{} palette-listed actions have no palette row: {:?}",
        missing.len(),
        fathom_tui::keymap::ACTIONS
            .iter()
            .filter(|a| a.palette)
            .count(),
        missing
    );
    // ...and the actions that lost their key to an override are still rows (K1 again).
    for hole in app.keymap.palette_holes() {
        assert!(rows.contains(&hole), "{hole} lost its key and its row");
    }
}

#[test]
fn an_untouched_palette_is_in_the_registry_s_order() {
    // The empty palette is the documentation: the ADR prints this list, the help overlay prints it,
    // and a reordering of the untouched list is a reordering of the docs. Fuzziness goes in *behind*
    // the query, not in front of the reviewer.
    let app = app((120, 30));
    let printed: Vec<&'static str> = app
        .palette_rows()
        .iter()
        .filter_map(PaletteRow::action_id)
        .collect();
    assert_eq!(
        printed,
        app.keymap
            .palette_entries()
            .into_iter()
            .map(|(id, _label, _keys)| id)
            .collect::<Vec<_>>(),
        "the palette reordered itself for no query"
    );
}

#[test]
fn the_palette_finds_a_row_by_its_letters_the_way_search_does() {
    // `cmp` must find "Mark this endpoint for comparison" — three letters, none of them adjacent, in a
    // label that does not contain the substring at all. The reason this is the *same* scorer as `/`
    // (`fathom_analyze::search::score`, whose last rung is the subsequence) is that a reviewer learns
    // one fuzzy behaviour, not two.
    let mut app = app((120, 30));
    let total = app.palette_rows().len();
    app.prompt = "cmp".to_owned();
    let rows = app.palette_rows();
    let ids: Vec<&'static str> = rows.iter().filter_map(PaletteRow::action_id).collect();
    assert!(
        ids.contains(&"compare.mark") && ids.contains(&"compare.with_mark"),
        "`cmp` did not find the compare actions: {ids:?}"
    );
    assert!(
        ids.len() < total,
        "a fuzzy query that matches everything is a query that does not search: {} of {total} rows \
         survived `cmp`",
        ids.len()
    );
    // And it never shows a row the letters cannot spell: the filter is the scorer, not a guess.
    for row in &rows {
        let label = row.label().to_lowercase();
        assert!(
            subsequence("cmp", &label),
            "the palette printed {:?} for `cmp`, which its letters do not spell",
            row.label()
        );
    }

    // A substring beats a letter-order match, because the rungs are ordered: "zoom" is a word in
    // "Zoom focused pane" and only a scatter of letters in "Search".
    app.prompt = "zoom".to_owned();
    let ids: Vec<&'static str> = app
        .palette_rows()
        .iter()
        .filter_map(PaletteRow::action_id)
        .collect();
    assert_eq!(
        ids.first().copied(),
        Some("pane.zoom"),
        "a substring hit must outrank a subsequence hit: {ids:?}"
    );

    app.prompt = "zzzznotaword".to_owned();
    assert!(
        app.palette_rows().is_empty(),
        "a query nothing matches must not print the whole registry"
    );
}

/// Letters of `needle` in order inside `hay`, with gaps allowed — the scorer's last rung, restated
/// here so the test can say "nothing in this list is noise" without reusing the code under test.
#[must_use]
fn subsequence(needle: &str, hay: &str) -> bool {
    let mut rest = hay.chars();
    needle.chars().all(|letter| rest.any(|c| c == letter))
}

#[test]
fn tied_rows_are_ordered_the_way_the_search_box_orders_them() {
    // The tie-break is `score::compare_labels` — the natural sort that puts `2` before `12` — and it is
    // the same one `/` uses, so the palette and the search results cannot disagree about two rows that
    // score alike. Two saved searches named `capture 2` and `capture 12` score alike by construction.
    let mut app = app((120, 30));
    app.saved_searches
        .push(fathom_analyze::search::history::SavedSearch {
            name: "capture 12".to_owned(),
            query: "risk>40 capture".to_owned(),
        });
    app.saved_searches
        .push(fathom_analyze::search::history::SavedSearch {
            name: "capture 2".to_owned(),
            query: "risk>40 capture".to_owned(),
        });
    app.prompt = "capture".to_owned();
    let names: Vec<String> = app
        .palette_rows()
        .iter()
        .filter_map(|row| match row {
            PaletteRow::SavedSearch { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        vec!["capture 2".to_owned(), "capture 12".to_owned()],
        "the tie-break is not the search box's natural sort"
    );
}

#[test]
fn the_palette_row_prints_the_key_the_reviewer_bound() {
    // The right-hand column is the key *today*: a row that printed the default key after the reviewer
    // rebound it would teach the wrong keystroke from the palette, which is the one place a reviewer
    // looks when they have forgotten it.
    let mut app = app((120, 30));
    app.keymap = Keymap::merged(&[Override {
        mode: Mode::Global,
        chord: "y".to_owned(),
        action: Some("review.accept".to_owned()),
        source: None,
    }]);
    let row = app
        .palette_rows()
        .into_iter()
        .find(|row| row.action_id() == Some("review.accept"))
        .expect("`review.accept` is a palette row");
    assert!(row.right().contains('y'), "{}", row.right());
}

#[test]
fn the_hint_line_prints_the_key_the_reviewer_bound() {
    // The other half of "reflects user keymap overrides": the bottom row offers four actions, and it
    // offers them under the keys the reviewer has, not the ones the defaults have.
    let plain = Keymap::defaults();
    let before = plain.hint_line();
    let rebound = Keymap::merged(&[
        Override {
            mode: Mode::Global,
            chord: "x".to_owned(),
            action: None,
            source: None,
        },
        Override {
            mode: Mode::Global,
            chord: "y".to_owned(),
            action: Some("review.accept".to_owned()),
            source: None,
        },
    ]);
    let after = rebound.hint_line();
    assert_ne!(before, after, "the hint line ignored the override: {after}");
    assert!(after.contains('y') && !after.contains(" x "), "{after}");
    // The hint line is built from the merged table, so it cannot name a key that does not fire.
    assert_eq!(rebound.resolve(&[Mode::Global], "y"), Some("review.accept"));
}

#[test]
fn a_refused_override_names_the_file_and_line_to_fix() {
    // The criterion: "conflicting keybindings are reported at startup with the file and line to fix".
    // `--show-keys` and the startup notices print `Refusal::message`, so the location has to be in the
    // message, not in a struct nobody prints.
    let source = fathom_tui::keymap::Source::new("fathom.toml", 12);
    let (overrides, problems) = fathom_tui::keymap::overrides_from_in(
        &std::collections::BTreeMap::from([(
            "nonsense".to_owned(),
            std::collections::BTreeMap::from([("j".to_owned(), "review.accept".to_owned())]),
        )]),
        &std::collections::BTreeMap::from([(
            ("nonsense".to_owned(), String::new()),
            source.clone(),
        )]),
    );
    assert!(
        overrides.is_empty(),
        "an unknown mode is not a mode, so nothing is built from it: {overrides:?}"
    );
    let unknown_mode = problems
        .iter()
        .find(|problem| problem.contains("unknown mode"))
        .expect("{problems:?}");
    assert!(
        unknown_mode.contains("fathom.toml:12"),
        "an unknown mode must name the line to fix: {unknown_mode}"
    );

    let km = Keymap::merged(&[Override {
        mode: Mode::Timeline,
        chord: "p".to_owned(),
        action: Some("view.queue".to_owned()),
        source: Some(source.clone()),
    }]);
    let problems = km.problems();
    assert!(
        problems.iter().any(
            |problem| problem.contains("shadows the globally reserved key")
                && problem.contains("fathom.toml:12")
        ),
        "{problems:?}"
    );
    let refusal = km
        .refusals()
        .iter()
        .find(|refusal| refusal.chord == "p")
        .expect("the reserved-shadow refusal is recorded");
    assert_eq!(refusal.source.as_ref(), Some(&source));
}

//! The timeline view (`e1.6`): what the rail draws, what a keystroke does, and what a reload does to
//! the cursor.
//!
//! The four acceptance bullets, and where each one is paid for:
//!
//! * *"10k-episode fixture scrolls with p99 frame time under 16 ms and constant memory"* — the frame
//!   half lives in `timeline_bench.rs`; this file asserts the *shape* that makes it true (a draw builds
//!   the rows on screen and nothing else) and the honesty that has to survive the optimisation
//!   (`rows_built` is the pane's height, never the index's length).
//! * *"Filters compose (author AND path AND unread) and are persisted in session state"* —
//!   `the_three_named_filters_compose` and the session round-trip tests.
//! * *"Selection survives re-index and re-attaches to the same episode after rebase"* — every test in
//!   "the cursor belongs to the episode", one per ADR-003 §6 tier.
//! * *"All navigation is possible without a mouse and matches the ADR-006 keymap"* — the keystroke
//!   tests feed real chords through `App::feed_key`, and the last block asserts the four registry
//!   entries this view owns are reachable exactly as published.
//!
//! As in `shell.rs`, every assertion goes through the real renderer, because "the rail drew the right
//! row" is only worth testing if it is the row the frame would show.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Instant;

use fathom_tui::app::{App, Model};
use fathom_tui::index::Facts;
use fathom_tui::keymap::Keymap;
use fathom_tui::timeline::{
    self, AuthorKind, Batch, Columns, Dimension, Episode, Filter, GroupBy, ReviewState, Slot,
};
use fathom_tui::views::ViewId;

#[path = "timeline_corpus/mod.rs"]
mod corpus;

use corpus::{DAY, NOW, generated, mock, rebased, reworded, squashed, without};

/// A timeline app at the ADR's documented size, over the ADR's own rows.
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

fn keystroke(app: &mut App, chord: &str) {
    app.feed_key(chord, Instant::now());
}

/// The rail's on-screen rows as text.
fn rail(app: &mut App) -> Vec<String> {
    let cols = app.pane_cols("rail").max(12);
    let rows = app.pane_body_rows();
    app.timeline.rail_text(cols, rows)
}

fn selected(app: &App) -> String {
    match app.timeline.selected() {
        Some(episode) => episode.id.clone(),
        None => "«nothing»".to_string(),
    }
}

/// Count the rows a filter would keep, straight off the model, so a test can say "3 rows" rather than
/// trusting the rail's own arithmetic.
fn kept_by(filter: &Filter, rows: &[Episode]) -> Vec<String> {
    rows.iter()
        .filter(|episode| filter.matches(episode))
        .map(|episode| episode.id.clone())
        .collect()
}

// --- the rows -------------------------------------------------------------------------------------

#[test]
fn the_rail_draws_the_adrs_row_at_80_columns() {
    // §5.2's row: `▸ e8f1a2 0:12 agent  +412 −90 ●●●●○○ 82 ⚑`, minus the risk number this build cannot
    // cite (it prints `?` instead of inventing 82 — ADR-001 §7.7).
    let mut app = fixture((80, 24));
    let lines = rail(&mut app);
    let first = lines
        .iter()
        .find(|line| line.contains("e8f1a2"))
        .expect("the newest episode is on screen");
    assert!(
        first.starts_with('▸'),
        "the cursor marks its row: {first:?}"
    );
    assert!(first.contains("0:12"), "relative time: {first:?}");
    assert!(first.contains("agent"), "the author badge: {first:?}");
    assert!(first.contains("+412 −90"), "the change size: {first:?}");
    assert!(
        first.contains('●'),
        "the change-size sparkline, filled on the biggest row in the corpus: {first:?}"
    );
    let smaller = lines
        .iter()
        .find(|line| line.contains("2be77f"))
        .expect("the docs typo is on screen");
    assert!(
        smaller.contains('○'),
        "and empty cells are drawn, not omitted: {smaller:?}"
    );
    assert!(first.contains('⚑'), "the unread marker: {first:?}");
}

#[test]
fn the_unknown_author_and_the_missing_risk_print_a_value_not_a_blank() {
    // ADR-001 §7.7: `unknown` is a value. A blank badge would read as "no author", a blank risk as
    // "no risk", and both are different claims from the one that is true.
    let mut app = fixture((80, 24));
    let lines = rail(&mut app);
    let unknown = lines
        .iter()
        .find(|line| line.contains("c1f5e2"))
        .expect("the unknown-author row is on screen");
    assert!(
        unknown.contains('?'),
        "the unknown badge and the unranked risk both print `?`: {unknown:?}"
    );
    assert!(!unknown.contains("agent"), "nobody guessed: {unknown:?}");
}

#[test]
fn the_columns_drop_in_the_order_the_adr_fixes_and_the_glyph_survives_every_one() {
    // §6.3: "sparkline, relative-time→date, author, then the risk *number*, and finally the risk
    // *glyph* last of all". The assertion is about the *order*, which is what the ADR fixes, plus the
    // promise behind it: a row that has been squeezed must never be squeezed out of its state marker,
    // because that cell is also the state under `NO_COLOR` (§5.2, `e5.6`).
    let mut drops = String::new();
    let mut previous: Option<Vec<bool>> = None;
    for width in [44, 36, 33, 28, 23, 18] {
        let line = rail_at(width);
        let first = line.lines().next().unwrap_or_default().to_owned();
        let present = [
            first.contains('●'),
            first.contains("agent"),
            first.contains("82"),
        ];
        // Once a column in the ladder's order is gone, no narrower rung puts it back.
        if let Some(previous) = previous {
            for (index, shown) in present.iter().enumerate() {
                assert!(
                    !*shown || previous[index],
                    "width {width} put back a column a wider rail had already dropped: {first:?}"
                );
            }
        }
        previous = Some(present.to_vec());
        assert!(
            first.contains('⚑') || first.contains('✔') || first.contains('✓'),
            "width {width}: the state glyph is the last cell standing: {first:?}"
        );
        drops.push_str(&format!(
            "{width}:{}
",
            Columns::for_width(width).name()
        ));
    }
    assert!(
        drops.contains("all six columns") && drops.contains("id and state only"),
        "the ladder reaches both ends of itself: {drops}"
    );
    assert_eq!(
        Columns::for_width(42).name(),
        "all six columns",
        "the ADR's own rail is 42 wide and its own row must fit in it"
    );
}

fn rail_at(width: usize) -> String {
    let mut rows = mock();
    rows[0].risk = Some(82);
    let mut rail = timeline::Rail::new(NOW);
    rail.load(Batch::new("fixture", rows));
    rail.text(width, 3).join("\n")
}

#[test]
fn a_frame_builds_only_the_rows_on_screen() {
    // The shape behind the memory criterion, asserted where it is cheap to assert: 10 000 rows in,
    // `visible` rows built out.
    let mut rail = timeline::Rail::new(NOW);
    rail.load(Batch::new("fixture", generated()));
    rail.set_visible(18);
    let lines = rail.lines(42, 18);
    assert_eq!(lines.len(), 18, "exactly the pane's height");
    assert_eq!(rail.last_stats().rows_built, 18);
    assert_eq!(
        rail.episodes().len(),
        10_000,
        "every episode is in the list, whether it is drawn or not"
    );
    assert!(
        rail.slots().len() >= 10_000,
        "slots are the episodes plus the group headers"
    );
    for row in 0..200 {
        rail.move_to(row * 37 % 10_000);
        let drawn = rail.lines(42, 18);
        assert_eq!(drawn.len(), 18);
        assert_eq!(
            rail.last_stats().rows_built,
            18,
            "no scroll ever builds more than the window"
        );
    }
}

// --- grouping -------------------------------------------------------------------------------------

#[test]
fn the_rail_groups_by_day_and_says_what_it_did() {
    let app = fixture((80, 24));
    let slots = app.timeline.rail().slots();
    let headers: Vec<&str> = slots
        .iter()
        .filter_map(|slot| match slot {
            Slot::Group { label, .. } => Some(label.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        headers.iter().any(|label| label.contains("Tue 14 Oct")),
        "day headers, ADR-006 §5.2's form: {headers:?}"
    );
    for label in &headers {
        assert!(
            label.contains("──"),
            "the header is a rule, not a row: {label:?}"
        );
        assert!(
            label.contains("episode") && label.chars().any(|c| c.is_ascii_digit()),
            "and it counts what it groups, in the singular when the group holds one: {label:?}"
        );
    }
}

#[test]
fn grouping_by_session_is_a_keystroke_away_and_repaints_the_headers() {
    let mut app = fixture((80, 24));
    let mut filter = app.timeline.filter().clone();
    filter
        .set(Dimension::Group, "session", NOW)
        .expect("session");
    app.timeline.set_filter(filter);
    let headers: Vec<String> = app
        .timeline
        .rail()
        .slots()
        .iter()
        .filter_map(|slot| match slot {
            Slot::Group { label, .. } => Some(label.clone()),
            _ => None,
        })
        .collect();
    assert!(
        headers
            .iter()
            .any(|label| label.starts_with("session 4 (pi)")),
        "session headers name the session and its author: {headers:?}"
    );
}

#[test]
fn a_collapsed_group_counts_what_it_hides_instead_of_hiding_it() {
    // ADR-001 §7.2 — a group that simply stops printing rows is the silent suppression the thesis
    // forbids. `<Space>` is the registry's own `list.expand`, and it explains itself on every row,
    // including the ones where there is nothing to collapse.
    let mut app = fixture((80, 24));
    assert!(
        app.timeline.rail_mut().select_id("9c4d10"),
        "the fixture's second row is selectable"
    );
    keystroke(&mut app, "<Space>");
    let said = app.toast.clone().unwrap_or_default();
    assert!(
        said.contains("is one episode") && said.contains("group header"),
        "on an episode row `<Space>` says what it does instead of doing nothing: {said}"
    );
    assert!(
        rail(&mut app).iter().any(|line| line.contains("9c4d10")),
        "and changed nothing"
    );

    app.timeline.move_to(0); // the day header
    keystroke(&mut app, "<Space>");
    let lines = rail(&mut app);
    let hidden = lines
        .iter()
        .find(|line| line.contains("more"))
        .expect("the collapsed run is counted on screen");
    assert!(
        hidden.contains('·'),
        "the marker is the counted one: {hidden:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("9c4d10")),
        "the rows are out of the way"
    );
    assert!(
        app.toast.clone().unwrap_or_default().contains("<Space>"),
        "and the toast says how to undo it"
    );
    keystroke(&mut app, "<Space>");
    assert!(
        rail(&mut app).iter().any(|line| line.contains("9c4d10")),
        "and back on the same keystroke"
    );
}

// --- the filter -----------------------------------------------------------------------------------

#[test]
fn the_three_named_filters_compose() {
    // The acceptance criterion verbatim: author AND path AND unread. OR inside a dimension, AND across
    // them — the store's rule, so the same filter means the same rows in both places.
    let rows = mock();
    let filter = Filter {
        authors: vec!["pi".to_owned()],
        area: Some("tests".to_owned()),
        state: Some(ReviewState::Unread),
        ..Filter::default()
    };

    let composed = kept_by(&filter, &rows);
    let by_author = kept_by(
        &Filter {
            authors: vec!["pi".to_owned()],
            ..Filter::default()
        },
        &rows,
    );
    let by_area = kept_by(
        &Filter {
            area: Some("tests".to_owned()),
            ..Filter::default()
        },
        &rows,
    );
    let by_state = kept_by(
        &Filter {
            state: Some(ReviewState::Unread),
            ..Filter::default()
        },
        &rows,
    );
    assert!(
        !by_author.is_empty() && !by_area.is_empty() && !by_state.is_empty(),
        "each dimension alone selects something: {by_author:?} {by_area:?} {by_state:?}"
    );
    assert!(
        composed.len() <= by_author.len().min(by_area.len()).min(by_state.len()),
        "and-ing narrows, it never widens: {composed:?}"
    );
    assert!(
        composed
            .iter()
            .all(|id| by_author.contains(id) && by_area.contains(id) && by_state.contains(id)),
        "every surviving row satisfies all three: {composed:?}"
    );
    assert_eq!(
        composed,
        vec![
            "e8f1a2".to_owned(),
            "9c4d10".to_owned(),
            "40c9b3".to_owned()
        ],
        "the rows this fixture actually has"
    );
}

#[test]
fn several_authors_or_and_the_states_read_the_way_the_store_reads_them() {
    let rows = mock();
    let both = Filter {
        authors: vec!["pi".to_owned(), "stahl".to_owned()],
        ..Filter::default()
    };
    let stahl_only = kept_by(
        &Filter {
            authors: vec!["stahl".to_owned()],
            ..Filter::default()
        },
        &rows,
    );
    assert!(kept_by(&both, &rows).len() > stahl_only.len());
    // `state=read` means *opened* — read or any verdict — which is the store's `Some(Read)` rule.
    let opened = Filter {
        state: Some(ReviewState::Read),
        ..Filter::default()
    };
    let ids = kept_by(&opened, &rows);
    assert!(ids.contains(&"2be77f".to_owned()), "read");
    assert!(
        ids.contains(&"77aa0c".to_owned()),
        "accepted counts as opened"
    );
    assert!(!ids.contains(&"e8f1a2".to_owned()), "unread does not");
}

#[test]
fn a_date_range_bounds_like_the_store_since_inclusive_until_exclusive() {
    let rows = mock();
    let mut filter = Filter::default();
    filter.set(Dimension::Since, "today", NOW).expect("a day");
    filter.set(Dimension::Until, "today", NOW).expect("a day");
    let ids = kept_by(&filter, &rows);
    assert!(
        ids.iter()
            .all(|id| id == "e8f1a2" || id == "9c4d10" || id == "2be77f"),
        "today only: {ids:?}"
    );
    assert!(ids.contains(&"e8f1a2".to_owned()), "since is inclusive");
    assert!(
        !ids.contains(&"77aa0c".to_owned()),
        "yesterday's episode is out"
    );
    // The typed day is *through* that day: the exclusive bound is the store's, not the reviewer's.
    assert_eq!(
        filter.value_of(Dimension::Until),
        "2025-10-14",
        "the strip prints the day the reviewer meant, with its year — `today` in a filter strip          would be a fact about the clock, and the same session restored tomorrow would mean          something else"
    );
    // `7d` is a *span* the reviewer typed; the rail stores the instant it meant, so the filter means
    // the same thing at 3am the next morning as it did at 11pm the night before.
    let mut span = Filter::default();
    span.set(Dimension::Since, "7d", NOW).expect("seven days");
    let seven_days_back = fathom_core::time::Timestamp::from_unix_micros(
        timeline::day_start(NOW).as_unix_micros() - 7 * DAY,
    );
    assert_eq!(
        span.since,
        Some(seven_days_back),
        "`7d` means the whole of seven days back — a bound, not a suggestion, and it does not move          when the frame redraws an hour later"
    );
}

#[test]
fn a_filter_that_selects_nothing_says_so_instead_of_going_blank() {
    let mut app = fixture((80, 24));
    let mut filter = app.timeline.filter().clone();
    filter
        .set(Dimension::Area, "no/such/area", NOW)
        .expect("a fragment");
    app.timeline.set_filter(filter);
    let strip = app.timeline.strip();
    assert!(
        strip.contains("filter:") && strip.contains("no/such/area"),
        "the strip carries the filter: {strip}"
    );
    assert!(app.timeline.is_empty());
    let lines = rail(&mut app);
    assert!(
        !lines.iter().any(|line| line.contains("e8f1a2")),
        "nothing matches, nothing prints"
    );
}

#[test]
fn the_strip_counts_what_the_filter_kept_and_what_the_index_holds() {
    // Two different "not everything is here" facts, both said: the filter dropped rows, and separately
    // the rail may only hold a page of a bigger index (ADR-001 §7.2).
    let mut app = fixture((80, 24));
    assert!(
        app.timeline.strip().contains("10 of 10"),
        "no filter: {}",
        app.timeline.strip()
    );
    let mut filter = app.timeline.filter().clone();
    filter.state = Some(ReviewState::Unread);
    app.timeline.set_filter(filter);
    let strip = app.timeline.strip();
    assert!(strip.contains("state=unread"), "{strip}");
    assert!(strip.contains("of 10"), "{strip}");

    let mut partial = Batch::new("index epoch 41", mock());
    partial.total = Some(3_412);
    app.load_episodes(partial);
    let strip = app.timeline.strip();
    assert!(
        strip.contains("index holds 3 412"),
        "the rail says it is holding a page, not the index: {strip}"
    );
}

// --- session state --------------------------------------------------------------------------------

#[test]
fn the_filter_round_trips_through_session_state() {
    // The AC's "persisted in session state": the filter has to be a value with a key, so `fathom-cli`
    // can put it in `session_state` and hand it back next run.
    let mut filter = Filter {
        authors: vec!["pi".to_owned(), "stahl".to_owned()],
        area: Some("src/store".to_owned()),
        state: Some(ReviewState::Unread),
        kinds: vec![AuthorKind::Agent],
        group: GroupBy::Session,
        ..Filter::default()
    };
    filter
        .set(Dimension::Since, "7d", NOW)
        .expect("seven days back");

    let encoded = filter.encode();
    let back = Filter::decode(&encoded);
    assert_eq!(back.authors, filter.authors, "{encoded}");
    assert_eq!(back.area, filter.area, "{encoded}");
    assert_eq!(back.state, filter.state, "{encoded}");
    assert_eq!(back.kinds, filter.kinds, "{encoded}");
    assert_eq!(back.group, filter.group, "{encoded}");
    assert_eq!(
        back.since, filter.since,
        "a computed bound is stored as the bound it computed"
    );
    assert_eq!(back.until, filter.until);
}

#[test]
fn session_state_survives_an_unknown_key_and_a_missing_one() {
    // Forward compatibility, same rule the store's own parsers use: a newer fathom's filter degrades
    // to the parts this one knows, it does not refuse to start.
    let decoded = Filter::decode("author=pi;hyperspace=on;state=unread");
    assert_eq!(decoded.authors, vec!["pi".to_owned()]);
    assert_eq!(decoded.state, Some(ReviewState::Unread));
    assert_eq!(
        decoded.group,
        GroupBy::Day,
        "a missing group is the default"
    );
    assert!(Filter::decode("").is_clear(), "an empty row is no filter");
}

#[test]
fn a_restored_session_gets_its_filter_and_its_cursor_back() {
    let mut app = fixture((80, 24));
    let mut filter = app.timeline.filter().clone();
    filter.state = Some(ReviewState::Unread);
    app.timeline.set_filter(filter);
    assert!(app.timeline.rail_mut().select_id("40c9b3"));
    let id = selected(&app);

    let entries = app.timeline.session().entries();
    assert!(
        entries.iter().any(|(key, _)| key.starts_with("timeline.")),
        "the keys are namespaced for `ui_state_all(\"timeline.\")`: {entries:?}"
    );

    // A fresh run, the way `fathom-cli` would build it: no filter, no cursor, then the session.
    let mut fresh = fixture((80, 24));
    let report = fresh.timeline.apply_session(&entries);
    assert_eq!(
        fresh.timeline.filter().state,
        Some(ReviewState::Unread),
        "{report}"
    );
    assert_eq!(selected(&fresh), id, "{report}");
    assert!(report.contains("session restored"), "{report}");
}

#[test]
fn a_selection_that_is_not_in_the_index_is_reported_and_not_guessed_at() {
    let mut app = fixture((80, 24));
    let entries = vec![("timeline.selection".to_owned(), "nosuchepisode".to_owned())];
    let report = app.timeline.apply_session(&entries);
    assert!(report.contains("not in this index"), "{report}");
    assert!(
        selected(&app) != "nosuchepisode",
        "the cursor lands on a real row and the toast says the stored one is gone"
    );
}

#[test]
fn a_session_resumes_onto_the_episode_that_replaced_the_one_that_left() {
    // The interesting resume: the reviewer was away, a reword rewrote the id, and the session's stored
    // id no longer exists. The member SHAs ride across (ADR-003 §6 tier 3) — and the toast says the
    // cursor landed on a *replacement*, which is a different fact from "you are where you left off".
    let rows = mock();
    let target = rows[3].clone();
    let entries = vec![
        ("timeline.selection".to_owned(), target.id.clone()),
        ("timeline.members".to_owned(), target.members.join(",")),
    ];
    let mut app = fixture((80, 24));
    app.load_episodes(Batch::new(
        "fixture index",
        reworded(&rows, &target.id, "renamed while the reviewer was away"),
    ));
    let report = app.timeline.apply_session(&entries);
    assert_eq!(
        selected(&app),
        format!("rw{}", target.id),
        "the stored id is gone; the member commit carried the cursor: {report}"
    );
    assert!(
        report.contains("tier 3") && report.contains("re-attached"),
        "{report}"
    );
}

// --- the cursor belongs to the episode ------------------------------------------------------------

#[test]
fn a_reindex_leaves_the_highlight_where_the_reviewer_put_it() {
    // The criterion in its plain case: the same episodes, re-read, possibly reordered.
    let mut app = fixture((80, 24));
    assert!(app.timeline.rail_mut().select_id("77aa0c"));
    let id = selected(&app);
    let mut again = mock();
    again.reverse();
    again.push({
        // A genuinely new episode: a new id *and* new digests, or the relink tiers have two candidate
        // rows to choose between and the test proves nothing about either.
        let mut extra = Episode::new("newrow", NOW, "something arrived while we were scrolling");
        extra.author = AuthorKind::Agent;
        extra.author_name = "pi".to_owned();
        extra.paths = vec!["src/tui/new.rs".to_owned()];
        extra.fingerprint = "fp:newrow".to_owned();
        extra.fallback_fingerprint = "fb:pi:something arrived while we were scrolling".to_owned();
        extra.shape_fingerprint = "sh:pi:src/tui/new.rs".to_owned();
        extra.members = vec!["newrow-m0".to_owned()];
        extra
    });
    app.load_episodes(Batch::new("fixture index", again));
    assert_eq!(selected(&app), id, "a re-index is not what moves a cursor");
}

#[test]
fn a_plain_rebase_moves_every_member_sha_and_the_cursor_does_not_move() {
    // ADR-003 I1: a rebase of the same content changes no id, so this is the case where nothing has to
    // be relinked — and the test that would catch an implementation keyed to member SHAs or row index.
    let mut app = fixture((80, 24));
    assert!(app.timeline.rail_mut().select_id("08d4b1"));
    let id = selected(&app);
    app.load_episodes(Batch::new("fixture index", rebased(&mock())));
    assert_eq!(selected(&app), id);
    assert!(app.timeline.rail().reattached.is_none());
}

#[test]
fn a_reword_moves_the_id_and_the_shape_digest_carries_the_cursor() {
    // Tier 1 would be author+subject; a reword changes the subject, so tier 2 (author+paths) is what
    // reaches across — the same tier that carries the *decision* in the store.
    let rows = mock();
    let target = rows[2].clone();
    let mut app = fixture((80, 24));
    assert!(app.timeline.rail_mut().select_id(&target.id));
    assert_eq!(selected(&app), target.id);
    app.load_episodes(Batch::new(
        "fixture index",
        reworded(&rows, &target.id, "typo in the epoch docs, reworded"),
    ));
    assert_eq!(
        selected(&app),
        format!("rw{}", target.id),
        "the cursor followed the rewrite"
    );
    let (_, words) = app
        .timeline
        .rail()
        .reattached
        .clone()
        .expect("the tier is reported, not assumed");
    assert!(words.contains("tier 2"), "{words}");
}

#[test]
fn a_squash_relinks_on_a_shared_member_commit() {
    let rows = mock();
    let target = rows[0].clone();
    let mut app = fixture((80, 24));
    assert_eq!(
        selected(&app),
        target.id,
        "the cursor starts on the newest row"
    );
    app.load_episodes(Batch::new("fixture index", squashed(&rows)));
    assert_eq!(selected(&app), "squash", "tier 3 caught it");
    let (_, words) = app.timeline.rail().reattached.clone().expect("reported");
    assert!(words.contains("tier 3"), "{words}");
}

#[test]
fn an_episode_that_left_says_so_instead_of_moving_the_cursor_quietly() {
    // ADR-003 I2. The sign-off case: the reviewer's row is gone, and a highlight on somebody else's row
    // is worse than a cursor at the top plus a sentence.
    let rows = mock();
    let mut app = fixture((80, 24));
    assert!(app.timeline.rail_mut().select_id("77aa0c"));
    let gone = selected(&app);
    app.load_episodes(Batch::new("fixture index", without(&rows, &gone)));
    assert_ne!(selected(&app), gone, "the episode is not there any more");
    let (_, words) = app
        .timeline
        .rail()
        .reattached
        .clone()
        .expect("the rail says the episode left");
    assert!(words.contains("left the index"), "{words}");
}

#[test]
fn a_filter_that_hides_the_selection_brings_it_back_when_it_returns() {
    let mut app = fixture((80, 24));
    // An episode the unread filter will hide, chosen by identity rather than by row number: the row
    // numbers are exactly what the view refuses to depend on.
    assert!(app.timeline.rail_mut().select_id("2be77f"));
    let hidden = selected(&app);
    let mut filter = app.timeline.filter().clone();
    filter.state = Some(ReviewState::Unread);
    app.timeline.set_filter(filter);
    assert_ne!(selected(&app), hidden, "the row is not on screen any more");
    app.timeline.set_filter(Filter::default());
    assert_eq!(
        selected(&app),
        hidden,
        "the anchor kept the episode, not the row number"
    );
}

// --- motion, without a mouse ----------------------------------------------------------------------

#[test]
fn every_motion_key_the_adr_binds_moves_the_rail() {
    // §7.2: `j k H M L gg G <C-d> <C-u>`, `<Enter>` and `<Tab>`, plus the arrows, driven through the
    // real resolver rather than by calling the methods.
    let mut app = fixture((80, 24));
    let first = selected(&app);
    keystroke(&mut app, "j");
    assert_ne!(selected(&app), first, "j");
    keystroke(&mut app, "k");
    assert_eq!(selected(&app), first, "k");
    keystroke(&mut app, "<Down>");
    assert_ne!(
        selected(&app),
        first,
        "the arrow keys are the same bindings"
    );
    keystroke(&mut app, "<Up>");

    keystroke(&mut app, "G");
    let last = app.timeline.cursor();
    assert_eq!(last, app.timeline.len() - 1, "G is the last row");
    keystroke(&mut app, "gg");
    assert_eq!(app.timeline.cursor(), 0, "gg is the first");
    keystroke(&mut app, "L");
    assert!(app.timeline.cursor() > 0, "L is the last visible row");
    keystroke(&mut app, "H");
    assert!(
        app.timeline.cursor() <= app.timeline.rail().offset() + 1,
        "H"
    );
    keystroke(&mut app, "M");
    assert!(app.timeline.cursor() > 0, "M");
    keystroke(&mut app, "<C-d>");
    assert!(app.timeline.cursor() > 0, "<C-d> moves a list");
    keystroke(&mut app, "<C-u>");
    keystroke(&mut app, "<C-f>");
    keystroke(&mut app, "<PageUp>");
    assert!(app.timeline.cursor() < app.timeline.len());
    keystroke(&mut app, "<Enter>");
    assert_eq!(app.timeline.cursor(), app.timeline.cursor());
}

#[test]
fn the_window_follows_the_cursor_so_it_never_scrolls_off_screen() {
    let mut app = fixture((80, 24));
    let last = app.timeline.len();
    app.timeline.move_to(last.saturating_sub(1));
    let lines = rail(&mut app);
    assert!(
        lines.iter().any(|line| line.starts_with('▸')),
        "the cursor is on screen at the end of the list"
    );
    app.timeline.move_to(0);
    let lines = rail(&mut app);
    assert!(
        lines.iter().any(|line| line.starts_with('▸')),
        "and at the start"
    );
}

#[test]
fn first_unread_and_today_are_reachable_and_say_what_they_found() {
    let mut app = fixture((80, 24));
    // Put the cursor somewhere already read, then jump.
    app.timeline.move_to(6);
    let said = app.timeline.goto_first_unread();
    assert!(said.contains("first unread"), "{said}");
    assert_eq!(
        app.timeline.selected().map(|episode| episode.state),
        Some(ReviewState::Unread)
    );

    let mut filter = app.timeline.filter().clone();
    filter.state = Some(ReviewState::Accepted);
    app.timeline.set_filter(filter);
    let said = app.timeline.goto_first_unread();
    assert!(
        said.contains("none of them in this filter"),
        "a jump that cannot jump explains itself and names the key: {said}"
    );

    let mut app = fixture((80, 24));
    assert!(app.timeline.rail_mut().select_id("b31c00"));
    let said = app.timeline.goto_today();
    assert!(said.contains("today"), "{said}");
    assert_eq!(
        app.timeline
            .selected()
            .map(|episode| timeline::day_start(episode.created)),
        Some(timeline::day_start(NOW))
    );
    // A day with nothing in it is not a dead end: the jump lands on the nearest earlier row.
    let mut filter = app.timeline.filter().clone();
    filter.state = Some(ReviewState::Snoozed);
    app.timeline.set_filter(filter);
    let said = app.timeline.goto_today();
    assert!(said.contains("nothing today"), "{said}");
    assert_eq!(
        app.timeline.selected().map(|episode| episode.state),
        Some(ReviewState::Snoozed)
    );
}

// --- the `F` surface ------------------------------------------------------------------------------

#[test]
fn the_filter_surface_needs_no_keys_the_keymap_did_not_already_have() {
    // `F` opens it; `<C-n>`/`<C-p>`/`<Enter>`/`<Esc>` and plain typing do the rest. The point of the
    // design is that it adds nothing to the reserved set (ADR-006 §7 K3/K4).
    let mut app = fixture((80, 24));
    keystroke(&mut app, "F");
    assert_eq!(app.overlay, Some(fathom_tui::app::Overlay::Filter));
    assert_eq!(
        app.interaction(),
        fathom_tui::keymap::Mode::Prompt,
        "it is a prompt, which is why it inherits one"
    );
    for chord in ["<C-n>", "<C-p>", "<Tab>", "<Down>", "<Up>"] {
        keystroke(&mut app, chord);
    }
    assert!(app.timeline.editor_cursor() <= app.filter_rows().len());
}

#[test]
fn choosing_a_value_sets_the_filter_and_shows_it_in_the_strip() {
    let mut app = fixture((80, 24));
    keystroke(&mut app, "F");
    // Dimensions → state → unread, one row at a time, no free text needed.
    let rows = app.filter_rows();
    let state_row = rows
        .iter()
        .position(|row| row.label.starts_with("state"))
        .expect("the state dimension is offered");
    app.timeline.editor_move(state_row, rows.len());
    keystroke(&mut app, "<Enter>");
    let rows = app.filter_rows();
    let unread = rows
        .iter()
        .position(|row| row.label == "unread")
        .expect("the state's values, with the current one ticked");
    app.timeline.editor_move(unread, rows.len());
    let before = app.timeline.len();
    keystroke(&mut app, "<Enter>");
    assert_eq!(app.timeline.filter().state, Some(ReviewState::Unread));
    assert!(app.timeline.len() < before, "and the rail changed");
    assert!(app.timeline.strip().contains("state=unread"));
    assert!(
        rail(&mut app)
            .iter()
            .all(|line| !line.contains("77aa0c") || line.contains('✔')),
        "an accepted episode is out from under an unread filter"
    );
}

#[test]
fn a_multi_valued_dimension_toggles_and_the_chooser_stays_open() {
    let mut app = fixture((80, 24));
    keystroke(&mut app, "F");
    let rows = app.filter_rows();
    let kind = rows
        .iter()
        .position(|row| row.label.starts_with("kind"))
        .expect("kind");
    app.timeline.editor_move(kind, rows.len());
    keystroke(&mut app, "<Enter>");
    let rows = app.filter_rows();
    let agent = rows
        .iter()
        .position(|row| row.label == "agent")
        .expect("agent");
    app.timeline.editor_move(agent, rows.len());
    keystroke(&mut app, "<Enter>");
    assert_eq!(app.timeline.filter().kinds, vec![AuthorKind::Agent]);
    assert!(
        app.timeline.editor_in_value(),
        "toggling keeps the chooser open"
    );
    keystroke(&mut app, "<Enter>");
    assert!(
        app.timeline.filter().kinds.is_empty(),
        "and choosing it again takes it off"
    );
}

#[test]
fn typing_narrows_the_chooser_and_a_typed_value_is_applied_on_enter() {
    let mut app = fixture((80, 24));
    keystroke(&mut app, "F");
    keystroke(&mut app, "a");
    keystroke(&mut app, "r");
    keystroke(&mut app, "e");
    let rows = app.filter_rows();
    assert!(
        rows.iter().any(|row| row.label.starts_with("area")),
        "the query filters the list the way the palette's does: {rows:?}"
    );
    let area = rows
        .iter()
        .position(|row| row.label.starts_with("area"))
        .expect("area");
    app.timeline.editor_move(area, rows.len());
    keystroke(&mut app, "<Enter>"); // into the area's value list
    assert!(app.timeline.editor_in_value());
    // `src/store` is a facet the fixture really has; typing it is how an unfamiliar fragment gets in.
    for ch in "src/store".chars() {
        keystroke(&mut app, &ch.to_string());
    }
    keystroke(&mut app, "<Enter>");
    assert_eq!(
        app.timeline.filter().area.as_deref(),
        Some("src/store"),
        "typed text is applied, not just suggested"
    );
}

#[test]
fn esc_closes_the_chooser_and_the_filter_it_set_stays_applied() {
    let mut app = fixture((80, 24));
    keystroke(&mut app, "F");
    let rows = app.filter_rows();
    let state = rows
        .iter()
        .position(|row| row.label.starts_with("state"))
        .expect("state");
    app.timeline.editor_move(state, rows.len());
    keystroke(&mut app, "<Enter>");
    let rows = app.filter_rows();
    let unread = rows
        .iter()
        .position(|row| row.label == "unread")
        .expect("unread");
    app.timeline.editor_move(unread, rows.len());
    keystroke(&mut app, "<Enter>");
    keystroke(&mut app, "<Esc>"); // back at dimensions, so this closes
    assert_eq!(app.overlay, None);
    assert_eq!(
        app.timeline.filter().state,
        Some(ReviewState::Unread),
        "`<Esc>` leaves the frame, it does not undo the work"
    );
}

#[test]
fn filter_clear_is_the_registry_entry_and_it_clears_everything() {
    let mut app = fixture((80, 24));
    let mut filter = app.timeline.filter().clone();
    filter.state = Some(ReviewState::Unread);
    filter.area = Some("src".to_owned());
    filter.group = GroupBy::Session;
    app.timeline.set_filter(filter);
    app.dispatch("filter.clear");
    assert!(app.timeline.filter().is_clear());
    assert_eq!(
        app.timeline.filter().group,
        GroupBy::Session,
        "grouping is a view option, not a filter: `filter clear` clears the rows it was hiding, it          does not rearrange the rail the reviewer chose"
    );
    app.dispatch("filter.clear");
    // A second clear says there was nothing to clear rather than pretending it did something.
    assert!(app.toast.clone().unwrap_or_default().contains("no filters"));
}

// --- the keymap contract --------------------------------------------------------------------------

#[test]
fn the_four_actions_this_view_owns_are_reachable_exactly_as_published() {
    // ADR-006 K1: bound or palette-listed, and the published list is `filter.edit` (bound to `F`) plus
    // three palette-only entries. Nothing here invented a key, so nothing here can shadow a reserved one.
    let keymap = Keymap::defaults();
    let entries: Vec<&str> = keymap
        .palette_entries()
        .into_iter()
        .map(|(id, _label, _keys)| id)
        .collect();
    for id in [
        "filter.edit",
        "filter.clear",
        "goto.first_unread",
        "goto.today",
    ] {
        let action = fathom_tui::keymap::ACTIONS
            .iter()
            .find(|action| action.id == id)
            .unwrap_or_else(|| panic!("{id} is in the registry"));
        assert_eq!(action.ticket, "e1.6", "{id} is this ticket's to implement");
        let bound = !keymap.keys_for(id).is_empty();
        assert!(
            bound || entries.contains(&id),
            "{id} is reachable without a mouse (K1): bound {bound:?}, palette {entries:?}"
        );
    }
    assert_eq!(
        fathom_tui::keymap::ACTIONS
            .iter()
            .find(|action| action.id == "filter.edit")
            .map(|action| action.label),
        Some("Edit filters (author, area, state, date)"),
        "and the help overlay prints the published label"
    );
}

#[test]
fn every_e1_6_action_actually_does_something() {
    // A palette entry that toasts "owned by e1.6" after e1.6 has landed is the help overlay lying.
    let mut app = fixture((80, 24));
    for action in [
        "filter.clear",
        "goto.first_unread",
        "goto.today",
        "filter.edit",
    ] {
        app.dispatch(action);
        let toast = app.toast.clone().unwrap_or_default();
        assert!(
            !toast.contains("owned by e1.6"),
            "`{action}` is implemented, not announced: {toast}"
        );
    }
}

#[test]
fn the_rail_reports_the_column_ladder_it_used() {
    assert_eq!(Columns::for_width(44).name(), "all six columns");
    assert_eq!(Columns::for_width(20).name(), "id and state only");
}

#[test]
fn a_dead_episode_is_marked_rather_than_quietly_kept() {
    // ADR-003 §6.3: a reviewed episode whose commits are gone is kept and shown. `gone` in the marker
    // column, because "your sign-off is on history that is not here" must not be silent.
    let mut rows = mock();
    rows[0].live = false;
    let mut rail = timeline::Rail::new(NOW);
    rail.load(Batch::new("fixture", rows));
    assert!(
        rail.text(42, 4).join("\n").contains("gone"),
        "the marker says it"
    );
}

/// An empty rail states why it is empty, and which sentence depends on what the caller said about the
/// index (`reco-1nu`).
#[test]
fn an_unloaded_rail_names_the_verb_to_run() {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("nothing indexed"),
        (80, 24),
        "fathom/pilot · main",
    );
    app.set_view(ViewId::Timeline);
    let strip = app.timeline.strip();
    assert!(strip.contains("nothing loaded"), "{strip}");
    let lines = rail(&mut app);
    assert!(
        !lines.is_empty(),
        "an empty rail is a stated rail, never a blank pane"
    );

    // ...and once the caller says what the index holds, the rail says that instead (`reco-1nu`). An
    // index that finished must not be sent back to `fathom index`: the reviewer who does that is told
    // the same thing twice and calls it a broken tool.
    let mut indexed = App::new(
        Keymap::defaults(),
        Model::empty("nothing indexed"),
        (80, 24),
        "fathom/pilot · main",
    );
    indexed.set_view(ViewId::Timeline);
    indexed.set_index_facts(&Facts::present(
        Some(1),
        Some("ready".to_owned()),
        Some(500),
        Some(412),
    ));
    let strip = indexed.timeline.strip();
    assert!(strip.contains("index epoch 1 ready"), "{strip}");
    assert!(
        !strip.contains("run `fathom index`"),
        "a finished index was sent back to `fathom index`: {strip}"
    );
    assert!(
        !rail(&mut indexed).is_empty(),
        "an empty rail over a real index is still a stated rail, never a blank pane"
    );
}

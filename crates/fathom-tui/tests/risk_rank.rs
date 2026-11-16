//! The per-episode risk figure, in the shipped shell (`reco-c05`; re-hosted on the rail by `e10.3`,
//! `reco-ydek.3`).
//!
//! `reco-glv`'s acceptance step (b) was "find and read the three riskiest episodes", and the answer to
//! it used to be a ranked pane. `e10.1` (ADR-006 §5.1.2 §5) decided otherwise and `e10.3` deleted the
//! pane: the score stays a per-episode figure printed on a row and explained in the card, and it stops
//! being the ordering principle of a second view. What this file asserts is therefore the half that
//! survived, and it is the half the ticket's language was always about: **say the number, say why, and
//! say what the number cannot see**. What it no longer asserts is an order the shell does not own —
//! the rail is a chronology, so the fixture is still scored out of chronological order and the test is
//! that the rows print their own figures rather than a borrowed one.
//!
//! The shell may not *rank* anything (ADR-006 §2.1; ADR-002 §5.2, lint `E01`): a risk arrives as a
//! number with its parts attached, computed by `fathom-analyze::risk` and handed over by the
//! composition root. So these tests play the composition root's role — they build the rows an index
//! would hand over, in the chronological order a store read returns, with scores deliberately out of
//! order.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::time::Instant;

use fathom_analyze::insight::{EpisodeDelta, RiskComponent, RiskView};
use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::render::frame_lines;
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState};
use fathom_tui::views::ViewId;

/// Fixed clock, as every timeline fixture uses: a fixture that reads the clock is a fixture with a
/// flake in it (ADR-002 §4.4).
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
const MINUTE: i64 = 60_000_000;
const SIZE: (u16, u16) = (110, 34);

/// A risk in the shape the model carries it: the ranking that made it, its arithmetic, its reasons and
/// its ceiling.
///
/// It carries no *place*: `rank N of M` left with the ranked set in `e10.3` (ADR-006 §5.1.2 §5), and a
/// fixture cannot offer what the document no longer has. The third argument is gone for the same reason
/// — there is no set to state a denominator against.
fn risk(score: u64, why: &str) -> RiskView {
    RiskView {
        model: "spine-3".to_owned(),
        score: i64::try_from(score).unwrap_or(0),
        vector: format!("spine-3 {score} = surfaces {score}"),
        components: vec![RiskComponent {
            name: "surfaces".to_owned(),
            points: i64::try_from(score).unwrap_or(0),
            cap: 50,
            detail: why.to_owned(),
        }],
        surfaces: vec!["migration".to_owned()],
        caveat: None,
        blind_spots: vec![
            "whether a schema migration can be rolled back".to_owned(),
            "whether a new environment variable is ever read by the program".to_owned(),
        ],
    }
}

/// Four episodes, in the order a chronological read returns them, scored out of order so that a row
/// printing somebody else's figure cannot pass. The third carries no risk at all, which is every row
/// the shipped build had.
fn rows() -> Vec<Episode> {
    let make = |id: &str, ago: i64, score: u64, why: &str| {
        let created = Timestamp::from_unix_micros(NOW.as_unix_micros() - ago);
        let mut episode = Episode::new(id, created, format!("episode {id}"));
        episode.last = created;
        episode.author = AuthorKind::Agent;
        episode.author_name = "pi".to_owned();
        episode.author_confidence = Some(0.94);
        episode.n_add = 10;
        episode.n_del = 2;
        episode.n_files = 3;
        episode.paths = vec![format!("db/migrations/{id}.sql")];
        episode.state = ReviewState::Unread;
        let ranking = (score > 0).then(|| risk(score, why));
        episode.delta = ranking
            .map(|ranking| EpisodeDelta::sparse(&episode.id, &episode.summary, Some(ranking)));
        episode
    };
    vec![
        make(
            "e-aaaa",
            4 * MINUTE,
            41,
            "a workflow change travels with the artefact",
        ),
        make(
            "e-bbbb",
            3 * MINUTE,
            88,
            "a schema migration that may not roll back",
        ),
        make(
            "e-cccc",
            2 * MINUTE,
            0,
            "nothing ranked this episode at all",
        ),
        make(
            "e-dddd",
            MINUTE,
            64,
            "a lockfile the reviewer did not choose",
        ),
    ]
}

fn loaded(rows: Vec<Episode>) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        SIZE,
        "harbour · main",
    );
    app.timeline.set_now(NOW);
    app.set_view(ViewId::Timeline);
    app.load_episodes(Batch::new("fixture index", rows));
    app
}

fn keystroke(app: &mut App, chord: &str) {
    app.feed_key(chord, Instant::now());
}

/// The rail pane's body as the shell draws it.
fn rail(app: &mut App) -> Vec<String> {
    let pane = app
        .current()
        .panes
        .iter()
        .find(|pane| pane.name == "rail")
        .expect("the timeline view has a rail pane");
    app.timeline_text(pane)
        .expect("the rail has text once the index has rows")
}

/// The four ids in the order the rail drew them — which is chronology, newest first.
fn order(app: &mut App) -> Vec<String> {
    rail(app)
        .iter()
        .filter_map(|line| {
            ["e-dddd", "e-cccc", "e-bbbb", "e-aaaa"]
                .into_iter()
                .find(|id| line.contains(id))
                .map(str::to_owned)
        })
        .collect()
}

// --- the figure on the row ------------------------------------------------------------------------

#[test]
fn the_rail_is_chronological_and_prints_each_rows_own_score() {
    let app = &mut loaded(rows());
    let lines = rail(app);
    let text = lines.join("\n");
    assert_eq!(
        order(app),
        vec!["e-dddd", "e-cccc", "e-bbbb", "e-aaaa"],
        "newest first: the rail is a chronology, not a ranking (ADR-006 §5.1.2 §3):\n{text}"
    );
    // Every row carries its own number. The point of keeping the score once the ranked pane went is
    // that a reviewer reading in time order still sees the figure per row; the point of *not* keeping
    // the pane is that nobody has to agree with the order it implied.
    for (id, score) in [("e-dddd", 64), ("e-bbbb", 88), ("e-aaaa", 41)] {
        let row = lines
            .iter()
            .find(|line| line.contains(id))
            .cloned()
            .unwrap_or_default();
        assert!(
            row.split_whitespace().any(|word| word == score.to_string()),
            "{id} carries its own score, not the highest one: {row}\n{text}"
        );
    }
    assert!(
        !text.contains("rank "),
        "no row claims a place in a set nothing ranks any more:\n{text}"
    );
}

#[test]
fn the_strip_names_the_order_it_is_in_and_the_model_behind_the_number() {
    // ADR-006 §5.1.2 §3: the worklist's strip said `order: risk × staleness × blast radius`, and the
    // rule it existed for outlived it — *a list names the order it is in, in words the renderer
    // implements*. The rail's strip does both halves now: the chronology, and where the printed score
    // came from.
    let app = &mut loaded(rows());
    let pane = app
        .current()
        .panes
        .iter()
        .find(|pane| pane.name == "rail")
        .expect("the timeline view has a rail pane");
    let strip = app
        .timeline_strip(pane)
        .expect("a rail with rows always says how it ordered them");
    assert!(
        strip.contains("order: newest first"),
        "the order, in words the renderer implements: {strip}"
    );
    assert!(
        !strip.contains("risk × staleness"),
        "the retired function is named nowhere a reviewer reads: {strip}"
    );
    assert!(
        strip.contains("spine-3"),
        "the rows print a number and the strip says whose it is: {strip}"
    );
    assert!(
        strip.contains("4 unread"),
        "and how much of what it lists is still asking to be read: {strip}"
    );
}

#[test]
fn each_row_carries_its_own_reason() {
    // The reason used to be the worklist's second line. On the rail the row has one line and the reason
    // is one `w` away in the card — which is the same promise with a different geometry, so what is
    // asserted here is that the reason belongs to the row that was asked about.
    let app = &mut loaded(rows());
    keystroke(app, "w");
    let card = card_rows(app).join(" ");
    // The card elides a long value at this width, so the comparison is with the *other* rows' reasons:
    // the one printed is the cursor row's, and none of the others' are on screen.
    assert!(
        card.contains("a lockfil"),
        "the card answers the row under the cursor:\n{card}"
    );
    for not_this_one in [
        "a schema migration that may not roll back",
        "a workflow change travels with the artefact",
        "nothing ranked this episode at all",
    ] {
        assert!(
            !card.contains(not_this_one),
            "and not a reason belonging to another row: {not_this_one}\n{card}"
        );
    }
}

#[test]
fn an_unranked_index_still_draws_every_row_and_says_what_is_missing() {
    // The degraded case is the shipped case until an index runs the ranking: the rail must stay a rail,
    // and must never print a missing score as zero (ADR-001 §7.7).
    let mut rows = rows();
    for episode in &mut rows {
        episode.delta = None;
    }
    let app = &mut loaded(rows);
    let text = rail(app).join("\n");
    for id in ["e-aaaa", "e-bbbb", "e-dddd", "e-cccc"] {
        assert!(
            text.contains(id),
            "no episode is dropped because it has no score: {id}\n{text}"
        );
    }
    assert!(
        !text.contains("risk 0"),
        "a missing score is never a zero:\n{text}"
    );
    assert!(
        text.split_whitespace().any(|word| word == "?"),
        "and is printed as unknown, not nothing:\n{text}"
    );
    let pane = app
        .current()
        .panes
        .iter()
        .find(|pane| pane.name == "rail")
        .expect("a pane");
    let strip = app.timeline_strip(pane).unwrap_or_default();
    // An unranked row is counted as unranked, and the open-work count is unaffected by it: the count is
    // about what is unread, which is a fact about the reviewer, not about the scorer.
    assert!(strip.contains("4 unread"), "{strip}");
    assert!(
        !strip.contains("spine-3"),
        "no model is cited beside no number: {strip}"
    );
}

/// The card pane's own rows at the frame's geometry, so a wrapped sentence is one string here and not
/// three lines wearing a pane border.
fn card_rows(app: &App) -> Vec<String> {
    fathom_tui::timeline::card_lines(
        app.timeline.selected(),
        NOW,
        "fixture index",
        app.pane_cols("card"),
        app.pane_body_rows(),
        app.card_window(),
    )
    .iter()
    .map(|line| line.text().trim_end().to_owned())
    .collect()
}

// --- the ranking band ----------------------------------------------------------------------------

#[test]
fn w_expands_the_ranking_band_in_place_and_w_folds_it_again() {
    let app = &mut loaded(rows());
    assert_eq!(
        app.expanded_rank, None,
        "nothing is expanded before it is asked for"
    );
    let before = frame_lines(app).join("\n");
    assert!(
        before.contains("a spine cannot see"),
        "the card already says a ranking has a ceiling, and how many items are hidden:\n{before}"
    );
    assert!(
        !before.contains("environment variable"),
        "but it prints the count, not the list — the card has a size:\n{before}"
    );

    keystroke(app, "w");
    assert_eq!(app.expanded_rank.as_deref(), Some("e-dddd"));
    assert!(
        app.toast
            .as_ref()
            .is_some_and(|toast| toast.contains("spine-3 64")),
        "the toast carries the arithmetic, for a reader who will not open the card: {:?}",
        app.toast
    );
    assert!(
        !app.toast
            .as_ref()
            .is_some_and(|toast| toast.contains("rank ")),
        "and no longer a place in a ranking that no longer exists: {:?}",
        app.toast
    );

    // Asserted on the card pane's own rows: the frame would break a wrapped sentence with the pane's
    // borders, and a test that cannot tell a wrapped row from a missing one is not checking anything.
    let card = card_rows(app);
    let said = card
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        said.contains("spine-3 64 = surfaces 64"),
        "the arithmetic is on the screen, never a bare number:\n{card:?}"
    );
    assert!(
        said.contains("whether a new environment variable is ever read by the program"),
        "and the whole ceiling, in its own words, not a `(1 more)`:\n{card:?}"
    );
    assert!(
        said.contains("read those by hand: the ranking is a reading order, not an all-clear"),
        "and what to do with a ceiling, in the model's own words:\n{card:?}"
    );
    keystroke(app, "w");
    assert_eq!(
        app.expanded_rank, None,
        "the same key folds the band it opened, rather than stacking a pane on it"
    );
    let card = card_rows(app);
    assert!(
        !card.join(" ").contains("environment variable"),
        "and the card is the brief card again:\n{card:?}"
    );
}

#[test]
fn expanding_the_band_leaves_every_other_band_on_the_screen() {
    // The sentence `e9.3` is built on: `w` grows one band in place, it does not swap the pane. So the
    // change half of the card has to survive the keystroke that opens the ranking half.
    let app = &mut loaded(rows());
    // Every band the brief card printed is still printed with the ranking open — read off the card
    // rather than a hand-typed list, because these fixtures have no delta document with a `what` row:
    // they print the spine's `size` where a documented episode prints the document's `what`.
    let before = card_rows(app);
    keystroke(app, "w");
    let frame = card_rows(app);
    let band = |lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .filter_map(|line| {
                let word = line
                    .split_whitespace()
                    .next()?
                    .trim_start_matches(['~', 'x', '?']);
                [
                    "size",
                    "what",
                    "noise",
                    "where",
                    "risk",
                    "attention",
                    "surfaces",
                    "blind",
                    "intent",
                ]
                .contains(&word)
                .then(|| word.to_owned())
            })
            .collect::<Vec<_>>()
    };
    assert!(
        band(&before).contains(&"size".to_owned()) && band(&before).contains(&"where".to_owned()),
        "the change bands are on the card before the keystroke:\n{before:?}"
    );
    assert!(
        band(&frame).contains(&"blind".to_owned()),
        "the ceiling is still counted with the band open:\n{frame:?}"
    );
    for label in band(&before) {
        assert!(
            band(&frame).contains(&label),
            "the `{label}` band is still on the card with the expanded ranking:\n{frame:?}"
        );
    }
}

#[test]
fn moving_on_folds_the_band_instead_of_letting_it_lie() {
    let app = &mut loaded(rows());
    keystroke(app, "w");
    keystroke(app, "j");
    assert_eq!(
        app.expanded_rank, None,
        "the band was opened about the episode that was selected, not about the pane"
    );
    assert_eq!(
        app.timeline.selected().map(|episode| episode.id.as_str()),
        Some("e-cccc"),
        "and the cursor moved down the chronology, which is what a rail cursor is"
    );
}

#[test]
fn a_refresh_keeps_the_reviewer_on_the_episode_they_were_reading() {
    // Background refreshes replace every row (ADR-006 §5.3). A rail that re-resolved the cursor by
    // position would move the reviewer off the episode they were reading mid-keystroke.
    let app = &mut loaded(rows());
    keystroke(app, "j");
    let before = app.timeline.selected().map(|episode| episode.id.clone());
    assert_eq!(before.as_deref(), Some("e-cccc"));
    app.load_episodes(Batch::new("fixture index", rows()));
    assert_eq!(
        app.timeline.selected().map(|episode| episode.id.clone()),
        before,
        "a refreshed rail keeps the reviewer where they were"
    );
}

// --- the frame ------------------------------------------------------------------------------------

#[test]
fn a_rail_frame_draws_exactly_at_every_promised_size() {
    // The ADR-006 §5.5 audit (the selected row reachable at every promised size) has to hold with rows
    // in the pane — not only with the "nothing to read yet" placeholder this build shipped with.
    for size in [(80u16, 24u16), (100, 30), (200, 60)] {
        let mut app = App::new(
            Keymap::defaults(),
            Model::from_facade(),
            size,
            "harbour · main",
        );
        app.timeline.set_now(NOW);
        app.set_view(ViewId::Timeline);
        app.load_episodes(Batch::new("fixture index", rows()));
        let lines = frame_lines(&app);
        assert_eq!(lines.len(), usize::from(size.1), "height at {size:?}");
        for (index, line) in lines.iter().enumerate() {
            assert_eq!(
                fathom_tui::layout::display_width(line),
                usize::from(size.0),
                "row {index} at {size:?}: {line:?}"
            );
        }
        let text = lines.join("\n");
        assert!(
            text.contains("e-dddd"),
            "the newest row is on screen at {size:?}:\n{text}"
        );
    }
}

#[test]
fn the_status_line_counts_the_open_work_the_shell_is_showing() {
    // Before this the status row printed `unread unknown` forever, because nothing in the shell knew
    // what its own rail held. The count is the same figure the deleted queue's strip printed, counted
    // over the same rows — which is the whole reason *caught up* stayed falsifiable when the pane left.
    let frame = frame_lines(&loaded(rows())).join("\n");
    assert!(frame.contains("4 unread"), "four unread episodes:\n{frame}");
    assert!(
        !frame.contains("unread unknown"),
        "and not the placeholder this build shipped with:\n{frame}"
    );
    assert!(
        !frame.contains("queue"),
        "the word is gone from the line that had it: {frame}"
    );

    // The count is the rows on screen, so a read episode leaves it — which is also the case a
    // dispositioning key (`x`, `e4.6`) will move through later.
    let mut read = rows();
    read[1].state = ReviewState::Read;
    let frame = frame_lines(&loaded(read)).join("\n");
    assert!(
        frame.contains("3 unread"),
        "one already-read episode is not open:\n{frame}"
    );
}

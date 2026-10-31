//! The rail's age column, and the clock it is aged against (`reco-5oe`).
//!
//! The ticket was a column that lied twice over. An episode that landed four seconds ago printed
//! `0:00`, and printed `0:00` an hour later — one sentence, two defects:
//!
//! * **the bucket.** `0:00` is what the `h:mm` arithmetic says at forty seconds and what it says at
//!   zero, so the column said "this hour" about a row that was seconds old. The column's own
//!   resolution is a minute, so below a minute it now says `<1m`, and a row the frame has not reached
//!   (clock skew, a fixture dated forward) says `ahead` rather than a clipped zero.
//! * **the frozen instant.** The frame is handed one wall-clock `Timestamp` at the CLI's edge and
//!   nothing below may read a clock (ADR-002 §4.4), which is what made the first half permanent: an
//!   open rail aged every row against the instant it opened with. The loop now carries that instant
//!   forward on the monotonic clock it already owns ([`App::age_clock`]) and asks for a draw only
//!   when the instant crossed something the rail can print — a minute where the rail prints an age,
//!   a day where §5.2's ladder has collapsed the age to a date.
//!
//! The clock is injected rather than read, so nothing here sleeps and nothing here is flaky: a test
//! hands `age_clock` the monotonic instant the loop would have woken at, forty minutes from now if
//! that is the case under test.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::event::Bus;
use fathom_tui::frame::{HeadlessSink, LoopEnd, run_loop};
use fathom_tui::keymap::Keymap;
use fathom_tui::timeline::{self, Batch, Columns, Episode, relative};
use fathom_tui::views::ViewId;

/// A timestamp that lands on a minute mark, so the minute arithmetic is arithmetic and not a
/// remainder nobody meant to test.
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
const MICROS_PER_SEC: i64 = 1_000_000;
const MICROS_PER_MIN: i64 = 60 * MICROS_PER_SEC;
const MICROS_PER_HOUR: i64 = 60 * MICROS_PER_MIN;
const MICROS_PER_DAY: i64 = 24 * MICROS_PER_HOUR;
const SIZE: (u16, u16) = (100, 30);

/// A row `micros` older than [`NOW`] — negative for a row the frame has not reached yet.
fn then(micros: i64) -> Timestamp {
    Timestamp::from_unix_micros(NOW.as_unix_micros() - micros)
}

fn episode(id: &str, age_micros: i64) -> Episode {
    let created = then(age_micros);
    let mut row = Episode::new(id, created, format!("episode {id}"));
    row.last = created;
    row.author_name = "pi".to_owned();
    row.n_add = 10;
    row.n_del = 2;
    row.n_files = 1;
    row.paths = vec![format!("crates/{id}/mod.rs")];
    row.fingerprint = format!("sha256:{id}");
    row
}

/// A timeline over one row of the given age, at a size whose rail prints an age and not a date.
fn shell(size: (u16, u16), age_micros: i64) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        size,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.set_view(ViewId::Timeline);
    app.load_episodes(Batch::new("fixture index", vec![episode("e1", age_micros)]));
    app
}

/// The rail's on-screen rows as text.
fn rail(app: &mut App) -> String {
    let cols = app.pane_cols("rail").max(12);
    let rows = app.pane_body_rows();
    app.timeline.rail_text(cols, rows).join("\n")
}

/// The card's body for one episode, as text.
fn card(episode: &Episode, now: Timestamp) -> String {
    timeline::card_lines(Some(episode), now, "fixture index", 60, 12)
        .into_iter()
        .map(|line| line.text().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------------------------
// the bucket: what the column says about a row younger than its own resolution
// ---------------------------------------------------------------------------------------------

#[test]
fn a_row_under_a_minute_old_is_not_called_0_00() {
    for seconds in [0, 1, 4, 30, 59] {
        let text = relative(NOW, then(seconds * MICROS_PER_SEC));
        assert_eq!(
            text, "<1m",
            "{seconds}s is under the column's resolution, so it may not print a number that is also true of an hour"
        );
        assert_ne!(text, "0:00", "the sentence this ticket was filed about");
    }
}

#[test]
fn the_hour_form_picks_up_exactly_where_the_bucket_ends() {
    // The bucket is a floor, not a change of dialect: everything from a minute to a day is still
    // `h:mm`, and the first value the bucket hands back is the one the arithmetic was always right about.
    assert_eq!(relative(NOW, then(MICROS_PER_MIN)), "0:01");
    assert_eq!(relative(NOW, then(12 * MICROS_PER_MIN)), "0:12");
    assert_eq!(relative(NOW, then(3 * MICROS_PER_HOUR)), "3:00");
    assert_eq!(
        relative(NOW, then(23 * MICROS_PER_HOUR + 59 * MICROS_PER_MIN)),
        "23:59"
    );
    assert_eq!(
        relative(NOW, then(2 * MICROS_PER_DAY + 4 * MICROS_PER_HOUR)),
        "2d4h"
    );
    assert_eq!(relative(NOW, then(45 * MICROS_PER_DAY)), "1mo");
    assert_eq!(relative(NOW, then(400 * MICROS_PER_DAY)), "13mo");
    assert_eq!(relative(NOW, then(800 * MICROS_PER_DAY)), "2y");
}

#[test]
fn a_row_the_frame_has_not_reached_yet_says_so_instead_of_0_00() {
    // An age cannot be negative, and `0:00` was the old way of pretending it could not be: a commit
    // dated ahead by clock skew read as "just landed", which is the opposite fact.
    assert_eq!(relative(NOW, then(-MICROS_PER_MIN)), "ahead");
    assert_eq!(relative(NOW, then(-MICROS_PER_DAY)), "ahead");
    // Zero age is the young end of the bucket, not the ahead end.
    assert_eq!(relative(NOW, NOW), "<1m");
}

#[test]
fn the_card_says_the_same_thing_in_english() {
    // The card's sentence is `… · 0:12 ago`, and neither of the two new buckets reads as English with
    // ` ago` glued on — `<1m ago` and `ahead ago` are not sentences.
    let fresh = card(&episode("fresh", 4 * MICROS_PER_SEC), NOW);
    assert!(
        fresh.contains("just now"),
        "a four-second-old row: {fresh:?}"
    );
    assert!(
        !fresh.contains("<1m ago"),
        "the column's shape is not the sentence's: {fresh:?}"
    );

    let ahead = card(&episode("ahead", -30 * MICROS_PER_SEC), NOW);
    assert!(
        ahead.contains("dated ahead of this frame"),
        "a row ahead of the frame: {ahead:?}"
    );

    let older = card(&episode("older", 12 * MICROS_PER_MIN), NOW);
    assert!(older.contains("0:12 ago"), "an ordinary row: {older:?}");
}

#[test]
fn the_rail_prints_the_bucket_over_the_zero() {
    let mut app = shell(SIZE, 4 * MICROS_PER_SEC);
    let text = rail(&mut app);
    assert!(text.contains("<1m"), "a four-second-old row: {text:?}");
    assert!(!text.contains("0:00"), "the ticket's own screen: {text:?}");
}

// ---------------------------------------------------------------------------------------------
// the clock: an open rail ages instead of freezing on the instant it opened with
// ---------------------------------------------------------------------------------------------

#[test]
fn an_open_rail_ages_with_the_clock_it_was_handed() {
    let mut app = shell(SIZE, 4 * MICROS_PER_SEC);
    let opened = Instant::now();
    assert!(
        !app.age_clock(opened),
        "the first wake-up only anchors the instant; nothing has moved"
    );
    assert_eq!(
        app.timeline.now(),
        NOW,
        "anchoring does not move the instant"
    );
    assert!(rail(&mut app).contains("<1m"));

    // Forty minutes of a reviewer reading the same screen.
    assert!(
        app.age_clock(opened + Duration::from_secs(40 * 60)),
        "a minute crossed, so the frame has to be drawn"
    );
    assert_eq!(
        app.timeline.now().as_unix_micros(),
        NOW.as_unix_micros() + 40 * MICROS_PER_MIN,
        "the instant is the one the frame opened with, plus the monotonic distance since"
    );
    let text = rail(&mut app);
    assert!(
        text.contains("0:40"),
        "the row aged with the frame: {text:?}"
    );
    assert!(
        !text.contains("<1m"),
        "and stopped claiming to be new: {text:?}"
    );
}

#[test]
fn a_wake_up_inside_the_same_minute_buys_no_frame() {
    // ADR-006 §0.1's tickless loop, applied to the clock: the loop wakes ten times a second to notice
    // `<C-c>`, and an age the eye cannot see moving may not become sixty draws a minute.
    let mut app = shell(SIZE, 4 * MICROS_PER_SEC);
    let opened = Instant::now();
    app.age_clock(opened);
    for seconds in [0, 1, 17, 42, 59] {
        assert!(
            !app.age_clock(opened + Duration::from_secs(seconds)),
            "{seconds}s in: the same minute, so there is nothing to draw"
        );
    }
    assert!(
        app.age_clock(opened + Duration::from_secs(60)),
        "the minute mark itself is the change"
    );
}

#[test]
fn a_rail_printing_dates_not_ages_does_not_draw_for_a_minute() {
    // Below `Columns::NoSparkline` the age column is a date read off the row, and the only `now` the
    // rail prints is the footer's day label. A minute there is a draw that repaints the frame,
    // unchanged — which is exactly what §0.1 forbids — so the day is what has to turn.
    let mut app = shell((46, 20), 4 * MICROS_PER_SEC);
    let width = app.pane_cols("rail").max(12);
    assert!(
        !matches!(
            Columns::for_width(width),
            Columns::Full | Columns::NoSparkline
        ),
        "the fixture's rail must be on a date-only rung, and is {width} columns"
    );

    let opened = Instant::now();
    app.age_clock(opened);
    assert!(
        !app.age_clock(opened + Duration::from_secs(40 * 60)),
        "forty minutes turn nothing on a rail that prints no ages"
    );
    assert!(
        app.age_clock(opened + Duration::from_secs(24 * 60 * 60)),
        "a day turns the footer's date, and that one is worth a frame"
    );
}

#[test]
fn a_wider_rail_takes_its_minutes() {
    // The other half of the rung question: the same clock on a rail that *is* printing ages.
    let mut app = shell(SIZE, 4 * MICROS_PER_SEC);
    let width = app.pane_cols("rail").max(12);
    assert!(
        matches!(
            Columns::for_width(width),
            Columns::Full | Columns::NoSparkline
        ),
        "the fixture's rail must be on an age rung, and is {width} columns"
    );
    let opened = Instant::now();
    app.age_clock(opened);
    assert!(app.age_clock(opened + Duration::from_secs(61)));
}

#[test]
fn a_frame_whose_instant_moved_on_its_own_re_anchors() {
    // `prime`, a scripted test, a caller that hands the rail a timestamp: the monotonic anchor
    // follows the frame's instant instead of arguing with it and printing an age nobody asked for.
    let mut app = shell(SIZE, 4 * MICROS_PER_SEC);
    let opened = Instant::now();
    app.age_clock(opened);
    let later = Timestamp::from_unix_micros(NOW.as_unix_micros() + MICROS_PER_HOUR);
    app.timeline.set_now(later);
    assert!(
        !app.age_clock(opened + Duration::from_secs(5)),
        "the instant jumped forward on its own; the re-anchor is bookkeeping, not a change"
    );
    assert_eq!(app.timeline.now(), later, "and the jump is kept");
    assert!(
        app.age_clock(opened + Duration::from_secs(60 * 61)),
        "and the anchor is the new instant, not the old one"
    );
    assert!(
        app.timeline.now().as_unix_micros() >= later.as_unix_micros() + 60 * MICROS_PER_MIN,
        "an hour plus a minute of monotonic distance from the re-anchor"
    );
}

#[test]
fn a_quiet_loop_asks_the_clock_and_draws_nothing_for_it() {
    // The wiring, as the loop runs it: the frame's instant is carried forward by the loop itself, and
    // a wake-up that changed nothing still costs no draw. `NOW` is on a minute mark and a scripted run
    // lasts a poll window, so no minute can turn inside this test — one frame is the whole claim.
    let mut app = shell(SIZE, 4 * MICROS_PER_SEC);
    let bus = Bus::new();
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: Some(SIZE),
    };
    let report = run_loop(&mut app, &bus, &mut sink, None, Some(2))
        .expect("the loop runs with nothing on the bus");
    assert_eq!(report.end, LoopEnd::Idle);
    assert_eq!(report.frames, 1, "the frame it had to draw, and no more");
    assert!(
        app.clock.is_some(),
        "the loop asked the clock, or the rail is frozen again"
    );
    assert!(app.timeline.now() >= NOW);
}

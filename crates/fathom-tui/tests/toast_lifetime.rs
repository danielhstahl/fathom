//! A toast borrows the hint row, and it gives it back (`reco-2ll`, ADR-006 §7.5).
//!
//! The hint line is the surface that makes `?` unnecessary for the common case, so a toast that
//! never expires is not a message but an eviction: a reviewer who dismisses the help overlay once
//! loses the hints for the rest of the session. The frame that fixes it is *tickless* — it draws
//! when something changed and otherwise draws nothing — so the lifetime has to be a clock and the
//! expiry has to be a reason to draw. Both halves are asserted here: the lifetime in the shell, and
//! the frame that returns the row with no keystroke to ask for it.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use fathom_tui::app::{App, Model, TOAST_LIFETIME};
use fathom_tui::error::Error;
use fathom_tui::event::{Bus, Message};
use fathom_tui::frame::{FrameSink, LoopEnd, run_loop};
use fathom_tui::keymap::Keymap;
use fathom_tui::render::frame_lines;
use fathom_tui::terminal::Signal;

fn app(size: (u16, u16)) -> App {
    App::new(
        Keymap::defaults(),
        Model::from_facade(),
        size,
        "fathom/pilot · main",
    )
}

/// The last row of the drawn frame — the row the toast and the hint line compete for.
fn last_line(app: &App) -> String {
    frame_lines(app)
        .last()
        .cloned()
        .unwrap_or_default()
        .trim_end()
        .to_owned()
}

const TOAST: &str = "help closed — the view behind it kept its selection";

#[test]
fn an_unexpired_toast_wins_the_hint_row() {
    let mut app = app((120, 40));
    let started = Instant::now();
    app.toast_at(TOAST, started);

    // While it lasts it is the line: the key hints are behind it, not beside it (ADR-006 §7.5).
    assert_eq!(
        last_line(&app),
        TOAST,
        "the toast is the last row of the frame"
    );
    assert_eq!(app.hint_line(), TOAST, "and the line the renderer asks for");

    // …and it lasts: a moment before the deadline it is still there, so a reviewer gets to read it.
    let halfway = started + TOAST_LIFETIME / 2;
    assert!(
        app.toast_left(halfway).is_some_and(|left| !left.is_zero()),
        "halfway through its lifetime the toast still has time left"
    );
    assert!(
        !app.age_toast(halfway),
        "a toast that is not due yet is not retired, and its expiry is not a change"
    );
    assert_eq!(
        app.hint_line(),
        TOAST,
        "still the toast at half its lifetime"
    );
}

#[test]
fn a_toast_gives_the_hint_row_back_when_its_lifetime_is_up() {
    let mut app = app((120, 40));
    let started = Instant::now();
    app.toast_at(TOAST, started);
    assert_eq!(app.hint_line(), TOAST);

    // One tick before the deadline: still there. A lifetime is a promise to the reader, and a
    // toast that vanishes early is a message a reviewer never finished.
    assert!(!app.age_toast(started + TOAST_LIFETIME - Duration::from_millis(1)));
    assert_eq!(app.hint_line(), TOAST);

    // At it: the row returns, and `age_toast` says so, which is the loop's cue to draw.
    assert!(
        app.age_toast(started + TOAST_LIFETIME),
        "the expiry is a change to the frame, and the loop has to be told"
    );
    assert_eq!(app.toast, None, "the toast is gone, not hidden");
    assert_eq!(app.toast_left(started + TOAST_LIFETIME), None);
    let hints = app.hint_line().clone();
    assert!(
        hints.contains("Command palette") && hints.contains("Mark reviewed"),
        "the key-hint row is back, in the keymap's own words: {hints}"
    );
    assert_eq!(last_line(&app), hints.trim_end());
    assert!(
        !hints.contains("help closed"),
        "the retired toast is nowhere on the row: {hints}"
    );
    // Ageing an already-expired frame is not a change: the loop asks every 100 ms and must not
    // repaint forever because of one toast.
    assert!(!app.age_toast(started + TOAST_LIFETIME * 2));
}

#[test]
fn a_new_toast_starts_its_clock_again() {
    let mut app = app((120, 40));
    let started = Instant::now();
    app.toast_at("the first line", started);
    app.toast_at(TOAST, started + TOAST_LIFETIME - Duration::from_millis(1));
    // The second act is the one on screen, and it is owed the row for a whole lifetime of its own.
    assert_eq!(app.hint_line(), TOAST);
    assert!(!app.age_toast(started + TOAST_LIFETIME));
    assert_eq!(app.hint_line(), TOAST);
    assert!(app.age_toast(started + TOAST_LIFETIME * 2));
    assert!(app.toast.is_none());
}

#[test]
fn a_resize_gives_the_hint_row_back_to_the_size_that_asked_for_it() {
    // A toast belongs to the size that produced it. A resize re-asks the layout ladder, so the old
    // line is retired rather than left standing under a frame that has changed underneath it.
    let mut app = app((120, 40));
    app.toast_at(TOAST, Instant::now());
    assert_eq!(app.hint_line(), TOAST);
    fathom_tui::frame::apply(
        &mut app,
        Message::Resize {
            cols: 110,
            rows: 38,
        },
    );
    assert_eq!(app.size, (110, 38), "and the resize happened");
    assert!(
        !app.hint_line().contains("help closed"),
        "the toast drawn for the old size is gone: {:?}",
        app.hint_line()
    );
}

/// The frame a reviewer actually waits for: a toast, then nothing at all.
///
/// Two keystrokes go in — `?` then `<Esc>`, the ticket's own repro — and then the bus is silent.
/// The run ends when this sink has seen the hint row come back, or when its own deadline says the
/// expiry never came; either way the loop stops, so a broken expiry fails an assertion instead of
/// hanging the suite.
struct UntilHintsBack {
    /// To end the run from inside a draw, the way a `<C-c>` would.
    bus: Arc<Bus>,
    frames: Vec<Vec<String>>,
    /// The wall-clock backstop, so the test cannot hang.
    deadline: Instant,
    ended: bool,
}

impl UntilHintsBack {
    fn new(bus: Arc<Bus>) -> Self {
        Self {
            bus,
            frames: Vec::new(),
            deadline: Instant::now() + Duration::from_secs(10),
            ended: false,
        }
    }

    fn last(&self) -> String {
        self.frames
            .last()
            .and_then(|frame| frame.last())
            .cloned()
            .unwrap_or_default()
            .trim_end()
            .to_owned()
    }
}

impl FrameSink for UntilHintsBack {
    fn draw(&mut self, app: &mut App) -> Result<(), Error> {
        self.frames.push(frame_lines(app));
        if self.ended {
            return Ok(());
        }
        let row = self.last();
        let hints_back = row.contains("Command palette");
        if hints_back || Instant::now() > self.deadline {
            self.ended = true;
            let _ = self.bus.send(Message::Signal(Signal::Interrupt));
        }
        Ok(())
    }
}

#[test]
fn the_hint_row_comes_back_on_an_idle_frame_with_no_keystroke_to_ask() {
    let mut app = app((120, 40));
    // Short enough to test, and the only concession here: the lifetime itself is a wall clock, so a
    // test that shortens it is shortening the same clock the loop reads.
    app.toast_lifetime = Duration::from_millis(250);
    let bus = Bus::new();
    // The ticket's repro: open the help, dismiss it. The dismissal is an act, so it toasts.
    bus.send(Message::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    bus.send(Message::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    )));
    let started = Instant::now();
    let mut sink = UntilHintsBack::new(bus.clone());
    // `max_frames: None` is the real run: it ends on a quit, which here is the sink's own
    // interrupt. A scripted run would end on the first quiet poll, before any lifetime could pass.
    let report = run_loop(&mut app, &bus, &mut sink, None, None).expect("the loop runs headlessly");
    let elapsed = started.elapsed();

    assert_eq!(report.end, LoopEnd::Quit, "{:?}", report.end);
    assert!(
        sink.frames.len() >= 2,
        "the frame that returns the hint row is a frame of its own: {} drawn",
        sink.frames.len()
    );
    let first = sink.frames.first().unwrap().last().unwrap().trim_end();
    assert!(
        first.contains("help closed"),
        "the first frame says what the keystroke did: {first}"
    );
    let last = sink.last();
    assert!(
        last.contains("Command palette") && !last.contains("help closed"),
        "the last frame is the key-hint row, and nothing else: {last}"
    );
    assert!(
        elapsed >= Duration::from_millis(250),
        "the toast kept the row for its lifetime, and not for less: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(4),
        "the loop woke for the deadline rather than waiting out a poll it never set: {elapsed:?}"
    );
    assert_eq!(
        report.events, 3,
        "two keystrokes and the sink's own way out — the hint row came back without a press"
    );
}

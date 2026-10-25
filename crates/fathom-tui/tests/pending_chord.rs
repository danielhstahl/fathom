//! A chord in the air is marked on the frame it lands on (`reco-wlt`, ADR-006 §7.2, §7.5).
//!
//! `g` is `view.graph` and the first half of `gg`, so a lone `g` cannot be answered until
//! [`CHORD_TIMEOUT`] expires. ADR-006 §7.2 accepts that cost, and the cost is not the defect: the
//! defect is that the wait was invisible. A reviewer pressed `g`, saw the same screen, pressed it
//! again — which is now `gg`, a different action in a different mode — and concluded the graph view
//! was broken. Every other view key paints in 0.5–2 ms through the pty; `g` took 250.5 ms and said
//! nothing about why.
//!
//! So the frame now says so, in the one frame the wait costs, and these tests hold the four things
//! that claim means: the mark is on the frame the keystroke lands on (not on the one that follows
//! it), it says what is held and what the wait is waiting *for*, it is read out of the merged table
//! rather than written beside it, and it is gone the moment the chord resolves.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use fathom_tui::app::{App, Model};
use fathom_tui::error::Error;
use fathom_tui::event::{Bus, Message};
use fathom_tui::frame::{FrameSink, LoopEnd, run_loop};
use fathom_tui::keymap::{CHORD_TIMEOUT, ChordState, Keymap, Mode, Override, mode_chain};
use fathom_tui::render::frame_lines;
use fathom_tui::terminal::Signal;
use fathom_tui::views::ViewId;

/// The size the walkthrough used, and a size the hint row survives at (`e1.5`).
const SIZE: (u16, u16) = (120, 40);

/// The shell the walkthrough was holding: the shipped keymap, the façade's own model.
fn shell() -> App {
    App::new(
        Keymap::defaults(),
        Model::from_facade(),
        SIZE,
        "fathom/pilot · main",
    )
}

/// The drawn frame, trailing blanks trimmed.
fn frame(app: &App) -> Vec<String> {
    frame_lines(app)
        .iter()
        .map(|line| line.trim_end().to_owned())
        .collect()
}

/// The frame without its last row — the row the hint line, the toast and the mark compete for.
fn body(app: &App) -> Vec<String> {
    let mut lines = frame(app);
    let keep = lines.len().saturating_sub(1);
    lines.truncate(keep);
    lines
}

/// The last row of the drawn frame.
fn row(app: &App) -> String {
    frame(app).pop().unwrap_or_default()
}

/// Press one chord the way the loop does.
fn press(app: &mut App, chord: &str) -> Vec<String> {
    app.feed_key(chord, Instant::now())
}

/// The chain a keystroke is resolved against in `view` — the same arithmetic `App::chain` does,
/// for the tests that ask the keymap directly rather than through an `App`.
fn chain(view: ViewId) -> Vec<Mode> {
    mode_chain(Mode::for_view(view), Mode::List)
}

/// Every row of the frame that says a chord is being held.
fn marks(app: &App) -> Vec<String> {
    frame(app)
        .into_iter()
        .filter(|line| line.contains("… waiting"))
        .collect()
}

// --- the mark -------------------------------------------------------------------------------

#[test]
fn a_held_chord_marks_the_frame_the_keystroke_lands_on() {
    let mut app = shell();
    let before = frame(&app);
    let before_body = body(&app);
    assert!(
        !row(&app).contains("… waiting"),
        "nothing is held at rest, so nothing says it is: {:?}",
        row(&app)
    );

    press(&mut app, "g");

    let after = row(&app);
    assert!(
        after.contains("`g` … waiting"),
        "the frame the `g` keystroke produces has to say a key is in the air: {after}"
    );
    assert!(
        after.contains("250 ms"),
        "…and what the wait costs, or it is a spinner with a letter on it: {after}"
    );
    // The two things the reviewer is deciding between, in the order they will act on them: what the
    // held key becomes if nothing arrives, and what a second key would still turn it into.
    assert!(
        after.contains("`g` alone = Graph in 250 ms"),
        "the wait has to name what it is waiting for: {after}"
    );
    assert!(
        after.contains("`gg` = First row"),
        "and the other meaning the second key still owns: {after}"
    );
    assert_eq!(
        body(&app),
        before_body,
        "and nothing else moved: the mark is the whole of what `g` did so far"
    );
    assert_ne!(
        frame(&app),
        before,
        "the frame the keystroke produced is a different frame"
    );
}

#[test]
fn the_mark_is_gone_when_the_chord_resolves() {
    let mut app = shell();
    press(&mut app, "g");
    assert!(app.chords.is_pending());
    // The double tap: the second key arrives inside the wait, so nothing waits and nothing marks.
    let applied = press(&mut app, "g");
    assert_eq!(applied, vec!["list.first".to_owned()]);
    assert!(!app.chords.is_pending());
    assert!(
        marks(&app).is_empty(),
        "a resolved chord must not leave its mark on the frame: {:?}",
        frame(&app)
    );
    assert!(
        !row(&app).contains("… waiting"),
        "and the hint row is the hint row again: {:?}",
        row(&app)
    );
}

#[test]
fn a_held_chord_outranks_a_toast_and_then_gives_the_row_back() {
    let mut app = shell();
    let toast = "e8f1a2 is marked reviewed — `c` compares against it";
    app.toast(toast);
    assert_eq!(app.hint_line(), toast);

    press(&mut app, "g");
    let line = app.hint_line();
    assert!(
        line.contains("… waiting") && !line.contains(toast),
        "the question the reviewer is asking right now outranks an act that is already over: {line}"
    );

    // The loan is only a wait: with the chord gone and nothing printed for it, the toast that was
    // there before is still there, on its own clock (`reco-2ll`).
    app.chords.clear();
    assert_eq!(
        app.hint_line(),
        toast,
        "the toast is not cancelled by a chord, only interrupted by one"
    );
}

#[test]
fn a_prefix_that_binds_nothing_of_its_own_says_so() {
    let mut app = shell();
    // `<C-w>` is a prefix with no binding of its own: the wait is a wait, not a decision, and the
    // line says the prefix is unbound rather than inventing a meaning for it.
    press(&mut app, "<C-w>");
    let line = app.hint_line();
    assert!(
        line.contains("`<C-w>` alone is unbound"),
        "a bare prefix is not a bound key: {line}"
    );
    assert!(
        line.contains("`<C-w>o` =") || line.contains("`<C-w>1` ="),
        "and the endings it could still take are the table's own: {line}"
    );
    assert!(
        line.contains("more — `?`"),
        "the `<C-w>` family does not fit on one row, and says so instead of clipping: {line}"
    );
}

// --- where the mark goes --------------------------------------------------------------------

#[test]
fn every_view_marks_a_held_chord_somewhere_on_the_frame() {
    // The mark is only a mark if the frame it lands on can carry it. Glance has no hint row, and a
    // size the ladder sells the hint row at has none either — so the rule is "somewhere on this
    // frame", and the test is all nine views at the narrowest size the shell promises a layout for.
    for view in ViewId::ALL {
        for size in [SIZE, (80, 24)] {
            let mut app = shell();
            app.set_view(view);
            app.set_hints(true);
            let before = frame(&app);
            press(&mut app, "g");
            if app.interaction().is_exclusive() {
                // The search box owns the keyboard (ADR-006 §7.1), so `g` there is a letter, not a
                // chord in the air: nothing is waiting and nothing may say it is.
                assert!(
                    marks(&app).is_empty(),
                    "{view:?} at {size:?} owns the keyboard: `g` typed, it did not wait"
                );
                continue;
            }
            let marked = marks(&app);
            assert_eq!(
                marked.len(),
                1,
                "{view:?} at {size:?} must say it exactly once, said {marked:?}: {:?}",
                frame(&app)
            );
            assert!(
                marked[0].contains("`g` … waiting"),
                "{view:?} at {size:?}: {:?}",
                marked[0]
            );
            assert_ne!(
                frame(&app),
                before,
                "and it has to be the frame the keystroke produced, {view:?} {size:?}"
            );
        }
    }
}

#[test]
fn a_view_with_no_hint_row_prints_the_mark_on_the_status_row() {
    // Glance is `CH_STATUS` alone: no hint row exists to print on, and the status row is the row
    // every shipped view has.
    let mut app = shell();
    app.set_view(ViewId::Ambient);
    assert!(
        !app.hint_row(),
        "glance has no hint row, which is the whole reason this case exists"
    );
    press(&mut app, "g");
    let last = row(&app);
    assert!(
        last.contains("… waiting") && last.contains("`g` alone = Graph"),
        "the status row carries it: {last}"
    );
}

#[test]
fn a_hint_line_the_reviewer_turned_off_still_marks_the_wait() {
    // `ui.hints = false` is a setting about the key hints, not a licence to go silent about a key
    // in the air (`reco-wlt`); ADR-006 §7.5's row is what it is *for*.
    let mut app = shell();
    app.set_hints(false);
    assert!(!app.hint_row());
    press(&mut app, "g");
    assert!(
        row(&app).contains("… waiting"),
        "with the hints off there is only the status row, and it says it: {:?}",
        row(&app)
    );
}

// --- the mark is the table -------------------------------------------------------------------

#[test]
fn the_mark_reads_the_merged_table_and_not_a_copy_of_it() {
    let km = Keymap::defaults();
    let queue = chain(ViewId::Queue);
    assert_eq!(
        km.extensions(&queue, "g"),
        vec![("gg".to_owned(), "First row".to_owned())],
        "the shipped table holds exactly one extension of `g`"
    );
    assert_eq!(
        km.pending_hint(&queue, "g"),
        "`g` … waiting · `g` alone = Graph in 250 ms · `gg` = First row"
    );
    assert_eq!(km.pending_hint(&queue, ""), "");
    // The ADR's claim, measured: `g`/`gg` is the one pair the published table ships, per view.
    // A character-wise prefix test would add `<`, because the table spells its named keys `<…>`.
    assert_eq!(
        km.ambiguous_chords()
            .iter()
            .map(|(chord, _)| chord.as_str())
            .collect::<Vec<_>>(),
        vec!["g"],
        "one ambiguous pair, in one place: {:?}",
        km.ambiguous_chords()
    );

    // An override that gives `g` a second extension prints it, because the line is read from the
    // table the resolver will use and not written beside it (ADR-006 §7.3, §8.3).
    let km = Keymap::merged(&[Override {
        mode: Mode::List,
        chord: "gG".to_owned(),
        action: Some("list.last".to_owned()),
        source: None,
    }]);
    let line = km.pending_hint(&chain(ViewId::Timeline), "g");
    assert!(
        line.contains("`gG` = Last row"),
        "a chord added by config has to be announced by the frame: {line}"
    );
}

#[test]
fn a_named_chord_is_not_an_extension_of_its_first_character() {
    // The other way a key can wait for nothing. The table spells its named keys `<…>`, so *every*
    // one of them starts with the character `<` — and `<` is itself a key, `structure.sort_cycle`
    // and `graph.hop_less`. A character-wise prefix test holds it for the full wait on a second key
    // no terminal can deliver: `<C-d>` arrives as one key event, never as `<` then `C-d`.
    let km = Keymap::defaults();
    for view in [ViewId::Structure, ViewId::Graph] {
        let chain = chain(view);
        assert!(
            km.resolve(&chain, "<").is_some(),
            "`<` is a real key in {view:?}: {:?}",
            km.resolve(&chain, "<")
        );
        assert!(
            !km.has_extension(&chain, "<"),
            "{view:?} binds `<C-d>` and `<Space>`, and neither starts with the keystroke `<`"
        );
        let mut state = ChordState::new();
        let steps = state.feed(&km, &chain, "<");
        assert!(
            steps[0].action().is_some() && !state.is_pending(),
            "{view:?}: `<` answers on the keystroke, not 250 ms later: {:?}",
            steps[0].action()
        );
    }
}

#[test]
fn unbinding_the_extension_takes_the_wait_away_with_it() {
    // The other half of the same claim: `g` waits *because* the table binds `gg`. A reviewer who
    // unbinds `gg` gets a `g` that answers immediately, and the frame never prints a mark for it.
    let km = Keymap::merged(&[Override {
        mode: Mode::List,
        chord: "gg".to_owned(),
        action: None,
        source: None,
    }]);
    let queue = chain(ViewId::Queue);
    assert!(!km.has_extension(&queue, "g"));
    let mut state = ChordState::new();
    let steps = state.feed(&km, &queue, "g");
    assert_eq!(
        steps.len(),
        1,
        "no extension means no wait, so one keystroke is one step"
    );
    assert_eq!(
        steps[0].action(),
        Some("view.graph"),
        "and the answer is the graph view, straight away"
    );
    assert!(!state.is_pending());
}

#[test]
fn the_help_overlay_says_which_keys_wait() {
    // The ticket's second complaint: `?` lists `g` as "Go to: Graph" and says nothing about the
    // timeout, so the table promises an instant answer the shell will not give for 250 ms.
    let overlay = Keymap::defaults().help_lines(100).join("\n");
    assert!(
        overlay.contains("held keys"),
        "the overlay has a section for this now:\n{overlay}"
    );
    assert!(
        overlay.contains("`g` … waiting · `g` alone = Graph in 250 ms · `gg` = First row"),
        "and it says it in the same words the frame does:\n{overlay}"
    );
    assert_eq!(
        Keymap::defaults().ambiguous_chords().len(),
        1,
        "the overlay prints one line per ambiguous pair, and the table ships one pair: {:?}",
        Keymap::defaults().ambiguous_chords()
    );
}

// --- the loop, end to end --------------------------------------------------------------------

/// A sink that stamps every frame it is given and ends the run when the chord has resolved.
struct Stamped {
    bus: Arc<Bus>,
    frames: Vec<(Instant, Vec<String>)>,
    deadline: Instant,
    ended: bool,
}

impl Stamped {
    fn new(bus: Arc<Bus>) -> Self {
        Self {
            bus,
            frames: Vec::new(),
            deadline: Instant::now() + Duration::from_secs(5),
            ended: false,
        }
    }
}

impl FrameSink for Stamped {
    fn draw(&mut self, app: &mut App) -> Result<(), Error> {
        self.frames.push((Instant::now(), frame_lines(app)));
        if self.ended {
            return Ok(());
        }
        let marks = self
            .frames
            .last()
            .map(|(_, lines)| lines.iter().any(|l| l.contains("… waiting")))
            .unwrap_or(false);
        // Done once the wait has been paid and spent: a frame that no longer holds the chord.
        let overdue = Instant::now() > self.deadline;
        if overdue || (!marks && self.frames.len() > 1) {
            self.ended = true;
            let _ = self.bus.send(Message::Signal(Signal::Interrupt));
        }
        Ok(())
    }
}

#[test]
fn the_mark_reaches_the_screen_long_before_the_chord_does() {
    // The ticket's measurement, at the loop's own clock: the frame the keystroke causes carries
    // the mark, and it is on the screen a fraction of the wait after the press — which is the same
    // claim the pty harness makes in `tests/test_pending_chord_contract.py`, measured where a test
    // can hold a stopwatch to it.
    let mut app = shell();
    let bus = Bus::new();
    let pressed = Instant::now();
    bus.send(Message::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::NONE,
    )));
    let mut sink = Stamped::new(bus.clone());
    let report = run_loop(&mut app, &bus, &mut sink, None, None).expect("the loop runs headlessly");
    let elapsed = pressed.elapsed();

    assert_eq!(report.end, LoopEnd::Quit, "{:?}", report.end);
    assert!(
        sink.frames.len() >= 2,
        "one frame for the mark, one for the view it was waiting about: {}",
        sink.frames.len()
    );
    let (mark_at, first) = &sink.frames[0];
    let since_press = mark_at.duration_since(pressed);
    assert!(
        first.iter().any(|line| line.contains("`g` … waiting")),
        "the first frame after the press says so: {first:?}"
    );
    assert!(
        since_press < Duration::from_millis(120),
        "the mark arrived {since_press:?} after the press; the wait it is marking is {CHORD_TIMEOUT:?}"
    );

    let (answer_at, last) = sink.frames.last().unwrap();
    assert!(
        !last.iter().any(|line| line.contains("… waiting")),
        "the wait ended and the mark went with it: {last:?}"
    );
    assert!(
        last.iter().any(|line| line.contains("Graph")),
        "and the key landed on its view at last: {:?}",
        last.first()
    );
    assert!(
        answer_at.duration_since(pressed) >= Duration::from_millis(200),
        "the graph view itself still costs the wait it always cost, counted from the press: {:?}",
        answer_at.duration_since(pressed)
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the loop woke for the deadline rather than sitting on a poll: {elapsed:?}"
    );
    assert_eq!(
        report.events, 2,
        "one keystroke and the sink's own way out — the mark is not an event"
    );
}

#[test]
fn a_double_tap_never_marks_anything() {
    // The other half of the budget: a reviewer typing `gg` at a deliberate speed gets the motion,
    // in one frame, with no mark on it — the mark is for the wait, not for the chord.
    let mut app = shell();
    let bus = Bus::new();
    bus.send(Message::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::NONE,
    )));
    bus.send(Message::Key(KeyEvent::new(
        KeyCode::Char('g'),
        KeyModifiers::NONE,
    )));
    let mut sink = RecordingSink::default();
    let report = run_loop(&mut app, &bus, &mut sink, None, Some(1)).expect("the loop runs");
    assert_eq!(report.frames, 1);
    assert!(
        !sink.lines.iter().any(|line| line.contains("… waiting")),
        "nothing waited, so nothing says it is: {:?}",
        sink.lines.first()
    );
}

#[derive(Default)]
struct RecordingSink {
    lines: Vec<String>,
}

impl FrameSink for RecordingSink {
    fn draw(&mut self, app: &mut App) -> Result<(), Error> {
        self.lines.extend(frame_lines(app));
        Ok(())
    }
}

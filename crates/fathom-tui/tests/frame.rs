//! The frame contract, as a test (ADR-002 §3, §5.3; ADR-006's headless render).
//!
//! If a view ever needs a size or a clock, it has to be added *here*, and this file is where a reviewer
//! sees the cost of that.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::insight::PAGE_LIMIT_DEFAULT;
use fathom_core::time::Timestamp;
use fathom_fixtures::FIXED_TIMESTAMP;
use fathom_tui::frame::{FrameContext, HEADLESS_SIZE};

#[test]
fn the_headless_frame_is_the_smallest_promised_layout() {
    let frame = FrameContext::headless(FIXED_TIMESTAMP);
    assert_eq!((frame.width(), frame.height()), (80, 24));
    assert_eq!((frame.width(), frame.height()), HEADLESS_SIZE);
    assert_eq!(frame.cells(), 80 * 24);
    assert_eq!(frame.timestamp(), FIXED_TIMESTAMP);
}

#[test]
fn a_frame_carries_the_pages_the_facade_agreed_to() {
    // The view does not choose its own page size; asking is the façade's job (§5.3), and a TUI that
    // paged differently is the two-realities bug ADR-001 §7.10 names.
    let default_page = FrameContext::headless(FIXED_TIMESTAMP).page();
    assert_eq!(default_page.limit(), PAGE_LIMIT_DEFAULT);

    let asked = FrameContext::new(120, 40, FIXED_TIMESTAMP, Some(10_000));
    assert_eq!(asked.width(), 120);
    assert_eq!(asked.height(), 40);
    assert!(
        asked.page().limit() < 10_000,
        "the facade clamps what a view asks for"
    );
}

#[test]
fn a_frame_is_a_value_so_a_test_can_hold_two_of_them() {
    let a = FrameContext::headless(Timestamp::UNIX_EPOCH);
    let b = FrameContext::headless(FIXED_TIMESTAMP);
    assert_ne!(a, b, "the instant is part of what a frame depends on");
    assert!(a.timestamp() < b.timestamp());
}

#[test]
fn a_plain_frame_is_text_and_nothing_else() {
    // `--plain` is the same renderer with no terminal: the shell's own advice in the refusal panel
    // promises "text for pipes", so this is the promise kept (ADR-006 §6.7).
    let options = fathom_tui::frame::Options::default();
    let lines = fathom_tui::frame::render_plain(&options);
    assert!(
        !lines.iter().any(|line| line.contains('\x1b')),
        "a plain frame may not contain an escape sequence: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("epoch unknown")),
        "unknown is a value, not a blank (ADR-001 §7.7): {lines:?}"
    );
}

/// The run's numbers, and the run's numbers *stated* (`reco-08r`).
///
/// The frame budget is a claim about these numbers, and the defect this test exists for is a binary that
/// computed them and threw them away. So the loop is driven for real, and the report is checked twice:
/// once as data (every statistic with the denominator and the budget it was taken against) and once as
/// the line the shell prints on the way out, which has to say the same things.
#[test]
fn a_run_reports_its_own_frame_budget_and_says_what_it_measured_against() {
    use std::time::Duration;

    use crossterm::event::{KeyCode, KeyModifiers};
    use fathom_tui::app::{App, Model};
    use fathom_tui::event::{Bus, Message};
    use fathom_tui::frame::{HeadlessSink, LoopEnd, run_loop};
    use fathom_tui::keymap::Keymap;

    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        (100, 30),
        "fathom/pilot · main",
    );
    let bus = Bus::new();
    for ch in "jjk".chars() {
        bus.send(Message::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char(ch),
            KeyModifiers::NONE,
        )));
    }
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: None,
    };
    let report = run_loop(&mut app, &bus, &mut sink, None, Some(1)).expect("the loop runs");

    assert_eq!(report.end, LoopEnd::FrameLimit);
    assert!(
        report.frames >= 1,
        "a loop that drew nothing measured nothing"
    );
    assert_eq!(report.events, 3);
    assert_eq!(
        report.budget,
        Duration::from_millis(16),
        "ADR-002 §6's budget travels with the number measured against it"
    );
    assert_eq!(report.size, (100, 30), "a frame cost is a cost at a size");
    assert_eq!(
        report.window,
        usize::try_from(report.frames)
            .unwrap_or(usize::MAX)
            .min(fathom_tui::event::FrameScheduler::WINDOW),
        "the window is the denominator of every statistic below it"
    );
    assert!(
        report.p99 <= report.worst,
        "a p99 above the worst draw is not a percentile"
    );
    assert!(report.over_budget <= report.window);

    let line = report.summary();
    for fact in [
        "frames 1",
        "events 3",
        "p99 ",
        "worst ",
        &format!("over-budget {} of the last 1 draws", report.over_budget),
        "budget 16.00ms",
        "100x30",
        "frame limit reached",
    ] {
        assert!(
            line.contains(fact),
            "the exit line is missing {fact:?}: {line}"
        );
    }
}

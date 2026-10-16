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

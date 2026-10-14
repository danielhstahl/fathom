//! The frame contract: what a view is handed, and therefore what a view can depend on.
//!
//! A view that reads the terminal size or the clock itself is a view that cannot be tested without a
//! terminal and a timezone. Both arrive as arguments instead, which is what buys the headless render
//! `e5.5` needs and the golden file that stays golden.

use crate::error::Error;
use fathom_analyze::insight::{self, PageWindow};
use fathom_core::time::Timestamp;

/// The size a headless test renders at (ADR-002 §3, `e5.5`): the smallest terminal fathom promises a
/// layout for, so a passing golden file is the narrow case, not the comfortable one.
pub const HEADLESS_SIZE: (u16, u16) = (80, 24);

/// Everything a frame may depend on.
///
/// Deliberately: a size, an instant and the page the façade agreed to. Not a store, not a terminal, not
/// "now".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameContext {
    width: u16,
    height: u16,
    now: Timestamp,
    page: PageWindow,
}

impl FrameContext {
    /// The fixed frame a test uses. `now` is still a parameter, because a fixture that reads a clock is
    /// a fixture with a flake in it (ADR-002 §4.4).
    #[must_use]
    pub fn headless(now: Timestamp) -> Self {
        Self::new(HEADLESS_SIZE.0, HEADLESS_SIZE.1, now, None)
    }

    /// Build a frame, asking the façade — not the view — for the page window (§5.3).
    #[must_use]
    pub fn new(width: u16, height: u16, now: Timestamp, page_limit: Option<u32>) -> Self {
        FrameContext {
            width,
            height,
            now,
            page: insight::PageWindow::new(None, page_limit),
        }
    }

    /// Columns available to the layout.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// Rows available to the layout.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    /// The instant this frame is drawn for; the only way a view learns what "recent" means.
    #[must_use]
    pub const fn timestamp(&self) -> Timestamp {
        self.now
    }

    /// The page the façade will answer with, already clamped.
    #[must_use]
    pub const fn page(&self) -> PageWindow {
        self.page
    }

    /// Cells in the frame — the budget a view is not allowed to exceed (ADR-004 §7.3).
    #[must_use]
    pub const fn cells(&self) -> usize {
        self.width as usize * self.height as usize
    }
}

/// Run the application until it exits.
///
/// Returns the typed failure until `e1.5` (`reco-v6t`) lands the event loop; the binary's `tui` feature
/// gates whether this is even compiled in, and `e5.5` adds the non-TTY degradation that turns this
/// refusal into a useful message instead of a wall of escape codes.
///
/// # Errors
///
/// [`Error::NotImplemented`] until the shell exists.
pub fn run(_now: Timestamp) -> Result<(), Error> {
    Err(Error::NotImplemented {
        what: "the terminal application",
        ticket: "e1.5",
    })
}

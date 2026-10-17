//! `fathom_analyze::insight` — the narrow waist (ADR-002 §5.3, `e2.11`/`reco-tt4`).
//!
//! This is the only module in `fathom-analyze` that `fathom-tui` and `fathom-mcp` may name, and it is a
//! *service*, not a bag of types: it owns pagination, the memory budget, the truncation flags and the
//! `unknown` values, so that a page of episodes means the same thing in the TUI, in `fathom --json`,
//! in an exported artefact and over MCP (ADR-001 §7.10, ADR-004 §5.6). When a view needs something the
//! façade does not expose, the façade gains a method in a PR `analyze-insight` reviews; the view does
//! not reach past it, and lint `M02` is what makes that sentence a fact rather than a hope.
//!
//! The delta model itself is `e2.11`'s. What is implemented here at the scaffold is the part that must
//! not be invented twice: the version marker and the paging rule.

#![deny(missing_docs)]

use crate::error::Error;

/// The version of the shapes this façade will publish.
///
/// ADR-004 §5.2 versions serialised structures by declaring the number they were written against, and
/// the compiled-in feature set has to reach that number too (§6.4, recorded as `reco-quk`). Bumping this
/// is a schema decision, not a code edit.
pub const SCHEMA_VERSION: u32 = 1;

/// The page size a caller gets when it does not ask for one.
pub const PAGE_LIMIT_DEFAULT: u32 = 100;

/// The largest page any caller may ask for. One number, here, so that "the TUI asked for 100k episodes
/// and the process died" is not a per-view discovery (ADR-002 §5.3: the façade owns the memory budget).
pub const PAGE_LIMIT_MAX: u32 = 500;

/// A requested window over a collection: offset, size, and no opinion about what is in it.
///
/// Constructed through [`PageWindow::new`] so the clamping rule is applied rather than remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageWindow {
    offset: u32,
    limit: u32,
}

impl PageWindow {
    /// Clamp a caller's request into what the façade will answer.
    ///
    /// `None` means "the default", not "unbounded": an unbounded page is a memory ceiling somewhere
    /// downstream, and ADR-004 §7 knows where those lead.
    #[must_use]
    pub fn new(offset: Option<u32>, limit: Option<u32>) -> Self {
        let limit = limit.unwrap_or(PAGE_LIMIT_DEFAULT).min(PAGE_LIMIT_MAX);
        PageWindow {
            offset: offset.unwrap_or(0),
            limit,
        }
    }

    /// The index of the first item in the window.
    #[must_use]
    pub const fn offset(&self) -> u32 {
        self.offset
    }

    /// How many items the window may hold — an upper bound, not a promise.
    #[must_use]
    pub const fn limit(&self) -> u32 {
        self.limit
    }

    /// One past the last index in the window, saturating so a hostile offset cannot wrap to zero.
    #[must_use]
    pub const fn end(&self) -> u32 {
        self.offset.saturating_add(self.limit)
    }

    /// Is this an empty window? A caller that skips the query on `true` is why the type is worth having.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.limit == 0
    }
}

/// What the façade is, as a value a surface can print.
///
/// `views_ready` is false until `e2.11` lands the delta model: a surface that asks gets told, which is
/// the "degrade, never block" rule (ADR-001 §7.6) applied to the façade itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FacadeStatus {
    /// The version marker above.
    pub schema_version: u32,
    /// Whether any view can be answered yet.
    pub views_ready: bool,
}

/// The façade's own status, for `fathom doctor` and for the headless binary's self-check.
#[must_use]
pub const fn status() -> FacadeStatus {
    FacadeStatus {
        schema_version: SCHEMA_VERSION,
        views_ready: false,
    }
}

/// The rows a page of the delta model will carry.
///
/// `e2.11` replaces the unit type with `EpisodeDelta`; the *shape around it* — the window, the
/// truncation flag, the `unknown` values — is what this façade owns and what every surface draws
/// through, so the alias exists to be edited exactly once.
pub type DeltaRows = Vec<()>;

/// Ask for a page of episodes (ADR-002 §5.3: one answer, in one place).
///
/// Returns the typed failure until `e2.11` replaces the body with the delta model. The signature is
/// the point: callers already receive a [`PageWindow`] they did not have to clamp themselves, and a
/// view that wanted to clamp differently has already lost.
///
/// # Errors
///
/// [`Error::NotImplemented`] until the delta model exists.
pub fn episodes(_window: PageWindow) -> Result<DeltaRows, Error> {
    Err(Error::NotImplemented {
        what: "the versioned delta model",
        ticket: "e2.11",
    })
}

// --- the spine-only ranking (`reco-c05`) ---------------------------------------------------------
//
// The façade does not compute it — [`crate::risk`] does, in the open — but this is the door the
// surfaces name, and ADR-002 §5.3 is not a rule you may route around when the thing behind the door
// happens to be pure. Before this, a reviewer in the shipped TUI could not answer "which three
// episodes are the riskiest": the rail's risk column printed `?`, the queue had no rows, and `w`
// said *owned by e4.4*. Every input here is a row the spine already writes, so the ordering is
// auditable from the frame while `e4.4`'s feature vector is still on the other side of epic 2.
//
// `views_ready` stays false and this does not change it: a ranking built from the spine is not the
// delta model, and a surface that reads these numbers is not being told `e2.11` landed.
pub use crate::risk::{
    BLIND_SPOTS, COMPONENTS, MODEL, Risk, Signals, Surface, classify, rank,
};

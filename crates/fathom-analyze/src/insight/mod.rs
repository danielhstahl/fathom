//! `fathom_analyze::insight` — the narrow waist (ADR-002 §5.3, `e2.11`/`reco-tt4`).
//!
//! This is the only module in `fathom-analyze` that `fathom-tui` and `fathom-mcp` may name, and it is a
//! *service*, not a bag of types: it owns pagination, the memory budget, the truncation flags and the
//! `unknown` values, so that a page of episodes means the same thing in the TUI, in `fathom --json`,
//! in an exported artefact and over MCP (ADR-001 §7.10, ADR-004 §5.6). When a view needs something the
//! façade does not expose, the façade gains a method in a PR `analyze-insight` reviews; the view does
//! not reach past it, and lint `M02` is what makes that sentence a fact rather than a hope.
//!
//! # What landed in `e2.11`
//!
//! [`delta`] is the versioned model every surface renders: [`EpisodeDelta`] and the seven sections, with
//! their evidence pointers and their self-check. [`schema`] generates `schemas/episode-delta.schema.json`
//! from it. [`status`] now says `views_ready: true`, which is a claim a consumer can act on: the model
//! exists, it is versioned, and a surface may render it instead of printing a placeholder.
//!
//! What is *not* here is a second set of calculations. The sections are composed from rows the caller
//! already read ([`delta::Input`]) plus whatever analyses the caller can lend ([`delta::Supplied`]); the
//! analyses themselves live where they always lived ([`crate::risk`], [`crate::deps`], and the Python
//! surface behind `e2.2`/`e2.3`/`e2.8`/`e2.9`). The façade is the door and the shaper, never a second engine.

#![deny(missing_docs)]

mod delta;
mod schema;

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
use crate::error::Error;

pub use delta::{
    AttributionEvidence, AttributionRef, BriefRow, EpisodeDelta, EpisodeRef, Evidence, Figure,
    FileDelta, Finding, Gap, INTENT_DIGEST_ONLY, INTENT_NONE, INTENT_RECORDED, Input, Integrity,
    Provenance, RiskComponent, RiskView, STATE_ALL, STATE_MEASURED, STATE_PARTIAL,
    STATE_UNAVAILABLE, Section, SectionId, Supplied, SuppliedBlock, Totals, brief, compose,
    glyph_of, intent_block, render, short_id,
};
pub use schema::{
    SCHEMA_FILE, describe as schema_describe, document as schema_document, limits as schema_limits,
    to_text as schema_to_text,
};

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub use delta::{for_commit, for_episode, for_rows};

/// The version of the shapes this façade publishes.
///
/// ADR-004 §5.2 versions serialised structures by declaring the number they were written against, and the
/// compiled-in feature set has to reach that number too (§6.4, recorded as `reco-quk`). Bumping this is a
/// schema decision, not a code edit: it lives in [`delta`] beside the types it describes, and
/// `tools/check_delta_compat.py` fails CI when the generated schema and this number disagree about what
/// version they are.
pub const SCHEMA_VERSION: u32 = delta::VERSION;

/// The one verb that produces a delta document, quoted verbatim by every surface that has to admit it
/// did not get one.
///
/// A surface with no document has exactly one useful thing to say, and it is an instruction: go run this.
/// Keeping the string here means the card, the CLI and the export cannot drift into naming three different
/// verbs for the same document, which is how "not in this build" turns into a permanent shrug.
pub const UNDELIVERED_VERB: &str = "fathom analyze <episode> --json";

/// The contract id every payload carries ([`delta::CONTRACT`]).
pub const CONTRACT: &str = delta::CONTRACT;

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
/// `views_ready` became true when `e2.11` landed the delta model: it is the answer to "can a view be
/// answered at all", and a surface that asks gets told, which is the "degrade, never block" rule
/// (ADR-001 §7.6) applied to the façade itself. It is *not* a claim that every section is filled — a
/// section that has no answer says so inside the document, with the ticket that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FacadeStatus {
    /// The version marker above.
    pub schema_version: u32,
    /// The contract id, so a status line can name the shape rather than only its age.
    pub contract: &'static str,
    /// Whether any view can be answered yet.
    pub views_ready: bool,
}

/// The façade's own status, for `fathom doctor` and for the headless binary's self-check.
#[must_use]
pub const fn status() -> FacadeStatus {
    FacadeStatus {
        schema_version: SCHEMA_VERSION,
        contract: CONTRACT,
        views_ready: true,
    }
}

/// A page of deltas, with what the read left out.
///
/// The truncation flag is the point of the struct: a consumer that receives a bare `Vec` cannot tell "the
/// repository has 4 episodes" from "you asked for 4 of 5 000", and ADR-004 §7's answer to a silently short
/// page is a support ticket.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#[derive(Debug, Clone)]
pub struct Page {
    /// The window that was asked for, already clamped.
    pub window: PageWindow,
    /// How many live episodes the store holds under the same query — the denominator, not the page.
    pub total: u64,
    /// The deltas, in the store's order.
    pub deltas: Vec<EpisodeDelta>,
    /// Whether the store held more than the window allowed.
    pub truncated: bool,
}

/// Ask for a page of deltas (ADR-002 §5.3: one answer, in one place).
///
/// The window is clamped here and nowhere else; the count and the page come from one query pair, so the
/// "3 of 5 000" a strip prints is the same denominator the store gave the page. Rows are read with the
/// summed per-path rollup — the rail's basis — so a page of five hundred deltas costs four statements,
/// not fifteen hundred. A caller that wants the per-commit detail for one episode uses [`delta::for_episode`],
/// which says so in its own `provenance.files_detail`.
///
/// # Errors
///
/// [`Error::Store`] when the store refuses.
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub fn episodes(
    store: &fathom_store::spine::Store,
    query: &fathom_store::episode::EpisodeQuery,
    window: PageWindow,
) -> Result<Page, Error> {
    use fathom_store::episode::ReviewState;

    let total = store.count_episodes(query)?;
    let rows = store.episodes(query)?;
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let files = store.episode_files(&ids)?;
    let states = store.review_states()?;
    let deltas = rows
        .iter()
        .map(|row| {
            let review = states.get(&row.id).copied().unwrap_or(ReviewState::Unread);
            let rows = files.by_episode.get(&row.id).map(Vec::as_slice);
            delta::for_rows(
                row,
                rows.unwrap_or_default(),
                review,
                files.truncated || rows.is_none(),
            )
        })
        .collect();
    Ok(Page {
        window,
        total: u64::try_from(total.max(0)).unwrap_or(u64::MAX),
        truncated: u64::from(window.limit()) < u64::try_from(total.max(0)).unwrap_or(u64::MAX),
        deltas,
    })
}

// --- the spine-only ranking (`reco-c05`) ---------------------------------------------------------
//
// The façade does not compute it — [`crate::risk`] does, in the open — but this is the door the
// surfaces name, and ADR-002 §5.3 is not a rule you may route around when the thing behind the door
// happens to be pure. Every input here is a row the spine already writes, so the ordering is auditable
// from the frame while `e4.4`'s feature vector is still on the other side of epic 2.
//
// `e2.11` did not change the ranking; it gave it a place to travel. The same [`Risk`] that ranks the rail is
// copied into every delta's `risk` section, so the card, the queue, the artefact and the MCP tool print one
// number rather than four close approximations of it.
pub use crate::risk::FileChange;
pub use crate::risk::{BLIND_SPOTS, COMPONENTS, MODEL, Risk, Signals, Surface, classify, rank};

// ---------------------------------------------------------------------------
// the attention queue, through the same door (`e4.5`, `reco-wsj`)
// ---------------------------------------------------------------------------
//
// The queue's ordering, its four triage acts and its retrospective are computed in
// [`crate::queue`] — the façade computes nothing, as its own doc insists — but every surface that *shows*
// them has to name them, and `fathom-tui` may not reach past this module (ADR-002 §5.3, lint `M02`). Same
// arrangement as the risk block above: the door is the façade, the arithmetic is `queue`'s.
//
// The functions come through too, and that is deliberate rather than leakage: the queue re-ranks itself
// after every keystroke, and a shell that re-derived the order locally would be a second ranking — the
// same argument `e2.11` made about a second copy of `RiskView`. `rank` is the spine's ([`crate::risk::rank`]),
// so the queue's arrives as `rank_queue` and nobody has to remember which is which.
pub use crate::queue::{
    BlastInput, CONDITIONS, DismissReason, Disposition, QueueCounts, QueuePlacement, QueuePolicy,
    QueueRanking, QueueRetrospective, QueueRow, QueueWindow, REASONS, ReasonVocabulary, Severity,
    SnoozeSituation, SnoozeUntil, TriageAct, TriageRecord, rank_queue, retrospect,
};

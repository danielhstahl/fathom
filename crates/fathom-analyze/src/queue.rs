//! `fathom_analyze::queue` — the attention queue: the ordering, the triage vocabulary, the retrospective
//! (`e4.5`, `reco-wsj`).
//!
//! ADR-006 §5.1 calls the queue *the home view* and draws it with a strip that says
//! `order: risk × staleness × blast radius (w)`. This module is the sentence behind that strip, and the
//! three things a strip like that owes a reviewer:
//!
//! * **the order** ([`rank_queue`]) — risk, weighted by how long it has sat unread and by what depends on
//!   it, as three printed integers whose product is the number on the row, with a tie-break chain that
//!   ends at an id so the same index always draws the same screen;
//! * **the acts** ([`TriageAct`]) — four of them, one keystroke each, two of which refuse to happen
//!   without a reason, and each of which produces a [`TriageRecord`] naming who, when, what they claimed
//!   and which episode identity they signed (`e4.6` keys a sign-off to that identity and its
//!   fingerprint);
//! * **the retrospective** ([`retrospect`]) — the same acts rolled back up, so a dismissal category that
//!   has swallowed a third of the queue is a number on a screen rather than a feeling.
//!
//! # What is deliberately *not* here
//!
//! **No store, no clock, no config.** `fathom-cli` reads all three and hands them over: it stamps the
//! [`TriageRecord`]s it writes with its own identity and clock ([`TriageAct`] carries neither), passes
//! `now` to every function that needs an age ([`rank_queue`], [`retrospect`]), and builds the
//! [`ReasonVocabulary`] from `fathom.toml` before this module ever sees it (ADR-002 §4.4, §4.5). That is
//! what lets every number here be a golden test rather than a screenshot of this afternoon.
//!
//! **No scoring.** [`QueueRow::risk`] is a number this module is handed, produced by
//! [`crate::risk`](crate::risk) (`spine-1` today) or by [`crate::risk::score`](crate::risk::score)
//! (`features-1`) the day a caller computes it for a whole page. Computing `features-1` costs a corpus
//! read plus a detector run *per episode*, which is why the queue page does not do it inside a
//! four-query frame budget — and why the queue prints the score's model name on every row instead of
//! implying that the queue scored it.
//!
//! **No blast radius of its own.** [`BlastInput::Unmeasured`] is the shipped state until `e3.1`/`e3.2`
//! build the module graph; the factor is a door with a name on it, exactly as
//! [`crate::risk::collect::Supply`] is, and the strip says the factor is sitting at `1.00` rather than
//! letting a missing input look like an absent risk.
//!
//! # Where it runs
//!
//! `fathom-cli` is the only caller that computes: it ranks a page in the `episodes` module the way it
//! ranks the rail today, writes the four acts to the store's ledger, and hands the placements and the
//! retrospective's lines to the shell through [`crate::insight`], which re-exports the handful of shapes
//! a surface has to name so the TUI and the MCP server read one of each rather than inventing their own
//! (lint `M02`). The keystrokes themselves are `fathom-tui`'s; what a keystroke *means* is here.

#![deny(missing_docs)]

mod disposition;
mod order;
mod retrospect;

pub use disposition::{
    CONDITIONS, DismissReason, Disposition, REASONS, ReasonVocabulary, SnoozeSituation,
    SnoozeUntil, TriageAct, TriageRecord, is_code, newest_first, who_and_when,
};
pub use order::{
    BLAST_CAP_H, BLAST_STEP_H, BlastInput, DAY_SECONDS, QUEUE_MODEL, QueueCounts, QueuePlacement,
    QueuePolicy, QueueRanking, QueueRow, SEVERITY_HIGH, SEVERITY_MEDIUM, STALENESS_CAP_H,
    STALENESS_STEP_H, Severity, WeightFactor, age_words, rank_queue, staleness,
};
pub use retrospect::{
    Coverage, QueueRetrospective, QueueWindow, Rollup, SYSTEMATIC_MIN_COUNT,
    SYSTEMATIC_MIN_SHARE_BP, SkippedEpisode, retrospect,
};

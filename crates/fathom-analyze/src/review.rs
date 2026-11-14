//! `fathom_analyze::review` — the review ledger's analysis: the triage vocabulary, the open-work
//! counts, the retrospective (`e4.5` `reco-wsj` for the acts, `e10.3` `reco-ydek.3` for this name).
//!
//! This is what survived the attention queue. ADR-006 §5.1.2 is the decision; the short version is that
//! the queue was a pane over three durable things — the four acts, the ledger they write, and the
//! rollup that reads them back — and only the pane was retired. What used to be `fathom_analyze::queue`
//! is this, and the ordering that gave the old module its name is deleted rather than renamed: a
//! `rank_queue` nobody calls would be a second answer to a question the product stopped asking.
//!
//! The three things a reviewer's ledger owes a surface, and where each lives:
//!
//! * **the acts** ([`TriageAct`]) — four of them, one keystroke each, two of which refuse to happen
//!   without a reason, and each of which produces a [`TriageRecord`] naming who, when, what they claimed
//!   and which episode identity they signed (`e4.6` keys a sign-off to that identity and its
//!   fingerprint);
//! * **the count** ([`count_rows`], [`ReviewCounts`]) — how many episodes are still asking to be read,
//!   in which severity bands, and what has left. This is the number behind *caught up*, on the rail's
//!   strip and in the ambient glance, and it is a count of episodes in review states rather than of
//!   rows in a pane (ADR-006 §5.1.2 §3 part 2);
//! * **the retrospective** ([`retrospect`]) — the same acts rolled back up, so a dismissal category
//!   that has swallowed a third of the review is a number on a screen rather than a feeling.
//!
//! # What is deliberately *not* here
//!
//! **No store, no clock, no config.** `fathom-cli` reads all three and hands them over: it stamps the
//! [`TriageRecord`]s it writes with its own identity and clock ([`TriageAct`] carries neither), passes
//! `now` to every function that needs an age ([`retrospect`]), and builds the [`ReasonVocabulary`] from
//! `fathom.toml` before this module ever sees it (ADR-002 §4.4, §4.5). That is what lets every number
//! here be a golden test rather than a screenshot of this afternoon.
//!
//! **No scoring.** [`ReviewRow::risk`] is a number this module is handed, produced by
//! [`crate::risk`](crate::risk) (`spine-2` today) or by [`crate::risk::score`](crate::risk::score)
//! (`features-2`) the day a caller computes it for a whole page. The risk score stays a per-episode
//! figure printed on a row and explained in the card (ADR-006 §5.1.2 §5); nothing here re-ranks it.
//!
//! **No ordering.** The rail is a chronology and prints `order: newest first`; that order is the
//! renderer's business, not this module's. A caller that wants the riskiest row first sorts the rows it
//! was handed — which is what `fathom-tui`'s rail does, and it does it with the score it was given.
//!
//! # Where it runs
//!
//! `fathom-cli` is the only caller that computes: it reads a page in the `episodes` module, writes the
//! four acts to the store's ledger, and hands the counts and the retrospective's lines to the shell
//! through [`crate::insight`], which re-exports the handful of shapes a surface has to name so the TUI
//! and the MCP server read one of each rather than inventing their own (lint `M02`). The keystrokes
//! themselves are `fathom-tui`'s; what a keystroke *means* is here.

#![deny(missing_docs)]

mod counts;
mod disposition;
mod retrospect;

pub use counts::{
    ReviewCounts, ReviewRow, SEVERITY_HIGH, SEVERITY_MEDIUM, Severity, count_rows,
};
pub use disposition::{
    CONDITIONS, DismissReason, Disposition, REASONS, ReasonVocabulary, SnoozeSituation,
    SnoozeUntil, TriageAct, TriageRecord, is_code, newest_first, who_and_when,
};
pub use retrospect::{
    Coverage, ReviewRetrospective, ReviewWindow, Rollup, SYSTEMATIC_MIN_COUNT,
    SYSTEMATIC_MIN_SHARE_BP, SkippedEpisode, retrospect,
};

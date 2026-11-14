//! The counts and the bands: what a set of episodes still asking to be read adds up to, and which
//! severity band each open row falls in (`e4.5` shipped these inside the queue's ordering; `e10.3`
//! deleted the ordering and kept the arithmetic that outlived it).
//!
//! ```text
//! 7 of 12 open · 2 high 3 med 2 low · 4 dismissed 1 snoozed
//! ```
//!
//! That line is printed above the rail's rows, in the ambient glance, and in the review artefact, and
//! it is one function rather than three because a count of open work printed in two fonts is a count
//! that can disagree with itself (ADR-001 §7.10).
//!
//! # What changed when the queue went
//!
//! The ordering — `queue-1`, risk × staleness × blast — is deleted, and with it [`Ordering`-shaped]
//! figures no surface can cite any more: a place in a ranked set, the two multipliers, and the strip
//! that named the function. What survives here is a *count of episodes in review states*, which was
//! never a property of a pane: ADR-006 §5.1.2 §3 part 2 says so plainly, and the ambient pane's
//! open-work line is the same number wearing the rail's clothes.
//!
//! [`Severity`] stays because two surfaces bucket the score the same way and must not do it twice: the
//! glance's *look now* line and the count's own bands. Its floors are the shipped ones; the ordering's
//! knob (`QueuePolicy`) went with the ordering and nothing ever configured it.
//!
//! # What is deliberately *not* here
//!
//! No store, no clock, no scoring — [`ReviewRow::risk`] is a number the caller read, exactly as it was
//! when it lived in `order` (ADR-002 §4.4).

use serde::Serialize;

use super::disposition::Disposition;

/// A score at or above this is `high`.
///
/// This buckets the score a caller is *handed*, which is the delta's `spine-2` number, and it is
/// deliberately **not** `risk.tau`. The two were the same number until `reco-6co` measured τ against real
/// history: `risk.tau` is now 40 because that is the `features-2` floor three real repositories support
/// (`features-2`'s ceiling on that corpus was 63, so 70 flagged nothing), and this constant still buckets
/// `spine-2`, a different model with its own range. "Flagged" is what `fathom why` says about the score it
/// prints; "high" is what a count says about the score in the delta. A repository that wants one number
/// for both sets `[risk] tau` and says so in the same commit that changes this constant — what is not
/// allowed is a document that quotes one as the other.
pub const SEVERITY_HIGH: u8 = 70;

/// A score at or above this is `medium`, and below it `low`. Half of [`SEVERITY_HIGH`], and stated
/// rather than derived at the call site so a document can quote one number.
pub const SEVERITY_MEDIUM: u8 = 35;

/// How consequential a change is, in three words.
///
/// Severity is the *risk score* bucketed; the rail's order is the same episodes in the order they
/// happened. The two are different questions and a glance prints both — "3 high, 6 medium, 22 low" is a
/// statement about the code, "newest first" is a statement about the reading order (ADR-006 §5.1.2 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// At or above [`SEVERITY_HIGH`].
    High,
    /// At or above [`SEVERITY_MEDIUM`].
    Medium,
    /// Below [`SEVERITY_MEDIUM`].
    Low,
}

impl Severity {
    /// The band a score falls in, at the shipped floors.
    #[must_use]
    pub const fn of(score: u8) -> Self {
        if score >= SEVERITY_HIGH {
            Severity::High
        } else if score >= SEVERITY_MEDIUM {
            Severity::Medium
        } else {
            Severity::Low
        }
    }

    /// The word the screen prints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Severity::High => "high",
            Severity::Medium => "med",
            Severity::Low => "low",
        }
    }
}

/// One episode, in the shape the counting needs.
///
/// Everything here is a row the caller already read. This module adds no measurement of its own, which is
/// what lets the same function count a page of the index, a fixture and the artefact.
#[derive(Debug, Clone)]
pub struct ReviewRow {
    /// The episode's identity, as every other surface names it.
    pub episode: String,
    /// The risk score, when anything scored this episode. `None` is *unscored*, which is a state of the
    /// tool and not a score of zero.
    pub risk: Option<u8>,
    /// Which model scored it, or the empty string.
    pub risk_model: String,
    /// The disposition this episode carries into the count.
    pub disposition: Disposition,
}

impl ReviewRow {
    /// A row something scored.
    #[must_use]
    pub fn scored(episode: impl Into<String>, risk: u8, disposition: Disposition) -> Self {
        ReviewRow {
            episode: episode.into(),
            risk: Some(risk),
            risk_model: String::new(),
            disposition,
        }
    }

    /// A row nothing scored.
    #[must_use]
    pub fn unranked(episode: impl Into<String>, disposition: Disposition) -> Self {
        ReviewRow {
            episode: episode.into(),
            risk: None,
            risk_model: String::new(),
            disposition,
        }
    }

    /// Is this row still asking to be read? A snooze counts as open once `released` says its condition
    /// has happened.
    #[must_use]
    pub fn open(&self, released: bool) -> bool {
        self.disposition.open(released)
    }
}

/// What a set of episodes holds, counted the way the strip prints it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ReviewCounts {
    /// Rows in the run at all.
    pub total: usize,
    /// Rows still asking to be read.
    pub open: usize,
    /// Open rows with no score at all — printed, because a pile of `?`s is not a caught-up pile.
    pub unranked: usize,
    /// Open rows by severity band.
    pub high: usize,
    /// See [`ReviewCounts::high`].
    pub medium: usize,
    /// See [`ReviewCounts::high`].
    pub low: usize,
    /// Rows signed off.
    pub accepted: usize,
    /// Rows flagged for someone else.
    pub flagged: usize,
    /// Rows dismissed with a reason.
    pub dismissed: usize,
    /// Rows snoozed and not yet released.
    pub snoozed: usize,
    /// Rows snoozed whose condition has since been met — counted separately because they are about to
    /// be open again and the reviewer should see that coming.
    pub released: usize,
}

/// Count a set of rows: how many are open, in which bands, and what has left.
///
/// `released` says, per episode, whether a snooze's condition has since been met — the caller knows that
/// (it holds the index epoch and the session), and this function must not guess. A released snooze is an
/// open row again.
///
/// One pass, no ordering: the count and the rail's rows come from the same set, and the count is not a
/// claim about which row a human should read first.
#[must_use]
pub fn count_rows(inputs: &[ReviewRow], released: impl Fn(&str) -> bool) -> ReviewCounts {
    let mut counts = ReviewCounts {
        total: inputs.len(),
        ..ReviewCounts::default()
    };
    for input in inputs {
        match input.disposition {
            Disposition::Unread | Disposition::Read => {}
            Disposition::Accepted => counts.accepted += 1,
            Disposition::Flagged => counts.flagged += 1,
            Disposition::Dismissed => counts.dismissed += 1,
            Disposition::Snoozed => {
                if input.open(released(&input.episode)) {
                    counts.released += 1;
                } else {
                    counts.snoozed += 1;
                }
            }
        }
        if !input.open(released(&input.episode)) {
            continue;
        }
        counts.open += 1;
        match input.risk.map(Severity::of) {
            Some(Severity::High) => counts.high += 1,
            Some(Severity::Medium) => counts.medium += 1,
            Some(Severity::Low) => counts.low += 1,
            None => counts.unranked += 1,
        }
    }
    counts
}

impl ReviewCounts {
    /// The one-line count above the rows, and the one the ambient pane prints.
    ///
    /// `7 of 12 open · 2 high 3 med 2 low · 4 dismissed · 1 snoozed`. Zero bands are left out rather
    /// than printed as zeroes, which is the difference between a count and a form, and the denominator
    /// is there so "7 open" cannot be read as "7 of everything the repository has ever had".
    ///
    /// It deliberately does not say "queue", and since `e10.3` it does not have to explain itself: the
    /// frame's status line prints the same open-work count, and both count episodes in open review
    /// states among the rows the pane was handed — unread, opened, flagged, and a snooze whose condition
    /// has come due. A reviewer who sees one number in two places must be able to check that it is the
    /// same number, which is what one function producing both sentences buys.
    #[must_use]
    pub fn line(&self) -> String {
        if self.total == 0 {
            return "nothing to review yet — the index holds no episodes".to_owned();
        }
        if self.open == 0 {
            return self.caught_up_line();
        }
        let mut out = self.open_words();
        let tally = self.tally();
        if !tally.is_empty() {
            out.push_str(" · ");
            out.push_str(&tally);
        }
        out
    }

    /// The count itself — `7 unread of 12 · 2 high 3 med 1 low · 6 unscored` — without what left.
    #[must_use]
    pub fn open_words(&self) -> String {
        let mut out = format!("{} unread of {}", self.open, self.total);
        let bands = [
            (Severity::High.label(), self.high),
            (Severity::Medium.label(), self.medium),
            (Severity::Low.label(), self.low),
        ];
        let bands: Vec<String> = bands
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(label, n)| format!("{n} {label}"))
            .collect();
        if !bands.is_empty() {
            out.push_str(" · ");
            out.push_str(&bands.join(" "));
        }
        if self.unranked > 0 {
            out.push_str(&format!(" · {} unscored", self.unranked));
        }
        out
    }

    /// What left the set, and as what. Empty when nothing has been dispositioned, so the caller can
    /// tell "nothing to report" from "a set of nothing" without a second test.
    ///
    /// Split out because the count line has a width to respect and this is its least important clause.
    #[must_use]
    pub fn tally(&self) -> String {
        [
            ("dismissed", self.dismissed),
            ("snoozed", self.snoozed),
            ("flagged", self.flagged),
            ("reviewed", self.accepted),
        ]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(label, n)| format!("{n} {label}"))
        .collect::<Vec<_>>()
        .join(" ")
    }

    /// Whether the reviewer has nothing left to read.
    ///
    /// `total > 0` is part of the condition: an empty index is not a caught-up reviewer, and printing
    /// the caught-up state over no data is the tool congratulating itself for nothing.
    #[must_use]
    pub const fn caught_up(&self) -> bool {
        self.total > 0 && self.open == 0
    }

    /// The caught-up sentence, which says what *how* — a set emptied by reading and one emptied by
    /// dismissing are different afternoons and the reviewer should be able to tell them apart.
    #[must_use]
    pub fn caught_up_line(&self) -> String {
        format!(
            "caught up — {} episodes, {} reviewed, {} dismissed, {} snoozed, {} flagged",
            self.total, self.accepted, self.dismissed, self.snoozed, self.flagged
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, risk: Option<u8>, disposition: Disposition) -> ReviewRow {
        ReviewRow {
            episode: id.to_owned(),
            risk,
            risk_model: String::new(),
            disposition,
        }
    }

    #[test]
    fn severity_bands_use_the_shipped_floors() {
        assert_eq!(Severity::of(SEVERITY_HIGH), Severity::High);
        assert_eq!(Severity::of(SEVERITY_MEDIUM), Severity::Medium);
        assert_eq!(Severity::of(SEVERITY_MEDIUM - 1), Severity::Low);
    }

    #[test]
    fn an_unscored_open_row_is_counted_as_unscored_and_not_as_low() {
        let counts = count_rows(
            &[row("a", None, Disposition::Unread), row("b", Some(90), Disposition::Unread)],
            |_| false,
        );
        assert_eq!(counts.open, 2, "both are still asking to be read");
        assert_eq!(counts.unranked, 1, "`?` is a state of the tool, not a low score");
        assert_eq!(counts.high, 1);
    }

    #[test]
    fn a_released_snooze_is_open_work_again() {
        let rows = [row("a", Some(10), Disposition::Snoozed)];
        assert_eq!(count_rows(&rows, |_| false).snoozed, 1);
        let released = count_rows(&rows, |_| true);
        assert_eq!(released.released, 1);
        assert_eq!(released.open, 1, "a snooze whose condition happened is work");
    }

    #[test]
    fn an_empty_set_is_not_a_caught_up_reviewer() {
        assert!(!ReviewCounts::default().caught_up());
        assert_eq!(
            ReviewCounts::default().line(),
            "nothing to review yet — the index holds no episodes"
        );
    }

    #[test]
    fn the_caught_up_line_says_how_the_set_emptied() {
        let counts = ReviewCounts {
            total: 4,
            accepted: 2,
            dismissed: 1,
            ..ReviewCounts::default()
        };
        assert!(counts.caught_up());
        assert_eq!(
            counts.line(),
            "caught up — 4 episodes, 2 reviewed, 1 dismissed, 0 snoozed, 0 flagged"
        );
    }
}

//! The ordering: risk, weighted by staleness and blast radius, in an order that reproduces itself.
//!
//! ```text
//! queue-1 106 = risk 82 × staleness 1.20 (14d unread) × blast 1.08 (3 direct)
//! ```
//!
//! Three numbers, multiplied, divided by a power of ten. That is the whole ordering function, and every
//! one of the three is printed beside the result, which is what makes it audit rather than taste
//! ([`crate::risk::score`]'s rule for the score itself, applied one stage later —
//! ADR-001 §7.9: *if we cannot say why it is at 82, it should not be at 82*).
//!
//! # Why multiplication, and not a weighted sum
//!
//! Staleness and blast radius are not extra evidence about how bad a change is; they are answers to
//! *when* to read it. A three-line migration is a 50-point change on Monday and an urgent one after two
//! weeks of nobody having looked at it, and the thing that changed is not the diff. Multiplying keeps
//! `risk` the number it already is — so `queue 106` next to `risk 82` reads as "same risk, read it
//! earlier" rather than as a second opinion about the code — and it keeps each factor separately
//! editable: a factor is a band in hundredths, printed, and a reviewer who disagrees with `+5%/day` can
//! say which row it moved and by how much.
//!
//! # The arithmetic, exactly
//!
//! ```text
//! S = 100 + min(50, days_unread · 5)        hundredths, 1.00 .. 1.50
//! B = 100 + min(30, direct_dependents · 3)  hundredths, 1.00 .. 1.30 — 100 when not measured
//! product   = risk · S · B                  an exact integer product of the printed numbers
//! points    = round_half_up(product ÷ 10⁴)  the printed headline, 0 .. 195
//! ```
//!
//! `product` is what the ordering compares, and `points` is what the screen prints. They disagree by
//! design: rounding to a printed integer every few points creates ties, and a tie resolved by the
//! unrounded product is a tie resolved by something the reviewer can check rather than by row order.
//! The full tie-break chain is [`precede`] and it is total — two runs over the same index draw the same
//! screen, which is the acceptance criterion.
//!
//! # What a factor cannot see
//!
//! Blast radius needs the module graph (`e3.1`/`e3.2`), and this build has none. [`BlastInput::Unmeasured`]
//! is therefore the shipped state: `B` is `1.00`, the row prints `blast 1.00 (not measured · e3.2)`,
//! and the strip says the ordering is missing a factor. That is the same rule
//! [`crate::risk::score`] uses for a feature with no input — an unknown leaves the arithmetic and says
//! it has left, and never poses as a zero — and it is why the factor is a door rather than a guess: the
//! day `e3.2` hands over a count, [`QueueRow::blast`] carries it and no row's *shape* changes.
//!
//! A path-prefix heuristic was the alternative and is refused for the reason `insight`'s blast section
//! already gives: a reach estimate built from path prefixes is a guess wearing a number.

use std::cmp::Ordering;

use fathom_core::time::Timestamp;
use serde::Serialize;

use super::disposition::Disposition;

/// The ordering's name, printed wherever one of its numbers appears.
///
/// [`crate::risk::MODEL`] names the *score*; this names the *order*. A row therefore carries two model
/// strings and prints both when it is asked why it is where it is: `spine-1` is how the risk was scored,
/// `queue-1` is how the queue was ordered. Collapsing them into one name is how a ranking stops being
/// auditable one level up.
pub const QUEUE_MODEL: &str = "queue-1";

/// Staleness added per day of being unread, in hundredths of the risk.
///
/// Five percent a day, which says out loud what it is: an episode that has sat unread for a fortnight
/// counts as a fifth more urgent than one that landed an hour ago, and one that landed this morning
/// counts as exactly itself. Anything smaller disappears into the rounding of `points`; anything larger
/// turns the queue into an age list, and the age list is the timeline (`e1.6`), which already exists.
pub const STALENESS_STEP_H: i64 = 5;

/// The ceiling on the staleness factor, in hundredths above one.
///
/// Ten days, after which "old" has said everything it is going to say. An uncapped staleness buries
/// every new arrival under the backlog, which is the failure mode a queue that is never caught up turns
/// into — the reviewer stops believing the top row.
pub const STALENESS_CAP_H: i64 = 50;

/// Blast radius added per direct dependent, in hundredths of the risk.
pub const BLAST_STEP_H: i64 = 3;

/// The ceiling on the blast factor, in hundredths above one: ten direct dependents.
pub const BLAST_CAP_H: i64 = 30;

/// The words for a blast factor with no measurement behind it: the value, who would have measured it,
/// and who would have shown it. All three, because "unmeasured" on its own leaves the reviewer to guess
/// whether the factor is missing or the factor is one — and a `1.00` on a review queue reads as "safe".
pub const BLAST_UNMEASURED: &str = "not measured · e3.2 owns it · displayed by fathom-tui";

/// A score at or above this is `high`.
///
/// This buckets the score the queue is *handed*, which is the delta's `spine-1` number, and it is
/// deliberately **not** `risk.tau`. The two were the same number until `reco-6co` measured τ against real
/// history: `risk.tau` is now 40 because that is the `features-1` floor three real repositories support
/// (`features-1`'s ceiling on that corpus was 63, so 70 flagged nothing), and this constant still buckets
/// `spine-1`, a different model with its own range. "Flagged" is what `fathom why` says about the score it
/// prints; "high" is what the queue says about the score in the delta. A repository that wants one number
/// for both sets `[risk] tau` and the queue's policy (`QueuePolicy::high_floor`) to agree — what is not
/// allowed is a document that quotes one as the other.
pub const SEVERITY_HIGH: u8 = 70;

/// A score at or above this is `medium`, and below it `low`. Half of [`SEVERITY_HIGH`], and stated
/// rather than derived at the call site so a document can quote one number.
pub const SEVERITY_MEDIUM: u8 = 35;

/// One day, in seconds, and the unit staleness is measured in.
pub const DAY_SECONDS: i64 = 86_400;

/// How consequential a change is, in three words.
///
/// Severity is the *risk score* bucketed; priority is severity ordered in time. The two are different
/// questions and the queue prints both — "3 high, 6 medium, 22 low" is a statement about the code,
/// "riskiest first" is a statement about the reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Severity {
    /// At or above [`SEVERITY_HIGH`].
    High,
    /// At or above [`SEVERITY_MEDIUM`].
    Medium,
    /// Below [`SEVERITY_MEDIUM`].
    Low,
}

impl Severity {
    /// The band a score falls in.
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

    /// The printed word. `med` rather than `medium` because it goes in a strip beside two others.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Severity::High => "high",
            Severity::Medium => "med",
            Severity::Low => "low",
        }
    }
}

/// One multiplier, in hundredths, with the sentence that produced it.
///
/// The hundredths are kept as an integer rather than a float so the printed `1.20` is the number the
/// arithmetic used, not a rounding of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WeightFactor {
    /// The multiplier in hundredths: `120` is `1.20`.
    pub hundredths: i64,
    /// What it was measured from, in the reviewer's words: `14d unread`, `3 direct`, `not measured`.
    pub basis: String,
}

impl WeightFactor {
    /// A factor of exactly one, which is "this factor saw nothing".
    #[must_use]
    pub const fn neutral() -> Self {
        WeightFactor {
            hundredths: 100,
            basis: String::new(),
        }
    }

    /// The multiplier as a decimal string, always with two digits: `1.00`, `1.20`, `12.50`.
    #[must_use]
    pub fn decimal(&self) -> String {
        format!("{}.{:02}", self.hundredths / 100, self.hundredths % 100)
    }
}

/// What the ordering knows about what depends on this change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum BlastInput {
    /// A count of things that depend on what this episode touched, and who measured it.
    Direct {
        /// The count.
        count: u64,
        /// Whose number it is, printed beside it: `e3.2 module graph`.
        basis: String,
    },
    /// Nothing. The honest default in this build, and printed as such rather than as `1.00`.
    Unmeasured,
}

impl BlastInput {
    /// The factor this blast contributes.
    #[must_use]
    pub fn factor(&self) -> WeightFactor {
        match self {
            BlastInput::Direct { count, basis } => WeightFactor {
                hundredths: 100
                    + BLAST_CAP_H.min(
                        i64::try_from(*count)
                            .unwrap_or(i64::MAX)
                            .saturating_mul(BLAST_STEP_H),
                    ),
                basis: format!("{count} direct · {basis}"),
            },
            BlastInput::Unmeasured => WeightFactor {
                hundredths: 100,
                basis: BLAST_UNMEASURED.to_owned(),
            },
        }
    }

    /// Whether this blast says anything.
    #[must_use]
    pub const fn measured(&self) -> bool {
        matches!(self, BlastInput::Direct { .. })
    }
}

/// One episode, in the shape the ordering needs.
///
/// Everything here is a row the caller already read. The ordering adds no measurement of its own, which
/// is what lets the same function order a page of the index, a fixture and the artefact.
#[derive(Debug, Clone)]
pub struct QueueRow {
    /// The episode's identity.
    pub episode: String,
    /// Its risk score, or `None` when nothing has ranked it. `None` is never scored as zero.
    pub risk: Option<u8>,
    /// Which scoring model produced it (`spine-1` today), printed beside the number.
    pub risk_model: String,
    /// When the episode began, which is what staleness measures from.
    pub created: Timestamp,
    /// Churn, `n_add + n_del`, used only as a tie-break.
    pub size: u64,
    /// Direct dependents, if anything measured them.
    pub blast: BlastInput,
    /// What the reviewer has already done with it, which decides whether it is *open*.
    pub disposition: Disposition,
}

impl QueueRow {
    /// An input with the fewest facts a row can have: an identity, a date and a risk score.
    #[must_use]
    pub fn scored(episode: impl Into<String>, risk: u8, created: Timestamp) -> Self {
        QueueRow {
            episode: episode.into(),
            risk: Some(risk),
            risk_model: crate::risk::MODEL.to_owned(),
            created,
            size: 0,
            blast: BlastInput::Unmeasured,
            disposition: Disposition::Unread,
        }
    }

    /// An input nothing has ranked: it goes last and prints `?`, never `0`.
    #[must_use]
    pub fn unranked(episode: impl Into<String>, created: Timestamp) -> Self {
        QueueRow {
            episode: episode.into(),
            risk: None,
            risk_model: String::new(),
            created,
            size: 0,
            blast: BlastInput::Unmeasured,
            disposition: Disposition::Unread,
        }
    }

    /// Whether this row still wants reading.
    #[must_use]
    pub fn open(&self, released: bool) -> bool {
        self.disposition.open(released)
    }
}

/// One episode's place in the queue, with everything that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueuePlacement {
    /// The episode placed.
    pub episode: String,
    /// The exact product `risk · S · B`, in ten-thousandths of a risk point.
    ///
    /// This, not [`QueuePlacement::points`], is what the order compares. It is an exact integer made of the
    /// three integers printed in [`QueuePlacement::explain`], so the order is reproducible with a calculator.
    pub product: i64,
    /// `round_half_up(product ÷ 10⁴)` — the printed headline, on a scale whose ordinary ceiling is 100
    /// and whose ceiling with every factor at full tilt is 195.
    pub points: i64,
    /// The risk score it was built from, and which model scored it.
    pub risk: Option<u8>,
    /// See [`QueuePlacement::risk`].
    pub risk_model: String,
    /// The staleness multiplier.
    pub staleness: WeightFactor,
    /// The blast multiplier.
    pub blast: WeightFactor,
    /// Whether a real blast number went into that factor.
    pub blast_measured: bool,
    /// The severity band, or `None` on a row nothing ranked.
    pub severity: Option<Severity>,
    /// This row's position among the open rows, `1`-based. `None` on a row that is not open: a dismissed
    /// episode has no rank, and printing `rank 4` on it would be a lie about a queue it has left.
    pub rank: Option<usize>,
    /// How many open rows were ranked in the same run.
    pub ranked: Option<usize>,
    /// Whether this row is still asking to be read.
    pub open: bool,
    /// The disposition it carried into the ordering.
    pub disposition: Disposition,
}

impl QueuePlacement {
    /// The sentence that produced the number, exactly reproducible from the numbers it prints.
    #[must_use]
    pub fn explain(&self) -> String {
        let Some(risk) = self.risk else {
            return format!(
                "{QUEUE_MODEL} · unranked — nothing scored this episode, so it is placed after every \
                 ranked one and prints `?` (owner: whatever would have scored it)"
            );
        };
        format!(
            "{QUEUE_MODEL} {points} = risk {risk} ({model}) × staleness {stale} ({stale_basis}) × \
             blast {blast} ({blast_basis})",
            points = self.points,
            risk = risk,
            model = if self.risk_model.is_empty() {
                "unscored model"
            } else {
                &self.risk_model
            },
            stale = self.staleness.decimal(),
            stale_basis = self.staleness.basis,
            blast = self.blast.decimal(),
            blast_basis = self.blast.basis,
        )
    }

    /// The short form a row prints: `queue 106`.
    #[must_use]
    pub fn headline(&self) -> String {
        if self.risk.is_none() {
            return "queue ?".to_owned();
        }
        format!("queue {}", self.points)
    }

    /// `rank 4 of 7`, or what is true instead.
    #[must_use]
    pub fn place(&self) -> String {
        match (self.rank, self.ranked) {
            (Some(rank), Some(ranked)) => format!("rank {rank} of {ranked} open"),
            _ if self.open => "unranked — after every ranked row".to_owned(),
            _ => format!("not open — {}", self.disposition.label()),
        }
    }
}

/// The knobs of the ordering, with the one honest default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueuePolicy {
    /// The score at or above which a row is `high`.
    ///
    /// A knob because `risk.tau` is already a knob (`fathom.toml [risk.weights] tau`) and a queue that
    /// called a flagged episode `low` would contradict the verb that flagged it.
    pub high_floor: u8,
    /// The score at or above which a row is `medium`.
    pub medium_floor: u8,
}

impl Default for QueuePolicy {
    fn default() -> Self {
        QueuePolicy {
            high_floor: SEVERITY_HIGH,
            medium_floor: SEVERITY_MEDIUM,
        }
    }
}

impl QueuePolicy {
    /// The band a score falls in under this policy.
    #[must_use]
    pub const fn severity(self, score: u8) -> Severity {
        if score >= self.high_floor {
            Severity::High
        } else if score >= self.medium_floor {
            Severity::Medium
        } else {
            Severity::Low
        }
    }
}

/// What the queue holds, counted the way the strip prints it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct QueueCounts {
    /// Rows in the run at all.
    pub total: usize,
    /// Rows still asking to be read.
    pub open: usize,
    /// Open rows with no score at all — printed, because a queue of `?`s is not a caught-up queue.
    pub unranked: usize,
    /// Open rows by severity band.
    pub high: usize,
    /// See [`QueueCounts::high`].
    pub medium: usize,
    /// See [`QueueCounts::high`].
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

impl QueueCounts {
    /// The one-line count above the rows, and the one the ambient pane prints.
    ///
    /// `7 of 12 open · 2 high 3 med 2 low · 4 dismissed · 1 snoozed`. Zero bands are left out rather
    /// than printed as zeroes, which is the difference between a count and a form, and the denominator
    /// is there so "7 open" cannot be read as "7 of everything the repository has ever had".
    ///
    /// It deliberately does not say "queue". The frame's status line already prints `queue N open`, and
    /// that N counts the rows nobody has opened yet; this one counts everything still asking to be read —
    /// unread, opened, flagged, and a snooze whose condition has come due — among the rows the pane was
    /// handed. Two honest answers to two different questions, and the second one must not borrow the first
    /// one's label: a reviewer who sees `queue 4 open` and `queue 7 open` on one screen stops believing
    /// both. The pane's own title says what this one belongs to.
    #[must_use]
    pub fn line(&self) -> String {
        if self.total == 0 {
            return "nothing to triage yet — the index holds no episodes".to_owned();
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

    /// The count itself — `6 of 10 open · 2 high 3 med 1 low · 6 unranked` — without what left the queue.
    ///
    /// Split out because [`QueueRanking::count_line`] has one more clause to fit in front of the tally,
    /// and the clause about the factor that had no number matters more than the bookkeeping does.
    #[must_use]
    pub fn open_words(&self) -> String {
        let mut out = format!("{} of {} open", self.open, self.total);
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
            out.push_str(&format!(" · {} unranked", self.unranked));
        }
        out
    }

    /// What left the queue, and as what. Empty when nothing has been dispositioned, so the caller can
    /// tell "nothing to report" from "a queue of nothing" without a second test.
    ///
    /// Split out because the count line has a width to respect and this is its least important clause:
    /// the ordering's missing factor is printed ahead of it, and a reviewer who has dismissed three things
    /// can live with the tally falling off the right edge before the gap does.
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

    /// The caught-up sentence, which says what *how* — a queue emptied by reading and one emptied by
    /// dismissing are different afternoons and the reviewer should be able to tell them apart.
    #[must_use]
    pub fn caught_up_line(&self) -> String {
        format!(
            "caught up — {} episodes, {} reviewed, {} dismissed, {} snoozed, {} flagged",
            self.total, self.accepted, self.dismissed, self.snoozed, self.flagged
        )
    }
}

/// A whole queue: every row placed, and the counts drawn from the same pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueRanking {
    /// Every input, in queue order: open rows first (riskiest first), then the dispositioned ones.
    pub order: Vec<QueuePlacement>,
    /// The counts, computed while ordering so a strip and a list cannot disagree about a number.
    pub counts: QueueCounts,
    /// Whether any row's blast came from a measurement.
    pub blast_measured: bool,
}

impl QueueRanking {
    /// The queue's total order, as one comparator so no two callers can drift apart.
    ///
    /// Open rows first; then, within each group, the exact product descending. The tie-breaks after that
    /// are ordered by how much each one tells the reviewer something they would have agreed with, and the
    /// last one is an id so the order is a total function of the data:
    ///
    /// 1. open before dispositioned — a row a human has decided on never outranks one that is waiting;
    /// 2. `product` descending — the whole ordering;
    /// 3. risk descending — so a tie between a 90×1.00 and a 60×1.50 reads as the riskier change first;
    /// 4. staleness descending, then blast descending — the tie broken by the factor that moved;
    /// 5. size descending, then recency descending — the `spine-1` tie-breaks, kept so a queue tie and a
    ///    rail tie are the same tie;
    /// 6. episode id ascending — the last resort, and the reason two runs cannot differ.
    #[must_use]
    pub fn precede(left: &QueuePlacement, right: &QueuePlacement) -> Ordering {
        right
            .open
            .cmp(&left.open)
            .then_with(|| right.product.cmp(&left.product))
            .then_with(|| right.risk.cmp(&left.risk))
            .then_with(|| right.staleness.hundredths.cmp(&left.staleness.hundredths))
            .then_with(|| right.blast.hundredths.cmp(&left.blast.hundredths))
            .then_with(|| right.disposition.cmp(&left.disposition))
            .then_with(|| left.episode.cmp(&right.episode))
    }

    /// The strip: the ordering named, what it counted, and what it could not see.
    ///
    /// ADR-006 §5.1's binding note is that a strip which does not say *how* it ordered asks for trust.
    /// This one also says when one of its three factors was absent, because a strip that reads `risk ×
    /// staleness × blast` over a build that cannot measure blast is describing a machine it is not.
    #[must_use]
    pub fn strip(&self) -> String {
        // Compact on purpose. The strip lives in a pane that is 40 columns wide on an 80-column terminal,
        // and the one thing it may never lose is the *name* of the ordering — ADR-006 §5.1's note is that a
        // strip which does not say how it ordered asks for trust. Everything else about the ordering (the
        // factor that was absent, the arithmetic, whose number it used) has a wider home: the count line
        // above the rows, and `w`'s sentence.
        let ranked = self.counts.open - self.counts.unranked;
        let mut out = format!(
            "order: {QUEUE_MODEL} = risk × staleness × blast · {ranked} of {} open",
            self.counts.open
        );
        if self.counts.unranked > 0 {
            out.push_str(&format!(" · {} unranked", self.counts.unranked));
        }
        // No clause here about the factor that had no number: at forty columns the sentence would be cut
        // before it said anything. [`Self::count_line`] carries it above the rows, and
        // [`QueuePlacement::explain`] carries the whole attribution in the card pane.
        out
    }

    /// The line above the rows: what is open, in how many buckets, and which factor had no number.
    ///
    /// The strip is forty columns wide on an eighty-column terminal and cannot carry both sentences. This
    /// one gets a pane's worth, and ADR-006 §5.1's instruction to say the third factor is idle belongs
    /// here rather than truncated away at the point where it stops making sense.
    #[must_use]
    pub fn count_line(&self) -> String {
        // An empty index and a caught-up reviewer already have their own sentences, and neither wants a
        // clause about a factor that had no number: with nothing open, the ordering had nothing to order.
        if self.counts.total == 0 || self.counts.caught_up() {
            return self.counts.line();
        }
        let mut out = self.counts.open_words();
        if !self.blast_measured {
            // Ahead of the tally on purpose: "the third factor was asleep" is the sentence that changes
            // what a reviewer concludes about the order, and the tally is bookkeeping.
            out.push_str(" · blast unmeasured (e3.2) · holding this list at 1.00");
        }
        let tally = self.counts.tally();
        if !tally.is_empty() {
            out.push_str(" · ");
            out.push_str(&tally);
        }
        out
    }
}

/// The staleness factor for one episode's age, in whole days.
#[must_use]
pub fn staleness(now: Timestamp, created: Timestamp) -> WeightFactor {
    let seconds = (now.as_unix_seconds() - created.as_unix_seconds()).max(0);
    let days = seconds / DAY_SECONDS;
    let above = STALENESS_CAP_H.min(days.saturating_mul(STALENESS_STEP_H));
    WeightFactor {
        hundredths: 100 + above,
        basis: age_words(seconds),
    }
}

/// An age in the words the row prints: `4h`, `14d`, `9w`.
///
/// The coarse unit is deliberate: the factor's own step is a day, so minutes are decoration.
#[must_use]
pub fn age_words(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 3_600 {
        return format!("{}m unread", seconds / 60);
    }
    if seconds < DAY_SECONDS {
        return format!("{}h unread", seconds / 3_600);
    }
    let days = seconds / DAY_SECONDS;
    if days < 14 {
        return format!("{days}d unread");
    }
    format!("{}w unread", days / 7)
}

/// Order every input and count what was ordered.
///
/// `released` says, per episode, whether a snooze's condition has since been met — the caller knows that
/// (it holds the index epoch and the session), and this function must not guess. A released snooze is an
/// open row again.
///
/// Ranks are assigned only to open, ranked rows, so `rank 4 of 7` has no holes in it, and only after the
/// whole set is present, because a rank is a statement about peers.
#[must_use]
pub fn rank_queue(
    inputs: &[QueueRow],
    now: Timestamp,
    policy: &QueuePolicy,
    released: impl Fn(&str) -> bool,
) -> QueueRanking {
    let mut order: Vec<QueuePlacement> = inputs
        .iter()
        .map(|input| {
            let staleness = staleness(now, input.created);
            let blast = input.blast.factor();
            let product = i64::from(input.risk.unwrap_or(0))
                .saturating_mul(staleness.hundredths)
                .saturating_mul(blast.hundredths);
            let points = if input.risk.is_none() {
                0
            } else {
                product.saturating_add(5_000) / 10_000
            };
            QueuePlacement {
                open: input.disposition.open(released(&input.episode)),
                episode: input.episode.clone(),
                product,
                points,
                risk: input.risk,
                risk_model: input.risk_model.clone(),
                staleness,
                blast,
                blast_measured: input.blast.measured(),
                severity: input.risk.map(|score| policy.severity(score)),
                rank: None,
                ranked: None,
                disposition: input.disposition,
            }
        })
        .collect();
    order.sort_by(QueueRanking::precede);

    let mut counts = QueueCounts {
        total: order.len(),
        ..QueueCounts::default()
    };
    for placement in &mut order {
        match placement.disposition {
            Disposition::Unread | Disposition::Read => {}
            Disposition::Accepted => counts.accepted += 1,
            Disposition::Flagged => counts.flagged += 1,
            Disposition::Dismissed => counts.dismissed += 1,
            Disposition::Snoozed => {
                if placement.open {
                    counts.released += 1;
                } else {
                    counts.snoozed += 1;
                }
            }
        }
        if !placement.open {
            continue;
        }
        counts.open += 1;
        match placement.severity {
            Some(Severity::High) => counts.high += 1,
            Some(Severity::Medium) => counts.medium += 1,
            Some(Severity::Low) => counts.low += 1,
            None => counts.unranked += 1,
        }
    }
    let ranked = counts.open - counts.unranked;
    let mut place = 0_usize;
    for placement in &mut order {
        if placement.open && placement.risk.is_some() {
            place += 1;
            placement.rank = Some(place);
            placement.ranked = Some(ranked);
        }
    }
    QueueRanking {
        order,
        counts,
        blast_measured: inputs.iter().any(|input| input.blast.measured()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_micros(seconds * 1_000_000)
    }

    /// An input `age_days` before the clock of these tests, unmeasured blast, nothing decided.
    fn row(id: &str, risk: u8, age_days: i64) -> QueueRow {
        QueueRow {
            risk: Some(risk),
            ..QueueRow::scored(id, risk, at(NOW - age_days * DAY_SECONDS))
        }
    }

    const BASE: i64 = 1_700_000_000;
    const NOW: i64 = BASE + 2 * DAY_SECONDS;

    fn ranked(ids_and_risk: &[(String, u8)], ages: &[i64]) -> Vec<QueuePlacement> {
        let inputs: Vec<QueueRow> = ids_and_risk
            .iter()
            .zip(ages)
            .map(|((id, risk), age)| row(id, *risk, *age))
            .collect();
        rank_queue(&inputs, at(NOW), &QueuePolicy::default(), |_| false).order
    }

    #[test]
    fn staleness_is_five_percent_a_day_and_stops_at_fifty() {
        let fresh = staleness(at(NOW), at(NOW));
        assert_eq!(fresh.hundredths, 100, "nothing unread is no multiplier");
        assert_eq!(staleness(at(NOW), at(NOW - DAY_SECONDS)).hundredths, 105);
        assert_eq!(
            staleness(at(NOW), at(NOW - 10 * DAY_SECONDS)).hundredths,
            150,
            "ten days is the cap, not a rounding artefact"
        );
        assert_eq!(
            staleness(at(NOW), at(NOW - 400 * DAY_SECONDS)).hundredths,
            150,
            "two years in the queue does not make a finding a hundred times worse"
        );
    }

    #[test]
    fn age_is_printed_in_the_unit_the_factor_actually_steps_in() {
        assert_eq!(age_words(2 * 60), "2m unread");
        assert_eq!(age_words(4 * 3_600), "4h unread");
        assert_eq!(age_words(2 * DAY_SECONDS), "2d unread");
        assert_eq!(age_words(21 * DAY_SECONDS), "3w unread");
    }

    #[test]
    fn blast_is_one_point_zero_until_somebody_measures_it() {
        let direct = |count: u64| BlastInput::Direct {
            count,
            basis: "fixture graph".to_owned(),
        };
        assert_eq!(BlastInput::Unmeasured.factor().hundredths, 100);
        assert_eq!(direct(0).factor().hundredths, 100);
        assert_eq!(direct(4).factor().hundredths, 112);
        assert_eq!(direct(10).factor().hundredths, 130);
        assert_eq!(
            direct(400).factor().hundredths,
            130,
            "the factor is a prior with a ceiling, not an arms race"
        );
        assert!(
            BlastInput::Unmeasured.factor().basis.contains("e3.2"),
            "an idle factor says who owns it: {}",
            BlastInput::Unmeasured.factor().basis
        );
        assert!(
            direct(3).factor().basis.contains("3 direct"),
            "the count travels with its owner"
        );
    }

    #[test]
    fn the_anchors_come_out_where_the_code_changed_them() {
        // The ADR's worked example, kept as a test because the ADR is prose and this is a check.
        let placements = ranked(&[("e".to_owned(), 82)], &[14]);
        let anchor = &placements[0];
        assert_eq!(anchor.staleness.hundredths, 150);
        assert_eq!(anchor.blast.hundredths, 100);
        assert_eq!(anchor.product, 82 * 150 * 100);
        assert_eq!(anchor.points, 123);
        assert!(anchor.explain().contains("queue-1"));
        assert!(
            anchor.explain().contains("blast 1.00"),
            "{}",
            anchor.explain()
        );
    }

    #[test]
    fn the_product_is_the_order_and_the_printed_points_are_rounded_after_it() {
        // Both rows print `queue 88`. They are not tied: the exact products differ, and the order has to
        // follow the product, because a reviewer who can see two identical numbers will ask which one is
        // first and the answer must be arithmetic rather than a coin toss.
        let inputs = vec![
            row("high-but-fresh", 88, 0),     // 88 × 1.00 → 88.0
            row("middling-but-stale", 84, 1), // 84 × 1.05 → 88.2
        ];
        let ranking = rank_queue(&inputs, at(NOW), &QueuePolicy::default(), |_| false);
        assert_eq!(
            ranking.order[0].points, ranking.order[1].points,
            "the fixture depends on the printed numbers colliding: {:?} vs {:?}",
            ranking.order[0].points, ranking.order[1].points
        );
        assert!(
            ranking.order[0].episode == "middling-but-stale",
            "the order must be the exact product, not the rounded headline: {:?}",
            ranking
                .order
                .iter()
                .map(|p| (&p.episode, p.points, p.product))
                .collect::<Vec<_>>()
        );
        assert!(ranking.order[0].product > ranking.order[1].product);
    }

    #[test]
    fn staleness_overtakes_a_bigger_number_and_a_flag_stays_in_the_list() {
        // 60 × 1.50 (ten days unread) is 90, which is more than a fresh 80: exactly the change ADR-006
        // §5.1 asked for, and the reason staleness is a multiplier rather than a footnote.
        let inputs = vec![
            row("fresh-high", 80, 0),
            row("stale-medium", 60, 10),
            QueueRow {
                disposition: Disposition::Flagged,
                ..row("flagged-low", 20, 0)
            },
            QueueRow {
                disposition: Disposition::Accepted,
                ..row("already-reviewed", 99, 30)
            },
        ];
        let ranking = rank_queue(&inputs, at(NOW), &QueuePolicy::default(), |_| false);
        let open: Vec<&str> = ranking
            .order
            .iter()
            .filter(|p| p.open)
            .map(|p| p.episode.as_str())
            .collect();
        assert_eq!(
            open,
            vec!["stale-medium", "fresh-high", "flagged-low"],
            "a flag keeps its episode in the queue (ADR-004 §5) and an accept takes it out: {:?}",
            ranking
                .order
                .iter()
                .map(|p| (&p.episode, p.points, p.open))
                .collect::<Vec<_>>()
        );
        assert_eq!(ranking.counts.flagged, 1);
        assert_eq!(ranking.counts.accepted, 1);
        assert_eq!(
            ranking.counts.open, 3,
            "a flagged row is still asking to be read"
        );
    }

    #[test]
    fn the_tie_break_ends_at_the_episode_id_so_a_run_is_reproducible() {
        let a = row("aaaa", 40, 1);
        let b = row("bbbb", 40, 1);
        let forwards = rank_queue(
            &[a.clone(), b.clone()],
            at(NOW),
            &QueuePolicy::default(),
            |_| false,
        );
        let backwards = rank_queue(&[b, a], at(NOW), &QueuePolicy::default(), |_| false);
        let first = |ranking: &QueueRanking| {
            ranking
                .order
                .first()
                .map(|placement| placement.episode.clone())
        };
        assert_eq!(first(&forwards), first(&backwards));
        assert_eq!(first(&forwards).as_deref(), Some("aaaa"));
    }

    #[test]
    fn an_unscored_row_waits_behind_every_scored_one_and_says_so() {
        let inputs = vec![
            QueueRow::unranked("nobody-scored-me", at(NOW)),
            row("scored-low", 5, 0),
        ];
        let ranking = rank_queue(&inputs, at(NOW), &QueuePolicy::default(), |_| false);
        assert_eq!(ranking.order[0].episode, "scored-low");
        let unranked = &ranking.order[1];
        assert_eq!(unranked.rank, None, "an unranked row has no rank to print");
        assert_eq!(unranked.points, 0, "unscored is not zero");
        assert_eq!(ranking.counts.unranked, 1);
        assert!(unranked.explain().contains("nothing scored this episode"));
        assert_eq!(unranked.headline(), "queue ?");
    }

    #[test]
    fn closed_rows_leave_the_queue_and_a_released_snooze_comes_back() {
        let inputs = vec![
            row("still-open", 30, 0),
            QueueRow {
                disposition: Disposition::Accepted,
                ..row("signed-off", 90, 0)
            },
            QueueRow {
                disposition: Disposition::Dismissed,
                ..row("skipped", 90, 0)
            },
            QueueRow {
                disposition: Disposition::Snoozed,
                ..row("sleeping", 90, 0)
            },
            QueueRow {
                disposition: Disposition::Read,
                ..row("just-was-noticed", 20, 0)
            },
        ];
        let quiet = rank_queue(&inputs, at(NOW), &QueuePolicy::default(), |_| false);
        let open: Vec<&str> = quiet
            .order
            .iter()
            .filter(|p| p.open)
            .map(|p| p.episode.as_str())
            .collect();
        assert_eq!(
            open,
            vec!["still-open", "just-was-noticed"],
            "accepted and dismissed leave; a snooze stays out until its condition happens"
        );
        assert_eq!(quiet.counts.snoozed, 1);
        assert_eq!(quiet.counts.accepted, 1);
        assert_eq!(quiet.counts.dismissed, 1);
        assert_eq!(quiet.counts.flagged, 0);

        let woken = rank_queue(&inputs, at(NOW), &QueuePolicy::default(), |id| {
            id == "sleeping"
        });
        assert!(woken.counts.released >= 1);
        assert!(
            woken
                .order
                .iter()
                .filter(|p| p.open)
                .any(|p| p.episode == "sleeping")
        );
    }

    #[test]
    fn the_strip_names_the_ranking_and_the_counts_say_what_is_open() {
        // The strip's whole duty is to name the ordering, by name and version, in the width a pane has:
        // ADR-006 §5.1's note is that a strip which does not say *how* it ordered asks for trust. What it
        // was missing is said by the caller beside the counts, because a 40-column pane cannot carry both
        // sentences and the name is the one that may not be truncated away.
        let ranking = rank_queue(
            &[row("a", 80, 1), row("b", 30, 0)],
            at(NOW),
            &QueuePolicy::default(),
            |_| false,
        );
        let strip = ranking.strip();
        assert!(strip.contains("queue-1"), "{strip}");
        assert!(strip.contains("risk × staleness × blast"), "{strip}");
        assert!(strip.contains("2 of 2 open"), "{strip}");
        assert!(
            strip.chars().count() <= 56,
            "a strip truncated before its own name asks for trust: {strip}"
        );
        assert!(
            !ranking.blast_measured,
            "and the absence is a fact of the ranking"
        );
        assert!(
            ranking.count_line().contains("blast unmeasured (e3.2)"),
            "the line above the rows says what the strip had no room for: {}",
            ranking.count_line()
        );
        assert_eq!(
            ranking
                .order
                .iter()
                .filter(|placement| placement.explain().contains("queue-1"))
                .count(),
            2,
            "every row's explain sentence names the ordering that placed it"
        );
    }

    #[test]
    fn an_absent_factor_is_attributed_to_whoever_would_have_measured_it() {
        // The other half of the same promise, where there is room for a sentence: `w`, and anything that
        // prints `explain`, says which factor had no number and whose job it would have been.
        let ranking = rank_queue(&[row("a", 80, 1)], at(NOW), &QueuePolicy::default(), |_| {
            false
        });
        let explain = ranking.order[0].explain();
        assert!(explain.contains("blast 1.00"), "{explain}");
        assert!(
            explain.contains("e3.2") && explain.contains("fathom-tui"),
            "who owns the number, and who would have shown it: {explain}"
        );
        assert!(
            !explain.contains("attempted to read"),
            "an explain sentence is reproducible arithmetic, not an apology: {explain}"
        );
    }

    #[test]
    fn an_unmeasured_row_is_counted_as_unranked_rather_than_scored_zero() {
        let ranking = rank_queue(
            &[
                QueueRow::unranked("nothing-scored-me", at(NOW)),
                row("scored", 50, 0),
            ],
            at(NOW),
            &QueuePolicy::default(),
            |_| false,
        );
        assert_eq!(ranking.counts.open, 2);
        assert_eq!(ranking.counts.unranked, 1);
        assert!(
            ranking.strip().contains("1 unranked"),
            "the strip says a row waits on a score rather than implying it scored nil: {}",
            ranking.strip()
        );
    }

    #[test]
    fn the_counts_say_what_is_open_and_the_catch_up_line_says_so_aloud() {
        let counts = QueueCounts {
            total: 4,
            open: 2,
            high: 1,
            medium: 1,
            ..QueueCounts::default()
        };
        let line = counts.line();
        assert!(line.contains("2 of 4 open"), "{line}");
        assert!(line.contains("1 high"), "{line}");
        assert!(line.contains("1 med"), "{line}");
        assert!(
            !line.contains("queue "),
            "the frame's status line owns the phrase `queue N open`, and this is a different count: {line}"
        );

        let caught = QueueCounts {
            total: 6,
            open: 0,
            accepted: 3,
            dismissed: 2,
            snoozed: 1,
            ..QueueCounts::default()
        };
        let line = caught.caught_up_line();
        assert!(line.contains("caught up"), "{line}");
        assert!(line.contains("3 reviewed"), "{line}");
        assert!(line.contains("2 dismissed"), "{line}");
        assert!(line.contains("1 snoozed"), "{line}");
        assert!(
            caught.caught_up(),
            "an emptied queue says so in one branch only"
        );

        let empty = QueueCounts::default();
        assert!(
            !empty.caught_up(),
            "an empty index is not a triumph, so it does not get the triumph line"
        );
        assert!(
            empty.line().contains("nothing to triage"),
            "{}",
            empty.line()
        );
    }

    #[test]
    fn severity_bands_follow_the_knobs_because_the_seam_only_prints_one_number() {
        assert_eq!(QueuePolicy::default().severity(70), Severity::High);
        assert_eq!(QueuePolicy::default().severity(69), Severity::Medium);
        assert_eq!(QueuePolicy::default().severity(35), Severity::Medium);
        assert_eq!(QueuePolicy::default().severity(34), Severity::Low);
        assert_eq!(
            QueuePolicy {
                high_floor: 90,
                ..QueuePolicy::default()
            }
            .severity(80),
            Severity::Medium,
            "a repository that raised its high floor must get a queue that agrees with it"
        );
        assert_eq!(Severity::High.label(), "high");
    }
}

//! "Since my last review" — the one number a reviewer who has been away two days is asking for
//! (`e3.9`, `reco-al1`).
//!
//! The ticket's sentence is *"'since my last review' deltas so catching up after two days is one
//! number"*, and the acceptance criterion is the hard half of it: the delta has to stay **correct across
//! a rebase**. A delta computed from commit positions ("the last 40 revisions") is wrong the moment the
//! history is rewritten, and a rebased branch is exactly when a reviewer has been away. So the anchor is
//! resolved by a ladder of identity rungs, the same shape `e4.6`'s relink ladder uses, and the rung that
//! answered is printed with the answer rather than assumed.
//!
//! # The ladder
//!
//! | rung | what matched | what it means |
//! |---|---|---|
//! | `revision` | the revision the review ended at still has a scorecard row | exact: two stored rows, compared |
//! | `episode` | that revision is gone, but the *episode* it belonged to is still live and still names a scored revision | the history moved, the reviewed work did not |
//! | `instant` | both are gone; the review's own instant falls before some scored revision's index time | a time ordering, and it says so: a rebase that re-indexed already-reviewed work can be counted as new here |
//! | `window` | nothing in this index can be tied to the review at all | the whole window is the delta, and the report calls it that |
//!
//! Every rung is a comparison of **content and identity**, never of position: the reviewed episode's
//! `fingerprint` is `e1.3`'s content digest and the store already re-attaches a review's `review_state`
//! row across an amend, a reword, a squash or a split (`relink_review_state`, migration 3), which is what
//! makes the first two rungs survive a rebase without a fresh decision from the reviewer.
//!
//! # What is deliberately not here
//!
//! **No clock, no store, no git.** [`Review`] is a value the caller fills from the index
//! (`crates/fathom-cli/src/architecture.rs`), and [`resolve`] is a pure function of that value and the
//! stored rows. That is what lets the rebase case be a test with four hand-written rows instead of a
//! fixture repository with a rewritten branch, and it is the same IO-free split every other analysis in
//! this crate keeps (ADR-002 §4.5).
//!
//! **No new decision recorded.** This module reads what a review said; it never writes one. Signing off is
//! `fathom review` (**e4.6**) and the triage acts (**e4.5**) — a second door that could mark work reviewed
//! would be a second answer to "did a human look at this".

use serde::{Deserialize, Serialize};

use crate::risk::score::Points;

use super::score::Scorecard;
use super::trend::Row;

/// How the review's endpoint was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnchorKind {
    /// The revision the review ended at is itself scored. The exact rung.
    Revision,
    /// That revision is gone, but the reviewed episode is still live and its own scored revision was used.
    Episode,
    /// Nothing content-addressable survived; the review's instant ordered the rows instead.
    Instant,
    /// Nothing tied to the review is in this index; the delta spans the whole window.
    Window,
    /// There is no review to anchor on at all.
    None,
}

impl AnchorKind {
    /// The printed spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            AnchorKind::Revision => "revision",
            AnchorKind::Episode => "episode",
            AnchorKind::Instant => "instant",
            AnchorKind::Window => "window",
            AnchorKind::None => "none",
        }
    }

    /// The price of this rung, said out loud. A rung that is a weaker claim is not a wrong claim, and the
    /// reader is owed the difference.
    #[must_use]
    pub const fn price(self) -> &'static str {
        match self {
            AnchorKind::Revision => {
                "exact: the revision your review ended at is the revision this delta starts from"
            }
            AnchorKind::Episode => {
                "the reviewed episode is still live under its own identity, so the endpoint is its own \
                 revision even though the one you signed off at is no longer in the index"
            }
            AnchorKind::Instant => {
                "anchored on the indexed instant, not on content: a rebase that re-indexed work you had \
                 already reviewed can count that work as new here"
            }
            AnchorKind::Window => {
                "nothing in this index could be tied to your last review, so the delta below spans the \
                 whole window and is not what one reviewer missed"
            }
            AnchorKind::None => {
                "nobody has reviewed anything in this index, so there is no 'since' — the window score is \
                 printed instead"
            }
        }
    }
}

/// What a review left behind, as the caller resolved it from the index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    /// The episode the reviewer last acted on, as it is identified *now*.
    pub episode: String,
    /// `e1.3`'s content digest of that episode — the thing a rebase leaves alone.
    pub fingerprint: String,
    /// The review's verdict in the store's own vocabulary: `read`, `accepted`, `flagged`, `dismissed`,
    /// `snoozed`.
    pub state: String,
    /// Who acted, and — when a human acted through an agent — on whose behalf.
    pub actor: String,
    /// The revision the episode ended at, when the index still has that commit. `None` after a rewrite
    /// that dropped it, which is the case rung 2 exists for.
    pub revision: Option<String>,
    /// The instant the reviewed work ended, in microseconds.
    pub instant: i64,
    /// How the store re-attached the review to a live episode, when it had to: `fingerprint`, `shape` or
    /// `member-set` (`e4.6`'s ladder). Empty when the row never moved.
    pub relinked_via: String,
}

impl Review {
    /// The caller found no review at all.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            episode: String::new(),
            fingerprint: String::new(),
            state: String::new(),
            actor: String::new(),
            revision: None,
            instant: 0,
            relinked_via: String::new(),
        }
    }

    /// Whether there is a review to anchor on.
    #[must_use]
    pub fn exists(&self) -> bool {
        !self.episode.is_empty()
    }

    /// The line that names the review: `ep-2f9c1a (accepted by dan, fingerprint 7d3a…)`.
    #[must_use]
    pub fn sentence(&self) -> String {
        if !self.exists() {
            return "no review in this index".to_owned();
        }
        let mut out = format!(
            "{} ({} by {}",
            self.episode,
            if self.state.is_empty() {
                "seen"
            } else {
                &self.state
            },
            if self.actor.is_empty() {
                "nobody"
            } else {
                &self.actor
            }
        );
        if let Some(revision) = &self.revision {
            out.push_str(&format!(", at {}", short(revision)));
        } else {
            out.push_str(", at a revision this index no longer holds");
        }
        if !self.relinked_via.is_empty() {
            out.push_str(&format!(", re-attached by {}", self.relinked_via));
        }
        out.push(')');
        out
    }
}

/// The anchor a delta starts from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    /// Which rung answered.
    pub kind: AnchorKind,
    /// The revision the delta starts at, when a row was found for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// The review it anchored on, if there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<Review>,
    /// Why this rung, in one sentence — always printed, never only in the JSON.
    pub why: String,
}

impl Anchor {
    /// The rung's own sentence: `anchored on revision — …`.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!("anchored on {} — {}", self.kind.as_str(), self.why)
    }
}

/// Resolve the anchor for a review against the stored rows, newest first.
#[must_use]
pub fn resolve(review: Option<&Review>, rows: &[Row]) -> Anchor {
    let Some(review) = review.filter(|review| review.exists()) else {
        return Anchor {
            kind: AnchorKind::None,
            revision: rows.last().map(|row| row.revision.clone()),
            review: None,
            why: "nobody has reviewed anything this index holds".to_owned(),
        };
    };
    // (1) the exact rung: the reviewed revision is itself scored.
    if let Some(wanted) = review.revision.as_deref()
        && let Some(row) = rows.iter().find(|row| row.revision == wanted)
    {
        return Anchor {
            kind: AnchorKind::Revision,
            revision: Some(row.revision.clone()),
            review: Some(review.clone()),
            why: format!(
                "the review's own revision is scored: {} ({})",
                short(&row.revision),
                review.sentence()
            ),
        };
    }
    // (2) the episode survived even though its revision did not: use the row scored *as* that episode.
    if let Some(row) = rows
        .iter()
        .find(|row| row.scorecard.episode.as_deref() == Some(review.episode.as_str()))
    {
        return Anchor {
            kind: AnchorKind::Episode,
            revision: Some(row.revision.clone()),
            review: Some(review.clone()),
            why: format!(
                "the reviewed episode is still live and its own scored revision is {}; its original \
                 revision is no longer in the index ({})",
                short(&row.revision),
                review.sentence()
            ),
        };
    }
    // (3) a time ordering. `rows` is newest-first, so the *last* row at or after the review's instant is
    // the oldest one that can still be called "since". Comparing index times rather than commit times is
    // the rung's stated weakness and [`AnchorKind::price`] says what it costs.
    let older = rows
        .iter()
        .rev()
        .find(|row| row.built_utc >= review.instant)
        .or_else(|| rows.last());
    match older {
        Some(row) if row.built_utc >= review.instant => Anchor {
            kind: AnchorKind::Instant,
            revision: Some(row.revision.clone()),
            review: Some(review.clone()),
            why: format!(
                "neither the reviewed revision nor its episode is in the index; the delta starts at the \
                 oldest scored revision indexed after it, {}",
                short(&row.revision)
            ),
        },
        // (4) nothing left: the whole window is the delta.
        _ => Anchor {
            kind: AnchorKind::Window,
            revision: rows.last().map(|row| row.revision.clone()),
            review: Some(review.clone()),
            why: format!(
                "no scored revision in this index can be tied to {} — the delta spans the {} scored \
                 revision(s) held here",
                review.sentence(),
                rows.len()
            ),
        },
    }
}

/// Which way a number moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Nothing, within the flat band.
    Flat,
    /// Up, and the thing being measured got better.
    Improved,
    /// Down, and it got worse.
    Declined,
    /// It cannot be said: one side was not measured.
    Unknown,
}

impl Direction {
    /// The printed spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Direction::Flat => "flat",
            Direction::Improved => "improved",
            Direction::Declined => "declined",
            Direction::Unknown => "unknown",
        }
    }

    /// The arrow a card prints, with a word beside it so the arrow is never the only carrier
    /// (ADR-006 §6.6: no colour- or glyph-only meaning).
    #[must_use]
    pub const fn arrow(self) -> &'static str {
        match self {
            Direction::Flat => "±",
            Direction::Improved => "↑",
            Direction::Declined => "↓",
            Direction::Unknown => "?",
        }
    }

    /// Decide from a delta and the flat band, on a *badness*: down is better.
    ///
    /// The band is expressed in badness (a two-point `flat_within` becomes `0.02`, i.e. two percentage
    /// points of badness) because a dimension's *points* can move when a neighbour becomes measurable —
    /// the denominator is shared — while its badness is its own. Both numbers are printed, and
    /// [`TermDelta::points_delta`] is the one that adds up to the total.
    #[must_use]
    pub fn of_badness(delta: f64, flat_within: f64) -> Self {
        if delta.abs() <= flat_within {
            Direction::Flat
        } else if delta < 0.0 {
            Direction::Improved
        } else {
            Direction::Declined
        }
    }

    /// Decide from a delta and the flat band, on a *score*: up is better.
    #[must_use]
    pub fn of_score(delta: i64, flat_within: i64) -> Self {
        if delta.abs() <= flat_within {
            Direction::Flat
        } else if delta > 0 {
            Direction::Improved
        } else {
            Direction::Declined
        }
    }
}

/// One dimension's movement between two scorecards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TermDelta {
    /// Which dimension.
    pub dimension: super::dimension::Dimension,
    /// Whether both sides measured it. `false` means the comparison is not available and `why` says which
    /// side is missing — the one case where a delta must not print a number.
    pub comparable: bool,
    /// The badness at the anchor, when comparable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_badness: Option<f64>,
    /// The badness at the head, when comparable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_badness: Option<f64>,
    /// The change in badness, `to − from`, four decimals. Negative is better.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badness_delta: Option<f64>,
    /// The change in this dimension's points contribution, in hundredths.
    pub points_delta: i64,
    /// Which way, on the badness.
    pub direction: Direction,
    /// Why there is no comparison, when there is not one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// The delta from the last review to here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    /// The anchor it starts from.
    pub anchor: Anchor,
    /// How many scored revisions lie between the anchor and this one, this one included.
    pub revisions: usize,
    /// The score on each side.
    pub from_score: u8,
    /// As above, at the head.
    pub to_score: u8,
    /// Whether both sides carried a score at all.
    pub comparable: bool,
    /// The change in score, in points — the ticket's "one number".
    pub score_delta: i64,
    /// The change in the exact hundredths total, so the one number survives full precision.
    pub points_delta: i64,
    /// Which way the total moved, decided against the flat band.
    pub direction: Direction,
    /// One row per dimension, in model order.
    pub terms: Vec<TermDelta>,
    /// The dimensions that moved most, worst first: what to read first.
    pub worst: Vec<super::dimension::Dimension>,
    /// The basis line: rung, count, and the band the verdict used.
    pub basis: String,
    /// Notices, including the rung's price.
    pub notices: Vec<String>,
}

impl Delta {
    /// Compare two scorecards across an anchor.
    ///
    /// `flat_within` is in points and comes from the policy, because "did it move" is a threshold the
    /// reader is entitled to tune and the report must name either way.
    #[must_use]
    pub fn between(
        from: &Scorecard,
        to: &Scorecard,
        anchor: Anchor,
        revisions: usize,
        flat_within: i64,
    ) -> Self {
        let comparable = from.scored && to.scored;
        let score_delta = i64::from(to.score) - i64::from(from.score);
        let points_delta = to.points_delta(from);
        let mut terms: Vec<TermDelta> = Vec::new();
        for dimension in super::dimension::Dimension::ALL {
            let before = from.term(dimension);
            let after = to.term(dimension);
            match (before, after) {
                (Some(before), Some(after)) if before.measured && after.measured => {
                    let delta = super::dimension::round4(after.badness - before.badness);
                    terms.push(TermDelta {
                        dimension,
                        comparable: true,
                        from_badness: Some(before.badness),
                        to_badness: Some(after.badness),
                        badness_delta: Some(delta),
                        points_delta: after.points - before.points,
                        direction: Direction::of_badness(delta, (flat_within as f64) / 100.0),
                        why: None,
                    });
                }
                _ => terms.push(TermDelta {
                    dimension,
                    comparable: false,
                    from_badness: None,
                    to_badness: None,
                    badness_delta: None,
                    points_delta: after.map_or(0, |term| term.points)
                        - before.map_or(0, |term| term.points),
                    direction: Direction::Unknown,
                    why: Some(match (before, after) {
                        (None, _) | (_, None) => format!(
                            "{} is not in one of the two scorecards, so its movement is not \
                             knowable",
                            dimension.id()
                        ),
                        (Some(before), Some(after)) => format!(
                            "{} was measured at one end only ({} → {}), so there is no before-and-after \
                             for it",
                            dimension.id(),
                            if before.measured { "yes" } else { "no" },
                            if after.measured { "yes" } else { "no" }
                        ),
                    }),
                }),
            }
        }
        let mut worst: Vec<(super::dimension::Dimension, i64)> = terms
            .iter()
            .filter(|term| term.comparable)
            .map(|term| {
                (
                    term.dimension,
                    // Magnitude of the badness movement, scaled so a comparison is an integer: the
                    // hundredths of badness, which is what the points moved by too.
                    (term.badness_delta.unwrap_or(0.0).abs() * 10_000.0).round() as i64,
                )
            })
            .filter(|(_, magnitude)| *magnitude > 0)
            .collect();
        worst.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let worst = worst
            .into_iter()
            .take(3)
            .map(|(dimension, _)| dimension)
            .collect();
        let basis = format!(
            "{} · {revisions} scored revision(s) · flat within {} point(s)",
            anchor.sentence(),
            flat_within
        );
        let mut notices = vec![anchor.kind.price().to_owned()];
        if !comparable {
            notices.push(
                "one side of this comparison carried no score, so the score delta is not printed as \
                 movement"
                    .to_owned(),
            );
        }
        Self {
            anchor,
            revisions,
            from_score: from.score,
            to_score: to.score,
            comparable,
            score_delta,
            points_delta,
            direction: if comparable {
                Direction::of_score(points_delta, flat_within * 100)
            } else {
                Direction::Unknown
            },
            terms,
            worst,
            basis,
            notices,
        }
    }

    /// The ticket's one number, in one line: `since your last review (23 revisions): 74 → 61 (−13)`.
    #[must_use]
    pub fn headline(&self) -> String {
        if !self.comparable {
            return format!(
                "since the last review: no score on one side, so no delta — {}",
                self.basis
            );
        }
        format!(
            "since your last review ({} scored revision{}): {} → {} ({}{})",
            self.revisions,
            if self.revisions == 1 { "" } else { "s" },
            self.from_score,
            self.to_score,
            if self.score_delta > 0 { "+" } else { "" },
            self.score_delta
        )
    }

    /// The lines under the headline: one per dimension, worst first, then the rung's price.
    #[must_use]
    pub fn render(&self) -> Vec<String> {
        let mut out = vec![self.headline(), format!("  {}", self.basis)];
        for term in &self.terms {
            if !term.comparable {
                out.push(format!(
                    "  {:<11} {:>3}  {}",
                    term.dimension.id(),
                    term.direction.arrow(),
                    term.why.as_deref().unwrap_or("no comparison")
                ));
                continue;
            }
            let delta = term.badness_delta.unwrap_or(0.0);
            out.push(format!(
                "  {:<11} {:>3}  badness {} → {} ({}) · points {}",
                term.dimension.id(),
                term.direction.arrow(),
                super::dimension::round4(term.from_badness.unwrap_or(0.0)),
                super::dimension::round4(term.to_badness.unwrap_or(0.0)),
                signed4(delta),
                Points(term.points_delta),
            ));
        }
        if !self.worst.is_empty() {
            out.push(format!(
                "  read first: {}",
                self.worst
                    .iter()
                    .map(|dimension| dimension.id())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for notice in &self.notices {
            out.push(format!("  note: {notice}"));
        }
        out
    }

    /// The durable half of this delta, as the compact JSON the scorecard row keeps in its `since` column.
    ///
    /// Only the half that stays true after the review moves on: which anchor was used, who signed it, how
    /// many scored revisions it spanned, and the two scores. Not the per-dimension rows — those are the
    /// scorecards' own business, recomputable from the two rows, and storing them would be storing the same
    /// fact in two places with two chances to disagree. Written in the run that learned the anchor, which is
    /// what makes it a record rather than a guess: a report read a month from now, or a review state since
    /// cleared, cannot otherwise say what "since your last review" meant on the day the revision was scored.
    #[must_use]
    pub fn companion_json(&self) -> String {
        serde_json::json!({
            "model": super::score::MODEL,
            "anchor_kind": self.anchor.kind.as_str(),
            "anchor_revision": self.anchor.revision,
            "episode": self.anchor.review.as_ref().map(|r| r.episode.clone()),
            "state": self.anchor.review.as_ref().map(|r| r.state.clone()),
            "actor": self.anchor.review.as_ref().map(|r| r.actor.clone()),
            "instant": self.anchor.review.as_ref().map(|r| r.instant),
            "revisions": self.revisions,
            "from_score": self.from_score,
            "to_score": self.to_score,
            "direction": self.direction.as_str(),
        })
        .to_string()
    }

    /// The exported form.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "headline": self.headline(),
            "anchor": {
                "kind": self.anchor.kind.as_str(),
                "revision": self.anchor.revision,
                "why": self.anchor.why,
                "price": self.anchor.kind.price(),
                "review": self.anchor.review,
            },
            "revisions": self.revisions,
            "from_score": self.from_score,
            "to_score": self.to_score,
            "comparable": self.comparable,
            "score_delta": self.score_delta,
            "points_delta": self.points_delta,
            "direction": self.direction.as_str(),
            "basis": self.basis,
            "terms": self.terms.iter().map(|term| serde_json::json!({
                "dimension": term.dimension.id(),
                "comparable": term.comparable,
                "from_badness": term.from_badness,
                "to_badness": term.to_badness,
                "badness_delta": term.badness_delta,
                "points_delta": term.points_delta,
                "direction": term.direction.as_str(),
                "why": term.why,
            })).collect::<Vec<_>>(),
            "worst": self.worst.iter().map(|d| d.id()).collect::<Vec<_>>(),
            "notices": self.notices,
        })
    }
}

/// A signed four-decimal number, for a delta that must show its sign.
#[must_use]
fn signed4(value: f64) -> String {
    let rounded = super::dimension::round4(value);
    if rounded > 0.0 {
        format!("+{rounded}")
    } else if rounded < 0.0 {
        format!("{rounded}")
    } else {
        "0".to_owned()
    }
}

/// A revision shortened the way every other pane shortens one.
#[must_use]
fn short(revision: &str) -> String {
    if revision.chars().count() <= 12 {
        return revision.to_owned();
    }
    revision.chars().take(7).collect()
}

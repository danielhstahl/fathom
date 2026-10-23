//! The trend: hundreds of episodes of scorecards, rendered without growing a heap to hold them
//! (`e3.9`, `reco-al1`).
//!
//! A trend is the only place the scorecard earns its keep — one revision's number is a snapshot, thirty
//! of them are a story — and it is the one place the acceptance criteria bite:
//!
//! * **"renders across at least 500 episodes without memory growth"** — [`Trend::feed`] is a streaming
//!   fold. It keeps a *bounded* sample of the series and computes every aggregate from running sums, so
//!   the retained heap is a function of [`Policy::trend_cap`] and [`Policy::window`] and not of the number
//!   of revisions fed. [`Trend::heap_bytes`] is the assertion a test makes at 500 rows and again at 5 000,
//!   and it is the same shape as the ceiling argument in `reco-93w`: a bound you cannot measure is a wish.
//! * **the printed numbers are re-derivable** — the window's mean, best, worst and least-squares slope are
//!   exported with the sums they came from ([`Window::to_json`]), because "the trend is up" is not a fact a
//!   reviewer can check and `n 30 · Σx 435 · Σy 2214 · Σxy 33 210 · Σx² 6 355` is.
//!
//! # The thinning, and what it costs
//!
//! The series keeps the newest [`Policy::trend_cap`] / 2 revisions at full resolution. Older revisions are
//! sampled: every one of them until the second half is full, then every second, then every fourth — a
//! doubling stride, so a 5 000-revision history still shows the shape of a slump and a recovery while
//! holding at most `trend_cap` points. The count of revisions that were read and *not* plotted is printed
//! as `thinned`, because a series that silently drops points is a chart that lies, and one that says "3 872
//! revisions read, 240 plotted, older ones sampled every 16th" is a chart you can interrogate.
//!
//! # What is not here
//!
//! **No store read.** [`Row`] is a value; `crates/fathom-store`'s `scorecard_*` calls produce them and
//! `crates/fathom-cli` hands them over newest-first. **No clock** — a row's `built_utc` came from the store
//! row it belongs to. **No recomputation**: a row's numbers came from a scorecard computed once, when the
//! revision was indexed, and a trend that recomputed them would be a second analysis for a question that
//! has an answer on disk (lint `M03`).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::dimension::Dimension;
use super::policy::Policy;
use super::review::Direction;
use super::score::Scorecard;

/// One stored scorecard, as read back from the index.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The revision the row is keyed by.
    pub revision: String,
    /// The instant it was written, microseconds — the ordering key of the trend.
    pub built_utc: i64,
    /// The scorecard as scored then, numbers only (the prose lives in the source reports, not in the row).
    pub scorecard: Scorecard,
}

impl Row {
    /// Parse a stored `scorecard.dims` payload.
    ///
    /// `None` is a *stated* unreadable row, counted by [`Trend::rows_unreadable`]; a row written by a
    /// different model version is the same kind of thing and is counted separately, because a scorecard
    /// model change makes old rows incomparable and the report has to say which it is looking at.
    #[must_use]
    pub fn parse(revision: &str, built_utc: i64, dims: &str) -> Parse {
        match Scorecard::from_stored(dims) {
            Some(scorecard) if scorecard.model != super::score::MODEL => Parse::Foreign {
                model: scorecard.model.clone(),
            },
            Some(scorecard) => Parse::Row(Self {
                revision: revision.to_owned(),
                built_utc,
                scorecard,
            }),
            None => Parse::Unreadable,
        }
    }

    /// The point this row plots.
    #[must_use]
    pub fn point(&self) -> Point {
        Point::of(&self.scorecard)
    }

    /// Roughly what this row costs in memory, for the ceiling test.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.revision.len()
            + self.scorecard.episode.as_deref().map_or(0, str::len)
            + self.scorecard.terms.len() * std::mem::size_of::<super::score::Term>()
    }
}

/// What [`Row::parse`] found.
//
// The boxed variant is not shyness about the type: a `Row` carries its two revision strings and five badness
// figures, and a trend folds one of these per revision it read. Left unboxed, every `Foreign` and
// `Unreadable` — the common case in an index written by an older analyzer — would pay for the largest
// variant.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Parse {
    /// A row of this model, usable.
    Row(Row),
    /// A row written by a different scorecard model: not comparable, and said so.
    Foreign {
        /// The model the row claims.
        model: String,
    },
    /// A row whose payload did not parse: corruption, or a write that was cut short.
    Unreadable,
}

/// One plotted point: the minimum needed to draw and to compare.
///
/// Fixed-size on purpose. A point is what a trend holds thousands of, so the per-dimension part is an
/// array indexed by [`Dimension::index`] rather than a map, and the strings are the two a reader needs to
/// find the revision again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// The revision, as the store keyed it.
    pub revision: String,
    /// The instant it was scored, microseconds.
    pub built_utc: i64,
    /// The score, `0..=100`. Only meaningful with [`Point::scored`].
    pub score: u8,
    /// Whether a score exists at all.
    pub scored: bool,
    /// The exact hundredths total, so the mean is computed at full precision.
    pub total: i64,
    /// Each dimension's badness, indexed by [`Dimension::index`]; `0.0` where unmeasured, and the
    /// dimension is excluded from that dimension's mean by [`Window`] regardless of what is here.
    pub badness: [f64; 5],
    /// One bit per dimension, set where that dimension was measured. A badness of `0.0` and a dimension
    /// that said nothing are different facts, and this is the only place the difference survives the trip
    /// through the store.
    pub measured_bits: u8,
    /// How many dimensions were measured, so a row that lost one reads as a change of evidence rather
    /// than a change of health.
    pub measured: u8,
}

impl Point {
    /// Whether one dimension was measured at this point.
    #[must_use]
    pub const fn measured(&self, dimension: Dimension) -> bool {
        self.measured_bits & (1 << dimension.index()) != 0
    }
}

impl Point {
    /// Take the point a scorecard plots.
    #[must_use]
    pub fn of(scorecard: &Scorecard) -> Self {
        let mut badness = [0.0; 5];
        for term in &scorecard.terms {
            badness[term.dimension.index()] = term.badness;
        }
        let mut measured_bits = 0_u8;
        for term in &scorecard.terms {
            if term.measured {
                measured_bits |= 1 << term.dimension.index();
            }
        }
        Self {
            revision: scorecard.revision.clone(),
            built_utc: scorecard.built_utc,
            score: scorecard.score,
            scored: scorecard.scored,
            total: scorecard.total,
            badness,
            measured_bits,
            measured: scorecard.measured as u8,
        }
    }
}

/// The window's aggregates, computed from the window's points.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window {
    /// How many revisions the window is meant to cover.
    pub requested: usize,
    /// How many it actually holds.
    pub revisions: usize,
    /// How many of those carried a score.
    pub scored: usize,
    /// Whether the window is shorter than it asked to be because the index holds fewer revisions, as
    /// opposed to because the trend's own cap got in the way — two different partials.
    pub short_reason: Option<String>,
    /// The mean score across the scored rows, in points, two decimals.
    pub mean_score: f64,
    /// The same mean in the exact unit the sums are in: hundredths.
    pub mean_points: i64,
    /// The best row in the window.
    pub best: Option<(u8, String)>,
    /// The worst row in the window.
    pub worst: Option<(u8, String)>,
    /// The oldest row in the window — where a reader's eye starts.
    pub first: Option<(u8, String)>,
    /// The newest row, which is also the head of the trend.
    pub last: Option<(u8, String)>,
    /// Per-dimension mean badness, with how many rows measured it: `(mean, measured, of)`.
    pub mean_badness: Vec<(String, f64, usize, usize)>,
    /// The least-squares sums, in chronological order with `x` from 0.
    pub sums: Sums,
    /// The fitted slope, in score points per revision.
    pub slope: f64,
    /// The same slope per hundred revisions, which is the number that can be compared to the flat band.
    pub slope_per_100: f64,
    /// Which way the window is going, decided against the flat band over the window's own span.
    pub direction: Direction,
    /// The band that decided it, in points.
    pub flat_within: i64,
}

/// The five sums a least-squares fit is made of — exported so the slope can be redone by hand.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Sums {
    /// Count.
    pub n: f64,
    /// Σx.
    pub sum_x: f64,
    /// Σy.
    pub sum_y: f64,
    /// Σxy.
    pub sum_xy: f64,
    /// Σx².
    pub sum_xx: f64,
}

impl Sums {
    /// The fitted slope, `None` when the fit is not defined (fewer than two rows, or all `x` equal).
    #[must_use]
    pub fn slope(&self) -> Option<f64> {
        if self.n < 2.0 {
            return None;
        }
        let denominator = self.n * self.sum_xx - self.sum_x * self.sum_x;
        if denominator.abs() < f64::EPSILON {
            return None;
        }
        Some((self.n * self.sum_xy - self.sum_x * self.sum_y) / denominator)
    }
}

/// The trend: a bounded sample of the series plus the window's aggregates.
#[derive(Debug, Clone)]
pub struct Trend {
    /// The cap that was asked for.
    pub cap: usize,
    /// The window that was asked for.
    pub window: usize,
    /// The flat band, in points.
    pub flat_within: i64,
    /// The newest revisions, at full resolution, newest first. Bounded by `cap / 2`.
    recent: VecDeque<Point>,
    /// The sampled tail, older than `recent`, newest first. Bounded by `cap / 2`.
    older: Vec<Point>,
    /// The current sampling stride for [`Trend::older`].
    pub stride: usize,
    /// How many rows were fed.
    pub rows_read: usize,
    /// Rows that came from a different scorecard model, and were refused.
    pub rows_foreign: usize,
    /// Rows whose payload did not parse.
    pub rows_unreadable: usize,
    /// The first thing fed — the head of the trend, kept for the per-episode step.
    pub head: Option<Point>,
    /// The second thing fed: the "previous" side of the per-episode delta.
    pub previous: Option<Point>,
}

impl Trend {
    /// A trend with the policy's cap, window and band.
    #[must_use]
    pub fn new(policy: &Policy) -> Self {
        Self {
            cap: policy.trend_cap,
            window: policy.window,
            flat_within: policy.flat_within,
            recent: VecDeque::new(),
            older: Vec::new(),
            stride: 1,
            rows_read: 0,
            rows_foreign: 0,
            rows_unreadable: 0,
            head: None,
            previous: None,
        }
    }

    /// The resolution each half of the sample gets.
    #[must_use]
    fn halves(&self) -> (usize, usize) {
        let recent = (self.cap / 2).max(1);
        let older = self.cap.saturating_sub(recent).max(1);
        (recent, older)
    }

    /// Fold one row in. **Rows arrive newest-first**, which is the order the store lists them in and the
    /// order that lets the window be a prefix rather than a ring.
    pub fn feed(&mut self, row: &Row) {
        self.rows_read += 1;
        let point = row.point();
        if self.head.is_none() {
            self.head = Some(point.clone());
        } else if self.previous.is_none() {
            self.previous = Some(point.clone());
        }
        let (recent_cap, older_cap) = self.halves();
        if self.recent.len() < recent_cap {
            self.recent.push_back(point);
            return;
        }
        if self.rows_read.is_multiple_of(self.stride) {
            self.older.push(point);
        }
        if self.older.len() > older_cap {
            // Full: double the stride and thin what is held by keeping every second point, which is the
            // same operation as "sampled every 2× before, now every 4×". Done in place, so the peak is the
            // cap and not twice it.
            self.stride *= 2;
            let keep: Vec<Point> = self.older.iter().step_by(2).cloned().collect();
            self.older = keep;
        }
    }

    /// Refuse a row that was written by another model version, counting it.
    pub fn refuse_foreign(&mut self) {
        self.rows_foreign += 1;
    }

    /// Count a row whose payload would not parse.
    pub fn count_unreadable(&mut self) {
        self.rows_unreadable += 1;
    }

    /// The series, newest first: the full-resolution head, then the sampled tail.
    #[must_use]
    pub fn points(&self) -> Vec<Point> {
        let mut out: Vec<Point> = Vec::with_capacity(self.recent.len() + self.older.len());
        out.extend(self.recent.iter().cloned());
        out.extend(self.older.iter().cloned());
        out
    }

    /// How many revisions were read and not plotted — the honest denominator of a thinned chart.
    #[must_use]
    pub fn thinned(&self) -> usize {
        self.rows_read.saturating_sub(self.points().len())
    }

    /// The window's points: the newest `window` rows fed, newest first.
    #[must_use]
    pub fn window_points(&self) -> Vec<Point> {
        self.recent.iter().take(self.window).cloned().collect()
    }

    /// The window's aggregates.
    #[must_use]
    pub fn window_summary(&self) -> Window {
        let points = self.window_points();
        let scored: Vec<&Point> = points.iter().filter(|point| point.scored).collect();
        let total_hundredths: i64 = scored.iter().map(|point| point.total).sum();
        let mean_points = if scored.is_empty() {
            0
        } else {
            // The mean of the exact totals, rounded once, so the printed mean is the mean of the printed
            // numbers rather than a third rounding of each.
            (total_hundredths + (i64::try_from(scored.len()).unwrap_or(1) / 2))
                / i64::try_from(scored.len()).unwrap_or(1)
        };
        let best = scored
            .iter()
            .max_by(|left, right| {
                left.score
                    .cmp(&right.score)
                    .then_with(|| right.revision.cmp(&left.revision))
            })
            .map(|point| (point.score, short(&point.revision)));
        let worst = scored
            .iter()
            .min_by(|left, right| {
                left.score
                    .cmp(&right.score)
                    .then_with(|| left.revision.cmp(&right.revision))
            })
            .map(|point| (point.score, short(&point.revision)));
        // The least-squares fit runs in chronological order with `x` from 0, which is the spelling the
        // exported sums mean. The window arrived newest-first, so walk it backwards.
        let mut sums = Sums::default();
        for (index, point) in scored.iter().rev().enumerate() {
            let x = f64::from(i32::try_from(index).unwrap_or(i32::MAX));
            let y = f64::from(u32::from(point.score));
            sums.n += 1.0;
            sums.sum_x += x;
            sums.sum_y += y;
            sums.sum_xy += x * y;
            sums.sum_xx += x * x;
        }
        let slope = sums.slope().unwrap_or(0.0);
        let slope_per_100 = round2(slope * 100.0);
        let span = i64::try_from(scored.len().saturating_sub(1)).unwrap_or(i64::MAX);
        let predicted = (slope * f64::from(i32::try_from(span).unwrap_or(i32::MAX))).round() as i64;
        let short_reason = if points.len() < self.window {
            Some(format!(
                "the index holds {} scored revision(s) in all, so the window is {} rather than {}",
                self.rows_read,
                points.len(),
                self.window
            ))
        } else if self.recent.len() < self.window {
            Some(format!(
                "the trend's own resolution keeps {} point(s), so the window is shorter than {} \
                 revisions; raise `scorecard.trend_cap` to widen it",
                self.recent.len(),
                self.window
            ))
        } else {
            None
        };
        let mut mean_badness: Vec<(String, f64, usize, usize)> = Vec::new();
        for dimension in Dimension::ALL {
            let at = dimension.index();
            let measured: Vec<f64> = points
                .iter()
                .filter(|point| point.measured(dimension))
                .map(|point| point.badness[at])
                .collect();
            // A dimension that was unmeasured in some rows is averaged over the rows that measured it,
            // and the count travels with the mean: a mean over 4 of 30 rows is a different claim.
            let mean = if measured.is_empty() {
                0.0
            } else {
                round4(
                    measured.iter().fold(0.0_f64, |sum, value| sum + value)
                        / f64::from(i32::try_from(measured.len()).unwrap_or(1)),
                )
            };
            mean_badness.push((
                dimension.id().to_owned(),
                mean,
                measured.len(),
                points.len(),
            ));
        }
        Window {
            requested: self.window,
            revisions: points.len(),
            scored: scored.len(),
            short_reason,
            mean_score: round2(f64::from(i32::try_from(mean_points).unwrap_or(i32::MAX)) / 100.0),
            mean_points,
            best,
            worst,
            first: scored
                .last()
                .map(|point| (point.score, short(&point.revision))),
            last: scored
                .first()
                .map(|point| (point.score, short(&point.revision))),
            mean_badness,
            sums,
            slope: round4(slope),
            slope_per_100,
            direction: Direction::of_score(predicted, self.flat_within),
            flat_within: self.flat_within,
        }
    }

    /// The step from the previous scored revision to the head — the per-episode delta, as a pair of points
    /// the caller can turn into a [`super::review::Delta`] if it wants the anchor's version of the same
    /// comparison.
    #[must_use]
    pub fn step(&self) -> Option<(Point, Point)> {
        match (&self.head, &self.previous) {
            (Some(head), Some(previous)) => Some((head.clone(), previous.clone())),
            _ => None,
        }
    }

    /// The bytes this trend will hold at most, whatever it is fed.
    ///
    /// Measured rather than counted by hand because the acceptance criterion is about growth, and the
    /// assertion that matters is "`heap_bytes` after 5 000 rows is the same as after 500", which a
    /// hand-count of `size_of::<Point>()` would not prove.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        let point = std::mem::size_of::<Point>();
        self.recent.capacity() * point
            + self.older.capacity() * point
            + self
                .recent
                .iter()
                .chain(self.older.iter())
                .map(|point| point.revision.len())
                .sum::<usize>()
    }

    /// The lines under the score: the window's numbers, then the series in a sparkline, then the thinning
    /// that made it.
    #[must_use]
    pub fn render(&self) -> Vec<String> {
        let window = self.window_summary();
        let mut out = Vec::new();
        out.push(format!(
            "window {} revision(s) · {} scored · mean {} · best {} · worst {}",
            window.revisions,
            window.scored,
            window.mean_score,
            window
                .best
                .as_ref()
                .map(|(score, rev)| format!("{score} at {rev}"))
                .unwrap_or_else(|| "—".to_owned()),
            window
                .worst
                .as_ref()
                .map(|(score, rev)| format!("{score} at {rev}"))
                .unwrap_or_else(|| "—".to_owned()),
        ));
        out.push(format!(
            "  trend {} across the window · slope {} points/100 revisions ({} in window)",
            window.direction.as_str(),
            window.slope_per_100,
            window.sums.n as usize
        ));
        if let Some(reason) = &window.short_reason {
            out.push(format!("  window is shorter than asked: {reason}"));
        }
        out.push(format!(
            "  per-dimension mean badness: {}",
            window
                .mean_badness
                .iter()
                .map(|(id, mean, measured, of)| format!("{id} {mean} ({measured}/{of})"))
                .collect::<Vec<_>>()
                .join(" · ")
        ));
        if let Some((first, last)) = window.first.zip(window.last) {
            out.push(format!(
                "  {} at {} → {} at {} across it",
                first.0, first.1, last.0, last.1
            ));
        }
        let points = self.points();
        if points.len() > 1 {
            out.push(format!("  series {}", sparkline(&points)));
        }
        out.push(format!(
            "  series: {} point(s) plotted of {} revision(s) read · {} thinned · older sampled every \
             {}th · {} row(s) from another model · {} row(s) unreadable",
            points.len(),
            self.rows_read,
            self.thinned(),
            self.stride,
            self.rows_foreign,
            self.rows_unreadable,
        ));
        out
    }

    /// The exported form.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let window = self.window_summary();
        serde_json::json!({
            "rows_read": self.rows_read,
            "rows_foreign": self.rows_foreign,
            "rows_unreadable": self.rows_unreadable,
            "cap": self.cap,
            "stride": self.stride,
            "plotted": self.points().len(),
            "thinned": self.thinned(),
            "heap_bytes": self.heap_bytes(),
            "window": {
                "requested": window.requested,
                "revisions": window.revisions,
                "scored": window.scored,
                "short_reason": window.short_reason,
                "mean_score": window.mean_score,
                "mean_points": window.mean_points,
                "best": window.best.map(|(score, rev)| serde_json::json!({"score": score, "revision": rev})),
                "worst": window.worst.map(|(score, rev)| serde_json::json!({"score": score, "revision": rev})),
                "first": window.first.map(|(score, rev)| serde_json::json!({"score": score, "revision": rev})),
                "last": window.last.map(|(score, rev)| serde_json::json!({"score": score, "revision": rev})),
                "mean_badness": window.mean_badness.iter().map(|(id, mean, measured, of)| serde_json::json!({
                    "dimension": id, "mean": mean, "measured": measured, "of": of,
                })).collect::<Vec<_>>(),
                "sums": window.sums,
                "slope_per_revision": window.slope,
                "slope_per_100": window.slope_per_100,
                "direction": window.direction.as_str(),
                "flat_within": window.flat_within,
            },
            "step": self.step().map(|(head, previous)| serde_json::json!({
                "head": head, "previous": previous,
            })),
            "series": self.points().iter().map(|point| serde_json::json!({
                "revision": short(&point.revision),
                "revision_full": point.revision,
                "built_utc": point.built_utc,
                "score": point.score,
                "scored": point.scored,
                "total": point.total,
                "measured": point.measured,
                "dimensions": Dimension::ALL.iter().enumerate().map(|(at, dimension)| serde_json::json!({
                    "dimension": dimension.id(),
                    "badness": point.badness[at],
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// Draw the series as one line of blocks, oldest on the left.
///
/// Eight levels of block are enough for a score and each is also a distinct height, so the shape survives
/// a terminal with no colour (ADR-006 §6.6).
#[must_use]
pub fn sparkline(points: &[Point]) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let mut out = String::new();
    for point in points.iter().rev() {
        if !point.scored {
            out.push('·');
            continue;
        }
        let at = (usize::from(point.score) * BLOCKS.len() / 101).min(BLOCKS.len() - 1);
        out.push(BLOCKS[at]);
    }
    out
}

#[must_use]
fn round2(value: f64) -> f64 {
    super::dimension::round2(value)
}

#[must_use]
fn round4(value: f64) -> f64 {
    super::dimension::round4(value)
}

/// A revision shortened the way every other pane shortens one.
#[must_use]
fn short(revision: &str) -> String {
    if revision.chars().count() <= 12 {
        return revision.to_owned();
    }
    revision.chars().take(7).collect()
}

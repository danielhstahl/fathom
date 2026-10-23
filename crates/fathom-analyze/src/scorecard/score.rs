//! The arithmetic: five measurements and five weights in, one explainable number out (`e3.9`,
//! `reco-al1`).
//!
//! ```text
//! architecture-1 74 = boundaries 29.16 + acyclicity 22.10 + hotspots 9.06 + coupling 13.68
//! ```
//!
//! # The whole model, in five lines
//!
//! ```text
//! x_d      = badness of dimension d, from n and d (or n and k) as printed by that dimension
//! h_d      = 1 − x_d                              the health of the dimension, 0.0..=1.0
//! W        = Σ w_d                                over the dimensions that were measured
//! points_d = round(10000 · w_d · h_d / W)         integer hundredths
//! score    = clamp(round(Σ points_d / 100), 0, 100)
//! ```
//!
//! That is all of it. There is no hidden layer, no learned coefficient, no normalisation a reader cannot
//! redo from the printed line, and — because the ticket asks for a formula *"reproducible by hand from
//! exported JSON"* — every integer and every rounding the arithmetic used is in the JSON next to the
//! number it produced. This is [`crate::risk::score`]'s model, deliberately: two scores in one product
//! that both claim to be explainable must not be explainable in two ways, so this module reuses
//! [`Points`] and the hundredths convention rather than inventing a second one.
//!
//! # Health goes up; badness is what was measured
//!
//! A dimension measures *badness* (`0.0` = nothing found) and the score is built from its
//! *health* (`1.0` = clean), so a higher scorecard is a better architecture — which is the direction a
//! human tracking a week of agent output expects, and the direction the word "scorecard" has always meant
//! in this product's glossary. Both numbers are exported per dimension: the health is what multiplied the
//! weight, the badness is what the dimension's own formula produced, and a reader checking the sum needs
//! to see that they are `1 − each other` rather than two separately-rounded quantities.
//!
//! # Four properties the tests hold this to
//!
//! * **The printed parts sum to the printed total.** Terms are integer hundredths, so
//!   `29.16 + 22.10 + 9.06 + 13.68` is exact in base 10 and a reviewer with a calculator arrives at the
//!   same number. [`Scorecard::check_sum`] is the assertion, not a coincidence.
//! * **An unmeasured dimension redistributes and says so.** `W` is the weight sum over the *measured*
//!   dimensions, so a missing dimension gives its share to the rest instead of quietly lowering the score
//!   — and the printed line names the denominator, because a denominator the reader cannot see is a
//!   second hidden layer. A dimension weighted zero is kept, with zero points, rather than dropped:
//!   "switched off on purpose" and "could not be measured" are different statements.
//! * **Monotone.** Improving a dimension never lowers the score, and a scorecard at least as healthy as
//!   another on every dimension stays at or above it under any non-negative weights — including after a
//!   retune. That is what makes retuning safe to ship, and it holds here for the same reason it holds in
//!   `e4.4`: the arithmetic is one division per dimension and an integer sum.
//! * **No score is a refusal, not a zero.** When nothing was measured, or every weight is zero,
//!   `scored` is false and the report prints why. A `0` read as "the architecture is a disaster" is
//!   worse than no number.

use serde::{Deserialize, Serialize};

use crate::risk::score::Points;

use super::dimension::{Dimension, Measurement};
use super::policy::Policy;

/// The model's name, stored with every scorecard so a two-year-old row can still be read correctly.
pub const MODEL: &str = "architecture-1";

/// The model's version. Bumped by a change to a curve, a default weight, or the rounding rule — each of
/// which changes the printed number for the same repository.
pub const MODEL_VERSION: u32 = 1;

/// One dimension's contribution: its measurement, its weight, its points, and the sentence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Term {
    /// Which dimension.
    pub dimension: Dimension,
    /// Whether it was measured at all.
    pub measured: bool,
    /// Why there is no measurement, when there is not one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unscored: Option<String>,
    /// The curve and its integers — the inputs a hand-check starts from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<super::dimension::Curve>,
    /// The arithmetic written out: `3 / 412 = 0.0073`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// The badness the curve produced, `0.0..=1.0`, four decimals.
    pub badness: f64,
    /// The health the score was built from: exactly `1 − badness`.
    pub health: f64,
    /// The weight in force.
    pub weight: f64,
    /// Where the weight came from: `default` or `fathom.toml`.
    pub origin: String,
    /// The contribution, in hundredths of a point. Zero when unmeasured.
    pub points: i64,
    /// What this dimension would have contributed at health `1.0` — the ceiling the points are spent
    /// out of, printed as `22.10/25.00` so a low number reads as "this axis was healthy" and not as
    /// "this axis was left out".
    pub ceiling: i64,
    /// What it was measured from.
    pub basis: String,
    /// The claim a reviewer can check against a file.
    pub reason: String,
    /// Whether the measurement was complete at its own grain.
    pub complete: bool,
    /// Every caveat, in the order found.
    pub caveats: Vec<String>,
}

impl Term {
    /// The points as printed: `22.10`.
    #[must_use]
    pub fn points_shown(&self) -> Points {
        Points(self.points)
    }

    /// The `points/ceiling` pair, as `e4.4` prints it.
    #[must_use]
    pub fn share_shown(&self) -> String {
        format!("{}/{}", Points(self.points), Points(self.ceiling))
    }
}

/// One scorecard: five dimensions, their arithmetic, and the number they make.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scorecard {
    /// [`MODEL`].
    pub model: String,
    /// [`MODEL_VERSION`].
    pub model_version: u32,
    /// The revision this was scored at, as the caller named it.
    pub revision: String,
    /// The episode this was scored as, when the caller had one to name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<String>,
    /// All five dimensions, in [`Dimension::ALL`] order, measured or not — nothing is dropped, because a
    /// dropped dimension is a hole and an unmeasured one is a sentence.
    pub terms: Vec<Term>,
    /// The score, `0..=100`. Meaningless unless [`Scorecard::scored`].
    pub score: u8,
    /// Whether a score exists.
    pub scored: bool,
    /// Why there is no score, when there is not one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unscored: Option<String>,
    /// The denominator actually used: the weight sum over the measured dimensions, printed and rounded
    /// exactly as the arithmetic used it.
    pub denominator: f64,
    /// The exact sum of [`Term::points`], so the printed total survives full precision.
    pub total: i64,
    /// How many of the five were measured.
    pub measured: usize,
    /// How many dimensions the model has.
    pub dimensions: usize,
    /// The threshold below which [`Scorecard::alert`] is true. A report's line, not an exit code: the
    /// gate that exits is `fathom check` (**reco-ngk**).
    pub alert_below: u8,
    /// Whether the score sits below the threshold.
    pub alert: bool,
    /// What the reads could not do, gathered from every dimension and from the caller.
    pub notices: Vec<String>,
    /// Which knobs produced this, so a stored row can be read without the config.
    pub weight_sum: f64,
    /// When this was scored, in `fathom_core`'s microseconds — the trend's ordering key.
    pub built_utc: i64,
}

impl Scorecard {
    /// Score one revision from five measurements.
    ///
    /// The denominator is the weight sum over the *measured* dimensions and is rounded before it divides,
    /// for the reason `e4.4` gives: what is printed is then exactly what was used, so a reader with a
    /// calculator arrives at the same points rather than at `73.99999`.
    #[must_use]
    pub fn of(
        measurements: &[Measurement],
        policy: &Policy,
        revision: &str,
        built_utc: i64,
    ) -> Self {
        let measured: Vec<&Measurement> = measurements.iter().filter(|m| m.measured).collect();
        let raw: f64 = measured
            .iter()
            .map(|item| policy.weight(item.dimension))
            .fold(0.0_f64, |total, weight| total + weight);
        let denominator = super::dimension::round4(raw);

        let mut terms: Vec<Term> = measurements
            .iter()
            .map(|measurement| {
                let weight = policy.weight(measurement.dimension);
                let origin = policy.origin(measurement.dimension).as_str();
                let health = measurement.health();
                let ceiling = if measurement.measured && denominator > 0.0 {
                    points(weight * 1.0 / denominator)
                } else {
                    0
                };
                let points = if measurement.measured && denominator > 0.0 {
                    points(weight * health / denominator)
                } else {
                    0
                };
                Term {
                    dimension: measurement.dimension,
                    measured: measurement.measured,
                    unscored: measurement.unscored.clone(),
                    curve: measurement.curve,
                    formula: measurement.curve.map(|curve| curve.formula()),
                    badness: measurement.badness(),
                    health,
                    weight,
                    origin: origin.to_owned(),
                    points,
                    ceiling,
                    basis: measurement.basis.clone(),
                    reason: measurement.reason.clone(),
                    complete: measurement.complete,
                    caveats: measurement.caveats.clone(),
                }
            })
            .collect();
        // The printed order is the model's order. A reader who is checking the sum wants to find
        // `boundaries` in the same place in every report, and a sort by points would move it with the
        // news. `fathom why` sorts because twelve features is a list; five dimensions is a table.
        terms.sort_by_key(|term| term.dimension.index());

        let total: i64 = terms.iter().fold(0_i64, |sum, term| sum + term.points);
        let scored = denominator > 0.0 && !measured.is_empty();
        let unscored = if scored {
            None
        } else if measured.is_empty() {
            Some(
                "no dimension could be measured from what this index holds, so there is no score — \
                 which is not the same as a score of zero",
            )
        } else {
            Some(
                "every dimension that was measured is weighted zero, so the score would say nothing the \
                 five dimensions do not already say",
            )
        };
        let unscored: Option<String> = unscored.map(str::to_owned);
        let score = if scored {
            // Half-up on the hundredths, which is what `round` means for a positive integer sum here, and
            // what makes `score` the number the printed `total` implies rather than a third rounding.
            let scaled = (total as f64 / 100.0).round();
            scaled.clamp(0.0, 100.0) as u8
        } else {
            0
        };
        let mut notices: Vec<String> = Vec::new();
        for measurement in measurements {
            if !measurement.measured
                && let Some(why) = &measurement.unscored
            {
                notices.push(format!("{}: {why}", measurement.dimension.id()));
            }
            for caveat in &measurement.caveats {
                let line = format!("{}: {caveat}", measurement.dimension.id());
                if !notices.contains(&line) {
                    notices.push(line);
                }
            }
        }
        let alert_below = policy.alert_below;
        Self {
            model: MODEL.to_owned(),
            model_version: MODEL_VERSION,
            revision: revision.to_owned(),
            episode: None,
            terms,
            score,
            scored,
            unscored,
            denominator,
            total,
            measured: measured.len(),
            dimensions: Dimension::ALL.len(),
            alert_below,
            alert: scored && score < alert_below,
            notices,
            weight_sum: denominator,
            built_utc,
        }
    }

    /// One dimension's term, by name.
    #[must_use]
    pub fn term(&self, dimension: Dimension) -> Option<&Term> {
        self.terms.iter().find(|term| term.dimension == dimension)
    }

    /// How many points this scorecard sits above or below another — the "one number" a reviewer who has
    /// been away two days is actually asking for.
    #[must_use]
    pub fn points_delta(&self, other: &Scorecard) -> i64 {
        self.total - other.total
    }

    /// Whether the score moved more than "flat" allows, given the band.
    #[must_use]
    pub fn moved(&self, other: &Scorecard, flat_within: i64) -> bool {
        self.points_delta(other).abs() > flat_within * 100
    }

    /// The contribution line: `architecture-1 74 = boundaries 29.16 + …`.
    #[must_use]
    pub fn vector(&self) -> String {
        if !self.scored {
            return format!(
                "{} — no score: {}",
                self.model,
                self.unscored.as_deref().unwrap_or("nothing was measured")
            );
        }
        let parts: Vec<String> = self
            .terms
            .iter()
            .filter(|term| term.measured)
            .map(|term| format!("{} {}", term.dimension.id(), Points(term.points)))
            .collect();
        if parts.is_empty() {
            return format!("{} {} = no contribution", self.model, self.score);
        }
        format!("{} {} = {}", self.model, self.score, parts.join(" + "))
    }

    /// The paragraph a pane prints: the vector, the denominator it was divided by, and one line per
    /// dimension with its formula.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut out = vec![self.vector()];
        if self.scored {
            out.push(format!(
                "  {} of 100 · denominator {} (weights over {}/{} dimensions measured)",
                self.score,
                round4(self.denominator),
                self.measured,
                self.dimensions
            ));
        } else if let Some(why) = &self.unscored {
            out.push(format!("  {why}"));
        }
        for term in &self.terms {
            if term.measured {
                out.push(format!(
                    "  {:<11} {:>13} {:>7} {:>13}  {}",
                    term.dimension.id(),
                    term.share_shown(),
                    term.health,
                    term.formula.clone().unwrap_or_default(),
                    term.reason
                ));
            } else {
                out.push(format!(
                    "  {:<11} {:>13} {:>7} {:>13}  {}",
                    term.dimension.id(),
                    "—",
                    "—",
                    "—",
                    term.unscored.as_deref().unwrap_or("not measured")
                ));
            }
        }
        out.join("\n")
    }

    /// The one line for a card or a CI log: the number, and the two dimensions that moved most.
    #[must_use]
    pub fn headline(&self) -> String {
        if !self.scored {
            return format!(
                "architecture: no score — {}",
                self.unscored.as_deref().unwrap_or("nothing was measured")
            );
        }
        format!(
            "architecture {}/100 · {}/{} dimensions measured · denominator {}",
            self.score,
            self.measured,
            self.dimensions,
            round4(self.denominator)
        )
    }

    /// The exact sum of the printed parts, so a test — and a reviewer — can assert it rather than trust
    /// it. Mirrors [`crate::hotspots::report::check_sum`]'s role for the hotspot table.
    #[must_use]
    pub fn check_sum(&self) -> Points {
        Points(self.terms.iter().fold(0_i64, |sum, term| sum + term.points))
    }

    /// The exported form: the whole document a dashboard or `fathom check` reads.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "model_version": self.model_version,
            "revision": short(&self.revision),
            "revision_full": self.revision,
            "episode": self.episode,
            "score": self.score,
            "scored": self.scored,
            "unscored": self.unscored,
            "alert": self.alert,
            "alert_below": self.alert_below,
            "denominator": round4(self.denominator),
            "total": self.total,
            "total_shown": Points(self.total).to_string(),
            "checked_sum": self.check_sum().0,
            "measured": self.measured,
            "dimensions": self.dimensions,
            "weight_sum": round4(self.weight_sum),
            "built_utc": self.built_utc,
            "terms": self.terms.iter().map(term_json).collect::<Vec<_>>(),
            "notices": self.notices,
        })
    }

    /// The compact form stored in `scorecard.dims`: enough to rebuild a trend point and a
    /// since-last-review delta without re-reading a graph.
    ///
    /// Deliberately not the whole document: the per-term prose is large, reprintable from the source
    /// reports, and a trend that stored it for 500 revisions would be a trend that stored the same
    /// sentence five hundred times. The numbers a delta needs — badness, curve, measured — are here.
    #[must_use]
    pub fn to_stored(&self) -> String {
        let value = serde_json::json!({
            "model": self.model,
            "model_version": self.model_version,
            "score": self.score,
            "scored": self.scored,
            "total": self.total,
            "denominator": round4(self.denominator),
            "measured": self.measured,
            "dimensions": self.dimensions,
            "alert": self.alert,
            "alert_below": self.alert_below,
            "revision": self.revision,
            "episode": self.episode,
            "built_utc": self.built_utc,
            "terms": self.terms.iter().map(|term| serde_json::json!({
                "dimension": term.dimension,
                "measured": term.measured,
                "badness": term.badness,
                "points": term.points,
                "ceiling": term.ceiling,
                "weight": term.weight,
                "curve": term.curve,
                "complete": term.complete,
            })).collect::<Vec<_>>(),
        });
        // A `serde_json` failure here would mean the document contains a float that is NaN or infinite,
        // and both are impossible by construction: every float is a ratio of two finite counts, rounded.
        // The fallback is still stated rather than swallowed: `null` is what a reader will see, and the
        // store write refuses a row whose `dims` does not parse.
        value.to_string()
    }

    /// Read back a [`Scorecard::to_stored`] payload.
    #[must_use]
    pub fn from_stored(text: &str) -> Option<Scorecard> {
        serde_json::from_str::<StoredScore>(text)
            .ok()
            .map(|stored| stored.into_scorecard())
    }
}

/// The stored shape: the same fields, with the prose absent.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct StoredScore {
    model: String,
    model_version: u32,
    revision: String,
    #[serde(default)]
    episode: Option<String>,
    score: u8,
    #[serde(default)]
    scored: bool,
    #[serde(default)]
    total: i64,
    #[serde(default)]
    denominator: f64,
    #[serde(default)]
    measured: usize,
    #[serde(default)]
    dimensions: usize,
    #[serde(default)]
    alert: bool,
    #[serde(default)]
    alert_below: u8,
    #[serde(default)]
    built_utc: i64,
    #[serde(default)]
    terms: Vec<StoredTerm>,
}

impl StoredScore {
    /// Rebuild a scorecard from a stored row. The prose is gone and is not re-invented: a stored row's
    /// reason line is empty, and every consumer that wants prose re-reads the source report.
    fn into_scorecard(self) -> Scorecard {
        Scorecard {
            model: self.model,
            model_version: self.model_version,
            revision: self.revision,
            episode: self.episode,
            terms: self
                .terms
                .into_iter()
                .map(|term| Term {
                    dimension: term.dimension,
                    measured: term.measured,
                    unscored: if term.measured {
                        None
                    } else {
                        Some("measured at an earlier revision; the prose was not stored".to_owned())
                    },
                    curve: term.curve,
                    formula: term.curve.map(|curve| curve.formula()),
                    badness: term.badness,
                    health: super::dimension::round4(1.0 - term.badness),
                    weight: term.weight,
                    origin: String::new(),
                    points: term.points,
                    ceiling: term.ceiling,
                    basis: String::new(),
                    reason: String::new(),
                    complete: term.complete,
                    caveats: Vec::new(),
                })
                .collect(),
            score: self.score,
            scored: self.scored,
            unscored: if self.scored {
                None
            } else {
                Some("stored unscored".to_owned())
            },
            denominator: self.denominator,
            total: self.total,
            measured: self.measured,
            dimensions: self.dimensions,
            alert_below: self.alert_below,
            alert: self.alert,
            notices: Vec::new(),
            weight_sum: self.denominator,
            built_utc: self.built_utc,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct StoredTerm {
    dimension: Dimension,
    #[serde(default)]
    measured: bool,
    #[serde(default)]
    badness: f64,
    #[serde(default)]
    points: i64,
    #[serde(default)]
    ceiling: i64,
    #[serde(default)]
    weight: f64,
    #[serde(default)]
    curve: Option<super::dimension::Curve>,
    #[serde(default)]
    complete: bool,
}

/// A term's full JSON: everything the printed line depends on, each one named.
fn term_json(term: &Term) -> serde_json::Value {
    serde_json::json!({
        "dimension": term.dimension.id(),
        "title": term.dimension.title(),
        "question": term.dimension.question(),
        "source": term.dimension.source(),
        "measured": term.measured,
        "unscored": term.unscored,
        "curve": term.curve,
        "formula": term.formula,
        "badness": round4(term.badness),
        "health": round4(term.health),
        "weight": term.weight,
        "origin": term.origin,
        "points": term.points,
        "points_shown": Points(term.points).to_string(),
        "ceiling": term.ceiling,
        "ceiling_shown": Points(term.ceiling).to_string(),
        "basis": term.basis,
        "reason": term.reason,
        "complete": term.complete,
        "caveats": term.caveats,
    })
}

/// One dimension's points, in hundredths, rounded half-up — the single rounding step of the model.
#[must_use]
fn points(fraction: f64) -> i64 {
    if !fraction.is_finite() {
        return 0;
    }
    (fraction * 10_000.0).round() as i64
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

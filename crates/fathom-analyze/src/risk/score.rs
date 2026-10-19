//! The arithmetic: twelve measurements and twelve weights in, one explainable number out (`reco-884`).
//!
//! ```text
//! features-1 82 (flagged, τ 70) = findings 24.00 + surface 19.50 + migration 9.00 + …
//! ```
//!
//! # The whole model, in four lines
//!
//! ```text
//! W       = Σ w_i       over the features that were measured
//! points_i = round(10000 · w_i · x_i / W)      x_i in 0.0..=1.0, w_i in 0.0..=1.0
//! total   = Σ points_i                          an exact integer sum of the printed parts
//! score   = clamp(round(total / 100), 0, 100)
//! ```
//!
//! That is the model. There is no hidden layer, no learned coefficient, no normalisation the reader
//! cannot redo from the printed line — which is what ADR-001 §7.9 asks for, and what "if we cannot say
//! why an episode is at 82, it should not be at 82" means as code.
//!
//! # Four properties the tests hold this to
//!
//! * **The printed parts sum to the printed total.** Terms are integer hundredths of a point, so
//!   `24.00 + 19.50 + …` is exact in base 10 and a reviewer with a calculator arrives at the same
//!   number. Float accumulation, which does not, is why the arithmetic is in hundredths.
//! * **Reproducible.** Same [`Vector`], same [`Weights`], same bytes. No clock, no locale, no hash
//!   iteration order, no HashMap in the path from measurement to printed line.
//! * **Monotonic.** Adding to a feature never lowers a score, and if `A` is at least `B` on every
//!   measured feature then `A` stays at or above `B` under *any* non-negative weights — including after
//!   a rename of the weights, which is the property that makes retuning on a calibration set safe to
//!   ship. `crates/fathom-analyze/tests/risk_score.rs` proves both with `proptest`.
//! * **Divided, not renormalised behind you.** `W` is over the measured features, so a missing feature
//!   redistributes its share instead of quietly lowering the score — and the printed line says `11 of 12
//!   features measured; denominator 0.91`, because a denominator the reader cannot see is a second
//!   hidden layer.

use serde::Serialize;

use super::feature::{Feature, Measurement, Vector};
use super::weights::Weights;

/// One hundredth of a point, and the unit the whole arithmetic happens in.
///
/// `Points(2400)` prints as `24.00`. Sums of these are exact, which is the only way "the parts add up to
/// the total" survives being a promise rather than a coincidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Points(pub i64);

impl std::fmt::Display for Points {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        write!(f, "{sign}{}.{:02}", magnitude / 100, magnitude % 100)
    }
}

/// One feature's contribution: the measurement, the weight, the points, and the sentence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Term {
    /// The feature.
    pub feature: Feature,
    /// The raw measurement in the feature's own unit, printed beside the value it produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<f64>,
    /// That value as printed, with the unit: `3 severity-points`.
    pub raw_shown: String,
    /// The normalised risk value, `0.0..=1.0`.
    pub value: f64,
    /// The weight in force.
    pub weight: f64,
    /// Where the weight came from, in one word: `default`, `fathom.toml`, `--weight`.
    pub origin: &'static str,
    /// The contribution, in points out of 100.
    pub points: Points,
    /// What this feature could have contributed had it measured `1.0` — the ceiling the points are
    /// spent out of, printed as `24.00/26.00` so a low score says "this axis was quiet" rather than
    /// "this axis was left out".
    pub cap: Points,
    /// What the measurement was taken from.
    pub basis: String,
    /// The claim a reviewer can check, naming paths and counts.
    pub reason: String,
}

/// A feature that could not be scored, and who owns the missing input.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Absent {
    /// The printed key.
    pub name: &'static str,
    /// Why it is missing, in the model's own words.
    pub why: String,
    /// The plan key that owns the input.
    pub owner: &'static str,
    /// The weight it would have carried — printed, because "worth 9 of your 100" is the reason to
    /// care that it is missing.
    pub weight: f64,
}

/// One episode's score, with everything that produced it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Score {
    /// The model's name ([`super::feature::MODEL`]).
    pub model: &'static str,
    /// The model's version.
    pub model_version: u32,
    /// The episode scored.
    pub episode: String,
    /// The score, `0..=100`. Meaningless unless [`Score::scored`] is true, and then it is exactly
    /// [`Score::terms`]' `points` summed.
    pub score: u8,
    /// Whether a score exists at all: false when nothing was measured or every weight is zero, both of
    /// which print as a refusal rather than a `0` a reader would read as "safe".
    pub scored: bool,
    /// Why there is no score, when there is not one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unscored: Option<String>,
    /// The contributions, largest first, then in [`Feature::ALL`]'s order.
    pub terms: Vec<Term>,
    /// The features that were not measured, in [`Feature::ALL`]'s order.
    pub absent: Vec<Absent>,
    /// How many of the twelve were measured.
    pub measured: usize,
    /// How many features the model has.
    pub features: usize,
    /// The denominator actually used: the weight sum over the measured features.
    pub denominator: f64,
    /// The exact sum of [`Term::points`], so the printed total can be checked at full precision.
    pub total: Points,
    /// The threshold that decides [`Score::flagged`].
    pub tau: u8,
    /// Whether this episode is at or over the threshold.
    pub flagged: bool,
    /// Anything the read had to caveat, from the collector.
    pub caveats: Vec<String>,
    /// What the weights file or flags could not honour.
    pub notices: Vec<String>,
}

impl Score {
    /// The contributions in the order a reviewer reads them: most points first.
    #[must_use]
    pub fn reasons(&self) -> &[Term] {
        &self.terms
    }

    /// `features-1 82 = findings 24.00 + surface 19.50 + migration 9.00`.
    ///
    /// The shape [`super::Risk::vector`] uses for `spine-1`, on purpose: the strip, the card's `w` and
    /// `fathom why` print one kind of sentence rather than two (ADR-006 §5.1's binding note).
    #[must_use]
    pub fn vector(&self) -> String {
        if !self.scored {
            return format!(
                "{} — no score: {}",
                self.model,
                self.unscored.as_deref().unwrap_or("nothing was measured")
            );
        }
        if self.terms.is_empty() {
            return format!("{} {} = no contribution", self.model, self.score);
        }
        let parts: Vec<String> = self
            .terms
            .iter()
            .map(|term| format!("{} {}", term.feature.id(), term.points))
            .collect();
        format!("{} {} = {}", self.model, self.score, parts.join(" + "))
    }

    /// The payload `fathom why --json` prints, minus the envelope the verb adds.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Score one episode.
///
/// Every feature in `vector` is either a term or an [`Absent`]; nothing is dropped, and the denominator
/// is the weight sum over the terms. A feature weighted `0` is *kept* with `0` points rather than
/// dropped from the denominator: switching a feature off by weight and discovering its measurement was
/// missing are two different statements, and the output keeps them apart.
///
/// The arithmetic is integer after one division per feature, so the printed sum is exact; the division
/// is by the *measured* weight sum, which is printed so the reader can redo it.
#[must_use]
pub fn score(vector: &Vector, weights: &Weights) -> Score {
    let measured: Vec<&Measurement> = vector
        .measurements
        .iter()
        .filter(|item| item.is_measured())
        .collect();
    let denominator: f64 = measured
        .iter()
        .map(|item| weights.weight(item.feature))
        .fold(0.0_f64, |total, weight| total + weight);

    // The denominator is printed, so it is also *stored* rounded: `0.83` rather than
    // `0.8300000000000003`, which a reader who takes out a calculator will otherwise think is a bug. The
    // rounded value is the one the arithmetic below divides by, so what is printed is what was used.
    let denominator = (denominator * 10_000.0).round() / 10_000.0;

    let mut terms: Vec<Term> = Vec::with_capacity(measured.len());
    let mut hundredths: i64 = 0;
    if denominator > 0.0 {
        for item in &measured {
            let weight = weights.weight(item.feature);
            let raw = item.raw.unwrap_or(0.0);
            // One division per feature, rounded half away from zero: `x.5` goes up, in every Rust build,
            // which is the direction `f64::round` documents and the one the printed examples use.
            let points = (10_000.0 * weight * item.value / denominator)
                .round()
                .max(0.0) as i64;
            let cap = (10_000.0 * weight / denominator).round().max(0.0) as i64;
            hundredths = hundredths.saturating_add(points);
            terms.push(Term {
                feature: item.feature,
                raw: item.raw,
                raw_shown: format!(
                    "{} {}",
                    item.feature.format_raw(raw),
                    item.feature.spec().unit
                ),
                value: item.value,
                weight,
                origin: super::weights::Origin::as_str(origin_of(weights, item.feature)),
                points: Points(points),
                cap: Points(cap),
                basis: item.basis.clone(),
                reason: item.detail.clone(),
            });
        }
    }
    hundredths = hundredths.min(10_000);
    terms.sort_by(|a, b| {
        b.points
            .cmp(&a.points)
            .then_with(|| index(a.feature).cmp(&index(b.feature)))
    });

    let absent: Vec<Absent> = vector
        .measurements
        .iter()
        .filter(|item| !item.is_measured())
        .map(|item| Absent {
            name: item.feature.id(),
            why: item.detail.clone(),
            owner: item.owner.unwrap_or(item.feature.spec().owner),
            weight: weights.weight(item.feature),
        })
        .collect();

    let scored = denominator > 0.0 && !measured.is_empty();
    let unscored = if scored {
        None
    } else if measured.is_empty() {
        Some("nothing was measured, so there is nothing to weigh".to_owned())
    } else {
        Some(
            "every measured feature is weighted 0, so the score has no denominator — `[risk.weights]` \
             has switched off everything this episode could report"
                .to_owned(),
        )
    };
    let total = Points(hundredths);
    let value = ((hundredths + 50) / 100).clamp(0, 100);
    // The rounding above is to the nearest point, so the printed integer can leave the exact sum by up
    // to half a point either way; clamp after the division, and print the exact sum beside it.
    let score = u8::try_from(value).unwrap_or(100);

    let mut caveats = vector.caveats.clone();
    if let Some(extra) = truncated_note(vector) {
        caveats.push(extra);
    }

    Score {
        model: super::feature::MODEL,
        model_version: super::feature::MODEL_VERSION,
        episode: vector.episode.clone(),
        score,
        scored,
        unscored,
        terms,
        absent,
        measured: measured.len(),
        features: vector.measurements.len(),
        denominator,
        total,
        tau: weights.tau,
        flagged: scored && score >= weights.tau,
        caveats,
        notices: weights.notices.clone(),
    }
}

/// A feature's position in the printed order, for a deterministic tie-break.
fn index(feature: Feature) -> usize {
    Feature::ALL
        .iter()
        .position(|candidate| *candidate == feature)
        .unwrap_or(Feature::ALL.len())
}

/// The origin of the weight a term used.
fn origin_of(weights: &Weights, feature: Feature) -> super::weights::Origin {
    weights
        .weights
        .iter()
        .find(|weight| weight.feature == feature)
        .map_or(super::weights::Origin::Default, |weight| weight.origin)
}

/// The caveat a capped file read earns, if it earned one.
///
/// The collector marks one in [`Vector::caveats`]; this is the shared sentence so the card, the verb and
/// a JSON consumer print the same words about the same hole.
fn truncated_note(vector: &Vector) -> Option<String> {
    vector
        .measurements
        .iter()
        .find(|item| item.basis.starts_with("truncated read"))
        .map(|item| item.detail.clone())
}

/// The lines `fathom why` prints, in the order a reviewer reads them: the number, the arithmetic, the
/// feature table, the reasons, and then what the model could not see.
///
/// It lives here rather than in the CLI for the reason every other renderer in this crate lives with its
/// model: the sentence "the score cannot see the module graph" must not become three slightly different
/// sentences in three surfaces.
#[must_use]
pub fn render(score: &Score) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "{} {}{}",
        score.model,
        if score.scored {
            score.score.to_string()
        } else {
            "—".to_owned()
        },
        if score.scored {
            if score.flagged {
                format!(" · flagged (score ≥ τ {})", score.tau)
            } else {
                format!(" · below τ {}", score.tau)
            }
        } else {
            String::new()
        }
    ));
    if !score.scored {
        out.push(format!(
            "  no score: {}",
            score.unscored.as_deref().unwrap_or("nothing was measured")
        ));
    }
    out.push(format!(
        "  {} of {} features measured · weight denominator {:.2} · total {} of 100.00 points",
        score.measured, score.features, score.denominator, score.total
    ));
    out.push(format!("  {}", score.vector()));

    out.push(String::new());
    out.push(format!(
        "{:<12} {:>18} {:>7} {:>7} {:>9} {}",
        "feature", "measured", "value", "weight", "points", "why"
    ));
    for term in &score.terms {
        out.push(format!(
            "{:<12} {:>18} {:>7.2} {:>7.2} {:>9} {} · {}",
            term.feature.id(),
            term.raw_shown,
            term.value,
            term.weight,
            format!("{}/{}", term.points, term.cap),
            term.reason,
            term.basis,
        ));
    }
    if !score.absent.is_empty() {
        out.push(format!(
            "{:<12} {:>18} {:>7} {:>7} {:>9} not measured, and not counted",
            "—", "—", "—", "—", "—"
        ));
        for absent in &score.absent {
            out.push(format!(
                "{:<12} {:>18} {:>7} {:>7.2} {:>9} {} · owned by {}",
                absent.name, "—", "—", absent.weight, "—", absent.why, absent.owner
            ));
        }
    }

    out.push(String::new());
    out.push("why, in order:".to_owned());
    for (place, term) in score.terms.iter().enumerate() {
        out.push(format!(
            " {}. {:>6} — {}",
            place + 1,
            term.points,
            term.reason
        ));
    }
    if score.terms.is_empty() {
        out.push("  nothing contributed: every measured feature came in at zero".to_owned());
    }

    for caveat in &score.caveats {
        out.push(format!("  caveat: {caveat}"));
    }
    for notice in &score.notices {
        out.push(format!("  config: {notice}"));
    }
    // The ceiling is printed with the score, not in a document somewhere else. A number without its
    // blind spots is a number a reader will over-trust, and the whole ticket is explanation.
    out.push(String::new());
    out.push(format!(
        "what {} cannot see ({} things):",
        score.model,
        super::ceiling().len()
    ));
    out.extend(super::ceiling().iter().map(|line| format!("  · {line}")));
    out
}

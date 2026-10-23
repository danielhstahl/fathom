//! The knobs: five weights and four published numbers, each with its origin recorded
//! (`e3.9`, `reco-al1`).
//!
//! A scorecard that cannot name its weights is a mood, not a measurement, so every knob here has a
//! default, a range, a unit and an [`Origin`] recorded beside every value it ends up with — the same
//! discipline [`crate::hotspots::policy`] and [`crate::risk::weights`] apply to their own scores. The
//! knobs are read from `[scorecard]` in `fathom.toml` by [`policy_from_config_text`], which never
//! fails: a knob that will not parse is reported and the default stands, because a typo in a weight
//! must not turn a read into an error.
//!
//! # What is deliberately *not* a knob
//!
//! **The curve shapes.** A share is `n / d` and a knee is `n / (n + k)`; those two formulas *are* the
//! model, and making them configurable would put the thing the acceptance criterion says a reader can
//! reproduce by hand behind a config file. Only the inputs to the curves are tunable: the weights and
//! the four published numbers (`coupling_ceiling`, `api_knee`, `hotspot_bar`, `alert_below`).
//!
//! **A threshold that decides an exit code.** The CI gate is `fathom check` (**reco-ngk**). This module
//! reports [`Policy::alert_below`] and whether the score sits under it, and the gate decides what that
//! means for a pipeline. Two places deciding "under 50 is a failure" is how a gate and a dashboard end
//! up disagreeing about the same run.

use serde::Serialize;

use super::dimension::{Dimension, Origin};

/// The window, in revisions, a per-window score is computed over.
///
/// Thirty is the window every other "recent" measure in this product already uses — the heat map in
/// `crates/fathom-cli/src/architecture.rs` and `hotspots.window` — and a second, different number for
/// the same word would be two answers to one question.
pub const DEFAULT_WINDOW: usize = 30;

/// How many points of score movement still counts as *no movement*.
///
/// Two out of a hundred: the arithmetic is exact, but a score is a ratio over a graph that moves one
/// module at a time, and a reviewer told "the architecture moved 1 point this week" is being invited to
/// read noise as news. The band is printed beside the verdict, never applied silently.
pub const DEFAULT_FLAT_WITHIN: i64 = 2;

/// The default weight of each dimension, in [`Dimension::ALL`] order.
///
/// Sum: 1.00. Boundaries and acyclicity carry the most, because they are the two dimensions with a
/// *written rule* behind them — a violated decision and a violated acyclicity are findings a reviewer
/// can act on, while coupling and hotspot pressure describe a shape that may be entirely deliberate.
/// API hygiene is smallest because this build measures it at the grain of the module graph rather than
/// the type-checker's (see [`super::dimension::ApiSignal`]), and a coarse signal should not be handed a
/// fine share of the verdict.
pub const DEFAULT_WEIGHTS: [f64; 5] = [0.30, 0.15, 0.25, 0.15, 0.15];

/// The knobs in force for one scorecard.
///
/// Serialized only. A stored scorecard carries the *weights it was scored with* in its own terms
/// ([`super::score::Term::weight`]), which is the copy a reader needs; re-reads of the knobs come from
/// `fathom.toml`, where they belong, and deserialising a policy from an old artefact would be a config
/// layer nobody wrote.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Policy {
    /// One weight per dimension, in [`Dimension::ALL`] order.
    pub weights: [f64; 5],
    /// Where each weight came from, same order: `default` or `fathom.toml`.
    pub origins: [Origin; 5],
    /// The window, in revisions, a per-window score covers.
    pub window: usize,
    /// How many points of movement is still "flat".
    pub flat_within: i64,
    /// How many revisions a printed trend series may hold before older ones are thinned
    /// ([`super::trend`]).
    pub trend_cap: usize,
    /// Mean dependency edges per module that scores as fully coupled.
    pub coupling_ceiling: f64,
    /// Vanished exports that score API hygiene at a half.
    pub api_knee: f64,
    /// The hotspot score at or above which a module counts as pressure.
    pub hotspot_bar: u8,
    /// Report `alert: true` below this score. Not an exit code — see the module doc.
    pub alert_below: u8,
    /// Every knob this file could not honour, in the order it was found.
    pub notices: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Policy {
    /// The built-in defaults, all five weights untouched.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            weights: DEFAULT_WEIGHTS,
            origins: [Origin::Default; 5],
            window: DEFAULT_WINDOW,
            flat_within: DEFAULT_FLAT_WITHIN,
            trend_cap: 240,
            coupling_ceiling: 8.0,
            api_knee: 4.0,
            hotspot_bar: 70,
            alert_below: 50,
            notices: Vec::new(),
        }
    }

    /// The weight in force for one dimension.
    #[must_use]
    pub fn weight(&self, dimension: Dimension) -> f64 {
        self.weights[dimension.index()]
    }

    /// Where that weight came from.
    #[must_use]
    pub fn origin(&self, dimension: Dimension) -> Origin {
        self.origins[dimension.index()]
    }

    /// Set one dimension's weight, recording where it came from.
    ///
    /// Out-of-range values are refused with the reason rather than clamped silently:
    /// `scorecard.weight.boundaries = 3` is a typo, and saying so is cheaper than scoring with it.
    pub fn set_weight(
        &mut self,
        dimension: Dimension,
        value: f64,
        origin: Origin,
    ) -> Result<(), String> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(format!(
                "`scorecard.weight.{}` must be a number between 0.0 and 1.0; {value} was ignored",
                dimension.id()
            ));
        }
        let at = dimension.index();
        self.weights[at] = value;
        self.origins[at] = origin;
        Ok(())
    }

    /// The sum of the weights over *every* dimension — the number a reader compares the printed
    /// denominator against, and the reason a dimension weighted to zero is visible.
    #[must_use]
    pub fn weight_sum(&self) -> f64 {
        self.weights
            .iter()
            .fold(0.0_f64, |total, weight| total + weight)
    }

    /// Whether every weight is still the built-in one — printed, so a scorecard that was tuned says so.
    #[must_use]
    pub fn untouched(&self) -> bool {
        self.weights == DEFAULT_WEIGHTS
    }

    /// The knobs as lines: what the report prints beside the score.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for dimension in Dimension::ALL {
            out.push(format!(
                "  weight {:<11} {:.2} ({})",
                dimension.id(),
                self.weight(dimension),
                self.origin(dimension).as_str()
            ));
        }
        out.push(format!(
            "  window {} revision(s) · flat within {} point(s) · trend cap {} point(s)",
            self.window, self.flat_within, self.trend_cap
        ));
        out.push(format!(
            "  coupling_ceiling {} edges/module · api_knee {} vanished export(s) · hotspot_bar {} · \
             alert_below {}",
            self.coupling_ceiling, self.api_knee, self.hotspot_bar, self.alert_below
        ));
        out
    }

    /// The exported form — every knob, so a stored scorecard can be read a year later without the
    /// config that produced it.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "weights": Dimension::ALL
                .iter()
                .map(|dimension| serde_json::json!({
                    "dimension": dimension.id(),
                    "weight": self.weight(*dimension),
                    "origin": self.origin(*dimension).as_str(),
                    "default": dimension.default_weight(),
                }))
                .collect::<Vec<_>>(),
            "weight_sum": (self.weight_sum() * 10_000.0).round() / 10_000.0,
            "untouched": self.untouched(),
            "window": self.window,
            "flat_within": self.flat_within,
            "trend_cap": self.trend_cap,
            "coupling_ceiling": self.coupling_ceiling,
            "api_knee": self.api_knee,
            "hotspot_bar": self.hotspot_bar,
            "alert_below": self.alert_below,
            "notices": self.notices,
        })
    }
}

/// Read `[scorecard]` out of a `fathom.toml`.
///
/// Never fails. A document that does not parse, a knob of the wrong type, or a value outside its range
/// each arrive as a notice beside the default, which is the rule every other `[section]` reader in this
/// crate follows (`[layering]`, `[hotspots]`, `[api]`).
#[must_use]
pub fn policy_from_config_text(text: &str) -> (Policy, Vec<String>) {
    let mut policy = Policy::defaults();
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(err) => {
            policy.notices.push(format!(
                "fathom.toml does not parse, so every scorecard knob stayed at its built-in default: \
                 {err}"
            ));
            let notices = std::mem::take(&mut policy.notices);
            return (policy, notices);
        }
    };
    let Some(table) = document.get("scorecard").and_then(|item| item.as_table()) else {
        let notices = std::mem::take(&mut policy.notices);
        return (policy, notices);
    };

    if let Some(weights) = table.get("weight").and_then(|item| item.as_table()) {
        for dimension in Dimension::ALL {
            let Some(value) = weights.get(dimension.id()) else {
                continue;
            };
            // `0.3`, `"0.3"` and `1` all mean the same thing to a person, so all three are accepted;
            // anything else is reported rather than coerced into a weight.
            let number = value
                .as_float()
                .or_else(|| value.as_integer().map(|value| value as f64))
                .or_else(|| {
                    value
                        .as_str()
                        .and_then(|text| text.trim().parse::<f64>().ok())
                });
            match number {
                Some(number) => {
                    if let Err(why) = policy.set_weight(dimension, number, Origin::Repo) {
                        policy.notices.push(why);
                    }
                }
                None => policy.notices.push(format!(
                    "`scorecard.weight.{}` is not a number, so its default stands",
                    dimension.id()
                )),
            }
        }
    }

    // Each knob: absent means default, wrong type is a notice, out-of-range is a notice. The four
    // refusals are written out because "silently kept the default" and "used your value" must be
    // distinguishable in the report.
    match integer(table, "window") {
        Knob::Absent => {}
        Knob::Refused(why) => policy.notices.push(why),
        Knob::Value(value) => {
            if value == 0 {
                policy.notices.push(
                    "`scorecard.window` of 0 revisions is refused; the default stands".to_owned(),
                );
            } else {
                policy.window = usize::try_from(value).unwrap_or(usize::MAX);
            }
        }
    }
    match integer(table, "flat_within") {
        Knob::Absent => {}
        Knob::Refused(why) => policy.notices.push(why),
        Knob::Value(value) => policy.flat_within = value.max(0),
    }
    match integer(table, "trend_cap") {
        Knob::Absent => {}
        Knob::Refused(why) => policy.notices.push(why),
        Knob::Value(value) => {
            if value == 0 {
                policy.notices.push(
                    "`scorecard.trend_cap` of 0 would print no series; the default stands"
                        .to_owned(),
                );
            } else {
                policy.trend_cap = usize::try_from(value).unwrap_or(usize::MAX);
            }
        }
    }
    match number(table, "coupling_ceiling") {
        Knob::Absent => {}
        Knob::Refused(why) => policy.notices.push(why),
        Knob::Value(value) if value <= 0.0 => policy
            .notices
            .push("`scorecard.coupling_ceiling` must be above zero; the default stands".to_owned()),
        Knob::Value(value) => policy.coupling_ceiling = value,
    }
    match number(table, "api_knee") {
        Knob::Absent => {}
        Knob::Refused(why) => policy.notices.push(why),
        Knob::Value(value) if value <= 0.0 => policy
            .notices
            .push("`scorecard.api_knee` must be above zero; the default stands".to_owned()),
        Knob::Value(value) => policy.api_knee = value,
    }
    match integer(table, "hotspot_bar") {
        Knob::Absent => {}
        Knob::Refused(why) => policy.notices.push(why),
        Knob::Value(value) if value > 100 => policy
            .notices
            .push("`scorecard.hotspot_bar` is a 0–100 score; the default stands".to_owned()),
        Knob::Value(value) => policy.hotspot_bar = value.max(0) as u8,
    }
    match integer(table, "alert_below") {
        Knob::Absent => {}
        Knob::Refused(why) => policy.notices.push(why),
        Knob::Value(value) if value > 100 => policy
            .notices
            .push("`scorecard.alert_below` is a 0–100 score; the default stands".to_owned()),
        Knob::Value(value) => policy.alert_below = value.max(0) as u8,
    }

    let notices = std::mem::take(&mut policy.notices);
    (policy, notices)
}

/// What one knob said: nothing, something unusable, or a value.
enum Knob<T> {
    Absent,
    Refused(String),
    Value(T),
}

/// One integer knob, with "absent means default" and "wrong type is a notice" in one place.
fn integer(table: &toml_edit::Table, key: &str) -> Knob<i64> {
    match table.get(key) {
        None => Knob::Absent,
        Some(value) => match value.as_integer() {
            Some(number) => Knob::Value(number),
            None => Knob::Refused(format!(
                "`scorecard.{key}` is not an integer, so its default stands"
            )),
        },
    }
}

/// One float knob.
fn number(table: &toml_edit::Table, key: &str) -> Knob<f64> {
    match table.get(key) {
        None => Knob::Absent,
        Some(value) => match value
            .as_float()
            .or_else(|| value.as_integer().map(|value| value as f64))
        {
            Some(number) => Knob::Value(number),
            None => Knob::Refused(format!(
                "`scorecard.{key}` is not a number, so its default stands"
            )),
        },
    }
}

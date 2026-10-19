//! The weights, where they come from, and what this build refused to use (`reco-884`).
//!
//! A weight is one number per feature, `0.0..=1.0`, and a repository sets them in `fathom.toml`:
//!
//! ```toml
//! [risk]
//! tau = 70
//!
//! [risk.weights]
//! findings = 0.22
//! migration = 0.14
//! size = 0.04
//! ```
//!
//! # Three rules, each of which is a refusal
//!
//! * **Clamped, never normalised behind your back.** A value outside `0.0..=1.0` is clamped to that
//!   range and the clamp is *printed*, not applied quietly. The weights are not renormalised to sum to
//!   one either — the score divides by the weight sum over the measured features ([`super::score`]),
//!   so raising one weight moves the score toward that feature instead of shrinking every other one,
//!   which is the behaviour a person editing one line expects.
//! * **An unknown key is refused loudly.** `[risk.weights]` is an open map in the config layer
//!   (`risk.weights.<weight>`), so `comms = 0.2` resolves happily and then goes nowhere: this parser
//!   notices, names the valid keys, and says the value it ignored. A typo that silently renames a
//!   feature is the same failure as a typo in a `CI` secret name — invisible until the thing it was
//!   guarding arrives.
//! * **Every value carries where it came from.** `default`, `fathom.toml` or `--weight`. `fathom why`
//!   prints the origin beside each weight, because "your score moved because of a line in a file you
//!   wrote two months ago" has to be reconstructible from the output and not from git blame.
//!
//! The parser takes *text*, not a path: ADR-002 §4.5 forbids a crate below `fathom-cli` from opening a
//! config file, so the CLI reads `fathom.toml` and hands the text down — the same shape
//! [`crate::detectors::policy::policy_from_toml`] uses.

use serde::Serialize;

use super::feature::{Feature, MAX_WEIGHT};

/// Where a weight came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The built-in default in [`Feature::default_weight`].
    Default,
    /// The repository's `fathom.toml`.
    Repo,
    /// A `--weight name=value` on the command line.
    Flag,
}

impl Origin {
    /// The printed spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Origin::Default => "default",
            Origin::Repo => "fathom.toml",
            Origin::Flag => "--weight",
        }
    }
}

/// One feature's effective weight, with the history of how it got there.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Weight {
    /// The feature.
    pub feature: Feature,
    /// The value the arithmetic uses, already clamped.
    pub value: f64,
    /// Which layer won.
    pub origin: Origin,
    /// The value as written, when it was outside `0.0..=1.0` and got clamped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clamped_from: Option<f64>,
}

/// The twelve effective weights, and everything this build could not use.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Weights {
    /// One per feature, in [`Feature::ALL`]'s order.
    pub weights: Vec<Weight>,
    /// The flag that routes a flagged episode: score ≥ tau. A threshold, not a weight, and the
    /// existing `risk.tau` key (`e2.10`) rather than a second one.
    pub tau: u8,
    /// One line per value that was read and could not be used, naming the key and what stood instead.
    pub notices: Vec<String>,
    /// The file the values came from, when there was one — printed so a score names its config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl Default for Weights {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Weights {
    /// The built-in weights, for a repository that configures nothing.
    #[must_use]
    pub fn defaults() -> Self {
        Weights {
            weights: Feature::ALL
                .iter()
                .map(|feature| Weight {
                    feature: *feature,
                    value: feature.default_weight(),
                    origin: Origin::Default,
                    clamped_from: None,
                })
                .collect(),
            tau: DEFAULT_TAU,
            notices: Vec::new(),
            source: None,
        }
    }

    /// One feature's effective weight.
    #[must_use]
    pub fn weight(&self, feature: Feature) -> f64 {
        self.weights
            .iter()
            .find(|weight| weight.feature == feature)
            .map_or(0.0, |weight| weight.value)
    }

    /// Set one weight, clamping and recording the origin. `Err` names the reason a name was not a
    /// feature, so a CLI can turn it into a usage error rather than a shrug.
    pub fn set(&mut self, name: &str, value: f64, origin: Origin) -> Result<(), String> {
        let Some(feature) = Feature::parse(name) else {
            return Err(format!(
                "`{name}` is not a feature of this vector; the twelve are {}",
                Feature::ids().join(", ")
            ));
        };
        let clamped = if value.is_finite() {
            value.clamp(0.0, MAX_WEIGHT)
        } else {
            self.notices.push(format!(
                "risk.weights.{} is not a number, so its previous value stands",
                feature.id()
            ));
            return Ok(());
        };
        let index = self
            .weights
            .iter()
            .position(|weight| weight.feature == feature);
        match index {
            Some(index) => {
                let slot = &mut self.weights[index];
                slot.clamped_from = if (clamped - value).abs() > f64::EPSILON {
                    Some(value)
                } else {
                    None
                };
                slot.value = clamped;
                slot.origin = origin;
            }
            // Only reachable from a `Weights` built by hand with a feature missing; `defaults()` always
            // fills all twelve, and a hand-built one gets the feature appended rather than losing it.
            None => self.weights.push(Weight {
                feature,
                value: clamped,
                origin,
                clamped_from: if (clamped - value).abs() > f64::EPSILON {
                    Some(value)
                } else {
                    None
                },
            }),
        }
        Ok(())
    }

    /// The sum over a set of features — the score's denominator ([`super::score`]).
    #[must_use]
    pub fn sum(&self, features: &[Feature]) -> f64 {
        features
            .iter()
            .map(|feature| self.weight(*feature))
            .fold(0.0_f64, |total, weight| total + weight)
    }

    /// How many of the twelve still carry their built-in value.
    #[must_use]
    pub fn untouched(&self) -> usize {
        self.weights
            .iter()
            .filter(|weight| weight.origin == Origin::Default)
            .count()
    }

    /// The weights as a `fathom.toml` block — what `fathom why --calibrate` proposes, and what a
    /// reviewer pastes when the calibration report convinced them. Only the features that differ from
    /// the built-in default are written: a config file that restates every default is a config file
    /// whose meaning is invisible.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        if self.tau != DEFAULT_TAU {
            out.push_str(&format!("[risk]\ntau = {}\n", self.tau));
        }
        out.push_str("[risk.weights]\n");
        let mut wrote = 0_usize;
        for weight in &self.weights {
            if weight.origin == Origin::Default && weight.clamped_from.is_none() {
                continue;
            }
            out.push_str(&format!("{} = {:.2}\n", weight.feature.id(), weight.value));
            wrote += 1;
        }
        if wrote == 0 {
            out.push_str("# every weight is at its built-in default\n");
        }
        out
    }
}

/// `risk.tau`, as declared in the key spec (`e2.10`), which is `70` and stays `70`.
///
/// The honest reason it stays: the key is shared with `spine-1`, whose scores run high, and the Python
/// reference policy (`src/recommender/card/policy.py`) declares `70` — one key, one default, whatever the
/// model. What `e4.4` adds is the means to *check* it: on the calibration corpus, `features-1` scores the
/// worst labelled case 45, so a threshold of `70` flags nothing, and `fathom why --calibrate` says so in
/// numbers and proposes the corpus's own decile floor instead. That is the ticket's whole point — a knob
/// change evaluated rather than felt, including this one.
pub const DEFAULT_TAU: u8 = 70;

/// `fathom.toml`'s `[risk]` and `[risk.weights]`, read.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    /// The weights that resulted.
    pub weights: Weights,
    /// One line per key that was read and could not be honoured.
    ///
    /// The same lines are on [`Weights::notices`], because every consumer holds a `Weights` and not this
    /// struct — the score, the JSON payload and the printed table all read the copy that travels with the
    /// weights, and a notice that only the parser can see is a notice nobody prints.
    pub notices: Vec<String>,
}

impl Resolved {
    /// Record one refusal, in both the places a reader can look for it.
    ///
    /// Public because the CLI adds the notices for what *it* overrode — `--weight`, `--tau` — and a
    /// reviewer comparing a score against a `fathom.toml` needs one list, not two.
    pub fn notice(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.notices.push(text.clone());
        self.weights.notices.push(text);
    }
}

/// Read `[risk]` and `[risk.weights]` out of `fathom.toml` text.
///
/// A file that does not parse is a notice and the defaults — the same rule every other reader of
/// `fathom.toml` in this workspace follows, and for the same reason: a config that fails to parse and
/// then analyses like an empty one is a silent behaviour change. A table with one bad value is a notice
/// and the rest of the table.
///
/// Keys outside the twelve are *not* an error to the config layer (the key spec declares
/// `risk.weights.<weight>` as a deep open map, so `e2.9`'s `comms` still resolves); they are a notice
/// here, because this is the layer that knows what a feature is.
#[must_use]
pub fn weights_from_toml(text: &str) -> Resolved {
    let mut out = Resolved {
        weights: Weights::defaults(),
        notices: Vec::new(),
    };
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(err) => {
            out.notice(format!(
                "fathom.toml does not parse, so every weight stayed at its built-in default: {err}"
            ));
            return out;
        }
    };
    let Some(table) = document.get("risk").and_then(toml_edit::Item::as_table) else {
        return out;
    };

    if let Some(value) = table.get("tau") {
        match value.as_value().and_then(toml_edit::Value::as_integer) {
            Some(value) => match u8::try_from(value) {
                Ok(tau) => out.weights.tau = tau,
                Err(_) => out.notice(format!(
                    "risk.tau = {value} is outside 0..=100, so {} stands",
                    out.weights.tau
                )),
            },
            None => out
                .notices
                .push("risk.tau is not an integer, so it was not used".to_owned()),
        }
    }

    let weights = match table.get("weights") {
        Some(item) => item,
        None => return out,
    };
    // Both spellings of the same table: `[risk.weights]` is an item table, `weights = { … }` under
    // `[risk]` is an inline one, and a person editing config will write either.
    let pairs: Vec<(String, toml_edit::Item)> = match weights {
        toml_edit::Item::Table(table) => table
            .iter()
            .map(|(key, item)| (key.to_owned(), item.clone()))
            .collect(),
        toml_edit::Item::Value(toml_edit::Value::InlineTable(inline)) => inline
            .iter()
            .map(|(key, value)| (key.to_owned(), toml_edit::Item::Value(value.clone())))
            .collect(),
        _ => {
            out.notices
                .push("risk.weights is not a table, so no weight was read from it".to_owned());
            return out;
        }
    };

    for (key, item) in pairs {
        let Some(value) = item.as_value() else {
            out.notice(format!(
                "risk.weights.{key} is not a value, so it was not read"
            ));
            continue;
        };
        // A TOML integer is exact to 2^53 as an f64, and a weight is nowhere near that; the cast is the
        // one place this file widens a number and it is worth saying why it is safe.
        let Some(number) = value
            .as_float()
            .or_else(|| value.as_integer().map(|value| value as f64))
        else {
            out.notice(format!(
                "risk.weights.{key} is not a number, so it was not read"
            ));
            continue;
        };
        if let Err(reason) = out.weights.set(&key, number, Origin::Repo) {
            out.notice(format!("risk.weights.{key}: {reason}"));
            continue;
        }
        if let Some(weight) = out
            .weights
            .weights
            .iter()
            .find(|weight| weight.feature.id() == key)
            .filter(|weight| weight.clamped_from.is_some())
        {
            // `set` already clamped and recorded the original; the notice is printed here because this is
            // the layer that knows which key in the file asked for it.
            let from = weight.clamped_from.unwrap_or(0.0);
            out.notice(format!(
                "risk.weights.{key} = {from} is outside 0.0..={MAX_WEIGHT}, and was clamped to {}",
                weight.value
            ));
        }
    }
    out
}

/// Parse one `--weight name=value` token.
pub fn parse_weight_flag(token: &str) -> Result<(String, f64), String> {
    let Some((name, value)) = token.split_once('=') else {
        return Err(format!(
            "`{token}` is not name=value; the shape is --weight findings=0.2"
        ));
    };
    let trimmed = value.trim();
    let parsed: f64 = trimmed
        .parse()
        .map_err(|_| format!("`{value}` in `{token}` is not a number between 0 and 1"))?;
    Ok((name.trim().to_owned(), parsed))
}

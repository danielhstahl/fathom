//! The five dimensions, each with the two numbers it is a ratio of and the sentence it earned
//! (`e3.9`, `reco-al1`).
//!
//! The ticket names five: **boundaries respected, coupling, acyclicity, API hygiene, hotspot pressure**.
//! Each one here is a *measurement*, never a verdict: a numerator, a denominator or a published knee,
//! the basis it was taken from, and — when it could not be taken — the reason. That split is what makes
//! the scorecard's arithmetic the same arithmetic [`crate::risk::score`] and [`crate::hotspots::rank`]
//! use, and it is what lets the acceptance criterion "reproducible by hand from exported JSON" be
//! something a test can assert rather than a claim nobody can check.
//!
//! # The two curves, and why there are only two
//!
//! ```text
//! share    badness = n / d            n findings, d things examined   (d > 0 required)
//! knee     badness = n / (n + k)      n findings, k the published knee (n = 0 → 0.0)
//! ```
//!
//! A **share** is used wherever there is an honest denominator: violations per dependency row examined,
//! nodes in live cycles per node, hot modules per module scored. A **knee** is used where there is no
//! denominator that would not be invented: there is no meaningful "public API surface this episode could
//! have broken" count that a coarse read of the module graph can supply, so a count is bent by a
//! published knee instead — `api_knee = 4` vanished exports scores API hygiene at exactly a half.
//!
//! Both curves are monotone in `n`, both land in `0.0..=1.0`, and both print the integers they were
//! computed from, which is the whole requirement. Adding a third curve would mean a reader had to learn
//! three shapes to check one scorecard.
//!
//! # Unmeasured is not zero
//!
//! A dimension that could not be measured is `measured: false` with a reason, and [`super::score`]
//! redistributes its weight across the dimensions that *were* measured and prints the new denominator.
//! Every one of those cases is a real state of a real repository:
//!
//! | dimension | when it says nothing |
//! |---|---|
//! | boundaries | no `[layering]` rules are declared, or the revision has no dependency rows to judge |
//! | coupling | no modules in the graph, or a chain that would not materialise |
//! | acyclicity | no modules to ask about |
//! | API hygiene | no earlier revision to compare against — nothing arrived and nothing left |
//! | hotspot pressure | nothing was ranked (a graph of one revision has no velocity) |
//!
//! Printing `0` for "no rules declared" would be the single most misleading number on the page: it
//! reads as a repository that respects its boundaries, and it means a repository that never wrote any
//! down. This is `e3.6`'s own complaint about "0 dependency row(s) checked" and `e3.8`'s about an unread
//! file ranked as tiny; the scorecard inherits both verdicts rather than restating them.
//!
//! # Where the numbers come from
//!
//! Nothing here reads a store, a file, a clock or an environment (ADR-002 §4.5). Each constructor takes
//! the *report another module already exported* — [`crate::layering::Report`], [`crate::cycles::Report`],
//! [`crate::hotspots::Table`], a materialised [`Graph`] and a [`crate::api`] diff's totals — so the
//! scorecard never runs a second analysis to answer a question one has already answered (lint `M03`).

use serde::{Deserialize, Serialize};

use crate::api::diff::Diff;
use crate::cycles::report::Report as CycleReport;
use crate::graph::model::{Graph, RowKind};
use crate::graph::query::Naming;
use crate::hotspots::Table as HotspotTable;
use crate::layering::report::Report as LayerReport;

use super::policy::Policy;

pub use crate::hotspots::model::Origin;

/// The five dimensions, in printed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Dimension {
    /// What the declared architecture allows, and what this revision does anyway.
    Boundaries,
    /// How much of the graph each module is wired to.
    Coupling,
    /// How much of the graph sits inside a loop nobody budgeted.
    Acyclicity,
    /// What this episode did to the contracts other code compiles against.
    Api,
    /// How many modules are expensive to change and widely felt.
    Hotspots,
}

impl Dimension {
    /// Every dimension, in the order a report prints them.
    pub const ALL: [Dimension; 5] = [
        Dimension::Boundaries,
        Dimension::Coupling,
        Dimension::Acyclicity,
        Dimension::Api,
        Dimension::Hotspots,
    ];

    /// The machine name — the key in the JSON and in `fathom.toml`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Dimension::Boundaries => "boundaries",
            Dimension::Coupling => "coupling",
            Dimension::Acyclicity => "acyclicity",
            Dimension::Api => "api",
            Dimension::Hotspots => "hotspots",
        }
    }

    /// The human label.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Dimension::Boundaries => "boundaries respected",
            Dimension::Coupling => "coupling",
            Dimension::Acyclicity => "acyclicity",
            Dimension::Api => "API hygiene",
            Dimension::Hotspots => "hotspot pressure",
        }
    }

    /// The question the dimension answers, in its own words — printed so a reader who has forgotten the
    /// model does not have to leave the report to remember it.
    #[must_use]
    pub const fn question(self) -> &'static str {
        match self {
            Dimension::Boundaries => {
                "does this revision respect the layering rules this repository wrote down?"
            }
            Dimension::Coupling => "how much of the graph does an average module reach?",
            Dimension::Acyclicity => "how much of the graph is inside a loop that nobody budgeted?",
            Dimension::Api => {
                "what did this episode do to the contracts other code compiles against?"
            }
            Dimension::Hotspots => "how many modules are expensive to change and widely felt?",
        }
    }

    /// Which report supplied it, named the way that report names itself.
    #[must_use]
    pub const fn source(self) -> &'static str {
        match self {
            Dimension::Boundaries => "e3.6 layering report (reco-cfp)",
            Dimension::Coupling => "e3.2 module graph (reco-8s4)",
            Dimension::Acyclicity => "e3.5 cycle report (reco-8md)",
            Dimension::Api => "e3.7 API surface (reco-p5u), or the module graph at export grain",
            Dimension::Hotspots => "e3.8 hotspot table (reco-wky)",
        }
    }

    /// Its index in [`Policy::weights`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Dimension::Boundaries => 0,
            Dimension::Coupling => 1,
            Dimension::Acyclicity => 2,
            Dimension::Api => 3,
            Dimension::Hotspots => 4,
        }
    }

    /// The built-in weight, for a report that prints `yours vs default`.
    #[must_use]
    pub fn default_weight(self) -> f64 {
        super::policy::DEFAULT_WEIGHTS[self.index()]
    }

    /// The curve shape and its published constant, as one line, before any data exists. A measured report
    /// prints the two integers it divided (`Curve::formula`); this prints the *shape*, so `--explain` can
    /// state the arithmetic of a repository it has not scored yet.
    #[must_use]
    pub fn curve_hint(self, policy: &Policy) -> String {
        match self {
            Dimension::Boundaries => {
                "share: violations / checked dependency edges (refused when nothing was checked)"
                    .to_owned()
            }
            Dimension::Coupling => format!(
                "share: dependency edges / (modules x {:.2} edges per module)",
                policy.coupling_ceiling
            ),
            Dimension::Acyclicity => {
                "share: modules inside an unaccepted cycle / modules".to_owned()
            }
            Dimension::Api => format!(
                "knee: n / (n + {:.2}), n = exports a module stopped exporting",
                policy.api_knee
            ),
            Dimension::Hotspots => format!(
                "share: modules scoring {} or above / scored modules",
                policy.hotspot_bar
            ),
        }
    }

    /// This dimension as a paragraph a reviewer can read: the question, where the raw number comes from,
    /// the curve with its published constant, and the weight in force beside the default it was changed
    /// from. `fathom architecture --explain` prints these five paragraphs, generated from the same
    /// constants the arithmetic uses — a formula page that drifted from the code would be worse than no
    /// page, and drift is what a reader trips over first.
    #[must_use]
    pub fn explain(self, policy: &Policy) -> String {
        let weight = policy.weight(self);
        let moved = if (weight - self.default_weight()).abs() < f64::EPSILON {
            String::new()
        } else {
            format!(" (default {})", round2(self.default_weight()))
        };
        format!(
            "{name} — {question}\n    source: {source}\n    curve: {curve}\n    weight: {weight}{moved}",
            name = self.id(),
            question = self.question(),
            source = self.source(),
            curve = self.curve_hint(policy),
            weight = round2(weight),
            moved = moved,
        )
    }

    /// The whole scale as paragraphs — the five dimensions in their printed order, then the total's
    /// arithmetic in one sentence. This is the text behind `fathom architecture --explain`: one place, so a
    /// ticket that changes a constant changes the explanation whether anyone remembers to edit the prose
    /// or not.
    #[must_use]
    pub fn explain_all(policy: &Policy) -> Vec<String> {
        let mut lines: Vec<String> = Dimension::ALL.iter().map(|d| d.explain(policy)).collect();
        lines.push(format!(
            "total — health = 1 - badness; points = round(weight / measured weight sum x health), summed as hundredths; score = points / 100.\n    denominator: {} — the weights of the measured dimensions only, so an unmeasured dimension scores nothing and flatters nobody\n    flat band: a move of {} point(s) or less is reported as flat\n    alert: a score below {} is reported as an alert, by `fathom architecture` and never as an exit code — `fathom check` owns pass and fail",
            round2(policy.weight_sum()),
            policy.flat_within,
            policy.alert_below
        ));
        lines
    }

    /// Parse a dimension name. `None` for anything else, so a typo in `fathom.toml` is refused rather
    /// than guessed at.
    pub fn parse(text: &str) -> Option<Self> {
        Dimension::ALL.into_iter().find(|d| d.id() == text)
    }
}

/// One dimension's curve: the two integers, or the count and the knee.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "curve", rename_all = "lowercase")]
pub enum Curve {
    /// `n / d`, over `d` things examined. Refused when `d` is zero, which is a *stated absence*.
    Share {
        /// The findings counted.
        numerator: i64,
        /// The things examined — the denominator the number is a share *of*.
        denominator: i64,
    },
    /// `n / (n + k)`, for a finding whose honest denominator is not knowable at this grain.
    Knee {
        /// The findings counted.
        count: i64,
        /// The published knee: the count at which the curve is exactly a half.
        knee: f64,
    },
}

impl Curve {
    /// The curve's name in the JSON and the prose.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Curve::Share { .. } => "share",
            Curve::Knee { .. } => "knee",
        }
    }

    /// The finding count, whichever curve it is on.
    #[must_use]
    pub const fn count(&self) -> i64 {
        match self {
            Curve::Share { numerator, .. } => *numerator,
            Curve::Knee { count, .. } => *count,
        }
    }

    /// The arithmetic as one line, in the order a reader will redo it: `3 / 412 = 0.00728`.
    #[must_use]
    pub fn formula(&self) -> String {
        match self {
            Curve::Share {
                numerator,
                denominator,
            } => format!(
                "{numerator} / {denominator} = {}",
                round4(
                    f64::from(i32::try_from(*numerator).unwrap_or(i32::MAX))
                        / f64::from(i32::try_from(*denominator).unwrap_or(i32::MAX).max(1))
                )
            ),
            Curve::Knee { count, knee } => format!(
                "{count} / ({count} + {}) = {}",
                round2(*knee),
                badness_of(self)
            ),
        }
    }
}

/// One dimension's measurement, with everything needed to print and re-check it.
///
/// The badness is a *badness*: `0.0` is nothing found, `1.0` is as bad as this curve can say. Health —
/// the number the score is built from — is [`Measurement::health`], and both are exported, because a
/// reader who is checking the arithmetic needs the one they divided and a reader skimming the card needs
/// the one that goes up when things get better.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    /// Which dimension this is.
    pub dimension: Dimension,
    /// The curve and its integers. `None` when nothing could be measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
    /// Whether a measurement exists at all.
    pub measured: bool,
    /// Why there is no measurement, in the sentence a pane prints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unscored: Option<String>,
    /// What it was measured from, in one clause: "34 dependency rows judged by 3 rule(s)".
    pub basis: String,
    /// The claim a reviewer can check against a file: counts, and the report it came from.
    pub reason: String,
    /// Extra exported integers a hand-check needs beyond the curve's own two, in the order they matter.
    pub evidence: Vec<(String, i64)>,
    /// Whether the answer is complete. `false` is a stated partial, never a shorter list.
    pub complete: bool,
    /// Every caveat, in the order found.
    pub caveats: Vec<String>,
}

impl Measurement {
    /// A dimension that says nothing, with the reason.
    #[must_use]
    pub fn unmeasured(
        dimension: Dimension,
        why: impl Into<String>,
        basis: impl Into<String>,
    ) -> Self {
        Self {
            dimension,
            curve: None,
            measured: false,
            unscored: Some(why.into()),
            basis: basis.into(),
            reason: String::new(),
            evidence: Vec::new(),
            complete: false,
            caveats: Vec::new(),
        }
    }

    /// A measured dimension on `n / d`, refused to `unmeasured` by the caller when `d` is zero.
    #[must_use]
    pub fn share(
        dimension: Dimension,
        numerator: i64,
        denominator: i64,
        basis: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            dimension,
            curve: Some(Curve::Share {
                numerator,
                denominator,
            }),
            measured: true,
            unscored: None,
            basis: basis.into(),
            reason: reason.into(),
            evidence: Vec::new(),
            complete: true,
            caveats: Vec::new(),
        }
    }

    /// A measured dimension on `n / (n + k)`.
    #[must_use]
    pub fn knee(
        dimension: Dimension,
        count: i64,
        knee: f64,
        basis: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            dimension,
            curve: Some(Curve::Knee { count, knee }),
            measured: true,
            unscored: None,
            basis: basis.into(),
            reason: reason.into(),
            evidence: Vec::new(),
            complete: true,
            caveats: Vec::new(),
        }
    }

    /// The badness, `0.0..=1.0`, rounded to four decimals — the rounding rule every score in this
    /// product uses so the printed line is the arithmetic that produced the score.
    #[must_use]
    pub fn badness(&self) -> f64 {
        match &self.curve {
            Some(curve) => badness_of(curve),
            None => 0.0,
        }
    }

    /// The health, `1.0 − badness`: what the score is built from, and what goes up when things improve.
    #[must_use]
    pub fn health(&self) -> f64 {
        round4(1.0 - self.badness())
    }

    /// The one line a pane prints: `boundaries 3/412 (0.0073) — 3 violation(s) over 34 rows judged`.
    #[must_use]
    pub fn sentence(&self) -> String {
        if !self.measured {
            return format!(
                "{} — not measured: {}",
                self.dimension.title(),
                self.unscored.as_deref().unwrap_or("nothing was examined")
            );
        }
        let curve = self
            .curve
            .as_ref()
            .map(|curve| curve.formula())
            .unwrap_or_default();
        format!(
            "{} {curve} (badness {}) — {}",
            self.dimension.title(),
            round4(self.badness()),
            self.reason
        )
    }

    /// The exported form. Every number the sentence depends on is here, named.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "dimension": self.dimension.id(),
            "title": self.dimension.title(),
            "question": self.dimension.question(),
            "source": self.dimension.source(),
            "measured": self.measured,
            "unscored": self.unscored,
            "curve": self.curve,
            "formula": self.curve.as_ref().map(Curve::formula),
            "badness": round4(self.badness()),
            "health": self.health(),
            "basis": self.basis,
            "reason": self.reason,
            "complete": self.complete,
            "caveats": self.caveats,
            "evidence": self
                .evidence
                .iter()
                .map(|(key, value)| serde_json::json!({ "name": key, "value": value }))
                .collect::<Vec<_>>(),
        })
    }

    /// The stored, compact form — the numbers a trend needs to rebuild a delta without recomputing a
    /// graph. Deliberately a subset of [`Measurement::to_json`], and round-tripped in the tests.
    #[must_use]
    pub fn to_stored(&self) -> serde_json::Value {
        serde_json::json!({
            "dimension": self.dimension.id(),
            "measured": self.measured,
            "badness": round4(self.badness()),
            "curve": self.curve,
        })
    }
}

/// What the API-hygiene dimension was measured from.
///
/// Two grains, one dimension, and the grain is always printed:
///
/// * [`ApiGrain::Graph`] — the module graph's own export rows. A symbol that arrived or vanished between
///   two revisions is what this can see; an item whose *signature* changed while its name survived is
///   invisible here, so the measurement is never called breaking-change-free and always says `complete:
///   false`. This is the grain this build can afford from stored rows, which is why it is the default.
/// * [`ApiGrain::Diff`] — `e3.7`'s classified diff, when a caller has one (it needs the file contents of
///   both revisions). Same dimension, finer claim, and the note says which diff answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiSignal {
    /// Which grain answered.
    pub grain: ApiGrain,
    /// How many changes count as breaking at this grain.
    pub breaking: i64,
    /// How many changes could not be classified at this grain — neither a pass nor an accusation.
    pub indeterminate: i64,
    /// Exports that arrived (graph grain) / items added (diff grain).
    pub arrived: i64,
    /// Rows the diff saw in total, for the ratio a reader can print beside the count.
    pub examined: i64,
    /// Whether the read was complete at its own grain.
    pub complete: bool,
    /// The sentence that says what this grain can and cannot see.
    pub note: String,
    /// Whether any read happened at all.
    pub measured: bool,
}

/// The grain an [`ApiSignal`] was taken at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiGrain {
    /// Export rows of the module graph (`e3.2`).
    Graph,
    /// The classified surface diff (`e3.7`).
    Diff,
}

impl ApiGrain {
    /// The printed spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ApiGrain::Graph => "graph",
            ApiGrain::Diff => "diff",
        }
    }
}

impl ApiSignal {
    /// The graph grain: one revision's stored rows.
    ///
    /// `removed` and `added` are the revision's own ops, as the lane read them. A removed *symbol* node
    /// is an export that is gone as the extractor sees it; the indeterminate half is every endpoint the
    /// dictionary could not name, which is a real count of what this grain cannot say.
    #[must_use]
    pub fn of_rows(
        added: &[crate::graph::model::Row],
        removed: &[crate::graph::model::Row],
        naming: &Naming,
    ) -> Self {
        let count = |rows: &[crate::graph::model::Row], dependency: bool| -> i64 {
            i64::try_from(
                rows.iter()
                    .filter(|(kind, _, _)| kind.is_dependency() == dependency)
                    .count(),
            )
            .unwrap_or(i64::MAX)
        };
        let exports_removed = i64::try_from(
            removed
                .iter()
                .filter(|(kind, _, _)| *kind == RowKind::Symbol)
                .count(),
        )
        .unwrap_or(i64::MAX);
        let exports_added = i64::try_from(
            added
                .iter()
                .filter(|(kind, _, _)| *kind == RowKind::Symbol)
                .count(),
        )
        .unwrap_or(i64::MAX);
        let unnamed = i64::try_from(
            added
                .iter()
                .chain(removed.iter())
                .filter(|(_, src, dst)| {
                    naming.vertex(*src).is_none() || naming.vertex(*dst).is_none()
                })
                .count(),
        )
        .unwrap_or(i64::MAX);
        Self {
            grain: ApiGrain::Graph,
            breaking: exports_removed,
            indeterminate: unnamed,
            arrived: exports_added,
            examined: count(added, true) + count(removed, true),
            complete: unnamed == 0,
            note: format!(
                "measured at the graph's export grain: {exports_removed} symbol node(s) left the graph \
                 and {exports_added} arrived. A signature change that kept its name is invisible at \
                 this grain, so nothing here is claimed as 'no breaking change'."
            ),
            measured: true,
        }
    }

    /// The `e3.7` grain, from a diff the caller already has.
    #[must_use]
    pub fn of_diff(diff: &Diff) -> Self {
        Self {
            grain: ApiGrain::Diff,
            breaking: i64::try_from(diff.totals.breaking).unwrap_or(i64::MAX),
            indeterminate: i64::try_from(diff.totals.indeterminate).unwrap_or(i64::MAX),
            arrived: i64::try_from(diff.totals.additive).unwrap_or(i64::MAX),
            examined: i64::try_from(diff.totals.total()).unwrap_or(i64::MAX),
            complete: diff.answered != crate::api::diff::Answered::Incomplete,
            note: format!(
                "measured at e3.7's classified grain: {} ({} answered {})",
                diff.totals.summary(),
                if diff.answered == crate::api::diff::Answered::Incomplete {
                    "partly — a surface could not be read"
                } else {
                    "fully"
                },
                diff.episode.clone().unwrap_or_default()
            ),
            measured: true,
        }
    }

    /// No read at all: nothing to compare this revision against, or no surface to read.
    #[must_use]
    pub fn absent(why: impl Into<String>) -> Self {
        Self {
            grain: ApiGrain::Graph,
            breaking: 0,
            indeterminate: 0,
            arrived: 0,
            examined: 0,
            complete: false,
            note: why.into(),
            measured: false,
        }
    }

    /// The dimension this signal produces.
    #[must_use]
    pub fn measurement(&self, policy: &Policy) -> Measurement {
        if !self.measured {
            return Measurement::unmeasured(
                Dimension::Api,
                self.note.clone(),
                "no API read was possible for this revision",
            );
        }
        let mut measurement = Measurement::knee(
            Dimension::Api,
            self.breaking,
            policy.api_knee,
            format!(
                "{} grain: {} row(s) examined",
                self.grain.as_str(),
                self.examined
            ),
            if self.breaking == 0 {
                format!(
                    "no {} at this grain; {} arrived",
                    if self.grain == ApiGrain::Diff {
                        "breaking change"
                    } else {
                        "export left the graph"
                    },
                    self.arrived
                )
            } else {
                format!(
                    "{} {} · {} arrived",
                    self.breaking,
                    if self.grain == ApiGrain::Diff {
                        "breaking change(s)"
                    } else {
                        "export(s) left the graph"
                    },
                    self.arrived
                )
            },
        );
        measurement.complete = self.complete;
        measurement.evidence.push(("grain".to_owned(), 0));
        measurement
            .evidence
            .push((self.grain.as_str().to_owned(), self.breaking));
        measurement
            .evidence
            .push(("arrived".to_owned(), self.arrived));
        measurement
            .evidence
            .push(("indeterminate".to_owned(), self.indeterminate));
        measurement.caveats.push(self.note.clone());
        if self.indeterminate > 0 {
            measurement.caveats.push(format!(
                "{} change(s) this grain could not classify",
                self.indeterminate
            ));
        }
        measurement
    }
}

/// The boundaries dimension: what the declared rules allow against what this revision does.
///
/// Debt is not counted. `e3.6`'s verdict already separates "baselined with an owner and an expiry" from
/// "news", and a scorecard that counted both would tell a reviewer the architecture is degrading on the
/// day they wrote down that it was already degraded — the exact failure the ratchet exists to prevent.
#[must_use]
pub fn boundaries(report: &LayerReport) -> Measurement {
    let counters = report.counters;
    if report.rules().is_inert() {
        return Measurement::unmeasured(
            Dimension::Boundaries,
            "no layering rules are declared, so \"respected\" has no definition here — this is not a \
             clean bill of health, it is an unwritten one",
            "no [layering] rules in force",
        );
    }
    if counters.checked == 0 {
        return Measurement::unmeasured(
            Dimension::Boundaries,
            "this revision has no dependency rows to judge, which is not the same as a clean \
             architecture",
            "0 dependency row(s) checked",
        );
    }
    let found = i64::try_from(report.violations_count()).unwrap_or(i64::MAX);
    let checked = i64::try_from(counters.checked).unwrap_or(i64::MAX);
    let mut measurement = Measurement::share(
        Dimension::Boundaries,
        found,
        checked,
        format!(
            "{checked} dependency row(s) judged by {} rule(s)",
            counters.rules
        ),
        if found == 0 {
            format!(
                "no violation without an allowance over {checked} dependency row(s); {} baselined as \
                 debt",
                report.baselined_violations()
            )
        } else {
            format!(
                "{found} violation(s) without an allowance over {checked} dependency row(s); {} \
                 baselined as debt, {} introduced by this episode",
                report.baselined_violations(),
                report.new_violations()
            )
        },
    );
    measurement.evidence = vec![
        (
            "baselined_violations".to_owned(),
            i64::try_from(report.baselined_violations()).unwrap_or(i64::MAX),
        ),
        (
            "new_violations".to_owned(),
            i64::try_from(report.new_violations()).unwrap_or(i64::MAX),
        ),
        (
            "rules_active".to_owned(),
            i64::try_from(counters.rules).unwrap_or(i64::MAX),
        ),
        (
            "rules_refused".to_owned(),
            i64::try_from(counters.refused).unwrap_or(i64::MAX),
        ),
        (
            "found_before_bound".to_owned(),
            i64::try_from(report.found).unwrap_or(i64::MAX),
        ),
    ];
    if counters.refused > 0 {
        measurement.complete = false;
        measurement
            .caveats
            .push(format!("{} rule(s) were refused at load", counters.refused));
    }
    if report.truncated {
        measurement.complete = false;
        measurement.caveats.push(format!(
            "the report bound stopped the list: {} violation(s) were found, {} are listed",
            report.found,
            report.violations.len()
        ));
    }
    measurement.caveats.extend(report.notices.iter().cloned());
    measurement
}

/// The coupling dimension: mean dependency edges per module, over a published ceiling.
///
/// The ceiling is what makes the number mean something: 3 edges per module is not a fact, it is a fact
/// *against a bar*, and [`Policy::coupling_ceiling`] is that bar — printed, tunable, and named in the
/// formula so a reader can redo the division. Wildcard and package-granular edges are counted (an
/// unread file is a dependency, not an absence of one) and said out loud, which is `s1` findings 2 and 4
/// arriving in a second place.
#[must_use]
pub fn coupling(graph: &Graph, naming: &Naming, policy: &Policy) -> Measurement {
    let modules = graph
        .nodes()
        .values()
        .filter(|kind| **kind == RowKind::Module)
        .count();
    if modules == 0 {
        return Measurement::unmeasured(
            Dimension::Coupling,
            "the graph has no module nodes at this revision, so there is nothing to be coupled",
            "empty module graph",
        );
    }
    let edges = graph
        .edges()
        .iter()
        .filter(|(kind, _, _)| kind.is_dependency())
        .count();
    let wildcards = graph
        .edges()
        .iter()
        .filter(|(kind, _, _)| *kind == RowKind::Wildcard)
        .count();
    let package_granular = graph
        .edges()
        .iter()
        .filter(|(kind, _, _)| *kind == RowKind::ImportsPackage)
        .count();
    let unnamed = graph
        .edges()
        .iter()
        .filter(|(_, src, dst)| naming.vertex(*src).is_none() || naming.vertex(*dst).is_none())
        .count();
    let ceiling = policy.coupling_ceiling;
    let numerator = i64::try_from(edges).unwrap_or(i64::MAX);
    let ceil = (modules as f64 * ceiling).ceil();
    let denominator = if ceil > i64::MAX as f64 {
        i64::MAX
    } else {
        ceil as i64
    };
    let mut measurement = Measurement::share(
        Dimension::Coupling,
        numerator,
        denominator,
        format!("{modules} module(s), {edges} dependency edge(s)"),
        format!(
            "{} dependency edge(s) over {modules} module(s) = {} per module against a ceiling of {}",
            edges,
            round4(f64::from(i32::try_from(edges).unwrap_or(i32::MAX)) / modules as f64),
            round2(ceiling)
        ),
    );
    measurement.evidence = vec![
        (
            "modules".to_owned(),
            i64::try_from(modules).unwrap_or(i64::MAX),
        ),
        ("dependency_edges".to_owned(), numerator),
        (
            "wildcard_edges".to_owned(),
            i64::try_from(wildcards).unwrap_or(i64::MAX),
        ),
        (
            "package_granular_edges".to_owned(),
            i64::try_from(package_granular).unwrap_or(i64::MAX),
        ),
        (
            "unnamed_endpoints".to_owned(),
            i64::try_from(unnamed).unwrap_or(i64::MAX),
        ),
    ];
    if wildcards > 0 {
        measurement.caveats.push(format!(
            "{wildcards} edge(s) come from a dependency whose contents were never read — counted, and \
             counted as unknown rather than as nothing"
        ));
    }
    if package_granular > 0 {
        measurement.caveats.push(format!(
            "{package_granular} edge(s) are package-granular, so the grain is the package, not the file"
        ));
    }
    if unnamed > 0 {
        measurement.complete = false;
        measurement.caveats.push(format!(
            "{unnamed} edge endpoint(s) are not in the dictionary — not claimed complete"
        ));
    }
    measurement
}

/// The acyclicity dimension: how much of the graph is inside a loop nobody budgeted.
///
/// `e3.5` exports exactly this pair and says the scorecard is written against it
/// ([`crate::cycles::Health::share`]), with one refinement here: the *nodes inside debt* are subtracted,
/// so a baselined loop costs this dimension nothing. The two numbers stay side by side in the JSON, so a
/// reader can see both the share of everything cyclic and the share of what is nobody's declared debt.
#[must_use]
pub fn acyclicity(report: &CycleReport) -> Measurement {
    let nodes = report.health.nodes;
    if nodes == 0 {
        return Measurement::unmeasured(
            Dimension::Acyclicity,
            "the graph has no nodes at this revision, so no part of it can be inside a loop",
            "empty module graph",
        );
    }
    let in_cycles = i64::try_from(report.health.nodes_in_cycles).unwrap_or(i64::MAX);
    let nodes = i64::try_from(nodes).unwrap_or(i64::MAX);
    let owed: i64 = report
        .verdict
        .debt
        .iter()
        .map(|debt| i64::try_from(debt.cycle.members.len()).unwrap_or(0))
        .sum();
    let news = (in_cycles - owed).max(0);
    let cycles = i64::try_from(report.health.cycles).unwrap_or(i64::MAX);
    let mut measurement = Measurement::share(
        Dimension::Acyclicity,
        news,
        nodes,
        format!("{nodes} node(s), {cycles} cycle(s)"),
        if in_cycles == 0 {
            format!(
                "no node sits inside a loop in the {} view",
                report.health.view.as_str()
            )
        } else {
            format!(
                "{news} of {nodes} node(s) sit inside a loop with no allowance · {owed} more are \
                 baselined debt · {} cycle(s), largest {} members",
                report.health.cycles, report.health.largest_members
            )
        },
    );
    measurement.evidence = vec![
        ("cycles".to_owned(), cycles),
        ("nodes_in_cycles".to_owned(), in_cycles),
        ("nodes".to_owned(), nodes),
        ("nodes_in_debt".to_owned(), owed),
        (
            "new_cycles".to_owned(),
            i64::try_from(report.new_cycles()).unwrap_or(i64::MAX),
        ),
    ];
    if report.health.wildcard_claimed > 0 {
        measurement.caveats.push(format!(
            "{} cycle(s) are bounded by a dependency with unread contents",
            report.health.wildcard_claimed
        ));
    }
    if report.health.unnamed_endpoints > 0 || report.truncated {
        measurement.complete = false;
        if report.health.unnamed_endpoints > 0 {
            measurement.caveats.push(format!(
                "{} endpoint(s) not in the dictionary",
                report.health.unnamed_endpoints
            ));
        }
        if report.truncated {
            measurement
                .caveats
                .push("a report bound stopped one of the lists".to_owned());
        }
    }
    measurement
}

/// The hotspot-pressure dimension: how many modules are expensive to change and widely felt.
///
/// The bar is [`Policy::hotspot_bar`], a score on the same 0–100 ladder `e3.8` prints, and the answer is
/// a share: modules at or over it, over modules scored. This is *not* a claim that a hot module is a
/// defect — `e3.8`'s own warning that a high score says "load-bearing" is inherited wholesale — it is a
/// statement about where a change will cost the most attention, which is the thing a scorecard is for.
#[must_use]
pub fn hotspots(table: &HotspotTable, policy: &Policy) -> Measurement {
    if !table.ranked {
        return Measurement::unmeasured(
            Dimension::Hotspots,
            table.empty_reason.clone().unwrap_or_else(|| {
                format!(
                    "nothing was ranked: this index holds {} revision(s), and a graph of one has no \
                     churn to measure",
                    table.window
                )
            }),
            format!("no ranking (window {} revision(s))", table.window),
        );
    }
    let bar = policy.hotspot_bar;
    let above = table
        .rows
        .iter()
        .filter(|row| row.scored && row.score >= bar)
        .count();
    let scored = i64::try_from(table.modules_scored).unwrap_or(i64::MAX);
    let above = i64::try_from(above).unwrap_or(i64::MAX);
    let mut measurement = Measurement::share(
        Dimension::Hotspots,
        above,
        scored,
        format!(
            "{} module(s) ranked, {scored} scored, bar {bar}/100",
            table.modules_seen
        ),
        if above == 0 {
            format!(
                "no module reaches {bar}/100; the highest is {}",
                table
                    .rows
                    .first()
                    .map(|row| format!("{} at {}/100", row.module, row.score))
                    .unwrap_or_else(|| "nothing".to_owned())
            )
        } else {
            format!(
                "{above} of {scored} module(s) reach {bar}/100; first is {}",
                table
                    .rows
                    .first()
                    .map(|row| format!("{} at {}/100", row.module, row.score))
                    .unwrap_or_else(|| "nothing".to_owned())
            )
        },
    );
    measurement.evidence = vec![
        (
            "modules_seen".to_owned(),
            i64::try_from(table.modules_seen).unwrap_or(i64::MAX),
        ),
        ("modules_scored".to_owned(), scored),
        ("bar".to_owned(), i64::from(bar)),
        (
            "window".to_owned(),
            i64::try_from(table.window).unwrap_or(i64::MAX),
        ),
    ];
    if table.rows_dropped > 0 {
        let lowest_kept = table.rows.last().map_or(0, |row| row.score);
        if lowest_kept >= bar {
            measurement.complete = false;
            measurement.caveats.push(format!(
                "the ranking list stops at {bar}/100 with {} module(s) dropped, so the count at or over \
                 the bar is a floor, not the total",
                table.rows_dropped
            ));
        }
    }
    measurement.caveats.extend(table.notices.iter().cloned());
    measurement
}

/// The badness a curve encodes, in `0.0..=1.0`, rounded to four decimals.
///
/// `f64::MAX` guards on the integer casts are here because a count that does not fit in `i64` is a count
/// of "astronomically many", and the honest curve value for that is 1.0 rather than a panic or a NaN.
#[must_use]
pub fn badness_of(curve: &Curve) -> f64 {
    match curve {
        Curve::Share {
            numerator,
            denominator,
        } => {
            if *denominator <= 0 {
                return 0.0;
            }
            if *numerator >= *denominator {
                return 1.0;
            }
            round4(
                f64::from(i32::try_from(*numerator).unwrap_or(i32::MAX))
                    / f64::from(i32::try_from(*denominator).unwrap_or(i32::MAX)),
            )
        }
        Curve::Knee { count, knee } => {
            let count = f64::from(i32::try_from((*count).max(0)).unwrap_or(i32::MAX));
            if count <= 0.0 {
                return 0.0;
            }
            round4(count / (count + knee.max(f64::MIN_POSITIVE)))
        }
    }
}

/// Round to four decimals — the published rounding rule of this model.
#[must_use]
pub fn round4(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    (value * 10_000.0).round() / 10_000.0
}

/// Round to two decimals, for a published constant printed beside a formula.
#[must_use]
pub fn round2(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    (value * 100.0).round() / 100.0
}

//! The feature vector `e4.4` scores an episode on (`reco-884`).
//!
//! Twelve features, one claim each, and every one of them is a number a reviewer can go and look at.
//! This is the part of the risk score that is *not* arithmetic: a feature is a measurement, the ruler
//! that turns the measurement into a 0..=1 risk value, the sentence that says what the ruler means, and
//! the row the measurement came from. [`Feature::normalise`] and [`Feature::spec`] live in this file
//! beside the ruler they apply, so a document cannot describe a ruler the code does not implement —
//! `tests/test_risk_score_contract.py` reads the table below and compares it to
//! [`docs/concepts/risk-scoring.md`](../../../../docs/concepts/risk-scoring.md) and to the default
//! weights in the config key spec.
//!
//! # Three rules every feature here obeys
//!
//! * **A feature is a measurement, never an adjective.** `findings` is "eight severity-points", not
//!   "worrying". `noise` is a share of churn, not "generated stuff".
//! * **A feature is risk-oriented**: `1.0` means "as much attention as this axis ever asks for", and
//!   the inversion, where there is one, is said out loud. `test_ratio` is the only inverted axis: it
//!   *measures* the share of new logic that arrived with tests and *risks* on its complement.
//! * **A feature can be unmeasured, and unmeasured is not zero.** [`Measurement::Missing`] carries the
//!   reason and the ticket that owns the missing input, [`super::score`] leaves it out of the
//!   denominator, and `fathom why` prints the hole beside the numbers that filled in. The alternative —
//!   a missing blast radius read as "nothing depends on this" — is the failure ADR-001 §7.7 refuses.
//!
//! # What is deliberately not a feature
//!
//! Nothing that needs a model or the network. `blast` exists in the vector because the ticket names it,
//! and the count itself arrives through [`super::collect::Supply`] from a caller that has walked the graph;
//! until a caller pays for that walk the axis is `Missing` with the *index's own answer* as its reason
//! ([`super::blast`]), which is a fact about this store and not a promise about a plan. Advisory age
//! (`net-advisories`) is deliberately absent: a feature whose value depends on whether a request succeeded
//! is a feature whose score is a function of the network, and ADR-001 §8.2 will not have it.

use serde::{Deserialize, Serialize};

/// The model's name, printed beside every number it produces.
///
/// `spine-1` ([`super::MODEL`]) is the ordering the rail and the queue use today; `features-1` is the
/// vector this ticket owns. Both names appear on purpose: a screenshot has to say which of the two
/// produced it, and `e4.5` switches the queue from one to the other without changing a pane.
pub const MODEL: &str = "features-1";

/// The version of the vector and its arithmetic, bumped when a feature is added, dropped or re-ruled.
///
/// A weight change is *not* a version change — that is the whole point of tunable weights — but a new
/// feature changes what a score means, so it is. `fathom why` prints this number, and a bug report that
/// names it says which ruler the score was taken with.
pub const MODEL_VERSION: u32 = 1;

/// The ceiling on a weight, and on a normalised value.
///
/// One constant for both, because the printed rule is one sentence: *nothing in this model is above 1*.
/// A `fathom.toml` that asks for `size = 3` gets clamped to this and told about it, rather than
/// silently becoming a model nobody recognises.
pub const MAX_WEIGHT: f64 = 1.0;

/// One feature of the vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    /// What the `e4.3` detectors found, weighted by severity.
    Findings,
    /// The most consequential *kind* of file the episode touched.
    Surface,
    /// How big the change is, discounted for the churn a review does not read.
    Size,
    /// What depends on the touched code. Supplied by the graph, never guessed.
    Blast,
    /// Dependency and lockfile exposure.
    Dependency,
    /// Schema-migration exposure, including the statements that may not be reversible.
    Migration,
    /// How much of the new logic arrived without a test alongside it.
    TestRatio,
    /// How much of the change is removal, and whether the test surface shrank.
    Deletions,
    /// How sure the attribution is that it is right.
    Attribution,
    /// Whether a program wrote this, which is the thesis (ADR-001 §7.3), not a detail.
    Authorship,
    /// The share of the churn that is generated, locked, vendored or binary.
    Noise,
    /// How far the change spread: surfaces, files, commits.
    Breadth,
}

/// Everything a reader needs to argue with one feature, as data.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    /// The printed key — also the `fathom.toml` key under `[risk.weights]`.
    pub id: &'static str,
    /// The question the feature answers, in one clause.
    pub asks: &'static str,
    /// The unit of the raw measurement, printed beside it.
    pub unit: &'static str,
    /// What it takes to fill the axis, printed beside the value.
    pub ruler: &'static str,
    /// The built-in weight, in [`DEFAULTS`].
    pub default: f64,
    /// The row or analysis the measurement comes from.
    pub source: &'static str,
    /// The ticket that owns the input, printed when the input is missing — or, when the input already
    /// exists in this build and only a caller is missing, the thing that is missing, in words.
    pub owner: &'static str,
}

/// The twelve specs, in printed order: the ones a reviewer acts on first.
///
/// The order is the order `fathom why` prints when contributions tie, and the order the docs table is
/// written in. It is not the order of the arithmetic — contributions decide that — but it is the order
/// of *explainability*, so that a table read top-down answers "what matters most" before the arithmetic
/// has been read at all.
pub const SPECS: [Spec; 12] = [
    Spec {
        id: "findings",
        asks: "what did the detectors find, and how bad was each one",
        unit: "severity-points",
        ruler: "8 severity-points fill the axis (a high finding is 3, a medium 2, a low 1)",
        default: 0.16,
        source: "`e4.3` detectors",
        owner: "e4.3",
    },
    Spec {
        id: "surface",
        asks: "what kind of file did this touch",
        unit: "surface weight",
        ruler: "a migration (50) fills the axis; source is 10 and prose is 3",
        default: 0.13,
        source: "`git_commit_file.path` via `risk::classify`",
        owner: "e4.4",
    },
    Spec {
        id: "migration",
        asks: "does this change schema history, and does it say something irreversible",
        unit: "exposure",
        ruler: "one migration file is 0.55; a `DROP`/`DELETE`/`TRUNCATE` in the added lines is 1.0",
        default: 0.09,
        source: "path class plus the added lines of `*.sql`",
        owner: "e4.4",
    },
    Spec {
        id: "blast",
        asks: "what depends on the code this touched",
        unit: "dependents",
        ruler: "40 dependent paths fill the axis",
        default: 0.09,
        source: "a dependent count handed over by the caller",
        owner: "the caller",
    },
    Spec {
        id: "test_ratio",
        asks: "how much of the new logic arrived without a test alongside it",
        unit: "coverage share",
        ruler: "inverted: 0 tests beside new logic fills the axis, and a suite that shrank floors it at 0.75",
        default: 0.08,
        source: "per-path `add`/`del` split by surface",
        owner: "e4.4",
    },
    Spec {
        id: "size",
        asks: "how much does a reviewer have to read",
        unit: "reviewable lines",
        ruler: "20 000 reviewable lines fill the axis, on a log scale",
        default: 0.07,
        source: "`episode.n_add`, `n_del`, per-path noise classes",
        owner: "e4.4",
    },
    Spec {
        id: "dependency",
        asks: "what does this change about what the build depends on",
        unit: "exposure",
        ruler: "one lockfile is 0.55, a manifest 0.35, a supplied direct add 0.8 and a major move 1.0",
        default: 0.07,
        source: "path class, and `e2.6`'s delta when a caller supplies it",
        owner: "e2.6",
    },
    Spec {
        id: "deletions",
        asks: "how much of the change is removal, and did the cover go with it",
        unit: "removal shape",
        ruler: "a change that is three-fifths removal is 0.6; a test surface that shrank while source grew floors it at 0.75",
        default: 0.07,
        source: "per-path `add`/`del` split by surface",
        owner: "e4.4",
    },
    Spec {
        id: "noise",
        asks: "how much of the churn is text a review does not read",
        unit: "share of churn",
        ruler: "the share itself: all-generated churn fills the axis",
        default: 0.07,
        source: "`episode.noise_share` when measured, path classes otherwise",
        owner: "e2.2",
    },
    Spec {
        id: "breadth",
        asks: "how far did this spread",
        unit: "spread",
        ruler: "five surfaces past the first, or thirty files, fills the axis",
        default: 0.07,
        source: "`episode.n_files`, distinct surfaces, `n_commits`",
        owner: "e4.4",
    },
    Spec {
        id: "attribution",
        asks: "how sure are we who wrote this",
        unit: "confidence",
        ruler: "inverted: 1 − the attribution confidence, floored at 0.8 when nobody is credited",
        default: 0.05,
        source: "`episode.author_confidence`, `author_kind`",
        owner: "e4.1",
    },
    Spec {
        id: "authorship",
        asks: "is the thing that wrote this a program",
        unit: "authorship",
        ruler: "an agent is 0.7, an uncredited author is 1.0, a person is 0",
        default: 0.05,
        source: "`episode.author_kind`",
        owner: "e4.1",
    },
];

/// The built-in weights, as a list in [`SPECS`]' order.
///
/// These are the numbers a repository gets when it writes nothing. They are an argument, not a
/// measurement: findings and kind-of-file carry the score because those are the two things a reviewer
/// acts on, and size carries the least of the seven because size is what the *queue* already buries. A
/// repository that disagrees edits `[risk.weights]`, and `fathom why --calibrate` is how the disagreement
/// gets measured instead of felt.
pub const DEFAULTS: [(Feature, f64); 12] = [
    (Feature::Findings, 0.16),
    (Feature::Surface, 0.13),
    (Feature::Migration, 0.09),
    (Feature::Blast, 0.09),
    (Feature::TestRatio, 0.08),
    (Feature::Size, 0.07),
    (Feature::Dependency, 0.07),
    (Feature::Deletions, 0.07),
    (Feature::Noise, 0.07),
    (Feature::Breadth, 0.07),
    (Feature::Attribution, 0.05),
    (Feature::Authorship, 0.05),
];

impl Feature {
    /// Every feature, in printed order.
    pub const ALL: [Feature; 12] = [
        Feature::Findings,
        Feature::Surface,
        Feature::Migration,
        Feature::Blast,
        Feature::TestRatio,
        Feature::Size,
        Feature::Dependency,
        Feature::Deletions,
        Feature::Noise,
        Feature::Breadth,
        Feature::Attribution,
        Feature::Authorship,
    ];

    /// The spec: id, question, unit, ruler, default weight, source, owner.
    #[must_use]
    pub const fn spec(self) -> &'static Spec {
        match self {
            Feature::Findings => &SPECS[0],
            Feature::Surface => &SPECS[1],
            Feature::Migration => &SPECS[2],
            Feature::Blast => &SPECS[3],
            Feature::TestRatio => &SPECS[4],
            Feature::Size => &SPECS[5],
            Feature::Dependency => &SPECS[6],
            Feature::Deletions => &SPECS[7],
            Feature::Noise => &SPECS[8],
            Feature::Breadth => &SPECS[9],
            Feature::Attribution => &SPECS[10],
            Feature::Authorship => &SPECS[11],
        }
    }

    /// The printed key.
    #[must_use]
    pub const fn id(self) -> &'static str {
        self.spec().id
    }

    /// The built-in weight.
    #[must_use]
    pub const fn default_weight(self) -> f64 {
        self.spec().default
    }

    /// Every id, in printed order — for a refusal that has to list the valid names.
    #[must_use]
    pub fn ids() -> Vec<&'static str> {
        Self::ALL.iter().map(|feature| feature.id()).collect()
    }

    /// Parse a `fathom.toml` key or a `--weight` name. Exact, case-sensitive: these are keys, and a
    /// parser that "helpfully" folded case would accept `SIZE` and then never match it again.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        Self::ALL
            .iter()
            .copied()
            .find(|feature| feature.id() == value)
    }

    /// Turn a raw measurement in [`Spec::unit`] into a risk value in `0.0..=1.0`.
    ///
    /// Every arm is monotonic in its input, which is what makes a raised weight's effect on a ranking
    /// provable rather than observable ([`super::score`] and the property tests). The two inversions —
    /// `test_ratio` and `attribution` — invert a *share*, so they stay monotonic in the quantity the
    /// feature is actually about (the coverage gap, the doubt).
    #[must_use]
    pub fn normalise(self, raw: f64) -> f64 {
        let value = match self {
            Feature::Findings => raw / 8.0,
            Feature::Surface => raw / 50.0,
            Feature::Size => {
                if raw <= 0.0 {
                    0.0
                } else {
                    fn_ln(1.0 + raw) / fn_ln(1.0 + super::SIZE_CEILING as f64)
                }
            }
            Feature::Blast => raw / 40.0,
            // Exposure scores arrive already normalised; the ceiling is a clamp, not a rule.
            Feature::Dependency
            | Feature::Migration
            | Feature::Deletions
            | Feature::Noise
            | Feature::Breadth
            | Feature::Authorship => raw,
            Feature::TestRatio => 1.0 - raw,
            Feature::Attribution => 1.0 - raw,
        };
        if !value.is_finite() {
            // A NaN or an infinity in a feature is a bug in the collector, and the honest answer is "the
            // axis says nothing", not a score that prints as `nan`.
            return 0.0;
        }
        value.clamp(0.0, MAX_WEIGHT)
    }

    /// The value's printed form in the feature's own unit.
    ///
    /// Whole numbers stay whole (`3 severity-points`, not `3.00`), and a share keeps two digits. This is
    /// the difference between a table a reviewer reads and a table a reviewer skips.
    #[must_use]
    pub fn format_raw(self, raw: f64) -> String {
        match self {
            Feature::Findings | Feature::Surface | Feature::Blast => {
                format!("{:.0}", raw.round())
            }
            _ => format!("{:.2}", raw),
        }
    }
}

/// `ln`, named so the one place a float transcendental is called in the scorer is findable by grep.
fn fn_ln(value: f64) -> f64 {
    value.ln()
}

/// One feature's measurement, or its stated absence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Measurement {
    /// Which feature.
    pub feature: Feature,
    /// `measured` or `missing`.
    pub state: &'static str,
    /// The raw value in [`Spec::unit`], absent when the feature was not measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<f64>,
    /// The raw value after [`Feature::normalise`], `0.0..=1.0`.
    pub value: f64,
    /// What the measurement was taken from, in one clause: `3 high findings`, `lockfile path`,
    /// `co-change proxy`. Printed, never assumed.
    pub basis: String,
    /// The claim a reviewer can check against the diff — a path, a count, a name. Never an adjective.
    pub detail: String,
    /// The ticket that owns the missing input, when the feature is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<&'static str>,
}

impl Measurement {
    /// A measured feature: the raw value, how it was taken, and what it says.
    ///
    /// The raw value is kept as measured, except that a negative is refused: a count cannot go below
    /// zero, and a share cannot leave `0.0..=1.0`. An over-large raw is *not* quietly capped, because a
    /// reader who is told "61 severity-points" under a ruler that tops out at 8 learns something real
    /// (the axis is saturated) that a silently-capped `8` would hide. [`Feature::normalise`] does the
    /// clamping, and the printed `value` is the clamped one.
    #[must_use]
    pub fn measured(
        feature: Feature,
        raw: f64,
        basis: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        let raw = match feature {
            Feature::Findings | Feature::Surface | Feature::Size | Feature::Blast => raw.max(0.0),
            _ => raw.clamp(0.0, 1.0),
        };
        Measurement {
            feature,
            state: "measured",
            raw: Some(raw),
            value: feature.normalise(raw),
            basis: basis.into(),
            detail: detail.into(),
            owner: None,
        }
    }

    /// A feature this build could not measure. Not an error, and not a zero.
    #[must_use]
    pub fn missing(feature: Feature, why: impl Into<String>, owner: &'static str) -> Measurement {
        Measurement {
            feature,
            state: "missing",
            raw: None,
            value: 0.0,
            basis: String::new(),
            detail: why.into(),
            owner: Some(owner),
        }
    }

    /// Was it measured?
    #[must_use]
    pub fn is_measured(&self) -> bool {
        self.state == "measured"
    }

    /// The value, or `0.0` when unmeasured — which is only ever used by a caller that has already
    /// excluded the unmeasured ones from its arithmetic.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.value
    }
}

/// The vector: one [`Measurement`] per feature, always all twelve.
///
/// All twelve even when five are missing, because a partial list cannot answer "why is there no blast
/// radius row" — the answer has to be a row that says so. Order is [`Feature::ALL`], so two runs of the
/// same index print the same bytes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Vector {
    /// The episode these measurements describe.
    pub episode: String,
    /// One per feature, in [`Feature::ALL`]'s order.
    pub measurements: Vec<Measurement>,
    /// Anything the read had to caveat: a capped file read, a blob it could not open.
    pub caveats: Vec<String>,
}

impl Vector {
    /// Build a vector from measurements, filling every feature the caller did not mention as missing
    /// with the spec's owner. A feature that is simply absent from the input is a feature nobody
    /// measured, and that is what gets printed.
    #[must_use]
    pub fn new(episode: impl Into<String>, measurements: Vec<Measurement>) -> Self {
        let mut filled: Vec<Measurement> = Vec::with_capacity(Feature::ALL.len());
        for feature in Feature::ALL {
            match measurements.iter().find(|item| item.feature == feature) {
                Some(found) => filled.push(found.clone()),
                None => filled.push(Measurement::missing(
                    feature,
                    format!(
                        "nothing supplied a {} measurement, and this read had no other way to take \
                         one, so the axis leaves the arithmetic with its weight still printed",
                        feature.id()
                    ),
                    feature.spec().owner,
                )),
            }
        }
        Vector {
            episode: episode.into(),
            measurements: filled,
            caveats: Vec::new(),
        }
    }

    /// One feature's measurement.
    #[must_use]
    pub fn get(&self, feature: Feature) -> Option<&Measurement> {
        self.measurements
            .iter()
            .find(|item| item.feature == feature)
    }

    /// How many of the twelve were measured.
    #[must_use]
    pub fn measured(&self) -> usize {
        self.measurements
            .iter()
            .filter(|item| item.is_measured())
            .count()
    }

    /// The features that were not measured, in printed order.
    #[must_use]
    pub fn missing(&self) -> Vec<&Measurement> {
        self.measurements
            .iter()
            .filter(|item| !item.is_measured())
            .collect()
    }
}

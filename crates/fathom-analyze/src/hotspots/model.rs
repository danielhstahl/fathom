//! The vocabulary of a hotspot: what a metric is, what a measurement is, what a contribution is,
//! and what a ranked row is allowed to say about a module (`e3.8`, `reco-wky`).
//!
//! Six metrics, and every one of them is a number a reviewer can go and check:
//!
//! | id | what the raw number counts | where it was measured from |
//! |---|---|---|
//! | `fan_in` | modules that depend on this one | the module graph ([`crate::graph::query::dependents_of`]) |
//! | `fan_out` | modules this one depends on | the module graph ([`crate::graph::query::dependencies_of`]) |
//! | `instability` | `fan_out / (fan_in + fan_out)` | the same two numbers, divided |
//! | `size` | code lines in the module | `e3.1`'s file-level measurement ([`crate::structure::FileLevel::code_lines`]) |
//! | `complexity` | branches, loops, returns and nesting | `e3.1`'s per-declaration spans ([`crate::structure::Complexity`]) |
//! | `churn` | graph edge events per revision | `e3.3`'s churn window ([`crate::graph::ChurnReport`]) |
//!
//! # A raw number is never a value
//!
//! Each metric is turned into a `0.0..=1.0` value by one saturating curve, `raw / (raw + knee)`, and
//! the knee is a published number ([`MetricSpec::default_knee`], configurable per metric as
//! `hotspots.knee.<metric>`). Saturation rather than a corpus-relative maximum for three reasons, each
//! of which is a refusal of something easier:
//!
//! * a corpus-relative value makes *this* module's score depend on *that* unrelated module appearing,
//!   so a score could move because somebody added a file somewhere else — which makes a trend
//!   meaningless and a ratchet unusable;
//! * a hard cut-off (`lines > 1000 → 1.0`) makes a 999-line and a 1 001-line module differ by nothing
//!   and a 1 000-line and a 40 000-line module differ by nothing either;
//! * saturation keeps the arithmetic monotone by construction, so retuning a weight cannot invert a
//!   ranking that every metric says is worse in every way.
//!
//! The printed row always shows the raw number, the knee it was read against, and the value that came
//! out, so the sentence "0.63 = 20 / (20 + 12)" is checkable with a calculator. [`Metric::saturate`] is
//! the only place that curve lives.
//!
//! # `instability` is undefined, not zero
//!
//! A module with no edges in either direction has no instability: `0/0` is not `0.0`, and printing
//! `0.00` for it would tell a reviewer the module is *stable* when the honest answer is that nothing
//! is known about it. [`Measurement::unmeasured`] is how that gets said, and an unmeasured metric is
//! left out of the denominator exactly as [`crate::risk`] leaves out an unmeasured feature — the
//! redistribution is printed, never hidden.

use serde::Serialize;

use crate::risk::score::Points;

/// The six metrics, in the order a printed table lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// How many modules depend on this one.
    FanIn,
    /// How many modules this one depends on.
    FanOut,
    /// `fan_out / (fan_in + fan_out)`: how much of a module's coupling points outward.
    Instability,
    /// How big the module is, in code lines.
    Size,
    /// How much a reader has to hold inside it, from `e3.1`'s span proxies.
    Complexity,
    /// How often it is reworked, in graph edge events per revision of the window.
    Churn,
}

/// Everything fixed about one metric: its name, its unit, its defaults and its limits.
#[derive(Debug, Clone, Copy)]
pub struct MetricSpec {
    /// The metric.
    pub metric: Metric,
    /// The stable key, which is also the `hotspots.weights.<id>` and `hotspots.knee.<id>` spelling.
    pub id: &'static str,
    /// The column heading.
    pub label: &'static str,
    /// The raw number's unit, printed beside it.
    pub unit: &'static str,
    /// `hotspots.weights.<id>`'s built-in value.
    pub default_weight: f64,
    /// The knee of the saturating curve; `None` when the raw number is already a `0.0..=1.0` ratio.
    pub default_knee: Option<f64>,
    /// What the raw number counts, in one sentence a table prints.
    pub what: &'static str,
    /// Where the number came from — the basis a reviewer checks it against.
    pub basis: &'static str,
}

/// The six specs, in [`Metric::ALL`]'s order.
const SPECS: [MetricSpec; 6] = [
    MetricSpec {
        metric: Metric::FanIn,
        id: "fan_in",
        label: "fan-in",
        unit: "callers",
        default_weight: 0.25,
        default_knee: Some(12.0),
        what: "modules that depend on this one, named and wildcard counted apart",
        basis: "the module graph at this revision",
    },
    MetricSpec {
        metric: Metric::FanOut,
        id: "fan_out",
        label: "fan-out",
        unit: "imports",
        default_weight: 0.10,
        default_knee: Some(12.0),
        what: "modules this one depends on",
        basis: "the module graph at this revision",
    },
    MetricSpec {
        metric: Metric::Instability,
        id: "instability",
        label: "instability",
        unit: "ratio",
        default_weight: 0.10,
        default_knee: None,
        what: "fan-out over total coupling: how much of it points outward",
        basis: "the two fan numbers above, divided",
    },
    MetricSpec {
        metric: Metric::Size,
        id: "size",
        label: "size",
        unit: "lines",
        default_weight: 0.15,
        default_knee: Some(600.0),
        what: "code lines in the module, comments and blanks excluded",
        basis: "e3.1's file-level measurement of this path",
    },
    MetricSpec {
        metric: Metric::Complexity,
        id: "complexity",
        label: "complexity",
        unit: "points",
        default_weight: 0.20,
        default_knee: Some(120.0),
        what: "branches, loops, early returns and nesting across its declarations",
        basis: "e3.1's per-declaration spans",
    },
    MetricSpec {
        metric: Metric::Churn,
        id: "churn",
        label: "churn",
        unit: "events/rev",
        default_weight: 0.20,
        default_knee: Some(2.0),
        what: "graph edge events touching this module, per revision of the window",
        basis: "e3.3's churn over the window",
    },
];

impl Metric {
    /// Every metric, in printed order.
    pub const ALL: [Metric; 6] = [
        Metric::FanIn,
        Metric::FanOut,
        Metric::Instability,
        Metric::Size,
        Metric::Complexity,
        Metric::Churn,
    ];

    /// The stable key.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Metric::FanIn => "fan_in",
            Metric::FanOut => "fan_out",
            Metric::Instability => "instability",
            Metric::Size => "size",
            Metric::Complexity => "complexity",
            Metric::Churn => "churn",
        }
    }

    /// The metric's fixed properties.
    #[must_use]
    pub const fn spec(self) -> &'static MetricSpec {
        match self {
            Metric::FanIn => &SPECS[0],
            Metric::FanOut => &SPECS[1],
            Metric::Instability => &SPECS[2],
            Metric::Size => &SPECS[3],
            Metric::Complexity => &SPECS[4],
            Metric::Churn => &SPECS[5],
        }
    }

    /// Parse the key, so `hotspots.weights.fan_in` and a `--weight fan_in=…` resolve to a metric.
    #[must_use]
    pub fn parse(text: &str) -> Option<Metric> {
        Metric::ALL
            .iter()
            .copied()
            .find(|metric| metric.id() == text)
    }

    /// Every key, for the notice that names the valid set.
    #[must_use]
    pub fn ids() -> Vec<&'static str> {
        Metric::ALL.iter().map(|metric| metric.id()).collect()
    }

    /// The knee in force by default for this metric. An unmeasured knee (a ratio) means the raw value
    /// is already the value.
    #[must_use]
    pub fn knee(&self, configured: Option<f64>) -> Option<f64> {
        configured.or(self.spec().default_knee)
    }

    /// The one line of arithmetic this module exists to make explainable.
    ///
    /// ```text
    /// value = raw / (raw + knee)          knee > 0
    /// value = raw                         a ratio already in 0.0..=1.0
    /// ```
    ///
    /// Exactly `0.5` at the knee, `0.67` at 2× the knee, `0.75` at 3× — a curve a reader can compute
    /// in their head, which is the whole difference between a score and a hotspot score.
    #[must_use]
    pub fn saturate(self, raw: f64, knee: Option<f64>) -> f64 {
        if !raw.is_finite() || raw <= 0.0 {
            return 0.0;
        }
        let Some(knee) = knee else {
            return raw.clamp(0.0, 1.0);
        };
        if knee <= 0.0 {
            // A knee of zero would divide by zero. The parser refuses it; this is the second door.
            return 1.0;
        }
        raw / (raw + knee)
    }

    /// The raw number as printed, with digit grouping and no more precision than the metric carries.
    #[must_use]
    pub fn format_raw(self, raw: f64) -> String {
        if self == Metric::Instability {
            return format!("{raw:.2}");
        }
        let rounded = (raw * 100.0).round() / 100.0;
        if (rounded - rounded.round()).abs() < f64::EPSILON {
            group(rounded.round() as i64)
        } else {
            groupf(rounded)
        }
    }
}

/// Group an integer's digits in threes, the way every number in this repository's prose is written.
#[must_use]
fn group(value: i64) -> String {
    let digits = value.abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        out.push('-');
    }
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// The same grouping for a number with a fractional part, which only ever carries two decimals.
#[must_use]
fn groupf(value: f64) -> String {
    let whole = value.trunc() as i64;
    let fraction = ((value - value.trunc()).abs() * 100.0).round() as i64;
    if fraction == 0 {
        group(whole)
    } else {
        format!("{}.{fraction:02}", group(whole))
    }
}

/// Where a weight came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The built-in in [`MetricSpec::default_weight`].
    Default,
    /// The repository's `fathom.toml`.
    Repo,
}

impl Origin {
    /// The printed spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Origin::Default => "default",
            Origin::Repo => "fathom.toml",
        }
    }
}

/// One metric's measurement for one module, with everything needed to print it.
///
/// A measurement is either *made* or *refused*. Refusal is a first-class state here: a module with no
/// structural record (unread, or a grammar this binary does not carry) has no size, and printing `0`
/// for it would be the most misleading number on the page — it would read as "tiny" and rank the
/// unknown module first among the quiet ones.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Measurement {
    /// The metric.
    pub metric: Metric,
    /// Whether the number was obtained.
    pub measured: bool,
    /// The raw number, in the metric's own unit. `None` when unmeasured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<f64>,
    /// That number as printed, unit included: `1 240 lines`.
    pub raw_shown: String,
    /// The saturated value, `0.0..=1.0`. `0.0` and unmeasured, never a fabricated value.
    pub value: f64,
    /// The knee it was read against, printed beside the value so the division is checkable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knee: Option<f64>,
    /// The weight in force.
    pub weight: f64,
    /// Which layer set that weight.
    pub origin: Origin,
    /// The contribution: hundredths of a point out of 100.
    pub points: Points,
    /// What it could have contributed at `1.0` — the ceiling the points are spent out of, so a low
    /// score says "this axis is quiet", not "this axis was skipped".
    pub cap: Points,
    /// Where the number came from.
    pub basis: String,
    /// The checkable claim: names, counts and the arithmetic.
    pub reason: String,
    /// Why there is no number, when there is not one — and who owns the missing input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unmeasured: Option<String>,
}

impl Measurement {
    /// A measurement that was made.
    #[must_use]
    pub fn measured(
        metric: Metric,
        raw: f64,
        knee: Option<f64>,
        weight: f64,
        origin: Origin,
        basis: String,
        reason: String,
    ) -> Self {
        Measurement {
            metric,
            measured: true,
            raw: Some(raw),
            raw_shown: format!("{} {}", metric.format_raw(raw), metric.spec().unit),
            value: metric.saturate(raw, knee),
            knee,
            weight,
            origin,
            points: Points(0),
            cap: Points(0),
            basis,
            reason,
            unmeasured: None,
        }
    }

    /// A measurement this revision could not make, and what would make it possible.
    #[must_use]
    pub fn unmeasured(metric: Metric, weight: f64, origin: Origin, why: String) -> Self {
        Measurement {
            metric,
            measured: false,
            raw: None,
            raw_shown: "—".to_string(),
            value: 0.0,
            knee: metric.spec().default_knee,
            weight,
            origin,
            points: Points(0),
            cap: Points(0),
            basis: "not measured".to_string(),
            reason: why.clone(),
            unmeasured: Some(why),
        }
    }
}

/// The complexity half of a module's facts, as `e3.1`'s spans produced them.
///
/// `points` is the sum over the module's **top-level** declarations only, and that restriction is the
/// whole reason the number means anything: `e3.1` counts each declaration's own subtree, so a nested
/// declaration's branches are already inside its enclosing declaration's count. Summing every
/// declaration would count a method's `if` twice — once as the method's and once as its `impl`'s — and
/// a file that nests deeply would score its nesting in every ancestor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComplexityFacts {
    /// The summed proxies: branches + loops + returns + nesting, top-level declarations only.
    pub points: usize,
    /// How many declarations were summed. A module of one 1 000-line function and a module of forty
    /// small ones have different shapes and the table says which is which.
    pub declarations: usize,
    /// The longest single declaration, in lines — the "god function" number beside the god module one.
    pub max_body_lines: usize,
    /// The deepest control-flow nesting found.
    pub max_nesting: usize,
}

/// The churn half of a module's facts: `e3.3`'s window, already cut to this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChurnFacts {
    /// Edge and node events on either side of this module.
    pub events: usize,
    /// How many revisions of the window touched it — the number that separates one big commit from a
    /// module that keeps coming back.
    pub revisions: usize,
    /// Revisions in the window: the denominator every rate in this module prints.
    pub window: usize,
}

impl ChurnFacts {
    /// Events per revision, with the window as the denominator even when the module was touched once.
    #[must_use]
    pub fn per_revision(&self) -> f64 {
        if self.window == 0 {
            return 0.0;
        }
        self.events as f64 / self.window as f64
    }
}

/// Everything the score needs to know about one module, already read out of the graph and `e3.1`.
///
/// This is the seam. [`crate::graph::lane`] can produce it from a materialised revision plus a churn
/// window; a test can hand-write it; a future scorecard can store it. Nothing below this point reads a
/// store, a clock or a file.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleFacts {
    /// The module path, as the graph keys it.
    pub module: String,
    /// The `[layers]` tag, `""` when untagged — printed, never used to decide a score.
    pub layer: String,
    /// Dependents whose contents are known.
    pub fan_in: usize,
    /// Dependents whose contents are unknown by construction (a wildcard edge, `s1` finding 2).
    pub fan_in_wildcard: usize,
    /// Dependents whose dependency is package-granular rather than file-granular (`s1` finding 4).
    pub fan_in_package: usize,
    /// The same three on the outbound side.
    pub fan_out: usize,
    /// Outbound wildcards.
    pub fan_out_wildcard: usize,
    /// Outbound package-granular edges.
    pub fan_out_package: usize,
    /// Code lines, from `e3.1`. `None` when the path has no structural record.
    pub code_lines: Option<usize>,
    /// The span proxies, from `e3.1`. `None` for the same reason.
    pub complexity: Option<ComplexityFacts>,
    /// The window's churn. `None` when there is no window to measure — a cold index, or a store that
    /// holds one revision.
    pub churn: Option<ChurnFacts>,
    /// Whether this module's declaration list may have shrunk rather than shrunk (`s1` finding 1): a
    /// macro-heavy scope whose outbound lists are a lower bound.
    pub incomplete: bool,
}

impl ModuleFacts {
    /// A module nobody depends on and who depends on nobody, with nothing known about its size.
    #[must_use]
    pub fn blank(module: impl Into<String>) -> Self {
        ModuleFacts {
            module: module.into(),
            layer: String::new(),
            fan_in: 0,
            fan_in_wildcard: 0,
            fan_in_package: 0,
            fan_out: 0,
            fan_out_wildcard: 0,
            fan_out_package: 0,
            code_lines: None,
            complexity: None,
            churn: None,
            incomplete: false,
        }
    }

    /// Total coupling, the way the classic metric counts it: both directions, wildcards included,
    /// because a wildcard dependency is a dependency whose contents are unknown and not a dependency
    /// that does not exist.
    #[must_use]
    pub fn coupling(&self) -> usize {
        self.fan_in_total() + self.fan_out_total()
    }

    /// Fan-in including the two uncertain kinds, which is the number the score uses.
    #[must_use]
    pub fn fan_in_total(&self) -> usize {
        self.fan_in + self.fan_in_wildcard + self.fan_in_package
    }

    /// Fan-out including the two uncertain kinds.
    #[must_use]
    pub fn fan_out_total(&self) -> usize {
        self.fan_out + self.fan_out_wildcard + self.fan_out_package
    }

    /// `fan_out / (fan_in + fan_out)`, `None` when the division has no denominator.
    ///
    /// `None` and not `0.0`: see the module docs. A module with no edges has no instability, and a
    /// table that printed `0.00` would be claiming it is stable.
    #[must_use]
    pub fn instability(&self) -> Option<f64> {
        let total = self.coupling();
        if total == 0 {
            return None;
        }
        Some(self.fan_out_total() as f64 / total as f64)
    }

    /// How many of the six numbers this module can answer.
    #[must_use]
    pub fn answerable(&self) -> usize {
        2 + usize::from(self.instability().is_some())
            + usize::from(self.code_lines.is_some())
            + usize::from(self.complexity.is_some())
            + usize::from(self.churn.is_some())
    }
}

/// The churn × complexity quadrant — the classic pair of axes, because a module that is both changing
/// and hard to read is where a refactor pays and a module that is only one of the two is where it does
/// not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Quadrant {
    /// Changing and hard to hold: the hotspot the ticket is about.
    HotComplex,
    /// Changing, and simple: usually a table, a fixture or a list being edited, not a tangle.
    HotPlain,
    /// Hard to hold, and quiet: leave it alone unless you have to go in.
    ColdComplex,
    /// Neither: the majority of every repository, and the correct answer to "not now".
    Quiet,
}

impl Quadrant {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Quadrant::HotComplex => "hot_complex",
            Quadrant::HotPlain => "hot_plain",
            Quadrant::ColdComplex => "cold_complex",
            Quadrant::Quiet => "quiet",
        }
    }

    /// The sentence a pane prints beside the axis values, including what the axes were compared
    /// against — a quadrant with no thresholds on it is a label, and a label is not a finding.
    #[must_use]
    pub const fn sentence(self) -> &'static str {
        match self {
            Quadrant::HotComplex => {
                "changing and hard to hold: this is the quadrant a refactor pays off in"
            }
            Quadrant::HotPlain => {
                "changing, and simple: churn without complexity is usually data being edited"
            }
            Quadrant::ColdComplex => {
                "hard to hold, and quiet: it is debt, not a moving target, and it can wait"
            }
            Quadrant::Quiet => "neither: nothing here says this turn should spend attention on it",
        }
    }
}

/// The shape a module's numbers make, when they make one.
///
/// Every shape is a stated bar over the saturated values, printed with the bar it cleared, and
/// `Shape::None` is the common case. The point of naming a god module is not the name: it is that a
/// reviewer who is told *which* axes made the call can disagree about that module instead of about
/// the whole ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Big, complex, depended on by many and still changing.
    God,
    /// Depended on by many, small and simple: a vocabulary module, where every edit fans out.
    Hub,
    /// Depends on almost everything, depended on by nothing: free to change, expensive to hold.
    Sink,
    /// Big and complex with few callers: the refactor nobody is forced to do, which is why it waits.
    Giant,
    /// Nothing cleared a bar. This is the honest majority of any repository.
    None,
}

impl Shape {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Shape::God => "god",
            Shape::Hub => "hub",
            Shape::Sink => "sink",
            Shape::Giant => "giant",
            Shape::None => "none",
        }
    }
}

/// One ranked module: the number, and every number that produced it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hotspot {
    /// The module, as the graph keys it.
    pub module: String,
    /// The `[layers]` tag, `""` when untagged.
    pub layer: String,
    /// 1-based place in the ranking. `0` when the table refused to rank at all.
    pub rank: usize,
    /// The score, `0..=100`. Meaningless unless [`Hotspot::scored`] is true.
    pub score: u8,
    /// Whether a score exists: false when every measured metric is weighted `0`, or when nothing was
    /// measured. A `0` a reader reads as "this module is fine" is worse than no number.
    pub scored: bool,
    /// Why there is no score, when there is not one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unscored: Option<String>,
    /// The exact sum of the contributions' points, so the printed total survives full precision.
    pub total: Points,
    /// The denominator actually used: the weight sum over the measured metrics. Printed because the
    /// reader is expected to redo the arithmetic.
    pub denominator: f64,
    /// How many of the six were measured.
    pub measured: usize,
    /// The six contributions, largest first, then in [`Metric::ALL`]'s order. Every metric appears,
    /// measured or not: nothing is dropped, because a dropped metric is a hole and an unmeasured one
    /// is a sentence.
    pub contributions: Vec<Measurement>,
    /// The churn × complexity quadrant, with the bars it was decided against beside it.
    pub quadrant: Quadrant,
    /// The shape the numbers make, and the bar that named it.
    pub shape: Shape,
    /// The shape's sentence, including the axes that made it. Empty for [`Shape::None`].
    pub shape_why: String,
    /// What this row cannot see: wildcards, package-granular edges, a shrunk declaration list, an
    /// unmeasured metric. Attached to the row and not to the table, because they differ per row.
    pub caveats: Vec<String>,
}

impl Hotspot {
    /// The contributions in the order a reviewer reads them.
    #[must_use]
    pub fn reasons(&self) -> &[Measurement] {
        &self.contributions
    }

    /// The measured contributions, largest first — the ones a pane prints as "why".
    #[must_use]
    pub fn measured(&self) -> Vec<&Measurement> {
        self.contributions
            .iter()
            .filter(|item| item.measured)
            .collect()
    }

    /// `hotspots-1 74 = fan_in 18.75 + complexity 16.00 + size 14.29 + …`, the same sentence shape
    /// `fathom why` uses so the two scores read alike.
    #[must_use]
    pub fn vector(&self) -> String {
        if !self.scored {
            return format!(
                "hotspots-1 — no score: {}",
                self.unscored.as_deref().unwrap_or("nothing was measured")
            );
        }
        if self.contributions.is_empty() {
            return format!("hotspots-1 {} = no contribution", self.score);
        }
        let parts: Vec<String> = self
            .contributions
            .iter()
            .filter(|item| item.measured)
            .map(|item| format!("{} {}", item.metric.id(), item.points))
            .collect();
        if parts.is_empty() {
            return format!("hotspots-1 {} = no contribution", self.score);
        }
        format!("hotspots-1 {} = {}", self.score, parts.join(" + "))
    }
}

/// The ranked answer, with the state that produced it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Table {
    /// The shape tag a machine reads first.
    pub contract: &'static str,
    /// The revision the graph was read at, as its caller named it.
    pub revision: String,
    /// The churn window, in revisions: the denominator of the churn column.
    pub window: usize,
    /// Whether a ranking exists. False is an *empty state*, not a ranking of zeros: a new repository
    /// has no history and a graph of one revision has no velocity, and printing either as a table of
    /// zeroes would be the nonsense this ticket's third acceptance criterion refuses.
    pub ranked: bool,
    /// Why there is no ranking, when there is not one. Printed instead of a table, never in addition
    /// to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
    /// The rows, best-placed first, truncated to `hotspots.top`.
    pub rows: Vec<Hotspot>,
    /// Modules in the graph at this revision — including the ones too small to place and the ones the
    /// truncation dropped.
    pub modules_seen: usize,
    /// Modules that got a score at all.
    pub modules_scored: usize,
    /// How many modules the truncation dropped, so "and 41 more" is a count and not a shrug.
    pub rows_dropped: usize,
    /// What the read could not do, from the caller: a partial chain, a grammar that is not compiled
    /// in, a file the extractor refused to open.
    pub notices: Vec<String>,
    /// The weights in force, with their origin — a ranking that does not name its weights cannot be
    /// reproduced next week.
    pub weights: Vec<Weight>,
    /// The knees in force, same reason.
    pub knees: Vec<Weight>,
}

/// One metric's effective number and where it came from — one type for the weights and the knees,
/// because provenance works the same way for both.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Weight {
    /// The metric.
    pub metric: Metric,
    /// The value the arithmetic used, already clamped.
    pub value: f64,
    /// Which layer won.
    pub origin: Origin,
    /// The value as written when it was outside its range and got clamped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clamped_from: Option<f64>,
}

impl Table {
    /// The empty state: no ranking, and the reason in the caller's own words.
    #[must_use]
    pub fn empty(
        revision: impl Into<String>,
        window: usize,
        reason: impl Into<String>,
        weights: Vec<Weight>,
        knees: Vec<Weight>,
        notices: Vec<String>,
    ) -> Self {
        Table {
            contract: super::CONTRACT,
            revision: revision.into(),
            window,
            ranked: false,
            empty_reason: Some(reason.into()),
            rows: Vec::new(),
            modules_seen: 0,
            modules_scored: 0,
            rows_dropped: 0,
            notices,
            weights,
            knees,
        }
    }

    /// The highest-ranked row, when there is one.
    #[must_use]
    pub fn first(&self) -> Option<&Hotspot> {
        self.rows.first()
    }

    /// The rows that cleared a shape bar — the "god modules" section of the pane, which is a filter
    /// over the ranking and not a second ranking, so it can never disagree with the first one.
    #[must_use]
    pub fn shaped(&self) -> Vec<&Hotspot> {
        self.rows
            .iter()
            .filter(|row| row.shape != Shape::None)
            .collect()
    }
}

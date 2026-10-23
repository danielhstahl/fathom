//! The knobs: what each metric is worth, where each curve saturates, how long a window is, and what
//! the read refuses to claim (`e3.8`, `reco-wky`).
//!
//! ```toml
//! [hotspots]
//! window = 30
//! min_history = 2
//! top = 20
//! dead_code_after = 3
//! god_bar = 0.50
//! flags = ["postgres"]
//! ignore = ["crates/**/generated/**"]
//!
//! [hotspots.weights]
//! fan_in = 0.25
//! fan_out = 0.10
//! instability = 0.10
//! size = 0.15
//! complexity = 0.20
//! churn = 0.20
//!
//! [hotspots.knee]
//! fan_in = 12
//! size = 600
//! ```
//!
//! # Three rules, each of which is a refusal
//!
//! The same three [`crate::risk::weights`] runs its weights by, inherited rather than reinvented, so
//! that a person who has tuned one table is not surprised by the second:
//!
//! * **Clamped, never renormalised behind your back.** A weight outside `0.0..=1.0` is clamped and the
//!   clamp is printed. The weights are not rescaled to sum to one: the score divides by the weight sum
//!   over the *measured* metrics ([`super::rank`]), so raising one weight moves the score toward that
//!   metric instead of shrinking every other one.
//! * **An unknown key is refused loudly.** `hotspots.weights` and `hotspots.knee` are open maps in the
//!   config layer, so `fanin = 0.3` resolves happily and goes nowhere. The parser notices, names the
//!   six, and says the value it ignored.
//! * **Every value carries where it came from.** `default` or `fathom.toml`, printed beside every
//!   contribution — "this module moved because of a line you wrote two months ago" has to be
//!   reconstructible from the output.
//!
//! A knee of `0` is refused rather than honoured: it would divide by zero in
//! [`Metric::saturate`] and every module would saturate to `1.0` at the first event, which is a
//! ranking of noise.

use serde::Serialize;
use toml_edit::Item as TomlItem;

use super::model::{Metric, Origin, Weight};

/// `hotspots.window`: how many revisions the churn velocity is measured over. `e3.3`'s window, and
/// the denominator the churn column prints. Thirty episodes is roughly a working month of an
/// agent-driven repository: long enough that a one-commit spike does not top the table, short enough
/// that a refactor that stopped three weeks ago stops being blamed.
pub const DEFAULT_WINDOW: usize = 30;

/// `hotspots.min_history`: how many revisions the store must hold before a churn column is printed at
/// all. Below it there is no velocity to measure and the metric is *unmeasured* rather than zero —
/// which is the difference between "this module is not being reworked" and "this repository is too
/// young to say".
pub const DEFAULT_MIN_HISTORY: usize = 2;

/// `hotspots.top`: how many rows a surface prints before it says how many it dropped.
pub const DEFAULT_TOP: usize = 20;

/// `hotspots.dead_code_after`: how many episodes a newly added symbol may go unreferenced before it
/// is a candidate. A symbol added in the current episode has had exactly no chance; three is the
/// shortest window in which "nobody has used it yet" starts to mean something.
pub const DEFAULT_DEAD_CODE_AFTER: usize = 3;

/// `hotspots.god_bar`: the saturated value a metric must reach to count as "high" for a shape call
/// ([`super::model::Shape`]) and for a quadrant. `0.5` is the knee itself — the raw number that equals
/// the published knee — which makes every bar in the file one sentence: *at least the knee*.
pub const DEFAULT_GOD_BAR: f64 = 0.5;

/// `hotspots.duplicate_block_lines`: how many folded lines a repeated block must be to be a dead-code
/// *smell* rather than a coincidence. `e4.3`'s `detectors.copy_paste_growth` uses the same 12 and for
/// the same reason: below it, `} else {` is a duplicate.
pub const DEFAULT_DUPLICATE_BLOCK_LINES: usize = 12;

/// The whole hotspot policy: the six weights, the six knees, the window, and the dead-code bars.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// One entry per metric, in [`Metric::ALL`]'s order.
    pub weights: Vec<Weight>,
    /// One entry per metric, in [`Metric::ALL`]'s order. `instability`'s knee is `None` and stays so:
    /// a ratio is not saturatable, and a config that tried would get a notice, not a lie.
    pub knees: Vec<Weight>,
    /// `hotspots.window`.
    pub window: usize,
    /// `hotspots.min_history`.
    pub min_history: usize,
    /// `hotspots.top`.
    pub top: usize,
    /// `hotspots.dead_code_after`.
    pub dead_code_after: usize,
    /// `hotspots.god_bar`.
    pub god_bar: f64,
    /// `hotspots.duplicate_block_lines`.
    pub duplicate_block_lines: usize,
    /// `hotspots.flags`: the feature flags this repository declares are **on**. The only input the
    /// gated-code class has — see [`super::deadcode`], which is explicit that this is a claim from the
    /// owner and not a resolved build configuration.
    pub flags: Vec<String>,
    /// `hotspots.ignore`: paths excluded from the dead-code sweep. Counted and named in the report,
    /// because a candidate list that quietly skipped a third of the repository is a candidate list
    /// about nothing.
    pub ignore: Vec<String>,
    /// One line per value that was read and could not be used.
    pub notices: Vec<String>,
    /// The file the values came from, when there was one.
    pub source: Option<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Policy {
    /// The built-in policy, for a repository that configures nothing.
    #[must_use]
    pub fn defaults() -> Self {
        Policy {
            weights: Metric::ALL
                .iter()
                .map(|metric| Weight {
                    metric: *metric,
                    value: metric.spec().default_weight,
                    origin: Origin::Default,
                    clamped_from: None,
                })
                .collect(),
            knees: Metric::ALL
                .iter()
                .map(|metric| Weight {
                    metric: *metric,
                    value: metric.spec().default_knee.unwrap_or(0.0),
                    origin: Origin::Default,
                    clamped_from: None,
                })
                .collect(),
            window: DEFAULT_WINDOW,
            min_history: DEFAULT_MIN_HISTORY,
            top: DEFAULT_TOP,
            dead_code_after: DEFAULT_DEAD_CODE_AFTER,
            god_bar: DEFAULT_GOD_BAR,
            duplicate_block_lines: DEFAULT_DUPLICATE_BLOCK_LINES,
            flags: Vec::new(),
            ignore: Vec::new(),
            notices: Vec::new(),
            source: None,
        }
    }

    /// The weight in force for one metric.
    #[must_use]
    pub fn weight(&self, metric: Metric) -> f64 {
        self.weights
            .iter()
            .find(|weight| weight.metric == metric)
            .map_or(0.0, |weight| weight.value)
    }

    /// Where that weight came from.
    #[must_use]
    pub fn origin(&self, metric: Metric) -> Origin {
        self.weights
            .iter()
            .find(|weight| weight.metric == metric)
            .map_or(Origin::Default, |weight| weight.origin)
    }

    /// The knee in force for one metric, `None` for a metric that is already a ratio.
    #[must_use]
    pub fn knee(&self, metric: Metric) -> Option<f64> {
        // A ratio has no curve to set the knee of; `Policy` refuses a `hotspots.knee.instability` at
        // load, and this is the second half of that refusal.
        let default = metric.spec().default_knee?;
        Some(
            self.knees
                .iter()
                .find(|knee| knee.metric == metric)
                .map(|knee| knee.value)
                .unwrap_or(default),
        )
    }

    /// Set a weight, clamping and recording the origin. `Err` names the reason a key was not a metric.
    pub fn set_weight(&mut self, name: &str, value: f64, origin: Origin) -> Result<(), String> {
        let Some(metric) = Metric::parse(name) else {
            return Err(format!(
                "hotspots.weights.{name} is not a metric of this score; the six are {}",
                Metric::ids().join(", ")
            ));
        };
        let Some(clamped) = clamp_weight(&mut self.notices, "weights", name, value) else {
            return Ok(());
        };
        if let Some(slot) = self
            .weights
            .iter_mut()
            .find(|weight| weight.metric == metric)
        {
            slot.value = clamped;
            slot.origin = origin;
            slot.clamped_from = clamped_from(value, clamped);
        }
        Ok(())
    }

    /// The same bar as a shape call, so a quadrant, a shape and a printed "high" agree.
    #[must_use]
    pub fn high(&self, value: f64) -> bool {
        value >= self.god_bar
    }

    /// Whether a path is excluded from the dead-code sweep by `hotspots.ignore`.
    ///
    /// Glob-style, with the two shapes a repository actually writes: a `**` pattern matched with
    /// [`globset`], and a bare directory or path prefix matched by containment. A bad glob is a notice
    /// and no match, never a silent "nothing matches".
    #[must_use]
    pub fn ignores(&self, path: &str) -> bool {
        self.ignore
            .iter()
            .any(|pattern| pattern_matches(pattern, path))
    }

    /// Whether a feature flag is declared on. Case-sensitive, because every flag spelling this reads is
    /// case-sensitive in the language that wrote it.
    #[must_use]
    pub fn flag_on(&self, flag: &str) -> bool {
        self.flags.iter().any(|known| known == flag)
    }

    /// What the knobs imply for each other, as notices.
    #[must_use]
    pub fn coherence(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.window < self.min_history {
            out.push(format!(
                "hotspots.window ({}) is below hotspots.min_history ({}): the churn column can never \
                 be measured, so one of the six metrics is always missing",
                self.window, self.min_history
            ));
        }
        if self.weights.iter().all(|weight| weight.value == 0.0) {
            out.push(
                "every hotspots.weights entry is 0, so no module can be scored — the table prints a \
                 refusal rather than a ranking of zeroes"
                    .to_string(),
            );
        }
        if self.god_bar <= 0.0 || self.god_bar > 1.0 {
            out.push(format!(
                "hotspots.god_bar is {}, which is outside the saturated range: every shape call and \
                 every quadrant would be decided by a bar no metric can cross",
                self.god_bar
            ));
        }
        out
    }
}

/// One value, clamped into `0.0..=1.0`, with a notice for a value that is not a number at all.
fn clamp_weight(notices: &mut Vec<String>, table: &str, name: &str, value: f64) -> Option<f64> {
    if value.is_finite() {
        return Some(value.clamp(0.0, 1.0));
    }
    notices.push(format!(
        "hotspots.{table}.{name} is not a number, so its previous value stands"
    ));
    None
}

/// `Some(original)` when the clamp moved the value, so the report can print both.
fn clamped_from(written: f64, clamped: f64) -> Option<f64> {
    if (clamped - written).abs() > f64::EPSILON {
        Some(written)
    } else {
        None
    }
}

/// A glob when it looks like one, a prefix when it does not.
///
/// `crates/x/` and `crates/x` both mean "under there"; `crates/**/generated/**` is matched as a glob.
/// A pattern that fails to compile as a glob is reported once by [`notices_of`] and never matches.
#[must_use]
fn pattern_matches(pattern: &str, path: &str) -> bool {
    if pattern.contains('*') || pattern.contains('?') {
        return globset::Glob::new(pattern)
            .map(|glob| glob.compile_matcher().is_match(path))
            .unwrap_or(false);
    }
    let trimmed = pattern.trim_end_matches('/');
    path == trimmed || path.starts_with(&format!("{trimmed}/"))
}

/// Read `[hotspots]`, `[hotspots.weights]` and `[hotspots.knee]` out of a `fathom.toml` document.
///
/// A document that does not parse is a notice and the defaults — the rule every reader of
/// `fathom.toml` in this workspace follows, because a file that fails to parse and then behaves like an
/// empty one is a silent behaviour change. One bad value is a notice and the rest of the table.
#[must_use]
pub fn policy_from_config_text(text: &str) -> (Policy, Vec<String>) {
    let mut policy = Policy::defaults();
    let mut notices = Vec::new();
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(err) => {
            notices.push(format!(
                "fathom.toml does not parse, so every hotspot knob stayed at its built-in default: \
                 {err}"
            ));
            return (policy, notices);
        }
    };
    let Some(table) = document.get("hotspots").and_then(TomlItem::as_table) else {
        return (policy, notices);
    };
    for (key, name) in [
        ("window", "window"),
        ("min_history", "min_history"),
        ("top", "top"),
        ("dead_code_after", "dead_code_after"),
        ("duplicate_block_lines", "duplicate_block_lines"),
    ] {
        let Some(value) = table.get(key).and_then(TomlItem::as_value) else {
            continue;
        };
        let Some(number) = value.as_integer() else {
            notices.push(format!(
                "hotspots.{name} is not a whole number, so it was not used"
            ));
            continue;
        };
        if number < 1 {
            notices.push(format!(
                "hotspots.{name} is {number}, which would bound the report to nothing; kept the \
                 built-in default"
            ));
            continue;
        }
        let width = usize::try_from(number).unwrap_or(usize::MAX);
        match name {
            "window" => policy.window = width,
            "min_history" => policy.min_history = width,
            "top" => policy.top = width,
            "dead_code_after" => policy.dead_code_after = width,
            _ => policy.duplicate_block_lines = width,
        }
    }
    if let Some(value) = table.get("god_bar").and_then(TomlItem::as_value) {
        match value
            .as_float()
            .or_else(|| value.as_integer().map(|n| n as f64))
        {
            Some(number) => {
                if (0.05..=1.0).contains(&number) {
                    policy.god_bar = number;
                } else {
                    notices.push(format!(
                        "hotspots.god_bar is {number}, outside 0.05..=1.0 (a bar at or below 0 makes \
                         every module a god module and a bar above 1 makes none); kept the built-in \
                         default"
                    ));
                }
            }
            None => {
                notices.push("hotspots.god_bar is not a number, so it was not used".to_string())
            }
        }
    }
    for (key, name) in [("flags", "flags"), ("ignore", "ignore")] {
        let Some(list) = table
            .get(key)
            .and_then(TomlItem::as_value)
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        let mut entries = Vec::new();
        for entry in list.iter() {
            if let Some(text) = entry.as_str() {
                entries.push(text.to_string());
            } else {
                notices.push(format!(
                    "hotspots.{name} has an entry that is not a string; it was skipped"
                ));
            }
        }
        // Both keys are `merge = "extend"` in the schema: a repository that names one more ignored
        // directory has not asked to un-inherit whatever the user config declared.
        let slot = if name == "flags" {
            &mut policy.flags
        } else {
            &mut policy.ignore
        };
        for entry in entries {
            if !slot.contains(&entry) {
                slot.push(entry);
            }
        }
    }
    // The two open maps, read in the order they are documented. Each unknown key is a notice naming the
    // six, because an open map in the config layer happily resolves a typo and then ignores it silently.
    if let Some(sub) = table.get("weights").and_then(TomlItem::as_table) {
        let mut lines = Vec::new();
        for (key, item) in sub.iter() {
            let Some(value) = number(item) else {
                lines.push(format!(
                    "hotspots.weights.{key} is not a number, so its previous value stands"
                ));
                continue;
            };
            if Metric::parse(key).is_none() {
                lines.push(format!(
                    "hotspots.weights.{key} is not a metric of this score; the six are {}",
                    Metric::ids().join(", ")
                ));
                continue;
            }
            if let (Some(clamped), Some(slot)) = (
                clamp_weight(&mut lines, "weights", key, value),
                policy
                    .weights
                    .iter_mut()
                    .find(|weight| weight.metric.id() == key),
            ) {
                slot.value = clamped;
                slot.origin = Origin::Repo;
                slot.clamped_from = clamped_from(value, clamped);
            }
        }
        notices.extend(lines);
    }
    if let Some(sub) = table.get("knee").and_then(TomlItem::as_table) {
        for (key, item) in sub.iter() {
            let Some(value) = number(item) else {
                notices.push(format!(
                    "hotspots.knee.{key} is not a number, so it was not used"
                ));
                continue;
            };
            let Some(metric) = Metric::parse(key) else {
                notices.push(format!(
                    "hotspots.knee.{key} is not a metric of this score; the six are {}",
                    Metric::ids().join(", ")
                ));
                continue;
            };
            if metric.spec().default_knee.is_none() {
                notices.push(format!(
                    "hotspots.knee.{key} was ignored: {key} is already a 0.0..=1.0 ratio, so there is \
                     no curve to set the knee of"
                ));
                continue;
            }
            if value <= 0.0 || !value.is_finite() {
                notices.push(format!(
                    "hotspots.knee.{key} is {value}; a knee at or below zero would saturate every \
                     module to 1.0 at its first event, so the built-in default stands"
                ));
                continue;
            }
            if let Some(slot) = policy.knees.iter_mut().find(|knee| knee.metric == metric) {
                slot.value = value;
                slot.origin = Origin::Repo;
                slot.clamped_from = None;
            }
        }
    }
    for line in policy.coherence() {
        notices.push(line);
    }
    // A malformed `hotspots.ignore` glob is refused where the list is read, not only where it is
    // checked: a report that quietly stopped excluding anything has to say so on every run.
    let malformed = malformed_globs(&policy);
    for line in malformed {
        if !notices.contains(&line) {
            notices.push(line);
        }
    }
    policy.notices = notices.clone();
    (policy, notices)
}

/// One TOML number, accepting `600` where a reviewer meant `600.0`.
fn number(item: &TomlItem) -> Option<f64> {
    let value = item.as_value()?;
    value
        .as_float()
        .or_else(|| value.as_integer().map(|number| number as f64))
}

/// The malformed-glob complaints about a policy's `hotspots.ignore` list.
///
/// Raised at load — so a report prints them without being asked — and again by [`notices_of`], for a
/// caller that built a [`Policy`] some other way. A broken pattern matches nothing, which is the silent
/// failure this refuses: a repository that meant to exclude its generated tree and mistyped the glob
/// would otherwise read a generated file as candidate dead code.
#[must_use]
pub fn malformed_globs(policy: &Policy) -> Vec<String> {
    policy
        .ignore
        .iter()
        .filter(|pattern| {
            (pattern.contains('*') || pattern.contains('?')) && globset::Glob::new(pattern).is_err()
        })
        .map(|pattern| {
            format!(
                "hotspots.ignore entry `{pattern}` is not a valid glob, so it matches nothing and \
                 nothing under it is actually excluded"
            )
        })
        .collect()
}

/// The notices a caller prints about a policy it was handed: what the parser refused, the malformed
/// globs `ignores` refuses to fail on, and the knobs that disagree with each other.
#[must_use]
pub fn notices_of(policy: &Policy) -> Vec<String> {
    let mut out = policy.notices.clone();
    for line in malformed_globs(policy)
        .into_iter()
        .chain(policy.coherence())
    {
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

/// The `[hotspots]` block as a reviewer would paste it: only what differs from the built-in defaults,
/// because a file that restates every default hides what it means.
#[must_use]
pub fn to_toml(policy: &Policy) -> String {
    let defaults = Policy::defaults();
    let mut out = String::from("[hotspots]\n");
    let mut wrote = false;
    if policy.window != defaults.window {
        out.push_str(&format!("window = {}\n", policy.window));
        wrote = true;
    }
    if policy.min_history != defaults.min_history {
        out.push_str(&format!("min_history = {}\n", policy.min_history));
        wrote = true;
    }
    if policy.top != defaults.top {
        out.push_str(&format!("top = {}\n", policy.top));
        wrote = true;
    }
    if policy.dead_code_after != defaults.dead_code_after {
        out.push_str(&format!("dead_code_after = {}\n", policy.dead_code_after));
        wrote = true;
    }
    if (policy.god_bar - defaults.god_bar).abs() > f64::EPSILON {
        out.push_str(&format!("god_bar = {:.2}\n", policy.god_bar));
        wrote = true;
    }
    if !policy.flags.is_empty() {
        out.push_str(&format!(
            "flags = [{}]\n",
            policy
                .flags
                .iter()
                .map(|flag| format!("\"{flag}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        wrote = true;
    }
    if !wrote {
        out.push_str("# every hotspot knob is at its built-in default\n");
    }
    let tuned: Vec<String> = policy
        .weights
        .iter()
        .filter(|weight| weight.origin != Origin::Default)
        .map(|weight| format!("{} = {:.2}", weight.metric.id(), weight.value))
        .collect();
    if !tuned.is_empty() {
        out.push_str("\n[hotspots.weights]\n");
        for line in tuned {
            out.push_str(&format!("{line}\n"));
        }
    }
    let knees: Vec<String> = policy
        .knees
        .iter()
        .filter(|knee| knee.origin != Origin::Default)
        .map(|knee| format!("{} = {:.2}", knee.metric.id(), knee.value))
        .collect();
    if !knees.is_empty() {
        out.push_str("\n[hotspots.knee]\n");
        for line in knees {
            out.push_str(&format!("{line}\n"));
        }
    }
    out
}

/// The serialisable form of the knobs, for the JSON the surfaces read: the provenance list, which is
/// what makes a stored ranking readable a year later.
#[derive(Debug, Clone, Serialize)]
pub struct Provenance {
    /// The weights in force, with their origin.
    pub weights: Vec<Weight>,
    /// The knees in force, with their origin.
    pub knees: Vec<Weight>,
    /// The window the churn column used.
    pub window: usize,
    /// The bars in force, by name, so a JSON reader can print what a row was judged against.
    pub bars: Vec<(&'static str, String)>,
}

impl Provenance {
    /// Snapshot the knobs a report was built with.
    #[must_use]
    pub fn of(policy: &Policy) -> Self {
        Provenance {
            weights: policy.weights.clone(),
            knees: policy.knees.clone(),
            window: policy.window,
            bars: vec![
                ("min_history", policy.min_history.to_string()),
                ("top", policy.top.to_string()),
                ("dead_code_after", policy.dead_code_after.to_string()),
                ("god_bar", format!("{:.2}", policy.god_bar)),
                (
                    "duplicate_block_lines",
                    policy.duplicate_block_lines.to_string(),
                ),
            ],
        }
    }
}

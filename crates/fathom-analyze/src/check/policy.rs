//! `[check]` — the per-repository policy the gate runs, and the refusals that keep it honest
//! (`e6.6`, `reco-ngk`).
//!
//! One table decides what the gate looks at, how loudly each family speaks, and where the run turns red.
//! It is per-repository (it lives in `fathom.toml`, next to the `[layering]` rules and the `[api]` surface
//! it judges), it is versioned ([`Policy::version`]), and — the part that makes it infrastructure rather
//! than a knob — **it refuses rather than defaults** wherever a wrong value would mean a check that looks
//! like it runs and does not.
//!
//! ```toml
//! [check]
//! version = 1                     # a version this build cannot read is a refusal, not a partial read
//! mode = "deny"                   # off | warn | deny — the volume of the whole gate
//! fail_on = "warning"             # note | warning | error | never
//! base = "origin/main"            # the default base when --base is not given
//! head = "HEAD"
//! require = ["layering", "api"]   # a required check that cannot run is a failure, not a shrug
//! ignore = ["tests:crates/fathom-fixtures/**"]  # silenced, counted, and printed as silenced
//! max_findings = 40               # listed findings per check; the counts stay whole
//! api_files = 6000                # blobs read per side by the api check; hitting the cap blocks it
//! risk_episodes = 40              # episodes the scorer may read in one run
//! max_episodes = 200              # episodes in a range before the episode-shaped checks refuse to guess
//!
//! [check.severity]                # `off` switches one check off, and says so in every output
//! layering = "error"
//! cycles = "error"
//! api = "error"
//! dependencies = "warning"
//! tests = "warning"
//! sign-off = "warning"
//!
//! [check.tests]
//! min_new_lines = 200             # below this, "no tests" is a review comment, not a gate
//! min_tested_ratio = 0.5          # test lines added, as a share of logic lines added
//!
//! [check.dependencies]
//! new_direct = "warning"
//! major_change = "warning"
//! new_source = "warning"
//! downgrade = "warning"
//! forbid = ["left-pad"]           # package names, exact or `glob`
//!
//! [check.risk]
//! threshold = 70                  # the score at or over which an unsigned episode is a finding
//! ```
//!
//! # The failure modes, all of them loud
//!
//! The acceptance criterion is "no silent passes", and the way a gate passes silently is by *not running*
//! while still exiting `0`. Four defences, all of them visible in the report:
//!
//! * **a value that will not parse is a refusal** ([`PolicyError`], exit `5` `CONFIG_INVALID`): an
//!   unreadable `fail_on` is not defaulted to `warn`, because the difference between `warn` and `never`
//!   is the whole reason the file says one and not the other. Same for a bad severity, a bad mode, an
//!   unparseable ratio, a zero cap, an unknown key (a typo'd key is an unwritten key, and an unwritten
//!   key is the default), and an unknown check name in `[check.severity]`, `require` or `ignore`.
//! * **a policy version this build cannot read is a refusal.** Partial understanding of a newer policy
//!   is a gate that enforces some of what the file says.
//! * **a check that could not run is a state** ([`super::finding::Status::Blocked`]) with the reason, and
//!   `require` promotes it into a failure.
//! * **a silenced finding is counted.** `ignore` never makes a finding invisible, only non-blocking.
//!
//! # What this module deliberately is not
//!
//! **Not a config system.** It reads one table out of the text the composition root already read
//! (`ADR-002 §4.5`: nothing below `fathom-cli` opens a file), and it reads no other layer — a user-level
//! `[check]` is not a thing, on purpose: a gate whose threshold can be loosened by whoever set a shell
//! variable in the runner is a gate with a bypass.

use std::collections::BTreeMap;

use serde::Serialize;

use super::finding::{Check, Severity};
use crate::ratchet::Mode;

/// The highest policy layout this build understands.
///
/// A file may declare `check.version`; a declaration *above* this is refused rather than partially read.
/// Bumping this number is the act that says "a policy written for the new layout means something the old
/// build would get wrong".
pub const POLICY_VERSION: u32 = 1;

/// The default listed findings per check before the rest are folded into a count.
pub const DEFAULT_MAX_FINDINGS: usize = 40;

/// The default number of blobs the api check may read per side.
///
/// Two sides of a real repository is the cost, not one: the surface has to be extracted from what the base
/// had, or a file that was never touched reads as a deletion. Six thousand files per side is a large
/// monorepo's public surface and lands well inside the gate's time budget; past it the check says it was
/// capped rather than reporting a surface it did not read.
pub const DEFAULT_API_FILE_CAP: usize = 6_000;

/// The default number of episodes the risk scorer may read in one run.
///
/// Scoring one episode is a store read plus a detector pass over the recent corpus; forty is minutes-bounds
/// on a huge repository and covers any range a human reviews in one pull request.
pub const DEFAULT_RISK_EPISODE_CAP: usize = 40;

/// The default ceiling on episodes in a checked range.
pub const DEFAULT_EPISODE_CAP: usize = 200;

/// The size, in added logic lines, below which "no tests" is not a gate failure.
pub const DEFAULT_MIN_NEW_LINES: u64 = 200;

/// The default share of added logic that must arrive with test lines beside it.
pub const DEFAULT_MIN_TESTED_RATIO: f64 = 0.5;

/// The risk score at or over which an episode is "high-risk" for the sign-off check.
///
/// Not `risk.tau`, deliberately: `tau` routes a queue ([`crate::risk::score`]), and the pipeline recipe is
/// explicit that a routing knob must not become a merge threshold, because a repository-wide number on it
/// fails a large honest change and passes a small dishonest one. This threshold is paired with *"and nobody
/// signed it off"*, which is what makes it a claim about the review rather than about the size of a diff.
pub const DEFAULT_RISK_THRESHOLD: u8 = 70;

/// The policy could not be understood, and the gate refuses to guess at it.
///
/// Carries the sentence a human needs: the key, what was found, and what to type. The composition root
/// turns this into exit `5` (`CONFIG_INVALID`) — the same code `fathom.toml` fails the schema with, so a
/// pipeline that already handles `5` needs no new branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyError(pub String);

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PolicyError {}

/// What `[check.tests]` refuses.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TestPolicy {
    /// Added logic lines below which the check does not fire.
    pub min_new_lines: u64,
    /// Test lines added as a share of logic lines added, below which it fires.
    pub min_tested_ratio: f64,
}

impl Default for TestPolicy {
    fn default() -> Self {
        Self {
            min_new_lines: DEFAULT_MIN_NEW_LINES,
            min_tested_ratio: DEFAULT_MIN_TESTED_RATIO,
        }
    }
}

/// What `[check.dependencies]` refuses. Each field is `None` when that family is switched off.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct DependencyPolicy {
    /// A dependency that is now direct and was not.
    pub new_direct: Option<Severity>,
    /// A direct dependency that moved a major version, up or down.
    pub major_change: Option<Severity>,
    /// A new registry, git URL, vendored or path source.
    pub new_source: Option<Severity>,
    /// A direct dependency that moved down.
    pub downgrade: Option<Severity>,
    /// Package names that may not arrive at all, exact or glob.
    pub forbid: Vec<String>,
}

/// What `[check.risk]` refuses.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RiskPolicy {
    /// The score at or over which an unsigned episode is a finding.
    pub threshold: u8,
}

impl Default for RiskPolicy {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_RISK_THRESHOLD,
        }
    }
}

/// The gate's policy in force: one struct, built once, printed by every output format.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Policy {
    /// The declared layout version ([`POLICY_VERSION`]).
    pub version: u32,
    /// The volume of the whole gate. `Off` reports and decides nothing, and says so.
    #[serde(serialize_with = "crate::check::policy::serialize_mode")]
    pub mode: Mode,
    /// The threshold at or above which a finding fails the run. `None` is `never`.
    pub fail_on: Option<Severity>,
    /// The default base revision, when the file names one.
    pub base: Option<String>,
    /// The default head revision.
    pub head: String,
    /// The checks that must run; one that cannot is a failure rather than a notice.
    pub require: Vec<Check>,
    /// `check/rule` keys silenced, each with the reason count the report prints.
    pub ignore: Vec<String>,
    /// The volume of each check, in [`Check::ALL`] order. `None` is `off`.
    #[serde(serialize_with = "crate::check::policy::serialize_severities")]
    pub severity: [Option<Severity>; 6],
    /// Listed findings per check before the tail is folded into a count.
    pub max_findings: usize,
    /// The api check's per-side blob ceiling.
    pub api_file_cap: usize,
    /// The risk check's episode ceiling.
    pub risk_episode_cap: usize,
    /// The ceiling on episodes a range may hold before the episode-shaped checks refuse to guess.
    pub episode_cap: usize,
    /// `[check.tests]`.
    pub tests: TestPolicy,
    /// `[check.dependencies]`.
    pub dependencies: DependencyPolicy,
    /// `[check.risk]`.
    pub risk: RiskPolicy,
    /// The soft notes: what the file said that was understood but worth repeating (a silenced rule, a
    /// defaulted `base`).
    pub notices: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Serialize the gate's [`Mode`] as its stored spelling. `Mode` is [`crate::ratchet`]'s type and carries
/// no `Serialize` of its own — it is a config enum, not a wire type, so the wire shape is written here.
fn serialize_mode<S: serde::Serializer>(mode: &Mode, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(mode.as_str())
}

/// Serialize `[Option<Severity>; 6]` as the `{check: level}` map the JSON prints, so the machine-readable
/// policy reads exactly like the `[check.severity]` table it came from.
pub fn serialize_severities<S: serde::Serializer>(
    severities: &[Option<Severity>; 6],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap as _;
    let mut map = serializer.serialize_map(Some(severities.len()))?;
    for (check, severity) in Check::ALL.iter().zip(severities) {
        map.serialize_entry(check.as_str(), &severity.map_or("off", Severity::as_str))?;
    }
    map.end()
}

impl Policy {
    /// The built-in policy: nothing declared, everything at its default volume.
    #[must_use]
    pub fn defaults() -> Self {
        let mut severity = [None; 6];
        for (slot, check) in Check::ALL.iter().enumerate() {
            severity[slot] = Some(match check {
                Check::Layering | Check::Cycles | Check::Api => Severity::Error,
                Check::Dependencies | Check::Tests | Check::SignOff => Severity::Warning,
            });
        }
        Self {
            version: POLICY_VERSION,
            mode: Mode::Warn,
            fail_on: Some(Severity::Warning),
            base: None,
            head: "HEAD".to_owned(),
            require: Vec::new(),
            ignore: Vec::new(),
            severity,
            max_findings: DEFAULT_MAX_FINDINGS,
            api_file_cap: DEFAULT_API_FILE_CAP,
            risk_episode_cap: DEFAULT_RISK_EPISODE_CAP,
            episode_cap: DEFAULT_EPISODE_CAP,
            tests: TestPolicy::default(),
            dependencies: default_dependencies(),
            risk: RiskPolicy::default(),
            notices: Vec::new(),
        }
    }

    /// Read `[check]` out of a `fathom.toml` document.
    ///
    /// A missing `[check]` is not an error — the defaults are a policy, and a repository that has never
    /// configured the gate still gets a gate. A `[check]` that is *present* and wrong is
    /// [`PolicyError`], because present-and-wrong is the case where the author's intent and the built-in
    /// default disagree and only one of them can win.
    ///
    /// # Errors
    ///
    /// [`PolicyError`] when the table cannot be understood: an unparsable document, a version this build
    /// cannot read, a value of the wrong kind, an unknown key, a check or severity name that is not one.
    pub fn from_config_text(text: &str) -> Result<Self, PolicyError> {
        let mut policy = Self::defaults();
        if text.trim().is_empty() {
            policy
                .notices
                .push("fathom.toml is empty, so the gate runs on its built-in defaults".to_owned());
            return Ok(policy);
        }
        let document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|err| PolicyError(format!("fathom.toml does not parse: {err}")))?;
        let Some(table) = document.get("check").and_then(toml_edit::Item::as_table) else {
            policy.notices.push(
                "fathom.toml declares no [check], so the gate runs on its built-in defaults: mode \
                 `warn`, fail at `warning`, every check at its default volume"
                    .to_owned(),
            );
            return Ok(policy);
        };

        // The version first: every later read is a read of *this* layout.
        if let Some(value) = table.get("version") {
            let declared = value
                .as_value()
                .and_then(toml_edit::Value::as_integer)
                .ok_or_else(|| PolicyError("check.version must be an integer".to_owned()))?;
            if declared > i64::from(POLICY_VERSION) {
                return Err(PolicyError(format!(
                    "check.version is {declared}, and this build understands version \
                     {POLICY_VERSION}; upgrade fathom rather than let a policy be partly understood"
                )));
            }
            if declared < 1 {
                return Err(PolicyError(format!(
                    "check.version is {declared}, which was never a policy layout; expected 1..={POLICY_VERSION}"
                )));
            }
            policy.version = declared as u32;
        }

        if let Some(value) = table.get("mode") {
            policy.mode = parse_mode(str_of(value, "check.mode")?, "check.mode")?;
        }
        policy.fail_on =
            match table.get("fail_on") {
                Some(value) => {
                    let text = str_of(value, "check.fail_on")?;
                    match text.as_str() {
                        "never" => None,
                        other => Some(Severity::parse(other).ok_or_else(|| {
                            bad("check.fail_on", other, "note|warning|error|never")
                        })?),
                    }
                }
                None => policy.fail_on,
            };
        policy.base = match table.get("base") {
            Some(value) => Some(str_of(value, "check.base")?),
            None => None,
        };
        if let Some(value) = table.get("head") {
            policy.head = str_of(value, "check.head")?;
        }
        if let Some(value) = table.get("require") {
            for name in string_list(value, "check.require")? {
                policy.require.push(
                    Check::parse(&name)
                        .ok_or_else(|| bad("check.require", &name, "a check name"))?,
                );
            }
            policy
                .require
                .sort_by_key(|check| check.as_str().to_owned());
            policy.require.dedup();
        }
        if let Some(value) = table.get("ignore") {
            for pattern in string_list(value, "check.ignore")? {
                // `ignore` entries are `check[:rule]` keys. The family half must name a check: a pattern
                // that names nothing silences nothing, which is the mirror image of the typo that
                // silences something by accident, and both are refusals rather than surprises.
                let family = pattern.split(':').next().unwrap_or_default();
                if Check::parse(family).is_none() {
                    return Err(bad(
                        "check.ignore",
                        &pattern,
                        "`<check>` or `<check>:<rule>`, e.g. `tests:fixtures`",
                    ));
                }
                policy.ignore.push(pattern);
            }
        }
        for (key, name) in [
            ("max_findings", "check.max_findings"),
            ("api_files", "check.api_files"),
            ("risk_episodes", "check.risk_episodes"),
            ("max_episodes", "check.max_episodes"),
        ] {
            if let Some(value) = table.get(key) {
                let number = value
                    .as_value()
                    .and_then(toml_edit::Value::as_integer)
                    .ok_or_else(|| PolicyError(format!("{name} must be an integer")))?;
                if number < 1 {
                    return Err(PolicyError(format!(
                        "{name} is {number}; a bound of zero would bound the answer to nothing"
                    )));
                }
                let sized = usize::try_from(number).unwrap_or(usize::MAX);
                match key {
                    "max_findings" => policy.max_findings = sized,
                    "api_files" => policy.api_file_cap = sized,
                    "risk_episodes" => policy.risk_episode_cap = sized,
                    _ => policy.episode_cap = sized,
                }
            }
        }

        // The four sub-tables are keys of `[check]` in TOML's own terms — `[check.severity]` is read as
        // `check["severity"]` — so they belong in the known list as plainly as `mode` does, or the
        // per-check volumes would be a refusal.
        let known = [
            "version",
            "mode",
            "fail_on",
            "base",
            "head",
            "require",
            "ignore",
            "max_findings",
            "api_files",
            "risk_episodes",
            "max_episodes",
            "severity",
            "tests",
            "dependencies",
            "risk",
        ];
        for key in table.iter().map(|(key, _)| key) {
            if !known.contains(&key) {
                return Err(PolicyError(format!(
                    "check.{key} is not a key this build reads; the keys are {} — a key you wrote and \
                     the gate ignored is a default you did not choose",
                    known.join(", ")
                )));
            }
        }

        policy.severity = read_severities(&document)?;
        policy.tests = read_tests(&document)?;
        policy.dependencies = read_dependencies(&document)?;
        policy.risk = read_risk(&document)?;
        Ok(policy)
    }

    /// The volume of one check: `None` when it is switched off.
    #[must_use]
    pub fn severity(&self, check: Check) -> Option<Severity> {
        self.severity[Check::ALL.iter().position(|c| *c == check).unwrap_or(0)]
    }

    /// Whether the gate decides anything at all. `mode = off` reports and exits `0`.
    #[must_use]
    pub fn decides(&self) -> bool {
        self.mode != Mode::Off
    }

    /// Whether a failing verdict is allowed to block the pipeline. `mode = warn` says *no*: it objects,
    /// prints, annotates and uploads, and exits `0`.
    ///
    /// This is the mirror of [`crate::ratchet`]'s `mode`, and it is the reason the shipped default can be
    /// `warn`: a gate that blocks on the day it is installed gets uninstalled, and one that tells the truth
    /// on the day it is installed gets turned up when the team trusts it.
    #[must_use]
    pub fn enforces(&self) -> bool {
        self.mode == Mode::Deny
    }

    /// Whether `check` is in the required list.
    #[must_use]
    pub fn requires(&self, check: Check) -> bool {
        self.require.contains(&check)
    }

    /// Whether a finding's `check/rule` key is silenced, and by which pattern.
    ///
    /// The matcher is a prefix on the `check` half and a substring on the rule half, so `tests` silences a
    /// whole family, `tests:fixtures` silences one rule, and `fixtures` silences nothing (which is stated
    /// at load rather than discovered at review time).
    #[must_use]
    pub fn silenced_by(&self, key: &str) -> Option<&str> {
        let (check, rule) = key.split_once('/').unwrap_or((key, ""));
        self.ignore
            .iter()
            .find(|pattern| {
                let (family, sub) = match pattern.split_once(':') {
                    Some((family, sub)) => (family, Some(sub)),
                    None => (pattern.as_str(), None),
                };
                family == check && sub.is_none_or(|needle| rule.contains(needle))
            })
            .map(String::as_str)
    }
}

/// Parse the three mode spellings, refusing anything else.
fn parse_mode(text: String, name: &str) -> Result<Mode, PolicyError> {
    Mode::parse(&text).ok_or_else(|| bad(name, &text, "off|warn|deny"))
}

/// The refusing form of "this value is not one of those".
fn bad(name: &str, found: &str, expected: &str) -> PolicyError {
    PolicyError(format!(
        "{name} is `{found}`, which is not {expected}; the gate refuses to guess which you meant"
    ))
}

/// One string value, or the refusal that names the key.
fn str_of(value: &toml_edit::Item, name: &str) -> Result<String, PolicyError> {
    value
        .as_value()
        .and_then(toml_edit::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| PolicyError(format!("{name} must be a string")))
}

/// One array-of-strings value.
fn string_list(value: &toml_edit::Item, name: &str) -> Result<Vec<String>, PolicyError> {
    let array = value
        .as_value()
        .and_then(toml_edit::Value::as_array)
        .ok_or_else(|| PolicyError(format!("{name} must be an array of strings")))?;
    let mut out = Vec::with_capacity(array.len());
    for entry in array.iter() {
        out.push(
            entry
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| PolicyError(format!("{name} must contain only strings")))?,
        );
    }
    Ok(out)
}

/// One `[check.<child>]` table, or `None` when the file does not have one.
///
/// The two-level lookup is the whole function, and getting it wrong is the quietest bug a config reader can
/// write: `document.get("check.severity")` asks the *root* table for a key literally named
/// `check.severity`, which no TOML file has, so a hand-written sub-table would read as absent and every
/// volume in it would silently stay at its default. `[check.severity]` lives *inside* `check`, and so must
/// this lookup.
fn sub_table(
    document: &toml_edit::DocumentMut,
    parent: &str,
    child: &str,
    keys: &[&str],
) -> Result<Option<toml_edit::Table>, PolicyError> {
    let name = format!("{parent}.{child}");
    let Some(parent) = document.get(parent).and_then(toml_edit::Item::as_table) else {
        return Ok(None);
    };
    let Some(item) = parent.get(child) else {
        return Ok(None);
    };
    // `as_table_like`, not `as_table`: `[check.risk]` and `risk = { threshold = 70 }` are the same document
    // to a reader, and refusing the second would be a spelling test rather than a policy.
    let table = item.as_table_like().ok_or_else(|| {
        PolicyError(format!(
            "[{name}] is not a table; its keys are {}",
            keys.join(", ")
        ))
    })?;
    let mut out = toml_edit::Table::new();
    for (key, value) in table.iter() {
        out.insert(key, value.clone());
    }
    Ok(Some(out))
}

/// Refuse a table's unknown keys — the same argument as `[check]`'s: an unwritten key and a typo'd one
/// look identical in the run, and only one of them is the default you meant.
fn refuse_unknown(table: &toml_edit::Table, name: &str, known: &[&str]) -> Result<(), PolicyError> {
    for (key, _) in table.iter() {
        if !known.contains(&key) {
            return Err(PolicyError(format!(
                "{name}.{key} is not a key this build reads; the keys are {}",
                known.join(", ")
            )));
        }
    }
    Ok(())
}

/// `[check.severity]`: one volume per check, `off` allowed.
fn read_severities(
    document: &toml_edit::DocumentMut,
) -> Result<[Option<Severity>; 6], PolicyError> {
    let names: Vec<&str> = Check::ALL.iter().map(|check| check.as_str()).collect();
    let Some(table) = sub_table(document, "check", "severity", &names)? else {
        return Ok(default_severity_slots());
    };
    let mut slots = default_severity_slots();
    // Iterated from the table rather than from `Check::ALL` so that a name can arrive in either spelling:
    // `sign-off` is canonical and `sign_off` is what a keyboard types, and both name one check. A key that
    // names no check is still a refusal — the same argument as `[check]`'s unknown keys.
    for (key, value) in table.iter() {
        let Some(check) = Check::parse(key) else {
            return Err(PolicyError(format!(
                "check.severity.{key} names no check; the checks are {}",
                names.join(", ")
            )));
        };
        let text = str_of(value, &format!("check.severity.{key}"))?;
        let slot = &mut slots[Check::ALL.iter().position(|c| *c == check).unwrap_or(0)];
        *slot = match text.as_str() {
            "off" => None,
            other => Some(Severity::parse(other).ok_or_else(|| {
                bad(
                    &format!("check.severity.{key}"),
                    other,
                    "off|note|warning|error",
                )
            })?),
        };
    }
    Ok(slots)
}

/// The default volumes as slots, so the default map is written once.
fn default_severity_slots() -> [Option<Severity>; 6] {
    Policy::defaults().severity
}

/// `[check.tests]`.
fn read_tests(document: &toml_edit::DocumentMut) -> Result<TestPolicy, PolicyError> {
    let names = ["min_new_lines", "min_tested_ratio"];
    let Some(table) = sub_table(document, "check", "tests", &names)? else {
        return Ok(TestPolicy::default());
    };
    refuse_unknown(&table, "check.tests", &names)?;
    let mut out = TestPolicy::default();
    if let Some(value) = table.get("min_new_lines") {
        let number = value
            .as_value()
            .and_then(toml_edit::Value::as_integer)
            .ok_or_else(|| {
                PolicyError("check.tests.min_new_lines must be an integer".to_owned())
            })?;
        if number < 0 {
            return Err(PolicyError(
                "check.tests.min_new_lines is negative, which no size is".to_owned(),
            ));
        }
        out.min_new_lines = number as u64;
    }
    if let Some(value) = table.get("min_tested_ratio") {
        let ratio = value
            .as_value()
            .and_then(toml_edit::Value::as_float)
            .ok_or_else(|| {
                PolicyError("check.tests.min_tested_ratio must be a float in 0.0..=1.0".to_owned())
            })?;
        if !(0.0..=1.0).contains(&ratio) {
            return Err(PolicyError(format!(
                "check.tests.min_tested_ratio is {ratio}, which is not a share (0.0..=1.0)"
            )));
        }
        out.min_tested_ratio = ratio;
    }
    Ok(out)
}

/// The dependency sub-volumes every reader starts from: each rule speaks at the volume the check itself
/// ships at, because a `dependencies` row that reports itself `ran` while all four of its rules are off is
/// a silent pass wearing a costume.
#[must_use]
fn default_dependencies() -> DependencyPolicy {
    DependencyPolicy {
        new_direct: Some(Severity::Warning),
        major_change: Some(Severity::Warning),
        new_source: Some(Severity::Warning),
        downgrade: Some(Severity::Warning),
        forbid: Vec::new(),
    }
}

/// `[check.dependencies]`.
fn read_dependencies(document: &toml_edit::DocumentMut) -> Result<DependencyPolicy, PolicyError> {
    let names = [
        "new_direct",
        "major_change",
        "new_source",
        "downgrade",
        "forbid",
    ];
    let Some(table) = sub_table(document, "check", "dependencies", &names)? else {
        return Ok(default_dependencies());
    };
    refuse_unknown(&table, "check.dependencies", &names)?;
    let mut out = default_dependencies();
    for key in ["new_direct", "major_change", "new_source", "downgrade"] {
        if let Some(value) = table.get(key) {
            let text = str_of(value, &format!("check.dependencies.{key}"))?;
            let severity = match text.as_str() {
                "off" => None,
                other => Some(Severity::parse(other).ok_or_else(|| {
                    bad(
                        &format!("check.dependencies.{key}"),
                        &text,
                        "off|note|warning|error",
                    )
                })?),
            };
            match key {
                "new_direct" => out.new_direct = severity,
                "major_change" => out.major_change = severity,
                "new_source" => out.new_source = severity,
                _ => out.downgrade = severity,
            }
        }
    }
    if let Some(value) = table.get("forbid") {
        out.forbid = string_list(value, "check.dependencies.forbid")?;
    }
    Ok(out)
}

/// `[check.risk]`.
fn read_risk(document: &toml_edit::DocumentMut) -> Result<RiskPolicy, PolicyError> {
    let names = ["threshold"];
    let Some(table) = sub_table(document, "check", "risk", &names)? else {
        return Ok(RiskPolicy::default());
    };
    refuse_unknown(&table, "check.risk", &names)?;
    let mut out = RiskPolicy::default();
    if let Some(value) = table.get("threshold") {
        let number = value
            .as_value()
            .and_then(toml_edit::Value::as_integer)
            .ok_or_else(|| PolicyError("check.risk.threshold must be an integer".to_owned()))?;
        if !(0..=100).contains(&number) {
            return Err(PolicyError(format!(
                "check.risk.threshold is {number}; the score runs 0..=100"
            )));
        }
        out.threshold = number as u8;
    }
    Ok(out)
}

/// The severity map as a `{check: level}` map, for `--explain` and the JSON payload.
#[must_use]
pub fn severity_map(policy: &Policy) -> BTreeMap<String, String> {
    Check::ALL
        .iter()
        .map(|check| {
            (
                check.as_str().to_owned(),
                policy
                    .severity(*check)
                    .map_or("off", Severity::as_str)
                    .to_owned(),
            )
        })
        .collect()
}

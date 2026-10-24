//! `policy.rs` — which detectors a repository runs, with what thresholds, and what it has suppressed.
//!
//! Three rules shape this module, and each is a refusal of something easier.
//!
//! **Membership is configurable; the catalogue is not.** `detectors.enabled` *adds* to the built-in set
//! and cannot reorder or re-price anything — the same refusal `[intent] sources` and `[episode]
//! boundaries` make. Which detectors ship on is a table in [`crate::detectors`] that this module reads,
//! and the argument for each entry is in its own file.
//!
//! **Suppression is the off switch, and it is one key.** `detectors.suppress` holds suppression keys, and
//! a bare detector id (`"copy_paste_growth"`) silences that detector entirely. So `enabled` extends and
//! `suppress` removes, and turning a shipped-on detector off in one repository is one line in
//! `fathom.toml` — which is also what makes the dismissal in `e4.5` and the per-repo switch the same
//! mechanism rather than two.
//!
//! **A bad value is a notice, never a guess.** A knob set to zero, a list holding a number, a pattern that
//! will not compile: each is reported in `Resolved::notices` and the default stands. The alternative —
//! a hard error — means one typo in `fathom.toml` takes the whole review surface away, which is
//! ADR-001 §7.6's "degrade, never block" with the block removed.
//!
//! # The keys, in `fathom.toml`
//!
//! Every key here is declared in the config schema (`src/recommender/config/schema.py`, which generates
//! `schemas/fathom.schema.json`) and is listed in the `findings.detect` analyzer's declared keys
//! (`fathom-store::cache::ANALYZERS`), which is what makes a config change invalidate the stored
//! findings and nothing else (ADR-004 §5.5).
//!
//! The same membership answers the `--detectors`, `--enable-detector` and `--disable-detector` flags
//! (`e6.7`, `reco-raw`), which are the same three knobs typed once at a prompt: [`Selection`] is the
//! resolved form, [`resolve`] applies it, and a detector switched off at the command line lands in the
//! report as `disabled` for exactly the same reason a detector switched off in the file does.
//!
//! | Key | Default | What it moves |
//! |---|---|---|
//! | `detectors.enabled` | the built-in set | detector ids that run, in addition to the defaults |
//! | `detectors.disable` | `[]` | built-in detector ids that do **not** run, defaults included |
//! | `detectors.suppress` | `[]` | suppression keys, or bare detector ids, to silence |
//! | `detectors.max_findings` | `12` | rows one detector may print per episode, past which it says so |
//! | `detectors.churn.min_episodes` | `3` | how many episodes re-touching a region is a churn loop |
//! | `detectors.rework.min_overlap` | `0.6` | line overlap that makes two episodes a revert |
//! | `detectors.shotgun.min_clusters` | `4` | unrelated directory clusters in one episode |
//! | `detectors.copy.min_block_lines` | `12` | repeated lines that count as copied growth |
//! | `detectors.untested.min_added_lines` | `20` | new logic big enough to owe a test |
//! | `detectors.todo.min_new` | `1` | new TODO markers that are debt worth printing |

use std::collections::BTreeSet;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// The default number of rows one detector may print for one episode.
///
/// A row cap, not a truncation: when a detector hits it, the last row says `(+N more)` — the same rule
/// the store's `EPISODE_FILE_CAP` and the card's row caps follow. Twelve is above what any real episode
/// in the fixture corpus produces and below what would push the attention section off a screen.
pub const DEFAULT_MAX_FINDINGS: usize = 12;

/// How many episodes re-touching one region is a churn loop.
pub const DEFAULT_CHURN_MIN_EPISODES: usize = 3;

/// The line overlap that makes two episodes a revert of one another.
pub const DEFAULT_REWORK_MIN_OVERLAP: f64 = 0.6;

/// How many unrelated clusters one episode may touch before it is a shotgun.
pub const DEFAULT_SHOTGUN_MIN_CLUSTERS: usize = 4;

/// How many repeated lines count as copied growth rather than as a coincidence.
pub const DEFAULT_COPY_MIN_BLOCK_LINES: usize = 12;

/// How much new logic owes a test, in added lines.
pub const DEFAULT_UNTESTED_MIN_ADDED_LINES: i64 = 20;

/// How many new TODO markers are debt worth a row.
pub const DEFAULT_TODO_MIN_NEW: i64 = 1;

/// How many paths one finding may name before the message says `and N more`.
pub const DEFAULT_NAME_CAP: usize = 6;

/// How many quoted lines one piece of evidence may carry.
pub const DEFAULT_EVIDENCE_LINES: usize = 4;

/// Everything a detector is allowed to be told about the repository's wishes.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// The detector ids that will run. Defaults plus `detectors.enabled`, minus
    /// [`Policy::disabled_ids`].
    pub enabled: BTreeSet<String>,
    /// The ids switched off by `detectors.disable` (or by `--disable-detector`), kept beside the set
    /// rather than folded into it so a report can say *why* a detector did not run: never enabled, or
    /// switched off on purpose. Those are different answers to a reviewer's question.
    pub disabled_ids: BTreeSet<String>,
    /// The suppression patterns in force. Matched by [`Suppressions`].
    pub suppress: Vec<String>,
    /// Rows per detector per episode.
    pub max_findings: usize,
    /// Paths named in one message before it says `and N more`.
    pub name_cap: usize,
    /// Lines quoted in one piece of evidence.
    pub evidence_lines: usize,
    /// `detectors.churn.min_episodes`.
    pub churn_min_episodes: usize,
    /// `detectors.rework.min_overlap`.
    pub rework_min_overlap: f64,
    /// `detectors.shotgun.min_clusters`.
    pub shotgun_min_clusters: usize,
    /// `detectors.copy.min_block_lines`.
    pub copy_min_block_lines: usize,
    /// `detectors.untested.min_added_lines`.
    pub untested_min_added_lines: i64,
    /// `detectors.todo.min_new`.
    pub todo_min_new: i64,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            enabled: crate::detectors::default_enabled(),
            disabled_ids: BTreeSet::new(),
            suppress: Vec::new(),
            max_findings: DEFAULT_MAX_FINDINGS,
            name_cap: DEFAULT_NAME_CAP,
            evidence_lines: DEFAULT_EVIDENCE_LINES,
            churn_min_episodes: DEFAULT_CHURN_MIN_EPISODES,
            rework_min_overlap: DEFAULT_REWORK_MIN_OVERLAP,
            shotgun_min_clusters: DEFAULT_SHOTGUN_MIN_CLUSTERS,
            copy_min_block_lines: DEFAULT_COPY_MIN_BLOCK_LINES,
            untested_min_added_lines: DEFAULT_UNTESTED_MIN_ADDED_LINES,
            todo_min_new: DEFAULT_TODO_MIN_NEW,
        }
    }
}

impl Policy {
    /// Turn one id off, and say whether it was running before.
    pub fn disable(&mut self, detector_id: &str) -> bool {
        let was = self.enabled.remove(detector_id);
        self.disabled_ids.insert(detector_id.to_owned());
        was
    }

    /// Turn one id on: out of the off list and into the running set.
    pub fn enable(&mut self, detector_id: &str) {
        self.disabled_ids.remove(detector_id);
        self.enabled.insert(detector_id.to_owned());
    }

    /// A policy with **every** detector in the catalogue enabled, defaults otherwise.
    ///
    /// This is what the calibration harness (`e4.3a`) and the per-detector fixtures use: a measurement of
    /// a detector's false positives cannot be taken through a switch that has it off.
    #[must_use]
    pub fn all_enabled() -> Self {
        Policy {
            enabled: crate::detectors::ids()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ..Default::default()
        }
    }

    /// A policy with exactly the named ids enabled, defaults otherwise — for a test that wants one
    /// detector and nothing else.
    #[must_use]
    pub fn only(ids: &[&str]) -> Self {
        Policy {
            enabled: ids.iter().map(|id| (*id).to_owned()).collect(),
            ..Default::default()
        }
    }

    /// Whether a detector runs under this policy.
    #[must_use]
    pub fn enabled(&self, detector_id: &str) -> bool {
        !self.disabled_ids.iter().any(|id| id == detector_id)
            && self.enabled.iter().any(|id| id == detector_id)
    }

    /// Whether this id was switched off on purpose, which is a different fact from "never came on".
    #[must_use]
    pub fn disabled(&self, detector_id: &str) -> bool {
        self.disabled_ids.iter().any(|id| id == detector_id)
    }

    /// The knobs, printed with the values in force — the receipts pattern every analysis in this crate
    /// follows: a verdict without the numbers that made it is a shrug with a number attached.
    #[must_use]
    pub fn knobs(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "detectors.enabled",
                self.enabled.iter().cloned().collect::<Vec<_>>().join(","),
            ),
            (
                "detectors.disable",
                self.disabled_ids
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            ("detectors.suppress", self.suppress.join(",")),
            ("detectors.max_findings", self.max_findings.to_string()),
            (
                "detectors.churn.min_episodes",
                self.churn_min_episodes.to_string(),
            ),
            (
                "detectors.rework.min_overlap",
                format!("{:.2}", self.rework_min_overlap),
            ),
            (
                "detectors.shotgun.min_clusters",
                self.shotgun_min_clusters.to_string(),
            ),
            (
                "detectors.copy.min_block_lines",
                self.copy_min_block_lines.to_string(),
            ),
            (
                "detectors.untested.min_added_lines",
                self.untested_min_added_lines.to_string(),
            ),
            ("detectors.todo.min_new", self.todo_min_new.to_string()),
        ]
    }

    /// The compiled suppression matcher. Built once per scan, not once per finding.
    #[must_use]
    pub fn suppressions(&self) -> Suppressions {
        Suppressions::new(&self.suppress)
    }
}

/// `fathom.toml` plus what this build could not use from it.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    /// The policy that resulted.
    pub policy: Policy,
    /// One line per value that was refused, naming the key and the default that stood.
    pub notices: Vec<String>,
}

impl Resolved {
    /// The built-in answer, for a repository with no `fathom.toml`.
    #[must_use]
    pub fn defaults() -> Self {
        Resolved::default()
    }

    /// The set that resulted, in one line, for whatever prints the knobs in force — a report's receipt, or
    /// one `selection.resolved` event (`e6.7`).
    ///
    /// It names the built-ins that were switched off on purpose as well as the count that remains, because
    /// "eleven of twelve, `secret_string` disabled" is the answer a reviewer is actually asking, and a
    /// count alone cannot be checked against a config file.
    #[must_use]
    pub fn describe(&self) -> String {
        let catalogue = super::CATALOGUE;
        let running: Vec<&str> = catalogue
            .iter()
            .map(|descriptor| descriptor.id)
            .filter(|id| self.policy.enabled(id))
            .collect();
        let mut text = format!(
            "{} of {} built-in detector(s) running",
            running.len(),
            catalogue.len()
        );
        let switched_off = self.disabled_ids_named();
        let off: Vec<&str> = switched_off.iter().map(String::as_str).collect();
        if !off.is_empty() {
            text.push_str(&format!("; switched off: {}", off.join(", ")));
        }
        if !self.policy.suppress.is_empty() {
            text.push_str(&format!(
                "; suppressing {} pattern(s)",
                self.policy.suppress.len()
            ));
        }
        text
    }

    /// The built-ins that were switched off by name, whether they ship on or not.
    #[must_use]
    pub fn disabled_ids_named(&self) -> Vec<String> {
        self.policy
            .disabled_ids
            .iter()
            .filter(|id| {
                super::CATALOGUE
                    .iter()
                    .any(|descriptor| descriptor.id == id.as_str())
            })
            .cloned()
            .collect()
    }
}

/// Parse the `[detectors]` table out of `fathom.toml` text.
///
/// A file that does not parse is a notice and the defaults; a table with one bad value is a notice and
/// the rest of the table. This is [`crate`]'s reading of `[detectors]` only — the schema, the layering
/// and the precedence of `fathom.toml` itself belong to `e0.8` and are not re-implemented here.
#[must_use]
pub fn policy_from_toml(text: &str) -> Resolved {
    let mut out = Resolved::default();
    let document = match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => document,
        Err(err) => {
            out.notices.push(format!(
                "fathom.toml does not parse, so every detector ran on its default: {err}"
            ));
            return out;
        }
    };
    let Some(table) = document
        .get("detectors")
        .and_then(toml_edit::Item::as_table)
    else {
        return out;
    };

    if let Some(entries) = table.get("enabled").and_then(toml_edit::Item::as_array) {
        for entry in entries.iter() {
            let Some(name) = entry.as_str() else {
                out.notices.push(
                    "detectors.enabled holds a non-string entry, which is not a detector id"
                        .to_owned(),
                );
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                out.notices
                    .push("detectors.enabled holds an empty detector id".to_owned());
                continue;
            }
            match crate::detectors::by_id(name) {
                Some(descriptor) => {
                    out.policy.enabled.insert(descriptor.id.to_owned());
                }
                None => out.notices.push(format!(
                    "detectors.enabled names `{name}`, which is not in this build's catalogue; it is \
                     ignored. Valid ids: {}",
                    crate::detectors::ids().join(", ")
                )),
            }
        }
    }

    // The off switch that is not a suppression (`e6.7`). Read after `enabled` so a file that both names a
    // detector and turns it off ends with it off: the off switch losing to nothing is the whole point of
    // calling it an off switch.
    if let Some(entries) = table.get("disable").and_then(toml_edit::Item::as_array) {
        for entry in entries.iter() {
            let Some(name) = entry.as_str() else {
                out.notices.push(
                    "detectors.disable holds a non-string entry, which is not a detector id"
                        .to_owned(),
                );
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                out.notices
                    .push("detectors.disable holds an empty detector id".to_owned());
                continue;
            }
            match crate::detectors::by_id(name) {
                Some(descriptor) => {
                    let was = out.policy.disable(descriptor.id);
                    if was {
                        out.notices.push(format!(
                            "detectors.disable turned `{}` off for this repository",
                            descriptor.id
                        ));
                    }
                }
                None => out.notices.push(format!(
                    "detectors.disable names `{name}`, which is not in this build's catalogue; it is \
                     ignored. Valid ids: {}",
                    crate::detectors::ids().join(", ")
                )),
            }
        }
    }

    if let Some(entries) = table.get("suppress").and_then(toml_edit::Item::as_array) {
        for entry in entries.iter() {
            let Some(name) = entry.as_str() else {
                out.notices.push(
                    "detectors.suppress holds a non-string entry, which is not a key".to_owned(),
                );
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                out.notices
                    .push("detectors.suppress holds an empty key".to_owned());
                continue;
            }
            out.policy.suppress.push(name.to_owned());
        }
    }

    let mut notices: Vec<String> = Vec::new();
    {
        let policy = &mut out.policy;
        for (key, mut sink) in [
            (
                "max_findings",
                SinkKind::PositiveInteger(&mut policy.max_findings),
            ),
            (
                "churn.min_episodes",
                SinkKind::PositiveInteger(&mut policy.churn_min_episodes),
            ),
            (
                "shotgun.min_clusters",
                SinkKind::PositiveInteger(&mut policy.shotgun_min_clusters),
            ),
            (
                "copy.min_block_lines",
                SinkKind::PositiveInteger(&mut policy.copy_min_block_lines),
            ),
            (
                "untested.min_added_lines",
                SinkKind::Integer(&mut policy.untested_min_added_lines),
            ),
            ("todo.min_new", SinkKind::Integer(&mut policy.todo_min_new)),
        ] {
            let Some(value) = knob(table, key).and_then(toml_edit::Item::as_value) else {
                continue;
            };
            match &mut sink {
                SinkKind::PositiveInteger(slot) => {
                    let Some(number) = value.as_integer() else {
                        notices.push(format!("detectors.{key} must be a whole number of items"));
                        continue;
                    };
                    if number <= 0 {
                        notices.push(format!(
                            "detectors.{key} = {number} would report nothing; the default \
                             ({}) stood",
                            **slot
                        ));
                        continue;
                    }
                    **slot = usize::try_from(number).unwrap_or(usize::MAX);
                }
                SinkKind::Integer(slot) => {
                    let Some(number) = value.as_integer() else {
                        notices.push(format!("detectors.{key} must be a whole number"));
                        continue;
                    };
                    **slot = number;
                }
            }
        }
    }
    out.notices.extend(notices);

    if let Some(value) = knob(table, "rework.min_overlap").and_then(toml_edit::Item::as_value) {
        match value
            .as_float()
            .or_else(|| value.as_integer().map(|n| n as f64))
        {
            Some(number) if (0.0..=1.0).contains(&number) => out.policy.rework_min_overlap = number,
            Some(number) => out.notices.push(format!(
                "detectors.rework.min_overlap = {number} is not a 0–1 fraction; the default \
                 ({DEFAULT_REWORK_MIN_OVERLAP}) stood"
            )),
            None => out
                .notices
                .push("detectors.rework.min_overlap must be a number between 0 and 1".to_owned()),
        }
    }

    // A key in `[detectors]` that no knob reads is a typo waiting to be believed, and a repository that
    // adds `detectors.todo.min_new` to the wrong table should hear about it now rather than after a
    // month of findings it cannot explain.
    for key in table.iter() {
        let name = key.0;
        let known = [
            "enabled",
            "disable",
            "suppress",
            "max_findings",
            "rework.min_overlap",
        ]
        .into_iter()
        .chain(KNOBS)
        .any(|knob| knob == name || knob.split('.').next() == name.split('.').next());
        if !known {
            out.notices.push(format!(
                "detectors.{name} is not a knob this build reads; the catalogue's knobs are {}",
                KNOBS.join(", ")
            ));
        }
    }

    out
}

/// Every numeric knob [`policy_from_toml`] reads, by its printed name.
const KNOBS: [&str; 7] = [
    "max_findings",
    "churn.min_episodes",
    "shotgun.min_clusters",
    "copy.min_block_lines",
    "untested.min_added_lines",
    "todo.min_new",
    "rework.min_overlap",
];

/// Look one knob up by its printed dotted name.
///
/// TOML reads `todo.min_new = 0` inside `[detectors]` as a *nested* table, so the dotted names this
/// ticket prints have to be walked rather than used as one key. The literal dotted key is tried first so
/// a repository that quoted it (`"todo.min_new" = 0`) is read the same way.
fn knob<'t>(table: &'t toml_edit::Table, path: &str) -> Option<&'t toml_edit::Item> {
    if let Some(item) = table.get(path) {
        return Some(item);
    }
    let mut segments = path.split('.');
    let mut item: Option<&toml_edit::Item> = table.get(segments.next()?);
    for segment in segments {
        item = item?.get(segment);
    }
    item
}

/// The two shapes of numeric knob the table reads, so one loop covers both without a macro.
enum SinkKind<'a> {
    /// A count that must be at least one, because zero would mean "report nothing".
    PositiveInteger(&'a mut usize),
    /// A count or a threshold that may legitimately be zero.
    Integer(&'a mut i64),
}

/// The compiled `detectors.suppress` list.
///
/// Three ways a pattern silences a finding, in this order:
///
/// * it **is the detector id** — `copy_paste_growth` silences that detector, which is the "turn this off
///   for my repository" case and does not make the reviewer find a subject to name;
/// * it **is the exact key** — `secret_string:src/config.rs` silences that family;
/// * it **globs the key** — the same `globset` semantics as a declared scope and as the path taxonomy:
///   `*` inside one segment, `**` across them, so `todo_debt:**/legacy/**` silences a subtree.
#[derive(Debug, Clone, Default)]
pub struct Suppressions {
    patterns: Vec<String>,
    set: Option<GlobSet>,
    /// Patterns that would not compile, kept so a scan can print them instead of ignoring them quietly.
    pub invalid: Vec<String>,
}

impl Suppressions {
    /// Compile a list of keys and globs.
    #[must_use]
    pub fn new(patterns: &[String]) -> Self {
        let mut builder = GlobSetBuilder::new();
        let mut set_built = false;
        let mut invalid = Vec::new();
        for pattern in patterns {
            if !pattern.is_empty() && !pattern.contains(['*', '?', '[', '{']) {
                continue; // an exact key needs no glob
            }
            match GlobBuilder::new(pattern).literal_separator(true).build() {
                Ok(glob) => {
                    builder.add(glob);
                    set_built = true;
                }
                Err(_) => invalid.push(pattern.clone()),
            }
        }
        Suppressions {
            patterns: patterns.to_vec(),
            set: if set_built {
                builder.build().ok()
            } else {
                None
            },
            invalid,
        }
    }

    /// Nothing to silence, so every finding stands.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Does this finding's key fall inside the suppressions?
    #[must_use]
    pub fn silences(&self, detector_id: &str, key: &str) -> bool {
        self.patterns.iter().any(|pattern| {
            pattern == detector_id
                || pattern == key
                || self.set.as_ref().is_some_and(|set| set.is_match(key))
        })
    }
}

/// The `--detectors` / `--enable-detector` / `--disable-detector` grammar, resolved (`e6.7`, `reco-raw`).
///
/// One shape for three flags, because "which detectors run" has one answer and the product may not have two
/// ways to spell it that disagree. A spec is a comma-separated list of:
///
/// | token | meaning |
/// |---|---|
/// | `id` or `+id` | run this detector as well |
/// | `-id` | do not run this detector, even if it ships on |
/// | `all` | the base is every built-in detector, defaults aside |
/// | `none` | the base is nothing; list what you want with `+id` |
///
/// `all` and `none` set the *base* and the rest move it, so `--detectors none,+churn_loop` is one detector
/// and `--detectors all,-secret_string` is eleven. A plugin detector's id is namespaced
/// (`acme.orphan_test`, [plugin API](crate::plugin)) and `all` never claims a plugin: an out-of-tree
/// detector is opted into by name, which is the same rule [`Meta::default_on`] states for a plugin that
/// ships itself on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection {
    /// Ids named on (`+id`, `--enable-detector`).
    pub enable: BTreeSet<String>,
    /// Ids named off (`-id`, `--disable-detector`).
    pub disable: BTreeSet<String>,
    /// `Some` when `all` or `none` replaced the default set instead of moving it.
    pub base: Option<BTreeSet<String>>,
    /// One line per token this build could not use. A bad token never changes the answer for the good ones.
    pub notices: Vec<String>,
}

impl Selection {
    /// The names that carry a namespace — a plugin's `acme.thing`, which no built-in id can look like.
    ///
    /// A host that links no plugins still has to say something when asked to run one, because the silence
    /// otherwise reads as "it ran and found nothing" (`e6.7`). No built-in id is ever dotted, so this list
    /// is complete, and a plugin host reads the same selection ([`crate::plugin::Registry::run`]).
    #[must_use]
    pub fn namespaced(&self) -> Vec<String> {
        self.enable
            .iter()
            .chain(self.disable.iter())
            .filter(|id| id.contains('.'))
            .cloned()
            .collect()
    }
    /// Parse one `--detectors` value.
    #[must_use]
    pub fn parse(spec: &str) -> Self {
        let mut out = Selection::default();
        for token in spec.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            out.push(token);
        }
        if out.enable.is_empty()
            && out.disable.is_empty()
            && out.base.is_none()
            && !spec.trim().is_empty()
        {
            out.notices.push(format!(
                "`{spec}` named no detector; every token is an id, `+id`, `-id`, `all` or `none`"
            ));
        }
        out
    }

    /// Parse the repeatable `--enable-detector` and `--disable-detector` flags.
    #[must_use]
    pub fn from_flags<S: AsRef<str>>(specs: &[S], enabled: &[S], disabled: &[S]) -> Self {
        let mut out = Selection::default();
        for spec in specs {
            out.merge(&Selection::parse(spec.as_ref()));
        }
        for id in enabled {
            for token in id.as_ref().split(',') {
                let token = token.trim();
                if !token.is_empty() {
                    out.push(&token.replace('+', ""));
                }
            }
        }
        for id in disabled {
            for token in id.as_ref().split(',') {
                let token = token.trim();
                if !token.is_empty() {
                    out.push(&format!("-{}", token.trim_start_matches('-')));
                }
            }
        }
        out
    }

    /// One token.
    fn push(&mut self, token: &str) {
        match token {
            "all" => {
                self.base = Some(
                    crate::detectors::ids()
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                );
            }
            "none" => {
                self.base = Some(BTreeSet::new());
            }
            _ if token.starts_with('-') => {
                self.disable.insert(token[1..].to_owned());
            }
            _ if token.starts_with('+') => {
                self.enable.insert(token[1..].to_owned());
            }
            _ => {
                self.enable.insert(token.to_owned());
            }
        }
    }

    /// Fold another selection in, later tokens winning: a flag beats a flag later on the same line, and a
    /// flag beats the file because [`resolve`] applies this after the file.
    pub fn merge(&mut self, other: &Selection) {
        self.enable.extend(other.enable.clone());
        self.disable.extend(other.disable.clone());
        if other.base.is_some() {
            self.base = other.base.clone();
        }
        self.notices.extend(other.notices.clone());
    }

    /// Whether nothing was asked, which is the ordinary case and the one that changes no answer.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.enable.is_empty() && self.disable.is_empty() && self.base.is_none()
    }

    /// Whether one detector runs under this selection.
    ///
    /// `default_on` is the catalogue's answer — [`Descriptor::default_on`] for a built-in,
    /// [`Meta::default_on`](crate::plugin::Meta) for a plugin — because "does it run" is a question about
    /// the default set and the three moves made on top of it, in that order and no other.
    #[must_use]
    pub fn allows(&self, id: &str, default_on: bool) -> bool {
        if self.disable.iter().any(|disabled| disabled == id) {
            return false;
        }
        if let Some(base) = &self.base {
            return base.iter().any(|member| member == id) || self.enable.iter().any(|on| on == id);
        }
        default_on || self.enable.iter().any(|on| on == id)
    }

    /// The printed form, for the receipts a report carries.
    #[must_use]
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(base) = &self.base {
            parts.push(if base.is_empty() {
                "none".to_owned()
            } else {
                format!("all({})", base.len())
            });
        }
        if !self.enable.is_empty() {
            parts.push(format!(
                "+{}",
                self.enable.iter().cloned().collect::<Vec<_>>().join("+")
            ));
        }
        if !self.disable.is_empty() {
            parts.push(format!(
                "-{}",
                self.disable.iter().cloned().collect::<Vec<_>>().join("-")
            ));
        }
        if parts.is_empty() {
            "(file defaults)".to_owned()
        } else {
            parts.join(" ")
        }
    }
}

/// `policy_from_toml`, with a [`Selection`] applied on top — the flags' whole path into the policy.
///
/// Precedence is the file, then the flags, because a flag is typed at a prompt by the person who wants a
/// different answer *today*. An id that is neither a built-in nor namespaced is a notice, not a guess: a
/// run that silently ignored `--disable-detector secrt_string` would tell the reviewer their detector was
/// off when it was not.
#[must_use]
pub fn resolve(text: &str, selection: &Selection) -> Resolved {
    let mut out = policy_from_toml(text);
    out.notices.extend(selection.notices.clone());
    if selection.is_empty() {
        return out;
    }
    if let Some(base) = &selection.base {
        out.policy.enabled = BTreeSet::new();
        out.policy.disabled_ids = BTreeSet::new();
        for id in base {
            out.policy.enabled.insert(id.clone());
        }
    }
    for id in &selection.enable {
        match crate::detectors::by_id(id) {
            Some(descriptor) => {
                out.policy.enable(descriptor.id);
            }
            None if id.contains('.') => {
                // A namespaced id belongs to the plugin registry, which reads this same selection.
                out.policy.enabled.insert(id.clone());
            }
            None => out.notices.push(format!(
                "`{id}` is not a detector this build has: the built-ins are {}, and a plugin detector \
                 is named `namespace.id`",
                crate::detectors::ids().join(", ")
            )),
        }
    }
    for id in &selection.disable {
        match crate::detectors::by_id(id) {
            Some(descriptor) => {
                if out.policy.disable(descriptor.id) {
                    out.notices.push(format!(
                        "`--detectors` turned `{}` off for this run",
                        descriptor.id
                    ));
                }
            }
            // A namespaced id belongs to the plugin registry, which reads this same selection. Anything
            // else is a typo, and a run that quietly ignored `--disable-detector secrt_string` would tell
            // the reviewer their detector was off when it was running the whole time.
            None if id.contains('.') => {
                out.policy.disable(id);
            }
            None => {
                out.policy.disable(id);
                out.notices.push(format!(
                    "`{id}` is not a detector this build has, so nothing was turned off: the built-ins \
                     are {}, and a plugin detector is named `namespace.id`",
                    crate::detectors::ids().join(", ")
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::CATALOGUE;

    fn configured(text: &str) -> Resolved {
        policy_from_toml(text)
    }

    #[test]
    fn no_file_is_the_default_and_says_nothing() {
        let resolved = policy_from_toml("");
        assert!(resolved.notices.is_empty());
        assert_eq!(resolved.policy, Policy::default());
    }

    #[test]
    fn enabling_names_the_catalogue_and_refuses_an_invention() {
        let resolved = configured("[detectors]\nenabled = [\"copy_paste_growth\", \"typos\"]");
        assert!(resolved.policy.enabled.contains("copy_paste_growth"));
        assert_eq!(resolved.notices.len(), 1, "one bogus id, one notice");
        assert!(resolved.notices[0].contains("`typos`"));
        assert!(
            resolved.notices[0].contains("churn_loop"),
            "the notice names valid ids, which is what makes the typo self-correcting: {}",
            resolved.notices[0]
        );
    }

    #[test]
    fn enabling_adds_and_never_removes_a_default_detector() {
        // Membership only. A repository that wants a default-on detector off uses `suppress`, and the
        // test that the default set is the shipped set is in `mod.rs`.
        let resolved = configured("[detectors]\nenabled = [\"churn_loop\"]");
        for descriptor in CATALOGUE.iter().filter(|row| row.default_on) {
            assert!(
                resolved.policy.enabled(descriptor.id),
                "{} is a default-on detector and `enabled` may not turn it off",
                descriptor.id
            );
        }
        assert!(resolved.policy.enabled("churn_loop"));
    }

    #[test]
    fn a_bare_detector_id_in_suppress_is_the_off_switch() {
        let policy = configured("[detectors]\nsuppress = [\"secret_string\"]").policy;
        let suppressions = policy.suppressions();
        assert!(suppressions.silences("secret_string", "secret_string:src/config.rs"));
        assert!(!suppressions.silences("lint_escape", "lint_escape:src/config.rs"));
    }

    #[test]
    fn a_key_globs_the_same_way_a_declared_scope_does() {
        let suppressions = configured(
            "[detectors]\nsuppress = [\"todo_debt:src/legacy/**\", \"copy_paste_growth:src/**\"]",
        )
        .policy
        .suppressions();
        assert!(suppressions.silences("todo_debt", "todo_debt:src/legacy/a.rs"));
        assert!(
            !suppressions.silences("todo_debt", "todo_debt:src/other/a.rs"),
            "`**` crosses segments and one `*` does not — one glob language in the product"
        );
        assert!(suppressions.silences("copy_paste_growth", "copy_paste_growth:src/a.rs"));
    }

    #[test]
    fn an_uncompilable_pattern_is_said_loudly_not_ignored_quietly() {
        let suppressions = Suppressions::new(&["src/[a-".to_owned(), "todo_debt:docs".to_owned()]);
        assert_eq!(suppressions.invalid, ["src/[a-"]);
        assert!(suppressions.silences("todo_debt", "todo_debt:docs"));
    }

    #[test]
    fn a_knob_that_would_report_nothing_is_a_notice_and_the_default_stands() {
        let resolved = configured("[detectors]\nmax_findings = 0\ntodo.min_new = 0\n");
        assert_eq!(resolved.policy.max_findings, DEFAULT_MAX_FINDINGS);
        assert_eq!(
            resolved.policy.todo_min_new, 0,
            "a threshold of zero is a real threshold; only a *count* of zero is nonsense"
        );
        assert!(
            resolved
                .notices
                .iter()
                .any(|notice| notice.contains("max_findings"))
        );
        assert!(
            resolved
                .notices
                .iter()
                .all(|notice| !notice.contains("todo.min_new")),
            "{:?}",
            resolved.notices
        );
    }

    #[test]
    fn a_fraction_outside_zero_and_one_is_refused_by_name() {
        let resolved = configured("[detectors]\nrework.min_overlap = 3\n");
        assert!(
            (resolved.policy.rework_min_overlap - DEFAULT_REWORK_MIN_OVERLAP).abs() < f64::EPSILON
        );
        assert!(resolved.notices[0].contains("rework.min_overlap"));
        let fine = configured("[detectors]\nrework.min_overlap = 0.8\n");
        assert!((fine.policy.rework_min_overlap - 0.8).abs() < f64::EPSILON);
        assert!(fine.notices.is_empty(), "{:?}", fine.notices);
    }

    #[test]
    fn every_knob_this_module_reads_is_declared_in_the_policy_table() {
        // The five numbers a fixture tunes, plus the two list knobs: if a knob is added to `Policy` and
        // not to `knobs()`, the printed receipts lie, and this fails first.
        let names: Vec<&str> = Policy::default()
            .knobs()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        for expected in [
            "detectors.enabled",
            "detectors.suppress",
            "detectors.max_findings",
            "detectors.churn.min_episodes",
            "detectors.rework.min_overlap",
            "detectors.shotgun.min_clusters",
            "detectors.copy.min_block_lines",
            "detectors.untested.min_added_lines",
            "detectors.todo.min_new",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} is missing from the printed knobs"
            );
        }
    }

    #[test]
    fn a_file_that_is_not_toml_is_a_notice_not_a_failure() {
        let resolved = configured("this is not [toml");
        assert_eq!(resolved.notices.len(), 1);
        assert!(resolved.notices[0].contains("does not parse"));
        assert!(
            resolved.policy.enabled("scope_creep"),
            "and the defaults still ran"
        );
    }
}

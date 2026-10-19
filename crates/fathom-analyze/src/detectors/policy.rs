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
//! | Key | Default | What it moves |
//! |---|---|---|
//! | `detectors.enabled` | the built-in set | detector ids that run, in addition to the defaults |
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
    /// The detector ids that will run. Defaults plus `detectors.enabled`, minus nothing — the off switch
    /// is [`Policy::suppress`].
    pub enabled: BTreeSet<String>,
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
        self.enabled.iter().any(|id| id == detector_id)
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
        let known = ["enabled", "suppress", "max_findings", "rework.min_overlap"]
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

//! `[api]` — which paths' exports the rest of the world compiles against.
//!
//! Every other number in this module is derived; this one is a claim about the repository that only
//! its owner can make. A workspace of nine crates has exactly two of them whose `pub` items are
//! somebody else's contract, and a tool that reports a breaking change in the third is reporting
//! noise that will be ignored, which is worse than reporting nothing: the four files that were
//! ignored on purpose are the reason the one that mattered was scrolled past.
//!
//! So a handful of knobs, each refused loudly when written wrong (the same discipline
//! [`crate::layering::rules`] applies to a rule that can match nothing):
//!
//! ```toml
//! [api]
//! mode = "warn"                       # off | warn | deny (deny is the future `fathom check` exit)
//! ignore = ["**/tests/**", "crates/fathom-cli/**"]
//! package = ["crates/fathom-core = fathom-core", "sdk/ts = @acme/sdk"]
//! private = ["fathom-fixtures"]
//! max_items_per_package = 20000
//! track_tests = false                 # the built-in test table; `true` tracks tests as surface
//! ```
//!
//! * [`ApiPolicy::ignore`] — paths whose exports are not tracked at all. A *stated* exclusion: an
//!   ignored file is counted and named in the report, so "the surface did not change" always says
//!   how much of the repository it declined to look at.
//! * [`ApiPolicy::pins`] — a path prefix or glob pinned to a package name, for the repositories
//!   where the manifest either does not exist (a TS directory with no `package.json`) or says
//!   something different from what the reviewer means by a package.
//! * [`ApiPolicy::private`] — packages that exist and are published to nobody: their surface is
//!   still extracted, and its changes are still reported, but the semver recommendation says
//!   *internal* rather than proposing a version bump for a crate nobody outside downloads.
//! * [`ApiPolicy::track_tests`] — the one knob here that is *not* the owner's claim: the built-in test
//!   table ([`crate::api::testpath`]) declines a test file because the language says nothing can import
//!   it, not because anybody wrote a line saying so. It applies by default (`reco-u1i`: this check
//!   reported the rename of a Python test function as a removed public item), it can be overruled with
//!   one key, and the files it declines are counted and named rather than silently dropped — which is
//!   what keeps `[api] ignore` (the owner's exclusion) a different sentence in the report from this
//!   one.

use crate::api::testpath::{self, TestRule};
use crate::graph::layers::glob_match;
use crate::structure::Language;
use serde::Serialize;

/// The built-in ceiling on items per package. It is a memory bound, not a correctness bound: above
/// it the package says so instead of growing without limit ([`crate::api::Package::truncated`]).
pub const DEFAULT_MAX_ITEMS_PER_PACKAGE: usize = 20_000;

/// Whether test files are public surface unless the repository says otherwise.
///
/// `false`, and the argument is [`crate::api::testpath`]: a test function is not a contract, and a
/// gate that calls the renaming of one a breaking change is a gate that gets switched off.
pub const DEFAULT_TRACK_TESTS: bool = false;

/// Why a path is not on the tracked surface.
///
/// Carried as data rather than as a `bool`, because the report has to say *which* rule declined to
/// look at a file: an exclusion a reader cannot check is a hole in the answer that reads as a clean
/// bill of health.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "exclusion")]
pub enum Exclusion {
    /// The path is tracked: nothing excluded it.
    Tracked,
    /// `[api] ignore` named it — the repository owner's own exclusion.
    Ignored {
        /// The pattern that matched, as written in `fathom.toml`.
        pattern: String,
    },
    /// The built-in test table claimed it ([`crate::api::testpath`]); `[api] ignore` did not have to
    /// say so. `reco-u1i`.
    TestFile {
        /// The convention that claimed it, per language.
        rule: TestRule,
    },
}

impl Exclusion {
    /// Whether this path is off the surface.
    #[must_use]
    pub fn is_excluded(&self) -> bool {
        !matches!(self, Exclusion::Tracked)
    }

    /// The short reason a report prints beside a path.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Exclusion::Tracked => "tracked".to_string(),
            Exclusion::Ignored { pattern } => format!("`[api] ignore` pattern {pattern:?}"),
            Exclusion::TestFile { rule } => format!("test file {}", rule.code()),
        }
    }
}

/// How loudly an API change speaks. The three modes are `layering.mode`'s, and they mean the same
/// thing here: the volume, never the facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Nothing is examined and the report says so. Distinct from "examined and clean", which is the
    /// difference a CI gate has to be able to tell.
    Off,
    /// Everything is examined and reported; nothing decides an exit code. The default, because a
    /// tool that fails a build on its first run has taught nobody anything.
    #[default]
    Warn,
    /// As [`Mode::Warn`], and a breaking change is the `fathom check` violation exit (7) once
    /// `e6.6` wires the gate. The ratchet baseline is what keeps existing debt out of it.
    Deny,
}

impl Mode {
    /// The stable spelling, which is also the `fathom.toml` and JSON spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Warn => "warn",
            Mode::Deny => "deny",
        }
    }

    /// Parse the three spellings. `None` is a stated refusal at load, never a silent `off`.
    pub fn parse(text: &str) -> Option<Mode> {
        match text {
            "off" => Some(Mode::Off),
            "warn" => Some(Mode::Warn),
            "deny" => Some(Mode::Deny),
            _ => None,
        }
    }
}

/// One `package = "glob = name"` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackagePin {
    /// The path prefix or glob, as written.
    pub pattern: String,
    /// The package name a matching file belongs to.
    pub package: String,
    /// The line the entry was written on, for the notice that refuses it.
    pub line: usize,
}

impl PackagePin {
    /// Whether this pin claims `path`. The longest pattern that matches wins, so a repository can
    /// pin `sdk/ts` beside `sdk/ts/vendor`.
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        if is_glob(&self.pattern) {
            glob_match(&self.pattern, path)
        } else {
            path == self.pattern
                || path
                    .strip_prefix(&self.pattern)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
    }
}

/// The `[api]` table, read once and then trusted by everything in this module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiPolicy {
    /// [`Mode::Off`]'s promise is that nothing is claimed, not that nothing is wrong.
    pub mode: Mode,
    /// Paths whose exports are not part of any tracked surface.
    pub ignore: Vec<String>,
    /// Pinned package attributions, longest-match-wins.
    pub pins: Vec<PackagePin>,
    /// Packages whose changes are reported but never get a semver recommendation.
    pub private: Vec<String>,
    /// The memory ceiling, per package.
    pub max_items_per_package: usize,
    /// Whether files the built-in test table claims are tracked as public surface.
    ///
    /// `false` by default — see [`crate::api::testpath`] for the argument. A repository whose tests
    /// *are* the published artefact sets this to `true` and gets the pre-`reco-u1i` behaviour back,
    /// with the exclusion counted and named either way.
    pub track_tests: bool,
}

impl Default for ApiPolicy {
    /// The default policy, which is [`ApiPolicy::default_policy`] and not the derived zero: a
    /// derived default would set the item ceiling to zero items per package, which reads as an empty
    /// surface rather than as an unconfigured policy.
    fn default() -> Self {
        Self::default_policy()
    }
}

impl ApiPolicy {
    /// The default policy: everything tracked, everything public, `warn`.
    ///
    /// A repository with no `[api]` table is not a repository with no public API; it is a repository
    /// that has not told us which half of it is internal, and the honest default is to track it all
    /// and say so.
    #[must_use]
    pub fn default_policy() -> Self {
        ApiPolicy {
            mode: Mode::Warn,
            ignore: Vec::new(),
            pins: Vec::new(),
            private: Vec::new(),
            max_items_per_package: DEFAULT_MAX_ITEMS_PER_PACKAGE,
            track_tests: DEFAULT_TRACK_TESTS,
        }
    }

    /// Read `[api]` out of a `fathom.toml` document.
    ///
    /// Absent is a normal state and is not a notice: the default policy is a real answer. A key that
    /// is *present and wrong* is a notice with the key's line in it, because a `mode = "report"`
    /// typo that quietly fell back to `warn` is the silent no-op this module refuses to be.
    #[must_use]
    pub fn from_repo_config_text(text: &str) -> (Self, Vec<String>) {
        let mut policy = Self::default_policy();
        let mut notices = Vec::new();
        let document = match text.parse::<toml_edit::DocumentMut>() {
            Ok(document) => document,
            Err(err) => {
                notices.push(format!(
                    "fathom.toml does not parse, so no [api] policy was applied: every path in the \
                     repository is tracked as public; parse error: {err}"
                ));
                return (policy, notices);
            }
        };
        let Some(table) = document.get("api").and_then(|item| item.as_table()) else {
            return (policy, notices);
        };
        if let Some(item) = table.get("mode").and_then(|v| v.as_value()) {
            match item.as_str().and_then(Mode::parse) {
                Some(mode) => policy.mode = mode,
                None => notices.push(format!(
                    "api.mode is {:?}, which is not `off`, `warn` or `deny`; kept `{}`",
                    item.as_str().unwrap_or_default(),
                    policy.mode.as_str()
                )),
            }
        }
        policy.ignore = string_list(table, "ignore");
        policy.private = string_list(table, "private");
        if let Some(item) = table.get("track_tests").and_then(|v| v.as_value()) {
            match item.as_bool() {
                Some(track) => policy.track_tests = track,
                None => {
                    let written = match (item.as_str(), item.as_integer()) {
                        (Some(text), _) => format!("the string {text:?}"),
                        (_, Some(number)) => format!("the number {number}"),
                        _ => "a value that is not a bool".to_string(),
                    };
                    notices.push(format!(
                        "api.track_tests is {written}, which is not a boolean; test files stay {} \
                         (write `track_tests = true` to track them as surface, `false` to exclude \
                         them)",
                        if DEFAULT_TRACK_TESTS {
                            "excluded from the surface"
                        } else {
                            "tracked as surface"
                        }
                    ));
                }
            }
        }
        if let Some(item) = table
            .get("max_items_per_package")
            .and_then(|v| v.as_value())
        {
            match item.as_integer() {
                Some(number) if number >= 1 => {
                    policy.max_items_per_package = usize::try_from(number).unwrap_or(usize::MAX);
                }
                Some(number) => notices.push(format!(
                    "api.max_items_per_package is {number}, which would bound the surface to \
                     nothing; kept {DEFAULT_MAX_ITEMS_PER_PACKAGE}"
                )),
                None => notices.push(
                    "api.max_items_per_package is not a whole number; kept the built-in bound"
                        .to_string(),
                ),
            }
        }
        if let Some(item) = table.get("package") {
            let line = key_line(text, "package");
            match item.as_array() {
                None => notices.push(format!(
                    "api.package (line {line}) is not a list of `\"glob = name\"` strings; it \
                     pinned nothing"
                )),
                Some(list) => {
                    for entry in list.iter() {
                        match entry.as_str() {
                            None => notices.push(
                                "api.package has an entry that is not a string; it was skipped"
                                    .to_string(),
                            ),
                            Some(raw) => match parse_pin(raw, line) {
                                Ok(pin) => policy.pins.push(pin),
                                Err(why) => {
                                    notices.push(format!("api.package entry {raw:?}: {why}"))
                                }
                            },
                        }
                    }
                    // Longest pattern first, so a specific pin beats a broad one wherever both
                    // match, without depending on the order the author typed them in.
                    policy.pins.sort_by(|a, b| {
                        b.pattern
                            .len()
                            .cmp(&a.pattern.len())
                            .then_with(|| a.pattern.cmp(&b.pattern))
                    });
                }
            }
        }
        (policy, notices)
    }

    /// Whether a file's exports are tracked at all.
    #[must_use]
    pub fn ignores(&self, path: &str) -> bool {
        self.ignore_pattern(path).is_some()
    }

    /// The `[api] ignore` pattern claiming `path`, if one does. A prefix matches only at a path
    /// boundary, so `crates/api` never swallows `crates/apifoo`.
    fn ignore_pattern(&self, path: &str) -> Option<&String> {
        self.ignore.iter().find(|pattern| {
            let pattern: &str = pattern.as_str();
            if is_glob(pattern) {
                glob_match(pattern, path)
            } else {
                path == pattern
                    || path
                        .strip_prefix(pattern)
                        .is_some_and(|rest| rest.starts_with('/'))
            }
        })
    }

    /// Why this path is off the tracked surface, or [`Exclusion::Tracked`].
    ///
    /// The two exclusions are kept apart on purpose. `[api] ignore` is a claim the repository's owner
    /// makes and can walk back; the test table is a claim the *language* makes, and a report that
    /// merged the two would let a repository conclude that nothing had been excluded when in fact a
    /// third of its files were declined by a built-in rule.
    #[must_use]
    pub fn exclusion(&self, language: Language, path: &str) -> Exclusion {
        if let Some(pattern) = self.ignore_pattern(path) {
            return Exclusion::Ignored {
                pattern: pattern.clone(),
            };
        }
        if !self.track_tests
            && let Some(rule) = testpath::rule(language, path)
        {
            return Exclusion::TestFile { rule };
        }
        Exclusion::Tracked
    }

    /// Whether a file's exports are tracked at all, by either rule.
    #[must_use]
    pub fn excludes(&self, language: Language, path: &str) -> bool {
        self.exclusion(language, path).is_excluded()
    }

    /// The pinned package for a path, if a pin claims it.
    #[must_use]
    pub fn pinned_package(&self, path: &str) -> Option<&str> {
        self.pins
            .iter()
            .find(|pin| pin.matches(path))
            .map(|pin| pin.package.as_str())
    }

    /// Whether a package's changes get a semver recommendation.
    #[must_use]
    pub fn is_private(&self, package: &str) -> bool {
        self.private.iter().any(|name| name == package)
    }

    /// The sentence that says how much of the repository this policy declined to look at, which is
    /// the sentence that has to be true for "no public API change" to mean anything.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut out = format!("api.mode = {}", self.mode.as_str());
        if !self.ignore.is_empty() {
            out.push_str(&format!(
                "; {} ignore pattern(s) exclude paths from the surface: {}",
                self.ignore.len(),
                self.ignore.join(", ")
            ));
        }
        if !self.private.is_empty() {
            out.push_str(&format!(
                "; {} package(s) are reported without a version recommendation: {}",
                self.private.len(),
                self.private.join(", ")
            ));
        }
        out.push_str(if self.track_tests {
            "; `api.track_tests = true`, so test files are tracked as surface like any other file"
        } else {
            "; test files are not surface (the built-in table in `api::testpath`), counted and named \
             when a report declines them — `api.track_tests = true` tracks them anyway"
        });
        if self.pins.is_empty() && self.ignore.is_empty() && self.private.is_empty() {
            out.push_str(
                "; no [api] table, so every path with a grammar is tracked as public surface — \
                 say `ignore` or `private` for the halves that are not",
            );
        }
        out
    }
}

/// `package = "glob = name"` — the one line of grammar this table has.
fn parse_pin(raw: &str, line: usize) -> Result<PackagePin, String> {
    let (pattern, package) = raw
        .split_once('=')
        .ok_or_else(|| "want `\"<path or glob> = <package name>\"`".to_string())?;
    let pattern = pattern.trim();
    let package = package.trim();
    if pattern.is_empty() {
        return Err("the path side is empty; it would match nothing".to_string());
    }
    if package.is_empty() {
        return Err("the package name is empty; say what the path is called".to_string());
    }
    if package.contains(char::is_whitespace) {
        return Err(format!(
            "the package name {package:?} contains whitespace; a package name is one word"
        ));
    }
    Ok(PackagePin {
        pattern: pattern.to_string(),
        package: package.to_string(),
        line,
    })
}

/// Whether a pattern has to be read as a glob. Same rule as `[layers]` and `[layering]`.
fn is_glob(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

/// A list of strings, or nothing — with the refusal said by the caller that has the line number.
fn string_list(table: &toml_edit::Table, key: &str) -> Vec<String> {
    let Some(item) = table.get(key) else {
        return Vec::new();
    };
    let Some(list) = item.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| entry.as_str().map(str::to_string))
        .collect()
}

/// The one-based line a top-level-ish key was written on, for a notice a human can act on.
fn key_line(text: &str, key: &str) -> usize {
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == key || trimmed.starts_with(&format!("{key} ")) {
            return index + 1;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_table_is_the_default_policy_and_says_nothing() {
        let (policy, notices) = ApiPolicy::from_repo_config_text("[layers]\napi = []\n");
        // A key named `api` that is not a table is not `[api]`; the default stands and no notice is
        // invented for a repository that never wrote the section.
        assert_eq!(policy, ApiPolicy::default_policy());
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn a_wrong_mode_is_refused_by_name_and_line() {
        let text = "[api]\nmode = \"report\"\n";
        let (policy, notices) = ApiPolicy::from_repo_config_text(text);
        assert_eq!(policy.mode, Mode::Warn);
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].contains("api.mode"), "{notices:?}");
    }

    #[test]
    fn pins_are_longest_match_wins() {
        let text =
            "[api]\npackage = [\"sdk = broad\", \"sdk/ts/vendor = vendored\", \"sdk/ts = sdk\"]\n";
        let (policy, notices) = ApiPolicy::from_repo_config_text(text);
        assert!(notices.is_empty(), "{notices:?}");
        assert_eq!(policy.pinned_package("sdk/ts/a.ts"), Some("sdk"));
        assert_eq!(
            policy.pinned_package("sdk/ts/vendor/b.ts"),
            Some("vendored")
        );
        assert_eq!(policy.pinned_package("sdk/other.ts"), Some("broad"));
        assert_eq!(policy.pinned_package("elsewhere.ts"), None);
    }

    #[test]
    fn a_pin_with_no_package_name_is_refused() {
        let text = "[api]\npackage = [\"sdk/ts =\", \"= nothing\", \"a b = c d\"]\n";
        let (policy, notices) = ApiPolicy::from_repo_config_text(text);
        assert!(policy.pins.is_empty());
        assert_eq!(notices.len(), 3, "{notices:?}");
    }

    #[test]
    fn ignore_matches_a_prefix_only_at_a_path_boundary() {
        let policy = ApiPolicy {
            ignore: vec!["crates/api".to_string()],
            ..ApiPolicy::default_policy()
        };
        assert!(policy.ignores("crates/api/lib.rs"));
        assert!(policy.ignores("crates/api"));
        assert!(
            !policy.ignores("crates/apifoo/lib.rs"),
            "a prefix that is not a directory boundary must not swallow a sibling"
        );
    }

    #[test]
    fn test_files_are_off_the_surface_by_default_and_the_says_so() {
        let policy = ApiPolicy::default_policy();
        assert!(!policy.track_tests);
        assert!(policy.excludes(Language::Python, "tests/test_release_contract.py"));
        assert!(!policy.excludes(Language::Python, "src/recommender/api/extract.py"));
        // The two exclusions stay distinguishable, because a reader has to be able to tell a rule
        // the repository wrote from a rule the language wrote.
        let stated = ApiPolicy {
            ignore: vec!["vendor/**".to_string()],
            ..ApiPolicy::default_policy()
        };
        assert!(matches!(
            stated.exclusion(Language::Python, "vendor/lib/thing.py"),
            Exclusion::Ignored { .. }
        ));
        assert!(matches!(
            stated.exclusion(Language::Python, "tests/test_thing.py"),
            Exclusion::TestFile { .. }
        ));
        assert!(policy.sentence().contains("test files are not surface"));
    }

    #[test]
    fn track_tests_true_puts_the_test_tree_back_on_the_surface() {
        let text = "[api]\ntrack_tests = true\n";
        let (policy, notices) = ApiPolicy::from_repo_config_text(text);
        assert!(notices.is_empty(), "{notices:?}");
        assert!(policy.track_tests);
        assert!(!policy.excludes(Language::Python, "tests/test_release_contract.py"));
        assert!(policy.sentence().contains("api.track_tests = true"));
    }

    #[test]
    fn a_track_tests_that_is_not_a_boolean_is_refused() {
        let (policy, notices) = ApiPolicy::from_repo_config_text("[api]\ntrack_tests = \"yes\"\n");
        assert!(!policy.track_tests, "a wrong value keeps the built-in");
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].contains("api.track_tests"), "{notices:?}");
    }
}

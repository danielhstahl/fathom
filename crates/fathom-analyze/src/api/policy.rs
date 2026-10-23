//! `[api]` — which paths' exports the rest of the world compiles against.
//!
//! Every other number in this module is derived; this one is a claim about the repository that only
//! its owner can make. A workspace of nine crates has exactly two of them whose `pub` items are
//! somebody else's contract, and a tool that reports a breaking change in the third is reporting
//! noise that will be ignored, which is worse than reporting nothing: the four files that were
//! ignored on purpose are the reason the one that mattered was scrolled past.
//!
//! So three knobs, each refused loudly when written wrong (the same discipline
//! [`crate::layering::rules`] applies to a rule that can match nothing):
//!
//! ```toml
//! [api]
//! mode = "warn"                       # off | warn | deny (deny is the future `fathom check` exit)
//! ignore = ["**/tests/**", "crates/fathom-cli/**"]
//! package = ["crates/fathom-core = fathom-core", "sdk/ts = @acme/sdk"]
//! private = ["fathom-fixtures"]
//! max_items_per_package = 20000
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

use crate::graph::layers::glob_match;
use serde::Serialize;

/// The built-in ceiling on items per package. It is a memory bound, not a correctness bound: above
/// it the package says so instead of growing without limit ([`crate::api::Package::truncated`]).
pub const DEFAULT_MAX_ITEMS_PER_PACKAGE: usize = 20_000;

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
        self.ignore.iter().any(|pattern| {
            if is_glob(pattern) {
                glob_match(pattern, path)
            } else {
                path == *pattern
                    || path
                        .strip_prefix(pattern)
                        .is_some_and(|rest| rest.starts_with('/'))
            }
        })
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
}

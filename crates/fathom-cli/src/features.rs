//! What this binary was compiled with, as data (ADR-002 §6.4).
//!
//! `fathom --version` prints the resolved feature set and `fathom doctor --privacy` prints it with the
//! consent column, so a bug report says what was built. That is the difference between a reproducible
//! report and a folklore argument, and it is also the only way the `net-*` promise can be checked by a
//! user rather than trusted by one.

use fathom_core::privacy::NET_FEATURES;

/// The version cargo built this binary with — the one number every crate shares.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every feature this crate declares, with whether *this build* has it. Order is the order §6.2
/// publishes, so `--version` output is diffable against the ADR table.
const DECLARED: [(&str, bool); 11] = [
    ("tui", cfg!(feature = "tui")),
    ("mcp", cfg!(feature = "mcp")),
    ("watch", cfg!(feature = "watch")),
    ("sqlite-bundled", cfg!(feature = "sqlite-bundled")),
    ("langs-all", cfg!(feature = "langs-all")),
    ("deps-all", cfg!(feature = "deps-all")),
    ("minimal", cfg!(feature = "minimal")),
    ("net-advisories", cfg!(feature = "net-advisories")),
    ("net-llm", cfg!(feature = "net-llm")),
    ("net-http-transport", cfg!(feature = "net-http-transport")),
    ("schema-gen", cfg!(feature = "schema-gen")),
];

/// The features compiled into this binary.
#[must_use]
pub fn compiled() -> Vec<&'static str> {
    DECLARED
        .iter()
        .filter(|(_name, enabled)| *enabled)
        .map(|(name, _enabled)| *name)
        .collect()
}

/// Is this feature name compiled in? Unknown names are `false`, not an error: the caller is describing
/// a build, not validating a spelling.
#[must_use]
pub fn compiled_in(feature: &str) -> bool {
    DECLARED
        .iter()
        .any(|(name, enabled)| *enabled && *name == feature)
}

/// The string behind `fathom --version`.
///
/// `e0.7` owns this format (ADR-002 §10.1): version, then the resolved feature set, one line, stable
/// enough to grep. Anything more (a git SHA, a target triple) belongs in `fathom doctor`, which is the
/// verb for diagnosis; `--version` stays a one-liner scripts can parse.
#[must_use]
pub fn version_line() -> String {
    let features = compiled().join(" ");
    let features = if features.is_empty() {
        "(none)".to_owned()
    } else {
        features
    };
    format!("{VERSION} features: {features}")
}

/// A privacy row: a gated feature, whether it is in this build, and the key that unlocks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacyRow {
    /// The `net-*` feature name.
    pub feature: &'static str,
    /// The crate that owns it.
    pub owner: &'static str,
    /// Whether this binary was compiled with it.
    pub compiled_in: bool,
    /// The `fathom.toml` key that consents to it.
    pub consent_key: &'static str,
    /// The plan key that owns the implementation.
    pub ticket: &'static str,
}

/// The table `fathom doctor --privacy` prints, and the one the privacy CI job asserts against.
///
/// Note what a `false` in `compiled_in` means: the code is not in the binary at all, so no consent and
/// no runtime flag can make it reach a network. That is the compile-time half of ADR-002 §6.3, and this
/// row is where a user can see it.
#[must_use]
pub fn privacy_table() -> Vec<PrivacyRow> {
    NET_FEATURES
        .iter()
        .map(|feature| PrivacyRow {
            feature: feature.feature,
            owner: feature.owner,
            compiled_in: compiled_in(feature.feature),
            consent_key: feature.consent_key,
            ticket: feature.ticket,
        })
        .collect()
}

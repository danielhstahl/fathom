//! What this binary was compiled with, as data (ADR-002 §6.4), and what the user has consented to.
//!
//! `fathom --version` prints the resolved feature set and `fathom doctor --privacy` prints it with the
//! consent column, so a bug report says what was built. That is the difference between a reproducible
//! report and a folklore argument, and it is also the only way the `net-*` promise can be checked by a
//! user rather than trusted by one.
//!
//! The consent half is deliberately read from the *user's* files only. `privacy.*` keys are user-scoped
//! (ADR-001 §4.6, ADR-002 §6.3): a repository may not consent on its owner's behalf, so this module does
//! not open the repository's `fathom.toml` for consent at all — except to say, when a clone did set one of
//! those keys, that the setting is ignored and where the real switch lives.

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

/// One consent key, whether the user has granted it, and where that answer came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentState {
    /// The `privacy.*` key.
    pub key: &'static str,
    /// Whether the user's own config says `true`. A repository file never contributes here (ADR-002 §6.3).
    pub granted: bool,
    /// The layer that decided it — `user`, `legacy`, or `default` when no user file names the key.
    pub layer: &'static str,
}

/// The user-scoped layers `[privacy]` may live in, in precedence order, as `(layer, path)`.
///
/// `fathom doctor --privacy` prints these paths (redacted, as every path in that report is) because
/// "consent is off" is only actionable if the user knows which file to open.
#[must_use]
pub fn consent_files() -> Vec<(&'static str, std::path::PathBuf)> {
    let legacy_dir =
        std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".fathom"));
    crate::userconfig::candidates()
        .into_iter()
        // `userconfig::candidates()` is XDG-first, then the legacy `~/.fathom` spelling; these are the
        // same two layer names `fathom doctor`'s config section prints for the same files.
        .map(|path| {
            (
                if legacy_dir
                    .as_deref()
                    .is_some_and(|dir| path.starts_with(dir))
                {
                    "legacy"
                } else {
                    "user"
                },
                path,
            )
        })
        .collect()
}

/// The `[privacy]` table of one file, as `(key, value)` pairs. An unreadable or unparseable file is no
/// table at all — `fathom index` already reported that the config does not parse, and `doctor --privacy`
/// is about what is switched on, not about re-reporting a broken file.
fn privacy_table_of(path: &std::path::Path) -> Vec<(String, bool)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let Some(table) = document.get("privacy").and_then(|item| item.as_table()) else {
        return Vec::new();
    };
    table
        .iter()
        .map(|(key, value)| {
            (
                key.to_owned(),
                // `privacy.llm = "yes"` is not consent. An unparseable value reads as `false`, because
                // that is the only direction that cannot leak a packet.
                value.as_bool().unwrap_or(false),
            )
        })
        .collect()
}

/// The consent state of every `privacy.*` key this build knows about.
///
/// One read per file, not one per `(file, key)`: the answer for three keys is two reads, not six.
#[must_use]
pub fn consent_states() -> Vec<ConsentState> {
    let layers = consent_files();
    let read: Vec<(&'static str, Vec<(String, bool)>)> = layers
        .iter()
        .map(|(layer, path)| (*layer, privacy_table_of(path)))
        .collect();
    NET_FEATURES
        .iter()
        .map(|feature| {
            let key = feature
                .consent_key
                .strip_prefix("privacy.")
                .unwrap_or(feature.consent_key);
            let found = read
                .iter()
                .find(|(_layer, table)| table.iter().any(|(k, _)| k == key));
            match found {
                Some((layer, table)) => ConsentState {
                    key: feature.consent_key,
                    granted: table
                        .iter()
                        .find(|(k, _)| k == key)
                        .is_some_and(|(_k, value)| *value),
                    layer,
                },
                None => ConsentState {
                    key: feature.consent_key,
                    granted: false,
                    layer: "default",
                },
            }
        })
        .collect()
}

/// Consent a *repository* tried to grant, and that this build ignores: every `privacy.*` key the
/// repository's own `fathom.toml` sets to true.
///
/// This is not a hypothetical: a cloned repository that ships `privacy.advisories = true` would otherwise
/// be a `git clone`-to-egress path one commit wide, which is the exact thing ADR-001 §4.6 forbids. Reporting it
/// is what turns "we ignored it" into something the user can see rather than a property of code they
/// cannot read.
#[must_use]
pub fn consent_the_repository_tried(root: &std::path::Path) -> Vec<String> {
    privacy_table_of(&root.join("fathom.toml"))
        .into_iter()
        .filter(|(_key, value)| *value)
        .map(|(key, _value)| format!("privacy.{key}"))
        .filter(|key| fathom_core::privacy::is_consent_key(key))
        .collect()
}

/// The local records this build writes, as the sentence `doctor --privacy` prints.
///
/// It is a claim about the *absence* of a thing, so it is worded as one: no endpoint, no identifier, no
/// background writer, and nothing on disk that leaves the machine on its own. `tools/check_privacy.py`
/// is what makes the claim a gate rather than a paragraph.
pub const LOCAL_ONLY_SENTENCE: &str = "no endpoint, no identifier, no background writer: everything \
                                      Fathom writes stays under `.fathom/` in this repository, where you \
                                      can read it, export it and delete it";

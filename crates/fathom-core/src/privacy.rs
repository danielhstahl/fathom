//! The network-consent table (ADR-002 §6.3, thesis glossary: *network consent*).
//!
//! Two independent gates keep the promise "no network call unless the user turned a named feature on":
//! the compile-time one (the code is not in the binary, which `cargo tree` and lint `N01`–`N03` check)
//! and this one, the runtime one. A `net-*` feature that *is* compiled in still does nothing until the
//! matching key in `fathom.toml` is true, and the refusal names the key in the message — which is the
//! difference between a user who is told and a user who is blocked by a mystery.
//!
//! The table is data, here in `fathom-core`, so that the crate that prints it (`fathom-cli`'s
//! `doctor --privacy`), the crate that enforces it and the test that checks it all read the same rows.
//! `tests/test_workspace_contract.py` fails if these rows drift from `tools/workspace_model.py`, which
//! is what generated the table in ADR-002 §6.2.

/// One opt-in: the feature, the crate that owns it, and the key that unlocks it at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetFeature {
    /// The cargo feature name — always `net-*`, which is what makes it greppable (lint `N02`).
    pub feature: &'static str,
    /// The crate the feature is declared in.
    pub owner: &'static str,
    /// The `fathom.toml` key whose truth unlocks the first outbound call.
    pub consent_key: &'static str,
    /// The plan key that owns the implementation.
    pub ticket: &'static str,
}

/// Every feature that can touch a socket, exactly as ADR-002 §6.2 publishes them.
pub const NET_FEATURES: [NetFeature; 3] = [
    NetFeature {
        feature: "net-advisories",
        owner: "fathom-analyze",
        consent_key: "privacy.advisories",
        ticket: "e2.7",
    },
    NetFeature {
        feature: "net-llm",
        owner: "fathom-analyze",
        consent_key: "privacy.llm",
        ticket: "e4.7",
    },
    NetFeature {
        feature: "net-http-transport",
        owner: "fathom-mcp",
        consent_key: "privacy.mcp_http_transport",
        ticket: "e6.4",
    },
];

impl NetFeature {
    /// The sentence a user gets when a compiled-in feature was asked to run without consent. It ends
    /// with the key, because "requires consent" without the key is a mystery, not an error.
    #[must_use]
    pub fn refusal(&self) -> String {
        format!(
            "`{}` is compiled in but not consented to: set `{} = true` in fathom.toml",
            self.feature, self.consent_key
        )
    }
}

/// The consent key for a `net-*` feature, or `None` when the feature is not a network feature at all.
/// `None` is not "no consent needed" — callers ask about features they already know are gated.
#[must_use]
pub fn consent_key(feature: &str) -> Option<&'static str> {
    NET_FEATURES
        .iter()
        .find(|candidate| candidate.feature == feature)
        .map(|candidate| candidate.consent_key)
}

/// Is `key` one of the consent keys this table defines? `fathom-cli` uses this to reject a `privacy.*`
/// key in `fathom.toml` that no feature will ever read, instead of accepting a typo forever.
#[must_use]
pub fn is_consent_key(key: &str) -> bool {
    NET_FEATURES
        .iter()
        .any(|candidate| candidate.consent_key == key)
}

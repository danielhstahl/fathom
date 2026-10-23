//! What the operator decided about this server: the policy the composition root resolves and the server
//! obeys (ADR-005 §4.2, ADR-002 §6.3).
//!
//! Nothing here reads a config file — `fathom-mcp` is IO-free below the surfaces (lint `W1`) — so every
//! value arrives as an argument, resolved by `fathom mcp serve` out of the layers `fathom config explain`
//! prints. The defaults are the interesting part, because they are the shipped posture:
//!
//! * **writes are off.** `mcp.allow_writes` defaults to `false`, the write tools are absent from
//!   `tools/list`, and a call that names one anyway is refused with `no_capability`.
//! * **`mark_reviewed` needs a token bound to the change**, and no token can be derived without the secret
//!   in `mcp.approval_token` — a user-scoped key a clone cannot write. Without the secret configured the
//!   tool refuses. An auto-approval path that could mark things reviewed without a human is refused here,
//!   in code, not by a prompt asking nicely (ADR-005 §4.2).
//! * **stdio.** The HTTP transport is a listener and so needs the compiled-in `net-http-transport` feature
//!   *and* `privacy.mcp_http_transport` ([`crate::transport::check_consent`]); neither is on in a default
//!   build, and asking for `http` is a refusal rather than a fallback.

use sha2::{Digest as _, Sha256};

use crate::tools::PAGE_LIMIT_MAX;

/// The transport the server was told to speak.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Transport {
    /// Standard input and output: no socket, no consent (ADR-002 §6.3).
    #[default]
    Stdio,
    /// A listener. Refused unless the feature *and* the consent key are present.
    Http {
        /// The address it was asked to bind.
        bind: String,
        /// Whether `mcp.auth_token` is set; an unauthenticated listener is refused on a non-loopback bind.
        auth_configured: bool,
    },
}

impl Transport {
    /// The wire name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http { .. } => "http",
        }
    }

    /// Is this a loopback bind? `mcp.bind` is checked when it is resolved; this is the server's own last
    /// look, because a listener that is not loopback is a disclosure and not a convenience.
    #[must_use]
    pub fn loopback(&self) -> bool {
        match self {
            Self::Stdio => true,
            Self::Http { bind, .. } => bind == "127.0.0.1" || bind == "localhost" || bind == "::1",
        }
    }
}

/// The resolved policy for one server run.
#[derive(Debug, Clone)]
pub struct Policy {
    /// What is serving, for the `server_version` an answer carries.
    pub server_version: String,
    /// The transport the operator asked for.
    pub transport: Transport,
    /// `mcp.allow_writes`: the write tools exist only when this is true (ADR-005 §4.2).
    pub writes_enabled: bool,
    /// `mcp.approval_token`: the secret `mark_reviewed`'s bound token is derived from. `None` means the
    /// tool refuses, which is the shipped default.
    pub approval_secret: Option<String>,
    /// `mcp.audit_reads`: also write an audit line for the read tools. Off by default — a read audit is a
    /// log of what an agent was curious about, which is worth having but is not what §4.2 asks for.
    pub audit_reads: bool,
    /// `mcp.max_response_bytes`: the ceiling on one envelope. A page that will not fit is cut and handed
    /// back with a cursor.
    pub max_response_bytes: usize,
    /// The largest page any tool will answer with, whatever the tool's own bound.
    pub page_limit_max: usize,
}

impl Default for Policy {
    /// The shipped posture: stdio, writes off, no approval secret, 256 KiB per answer.
    fn default() -> Self {
        Self {
            server_version: format!("fathom-mcp/{}", env!("CARGO_PKG_VERSION")),
            transport: Transport::default(),
            writes_enabled: false,
            approval_secret: None,
            audit_reads: false,
            max_response_bytes: 256 * 1024,
            page_limit_max: PAGE_LIMIT_MAX,
        }
    }
}

impl Policy {
    /// The token `mark_reviewed` will accept for `change_id`, or `None` when no secret is configured.
    ///
    /// It is a keyed digest over the tool name and the change id, truncated to 32 hex characters: long
    /// enough not to be guessed, short enough to paste, and — the property the gate exists for — *bound*:
    /// a token minted for one change cannot be replayed on another, so an agent that got one signed-off
    /// change past cannot use it to sign off the rest of the queue. Whoever holds the secret decides which
    /// changes the agent may mark; whoever does not, cannot.
    #[must_use]
    pub fn approval_token(&self, change_id: &str) -> Option<String> {
        let secret = self.approval_secret.as_deref()?;
        let mut hasher = Sha256::new();
        hasher.update(b"fathom.mcp.approval/1");
        hasher.update([0x1f]);
        hasher.update(secret.as_bytes());
        hasher.update([0x1f]);
        hasher.update(change_id.as_bytes());
        let digest = hasher.finalize();
        let hex: String = digest
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Some(hex)
    }

    /// Does the supplied token authorise this change? Constant-time in the obvious way that matters: a
    /// missing secret refuses before anything is compared, and a wrong token is simply not the token.
    #[must_use]
    pub fn approval_ok(&self, change_id: &str, supplied: Option<&str>) -> bool {
        let (Some(expected), Some(supplied)) = (self.approval_token(change_id), supplied) else {
            return false;
        };
        equal_bytes(expected.as_bytes(), supplied.as_bytes())
    }
}

/// A byte-length-sensitive equality that does not short-circuit on the first difference.
fn equal_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in a.iter().zip(b) {
        diff |= left ^ right;
    }
    diff == 0
}

/// The digest an audit line carries for one set of arguments.
///
/// Canonical means *sorted*: the same arguments in a different order must produce the same digest, or the
/// audit trail cannot be compared to itself. Values are hashed as their compact JSON, so a note body leaves
/// no plaintext trace in the log while still being provable-by-digest to whoever kept the call.
#[must_use]
pub fn args_digest(args: &[(String, serde_json::Value)]) -> String {
    let mut sorted: Vec<&(String, serde_json::Value)> = args.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    hasher.update(b"fathom.mcp.args/1");
    for (key, value) in sorted {
        hasher.update([0x1f]);
        hasher.update(key.as_bytes());
        hasher.update([0x1f]);
        let rendered = serde_json::to_string(value).unwrap_or_default();
        hasher.update(rendered.as_bytes());
    }
    let digest = hasher.finalize();
    format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use serde_json::json;

    #[test]
    fn a_bound_token_is_bound_to_one_change() {
        let policy = Policy {
            approval_secret: Some("a-secret".to_owned()),
            ..Policy::default()
        };
        let token = policy
            .approval_token("ep-1")
            .expect("a configured secret mints a token");
        assert!(policy.approval_ok("ep-1", Some(&token)));
        assert!(
            !policy.approval_ok("ep-2", Some(&token)),
            "a token for one change must not sign off another"
        );
    }

    #[test]
    fn no_secret_means_no_token_and_no_approval() {
        let policy = Policy::default();
        assert!(policy.approval_token("ep-1").is_none());
        assert!(!policy.approval_ok("ep-1", Some("whatever")));
    }

    #[test]
    fn an_argument_digest_is_stable_under_reordering() {
        let one = vec![
            ("change_id".to_owned(), json!("ep-1")),
            ("reviewer".to_owned(), json!("Ana")),
        ];
        let two = vec![
            ("reviewer".to_owned(), json!("Ana")),
            ("change_id".to_owned(), json!("ep-1")),
        ];
        assert_eq!(args_digest(&one), args_digest(&two));
    }
}

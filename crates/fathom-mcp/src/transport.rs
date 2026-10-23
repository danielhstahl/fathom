//! Transports, and the two gates in front of anything that could reach a socket (ADR-002 §6.3).

use std::sync::Arc;

use crate::error::Error;
use crate::policy::{Policy, Transport};
use crate::server::FathomMcp;
use crate::source::Source;
use fathom_core::privacy;

/// The transport fathom ships: standard input and standard output.
///
/// It is not a network feature and the distinction is the point — §6.3's claim is about sockets, and a pipe
/// is not one. Every consumer of this crate's default feature set gets this transport and nothing else,
/// which is why the privacy claim can be checked by looking at the link line rather than at a config file.
pub const STDIO: &str = "stdio";

/// The runtime half of the network-consent gate.
///
/// `consented` is the set of consent keys the resolved config says are true (owned by `e0.8`'s CLI
/// half). A feature that is not a gated `net-*` feature has nothing to consent to and returns `Ok`: this
/// function refuses, it does not police spelling.
///
/// # Errors
///
/// [`Error::NetworkConsentRequired`] when the feature is gated and its key was not set.
pub fn check_consent(feature: &'static str, consented: &[&'static str]) -> Result<(), Error> {
    let Some(key) = privacy::consent_key(feature) else {
        return Ok(());
    };
    if consented.contains(&key) {
        Ok(())
    } else {
        Err(Error::NetworkConsentRequired { feature, key })
    }
}

/// Serve the tool surface until the client disconnects.
///
/// The capability gate lives in the handler ([`crate::server`]), *not* in the transport, which is the whole
/// reason ADR-005 §4.3's rule is enforceable: choosing a different transport cannot walk around it, and the
/// stdio half of this function has no consent check to walk around.
///
/// # Errors
///
/// [`Error::NetworkConsentRequired`] when `policy.transport` is a listener and its consent key is not in
/// `consented`; [`Error::NotImplemented`] when a consented listener is asked of a build that carries the
/// marker but not the listener (ADR-002 §10.3); whatever rmcp reports for a broken pipe.
pub async fn serve(
    policy: Policy,
    source: Arc<dyn Source>,
    consented: &[&'static str],
) -> Result<(), Error> {
    if matches!(policy.transport, Transport::Http { .. }) {
        check_consent("net-http-transport", consented)?;
        // The gate above is the privacy claim, and it runs before anything else so that a listener cannot
        // be opened by a build that has not been consented to. What is behind the gate is a second matter:
        // `net-http-transport` is a marker feature (ADR-002 §10.3) because every streamable-HTTP/SSE
        // server crate in this ecosystem links `hyper`, and `deny.toml` bans `hyper` unconditionally. The
        // default build therefore carries the gate and not the listener, and says so rather than binding
        // something else and calling it the same thing.
        return Err(Error::NotImplemented {
            what: "the streamable-HTTP MCP transport (the gate passed; the listener is the marker's, see \
                   ADR-002 §10.3)",
            ticket: "e6.4",
        });
    }
    use rmcp::ServiceExt as _;
    let server = FathomMcp::new(policy, source);
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|reason| Error::Serve {
            reason: reason.to_string(),
        })?;
    service.waiting().await.map_err(|reason| Error::Serve {
        reason: reason.to_string(),
    })?;
    Ok(())
}

/// The blocking form of [`serve`], for a synchronous composition root.
///
/// `fathom-cli` owns no executor — tokio is this crate's dependency, not the binary's (ADR-002 §6.2's note
/// on why tokio is allowed at all) — so the boundary that needs one builds the smallest it can use and runs
/// exactly this future on it. A current-thread runtime is the honest size: one agent on one stdio pipe is
/// one conversation, and a thread pool would be a claim about concurrency this transport does not have.
///
/// # Errors
///
/// As [`serve`].
pub fn serve_blocking(
    policy: Policy,
    source: Arc<dyn Source>,
    consented: &[&'static str],
) -> Result<(), Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|reason| Error::Serve {
            reason: format!("no executor to serve on: {reason}"),
        })?;
    runtime.block_on(serve(policy, source, consented))
}

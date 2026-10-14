//! Transports, and the two gates in front of anything that could reach a socket (ADR-002 §6.3).

use crate::error::Error;
use fathom_core::privacy;

/// The transport fathom ships: standard input and standard output.
///
/// It is not a network feature and the distinction is the point — §6.3's claim is about sockets, and a
/// pipe is not one.
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

/// Serve the tool surface over stdio until the client disconnects.
///
/// The transport opens no socket, so nothing here needs consent; the capability gate for anything a
/// client may *write* belongs in the handler (ADR-005 §4.3) and is `e6.4`'s.
///
/// # Errors
///
/// [`Error::NotImplemented`] until `e6.4` lands the server.
pub async fn serve_stdio() -> Result<(), Error> {
    Err(Error::NotImplemented {
        what: "the MCP server",
        ticket: "e6.4",
    })
}

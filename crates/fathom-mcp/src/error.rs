//! `fathom_mcp::error` — including the refusal that makes "no network by default" a fact (ADR-002 §6.3).
//!
//! A `net-*` feature that is compiled in still does nothing until the matching key in `fathom.toml` is
//! true. When it is asked anyway, the caller does not get "permission denied"; it gets the key that
//! changes it, in the message, and the process leaves with exit code 8. That is the difference between
//! a user who is told and a user who opens an issue.

use fathom_core::error::{Diagnosable, ErrorCode};
use thiserror::Error;

/// Everything the server can fail with.
#[derive(Debug, Error)]
pub enum Error {
    /// A compiled-in `net-*` feature was asked to make a call the user has not consented to.
    #[error("`{feature}` is compiled in but not consented to: set `{} = true` in fathom.toml", .key)]
    NetworkConsentRequired {
        /// The feature that was asked for.
        feature: &'static str,
        /// The key that unlocks it — named in the message, never only in the documentation.
        key: &'static str,
    },

    /// A published contract this build does not implement yet.
    #[error("{what} is not implemented yet (owned by {ticket})")]
    NotImplemented {
        /// The capability the caller asked for.
        what: &'static str,
        /// The plan key that owns it.
        ticket: &'static str,
    },
}

impl Diagnosable for Error {
    fn code(&self) -> ErrorCode {
        match self {
            Error::NetworkConsentRequired { .. } => ErrorCode::NetworkConsentRequired,
            Error::NotImplemented { .. } => ErrorCode::NotImplemented,
        }
    }

    fn hint(&self) -> Option<&str> {
        match self {
            Error::NetworkConsentRequired { .. } => {
                Some("`fathom doctor --privacy` lists every consent key and its state")
            }
            Error::NotImplemented { ticket, .. } => Some(*ticket),
        }
    }
}

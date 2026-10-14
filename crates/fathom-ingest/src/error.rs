//! `fathom_ingest::error` — the door's failure vocabulary (ADR-002 §7.1).

use fathom_core::error::{Diagnosable, ErrorCode};
use thiserror::Error;

/// Everything getting facts in can fail with.
#[derive(Debug, Error)]
pub enum Error {
    /// The requested root is not a repository, or its object database cannot be read.
    #[error("{root} is not a git repository this build can read")]
    NotARepository {
        /// The path that was asked for, verbatim: a user decides faster about a path they wrote.
        root: String,
    },

    /// A published contract this build does not implement yet.
    #[error("{what} is not implemented yet (owned by {ticket})")]
    NotImplemented {
        /// The operation the caller asked for.
        what: &'static str,
        /// The plan key that owns it.
        ticket: &'static str,
    },
}

impl Diagnosable for Error {
    fn code(&self) -> ErrorCode {
        match self {
            Error::NotARepository { .. } => ErrorCode::NotARepository,
            Error::NotImplemented { .. } => ErrorCode::NotImplemented,
        }
    }

    fn hint(&self) -> Option<&str> {
        match self {
            // ADR-001 §8.2: fathom never fetches, so "clone it first" is the only honest hint.
            Error::NotARepository { .. } => {
                Some("run fathom inside a working copy; it never fetches")
            }
            Error::NotImplemented { ticket, .. } => Some(*ticket),
        }
    }
}

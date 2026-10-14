//! `fathom_analyze::error` — analysis returns values, including when it fails (ADR-002 §7.1, §7.2).
//!
//! "Degrade, never block" (ADR-001 §7.6) means a missing analyzer is a stated partial answer. In
//! practice that is two rules: an unbuilt path is `Err(NotImplemented)`, never `todo!()` (lint `W6`),
//! and an unknown value is a value in the delta model, so a view can print `unknown` instead of a
//! spinner that never ends.

use fathom_core::error::{Diagnosable, ErrorCode};
use thiserror::Error;

/// Everything analysis can fail with.
#[derive(Debug, Error)]
pub enum Error {
    /// A published contract this build does not implement yet.
    #[error("{what} is not implemented yet (owned by {ticket})")]
    NotImplemented {
        /// The analysis the caller asked for.
        what: &'static str,
        /// The plan key that owns it.
        ticket: &'static str,
    },

    /// The request itself cannot be answered — as opposed to the data being insufficient, which is an
    /// `unknown` in the result rather than an error.
    #[error("the request is not answerable: {why}")]
    InvalidRequest {
        /// What about the request made it unanswerable.
        why: &'static str,
    },

    /// A format string that is not one of the three fathom promises.
    #[error("`{given}` is not an export format; try markdown, html or json")]
    UnknownFormat {
        /// What was asked for, verbatim.
        given: String,
    },
}

impl Diagnosable for Error {
    fn code(&self) -> ErrorCode {
        match self {
            Error::NotImplemented { .. } => ErrorCode::NotImplemented,
            Error::InvalidRequest { .. } | Error::UnknownFormat { .. } => ErrorCode::Usage,
        }
    }

    fn hint(&self) -> Option<&str> {
        match self {
            Error::NotImplemented { ticket, .. } => Some(*ticket),
            Error::InvalidRequest { .. } => Some("ask the facade for a smaller window"),
            Error::UnknownFormat { .. } => Some("`fathom export --help` lists the three"),
        }
    }
}

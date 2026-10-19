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

    /// A spine row whose content is not what its column claims it is.
    ///
    /// Distinct from [`Error::Store`]: the database answered perfectly well and it is the *value* that will
    /// not parse. The exit code is still the store's, because the file needs rebuilding, but the message has
    /// to name the column and the value — "corrupt" without that is a search rather than a diagnosis.
    #[error("the store holds a row it cannot mean: {what}")]
    SpineRowUnreadable {
        /// Which column, what value, and why it is not one.
        what: String,
    },

    /// The git side refused to answer. Forwarded whole, because `fathom-ingest`'s variants already carry
    /// the exit code and the hint (§7.1: one value, so the message, the hint and the code cannot disagree).
    #[error(transparent)]
    Ingest(#[from] fathom_ingest::error::Error),

    /// The store refused to answer.
    #[error(transparent)]
    Store(#[from] fathom_store::error::Error),
}

impl Diagnosable for Error {
    fn code(&self) -> ErrorCode {
        match self {
            Error::NotImplemented { .. } => ErrorCode::NotImplemented,
            Error::InvalidRequest { .. } | Error::UnknownFormat { .. } => ErrorCode::Usage,
            Error::SpineRowUnreadable { .. } => ErrorCode::StoreCorrupt,
            Error::Ingest(err) => err.code(),
            Error::Store(err) => err.code(),
        }
    }

    fn hint(&self) -> Option<&str> {
        match self {
            // A ticket number is an owner, not a next action: the reader cannot run `e5.7`. The message
            // already names the owner (`{what} is not implemented yet (owned by {ticket})`), so the hint
            // does the other half — the verb that says what this build *does* have (`e5.9`: every
            // user-facing error states something to do).
            Error::NotImplemented { .. } => Some(
                "not in this build; `fathom doctor` prints what this build does, and the ticket in \
                      the message owns what it does not",
            ),
            Error::InvalidRequest { .. } => Some("ask the facade for a smaller window"),
            Error::UnknownFormat { .. } => Some("`fathom export --help` lists the three"),
            Error::SpineRowUnreadable { .. } => Some("`fathom index --rebuild`"),
            Error::Ingest(err) => err.hint(),
            Error::Store(err) => err.hint(),
        }
    }
}

//! `fathom_tui::error` — a surface reports refusals; it does not pattern-match upstream enums.
//!
//! Note what is *not* here: a `From<fathom_analyze::Error>`. The TUI may only name the `insight` and
//! `export` modules (ADR-002 §5.3, lint `M02`), and an upstream error type arriving through `?` would be
//! a dependency smuggled in through a `#[from]` attribute. The façade's refusal is carried as its
//! message, which is all a view can render anyway.

use fathom_core::error::{Diagnosable, ErrorCode};
use thiserror::Error;

/// Everything the shell can fail with.
#[derive(Debug, Error)]
pub enum Error {
    /// The façade declined to answer.
    #[error("the review model could not answer: {why}")]
    FacadeUnavailable {
        /// The refusal, as the façade phrased it.
        why: String,
    },

    /// The screen itself refused: raw mode could not be taken, or a frame could not be written.
    #[error("the terminal refused: {why}")]
    Terminal {
        /// The refusal, as the OS or the backend phrased it.
        why: String,
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
            Error::FacadeUnavailable { .. } => ErrorCode::Unknown,
            Error::Terminal { .. } => ErrorCode::Unknown,
            Error::NotImplemented { .. } => ErrorCode::NotImplemented,
        }
    }

    fn hint(&self) -> Option<&str> {
        match self {
            Error::FacadeUnavailable { .. } => {
                Some("run `fathom doctor` to see what is compiled in")
            }
            Error::NotImplemented { ticket, .. } => Some(*ticket),
            Error::Terminal { .. } => {
                Some("`fathom --plain` renders the same frame as text with no terminal at all")
            }
        }
    }
}

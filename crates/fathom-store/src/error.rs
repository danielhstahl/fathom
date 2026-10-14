//! `fathom_store::error` — one `thiserror` enum, mapped at the boundary (ADR-002 §7.1).
//!
//! A public signature in this crate may not leak `rusqlite::Error`: every caller would become coupled
//! to rusqlite's minor versions, and the `sqlite-system` packager's failure would arrive phrased in
//! somebody else's vocabulary.

use fathom_core::error::{Diagnosable, ErrorCode};
use thiserror::Error;

/// Everything this crate can fail with.
#[derive(Debug, Error)]
pub enum Error {
    /// The linked SQLite does not define a compile option the schema is written against.
    ///
    /// This is a *loud* failure by design (ADR-004 §4.0): a schema whose `STRICT` or FTS5 semantics
    /// quietly changed is corrupt data nobody notices, not a bug to work around.
    #[error("the linked SQLite does not define `{option}`")]
    SqliteCompileOptionMissing {
        /// The `PRAGMA compile_options` entry that is missing.
        option: &'static str,
    },

    /// An integrity check failed.
    #[error("store is not usable: {what}")]
    StoreCorrupt {
        /// What was checked, phrased for `fathom doctor`.
        what: String,
    },

    /// A published contract this build does not implement yet.
    ///
    /// A stub is a typed failure and never a panic (ADR-002 §7.2, §9 rule 2): a crate can be compiled
    /// and linked against a neighbour that is not finished, which is the difference between "blocked on
    /// another team" and "shipped behind a flag".
    #[error("{what} is not implemented yet (owned by {ticket})")]
    NotImplemented {
        /// The operation the caller asked for.
        what: &'static str,
        /// The plan key that owns it, so the message is a pointer rather than a dead end.
        ticket: &'static str,
    },
}

impl Diagnosable for Error {
    fn code(&self) -> ErrorCode {
        match self {
            Error::SqliteCompileOptionMissing { .. } => ErrorCode::SqliteCompileOptionMissing,
            Error::StoreCorrupt { .. } => ErrorCode::StoreCorrupt,
            Error::NotImplemented { .. } => ErrorCode::NotImplemented,
        }
    }

    fn hint(&self) -> Option<&str> {
        match self {
            Error::SqliteCompileOptionMissing { .. } => Some(
                "rebuild with `--features sqlite-bundled`, which compiles a SQLite that has them",
            ),
            Error::StoreCorrupt { .. } => {
                Some("run `fathom doctor`; the store is disposable (ADR-004 §12)")
            }
            Error::NotImplemented { ticket, .. } => Some(*ticket),
        }
    }
}

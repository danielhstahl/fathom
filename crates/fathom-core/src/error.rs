//! The error *vocabulary*, not a shared giant error enum (ADR-002 §7.1).
//!
//! Each library crate exports its own `pub enum <Crate>Error` built with `thiserror`, with third-party
//! error types mapped at the boundary so a public signature never leaks `rusqlite::Error`. What is
//! shared is the short stable string every one of those errors carries, and the [`Diagnosable`] trait
//! that lets `fathom doctor` and the JSON writers print an actionable, testable line without matching
//! on six upstream enums. A single shared enum would force the innermost crate in the workspace to know
//! about every outer one, which is uphill and therefore refused.

use crate::exit::ExitCode;

/// A short, stable, upper-snake string that survives refactors of the enum that carries it.
///
/// These strings are what `--json` output and `fathom doctor` print, so they are a user-visible
/// contract even though the Rust type they travel on is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// the bundled SQLite was compiled without an option ADR-004 §4.0 requires of the schema
    SqliteCompileOptionMissing,
    /// a key in `fathom.toml` is not in the schema
    ConfigUnknownKey,
    /// `fathom.toml` failed the schema, or a value is the wrong type
    ConfigInvalid,
    /// the store failed an integrity check
    StoreCorrupt,
    /// the requested root is not a git repository
    NotARepository,
    /// the repository is readable but a git object it needs is not in the local object database
    ObjectUnavailable,
    /// history could not be walked to its root: the repository is shallow or filter-cloned
    HistoryTruncated,
    /// the working tree or its index could not be read (bare repository, absent index)
    WorkingTreeUnavailable,
    /// the repository is readable but uses a form this build cannot interpret
    RepositoryUnsupported,
    /// the read needs a capability this binary does not carry; the `git`-binary fallback would serve it
    FallbackRequired,
    /// a revision expression was rejected
    RevSpecRejected,
    /// a compiled-in `net-*` feature was asked to run without runtime consent
    NetworkConsentRequired,
    /// the verb needs a feature that is not compiled into this binary
    FeatureAbsent,
    /// a code path a published contract promises but this build does not implement yet
    NotImplemented,
    /// cancelled by the user or by the loss of a writer lease
    Cancelled,
    /// a declared budget in `benchmarks/budgets.json` was exceeded
    BudgetExceeded,
    /// a platform capability the contract needs is missing
    UnsupportedPlatform,
    /// findings above the configured threshold
    CheckViolations,
    /// the arguments were rejected
    Usage,
    /// nothing better was available; the message is the whole story
    Unknown,
}

impl ErrorCode {
    /// The stable string. Never derived from `format!("{:?}", …)`: a rename of the Rust variant must
    /// not silently change what a script greps for.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::SqliteCompileOptionMissing => "SQLITE_COMPILE_OPTION_MISSING",
            ErrorCode::ConfigUnknownKey => "CONFIG_UNKNOWN_KEY",
            ErrorCode::ConfigInvalid => "CONFIG_INVALID",
            ErrorCode::StoreCorrupt => "STORE_CORRUPT",
            ErrorCode::NotARepository => "NOT_A_REPOSITORY",
            ErrorCode::ObjectUnavailable => "OBJECT_UNAVAILABLE",
            ErrorCode::HistoryTruncated => "HISTORY_TRUNCATED",
            ErrorCode::WorkingTreeUnavailable => "WORKING_TREE_UNAVAILABLE",
            ErrorCode::RepositoryUnsupported => "REPOSITORY_UNSUPPORTED",
            ErrorCode::FallbackRequired => "FALLBACK_REQUIRED",
            ErrorCode::RevSpecRejected => "REV_SPEC_REJECTED",
            ErrorCode::NetworkConsentRequired => "NETWORK_CONSENT_REQUIRED",
            ErrorCode::FeatureAbsent => "FEATURE_ABSENT",
            ErrorCode::NotImplemented => "NOT_IMPLEMENTED",
            ErrorCode::Cancelled => "CANCELLED",
            ErrorCode::BudgetExceeded => "BUDGET_EXCEEDED",
            ErrorCode::UnsupportedPlatform => "UNSUPPORTED_PLATFORM",
            ErrorCode::CheckViolations => "CHECK_VIOLATIONS",
            ErrorCode::Usage => "USAGE",
            ErrorCode::Unknown => "UNKNOWN",
        }
    }

    /// The exit code this error leaves the process with (ADR-002 §7.3).
    ///
    /// This is the one place where the error vocabulary and the process contract are joined, so
    /// "the error the human sees" and "the exit code CI branches on" cannot drift apart per-verb.
    #[must_use]
    pub const fn exit(self) -> ExitCode {
        match self {
            ErrorCode::SqliteCompileOptionMissing => ExitCode::StoreCorrupt,
            ErrorCode::ConfigUnknownKey | ErrorCode::ConfigInvalid => ExitCode::ConfigInvalid,
            ErrorCode::StoreCorrupt => ExitCode::StoreCorrupt,
            ErrorCode::NotARepository => ExitCode::NotARepository,
            // None of these mean "not a repository", and none of them get their own exit code: the exit
            // code table in ADR-002 §7.3 is a published contract a script branches on, and adding a row
            // to it is a change of that contract, not a detail of one crate. The remedy lives in the
            // message and the hint; the code is what a `--json` reader keys on.
            ErrorCode::ObjectUnavailable
            | ErrorCode::HistoryTruncated
            | ErrorCode::WorkingTreeUnavailable
            | ErrorCode::RepositoryUnsupported => ExitCode::Error,
            ErrorCode::FallbackRequired => ExitCode::FeatureAbsent,
            ErrorCode::RevSpecRejected => ExitCode::Usage,
            ErrorCode::NetworkConsentRequired => ExitCode::NetworkConsentRequired,
            ErrorCode::FeatureAbsent => ExitCode::FeatureAbsent,
            ErrorCode::Cancelled => ExitCode::Cancelled,
            ErrorCode::BudgetExceeded => ExitCode::BudgetExceeded,
            ErrorCode::UnsupportedPlatform => ExitCode::UnsupportedPlatform,
            ErrorCode::CheckViolations => ExitCode::CheckViolations,
            ErrorCode::Usage => ExitCode::Usage,
            ErrorCode::NotImplemented | ErrorCode::Unknown => ExitCode::Error,
        }
    }
}

impl core::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What every crate's error type must answer, so `doctor` and the JSON writers never match on an
/// upstream enum they do not own (ADR-002 §7.1).
pub trait Diagnosable {
    /// The stable code carried alongside the message.
    fn code(&self) -> ErrorCode;

    /// A concrete next step for the human, or `None` when the message already is one. `doctor` prints
    /// this indented under the error; a hint is advice, never a second error.
    fn hint(&self) -> Option<&str> {
        None
    }
}

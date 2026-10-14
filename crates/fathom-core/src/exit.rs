//! The exit codes, as a published contract (ADR-002 §7.3).
//!
//! The table below is the one `e6.6`'s CI gate and `e1.10`'s verbs must agree on, and it is checked
//! against `tools/workspace_model.py::EXIT_CODES` by `tests/test_workspace_contract.py` — the same
//! rule that generated the table in the ADR. Inventing a code inside a verb is how two scripts end up
//! disagreeing about what `7` means, so: add it here (and in the model) or not at all.
//!
//! Nothing in this module calls the process exit. `fathom-cli` owns that call, because a library that
//! terminates the process cannot be tested by the process that imports it.

use core::fmt;

/// The process contract: every fathom verb resolves to exactly one of these.
///
/// `#[repr(i32)]` and [`TryFrom<i32>`] so a shell script and a Rust test can both name the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ExitCode {
    /// did the work, nothing to report
    Ok = 0,
    /// uncategorised failure; the human-readable cause is on stderr with a code
    Error = 1,
    /// clap rejected the arguments (bad flag, unknown verb, missing value)
    Usage = 2,
    /// `<root>` is not a git repository, or `.git` is unreadable
    NotARepository = 3,
    /// store failed its integrity check; `fathom doctor` names the rebuild
    StoreCorrupt = 4,
    /// `fathom.toml` failed the schema, or a value is the wrong type
    ConfigInvalid = 5,
    /// user interrupt (ctrl-c) or the writer lease was lost mid-run
    Cancelled = 6,
    /// `fathom check` found findings above the configured threshold (CI reads this)
    CheckViolations = 7,
    /// a compiled-in `net-*` feature was asked to run without consent
    NetworkConsentRequired = 8,
    /// the verb needs a feature that is not compiled in; the message names it
    FeatureAbsent = 9,
    /// platform capability missing (ADR-005 §10's ABI probe refused)
    UnsupportedPlatform = 10,
    /// a perf/RSS budget in `benchmarks/budgets.json` was blown
    BudgetExceeded = 11,
}

impl ExitCode {
    /// Every code, in numeric order. `fathom doctor` prints the table so a bug report can name the
    /// contract it hit.
    pub const ALL: [ExitCode; 12] = [
        ExitCode::Ok,
        ExitCode::Error,
        ExitCode::Usage,
        ExitCode::NotARepository,
        ExitCode::StoreCorrupt,
        ExitCode::ConfigInvalid,
        ExitCode::Cancelled,
        ExitCode::CheckViolations,
        ExitCode::NetworkConsentRequired,
        ExitCode::FeatureAbsent,
        ExitCode::UnsupportedPlatform,
        ExitCode::BudgetExceeded,
    ];

    /// The number a shell sees. `main` calls `std::process::exit` with this; no crate below the
    /// composition root may (ADR-002 §4.5).
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// The stable name, as it appears in `docs/adr/0002-workspace.md` §7.3 and in `--json` output.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ExitCode::Ok => "OK",
            ExitCode::Error => "ERROR",
            ExitCode::Usage => "USAGE",
            ExitCode::NotARepository => "NOT_A_REPOSITORY",
            ExitCode::StoreCorrupt => "STORE_CORRUPT",
            ExitCode::ConfigInvalid => "CONFIG_INVALID",
            ExitCode::Cancelled => "CANCELLED",
            ExitCode::CheckViolations => "CHECK_VIOLATIONS",
            ExitCode::NetworkConsentRequired => "NETWORK_CONSENT_REQUIRED",
            ExitCode::FeatureAbsent => "FEATURE_ABSENT",
            ExitCode::UnsupportedPlatform => "UNSUPPORTED_PLATFORM",
            ExitCode::BudgetExceeded => "BUDGET_EXCEEDED",
        }
    }

    /// Is this the "nothing went wrong" code? Written as a method rather than a `match` at every call
    /// site so the definition stays in one place.
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, ExitCode::Ok)
    }
}

/// The reverse of [`ExitCode::as_i32`]: a test that ran the binary can name what it saw. An unknown
/// number is an error rather than a panic, because the number arrived from outside the process.
impl TryFrom<i32> for ExitCode {
    type Error = i32;

    // Written as `Result<ExitCode, i32>` rather than `Result<Self, Self::Error>`: `ExitCode` has a
    // variant named `Error`, so `Self::Error` is ambiguous in exactly the way the compiler complains about.
    fn try_from(value: i32) -> Result<ExitCode, i32> {
        Ok(match value {
            0 => ExitCode::Ok,
            1 => ExitCode::Error,
            2 => ExitCode::Usage,
            3 => ExitCode::NotARepository,
            4 => ExitCode::StoreCorrupt,
            5 => ExitCode::ConfigInvalid,
            6 => ExitCode::Cancelled,
            7 => ExitCode::CheckViolations,
            8 => ExitCode::NetworkConsentRequired,
            9 => ExitCode::FeatureAbsent,
            10 => ExitCode::UnsupportedPlatform,
            11 => ExitCode::BudgetExceeded,
            other => return Err(other),
        })
    }
}

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name(), self.as_i32())
    }
}

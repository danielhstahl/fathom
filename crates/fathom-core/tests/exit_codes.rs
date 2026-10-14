//! The exit-code contract, as a test (ADR-002 §7.3, §13).
//!
//! ADR-002 §8.3 says CI branches on exit codes; this is the test that says the codes CI branches on are
//! the codes the ADR publishes. The table in the ADR and the table in `src/exit.rs` are both checked
//! against `tools/workspace_model.py` — `tests/test_workspace_contract.py` does the ADR side.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::error::ErrorCode;
use fathom_core::exit::ExitCode;

#[test]
fn every_code_is_named_uniquely() {
    let mut names: Vec<&str> = ExitCode::ALL.iter().copied().map(ExitCode::name).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "two exit codes share a name");
}

#[test]
fn codes_are_dense_and_start_at_zero() {
    let mut previous = -1;
    for code in ExitCode::ALL {
        let number = code.as_i32();
        assert_eq!(number, previous + 1, "{code} leaves a hole in the table");
        previous = number;
    }
    assert_eq!(previous, 11, "the published table has twelve rows");
}

#[test]
fn the_number_and_the_name_round_trip() {
    for code in ExitCode::ALL {
        assert_eq!(ExitCode::try_from(code.as_i32()), Ok(code));
    }
    assert_eq!(
        ExitCode::try_from(12),
        Err(12),
        "an unknown number is not silently OK"
    );
    assert_eq!(ExitCode::try_from(-1), Err(-1));
}

#[test]
fn the_codes_ci_branches_on_are_the_adr_ones() {
    // `e6.6`'s CI gate reads 7; the consent gate in ADR-002 §6.3 exits 8; `e6.6`'s headless binary
    // exits 9 when a verb was not compiled in. These three are the ones a script depends on.
    assert_eq!(ExitCode::CheckViolations.as_i32(), 7);
    assert_eq!(ExitCode::NetworkConsentRequired.as_i32(), 8);
    assert_eq!(ExitCode::FeatureAbsent.as_i32(), 9);
    assert!(!ExitCode::Error.is_success() && ExitCode::Ok.is_success());
}

#[test]
fn errors_reach_the_exit_through_one_mapping() {
    // ADR-002 §7.1: the error vocabulary and the process contract are joined in exactly one place.
    assert_eq!(ErrorCode::ConfigUnknownKey.exit(), ExitCode::ConfigInvalid);
    assert_eq!(ErrorCode::NotARepository.exit(), ExitCode::NotARepository);
    assert_eq!(
        ErrorCode::NetworkConsentRequired.exit(),
        ExitCode::NetworkConsentRequired
    );
    assert_eq!(ErrorCode::FeatureAbsent.exit(), ExitCode::FeatureAbsent);
    assert_eq!(ErrorCode::NotImplemented.exit(), ExitCode::Error);
}

#[test]
fn error_codes_are_stable_strings_not_debug_output() {
    assert_eq!(ErrorCode::NotARepository.as_str(), "NOT_A_REPOSITORY");
    assert_eq!(
        ErrorCode::SqliteCompileOptionMissing.as_str(),
        "SQLITE_COMPILE_OPTION_MISSING"
    );
    assert_eq!(ErrorCode::NotImplemented.to_string(), "NOT_IMPLEMENTED");
    let mut seen: Vec<&str> = vec![
        ErrorCode::SqliteCompileOptionMissing,
        ErrorCode::ConfigUnknownKey,
        ErrorCode::ConfigInvalid,
        ErrorCode::StoreCorrupt,
        ErrorCode::NotARepository,
        ErrorCode::NetworkConsentRequired,
        ErrorCode::FeatureAbsent,
        ErrorCode::NotImplemented,
        ErrorCode::Cancelled,
        ErrorCode::BudgetExceeded,
        ErrorCode::UnsupportedPlatform,
        ErrorCode::CheckViolations,
        ErrorCode::Usage,
        ErrorCode::Unknown,
    ]
    .iter()
    .copied()
    .map(ErrorCode::as_str)
    .collect();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(before, seen.len(), "two error codes print the same string");
}

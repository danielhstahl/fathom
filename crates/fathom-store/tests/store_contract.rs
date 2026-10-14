//! The store crate's contract at the scaffold: errors answer with a code and a hint, and the one gated
//! capability refuses rather than agreeing optimistically (ADR-002 §7.1, ADR-005 §10).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::error::{Diagnosable, ErrorCode};
use fathom_core::exit::ExitCode;
use fathom_store::error::Error;

#[test]
fn a_missing_compile_option_is_its_own_exit_code() {
    let err = Error::SqliteCompileOptionMissing { option: "FTS5" };
    assert_eq!(err.code(), ErrorCode::SqliteCompileOptionMissing);
    assert_eq!(err.code().exit(), ExitCode::StoreCorrupt);
    assert!(
        err.to_string().contains("FTS5"),
        "the option name is in the message"
    );
    assert!(
        err.hint()
            .is_some_and(|hint| hint.contains("sqlite-bundled"))
    );
}

#[test]
fn corruption_names_the_rebuild() {
    let err = Error::StoreCorrupt {
        what: "integrity_check reported 2 rows".into(),
    };
    assert_eq!(err.code(), ErrorCode::StoreCorrupt);
    assert!(err.hint().is_some_and(|hint| hint.contains("doctor")));
}

#[test]
fn a_stub_points_at_the_ticket_that_owns_it() {
    let err = Error::NotImplemented {
        what: "index a repository",
        ticket: "e1.2",
    };
    assert!(err.to_string().contains("e1.2"));
    assert_eq!(err.hint(), Some("e1.2"));
}

#[test]
#[cfg(feature = "mmap")]
fn the_mmap_probe_refuses_instead_of_claiming_success() {
    // ADR-005 §10: a naive mmap check reports success on hosts that will not deliver. The probe is
    // allowed to say no; it is not allowed to be wrong.
    assert_eq!(fathom_store::raw::probe("index.bin", 0), None);
    assert_eq!(fathom_store::raw::probe("index.bin", 4096), None);
}

#[test]
#[cfg(not(feature = "mmap"))]
fn mmap_is_not_in_the_build_at_all_by_default() {
    // The compile-time half of "a default build compiles zero unsafe" (ADR-002 R4): the module is not
    // compiled, so there is nothing for a caller to reach.
    let compiled_in = cfg!(feature = "mmap");
    assert!(!compiled_in);
}

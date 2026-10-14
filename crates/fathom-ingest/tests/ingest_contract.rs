//! The ingest boundary at the scaffold: `gix` links with the pinned local-only feature list, and the
//! one failure a script can already branch on is `NOT_A_REPOSITORY` (ADR-002 §4.2, §7.3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::error::Diagnosable;
use fathom_core::error::ErrorCode;
use fathom_core::exit::ExitCode;
use fathom_ingest::error::Error;

#[test]
fn gix_links_with_the_local_only_feature_list() {
    // ADR-002 §4.2: `gix` with `default-features = false` and a feature list pinned to the local-only
    // allow-list is what makes "no fetch, no push, no remote" a dependency fact. This test is the
    // cheap proof that the pinned list resolves and links; `deny.toml` and lint `N05` are the rest.
    let no_repository: Option<gix::Repository> = None;
    assert!(
        no_repository.is_none(),
        "gix must be linkable from the ingest crate at all"
    );
}

#[test]
fn not_a_repository_is_exit_code_three() {
    let err = Error::NotARepository {
        root: "/tmp/not-a-repo".into(),
    };
    assert_eq!(err.code(), ErrorCode::NotARepository);
    assert_eq!(err.code().exit(), ExitCode::NotARepository);
    assert_eq!(ExitCode::NotARepository.as_i32(), 3);
    assert!(err.to_string().contains("/tmp/not-a-repo"));
    assert!(
        err.hint()
            .is_some_and(|hint| hint.contains("never fetches"))
    );
}

#[test]
fn a_missing_ingest_is_a_typed_failure_not_a_panic() {
    // ADR-002 §9 rule 2: a stub is a typed failure, so a crate can be linked against a neighbour that
    // is not finished yet. `todo!()` in a library is lint `W6` for exactly this reason.
    let err = Error::NotImplemented {
        what: "segment a history into episodes",
        ticket: "e1.3",
    };
    assert_eq!(err.code().exit(), ExitCode::Error);
    assert_eq!(err.hint(), Some("e1.3"));
}

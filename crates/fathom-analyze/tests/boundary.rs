//! ADR-002 §5 (R7, R2): fixtures link from `tests/`, and the façade is the only door this crate's
//! consumers may use — including from a test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::insight;
use fathom_fixtures as _;

#[test]
fn fixtures_link_and_the_facade_is_named_not_bypassed() {
    assert_eq!(fathom_fixtures::OWNED_BY, "e0.10");
    assert!(
        !insight::status().views_ready,
        "the facade says so rather than pretending"
    );
}

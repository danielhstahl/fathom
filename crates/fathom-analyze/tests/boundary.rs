//! ADR-002 §5 (R7, R2): fixtures link from `tests/`, and the façade is the only door this crate's
//! consumers may use — including from a test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::insight;
use fathom_fixtures as _;

#[test]
fn fixtures_link_and_the_facade_is_named_not_bypassed() {
    assert_eq!(fathom_fixtures::OWNED_BY, "e0.10");
    // `reco-tt4` replaced the scaffolded refusal with the model, so the honest assertion inverted: a
    // façade that still reported itself unavailable would be lying the other way, and every surface that
    // asks (`fathom doctor`, the TUI's startup) would keep printing a refusal nobody can act on.
    let status = insight::status();
    assert!(status.views_ready, "the delta model is published");
    assert_eq!(
        status.contract, "fathom-delta/1",
        "the contract id is what a consumer pins; changing it is a version bump, not an edit"
    );
}

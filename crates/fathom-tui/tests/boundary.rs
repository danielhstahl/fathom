//! ADR-002 §5 (R7): the fixture crate is reachable from `tests/` and from nowhere in `src/`.
//! Lint `E05` fails the other direction; this is the direction that is allowed, and it is the shape
//! `e0.10`'s corpus will arrive in.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_fixtures as _;

#[test]
fn fixtures_link_from_tests_only() {
    // `fathom_fixtures` is a dev-dependency of this crate and appears in none of its `src/` files;
    // that is R7, and the `use` above is what a cargo-level cycle check would otherwise make awkward to
    // demonstrate at all.
    assert_eq!(
        fathom_fixtures::FIXED_AUTHOR.email,
        "fixture@fathom.invalid"
    );
}

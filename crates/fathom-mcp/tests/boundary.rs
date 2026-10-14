//! ADR-002 §5 (R7): the fixture crate is reachable from `tests/` and from nowhere in `src/` — which
//! matters most in this crate, because a server that manufactures its own expectations would certify
//! the agent's own claims about itself.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_fixtures as _;

#[test]
fn fixtures_link_from_tests_only() {
    assert_eq!(fathom_fixtures::COLLATION, "C");
}

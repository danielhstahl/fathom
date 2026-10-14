//! The fixture policy, checked on itself (ADR-002 §4.4).
//!
//! A fixture crate whose own values are not deterministic is a fixture crate with a flake in it, so
//! this crate is tested with the standard it holds everyone else to.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::time::Timestamp;
use fathom_fixtures::{COLLATION, FIXED_AUTHOR, FIXED_TIMESTAMP, OWNED_BY};

#[test]
fn the_fixed_instant_is_a_value_not_a_read() {
    assert_eq!(FIXED_TIMESTAMP, Timestamp::FIXED_FIXTURE);
    assert_eq!(FIXED_TIMESTAMP.to_string(), "2025-01-01T00:00:00Z");
    // Two "reads" of a fixed clock are the same instant, which is the whole claim.
    assert_eq!(
        FIXED_TIMESTAMP.as_unix_micros(),
        FIXED_TIMESTAMP.as_unix_micros()
    );
}

#[test]
fn the_author_is_nobody() {
    assert_eq!(FIXED_AUTHOR.name, "Fathom Fixture");
    assert!(
        FIXED_AUTHOR.email.ends_with("@fathom.invalid"),
        "RFC 2606: a fixture address must never become somebody's inbox"
    );
    let json = serde_json::to_string(&FIXED_AUTHOR).unwrap_or_default();
    assert_eq!(
        json, r#"{"name":"Fathom Fixture","email":"fixture@fathom.invalid"}"#,
        "a golden file is written against exactly these bytes"
    );
}

#[test]
fn the_ordering_rule_is_byte_order() {
    // Not the locale's order, and not case-folded: a golden list is a byte list.
    let mut paths = ["src/lib.rs", "src/Zeta.rs", "src/api.rs", "Cargo.toml"];
    fathom_fixtures::sort_in_corpus_order(&mut paths);
    assert_eq!(
        paths,
        ["Cargo.toml", "src/Zeta.rs", "src/api.rs", "src/lib.rs"],
        "{COLLATION} collation: upper-case bytes sort before lower-case ones"
    );
    assert_eq!(OWNED_BY, "e0.10");
}

//! `ADR-007 §4.7`'s worked example, verbatim, so the number the ADR argues for is a number CI
//! re-measures rather than a number the ADR hopes for.
//!
//! A reader with this file open counts: two `if`s, two short-circuits, three arms, plus one.

pub fn classify(v: i32, flag: bool) -> String {
    if v > 10 && flag {
        "a".to_string()
    } else if v > 5 || flag {
        "b".to_string()
    } else {
        match v {
            1 | 2 => "one-two",
            3 => "three",
            _ => "many",
        }
        .to_string()
    }
}

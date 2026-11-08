//! The shapes the ADR's §4.1 table was written for: the tested exits, the folded scopes, and the
//! three kinds of thing a reader mistakes for a branch and is not one.
//!
//! `mccabe-1` counts arms, tests, short-circuits and `?`. It does not count an `else`, an
//! unconditional exit, an `unsafe`, an `async`, an `.await`, or a `panic!`.

use std::fmt;

/// A signature with no body is a unit with no figure. The body is in another file, and `0` would be
/// a claim about code this file does not contain.
pub trait Named {
    fn named(&self) -> String;

    fn label(&self) -> String {
        "unnamed".to_string()
    }
}

impl fmt::Display for () {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "()")
    }
}

/// The two spellings ADR-007 §4.1 gives for a guard and a `let … else` name nodes this grammar does
/// not emit; the *rules* — a guarded arm is two tests, a failed `let` pattern is one — are what is
/// counted here, on `match_pattern`'s `condition` field and `let_declaration`'s `alternative`.
pub fn awkward(input: Option<&str>) -> Result<String, String> {
    let Some(text) = input else {
        return Err("nothing to do".to_string());
    };
    let n = text.len();
    let kind = match n {
        0 => "empty",
        1 | 2 => "tiny",
        k if k > 100 => "huge",
        _ => "mid",
    };
    for index in 0..n {
        while index > 3 {
            break;
        }
        loop {
            continue;
        }
    }
    let scaled = |value: usize| if value > 1 { value / 2 } else { value };
    let nested = {
        fn bump(value: usize) -> usize {
            if value == 0 { 1 } else { value + 1 }
        }
        bump(n)
    };
    let early = if n == 0 { return Ok(kind.to_string()) } else { n };
    let _ = unsafe { std::ptr::null::<u8>() };
    let _ = async {}.await;
    if n > 1000 {
        panic!("too big");
    }
    Ok(format!("{kind}{}{}{}", scaled(early), nested, text))
}

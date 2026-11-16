//! A file with no callable body at all: a constants module and a type.
//!
//! Its figure is `0` with the reason `no_bodies`, which is a measurement of this file and not a
//! refusal. The `if` in `LIMIT` is module-level code inside a container: no unit owns it (ADR-007
//! §3.1), so it is not counted anywhere, and that is the correct answer rather than a loss.

pub struct Bounds {
    pub low: i32,
    pub high: i32,
}

pub const LIMIT: i32 = if true { 10 } else { 0 };

pub const NAMES: [&str; 1] = ["bounds"];

//! A file whose worst property is that one of its units has no body here.
//!
//! The file's figure is a floor with a count beside it (`~n (1 unmeasured)`), never the clean number
//! a fully readable file gets, and never the missing unit counted as a zero.

pub trait Shape {
    /// No body in this file: `body_not_visible`, which is a `?` about this file and not a `0`.
    fn area(&self) -> f64;

    /// A default body is a body: one path, no forks, so `1`.
    fn name(&self) -> String {
        "shape".to_string()
    }
}

impl Shape for f64 {
    fn area(&self) -> f64 {
        if *self < 0.0 {
            -*self
        } else {
            *self
        }
    }
}

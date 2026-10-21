//! Shapes, a trait, and the visibility spellings that decide what a *file* exports.

use crate::geo::point::Point;
use std::fmt;

pub type Area = f64;

pub const SIDES: u32 = 4;

pub static ORIGIN_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The one contract every figure in the crate keeps.
pub trait Shape: fmt::Debug {
    fn name(&self) -> String;

    fn area(&self) -> Area {
        0.0
    }

    const KIND: &'static str;
}

#[derive(Debug, Clone)]
pub struct Rect {
    pub w: f64,
    pub(crate) h: f64,
}

impl Rect {
    pub fn new(w: f64, h: f64) -> Self {
        Rect { w, h }
    }

    fn diagonal(&self) -> f64 {
        (self.w * self.w + self.h * self.h).sqrt()
    }

    pub(crate) fn grow(&mut self, by: f64) {
        self.w += by;
    }
}

impl Shape for Rect {
    const KIND: &'static str = "rect";

    fn name(&self) -> String {
        format!("rect {}", self.diagonal())
    }
}

pub enum Kind {
    Round,
    Squared { side: f64 },
}

pub struct Circle {
    pub r: f64,
}

impl Shape for Circle {
    const KIND: &'static str = "circle";
    fn name(&self) -> String {
        "circle".to_string()
    }
    fn area(&self) -> Area {
        std::f64::consts::PI * self.r * self.r
    }
}

impl fmt::Display for Circle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Circle({})", self.r)
    }
}

pub fn describe<S>(shape: &S) -> String
where
    S: Shape,
{
    format!("{} area {}", shape.name(), shape.area())
}

pub async fn measure<S: Shape>(shapes: Vec<S>) -> Vec<Area> {
    shapes.iter().map(|s| s.area()).collect()
}

pub(crate) fn crate_internal_total(shapes: &[Rect]) -> Area {
    shapes.iter().map(|s| s.area()).sum()
}

#[doc(hidden)]
pub fn hidden_but_public(p: Point) -> Point {
    p
}

#[cfg(feature = "extra")]
pub fn extra_only() -> Area {
    1.0
}

#[cfg(not(feature = "extra"))]
pub fn extra_only() -> Area {
    0.0
}

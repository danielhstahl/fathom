use crate::geo::point::Point as P;
use super::Point as ImportedPoint;
use std::f64;

pub fn distance(a: &P, b: &ImportedPoint) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

pub fn manhattan_between(a: &super::point::Point, b: &P) -> f64 {
    a.manhattan() + b.manhattan()
}

pub(crate) fn far(a: &P) -> bool {
    distance(a, &P::ZERO) > 1_000.0
}

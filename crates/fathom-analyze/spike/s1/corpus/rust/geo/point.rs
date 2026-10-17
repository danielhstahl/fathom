use std::ops::Add;

pub type Coordinates = (f64, f64);

pub const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    pub fn manhattan(&self) -> f64 {
        self.x.abs() + self.y.abs()
    }

    pub fn scale(mut self, k: f64) -> Self {
        self.x *= k;
        self.y *= k;
        self
    }
}

impl Add for Point {
    type Output = Point;

    fn add(self, rhs: Point) -> Point {
        Point {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

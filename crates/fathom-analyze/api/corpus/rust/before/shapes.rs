//! A trait, a struct and the shapes a signature comes in.

pub type Area = f64;

/// The one contract every figure in the crate keeps.
pub trait Shape: std::fmt::Debug {
    fn name(&self) -> String;
    fn area(&self) -> Area;
}

#[derive(Debug)]
pub struct Rect {
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(w: f64, h: f64) -> Rect {
        Rect { w, h }
    }

    pub fn area(&self) -> Area {
        self.w * self.h
    }

    pub fn width(&self) -> f64 {
        self.w
    }

    pub(crate) fn grow(&mut self, by: f64) {
        self.w += by;
    }
}

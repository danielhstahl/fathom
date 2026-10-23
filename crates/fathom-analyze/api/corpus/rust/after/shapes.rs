//! A trait, a struct and the shapes a signature comes in — after the episode.

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
    pub label: String,
}

impl Rect {
    pub fn new(w: f64, h: f64) -> Rect {
        Rect {
            w,
            h,
            label: String::new(),
        }
    }

    pub fn area(&self) -> Option<Area> {
        if self.w > 0.0 {
            Some(self.w * self.h)
        } else {
            None
        }
    }

    pub fn width(&mut self) -> f64 {
        self.w
    }

    pub(crate) fn grow(&mut self, by: f64) {
        self.w += by;
    }
}

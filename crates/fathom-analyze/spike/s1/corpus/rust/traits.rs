//! The impl-block shapes: methods live on a type, not in a file, and one type's methods can be
//! spread over several files.

use crate::geo::point::Point;
use crate::shapes::{describe, Area, Shape};
use std::fmt::{self, Debug};

pub trait Render {
    type Target;

    fn render(&self) -> Self::Target;

    fn render_to_string(&self) -> String
    where
        Self::Target: fmt::Display,
    {
        self.render().to_string()
    }
}

pub trait Measured {
    fn measured(&self) -> Area;
}

impl<T: Shape> Measured for Vec<T> {
    fn measured(&self) -> Area {
        self.iter().map(|s| s.area()).sum()
    }
}

impl Measured for Point {
    fn measured(&self) -> Area {
        self.manhattan()
    }
}

pub struct Batch<G>
where
    G: Debug,
{
    pub items: Vec<G>,
}

impl<G: Shape + Debug> Batch<G> {
    pub fn push(&mut self, item: G) {
        self.items.push(item);
    }

    pub fn total(&self) -> Area {
        self.items.iter().map(|i| i.area()).sum()
    }

    pub fn report(&self) -> String {
        self.items.iter().map(|i| describe(i)).collect::<Vec<_>>().join("\n")
    }
}

impl<G: Shape + Debug> Render for Batch<G> {
    type Target = String;

    fn render(&self) -> String {
        self.report()
    }
}

pub async fn collect<T: Shape + Send>(items: Vec<T>) -> Vec<String> {
    items.into_iter().map(|i| i.name()).collect()
}

pub unsafe fn raw_len(ptr: *const u8, len: usize) -> usize {
    let _ = ptr;
    len
}

pub extern "C" fn c_entry(code: i32) -> i32 {
    code
}

pub fn dyn_call(shape: &dyn Shape) -> String {
    shape.name()
}

pub mod helpers {
    use super::*;

    pub fn area_of<S: Shape>(s: &S) -> crate::shapes::Area {
        s.area()
    }

    pub trait Sealed {
        fn seal(&self);
    }

    impl Sealed for super::Batch<std::time::Duration> {
        fn seal(&self) {}
    }
}

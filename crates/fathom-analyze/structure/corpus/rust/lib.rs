//! Crate root for the s1 fixture crate. Not a real crate: see ../README.md.

pub mod shapes;
pub mod geo;
pub mod cfg_split;
pub mod macros;
pub mod traits;
mod secret;

pub use crate::shapes::Shape;
pub use crate::secret::helper as help;

pub mod prelude {
    pub use crate::geo::point::Point;
    pub use super::geo::point::ORIGIN;
}

#[cfg(test)]
mod tests {
    use crate::geo::dist::distance;
    use crate::shapes::*;

    #[test]
    fn shape_is_a_shape() {
        let _ = distance(&crate::geo::point::Point::ZERO, &crate::geo::point::ORIGIN);
    }
}

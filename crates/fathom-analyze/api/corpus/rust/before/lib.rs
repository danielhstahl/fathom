//! The crate root. In Rust the module tree is what makes a `pub` item reachable, so the root is part
//! of the public surface and is diffed like everything else (`e3.7`, `reco-p5u`).

pub mod shapes;
pub mod geo;
mod secret;

pub use crate::secret::helper;

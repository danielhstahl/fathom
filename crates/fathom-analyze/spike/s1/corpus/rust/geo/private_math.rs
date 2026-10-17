/// Re-exported by `geo/mod.rs` even though this module is private: the item is reachable,
/// the module path is not. Which of the two an "export" means is the question.
pub fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

pub const TAU: f64 = 6.283_185_307_179_586;

fn internal(n: f64) -> f64 {
    n * 2.0
}

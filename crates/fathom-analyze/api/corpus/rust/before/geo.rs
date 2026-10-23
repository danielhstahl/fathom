//! Distance, drawing and the two spellings of "public constant".

pub fn distance(a: f64, b: f64) -> f64 {
    (a - b).abs()
}

pub fn label(text: &str) -> String {
    text.to_string()
}

pub fn gone() -> u32 {
    7
}

pub fn legacy() -> String {
    "legacy".to_string()
}

pub const SIDES: u32 = 4;

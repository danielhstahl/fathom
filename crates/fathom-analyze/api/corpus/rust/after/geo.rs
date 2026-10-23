//! Distance, drawing and the two spellings of "public constant" — after the episode.

pub fn distance(a: f64, b: f64, eps: f64) -> f64 {
    (a - b).abs().max(eps)
}

pub fn label(text: impl AsRef<str>) -> String {
    text.as_ref().to_string()
}

/// Kept for one release; the note is the author's sentence for whoever has to migrate.
#[deprecated(note = "use `label` instead")]
pub fn legacy() -> String {
    "legacy".to_string()
}

pub fn perimeter(w: f64, h: f64) -> f64 {
    2.0 * (w + h)
}

pub const SIDES: u32 = 5;

pub const NEW: u32 = 1;

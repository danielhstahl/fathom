//! The only place in this workspace where `unsafe` is permitted, and the only place it is gated.
//!
//! ADR-002 §7.4 draws the line in three parts: `unsafe` appears only under `src/raw/`, only under
//! `#[cfg(feature = "mmap")]`, and only with a `// SAFETY:` comment per block. Lints `P02`, `P03` and
//! `P04` in `tools/check_workspace.py` check all three on every PR, and clippy's
//! `undocumented_unsafe_blocks` is denied from `[workspace.lints]`. The claim that falls out is the one
//! worth having — **a default build of this workspace compiles zero `unsafe`** — and it is checked
//! rather than intended.

// The crate root's `#![deny(unsafe_code)]` is lifted for this module and its children, which is the shape
// ADR-002 §7.4 actually asks for: the deny covers every other file in the crate, every item below is
// behind `#[cfg(feature = "mmap")]` so none of it exists in a default build, and rules `P02`–`P04` in
// `tools/check_workspace.py` are what prove both claims on every PR rather than this comment. Clippy's
// `undocumented_unsafe_blocks`, denied from `[workspace.lints]`, still fires on every block here.
#![allow(unsafe_code)]

#[cfg(feature = "mmap")]
mod mmap;

/// Ask the host whether a mapping of `len` bytes of `path` can be honoured.
///
/// `Some(len)` means the host said yes; `None` means the mapping is refused and the caller must take the
/// read() path instead. ADR-005 §10 requires exactly this shape: a naive `mmap_size` claim reports
/// success on hosts that will not deliver it, so the probe — not the caller's optimism — decides.
///
/// The real mapping is `e1.4`'s (`reco-f28`). At the scaffold the gate, the probe and the safety
/// contract exist and the answer is "no", which is the safe direction for a stub to fail in.
#[cfg(feature = "mmap")]
#[must_use]
pub fn probe(path: &str, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    // SAFETY: `len > 0` is established above, `mmap::map` maps nothing and returns null until `e1.4`
    // implements it, and this function never dereferences what comes back — it only asks whether the
    // mapping was honoured, which is the question ADR-005 §10 says must be asked before it is trusted.
    if unsafe { mmap::map(path, len) }.is_null() {
        None
    } else {
        Some(len)
    }
}

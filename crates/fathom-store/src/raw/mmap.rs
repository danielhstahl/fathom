//! The workspace's one `unsafe`, behind one feature gate (ADR-002 §7.4, ADR-004 §13.1, ADR-005 §10).
//!
//! This file compiles only under `#[cfg(feature = "mmap")]`, so a default build of `fathom-store` —
//! and therefore of the shipped binary — contains no `unsafe` at all. That is the point of putting the
//! gate here rather than at the call site: the gate and the block live in the same file, so `P03` can
//! check them together.

#![cfg(feature = "mmap")]
#![allow(unsafe_code)]

/// Map `len` bytes of `path` for read-only index access.
///
/// Returns null when the host cannot honour the mapping, which the caller must treat as "use the read
/// path", never as "the file is empty".
///
/// # Safety
///
/// The caller must guarantee that `len > 0`, that the mapping is dropped before the underlying file
/// descriptor is closed, and that no byte past `len` is ever read through the returned pointer.
// SAFETY: the contract above is the caller's; `map` itself maps nothing at the scaffold and returns
// null, so no pointer it hands out can be dereferenced. `e1.4` (reco-f28) replaces the body with the
// real `mmap` call behind this same signature, and ADR-005 §10's probe runs before any caller trusts
// a mapping.
#[must_use]
pub unsafe fn map(path: &str, len: usize) -> *mut u8 {
    let _ = (path, len);
    core::ptr::null_mut()
}

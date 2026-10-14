//! `cancel.rs` — the handle a caller uses to stop a run that is taking too long.
//!
//! ADR-004 §7.6 makes cancellation a first-class outcome (exit code 8, not a failure) and §6.6 R8 asks
//! that it be observed *between units of analysis* rather than only between runs. This is the primitive
//! both mean: one `Arc<AtomicBool>`, cheap enough to test once per analysis unit, cloneable so a UI thread
//! can hold one end and the worker hold the other.
//!
//! It is deliberately not a channel, a future or a runtime. `fathom` is a CLI first (ADR-001 §7.4): the
//! indexing loop is `rayon` over commit shards, and the whole interaction a cancel needs is "has anybody
//! said stop", answered without a scheduler.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// A cancellation flag. Clone it to share it.
///
/// ```
/// use fathom_analyze::cancel::Cancel;
/// let cancel = Cancel::default();
/// assert!(!cancel.is_triggered());
/// cancel.trigger();
/// assert!(cancel.is_triggered());
/// ```
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// A flag nobody has pulled yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask to stop. Idempotent, and safe to call from any thread.
    pub fn trigger(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether to stop. `Relaxed` on purpose: there is nothing to synchronise with, only a flag to notice.
    #[must_use]
    pub fn is_triggered(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

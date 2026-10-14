//! The one clock read in the workspace (ADR-002 §4.4).
//!
//! Everything below `fathom-cli` receives timestamps as arguments: `fathom_core::time::Clock` is the seam,
//! and its documentation says the only two implementations live here (the real clock) and in
//! `fathom-fixtures` (a fixed one). This file is that sentence made greppable — `rg SystemTime` across the
//! workspace should land here and nowhere else, which is a cheaper guarantee than any lint that would have
//! to know which function is allowed to look.
//!
//! One read per command, at the edge, before any verb runs: a run that stamped each row with whatever the
//! clock happened to say would make `--stats` non-reproducible and every golden file time-dependent.

use std::time::{SystemTime, UNIX_EPOCH};

use fathom_core::time::{Clock, Timestamp};

/// The system clock, read once.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        // A clock before the epoch is not a fathom bug to interpret; `UNIX_EPOCH` is the answer, and the
        // run stamps itself with an instant that is obviously wrong rather than with a panic in a verb.
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp::from_unix_micros(i64::try_from(since.as_micros()).unwrap_or(i64::MAX))
    }
}

/// The instant as the store wants it: unix milliseconds.
///
/// The `*_utc` columns are milliseconds because that is the unit ADR-004 §4.1's reference implementation
/// writes, and `Timestamp` is microseconds because that is the finest unit git stores. The conversion is
/// here, at the edge, so no crate below has to know that the two disagree.
#[must_use]
pub fn now_millis() -> i64 {
    SystemClock.now().as_unix_micros() / 1_000
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn the_clock_is_after_the_epoch_and_monotonic_enough_to_order_two_reads() {
        let first = SystemClock.now();
        let second = SystemClock.now();
        assert!(
            first.as_unix_micros() > Timestamp::FIXED_FIXTURE.as_unix_micros() / 2,
            "the fixture corpus is dated {0:?}; the real clock must be near it, not before it",
            Timestamp::FIXED_FIXTURE
        );
        assert!(
            second.as_unix_micros() >= first.as_unix_micros(),
            "time went backwards"
        );
    }
}

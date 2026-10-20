//! Time as a value, never as a read (ADR-002 §4.1).
//!
//! `fathom-core` may not look at the clock. Analysis receives timestamps the way it receives file
//! contents — as arguments — and [`Clock`] is the seam through which `fathom-cli` (and
//! `fathom-fixtures`, for the corpus) supplies them. The alternative, a `SystemTime::now()` inside a
//! detector, silently makes every golden test time-dependent and every ADR-004 §5 cache key a lie.

use serde::{Deserialize, Serialize};

/// Microseconds before or after the Unix epoch, in UTC.
///
/// Microseconds rather than nanoseconds because every source fathom reads (git, SQLite, hook-event
/// JSON) stores at most microseconds, and a unit finer than the input is a unit nobody can verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

const MICROS_PER_SEC: i64 = 1_000_000;
const MICROS_PER_MIN: i64 = 60 * MICROS_PER_SEC;
const MICROS_PER_HOUR: i64 = 3_600 * MICROS_PER_SEC;

/// Microseconds in a UTC day.
///
/// Public because "the day this falls in" is asked in three crates — the rail groups by it, the search
/// filter's relative dates are counted in it, and the ledger stamps one per sign-off — and a constant
/// copy per crate is three chances for a day to be a different length.
pub const MICROS_PER_DAY: i64 = 24 * MICROS_PER_HOUR;

impl Timestamp {
    /// The epoch itself; a useful zero for fixtures and for "we do not know".
    pub const UNIX_EPOCH: Timestamp = Timestamp(0);

    /// The fixed instant `fathom-fixtures` timestamps its corpus with (ADR-002 §4.4: a fixture is
    /// byte-deterministic, so it may not read a clock either).
    pub const FIXED_FIXTURE: Timestamp = Timestamp(1_735_689_600_123_456);

    /// Wrap a microsecond count. Public and `const` so a test can name an instant as data.
    #[must_use]
    pub const fn from_unix_micros(micros: i64) -> Self {
        Timestamp(micros)
    }

    /// The underlying count.
    #[must_use]
    pub const fn as_unix_micros(self) -> i64 {
        self.0
    }

    /// Whole seconds since the epoch, rounding towards negative infinity (so ordering matches the
    /// calendar for pre-1970 histories, which real repositories have).
    #[must_use]
    pub const fn as_unix_seconds(self) -> i64 {
        self.0.div_euclid(MICROS_PER_SEC)
    }

    /// The microsecond part of the current second, always in `[0, 1_000_000)`.
    #[must_use]
    pub const fn subsec_micros(self) -> u32 {
        (self.0.rem_euclid(MICROS_PER_SEC)) as u32
    }

    /// The first instant of the UTC day this falls in, rounding towards negative infinity so a
    /// pre-1970 history gets the day before, not the day after.
    #[must_use]
    pub const fn day_start(self) -> Timestamp {
        Timestamp(self.0.div_euclid(MICROS_PER_DAY) * MICROS_PER_DAY)
    }

    /// This instant shifted by whole days, for the relative date forms (`7d`, `yesterday`).
    #[must_use]
    pub const fn shift_days(self, days: i64) -> Timestamp {
        Timestamp(self.0 + days * MICROS_PER_DAY)
    }

    /// `(year, month, day)` in UTC. Days-to-civil by the division-free-of-eras algorithm, so a golden
    /// file's date is a computation and not a lookup table.
    #[must_use]
    pub fn civil(&self) -> (i64, u32, u32) {
        let days = self.0.div_euclid(MICROS_PER_DAY);
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let day_of_era = z - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let mut year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
        let month = if shifted_month < 10 {
            shifted_month + 3
        } else {
            shifted_month - 9
        } as u32;
        if month <= 2 {
            year += 1;
        }
        (year, month, day)
    }

    /// `(hour, minute, second)` in UTC, each in range.
    #[must_use]
    pub fn time_of_day(&self) -> (u32, u32, u32) {
        let day = self.0.rem_euclid(MICROS_PER_DAY);
        (
            (day / MICROS_PER_HOUR) as u32,
            (day % MICROS_PER_HOUR / MICROS_PER_MIN) as u32,
            (day % MICROS_PER_MIN / MICROS_PER_SEC) as u32,
        )
    }
}

impl core::fmt::Display for Timestamp {
    /// `YYYY-MM-DDTHH:MM:SSZ`. Deliberately second-resolution: a display format that prints microsecond
    /// noise makes two runs look different when they are not, which is the opposite of why this crate
    /// forbids the clock.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (year, month, day) = self.civil();
        let (hour, minute, second) = self.time_of_day();
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
        )
    }
}

/// Days since the Unix epoch for a `(year, month, day)` — the inverse of [`Timestamp::civil`], by the
/// same division-free-of-eras algorithm so the two cannot disagree about a date.
///
/// The public half of what `fathom-tui`'s rail groups on and what a search filter's `since:` needs: one
/// calendar, one implementation of it.
#[must_use]
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * i64::from(shifted) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Where "now" comes from. The only implementations live in `fathom-cli` (the real clock, read once per
/// command) and in `fathom-fixtures` (a fixed clock, ADR-002 §4.4).
pub trait Clock {
    /// The current instant. Called at the process edge and passed down; a crate below `fathom-cli`
    /// taking a `Clock` and calling this mid-analysis is the drift this trait exists to make visible.
    fn now(&self) -> Timestamp;
}

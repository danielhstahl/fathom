//! Time as data: golden dates, no clock, and a serialised shape that is a number (ADR-001 §7.4).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::time::{Clock, Timestamp};

#[test]
fn display_is_utc_at_second_resolution() {
    let cases = [
        (0, "1970-01-01T00:00:00Z"),
        (1_000_000_000_000_000, "2001-09-09T01:46:40Z"),
        (-1_000_000_000_000_000, "1938-04-24T22:13:20Z"),
        (1_735_689_600_123_456, "2025-01-01T00:00:00Z"),
        (2_534_023_007_999_999, "2050-04-19T23:16:47Z"),
    ];
    for (micros, want) in cases {
        assert_eq!(Timestamp::from_unix_micros(micros).to_string(), want);
    }
}

#[test]
fn civil_and_time_of_day_agree_with_display() {
    let t = Timestamp::from_unix_micros(1_735_689_600_123_456);
    assert_eq!(t.civil(), (2025, 1, 1));
    assert_eq!(t.time_of_day(), (0, 0, 0));
    assert_eq!(t.subsec_micros(), 123_456);
    // A pre-epoch history: real repositories have those, and floor division is what keeps ordering
    // and the calendar telling the same story.
    let old = Timestamp::from_unix_micros(-1_000_000_000_000_000);
    assert_eq!(old.civil(), (1938, 4, 24));
    assert_eq!(old.time_of_day(), (22, 13, 20));
    assert!(old < Timestamp::UNIX_EPOCH && Timestamp::UNIX_EPOCH < t);
}

#[test]
fn a_timestamp_serialises_as_a_number() {
    // ADR-004 §5 versions the serialised shapes; a timestamp that is sometimes a number and sometimes
    // an object is a shape change waiting to happen.
    let json = serde_json::to_string(&Timestamp::FIXED_FIXTURE).unwrap_or_default();
    assert_eq!(json, "1735689600123456");
    let back: Timestamp = serde_json::from_str(&json).unwrap_or(Timestamp::UNIX_EPOCH);
    assert_eq!(back, Timestamp::FIXED_FIXTURE);
}

/// The smallest possible clock: the one `fathom-fixtures` uses for the corpus (ADR-002 §4.4).
struct Fixed(Timestamp);

impl Clock for Fixed {
    fn now(&self) -> Timestamp {
        self.0
    }
}

#[test]
fn a_clock_is_a_trait_not_a_read() {
    // Nothing here can tell the time; it can only be handed a time. That is the whole clause.
    let clock = Fixed(Timestamp::FIXED_FIXTURE);
    assert_eq!(clock.now(), Timestamp::FIXED_FIXTURE);
    assert_eq!(clock.now().to_string(), "2025-01-01T00:00:00Z");
}

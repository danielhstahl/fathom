//! `fathom_tui::timeline::days` — the calendar arithmetic the rail needs, and nothing else.
//!
//! `fathom-core` gives a `Timestamp` and its `(year, month, day)`; the rail additionally needs a
//! weekday name, a day boundary to group and range-filter on, and an *age* rendered three ways
//! (h:mm, d+hh, and date-only) because ADR-006 §5.2's narrow ladder is defined as "relative time
//! collapses to date-only". All of that is arithmetic on the value it is given: no clock is read,
//! because `fathom-tui` is IO-free and a fixture that reads a clock is a fixture with a flake in it
//! (ADR-002 §4.4).
//!
//! The weekday is UTC-derived from the day number, and the rail says so in its card header (`· UTC`)
//! rather than pretending to know the reviewer's zone: local time is `e1.6`'s host's to supply, and
//! a silently-wrong "today" is a worse bug than a labelled UTC one.

use fathom_core::time::Timestamp;

/// Microseconds in a day.
const MICROS_PER_DAY: i64 = 86_400_000_000;
/// Microseconds in an hour.
const MICROS_PER_HOUR: i64 = 3_600_000_000;
/// Microseconds in a minute.
const MICROS_PER_MIN: i64 = 60_000_000;

/// Weekday names, indexed by `days % 7` from the Unix epoch (1970-01-01 was a Thursday).
const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
/// Month names, indexed 1..=12.
const MONTHS: [&str; 13] = [
    "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Days since the Unix epoch for a `(year, month, day)` — the inverse of `Timestamp::civil`, by the
/// same division-free-of-eras algorithm so the two cannot disagree.
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

/// The first instant of the UTC day a timestamp falls in.
#[must_use]
pub fn day_start(at: Timestamp) -> Timestamp {
    let micros = at.as_unix_micros();
    Timestamp::from_unix_micros(micros.div_euclid(MICROS_PER_DAY) * MICROS_PER_DAY)
}

/// The first instant of the day after `at`.
#[must_use]
pub fn day_end(at: Timestamp) -> Timestamp {
    Timestamp::from_unix_micros(day_start(at).as_unix_micros() + MICROS_PER_DAY)
}

/// `Tue` — the rail's group header and the narrow row's weekday.
#[must_use]
pub fn weekday(at: Timestamp) -> &'static str {
    let days = at.as_unix_micros().div_euclid(MICROS_PER_DAY);
    WEEKDAYS[days.rem_euclid(7) as usize]
}

/// `Tue 14 Oct` — the group header's date half (ADR-006 §5.2's mock).
#[must_use]
pub fn day_label(at: Timestamp) -> String {
    let (year, month, day) = at.civil();
    let _ = year;
    format!(
        "{} {day} {}",
        weekday(at),
        MONTHS
            .get(usize::try_from(month).unwrap_or(0))
            .copied()
            .unwrap_or("?")
    )
}

/// `14 Oct` — the narrow row's "date-only" column (ADR-006 §5.2: relative time collapses to a date,
/// the day count is what the eye is actually after once an episode is more than a day old).
#[must_use]
pub fn date_only(at: Timestamp) -> String {
    let (_year, month, day) = at.civil();
    format!(
        "{day} {}",
        MONTHS
            .get(usize::try_from(month).unwrap_or(0))
            .copied()
            .unwrap_or("?")
    )
}

/// `2026-10-14` — the ISO form, used where a date is a value the reviewer may retype.
#[must_use]
pub fn iso(at: Timestamp) -> String {
    let (year, month, day) = at.civil();
    format!("{year:04}-{month:02}-{day:02}")
}

/// How long ago, in the rail's own dialect: `0:12` for under a day (h:mm), then `2d4h`, then
/// `3mo`. Age is never negative: a commit dated in the future (clock skew, a fixture dated ahead)
/// reads `0:00` rather than crashing or printing a minus sign in a column of ages.
#[must_use]
pub fn relative(now: Timestamp, at: Timestamp) -> String {
    let age = now
        .as_unix_micros()
        .saturating_sub(at.as_unix_micros())
        .max(0);
    if age < MICROS_PER_DAY {
        let hours = age / MICROS_PER_HOUR;
        let minutes = (age % MICROS_PER_HOUR) / MICROS_PER_MIN;
        return format!("{hours}:{minutes:02}");
    }
    let days = age / MICROS_PER_DAY;
    if days < 31 {
        let hours = (age % MICROS_PER_DAY) / MICROS_PER_HOUR;
        return format!("{days}d{hours}h");
    }
    let months = days / 30;
    if months < 24 {
        return format!("{months}mo");
    }
    format!("{}y", days / 365)
}

/// `4 commits · agent 0.94 · 0:12 ago`, the card's age clause on its own: `0:12 ago`, `2d4h ago`.
#[must_use]
pub fn relative_ago(now: Timestamp, at: Timestamp) -> String {
    format!("{} ago", relative(now, at))
}

/// Parse a date the reviewer can type in the filter editor: `2026-10-14`, `today`, `yesterday`,
/// `3d` (three days back), `week`, `2w`, `month`. `None` for anything unparseable, with the message
/// the editor prints back.
///
/// Day-count forms are relative to `now` rather than to the newest episode, because `today` has to
/// mean the day the reviewer is standing in — that is what `goto.today` means too (ADR-006 §7.4).
///
/// `Err` carries the sentence the editor prints back; `Ok(None)` is `any`, not a failure.
pub fn parse_date(spec: &str, now: Timestamp) -> Result<Option<Timestamp>, String> {
    let text = spec.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let lowered = text.to_ascii_lowercase();
    match lowered.as_str() {
        "any" | "all" | "none" | "*" => return Ok(None),
        "today" => return Ok(Some(day_start(now))),
        "yesterday" => {
            return Ok(Some(Timestamp::from_unix_micros(
                day_start(now).as_unix_micros() - MICROS_PER_DAY,
            )));
        }
        "week" | "1w" => return Ok(Some(shift_days(now, -7))),
        "2w" => return Ok(Some(shift_days(now, -14))),
        "month" | "1mo" => return Ok(Some(shift_days(now, -30))),
        "year" | "1y" => return Ok(Some(shift_days(now, -365))),
        _ => {}
    }
    if let Some(days) = lowered.strip_suffix('d')
        && let Ok(count) = days.parse::<i64>()
    {
        return Ok(Some(shift_days(now, -count.max(0))));
    }
    if let Some(weeks) = lowered.strip_suffix('w')
        && let Ok(count) = weeks.parse::<i64>()
    {
        return Ok(Some(shift_days(now, -count.max(0) * 7)));
    }
    let parts: Vec<&str> = lowered.split('-').collect();
    if parts.len() == 3 {
        let (Ok(year), Ok(month), Ok(day)) = (
            parts[0].parse::<i64>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        ) else {
            return Err(format!(
                "`{text}` is not a date — try 2026-10-14, today, yesterday or 7d"
            ));
        };
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(format!(
                "`{text}` is not a date — month is 1..12 and day is 1..31"
            ));
        }
        let days = days_from_civil(year, month, day);
        return Ok(Some(Timestamp::from_unix_micros(days * MICROS_PER_DAY)));
    }
    Err(format!(
        "`{text}` is not a date — try 2026-10-14, today, yesterday or 7d"
    ))
}

/// `now` shifted by whole days, for the relative date forms.
fn shift_days(now: Timestamp, days: i64) -> Timestamp {
    Timestamp::from_unix_micros(day_start(now).as_unix_micros() + days * MICROS_PER_DAY)
}

/// The last day an exclusive `until` bound includes, spelled back the way the reviewer typed it.
/// The store's `until_utc` is exclusive; a reviewer who typed `14 Oct` means "through the 14th", so
/// the bound is stored as the following day and *shown* as the day they named.
#[must_use]
pub fn last_included(until: Timestamp) -> Timestamp {
    Timestamp::from_unix_micros(until.as_unix_micros() - MICROS_PER_DAY)
}

/// The day a `since`/`until` bound prints as, or `any`.
#[must_use]
pub fn date_or_any(at: Option<Timestamp>) -> String {
    at.map_or_else(|| "any".to_owned(), iso)
}

/// An exclusive upper bound, printed as the day it includes.
#[must_use]
pub fn last_included_display(until: Timestamp) -> String {
    iso(last_included(until))
}

/// Is `at` inside an inclusive-`since`, exclusive-`until` window? Mirrors `EpisodeQuery`'s bound rule
/// in the store so the same filter means the same rows in both places (ADR-001 §7.10).
#[must_use]
pub fn in_range(at: Timestamp, since: Option<Timestamp>, until: Option<Timestamp>) -> bool {
    since.is_none_or(|bound| at >= bound) && until.is_none_or(|bound| at < bound)
}

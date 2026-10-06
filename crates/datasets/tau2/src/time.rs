//! τ²-bench's message times: ISO 8601 local times written by Python's
//! `datetime.isoformat()` (`2025-06-04T12:22:38.915138`), read as UTC. The
//! fraction may be absent or of any length (digits past the sixth are
//! dropped); a `Z` or `+00:00` suffix is accepted, any other offset is not.
//!
//! The result is then checked as crosstalk-spec's `Timestamp::parse_rfc3339`
//! checks `YYYY-MM-DDTHH:MM:SS.ffffffZ` (fields in range, no leap second,
//! not before the epoch), with the same civil-date arithmetic (Howard
//! Hinnant's `days_from_civil`), so a time is accepted exactly when
//! crosstalk-eval accepts it and maps to the same microsecond.

use a2a_bench_format::time::Timestamp;

const MICROS_PER_SECOND: u64 = 1_000_000;
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{text:?} is not an ISO 8601 UTC time")]
pub struct TimeError {
    pub text: String,
}

/// The time `text` names.
pub fn parse_time(text: &str) -> Result<Timestamp, TimeError> {
    let error = || TimeError {
        text: text.to_owned(),
    };
    let body = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix("+00:00"))
        .unwrap_or(text);
    let (seconds, fraction) = match body.split_once('.') {
        Some((seconds, fraction)) => (seconds, fraction),
        None => (body, ""),
    };
    if seconds.len() != 19 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error());
    }
    let micros: String = fraction.chars().chain("000000".chars()).take(6).collect();
    parse_rfc3339(&format!("{seconds}.{micros}Z")).ok_or_else(error)
}

/// crosstalk-spec's `Timestamp::parse_rfc3339`: exactly
/// `YYYY-MM-DDTHH:MM:SS.ffffffZ`, fields in range, on or after the epoch.
fn parse_rfc3339(text: &str) -> Option<Timestamp> {
    let bytes = text.as_bytes();
    if bytes.len() != 27 {
        return None;
    }
    let separators = [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'.'),
        (26, b'Z'),
    ];
    if separators
        .iter()
        .any(|&(at, byte)| bytes.get(at) != Some(&byte))
    {
        return None;
    }
    let number = |from: usize, to: usize| -> Option<u64> {
        bytes.get(from..to)?.iter().try_fold(0u64, |value, &byte| {
            byte.is_ascii_digit()
                .then(|| value * 10 + u64::from(byte - b'0'))
        })
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    let fraction = number(20, 26)?;
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
        || year < 1970
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let seconds = days * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second;
    Some(Timestamp::from_micros(
        seconds * MICROS_PER_SECOND + fraction,
    ))
}

fn is_leap(year: u64) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn days_in_month(year: u64, month: u64) -> u64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a date on or after it (Hinnant's
/// `days_from_civil`, with the year starting in March so the leap day is
/// last).
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    // 719_468 days from 0000-03-01 to 1970-01-01.
    era * 146_097 + day_of_era - 719_468
}

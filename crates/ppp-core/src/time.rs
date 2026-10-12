//! Time helpers built on jiff. Nushell hands us chrono values; this module
//! is where they become jiff values.
//!
//! Output drops fractional seconds: calendar sources work in whole seconds,
//! and a fixed width keeps the JSON stable.

use chrono::{DateTime, FixedOffset};
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Converts a nushell datetime to a jiff timestamp. Fails only for dates
/// outside jiff's range (years -9999 to 9999).
pub fn timestamp(dt: &DateTime<FixedOffset>) -> Result<Timestamp, jiff::Error> {
    Timestamp::new(dt.timestamp(), dt.timestamp_subsec_nanos() as i32)
}

/// `2026-10-15T00:30:00Z`
pub fn format_instant(ts: Timestamp) -> String {
    ts.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// `2026-10-14T19:30:00`: the wall-clock time in `tz`, with no offset.
pub fn format_local(ts: Timestamp, tz: &TimeZone) -> String {
    ts.to_zoned(tz.clone())
        .strftime("%Y-%m-%dT%H:%M:%S")
        .to_string()
}

/// `2026-10-14`: the calendar date in `tz`.
pub fn format_date(ts: Timestamp, tz: &TimeZone) -> String {
    ts.to_zoned(tz.clone()).strftime("%Y-%m-%d").to_string()
}

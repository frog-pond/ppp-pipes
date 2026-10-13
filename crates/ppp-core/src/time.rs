//! Time helpers built on jiff. Nushell hands us chrono values; this module
//! is where they become jiff values.
//!
//! Output drops fractional seconds: calendar sources work in whole seconds,
//! and a fixed width keeps the JSON stable.

use chrono::{DateTime, FixedOffset};
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// The instants jiff can hold, for error messages: `Timestamp::MIN` to
/// `Timestamp::MAX`.
pub const RANGE: &str = "-9999-01-02T01:59:59Z to 9999-12-30T22:00:00Z";

/// Converts a nushell datetime to a jiff timestamp. Fails only for dates
/// outside jiff's range (`RANGE`).
pub fn timestamp(dt: &DateTime<FixedOffset>) -> Result<Timestamp, jiff::Error> {
    Timestamp::new(dt.timestamp(), dt.timestamp_subsec_nanos() as i32)
}

/// A fixed zone at the datetime's own offset, for writing dates that came
/// with no time zone (ICS `VALUE=DATE`).
pub fn own_zone(dt: &DateTime<FixedOffset>) -> TimeZone {
    let offset = jiff::tz::Offset::from_seconds(dt.offset().local_minus_utc())
        .expect("chrono offsets are under a day, inside jiff's range");
    TimeZone::fixed(offset)
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

/// Converts a jiff zoned time to a nushell datetime with the same instant
/// and the same UTC offset.
pub fn to_nu(zoned: &jiff::Zoned) -> DateTime<FixedOffset> {
    let ts = zoned.timestamp();
    let offset = FixedOffset::east_opt(zoned.offset().seconds())
        .expect("jiff offsets are under a day, inside chrono's range");
    DateTime::from_timestamp(ts.as_second(), ts.subsec_nanosecond() as u32)
        .expect("jiff timestamps are inside chrono's range")
        .with_timezone(&offset)
}

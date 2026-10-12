//! Recurrence expansion (RFC 5545 RRULE, RDATE and EXDATE).
//!
//! This is the only module that touches the `rrule` crate, which works in
//! chrono and chrono-tz; everything in and out is jiff.

use chrono::TimeZone as _;
use jiff::{SignedDuration, Timestamp, Zoned};
use rrule::{RRule, RRuleSet, Tz, Unvalidated};

/// The most occurrences one series may produce inside a window.
pub const MAX_OCCURRENCES: u16 = 10_000;

/// A recurring event's rule, as read from its master VEVENT.
pub struct Series<'a> {
    /// DTSTART, in the zone the rule repeats in.
    pub start: &'a Zoned,
    pub rrule: Option<&'a str>,
    pub rdates: &'a [Zoned],
    pub exdates: &'a [Zoned],
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecurError {
    #[error("{0}")]
    Rule(String),
    #[error("time zone \"{0}\" is not an IANA zone name")]
    Zone(String),
    #[error("the rule has more than {MAX_OCCURRENCES} occurrences in the window")]
    TooMany,
}

/// The start of every occurrence of `series` that overlaps [from, until),
/// in the series' zone, earliest first. Each occurrence lasts `length`; one
/// with no length is a moment and must start inside the window. DTSTART
/// always counts as an occurrence (RFC 5545 §3.8.5.3).
pub fn occurrences(
    series: &Series,
    length: SignedDuration,
    from: Timestamp,
    until: Timestamp,
) -> Result<Vec<Zoned>, RecurError> {
    let zone = series.start.time_zone().clone();
    let name = zone
        .iana_name()
        .ok_or_else(|| RecurError::Zone(format!("{zone:?}")))?;
    let tz = Tz::Tz(
        name.parse::<chrono_tz::Tz>()
            .map_err(|_| RecurError::Zone(name.to_string()))?,
    );
    let to_chrono = |ts: Timestamp| {
        tz.timestamp_opt(ts.as_second(), ts.subsec_nanosecond() as u32)
            .single()
            .expect("an instant has exactly one local time")
    };

    let dt_start = to_chrono(series.start.timestamp());
    let set = match series.rrule {
        Some(rule) => rule
            .parse::<RRule<Unvalidated>>()
            .and_then(|rule| rule.build(dt_start))
            .map_err(|e| RecurError::Rule(e.to_string()))?,
        None => RRuleSet::new(dt_start),
    };
    let mut rdates: Vec<_> = series
        .rdates
        .iter()
        .map(|z| to_chrono(z.timestamp()))
        .collect();
    rdates.push(dt_start);
    let exdates = series
        .exdates
        .iter()
        .map(|z| to_chrono(z.timestamp()))
        .collect();

    // `after` and `before` are inclusive; the exact window is applied below.
    let earliest = from.checked_sub(length).unwrap_or(from);
    let result = set
        .set_rdates(rdates)
        .set_exdates(exdates)
        .after(to_chrono(earliest))
        .before(to_chrono(until))
        .all(MAX_OCCURRENCES);
    if result.limited {
        return Err(RecurError::TooMany);
    }

    let mut starts: Vec<Zoned> = result
        .dates
        .iter()
        .map(|dt| {
            Timestamp::new(dt.timestamp(), dt.timestamp_subsec_nanos() as i32)
                .expect("chrono times from jiff stay in jiff's range")
                .to_zoned(zone.clone())
        })
        .filter(|start| {
            let begin = start.timestamp();
            let overlaps = if length.is_zero() {
                begin >= from
            } else {
                begin + length > from
            };
            overlaps && begin < until
        })
        .collect();
    starts.dedup();
    Ok(starts)
}

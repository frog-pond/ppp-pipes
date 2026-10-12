//! Recurrence expansion: wall-clock time across DST, RDATE, EXDATE, the
//! overlap window, and the limits.

use jiff::{SignedDuration, Timestamp, Zoned};
use ppp_core::recur::{Occurrence, Rdate, RecurError, Series, occurrences};

fn zoned(text: &str) -> Zoned {
    text.parse().unwrap()
}

fn instant(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn starts(series: &Series, length: SignedDuration, from: &str, until: &str) -> Vec<String> {
    occurrences(series, length, instant(from), instant(until))
        .unwrap()
        .iter()
        .map(|o| o.start.to_string())
        .collect()
}

fn rdate(start: &str) -> Rdate {
    Rdate {
        start: zoned(start),
        end: None,
    }
}

const TWO_HOURS: SignedDuration = SignedDuration::from_hours(2);

#[test]
fn weekly_keeps_its_wall_clock_time_across_dst() {
    let start = zoned("2026-10-27T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=WEEKLY;BYDAY=TU"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-10-26T00:00:00Z",
            "2026-11-12T00:00:00Z"
        ),
        [
            "2026-10-27T18:00:00-05:00[America/Chicago]",
            "2026-11-03T18:00:00-06:00[America/Chicago]",
            "2026-11-10T18:00:00-06:00[America/Chicago]",
        ]
    );
}

#[test]
fn a_time_in_the_spring_forward_gap_moves_later() {
    let start = zoned("2026-03-07T02:30:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY;COUNT=3"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-03-01T00:00:00Z",
            "2026-03-31T00:00:00Z"
        ),
        [
            "2026-03-07T02:30:00-06:00[America/Chicago]",
            "2026-03-08T03:30:00-05:00[America/Chicago]",
            "2026-03-09T02:30:00-05:00[America/Chicago]",
        ]
    );
}

#[test]
fn exdates_remove_and_rdates_add() {
    let start = zoned("2026-11-03T18:00:00[America/Chicago]");
    let exdates = [zoned("2026-11-10T18:00:00[America/Chicago]")];
    let rdates = [rdate("2026-11-12T12:00:00[America/Chicago]")];
    let series = Series {
        start: &start,
        rrule: Some("FREQ=WEEKLY;COUNT=3"),
        rdates: &rdates,
        exdates: &exdates,
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        ),
        [
            "2026-11-03T18:00:00-06:00[America/Chicago]",
            "2026-11-12T12:00:00-06:00[America/Chicago]",
            "2026-11-17T18:00:00-06:00[America/Chicago]",
        ]
    );
}

#[test]
fn rdates_alone_still_count_dtstart() {
    let start = zoned("2026-11-03T18:00:00[America/Chicago]");
    let rdates = [rdate("2026-11-05T18:00:00[America/Chicago]")];
    let series = Series {
        start: &start,
        rrule: None,
        rdates: &rdates,
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        )
        .len(),
        2
    );
}

#[test]
fn dtstart_counts_even_when_the_rule_skips_it() {
    // A Monday DTSTART with a Tuesday rule: RFC 5545 counts the Monday as
    // the first of the two.
    let start = zoned("2026-11-02T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=WEEKLY;BYDAY=TU;COUNT=2"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        ),
        [
            "2026-11-02T18:00:00-06:00[America/Chicago]",
            "2026-11-03T18:00:00-06:00[America/Chicago]",
        ]
    );
}

#[test]
fn an_occurrence_under_way_when_the_window_opens_is_kept() {
    let start = zoned("2026-11-03T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY"),
        rdates: &[],
        exdates: &[],
    };
    // 19:00 Chicago on Nov 4 is 01:00Z on Nov 5; the 18:00 occurrence runs until 20:00.
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-05T01:00:00Z",
            "2026-11-05T02:00:00Z"
        ),
        ["2026-11-04T18:00:00-06:00[America/Chicago]"]
    );
    // One that ends exactly as the window opens is out.
    assert!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-05T02:00:00Z",
            "2026-11-05T03:00:00Z"
        )
        .is_empty()
    );
}

#[test]
fn moments_must_start_inside_the_window() {
    let start = zoned("2026-11-03T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            SignedDuration::ZERO,
            "2026-11-04T00:00:00Z",
            "2026-11-05T00:00:00Z"
        ),
        ["2026-11-03T18:00:00-06:00[America/Chicago]"]
    );
    assert_eq!(
        starts(
            &series,
            SignedDuration::ZERO,
            "2026-11-05T00:00:00Z",
            "2026-11-06T00:00:00Z"
        ),
        ["2026-11-04T18:00:00-06:00[America/Chicago]"]
    );
}

#[test]
fn all_day_dates_in_utc() {
    let start = zoned("2026-10-17T00:00:00[UTC]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=YEARLY;COUNT=3"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            SignedDuration::from_hours(24),
            "2027-01-01T00:00:00Z",
            "2030-01-01T00:00:00Z"
        ),
        [
            "2027-10-17T00:00:00+00:00[UTC]",
            "2028-10-17T00:00:00+00:00[UTC]"
        ]
    );
}

#[test]
fn a_long_running_series_is_not_capped_by_its_past() {
    let start = zoned("1990-01-01T09:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-10-12T05:00:00Z",
            "2026-10-19T05:00:00Z"
        )
        .len(),
        7
    );
}

#[test]
fn too_many_occurrences_is_an_error() {
    let start = zoned("2026-10-27T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=MINUTELY"),
        rdates: &[],
        exdates: &[],
    };
    let err = occurrences(
        &series,
        TWO_HOURS,
        instant("2026-10-27T00:00:00Z"),
        instant("2026-12-01T00:00:00Z"),
    )
    .unwrap_err();
    assert_eq!(err, RecurError::TooMany);
}

#[test]
fn bad_rules_are_errors_that_name_the_rule() {
    let start = zoned("2026-10-27T18:00:00[America/Chicago]");
    for (rule, message) in [
        (
            "FREQ=SOMETIMES",
            "RRULE \"FREQ=SOMETIMES\" is not valid: `SOMETIMES` is not a valid frequency.",
        ),
        (
            "FREQ=DAILY;UNTIL=someday",
            "RRULE \"FREQ=DAILY;UNTIL=someday\" is not valid: UNTIL value \"someday\" is not a date or date-time",
        ),
        (
            "FREQ=DAILY;BYSETPOS=0",
            "RRULE \"FREQ=DAILY;BYSETPOS=0\" is not valid: `BYSETPOS` can't be `0`, must be larger or smaller then `0`.",
        ),
        (
            "FREQ=DAILY;UNTIL=20261201T000000;BYHOUR=99",
            "RRULE \"FREQ=DAILY;UNTIL=20261201T000000;BYHOUR=99\" is not valid: `99` is not a valid BYHOUR value. Expected a comma-separated list of values in range 0..=23, e.g. `1,3,4`",
        ),
    ] {
        let series = Series {
            start: &start,
            rrule: Some(rule),
            rdates: &[],
            exdates: &[],
        };
        let err = occurrences(
            &series,
            TWO_HOURS,
            instant("2026-10-27T00:00:00Z"),
            instant("2026-12-01T00:00:00Z"),
        )
        .unwrap_err();
        assert!(matches!(err, RecurError::Rule { .. }), "{rule}: {err:?}");
        assert_eq!(err.to_string(), message);
    }
}

#[test]
fn a_date_until_includes_its_whole_day() {
    // RFC 5545 requires a DATE UNTIL when DTSTART is a DATE.
    let start = zoned("2026-11-05T00:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=WEEKLY;UNTIL=20261126"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            SignedDuration::from_hours(24),
            "2026-11-01T00:00:00Z",
            "2027-01-01T00:00:00Z"
        ),
        [
            "2026-11-05T00:00:00-06:00[America/Chicago]",
            "2026-11-12T00:00:00-06:00[America/Chicago]",
            "2026-11-19T00:00:00-06:00[America/Chicago]",
            "2026-11-26T00:00:00-06:00[America/Chicago]",
        ]
    );
}

#[test]
fn a_floating_until_is_read_in_the_series_zone() {
    // RFC 5545 requires a floating UNTIL when DTSTART is floating; ours are
    // zoned through the calendar's zone, so UNTIL is read in that zone.
    let start = zoned("2026-10-27T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY;UNTIL=20261030T180000"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-10-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        )
        .len(),
        4
    );
}

#[test]
fn an_until_before_dtstart_leaves_just_dtstart() {
    let start = zoned("2026-10-27T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY;UNTIL=20261020T000000Z"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-10-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        ),
        ["2026-10-27T18:00:00-05:00[America/Chicago]"]
    );
}

#[test]
fn a_period_rdate_keeps_its_own_end() {
    let start = zoned("2026-11-03T18:00:00[America/Chicago]");
    let rdates = [Rdate {
        start: zoned("2026-11-05T09:00:00[America/Chicago]"),
        end: Some(zoned("2026-11-05T17:00:00[America/Chicago]")),
    }];
    let series = Series {
        start: &start,
        rrule: None,
        rdates: &rdates,
        exdates: &[],
    };
    // The window opens after the series' usual two hours would have ended,
    // but inside the period.
    let found = occurrences(
        &series,
        TWO_HOURS,
        instant("2026-11-05T21:00:00Z"),
        instant("2026-11-06T00:00:00Z"),
    )
    .unwrap();
    assert_eq!(
        found,
        [Occurrence {
            start: zoned("2026-11-05T09:00:00[America/Chicago]"),
            period_end: Some(zoned("2026-11-05T17:00:00[America/Chicago]")),
        }]
    );
}

#[test]
fn an_off_rule_dtstart_with_count_1_is_the_only_occurrence() {
    let start = zoned("2026-10-12T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=WEEKLY;BYDAY=TU;COUNT=1"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-10-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        ),
        ["2026-10-12T18:00:00-05:00[America/Chicago]"]
    );
}

#[test]
fn an_on_rule_dtstart_is_counted_once() {
    let start = zoned("2026-10-13T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=WEEKLY;BYDAY=TU;COUNT=2"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-10-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        ),
        [
            "2026-10-13T18:00:00-05:00[America/Chicago]",
            "2026-10-20T18:00:00-05:00[America/Chicago]",
        ]
    );
}

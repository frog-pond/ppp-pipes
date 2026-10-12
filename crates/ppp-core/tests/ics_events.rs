//! Reading VEVENTs: zones, dates, durations, recurrence fields, organizers,
//! and the saved live feeds.

use std::fs;
use std::path::Path;

use ppp_core::ics::events::{Event, IcsError, Organizer, parse_events};

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/ics")
            .join(name),
    )
    .unwrap()
}

/// A calendar wrapping the given VEVENT lines, with the given calendar-level lines.
fn calendar(header: &[&str], event: &[&str]) -> String {
    let mut lines = vec!["BEGIN:VCALENDAR"];
    lines.extend_from_slice(header);
    lines.push("BEGIN:VEVENT");
    lines.push("UID:x@example.edu");
    lines.extend_from_slice(event);
    lines.extend(["END:VEVENT", "END:VCALENDAR"]);
    lines.join("\r\n")
}

fn one(text: &str, tz: Option<&str>) -> Event {
    let mut events = parse_events(text, tz).unwrap();
    assert_eq!(events.len(), 1);
    events.remove(0)
}

fn show(z: &Option<jiff::Zoned>) -> String {
    z.as_ref().unwrap().to_string()
}

#[test]
fn tzid_times_are_zoned() {
    let e = one(
        &calendar(&[], &["DTSTART;TZID=America/Chicago:20261014T193000"]),
        None,
    );
    assert_eq!(show(&e.start), "2026-10-14T19:30:00-05:00[America/Chicago]");
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
    assert!(!e.all_day);
}

#[test]
fn floating_times_use_the_calendar_zone() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &["DTSTART:20261013T120000"],
        ),
        None,
    );
    assert_eq!(show(&e.start), "2026-10-13T12:00:00-05:00[America/Chicago]");
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
}

#[test]
fn timezone_id_and_a_single_vtimezone_also_name_the_calendar_zone() {
    let by_id = one(
        &calendar(
            &["TIMEZONE-ID:America/Denver"],
            &["DTSTART:20261013T120000"],
        ),
        None,
    );
    assert_eq!(by_id.tz.as_deref(), Some("America/Denver"));
    let by_vtimezone = one(
        &calendar(
            &["BEGIN:VTIMEZONE", "TZID:America/New_York", "END:VTIMEZONE"],
            &["DTSTART:20261013T120000"],
        ),
        None,
    );
    assert_eq!(by_vtimezone.tz.as_deref(), Some("America/New_York"));
}

#[test]
fn the_calendar_zone_wins_over_the_tz_flag() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &["DTSTART:20261013T120000"],
        ),
        Some("Europe/London"),
    );
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
}

#[test]
fn the_tz_flag_fills_in_when_the_calendar_has_no_zone() {
    let e = one(
        &calendar(&[], &["DTSTART:20261013T120000"]),
        Some("Europe/London"),
    );
    assert_eq!(show(&e.start), "2026-10-13T12:00:00+01:00[Europe/London]");
}

#[test]
fn floating_time_with_no_zone_anywhere_is_an_error() {
    let err = parse_events(&calendar(&[], &["DTSTART:20261013T120000"]), None).unwrap_err();
    assert_eq!(
        err.to_string(),
        "event x@example.edu: DTSTART has a floating time and the calendar has no time zone; pass --tz"
    );
}

#[test]
fn utc_times_keep_the_calendar_zone_for_display() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &["DTSTART:20261022T160000Z"],
        ),
        None,
    );
    assert_eq!(show(&e.start), "2026-10-22T16:00:00+00:00[UTC]");
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
}

#[test]
fn all_day_dates_without_any_zone_are_midnight_utc() {
    let e = one(
        &calendar(
            &[],
            &["DTSTART;VALUE=DATE:20261017", "DTEND;VALUE=DATE:20261019"],
        ),
        None,
    );
    assert!(e.all_day);
    assert_eq!(show(&e.start), "2026-10-17T00:00:00+00:00[UTC]");
    assert_eq!(show(&e.end), "2026-10-19T00:00:00+00:00[UTC]");
    assert_eq!(e.tz, None);
}

#[test]
fn all_day_dates_with_a_calendar_zone_are_local_midnight() {
    let e = one(
        &calendar(&["X-WR-TIMEZONE:America/Chicago"], &["DTSTART:20261017"]),
        None,
    );
    assert!(e.all_day);
    assert_eq!(show(&e.start), "2026-10-17T00:00:00-05:00[America/Chicago]");
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
}

#[test]
fn duration_sets_the_end() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &["DTSTART:20260908T180000", "DURATION:PT2H"],
        ),
        None,
    );
    assert_eq!(show(&e.end), "2026-09-08T20:00:00-05:00[America/Chicago]");
}

#[test]
fn spring_forward_gap_moves_later() {
    // 02:30 does not exist in Chicago on 2026-03-08; RFC 5545 reads it as 03:30.
    let e = one(
        &calendar(&[], &["DTSTART;TZID=America/Chicago:20260308T023000"]),
        None,
    );
    assert_eq!(show(&e.start), "2026-03-08T03:30:00-05:00[America/Chicago]");
}

#[test]
fn unknown_tzid_is_an_error() {
    let err = parse_events(
        &calendar(&[], &["DTSTART;TZID=Central Standard Time:20261013T120000"]),
        None,
    )
    .unwrap_err();
    assert_eq!(
        err,
        IcsError::UnknownZone {
            uid: "x@example.edu".into(),
            property: "DTSTART".into(),
            tz: "Central Standard Time".into()
        }
    );
}

#[test]
fn unknown_tz_flag_is_an_error() {
    let err = parse_events(&calendar(&[], &[]), Some("Mars/Olympus")).unwrap_err();
    assert_eq!(err, IcsError::UnknownDefaultZone("Mars/Olympus".into()));
}

#[test]
fn bad_dtstart_is_an_error_but_bad_created_is_skipped() {
    let err = parse_events(&calendar(&[], &["DTSTART:2026-10-13"]), None).unwrap_err();
    assert!(matches!(err, IcsError::BadValue { ref property, .. } if property == "DTSTART"));
    let e = one(
        &calendar(&[], &["DTSTART:20261013T120000Z", "CREATED:yesterday"]),
        None,
    );
    assert_eq!(e.created, None);
    assert!(e.props.iter().any(|p| p.name == "CREATED"));
}

#[test]
fn recurrence_fields() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/New_York"],
            &[
                "DTSTART:20261012T093000",
                "RRULE:FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR",
                "RDATE:20261017T093000",
                "EXDATE:20261126T093000,20261127T093000",
                "EXDATE;VALUE=DATE:20261225",
                "RECURRENCE-ID:20261014T093000",
            ],
        ),
        None,
    );
    assert_eq!(e.rrule.as_deref(), Some("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR"));
    assert_eq!(e.rdates.len(), 1);
    let exdates: Vec<String> = e.exdates.iter().map(|z| z.to_string()).collect();
    assert_eq!(
        exdates,
        [
            "2026-11-26T09:30:00-05:00[America/New_York]",
            "2026-11-27T09:30:00-05:00[America/New_York]",
            "2026-12-25T00:00:00-05:00[America/New_York]"
        ]
    );
    assert_eq!(
        show(&e.recurrence_id),
        "2026-10-14T09:30:00-04:00[America/New_York]"
    );
}

#[test]
fn text_fields_organizer_geo_and_categories() {
    let e = one(
        &calendar(
            &[],
            &[
                "DTSTART:20261022T160000Z",
                "SUMMARY:Fall Orchestra\\, Live",
                "LOCATION:Boe Chapel\\; Main Floor",
                "DESCRIPTION:Line one\\nLine two",
                "GEO:44.4614;-93.1822",
                "CATEGORIES:Music,Concert",
                "CATEGORIES:Arts",
                "ORGANIZER;CN=Priya Shah;CUTYPE=individual:MAILTO:priya@example.com",
                "STATUS:CONFIRMED",
            ],
        ),
        None,
    );
    assert_eq!(e.summary.as_deref(), Some("Fall Orchestra, Live"));
    assert_eq!(e.location.as_deref(), Some("Boe Chapel; Main Floor"));
    assert_eq!(e.description.as_deref(), Some("Line one\nLine two"));
    assert_eq!(e.geo, Some((44.4614, -93.1822)));
    assert_eq!(e.categories, ["Music", "Concert", "Arts"]);
    assert_eq!(
        e.organizer,
        Some(Organizer {
            name: Some("Priya Shah".into()),
            email: Some("priya@example.com".into()),
            cutype: Some("INDIVIDUAL".into())
        })
    );
    assert_eq!(e.status.as_deref(), Some("CONFIRMED"));
}

#[test]
fn vtimezone_rules_and_alarms_are_not_events() {
    let events = parse_events(&fixture("northfield.ics"), None).unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e.rrule.is_none()));
    let campus = parse_events(&fixture("campus-events.ics"), None).unwrap();
    assert_eq!(campus.len(), 4);
}

#[test]
fn live_feed_snapshots_parse() {
    for (name, count) in [
        ("stolaf.ics", 6),
        ("carleton.ics", 7),
        ("northfield.ics", 2),
    ] {
        let events = parse_events(&fixture(name), None).unwrap();
        assert_eq!(events.len(), count, "{name}");
        assert!(
            events.iter().all(|e| e.start.is_some() && e.tz.is_some()),
            "{name}"
        );
    }
}

#[test]
fn kdl_examples_parse() {
    let team = parse_events(&fixture("team-planning.ics"), None).unwrap();
    assert_eq!(team.len(), 3);
    assert_eq!(team[0].tz.as_deref(), Some("Europe/London"));
    assert_eq!(team[2].summary.as_deref(), Some("Daily standup (moved)"));
    assert!(team[2].recurrence_id.is_some());
    let minimal = parse_events(&fixture("minimal.ics"), None).unwrap();
    assert_eq!(minimal[0].tz.as_deref(), Some("UTC"));
}

#[test]
fn text_escapes_in_urls_are_removed() {
    let e = one(
        &calendar(
            &[],
            &[
                "DTSTART:20261022T160000Z",
                r"URL;VALUE=URI:https://example.edu/a?x=1&y\;z=2",
            ],
        ),
        None,
    );
    assert_eq!(e.url.as_deref(), Some("https://example.edu/a?x=1&y;z=2"));
}

#[test]
fn a_feed_the_size_of_carletons_parses() {
    // Carleton's live feed has about 2,400 events in 3.2 MB.
    let mut text = String::from("BEGIN:VCALENDAR\r\nX-WR-TIMEZONE:America/Chicago\r\n");
    for i in 0..5000 {
        text.push_str(&format!(
            "BEGIN:VEVENT\r\nUID:{i}@example.edu\r\nSUMMARY:Event {i}\r\nDTSTART:20261013T120000\r\nDESCRIPTION:{}\r\nEND:VEVENT\r\n",
            "x".repeat(500)
        ));
    }
    text.push_str("END:VCALENDAR\r\n");
    let events = parse_events(&text, None).unwrap();
    assert_eq!(events.len(), 5000);
    assert_eq!(events[4999].uid.as_deref(), Some("4999@example.edu"));
}

#[test]
fn input_that_is_not_a_calendar_is_an_error() {
    for text in [
        "",
        "<html><body>Moved</body></html>",
        "BEGIN:VCARD\nFN:Jane\nEND:VCARD\n",
    ] {
        assert_eq!(
            parse_events(text, None),
            Err(IcsError::NotCalendar),
            "{text:?}"
        );
    }
}

#[test]
fn an_empty_calendar_has_no_events() {
    assert_eq!(
        parse_events("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n", None),
        Ok(vec![])
    );
}

#[test]
fn times_must_have_exactly_the_rfc_5545_shape() {
    for value in [
        "20261013T12000",
        "20261013T1200000",
        "2026101T120000",
        "20261013 120000",
        "20261013T120000z",
    ] {
        let err = parse_events(
            &calendar(
                &["X-WR-TIMEZONE:America/Chicago"],
                &[&format!("DTSTART:{value}")],
            ),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(err, IcsError::BadValue { ref property, .. } if property == "DTSTART"),
            "{value}: {err:?}"
        );
    }
    for value in ["2026101", "202610130"] {
        let err = parse_events(
            &calendar(&[], &[&format!("DTSTART;VALUE=DATE:{value}")]),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, IcsError::BadValue { .. }), "{value}: {err:?}");
    }
}

#[test]
fn period_rdates_keep_their_end() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &[
                "DTSTART:20261013T120000",
                "RDATE;VALUE=PERIOD:20261014T120000/PT90M,20261015T120000/20261015T150000",
                "RDATE:20261016T120000",
            ],
        ),
        None,
    );
    let rdates: Vec<(String, Option<String>)> = e
        .rdates
        .iter()
        .map(|r| (r.start.to_string(), r.end.as_ref().map(|z| z.to_string())))
        .collect();
    assert_eq!(
        rdates,
        [
            (
                "2026-10-14T12:00:00-05:00[America/Chicago]".to_string(),
                Some("2026-10-14T13:30:00-05:00[America/Chicago]".to_string())
            ),
            (
                "2026-10-15T12:00:00-05:00[America/Chicago]".to_string(),
                Some("2026-10-15T15:00:00-05:00[America/Chicago]".to_string())
            ),
            (
                "2026-10-16T12:00:00-05:00[America/Chicago]".to_string(),
                None
            ),
        ]
    );
}

#[test]
fn a_bad_period_names_the_rdate() {
    let err = parse_events(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &[
                "DTSTART:20261013T120000",
                "RDATE;VALUE=PERIOD:20261014T120000/soon",
            ],
        ),
        None,
    )
    .unwrap_err();
    assert!(
        matches!(err, IcsError::BadValue { ref property, ref value, .. } if property == "RDATE" && value == "20261014T120000/soon"),
        "{err:?}"
    );
}

//! `into event` turns `from ics` rows into valid `event` records.

use std::fs;
use std::path::Path;

use nu_protocol::Value;
use ppp_core::engine::Engine;
use ppp_core::schema::Registry;
use ppp_core::serialize::record_to_json;
use serde_json::{Value as Json, json};

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/ics")
            .join(name),
    )
    .unwrap()
}

/// Runs `from ics | into event` on `ics`, then serializes each record and
/// checks it against the schema.
fn events(ics: &str) -> Vec<Json> {
    let registry = Registry::stdlib().unwrap();
    let time_fields = &registry.record_type("event").unwrap().time_fields;
    let mut engine = Engine::new();
    let rows = engine
        .eval(
            "use ppp *; $in | from ics | into event --source https://calendar.example.edu/feed.ics",
            Value::test_string(ics),
        )
        .unwrap();
    rows.as_list()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let json = record_to_json(row, time_fields).unwrap();
            let errors = registry.validate(i, &json);
            assert!(errors.is_empty(), "{json:#}\n{errors:#?}");
            json
        })
        .collect()
}

#[test]
fn every_fixture_becomes_valid_events() {
    for (name, count) in [
        ("minimal.ics", 1),
        ("campus-events.ics", 4),
        ("team-planning.ics", 3),
        ("stolaf.ics", 6),
        ("carleton.ics", 7),
        ("northfield.ics", 2),
    ] {
        assert_eq!(events(&fixture(name)).len(), count, "{name}");
    }
}

#[test]
fn a_timed_event() {
    let orchestra = events(&fixture("campus-events.ics")).remove(0);
    assert_eq!(
        orchestra,
        json!({
            "type": "event",
            "id": "20261014-orchestra@calendar.example.edu",
            "source": "https://calendar.example.edu/feed.ics",
            "summary": "Fall Orchestra Concert",
            "start": "2026-10-14T19:30:00",
            "end": "2026-10-14T21:00:00",
            "all_day": false,
            "tz": "America/Chicago",
            "status": "confirmed",
            "location": {
                "name": "Boe Memorial Chapel, 1520 St. Olaf Avenue, Northfield, MN 55057",
                "geo": { "lat": 44.4614, "lon": -93.1822 }
            },
            "description": "The orchestra opens its season with Dvořák's Symphony No. 8.\nFree and open to the public.",
            "url": "https://www.example.edu/events/fall-orchestra",
            "categories": ["Music", "Concert"],
            "organizers": [{ "org": { "name": "Music Department", "emails": [{ "address": "music@example.edu" }] } }],
            "created": "2026-08-06T18:11:07Z",
            "modified": "2026-09-02T15:53:15Z",
            "extra": { "dtstamp": "20261011T000000Z" }
        })
    );
}

#[test]
fn an_all_day_event() {
    let weekend = events(&fixture("campus-events.ics")).remove(1);
    assert_eq!(
        (weekend["start"].as_str(), weekend["end"].as_str()),
        (Some("2026-10-17"), Some("2026-10-19"))
    );
    assert_eq!(weekend["all_day"], true);
}

#[test]
fn all_day_dates_without_any_zone_keep_their_date() {
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nSUMMARY:Holiday\nDTSTART;VALUE=DATE:20261017\nEND:VEVENT\nEND:VCALENDAR\n";
    let holiday = events(ics).remove(0);
    assert_eq!(holiday["start"], "2026-10-17");
    assert_eq!(holiday.get("tz"), None);
}

#[test]
fn recurrence_overrides_get_an_occurrence_id() {
    let team = events(&fixture("team-planning.ics"));
    assert_eq!(team[1]["rrule"], "FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR");
    assert_eq!(
        team[1]["exdates"],
        json!(["2026-11-26T09:30:00", "2026-11-27T09:30:00"])
    );
    assert_eq!(
        team[2]["id"],
        "standup-2026@product.example.com_20261014T133000Z"
    );
    assert_eq!(team[2]["series"], "standup-2026@product.example.com");
    assert_eq!(team[2]["recurrence_id"], "2026-10-14T09:30:00");
}

#[test]
fn organizers_are_people_only_when_cutype_says_so() {
    let team = events(&fixture("team-planning.ics"));
    assert_eq!(
        team[0]["organizers"],
        json!([{ "person": { "name": { "display": "Priya Shah" }, "emails": [{ "address": "priya@example.com" }] } }])
    );
}

#[test]
fn repeated_unmapped_properties_are_kept_as_lists() {
    let team = events(&fixture("team-planning.ics"));
    assert_eq!(
        team[0]["extra"]["attendee"],
        json!(["mailto:priya@example.com", "mailto:sam@example.com"])
    );
}

#[test]
fn relative_urls_move_to_extra() {
    let northfield = events(&fixture("northfield.ics")).remove(0);
    assert_eq!(northfield.get("url"), None);
    assert_eq!(
        northfield["extra"]["url"],
        "/common/modules/iCalendar/iCalendar.aspx?feed=calendar&catID=41"
    );
}

#[test]
fn a_row_without_a_uid_is_an_error() {
    let mut engine = Engine::new();
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Mystery\nDTSTART:20261013T120000Z\nEND:VEVENT\nEND:VCALENDAR\n";
    let err = engine
        .eval(
            "use ppp *; $in | from ics | into event",
            Value::test_string(ics),
        )
        .unwrap_err();
    assert!(err.0.contains("event has no uid: Mystery"), "{err}");
}

#[test]
fn drop_empty_removes_only_empty_values() {
    let mut engine = Engine::new();
    let out = engine
        .eval(
            "use ppp *; {a: null, b: '', c: [], d: {}, e: 0, f: false, g: 'x', h: [1]} | drop-empty | to nuon",
            Value::test_nothing(),
        )
        .unwrap();
    assert_eq!(
        out.coerce_into_string().unwrap(),
        "{e: 0, f: false, g: x, h: [1]}"
    );
}

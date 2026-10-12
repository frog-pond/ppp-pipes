//! `expand rrule` inside the engine, from `from ics` rows to valid events.

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

/// `from ics | expand rrule … | into event`, serialized and validated.
fn expanded(ics: &str, from: &str, until: &str) -> Vec<Json> {
    let registry = Registry::stdlib().unwrap();
    let time_fields = &registry.record_type("event").unwrap().time_fields;
    let mut engine = Engine::new();
    let rows = engine
        .eval(
            &format!("use pipes *; $in | from ics | expand rrule --from {from} --until {until} | into event"),
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

fn summary_and_start(events: &[Json]) -> Vec<(String, String)> {
    events
        .iter()
        .map(|e| {
            (
                e["summary"].as_str().unwrap().into(),
                e["start"].as_str().unwrap().into(),
            )
        })
        .collect()
}

#[test]
fn a_weekly_event_becomes_its_occurrences() {
    let events = expanded(
        &fixture("campus-events.ics"),
        "2026-11-01T00:00:00-05:00",
        "2026-12-01T00:00:00-06:00",
    );
    let chess: Vec<&Json> = events
        .iter()
        .filter(|e| e["summary"] == "Chess Club")
        .collect();
    let starts: Vec<&str> = chess.iter().map(|e| e["start"].as_str().unwrap()).collect();
    // Weekly on Tuesdays; 2026-11-24 is an EXDATE.
    assert_eq!(
        starts,
        [
            "2026-11-03T18:00:00",
            "2026-11-10T18:00:00",
            "2026-11-17T18:00:00"
        ]
    );
    assert_eq!(
        chess[0]["id"],
        "weekly-chess@calendar.example.edu_20261104T000000Z"
    );
    assert_eq!(chess[0]["series"], "weekly-chess@calendar.example.edu");
    assert_eq!(chess[0]["end"], "2026-11-03T20:00:00");
    assert_eq!(chess[0].get("rrule"), None);
    assert_eq!(chess[0].get("exdates"), None);
}

#[test]
fn rows_that_do_not_recur_pass_through() {
    let events = expanded(
        &fixture("campus-events.ics"),
        "2026-11-01T00:00:00-05:00",
        "2026-12-01T00:00:00-06:00",
    );
    let others: Vec<&str> = events
        .iter()
        .filter(|e| e["summary"] != "Chess Club")
        .map(|e| e["summary"].as_str().unwrap())
        .collect();
    // `expand rrule` only expands; it doesn't drop the October events.
    assert_eq!(
        others,
        [
            "Fall Orchestra Concert",
            "Family Weekend",
            "Guest Lecture: Pipelines Everywhere"
        ]
    );
}

#[test]
fn an_override_stands_in_for_its_occurrence() {
    let events = expanded(
        &fixture("team-planning.ics"),
        "2026-10-12T00:00:00-04:00",
        "2026-10-17T00:00:00-04:00",
    );
    let standups: Vec<(String, String)> = summary_and_start(&events)
        .into_iter()
        .filter(|(summary, _)| summary.starts_with("Daily standup"))
        .collect();
    assert_eq!(
        standups,
        [
            ("Daily standup".into(), "2026-10-12T09:30:00".into()),
            ("Daily standup".into(), "2026-10-13T09:30:00".into()),
            ("Daily standup".into(), "2026-10-15T09:30:00".into()),
            ("Daily standup".into(), "2026-10-16T09:30:00".into()),
            ("Daily standup (moved)".into(), "2026-10-14T11:00:00".into()),
        ]
    );
    let moved = events
        .iter()
        .find(|e| e["summary"] == "Daily standup (moved)")
        .unwrap();
    assert_eq!(
        moved["id"],
        "standup-2026@product.example.com_20261014T133000Z"
    );
    let generated = events
        .iter()
        .find(|e| e["start"] == "2026-10-13T09:30:00")
        .unwrap();
    assert_eq!(
        generated["id"],
        "standup-2026@product.example.com_20261013T133000Z"
    );
}

#[test]
fn ids_are_the_same_whether_or_not_a_feed_was_expanded() {
    // Google pre-expands recurrences as `<uid>_<UTC start>`; so does `expand rrule`.
    let events = expanded(
        &fixture("team-planning.ics"),
        "2026-10-12T00:00:00-04:00",
        "2026-10-13T00:00:00-04:00",
    );
    let ids: Vec<&str> = events.iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert!(
        ids.contains(&"standup-2026@product.example.com_20261012T133000Z"),
        "{ids:?}"
    );
}

#[test]
fn bad_rules_name_the_event() {
    let ics = "BEGIN:VCALENDAR\nX-WR-TIMEZONE:America/Chicago\nBEGIN:VEVENT\nUID:odd@example.edu\nDTSTART:20261013T120000\nRRULE:FREQ=SOMETIMES\nEND:VEVENT\nEND:VCALENDAR\n";
    let mut engine = Engine::new();
    let err = engine
        .eval(
            "$in | from ics | expand rrule --from 2026-10-01T00:00:00Z --until 2026-11-01T00:00:00Z",
            Value::test_string(ics),
        )
        .unwrap_err();
    assert!(
        err.0.contains("cannot expand event odd@example.edu"),
        "{err}"
    );
    assert!(err.0.contains("SOMETIMES"), "{err}");
}

#[test]
fn the_window_is_required() {
    let mut engine = Engine::new();
    let err = engine
        .eval(
            "[] | expand rrule --from 2026-10-01T00:00:00Z",
            Value::test_nothing(),
        )
        .unwrap_err();
    assert!(err.0.contains("until"), "{err}");
}

#[test]
fn events_without_an_end_stay_without_one() {
    let ics = "BEGIN:VCALENDAR\nX-WR-TIMEZONE:America/Chicago\nBEGIN:VEVENT\nUID:m@example.edu\nSUMMARY:Bell\nDTSTART:20261013T120000\nRRULE:FREQ=DAILY;COUNT=2\nEND:VEVENT\nEND:VCALENDAR\n";
    let events = expanded(ics, "2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].get("end"), None);
    assert_eq!(events[1]["start"], json!("2026-10-14T12:00:00"));
}

#[test]
fn all_day_events_with_no_zone_keep_their_dates() {
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:h@example.edu\nSUMMARY:Founders Day\nDTSTART;VALUE=DATE:20261105\nDTEND;VALUE=DATE:20261106\nRRULE:FREQ=YEARLY\nEND:VEVENT\nEND:VCALENDAR\n";
    let events = expanded(ics, "2026-01-01T00:00:00Z", "2029-01-01T00:00:00Z");
    let dates: Vec<(&str, &str)> = events
        .iter()
        .map(|e| (e["start"].as_str().unwrap(), e["end"].as_str().unwrap()))
        .collect();
    assert_eq!(
        dates,
        [
            ("2026-11-05", "2026-11-06"),
            ("2027-11-05", "2027-11-06"),
            ("2028-11-05", "2028-11-06")
        ]
    );
    assert_eq!(events[1]["id"], "h@example.edu_20271105T000000Z");
}

#[test]
fn an_all_day_series_with_a_date_until() {
    // The form Google writes for all-day series.
    let ics = "BEGIN:VCALENDAR\nX-WR-TIMEZONE:America/Chicago\nBEGIN:VEVENT\nUID:w@example.edu\nSUMMARY:Writing Lab\nDTSTART;VALUE=DATE:20261105\nDTEND;VALUE=DATE:20261106\nRRULE:FREQ=WEEKLY;UNTIL=20261119\nEND:VEVENT\nEND:VCALENDAR\n";
    let events = expanded(
        ics,
        "2026-11-01T00:00:00-05:00",
        "2027-01-01T00:00:00-06:00",
    );
    let starts: Vec<&str> = events
        .iter()
        .map(|e| e["start"].as_str().unwrap())
        .collect();
    assert_eq!(starts, ["2026-11-05", "2026-11-12", "2026-11-19"]);
}

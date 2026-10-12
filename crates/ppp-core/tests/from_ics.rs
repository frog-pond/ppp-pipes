//! `from ics` inside the engine: columns, values and the --tz flag.

use std::fs;
use std::path::Path;

use nu_protocol::Value;
use ppp_core::engine::Engine;

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/ics")
            .join(name),
    )
    .unwrap()
}

fn nuon(engine: &mut Engine, source: &str, input: Value) -> String {
    let out = engine.eval(source, input).unwrap();
    engine
        .eval("$in | to nuon", out)
        .unwrap()
        .coerce_into_string()
        .unwrap()
}

#[test]
fn from_ics_returns_one_row_per_event() {
    let mut engine = Engine::new();
    let out = nuon(
        &mut engine,
        "$in | from ics | select summary start all_day tz categories | first 2",
        Value::test_string(fixture("campus-events.ics")),
    );
    assert_eq!(
        out,
        "[[summary, start, all_day, tz, categories]; [\"Fall Orchestra Concert\", 2026-10-14T19:30:00-05:00, false, America/Chicago, [Music, Concert]], [\"Family Weekend\", 2026-10-17T00:00:00-05:00, true, America/Chicago, [Community]]]"
    );
}

#[test]
fn from_ics_columns() {
    let mut engine = Engine::new();
    let out = nuon(
        &mut engine,
        "$in | from ics | columns",
        Value::test_string(fixture("minimal.ics")),
    );
    assert_eq!(
        out,
        "[summary, start, end, all_day, tz, location, description, url, categories, organizer, geo, uid, status, rrule, rdates, exdates, recurrence_id, created, last_modified, props]"
    );
}

#[test]
fn from_ics_tz_flag() {
    let mut engine = Engine::new();
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART:20261013T120000\nEND:VEVENT\nEND:VCALENDAR\n";
    let out = nuon(
        &mut engine,
        "$in | from ics --tz Europe/London | get start.0",
        Value::test_string(ics),
    );
    assert_eq!(out, "2026-10-13T12:00:00+01:00");
    let err = engine
        .eval("$in | from ics", Value::test_string(ics))
        .unwrap_err();
    assert!(err.0.contains("cannot read iCalendar text"), "{err}");
    assert!(err.0.contains("pass --tz"), "{err}");
}

#[test]
fn from_ics_reads_binary_input() {
    // Feeds served as application/octet-stream arrive from `http get --raw` as binary.
    let mut engine = Engine::new();
    let mut bytes = "\u{feff}".as_bytes().to_vec();
    bytes.extend(fixture("minimal.ics").into_bytes());
    let out = nuon(
        &mut engine,
        "$in | from ics | get summary",
        Value::test_binary(bytes),
    );
    assert_eq!(out, "[Dentist]");
}

#[test]
fn from_ics_refuses_binary_that_is_not_utf8() {
    let mut engine = Engine::new();
    let err = engine
        .eval("$in | from ics", Value::test_binary(vec![0xff, 0xfe, 0x00]))
        .unwrap_err();
    assert!(err.0.contains("is not UTF-8 text"), "{err}");
}

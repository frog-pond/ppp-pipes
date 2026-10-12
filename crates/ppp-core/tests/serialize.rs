//! Nushell records become JSON with each time field in the form its schema
//! declares.

use chrono::DateTime;
use nu_protocol::{Record, Value, record};
use ppp_core::schema::Registry;
use ppp_core::serialize::{SerializeError, record_to_json};
use serde_json::json;

fn date(rfc3339: &str) -> Value {
    Value::test_date(DateTime::parse_from_rfc3339(rfc3339).unwrap())
}

fn event(fields: Record) -> Value {
    let mut rec = record! {
        "type" => Value::test_string("event"),
        "id" => Value::test_string("x@example.edu"),
        "summary" => Value::test_string("Chess Club"),
    };
    for (k, v) in fields {
        rec.push(k, v);
    }
    Value::test_record(rec)
}

fn to_json(record: &Value) -> Result<serde_json::Value, SerializeError> {
    let registry = Registry::stdlib().unwrap();
    record_to_json(record, &registry.record_type("event").unwrap().time_fields)
}

#[test]
fn instant_fields_are_written_in_utc() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-22T16:00:00Z"),
        "created" => date("2026-08-06T13:11:07-05:00"),
    }))
    .unwrap();
    assert_eq!(json["created"], "2026-08-06T18:11:07Z");
}

#[test]
fn when_fields_with_tz_are_local_wall_time() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-15T00:30:00Z"),
        "tz" => Value::test_string("America/Chicago"),
    }))
    .unwrap();
    assert_eq!(json["start"], "2026-10-14T19:30:00");
}

#[test]
fn when_fields_on_all_day_records_are_dates() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-17T00:00:00-05:00"),
        "end" => date("2026-10-19T00:00:00-05:00"),
        "all_day" => Value::test_bool(true),
        "tz" => Value::test_string("America/Chicago"),
    }))
    .unwrap();
    assert_eq!(
        (json["start"].as_str(), json["end"].as_str()),
        (Some("2026-10-17"), Some("2026-10-19"))
    );
}

#[test]
fn when_fields_without_tz_are_instants() {
    let json = to_json(&event(
        record! { "start" => date("2026-10-22T11:00:00-05:00") },
    ))
    .unwrap();
    assert_eq!(json["start"], "2026-10-22T16:00:00Z");
}

#[test]
fn list_items_follow_the_schema() {
    let json = to_json(&event(record! {
        "start" => date("2026-09-08T18:00:00-05:00"),
        "tz" => Value::test_string("America/Chicago"),
        "exdates" => Value::test_list(vec![date("2026-11-24T18:00:00-06:00"), date("2026-12-01T18:00:00-06:00")]),
    }))
    .unwrap();
    assert_eq!(
        json["exdates"],
        json!(["2026-11-24T18:00:00", "2026-12-01T18:00:00"])
    );
}

#[test]
fn fall_back_hour_keeps_its_wall_time() {
    // 01:30 happens twice in Chicago on 2026-11-01; this is the second (CST).
    let json = to_json(&event(record! {
        "start" => date("2026-11-01T01:30:00-06:00"),
        "tz" => Value::test_string("America/Chicago"),
    }))
    .unwrap();
    assert_eq!(json["start"], "2026-11-01T01:30:00");
}

#[test]
fn datetimes_outside_time_fields_are_instants() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-22T16:00:00Z"),
        "extra" => Value::test_record(record! { "fetched" => date("2026-10-11T20:00:00-04:00") }),
    }))
    .unwrap();
    assert_eq!(json["extra"]["fetched"], "2026-10-12T00:00:00Z");
}

#[test]
fn all_day_without_tz_uses_the_datetimes_own_offset() {
    // An ICS `VALUE=DATE` has no zone. Midnight at +02:00 is still the 16th
    // in UTC, so this fails if the date is read in any zone but its own.
    let registry = Registry::stdlib().unwrap();
    let json = to_json(&event(record! {
        "start" => date("2026-10-17T00:00:00+02:00"),
        "end" => date("2026-10-19T00:00:00+02:00"),
        "all_day" => Value::test_bool(true),
    }))
    .unwrap();
    assert_eq!(
        (json["start"].as_str(), json["end"].as_str()),
        (Some("2026-10-17"), Some("2026-10-19"))
    );
    assert_eq!(registry.validate(0, &json), vec![]);
}

#[test]
fn unknown_tz_is_an_error() {
    let err = to_json(&event(record! {
        "start" => date("2026-10-17T00:00:00-05:00"),
        "tz" => Value::test_string("America/Chicgo"),
    }))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::UnknownTimeZone {
            pointer: "/start".into(),
            tz: "America/Chicgo".into()
        }
    );
}

#[test]
fn plain_values() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-22T16:00:00Z"),
        "duration" => Value::test_duration(2 * 60 * 60 * 1_000_000_000),
        "featured" => Value::test_bool(true),
        "location" => Value::test_record(record! { "name" => Value::test_glob("Regents Hall 150") }),
        "cost" => Value::test_record(record! { "min" => Value::test_int(5), "max" => Value::test_float(10.5), "currency" => Value::test_string("USD") }),
        "series" => Value::test_nothing(),
    }))
    .unwrap();
    assert_eq!(json["duration"], "PT2H");
    assert_eq!(json["featured"], true);
    assert_eq!(json["location"], json!({ "name": "Regents Hall 150" }));
    assert_eq!(
        json["cost"],
        json!({ "min": 5, "max": 10.5, "currency": "USD" })
    );
    assert_eq!(json["series"], serde_json::Value::Null);
}

#[test]
fn binary_is_an_error() {
    let err = to_json(&event(record! {
        "extra" => Value::test_record(record! { "raw" => Value::test_binary(vec![1u8, 2]) }),
    }))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::Unsupported {
            pointer: "/extra/raw".into(),
            type_name: "binary".into()
        }
    );
}

#[test]
fn nan_is_an_error() {
    let err = to_json(&event(
        record! { "extra" => Value::test_record(record! { "x" => Value::test_float(f64::NAN) }) },
    ))
    .unwrap_err();
    assert!(matches!(err, SerializeError::NonFinite { pointer, .. } if pointer == "/extra/x"));
}

#[test]
fn serialized_events_validate() {
    let registry = Registry::stdlib().unwrap();
    let json = to_json(&event(record! {
        "start" => date("2026-10-14T19:30:00-05:00"),
        "end" => date("2026-10-14T21:00:00-05:00"),
        "tz" => Value::test_string("America/Chicago"),
        "exdates" => Value::test_list(vec![date("2026-10-21T19:30:00-05:00")]),
        "rrule" => Value::test_string("FREQ=WEEKLY"),
        "created" => date("2026-08-06T18:11:07Z"),
    }))
    .unwrap();
    assert_eq!(registry.validate(0, &json), vec![]);
}

#[test]
fn pointers_escape_slashes_and_tildes() {
    let err = to_json(&event(record! {
        "extra" => Value::test_record(record! { "a/b~c" => Value::test_binary(vec![0u8]) }),
    }))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::Unsupported {
            pointer: "/extra/a~1b~0c".into(),
            type_name: "binary".into()
        }
    );
}

#[test]
fn datetimes_past_year_9999_are_an_error() {
    let far = chrono::NaiveDate::from_ymd_opt(12026, 10, 22)
        .unwrap()
        .and_hms_opt(16, 0, 0)
        .unwrap();
    let err = to_json(&event(
        record! { "start" => Value::test_date(far.and_utc().fixed_offset()) },
    ))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::OutOfRange {
            pointer: "/start".into()
        }
    );
    assert_eq!(
        err.to_string(),
        "/start: datetime is outside -9999-01-02T01:59:59Z to 9999-12-30T22:00:00Z"
    );
}

#[test]
fn the_range_named_in_errors_is_jiffs() {
    assert_eq!(
        ppp_core::time::RANGE,
        format!(
            "{} to {}",
            ppp_core::time::format_instant(jiff::Timestamp::MIN),
            ppp_core::time::format_instant(jiff::Timestamp::MAX)
        )
    );
}

#[test]
fn durations_are_whole_seconds() {
    let registry = Registry::stdlib().unwrap();
    let json = to_json(&event(record! {
        "start" => date("2026-10-22T16:00:00Z"),
        "duration" => Value::test_duration(90 * 60 * 1_000_000_000 + 500_000_000),
    }))
    .unwrap();
    assert_eq!(json["duration"], "PT1H30M");
    assert_eq!(registry.validate(0, &json), vec![]);
}

#[test]
fn negative_durations_are_an_error() {
    let err = to_json(&event(record! {
        "start" => date("2026-10-22T16:00:00Z"),
        "duration" => Value::test_duration(-60 * 60 * 1_000_000_000),
    }))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::NegativeDuration {
            pointer: "/duration".into()
        }
    );
}

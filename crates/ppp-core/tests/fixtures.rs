//! Every example record (JSON array or JSONL) is valid, and every invalid record fails at the
//! place its file name describes.

use std::fs;
use std::path::{Path, PathBuf};

use ppp_core::schema::{RecordError, Registry};
use serde_json::Value as Json;

fn fixtures(dir: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(dir)
}

fn records(path: &Path) -> Vec<Json> {
    let text = fs::read_to_string(path).unwrap();
    if path.extension().is_some_and(|ext| ext == "jsonl") {
        return text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    }
    match serde_json::from_str(&text).unwrap() {
        Json::Array(records) => records,
        other => panic!("{} is not a JSON array: {other}", path.display()),
    }
}

fn errors(registry: &Registry, path: &Path) -> Vec<RecordError> {
    records(path)
        .iter()
        .enumerate()
        .flat_map(|(i, record)| registry.validate(i, record))
        .collect()
}

#[test]
fn every_example_is_valid() {
    let registry = Registry::stdlib().unwrap();
    let mut checked = 0;
    for entry in fs::read_dir(fixtures("examples")).unwrap() {
        let path = entry.unwrap().path();
        let errors = errors(&registry, &path);
        assert!(errors.is_empty(), "{}: {errors:#?}", path.display());
        checked += 1;
    }
    assert_eq!(checked, 5);
}

/// Each invalid fixture and the JSON Pointer its one mistake is reported at.
const INVALID: &[(&str, &str)] = &[
    ("agent-both.json", "/organizers/0"),
    ("all-day-exdate-with-time.json", "/exdates/0"),
    ("all-day-rdate-end-with-time.json", "/rdates/0/end"),
    ("all-day-rdate-with-time.json", "/rdates/0/start"),
    ("all-day-recurrence-id-with-time.json", "/recurrence_id"),
    ("all-day-with-time.json", "/start"),
    ("bad-currency.json", "/cost"),
    ("bad-duration.json", "/duration"),
    ("bad-geo.json", "/location/place/geo/lat"),
    ("bad-month.json", "/start"),
    ("bad-rrule.json", "/rrule"),
    ("bad-status.json", "/status"),
    ("bad-type.json", "/type"),
    ("bad-url.json", "/url"),
    ("empty-summary.json", "/summary"),
    ("end-and-duration.json", ""),
    ("local-exdate-without-tz.json", ""),
    ("local-rdate-end-without-tz.json", ""),
    ("local-rdate-without-tz.json", ""),
    ("local-without-tz.json", ""),
    ("missing-id.json", ""),
    ("missing-start.json", ""),
    ("organizer-extra-field.json", "/organizers/0"),
    ("rdate-not-object.json", "/rdates/0"),
    ("unknown-field.json", ""),
    ("unknown-tz.json", "/tz"),
];

#[test]
fn every_invalid_record_fails_where_expected() {
    let registry = Registry::stdlib().unwrap();
    for (file, pointer) in INVALID {
        let errors = errors(&registry, &fixtures("invalid").join(file));
        assert!(
            errors.iter().any(|e| e.pointer == *pointer),
            "{file}: expected an error at {pointer:?}, got {errors:#?}"
        );
        assert!(
            errors.iter().all(|e| e.pointer.starts_with(pointer)),
            "{file}: errors outside {pointer:?}: {errors:#?}"
        );
    }
}

#[test]
fn every_invalid_fixture_is_listed() {
    let mut on_disk: Vec<String> = fs::read_dir(fixtures("invalid"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort();
    let listed: Vec<String> = INVALID.iter().map(|(f, _)| f.to_string()).collect();
    assert_eq!(on_disk, listed);
}

#[test]
fn records_that_are_not_objects_have_no_type() {
    let registry = Registry::stdlib().unwrap();
    let errors = registry.validate(3, &serde_json::json!(42));
    assert_eq!(errors.len(), 1);
    assert_eq!((errors[0].index, errors[0].pointer.as_str()), (3, ""));
    assert_eq!(errors[0].message, "record has no \"type\"");
}

#[test]
fn long_excerpts_are_cut_on_character_boundaries() {
    let registry = Registry::stdlib().unwrap();
    let summary = "Dvořák ".repeat(30);
    let record = serde_json::json!({
        "type": "event", "id": "x", "summary": summary, "start": "2026-10-22T16:00:00Z", "colour": "x"
    });
    let errors = registry.validate(0, &record);
    let root = errors.iter().find(|e| e.pointer.is_empty()).unwrap();
    assert_eq!(root.excerpt.chars().count(), 80);
    assert!(root.excerpt.ends_with('…'));
}

#[test]
fn a_value_failing_pattern_and_format_is_one_error() {
    let registry = Registry::stdlib().unwrap();
    let record = serde_json::json!({
        "type": "event", "id": "x", "summary": "Chess Club", "start": "2026-10-22T16:00:00Z",
        "duration": "2h", "url": "not a url"
    });
    let pointers: Vec<String> = registry
        .validate(0, &record)
        .into_iter()
        .map(|e| e.pointer)
        .collect();
    assert_eq!(pointers, ["/duration", "/url"]);
}

#[test]
fn bad_values_say_what_was_expected() {
    let registry = Registry::stdlib().unwrap();
    let record = serde_json::json!({
        "type": "event", "id": "x", "summary": "Chess Club", "start": "soon",
        "tz": "Mars/Base", "duration": "2h", "created": "yesterday",
        "recurrence_id": "2026-13-01", "url": "not a url"
    });
    let messages: Vec<(String, String)> = registry
        .validate(0, &record)
        .into_iter()
        .map(|e| (e.pointer, e.message))
        .collect();
    let expected = [
        (
            "/created",
            "\"yesterday\" is not an instant, such as 2026-10-14T00:30:00Z",
        ),
        (
            "/duration",
            "\"2h\" is not an ISO 8601 duration, such as PT2H",
        ),
        (
            "/recurrence_id",
            "\"2026-13-01\" is not a date (2026-10-14), an instant (2026-10-14T00:30:00Z) or a local date-time (2026-10-14T19:30:00)",
        ),
        (
            "/start",
            "\"soon\" is not a date (2026-10-14), an instant (2026-10-14T00:30:00Z) or a local date-time (2026-10-14T19:30:00)",
        ),
        (
            "/tz",
            "\"Mars/Base\" is not an IANA time zone name, such as America/Chicago",
        ),
        ("/url", "\"not a url\" is not a URL"),
    ];
    let mut messages = messages;
    messages.sort();
    assert_eq!(
        messages,
        expected.map(|(p, m)| (p.to_string(), m.to_string()))
    );
}

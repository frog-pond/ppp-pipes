//! Every example record is valid, and every invalid record fails at the
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
    assert_eq!(checked, 4);
}

/// Each invalid fixture and the JSON Pointer its one mistake is reported at.
const INVALID: &[(&str, &str)] = &[
    ("agent-both.json", "/organizers/0"),
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
    ("local-without-tz.json", ""),
    ("missing-id.json", ""),
    ("missing-start.json", ""),
    ("organizer-extra-field.json", "/organizers/0"),
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

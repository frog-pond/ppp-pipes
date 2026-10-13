//! The time-field walker finds every time field in a record type, and
//! refuses schemas it cannot read.

use ppp_core::schema::{Registry, SchemaError};
use ppp_core::time_fields::TimeKind;
use serde_json::{Value as Json, json};

fn common() -> Json {
    serde_json::from_str(include_str!("../../../stdlib/common.schema.json")).unwrap()
}

/// A one-type schema document whose `thing` record has the given properties
/// and extra keywords.
fn doc_with(props: Json, extra: Json) -> Json {
    let mut def = json!({
        "$anchor": "thing",
        "type": "object",
        "properties": props,
    });
    def.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://pipes.frogpond.tech/test/1",
        "$defs": { "thing": def }
    })
}

const C: &str = "https://pipes.frogpond.tech/stdlib/common/1#/$defs";

#[test]
fn event_time_fields() {
    let registry = Registry::stdlib().unwrap();
    let event = registry.record_type("event").unwrap();
    assert_eq!(
        event.uri,
        "https://pipes.frogpond.tech/stdlib/calendar/1#event"
    );
    assert_eq!(
        event.time_fields.paths(),
        vec![
            ("created".to_string(), TimeKind::Instant),
            ("end".to_string(), TimeKind::When),
            ("exdates[]".to_string(), TimeKind::When),
            ("modified".to_string(), TimeKind::Instant),
            ("rdates[].end".to_string(), TimeKind::When),
            ("rdates[].start".to_string(), TimeKind::When),
            ("recurrence_id".to_string(), TimeKind::When),
            ("start".to_string(), TimeKind::When),
        ]
    );
}

#[test]
fn nested_and_array_fields() {
    let doc = doc_with(
        json!({
            "at": { "$ref": format!("{C}/instant") },
            "days": { "type": "array", "items": { "$ref": format!("{C}/date") } },
            "inner": { "type": "object", "properties": { "wall": { "$ref": format!("{C}/local-date-time") } } }
        }),
        json!({}),
    );
    let registry = Registry::from_documents(vec![common(), doc]).unwrap();
    assert_eq!(
        registry.record_type("thing").unwrap().time_fields.paths(),
        vec![
            ("at".to_string(), TimeKind::Instant),
            ("days[]".to_string(), TimeKind::Date),
            ("inner.wall".to_string(), TimeKind::LocalDateTime),
        ]
    );
}

#[test]
fn time_field_under_one_of_is_refused() {
    let doc = doc_with(
        json!({}),
        json!({ "oneOf": [{ "properties": { "at": { "$ref": format!("{C}/instant") } } }] }),
    );
    match Registry::from_documents(vec![common(), doc]) {
        Err(SchemaError::UnsupportedTimeKeyword { keyword, .. }) => assert_eq!(keyword, "oneOf"),
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("expected an error"),
    }
}

#[test]
fn one_of_without_time_fields_is_fine() {
    let doc = doc_with(
        json!({ "cost": { "$ref": format!("{C}/money") }, "who": { "$ref": format!("{C}/agent") } }),
        json!({}),
    );
    let registry = Registry::from_documents(vec![common(), doc]).unwrap();
    assert!(
        registry
            .record_type("thing")
            .unwrap()
            .time_fields
            .is_empty()
    );
}

#[test]
fn conflicting_time_forms_are_refused() {
    let doc = doc_with(
        json!({ "at": { "$ref": format!("{C}/instant") } }),
        json!({ "allOf": [{ "properties": { "at": { "$ref": format!("{C}/date") } } }] }),
    );
    match Registry::from_documents(vec![common(), doc]) {
        Err(SchemaError::ConflictingTimeKinds { path }) => assert_eq!(path, "at"),
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("expected an error"),
    }
}

#[test]
fn recursive_schemas_end() {
    let doc = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://pipes.frogpond.tech/test/1",
        "$defs": {
            "node": {
                "$anchor": "node",
                "type": "object",
                "properties": {
                    "at": { "$ref": format!("{C}/instant") },
                    "parent": { "$ref": "#/$defs/node" }
                }
            }
        }
    });
    let registry = Registry::from_documents(vec![common(), doc]).unwrap();
    assert_eq!(
        registry.record_type("node").unwrap().time_fields.paths(),
        vec![
            ("at".to_string(), TimeKind::Instant),
            ("parent.at".to_string(), TimeKind::Instant)
        ]
    );
}

#[test]
fn relative_refs_resolve_against_the_document_id() {
    let doc = doc_with(
        json!({
            "up": { "$ref": "../stdlib/common/1#/$defs/instant" },
            "rooted": { "$ref": "/stdlib/common/1#/$defs/date" },
            "sibling": { "$ref": "2#/$defs/stamp" }
        }),
        json!({}),
    );
    let sibling = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://pipes.frogpond.tech/test/2",
        "$defs": {
            "stamp": {
                "type": "object",
                "properties": { "at": { "$ref": format!("{C}/instant") } }
            }
        }
    });
    let registry = Registry::from_documents(vec![common(), doc, sibling]).unwrap();
    assert_eq!(
        registry.record_type("thing").unwrap().time_fields.paths(),
        vec![
            ("rooted".to_string(), TimeKind::Date),
            ("sibling.at".to_string(), TimeKind::Instant),
            ("up".to_string(), TimeKind::Instant),
        ]
    );
    let record = json!({
        "type": "thing",
        "up": "2026-10-14T00:30:00Z",
        "rooted": "2026-10-14",
        "sibling": { "at": "2026-10-14T00:30:00Z" }
    });
    assert!(registry.validate(0, &record).is_empty());
    let bad = json!({ "type": "thing", "rooted": "soon" });
    assert_eq!(registry.validate(0, &bad)[0].pointer, "/rooted");
}

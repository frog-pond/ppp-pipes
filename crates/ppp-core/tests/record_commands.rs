//! `validate` and `to pipes-json` inside the engine.

use nu_protocol::Value;
use ppp_core::engine::Engine;

const EVENT: &str =
    "{type: event, id: a, summary: Concert, start: 2026-10-14T19:30:00-05:00, tz: America/Chicago}";

fn eval(source: &str) -> Result<Value, String> {
    Engine::new()
        .eval(source, Value::test_nothing())
        .map_err(|e| e.0)
}

#[test]
fn validate_passes_valid_records_on() {
    let out = eval(&format!("[{EVENT}] | validate | get 0.summary")).unwrap();
    assert_eq!(out.coerce_into_string().unwrap(), "Concert");
    let single = eval(&format!("{EVENT} | validate | get id")).unwrap();
    assert_eq!(single.coerce_into_string().unwrap(), "a");
}

#[test]
fn validate_names_the_failures() {
    let err = eval("[{type: event, id: a}] | validate").unwrap_err();
    assert!(err.contains("records are not valid"), "{err}");
    assert!(err.contains("\"summary\" is a required property"), "{err}");
    let unknown = eval("[{type: party, id: a}] | validate").unwrap_err();
    assert!(
        unknown.contains("unknown record type \\\"party\\\"")
            || unknown.contains("unknown record type \"party\""),
        "{unknown}"
    );
}

#[test]
fn to_pipes_json_writes_times_in_their_schema_form() {
    let text = eval(&format!("[{EVENT}] | to pipes-json"))
        .unwrap()
        .coerce_into_string()
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json[0]["start"], "2026-10-14T19:30:00");
    let one = eval(&format!("{EVENT} | to pipes-json"))
        .unwrap()
        .coerce_into_string()
        .unwrap();
    assert!(one.trim_start().starts_with('{'), "{one}");
}

#[test]
fn to_pipes_json_refuses_invalid_records() {
    let err = eval("{type: event, id: a} | to pipes-json").unwrap_err();
    assert!(err.contains("records are not valid"), "{err}");
}

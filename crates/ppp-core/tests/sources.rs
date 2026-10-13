//! The Tribe and Presence readers, through `into event` to valid events.

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
            .join("../../fixtures/json")
            .join(name),
    )
    .unwrap()
}

/// Runs `$in | from json | <reader> | into event` and validates each record.
fn events(json: &str, reader: &str) -> Vec<Json> {
    let registry = Registry::stdlib().unwrap();
    let time_fields = &registry.record_type("event").unwrap().time_fields;
    let mut engine = Engine::new();
    let rows = engine
        .eval(&format!("use ppp *; $in | from json | {reader} | into event --source https://example.edu/feed"), Value::test_string(json))
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

fn tribe() -> Vec<Json> {
    events(
        &fixture("tribe-page.json"),
        "get events | from tribe-events",
    )
}

fn by_summary<'a>(events: &'a [Json], prefix: &str) -> &'a Json {
    events
        .iter()
        .find(|e| e["summary"].as_str().unwrap().starts_with(prefix))
        .unwrap()
}

#[test]
fn every_tribe_event_is_valid() {
    assert_eq!(tribe().len(), 6);
}

#[test]
fn tribe_times_venue_and_organizer() {
    let events = tribe();
    let netvue = by_summary(&events, "NetVue");
    assert_eq!(netvue["tz"], "America/Chicago");
    assert!(netvue["start"].as_str().unwrap().starts_with("2026-10-1"));
    let place = &netvue["location"]["place"];
    assert_eq!(place["address"]["country"], "US");
    assert!(place["geo"]["lat"].is_f64(), "{place}");
    assert!(netvue["organizers"][0]["org"]["name"].is_string());
    assert_eq!(
        netvue["id"],
        "wp.stolaf.edu/calendar?id=".to_string()
            + netvue["id"].as_str().unwrap().rsplit('=').next().unwrap()
    );
}

#[test]
fn tribe_all_day_events_end_exclusively() {
    let events = tribe();
    let visit = by_summary(&events, "Admissions: Fall Visit Days");
    assert_eq!(visit["all_day"], true);
    // Tribe says the last day ends at 23:59:59; events end exclusively.
    assert_eq!(visit["start"], "2026-10-14");
    assert_eq!(visit["end"], "2026-10-17");
}

#[test]
fn tribe_text_is_plain() {
    let events = tribe();
    assert!(by_summary(&events, "Men’s Soccer").is_object());
    let recital = by_summary(&events, "Student Recital");
    assert_eq!(recital["cost"], json!({ "free": true }));
    assert_eq!(recital["content"]["type"], "html");
    assert!(!recital["description"].as_str().unwrap_or("").contains('<'));
}

#[test]
fn tribe_websites_are_cleaned() {
    let events = tribe();
    for event in &events {
        for link in event["links"].as_array().into_iter().flatten() {
            let url = link["url"].as_str().unwrap();
            assert!(!url.contains("&amp;") && !url.ends_with("%20"), "{url}");
        }
    }
}

#[test]
fn tribe_virtual_urls_without_a_scheme_get_https() {
    let page: Json = serde_json::from_str(&fixture("tribe-page.json")).unwrap();
    let event = |id: u32, virtual_url: Json| {
        let mut event = page["events"][0].clone();
        event["id"] = json!(id);
        event["global_id"] = json!(format!("event-{id}"));
        event["virtual_url"] = virtual_url;
        event
    };
    let feed = json!([
        event(1, json!("zoom.us/j/123")),
        event(2, json!("http://meet.example.edu/x")),
        event(3, Json::Null)
    ]);
    let online: Vec<Json> = events(&feed.to_string(), "from tribe-events")
        .iter()
        .map(|e| e["online"].clone())
        .collect();
    assert_eq!(
        online,
        [
            json!({ "url": "https://zoom.us/j/123" }),
            json!({ "url": "http://meet.example.edu/x" }),
            Json::Null
        ]
    );
}

#[test]
fn presence_events() {
    let events = events(
        &fixture("presence.json"),
        "from presence --photos https://stolaf-cdn.presence.io/event-photos/09ddef77-5009-4348-8540-c9bfc6ade6bc/",
    );
    assert_eq!(events.len(), 4);
    let first = &events[0];
    assert_eq!(first.get("tz"), None);
    assert!(first["start"].as_str().unwrap().ends_with('Z'));
    assert!(
        first["url"]
            .as_str()
            .unwrap()
            .starts_with("https://stolaf.presence.io/event/")
    );
    assert!(
        first["images"][0]["url"]
            .as_str()
            .unwrap()
            .starts_with("https://stolaf-cdn.presence.io/event-photos/")
    );
    assert!(first["organizers"][0]["org"]["name"].is_string());
    assert_eq!(
        events
            .iter()
            .filter(|e| e["organizers"].as_array().unwrap().len() == 1)
            .count(),
        1
    );
}

#[test]
fn presence_search_photos_are_left_out() {
    // Photos chosen through Presence's image search aren't under the
    // event-photos path, and Presence's own link to them 404s too.
    let feed: Json = serde_json::from_str(&fixture("presence.json")).unwrap();
    let event = |id: &str, photo_type: Option<&str>| {
        let mut event = feed[0].clone();
        event["eventNoSqlId"] = json!(id);
        match photo_type {
            Some(kind) => event["photoType"] = json!(kind),
            None => {
                event.as_object_mut().unwrap().remove("photoType");
            }
        }
        event
    };
    let feed = json!([
        event("upload", Some("upload")),
        event("search", Some("search")),
        event("untyped", None)
    ]);
    let image_counts: Vec<usize> = events(
        &feed.to_string(),
        "from presence --photos https://cdn.example.edu/photos/",
    )
    .iter()
    .map(|e| e["images"].as_array().map_or(0, Vec::len))
    .collect();
    assert_eq!(image_counts, [1, 0, 1]);
}

#[test]
fn uniq_events_compares_starts_as_moments() {
    let mut engine = Engine::new();
    let source = "use ppp *; [
        {summary: Concert, start: 2026-10-14T19:30:00-05:00, tz: America/Chicago}
        {summary: Concert, start: 2026-10-15T00:30:00Z, tz: America/Chicago}
        {summary: 'Fall Break', start: 2026-10-10T00:00:00-05:00, all_day: true, tz: America/Chicago}
        {summary: 'Fall Break', start: 2026-10-10T05:00:00Z, all_day: true, tz: America/Chicago}
        {summary: Concert, start: 2026-10-15T19:30:00-05:00, tz: America/Chicago}
    ] | uniq-events | length";
    let count = engine.eval(source, Value::test_nothing()).unwrap();
    assert_eq!(count.as_int().unwrap(), 3);
}

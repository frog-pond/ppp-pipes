//! The acceptance pipes in `pipes/`, run against saved copies of their
//! sources served by a local HTTP server, with a fixed window.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nu_protocol::{Record, Value, record};
use ppp_core::host::{Host, HostConfig};
use ppp_core::pipe::{Pipe, check_records};
use serde_json::Value as Json;
use support::{Reply, reply, serve};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo(path)).unwrap()
}

fn json_reply(body: String) -> Reply {
    Reply {
        status: "200 OK",
        headers: vec![("Content-Type", "application/json".into())],
        body: body.into_bytes(),
    }
}

/// Runs `pipes/<name>.nu` with `args`, checks its records, and returns them as JSON.
fn run(name: &str, args: Record) -> Result<Vec<Json>, String> {
    let host = Arc::new(Host::new(HostConfig {
        pipes_dir: repo("pipes"),
        ..HostConfig::default()
    }));
    let mut pipe = Pipe::load_with(&repo(&format!("pipes/{name}.nu")), host.clone())
        .map_err(|e| e.to_string())?;
    let records = pipe.run(&args).map_err(|e| e.to_string())?;
    check_records(&host.registry().unwrap(), pipe.info(), &records)
        .map_err(|errors| format!("{errors:#?}"))
}

fn window(source: &str) -> Record {
    record! {
        "source" => Value::test_string(source),
        "from" => Value::test_date("2026-09-01T00:00:00Z".parse().unwrap()),
        "within" => Value::test_duration(5 * 365 * 24 * 3600 * 1_000_000_000),
    }
}

#[test]
fn stolaf_wp_follows_pages() {
    let page: Json = serde_json::from_str(&read("fixtures/json/tribe-page.json")).unwrap();
    let events = page["events"].as_array().unwrap().clone();
    let second = serve(vec![json_reply(
        serde_json::json!({ "events": events[3..] }).to_string(),
    )]);
    let first = serve(vec![json_reply(
        serde_json::json!({ "events": events[..3], "next_rest_url": second.url }).to_string(),
    )]);
    let records = run("stolaf-wp", window(&first.url)).unwrap();
    assert_eq!(records.len(), 6);
    let request = &first.requests.lock().unwrap()[0];
    // Asks for events still running when the window opens (padded a day,
    // since the API compares whole days), not just those starting in it.
    assert!(request.contains("ends_after=2026-08-30"), "{request}");
    assert!(request.contains("starts_before="), "{request}");
}

#[test]
fn stolaf_wp_refuses_more_pages_than_its_cap() {
    let page = json_reply(r#"{"events": [], "next_rest_url": "http://127.0.0.1:9/again"}"#.into());
    let server = serve(vec![page]);
    let mut args = window(&server.url);
    args.push("max-pages", Value::test_int(1));
    let err = run("stolaf-wp", args).unwrap_err();
    assert!(
        err.contains("more than 1 pages; raise --max-pages"),
        "{err}"
    );
}

#[test]
fn presence_reads_its_api() {
    let server = serve(vec![json_reply(read("fixtures/json/presence.json"))]);
    let records = run("presence", window(&server.url)).unwrap();
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|e| {
        e["images"][0]["url"]
            .as_str()
            .unwrap()
            .contains("presence.io/event-photos/")
    }));
}

#[test]
fn carleton_week_cleans_up_its_feed() {
    let server = serve(vec![reply(
        "200 OK",
        &[("Content-Type", "text/calendar")],
        &read("fixtures/ics/carleton.ics"),
    )]);
    let records = run("carleton-week", window(&server.url)).unwrap();
    // The fixture has seven events, two of them the same event.
    assert_eq!(records.len(), 6);
    assert!(records.iter().any(|e| e["content"]["type"] == "html"));
    for event in &records {
        if let Some(url) = event["url"].as_str() {
            assert!(!url.contains("&amp;") && !url.contains('\\'), "{url}");
        }
    }
}

#[test]
fn northfield_links_each_event_to_its_page() {
    let server = serve(vec![reply(
        "200 OK",
        &[],
        &read("fixtures/ics/northfield.ics"),
    )]);
    let mut args = window(&server.url);
    args.insert(
        "within",
        Value::test_duration(20 * 365 * 24 * 3600 * 1_000_000_000),
    );
    let records = run("northfield", args).unwrap();
    assert_eq!(records.len(), 2);
    for event in &records {
        assert!(
            event["url"]
                .as_str()
                .unwrap()
                .starts_with("https://www.northfieldmn.gov/calendar.aspx?EID="),
            "{event}"
        );
        assert_eq!(event["location"]["name"], "In your neighborhood");
        assert!(!event["description"].as_str().unwrap().starts_with(' '));
    }
}

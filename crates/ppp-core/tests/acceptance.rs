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

#[test]
fn carleton_week_adds_pictures_from_its_page() {
    let feed = serve(vec![reply(
        "200 OK",
        &[],
        &read("fixtures/ics/carleton.ics"),
    )]);
    let page = serve(vec![reply(
        "200 OK",
        &[("Content-Type", "text/html")],
        &read("fixtures/html/carleton-calendar.html"),
    )]);
    let mut args = window(&feed.url);
    args.push("page", Value::test_string(page.url.clone()));
    let records = run("carleton-week", args).unwrap();
    let pictured: Vec<&Json> = records
        .iter()
        .filter(|e| e.get("images").is_some())
        .collect();
    assert!(!pictured.is_empty());
    for event in &pictured {
        let url = event["images"][0]["url"].as_str().unwrap();
        // The page shows a 100x100 crop; the record links the original.
        assert!(
            url.starts_with("https://cdn.carleton.edu/") && !url.contains('?'),
            "{url}"
        );
        assert_eq!(event["images"][0]["alt"], event["summary"]);
    }
    // The window runs from 2026-09-01T00:00Z for five 365-day years,
    // given as Chicago dates.
    let request = &page.requests.lock().unwrap()[0];
    assert!(
        request.contains("start_date=2026-08-31&end_date=2031-08-30"),
        "{request}"
    );
}

#[test]
fn carleton_week_keeps_the_query_its_page_already_has() {
    let feed = serve(vec![reply(
        "200 OK",
        &[],
        &read("fixtures/ics/carleton.ics"),
    )]);
    let page = serve(vec![reply(
        "200 OK",
        &[("Content-Type", "text/html")],
        &read("fixtures/html/carleton-calendar.html"),
    )]);
    let mut args = window(&feed.url);
    args.push(
        "page",
        Value::test_string(format!("{}?view=list&start_date=1999-01-01", page.url)),
    );
    let records = run("carleton-week", args).unwrap();
    assert!(records.iter().any(|e| e.get("images").is_some()));
    let request = &page.requests.lock().unwrap()[0];
    let target = request.lines().next().unwrap();
    assert_eq!(
        target,
        "get /feed?view=list&start_date=2026-08-31&end_date=2031-08-30 http/1.1"
    );
}

#[test]
fn carleton_week_resolves_relative_pictures_and_skips_inline_ones() {
    let feed = serve(vec![reply(
        "200 OK",
        &[],
        &read("fixtures/ics/carleton.ics"),
    )]);
    let html = read("fixtures/html/carleton-calendar.html")
        .replace(
            "https://cdn.carleton.edu/uploads/sites/513/",
            "/uploads/sites/513/",
        )
        .replace(
            "https://cdn.carleton.edu/uploads/sites/624/2023/05/ptm-logo.png",
            "data:image/gif;base64,R0lGODlhAQABAAAAACw=",
        );
    let page = serve(vec![reply(
        "200 OK",
        &[("Content-Type", "text/html")],
        &html,
    )]);
    let mut args = window(&feed.url);
    args.push("page", Value::test_string(page.url.clone()));
    let records = run("carleton-week", args).unwrap();
    assert_eq!(records.len(), 6);
    let urls: Vec<&str> = records
        .iter()
        .filter_map(|e| e["images"][0]["url"].as_str())
        .collect();
    assert!(
        urls.iter()
            .any(|u| u.starts_with("http://")
                && u.ends_with("/uploads/sites/513/2026/09/CACTS-map.jpg")),
        "{urls:?}"
    );
    assert!(urls.iter().all(|u| !u.starts_with("data:")), "{urls:?}");
}

#[test]
fn carleton_week_without_its_page_still_has_its_events() {
    let feed = serve(vec![reply(
        "200 OK",
        &[],
        &read("fixtures/ics/carleton.ics"),
    )]);
    let page = serve(vec![reply("503 Service Unavailable", &[], "")]);
    let host = Arc::new(Host::new(HostConfig {
        pipes_dir: repo("pipes"),
        ..HostConfig::default()
    }));
    let mut pipe = Pipe::load_with(&repo("pipes/carleton-week.nu"), host.clone()).unwrap();
    let mut args = window(&feed.url);
    args.push("page", Value::test_string(page.url.clone()));
    let records = pipe.run(&args).unwrap();
    assert_eq!(records.len(), 6);
    let warnings = host.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("Carleton event pictures are missing")
                && w.contains(&page.url)
                && w.contains("returned HTTP 503")),
        "{warnings:?}"
    );
}

#[test]
fn all_campus_keeps_going_when_a_source_fails() {
    let dir = std::env::temp_dir().join(format!("ppp-all-campus-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(repo("pipes/all-campus.nu"), dir.join("all-campus.nu")).unwrap();
    let working = read("fixtures/pipes/two-events.nu").replace(
        "--count: int = 2          # How many events to return",
        "--count: int = 2\n    --within: duration\n    --from: datetime",
    );
    let failing = read("fixtures/pipes/fails.nu").replace(
        "export def main []",
        "export def main [--within: duration, --from: datetime]",
    );
    for name in ["stolaf-wp", "campus-week", "carleton-week", "northfield"] {
        std::fs::write(dir.join(format!("{name}.nu")), &working).unwrap();
    }
    std::fs::write(dir.join("presence.nu"), failing).unwrap();

    let host = Arc::new(Host::new(HostConfig {
        pipes_dir: dir.clone(),
        ..HostConfig::default()
    }));
    let mut pipe = Pipe::load_with(&dir.join("all-campus.nu"), host.clone()).unwrap();
    let records = pipe.run(&Record::new());
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(records.unwrap().len(), 2);
    let warnings = host.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("presence was left out") && w.contains("the source is down")),
        "{warnings:?}"
    );
    // Warnings are read in a terminal or a log; colour codes would be noise.
    assert!(warnings.iter().all(|w| !w.contains('\x1b')), "{warnings:?}");
}

#[test]
fn carleton_week_with_an_unreadable_page_url_still_has_its_events() {
    let feed = serve(vec![reply(
        "200 OK",
        &[],
        &read("fixtures/ics/carleton.ics"),
    )]);
    let host = Arc::new(Host::new(HostConfig {
        pipes_dir: repo("pipes"),
        ..HostConfig::default()
    }));
    let mut pipe = Pipe::load_with(&repo("pipes/carleton-week.nu"), host.clone()).unwrap();
    let mut args = window(&feed.url);
    args.push("page", Value::test_string("not a url"));
    assert_eq!(pipe.run(&args).unwrap().len(), 6);
    let warnings = host.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("Carleton event pictures are missing")),
        "{warnings:?}"
    );
}

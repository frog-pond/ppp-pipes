//! What a run shares through its `Host`: our `http get`, `pipe get`,
//! the timeout, and the pipe's `refresh`.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nu_protocol::{Record, Value, record};
use ppp_core::engine::Engine;
use ppp_core::host::{Host, HostConfig};
use ppp_core::pipe::{Pipe, PipeError};
use support::{reply, serve, temp_dir};

const ICS: &str = "BEGIN:VCALENDAR\r\nX-WR-TIMEZONE:America/Chicago\r\nBEGIN:VEVENT\r\nUID:a@example.edu\r\nSUMMARY:Chess Club\r\nDTSTART:20261013T180000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

fn pipes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pipes")
}

fn host(cache: Option<&str>) -> Arc<Host> {
    Arc::new(Host::new(HostConfig {
        cache_dir: cache.map(temp_dir),
        pipes_dir: pipes_dir(),
        ..HostConfig::default()
    }))
}

fn eval(engine: &mut Engine, source: &str) -> String {
    let value = engine.eval(source, Value::test_nothing()).unwrap();
    engine
        .eval("$in | to nuon", value)
        .unwrap()
        .coerce_into_string()
        .unwrap()
}

#[test]
fn http_get_parses_json_by_content_type() {
    let server = serve(vec![
        reply(
            "200 OK",
            &[("Content-Type", "application/json")],
            r#"{"events": [{"id": 1}], "next": null}"#,
        ),
        reply(
            "200 OK",
            &[("Content-Type", "application/json")],
            r#"{"a": 1}"#,
        ),
    ]);
    let mut engine = Engine::with_host(host(None));
    assert_eq!(
        eval(&mut engine, &format!("http get {}", server.url)),
        "{events: [[id]; [1]], next: null}"
    );
    assert_eq!(
        eval(&mut engine, &format!("http get --raw {}", server.url)),
        r#""{\"a\": 1}""#
    );
}

#[test]
fn an_empty_json_response_is_nothing() {
    let server = serve(vec![
        reply("200 OK", &[("Content-Type", "application/json")], ""),
        reply("200 OK", &[("Content-Type", "application/json")], " \r\n"),
    ]);
    let mut engine = Engine::with_host(host(None));
    for _ in 0..2 {
        assert_eq!(
            eval(&mut engine, &format!("http get {} | describe", server.url)),
            r#""nothing""#
        );
    }
}

#[test]
fn http_get_returns_text_or_binary() {
    let server = serve(vec![
        reply(
            "200 OK",
            &[("Content-Type", "text/calendar")],
            "BEGIN:VCALENDAR",
        ),
        support::Reply {
            status: "200 OK",
            headers: vec![],
            body: vec![0xff, 0x00],
        },
    ]);
    let mut engine = Engine::with_host(host(None));
    assert_eq!(
        eval(&mut engine, &format!("http get {} | describe", server.url)),
        r#""string""#
    );
    assert_eq!(
        eval(&mut engine, &format!("http get {} | describe", server.url)),
        r#""binary""#
    );
}

#[test]
fn http_get_refuses_other_schemes() {
    let mut engine = Engine::with_host(host(None));
    let err = engine
        .eval("http get file:///etc/hosts", Value::test_nothing())
        .unwrap_err();
    assert!(
        err.0.contains("only http and https URLs can be fetched"),
        "{err}"
    );
}

#[test]
fn http_errors_are_engine_errors() {
    let server = serve(vec![reply("503 Service Unavailable", &[], "")]);
    let mut engine = Engine::with_host(host(None));
    let err = engine
        .eval(&format!("http get {}", server.url), Value::test_nothing())
        .unwrap_err();
    assert!(err.0.contains("returned HTTP 503"), "{err}");
}

#[test]
fn a_pipes_refresh_keeps_its_fetches_fresh() {
    let server = serve(vec![reply("200 OK", &[], ICS)]);
    let shared = host(Some("host-refresh"));
    let args = record! { "url" => Value::test_string(server.url.clone()) };
    let mut pipe = Pipe::load_with(&pipes_dir().join("fetches.nu"), shared.clone()).unwrap();
    assert_eq!(
        pipe.info().refresh,
        Some(jiff::SignedDuration::from_hours(1))
    );
    assert_eq!(pipe.run(&args).unwrap().len(), 1);
    // The server has served its one reply; the second run must use the cache.
    assert_eq!(pipe.run(&args).unwrap().len(), 1);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn pipe_get_returns_another_pipes_records() {
    let mut pipe = Pipe::load_with(&pipes_dir().join("gets-two-events.nu"), host(None)).unwrap();
    let records = pipe.run(&Record::new()).unwrap();
    assert_eq!(records.len(), 1);
    let summary = records[0]
        .get_data_by_key("summary")
        .unwrap()
        .coerce_into_string()
        .unwrap();
    assert_eq!(summary, "Got: Chess Club");
    // Datetimes stay datetimes.
    assert!(matches!(
        records[0].get_data_by_key("start"),
        Some(Value::Date { .. })
    ));
}

#[test]
fn pipe_get_loops_are_errors() {
    let mut pipe = Pipe::load_with(&pipes_dir().join("cycle-a.nu"), host(None)).unwrap();
    let err = pipe.run(&Record::new()).unwrap_err().to_string();
    assert!(
        err.contains("pipes get each other in a loop: cycle-a → cycle-b → cycle-a"),
        "{err}"
    );
}

#[test]
fn pipe_get_checks_the_other_pipes_records() {
    let mut pipe = Pipe::load_with(&pipes_dir().join("gets-invalid.nu"), host(None)).unwrap();
    let err = pipe.run(&Record::new()).unwrap_err().to_string();
    assert!(err.contains("pipe get invalid-event"), "{err}");
    assert!(err.contains("\"start\" is a required property"), "{err}");
}

#[test]
fn a_run_past_its_time_limit_stops() {
    let shared = host(None);
    let mut pipe = Pipe::load_with(&pipes_dir().join("slow.nu"), shared.clone()).unwrap();
    let started = std::time::Instant::now();
    let result = shared.with_timeout(Duration::from_millis(200), || pipe.run(&Record::new()));
    assert!(matches!(result, Err(PipeError::Engine(_))), "{result:?}");
    assert!(shared.interrupted());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_run_inside_its_time_limit_is_untouched() {
    let shared = host(None);
    let mut pipe = Pipe::load_with(&pipes_dir().join("two-events.nu"), shared.clone()).unwrap();
    let result = shared.with_timeout(Duration::from_secs(30), || pipe.run(&Record::new()));
    assert_eq!(result.unwrap().len(), 2);
    assert!(!shared.interrupted());
}

#[test]
fn pipe_get_names_stay_in_the_pipes_dir() {
    let mut engine = Engine::with_host(host(None));
    for name in ["../outside/escape", "/etc/hosts", "two-events.nu", "a b"] {
        let err = engine
            .eval(&format!("pipe get {name:?}"), Value::test_nothing())
            .unwrap_err();
        assert!(
            err.0
                .contains("pipe names may use only letters, digits, `-` and `_`"),
            "{name}: {err}"
        );
    }
}

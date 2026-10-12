//! nushell's `query web` and our `pipe warn`, inside the engine.

use std::path::Path;
use std::sync::Arc;

use nu_protocol::Value;
use ppp_core::engine::Engine;
use ppp_core::host::Host;

fn page() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/html/carleton-calendar.html");
    Value::test_string(std::fs::read_to_string(path).unwrap())
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
fn query_web_reads_attributes() {
    let mut engine = Engine::new();
    let hrefs = nuon(
        &mut engine,
        r#"$in | query web --query '.event_list--item picture a[href*="eId="]' --attribute href"#,
        page(),
    );
    assert!(hrefs.contains("?eId=uuff"), "{hrefs}");
}

#[test]
fn query_web_can_return_html_to_query_again() {
    let mut engine = Engine::new();
    let src = nuon(
        &mut engine,
        r#"$in | query web --query '.event_list--item picture a[href*="eId="]' --as-html | first | query web --query img --attribute src | first"#,
        page(),
    );
    assert!(src.starts_with("\"https://cdn.carleton.edu/"), "{src}");
}

#[test]
fn pipe_warn_reaches_the_host_and_passes_its_input_on() {
    let host = Arc::new(Host::default());
    let mut engine = Engine::with_host(host.clone());
    let out = engine
        .eval("42 | pipe warn 'the page is down'", Value::test_nothing())
        .unwrap();
    assert_eq!(out.as_int().unwrap(), 42);
    assert_eq!(host.take_warnings(), ["Warning: the page is down"]);
}

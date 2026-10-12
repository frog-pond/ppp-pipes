//! `query html` (the same flags as nushell's `query web`) and `pipe warn`,
//! inside the engine.

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

fn html(text: &str) -> Value {
    Value::test_string(text)
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
fn query_html_reads_attributes() {
    let mut engine = Engine::new();
    let hrefs = nuon(
        &mut engine,
        r#"$in | query html --query '.event_list--item picture a[href*="eId="]' --attribute href"#,
        page(),
    );
    assert!(hrefs.contains("?eId=uuff"), "{hrefs}");
}

#[test]
fn query_html_can_return_html_to_query_again() {
    let mut engine = Engine::new();
    let src = nuon(
        &mut engine,
        r#"$in | query html --query '.event_list--item picture a[href*="eId="]' --as-html | first | query html --query img --attribute src | first"#,
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

const LIST: &str = "<ul><li>Coffee</li><li>Tea</li><li>Milk</li></ul>";

#[test]
fn query_html_returns_each_matchs_text_nodes() {
    let mut engine = Engine::new();
    let text = nuon(
        &mut engine,
        "$in | query html --query 'li:first-child'",
        html(LIST),
    );
    assert_eq!(text, "[[Coffee]]");
    let nested = nuon(
        &mut engine,
        "$in | query html -q p",
        html(r#"<p>Hello there, <span>World</span></p>"#),
    );
    assert_eq!(nested, r#"[["Hello there, ", World]]"#);
}

#[test]
fn query_html_returns_matches_as_html() {
    let mut engine = Engine::new();
    let out = nuon(
        &mut engine,
        "$in | query html --query li --as-html",
        html(LIST),
    );
    assert_eq!(out, "[<li>Coffee</li>, <li>Tea</li>, <li>Milk</li>]");
}

#[test]
fn query_html_reads_several_attributes_into_records() {
    let mut engine = Engine::new();
    let out = nuon(
        &mut engine,
        "$in | query html --query a --attribute [href target]",
        html(
            r#"<a href="https://example.org" target="_blank">x</a><a href="https://example.com">y</a>"#,
        ),
    );
    assert_eq!(
        out,
        r#"[[href, target]; ["https://example.org", _blank], ["https://example.com", ""]]"#
    );
}

#[test]
fn query_html_finds_tables_by_their_headers() {
    let mut engine = Engine::new();
    let out = nuon(
        &mut engine,
        "$in | query html --as-table [name type]",
        html(
            "<table><tr><th>name</th><th>type</th></tr><tr><td>a</td><td>dir</td></tr><tr><td>b</td><td>file</td></tr></table>",
        ),
    );
    assert_eq!(out, "[[name, type]; [a, dir], [b, file]]");
}

#[test]
fn query_html_reads_a_whole_document_only_when_asked() {
    let mut engine = Engine::new();
    let page = "<html><head><title>x</title></head><body><p>foo</p></body></html>";
    assert_eq!(
        nuon(&mut engine, "$in | query html --query body", html(page)),
        "[]"
    );
    assert_eq!(
        nuon(
            &mut engine,
            "$in | query html --document --query body",
            html(page)
        ),
        "[[foo]]"
    );
}

#[test]
fn query_html_inspects_short_pages_without_crashing() {
    let host = Arc::new(Host::default());
    let mut engine = Engine::with_host(host.clone());
    engine
        .eval("$in | query html --as-table [name] --inspect", html(LIST))
        .unwrap();
    // Inspect output is for the pipe's author, so it goes where warnings go.
    let warnings = host.take_warnings();
    assert!(warnings.iter().any(|w| w.contains("<ul>")), "{warnings:?}");
}

#[test]
fn query_html_rejects_a_bad_selector() {
    let mut engine = Engine::new();
    let err = engine
        .eval("$in | query html --query 'li[['", html(LIST))
        .unwrap_err();
    assert!(err.0.contains("CSS query parse error"), "{}", err.0);
}

#[test]
fn query_html_needs_text() {
    let mut engine = Engine::new();
    let err = engine
        .eval("42 | query html --query li", Value::test_nothing())
        .unwrap_err();
    assert!(err.0.contains("Requires text input"), "{}", err.0);
}

//! The plugin's commands, run in-process through `nu-plugin-test-support`:
//! the same inputs and results as the engine's commands.

use std::path::{Path, PathBuf};

use nu_plugin_pipes::PipesPlugin;
use nu_plugin_test_support::PluginTest;
use nu_protocol::{PipelineData, Span, Value};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

/// The plugin in a test engine. The test engine has only nushell's language
/// commands, so the data commands the tests use are added.
fn plugin() -> PluginTest {
    let mut test = PluginTest::new("pipes", PipesPlugin::default().into()).unwrap();
    test.add_decl(Box::new(nu_command::Where)).unwrap();
    test.add_decl(Box::new(nu_command::Length)).unwrap();
    test
}

fn eval(test: &mut PluginTest, source: &str, input: Value) -> Result<Value, String> {
    match test.eval_with(source, PipelineData::Value(input, None)) {
        Ok(out) => out
            .into_value(Span::test_data())
            .map_err(|e| format!("{e:?}")),
        Err(err) => Err(format!("{err:?}")),
    }
}

fn ics(name: &str) -> Value {
    Value::test_string(std::fs::read_to_string(repo(&format!("fixtures/ics/{name}"))).unwrap())
}

fn summaries(value: &Value) -> Vec<String> {
    value
        .as_list()
        .unwrap()
        .iter()
        .map(|row| {
            row.get_data_by_key("summary")
                .unwrap()
                .coerce_into_string()
                .unwrap()
        })
        .collect()
}

#[test]
fn from_ics_reads_events() {
    let rows = eval(&mut plugin(), "from ics", ics("campus-events.ics")).unwrap();
    assert_eq!(
        summaries(&rows),
        [
            "Fall Orchestra Concert",
            "Family Weekend",
            "Chess Club",
            "Guest Lecture: Pipelines Everywhere"
        ]
    );
}

#[test]
fn from_ics_errors_are_labeled() {
    let err = eval(&mut plugin(), "from ics", Value::test_string("<html>")).unwrap_err();
    assert!(err.contains("cannot read iCalendar text"), "{err}");
}

#[test]
fn expand_rrule_expands() {
    let chess = eval(
        &mut plugin(),
        "from ics | expand rrule --from 2026-11-01T00:00:00Z --until 2026-12-01T00:00:00Z | where summary == 'Chess Club' | length",
        ics("campus-events.ics"),
    )
    .unwrap();
    assert_eq!(chess.as_int().unwrap(), 3);
}

#[test]
fn html_to_text_strips_tags() {
    let text = eval(
        &mut plugin(),
        "html to-text",
        Value::test_string("<p>Men&#8217;s <b>Soccer</b></p>"),
    )
    .unwrap();
    assert_eq!(text.coerce_into_string().unwrap(), "Men’s Soccer");
}

#[test]
fn validate_and_to_pipes_json() {
    let event = "{type: event, id: a, summary: Concert, start: 2026-10-14T19:30:00-05:00, tz: America/Chicago}";
    let mut test = plugin();
    let valid = eval(
        &mut test,
        &format!("[{event}] | validate"),
        Value::test_nothing(),
    )
    .unwrap();
    assert_eq!(summaries(&valid), ["Concert"]);
    let json = eval(
        &mut test,
        &format!("[{event}] | to pipes-json"),
        Value::test_nothing(),
    )
    .unwrap();
    assert!(
        json.coerce_into_string()
            .unwrap()
            .contains("\"start\": \"2026-10-14T19:30:00\"")
    );
    let err = eval(
        &mut test,
        "[{type: event, id: a}] | validate",
        Value::test_nothing(),
    )
    .unwrap_err();
    assert!(err.contains("records are not valid"), "{err}");
}

#[test]
fn query_html_queries_html() {
    let out = eval(
        &mut plugin(),
        "query html --query 'li:first-child'",
        Value::test_string("<ul><li>Coffee</li><li>Tea</li></ul>"),
    )
    .unwrap();
    assert_eq!(
        out,
        Value::test_list(vec![Value::test_list(vec![Value::test_string("Coffee")])])
    );
}

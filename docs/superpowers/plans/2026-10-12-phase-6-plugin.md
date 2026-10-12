# ppp-pipes Phase 6: The nushell Plugin — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `nu_plugin_pipes` gives stock `nu` our commands (`from ics`, `expand rrule`, `html to-text`, `validate`, `to pipes-json`, `pipe get`), so pipe files run unchanged outside `ppp` and the editor's LSP knows the commands.

**Architecture:** Each command's work becomes a plain function in `ppp-core` (`from_ics::read`, `expand_rrule::expand_values`, `html::to_text`, `validate::check`, `to_pipes_json::render`, `pipe_get::get`) returning a small `CommandError`. The engine's `Command` and the plugin's `SimplePluginCommand` are each a few lines around it; the plugin's wrappers borrow name, signature and description from the engine's command. `validate` and `to pipes-json` (spec §4.2) are new in both.

**Tech Stack:** nu-plugin 0.116.1, nu-plugin-test-support 0.116.1.

**Spec:** [`docs/superpowers/specs/2026-10-11-ppp-pipes-cli-prototype-design.md`](../specs/2026-10-11-ppp-pipes-cli-prototype-design.md) §4.2, §4.3, §4.5 — build-order step 6. §4.3 and §4.5 were updated in the same commit as this plan.

Every code block was compiled and tested in a scratch copy of the phase 5 branch: 200 tests pass, and the three ignored end-to-end tests pass in a real `nu` 0.116.1.

## Global Constraints

- Everything in the earlier phases' Global Constraints still holds.
- The plugin is pinned to nushell 0.116.1, like the engine; it loads only into that `nu`.

## Decisions made while planning

Wren was away and asked for the phases to continue; these are mine to review.

1. **`CommandError`** (title + message) is what the shared functions return, rather than `ShellError` (192 bytes, which clippy flags) or `LabeledError`.
2. **The plugin's `pipe get` takes `--pipes-dir`** (default `./pipes`, relative to nu's current directory), caches in the user cache directory, and allows 60 seconds.
3. **In-process plugin tests add `where` and `length`** to `PluginTest`'s engine (it has only the language commands), so tests read like real pipelines — Wren's suggestion.
4. **End-to-end tests are ignored by default:** they need `nu` 0.116.1 (set `PPP_TEST_NU`). This machine's `nu` on PATH is the Nix 0.115.1; I built 0.116.1 into the session scratchpad to run them.
5. **Found, not fixed — needs Wren:** typed at a prompt in the repository root, `use pipes *` finds the repo's `pipes/` directory (the acceptance pipes) before the library. Pipe files are unaffected. Fixing it means renaming the module or the directory.

## Review Focus

1. **A pipe file run in stock `nu`** must behave as under `ppp run` (`pipe_files_run_unchanged`) — Task 2.
2. **Plugin and engine commands must not drift:** same name, signature and description (the wrappers borrow them) — Task 2.
3. **`validate` on an unknown record type** must say so, not claim a declared-type mismatch — Task 1.
4. **`to pipes-json` on a single record** gives an object, on a table an array — Task 1.
5. **The plugin's `pipe get`** must use the sandboxed engine (no filesystem commands) even though stock `nu` has them — Task 2.

---

### Task 1: Shared command functions, `validate` and `to pipes-json`

**Files:** Modify `crates/ppp-core/src/commands/{mod,from_ics,expand_rrule,pipe_get}.rs`, `crates/ppp-core/src/pipe.rs` (`check_values`); Create `crates/ppp-core/src/commands/{validate,to_pipes_json}.rs`; Test `crates/ppp-core/tests/record_commands.rs`, `crates/ppp-core/tests/engine.rs` (collision list).

**Interfaces:** Produces `commands::{CommandError { title, message }, describe_failures}`, `from_ics::read(Value, Option<&str>, Span) -> Result<Value, CommandError>`, `expand_rrule::expand_values(Value, &Value, &Value, Span)`, `pipe_get::get(&Arc<Host>, &str, Record, Span)`, `validate::check(&Registry, Value)`, `to_pipes_json::render(&Registry, Value, Span)`, `pipe::check_values(&Registry, Option<&[String]>, &[Value])`, and the `validate` and `to pipes-json` engine commands.

- [ ] **Step 1: Write the failing test**

`crates/ppp-core/tests/record_commands.rs`:

```rust
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
```

`crates/ppp-core/tests/engine.rs`:

```rust
//! The sandboxed engine: evaluation, the embedded `pipes` module, and the
//! command set.

use std::path::Path;

use nu_protocol::Value;
use ppp_core::engine::Engine;

fn nuon(engine: &mut Engine, source: &str, input: Value) -> String {
    let out = engine.eval(source, input).unwrap();
    engine
        .eval("$in | to nuon", out)
        .unwrap()
        .coerce_into_string()
        .unwrap()
}

#[test]
fn evaluates_with_pipeline_input() {
    let mut engine = Engine::new();
    let out = nuon(
        &mut engine,
        "$in | lines | each { str uppercase }",
        Value::test_string("a\nb"),
    );
    assert_eq!(out, "[A, B]");
}

#[test]
fn definitions_persist_between_calls() {
    let mut engine = Engine::new();
    engine
        .eval("def twice [] { $in * 2 }", Value::test_nothing())
        .unwrap();
    assert_eq!(nuon(&mut engine, "21 | twice", Value::test_nothing()), "42");
}

#[test]
fn the_pipes_module_is_embedded() {
    let mut engine = Engine::new();
    engine.eval("use pipes *", Value::test_nothing()).unwrap();
}

#[test]
fn externals_and_filesystem_commands_are_not_registered() {
    let mut engine = Engine::new();
    for name in [
        "run-external",
        "open",
        "save",
        "cd",
        "rm",
        "ls",
        "mkdir",
        "cp",
        "mv",
        "source",
        "exec",
        "plugin add",
    ] {
        assert!(!engine.has_command(name), "{name} is registered");
    }
    for source in ["^ls", "open /etc/hosts", "'x' | save /tmp/ppp-sandbox-test"] {
        assert!(
            engine.eval(source, Value::test_nothing()).is_err(),
            "{source} ran"
        );
    }
    assert!(!Path::new("/tmp/ppp-sandbox-test").exists());
}

#[test]
fn our_commands_do_not_shadow_nushell_builtins() {
    let full = nu_command::add_shell_command_context(nu_cmd_lang::create_default_context());
    for name in [
        "from ics",
        "from tribe-events",
        "from presence",
        "html to-text",
        "to pipes-json",
        "validate",
        "pipe get",
        "expand rrule",
        "into event",
        "drop-empty",
        "during",
        "uniq-events",
    ] {
        assert!(
            full.find_decl(name.as_bytes(), &[]).is_none(),
            "nushell already has {name}"
        );
    }
}

#[test]
fn errors_are_rendered_with_their_source() {
    let mut engine = Engine::new();
    let err = engine
        .eval("[1 2] | each { $in + 'x' }", Value::test_nothing())
        .unwrap_err();
    assert!(err.0.contains("Error:"), "{err}");
    assert!(err.0.contains("$in + 'x'"), "{err}");
}

#[test]
fn the_working_directory_is_the_root() {
    let mut engine = Engine::new();
    assert_eq!(
        nuon(&mut engine, "$env.PWD", Value::test_nothing()),
        r#""/""#
    );
}

#[test]
fn no_registered_command_is_deprecated() {
    let mut engine = Engine::new();
    let deprecated = nuon(
        &mut engine,
        "scope commands | where type == built-in | where { $in.deprecation_info | where type == Command | is-not-empty } | get name",
        Value::test_nothing(),
    );
    assert_eq!(deprecated, "[]");
}

#[test]
fn parse_warnings_are_kept() {
    let mut engine = Engine::new();
    engine
        .eval(
            "@deprecated \"use new\"\ndef old [] { 1 }\nold",
            Value::test_nothing(),
        )
        .unwrap();
    let warnings = engine.take_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("old is deprecated"), "{}", warnings[0]);
    assert!(engine.take_warnings().is_empty());
}

#[test]
fn the_same_source_can_run_twice() {
    // nushell's parser caches blocks by source; a cached block came back
    // without its compiled form, so the second run failed.
    let mut engine = Engine::new();
    for i in 0..3 {
        let out = engine.eval("$in | to nuon", Value::test_int(i)).unwrap();
        assert_eq!(out.coerce_into_string().unwrap(), i.to_string());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test record_commands`
Expected: all 4 FAIL — `validate` and `to pipes-json` aren't registered.

- [ ] **Step 3: Write the code**

`crates/ppp-core/src/commands/mod.rs`:

```rust
//! Our own nushell commands.

use std::sync::{Arc, Mutex};

use jiff::SignedDuration;
use nu_protocol::engine::StateWorkingSet;
use nu_protocol::shell_error::generic::GenericError;
use nu_protocol::{LabeledError, ShellError, Span};

use crate::host::Host;
use crate::schema::RecordError;

pub mod expand_rrule;
pub mod from_ics;
mod html_to_text;
mod http_get;
pub mod pipe_get;
pub mod to_pipes_json;
pub mod validate;

pub use expand_rrule::ExpandRrule;
pub use from_ics::FromIcs;
pub use html_to_text::HtmlToText;
pub use http_get::HttpGet;
pub use pipe_get::PipeGet;
pub use to_pipes_json::ToPipesJson;
pub use validate::Validate;

/// What our commands share with the engine that runs them.
#[derive(Clone)]
pub struct Context {
    pub host: Arc<Host>,
    /// The running pipe's `refresh`, set when the pipe is loaded.
    pub refresh: Arc<Mutex<Option<SignedDuration>>>,
}

/// Registers every command in this module.
pub fn bind(working_set: &mut StateWorkingSet, context: &Context) {
    working_set.add_decl(Box::new(ExpandRrule));
    working_set.add_decl(Box::new(FromIcs));
    working_set.add_decl(Box::new(HtmlToText));
    working_set.add_decl(Box::new(HttpGet {
        context: context.clone(),
    }));
    working_set.add_decl(Box::new(PipeGet {
        context: context.clone(),
    }));
    working_set.add_decl(Box::new(ToPipesJson {
        context: context.clone(),
    }));
    working_set.add_decl(Box::new(Validate {
        context: context.clone(),
    }));
}

/// Why one of our commands failed, in a form both the engine's commands
/// and the plugin's can report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub title: String,
    pub message: String,
}

impl CommandError {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> CommandError {
        CommandError {
            title: title.into(),
            message: message.into(),
        }
    }

    pub fn into_shell_error(self, span: Span) -> ShellError {
        ShellError::Generic(GenericError::new(self.title, self.message, span))
    }

    pub fn into_labeled_error(self, span: Span) -> LabeledError {
        LabeledError::new(self.title).with_label(self.message, span)
    }
}

/// How many record failures an error message shows.
const SHOWN: usize = 5;

/// The first few record failures, one per line.
pub fn describe_failures(errors: &[RecordError]) -> String {
    let mut lines: Vec<String> = errors
        .iter()
        .take(SHOWN)
        .map(|e| format!("record {} {}: {}", e.index, e.pointer, e.message))
        .collect();
    if errors.len() > SHOWN {
        lines.push(format!("…and {} more", errors.len() - SHOWN));
    }
    lines.join("\n")
}
```

`crates/ppp-core/src/commands/from_ics.rs`:

```rust
//! `from ics`: iCalendar text to a table of events (spec §5.2).

use nu_engine::command_prelude::*;

use super::CommandError;
use crate::ics::events::{Event, parse_events};
use crate::ics::lines::Property;
use crate::time;

#[derive(Clone)]
pub struct FromIcs;

impl Command for FromIcs {
    fn name(&self) -> &str {
        "from ics"
    }

    fn description(&self) -> &str {
        "Parse iCalendar text into a table of events, one row per VEVENT."
    }

    fn extra_description(&self) -> &str {
        "Floating times use the calendar's X-WR-TIMEZONE, TIMEZONE-ID or only VTIMEZONE, then --tz. \
         All-day dates with no zone anywhere become midnight UTC."
    }

    fn signature(&self) -> Signature {
        Signature::build("from ics")
            .input_output_types(vec![
                (Type::String, Type::table()),
                (Type::Binary, Type::table()),
            ])
            .named(
                "tz",
                SyntaxShape::String,
                "Zone for floating times when the calendar names none",
                None,
            )
            .category(Category::Formats)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let tz: Option<String> = call.get_flag(engine_state, stack, "tz")?;
        let input = input.into_value(head)?;
        read(input, tz.as_deref(), head)
            .map(|rows| rows.into_pipeline_data())
            .map_err(|err| err.into_shell_error(head))
    }
}

/// Reads iCalendar text (a string, or UTF-8 binary) into a table of events.
pub fn read(input: Value, tz: Option<&str>, head: Span) -> Result<Value, CommandError> {
    let fail = |message: String| CommandError::new("cannot read iCalendar text", message);
    // Feeds served as application/octet-stream arrive as binary.
    let text = match input {
        Value::String { val, .. } => val,
        Value::Binary { val, .. } => String::from_utf8(val.to_vec())
            .map_err(|_| fail("the input is not UTF-8 text".into()))?,
        other => {
            return Err(fail(format!(
                "expected text or binary, got {}",
                other.get_type()
            )));
        }
    };
    let events = parse_events(&text, tz).map_err(|err| fail(err.to_string()))?;
    Ok(Value::list(
        events
            .iter()
            .map(|event| event_to_value(event, head))
            .collect(),
        head,
    ))
}

fn event_to_value(event: &Event, span: Span) -> Value {
    let text = |value: &Option<String>| match value {
        Some(text) => Value::string(text.clone(), span),
        None => Value::nothing(span),
    };
    let date = |value: &Option<jiff::Zoned>| match value {
        Some(zoned) => Value::date(time::to_nu(zoned), span),
        None => Value::nothing(span),
    };
    let instant = |value: &Option<jiff::Timestamp>| match value {
        Some(ts) => Value::date(time::to_nu(&ts.to_zoned(jiff::tz::TimeZone::UTC)), span),
        None => Value::nothing(span),
    };
    let dates = |values: &[jiff::Zoned]| {
        Value::list(
            values
                .iter()
                .map(|z| Value::date(time::to_nu(z), span))
                .collect(),
            span,
        )
    };
    Value::record(
        record! {
            "summary" => text(&event.summary),
            "start" => date(&event.start),
            "end" => date(&event.end),
            "all_day" => Value::bool(event.all_day, span),
            "tz" => text(&event.tz),
            "location" => text(&event.location),
            "description" => text(&event.description),
            "url" => text(&event.url),
            "categories" => Value::list(event.categories.iter().map(|c| Value::string(c.clone(), span)).collect(), span),
            "organizer" => match &event.organizer {
                Some(org) => Value::record(
                    record! {
                        "name" => text(&org.name),
                        "email" => text(&org.email),
                        "cutype" => text(&org.cutype),
                    },
                    span,
                ),
                None => Value::nothing(span),
            },
            "geo" => match event.geo {
                Some((lat, lon)) => Value::record(
                    record! { "lat" => Value::float(lat, span), "lon" => Value::float(lon, span) },
                    span,
                ),
                None => Value::nothing(span),
            },
            "uid" => text(&event.uid),
            "status" => text(&event.status),
            "rrule" => text(&event.rrule),
            "rdates" => dates(&event.rdates),
            "exdates" => dates(&event.exdates),
            "recurrence_id" => date(&event.recurrence_id),
            "created" => instant(&event.created),
            "last_modified" => instant(&event.last_modified),
            "props" => Value::list(event.props.iter().map(|p| property_to_value(p, span)).collect(), span),
        },
        span,
    )
}

fn property_to_value(prop: &Property, span: Span) -> Value {
    let mut params = Record::new();
    for (key, value) in &prop.params {
        params.push(key.clone(), Value::string(value.clone(), span));
    }
    Value::record(
        record! {
            "name" => Value::string(prop.name.clone(), span),
            "params" => Value::record(params, span),
            "value" => Value::string(prop.value.clone(), span),
        },
        span,
    )
}
```

`crates/ppp-core/src/commands/expand_rrule.rs`:

```rust
//! `expand rrule`: one row per occurrence of each recurring event (spec §5.4).

use std::collections::HashSet;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, Zoned};
use nu_engine::command_prelude::*;

use super::CommandError;
use crate::recur::{Series, occurrences};
use crate::time;

#[derive(Clone)]
pub struct ExpandRrule;

impl Command for ExpandRrule {
    fn name(&self) -> &str {
        "expand rrule"
    }

    fn description(&self) -> &str {
        "Replace each recurring event with one row per occurrence that overlaps a window."
    }

    fn extra_description(&self) -> &str {
        "Works on `from ics` rows. A row with `rrule` or `rdates` becomes one row per occurrence \
         overlapping [--from, --until), each with `recurrence_id` set to the occurrence's original \
         start and no `rrule`, `rdates` or `exdates`. An occurrence that has an override row \
         (same `uid`, matching `recurrence_id`) is left out, since the override row stands in \
         for it. Every other row passes through unchanged; filter them with `during`."
    }

    fn signature(&self) -> Signature {
        Signature::build("expand rrule")
            .input_output_types(vec![(Type::table(), Type::table())])
            .required_named("from", SyntaxShape::DateTime, "Start of the window", None)
            .required_named(
                "until",
                SyntaxShape::DateTime,
                "End of the window (exclusive)",
                None,
            )
            .category(Category::Filters)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let from: Value = call
            .get_flag(engine_state, stack, "from")?
            .expect("required flag");
        let until: Value = call
            .get_flag(engine_state, stack, "until")?
            .expect("required flag");
        expand_values(input.into_value(head)?, &from, &until, head)
            .map(|rows| rows.into_pipeline_data())
            .map_err(|err| err.into_shell_error(head))
    }
}

/// Expands the table `input` over [from, until), both nushell datetimes.
pub fn expand_values(
    input: Value,
    from: &Value,
    until: &Value,
    head: Span,
) -> Result<Value, CommandError> {
    let instant = |value: &Value| {
        let date = value.as_date().map_err(|_| {
            CommandError::new("expand rrule", "--from and --until must be datetimes")
        })?;
        time::timestamp(&date)
            .map_err(|_| CommandError::new("date out of range", "outside the years -9999 to 9999"))
    };
    let rows = match input {
        Value::List { vals, .. } => vals.to_vec(),
        Value::Nothing { .. } => Vec::new(),
        other => {
            return Err(CommandError::new(
                "expand rrule",
                format!("expected a table, got {}", other.get_type()),
            ));
        }
    };
    Ok(Value::list(
        expand(rows, instant(from)?, instant(until)?, head)?,
        head,
    ))
}

impl From<ExpandError> for CommandError {
    fn from(err: ExpandError) -> Self {
        CommandError::new(format!("cannot expand event {}", err.uid), err.message)
    }
}

/// Why one event could not be expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandError {
    pub uid: String,
    pub message: String,
}

/// Expands every recurring row in `rows` over [from, until).
pub fn expand(
    rows: Vec<Value>,
    from: Timestamp,
    until: Timestamp,
    span: Span,
) -> Result<Vec<Value>, ExpandError> {
    let overrides: HashSet<(String, Timestamp)> = rows
        .iter()
        .filter_map(|row| {
            Some((
                text(row, "uid")?,
                timestamp(&row.get_data_by_key("recurrence_id")?)?,
            ))
        })
        .collect();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if !is_recurring(&row) {
            out.push(row);
            continue;
        }
        let uid = text(&row, "uid").unwrap_or_else(|| "(no uid)".into());
        let fail = |message: String| ExpandError {
            uid: uid.clone(),
            message,
        };
        let zone = match text(&row, "tz") {
            Some(name) => {
                TimeZone::get(&name).map_err(|_| fail(format!("unknown time zone \"{name}\"")))?
            }
            None => TimeZone::UTC,
        };
        let start = row
            .get_data_by_key("start")
            .and_then(|v| timestamp(&v))
            .ok_or_else(|| fail("a recurring event needs a start".into()))?
            .to_zoned(zone.clone());
        let end = row
            .get_data_by_key("end")
            .and_then(|v| timestamp(&v))
            .map(|end| end.to_zoned(zone.clone()));
        let length = end.as_ref().map_or(SignedDuration::ZERO, |end| {
            end.timestamp().duration_since(start.timestamp())
        });
        // An all-day event lasts whole days, which are not all 24 hours long.
        let all_day = matches!(
            row.get_data_by_key("all_day"),
            Some(Value::Bool { val: true, .. })
        );
        let occurrence_length = end.as_ref().map(|end| match all_day {
            true => Length::Days(jiff::Span::new().days((end.date() - start.date()).get_days())),
            false => Length::Exact(length),
        });
        let in_zone = |key: &str| -> Vec<Zoned> {
            match row.get_data_by_key(key) {
                Some(Value::List { vals, .. }) => vals
                    .iter()
                    .filter_map(timestamp)
                    .map(|ts| ts.to_zoned(zone.clone()))
                    .collect(),
                _ => Vec::new(),
            }
        };
        let (rdates, exdates) = (in_zone("rdates"), in_zone("exdates"));
        let rrule = text(&row, "rrule");
        let series = Series {
            start: &start,
            rrule: rrule.as_deref(),
            rdates: &rdates,
            exdates: &exdates,
        };
        for occurrence in
            occurrences(&series, length, from, until).map_err(|e| fail(e.to_string()))?
        {
            if !overrides.contains(&(uid.clone(), occurrence.timestamp())) {
                out.push(occurrence_row(&row, &occurrence, occurrence_length, span));
            }
        }
    }
    Ok(out)
}

fn text(row: &Value, key: &str) -> Option<String> {
    match row.get_data_by_key(key)? {
        Value::String { val, .. } => Some(val),
        _ => None,
    }
}

fn timestamp(value: &Value) -> Option<Timestamp> {
    time::timestamp(&value.as_date().ok()?).ok()
}

fn is_recurring(row: &Value) -> bool {
    let has_rdates = matches!(
        row.get_data_by_key("rdates"),
        Some(Value::List { vals, .. }) if !vals.is_empty()
    );
    text(row, "rrule").is_some() || has_rdates
}

/// How far an occurrence's end is from its start.
#[derive(Clone, Copy)]
enum Length {
    Exact(SignedDuration),
    /// Calendar days, for all-day events.
    Days(jiff::Span),
}

/// The master row with its times moved to one occurrence.
fn occurrence_row(master: &Value, start: &Zoned, length: Option<Length>, span: Span) -> Value {
    let mut record = master.as_record().cloned().unwrap_or_default();
    let at = Value::date(time::to_nu(start), span);
    record.insert("start", at.clone());
    if let Some(length) = length {
        let end = match length {
            Length::Exact(duration) => start.checked_add(duration),
            Length::Days(days) => start.checked_add(days),
        }
        .expect("an occurrence's end is in range");
        record.insert("end", Value::date(time::to_nu(&end), span));
    }
    record.insert("recurrence_id", at);
    record.insert("rrule", Value::nothing(span));
    record.insert("rdates", Value::list(Vec::new(), span));
    record.insert("exdates", Value::list(Vec::new(), span));
    Value::record(record, span)
}
```

`crates/ppp-core/src/commands/pipe_get.rs`:

```rust
//! `pipe get`: another pipe's checked records, as nushell values (spec §5.5).

use nu_engine::command_prelude::*;

use std::sync::Arc;

use super::{CommandError, Context, describe_failures};
use crate::host::Host;
use crate::pipe::{Pipe, check_records, is_pipe_name};

#[derive(Clone)]
pub struct PipeGet {
    pub context: Context,
}

impl Command for PipeGet {
    fn name(&self) -> &str {
        "pipe get"
    }

    fn description(&self) -> &str {
        "Run another pipe and return its records, checked against its declared types."
    }

    fn extra_description(&self) -> &str {
        "The pipe is looked up as <name>.nu in the run's pipes directory, runs in its own \
         sandboxed engine sharing this run's cache and timeout, and returns nushell values \
         (datetimes stay datetimes). A pipe that, directly or not, gets itself is an error."
    }

    fn signature(&self) -> Signature {
        Signature::build("pipe get")
            .input_output_types(vec![(Type::Nothing, Type::table())])
            .required("name", SyntaxShape::String, "The pipe's name")
            .optional(
                "args",
                SyntaxShape::Record(Default::default()),
                "The pipe's flags, e.g. {within: 3day}",
            )
            .category(Category::Custom("pipes".into()))
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let name: Spanned<String> = call.req(engine_state, stack, 0)?;
        let args: Option<Record> = call.opt(engine_state, stack, 1)?;
        get(
            &self.context.host,
            &name.item,
            args.unwrap_or_default(),
            head,
        )
        .map(|records| records.into_pipeline_data())
        .map_err(|err| err.into_shell_error(name.span))
    }
}

/// Runs the pipe `name` from the host's pipes directory with `args`, checks
/// its records, and returns them.
pub fn get(host: &Arc<Host>, name: &str, args: Record, head: Span) -> Result<Value, CommandError> {
    let fail = |message: String| CommandError::new(format!("pipe get {name}"), message);
    if !is_pipe_name(name) {
        return Err(fail(
            "pipe names may use only letters, digits, `-` and `_`".into(),
        ));
    }
    let path = host.config.pipes_dir.join(format!("{name}.nu"));
    let mut pipe = Pipe::load_with(&path, host.clone()).map_err(|e| fail(e.to_string()))?;
    let records = pipe.run(&args);
    for warning in pipe.take_warnings() {
        host.warn(warning);
    }
    let records = records.map_err(|e| fail(e.to_string()))?;
    let registry = host.registry().map_err(|e| fail(e.to_string()))?;
    if let Err(errors) = check_records(&registry, pipe.info(), &records) {
        return Err(fail(format!(
            "its records failed the check:\n{}",
            describe_failures(&errors)
        )));
    }
    Ok(Value::list(records, head))
}
```

`crates/ppp-core/src/commands/validate.rs`:

```rust
//! `validate`: checks records against the stdlib schemas and passes them on.

use nu_engine::command_prelude::*;

use super::{CommandError, Context, describe_failures};
use crate::pipe::check_values;
use crate::schema::Registry;

#[derive(Clone)]
pub struct Validate {
    pub context: Context,
}

impl Command for Validate {
    fn name(&self) -> &str {
        "validate"
    }

    fn description(&self) -> &str {
        "Check records against the stdlib schemas; pass them on unchanged if they are valid."
    }

    fn signature(&self) -> Signature {
        Signature::build("validate")
            .input_output_types(vec![
                (Type::table(), Type::table()),
                (Type::record(), Type::record()),
            ])
            .category(Category::Custom("pipes".into()))
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let registry = self.context.host.registry().map_err(|e| {
            CommandError::new("stdlib schemas", e.to_string()).into_shell_error(head)
        })?;
        check(&registry, input.into_value(head)?)
            .map(|value| value.into_pipeline_data())
            .map_err(|err| err.into_shell_error(head))
    }
}

/// Returns `input` (a record, or a list of records) unchanged if every
/// record is valid.
pub fn check(registry: &Registry, input: Value) -> Result<Value, CommandError> {
    let records = match &input {
        Value::List { vals, .. } => vals.to_vec(),
        other => vec![other.clone()],
    };
    match check_values(registry, None, &records) {
        Ok(_) => Ok(input),
        Err(errors) => Err(CommandError::new(
            "records are not valid",
            describe_failures(&errors),
        )),
    }
}
```

`crates/ppp-core/src/commands/to_pipes_json.rs`:

```rust
//! `to pipes-json`: records as the canonical JSON the host would emit.

use nu_engine::command_prelude::*;

use super::{CommandError, Context, describe_failures};
use crate::pipe::check_values;
use crate::schema::Registry;

#[derive(Clone)]
pub struct ToPipesJson {
    pub context: Context,
}

impl Command for ToPipesJson {
    fn name(&self) -> &str {
        "to pipes-json"
    }

    fn description(&self) -> &str {
        "Write records as JSON with each time field in its schema's form, after checking them."
    }

    fn extra_description(&self) -> &str {
        "Unlike `to json`, dates come out as dates, local times without an offset (in the \
         record's `tz`) and instants in UTC, exactly as `ppp run` prints them."
    }

    fn signature(&self) -> Signature {
        Signature::build("to pipes-json")
            .input_output_types(vec![
                (Type::table(), Type::String),
                (Type::record(), Type::String),
            ])
            .category(Category::Custom("pipes".into()))
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let registry = self.context.host.registry().map_err(|e| {
            CommandError::new("stdlib schemas", e.to_string()).into_shell_error(head)
        })?;
        render(&registry, input.into_value(head)?, head)
            .map(|value| value.into_pipeline_data())
            .map_err(|err| err.into_shell_error(head))
    }
}

/// The records in `input` (a record, or a list of records) as a pretty JSON
/// array; a single record gives a single object.
pub fn render(registry: &Registry, input: Value, head: Span) -> Result<Value, CommandError> {
    let single = !matches!(input, Value::List { .. });
    let records = match input {
        Value::List { vals, .. } => vals.to_vec(),
        other => vec![other],
    };
    let json = check_values(registry, None, &records)
        .map_err(|errors| CommandError::new("records are not valid", describe_failures(&errors)))?;
    let text = if single {
        serde_json::to_string_pretty(&json[0])
    } else {
        serde_json::to_string_pretty(&json)
    }
    .expect("JSON values always serialize");
    Ok(Value::string(text, head))
}
```

`crates/ppp-core/src/pipe.rs`:

```rust
//! Loading a pipe file, reading its metadata, running it, and checking its
//! output at the boundary (spec §5.1).

use std::path::Path;
use std::sync::Arc;

use jiff::SignedDuration;
use nu_protocol::{Record, Span, Value};
use serde_json::Value as Json;

use crate::engine::{Engine, EngineError};
use crate::host::Host;
use crate::schema::{RecordError, Registry};
use crate::serialize::record_to_json;

#[derive(Debug, thiserror::Error)]
pub enum PipeError {
    #[error("{0}")]
    Engine(#[from] EngineError),
    #[error("{path}: {message}")]
    Metadata { path: String, message: String },
    #[error("{pipe} has no flag --{flag}")]
    UnknownFlag { pipe: String, flag: String },
    #[error("pipes get each other in a loop: {}", .0.join(" → "))]
    Cycle(Vec<String>),
}

/// What the host knows about a pipe without running it.
#[derive(Debug, Clone, PartialEq)]
pub struct PipeInfo {
    /// The command name: the file name without `.nu`.
    pub name: String,
    /// First line of `main`'s doc comment.
    pub title: String,
    /// The rest of `main`'s doc comment.
    pub description: String,
    /// `main`'s flags, in order.
    pub params: Vec<Param>,
    /// Record type URIs from `export const pipe = { output: [...] }`.
    pub output: Vec<String>,
    /// How long a fetched response stays fresh, from `pipe.refresh`.
    pub refresh: Option<SignedDuration>,
    /// From `@category`.
    pub category: String,
    /// From `@search-terms`.
    pub search_terms: Vec<String>,
    /// From `@example`.
    pub examples: Vec<Example>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub description: String,
    pub example: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    /// Nushell's name for the flag's type, e.g. `int` or `string`. Switches
    /// (flags with no value) are `switch`.
    pub shape: String,
    pub description: String,
    pub default: Option<Value>,
}

pub struct Pipe {
    engine: Engine,
    info: PipeInfo,
}

impl Pipe {
    /// Loads the pipe at `path` with a default `Host` (no cache).
    pub fn load(path: &Path) -> Result<Pipe, PipeError> {
        Pipe::load_with(path, Arc::new(Host::default()))
    }

    /// Loads the pipe at `path` into a fresh engine sharing `host`, and reads
    /// its metadata. Nothing in the pipe runs except its constant definitions.
    pub fn load_with(path: &Path, host: Arc<Host>) -> Result<Pipe, PipeError> {
        let metadata = |message: String| PipeError::Metadata {
            path: path.display().to_string(),
            message,
        };
        let full = path.canonicalize().map_err(|e| metadata(e.to_string()))?;
        let name = full
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| metadata("file name is not valid UTF-8".into()))?
            .to_string();
        // The name becomes the command to call and `$<name>.pipe`, both
        // spliced into nushell source, so it must be a plain identifier.
        if !is_pipe_name(&name) {
            return Err(metadata(format!(
                "pipe file names may use only letters, digits, `-` and `_`, not \"{name}\""
            )));
        }
        let mut engine = Engine::with_host(host);
        engine.eval(
            &format!("use {}", nu_string(&full.to_string_lossy())),
            Value::nothing(Span::unknown()),
        )?;

        let found = engine.eval(
            &format!("scope commands | where name == {} | select description extra_description category search_terms examples signatures", nu_string(&name)),
            Value::nothing(Span::unknown()),
        )?;
        let command = found
            .as_list()
            .ok()
            .and_then(|rows| rows.first())
            .ok_or_else(|| metadata("the file does not export a `main` command".into()))?;
        let field = |key: &str| {
            command
                .get_data_by_key(key)
                .and_then(|v| v.coerce_into_string().ok())
                .unwrap_or_default()
        };
        let params = read_params(command).map_err(metadata)?;

        let constant = format!("${}.pipe", name.replace('-', "_"));
        let output = engine
            .eval(
                &format!("{constant}.output? | default []"),
                Value::nothing(Span::unknown()),
            )
            .map_err(|_| metadata("the file has no `export const pipe = {...}`".into()))?;
        let output = output
            .as_list()
            .map_err(|_| metadata("`pipe.output` must be a list of record type URIs".into()))?
            .iter()
            .map(|v| {
                v.coerce_string()
                    .map_err(|_| metadata("`pipe.output` must be a list of strings".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if output.is_empty() {
            return Err(metadata(
                "`pipe.output` must name at least one record type".into(),
            ));
        }

        let refresh = match engine.eval(
            &format!("{constant}.refresh? | default null"),
            Value::nothing(Span::unknown()),
        )? {
            Value::Duration { val, .. } => Some(SignedDuration::from_nanos(val)),
            Value::Nothing { .. } => None,
            _ => {
                return Err(metadata(
                    "`pipe.refresh` must be a duration, e.g. 1hr".into(),
                ));
            }
        };
        engine.set_refresh(refresh);
        let search_terms = field("search_terms")
            .split(',')
            .map(|term| term.trim().to_string())
            .filter(|term| !term.is_empty())
            .collect();
        let examples = match command.get_data_by_key("examples") {
            Some(Value::List { vals, .. }) => vals
                .iter()
                .map(|example| {
                    let text = |key: &str| {
                        example
                            .get_data_by_key(key)
                            .and_then(|v| v.coerce_into_string().ok())
                            .unwrap_or_default()
                    };
                    Example {
                        description: text("description"),
                        example: text("example"),
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        let info = PipeInfo {
            name,
            title: field("description"),
            description: field("extra_description"),
            params,
            output,
            refresh,
            category: field("category"),
            search_terms,
            examples,
        };
        Ok(Pipe { engine, info })
    }

    pub fn info(&self) -> &PipeInfo {
        &self.info
    }

    /// Returns, and forgets, nushell's warnings from loading and running
    /// the pipe (deprecations, for example), rendered.
    pub fn take_warnings(&mut self) -> Vec<String> {
        self.engine.take_warnings()
    }

    /// Runs `main` with the given flag values and returns its records.
    /// Flags must be names from `info().params`; a switch is passed when its
    /// value is `true`.
    pub fn run(&mut self, args: &Record) -> Result<Vec<Value>, PipeError> {
        let mut call = format!("let args = $in; {}", self.info.name);
        for (flag, value) in args.iter() {
            let param = self
                .info
                .params
                .iter()
                .find(|p| &p.name == flag)
                .ok_or_else(|| PipeError::UnknownFlag {
                    pipe: self.info.name.clone(),
                    flag: flag.clone(),
                })?;
            if param.shape == "switch" {
                if value.as_bool().unwrap_or(false) {
                    call.push_str(&format!(" --{flag}"));
                }
            } else {
                call.push_str(&format!(" --{flag} $args.{flag}"));
            }
        }
        self.call(&call, Value::record(args.clone(), Span::unknown()))
    }

    /// Runs `main` with command-line arguments, the way `nu <pipe>.nu
    /// <args>` does: each argument is quoted as needed and nushell's parser
    /// checks them against `main`'s signature. For a CLI only; embedders
    /// pass typed values to `run`.
    pub fn run_command_line(&mut self, args: &[String]) -> Result<Vec<Value>, PipeError> {
        let mut call = self.info.name.clone();
        for arg in args {
            call.push(' ');
            call.push_str(&nu_parser::escape_for_script_arg(arg));
        }
        self.call(&call, Value::nothing(Span::unknown()))
    }

    fn call(&mut self, source: &str, input: Value) -> Result<Vec<Value>, PipeError> {
        let host = self.engine.host().clone();
        let _running = host.enter(&self.info.name).map_err(PipeError::Cycle)?;
        Ok(match self.engine.eval(source, input)? {
            Value::List { vals, .. } => vals.to_vec(),
            Value::Nothing { .. } => Vec::new(),
            other => vec![other],
        })
    }
}

/// Checks a pipe's records at the boundary: each must have a type the pipe
/// declares, serialize for that type, and validate. Returns the JSON
/// records, or every failure.
pub fn check_records(
    registry: &Registry,
    info: &PipeInfo,
    records: &[Value],
) -> Result<Vec<Json>, Vec<RecordError>> {
    check_values(registry, Some(&info.output), records)
}

/// Checks records against the stdlib: each must have a stdlib type (one of
/// `allowed`, given as type URIs, when that is given), serialize for it, and
/// validate. Returns the JSON records, or every failure.
pub fn check_values(
    registry: &Registry,
    allowed: Option<&[String]>,
    records: &[Value],
) -> Result<Vec<Json>, Vec<RecordError>> {
    let mut out = Vec::with_capacity(records.len());
    let mut errors = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let type_name = record
            .get_data_by_key("type")
            .and_then(|v| v.coerce_string().ok());
        let found = type_name
            .as_deref()
            .and_then(|name| registry.record_type(name));
        let declared = found.filter(|t| allowed.is_none_or(|uris| uris.contains(&t.uri)));
        let Some(record_type) = declared else {
            let message = match (&type_name, allowed) {
                (None, _) => "record has no \"type\"".to_string(),
                (Some(name), Some(uris)) => {
                    format!(
                        "type \"{name}\" is not one of the pipe's output types: {}",
                        uris.join(", ")
                    )
                }
                (Some(name), None) => format!("unknown record type \"{name}\""),
            };
            errors.push(RecordError {
                index,
                pointer: "/type".into(),
                message,
                excerpt: String::new(),
            });
            continue;
        };
        match record_to_json(record, &record_type.time_fields) {
            Ok(json) => {
                errors.extend(registry.validate(index, &json));
                out.push(json);
            }
            Err(err) => errors.push(RecordError {
                index,
                pointer: String::new(),
                message: err.to_string(),
                excerpt: String::new(),
            }),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

fn read_params(command: &Value) -> Result<Vec<Param>, String> {
    let signatures = command
        .get_data_by_key("signatures")
        .and_then(|s| s.as_record().ok().cloned())
        .ok_or("cannot read `main`'s signature")?;
    let Some((_, rows)) = signatures.iter().next() else {
        return Ok(Vec::new());
    };
    let mut params = Vec::new();
    for row in rows.as_list().map_err(|e| e.to_string())? {
        let text = |key: &str| {
            row.get_data_by_key(key)
                .and_then(|v| v.coerce_string().ok())
                .unwrap_or_default()
        };
        match text("parameter_type").as_str() {
            "input" | "output" => {}
            "named" | "switch" if text("parameter_name") == "help" => {}
            kind @ ("named" | "switch") => params.push(Param {
                name: text("parameter_name"),
                shape: if kind == "switch" {
                    "switch".into()
                } else {
                    text("syntax_shape")
                },
                description: text("description"),
                default: row
                    .get_data_by_key("parameter_default")
                    .filter(|v| !v.is_nothing()),
            }),
            other => {
                return Err(format!(
                    "`main` has a {other} parameter `{}`; pipe parameters must be flags",
                    text("parameter_name")
                ));
            }
        }
    }
    Ok(params)
}

/// Whether `name` can name a pipe: letters, digits, `-` and `_` only. A
/// pipe's name is spliced into nushell source and joined onto the pipes
/// directory, so nothing else is allowed.
pub fn is_pipe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A nushell double-quoted string literal for `text`.
fn nu_string(text: &str) -> String {
    serde_json::to_string(text).expect("strings always serialize")
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: every suite passes; `record_commands` 4.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add crates/ppp-core
git commit -m "Share command logic as plain functions; add validate and to pipes-json

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---

### Task 2: `nu_plugin_pipes`

**Files:** Modify `Cargo.toml` (member, `nu-plugin`, `nu-plugin-test-support`); Create `crates/nu_plugin_pipes/{Cargo.toml,src/lib.rs,src/main.rs}`; Test `crates/nu_plugin_pipes/tests/{plugin,stock_nu}.rs`.

**Interfaces:** Consumes Task 1. Produces the `nu_plugin_pipes` binary (`plugin add …; plugin use pipes`) and `nu_plugin_pipes::PipesPlugin`.

- [ ] **Step 1: Set up**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/ppp-core", "crates/ppp-cli", "crates/nu_plugin_pipes"]

[workspace.package]
edition = "2024"

[workspace.dependencies]
chrono = "0.4.45"
chrono-tz = "0.10"
clap = { version = "4", features = ["derive"] }
dirs = "7"
htmlize = { version = "1.1", features = ["unescape"] }
jiff = { version = "0.2.38", features = ["tzdb-bundle-always", "serde"] }
jsonschema = { version = "0.58.6", default-features = false }
nu-cmd-lang = "=0.116.1"
# `os` is needed: other nushell crates turn on nu-protocol's `os` feature, and
# nu-command does not compile with it on unless its own `os` is on too.
nu-command = { version = "=0.116.1", default-features = false, features = ["os", "network", "rustls-tls"] }
nu-engine = "=0.116.1"
nu-parser = "=0.116.1"
nu-plugin = "=0.116.1"
nu-plugin-test-support = "=0.116.1"
nu-protocol = { version = "=0.116.1", default-features = false }
rrule = "0.14"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.11"
thiserror = "2"
ureq = "~3.4"
```

`crates/nu_plugin_pipes/Cargo.toml`:

```toml
[package]
name = "nu_plugin_pipes"
version = "0.1.0"
edition.workspace = true

[[bin]]
name = "nu_plugin_pipes"
path = "src/main.rs"

[dependencies]
dirs.workspace = true
nu-plugin.workspace = true
nu-protocol = { workspace = true, features = ["plugin"] }
ppp-core = { path = "../ppp-core" }

[dev-dependencies]
nu-command.workspace = true
serde_json.workspace = true
nu-plugin-test-support.workspace = true
```

- [ ] **Step 2: Write the failing test**

`crates/nu_plugin_pipes/tests/plugin.rs`:

```rust
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
```

`crates/nu_plugin_pipes/tests/stock_nu.rs`:

```rust
//! The plugin inside a real `nu` (spec §4.5): our commands in stock nushell,
//! pipe files running unchanged, and `pipe get`.
//!
//! These need `nu` 0.116.1, the version the plugin is built against; plugins
//! must match nu's version. Set `PPP_TEST_NU` to its path, or have it on
//! PATH, and run `cargo test -p nu_plugin_pipes -- --ignored`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Runs `source` in `nu` with only our plugin loaded and `stdlib/` as a
/// library directory, and returns stdout. It runs outside the repository:
/// from the repository root, `use pipes *` would find the repo's own
/// `pipes/` directory (the acceptance pipes) before the library.
fn nu(source: &str) -> String {
    let nu = std::env::var("PPP_TEST_NU").unwrap_or_else(|_| "nu".into());
    let version = Command::new(&nu)
        .args(["-n", "-c", "version | get version"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "0.116.1",
        "{nu} is not nu 0.116.1"
    );
    let registry = Path::new(env!("CARGO_TARGET_TMPDIR")).join("empty-plugin-registry.msgpackz");
    let _ = std::fs::remove_file(&registry);
    let out = Command::new(&nu)
        .arg("-n")
        .arg("--plugin-config")
        .arg(&registry)
        .arg("--plugins")
        .arg(env!("CARGO_BIN_EXE_nu_plugin_pipes"))
        .arg("-I")
        .arg(repo().join("stdlib"))
        .arg("-c")
        .arg(source)
        .current_dir(env!("CARGO_TARGET_TMPDIR"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap()
}

#[test]
#[ignore]
fn our_commands_work_in_stock_nu() {
    let repo = repo().display().to_string();
    let out = nu(&format!(
        "use pipes *
        open --raw {repo}/fixtures/ics/campus-events.ics
        | from ics
        | expand rrule --from 2026-11-01T00:00:00Z --until 2026-12-01T00:00:00Z
        | into event
        | where summary == 'Chess Club'
        | to pipes-json"
    ));
    let events = json(&out);
    assert_eq!(events.as_array().unwrap().len(), 3);
    assert_eq!(events[0]["start"], "2026-11-03T18:00:00");
}

#[test]
#[ignore]
fn pipe_files_run_unchanged() {
    let repo = repo().display().to_string();
    let out = nu(&format!(
        "use {repo}/fixtures/pipes/two-events.nu; two-events --count 1 --loud | validate | to pipes-json"
    ));
    assert_eq!(json(&out)[0]["summary"], "CHESS CLUB");
}

#[test]
#[ignore]
fn pipe_get_runs_the_sandboxed_engine() {
    let repo = repo().display().to_string();
    let out = nu(&format!(
        "pipe get two-events --pipes-dir {repo}/fixtures/pipes {{count: 1}} | get 0.summary"
    ));
    assert_eq!(out.trim(), "Chess Club");
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p nu_plugin_pipes`
Expected: compile error — the crate has no `src/lib.rs` yet (`PipesPlugin` not found).

- [ ] **Step 4: Write the code**

`crates/nu_plugin_pipes/src/lib.rs`:

```rust
//! `nu_plugin_pipes`: ppp-pipes' commands for stock `nu` (spec §4.5).
//!
//! Each command here is a thin wrapper around the same function the
//! sandboxed engine's command calls; it borrows that command's name,
//! signature and description, so the two can't drift apart.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use nu_plugin::{EngineInterface, EvaluatedCall, Plugin, PluginCommand, SimplePluginCommand};
use nu_protocol::engine::Command;
use nu_protocol::{LabeledError, Record, Signature, SyntaxShape, Value};
use ppp_core::commands::{self, CommandError, Context};
use ppp_core::host::{Host, HostConfig};
use ppp_core::schema::Registry;

/// How long `pipe get` lets a pipe run.
const PIPE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct PipesPlugin {
    registry: OnceLock<Result<Arc<Registry>, String>>,
}

impl PipesPlugin {
    fn registry(&self) -> Result<Arc<Registry>, LabeledError> {
        self.registry
            .get_or_init(|| Registry::stdlib().map(Arc::new).map_err(|e| e.to_string()))
            .clone()
            .map_err(|e| {
                LabeledError::new("stdlib schemas").with_label(e, nu_protocol::Span::unknown())
            })
    }
}

impl Plugin for PipesPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
        let context = Context {
            host: Arc::new(Host::default()),
            refresh: Arc::new(Mutex::new(None)),
        };
        vec![
            Box::new(FromIcs(commands::FromIcs)),
            Box::new(ExpandRrule(commands::ExpandRrule)),
            Box::new(HtmlToText(commands::HtmlToText)),
            Box::new(Validate(commands::Validate {
                context: context.clone(),
            })),
            Box::new(ToPipesJson(commands::ToPipesJson {
                context: context.clone(),
            })),
            Box::new(PipeGet(commands::PipeGet { context })),
        ]
    }
}

/// The parts every wrapper takes from the engine command it wraps.
macro_rules! borrow_from_core {
    () => {
        type Plugin = PipesPlugin;
        fn name(&self) -> &str {
            self.0.name()
        }
        fn description(&self) -> &str {
            self.0.description()
        }
        fn extra_description(&self) -> &str {
            self.0.extra_description()
        }
    };
}

fn labeled(head: nu_protocol::Span) -> impl Fn(CommandError) -> LabeledError {
    move |err| err.into_labeled_error(head)
}

pub struct FromIcs(commands::FromIcs);

impl SimplePluginCommand for FromIcs {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        _: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        let tz: Option<String> = call.get_flag("tz")?;
        commands::from_ics::read(input.clone(), tz.as_deref(), call.head)
            .map_err(labeled(call.head))
    }
}

pub struct ExpandRrule(commands::ExpandRrule);

impl SimplePluginCommand for ExpandRrule {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        _: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        let from: Value = call.get_flag_value("from").expect("required flag");
        let until: Value = call.get_flag_value("until").expect("required flag");
        commands::expand_rrule::expand_values(input.clone(), &from, &until, call.head)
            .map_err(labeled(call.head))
    }
}

pub struct HtmlToText(commands::HtmlToText);

impl SimplePluginCommand for HtmlToText {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        _: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        Ok(match input {
            Value::Nothing { .. } => Value::nothing(call.head),
            value => Value::string(ppp_core::html::to_text(&value.coerce_str()?), call.head),
        })
    }
}

pub struct Validate(commands::Validate);

impl SimplePluginCommand for Validate {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        plugin: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        commands::validate::check(&*plugin.registry()?, input.clone()).map_err(labeled(call.head))
    }
}

pub struct ToPipesJson(commands::ToPipesJson);

impl SimplePluginCommand for ToPipesJson {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        plugin: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        commands::to_pipes_json::render(&*plugin.registry()?, input.clone(), call.head)
            .map_err(labeled(call.head))
    }
}

/// `pipe get` in stock `nu`: runs the pipe in the full sandboxed engine, with
/// the cache and a time limit, and returns its checked records.
pub struct PipeGet(commands::PipeGet);

impl SimplePluginCommand for PipeGet {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature().named(
            "pipes-dir",
            SyntaxShape::String,
            "Directory that pipe names are looked up in (default: ./pipes)",
            None,
        )
    }
    fn run(
        &self,
        _: &PipesPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _: &Value,
    ) -> Result<Value, LabeledError> {
        let name: String = call.req(0)?;
        let args: Option<Record> = call.opt(1)?;
        let pipes_dir: Option<String> = call.get_flag("pipes-dir")?;
        let cwd = PathBuf::from(engine.get_current_dir()?);
        let host = Arc::new(Host::new(HostConfig {
            cache_dir: dirs::cache_dir().map(|dir| dir.join("ppp-pipes").join("http")),
            pipes_dir: cwd.join(pipes_dir.unwrap_or_else(|| "pipes".into())),
            ..HostConfig::default()
        }));
        let result = host.with_timeout(PIPE_TIMEOUT, || {
            commands::pipe_get::get(&host, &name, args.unwrap_or_default(), call.head)
        });
        for warning in host.take_warnings() {
            eprintln!("{warning}");
        }
        result.map_err(labeled(call.head))
    }
}
```

`crates/nu_plugin_pipes/src/main.rs`:

```rust
use nu_plugin::{MsgPackSerializer, serve_plugin};
use nu_plugin_pipes::PipesPlugin;

fn main() {
    serve_plugin(&PipesPlugin::default(), MsgPackSerializer);
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test`
Expected: every suite passes, 200 tests; then `PPP_TEST_NU=<nu 0.116.1> cargo test -p nu_plugin_pipes --test stock_nu -- --ignored` passes 3.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add Cargo.toml Cargo.lock crates/nu_plugin_pipes
git commit -m "Add nu_plugin_pipes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---


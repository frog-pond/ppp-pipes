# ppp-pipes Phase 5: More Sources — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every acceptance source in spec §7 has a working pipe: St. Olaf's WordPress calendar, Presence, Carleton and Northfield, plus `all-campus`, which combines them with `pipe get`.

**Architecture:** A Rust `html to-text` command (`html.rs`) turns HTML into plain text. The `pipes` module gains two format readers, `from tribe-events` and `from presence`, which produce the shared intermediate table; `into event` learns that table's richer optional columns. Each pipe takes `--source` (default: the live URL) and `--from`/`--within`, so tests run it against saved responses on a local server with a fixed window.

**Tech Stack:** htmlize 1.1 (`unescape` feature) for HTML entities; nushell 0.116.1.

**Spec:** [`docs/superpowers/specs/2026-10-11-ppp-pipes-cli-prototype-design.md`](../specs/2026-10-11-ppp-pipes-cli-prototype-design.md) §6.2 and §7 — build-order step 5. Both sections were updated in the same commit as this plan.

Every code block was compiled and tested in a scratch copy of the phase 4 branch: 190 tests pass, and all six acceptance pipes pass against the live sources. The full saved responses (50 Tribe events, 440 Presence events) validate without errors.

## Global Constraints

- Everything in the earlier phases' Global Constraints still holds.
- No pipe may read a local time without a zone: Tribe times come from its `utc_*` fields.
- Fixtures are trimmed to events that each show a quirk.

## Decisions made while planning

Wren was away and asked for the phases to continue; these are mine to review.

1. **`html to-text` is a small state machine plus `htmlize`'s entity decoder**, not a renderer (`html2text` wraps lines and adds link footnotes). Paragraph tags get a blank line; `br`/`li`/`tr`/`dt`/`dd` start a line where they open; `<script>` and `<style>` contents are dropped; a `<` that doesn't start a tag is text.
2. **`into event` runs `html to-text` on `summary`, `location` and `excerpt`** for every source, since harmless on plain text and needed for Tribe titles (`Men&#8217;s`) and Northfield locations.
3. **Tribe country names map to ISO codes** (US, CA) or are dropped, since the schema wants `^[A-Z]{2}$`.
4. **Tribe paging counts pages in `generate`'s state** and fails as soon as the last allowed page still has a next page, without fetching it (using `take` fetched one page too many).
5. **Northfield's event links come from the description:** the feed's `URL` points back at the feed itself.
6. **`all-campus` fails if any source fails**, which keeps errors loud. If one dead source should only drop its events, that's a later change.
7. **Every windowed pipe takes `--from`** (default now), so tests and `pipe get` callers can fix the window. `campus-week` gains it too.

## Review Focus

1. **A Tribe site sending more pages than `--max-pages`** must fail without fetching past the cap — Task 3 (`stolaf_wp_refuses_more_pages_than_its_cap`).
2. **HTML-escaped and `%20`-padded URLs** must come out clean — Tasks 2 and 3.
3. **A `<` in plain text** ("1 < 2") must survive `html to-text` — Task 1.
4. **An all-day Tribe event** must end on the day after its last day, not at 23:59:59 — Task 2.
5. **A Presence event without a contact** must still be valid, with only its org as organizer — Task 2.

---

### Task 1: `html to-text`

**Files:** Modify `Cargo.toml`, `crates/ppp-core/Cargo.toml`, `crates/ppp-core/src/lib.rs`, `crates/ppp-core/src/commands/mod.rs`; Create `crates/ppp-core/src/html.rs`, `crates/ppp-core/src/commands/html_to_text.rs`; Test `crates/ppp-core/tests/html.rs`.

**Interfaces:** Produces `ppp_core::html::to_text(&str) -> String` and the `html to-text` command (string → string; null → null).

- [ ] **Step 1: Set up**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/ppp-core", "crates/ppp-cli"]

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
nu-protocol = { version = "=0.116.1", default-features = false }
rrule = "0.14"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.11"
thiserror = "2"
ureq = "~3.4"
```

`crates/ppp-core/Cargo.toml`:

```toml
[package]
name = "ppp-core"
version = "0.1.0"
edition.workspace = true

[dependencies]
chrono.workspace = true
chrono-tz.workspace = true
htmlize.workspace = true
jiff.workspace = true
jsonschema.workspace = true
nu-cmd-lang.workspace = true
nu-command.workspace = true
nu-engine.workspace = true
nu-parser.workspace = true
nu-protocol.workspace = true
rrule.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true
ureq.workspace = true

[dev-dependencies]
flate2 = "1"
```

- [ ] **Step 2: Write the failing test**

`crates/ppp-core/tests/html.rs`:

```rust
//! Plain text from HTML: tags, line breaks, entities, whitespace.

use nu_protocol::Value;
use ppp_core::engine::Engine;
use ppp_core::html::to_text;

#[test]
fn tags_go_and_entities_are_decoded() {
    assert_eq!(
        to_text("<p>Men&#8217;s Soccer &amp; <b>more</b></p>"),
        "Men’s Soccer & more"
    );
    assert_eq!(to_text("&lt;not a tag&gt; &nbsp;x"), "<not a tag> x");
}

#[test]
fn block_tags_break_lines() {
    assert_eq!(to_text("<p>One</p><p>Two</p>"), "One\n\nTwo");
    assert_eq!(to_text("a<br>b<BR/>c"), "a\nb\nc");
    assert_eq!(to_text("<ul><li>x</li><li>y</li></ul>"), "x\ny");
}

#[test]
fn scripts_and_styles_are_dropped() {
    assert_eq!(
        to_text("a<script>alert('<p>')</script>b<style>p{}</style>c"),
        "abc"
    );
}

#[test]
fn a_lone_less_than_is_text() {
    assert_eq!(to_text("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
}

#[test]
fn whitespace_is_tidied() {
    assert_eq!(to_text("  a   b \n\n\n\n c  "), "a b\n\nc");
    assert_eq!(to_text("<div>  </div>"), "");
}

#[test]
fn the_command_passes_null_through() {
    let mut engine = Engine::new();
    let text = engine
        .eval("'<b>x</b>' | html to-text", Value::test_nothing())
        .unwrap();
    assert_eq!(text.coerce_into_string().unwrap(), "x");
    let nothing = engine
        .eval("null | html to-text", Value::test_nothing())
        .unwrap();
    assert!(nothing.is_nothing());
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test html`
Expected: compile error, `unresolved import ppp_core::html`.

- [ ] **Step 4: Write the code**

`crates/ppp-core/src/html.rs`:

```rust
//! Plain text from HTML, for `html to-text` and `into event`.
//!
//! Sources put HTML in fields that should be plain text (Tribe titles,
//! Carleton descriptions, Northfield locations). This is not a renderer:
//! it drops tags, turns block-level tags into line breaks, drops the
//! contents of `<script>` and `<style>`, decodes entities, and tidies
//! whitespace.

/// Tags that stand apart as paragraphs: a blank line before and after.
const PARAGRAPH_TAGS: &[&str] = &[
    "address",
    "article",
    "blockquote",
    "div",
    "dl",
    "figcaption",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

/// Tags that start a new line where they open.
const LINE_TAGS: &[&str] = &["br", "dd", "dt", "li", "tr"];

pub fn to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let starts_tag =
            after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        let Some(close) = after.find('>').filter(|_| starts_tag) else {
            // A `<` that doesn't start a tag is text, e.g. "1 < 2".
            text.push('<');
            rest = after;
            continue;
        };
        let tag = &after[..close];
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        rest = &after[close + 1..];
        if (name == "script" || name == "style") && !closing {
            let end = format!("</{name}");
            rest = match rest.to_ascii_lowercase().find(&end) {
                Some(at) => rest[at..].split_once('>').map_or("", |(_, r)| r),
                None => "",
            };
        } else if PARAGRAPH_TAGS.contains(&name.as_str()) {
            text.push_str("\n\n");
        } else if LINE_TAGS.contains(&name.as_str()) && !closing {
            text.push('\n');
        }
    }
    text.push_str(rest);
    tidy(&htmlize::unescape(&text))
}

/// Collapses runs of spaces within lines, trims each line, and keeps at most
/// one blank line between paragraphs.
fn tidy(text: &str) -> String {
    let mut out = Vec::new();
    let mut blank = false;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push(String::new());
            blank = false;
        }
        out.push(line);
    }
    out.join("\n")
}
```

`crates/ppp-core/src/commands/html_to_text.rs`:

```rust
//! `html to-text`: plain text from an HTML string.

use nu_engine::command_prelude::*;

use crate::html::to_text;

#[derive(Clone)]
pub struct HtmlToText;

impl Command for HtmlToText {
    fn name(&self) -> &str {
        "html to-text"
    }

    fn description(&self) -> &str {
        "Turn HTML into plain text: drop tags, break lines at block tags, decode entities."
    }

    fn signature(&self) -> Signature {
        Signature::build("html to-text")
            .input_output_types(vec![
                (Type::String, Type::String),
                (Type::Nothing, Type::Nothing),
            ])
            .category(Category::Strings)
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        match input.into_value(head)? {
            Value::Nothing { .. } => Ok(Value::nothing(head).into_pipeline_data()),
            value => {
                Ok(Value::string(to_text(&value.coerce_into_string()?), head).into_pipeline_data())
            }
        }
    }
}
```

`crates/ppp-core/src/commands/mod.rs`:

```rust
//! Our own nushell commands.

use std::sync::{Arc, Mutex};

use jiff::SignedDuration;
use nu_protocol::engine::StateWorkingSet;

use crate::host::Host;

pub mod expand_rrule;
mod from_ics;
mod html_to_text;
mod http_get;
mod pipe_get;

pub use expand_rrule::ExpandRrule;
pub use from_ics::FromIcs;
pub use html_to_text::HtmlToText;
pub use http_get::HttpGet;
pub use pipe_get::PipeGet;

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
}
```

`crates/ppp-core/src/lib.rs`:

```rust
pub mod cache;
pub mod commands;
pub mod engine;
pub mod host;
pub mod html;
pub mod http;
pub mod ics;
pub mod pipe;
pub mod recur;
pub mod schema;
pub mod serialize;
pub mod time;
pub mod time_fields;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test html`
Expected: `test result: ok. 6 passed`

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add Cargo.toml Cargo.lock crates/ppp-core
git commit -m "Add html to-text

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---

### Task 2: The Tribe and Presence readers

**Files:** Modify `stdlib/pipes/mod.nu` (`into event` reads the richer shared table; adds `from tribe-events`, `from presence`), `crates/ppp-core/tests/engine.rs` (new names in the collision list); Create `fixtures/json/tribe-page.json`, `fixtures/json/presence.json`; Test `crates/ppp-core/tests/sources.rs`.

**Interfaces:** Consumes `html to-text` (Task 1). Produces `from tribe-events` (list → table), `from presence [--photos <base url>]` (list → table), and `into event` support for `description_html`, `excerpt`, `place`, `organizers`, `images`, `links`, `cost`, `keywords`, `online`, `attendance`, `featured`, `hidden`, `extra`.

The two JSON fixtures are trimmed copies of the live responses fetched on 2026-10-11 (7 Tribe events chosen for: venue with geo and organizer, no venue, all-day, free, an entity in the title, `&amp;` in the website; 4 Presence events including one without a contact). Write them from the saved responses rather than by hand: they are 30 KB and 7 KB.

- [ ] **Step 1: Write the failing test**

`crates/ppp-core/tests/sources.rs`:

```rust
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
        .eval(&format!("use pipes *; $in | from json | {reader} | into event --source https://example.edu/feed"), Value::test_string(json))
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
    let (start, end) = (
        visit["start"].as_str().unwrap(),
        visit["end"].as_str().unwrap(),
    );
    assert_eq!(start.len(), 10);
    assert!(end > start, "{start} .. {end}");
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
        "expand rrule",
        "into event",
        "drop-empty",
        "during",
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

Run: `cargo test -p ppp-core --test sources`
Expected: all 6 FAIL — the fixtures don't exist yet and `from tribe-events`/`from presence` aren't defined.

- [ ] **Step 3: Write the code**

`stdlib/pipes/mod.nu`:

```nu
# The pipes standard library: helpers that turn source data into stdlib records.

# iCalendar properties that `into event` maps to event fields. Every other
# property is kept under `extra`, keyed by its lower-cased name.
const MAPPED_PROPS = [
    UID SUMMARY DESCRIPTION LOCATION URL STATUS DTSTART DTEND DURATION CATEGORIES
    ORGANIZER GEO RRULE RDATE EXDATE RECURRENCE-ID CREATED LAST-MODIFIED
]

# Remove the fields whose value is null, "", [] or {}.
export def drop-empty []: record -> record {
    items {|key, value| [$key $value] }
    | where {|pair| $pair.1 not-in [null "" [] {}] }
    | into record
}

# Keep the rows whose time span overlaps the window [from, until). A row runs
# from `start` to `end`; a row with no `end` is a moment at `start`. Events
# already under way when the window opens are kept.
export def during [from: datetime, until: datetime]: table -> table {
    where {|row|
        if $row.end? == null {
            $row.start >= $from and $row.start < $until
        } else {
            $row.start < $until and $row.end > $from
        }
    }
}

# Turn rows from a format reader (`from ics`, `from tribe-events`,
# `from presence`) into stdlib `event` records.
#
# Rows share one shape: the `from ics` columns, plus optional
# `description_html`, `excerpt`, `place`, `organizers`, `images`, `links`,
# `cost`, `keywords`, `online`, `attendance`, `featured`, `hidden` and `extra`.
# HTML in `summary`, `location`, `excerpt` and `description_html` becomes
# plain text; `description_html` is also kept as `content`.
#
# An ICS ORGANIZER becomes a person when its CUTYPE is INDIVIDUAL and an
# organization otherwise, since campus calendars are mostly run by offices.
# All-day dates that arrive with no time zone are midnight UTC; moving them
# with `date to-timezone` before this command shifts the date.
export def "into event" [
    --source: string  # URL the rows came from, kept on each record
    --tz: string      # Time zone for rows that have none
]: table -> table {
    each {|row| $row | event-from-row $source $tz }
}

def event-from-row [source: any, tz: any]: record -> record {
    let row = $in
    if ($row.uid? | is-empty) {
        error make {msg: $"event has no uid: ($row.summary? | default '(no summary)')"}
    }
    let identity = if $row.recurrence_id? != null {
        let at = $row.recurrence_id | date to-timezone UTC | format date "%Y%m%dT%H%M%SZ"
        {id: $"($row.uid)_($at)", series: $row.uid}
    } else {
        {id: $row.uid}
    }
    let status = $row.status? | default "" | str lowercase
    let location_name = $row.location? | html to-text
    let has_location = ($location_name | is-not-empty) or ($row.place? | is-not-empty)
    let has_web_url = ($row.url? | default "") =~ '^https?://'
    let leftovers = $row.props? | default [] | where name not-in $MAPPED_PROPS | group-by name
        | items {|name, props|
            let values = $props | get value
            [($name | str lowercase) (if ($values | length) == 1 { $values.0 } else { $values })]
        }
        | into record
    let extra = $leftovers
        | merge ($row.extra? | default {})
        | merge (if $has_web_url or $row.url? == null { {} } else { {url: $row.url} })
        | merge (if $has_location or $row.geo? == null { {} } else { {geo: $row.geo} })
    let location = if $has_location {
        {name: $location_name, place: $row.place?, geo: $row.geo?} | drop-empty
    }
    let content = if ($row.description_html? | is-not-empty) {
        {type: html, value: $row.description_html}
    }

    {type: event}
    | merge $identity
    | merge {
        source: $source
        summary: ($row.summary? | html to-text)
        start: $row.start?
        end: $row.end?
        all_day: ($row.all_day? | default false)
        tz: ($row.tz? | default $tz)
        status: (if $status in [tentative confirmed cancelled] { $status })
        location: $location
        attendance: $row.attendance?
        online: $row.online?
        excerpt: ($row.excerpt? | html to-text)
        description: ($row.description? | default ($row.description_html? | html to-text) | if $in != null { str trim })
        content: $content
        url: (if $has_web_url { $row.url })
        links: ($row.links? | default [])
        images: ($row.images? | default [])
        cost: $row.cost?
        categories: ($row.categories? | default [] | uniq)
        keywords: ($row.keywords? | default [] | uniq)
        organizers: ($row.organizers? | default ($row.organizer? | organizers))
        rrule: $row.rrule?
        rdates: ($row.rdates? | default [])
        exdates: ($row.exdates? | default [])
        recurrence_id: $row.recurrence_id?
        created: $row.created?
        modified: $row.last_modified?
        featured: $row.featured?
        hidden: $row.hidden?
        extra: $extra
    }
    | drop-empty
}

# An ICS organizer record ({name, email, cutype}) as a list of stdlib agents.
def organizers []: any -> list<record> {
    let organizer = $in
    if $organizer == null { return [] }
    let name = $organizer.name? | default $organizer.email?
    if $name == null { return [] }
    let emails = if $organizer.email? != null { [{address: $organizer.email}] } else { [] }
    if $organizer.cutype? == "INDIVIDUAL" {
        [{person: ({name: {display: $name}, emails: $emails} | drop-empty)}]
    } else {
        [{org: ({name: $name, emails: $emails} | drop-empty)}]
    }
}

# Turn events from The Events Calendar's WordPress REST API
# (`/wp-json/tribe/events/v1/events`, the `events` list of each page) into
# the rows `into event` reads.
export def "from tribe-events" []: list<any> -> table {
    each {|event| $event | tribe-row }
}

def tribe-row []: record -> record {
    let event = $in
    let utc = {|text| $"($text)+00:00" | into datetime }
    let start = do $utc $event.utc_start_date
    let end = do $utc $event.utc_end_date
    # All-day events end at 23:59:59 on their last day; events end exclusively.
    let end = if $event.all_day and ($event.end_date | str ends-with "23:59:59") { $end + 1sec } else { $end }
    let venue = $event.venue? | if ($in | describe | str starts-with record) { $in }
    let free = ($event.cost_details?.values? | default [] | any {|v| $v == free }) or ($event.cost? =~ '(?i)^\s*free\s*$')
    let amounts = $event.cost_details?.values? | default [] | where {|v| $v =~ '^[0-9.]+$' } | into float
    let cost = if $free {
        {free: true}
    } else if ($amounts | is-not-empty) {
        {min: ($amounts | math min), max: (if ($amounts | length) > 1 { $amounts | math max }), currency: $event.cost_details.currency_code?} | drop-empty
    }
    let website = $event.website? | default "" | html to-text | str trim | str replace -r '(%20)+$' ''
    let image = $event.image? | if ($in | describe | str starts-with record) { $in }
    {
        uid: ($event.global_id? | default ($event.id | into string))
        summary: $event.title
        start: $start
        end: $end
        all_day: $event.all_day
        tz: $event.timezone?
        description_html: $event.description?
        excerpt: $event.excerpt?
        url: $event.url?
        links: (if $website =~ '^https?://' { [{url: $website, rel: related}] } else { [] })
        place: (if $venue != null { $venue | tribe-place })
        organizers: ($event.organizer? | default [] | each {|o| {org: ($o | tribe-org)} })
        cost: $cost
        categories: ($event.categories? | default [] | get name? | default [] | each { html to-text })
        keywords: ($event.tags? | default [] | get name? | default [] | each { html to-text })
        images: (if $image != null { [({url: $image.url, alt: ($event.title | html to-text), width: $image.width?, height: $image.height?} | drop-empty)] } else { [] })
        online: (if ($event.virtual_url? | default "") =~ '^https?://' { {url: $event.virtual_url} })
        created: (do $utc $event.date_utc)
        last_modified: (do $utc $event.modified_utc)
        featured: $event.featured?
        hidden: $event.hide_from_listings?
        extra: (if $cost == null and ($event.cost? | is-not-empty) { {cost: $event.cost} } else { {} })
    }
}

# A Tribe venue as a stdlib place.
def tribe-place []: record -> record {
    let venue = $in
    let country = match ($venue.country? | default "" | str trim | str lowercase) {
        "united states" | "united states of america" | "usa" | "us" => "US"
        "canada" | "ca" => "CA"
        _ => null
    }
    let lat = $venue.geo_lat? | default "" | into string
    let lon = $venue.geo_lng? | default "" | into string
    {
        id: ($venue.id? | into string)
        name: ($venue.venue | html to-text)
        address: ({
            lines: (if ($venue.address? | is-not-empty) { [$venue.address] } else { [] })
            locality: $venue.city?
            region: ($venue.stateprovince? | default $venue.state?)
            postcode: $venue.zip?
            country: $country
        } | drop-empty)
        geo: (if ($lat =~ '^-?[0-9.]+$') and ($lon =~ '^-?[0-9.]+$') { {lat: ($lat | into float), lon: ($lon | into float)} })
        url: (if ($venue.url? | default "") =~ '^https?://' { $venue.url })
    }
    | drop-empty
}

# A Tribe organizer as a stdlib org.
def tribe-org []: record -> record {
    let organizer = $in
    {
        id: ($organizer.id? | into string)
        name: ($organizer.organizer | html to-text)
        emails: (if ($organizer.email? | is-not-empty) { [{address: ($organizer.email | html to-text)}] } else { [] })
        phones: (if ($organizer.phone? | default "") =~ '^[0-9+()./ x-]{3,}$' { [{number: $organizer.phone}] } else { [] })
        url: (if ($organizer.website? | default "") =~ '^https?://' { $organizer.website })
    }
    | drop-empty
}

# Turn events from Presence's events API (`https://api.presence.io/<campus>/v1/events`)
# into the rows `into event` reads. Presence times are instants, so the rows
# have no `tz`.
export def "from presence" [
    --photos: string  # Base URL that cover photos' file names are appended to
]: list<any> -> table {
    each {|event|
        let organizers = [
            ({org: {name: $event.organizationName}})
            (if ($event.contactName? | is-not-empty) {
                {person: ({name: {display: $event.contactName}, emails: (if ($event.contactEmail? | is-not-empty) { [{address: $event.contactEmail}] } else { [] })} | drop-empty)}
            })
        ] | compact
        {
            uid: $event.eventNoSqlId
            summary: $event.eventName
            start: ($event.startDateTimeUtc | into datetime)
            end: ($event.endDateTimeUtc? | if $in != null { into datetime })
            description_html: $event.description?
            location: $event.location?
            url: $"https://($event.subdomain).presence.io/event/($event.uri)"
            images: (if $event.hasCoverImage? == true and $photos != null {
                [{url: $"($photos)($event.photoUriWithVersion)", alt: $event.eventName}]
            } else { [] })
            organizers: $organizers
            keywords: ($event.tags? | default [])
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ppp-core`
Expected: every suite passes; `sources` 6.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add stdlib/pipes fixtures/json crates/ppp-core/tests
git commit -m "Add the Tribe and Presence readers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---

### Task 3: The acceptance pipes

**Files:** Create `pipes/{stolaf-wp,presence,carleton-week,northfield,all-campus}.nu`; Modify `pipes/campus-week.nu` (adds `--from`), `crates/ppp-cli/tests/run.rs` (live test for every pipe); Test `crates/ppp-core/tests/acceptance.rs`.

**Interfaces:** Consumes everything above plus `pipe get`, `expand rrule`, `during`. Produces the six pipes of spec §7.

- [ ] **Step 1: Write the failing test**

`crates/ppp-core/tests/acceptance.rs`:

```rust
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
    assert!(request.contains("start_date=2026-08-31"), "{request}");
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
```

`crates/ppp-cli/tests/run.rs`:

```rust
//! `ppp run` runs pipes, passes their flags, and reports failures with the
//! right exit codes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value as Json;

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn ppp(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ppp"))
        .args(args)
        .current_dir(repo(""))
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn records(out: &Output) -> Vec<Json> {
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    serde_json::from_slice::<Vec<Json>>(&out.stdout).unwrap()
}

#[test]
fn runs_a_pipe_by_path() {
    let out = ppp(&["run", "fixtures/pipes/two-events.nu"]);
    let events = records(&out);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["start"], "2026-10-13T18:00:00");
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn runs_a_pipe_by_name() {
    let out = ppp(&["run", "--pipes-dir", "fixtures/pipes", "two-events"]);
    assert_eq!(records(&out).len(), 2);
}

#[test]
fn passes_flags_to_the_pipe() {
    let out = ppp(&[
        "run",
        "fixtures/pipes/two-events.nu",
        "--count",
        "1",
        "--prefix=Club: ",
        "--loud",
    ]);
    let events = records(&out);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["summary"], "CLUB: CHESS CLUB");
}

#[test]
fn nushell_reports_flag_errors_and_they_exit_2() {
    for (args, code) in [
        (vec!["--days", "3"], "nu::parser::unknown_flag"),
        (vec!["--count", "two"], "nu::parser::parse_mismatch"),
        (vec!["--count"], "nu::parser::missing_flag_param"),
        (vec!["3"], "nu::parser::extra_positional"),
    ] {
        let mut all = vec!["run", "fixtures/pipes/two-events.nu"];
        all.extend(&args);
        let out = ppp(&all);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert_eq!(text(&out.stdout), "", "{args:?}");
        assert!(
            text(&out.stderr).contains(code),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn invalid_records_exit_1() {
    let out = ppp(&["run", "fixtures/pipes/invalid-event.nu"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("invalid-event: record 0: \"start\" is a required property"),
        "{stderr}"
    );
    assert!(
        stderr.ends_with("invalid-event: 1 error in 1 record\n"),
        "{stderr}"
    );
}

#[test]
fn pipes_that_cannot_load_exit_2() {
    let missing = ppp(&["run", "fixtures/pipes/nope.nu"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(text(&missing.stderr).starts_with("ppp: fixtures/pipes/nope.nu: "));

    let no_const = ppp(&["run", "fixtures/pipes/no-const.nu"]);
    assert_eq!(no_const.status.code(), Some(2));
}

/// Fetches the live St. Olaf calendar. Run with `cargo test -- --ignored`.
#[test]
#[ignore]
fn campus_week_live() {
    let out = ppp(&["run", "--no-cache", "campus-week", "--within", "3day"]);
    let events = records(&out);
    assert!(!events.is_empty());
    assert!(
        events
            .iter()
            .all(|e| e["source"] == "https://www.stolaf.edu/apps/calendar/ical.cfm")
    );
}

#[test]
fn runtime_errors_exit_2_with_the_diagnostic() {
    let out = ppp(&["run", "fixtures/pipes/fails.nu"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(text(&out.stdout), "");
    let stderr = text(&out.stderr);
    assert!(stderr.contains("the source is down"), "{stderr}");
    // The pipe must load and then fail while running, not fail to parse.
    assert!(!stderr.contains("nu::parser"), "{stderr}");
}

#[test]
fn a_switch_set_to_false_is_off() {
    let out = ppp(&["run", "fixtures/pipes/two-events.nu", "--loud=false"]);
    assert_eq!(records(&out)[0]["summary"], "Chess Club");
}

#[test]
fn warnings_are_printed_and_the_run_succeeds() {
    let out = ppp(&["run", "fixtures/pipes/warns.nu"]);
    assert_eq!(records(&out).len(), 0);
    let stderr = text(&out.stderr);
    assert!(stderr.contains("old-helper is deprecated"), "{stderr}");
}

#[test]
fn a_pipe_past_its_timeout_is_stopped() {
    let started = std::time::Instant::now();
    let out = ppp(&["run", "--timeout", "1", "fixtures/pipes/slow.nu"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(text(&out.stdout), "");
    assert_eq!(
        text(&out.stderr),
        "ppp: slow ran longer than 1s and was stopped\n"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn pipe_get_finds_pipes_in_the_pipes_dir() {
    let out = ppp(&["run", "--pipes-dir", "fixtures/pipes", "gets-two-events"]);
    let events = records(&out);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["summary"], "Got: Chess Club");
}

#[test]
fn cache_dir_and_no_cache_conflict() {
    let out = ppp(&[
        "run",
        "--no-cache",
        "--cache-dir",
        "x",
        "fixtures/pipes/two-events.nu",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("cannot be used with"),
        "{}",
        text(&out.stderr)
    );
}

/// Fetches each live source. Run with `cargo test -- --ignored`.
#[test]
#[ignore]
fn every_acceptance_pipe_live() {
    for (pipe, within) in [
        ("stolaf-wp", "1wk"),
        ("presence", "1wk"),
        ("carleton-week", "3day"),
        ("northfield", "520wk"),
        ("all-campus", "3day"),
    ] {
        let out = ppp(&["run", "--no-cache", pipe, "--within", within]);
        assert_eq!(out.status.code(), Some(0), "{pipe}: {}", text(&out.stderr));
        assert!(!records(&out).is_empty(), "{pipe} returned no events");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test acceptance`
Expected: all 5 FAIL — the pipe files don't exist yet.

- [ ] **Step 3: Write the code**

`pipes/stolaf-wp.nu`:

```nu
# Upcoming events from St. Olaf's WordPress calendar (The Events Calendar).
# It has venues, costs and organizers that the iCalendar feed lacks.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# St. Olaf events (WordPress)
#
# Upcoming events from St. Olaf's WordPress calendar, with venues and costs.
@category pipes
@search-terms calendar events
@example "Next three days" { stolaf-wp --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
    --max-pages: int = 20     # Safety cap on requests
    --source: string = "https://wp.stolaf.edu/calendar/wp-json/tribe/events/v1/events"  # The API's events URL
]: nothing -> table {
    use pipes *
    let start = $from | default (date now)
    let until = $start + $within
    # The API reads its date filters in the calendar's own zone.
    let local = {|at| $at | date to-timezone America/Chicago | format date "%Y-%m-%d %H:%M:%S" }
    let first = $source
        | url parse
        | merge {params: {per_page: 50, start_date: (do $local $start), end_date: (do $local $until)}}
        | url join
    # Follow `next_rest_url`, but never past --max-pages: a partial calendar
    # that looks complete is worse than a failed run.
    let pages = generate {|state|
        let page = http get $state.url
        let next = $page.next_rest_url?
        if ($next | is-empty) {
            {out: $page}
        } else if $state.count >= $max_pages {
            error make {msg: $"more than ($max_pages) pages; raise --max-pages"}
        } else {
            {out: $page, next: {url: $next, count: ($state.count + 1)}}
        }
    } {url: $first, count: 1}
    | collect
    $pages
    | each { $in.events? | default [] }
    | flatten
    | from tribe-events
    | during $start $until
    | into event --source $source
}
```

`pipes/presence.nu`:

```nu
# Upcoming events from St. Olaf's student organizations, on Presence.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# Student org events
#
# Upcoming events from St. Olaf student organizations (Presence).
@category pipes
@search-terms calendar events student organizations
@example "Next three days" { presence --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
    --source: string = "https://api.presence.io/stolaf/v1/events"  # The API's events URL
    --photos: string = "https://stolaf-cdn.presence.io/event-photos/09ddef77-5009-4348-8540-c9bfc6ade6bc/"  # Where cover photos live
]: nothing -> table {
    use pipes *
    let start = $from | default (date now)
    http get $source
    | from presence --photos $photos
    | during $start ($start + $within)
    | into event --source $source
}
```

`pipes/carleton-week.nu`:

```nu
# Upcoming events from the Carleton College campus calendar.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# This week at Carleton
#
# Upcoming events from the Carleton campus calendar.
@category pipes
@search-terms calendar events carleton
@example "Next three days" { carleton-week --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
    --source: string = "https://www.carleton.edu/calendar/?loadFeed=calendar&stamp=1714843628"  # The iCalendar feed
]: nothing -> table {
    use pipes *
    let start = $from | default (date now)
    let until = $start + $within
    http get --raw $source
    | from ics
    # Carleton puts HTML in DESCRIPTION, a TEXT field, and HTML-escapes its URLs.
    | rename --column {description: description_html}
    | update url {|row| $row.url | html to-text }
    | expand rrule --from $start --until $until
    | during $start $until
    # The feed merges several calendars, so one event can appear more than once.
    | uniq-by summary start
    | into event --source $source
}
```

`pipes/northfield.nu`:

```nu
# Upcoming events from the City of Northfield's community calendar.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 6hr
}

# Northfield community events
#
# Upcoming events from the City of Northfield's calendar.
@category pipes
@search-terms calendar events northfield city
@example "Next month" { northfield --within 4wk }
export def main [
    --within: duration = 4wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
    --source: string = "https://www.northfieldmn.gov/common/modules/iCalendar/iCalendar.aspx?catID=41&feed=calendar"  # The iCalendar feed
]: nothing -> table {
    use pipes *
    let start = $from | default (date now)
    let until = $start + $within
    http get --raw $source
    | from ics
    # Each event's URL points back at the feed; its page is in the description.
    # Locations are HTML with a trailing " -".
    | update url {|row|
        let page = $row.description? | default "" | parse -r '(?<url>https?://\S+)' | get url.0?
        $page | default ($row.url | if $in != null { url parse --base $source | url join })
    }
    | update location {|row|
        if $row.location != null { $row.location | html to-text | str replace -r '\s*-\s*$' '' }
    }
    | expand rrule --from $start --until $until
    | during $start $until
    | into event --source $source
}
```

`pipes/all-campus.nu`:

```nu
# Everything happening around campus: St. Olaf (both calendars and student
# orgs), Carleton and Northfield, in one list.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# All campus events
#
# Events from every campus pipe, without duplicates, earliest first.
@category pipes
@search-terms calendar events
@example "Next three days" { all-campus --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
]: nothing -> table {
    let args = {within: $within} | merge (if $from != null { {from: $from} } else { {} })
    [campus-week stolaf-wp presence carleton-week northfield]
    | each {|name| pipe get $name $args }
    | flatten
    # St. Olaf's two calendars list many of the same events.
    | uniq-by summary start
    | sort-by start
}
```

`pipes/campus-week.nu`:

```nu
# Upcoming events from the St. Olaf calendar. Run with `ppp run campus-week`;
# the host reads everything above `main`'s body without running it.

# Pipe metadata that has no built-in attribute.
export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

const SOURCE = "https://www.stolaf.edu/apps/calendar/ical.cfm"

# This week on campus
#
# Upcoming events from the St. Olaf calendar.
@category pipes
@search-terms calendar events
@example "Next three days" { campus-week --within 3day }
export def main [
    --within: duration = 1wk  # How far ahead to look
    --from: datetime          # Start of the window (default: now)
]: nothing -> table {
    use pipes *
    let now = $from | default (date now)
    let until = $now + $within
    http get --raw $SOURCE
    | from ics
    | expand rrule --from $now --until $until
    | during $now $until
    | into event --source $SOURCE
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: every suite passes, 190 tests. Then `cargo test -p ppp-cli --test run -- --ignored` passes against every live source.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add pipes crates/ppp-core/tests/acceptance.rs crates/ppp-cli/tests/run.rs
git commit -m "Add the acceptance pipes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---


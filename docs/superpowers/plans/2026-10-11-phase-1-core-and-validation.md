# ppp-pipes Phase 1: Core and Validation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust workspace whose `ppp-core` library validates `event` records against the stdlib JSON Schemas and turns nushell records into schema-correct JSON, plus a `ppp validate` command.

**Architecture:** The `common` and `calendar` schemas live once in `stdlib/` and are embedded with `include_str!`. `schema.rs` builds an offline `jsonschema` registry and compiles one validator per record type (each `$def` with an `$anchor`). `time_fields.rs` walks each record type's schema to find its time fields, and `serialize.rs` uses that map to write nushell datetimes as instants, dates or local times. `ppp-cli` is a thin `clap` front end.

**Tech Stack:** Rust 2024 edition (developed on rustc 1.98.0), `jsonschema` 0.58.6, `jiff` 0.2.38, `nu-protocol` 0.116.1, `chrono` 0.4.45 (only because nushell's `Value::Date` is chrono), `serde_json`, `thiserror` 2, `clap` 4.

**Spec:** [`docs/superpowers/specs/2026-10-11-ppp-pipes-cli-prototype-design.md`](../specs/2026-10-11-ppp-pipes-cli-prototype-design.md) — this plan is build-order step 1. Field conventions come from [`brainstorm.md`](../../../brainstorm.md) §0–2 and §7.

Every code block below was compiled and tested in a scratch workspace before this plan was written: the final state passes `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` with no warnings. The code is shown already formatted by `cargo fmt`.

## Global Constraints

- JSON Schema draft **2020-12**; schema `$id`s are `https://pipes.frogpond.tech/stdlib/<name>/1`.
- `stdlib/` holds the only copy of each schema; Rust embeds it with `include_str!`. Validation never fetches: `jsonschema` is built with `default-features = false` and `.offline()`.
- Format assertion is on, and schemas also carry `pattern`s, so results don't depend on validator settings.
- Records are closed (`unevaluatedProperties: false`) except for the open `extra` object.
- Nushell crates are pinned with `=0.116.1`.
- `jiff` uses the `tzdb-bundle-always` feature so results don't depend on the machine's zoneinfo. All our time logic uses jiff; chrono appears only in `time.rs` (the conversion) and in tests that build nushell values.
- Never fall back to the machine's local time zone.
- `ppp` exit codes: **0** success, **1** validation failed, **2** usage or I/O error.
- Tests must pass with pristine output; `cargo clippy --all-targets` and `cargo fmt --check` must be clean before each commit.
- Commit messages are plain sentences (repo style) ending with the `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` trailer.

## Decisions made while planning

These refine the spec. Each is small; call them out in review if they look wrong.

1. **The time-field walker skips `if`/`then`/`else`.** The spec (§5.3) lists them as understood keywords. In practice they only narrow fields that `properties` already declares (e.g. "if `all_day`, `start` is a `date`"), and walking them would record conflicting forms for `start`. Time fields must therefore be declared under `properties`.
2. **Recursive `$ref`s are followed once.** A schema like `org.parent → org` would otherwise never end; a time field two levels down a recursion is written as an instant.
3. **Output drops fractional seconds.** Calendar sources use whole seconds, and a fixed width keeps JSON stable.
4. **`id` is optional on embedded `person`, `org` and `place`.** Sources often give a name only (e.g. KDL's `organizer "Music Department"`).
5. **Organizers are `{"org": …}` or `{"person": …}`.** The KDL examples were converted by judgment: departments and associations became orgs, "Priya Shah" became a person. Phase 2 must choose for ICS `ORGANIZER`, which doesn't say.
6. **Two custom formats:** `iana-tz` (checked against jiff's bundled tz database) and `local-date-time` (parsed by jiff).
7. **Event fields beyond brainstorm §7:** `rdates`, `series`, `recurrence_id` (spec §6.1), and `reminders[].before` for the KDL `reminder` duration.

## Review Focus

Inputs the spec implies but that no feature test would naturally hit. Each has a test in the owning task.

1. **A record that is not an object** (e.g. `42` in an array) must give one clean error, "record has no \"type\"", not a panic — Task 1.
2. **Long values with multi-byte characters** (e.g. "Dvořák") must be cut on character boundaries in error excerpts, not panic on a byte index — Task 1.
3. **Source keys containing `/` or `~`** (common in `extra`) must be escaped in error pointers per RFC 6901 — Task 4.
4. **Datetimes beyond year 9999** (chrono allows them; jiff doesn't) must be a `OutOfRange` error, not a panic — Task 4.
5. **JSONL files with CRLF line endings** must validate like LF files — Task 5.

---

## File structure

```
Cargo.toml                         workspace: members, shared dependency versions
.gitignore                         /target
crates/ppp-core/
  Cargo.toml
  src/lib.rs                       module list
  src/schema.rs                    Registry, RecordType, RecordError, SchemaError
  src/time_fields.rs               TimeKind, TimeTree, the schema walker
  src/time.rs                      chrono -> jiff conversion and the three output formats
  src/serialize.rs                 nushell Value -> JSON using a TimeTree
  tests/fixtures.rs                examples pass, invalid records fail where expected
  tests/time_fields.rs             walker behavior
  tests/serialize.rs               serializer behavior
crates/ppp-cli/
  Cargo.toml
  src/main.rs                      clap definitions
  src/validate.rs                  `ppp validate`
  tests/validate.rs                runs the built binary
stdlib/common.schema.json          shared definitions
stdlib/calendar.schema.json        the `event` record type
fixtures/examples/*.json           valid records (the four KDL examples, converted)
fixtures/invalid/*.json            one mistake per file
```

---

### Task 1: Workspace, stdlib schemas and the schema registry

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `crates/ppp-core/Cargo.toml`, `crates/ppp-core/src/lib.rs`, `crates/ppp-core/src/schema.rs`
- Create: `stdlib/common.schema.json`, `stdlib/calendar.schema.json`
- Create: `fixtures/examples/{minimal,campus-events,stolaf-feed,team-planning}.json`
- Test: `crates/ppp-core/tests/fixtures.rs`

**Interfaces:**
- Consumes: nothing.
- Produces (in `ppp_core::schema`):
  - `Registry::stdlib() -> Result<Registry, SchemaError>`
  - `Registry::from_documents(Vec<serde_json::Value>) -> Result<Registry, SchemaError>`
  - `Registry::record_type(&self, name: &str) -> Option<&RecordType>`
  - `Registry::validate(&self, index: usize, record: &serde_json::Value) -> Vec<RecordError>`
  - `pub struct RecordType { pub name: String, pub uri: String, /* private validator */ }`
  - `pub struct RecordError { pub index: usize, pub pointer: String, pub message: String, pub excerpt: String }` (derives `Debug, Clone, PartialEq, Eq`)
  - `pub enum SchemaError { Json, MissingId, Register { id, message }, Compile { name, message } }`

- [ ] **Step 1: Create the workspace skeleton**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/ppp-core"]

[workspace.package]
edition = "2024"

[workspace.dependencies]
chrono = "0.4.45"
clap = { version = "4", features = ["derive"] }
jiff = { version = "0.2.38", features = ["tzdb-bundle-always"] }
jsonschema = { version = "0.58.6", default-features = false }
nu-protocol = { version = "=0.116.1", default-features = false }
serde_json = "1"
thiserror = "2"
```

`.gitignore`:

```
/target
```

`crates/ppp-core/Cargo.toml`:

```toml
[package]
name = "ppp-core"
version = "0.1.0"
edition.workspace = true

[dependencies]
jiff.workspace = true
jsonschema.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

`crates/ppp-core/src/lib.rs`:

```rust
pub mod schema;
```

Create `crates/ppp-core/src/schema.rs` as an empty file.


- [ ] **Step 2: Write the example fixtures**

These are the four documents in `kdl-calendar-schema/examples/`, converted to `event` records. Calendar-level `tz` moves onto each record that has a local time; `venue` references become embedded `place`s; `color` is dropped.

`fixtures/examples/minimal.json`:

```json
[
  {
    "type": "event",
    "id": "dentist-2026-10-20",
    "summary": "Dentist",
    "start": "2026-10-20T08:15:00Z"
  }
]
```

`fixtures/examples/campus-events.json`:

```json
[
  {
    "type": "event",
    "id": "20261014-orchestra@calendar.example.edu",
    "source": "https://www.stolaf.edu/apps/calendar/ical.cfm",
    "featured": true,
    "summary": "Fall Orchestra Concert",
    "start": "2026-10-14T19:30:00",
    "end": "2026-10-14T21:00:00",
    "tz": "America/Chicago",
    "status": "confirmed",
    "location": {
      "place": {
        "id": "boe-chapel",
        "name": "Boe Memorial Chapel",
        "address": { "lines": ["1520 St. Olaf Avenue"], "locality": "Northfield", "region": "MN", "postcode": "55057", "country": "US" },
        "geo": { "lat": 44.4614, "lon": -93.1822 }
      }
    },
    "attendance": "hybrid",
    "online": { "url": "https://www.example.edu/live/orchestra", "platform": "youtube" },
    "excerpt": "The orchestra opens its season with Dvořák's Eighth.",
    "description": "The orchestra opens its season with Dvořák's Symphony No. 8.\nFree and open to the public.",
    "url": "https://www.example.edu/events/fall-orchestra",
    "links": [{ "url": "https://tickets.example.edu/orchestra", "rel": "tickets", "title": "Reserve seats" }],
    "images": [{ "url": "https://www.example.edu/img/orchestra.jpg", "alt": "The orchestra on stage in Boe Chapel" }],
    "cost": { "free": true, "note": "Tickets required" },
    "categories": ["Music", "Concert"],
    "keywords": ["Orchestra", "Dvořák"],
    "organizers": [{
      "org": {
        "name": "Music Department",
        "emails": [{ "address": "music@example.edu" }],
        "phones": [{ "number": "507-786-3180" }],
        "url": "https://www.example.edu/music/"
      }
    }],
    "created": "2026-08-06T18:11:07Z",
    "modified": "2026-09-02T15:53:15Z",
    "reminders": [{ "before": "PT1H", "method": "display" }]
  },
  {
    "type": "event",
    "id": "20261018-family-weekend@calendar.example.edu",
    "summary": "Family Weekend",
    "start": "2026-10-17",
    "end": "2026-10-19",
    "all_day": true,
    "tz": "America/Chicago",
    "status": "confirmed",
    "location": { "name": "Campus-wide" },
    "categories": ["Community"]
  },
  {
    "type": "event",
    "id": "weekly-chess@calendar.example.edu",
    "summary": "Chess Club",
    "cost": { "min": 5, "max": 10, "currency": "USD", "note": "Dues per semester; first visit free" },
    "start": "2026-09-08T18:00:00",
    "duration": "PT2H",
    "tz": "America/Chicago",
    "location": { "name": "Buntrock Commons, Room 143" },
    "categories": ["Clubs"],
    "rrule": "FREQ=WEEKLY;BYDAY=TU;UNTIL=20261215T235959Z",
    "exdates": ["2026-11-24T18:00:00"]
  },
  {
    "type": "event",
    "id": "20261022-lecture@calendar.example.edu",
    "summary": "Guest Lecture: Pipelines Everywhere",
    "start": "2026-10-22T16:00:00Z",
    "end": "2026-10-22T17:15:00Z",
    "status": "cancelled",
    "location": { "name": "Regents Hall 150" },
    "description": "Cancelled: the speaker's flight was grounded. A new date will be announced.",
    "organizers": [{ "org": { "name": "Computer Science" } }]
  }
]
```

`fixtures/examples/stolaf-feed.json`:

```json
[
  {
    "type": "event",
    "id": "wp.stolaf.edu/calendar?id=117282",
    "source": "https://wp.stolaf.edu/calendar/wp-json/tribe/events/v1/events",
    "summary": "Norwegian-American Historical Association Biennial Meeting",
    "start": "2026-10-10T09:00:00",
    "end": "2026-10-10T17:00:00",
    "tz": "America/Chicago",
    "location": { "place": { "id": "buntrock-ballrooms", "name": "Ballrooms in Buntrock Commons" } },
    "excerpt": "A day of Norwegian-American learning and networking at St. Olaf College",
    "url": "https://wp.stolaf.edu/calendar/event/norwegian-american-historical-association-biennial-meeting/",
    "links": [{ "url": "https://norwegianamericanhistory.org/events/", "rel": "info" }],
    "cost": { "min": 90, "currency": "USD" },
    "categories": ["Speakers/Lectures"],
    "organizers": [{
      "org": {
        "name": "Norwegian-American Historical Association",
        "emails": [{ "address": "naha@stolaf.edu" }],
        "phones": [{ "number": "507-786-3221" }],
        "url": "https://naha.stolaf.edu"
      }
    }],
    "created": "2026-09-29T19:07:02Z",
    "modified": "2026-09-29T19:43:10Z"
  },
  {
    "type": "event",
    "id": "wp.stolaf.edu/calendar?id=115631",
    "source": "https://wp.stolaf.edu/calendar/wp-json/tribe/events/v1/events",
    "summary": "St. Olaf Orchestra at Troy High School (Troy, MI)",
    "start": "2026-10-12T19:00:00",
    "end": "2026-10-12T21:00:00",
    "tz": "America/Chicago",
    "url": "https://wp.stolaf.edu/calendar/event/st-olaf-orchestra-at-troy-high-school-troy-mi/",
    "links": [{ "url": "https://www.eventbrite.com/e/st-olaf-orchestra-at-troy-high-school-troy-mi-tickets-1994343429885?aff=oddtdtcreator&keep_tld=true", "rel": "tickets" }],
    "categories": ["Music"],
    "keywords": ["Orchestra", "St. Olaf Orchestra"],
    "organizers": [{ "org": { "name": "Music Organizations" } }],
    "created": "2026-08-06T18:26:30Z",
    "modified": "2026-09-02T15:53:56Z"
  },
  {
    "type": "event",
    "id": "wp.stolaf.edu/calendar?id=114660",
    "source": "https://wp.stolaf.edu/calendar/wp-json/tribe/events/v1/events",
    "summary": "Fall Break (no classes)",
    "start": "2026-10-10",
    "end": "2026-10-14",
    "all_day": true,
    "tz": "America/Chicago",
    "url": "https://wp.stolaf.edu/calendar/event/fall-break-no-classes-3/",
    "categories": ["Academic Year"],
    "created": "2026-05-07T21:12:36Z",
    "modified": "2026-09-04T15:08:44Z"
  }
]
```

`fixtures/examples/team-planning.json`:

```json
[
  {
    "type": "event",
    "id": "c1f6a3e2-5b7d-4e0a-9f3c-1d2e3f4a5b6c",
    "summary": "Quarterly planning",
    "start": "2026-11-02T10:00:00",
    "end": "2026-11-02T15:00:00",
    "tz": "Europe/London",
    "status": "tentative",
    "location": { "name": "London office, 4th floor" },
    "attendance": "hybrid",
    "online": { "url": "https://meet.example.com/q4-planning" },
    "organizers": [{
      "person": {
        "name": { "display": "Priya Shah" },
        "emails": [{ "address": "priya@example.com" }],
        "phones": [{ "number": "+44 20 7946 0000" }]
      }
    }],
    "attendees": [
      { "person": { "name": { "display": "Priya Shah" }, "emails": [{ "address": "priya@example.com" }] }, "role": "chair", "rsvp": "accepted" },
      { "person": { "name": { "display": "Sam Okafor" }, "emails": [{ "address": "sam@example.com" }] }, "role": "required", "rsvp": "needs-action" },
      { "person": { "name": { "display": "Lee Wong" } }, "role": "optional", "rsvp": "declined" }
    ],
    "reminders": [{ "before": "P1D", "method": "email" }, { "before": "PT15M" }]
  },
  {
    "type": "event",
    "id": "standup-2026@product.example.com",
    "summary": "Daily standup",
    "start": "2026-10-12T09:30:00-04:00",
    "duration": "PT15M",
    "tz": "America/New_York",
    "attendance": "online",
    "online": { "url": "https://meet.example.com/standup", "platform": "meet" },
    "rrule": "FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR",
    "exdates": ["2026-11-26T09:30:00-05:00", "2026-11-27T09:30:00-05:00"]
  }
]
```

- [ ] **Step 3: Write the failing test**

`crates/ppp-core/tests/fixtures.rs`:

```rust
//! Every example record is valid, and every invalid record fails at the
//! place its file name describes.

use std::fs;
use std::path::{Path, PathBuf};

use ppp_core::schema::{RecordError, Registry};
use serde_json::Value as Json;

fn fixtures(dir: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(dir)
}

fn records(path: &Path) -> Vec<Json> {
    let text = fs::read_to_string(path).unwrap();
    match serde_json::from_str(&text).unwrap() {
        Json::Array(records) => records,
        other => panic!("{} is not a JSON array: {other}", path.display()),
    }
}

fn errors(registry: &Registry, path: &Path) -> Vec<RecordError> {
    records(path)
        .iter()
        .enumerate()
        .flat_map(|(i, record)| registry.validate(i, record))
        .collect()
}

#[test]
fn every_example_is_valid() {
    let registry = Registry::stdlib().unwrap();
    let mut checked = 0;
    for entry in fs::read_dir(fixtures("examples")).unwrap() {
        let path = entry.unwrap().path();
        let errors = errors(&registry, &path);
        assert!(errors.is_empty(), "{}: {errors:#?}", path.display());
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn records_that_are_not_objects_have_no_type() {
    let registry = Registry::stdlib().unwrap();
    let errors = registry.validate(3, &serde_json::json!(42));
    assert_eq!(errors.len(), 1);
    assert_eq!((errors[0].index, errors[0].pointer.as_str()), (3, ""));
    assert_eq!(errors[0].message, "record has no \"type\"");
}

#[test]
fn long_excerpts_are_cut_on_character_boundaries() {
    let registry = Registry::stdlib().unwrap();
    let summary = "Dvořák ".repeat(30);
    let record = serde_json::json!({
        "type": "event", "id": "x", "summary": summary, "start": "2026-10-22T16:00:00Z", "colour": "x"
    });
    let errors = registry.validate(0, &record);
    let root = errors.iter().find(|e| e.pointer.is_empty()).unwrap();
    assert_eq!(root.excerpt.chars().count(), 80);
    assert!(root.excerpt.ends_with('…'));
}
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test fixtures`
Expected: compile error, `unresolved import ppp_core::schema::RecordError` (and `Registry`).

- [ ] **Step 5: Write the schemas**

`stdlib/common.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://pipes.frogpond.tech/stdlib/common/1",
  "title": "Shared definitions for stdlib record types",
  "version": "1.0.0",
  "$defs": {
    "record": {
      "description": "Fields every record has. Record types merge this in with allOf.",
      "type": "object",
      "properties": {
        "type": { "type": "string", "pattern": "^[a-z][a-z0-9-]*$" },
        "id": { "$ref": "#/$defs/text" },
        "source": { "$ref": "#/$defs/url" },
        "partial": { "type": "boolean" },
        "href": { "$ref": "#/$defs/url" },
        "extra": { "type": "object" }
      },
      "required": ["type", "id"]
    },
    "text": { "type": "string", "minLength": 1 },
    "content": {
      "type": "object",
      "properties": {
        "type": { "enum": ["text", "html", "markdown"] },
        "value": { "type": "string" },
        "lang": { "$ref": "#/$defs/text" }
      },
      "required": ["type", "value"],
      "unevaluatedProperties": false
    },
    "date": {
      "type": "string",
      "pattern": "^\\d{4}-\\d{2}-\\d{2}$",
      "format": "date"
    },
    "instant": {
      "type": "string",
      "pattern": "^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(\\.\\d+)?(Z|[+-]\\d{2}:\\d{2})$",
      "format": "date-time"
    },
    "local-date-time": {
      "type": "string",
      "pattern": "^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(\\.\\d+)?$",
      "format": "local-date-time"
    },
    "when": {
      "anyOf": [
        { "$ref": "#/$defs/date" },
        { "$ref": "#/$defs/instant" },
        { "$ref": "#/$defs/local-date-time" }
      ]
    },
    "tz": { "type": "string", "format": "iana-tz" },
    "duration": {
      "type": "string",
      "pattern": "^-?P",
      "format": "duration"
    },
    "rrule": {
      "type": "string",
      "pattern": "(^|;)FREQ=(SECONDLY|MINUTELY|HOURLY|DAILY|WEEKLY|MONTHLY|YEARLY)(;|$)"
    },
    "url": {
      "type": "string",
      "pattern": "^https?://",
      "format": "uri"
    },
    "email": {
      "type": "object",
      "properties": {
        "address": { "type": "string", "format": "email" },
        "kind": { "enum": ["work", "home", "other"] }
      },
      "required": ["address"],
      "unevaluatedProperties": false
    },
    "phone": {
      "type": "object",
      "properties": {
        "number": { "type": "string", "pattern": "^[0-9+()./ x-]{3,}$" },
        "kind": { "enum": ["work", "home", "mobile", "fax", "other"] }
      },
      "required": ["number"],
      "unevaluatedProperties": false
    },
    "geo": {
      "type": "object",
      "properties": {
        "lat": { "type": "number", "minimum": -90, "maximum": 90 },
        "lon": { "type": "number", "minimum": -180, "maximum": 180 }
      },
      "required": ["lat", "lon"],
      "unevaluatedProperties": false
    },
    "address": {
      "type": "object",
      "properties": {
        "lines": { "type": "array", "items": { "$ref": "#/$defs/text" }, "minItems": 1 },
        "locality": { "$ref": "#/$defs/text" },
        "region": { "$ref": "#/$defs/text" },
        "postcode": { "$ref": "#/$defs/text" },
        "country": { "type": "string", "pattern": "^[A-Z]{2}$" }
      },
      "minProperties": 1,
      "unevaluatedProperties": false
    },
    "place": {
      "type": "object",
      "properties": {
        "id": { "$ref": "#/$defs/text" },
        "name": { "$ref": "#/$defs/text" },
        "address": { "$ref": "#/$defs/address" },
        "geo": { "$ref": "#/$defs/geo" },
        "url": { "$ref": "#/$defs/url" },
        "phones": { "type": "array", "items": { "$ref": "#/$defs/phone" } },
        "room": { "$ref": "#/$defs/text" }
      },
      "required": ["name"],
      "unevaluatedProperties": false
    },
    "person": {
      "type": "object",
      "properties": {
        "id": { "$ref": "#/$defs/text" },
        "name": {
          "type": "object",
          "properties": {
            "display": { "$ref": "#/$defs/text" },
            "given": { "$ref": "#/$defs/text" },
            "family": { "$ref": "#/$defs/text" }
          },
          "required": ["display"],
          "unevaluatedProperties": false
        },
        "emails": { "type": "array", "items": { "$ref": "#/$defs/email" } },
        "phones": { "type": "array", "items": { "$ref": "#/$defs/phone" } },
        "url": { "$ref": "#/$defs/url" }
      },
      "required": ["name"],
      "unevaluatedProperties": false
    },
    "org": {
      "type": "object",
      "properties": {
        "id": { "$ref": "#/$defs/text" },
        "name": { "$ref": "#/$defs/text" },
        "short_name": { "$ref": "#/$defs/text" },
        "emails": { "type": "array", "items": { "$ref": "#/$defs/email" } },
        "phones": { "type": "array", "items": { "$ref": "#/$defs/phone" } },
        "url": { "$ref": "#/$defs/url" }
      },
      "required": ["name"],
      "unevaluatedProperties": false
    },
    "agent": {
      "description": "A person or an organization, e.g. an organizer or author.",
      "oneOf": [
        {
          "type": "object",
          "properties": { "person": { "$ref": "#/$defs/person" } },
          "required": ["person"]
        },
        {
          "type": "object",
          "properties": { "org": { "$ref": "#/$defs/org" } },
          "required": ["org"]
        }
      ]
    },
    "link": {
      "type": "object",
      "properties": {
        "url": { "$ref": "#/$defs/url" },
        "rel": { "type": "string", "pattern": "^[a-z][a-z0-9.-]*$" },
        "title": { "$ref": "#/$defs/text" },
        "type": { "$ref": "#/$defs/text" }
      },
      "required": ["url"],
      "unevaluatedProperties": false
    },
    "image": {
      "type": "object",
      "properties": {
        "url": { "$ref": "#/$defs/url" },
        "alt": { "type": "string" },
        "width": { "type": "integer", "minimum": 1 },
        "height": { "type": "integer", "minimum": 1 },
        "caption": { "$ref": "#/$defs/text" },
        "credit": { "$ref": "#/$defs/text" }
      },
      "required": ["url"],
      "unevaluatedProperties": false
    },
    "money": {
      "oneOf": [
        {
          "type": "object",
          "properties": {
            "free": { "const": true },
            "note": { "$ref": "#/$defs/text" }
          },
          "required": ["free"],
          "unevaluatedProperties": false
        },
        {
          "type": "object",
          "properties": {
            "min": { "type": "number", "minimum": 0 },
            "max": { "type": "number", "minimum": 0 },
            "currency": { "type": "string", "pattern": "^[A-Z]{3}$" },
            "per": { "enum": ["hour", "week", "semester", "item"] },
            "note": { "$ref": "#/$defs/text" }
          },
          "required": ["min", "currency"],
          "unevaluatedProperties": false
        }
      ]
    },
    "categories": {
      "type": "array",
      "items": { "$ref": "#/$defs/text" },
      "minItems": 1,
      "uniqueItems": true
    },
    "keywords": {
      "type": "array",
      "items": { "$ref": "#/$defs/text" },
      "minItems": 1,
      "uniqueItems": true
    },
    "created": { "$ref": "#/$defs/instant" },
    "modified": { "$ref": "#/$defs/instant" }
  }
}
```

`stdlib/calendar.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://pipes.frogpond.tech/stdlib/calendar/1",
  "title": "Calendar record types",
  "version": "1.0.0",
  "$defs": {
    "event": {
      "$anchor": "event",
      "description": "A calendar event, modeled on iCalendar VEVENT.",
      "type": "object",
      "allOf": [
        { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/record" },
        {
          "if": { "properties": { "partial": { "const": true } }, "required": ["partial"] },
          "else": { "required": ["summary", "start"] }
        },
        {
          "not": { "required": ["end", "duration"] }
        },
        {
          "if": { "properties": { "all_day": { "const": true } }, "required": ["all_day"] },
          "then": {
            "properties": {
              "start": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/date" },
              "end": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/date" }
            }
          }
        },
        {
          "if": { "properties": { "start": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/local-date-time" } }, "required": ["start"] },
          "then": { "required": ["tz"] }
        },
        {
          "if": { "properties": { "end": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/local-date-time" } }, "required": ["end"] },
          "then": { "required": ["tz"] }
        },
        {
          "if": { "properties": { "recurrence_id": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/local-date-time" } }, "required": ["recurrence_id"] },
          "then": { "required": ["tz"] }
        },
        {
          "if": { "properties": { "rdates": { "contains": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/local-date-time" } } }, "required": ["rdates"] },
          "then": { "required": ["tz"] }
        },
        {
          "if": { "properties": { "exdates": { "contains": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/local-date-time" } } }, "required": ["exdates"] },
          "then": { "required": ["tz"] }
        }
      ],
      "properties": {
        "type": { "const": "event" },
        "summary": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/text" },
        "start": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/when" },
        "end": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/when" },
        "duration": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/duration" },
        "all_day": { "type": "boolean" },
        "tz": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/tz" },
        "status": { "enum": ["tentative", "confirmed", "cancelled"] },
        "location": {
          "type": "object",
          "properties": {
            "name": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/text" },
            "place": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/place" },
            "geo": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/geo" }
          },
          "anyOf": [{ "required": ["name"] }, { "required": ["place"] }],
          "unevaluatedProperties": false
        },
        "attendance": { "enum": ["in-person", "online", "hybrid"] },
        "online": {
          "type": "object",
          "properties": {
            "url": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/url" },
            "platform": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/text" }
          },
          "required": ["url"],
          "unevaluatedProperties": false
        },
        "excerpt": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/text" },
        "description": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/text" },
        "content": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/content" },
        "url": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/url" },
        "links": { "type": "array", "items": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/link" } },
        "images": { "type": "array", "items": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/image" } },
        "cost": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/money" },
        "categories": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/categories" },
        "keywords": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/keywords" },
        "organizers": { "type": "array", "items": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/agent" } },
        "attendees": {
          "type": "array",
          "items": {
            "type": "object",
            "allOf": [{ "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/agent" }],
            "properties": {
              "role": { "enum": ["chair", "required", "optional"] },
              "rsvp": { "enum": ["needs-action", "accepted", "declined", "tentative"] }
            },
            "unevaluatedProperties": false
          }
        },
        "rrule": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/rrule" },
        "rdates": { "type": "array", "items": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/when" } },
        "exdates": { "type": "array", "items": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/when" } },
        "series": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/text" },
        "recurrence_id": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/when" },
        "reminders": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "before": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/duration" },
              "method": { "enum": ["display", "email"] }
            },
            "required": ["before"],
            "unevaluatedProperties": false
          }
        },
        "created": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/created" },
        "modified": { "$ref": "https://pipes.frogpond.tech/stdlib/common/1#/$defs/modified" },
        "featured": { "type": "boolean" },
        "hidden": { "type": "boolean" }
      },
      "unevaluatedProperties": false
    }
  }
}
```

- [ ] **Step 6: Write the registry**

`crates/ppp-core/src/schema.rs`:

```rust
//! The stdlib schema registry: compiles a validator for every record type,
//! and validates records against them.
//!
//! Schemas are embedded in the binary and resolved by `$id`. Nothing is ever
//! fetched, so validation is offline and deterministic.

use std::collections::BTreeMap;

use serde_json::Value as Json;

/// The stdlib schema documents, in the order they are registered.
const STDLIB: &[&str] = &[
    include_str!("../../../stdlib/common.schema.json"),
    include_str!("../../../stdlib/calendar.schema.json"),
];

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("schema document is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("schema document has no \"$id\"")]
    MissingId,
    #[error("could not register schema {id}: {message}")]
    Register { id: String, message: String },
    #[error("could not compile record type {name}: {message}")]
    Compile { name: String, message: String },
}

/// A validation failure for one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordError {
    /// Position of the record in its list (0-based).
    pub index: usize,
    /// JSON Pointer to the failing value inside the record; empty for the whole record.
    pub pointer: String,
    pub message: String,
    /// The failing value as compact JSON, cut to 80 characters.
    pub excerpt: String,
}

/// One record type: a `$def` in a stdlib schema that declares an `$anchor`.
pub struct RecordType {
    /// The value of the record's `type` field, e.g. `event`.
    pub name: String,
    /// `<schema $id>#<anchor>`, e.g. `https://pipes.frogpond.tech/stdlib/calendar/1#event`.
    pub uri: String,
    validator: jsonschema::Validator,
}

pub struct Registry {
    types: BTreeMap<String, RecordType>,
}

impl Registry {
    /// The registry of embedded stdlib schemas.
    pub fn stdlib() -> Result<Registry, SchemaError> {
        let documents = STDLIB
            .iter()
            .map(|text| serde_json::from_str(text))
            .collect::<Result<Vec<Json>, _>>()?;
        Registry::from_documents(documents)
    }

    /// Builds a registry from schema documents. Each must have an `$id`.
    pub fn from_documents(documents: Vec<Json>) -> Result<Registry, SchemaError> {
        let mut by_id = BTreeMap::new();
        for doc in documents {
            let id = doc
                .get("$id")
                .and_then(Json::as_str)
                .ok_or(SchemaError::MissingId)?
                .to_string();
            by_id.insert(id, doc);
        }

        let mut builder = jsonschema::Registry::new();
        for (id, doc) in &by_id {
            builder = builder
                .add(id.as_str(), doc.clone())
                .map_err(|e| SchemaError::Register {
                    id: id.clone(),
                    message: e.to_string(),
                })?;
        }
        let prepared = builder.prepare().map_err(|e| SchemaError::Register {
            id: "(registry)".into(),
            message: e.to_string(),
        })?;

        let mut types = BTreeMap::new();
        for (id, doc) in &by_id {
            let Some(defs) = doc.get("$defs").and_then(Json::as_object) else {
                continue;
            };
            for def in defs.values() {
                let Some(anchor) = def.get("$anchor").and_then(Json::as_str) else {
                    continue;
                };
                let uri = format!("{id}#{anchor}");
                // Draft 2020-12, formats asserted, no network or file access,
                // plus our two custom formats.
                let validator = jsonschema::draft202012::options()
                    .with_registry(&prepared)
                    .offline()
                    .should_validate_formats(true)
                    .with_format("iana-tz", |s: &str| jiff::tz::db().get(s).is_ok())
                    .with_format("local-date-time", |s: &str| {
                        s.parse::<jiff::civil::DateTime>().is_ok()
                    })
                    .build(&serde_json::json!({ "$ref": uri }))
                    .map_err(|e| SchemaError::Compile {
                        name: anchor.into(),
                        message: e.to_string(),
                    })?;
                types.insert(
                    anchor.to_string(),
                    RecordType {
                        name: anchor.to_string(),
                        uri,
                        validator,
                    },
                );
            }
        }

        Ok(Registry { types })
    }

    /// Looks up a record type by the value of a record's `type` field.
    pub fn record_type(&self, name: &str) -> Option<&RecordType> {
        self.types.get(name)
    }

    /// Validates one record against the record type its `type` field names.
    pub fn validate(&self, index: usize, record: &Json) -> Vec<RecordError> {
        let type_name = match record.get("type") {
            Some(Json::String(name)) => name,
            Some(other) => return vec![error(index, "/type", "\"type\" must be a string", other)],
            None => return vec![error(index, "", "record has no \"type\"", record)],
        };
        let Some(record_type) = self.record_type(type_name) else {
            let message = format!("unknown record type \"{type_name}\"");
            return vec![error(index, "/type", &message, &record["type"])];
        };
        record_type
            .validator
            .iter_errors(record)
            .map(|e| {
                error(
                    index,
                    &e.instance_path().to_string(),
                    &e.to_string(),
                    e.instance(),
                )
            })
            .collect()
    }
}

fn error(index: usize, pointer: &str, message: &str, value: &Json) -> RecordError {
    RecordError {
        index,
        pointer: pointer.to_string(),
        message: message.to_string(),
        excerpt: excerpt(value),
    }
}

fn excerpt(value: &Json) -> String {
    const LIMIT: usize = 80;
    let text = value.to_string();
    if text.chars().count() <= LIMIT {
        return text;
    }
    let cut: String = text.chars().take(LIMIT - 1).collect();
    format!("{cut}…")
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test fixtures`
Expected: `test result: ok. 3 passed`

If an example fails, the error names the record and pointer. Fix the schema when the example matches brainstorm §7; fix the example only if the conversion from KDL was wrong.

- [ ] **Step 8: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add Cargo.toml Cargo.lock .gitignore crates stdlib fixtures
git commit -m "Add stdlib schemas and schema registry

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 2: Invalid fixtures

Each file holds one record with one mistake. The test pins the JSON Pointer each mistake is reported at, so loosening the schema by accident fails a test.

**Files:**
- Create: `fixtures/invalid/*.json` (18 files)
- Modify: `crates/ppp-core/tests/fixtures.rs` (add a table and two tests)

**Interfaces:**
- Consumes: `Registry::stdlib`, `Registry::validate`, `RecordError` from Task 1.
- Produces: nothing new.

- [ ] **Step 1: Write the failing tests**

In `crates/ppp-core/tests/fixtures.rs`, insert this between `every_example_is_valid` and `records_that_are_not_objects_have_no_type`:

```rust
/// Each invalid fixture and the JSON Pointer its one mistake is reported at.
const INVALID: &[(&str, &str)] = &[
    ("agent-both.json", "/organizers/0"),
    ("all-day-with-time.json", "/start"),
    ("bad-currency.json", "/cost"),
    ("bad-duration.json", "/duration"),
    ("bad-geo.json", "/location/place/geo/lat"),
    ("bad-month.json", "/start"),
    ("bad-rrule.json", "/rrule"),
    ("bad-status.json", "/status"),
    ("bad-type.json", "/type"),
    ("bad-url.json", "/url"),
    ("empty-summary.json", "/summary"),
    ("end-and-duration.json", ""),
    ("local-exdate-without-tz.json", ""),
    ("local-without-tz.json", ""),
    ("missing-id.json", ""),
    ("missing-start.json", ""),
    ("unknown-field.json", ""),
    ("unknown-tz.json", "/tz"),
];

#[test]
fn every_invalid_record_fails_where_expected() {
    let registry = Registry::stdlib().unwrap();
    for (file, pointer) in INVALID {
        let errors = errors(&registry, &fixtures("invalid").join(file));
        assert!(
            errors.iter().any(|e| e.pointer == *pointer),
            "{file}: expected an error at {pointer:?}, got {errors:#?}"
        );
        assert!(
            errors.iter().all(|e| e.pointer.starts_with(pointer)),
            "{file}: errors outside {pointer:?}: {errors:#?}"
        );
    }
}

#[test]
fn every_invalid_fixture_is_listed() {
    let mut on_disk: Vec<String> = fs::read_dir(fixtures("invalid"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort();
    let listed: Vec<String> = INVALID.iter().map(|(f, _)| f.to_string()).collect();
    assert_eq!(on_disk, listed);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ppp-core --test fixtures`
Expected: `every_invalid_record_fails_where_expected` and `every_invalid_fixture_is_listed` FAIL with `No such file or directory` (the `fixtures/invalid` directory doesn't exist yet).

- [ ] **Step 3: Write the invalid fixtures**

All but `local-exdate-without-tz.json` start from the same valid record and change one thing.


`fixtures/invalid/agent-both.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago",
    "organizers": [
      {
        "person": {
          "name": {
            "display": "Jane Doe"
          }
        },
        "org": {
          "name": "Chess Club"
        }
      }
    ]
  }
]
```

`fixtures/invalid/all-day-with-time.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-10-17T00:00:00",
    "tz": "America/Chicago",
    "all_day": true
  }
]
```

`fixtures/invalid/bad-currency.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago",
    "cost": {
      "min": 5,
      "currency": "usd"
    }
  }
]
```

`fixtures/invalid/bad-duration.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago",
    "duration": "2 hours"
  }
]
```

`fixtures/invalid/bad-geo.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago",
    "location": {
      "place": {
        "name": "Boe Memorial Chapel",
        "geo": {
          "lat": 144.4614,
          "lon": -93.1822
        }
      }
    }
  }
]
```

`fixtures/invalid/bad-month.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-13-08T18:00:00",
    "tz": "America/Chicago"
  }
]
```

`fixtures/invalid/bad-rrule.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago",
    "rrule": "WEEKLY"
  }
]
```

`fixtures/invalid/bad-status.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago",
    "status": "canceled"
  }
]
```

`fixtures/invalid/bad-type.json`:

```json
[
  {
    "type": "evnt",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago"
  }
]
```

`fixtures/invalid/bad-url.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago",
    "url": "www.example.edu/chess"
  }
]
```

`fixtures/invalid/empty-summary.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago"
  }
]
```

`fixtures/invalid/end-and-duration.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago",
    "end": "2026-09-08T20:00:00",
    "duration": "PT2H"
  }
]
```

`fixtures/invalid/local-exdate-without-tz.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T23:00:00Z",
    "rrule": "FREQ=WEEKLY",
    "exdates": [
      "2026-11-24T18:00:00"
    ]
  }
]
```

`fixtures/invalid/local-without-tz.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00"
  }
]
```

`fixtures/invalid/missing-id.json`:

```json
[
  {
    "type": "event",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago"
  }
]
```

`fixtures/invalid/missing-start.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "tz": "America/Chicago"
  }
]
```

`fixtures/invalid/unknown-field.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicago",
    "colour": "#e5a823"
  }
]
```

`fixtures/invalid/unknown-tz.json`:

```json
[
  {
    "type": "event",
    "id": "x@example.edu",
    "summary": "Chess Club",
    "start": "2026-09-08T18:00:00",
    "tz": "America/Chicgo"
  }
]
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test fixtures`
Expected: `test result: ok. 5 passed`

If a fixture fails at a different pointer, don't change the expected pointer to match. Work out why the schema reports it there, and stop and ask if the schema seems wrong.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add fixtures/invalid crates/ppp-core/tests/fixtures.rs
git commit -m "Add invalid event fixtures

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 3: Time-field walker

Finds, for each record type, which fields are times and in which form, by walking the schema (spec §5.3).

**Files:**
- Create: `crates/ppp-core/src/time_fields.rs`
- Modify: `crates/ppp-core/src/schema.rs` (add `COMMON_ID`, three `SchemaError` variants, `RecordType::time_fields`, the walk call)
- Modify: `crates/ppp-core/src/lib.rs`
- Test: `crates/ppp-core/tests/time_fields.rs`

**Interfaces:**
- Consumes: `Registry`, `RecordType`, `SchemaError` from Task 1.
- Produces:
  - `ppp_core::time_fields::TimeKind { Instant, Date, LocalDateTime, When }` (derives `Debug, Clone, Copy, PartialEq, Eq`)
  - `ppp_core::time_fields::TimeTree { pub kind: Option<TimeKind>, pub fields: BTreeMap<String, TimeTree>, pub each: Option<Box<TimeTree>> }` with `is_empty(&self) -> bool` and `paths(&self) -> Vec<(String, TimeKind)>` (paths like `start`, `exdates[]`, `inner.wall`)
  - `RecordType::time_fields: TimeTree` (new public field)
  - `SchemaError::{UnresolvedRef { reference }, UnsupportedTimeKeyword { keyword, path }, ConflictingTimeKinds { path }}`
  - `ppp_core::schema::COMMON_ID: &str`

- [ ] **Step 1: Write the failing test**

`crates/ppp-core/tests/time_fields.rs`:

```rust
//! The time-field walker finds every time field in a record type, and
//! refuses schemas it cannot read.

use ppp_core::schema::{Registry, SchemaError};
use ppp_core::time_fields::TimeKind;
use serde_json::{Value as Json, json};

fn common() -> Json {
    serde_json::from_str(include_str!("../../../stdlib/common.schema.json")).unwrap()
}

/// A one-type schema document whose `thing` record has the given properties
/// and extra keywords.
fn doc_with(props: Json, extra: Json) -> Json {
    let mut def = json!({
        "$anchor": "thing",
        "type": "object",
        "properties": props,
    });
    def.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://pipes.frogpond.tech/test/1",
        "$defs": { "thing": def }
    })
}

const C: &str = "https://pipes.frogpond.tech/stdlib/common/1#/$defs";

#[test]
fn event_time_fields() {
    let registry = Registry::stdlib().unwrap();
    let event = registry.record_type("event").unwrap();
    assert_eq!(event.uri, "https://pipes.frogpond.tech/stdlib/calendar/1#event");
    assert_eq!(
        event.time_fields.paths(),
        vec![
            ("created".to_string(), TimeKind::Instant),
            ("end".to_string(), TimeKind::When),
            ("exdates[]".to_string(), TimeKind::When),
            ("modified".to_string(), TimeKind::Instant),
            ("rdates[]".to_string(), TimeKind::When),
            ("recurrence_id".to_string(), TimeKind::When),
            ("start".to_string(), TimeKind::When),
        ]
    );
}

#[test]
fn nested_and_array_fields() {
    let doc = doc_with(
        json!({
            "at": { "$ref": format!("{C}/instant") },
            "days": { "type": "array", "items": { "$ref": format!("{C}/date") } },
            "inner": { "type": "object", "properties": { "wall": { "$ref": format!("{C}/local-date-time") } } }
        }),
        json!({}),
    );
    let registry = Registry::from_documents(vec![common(), doc]).unwrap();
    assert_eq!(
        registry.record_type("thing").unwrap().time_fields.paths(),
        vec![
            ("at".to_string(), TimeKind::Instant),
            ("days[]".to_string(), TimeKind::Date),
            ("inner.wall".to_string(), TimeKind::LocalDateTime),
        ]
    );
}

#[test]
fn time_field_under_one_of_is_refused() {
    let doc = doc_with(
        json!({}),
        json!({ "oneOf": [{ "properties": { "at": { "$ref": format!("{C}/instant") } } }] }),
    );
    match Registry::from_documents(vec![common(), doc]) {
        Err(SchemaError::UnsupportedTimeKeyword { keyword, .. }) => assert_eq!(keyword, "oneOf"),
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("expected an error"),
    }
}

#[test]
fn one_of_without_time_fields_is_fine() {
    let doc = doc_with(
        json!({ "cost": { "$ref": format!("{C}/money") }, "who": { "$ref": format!("{C}/agent") } }),
        json!({}),
    );
    let registry = Registry::from_documents(vec![common(), doc]).unwrap();
    assert!(
        registry
            .record_type("thing")
            .unwrap()
            .time_fields
            .is_empty()
    );
}

#[test]
fn conflicting_time_forms_are_refused() {
    let doc = doc_with(
        json!({ "at": { "$ref": format!("{C}/instant") } }),
        json!({ "allOf": [{ "properties": { "at": { "$ref": format!("{C}/date") } } }] }),
    );
    match Registry::from_documents(vec![common(), doc]) {
        Err(SchemaError::ConflictingTimeKinds { path }) => assert_eq!(path, "at"),
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("expected an error"),
    }
}

#[test]
fn recursive_schemas_end() {
    let doc = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://pipes.frogpond.tech/test/1",
        "$defs": {
            "node": {
                "$anchor": "node",
                "type": "object",
                "properties": {
                    "at": { "$ref": format!("{C}/instant") },
                    "parent": { "$ref": "#/$defs/node" }
                }
            }
        }
    });
    let registry = Registry::from_documents(vec![common(), doc]).unwrap();
    assert_eq!(
        registry.record_type("node").unwrap().time_fields.paths(),
        vec![
            ("at".to_string(), TimeKind::Instant),
            ("parent.at".to_string(), TimeKind::Instant)
        ]
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test time_fields`
Expected: compile error, `unresolved import ppp_core::time_fields`.

- [ ] **Step 3: Write the walker**

`crates/ppp-core/src/time_fields.rs`:

```rust
//! Finds the time fields of a record type by walking its JSON Schema.
//!
//! The serializer needs to know, for each record type, which fields hold
//! dates, instants or local times. The schema already says so: those fields
//! `$ref` one of the time definitions in `common`. This module walks a
//! record type's `$def` and builds a tree of those paths.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::schema::{COMMON_ID, SchemaError};

/// How a time field is written in JSON (brainstorm §0.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeKind {
    /// RFC 3339 with an offset, written in UTC: `2026-10-15T00:30:00Z`.
    Instant,
    /// A calendar date: `2026-10-14`.
    Date,
    /// A wall-clock time with no offset, read in the record's `tz`.
    LocalDateTime,
    /// Any of the three, chosen per record (spec §5.3).
    When,
}

impl TimeKind {
    fn from_common_def(name: &str) -> Option<TimeKind> {
        match name {
            "instant" => Some(TimeKind::Instant),
            "date" => Some(TimeKind::Date),
            "local-date-time" => Some(TimeKind::LocalDateTime),
            "when" => Some(TimeKind::When),
            _ => None,
        }
    }
}

/// The time fields of a record type, as a tree that mirrors the record.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TimeTree {
    /// Set when the value at this node is itself a time.
    pub kind: Option<TimeKind>,
    /// Object properties that are, or contain, time fields.
    pub fields: BTreeMap<String, TimeTree>,
    /// Array items that are, or contain, time fields.
    pub each: Option<Box<TimeTree>>,
}

/// One step in a path through a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Each,
}

impl TimeTree {
    pub fn is_empty(&self) -> bool {
        self.kind.is_none() && self.fields.is_empty() && self.each.is_none()
    }

    /// Lists every time field as a path string, e.g. `start` or `exdates[]`.
    pub fn paths(&self) -> Vec<(String, TimeKind)> {
        let mut out = Vec::new();
        self.collect_paths(String::new(), &mut out);
        out
    }

    fn collect_paths(&self, prefix: String, out: &mut Vec<(String, TimeKind)>) {
        if let Some(kind) = self.kind {
            out.push((prefix.clone(), kind));
        }
        for (key, child) in &self.fields {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            child.collect_paths(path, out);
        }
        if let Some(child) = &self.each {
            child.collect_paths(format!("{prefix}[]"), out);
        }
    }

    fn insert(&mut self, path: &[Seg], kind: TimeKind) -> Result<(), SchemaError> {
        let mut node = self;
        for seg in path {
            node = match seg {
                Seg::Key(key) => node.fields.entry(key.clone()).or_default(),
                Seg::Each => node.each.get_or_insert_with(Default::default),
            };
        }
        match node.kind {
            Some(existing) if existing != kind => Err(SchemaError::ConflictingTimeKinds {
                path: path_string(path),
            }),
            _ => {
                node.kind = Some(kind);
                Ok(())
            }
        }
    }
}

/// Keywords whose subschemas the walker does not follow. If one of them
/// leads to a time field, the walker cannot know which form applies, so it
/// fails instead of guessing.
const UNSUPPORTED: &[&str] = &[
    "anyOf",
    "oneOf",
    "not",
    "prefixItems",
    "contains",
    "patternProperties",
    "additionalProperties",
    "dependentSchemas",
    "propertyNames",
    "unevaluatedProperties",
    "unevaluatedItems",
];

/// Looks up schema documents by `$id`.
pub trait Documents {
    fn document(&self, id: &str) -> Option<&Json>;
}

/// Builds the time tree for the schema at `schema`, found in document `doc_id`.
pub fn walk(docs: &dyn Documents, doc_id: &str, schema: &Json) -> Result<TimeTree, SchemaError> {
    let mut tree = TimeTree::default();
    let mut walker = Walker {
        docs,
        tree: &mut tree,
        stack: Vec::new(),
    };
    walker.walk(doc_id, schema, &mut Vec::new())?;
    Ok(tree)
}

struct Walker<'a> {
    docs: &'a dyn Documents,
    tree: &'a mut TimeTree,
    /// The `$ref` targets being walked, so recursive schemas end.
    stack: Vec<String>,
}

impl Walker<'_> {
    fn walk(
        &mut self,
        doc_id: &str,
        schema: &Json,
        path: &mut Vec<Seg>,
    ) -> Result<(), SchemaError> {
        let Some(obj) = schema.as_object() else {
            return Ok(());
        };

        if let Some(Json::String(reference)) = obj.get("$ref") {
            let (target_doc, pointer) = split_ref(doc_id, reference);
            if target_doc == COMMON_ID
                && let Some(kind) = pointer
                    .strip_prefix("/$defs/")
                    .and_then(TimeKind::from_common_def)
            {
                return self.tree.insert(path, kind);
            }
            let key = format!("{target_doc}#{pointer}");
            if !self.stack.contains(&key) {
                let target = resolve(self.docs, &target_doc, &pointer)
                    .ok_or_else(|| SchemaError::UnresolvedRef {
                        reference: reference.clone(),
                    })?
                    .clone();
                self.stack.push(key);
                self.walk(&target_doc, &target, path)?;
                self.stack.pop();
            }
        }

        if let Some(Json::Array(parts)) = obj.get("allOf") {
            for part in parts {
                self.walk(doc_id, part, path)?;
            }
        }

        if let Some(Json::Object(props)) = obj.get("properties") {
            for (key, sub) in props {
                path.push(Seg::Key(key.clone()));
                self.walk(doc_id, sub, path)?;
                path.pop();
            }
        }

        if let Some(items) = obj.get("items") {
            path.push(Seg::Each);
            self.walk(doc_id, items, path)?;
            path.pop();
        }

        // `if`, `then` and `else` only narrow fields that `properties`
        // already declares, so the walker skips them.

        for keyword in UNSUPPORTED {
            let Some(value) = obj.get(*keyword) else {
                continue;
            };
            let subschemas: Vec<&Json> = match value {
                Json::Array(list) => list.iter().collect(),
                Json::Object(map)
                    if *keyword == "patternProperties" || *keyword == "dependentSchemas" =>
                {
                    map.values().collect()
                }
                other => vec![other],
            };
            for sub in subschemas {
                let probe = walk_with_stack(self.docs, doc_id, sub, &self.stack)?;
                if !probe.is_empty() {
                    return Err(SchemaError::UnsupportedTimeKeyword {
                        keyword: keyword.to_string(),
                        path: path_string(path),
                    });
                }
            }
        }

        Ok(())
    }
}

fn walk_with_stack(
    docs: &dyn Documents,
    doc_id: &str,
    schema: &Json,
    stack: &[String],
) -> Result<TimeTree, SchemaError> {
    let mut tree = TimeTree::default();
    let mut walker = Walker {
        docs,
        tree: &mut tree,
        stack: stack.to_vec(),
    };
    walker.walk(doc_id, schema, &mut Vec::new())?;
    Ok(tree)
}

/// Splits a `$ref` into the target document's `$id` and a JSON Pointer.
/// An anchor (`…/calendar/1#event`) becomes the pointer to the `$def` that
/// declares it.
fn split_ref(doc_id: &str, reference: &str) -> (String, String) {
    let (base, fragment) = reference.split_once('#').unwrap_or((reference, ""));
    let target_doc = if base.is_empty() {
        doc_id.to_string()
    } else {
        base.to_string()
    };
    (target_doc, fragment.to_string())
}

fn resolve<'a>(docs: &'a dyn Documents, doc_id: &str, fragment: &str) -> Option<&'a Json> {
    let doc = docs.document(doc_id)?;
    if fragment.is_empty() || fragment.starts_with('/') {
        return doc.pointer(fragment);
    }
    doc.get("$defs")?
        .as_object()?
        .values()
        .find(|def| def.get("$anchor").and_then(Json::as_str) == Some(fragment))
}

fn path_string(path: &[Seg]) -> String {
    let mut out = String::new();
    for seg in path {
        match seg {
            Seg::Key(key) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(key);
            }
            Seg::Each => out.push_str("[]"),
        }
    }
    out
}
```

`crates/ppp-core/src/lib.rs`:

```rust
pub mod schema;
pub mod time_fields;
```

- [ ] **Step 4: Connect the walker to the registry**

Replace `crates/ppp-core/src/schema.rs` with:

```rust
//! The stdlib schema registry: compiles a validator and a time-field tree
//! for every record type, and validates records against them.
//!
//! Schemas are embedded in the binary and resolved by `$id`. Nothing is ever
//! fetched, so validation is offline and deterministic.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::time_fields::{self, Documents, TimeTree};

pub const COMMON_ID: &str = "https://pipes.frogpond.tech/stdlib/common/1";

/// The stdlib schema documents, in the order they are registered.
const STDLIB: &[&str] = &[
    include_str!("../../../stdlib/common.schema.json"),
    include_str!("../../../stdlib/calendar.schema.json"),
];

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("schema document is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("schema document has no \"$id\"")]
    MissingId,
    #[error("could not register schema {id}: {message}")]
    Register { id: String, message: String },
    #[error("could not compile record type {name}: {message}")]
    Compile { name: String, message: String },
    #[error("cannot resolve $ref {reference}")]
    UnresolvedRef { reference: String },
    #[error(
        "time field at {path} sits under \"{keyword}\", which the time-field walker does not follow"
    )]
    UnsupportedTimeKeyword { keyword: String, path: String },
    #[error("time field at {path} is declared with two different time forms")]
    ConflictingTimeKinds { path: String },
}

/// A validation failure for one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordError {
    /// Position of the record in its list (0-based).
    pub index: usize,
    /// JSON Pointer to the failing value inside the record; empty for the whole record.
    pub pointer: String,
    pub message: String,
    /// The failing value as compact JSON, cut to 80 characters.
    pub excerpt: String,
}

/// One record type: a `$def` in a stdlib schema that declares an `$anchor`.
pub struct RecordType {
    /// The value of the record's `type` field, e.g. `event`.
    pub name: String,
    /// `<schema $id>#<anchor>`, e.g. `https://pipes.frogpond.tech/stdlib/calendar/1#event`.
    pub uri: String,
    pub time_fields: TimeTree,
    validator: jsonschema::Validator,
}

pub struct Registry {
    types: BTreeMap<String, RecordType>,
}

impl Documents for BTreeMap<String, Json> {
    fn document(&self, id: &str) -> Option<&Json> {
        self.get(id)
    }
}

impl Registry {
    /// The registry of embedded stdlib schemas.
    pub fn stdlib() -> Result<Registry, SchemaError> {
        let documents = STDLIB
            .iter()
            .map(|text| serde_json::from_str(text))
            .collect::<Result<Vec<Json>, _>>()?;
        Registry::from_documents(documents)
    }

    /// Builds a registry from schema documents. Each must have an `$id`.
    pub fn from_documents(documents: Vec<Json>) -> Result<Registry, SchemaError> {
        let mut by_id = BTreeMap::new();
        for doc in documents {
            let id = doc
                .get("$id")
                .and_then(Json::as_str)
                .ok_or(SchemaError::MissingId)?
                .to_string();
            by_id.insert(id, doc);
        }

        let mut builder = jsonschema::Registry::new();
        for (id, doc) in &by_id {
            builder = builder
                .add(id.as_str(), doc.clone())
                .map_err(|e| SchemaError::Register {
                    id: id.clone(),
                    message: e.to_string(),
                })?;
        }
        let prepared = builder.prepare().map_err(|e| SchemaError::Register {
            id: "(registry)".into(),
            message: e.to_string(),
        })?;

        let mut types = BTreeMap::new();
        for (id, doc) in &by_id {
            let Some(defs) = doc.get("$defs").and_then(Json::as_object) else {
                continue;
            };
            for def in defs.values() {
                let Some(anchor) = def.get("$anchor").and_then(Json::as_str) else {
                    continue;
                };
                let uri = format!("{id}#{anchor}");
                // Draft 2020-12, formats asserted, no network or file access,
                // plus our two custom formats.
                let validator = jsonschema::draft202012::options()
                    .with_registry(&prepared)
                    .offline()
                    .should_validate_formats(true)
                    .with_format("iana-tz", |s: &str| jiff::tz::db().get(s).is_ok())
                    .with_format("local-date-time", |s: &str| {
                        s.parse::<jiff::civil::DateTime>().is_ok()
                    })
                    .build(&serde_json::json!({ "$ref": uri }))
                    .map_err(|e| SchemaError::Compile {
                        name: anchor.into(),
                        message: e.to_string(),
                    })?;
                let time_fields = time_fields::walk(&by_id, id, def)?;
                types.insert(
                    anchor.to_string(),
                    RecordType {
                        name: anchor.to_string(),
                        uri,
                        time_fields,
                        validator,
                    },
                );
            }
        }

        Ok(Registry { types })
    }

    /// Looks up a record type by the value of a record's `type` field.
    pub fn record_type(&self, name: &str) -> Option<&RecordType> {
        self.types.get(name)
    }

    /// Validates one record against the record type its `type` field names.
    pub fn validate(&self, index: usize, record: &Json) -> Vec<RecordError> {
        let type_name = match record.get("type") {
            Some(Json::String(name)) => name,
            Some(other) => return vec![error(index, "/type", "\"type\" must be a string", other)],
            None => return vec![error(index, "", "record has no \"type\"", record)],
        };
        let Some(record_type) = self.record_type(type_name) else {
            let message = format!("unknown record type \"{type_name}\"");
            return vec![error(index, "/type", &message, &record["type"])];
        };
        record_type
            .validator
            .iter_errors(record)
            .map(|e| {
                error(
                    index,
                    &e.instance_path().to_string(),
                    &e.to_string(),
                    e.instance(),
                )
            })
            .collect()
    }
}

fn error(index: usize, pointer: &str, message: &str, value: &Json) -> RecordError {
    RecordError {
        index,
        pointer: pointer.to_string(),
        message: message.to_string(),
        excerpt: excerpt(value),
    }
}

fn excerpt(value: &Json) -> String {
    const LIMIT: usize = 80;
    let text = value.to_string();
    if text.chars().count() <= LIMIT {
        return text;
    }
    let cut: String = text.chars().take(LIMIT - 1).collect();
    format!("{cut}…")
}
```

The changes from Task 1: the module comment; the `time_fields` import; `COMMON_ID`; three `SchemaError` variants; the `time_fields` field on `RecordType`; `impl Documents for BTreeMap<String, Json>`; and the `time_fields::walk` call in `from_documents`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ppp-core`
Expected: `fixtures` 5 passed, `time_fields` 6 passed.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add crates/ppp-core
git commit -m "Find time fields by walking record schemas

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 4: Serializer

Writes a nushell record as JSON, formatting each time field as its schema says (spec §5.3).

**Files:**
- Create: `crates/ppp-core/src/time.rs`, `crates/ppp-core/src/serialize.rs`
- Modify: `crates/ppp-core/Cargo.toml` (add `chrono`, `nu-protocol`), `crates/ppp-core/src/lib.rs`
- Test: `crates/ppp-core/tests/serialize.rs`

**Interfaces:**
- Consumes: `Registry`, `RecordType::time_fields`, `TimeTree`, `TimeKind` from Tasks 1 and 3.
- Produces:
  - `ppp_core::serialize::record_to_json(record: &nu_protocol::Value, time_fields: &TimeTree) -> Result<serde_json::Value, SerializeError>`
  - `ppp_core::serialize::SerializeError::{Unsupported { pointer, type_name }, NonFinite { pointer, value }, MissingTimeZone { pointer, form: &'static str }, UnknownTimeZone { pointer, tz }, OutOfRange { pointer }}` (derives `Debug, PartialEq, Eq`)
  - `ppp_core::time::{timestamp, format_instant, format_local, format_date}`

The rule for `when` fields: `all_day: true` → `date`; else a `tz` field → local date-time; else instant. Datetimes outside any time field are written as instants. Nushell durations become ISO 8601 strings (`PT2H`); filesizes become byte counts.

- [ ] **Step 1: Add dependencies**

`crates/ppp-core/Cargo.toml`:

```toml
[package]
name = "ppp-core"
version = "0.1.0"
edition.workspace = true

[dependencies]
chrono.workspace = true
jiff.workspace = true
jsonschema.workspace = true
nu-protocol.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

- [ ] **Step 2: Write the failing test**

`crates/ppp-core/tests/serialize.rs`:

```rust
//! Nushell records become JSON with each time field in the form its schema
//! declares.

use chrono::DateTime;
use nu_protocol::{Record, Value, record};
use ppp_core::schema::Registry;
use ppp_core::serialize::{SerializeError, record_to_json};
use serde_json::json;

fn date(rfc3339: &str) -> Value {
    Value::test_date(DateTime::parse_from_rfc3339(rfc3339).unwrap())
}

fn event(fields: Record) -> Value {
    let mut rec = record! {
        "type" => Value::test_string("event"),
        "id" => Value::test_string("x@example.edu"),
        "summary" => Value::test_string("Chess Club"),
    };
    for (k, v) in fields {
        rec.push(k, v);
    }
    Value::test_record(rec)
}

fn to_json(record: &Value) -> Result<serde_json::Value, SerializeError> {
    let registry = Registry::stdlib().unwrap();
    record_to_json(record, &registry.record_type("event").unwrap().time_fields)
}

#[test]
fn instant_fields_are_written_in_utc() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-22T16:00:00Z"),
        "created" => date("2026-08-06T13:11:07-05:00"),
    }))
    .unwrap();
    assert_eq!(json["created"], "2026-08-06T18:11:07Z");
}

#[test]
fn when_fields_with_tz_are_local_wall_time() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-15T00:30:00Z"),
        "tz" => Value::test_string("America/Chicago"),
    }))
    .unwrap();
    assert_eq!(json["start"], "2026-10-14T19:30:00");
}

#[test]
fn when_fields_on_all_day_records_are_dates() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-17T00:00:00-05:00"),
        "end" => date("2026-10-19T00:00:00-05:00"),
        "all_day" => Value::test_bool(true),
        "tz" => Value::test_string("America/Chicago"),
    }))
    .unwrap();
    assert_eq!(
        (json["start"].as_str(), json["end"].as_str()),
        (Some("2026-10-17"), Some("2026-10-19"))
    );
}

#[test]
fn when_fields_without_tz_are_instants() {
    let json = to_json(&event(
        record! { "start" => date("2026-10-22T11:00:00-05:00") },
    ))
    .unwrap();
    assert_eq!(json["start"], "2026-10-22T16:00:00Z");
}

#[test]
fn list_items_follow_the_schema() {
    let json = to_json(&event(record! {
        "start" => date("2026-09-08T18:00:00-05:00"),
        "tz" => Value::test_string("America/Chicago"),
        "exdates" => Value::test_list(vec![date("2026-11-24T18:00:00-06:00"), date("2026-12-01T18:00:00-06:00")]),
    }))
    .unwrap();
    assert_eq!(
        json["exdates"],
        json!(["2026-11-24T18:00:00", "2026-12-01T18:00:00"])
    );
}

#[test]
fn fall_back_hour_keeps_its_wall_time() {
    // 01:30 happens twice in Chicago on 2026-11-01; this is the second (CST).
    let json = to_json(&event(record! {
        "start" => date("2026-11-01T01:30:00-06:00"),
        "tz" => Value::test_string("America/Chicago"),
    }))
    .unwrap();
    assert_eq!(json["start"], "2026-11-01T01:30:00");
}

#[test]
fn datetimes_outside_time_fields_are_instants() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-22T16:00:00Z"),
        "extra" => Value::test_record(record! { "fetched" => date("2026-10-11T20:00:00-04:00") }),
    }))
    .unwrap();
    assert_eq!(json["extra"]["fetched"], "2026-10-12T00:00:00Z");
}

#[test]
fn all_day_without_tz_is_an_error() {
    let err = to_json(&event(record! {
        "start" => date("2026-10-17T00:00:00-05:00"),
        "all_day" => Value::test_bool(true),
    }))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::MissingTimeZone {
            pointer: "/start".into(),
            form: "date"
        }
    );
}

#[test]
fn unknown_tz_is_an_error() {
    let err = to_json(&event(record! {
        "start" => date("2026-10-17T00:00:00-05:00"),
        "tz" => Value::test_string("America/Chicgo"),
    }))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::UnknownTimeZone {
            pointer: "/start".into(),
            tz: "America/Chicgo".into()
        }
    );
}

#[test]
fn plain_values() {
    let json = to_json(&event(record! {
        "start" => date("2026-10-22T16:00:00Z"),
        "duration" => Value::test_duration(2 * 60 * 60 * 1_000_000_000),
        "featured" => Value::test_bool(true),
        "location" => Value::test_record(record! { "name" => Value::test_glob("Regents Hall 150") }),
        "cost" => Value::test_record(record! { "min" => Value::test_int(5), "max" => Value::test_float(10.5), "currency" => Value::test_string("USD") }),
        "series" => Value::test_nothing(),
    }))
    .unwrap();
    assert_eq!(json["duration"], "PT2H");
    assert_eq!(json["featured"], true);
    assert_eq!(json["location"], json!({ "name": "Regents Hall 150" }));
    assert_eq!(
        json["cost"],
        json!({ "min": 5, "max": 10.5, "currency": "USD" })
    );
    assert_eq!(json["series"], serde_json::Value::Null);
}

#[test]
fn binary_is_an_error() {
    let err = to_json(&event(record! {
        "extra" => Value::test_record(record! { "raw" => Value::test_binary(vec![1u8, 2]) }),
    }))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::Unsupported {
            pointer: "/extra/raw".into(),
            type_name: "binary".into()
        }
    );
}

#[test]
fn nan_is_an_error() {
    let err = to_json(&event(
        record! { "extra" => Value::test_record(record! { "x" => Value::test_float(f64::NAN) }) },
    ))
    .unwrap_err();
    assert!(matches!(err, SerializeError::NonFinite { pointer, .. } if pointer == "/extra/x"));
}

#[test]
fn serialized_events_validate() {
    let registry = Registry::stdlib().unwrap();
    let json = to_json(&event(record! {
        "start" => date("2026-10-14T19:30:00-05:00"),
        "end" => date("2026-10-14T21:00:00-05:00"),
        "tz" => Value::test_string("America/Chicago"),
        "exdates" => Value::test_list(vec![date("2026-10-21T19:30:00-05:00")]),
        "rrule" => Value::test_string("FREQ=WEEKLY"),
        "created" => date("2026-08-06T18:11:07Z"),
    }))
    .unwrap();
    assert_eq!(registry.validate(0, &json), vec![]);
}

#[test]
fn pointers_escape_slashes_and_tildes() {
    let err = to_json(&event(record! {
        "extra" => Value::test_record(record! { "a/b~c" => Value::test_binary(vec![0u8]) }),
    }))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::Unsupported {
            pointer: "/extra/a~1b~0c".into(),
            type_name: "binary".into()
        }
    );
}

#[test]
fn datetimes_past_year_9999_are_an_error() {
    let far = chrono::NaiveDate::from_ymd_opt(12026, 10, 22)
        .unwrap()
        .and_hms_opt(16, 0, 0)
        .unwrap();
    let err = to_json(&event(
        record! { "start" => Value::test_date(far.and_utc().fixed_offset()) },
    ))
    .unwrap_err();
    assert_eq!(
        err,
        SerializeError::OutOfRange {
            pointer: "/start".into()
        }
    );
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test serialize`
Expected: compile error, `unresolved import ppp_core::serialize`.

- [ ] **Step 4: Write the time helpers and the serializer**

`crates/ppp-core/src/time.rs`:

```rust
//! Time helpers built on jiff. Nushell hands us chrono values; this module
//! is where they become jiff values.
//!
//! Output drops fractional seconds: calendar sources work in whole seconds,
//! and a fixed width keeps the JSON stable.

use chrono::{DateTime, FixedOffset};
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Converts a nushell datetime to a jiff timestamp. Fails only for dates
/// outside jiff's range (years -9999 to 9999).
pub fn timestamp(dt: &DateTime<FixedOffset>) -> Result<Timestamp, jiff::Error> {
    Timestamp::new(dt.timestamp(), dt.timestamp_subsec_nanos() as i32)
}

/// `2026-10-15T00:30:00Z`
pub fn format_instant(ts: Timestamp) -> String {
    ts.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// `2026-10-14T19:30:00`: the wall-clock time in `tz`, with no offset.
pub fn format_local(ts: Timestamp, tz: &TimeZone) -> String {
    ts.to_zoned(tz.clone())
        .strftime("%Y-%m-%dT%H:%M:%S")
        .to_string()
}

/// `2026-10-14`: the calendar date in `tz`.
pub fn format_date(ts: Timestamp, tz: &TimeZone) -> String {
    ts.to_zoned(tz.clone()).strftime("%Y-%m-%d").to_string()
}
```

`crates/ppp-core/src/serialize.rs`:

```rust
//! Turns a nushell record into JSON for its record type.
//!
//! Inside a pipe, times are nushell datetimes, which always carry an
//! offset. The schema decides how each one is written (spec §5.3): as an
//! instant, as a date in the record's `tz`, or as a wall-clock time in the
//! record's `tz`.

use jiff::tz::TimeZone;
use nu_protocol::Value;
use serde_json::{Map, Number, Value as Json};

use crate::time;
use crate::time_fields::{TimeKind, TimeTree};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SerializeError {
    #[error("{pointer}: {type_name} values cannot be written as JSON")]
    Unsupported { pointer: String, type_name: String },
    #[error("{pointer}: {value} is not a finite number")]
    NonFinite { pointer: String, value: String },
    #[error("{pointer}: writing a {form} needs the record's \"tz\", and the record has none")]
    MissingTimeZone { pointer: String, form: &'static str },
    #[error("{pointer}: unknown time zone \"{tz}\"")]
    UnknownTimeZone { pointer: String, tz: String },
    #[error("{pointer}: datetime is outside the years -9999 to 9999")]
    OutOfRange { pointer: String },
}

/// Serializes one record, formatting the fields in `time_fields`.
/// Datetimes elsewhere are written as instants.
pub fn record_to_json(record: &Value, time_fields: &TimeTree) -> Result<Json, SerializeError> {
    let context = match record {
        Value::Record { val, .. } => Context {
            tz: match val.get("tz") {
                Some(Value::String { val, .. }) => Some(val.clone()),
                _ => None,
            },
            all_day: matches!(val.get("all_day"), Some(Value::Bool { val: true, .. })),
        },
        _ => Context {
            tz: None,
            all_day: false,
        },
    };
    context.convert(record, Some(time_fields), &mut String::new())
}

struct Context {
    tz: Option<String>,
    all_day: bool,
}

impl Context {
    fn convert(
        &self,
        value: &Value,
        tree: Option<&TimeTree>,
        pointer: &mut String,
    ) -> Result<Json, SerializeError> {
        Ok(match value {
            Value::Nothing { .. } => Json::Null,
            Value::Bool { val, .. } => Json::Bool(*val),
            Value::Int { val, .. } => Json::Number((*val).into()),
            Value::Filesize { val, .. } => Json::Number(val.get().into()),
            Value::Float { val, .. } => {
                Json::Number(
                    Number::from_f64(*val).ok_or_else(|| SerializeError::NonFinite {
                        pointer: pointer.clone(),
                        value: val.to_string(),
                    })?,
                )
            }
            Value::String { val, .. } | Value::Glob { val, .. } => Json::String(val.clone()),
            Value::Duration { val, .. } => {
                Json::String(jiff::SignedDuration::from_nanos(*val).to_string())
            }
            Value::Date { val, .. } => {
                let ts = time::timestamp(val).map_err(|_| SerializeError::OutOfRange {
                    pointer: pointer.clone(),
                })?;
                Json::String(self.format_time(ts, tree.and_then(|t| t.kind), pointer)?)
            }
            Value::Record { val, .. } => {
                let mut out = Map::new();
                for (key, child) in val.iter() {
                    let len = pointer.len();
                    pointer.push('/');
                    pointer.push_str(&escape_pointer(key));
                    let subtree = tree.and_then(|t| t.fields.get(key));
                    out.insert(key.clone(), self.convert(child, subtree, pointer)?);
                    pointer.truncate(len);
                }
                Json::Object(out)
            }
            Value::List { vals, .. } => {
                let subtree = tree.and_then(|t| t.each.as_deref());
                let mut out = Vec::with_capacity(vals.len());
                for (i, child) in vals.iter().enumerate() {
                    let len = pointer.len();
                    pointer.push('/');
                    pointer.push_str(&i.to_string());
                    out.push(self.convert(child, subtree, pointer)?);
                    pointer.truncate(len);
                }
                Json::Array(out)
            }
            other => {
                return Err(SerializeError::Unsupported {
                    pointer: pointer.clone(),
                    type_name: other.get_type().to_string(),
                });
            }
        })
    }

    fn format_time(
        &self,
        ts: jiff::Timestamp,
        kind: Option<TimeKind>,
        pointer: &str,
    ) -> Result<String, SerializeError> {
        let kind = match kind {
            Some(TimeKind::When) if self.all_day => TimeKind::Date,
            Some(TimeKind::When) if self.tz.is_some() => TimeKind::LocalDateTime,
            Some(TimeKind::When) | None => TimeKind::Instant,
            Some(kind) => kind,
        };
        match kind {
            TimeKind::Instant | TimeKind::When => Ok(time::format_instant(ts)),
            TimeKind::Date => Ok(time::format_date(ts, &self.zone(pointer, "date")?)),
            TimeKind::LocalDateTime => Ok(time::format_local(
                ts,
                &self.zone(pointer, "local date-time")?,
            )),
        }
    }

    fn zone(&self, pointer: &str, form: &'static str) -> Result<TimeZone, SerializeError> {
        let name = self
            .tz
            .as_deref()
            .ok_or_else(|| SerializeError::MissingTimeZone {
                pointer: pointer.to_string(),
                form,
            })?;
        TimeZone::get(name).map_err(|_| SerializeError::UnknownTimeZone {
            pointer: pointer.to_string(),
            tz: name.to_string(),
        })
    }
}

/// Escapes a key for use in a JSON Pointer (RFC 6901).
fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
```

`crates/ppp-core/src/lib.rs`:

```rust
pub mod schema;
pub mod serialize;
pub mod time;
pub mod time_fields;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ppp-core`
Expected: `fixtures` 5 passed, `serialize` 15 passed, `time_fields` 6 passed.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add crates/ppp-core Cargo.lock
git commit -m "Serialize nushell records using schema time fields

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 5: `ppp validate`

**Files:**
- Modify: `Cargo.toml` (add `crates/ppp-cli` to `members`)
- Create: `crates/ppp-cli/Cargo.toml`, `crates/ppp-cli/src/main.rs`, `crates/ppp-cli/src/validate.rs`
- Test: `crates/ppp-cli/tests/validate.rs`

**Interfaces:**
- Consumes: `Registry::stdlib`, `Registry::validate`, `RecordError` from Task 1.
- Produces: the `ppp` binary with `ppp validate <file>`. A `.jsonl` file is one record per line (blank lines skipped) and errors name the 1-based line; anything else must be a JSON array and errors name the 0-based record. Valid: one line on stdout, exit 0. Invalid: up to 20 failures on stderr, then `…and N more`, then a count line; exit 1. Unreadable or unparsable: `ppp: <file>: <reason>` on stderr, exit 2. Usage errors exit 2 (clap's default).

- [ ] **Step 1: Add the crate to the workspace**

In `Cargo.toml`, change the members line to:

```toml
members = ["crates/ppp-core", "crates/ppp-cli"]
```

`crates/ppp-cli/Cargo.toml`:

```toml
[package]
name = "ppp-cli"
version = "0.1.0"
edition.workspace = true

[[bin]]
name = "ppp"
path = "src/main.rs"

[dependencies]
clap.workspace = true
ppp-core = { path = "../ppp-core" }
serde_json.workspace = true
```

Create `crates/ppp-cli/src/main.rs` containing only `fn main() {}` so the binary builds.

- [ ] **Step 2: Write the failing test**

`crates/ppp-cli/tests/validate.rs`:

```rust
//! `ppp validate` reports valid files, invalid records and unreadable files
//! with the right exit codes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

fn scratch(name: &str, contents: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn ppp_validate(file: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ppp"))
        .arg("validate")
        .arg(file)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn valid_file() {
    let out = ppp_validate(&fixture("examples/campus-events.json"));
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).ends_with("campus-events.json: 4 records valid\n"));
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn invalid_record() {
    let out = ppp_validate(&fixture("invalid/unknown-tz.json"));
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("record 0 /tz: \"America/Chicgo\" is not a \"iana-tz\""),
        "{stderr}"
    );
    assert!(stderr.contains("    value: \"America/Chicgo\""), "{stderr}");
    assert!(
        stderr.ends_with("unknown-tz.json: 1 error in 1 record\n"),
        "{stderr}"
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn jsonl_reports_lines() {
    let file = scratch(
        "two.jsonl",
        "{\"type\":\"event\",\"id\":\"a\",\"summary\":\"A\",\"start\":\"2026-10-22T16:00:00Z\"}\n\
         \n\
         {\"type\":\"event\",\"id\":\"b\",\"summary\":\"\",\"start\":\"2026-10-22T16:00:00Z\"}\n",
    );
    let out = ppp_validate(&file);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("line 2 /summary:"), "{stderr}");
}

#[test]
fn long_reports_are_cut_at_twenty() {
    let record = r#"{"type":"event","id":"a","summary":"","start":"2026-10-22T16:00:00Z"}"#;
    let file = scratch("many.json", &format!("[{}]", vec![record; 25].join(",")));
    let out = ppp_validate(&file);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert_eq!(stderr.matches("/summary:").count(), 20, "{stderr}");
    assert!(stderr.contains("…and 5 more"), "{stderr}");
    assert!(stderr.ends_with("25 errors in 25 records\n"), "{stderr}");
}

#[test]
fn unreadable_files_exit_2() {
    let missing = ppp_validate(Path::new("no-such-file.json"));
    assert_eq!(missing.status.code(), Some(2));
    assert!(text(&missing.stderr).starts_with("ppp: no-such-file.json: "));

    let not_json = ppp_validate(&scratch("broken.json", "[{"));
    assert_eq!(not_json.status.code(), Some(2));

    let not_array = ppp_validate(&scratch("object.json", "{}"));
    assert_eq!(not_array.status.code(), Some(2));
    assert!(text(&not_array.stderr).ends_with("expected a JSON array of records\n"));
}

#[test]
fn jsonl_with_crlf_line_endings() {
    let line = r#"{"type":"event","id":"a","summary":"A","start":"2026-10-22T16:00:00Z"}"#;
    let out = ppp_validate(&scratch("crlf.jsonl", &format!("{line}\r\n{line}\r\n")));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).ends_with("crlf.jsonl: 2 records valid\n"));
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ppp-cli`
Expected: all 6 tests FAIL (the binary ignores its arguments and exits 0 with no output).

- [ ] **Step 4: Write the command**

`crates/ppp-cli/src/main.rs`:

```rust
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod validate;

/// Run and check ppp pipes.
#[derive(Parser)]
#[command(name = "ppp", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a JSON array or JSONL file of records against the stdlib schemas
    Validate {
        /// A `.json` file holding an array of records, or a `.jsonl` file with one record per line
        file: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Validate { file } => validate::run(&file),
    }
}
```

`crates/ppp-cli/src/validate.rs`:

```rust
//! `ppp validate`: checks a file of records against the stdlib schemas.
//!
//! Exit codes: 0 when every record is valid, 1 when any record is invalid,
//! 2 when the file cannot be read or parsed.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use ppp_core::schema::{RecordError, Registry};
use serde_json::Value as Json;

/// How many failures to print before summarizing the rest.
const SHOWN: usize = 20;

pub fn run(file: &Path) -> ExitCode {
    let (records, unit) = match read_records(file) {
        Ok(found) => found,
        Err(message) => {
            eprintln!("ppp: {}: {message}", file.display());
            return ExitCode::from(2);
        }
    };
    let registry = match Registry::stdlib() {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!("ppp: stdlib schemas: {e}");
            return ExitCode::from(2);
        }
    };

    let errors: Vec<RecordError> = records
        .iter()
        .enumerate()
        .flat_map(|(i, record)| registry.validate(i, record))
        .collect();

    if errors.is_empty() {
        println!(
            "{}: {} valid",
            file.display(),
            count(records.len(), "record")
        );
        return ExitCode::SUCCESS;
    }
    for e in errors.iter().take(SHOWN) {
        eprintln!("{}: {}", file.display(), describe(e, unit));
    }
    if errors.len() > SHOWN {
        eprintln!("{}: …and {} more", file.display(), errors.len() - SHOWN);
    }
    eprintln!(
        "{}: {} in {}",
        file.display(),
        count(errors.len(), "error"),
        count(records.len(), "record")
    );
    ExitCode::from(1)
}

/// How records are numbered in messages.
#[derive(Clone, Copy)]
enum Unit {
    /// 0-based position in a JSON array.
    Record,
    /// 1-based line in a JSONL file.
    Line,
}

fn read_records(file: &Path) -> Result<(Vec<Json>, Unit), String> {
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    if file.extension().is_some_and(|ext| ext == "jsonl") {
        let mut records = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            records.push(serde_json::from_str(line).map_err(|e| format!("line {}: {e}", i + 1))?);
        }
        return Ok((records, Unit::Line));
    }
    match serde_json::from_str(&text).map_err(|e| e.to_string())? {
        Json::Array(records) => Ok((records, Unit::Record)),
        _ => Err("expected a JSON array of records".to_string()),
    }
}

fn describe(e: &RecordError, unit: Unit) -> String {
    let place = match unit {
        Unit::Record => format!("record {}", e.index),
        Unit::Line => format!("line {}", e.index + 1),
    };
    let pointer = if e.pointer.is_empty() {
        String::new()
    } else {
        format!(" {}", e.pointer)
    };
    format!("{place}{pointer}: {}\n    value: {}", e.message, e.excerpt)
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test`
Expected: every suite passes — `ppp-cli` validate 6, `ppp-core` fixtures 5, serialize 15, time_fields 6.

Then try it by hand:

```bash
cargo run -q -p ppp-cli -- validate fixtures/examples/campus-events.json   # 4 records valid, exit 0
cargo run -q -p ppp-cli -- validate fixtures/invalid/bad-month.json        # record 0 /start: …, exit 1
```

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add Cargo.toml Cargo.lock crates/ppp-cli
git commit -m "Add ppp validate command

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```



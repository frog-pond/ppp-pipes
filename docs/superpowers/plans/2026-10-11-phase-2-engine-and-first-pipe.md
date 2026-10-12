# ppp-pipes Phase 2: Engine and First Pipe — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ppp run campus-week` fetches the live St. Olaf calendar, turns it into `event` records in a sandboxed nushell engine, checks them at the boundary, and prints them as JSON.

**Architecture:** `ppp-core` gains an iCalendar parser (`ics/lines.rs` for the text layer, `ics/events.rs` for VEVENTs), an embedded nushell engine with a hand-picked command set and a virtual `pipes` module (`engine.rs`), a Rust `from ics` command (`commands/from_ics.rs`), the `pipes` nushell module with `into event` (`stdlib/pipes/mod.nu`), and a pipe loader that reads metadata, runs `main` and checks records (`pipe.rs`). `ppp-cli` gains `ppp run`, sharing its error report with `ppp validate`.

**Tech Stack:** Rust 2024 (rustc 1.98.0), nushell crates `nu-protocol`, `nu-engine`, `nu-parser`, `nu-cmd-lang`, `nu-command` all `=0.116.1`; `jiff` 0.2.38; `chrono` 0.4.45 (nushell's datetime type only); `jsonschema` 0.58.6; `clap` 4.

**Spec:** [`docs/superpowers/specs/2026-10-11-ppp-pipes-cli-prototype-design.md`](../specs/2026-10-11-ppp-pipes-cli-prototype-design.md) — build-order step 2. Phase 1's plan is [`2026-10-11-phase-1-core-and-validation.md`](2026-10-11-phase-1-core-and-validation.md).

Every code block was compiled and tested in a scratch copy of the repo before this plan was written, task by task in this order: after each task, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` were clean, and the final state passes 101 tests plus the live `campus-week` test. Code is shown already formatted.

## Global Constraints

- Everything in phase 1's Global Constraints still holds (JSON Schema 2020-12, `pipes.frogpond.tech` ids, closed records, jiff for time logic, exit codes 0/1/2, pristine tests, commit trailer).
- Nushell crates are pinned `=0.116.1`. `nu-command` is built with `default-features = false, features = ["os", "network", "rustls-tls"]`.
- The engine registers only `nu-cmd-lang`, the data commands listed in `engine.rs`, and our commands. No externals, no filesystem commands, no plugins.
- chrono appears only in `time.rs`, in `commands/from_ics.rs` (through `time::to_nu`), and in tests.
- `from ics` never falls back to the machine's local time zone.
- Test fixtures under `fixtures/ics/` are kept byte-for-byte (CRLF line endings), via `.gitattributes`.

## Decisions made while planning

These refine the spec; the spec has been updated to match (same commit as this plan).

1. **Organizers (Wren's choice A):** `CUTYPE=INDIVIDUAL` → `person`, anything else or none → `org`.
2. **Calendar zone** also comes from the `TZID` of a calendar's only VTIMEZONE. St. Olaf's feed names its zone only that way.
3. **`URL` values are unescaped** like TEXT. Carleton escapes them (`&amp\;sport_id=`), which RFC 5545 doesn't allow, and a URI never contains a backslash.
4. **`extra` for ICS rows:** every unmapped property under its lower-cased name, as a list when it repeats (several `ATTENDEE`s). Non-`http(s)` URLs and a `geo` with no location name also go to `extra`, so the record stays valid.
5. **Engine setup:** `$env.PWD` is `/` (`http get` needs a working directory); the default rustls crypto provider is installed (`http get` fails without it); `nu-command` needs its `os` feature to compile next to the other nushell crates. The last one matters for mobile later.
6. **`DURATION` sets `end`;** the event doesn't keep a `duration` field from ICS.
7. **Pipe metadata comes from `scope commands`**, nushell's own reading of doc comments and signatures.
8. **`ppp run` flags:** `--flag value`, `--flag=value` and bare `--switch`; values typed by the flag's declared type (`string`, `int`, `float`, `number`, `bool`). `--pipes-dir` goes before the pipe name.
9. **`campus-week`** filters to `start >= now` as well as the upper bound, and drops its `--tz` flag (the feed names its zone). It's not yet recurrence-aware; that's phase 3.
10. **Deferred to later phases, per the build order:** `html to-text` and `description_html` (phase 5), the HTTP cache, limits and `ppp info` (phase 4), `expand rrule` (phase 3).
11. **Live feed fixtures are trimmed** to the events that show a quirk (Wren's request): St. Olaf 6 events, Carleton 7, Northfield 2, about 15 KB in all.

## Review Focus

Inputs the spec implies that no feature test would naturally hit. Each has a test in the owning task.

1. **A feed that starts with a UTF-8 byte-order mark** must parse normally, not silently yield zero events — Task 1.
2. **A feed the size of Carleton's** (about 2,400 events, 3.2 MB) must parse; the test uses 5,000 synthetic events — Task 2.
3. **A pipe that returns one record** instead of a list must give one record — Task 6.
4. **A pipe file whose path has spaces and quotes** must load (the path is spliced into a nushell `use`) — Task 6.
5. **A pipe that fails while running** must exit 2 with nushell's diagnostic and print nothing on stdout — Task 7.

---

## File structure

```
crates/ppp-core/src/
  ics/mod.rs              module list
  ics/lines.rs            unfolding, content lines, components, TEXT escapes
  ics/events.rs           VEVENT -> Event: zones, dates, durations, recurrence fields
  engine.rs               Engine: sandboxed EngineState, eval, rendered errors
  commands/mod.rs         bind(): registers our commands
  commands/from_ics.rs    the `from ics` nushell command
  pipe.rs                 Pipe::load/run, PipeInfo, check_records
  time.rs                 + to_nu (jiff -> nushell datetime)
crates/ppp-core/tests/
  ics_lines.rs ics_events.rs engine.rs from_ics.rs into_event.rs pipe.rs
crates/ppp-cli/src/
  report.rs               error report shared by validate and run
  run.rs                  `ppp run`
stdlib/pipes/mod.nu       `into event`, `drop-empty`
fixtures/ics/*.ics        KDL examples as ICS, trimmed live feeds
fixtures/pipes/*.nu       small pipes for tests
pipes/campus-week.nu      the first real pipe
```

---

### Task 1: iCalendar text layer

**Files:**
- Create: `crates/ppp-core/src/ics/mod.rs`, `crates/ppp-core/src/ics/lines.rs`
- Modify: `crates/ppp-core/src/lib.rs`
- Test: `crates/ppp-core/tests/ics_lines.rs`

**Interfaces:**
- Consumes: nothing.
- Produces (`ppp_core::ics::lines`):
  - `struct Property { pub name: String, pub params: Vec<(String, String)>, pub value: String }` with `param(&self, &str) -> Option<&str>`
  - `struct Component { pub name: String, pub properties: Vec<Property>, pub components: Vec<Component> }` with `property(&self, &str) -> Option<&Property>` and `all(&self, &str) -> impl Iterator<Item = &Property>`
  - `struct StructureError { pub line: usize, pub message: String }`
  - `fn unfold(&str) -> Vec<(usize, String)>`, `fn parse_line(&str) -> Option<Property>`, `fn parse_components(&str) -> Result<Vec<Component>, StructureError>`, `fn unescape_text(&str) -> String`, `fn split_text_list(&str) -> Vec<String>`

- [ ] **Step 1: Write the failing test**

`crates/ppp-core/tests/ics_lines.rs`:

```rust
//! The iCalendar text layer: unfolding, content lines, components, TEXT.

use ppp_core::ics::lines::{
    Property, parse_components, parse_line, split_text_list, unescape_text, unfold,
};

#[test]
fn unfolds_crlf_and_lf_with_space_or_tab() {
    let text = "SUMMARY:Fall Or\r\n chestra\r\nDESCRIPTION:a\n\tb\n\nEND:VEVENT";
    assert_eq!(
        unfold(text),
        vec![
            (1, "SUMMARY:Fall Orchestra".to_string()),
            (3, "DESCRIPTION:ab".to_string()),
            (6, "END:VEVENT".to_string()),
        ]
    );
}

#[test]
fn splits_name_params_and_value() {
    let prop = parse_line(r#"organizer;cn="Doe; Jane: Office";ROLE=CHAIR:mailto:doe@example.edu"#)
        .unwrap();
    assert_eq!(
        prop,
        Property {
            name: "ORGANIZER".into(),
            params: vec![
                ("CN".into(), "Doe; Jane: Office".into()),
                ("ROLE".into(), "CHAIR".into())
            ],
            value: "mailto:doe@example.edu".into(),
        }
    );
    assert_eq!(prop.param("CN"), Some("Doe; Jane: Office"));
}

#[test]
fn lines_without_a_colon_or_name_are_not_properties() {
    assert_eq!(parse_line("just some text"), None);
    assert_eq!(parse_line(":value"), None);
}

#[test]
fn nests_components() {
    let text = "BEGIN:VCALENDAR\nX-WR-TIMEZONE:America/Chicago\nBEGIN:VTIMEZONE\nTZID:America/Chicago\nBEGIN:STANDARD\nRRULE:FREQ=YEARLY\nEND:STANDARD\nEND:VTIMEZONE\nBEGIN:VEVENT\nUID:a\nBEGIN:VALARM\nACTION:DISPLAY\nEND:VALARM\nEND:VEVENT\nEND:VCALENDAR\n";
    let top = parse_components(text).unwrap();
    assert_eq!(top.len(), 1);
    let cal = &top[0];
    assert_eq!(cal.name, "VCALENDAR");
    assert_eq!(
        cal.property("X-WR-TIMEZONE").unwrap().value,
        "America/Chicago"
    );
    let names: Vec<&str> = cal.components.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["VTIMEZONE", "VEVENT"]);
    assert_eq!(
        cal.components[0].components[0]
            .property("RRULE")
            .unwrap()
            .value,
        "FREQ=YEARLY"
    );
    assert_eq!(cal.components[1].components[0].name, "VALARM");
    assert_eq!(cal.components[1].properties.len(), 1);
}

#[test]
fn mismatched_end_is_an_error() {
    let err = parse_components("BEGIN:VCALENDAR\nBEGIN:VEVENT\nEND:VCALENDAR\n").unwrap_err();
    assert_eq!(err.line, 3);
    assert_eq!(err.message, "END:VCALENDAR closes BEGIN:VEVENT");
}

#[test]
fn unclosed_begin_is_an_error() {
    let err = parse_components("BEGIN:VCALENDAR\nBEGIN:VEVENT\n").unwrap_err();
    assert_eq!(err.message, "BEGIN:VEVENT is never closed");
}

#[test]
fn unescapes_text() {
    assert_eq!(unescape_text(r"a\, b\; c\nd\Ne\\f"), "a, b; c\nd\ne\\f");
}

#[test]
fn splits_text_lists_on_unescaped_commas() {
    assert_eq!(
        split_text_list(r"Music, Concert\, Fall,,  Arts "),
        vec!["Music", "Concert, Fall", "Arts"]
    );
}

#[test]
fn a_byte_order_mark_is_ignored() {
    let top = parse_components("\u{feff}BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n").unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].name, "VCALENDAR");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test ics_lines`
Expected: compile error, `unresolved import ppp_core::ics`.

- [ ] **Step 3: Write the text layer**

`crates/ppp-core/src/ics/lines.rs`:

```rust
//! The iCalendar text layer (RFC 5545 §3.1): unfolds lines, splits each
//! content line into name, parameters and value, and nests components.

/// One content line, e.g. `DTSTART;TZID=America/Chicago:20261014T193000`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// Upper-cased, e.g. `DTSTART`.
    pub name: String,
    /// Upper-cased parameter names with their raw values, quotes removed, in
    /// source order. A parameter with several values keeps them comma-joined.
    pub params: Vec<(String, String)>,
    /// The raw value, still escaped.
    pub value: String,
}

impl Property {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// A `BEGIN:…`/`END:…` block and everything in it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Component {
    /// Upper-cased, e.g. `VEVENT`.
    pub name: String,
    pub properties: Vec<Property>,
    pub components: Vec<Component>,
}

impl Component {
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Property> {
        self.properties.iter().filter(move |p| p.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct StructureError {
    pub line: usize,
    pub message: String,
}

/// Joins folded lines: a line break followed by a space or tab continues
/// the previous line. Accepts CRLF and bare LF. Returns each logical line
/// with the 1-based number of the physical line it starts on. A leading
/// byte-order mark is dropped.
pub fn unfold(text: &str) -> Vec<(usize, String)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<(usize, String)> = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        match line.strip_prefix([' ', '\t']) {
            Some(rest) if !out.is_empty() => out.last_mut().unwrap().1.push_str(rest),
            _ => out.push((i + 1, line.to_string())),
        }
    }
    out.retain(|(_, line)| !line.trim().is_empty());
    out
}

/// Splits `NAME;P1=a;P2="b:c":value`. Returns `None` for a line with no
/// colon outside quotes or with no name.
pub fn parse_line(line: &str) -> Option<Property> {
    let mut in_quotes = false;
    let mut colon = None;
    let mut semis = Vec::new();
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => semis.push(i),
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let head = &line[..colon];
    let name_end = semis.first().copied().unwrap_or(colon);
    let name = head[..name_end].trim();
    if name.is_empty() {
        return None;
    }
    let mut params = Vec::new();
    let mut starts = semis.clone();
    starts.push(colon);
    for pair in starts.windows(2) {
        let part = &line[pair[0] + 1..pair[1]];
        if let Some((key, value)) = part.split_once('=') {
            let value = value.replace('"', "");
            params.push((key.trim().to_ascii_uppercase(), value));
        }
    }
    Some(Property {
        name: name.to_ascii_uppercase(),
        params,
        value: line[colon + 1..].to_string(),
    })
}

/// Parses the whole text into its top-level components (normally one
/// `VCALENDAR`). Lines that are not content lines are skipped.
pub fn parse_components(text: &str) -> Result<Vec<Component>, StructureError> {
    let mut stack: Vec<Component> = Vec::new();
    let mut top = Vec::new();
    for (line_no, line) in unfold(text) {
        let Some(prop) = parse_line(&line) else {
            continue;
        };
        match prop.name.as_str() {
            "BEGIN" => stack.push(Component {
                name: prop.value.trim().to_ascii_uppercase(),
                ..Default::default()
            }),
            "END" => {
                let name = prop.value.trim().to_ascii_uppercase();
                let Some(done) = stack.pop() else {
                    return Err(StructureError {
                        line: line_no,
                        message: format!("END:{name} with no matching BEGIN"),
                    });
                };
                if done.name != name {
                    return Err(StructureError {
                        line: line_no,
                        message: format!("END:{name} closes BEGIN:{}", done.name),
                    });
                }
                match stack.last_mut() {
                    Some(parent) => parent.components.push(done),
                    None => top.push(done),
                }
            }
            _ => {
                if let Some(current) = stack.last_mut() {
                    current.properties.push(prop);
                }
            }
        }
    }
    if let Some(open) = stack.last() {
        return Err(StructureError {
            line: 0,
            message: format!("BEGIN:{} is never closed", open.name),
        });
    }
    Ok(top)
}

/// Undoes TEXT escaping (`\n`, `\N`, `\,`, `\;`, `\\`).
pub fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Splits a TEXT list on unescaped commas, then unescapes and trims each
/// item, dropping empty ones.
pub fn split_text_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                current.push('\\');
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ',' => items.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    items.push(current);
    items
        .iter()
        .map(|item| unescape_text(item).trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}
```

`crates/ppp-core/src/ics/mod.rs`:

```rust
//! iCalendar (RFC 5545) parsing for `from ics`.

pub mod lines;
```

`crates/ppp-core/src/lib.rs`:

```rust
pub mod ics;
pub mod schema;
pub mod serialize;
pub mod time;
pub mod time_fields;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test ics_lines`
Expected: `test result: ok. 9 passed`

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add crates/ppp-core
git commit -m "Add the iCalendar text layer

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 2: Reading VEVENTs

**Files:**
- Create: `crates/ppp-core/src/ics/events.rs`, `fixtures/ics/{minimal,campus-events,team-planning,stolaf,carleton,northfield}.ics`, `.gitattributes`
- Modify: `crates/ppp-core/src/ics/mod.rs`
- Test: `crates/ppp-core/tests/ics_events.rs`

**Interfaces:**
- Consumes: everything in `ppp_core::ics::lines` from Task 1.
- Produces (`ppp_core::ics::events`):
  - `fn parse_events(text: &str, default_tz: Option<&str>) -> Result<Vec<Event>, IcsError>`
  - `struct Event { uid, summary, description, location, url, status, rrule: Option<String>, start, end, recurrence_id: Option<jiff::Zoned>, all_day: bool, tz: Option<String>, categories: Vec<String>, organizer: Option<Organizer>, geo: Option<(f64, f64)>, rdates, exdates: Vec<jiff::Zoned>, created, last_modified: Option<jiff::Timestamp>, props: Vec<Property> }` (all `pub`)
  - `struct Organizer { pub name: Option<String>, pub email: Option<String>, pub cutype: Option<String> }`
  - `enum IcsError { Structure(StructureError), NoZone { uid, property }, UnknownZone { uid, property, tz }, BadValue { uid, property, value }, UnknownDefaultZone(String) }` (derives `Debug, Clone, PartialEq`)

Zone rules (spec §5.2): `TZID`, then the calendar's `X-WR-TIMEZONE`, `TIMEZONE-ID` or only VTIMEZONE, then `default_tz`. A date with no zone anywhere is midnight UTC with `tz: None`.

- [ ] **Step 1: Write the fixtures**

The first three are the KDL examples in iCalendar form; `team-planning.ics` adds a RECURRENCE-ID override for phase 3. The last three are the live feeds as fetched on 2026-10-11, cut down to the events that show a quirk:
- `stolaf.ics`: all-day, timed with GEO and ORGANIZER, Apple structured location, escaped commas, multiple categories, no location.
- `carleton.ics`: floating time, multi-day all-day, HTML description, pre-expanded instance UID, `URL;VALUE=URI` with TEXT escapes, and two copies of one event.
- `northfield.ics`: two events and the VTIMEZONE with nested RRULEs.

Write each file below with LF line endings, then convert them all to CRLF and pin them with `.gitattributes` so git keeps the bytes:

```bash
perl -pi -e 's/\n/\r\n/' fixtures/ics/*.ics
printf 'fixtures/ics/*.ics -text\n' > .gitattributes
```


`fixtures/ics/campus-events.ics`:

```text
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//ppp-pipes//fixtures//EN
X-WR-CALNAME:Campus Events
X-WR-TIMEZONE:America/Chicago
X-WR-CALDESC:Concerts\, lectures\, and student events
BEGIN:VEVENT
UID:20261014-orchestra@calendar.example.edu
DTSTAMP:20261011T000000Z
SUMMARY:Fall Orchestra Concert
DTSTART;TZID=America/Chicago:20261014T193000
DTEND;TZID=America/Chicago:20261014T210000
STATUS:CONFIRMED
LOCATION:Boe Memorial Chapel\, 1520 St. Olaf Avenue\, Northfield\, MN 55057
GEO:44.4614;-93.1822
DESCRIPTION:The orchestra opens its season with Dvořák's Symphony No. 8.\nFree and open to th
 e public.
URL:https://www.example.edu/events/fall-orchestra
CATEGORIES:Music,Concert
ORGANIZER;CN=Music Department:mailto:music@example.edu
CREATED:20260806T181107Z
LAST-MODIFIED:20260902T155315Z
BEGIN:VALARM
ACTION:DISPLAY
TRIGGER:-PT1H
DESCRIPTION:Reminder
END:VALARM
END:VEVENT
BEGIN:VEVENT
UID:20261018-family-weekend@calendar.example.edu
DTSTAMP:20261011T000000Z
SUMMARY:Family Weekend
DTSTART;VALUE=DATE:20261017
DTEND;VALUE=DATE:20261019
STATUS:CONFIRMED
LOCATION:Campus-wide
CATEGORIES:Community
END:VEVENT
BEGIN:VEVENT
UID:weekly-chess@calendar.example.edu
DTSTAMP:20261011T000000Z
SUMMARY:Chess Club
DTSTART:20260908T180000
DURATION:PT2H
LOCATION:Buntrock Commons\, Room 143
CATEGORIES:Clubs
RRULE:FREQ=WEEKLY;BYDAY=TU;UNTIL=20261215T235959Z
EXDATE:20261124T180000
END:VEVENT
BEGIN:VEVENT
UID:20261022-lecture@calendar.example.edu
DTSTAMP:20261011T000000Z
SUMMARY:Guest Lecture: Pipelines Everywhere
DTSTART:20261022T160000Z
DTEND:20261022T171500Z
STATUS:CANCELLED
LOCATION:Regents Hall 150
DESCRIPTION:Cancelled: the speaker's flight was grounded. A new date will be announced.
ORGANIZER;CN=Computer Science:mailto:cs@example.edu
END:VEVENT
END:VCALENDAR
```

`fixtures/ics/carleton.ics`:

```text
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Carleton College//Calendar Events//EN
NAME:Carleton College: Campus Calendar Events
X-WR-CALNAME:Carleton College: Campus Calendar Events
TIMEZONE-ID:America/Chicago
X-WR-TIMEZONE:America/Chicago
REFRESH-INTERVAL;VALUE=DURATION:PT6H
X-PUBLISHED-TTL:PT6H
BEGIN:VEVENT
UID:80qf65vj24c4hnjrp6urbhoiec
SEQUENCE:1
DTSTAMP:20261012T011647
DTSTART;VALUE=DATE:20260908
DTEND;VALUE=DATE:20261119
X-MICROSOFT-CDO-ALLDAYEVENT:TRUE
X-MICROSOFT-MSNCALENDAR-ALLDAYEVENT:TRUE
SUMMARY:Gould Library Fall Term Exhibitions
TRANSP:TRANSPARENT
LOCATION:Gould Library
DESCRIPTION:Gould Library hosts a variety of <a href="https://www.carleton
 .edu/library/exhibitions/current/">exhibitions that rotate throughout the 
 academic year</a>. This year's programming focuses on celebrating the 100t
 h year of <a href="https://www.carleton.edu/arboretum/">Cowling Arboretum<
 /a>.\n\nVisit during fall term to see:\n\n<em>100 Years of the Arb</em>\n\
 n<em>Carleton’s Herbarium</em>\n\n<em>Lost and Found: Treasures from the
  Arb</em>\n\n<em>Tim Lloyd: Prairie Vessels</em>\n\n<em>Cowling Arboretum 
 Contemplative Transit System (C.A.C.T.S) Map</em>\n\n<em>Monuments: Prints
  by John-Mark Schlink</em>\n\nEvent Details:\nhttps://www.carleton.edu/lib
 rary/exhibitions/current/\n\nView this event on the Carleton College websi
 te:\nhttps://www.carleton.edu/_c/uuff
ORGANIZER;CN="Gould Library":mailto:8hmht0hmg17227b3oqh02sdff8@group.calen
 dar.google.com
STATUS:CONFIRMED
CREATED:20260909T144819
END:VEVENT
BEGIN:VEVENT
UID:la0q9a2ht1hjg7jbi7esrgam8c
SEQUENCE:0
DTSTAMP:20261012T011647
DTSTART;VALUE=DATE:20260924
DTEND;VALUE=DATE:20261119
X-MICROSOFT-CDO-ALLDAYEVENT:TRUE
X-MICROSOFT-MSNCALENDAR-ALLDAYEVENT:TRUE
SUMMARY:Field Notes: Studio Art Faculty Respond to the Arboretum’s Cente
 nnial Year
TRANSP:TRANSPARENT
LOCATION:Perlman Teaching Museum
DESCRIPTION:<blockquote>This exhibition presents new work by Carleton stud
 io art faculty in response to the Carleton College Cowling Arboretum’s l
 andscapes\, histories\, and ecological significance. It marks the 100th an
 niversary of the Arboretum\, an 800-acre living laboratory of prairie\, fo
 rest\, and river habitats that has shaped teaching\, research\, and life o
 n campus since the 1920s.<span style="color: #000000">\n</span><span style
 ="color: #000000">\nThe accompanying events program will include talks by 
 participating studio art faculty\, offering insight into their creative pr
 ocesses.</span></blockquote>\n\nView this event on the Carleton College we
 bsite:\nhttps://www.carleton.edu/_c/utaX
ORGANIZER;CN="Perlman Teaching Museum":mailto:7l4kbotpo5o218cf1dd40m7e8g@g
 roup.calendar.google.com
STATUS:CONFIRMED
CREATED:20260121T160129
END:VEVENT
BEGIN:VEVENT
UID:gmnm0o160ok6onorumja1oafng_20261011T210000Z
SEQUENCE:0
DTSTAMP:20261012T011647
DTSTART:20261011T160000
DTEND:20261011T220000
SUMMARY:QRC drop-in tutoring
TRANSP:TRANSPARENT
LOCATION:QRC\, 4th Libe\, East Wing
DESCRIPTION:Students can drop in for help with math\, statistics\, data\, 
 and graphs in any QRE course. To make an appointment or to see who's worki
 ng\, go to <a href="http://go.carleton.edu/QRC">go.carleton.edu/QRC</a>\n
 \nView this event on the Carleton College website:\nhttps://www.carleton.e
 du/_c/roUA
ORGANIZER;CN="Quantitative Resource Center":mailto:4eq2p4rs1fbo7s7t1k0plfl
 90o@group.calendar.google.com
STATUS:CONFIRMED
CREATED:20260916T195435
END:VEVENT
BEGIN:VEVENT
UID:d32qaac9f8nuo3460859id79ig_20261012T000000Z
SEQUENCE:0
DTSTAMP:20261012T011647
DTSTART:20261011T190000
DTEND:20261011T200000
SUMMARY:Catholic Bible Study
TRANSP:TRANSPARENT
LOCATION:Christian Chapel
DESCRIPTION:The Catholic Student Association will be reading through the G
 ospel according to Matthew and holding weekly conversations to foster grow
 th and understanding.\n\nView this event on the Carleton College website:\
 nhttps://www.carleton.edu/_c/roPN
ORGANIZER;CN="Chaplain's Office":mailto:gc0r4cojm5g3aensafau0dppd8@group.c
 alendar.google.com
STATUS:CONFIRMED
CREATED:20260908T142429
END:VEVENT
BEGIN:VEVENT
UID:vcal_10972-athletics.carleton.edu
SEQUENCE:0
DTSTAMP:20261012T011647
DTSTART:20261012T093000
DTEND:20261012T123000
SUMMARY:Carleton College Men's Golf at MIAC Championships
LOCATION:Brooklyn Park\, Minn. / Edinburgh USA
DESCRIPTION:Carleton College Men's Golf at MIAC Championships\n\n\nView th
 is event on the Carleton College website:\nhttps://www.carleton.edu/_c/und
 efined
URL;VALUE=URI:https://athletics.carleton.edu/calendar.aspx?game_id=10972&a
 mp\;sport_id=19
PRIORITY:3
END:VEVENT
BEGIN:VEVENT
UID:famkt1q7c1jiori9gt58u97nl0
SEQUENCE:0
DTSTAMP:20261012T011647
DTSTART:20261012T110000
DTEND:20261012T140000
SUMMARY:DIS Study Abroad in Scandinavia Info Table
TRANSP:TRANSPARENT
LOCATION:Sayles Table 3
DESCRIPTION:<span data-sheets-root="1">DIS in Copenhagen and Stockholm off
 ers 300+ courses taught in English by local experts\, with study tours aro
 und Europe and to the Arctic. Deepen your knowledge\, broaden your skills\
 , and apply your ideas alongside peers from other top U.S. colleges. Major
 s include Bio/Pre-Med\, ENTS\, and Psych/Neuro\, plus niche fields like As
 tropolitics\, Urban Studies\, and more. https://www.youtube.com/c/StudyAbr
 oadDIS</span>\n\nEvent Details:\nhttps://disabroad.org/\n\nView this event
  on the Carleton College website:\nhttps://www.carleton.edu/_c/uumG
ORGANIZER;CN="Off Campus Studies":mailto:4iqiub2cjhnjgpqu2joiv600f8@group.
 calendar.google.com
STATUS:CONFIRMED
CREATED:20260917T180011
END:VEVENT
BEGIN:VEVENT
UID:r1dpcidfj801hk8b91j9rj2sns
SEQUENCE:0
DTSTAMP:20261012T011647
DTSTART:20261012T110000
DTEND:20261012T140000
SUMMARY:DIS Study Abroad in Scandinavia Info Table
TRANSP:TRANSPARENT
LOCATION:Sayles Table 3
DESCRIPTION:<span data-sheets-root="1">DIS in Copenhagen and Stockholm off
 ers 300+ courses taught in English by local experts\, with study tours aro
 und Europe and to the Arctic. Deepen your knowledge\, broaden your skills\
 , and apply your ideas alongside peers from other top U.S. colleges. Major
 s include Bio/Pre-Med\, ENTS\, and Psych/Neuro\, plus niche fields like As
 tropolitics\, Urban Studies\, and more. https://www.youtube.com/c/StudyAbr
 oadDIS</span>\n\nEvent Details:\nhttps://disabroad.org/\n\nView this event
  on the Carleton College website:\nhttps://www.carleton.edu/_c/uumH
ORGANIZER;CN="Off Campus Studies":mailto:4iqiub2cjhnjgpqu2joiv600f8@group.
 calendar.google.com
STATUS:CONFIRMED
CREATED:20260820T104308
END:VEVENT
END:VCALENDAR
```

`fixtures/ics/minimal.ics`:

```text
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//ppp-pipes//fixtures//EN
X-WR-CALNAME:Personal
X-WR-TIMEZONE:UTC
BEGIN:VEVENT
UID:dentist-2026-10-20
DTSTAMP:20261011T000000Z
SUMMARY:Dentist
DTSTART:20261020T081500Z
END:VEVENT
END:VCALENDAR
```

`fixtures/ics/northfield.ics`:

```text
BEGIN:VCALENDAR
PRODID:iCalendar-Ruby
VERSION:2.0
X-WR-TIMEZONE:America/Chicago
BEGIN:VTIMEZONE
TZID:America/Chicago
X-LIC-LOCATION:America/Chicago
BEGIN:STANDARD
DTSTART:20241103T020000
RRULE:FREQ=YEARLY;BYDAY=1SU;BYMONTH=11
TZNAME:CST
TZOFFSETFROM:-0500
TZOFFSETTO:-0600
END:STANDARD
BEGIN:DAYLIGHT
DTSTART:20250309T020000
RRULE:FREQ=YEARLY;BYDAY=2SU;BYMONTH=3
TZNAME:CDT
TZOFFSETFROM:-0600
TZOFFSETTO:-0500
END:DAYLIGHT
END:VTIMEZONE
BEGIN:VEVENT
DESCRIPTION: https://www.northfieldmn.gov/calendar.aspx?EID=8677
DTEND;TZID=America/Chicago:20340801T220000
DTSTAMP;TZID=America/Chicago:20250627T094049
DTSTART;TZID=America/Chicago:20340801T100000
LAST-MODIFIED;TZID=America/Chicago:20250627T094049
LOCATION:<p>In your neighborhood</p> -     
SEQUENCE:0
SUMMARY:Night to Unite
UID:8677
URL:/common/modules/iCalendar/iCalendar.aspx?feed=calendar&catID=41
END:VEVENT
BEGIN:VEVENT
DESCRIPTION: https://www.northfieldmn.gov/calendar.aspx?EID=8676
DTEND;TZID=America/Chicago:20330802T220000
DTSTAMP;TZID=America/Chicago:20250627T094049
DTSTART;TZID=America/Chicago:20330802T100000
LAST-MODIFIED;TZID=America/Chicago:20250627T094049
LOCATION:<p>In your neighborhood</p> -     
SEQUENCE:0
SUMMARY:Night to Unite
UID:8676
URL:/common/modules/iCalendar/iCalendar.aspx?feed=calendar&catID=41
END:VEVENT
END:VCALENDAR
```

`fixtures/ics/stolaf.ics`:

```text
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Calendar - ECPv6.18.0//NONSGML v1.0//EN
CALSCALE:GREGORIAN
METHOD:PUBLISH
X-WR-CALNAME:Calendar
X-ORIGINAL-URL:https://wp.stolaf.edu/calendar
X-WR-CALDESC:Events for Calendar
REFRESH-INTERVAL;VALUE=DURATION:PT1H
X-Robots-Tag:noindex
X-PUBLISHED-TTL:PT1H
BEGIN:VTIMEZONE
TZID:America/Chicago
BEGIN:DAYLIGHT
TZOFFSETFROM:-0600
TZOFFSETTO:-0500
TZNAME:CDT
DTSTART:20250309T080000
END:DAYLIGHT
BEGIN:STANDARD
TZOFFSETFROM:-0500
TZOFFSETTO:-0600
TZNAME:CST
DTSTART:20251102T070000
END:STANDARD
BEGIN:DAYLIGHT
TZOFFSETFROM:-0600
TZOFFSETTO:-0500
TZNAME:CDT
DTSTART:20260308T080000
END:DAYLIGHT
BEGIN:STANDARD
TZOFFSETFROM:-0500
TZOFFSETTO:-0600
TZNAME:CST
DTSTART:20261101T070000
END:STANDARD
BEGIN:DAYLIGHT
TZOFFSETFROM:-0600
TZOFFSETTO:-0500
TZNAME:CDT
DTSTART:20270314T080000
END:DAYLIGHT
BEGIN:STANDARD
TZOFFSETFROM:-0500
TZOFFSETTO:-0600
TZNAME:CST
DTSTART:20271107T070000
END:STANDARD
END:VTIMEZONE
BEGIN:VEVENT
DTSTART;VALUE=DATE:20260911
DTEND;VALUE=DATE:20261102
DTSTAMP:20260911T174843Z
CREATED:20260824T144725Z
LAST-MODIFIED:20260911T174843Z
UID:10038154-1789084800-1793577599@wp.stolaf.edu
SUMMARY:Brenda Berkman ’73: Thirty-Six Views of One World Trade Center
DESCRIPTION:Marking a quarter of a century since the September 11\, 2001 attacks\, the Flaten Art Museum highlights a print series from its collection: Thirty-Six Views of One World Trade Center by St. Olaf alumna Brenda Berkman ’73. Berkman — the sole named class plaintiff who opened the New York City Fire Department to women firefighters in 1982 — was a fire officer and first responder at the World Trade Center on 9/11\, witnessing the destruction and loss that reverberated across the country.
URL:https://wp.stolaf.edu/calendar/event/brenda-berkman-36-views-of-one-world-trade-center/
CATEGORIES:Alumni,Performing and Visual Arts
ORGANIZER;CN="Flaten Art Museum":MAILTO:beckerj@stolaf.edu
LOCATION:Center for Art and Dance Groot Gallery
GEO:44.4582983;-93.161604
END:VEVENT
BEGIN:VEVENT
DTSTART;VALUE=DATE:20260911
DTEND;VALUE=DATE:20261207
DTSTAMP:20260827T201045Z
CREATED:20260601T143927Z
LAST-MODIFIED:20260827T201045Z
UID:10037911-1789084800-1796601599@wp.stolaf.edu
SUMMARY:Michon Weeks: Mystic Topographies
DESCRIPTION:Flaten Art Museum Exhibition – Michon Weeks: Mystic Topographies \nFor Michon Weeks\, painting is a contemplative practice akin to meditation and prayer. Working in egg tempera\, oil\, and acrylic\, Weeks paints the quiet terrain of everyday life: yards\, tree lines\, and utility poles. But these paintings do not describe the ordinary elements of the Minnesota landscape. They are the artist’s method for slow\, sustained presence—and an invitation to the viewer to stay with something long enough for meaning to surface. \nThis body of work has emerged alongside a sustained creative exchange with author\, scholar\, and fellow St. Olaf professor Trish Zimmerman. Audio excerpts from the mystics\, read and recorded by Weeks and Zimmerman\, invite visitors to explore the sacred mystery of perception.
URL:https://wp.stolaf.edu/calendar/event/michon-weeks-mystic-topographies/
CATEGORIES:Performing and Visual Arts
ORGANIZER;CN="Flaten Art Museum":MAILTO:beckerj@stolaf.edu
LOCATION:Center for Art and Dance
GEO:44.4582983;-93.161604
END:VEVENT
BEGIN:VEVENT
DTSTART;VALUE=DATE:20261010
DTEND;VALUE=DATE:20261014
DTSTAMP:20260904T150844Z
CREATED:20260507T211236Z
LAST-MODIFIED:20260904T150844Z
UID:10037839-1791590400-1791935999@wp.stolaf.edu
SUMMARY:Fall Break (no classes)
DESCRIPTION:
URL:https://wp.stolaf.edu/calendar/event/fall-break-no-classes-3/
CATEGORIES:Academic Year
END:VEVENT
BEGIN:VEVENT
DTSTART;TZID=America/Chicago:20261011T160000
DTEND;TZID=America/Chicago:20261013T110000
DTSTAMP:20260929T190952Z
CREATED:20260929T190952Z
LAST-MODIFIED:20260929T190952Z
UID:10038652-1791734400-1791889200@wp.stolaf.edu
SUMMARY:NetVue Undergraduate Education Conference
DESCRIPTION:The Lutheran Center invites St. Olaf faculty and staff to attend the conference’s plenary and breakout sessions Sunday\, October 11-Tuesday\, October 13. To view the schedule\, please follow this link: https://wp.stolaf.edu/lutherancenter/netvue-regional-conference-2026/.
URL:https://wp.stolaf.edu/calendar/event/netvue-undergraduate-education-conference/
CATEGORIES:Conferences/Camps
LOCATION:Buntrock Commons\, 1521 St Olaf Ave\, Northfield\, MN\, United States
GEO:44.468533;-93.162848
X-APPLE-STRUCTURED-LOCATION;VALUE=URI;X-ADDRESS=Buntrock Commons 1521 St Olaf Ave Northfield MN United States;X-APPLE-RADIUS=500;X-TITLE=1521 St Olaf Ave:geo:-93.162848,44.468533
END:VEVENT
BEGIN:VEVENT
DTSTART;TZID=America/Chicago:20261015T201500
DTEND;TZID=America/Chicago:20261015T211500
DTSTAMP:20261009T152516Z
CREATED:20260728T205008Z
LAST-MODIFIED:20261009T152516Z
UID:10038038-1792095300-1792098900@wp.stolaf.edu
SUMMARY:Student Recital
DESCRIPTION:Faculty\, staff\, students\, and the wider community are welcome to attend this Music Department recital\, featuring a variety of students fulfilling their degree requirements. \nThis event is free and open to the public.
URL:https://wp.stolaf.edu/calendar/event/student-recital-3/
CATEGORIES:Music,Performing and Visual Arts
ORGANIZER;CN="Music Department":MAILTO:wessel5@stolaf.edu
LOCATION:Christiansen Hall of Music 100 Urness Recital Hall
GEO:44.4582983;-93.161604
END:VEVENT
BEGIN:VEVENT
DTSTART;TZID=America/Chicago:20261016T190000
DTEND;TZID=America/Chicago:20261016T200000
DTSTAMP:20260930T190320Z
CREATED:20260723T190811Z
LAST-MODIFIED:20260930T190320Z
UID:10038026-1792177200-1792180800@wp.stolaf.edu
SUMMARY:Boe Steinway Dedication Recital: Soojung Hong\, piano
DESCRIPTION:Faculty\, staff\, students\, and the wider community are welcome to attend this faculty/guest recital featuring Soojung Hong\, piano\, and celebrating the new Steinway piano in Boe Memorial Chapel. \nDr. Soojung Hong has won national and international recognition as a pianist\, and performs as a soloist or with ensemble throughout the Midwest and beyond. Her work highlights the music of female composers. See this dedication in her video production of Fanny Mendelssohn’s cycle “Das Jahr” (“The Year”)—supported by the Minnesota State Arts Board’s Artist Initiative Grant— among other notable projects.  \nDr. Hong is also a devoted educator\, and many college campuses welcome her master class presentations and competition adjudications. Notably\, she serves as Founder and Artist Director for the PianoFun Minnesota Festival\, a piano professor at Normandale Community College\, and a piano faculty member at MacPhail Center for Music and North Central University.    \nThis event is free and open to the public.
URL:https://wp.stolaf.edu/calendar/event/boe-steinway-consecration-recital-soojung-hong-piano/
CATEGORIES:Music,Performing and Visual Arts
ORGANIZER;CN="Music Department":MAILTO:wessel5@stolaf.edu
LOCATION:Boe Memorial Chapel
GEO:44.4582983;-93.161604
END:VEVENT
END:VCALENDAR
```

`fixtures/ics/team-planning.ics`:

```text
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//ppp-pipes//fixtures//EN
X-WR-CALNAME:Product Team
X-WR-TIMEZONE:America/New_York
BEGIN:VEVENT
UID:c1f6a3e2-5b7d-4e0a-9f3c-1d2e3f4a5b6c
DTSTAMP:20261011T000000Z
SUMMARY:Quarterly planning
DTSTART;TZID=Europe/London:20261102T100000
DTEND;TZID=Europe/London:20261102T150000
STATUS:TENTATIVE
LOCATION:London office\, 4th floor
ORGANIZER;CN=Priya Shah;CUTYPE=INDIVIDUAL:mailto:priya@example.com
ATTENDEE;CN=Priya Shah;ROLE=CHAIR;PARTSTAT=ACCEPTED:mailto:priya@example.com
ATTENDEE;CN=Sam Okafor;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION:mailto:sam@example.com
END:VEVENT
BEGIN:VEVENT
UID:standup-2026@product.example.com
DTSTAMP:20261011T000000Z
SUMMARY:Daily standup
DTSTART;TZID=America/New_York:20261012T093000
DURATION:PT15M
RRULE:FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR
EXDATE;TZID=America/New_York:20261126T093000,20261127T093000
END:VEVENT
BEGIN:VEVENT
UID:standup-2026@product.example.com
DTSTAMP:20261011T000000Z
RECURRENCE-ID;TZID=America/New_York:20261014T093000
SUMMARY:Daily standup (moved)
DTSTART;TZID=America/New_York:20261014T110000
DURATION:PT15M
END:VEVENT
END:VCALENDAR
```

- [ ] **Step 2: Write the failing test**

`crates/ppp-core/tests/ics_events.rs`:

```rust
//! Reading VEVENTs: zones, dates, durations, recurrence fields, organizers,
//! and the saved live feeds.

use std::fs;
use std::path::Path;

use ppp_core::ics::events::{Event, IcsError, Organizer, parse_events};

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/ics")
            .join(name),
    )
    .unwrap()
}

/// A calendar wrapping the given VEVENT lines, with the given calendar-level lines.
fn calendar(header: &[&str], event: &[&str]) -> String {
    let mut lines = vec!["BEGIN:VCALENDAR"];
    lines.extend_from_slice(header);
    lines.push("BEGIN:VEVENT");
    lines.push("UID:x@example.edu");
    lines.extend_from_slice(event);
    lines.extend(["END:VEVENT", "END:VCALENDAR"]);
    lines.join("\r\n")
}

fn one(text: &str, tz: Option<&str>) -> Event {
    let mut events = parse_events(text, tz).unwrap();
    assert_eq!(events.len(), 1);
    events.remove(0)
}

fn show(z: &Option<jiff::Zoned>) -> String {
    z.as_ref().unwrap().to_string()
}

#[test]
fn tzid_times_are_zoned() {
    let e = one(
        &calendar(&[], &["DTSTART;TZID=America/Chicago:20261014T193000"]),
        None,
    );
    assert_eq!(show(&e.start), "2026-10-14T19:30:00-05:00[America/Chicago]");
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
    assert!(!e.all_day);
}

#[test]
fn floating_times_use_the_calendar_zone() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &["DTSTART:20261013T120000"],
        ),
        None,
    );
    assert_eq!(show(&e.start), "2026-10-13T12:00:00-05:00[America/Chicago]");
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
}

#[test]
fn timezone_id_and_a_single_vtimezone_also_name_the_calendar_zone() {
    let by_id = one(
        &calendar(
            &["TIMEZONE-ID:America/Denver"],
            &["DTSTART:20261013T120000"],
        ),
        None,
    );
    assert_eq!(by_id.tz.as_deref(), Some("America/Denver"));
    let by_vtimezone = one(
        &calendar(
            &["BEGIN:VTIMEZONE", "TZID:America/New_York", "END:VTIMEZONE"],
            &["DTSTART:20261013T120000"],
        ),
        None,
    );
    assert_eq!(by_vtimezone.tz.as_deref(), Some("America/New_York"));
}

#[test]
fn the_calendar_zone_wins_over_the_tz_flag() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &["DTSTART:20261013T120000"],
        ),
        Some("Europe/London"),
    );
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
}

#[test]
fn the_tz_flag_fills_in_when_the_calendar_has_no_zone() {
    let e = one(
        &calendar(&[], &["DTSTART:20261013T120000"]),
        Some("Europe/London"),
    );
    assert_eq!(show(&e.start), "2026-10-13T12:00:00+01:00[Europe/London]");
}

#[test]
fn floating_time_with_no_zone_anywhere_is_an_error() {
    let err = parse_events(&calendar(&[], &["DTSTART:20261013T120000"]), None).unwrap_err();
    assert_eq!(
        err.to_string(),
        "event x@example.edu: DTSTART has a floating time and the calendar has no time zone; pass --tz"
    );
}

#[test]
fn utc_times_keep_the_calendar_zone_for_display() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &["DTSTART:20261022T160000Z"],
        ),
        None,
    );
    assert_eq!(show(&e.start), "2026-10-22T16:00:00+00:00[UTC]");
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
}

#[test]
fn all_day_dates_without_any_zone_are_midnight_utc() {
    let e = one(
        &calendar(
            &[],
            &["DTSTART;VALUE=DATE:20261017", "DTEND;VALUE=DATE:20261019"],
        ),
        None,
    );
    assert!(e.all_day);
    assert_eq!(show(&e.start), "2026-10-17T00:00:00+00:00[UTC]");
    assert_eq!(show(&e.end), "2026-10-19T00:00:00+00:00[UTC]");
    assert_eq!(e.tz, None);
}

#[test]
fn all_day_dates_with_a_calendar_zone_are_local_midnight() {
    let e = one(
        &calendar(&["X-WR-TIMEZONE:America/Chicago"], &["DTSTART:20261017"]),
        None,
    );
    assert!(e.all_day);
    assert_eq!(show(&e.start), "2026-10-17T00:00:00-05:00[America/Chicago]");
    assert_eq!(e.tz.as_deref(), Some("America/Chicago"));
}

#[test]
fn duration_sets_the_end() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/Chicago"],
            &["DTSTART:20260908T180000", "DURATION:PT2H"],
        ),
        None,
    );
    assert_eq!(show(&e.end), "2026-09-08T20:00:00-05:00[America/Chicago]");
}

#[test]
fn spring_forward_gap_moves_later() {
    // 02:30 does not exist in Chicago on 2026-03-08; RFC 5545 reads it as 03:30.
    let e = one(
        &calendar(&[], &["DTSTART;TZID=America/Chicago:20260308T023000"]),
        None,
    );
    assert_eq!(show(&e.start), "2026-03-08T03:30:00-05:00[America/Chicago]");
}

#[test]
fn unknown_tzid_is_an_error() {
    let err = parse_events(
        &calendar(&[], &["DTSTART;TZID=Central Standard Time:20261013T120000"]),
        None,
    )
    .unwrap_err();
    assert_eq!(
        err,
        IcsError::UnknownZone {
            uid: "x@example.edu".into(),
            property: "DTSTART".into(),
            tz: "Central Standard Time".into()
        }
    );
}

#[test]
fn unknown_tz_flag_is_an_error() {
    let err = parse_events(&calendar(&[], &[]), Some("Mars/Olympus")).unwrap_err();
    assert_eq!(err, IcsError::UnknownDefaultZone("Mars/Olympus".into()));
}

#[test]
fn bad_dtstart_is_an_error_but_bad_created_is_skipped() {
    let err = parse_events(&calendar(&[], &["DTSTART:2026-10-13"]), None).unwrap_err();
    assert!(matches!(err, IcsError::BadValue { ref property, .. } if property == "DTSTART"));
    let e = one(
        &calendar(&[], &["DTSTART:20261013T120000Z", "CREATED:yesterday"]),
        None,
    );
    assert_eq!(e.created, None);
    assert!(e.props.iter().any(|p| p.name == "CREATED"));
}

#[test]
fn recurrence_fields() {
    let e = one(
        &calendar(
            &["X-WR-TIMEZONE:America/New_York"],
            &[
                "DTSTART:20261012T093000",
                "RRULE:FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR",
                "RDATE:20261017T093000",
                "EXDATE:20261126T093000,20261127T093000",
                "EXDATE;VALUE=DATE:20261225",
                "RECURRENCE-ID:20261014T093000",
            ],
        ),
        None,
    );
    assert_eq!(e.rrule.as_deref(), Some("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR"));
    assert_eq!(e.rdates.len(), 1);
    let exdates: Vec<String> = e.exdates.iter().map(|z| z.to_string()).collect();
    assert_eq!(
        exdates,
        [
            "2026-11-26T09:30:00-05:00[America/New_York]",
            "2026-11-27T09:30:00-05:00[America/New_York]",
            "2026-12-25T00:00:00-05:00[America/New_York]"
        ]
    );
    assert_eq!(
        show(&e.recurrence_id),
        "2026-10-14T09:30:00-04:00[America/New_York]"
    );
}

#[test]
fn text_fields_organizer_geo_and_categories() {
    let e = one(
        &calendar(
            &[],
            &[
                "DTSTART:20261022T160000Z",
                "SUMMARY:Fall Orchestra\\, Live",
                "LOCATION:Boe Chapel\\; Main Floor",
                "DESCRIPTION:Line one\\nLine two",
                "GEO:44.4614;-93.1822",
                "CATEGORIES:Music,Concert",
                "CATEGORIES:Arts",
                "ORGANIZER;CN=Priya Shah;CUTYPE=individual:MAILTO:priya@example.com",
                "STATUS:CONFIRMED",
            ],
        ),
        None,
    );
    assert_eq!(e.summary.as_deref(), Some("Fall Orchestra, Live"));
    assert_eq!(e.location.as_deref(), Some("Boe Chapel; Main Floor"));
    assert_eq!(e.description.as_deref(), Some("Line one\nLine two"));
    assert_eq!(e.geo, Some((44.4614, -93.1822)));
    assert_eq!(e.categories, ["Music", "Concert", "Arts"]);
    assert_eq!(
        e.organizer,
        Some(Organizer {
            name: Some("Priya Shah".into()),
            email: Some("priya@example.com".into()),
            cutype: Some("INDIVIDUAL".into())
        })
    );
    assert_eq!(e.status.as_deref(), Some("CONFIRMED"));
}

#[test]
fn vtimezone_rules_and_alarms_are_not_events() {
    let events = parse_events(&fixture("northfield.ics"), None).unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e.rrule.is_none()));
    let campus = parse_events(&fixture("campus-events.ics"), None).unwrap();
    assert_eq!(campus.len(), 4);
}

#[test]
fn live_feed_snapshots_parse() {
    for (name, count) in [
        ("stolaf.ics", 6),
        ("carleton.ics", 7),
        ("northfield.ics", 2),
    ] {
        let events = parse_events(&fixture(name), None).unwrap();
        assert_eq!(events.len(), count, "{name}");
        assert!(
            events.iter().all(|e| e.start.is_some() && e.tz.is_some()),
            "{name}"
        );
    }
}

#[test]
fn kdl_examples_parse() {
    let team = parse_events(&fixture("team-planning.ics"), None).unwrap();
    assert_eq!(team.len(), 3);
    assert_eq!(team[0].tz.as_deref(), Some("Europe/London"));
    assert_eq!(team[2].summary.as_deref(), Some("Daily standup (moved)"));
    assert!(team[2].recurrence_id.is_some());
    let minimal = parse_events(&fixture("minimal.ics"), None).unwrap();
    assert_eq!(minimal[0].tz.as_deref(), Some("UTC"));
}

#[test]
fn text_escapes_in_urls_are_removed() {
    let e = one(
        &calendar(
            &[],
            &[
                "DTSTART:20261022T160000Z",
                r"URL;VALUE=URI:https://example.edu/a?x=1&y\;z=2",
            ],
        ),
        None,
    );
    assert_eq!(e.url.as_deref(), Some("https://example.edu/a?x=1&y;z=2"));
}

#[test]
fn a_feed_the_size_of_carletons_parses() {
    // Carleton's live feed has about 2,400 events in 3.2 MB.
    let mut text = String::from("BEGIN:VCALENDAR\r\nX-WR-TIMEZONE:America/Chicago\r\n");
    for i in 0..5000 {
        text.push_str(&format!(
            "BEGIN:VEVENT\r\nUID:{i}@example.edu\r\nSUMMARY:Event {i}\r\nDTSTART:20261013T120000\r\nDESCRIPTION:{}\r\nEND:VEVENT\r\n",
            "x".repeat(500)
        ));
    }
    text.push_str("END:VCALENDAR\r\n");
    let events = parse_events(&text, None).unwrap();
    assert_eq!(events.len(), 5000);
    assert_eq!(events[4999].uid.as_deref(), Some("4999@example.edu"));
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test ics_events`
Expected: compile error, `unresolved import ppp_core::ics::events`.

- [ ] **Step 4: Write the event reader**

`crates/ppp-core/src/ics/events.rs`:

```rust
//! Reads the VEVENTs of a calendar into typed events (spec §5.2).

use jiff::civil;
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Zoned};

use super::lines::{
    Component, Property, StructureError, parse_components, split_text_list, unescape_text,
};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum IcsError {
    #[error("{0}")]
    Structure(#[from] StructureError),
    #[error(
        "event {uid}: {property} has a floating time and the calendar has no time zone; pass --tz"
    )]
    NoZone { uid: String, property: String },
    #[error("event {uid}: {property} names unknown time zone \"{tz}\"")]
    UnknownZone {
        uid: String,
        property: String,
        tz: String,
    },
    #[error("event {uid}: cannot read {property} value \"{value}\"")]
    BadValue {
        uid: String,
        property: String,
        value: String,
    },
    #[error("unknown time zone \"{0}\" given with --tz")]
    UnknownDefaultZone(String),
}

/// One VEVENT, with the columns of `nushell-ics/ics.nu` plus `tz`,
/// `rdates`, `exdates` and `recurrence_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub uid: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub url: Option<String>,
    pub status: Option<String>,
    pub start: Option<Zoned>,
    pub end: Option<Zoned>,
    /// True when DTSTART is a DATE (`VALUE=DATE` or eight digits).
    pub all_day: bool,
    /// The IANA zone of DTSTART (or of the calendar, for UTC starts);
    /// `None` when no zone exists anywhere.
    pub tz: Option<String>,
    pub categories: Vec<String>,
    pub organizer: Option<Organizer>,
    pub geo: Option<(f64, f64)>,
    pub rrule: Option<String>,
    pub rdates: Vec<Zoned>,
    pub exdates: Vec<Zoned>,
    pub recurrence_id: Option<Zoned>,
    pub created: Option<Timestamp>,
    pub last_modified: Option<Timestamp>,
    /// Every property of the VEVENT, as read.
    pub props: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organizer {
    /// The `CN` parameter.
    pub name: Option<String>,
    /// The value with any `mailto:` removed.
    pub email: Option<String>,
    /// The `CUTYPE` parameter, upper-cased (e.g. `INDIVIDUAL`, `GROUP`).
    pub cutype: Option<String>,
}

/// Reads every VEVENT in `text`. `default_tz` (from `--tz`) is used only
/// when the calendar names no zone itself.
pub fn parse_events(text: &str, default_tz: Option<&str>) -> Result<Vec<Event>, IcsError> {
    if let Some(tz) = default_tz {
        TimeZone::get(tz).map_err(|_| IcsError::UnknownDefaultZone(tz.to_string()))?;
    }
    let mut events = Vec::new();
    for calendar in parse_components(text)? {
        let zone = calendar_zone(&calendar).or(default_tz.map(str::to_string));
        for component in calendar.components.iter().filter(|c| c.name == "VEVENT") {
            events.push(read_event(component, zone.as_deref())?);
        }
    }
    Ok(events)
}

/// The calendar's own zone: `X-WR-TIMEZONE`, else `TIMEZONE-ID`, else the
/// TZID of its only VTIMEZONE.
fn calendar_zone(calendar: &Component) -> Option<String> {
    for name in ["X-WR-TIMEZONE", "TIMEZONE-ID"] {
        if let Some(prop) = calendar.property(name) {
            return Some(prop.value.trim().to_string());
        }
    }
    let mut zones = calendar.components.iter().filter(|c| c.name == "VTIMEZONE");
    match (zones.next(), zones.next()) {
        (Some(only), None) => only.property("TZID").map(|p| p.value.trim().to_string()),
        _ => None,
    }
}

struct Reader<'a> {
    uid: String,
    calendar_zone: Option<&'a str>,
}

fn read_event(event: &Component, calendar_zone: Option<&str>) -> Result<Event, IcsError> {
    let uid = event.property("UID").map(|p| p.value.trim().to_string());
    let reader = Reader {
        uid: uid.clone().unwrap_or_else(|| "(no UID)".into()),
        calendar_zone,
    };
    let text = |name: &str| event.property(name).map(|p| unescape_text(&p.value));
    let raw = |name: &str| event.property(name).map(|p| p.value.trim().to_string());

    let start = event
        .property("DTSTART")
        .map(|p| reader.time(p))
        .transpose()?;
    let all_day = event.property("DTSTART").is_some_and(is_date);
    let end = match event.property("DTEND") {
        Some(p) => Some(reader.time(p)?),
        None => match (event.property("DURATION"), &start) {
            (Some(p), Some(start)) => Some(reader.add_duration(start, p)?),
            _ => None,
        },
    };
    let tz = match event.property("DTSTART") {
        Some(p) if is_date(p) && p.param("TZID").is_none() => calendar_zone.map(str::to_string),
        Some(p) => p
            .param("TZID")
            .map(str::to_string)
            .or(calendar_zone.map(str::to_string)),
        None => None,
    };

    Ok(Event {
        summary: text("SUMMARY"),
        description: text("DESCRIPTION"),
        location: text("LOCATION"),
        // URL is a URI, which RFC 5545 does not escape, but some feeds
        // (Carleton's) escape it like TEXT anyway; a URI never contains `\`.
        url: text("URL").map(|url| url.trim().to_string()),
        status: raw("STATUS"),
        start,
        end,
        all_day,
        tz,
        categories: event
            .all("CATEGORIES")
            .flat_map(|p| split_text_list(&p.value))
            .collect(),
        organizer: event.property("ORGANIZER").map(|p| Organizer {
            name: p.param("CN").map(str::to_string),
            email: Some(strip_mailto(&p.value)).filter(|e| !e.is_empty()),
            cutype: p.param("CUTYPE").map(str::to_ascii_uppercase),
        }),
        geo: event.property("GEO").and_then(|p| read_geo(&p.value)),
        rrule: raw("RRULE"),
        rdates: reader.times(event.all("RDATE"))?,
        exdates: reader.times(event.all("EXDATE"))?,
        recurrence_id: event
            .property("RECURRENCE-ID")
            .map(|p| reader.time(p))
            .transpose()?,
        // CREATED and LAST-MODIFIED are optional and often malformed, so a
        // bad value is skipped rather than failing the event.
        created: event
            .property("CREATED")
            .and_then(|p| reader.time(p).ok())
            .map(|z| z.timestamp()),
        last_modified: event
            .property("LAST-MODIFIED")
            .and_then(|p| reader.time(p).ok())
            .map(|z| z.timestamp()),
        uid,
        props: event.properties.clone(),
    })
}

impl Reader<'_> {
    /// Reads a DATE or DATE-TIME property value into a zoned time.
    fn time(&self, prop: &Property) -> Result<Zoned, IcsError> {
        self.time_value(prop, prop.value.trim())
    }

    /// Reads every value of every given RDATE/EXDATE-style property.
    fn times<'p>(&self, props: impl Iterator<Item = &'p Property>) -> Result<Vec<Zoned>, IcsError> {
        let mut out = Vec::new();
        for prop in props {
            for value in prop
                .value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                out.push(self.time_value(prop, value)?);
            }
        }
        Ok(out)
    }

    fn time_value(&self, prop: &Property, value: &str) -> Result<Zoned, IcsError> {
        let bad = || IcsError::BadValue {
            uid: self.uid.clone(),
            property: prop.name.clone(),
            value: value.to_string(),
        };
        if prop.param("VALUE") == Some("DATE") || is_date_text(value) {
            let date = civil::Date::strptime("%Y%m%d", value).map_err(|_| bad())?;
            // A date belongs to no zone (RFC 5545). Use the event's zone when
            // there is one; otherwise midnight UTC, which writes back as the
            // same date (spec §5.3).
            let zone = match self.zone_name(prop) {
                Some(name) => self.zone(prop, &name)?,
                None => TimeZone::UTC,
            };
            return date.to_zoned(zone).map_err(|_| bad());
        }
        let (wall, utc) = match value.strip_suffix('Z') {
            Some(wall) => (wall, true),
            None => (value, false),
        };
        let wall = civil::DateTime::strptime("%Y%m%dT%H%M%S", wall).map_err(|_| bad())?;
        let zone = if utc {
            TimeZone::UTC
        } else {
            let name = self.zone_name(prop).ok_or_else(|| IcsError::NoZone {
                uid: self.uid.clone(),
                property: prop.name.clone(),
            })?;
            self.zone(prop, &name)?
        };
        wall.to_zoned(zone).map_err(|_| bad())
    }

    fn zone_name(&self, prop: &Property) -> Option<String> {
        prop.param("TZID")
            .map(str::to_string)
            .or(self.calendar_zone.map(str::to_string))
    }

    fn zone(&self, prop: &Property, name: &str) -> Result<TimeZone, IcsError> {
        TimeZone::get(name).map_err(|_| IcsError::UnknownZone {
            uid: self.uid.clone(),
            property: prop.name.clone(),
            tz: name.to_string(),
        })
    }

    fn add_duration(&self, start: &Zoned, prop: &Property) -> Result<Zoned, IcsError> {
        let bad = || IcsError::BadValue {
            uid: self.uid.clone(),
            property: prop.name.clone(),
            value: prop.value.clone(),
        };
        let span: Span = prop.value.trim().parse().map_err(|_| bad())?;
        start.checked_add(span).map_err(|_| bad())
    }
}

fn is_date(prop: &Property) -> bool {
    prop.param("VALUE") == Some("DATE") || is_date_text(prop.value.trim())
}

fn is_date_text(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit())
}

fn strip_mailto(value: &str) -> String {
    let value = value.trim();
    match value.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("mailto:") => value[7..].to_string(),
        _ => value.to_string(),
    }
}

fn read_geo(value: &str) -> Option<(f64, f64)> {
    let (lat, lon) = value.split_once(';')?;
    Some((lat.trim().parse().ok()?, lon.trim().parse().ok()?))
}
```

`crates/ppp-core/src/ics/mod.rs`:

```rust
//! iCalendar (RFC 5545) parsing for `from ics`.

pub mod events;
pub mod lines;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test ics_events`
Expected: `test result: ok. 21 passed`

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add .gitattributes fixtures/ics crates/ppp-core
git commit -m "Read VEVENTs from iCalendar feeds

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 3: The sandboxed engine

**Files:**
- Create: `crates/ppp-core/src/engine.rs`, `crates/ppp-core/src/commands/mod.rs`, `stdlib/pipes/mod.nu`
- Modify: `Cargo.toml`, `crates/ppp-core/Cargo.toml`, `crates/ppp-core/src/lib.rs`
- Test: `crates/ppp-core/tests/engine.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (`ppp_core::engine`):
  - `Engine::new() -> Engine` (also `Default`)
  - `Engine::eval(&mut self, source: &str, input: nu_protocol::Value) -> Result<Value, EngineError>` — definitions and `use`s persist between calls
  - `Engine::has_command(&self, name: &str) -> bool`
  - `struct EngineError(pub String)` — a rendered nushell diagnostic
  - `ppp_core::commands::bind(&mut StateWorkingSet)` — empty for now; Task 4 adds `from ics`

- [ ] **Step 1: Add dependencies**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/ppp-core", "crates/ppp-cli"]

[workspace.package]
edition = "2024"

[workspace.dependencies]
chrono = "0.4.45"
clap = { version = "4", features = ["derive"] }
jiff = { version = "0.2.38", features = ["tzdb-bundle-always"] }
jsonschema = { version = "0.58.6", default-features = false }
nu-cmd-lang = "=0.116.1"
# `os` is needed: other nushell crates turn on nu-protocol's `os` feature, and
# nu-command does not compile with it on unless its own `os` is on too.
nu-command = { version = "=0.116.1", default-features = false, features = ["os", "network", "rustls-tls"] }
nu-engine = "=0.116.1"
nu-parser = "=0.116.1"
nu-protocol = { version = "=0.116.1", default-features = false }
serde_json = "1"
thiserror = "2"
```

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
nu-cmd-lang.workspace = true
nu-command.workspace = true
nu-engine.workspace = true
nu-parser.workspace = true
nu-protocol.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

- [ ] **Step 2: Write the failing test**

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
        "$in | lines | each { str upcase }",
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
    for name in ["from ics", "into event", "drop-empty"] {
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
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test engine`
Expected: compile error, `unresolved import ppp_core::engine`. (The first build of the nushell crates takes a few minutes.)

- [ ] **Step 4: Write the engine**

`crates/ppp-core/src/engine.rs`:

```rust
//! The sandboxed nushell engine that runs pipes.
//!
//! The engine registers a hand-picked command set: nushell's language
//! commands, the data commands listed in `bind_data_commands`, and our own.
//! Nothing that runs external programs or touches the filesystem is
//! registered, so a pipe that tries gets "command not found". The `pipes`
//! module is embedded and loaded as a virtual directory, so `use pipes *`
//! works without files on disk.

use nu_engine::eval_block;
use nu_protocol::debugger::WithoutDebug;
use nu_protocol::engine::{EngineState, Stack, StateWorkingSet, VirtualPath};
use nu_protocol::{PipelineData, Span, Value, format_cli_error};

use crate::commands;

/// The embedded `pipes` nushell module.
const PIPES_MODULE: &str = include_str!("../../../stdlib/pipes/mod.nu");

/// A rendered nushell diagnostic, ready to print.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EngineError(pub String);

pub struct Engine {
    state: EngineState,
    stack: Stack,
}

impl Engine {
    pub fn new() -> Engine {
        // `http get` uses rustls, which needs a crypto provider installed
        // once per process. Later calls are no-ops.
        nu_command::tls::CRYPTO_PROVIDER.default();
        let mut state = nu_cmd_lang::create_default_context();
        let delta = {
            let mut working_set = StateWorkingSet::new(&state);
            bind_data_commands(&mut working_set);
            commands::bind(&mut working_set);
            let file = working_set.add_file("pipes/mod.nu", PIPES_MODULE.as_bytes());
            let module_file =
                working_set.add_virtual_path("pipes/mod.nu".into(), VirtualPath::File(file));
            let _ =
                working_set.add_virtual_path("pipes".into(), VirtualPath::Dir(vec![module_file]));
            working_set.render()
        };
        state
            .merge_delta(delta)
            .expect("the built-in command set and pipes module always merge");
        // Some commands (`http get` among them) need a working directory.
        // Pipes have no filesystem access, so the root is a neutral choice
        // that exists on every platform.
        state.add_env_var("PWD".into(), Value::string("/", Span::unknown()));
        Engine {
            state,
            stack: Stack::new(),
        }
    }

    /// Parses and runs `source` with `input` as its pipeline input, and
    /// collects the result. Definitions and `use`s persist for later calls.
    pub fn eval(&mut self, source: &str, input: Value) -> Result<Value, EngineError> {
        let block = {
            let mut working_set = StateWorkingSet::new(&self.state);
            let block =
                nu_parser::parse(&mut working_set, Some("source"), source.as_bytes(), false);
            if let Some(err) = working_set.parse_errors.first() {
                return Err(EngineError(format_cli_error(None, &working_set, err, None)));
            }
            if let Some(err) = working_set.compile_errors.first() {
                return Err(EngineError(format_cli_error(None, &working_set, err, None)));
            }
            let delta = working_set.render();
            self.state
                .merge_delta(delta)
                .map_err(|err| self.render(&err))?;
            block
        };
        let input = PipelineData::Value(input, None);
        let result = match eval_block::<WithoutDebug>(&self.state, &mut self.stack, &block, input) {
            Ok(out) => out.body.into_value(Span::unknown()),
            Err(err) => Err(err),
        };
        match result {
            Ok(Value::Error { error, .. }) => Err(self.render(&error)),
            Ok(value) => Ok(value),
            Err(err) => Err(self.render(&err)),
        }
    }

    /// Whether a command with this name is registered.
    pub fn has_command(&self, name: &str) -> bool {
        self.state.find_decl(name.as_bytes(), &[]).is_some()
    }

    fn render(&self, err: &nu_protocol::ShellError) -> EngineError {
        let working_set = StateWorkingSet::new(&self.state);
        EngineError(format_cli_error(Some(&self.stack), &working_set, err, None))
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

/// The nushell data commands a pipe may use: no externals, no filesystem,
/// no plugins. Network access is `http get` only.
fn bind_data_commands(working_set: &mut StateWorkingSet) {
    use nu_command::*;
    macro_rules! bind {
        ( $( $command:expr ),* $(,)? ) => { $( working_set.add_decl(Box::new($command)); )* };
    }
    bind!(
        // Filters
        All,
        Any,
        Append,
        Chunks,
        Columns,
        Compact,
        Default,
        Drop,
        Each,
        Enumerate,
        Find,
        First,
        Flatten,
        Get,
        GroupBy,
        IsEmpty,
        IsNotEmpty,
        Items,
        Last,
        Length,
        Merge,
        Move,
        Prepend,
        Reduce,
        Reject,
        Rename,
        Reverse,
        Select,
        Skip,
        SkipWhile,
        Sort,
        SortBy,
        Take,
        TakeWhile,
        Transpose,
        Uniq,
        UniqBy,
        Update,
        Upsert,
        Insert,
        Values,
        Where,
        Wrap,
        Zip,
        // Strings
        Parse,
        Split,
        SplitColumn,
        SplitList,
        SplitRow,
        StrContains,
        StrDowncase,
        StrEndswith,
        StrJoin,
        StrLength,
        StrReplace,
        StrStartsWith,
        StrSubstring,
        StrTrim,
        StrUpcase,
        Lines,
        // Conversions
        IntoBool,
        IntoDatetime,
        IntoDuration,
        IntoFloat,
        IntoInt,
        IntoRecord,
        IntoString,
        // Dates
        DateNow,
        DateToTimezone,
        FormatDate,
        // Formats
        FromJson,
        FromNuon,
        ToJson,
        ToNuon,
        // Math and generators
        MathMax,
        MathMin,
        MathSum,
        Generate,
        Seq,
        // URLs and network
        HttpGet,
        UrlBuildQuery,
        UrlDecode,
        UrlEncode,
        UrlJoin,
        UrlParse,
    );
}
```

`crates/ppp-core/src/commands/mod.rs`:

```rust
//! Our own nushell commands.

use nu_protocol::engine::StateWorkingSet;

/// Registers every command in this module.
pub fn bind(_working_set: &mut StateWorkingSet) {}
```

`stdlib/pipes/mod.nu`:

```nu
# The pipes standard library: helpers that turn source data into stdlib records.
```

`crates/ppp-core/src/lib.rs`:

```rust
pub mod commands;
pub mod engine;
pub mod ics;
pub mod schema;
pub mod serialize;
pub mod time;
pub mod time_fields;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test engine`
Expected: `test result: ok. 7 passed`

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add Cargo.toml Cargo.lock crates/ppp-core stdlib/pipes
git commit -m "Add the sandboxed nushell engine

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 4: `from ics`

**Files:**
- Create: `crates/ppp-core/src/commands/from_ics.rs`
- Modify: `crates/ppp-core/src/commands/mod.rs`, `crates/ppp-core/src/time.rs`
- Test: `crates/ppp-core/tests/from_ics.rs`

**Interfaces:**
- Consumes: `parse_events`, `Event`, `Property` (Tasks 1–2); `Engine` and `commands::bind` (Task 3).
- Produces: the `from ics [--tz <zone>]` command, string → table with columns `summary start end all_day tz location description url categories organizer geo uid status rrule rdates exdates recurrence_id created last_modified props`; `organizer` is `{name, email, cutype}` or null, `geo` is `{lat, lon}` or null, `props` is a list of `{name, params, value}`. Also `ppp_core::time::to_nu(&jiff::Zoned) -> chrono::DateTime<FixedOffset>`.

- [ ] **Step 1: Write the failing test**

`crates/ppp-core/tests/from_ics.rs`:

```rust
//! `from ics` inside the engine: columns, values and the --tz flag.

use std::fs;
use std::path::Path;

use nu_protocol::Value;
use ppp_core::engine::Engine;

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/ics")
            .join(name),
    )
    .unwrap()
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
fn from_ics_returns_one_row_per_event() {
    let mut engine = Engine::new();
    let out = nuon(
        &mut engine,
        "$in | from ics | select summary start all_day tz categories | first 2",
        Value::test_string(fixture("campus-events.ics")),
    );
    assert_eq!(
        out,
        "[[summary, start, all_day, tz, categories]; [\"Fall Orchestra Concert\", 2026-10-14T19:30:00-05:00, false, America/Chicago, [Music, Concert]], [\"Family Weekend\", 2026-10-17T00:00:00-05:00, true, America/Chicago, [Community]]]"
    );
}

#[test]
fn from_ics_columns() {
    let mut engine = Engine::new();
    let out = nuon(
        &mut engine,
        "$in | from ics | columns",
        Value::test_string(fixture("minimal.ics")),
    );
    assert_eq!(
        out,
        "[summary, start, end, all_day, tz, location, description, url, categories, organizer, geo, uid, status, rrule, rdates, exdates, recurrence_id, created, last_modified, props]"
    );
}

#[test]
fn from_ics_tz_flag() {
    let mut engine = Engine::new();
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART:20261013T120000\nEND:VEVENT\nEND:VCALENDAR\n";
    let out = nuon(
        &mut engine,
        "$in | from ics --tz Europe/London | get start.0",
        Value::test_string(ics),
    );
    assert_eq!(out, "2026-10-13T12:00:00+01:00");
    let err = engine
        .eval("$in | from ics", Value::test_string(ics))
        .unwrap_err();
    assert!(err.0.contains("cannot read iCalendar text"), "{err}");
    assert!(err.0.contains("pass --tz"), "{err}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test from_ics`
Expected: all 3 FAIL with an engine error — `from ics` isn't registered yet.

- [ ] **Step 3: Write the command**

`crates/ppp-core/src/commands/from_ics.rs`:

```rust
//! `from ics`: iCalendar text to a table of events (spec §5.2).

use nu_engine::command_prelude::*;
use nu_protocol::shell_error::generic::GenericError;

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
            .input_output_types(vec![(Type::String, Type::table())])
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
        let (text, span, _) = input.collect_string_strict(head)?;
        let events = parse_events(&text, tz.as_deref()).map_err(|err| {
            ShellError::Generic(GenericError::new(
                "cannot read iCalendar text",
                err.to_string(),
                span,
            ))
        })?;
        let rows = events
            .iter()
            .map(|event| event_to_value(event, head))
            .collect();
        Ok(Value::list(rows, head).into_pipeline_data())
    }
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

`crates/ppp-core/src/commands/mod.rs`:

```rust
//! Our own nushell commands.

use nu_protocol::engine::StateWorkingSet;

mod from_ics;

pub use from_ics::FromIcs;

/// Registers every command in this module.
pub fn bind(working_set: &mut StateWorkingSet) {
    working_set.add_decl(Box::new(FromIcs));
}
```

Append to `crates/ppp-core/src/time.rs`:

```rust
/// Converts a jiff zoned time to a nushell datetime with the same instant
/// and the same UTC offset.
pub fn to_nu(zoned: &jiff::Zoned) -> DateTime<FixedOffset> {
    let ts = zoned.timestamp();
    let offset = FixedOffset::east_opt(zoned.offset().seconds())
        .expect("jiff offsets are under a day, inside chrono's range");
    DateTime::from_timestamp(ts.as_second(), ts.subsec_nanosecond() as u32)
        .expect("jiff timestamps are inside chrono's range")
        .with_timezone(&offset)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ppp-core`
Expected: every suite passes; `from_ics` 3 passed.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add crates/ppp-core
git commit -m "Add the from ics command

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 5: `into event`

**Files:**
- Modify: `stdlib/pipes/mod.nu`
- Test: `crates/ppp-core/tests/into_event.rs`

**Interfaces:**
- Consumes: `Engine` (Task 3), `from ics` (Task 4), `Registry`, `record_to_json` (phase 1).
- Produces, in the `pipes` module: `into event [--source <url>] [--tz <zone>]` (table → table of `event` records) and `drop-empty` (record → record without `null`, `""`, `[]`, `{}`).

- [ ] **Step 1: Write the failing test**

`crates/ppp-core/tests/into_event.rs`:

```rust
//! `into event` turns `from ics` rows into valid `event` records.

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
            .join("../../fixtures/ics")
            .join(name),
    )
    .unwrap()
}

/// Runs `from ics | into event` on `ics`, then serializes each record and
/// checks it against the schema.
fn events(ics: &str) -> Vec<Json> {
    let registry = Registry::stdlib().unwrap();
    let time_fields = &registry.record_type("event").unwrap().time_fields;
    let mut engine = Engine::new();
    let rows = engine
        .eval(
            "use pipes *; $in | from ics | into event --source https://calendar.example.edu/feed.ics",
            Value::test_string(ics),
        )
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

#[test]
fn every_fixture_becomes_valid_events() {
    for (name, count) in [
        ("minimal.ics", 1),
        ("campus-events.ics", 4),
        ("team-planning.ics", 3),
        ("stolaf.ics", 6),
        ("carleton.ics", 7),
        ("northfield.ics", 2),
    ] {
        assert_eq!(events(&fixture(name)).len(), count, "{name}");
    }
}

#[test]
fn a_timed_event() {
    let orchestra = events(&fixture("campus-events.ics")).remove(0);
    assert_eq!(
        orchestra,
        json!({
            "type": "event",
            "id": "20261014-orchestra@calendar.example.edu",
            "source": "https://calendar.example.edu/feed.ics",
            "summary": "Fall Orchestra Concert",
            "start": "2026-10-14T19:30:00",
            "end": "2026-10-14T21:00:00",
            "all_day": false,
            "tz": "America/Chicago",
            "status": "confirmed",
            "location": {
                "name": "Boe Memorial Chapel, 1520 St. Olaf Avenue, Northfield, MN 55057",
                "geo": { "lat": 44.4614, "lon": -93.1822 }
            },
            "description": "The orchestra opens its season with Dvořák's Symphony No. 8.\nFree and open to the public.",
            "url": "https://www.example.edu/events/fall-orchestra",
            "categories": ["Music", "Concert"],
            "organizers": [{ "org": { "name": "Music Department", "emails": [{ "address": "music@example.edu" }] } }],
            "created": "2026-08-06T18:11:07Z",
            "modified": "2026-09-02T15:53:15Z",
            "extra": { "dtstamp": "20261011T000000Z" }
        })
    );
}

#[test]
fn an_all_day_event() {
    let weekend = events(&fixture("campus-events.ics")).remove(1);
    assert_eq!(
        (weekend["start"].as_str(), weekend["end"].as_str()),
        (Some("2026-10-17"), Some("2026-10-19"))
    );
    assert_eq!(weekend["all_day"], true);
}

#[test]
fn all_day_dates_without_any_zone_keep_their_date() {
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nSUMMARY:Holiday\nDTSTART;VALUE=DATE:20261017\nEND:VEVENT\nEND:VCALENDAR\n";
    let holiday = events(ics).remove(0);
    assert_eq!(holiday["start"], "2026-10-17");
    assert_eq!(holiday.get("tz"), None);
}

#[test]
fn recurrence_overrides_get_an_occurrence_id() {
    let team = events(&fixture("team-planning.ics"));
    assert_eq!(team[1]["rrule"], "FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR");
    assert_eq!(
        team[1]["exdates"],
        json!(["2026-11-26T09:30:00", "2026-11-27T09:30:00"])
    );
    assert_eq!(
        team[2]["id"],
        "standup-2026@product.example.com_20261014T133000Z"
    );
    assert_eq!(team[2]["series"], "standup-2026@product.example.com");
    assert_eq!(team[2]["recurrence_id"], "2026-10-14T09:30:00");
}

#[test]
fn organizers_are_people_only_when_cutype_says_so() {
    let team = events(&fixture("team-planning.ics"));
    assert_eq!(
        team[0]["organizers"],
        json!([{ "person": { "name": { "display": "Priya Shah" }, "emails": [{ "address": "priya@example.com" }] } }])
    );
}

#[test]
fn repeated_unmapped_properties_are_kept_as_lists() {
    let team = events(&fixture("team-planning.ics"));
    assert_eq!(
        team[0]["extra"]["attendee"],
        json!(["mailto:priya@example.com", "mailto:sam@example.com"])
    );
}

#[test]
fn relative_urls_move_to_extra() {
    let northfield = events(&fixture("northfield.ics")).remove(0);
    assert_eq!(northfield.get("url"), None);
    assert_eq!(
        northfield["extra"]["url"],
        "/common/modules/iCalendar/iCalendar.aspx?feed=calendar&catID=41"
    );
}

#[test]
fn a_row_without_a_uid_is_an_error() {
    let mut engine = Engine::new();
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Mystery\nDTSTART:20261013T120000Z\nEND:VEVENT\nEND:VCALENDAR\n";
    let err = engine
        .eval(
            "use pipes *; $in | from ics | into event",
            Value::test_string(ics),
        )
        .unwrap_err();
    assert!(err.0.contains("event has no uid: Mystery"), "{err}");
}

#[test]
fn drop_empty_removes_only_empty_values() {
    let mut engine = Engine::new();
    let out = engine
        .eval(
            "use pipes *; {a: null, b: '', c: [], d: {}, e: 0, f: false, g: 'x', h: [1]} | drop-empty | to nuon",
            Value::test_nothing(),
        )
        .unwrap();
    assert_eq!(
        out.coerce_into_string().unwrap(),
        "{e: 0, f: false, g: x, h: [1]}"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test into_event`
Expected: all 10 FAIL — the `pipes` module doesn't export `into event` or `drop-empty` yet.

- [ ] **Step 3: Write the module**

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
    items {|key, value|
        if $value in [null "" [] {}] { null } else { {key: $key, value: $value} }
    }
    | compact
    | reduce --fold {} {|field, acc| $acc | insert $field.key $field.value }
}

# Turn rows from `from ics` into stdlib `event` records.
#
# An ORGANIZER becomes a person when its CUTYPE is INDIVIDUAL and an
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
    let status = $row.status? | default "" | str downcase
    let has_location = $row.location? | is-not-empty
    let has_web_url = ($row.url? | default "") =~ '^https?://'
    let leftovers = $row.props? | default [] | where name not-in $MAPPED_PROPS | group-by name
        | items {|name, props|
            let values = $props | get value
            {key: ($name | str downcase), value: (if ($values | length) == 1 { $values.0 } else { $values })}
        }
        | reduce --fold {} {|field, acc| $acc | insert $field.key $field.value }
    let extra = $leftovers
        | merge (if $has_web_url or $row.url? == null { {} } else { {url: $row.url} })
        | merge (if $has_location or $row.geo? == null { {} } else { {geo: $row.geo} })

    {type: event}
    | merge $identity
    | merge {
        source: $source
        summary: $row.summary?
        start: $row.start?
        end: $row.end?
        all_day: ($row.all_day? | default false)
        tz: ($row.tz? | default $tz)
        status: (if $status in [tentative confirmed cancelled] { $status })
        location: (if $has_location { {name: $row.location} | merge (if $row.geo? != null { {geo: $row.geo} } else { {} }) })
        description: $row.description?
        url: (if $has_web_url { $row.url })
        categories: ($row.categories? | default [] | uniq)
        organizers: ($row.organizer? | organizers)
        rrule: $row.rrule?
        rdates: ($row.rdates? | default [])
        exdates: ($row.exdates? | default [])
        recurrence_id: $row.recurrence_id?
        created: $row.created?
        modified: $row.last_modified?
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test into_event`
Expected: `test result: ok. 10 passed`

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add stdlib/pipes crates/ppp-core/tests/into_event.rs
git commit -m "Add into event to the pipes module

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 6: Loading and running pipes

**Files:**
- Create: `crates/ppp-core/src/pipe.rs`, `fixtures/pipes/{two-events,no-const,positional,wrong-type,invalid-event,one-record,fails}.nu`
- Modify: `crates/ppp-core/src/lib.rs`
- Test: `crates/ppp-core/tests/pipe.rs`

**Interfaces:**
- Consumes: `Engine`, `EngineError` (Task 3); `into event` (Task 5); `Registry`, `RecordError`, `record_to_json` (phase 1).
- Produces (`ppp_core::pipe`):
  - `Pipe::load(&Path) -> Result<Pipe, PipeError>`, `Pipe::info(&self) -> &PipeInfo`, `Pipe::run(&mut self, args: &nu_protocol::Record) -> Result<Vec<Value>, PipeError>`
  - `struct PipeInfo { pub name, pub title, pub description: String, pub params: Vec<Param>, pub output: Vec<String> }`
  - `struct Param { pub name: String, pub shape: String /* "switch" for switches */, pub description: String, pub default: Option<Value> }`
  - `enum PipeError { Engine(EngineError), Metadata { path, message }, UnknownFlag { pipe, flag } }`
  - `fn check_records(&Registry, &PipeInfo, &[Value]) -> Result<Vec<serde_json::Value>, Vec<RecordError>>`

- [ ] **Step 1: Write the test pipes**


`fixtures/pipes/fails.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that fails while running.
export def main []: nothing -> table {
    error make {msg: "the source is down"}
}
```

`fixtures/pipes/invalid-event.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that returns an event with no start.
export def main []: nothing -> table { [{type: event, id: "x", summary: "Lost"}] }
```

`fixtures/pipes/no-const.nu`:

```nu
# A pipe missing `export const pipe`.
export def main []: nothing -> table { [] }
```

`fixtures/pipes/one-record.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that returns a single record instead of a list.
export def main []: nothing -> record {
    {type: event, id: "solo", summary: "Solo", start: 2026-10-22T16:00:00Z}
}
```

`fixtures/pipes/positional.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe with a positional parameter.
export def main [days: int]: nothing -> table { [] }
```

`fixtures/pipes/two-events.nu`:

```nu
# A test pipe that needs no network: it reads two events from inline text.

export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
}

# Two events
#
# Both from an inline calendar.
export def main [
    --count: int = 2          # How many events to return
    --prefix: string = ""     # Text put before each summary
    --loud                    # Shout the summaries
]: nothing -> table {
    use pipes *
    "BEGIN:VCALENDAR
X-WR-TIMEZONE:America/Chicago
BEGIN:VEVENT
UID:a@example.edu
SUMMARY:Chess Club
DTSTART:20261013T180000
END:VEVENT
BEGIN:VEVENT
UID:b@example.edu
SUMMARY:Choir
DTSTART:20261014T190000
END:VEVENT
END:VCALENDAR"
    | from ics
    | first $count
    | update summary {|row| $"($prefix)($row.summary)" | if $loud { str upcase } else { $in } }
    | into event
}
```

`fixtures/pipes/wrong-type.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that returns a record type it does not declare.
export def main []: nothing -> table { [{type: entry, id: "x"}] }
```

- [ ] **Step 2: Write the failing test**

`crates/ppp-core/tests/pipe.rs`:

```rust
//! Loading pipe files, running them, and checking their records.

use std::path::{Path, PathBuf};

use nu_protocol::{Record, Value, record};
use ppp_core::pipe::{Param, Pipe, PipeError, check_records};
use ppp_core::schema::Registry;

fn pipe_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pipes")
        .join(name)
}

fn load(name: &str) -> Pipe {
    Pipe::load(&pipe_path(name)).unwrap()
}

fn load_error(name: &str) -> String {
    match Pipe::load(&pipe_path(name)) {
        Err(err) => err.to_string(),
        Ok(_) => panic!("{name} loaded"),
    }
}

fn summaries(records: &[Value]) -> Vec<String> {
    records
        .iter()
        .map(|r| {
            r.get_data_by_key("summary")
                .unwrap()
                .coerce_into_string()
                .unwrap()
        })
        .collect()
}

#[test]
fn reads_metadata_without_running_main() {
    let pipe = load("two-events.nu");
    let info = pipe.info();
    assert_eq!(info.name, "two-events");
    assert_eq!(info.title, "Two events");
    assert_eq!(info.description, "Both from an inline calendar.");
    assert_eq!(
        info.output,
        ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    );
    assert_eq!(
        info.params,
        [
            Param {
                name: "count".into(),
                shape: "int".into(),
                description: "How many events to return".into(),
                default: Some(Value::test_int(2))
            },
            Param {
                name: "prefix".into(),
                shape: "string".into(),
                description: "Text put before each summary".into(),
                default: Some(Value::test_string(""))
            },
            Param {
                name: "loud".into(),
                shape: "switch".into(),
                description: "Shout the summaries".into(),
                default: None
            },
        ]
    );
}

#[test]
fn runs_with_defaults_and_with_flags() {
    let mut pipe = load("two-events.nu");
    assert_eq!(
        summaries(&pipe.run(&Record::new()).unwrap()),
        ["Chess Club", "Choir"]
    );
    let args = record! {
        "count" => Value::test_int(1),
        "prefix" => Value::test_string("Club: "),
        "loud" => Value::test_bool(true),
    };
    assert_eq!(summaries(&pipe.run(&args).unwrap()), ["CLUB: CHESS CLUB"]);
}

#[test]
fn a_false_switch_is_left_off() {
    let mut pipe = load("two-events.nu");
    let records = pipe
        .run(&record! { "loud" => Value::test_bool(false) })
        .unwrap();
    assert_eq!(summaries(&records), ["Chess Club", "Choir"]);
}

#[test]
fn unknown_flags_are_an_error() {
    let mut pipe = load("two-events.nu");
    let err = pipe
        .run(&record! { "days" => Value::test_int(3) })
        .unwrap_err();
    assert!(matches!(err, PipeError::UnknownFlag { ref flag, .. } if flag == "days"));
    assert_eq!(err.to_string(), "two-events has no flag --days");
}

#[test]
fn checked_records_are_json() {
    let registry = Registry::stdlib().unwrap();
    let mut pipe = load("two-events.nu");
    let records = pipe.run(&Record::new()).unwrap();
    let json = check_records(&registry, pipe.info(), &records).unwrap();
    assert_eq!(json[0]["start"], "2026-10-13T18:00:00");
    assert_eq!(json[0]["tz"], "America/Chicago");
}

#[test]
fn undeclared_record_types_fail_the_check() {
    let registry = Registry::stdlib().unwrap();
    let mut pipe = load("wrong-type.nu");
    let records = pipe.run(&Record::new()).unwrap();
    let errors = check_records(&registry, pipe.info(), &records).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].pointer, "/type");
    assert_eq!(
        errors[0].message,
        "type \"entry\" is not one of the pipe's output types: https://pipes.frogpond.tech/stdlib/calendar/1#event"
    );
}

#[test]
fn invalid_records_fail_the_check() {
    let registry = Registry::stdlib().unwrap();
    let mut pipe = load("invalid-event.nu");
    let records = pipe.run(&Record::new()).unwrap();
    let errors = check_records(&registry, pipe.info(), &records).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("\"start\" is a required property")),
        "{errors:#?}"
    );
}

#[test]
fn bad_pipe_files() {
    assert!(load_error("no-const.nu").ends_with("the file has no `export const pipe = {...}`"));
    assert!(
        load_error("positional.nu")
            .ends_with("`main` has a positional parameter `days`; pipe parameters must be flags")
    );
    assert!(load_error("missing.nu").contains("No such file"));
}

#[test]
fn a_single_record_is_one_record() {
    let mut pipe = load("one-record.nu");
    assert_eq!(summaries(&pipe.run(&Record::new()).unwrap()), ["Solo"]);
}

#[test]
fn paths_with_spaces_and_quotes_load() {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join("a dir with 'single' and \"double\" quotes");
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join("two-events.nu");
    std::fs::copy(pipe_path("two-events.nu"), &copy).unwrap();
    let mut pipe = Pipe::load(&copy).unwrap();
    assert_eq!(pipe.run(&Record::new()).unwrap().len(), 2);
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test pipe`
Expected: compile error, `unresolved import ppp_core::pipe`.

- [ ] **Step 4: Write the loader**

`crates/ppp-core/src/pipe.rs`:

```rust
//! Loading a pipe file, reading its metadata, running it, and checking its
//! output at the boundary (spec §5.1).

use std::path::Path;

use nu_protocol::{Record, Span, Value};
use serde_json::Value as Json;

use crate::engine::{Engine, EngineError};
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
    /// Loads the pipe at `path` into a fresh engine and reads its metadata.
    /// Nothing in the pipe runs except its constant definitions.
    pub fn load(path: &Path) -> Result<Pipe, PipeError> {
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
        let mut engine = Engine::new();
        engine.eval(
            &format!("use {}", nu_string(&full.to_string_lossy())),
            Value::test_nothing(),
        )?;

        let found = engine.eval(
            &format!("scope commands | where name == {} | select description extra_description signatures", nu_string(&name)),
            Value::test_nothing(),
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
                Value::test_nothing(),
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

        let info = PipeInfo {
            name,
            title: field("description"),
            description: field("extra_description"),
            params,
            output,
        };
        Ok(Pipe { engine, info })
    }

    pub fn info(&self) -> &PipeInfo {
        &self.info
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
        let output = self
            .engine
            .eval(&call, Value::record(args.clone(), Span::unknown()))?;
        Ok(match output {
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
    let mut out = Vec::with_capacity(records.len());
    let mut errors = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let type_name = record
            .get_data_by_key("type")
            .and_then(|v| v.coerce_string().ok());
        let declared = type_name
            .as_deref()
            .and_then(|name| registry.record_type(name))
            .filter(|record_type| info.output.contains(&record_type.uri));
        let Some(record_type) = declared else {
            let message = match type_name {
                Some(name) => format!(
                    "type \"{name}\" is not one of the pipe's output types: {}",
                    info.output.join(", ")
                ),
                None => "record has no \"type\"".to_string(),
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

/// A nushell double-quoted string literal for `text`.
fn nu_string(text: &str) -> String {
    serde_json::to_string(text).expect("strings always serialize")
}
```

`crates/ppp-core/src/lib.rs`:

```rust
pub mod commands;
pub mod engine;
pub mod ics;
pub mod pipe;
pub mod schema;
pub mod serialize;
pub mod time;
pub mod time_fields;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test pipe`
Expected: `test result: ok. 10 passed`

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add fixtures/pipes crates/ppp-core
git commit -m "Load, run and check pipes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

### Task 7: `ppp run` and `campus-week`

**Files:**
- Create: `crates/ppp-cli/src/report.rs`, `crates/ppp-cli/src/run.rs`
- Modify: `crates/ppp-cli/Cargo.toml`, `crates/ppp-cli/src/main.rs`, `crates/ppp-cli/src/validate.rs` (its error printing moves to `report.rs`), `pipes/campus-week.nu`
- Test: `crates/ppp-cli/tests/run.rs`

**Interfaces:**
- Consumes: `Pipe`, `Param`, `check_records` (Task 6); `Registry` (phase 1).
- Produces: `ppp run [--pipes-dir DIR] <pipe> [pipe flags…]`. Pretty JSON array on stdout and exit 0; boundary failures on stderr (same format as `ppp validate`) and exit 1; load, flag and runtime errors on stderr and exit 2.

- [ ] **Step 1: Write the failing test**

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
fn flag_errors_exit_2_and_list_the_flags() {
    let unknown = ppp(&["run", "fixtures/pipes/two-events.nu", "--days", "3"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert_eq!(
        text(&unknown.stderr),
        "ppp: two-events has no flag --days; two-events takes: --count <int>, --prefix <string>, --loud\n"
    );

    let bad = ppp(&["run", "fixtures/pipes/two-events.nu", "--count", "two"]);
    assert_eq!(bad.status.code(), Some(2));
    assert_eq!(text(&bad.stderr), "ppp: --count expects int, got \"two\"\n");

    let missing = ppp(&["run", "fixtures/pipes/two-events.nu", "--count"]);
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(text(&missing.stderr), "ppp: --count needs a value\n");

    let positional = ppp(&["run", "fixtures/pipes/two-events.nu", "3"]);
    assert_eq!(positional.status.code(), Some(2));
    assert!(text(&positional.stderr).starts_with("ppp: unexpected argument \"3\""));
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
    let out = ppp(&["run", "campus-week", "--days", "7"]);
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
    assert!(
        text(&out.stderr).contains("the source is down"),
        "{}",
        text(&out.stderr)
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-cli --test run`
Expected: 7 FAIL, 1 ignored — clap rejects `run` as an unknown subcommand.

- [ ] **Step 3: Write the command**

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
nu-protocol.workspace = true
ppp-core = { path = "../ppp-core" }
serde_json.workspace = true
```

`crates/ppp-cli/src/report.rs`:

```rust
//! Printing validation failures, shared by `ppp validate` and `ppp run`.

use ppp_core::schema::RecordError;

/// How many failures to print before summarizing the rest.
const SHOWN: usize = 20;

/// How records are numbered in messages.
pub enum Numbering {
    /// 0-based position in a list.
    Record,
    /// The 1-based line each record came from in a JSONL file.
    Lines(Vec<usize>),
}

/// Prints up to `SHOWN` failures to stderr, each prefixed with `label`,
/// then a count line.
pub fn print_errors(label: &str, errors: &[RecordError], numbering: &Numbering, total: usize) {
    for e in errors.iter().take(SHOWN) {
        eprintln!("{label}: {}", describe(e, numbering));
    }
    if errors.len() > SHOWN {
        eprintln!("{label}: …and {} more", errors.len() - SHOWN);
    }
    eprintln!(
        "{label}: {} in {}",
        count(errors.len(), "error"),
        count(total, "record")
    );
}

fn describe(e: &RecordError, numbering: &Numbering) -> String {
    let place = match numbering {
        Numbering::Record => format!("record {}", e.index),
        Numbering::Lines(lines) => format!("line {}", lines[e.index]),
    };
    let pointer = if e.pointer.is_empty() {
        String::new()
    } else {
        format!(" {}", e.pointer)
    };
    let value = if e.excerpt.is_empty() {
        String::new()
    } else {
        format!("\n    value: {}", e.excerpt)
    };
    format!("{place}{pointer}: {}{value}", e.message)
}

pub fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}
```

`crates/ppp-cli/src/run.rs`:

```rust
//! `ppp run`: runs a pipe and prints its records as a JSON array.
//!
//! Exit codes: 0 when the pipe ran and every record is valid, 1 when any
//! record fails the boundary check, 2 when the pipe cannot be loaded or
//! run, or its flags are wrong.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nu_protocol::{Record, Span, Value};
use ppp_core::pipe::{Param, Pipe, check_records};
use ppp_core::schema::Registry;

use crate::report::{Numbering, print_errors};

pub fn run(pipe: &str, pipes_dir: &Path, args: &[String]) -> ExitCode {
    match try_run(pipe, pipes_dir, args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn try_run(pipe: &str, pipes_dir: &Path, args: &[String]) -> Result<ExitCode, String> {
    let path = pipe_path(pipe, pipes_dir);
    let mut loaded = Pipe::load(&path).map_err(|e| format!("ppp: {e}"))?;
    let flags = parse_flags(loaded.info().name.as_str(), &loaded.info().params, args)?;
    let records = loaded.run(&flags).map_err(|e| format!("ppp: {e}"))?;
    let registry = Registry::stdlib().map_err(|e| format!("ppp: stdlib schemas: {e}"))?;
    match check_records(&registry, loaded.info(), &records) {
        Ok(json) => {
            let text = serde_json::to_string_pretty(&json).expect("JSON values always serialize");
            println!("{text}");
            Ok(ExitCode::SUCCESS)
        }
        Err(errors) => {
            print_errors(
                &loaded.info().name,
                &errors,
                &Numbering::Record,
                records.len(),
            );
            Ok(ExitCode::from(1))
        }
    }
}

/// A pipe name is looked up as `<pipes_dir>/<name>.nu`; anything with a `/`
/// or ending in `.nu` is a path.
fn pipe_path(pipe: &str, pipes_dir: &Path) -> PathBuf {
    if pipe.contains('/') || pipe.ends_with(".nu") {
        PathBuf::from(pipe)
    } else {
        pipes_dir.join(format!("{pipe}.nu"))
    }
}

/// Reads `--flag value`, `--flag=value` and bare `--switch` arguments into
/// typed values, using the pipe's declared flags.
fn parse_flags(pipe: &str, params: &[Param], args: &[String]) -> Result<Record, String> {
    let usage = || {
        let flags: Vec<String> = params
            .iter()
            .map(|p| {
                if p.shape == "switch" {
                    format!("--{}", p.name)
                } else {
                    format!("--{} <{}>", p.name, p.shape)
                }
            })
            .collect();
        if flags.is_empty() {
            format!("{pipe} takes no flags")
        } else {
            format!("{pipe} takes: {}", flags.join(", "))
        }
    };
    let mut record = Record::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let Some(flag) = arg.strip_prefix("--") else {
            return Err(format!("ppp: unexpected argument \"{arg}\"; {}", usage()));
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (flag, None),
        };
        let param = params
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("ppp: {pipe} has no flag --{name}; {}", usage()))?;
        if param.shape == "switch" {
            record.push(name, Value::bool(true, Span::unknown()));
            continue;
        }
        let text = match inline {
            Some(text) => text,
            None => rest
                .next()
                .cloned()
                .ok_or_else(|| format!("ppp: --{name} needs a value"))?,
        };
        record.push(name, typed_value(param, &text)?);
    }
    Ok(record)
}

fn typed_value(param: &Param, text: &str) -> Result<Value, String> {
    let span = Span::unknown();
    let bad = || {
        format!(
            "ppp: --{} expects {}, got \"{text}\"",
            param.name, param.shape
        )
    };
    Ok(match param.shape.as_str() {
        "string" => Value::string(text, span),
        "int" => Value::int(text.parse().map_err(|_| bad())?, span),
        "float" => Value::float(text.parse().map_err(|_| bad())?, span),
        "number" => match text.parse::<i64>() {
            Ok(n) => Value::int(n, span),
            Err(_) => Value::float(text.parse().map_err(|_| bad())?, span),
        },
        "bool" => Value::bool(text.parse().map_err(|_| bad())?, span),
        other => {
            return Err(format!(
                "ppp: --{} has type {other}, which can't be given on the command line yet",
                param.name
            ));
        }
    })
}
```

Replace `crates/ppp-cli/src/main.rs` with:


```rust
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod report;
mod run;
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
    /// Run a pipe and print its records as a JSON array
    Run {
        /// Directory that pipe names are looked up in
        #[arg(long, default_value = "pipes")]
        pipes_dir: PathBuf,
        /// A pipe name (looked up as <pipes-dir>/<name>.nu) or a path to a .nu file
        pipe: String,
        /// The pipe's own flags, e.g. --days 3
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Check a JSON array or JSONL file of records against the stdlib schemas
    Validate {
        /// A `.json` file holding an array of records, or a `.jsonl` file with one record per line
        file: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Run {
            pipes_dir,
            pipe,
            args,
        } => run::run(&pipe, &pipes_dir, &args),
        Command::Validate { file } => validate::run(&file),
    }
}
```

Replace `crates/ppp-cli/src/validate.rs` with (the printing now lives in `report.rs`; behavior is unchanged, so its tests stay green):


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

use crate::report::{Numbering, count, print_errors};

pub fn run(file: &Path) -> ExitCode {
    let (records, numbering) = match read_records(file) {
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
    print_errors(
        &file.display().to_string(),
        &errors,
        &numbering,
        records.len(),
    );
    ExitCode::from(1)
}

fn read_records(file: &Path) -> Result<(Vec<Json>, Numbering), String> {
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    if file.extension().is_some_and(|ext| ext == "jsonl") {
        let mut records = Vec::new();
        let mut lines = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            records.push(serde_json::from_str(line).map_err(|e| format!("line {}: {e}", i + 1))?);
            lines.push(i + 1);
        }
        return Ok((records, Numbering::Lines(lines)));
    }
    match serde_json::from_str(&text).map_err(|e| e.to_string())? {
        Json::Array(records) => Ok((records, Numbering::Record)),
        _ => Err("expected a JSON array of records".to_string()),
    }
}
```

Replace `pipes/campus-week.nu` with:


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
@example "Next three days" { campus-week --days 3 }
export def main [
    --days: int = 7  # How far ahead to look
]: nothing -> table {
    use pipes *
    let now = date now
    http get --raw $SOURCE
    | from ics
    | where start >= $now and start < ($now + ($days * 1day))
    | into event --source $SOURCE
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: every suite passes — ppp-cli `run` 7 (1 ignored) and `validate` 6; ppp-core `engine` 7, `fixtures` 5, `from_ics` 3, `ics_events` 21, `ics_lines` 9, `into_event` 10, `pipe` 10, `serialize` 17, `time_fields` 6. 101 in all.

Then the live test, which fetches the St. Olaf calendar:

Run: `cargo test -p ppp-cli --test run -- --ignored`
Expected: `test result: ok. 1 passed`

And by hand: `cargo run -q -p ppp-cli -- run campus-week --days 3` prints a JSON array of events.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add crates/ppp-cli pipes/campus-week.nu Cargo.lock
git commit -m "Add ppp run and the campus-week pipe

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```



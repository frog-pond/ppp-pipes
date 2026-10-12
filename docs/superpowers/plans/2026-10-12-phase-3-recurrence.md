# ppp-pipes Phase 3: Recurrence — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `expand rrule --from <dt> --until <dt>` turns recurring `from ics` rows into one row per occurrence, so `campus-week` (and every calendar pipe after it) handles weekly clubs, daily stand-ups and yearly holidays.

**Architecture:** `recur.rs` wraps the `rrule` crate behind a jiff API (`Series`, `occurrences`); it is the only module that touches `rrule`/`chrono-tz`. `commands/expand_rrule.rs` is the nushell command; its work is a plain function, `expand(rows, from, until)`, so phase 6's plugin can reuse it. Each occurrence row gets `recurrence_id` = its original start, which makes `into event` give it the Google-style id `<uid>_<UTC instant>` with no changes to `into event`.

**Tech Stack:** `rrule` 0.14 (chrono + chrono-tz 0.10), jiff 0.2.38, nushell 0.116.1.

**Spec:** [`docs/superpowers/specs/2026-10-11-ppp-pipes-cli-prototype-design.md`](../specs/2026-10-11-ppp-pipes-cli-prototype-design.md) §5.4 — build-order step 3. §5.4 was rewritten in the same commit as this plan to record the decisions below.

Every code block was compiled and tested in a scratch copy of `main` before this plan was written; after each task `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` were clean, and the final state passes 136 tests plus the live `campus-week` test.

## Global Constraints

- Everything in the phase 1 and 2 Global Constraints still holds.
- `rrule` and `chrono-tz` are used only in `crates/ppp-core/src/recur.rs`.
- The occurrence cap is 10,000 per series inside the window (`recur::MAX_OCCURRENCES`).

## Decisions made while planning

Wren was away and asked for the phases to continue; these are mine to review.

1. **The window is an overlap, like `during`,** not "starts in the window" as the spec first said. Otherwise a club meeting that started 30 minutes ago vanishes from "what's on now", the same bug the phase 2 review found in `campus-week`.
2. **Override rows pass through untouched;** `expand rrule` only drops the generated occurrence an override replaces. Moving an occurrence into or out of the window is then `during`'s job, and `expand rrule` keeps its one rule: rows without a rule pass through.
3. **Occurrences carry `recurrence_id`** (their original start). `into event` already turns that into `<uid>_<instant>` + `series`, so expanded, unexpanded and Google-pre-expanded feeds all produce the same ids.
4. **DTSTART always counts as an occurrence** (RFC 5545 §3.8.5.3), even when the rule wouldn't produce it. The `rrule` crate skips it, so `recur.rs` adds it as an RDATE.
5. **Rules are strict.** A floating `UNTIL` with a zoned `DTSTART` is an error (RFC 5545 requires UTC), in keeping with Wren's preference for strict time parsing. If real feeds turn out to be sloppy here, a lenient mode is a small change.
6. **The cap counts only occurrences in the window,** checked against the crate: a daily series running since 1990 asked for one week returns 7.
7. **Zone-less recurring all-day events expand in UTC**, which keeps their dates (spec §5.3).

## Review Focus

1. **An occurrence under way when the window opens** must be kept; one that ends exactly as it opens must not — Task 1 (`an_occurrence_under_way_when_the_window_opens_is_kept`).
2. **A spring-forward gap** (02:30 on 2026-03-08 in Chicago) must move to 03:30, not vanish or double — Task 1.
3. **A long-running series** (daily since 1990) must not trip the 10,000 cap — Task 1.
4. **A zone-less yearly all-day event** must keep its date every year, not drift to the day before — Task 2.
5. **An override whose original occurrence is in the window** must replace it exactly once (no duplicate standup) — Task 2.

---

### Task 1: Recurrence wrapper (`recur.rs`)

**Files:** Modify `Cargo.toml`, `crates/ppp-core/Cargo.toml`, `crates/ppp-core/src/lib.rs`; Create `crates/ppp-core/src/recur.rs`; Test `crates/ppp-core/tests/recur.rs`.

**Interfaces:** Produces `ppp_core::recur::{Series<'a> { start: &Zoned, rrule: Option<&str>, rdates: &[Zoned], exdates: &[Zoned] }, occurrences(&Series, length: SignedDuration, from: Timestamp, until: Timestamp) -> Result<Vec<Zoned>, RecurError>, RecurError { Rule(String), Zone(String), TooMany }, MAX_OCCURRENCES: u16}`.

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
jiff = { version = "0.2.38", features = ["tzdb-bundle-always"] }
jsonschema = { version = "0.58.6", default-features = false }
nu-cmd-lang = "=0.116.1"
# `os` is needed: other nushell crates turn on nu-protocol's `os` feature, and
# nu-command does not compile with it on unless its own `os` is on too.
nu-command = { version = "=0.116.1", default-features = false, features = ["os", "network", "rustls-tls"] }
nu-engine = "=0.116.1"
nu-parser = "=0.116.1"
nu-protocol = { version = "=0.116.1", default-features = false }
rrule = "0.14"
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
chrono-tz.workspace = true
jiff.workspace = true
jsonschema.workspace = true
nu-cmd-lang.workspace = true
nu-command.workspace = true
nu-engine.workspace = true
nu-parser.workspace = true
nu-protocol.workspace = true
rrule.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

- [ ] **Step 2: Write the failing test**

`crates/ppp-core/tests/recur.rs`:

```rust
//! Recurrence expansion: wall-clock time across DST, RDATE, EXDATE, the
//! overlap window, and the limits.

use jiff::{SignedDuration, Timestamp, Zoned};
use ppp_core::recur::{RecurError, Series, occurrences};

fn zoned(text: &str) -> Zoned {
    text.parse().unwrap()
}

fn instant(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn starts(series: &Series, length: SignedDuration, from: &str, until: &str) -> Vec<String> {
    occurrences(series, length, instant(from), instant(until))
        .unwrap()
        .iter()
        .map(|z| z.to_string())
        .collect()
}

const TWO_HOURS: SignedDuration = SignedDuration::from_hours(2);

#[test]
fn weekly_keeps_its_wall_clock_time_across_dst() {
    let start = zoned("2026-10-27T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=WEEKLY;BYDAY=TU"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-10-26T00:00:00Z",
            "2026-11-12T00:00:00Z"
        ),
        [
            "2026-10-27T18:00:00-05:00[America/Chicago]",
            "2026-11-03T18:00:00-06:00[America/Chicago]",
            "2026-11-10T18:00:00-06:00[America/Chicago]",
        ]
    );
}

#[test]
fn a_time_in_the_spring_forward_gap_moves_later() {
    let start = zoned("2026-03-07T02:30:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY;COUNT=3"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-03-01T00:00:00Z",
            "2026-03-31T00:00:00Z"
        ),
        [
            "2026-03-07T02:30:00-06:00[America/Chicago]",
            "2026-03-08T03:30:00-05:00[America/Chicago]",
            "2026-03-09T02:30:00-05:00[America/Chicago]",
        ]
    );
}

#[test]
fn exdates_remove_and_rdates_add() {
    let start = zoned("2026-11-03T18:00:00[America/Chicago]");
    let exdates = [zoned("2026-11-10T18:00:00[America/Chicago]")];
    let rdates = [zoned("2026-11-12T12:00:00[America/Chicago]")];
    let series = Series {
        start: &start,
        rrule: Some("FREQ=WEEKLY;COUNT=3"),
        rdates: &rdates,
        exdates: &exdates,
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        ),
        [
            "2026-11-03T18:00:00-06:00[America/Chicago]",
            "2026-11-12T12:00:00-06:00[America/Chicago]",
            "2026-11-17T18:00:00-06:00[America/Chicago]",
        ]
    );
}

#[test]
fn rdates_alone_still_count_dtstart() {
    let start = zoned("2026-11-03T18:00:00[America/Chicago]");
    let rdates = [zoned("2026-11-05T18:00:00[America/Chicago]")];
    let series = Series {
        start: &start,
        rrule: None,
        rdates: &rdates,
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        )
        .len(),
        2
    );
}

#[test]
fn dtstart_counts_even_when_the_rule_skips_it() {
    // A Monday DTSTART with a Tuesday rule: RFC 5545 counts the Monday too.
    let start = zoned("2026-11-02T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=WEEKLY;BYDAY=TU;COUNT=2"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-01T00:00:00Z",
            "2026-12-01T00:00:00Z"
        ),
        [
            "2026-11-02T18:00:00-06:00[America/Chicago]",
            "2026-11-03T18:00:00-06:00[America/Chicago]",
            "2026-11-10T18:00:00-06:00[America/Chicago]",
        ]
    );
}

#[test]
fn an_occurrence_under_way_when_the_window_opens_is_kept() {
    let start = zoned("2026-11-03T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY"),
        rdates: &[],
        exdates: &[],
    };
    // 19:00 Chicago on Nov 4 is 01:00Z on Nov 5; the 18:00 occurrence runs until 20:00.
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-05T01:00:00Z",
            "2026-11-05T02:00:00Z"
        ),
        ["2026-11-04T18:00:00-06:00[America/Chicago]"]
    );
    // One that ends exactly as the window opens is out.
    assert!(
        starts(
            &series,
            TWO_HOURS,
            "2026-11-05T02:00:00Z",
            "2026-11-05T03:00:00Z"
        )
        .is_empty()
    );
}

#[test]
fn moments_must_start_inside_the_window() {
    let start = zoned("2026-11-03T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            SignedDuration::ZERO,
            "2026-11-04T00:00:00Z",
            "2026-11-05T00:00:00Z"
        ),
        ["2026-11-03T18:00:00-06:00[America/Chicago]"]
    );
    assert_eq!(
        starts(
            &series,
            SignedDuration::ZERO,
            "2026-11-05T00:00:00Z",
            "2026-11-06T00:00:00Z"
        ),
        ["2026-11-04T18:00:00-06:00[America/Chicago]"]
    );
}

#[test]
fn all_day_dates_in_utc() {
    let start = zoned("2026-10-17T00:00:00[UTC]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=YEARLY;COUNT=3"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            SignedDuration::from_hours(24),
            "2027-01-01T00:00:00Z",
            "2030-01-01T00:00:00Z"
        ),
        [
            "2027-10-17T00:00:00+00:00[UTC]",
            "2028-10-17T00:00:00+00:00[UTC]"
        ]
    );
}

#[test]
fn a_long_running_series_is_not_capped_by_its_past() {
    let start = zoned("1990-01-01T09:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=DAILY"),
        rdates: &[],
        exdates: &[],
    };
    assert_eq!(
        starts(
            &series,
            TWO_HOURS,
            "2026-10-12T05:00:00Z",
            "2026-10-19T05:00:00Z"
        )
        .len(),
        7
    );
}

#[test]
fn too_many_occurrences_is_an_error() {
    let start = zoned("2026-10-27T18:00:00[America/Chicago]");
    let series = Series {
        start: &start,
        rrule: Some("FREQ=MINUTELY"),
        rdates: &[],
        exdates: &[],
    };
    let err = occurrences(
        &series,
        TWO_HOURS,
        instant("2026-10-27T00:00:00Z"),
        instant("2026-12-01T00:00:00Z"),
    )
    .unwrap_err();
    assert_eq!(err, RecurError::TooMany);
}

#[test]
fn bad_rules_are_errors() {
    let start = zoned("2026-10-27T18:00:00[America/Chicago]");
    for rule in ["FREQ=SOMETIMES", "FREQ=DAILY;UNTIL=20261030T180000"] {
        let series = Series {
            start: &start,
            rrule: Some(rule),
            rdates: &[],
            exdates: &[],
        };
        let err = occurrences(
            &series,
            TWO_HOURS,
            instant("2026-10-27T00:00:00Z"),
            instant("2026-12-01T00:00:00Z"),
        );
        assert!(matches!(err, Err(RecurError::Rule(_))), "{rule}: {err:?}");
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test recur`
Expected: compile error, `unresolved import ppp_core::recur`.

- [ ] **Step 4: Write the code**

`crates/ppp-core/src/recur.rs`:

```rust
//! Recurrence expansion (RFC 5545 RRULE, RDATE and EXDATE).
//!
//! This is the only module that touches the `rrule` crate, which works in
//! chrono and chrono-tz; everything in and out is jiff.

use chrono::TimeZone as _;
use jiff::{SignedDuration, Timestamp, Zoned};
use rrule::{RRule, RRuleSet, Tz, Unvalidated};

/// The most occurrences one series may produce inside a window.
pub const MAX_OCCURRENCES: u16 = 10_000;

/// A recurring event's rule, as read from its master VEVENT.
pub struct Series<'a> {
    /// DTSTART, in the zone the rule repeats in.
    pub start: &'a Zoned,
    pub rrule: Option<&'a str>,
    pub rdates: &'a [Zoned],
    pub exdates: &'a [Zoned],
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecurError {
    #[error("{0}")]
    Rule(String),
    #[error("time zone \"{0}\" is not an IANA zone name")]
    Zone(String),
    #[error("the rule has more than {MAX_OCCURRENCES} occurrences in the window")]
    TooMany,
}

/// The start of every occurrence of `series` that overlaps [from, until),
/// in the series' zone, earliest first. Each occurrence lasts `length`; one
/// with no length is a moment and must start inside the window. DTSTART
/// always counts as an occurrence (RFC 5545 §3.8.5.3).
pub fn occurrences(
    series: &Series,
    length: SignedDuration,
    from: Timestamp,
    until: Timestamp,
) -> Result<Vec<Zoned>, RecurError> {
    let zone = series.start.time_zone().clone();
    let name = zone
        .iana_name()
        .ok_or_else(|| RecurError::Zone(format!("{zone:?}")))?;
    let tz = Tz::Tz(
        name.parse::<chrono_tz::Tz>()
            .map_err(|_| RecurError::Zone(name.to_string()))?,
    );
    let to_chrono = |ts: Timestamp| {
        tz.timestamp_opt(ts.as_second(), ts.subsec_nanosecond() as u32)
            .single()
            .expect("an instant has exactly one local time")
    };

    let dt_start = to_chrono(series.start.timestamp());
    let set = match series.rrule {
        Some(rule) => rule
            .parse::<RRule<Unvalidated>>()
            .and_then(|rule| rule.build(dt_start))
            .map_err(|e| RecurError::Rule(e.to_string()))?,
        None => RRuleSet::new(dt_start),
    };
    let mut rdates: Vec<_> = series
        .rdates
        .iter()
        .map(|z| to_chrono(z.timestamp()))
        .collect();
    rdates.push(dt_start);
    let exdates = series
        .exdates
        .iter()
        .map(|z| to_chrono(z.timestamp()))
        .collect();

    // `after` and `before` are inclusive; the exact window is applied below.
    let earliest = from.checked_sub(length).unwrap_or(from);
    let result = set
        .set_rdates(rdates)
        .set_exdates(exdates)
        .after(to_chrono(earliest))
        .before(to_chrono(until))
        .all(MAX_OCCURRENCES);
    if result.limited {
        return Err(RecurError::TooMany);
    }

    let mut starts: Vec<Zoned> = result
        .dates
        .iter()
        .map(|dt| {
            Timestamp::new(dt.timestamp(), dt.timestamp_subsec_nanos() as i32)
                .expect("chrono times from jiff stay in jiff's range")
                .to_zoned(zone.clone())
        })
        .filter(|start| {
            let begin = start.timestamp();
            let overlaps = if length.is_zero() {
                begin >= from
            } else {
                begin + length > from
            };
            overlaps && begin < until
        })
        .collect();
    starts.dedup();
    Ok(starts)
}
```

`crates/ppp-core/src/lib.rs`:

```rust
pub mod commands;
pub mod engine;
pub mod ics;
pub mod pipe;
pub mod recur;
pub mod schema;
pub mod serialize;
pub mod time;
pub mod time_fields;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test recur`
Expected: `test result: ok. 11 passed`

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add Cargo.toml Cargo.lock crates/ppp-core
git commit -m "Expand recurrence rules with the rrule crate

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---

### Task 2: `expand rrule` and `campus-week`

**Files:** Create `crates/ppp-core/src/commands/expand_rrule.rs`; Modify `crates/ppp-core/src/commands/mod.rs`, `crates/ppp-core/tests/engine.rs` (adds `expand rrule` to the built-in collision list), `pipes/campus-week.nu`; Test `crates/ppp-core/tests/expand_rrule.rs`.

**Interfaces:** Consumes `recur::{Series, occurrences}` (Task 1), `time::{timestamp, to_nu}`, `Engine`, `into event`. Produces the `expand rrule --from <datetime> --until <datetime>` command (table → table) and `ppp_core::commands::expand_rrule::{expand(Vec<Value>, Timestamp, Timestamp, Span) -> Result<Vec<Value>, ExpandError>, ExpandError { uid, message }}`.

- [ ] **Step 1: Write the failing test**

`crates/ppp-core/tests/expand_rrule.rs`:

```rust
//! `expand rrule` inside the engine, from `from ics` rows to valid events.

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

/// `from ics | expand rrule … | into event`, serialized and validated.
fn expanded(ics: &str, from: &str, until: &str) -> Vec<Json> {
    let registry = Registry::stdlib().unwrap();
    let time_fields = &registry.record_type("event").unwrap().time_fields;
    let mut engine = Engine::new();
    let rows = engine
        .eval(
            &format!("use pipes *; $in | from ics | expand rrule --from {from} --until {until} | into event"),
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

fn summary_and_start(events: &[Json]) -> Vec<(String, String)> {
    events
        .iter()
        .map(|e| {
            (
                e["summary"].as_str().unwrap().into(),
                e["start"].as_str().unwrap().into(),
            )
        })
        .collect()
}

#[test]
fn a_weekly_event_becomes_its_occurrences() {
    let events = expanded(
        &fixture("campus-events.ics"),
        "2026-11-01T00:00:00-05:00",
        "2026-12-01T00:00:00-06:00",
    );
    let chess: Vec<&Json> = events
        .iter()
        .filter(|e| e["summary"] == "Chess Club")
        .collect();
    let starts: Vec<&str> = chess.iter().map(|e| e["start"].as_str().unwrap()).collect();
    // Weekly on Tuesdays; 2026-11-24 is an EXDATE.
    assert_eq!(
        starts,
        [
            "2026-11-03T18:00:00",
            "2026-11-10T18:00:00",
            "2026-11-17T18:00:00"
        ]
    );
    assert_eq!(
        chess[0]["id"],
        "weekly-chess@calendar.example.edu_20261104T000000Z"
    );
    assert_eq!(chess[0]["series"], "weekly-chess@calendar.example.edu");
    assert_eq!(chess[0]["end"], "2026-11-03T20:00:00");
    assert_eq!(chess[0].get("rrule"), None);
    assert_eq!(chess[0].get("exdates"), None);
}

#[test]
fn rows_that_do_not_recur_pass_through() {
    let events = expanded(
        &fixture("campus-events.ics"),
        "2026-11-01T00:00:00-05:00",
        "2026-12-01T00:00:00-06:00",
    );
    let others: Vec<&str> = events
        .iter()
        .filter(|e| e["summary"] != "Chess Club")
        .map(|e| e["summary"].as_str().unwrap())
        .collect();
    // `expand rrule` only expands; it doesn't drop the October events.
    assert_eq!(
        others,
        [
            "Fall Orchestra Concert",
            "Family Weekend",
            "Guest Lecture: Pipelines Everywhere"
        ]
    );
}

#[test]
fn an_override_stands_in_for_its_occurrence() {
    let events = expanded(
        &fixture("team-planning.ics"),
        "2026-10-12T00:00:00-04:00",
        "2026-10-17T00:00:00-04:00",
    );
    let standups: Vec<(String, String)> = summary_and_start(&events)
        .into_iter()
        .filter(|(summary, _)| summary.starts_with("Daily standup"))
        .collect();
    assert_eq!(
        standups,
        [
            ("Daily standup".into(), "2026-10-12T09:30:00".into()),
            ("Daily standup".into(), "2026-10-13T09:30:00".into()),
            ("Daily standup".into(), "2026-10-15T09:30:00".into()),
            ("Daily standup".into(), "2026-10-16T09:30:00".into()),
            ("Daily standup (moved)".into(), "2026-10-14T11:00:00".into()),
        ]
    );
    let moved = events
        .iter()
        .find(|e| e["summary"] == "Daily standup (moved)")
        .unwrap();
    assert_eq!(
        moved["id"],
        "standup-2026@product.example.com_20261014T133000Z"
    );
    let generated = events
        .iter()
        .find(|e| e["start"] == "2026-10-13T09:30:00")
        .unwrap();
    assert_eq!(
        generated["id"],
        "standup-2026@product.example.com_20261013T133000Z"
    );
}

#[test]
fn ids_are_the_same_whether_or_not_a_feed_was_expanded() {
    // Google pre-expands recurrences as `<uid>_<UTC start>`; so does `expand rrule`.
    let events = expanded(
        &fixture("team-planning.ics"),
        "2026-10-12T00:00:00-04:00",
        "2026-10-13T00:00:00-04:00",
    );
    let ids: Vec<&str> = events.iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert!(
        ids.contains(&"standup-2026@product.example.com_20261012T133000Z"),
        "{ids:?}"
    );
}

#[test]
fn bad_rules_name_the_event() {
    let ics = "BEGIN:VCALENDAR\nX-WR-TIMEZONE:America/Chicago\nBEGIN:VEVENT\nUID:odd@example.edu\nDTSTART:20261013T120000\nRRULE:FREQ=SOMETIMES\nEND:VEVENT\nEND:VCALENDAR\n";
    let mut engine = Engine::new();
    let err = engine
        .eval(
            "$in | from ics | expand rrule --from 2026-10-01T00:00:00Z --until 2026-11-01T00:00:00Z",
            Value::test_string(ics),
        )
        .unwrap_err();
    assert!(
        err.0.contains("cannot expand event odd@example.edu"),
        "{err}"
    );
    assert!(err.0.contains("SOMETIMES"), "{err}");
}

#[test]
fn the_window_is_required() {
    let mut engine = Engine::new();
    let err = engine
        .eval(
            "[] | expand rrule --from 2026-10-01T00:00:00Z",
            Value::test_nothing(),
        )
        .unwrap_err();
    assert!(err.0.contains("until"), "{err}");
}

#[test]
fn events_without_an_end_stay_without_one() {
    let ics = "BEGIN:VCALENDAR\nX-WR-TIMEZONE:America/Chicago\nBEGIN:VEVENT\nUID:m@example.edu\nSUMMARY:Bell\nDTSTART:20261013T120000\nRRULE:FREQ=DAILY;COUNT=2\nEND:VEVENT\nEND:VCALENDAR\n";
    let events = expanded(ics, "2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].get("end"), None);
    assert_eq!(events[1]["start"], json!("2026-10-14T12:00:00"));
}

#[test]
fn all_day_events_with_no_zone_keep_their_dates() {
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:h@example.edu\nSUMMARY:Founders Day\nDTSTART;VALUE=DATE:20261105\nDTEND;VALUE=DATE:20261106\nRRULE:FREQ=YEARLY\nEND:VEVENT\nEND:VCALENDAR\n";
    let events = expanded(ics, "2026-01-01T00:00:00Z", "2029-01-01T00:00:00Z");
    let dates: Vec<(&str, &str)> = events
        .iter()
        .map(|e| (e["start"].as_str().unwrap(), e["end"].as_str().unwrap()))
        .collect();
    assert_eq!(
        dates,
        [
            ("2026-11-05", "2026-11-06"),
            ("2027-11-05", "2027-11-06"),
            ("2028-11-05", "2028-11-06")
        ]
    );
    assert_eq!(events[1]["id"], "h@example.edu_20271105T000000Z");
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test expand_rrule`
Expected: all 8 FAIL with an engine error — `expand rrule` isn't registered yet.

- [ ] **Step 3: Write the code**

`crates/ppp-core/src/commands/expand_rrule.rs`:

```rust
//! `expand rrule`: one row per occurrence of each recurring event (spec §5.4).

use std::collections::HashSet;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, Zoned};
use nu_engine::command_prelude::*;
use nu_protocol::shell_error::generic::GenericError;

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
        let out_of_range = |value: &Value| {
            ShellError::Generic(GenericError::new(
                "date out of range",
                "outside the years -9999 to 9999",
                value.span(),
            ))
        };
        let from_ts = time::timestamp(&from.as_date()?).map_err(|_| out_of_range(&from))?;
        let until_ts = time::timestamp(&until.as_date()?).map_err(|_| out_of_range(&until))?;
        let rows = input.into_value(head)?.into_list()?;
        let expanded = expand(rows, from_ts, until_ts, head).map_err(|err| {
            ShellError::Generic(GenericError::new(
                format!("cannot expand event {}", err.uid),
                err.message,
                head,
            ))
        })?;
        Ok(Value::list(expanded, head).into_pipeline_data())
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
        let end = row.get_data_by_key("end").and_then(|v| timestamp(&v));
        let length = end.map_or(SignedDuration::ZERO, |end| {
            end.duration_since(start.timestamp())
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
                let length = end.map(|_| length);
                out.push(occurrence_row(&row, &occurrence, length, span));
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

/// The master row with its times moved to one occurrence.
fn occurrence_row(
    master: &Value,
    start: &Zoned,
    length: Option<SignedDuration>,
    span: Span,
) -> Value {
    let mut record = master.as_record().cloned().unwrap_or_default();
    let at = Value::date(time::to_nu(start), span);
    record.insert("start", at.clone());
    if let Some(length) = length {
        let end = start
            .checked_add(length)
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

`crates/ppp-core/src/commands/mod.rs`:

```rust
//! Our own nushell commands.

use nu_protocol::engine::StateWorkingSet;

mod expand_rrule;
mod from_ics;

pub use expand_rrule::ExpandRrule;
pub use from_ics::FromIcs;

/// Registers every command in this module.
pub fn bind(working_set: &mut StateWorkingSet) {
    working_set.add_decl(Box::new(ExpandRrule));
    working_set.add_decl(Box::new(FromIcs));
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
]: nothing -> table {
    use pipes *
    let now = date now
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
Expected: every suite passes, 136 tests; `expand_rrule` 8. Then `cargo test -p ppp-cli --test run -- --ignored` passes against the live feed.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add crates/ppp-core pipes/campus-week.nu
git commit -m "Add expand rrule and use it in campus-week

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---


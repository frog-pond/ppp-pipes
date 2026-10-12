//! `during` keeps the rows whose time span overlaps a window.

use nu_protocol::Value;
use ppp_core::engine::Engine;

fn kept(rows: &str, from: &str, until: &str) -> String {
    let mut engine = Engine::new();
    engine
        .eval(&format!("use pipes *; {rows} | during {from} {until} | get name | to nuon"), Value::test_nothing())
        .unwrap()
        .coerce_into_string()
        .unwrap()
}

const ROWS: &str = "[
    [name start end];
    [before 2026-10-10T09:00:00-05:00 2026-10-10T10:00:00-05:00]
    [ongoing 2026-10-01T00:00:00-05:00 2026-11-01T00:00:00-05:00]
    [today-all-day 2026-10-12T00:00:00-05:00 2026-10-13T00:00:00-05:00]
    [inside 2026-10-13T19:00:00-05:00 2026-10-13T21:00:00-05:00]
    [no-end 2026-10-14T12:00:00-05:00 null]
    [after 2026-10-20T09:00:00-05:00 2026-10-20T10:00:00-05:00]
]";

#[test]
fn keeps_events_that_overlap_the_window() {
    assert_eq!(
        kept(ROWS, "2026-10-12T15:00:00-05:00", "2026-10-15T15:00:00-05:00"),
        "[ongoing, today-all-day, inside, no-end]"
    );
}

#[test]
fn an_event_ending_as_the_window_opens_is_out() {
    assert_eq!(kept(ROWS, "2026-10-10T10:00:00-05:00", "2026-10-10T11:00:00-05:00"), "[ongoing]");
}

#[test]
fn an_event_without_an_end_is_a_moment() {
    assert_eq!(kept(ROWS, "2026-10-14T12:00:00-05:00", "2026-10-14T13:00:00-05:00"), "[ongoing, no-end]");
}

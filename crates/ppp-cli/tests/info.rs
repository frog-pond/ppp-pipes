//! `ppp info` prints what a pipe declares, without running it.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;

fn ppp(args: &[&str]) -> Output {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Command::new(env!("CARGO_BIN_EXE_ppp"))
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap()
}

#[test]
fn prints_the_pipes_declarations() {
    let out = ppp(&["info", "--pipes-dir", "fixtures/pipes", "fetches"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let info: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        info,
        json!({
            "name": "fetches",
            "title": "Events from a URL",
            "description": "For tests: reads an iCalendar feed from --url.",
            "category": "pipes",
            "search_terms": ["calendar", "test"],
            "examples": [{ "description": "A local feed", "example": "fetches --url http://localhost:8080/feed" }],
            "params": [{ "name": "url", "type": "string", "description": "The feed to read", "default": null }],
            "output": ["https://pipes.frogpond.tech/stdlib/calendar/1#event"],
            "refresh": "PT1H"
        })
    );
}

#[test]
fn defaults_are_json() {
    let out = ppp(&["info", "fixtures/pipes/two-events.nu"]);
    let info: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(info["params"][0]["default"], 2);
    assert_eq!(
        info["params"][2],
        json!({ "name": "loud", "type": "switch", "description": "Shout the summaries", "default": null })
    );
    assert_eq!(info["refresh"], serde_json::Value::Null);
}

#[test]
fn info_does_not_run_the_pipe() {
    // `slow` never finishes if run.
    let out = ppp(&["info", "fixtures/pipes/slow.nu"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn a_missing_pipe_exits_2() {
    let out = ppp(&["info", "fixtures/pipes/nope.nu"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("ppp: "));
}

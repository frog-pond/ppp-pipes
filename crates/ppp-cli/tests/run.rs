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
    let out = ppp(&["run", "campus-week", "--within", "3day"]);
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

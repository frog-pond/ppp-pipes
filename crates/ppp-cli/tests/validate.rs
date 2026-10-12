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

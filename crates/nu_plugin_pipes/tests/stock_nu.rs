//! The plugin inside a real `nu` (spec §4.5): our commands in stock nushell,
//! pipe files running unchanged, and `pipe get`.
//!
//! These need `nu` 0.116.1, the version the plugin is built against; plugins
//! must match nu's version. Set `PPP_TEST_NU` to its path, or have it on
//! PATH, and run `cargo test -p nu_plugin_pipes -- --ignored`.

use std::path::{Path, PathBuf};
use std::process::Command;

use nu_protocol::{Value, record};
use ppp_core::pipe::{Pipe, check_records};
use ppp_core::schema::Registry;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Runs `source` in `nu` with only our plugin loaded and `stdlib/` as a
/// library directory, from the repository root, and returns stdout. The
/// root has a `pipes/` directory (the acceptance pipes), which must not
/// shadow the library module.
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
        .current_dir(repo())
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
        "use ppp *
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
fn pipes_that_query_html_load_without_nu_plugin_query() {
    // Stock nu's own `query` would make `query web` a parse error here.
    let out = nu("use pipes/carleton-week.nu
        open --raw fixtures/html/carleton-calendar.html
        | query html --query 'picture img' --attribute src
        | length");
    assert_eq!(out.trim(), "3");
}

#[test]
#[ignore]
fn pipe_files_run_unchanged() {
    let repo = repo().display().to_string();
    let out = nu(&format!(
        "use {repo}/fixtures/pipes/two-events.nu; two-events --count 1 --loud | validate | to pipes-json"
    ));
    // The same records `ppp run two-events --count 1 --loud` writes.
    let mut pipe = Pipe::load(&Path::new(&repo).join("fixtures/pipes/two-events.nu")).unwrap();
    let records = pipe
        .run(&record! { "count" => Value::test_int(1), "loud" => Value::test_bool(true) })
        .unwrap();
    let expected = check_records(&Registry::stdlib().unwrap(), pipe.info(), &records).unwrap();
    assert_eq!(json(&out), serde_json::Value::Array(expected));
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

/// Like `nu`, but returns the exit status and stderr instead of asserting.
fn nu_failing(source: &str) -> String {
    let nu = std::env::var("PPP_TEST_NU").unwrap_or_else(|_| "nu".into());
    let registry = Path::new(env!("CARGO_TARGET_TMPDIR")).join("empty-plugin-registry-2.msgpackz");
    let _ = std::fs::remove_file(&registry);
    let out = Command::new(&nu)
        .args(["-n", "--plugin-config"])
        .arg(&registry)
        .arg("--plugins")
        .arg(env!("CARGO_BIN_EXE_nu_plugin_pipes"))
        .args(["-c", source])
        .current_dir(repo())
        .output()
        .unwrap();
    assert!(!out.status.success(), "{source} succeeded");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
#[ignore]
fn pipe_get_keeps_the_sandbox_in_stock_nu() {
    // Stock nu has `ls`; the pipe still runs in the sandboxed engine.
    let err = nu_failing("pipe get reads-files --pipes-dir fixtures/pipes");
    assert!(err.contains("External calls are not supported"), "{err}");
}

#[test]
#[ignore]
fn pipe_get_says_when_its_time_limit_stopped_it() {
    let err = nu_failing("pipe get slow --pipes-dir fixtures/pipes --timeout 1sec");
    assert!(
        err.contains("ran longer than 1sec and was stopped"),
        "{err}"
    );
}

#[test]
#[ignore]
fn ctrl_c_stops_pipe_get() {
    let nu = std::env::var("PPP_TEST_NU").unwrap_or_else(|_| "nu".into());
    let registry = Path::new(env!("CARGO_TARGET_TMPDIR")).join("empty-plugin-registry-3.msgpackz");
    let _ = std::fs::remove_file(&registry);
    let started = std::time::Instant::now();
    let child = Command::new(&nu)
        .args(["-n", "--plugin-config"])
        .arg(&registry)
        .arg("--plugins")
        .arg(env!("CARGO_BIN_EXE_nu_plugin_pipes"))
        .args(["-c", "pipe get slow --pipes-dir fixtures/pipes"])
        .current_dir(repo())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "took {:?}",
        started.elapsed()
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("was interrupted"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

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

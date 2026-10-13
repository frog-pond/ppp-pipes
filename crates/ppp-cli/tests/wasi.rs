//! `ppp` built for WASI (wasm32-wasip2) runs pipes as the native build
//! does.
//!
//! These need the wasm build and wasmtime. Build with
//! `cargo +nightly build -p ppp-cli --target wasm32-wasip2`, set
//! `PPP_TEST_WASM` to the `ppp.wasm` it writes, have `wasmtime` on PATH (or
//! set `PPP_TEST_WASMTIME`), and run
//! `cargo test -p ppp-cli --test wasi -- --ignored`.

#[path = "../../ppp-core/tests/support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value as Json;
use support::{reply, serve};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo().join(path)).unwrap()
}

/// `ppp <args>` natively, from the repository root.
fn native(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ppp"))
        .args(args)
        .current_dir(repo())
        .output()
        .unwrap()
}

/// `ppp <args>` under wasmtime, with the repository mounted at `/repo` and
/// the network open.
fn wasm(args: &[&str]) -> Output {
    let wasm = std::env::var("PPP_TEST_WASM").expect("set PPP_TEST_WASM to ppp.wasm");
    let wasmtime = std::env::var("PPP_TEST_WASMTIME").unwrap_or_else(|_| "wasmtime".into());
    Command::new(wasmtime)
        .args([
            "run",
            "-S",
            "inherit-network=y",
            "-S",
            "allow-ip-name-lookup=y",
        ])
        .arg("--dir")
        .arg(format!("{}::/repo", repo().display()))
        .arg(wasm)
        .args(args)
        .output()
        .unwrap()
}

fn records(out: &Output) -> Vec<Json> {
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
#[ignore]
fn a_pipe_runs_as_it_does_natively() {
    let flags = ["--count", "1", "--loud"];
    let native = native(
        &[
            &["run", "--no-cache", "fixtures/pipes/two-events.nu"][..],
            &flags,
        ]
        .concat(),
    );
    let wasm = wasm(
        &[
            &["run", "--no-cache", "/repo/fixtures/pipes/two-events.nu"][..],
            &flags,
        ]
        .concat(),
    );
    assert_eq!(records(&wasm), records(&native));
}

#[test]
#[ignore]
fn a_pipe_fetches_over_http() {
    let ics = read("fixtures/ics/campus-events.ics");
    let server = serve(vec![
        reply("200 OK", &[("Content-Type", "text/calendar")], &ics),
        reply("200 OK", &[("Content-Type", "text/calendar")], &ics),
    ]);
    let args = |pipe: &str| {
        vec![
            "run".to_string(),
            "--no-cache".into(),
            pipe.into(),
            "--url".into(),
            server.url.clone(),
        ]
    };
    let native_args = args("fixtures/pipes/fetches.nu");
    let wasm_args = args("/repo/fixtures/pipes/fetches.nu");
    let native = native(&native_args.iter().map(String::as_str).collect::<Vec<_>>());
    let wasm = wasm(&wasm_args.iter().map(String::as_str).collect::<Vec<_>>());
    let wasm = records(&wasm);
    assert!(!wasm.is_empty());
    assert_eq!(wasm, records(&native));
}

#[test]
#[ignore]
fn carleton_week_queries_its_page() {
    let ics = read("fixtures/ics/carleton.ics");
    let html = read("fixtures/html/carleton-calendar.html");
    let feed = serve(vec![reply("200 OK", &[], &ics), reply("200 OK", &[], &ics)]);
    let page = serve(vec![
        reply("200 OK", &[("Content-Type", "text/html")], &html),
        reply("200 OK", &[("Content-Type", "text/html")], &html),
    ]);
    let args = |pipes_dir: &str| {
        vec![
            "run".to_string(),
            "--no-cache".into(),
            "--pipes-dir".into(),
            pipes_dir.into(),
            "carleton-week".into(),
            "--from".into(),
            "2026-09-01T00:00:00Z".into(),
            "--within".into(),
            "1825day".into(),
            "--source".into(),
            feed.url.clone(),
            "--page".into(),
            page.url.clone(),
        ]
    };
    let native_args = args("pipes");
    let wasm_args = args("/repo/pipes");
    let native = native(&native_args.iter().map(String::as_str).collect::<Vec<_>>());
    let wasm = wasm(&wasm_args.iter().map(String::as_str).collect::<Vec<_>>());
    let wasm = records(&wasm);
    assert!(wasm.iter().any(|e| e.get("images").is_some()));
    assert_eq!(wasm, records(&native));
}

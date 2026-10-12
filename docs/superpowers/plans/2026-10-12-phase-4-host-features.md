# ppp-pipes Phase 4: Host Features — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repeat runs are fast and polite to sources (an on-disk HTTP cache), pipes can build on other pipes (`pipe get`), a stuck pipe stops (`--timeout`), and a UI can read a pipe's form without running it (`ppp info`).

**Architecture:** A `Host` (`host.rs`) holds what every engine of one run shares: cache dir, pipes dir, limits, the interrupt flag a timeout sets, the stack of running pipes, warnings. `cache.rs` stores responses; `http.rs` fetches through it (ureq). Our `http get` command replaces nushell's; `pipe get` loads another pipe into its own engine on the same host. `Pipe::load_with(path, host)` reads extra metadata (refresh, category, search terms, examples). The CLI gains `--cache-dir`/`--no-cache`/`--timeout` and `ppp info`.

**Tech Stack:** ureq 3.4 (already in the tree via nu-command; rustls + webpki roots), sha2 0.11, dirs 7, serde, jiff (+serde).

**Spec:** [`docs/superpowers/specs/2026-10-11-ppp-pipes-cli-prototype-design.md`](../specs/2026-10-11-ppp-pipes-cli-prototype-design.md) §4.1, §4.2, §4.4, §5.5, §8 — build-order step 4. The spec was updated in the same commit as this plan.

Every code block was compiled and tested in a scratch copy of the phase 3 branch; the final state passes 168 tests plus the live test, with clippy and fmt clean.

## Global Constraints

- Everything in the earlier phases' Global Constraints still holds.
- Network access in pipes is only through our `http get`, http(s) only.
- Tests talk to a real local HTTP server (`tests/support/mod.rs`), never a mock; the CLI's live test uses `--no-cache` so tests never write to the user's cache directory.

## Decisions made while planning

Wren was away and asked for the phases to continue; these are mine to review.

1. **Freshness:** the pipe's `refresh` wins over `Cache-Control: max-age`; with neither, every fetch revalidates (ETag / Last-Modified). `no-store`/`no-cache` mean no max-age.
2. **Stale fallback:** only a network failure (not an HTTP error status, not an oversized body) falls back to a cached copy, with a warning on stderr.
3. **`http get` output:** JSON (by Content-Type) is parsed unless `--raw`; anything else is text when UTF-8, else binary. Only http and https URLs.
4. **No run log yet.** Spec §4.2 lists one for `http get`, but nothing reads it; it waits for the server work.
5. **`dirs` instead of `directories`** for the default cache location (`dirs` is already in the tree).
6. **Timeout** uses nushell's interrupt flag, shared by every engine of the run (so `pipe get` children stop too); HTTP requests also have a 30 s limit each.
7. **`pipe get` errors** show the other pipe's first five failures.
8. **`ppp info`** prints JSON (machine-readable, for the future UI), with durations in ISO 8601.

## Review Focus

1. **A cached response must never pair new metadata with an old body** (writes are body-then-metadata, each via rename) — Task 1.
2. **A server that goes away after the first fetch** must leave the pipe working from cache, with a warning — Task 1.
3. **A loop of pipes** (`a` gets `b` gets `a`) must stop with the chain, not overflow the stack — Task 2.
4. **`ppp info` must not run the pipe** (a pipe whose `main` never ends) — Task 3.
5. **A pipe past its timeout** must stop within seconds and print nothing on stdout — Task 3.

---

### Task 1: The HTTP cache and fetching

**Files:** Modify `Cargo.toml`, `crates/ppp-core/Cargo.toml`, `crates/ppp-core/src/lib.rs`; Create `crates/ppp-core/src/host.rs`, `crates/ppp-core/src/cache.rs`, `crates/ppp-core/src/http.rs`; Test `crates/ppp-core/tests/http.rs`, `crates/ppp-core/tests/support/mod.rs`.

**Interfaces:** Produces `host::{Host, HostConfig { cache_dir, pipes_dir, max_response_bytes, http_timeout }, DEFAULT_MAX_RESPONSE_BYTES, Running}` with `Host::new`, `interrupt`, `interrupted`, `with_timeout`, `registry`, `warn`, `take_warnings`, `enter`; `cache::{Cache, Meta, max_age}`; `http::{fetch(&Host, &str, Option<SignedDuration>) -> Result<Fetched, FetchError>, Fetched { body, content_type, from_cache }, FetchError { Status, Network, TooLarge }}`.

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
dirs = "7"
jiff = { version = "0.2.38", features = ["tzdb-bundle-always", "serde"] }
jsonschema = { version = "0.58.6", default-features = false }
nu-cmd-lang = "=0.116.1"
# `os` is needed: other nushell crates turn on nu-protocol's `os` feature, and
# nu-command does not compile with it on unless its own `os` is on too.
nu-command = { version = "=0.116.1", default-features = false, features = ["os", "network", "rustls-tls"] }
nu-engine = "=0.116.1"
nu-parser = "=0.116.1"
nu-protocol = { version = "=0.116.1", default-features = false }
rrule = "0.14"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.11"
thiserror = "2"
ureq = "~3.4"
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
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true
ureq.workspace = true
```

- [ ] **Step 2: Write the failing test**

`crates/ppp-core/tests/support/mod.rs`:

```rust
//! A tiny HTTP server for tests: it answers each connection with the next
//! scripted response and records the requests it received.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

pub struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
}

/// One scripted response: status line text, extra headers, body.
pub struct Reply {
    pub status: &'static str,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

pub fn reply(status: &'static str, headers: &[(&'static str, &str)], body: &str) -> Reply {
    Reply {
        status,
        headers: headers.iter().map(|(k, v)| (*k, v.to_string())).collect(),
        body: body.as_bytes().to_vec(),
    }
}

/// Serves `replies` in order, one per connection, then stops listening.
pub fn serve(replies: Vec<Reply>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/feed", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    thread::spawn(move || {
        for reply in replies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            while !request.ends_with(b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            seen.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&request).to_lowercase());
            let mut head = format!(
                "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                reply.status,
                reply.body.len()
            );
            for (name, value) in &reply.headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str("\r\n");
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&reply.body);
        }
    });
    Server { url, requests }
}

/// A fresh, empty directory under the target's temp dir.
pub fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
```

`crates/ppp-core/tests/http.rs`:

```rust
//! `http.rs` and `cache.rs`: caching, revalidation, limits and fallbacks,
//! against a real local HTTP server.

mod support;

use jiff::{SignedDuration, Timestamp};
use ppp_core::cache::{Meta, max_age};
use ppp_core::host::{Host, HostConfig};
use ppp_core::http::{FetchError, fetch};
use support::{reply, serve, temp_dir};

fn host_with_cache(name: &str) -> Host {
    Host::new(HostConfig {
        cache_dir: Some(temp_dir(name)),
        ..HostConfig::default()
    })
}

#[test]
fn a_fresh_cached_response_is_used_without_asking() {
    let server = serve(vec![reply(
        "200 OK",
        &[
            ("Cache-Control", "max-age=60"),
            ("Content-Type", "text/calendar"),
        ],
        "hello",
    )]);
    let host = host_with_cache("http-fresh");
    let first = fetch(&host, &server.url, None).unwrap();
    assert_eq!(
        (first.body.as_slice(), first.from_cache),
        (b"hello".as_slice(), false)
    );
    let second = fetch(&host, &server.url, None).unwrap();
    assert_eq!(
        (second.body.as_slice(), second.from_cache),
        (b"hello".as_slice(), true)
    );
    assert_eq!(second.content_type.as_deref(), Some("text/calendar"));
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn the_pipes_refresh_wins_over_max_age() {
    let server = serve(vec![reply(
        "200 OK",
        &[("Cache-Control", "max-age=0")],
        "hello",
    )]);
    let host = host_with_cache("http-refresh");
    fetch(&host, &server.url, Some(SignedDuration::from_hours(1))).unwrap();
    assert!(
        fetch(&host, &server.url, Some(SignedDuration::from_hours(1)))
            .unwrap()
            .from_cache
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn a_stale_response_is_revalidated_with_its_etag() {
    let server = serve(vec![
        reply(
            "200 OK",
            &[
                ("ETag", "\"v1\""),
                ("Last-Modified", "Mon, 12 Oct 2026 04:00:00 GMT"),
            ],
            "hello",
        ),
        reply("304 Not Modified", &[], ""),
    ]);
    let host = host_with_cache("http-revalidate");
    fetch(&host, &server.url, None).unwrap();
    let second = fetch(&host, &server.url, None).unwrap();
    assert_eq!(
        (second.body.as_slice(), second.from_cache),
        (b"hello".as_slice(), true)
    );
    let requests = server.requests.lock().unwrap();
    assert!(
        requests[1].contains("if-none-match: \"v1\""),
        "{}",
        requests[1]
    );
    assert!(
        requests[1].contains("if-modified-since: mon, 12 oct 2026 04:00:00 gmt"),
        "{}",
        requests[1]
    );
}

#[test]
fn http_errors_name_the_url_and_status() {
    let server = serve(vec![reply("404 Not Found", &[], "gone")]);
    let err = fetch(&Host::default(), &server.url, None).unwrap_err();
    assert_eq!(
        err,
        FetchError::Status {
            url: server.url.clone(),
            status: 404
        }
    );
    assert_eq!(
        err.to_string(),
        format!("GET {} returned HTTP 404", server.url)
    );
}

#[test]
fn bodies_over_the_limit_are_refused() {
    let server = serve(vec![reply("200 OK", &[], &"x".repeat(100))]);
    let host = Host::new(HostConfig {
        max_response_bytes: 10,
        ..HostConfig::default()
    });
    assert!(matches!(
        fetch(&host, &server.url, None),
        Err(FetchError::TooLarge { limit: 10, .. })
    ));
}

#[test]
fn a_network_failure_falls_back_to_the_cached_copy() {
    let server = serve(vec![reply(
        "200 OK",
        &[("Cache-Control", "max-age=0")],
        "hello",
    )]);
    let host = host_with_cache("http-fallback");
    fetch(&host, &server.url, None).unwrap();
    // The server has answered its one reply and stopped listening.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let second = fetch(&host, &server.url, None).unwrap();
    assert_eq!(
        (second.body.as_slice(), second.from_cache),
        (b"hello".as_slice(), true)
    );
    let warnings = host.take_warnings();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("using the copy cached at"),
        "{}",
        warnings[0]
    );
}

#[test]
fn a_network_failure_with_nothing_cached_is_an_error() {
    let server = serve(vec![]);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(matches!(
        fetch(&Host::default(), &server.url, None),
        Err(FetchError::Network { .. })
    ));
}

#[test]
fn without_a_cache_every_fetch_asks() {
    let server = serve(vec![
        reply("200 OK", &[("Cache-Control", "max-age=60")], "one"),
        reply("200 OK", &[("Cache-Control", "max-age=60")], "two"),
    ]);
    let host = Host::default();
    assert_eq!(fetch(&host, &server.url, None).unwrap().body, b"one");
    assert_eq!(fetch(&host, &server.url, None).unwrap().body, b"two");
}

#[test]
fn freshness_and_max_age() {
    let meta = Meta {
        url: "u".into(),
        fetched: "2026-10-12T00:00:00Z".parse().unwrap(),
        content_type: None,
        etag: None,
        last_modified: None,
        max_age: Some(SignedDuration::from_mins(10)),
    };
    let at = |s: &str| s.parse::<Timestamp>().unwrap();
    assert!(meta.is_fresh(at("2026-10-12T00:09:59Z"), None));
    assert!(!meta.is_fresh(at("2026-10-12T00:10:00Z"), None));
    assert!(meta.is_fresh(
        at("2026-10-12T00:30:00Z"),
        Some(SignedDuration::from_hours(1))
    ));
    assert_eq!(
        max_age("public, max-age=600"),
        Some(SignedDuration::from_secs(600))
    );
    assert_eq!(max_age("max-age=600, no-cache"), None);
    assert_eq!(max_age("no-store"), None);
    assert_eq!(max_age("private"), None);
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test http`
Expected: compile error, `unresolved import ppp_core::cache`.

- [ ] **Step 4: Write the code**

`crates/ppp-core/src/host.rs`:

```rust
//! What every engine in one run shares: where the HTTP cache lives, where
//! `pipe get` finds pipes, the limits, and the flag a timeout sets.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::schema::{Registry, SchemaError};

/// The largest response body `http get` accepts by default (spec §8.2).
pub const DEFAULT_MAX_RESPONSE_BYTES: u64 = 20 * 1024 * 1024;

/// The settings a CLI or embedder chooses.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// Where `http get` keeps responses; `None` turns the cache off.
    pub cache_dir: Option<PathBuf>,
    /// Where `pipe get <name>` looks for `<name>.nu`.
    pub pipes_dir: PathBuf,
    /// The largest response body `http get` accepts.
    pub max_response_bytes: u64,
    /// The longest one HTTP request may take.
    pub http_timeout: Duration,
}

impl Default for HostConfig {
    fn default() -> Self {
        HostConfig {
            cache_dir: None,
            pipes_dir: PathBuf::from("pipes"),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            http_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Default)]
pub struct Host {
    pub config: HostConfig,
    interrupt: Arc<AtomicBool>,
    running: Mutex<Vec<String>>,
    warnings: Mutex<Vec<String>>,
    registry: OnceLock<Arc<Registry>>,
}

impl Host {
    pub fn new(config: HostConfig) -> Host {
        Host {
            config,
            ..Host::default()
        }
    }

    /// The flag every engine of this host checks; setting it stops them.
    pub fn interrupt(&self) -> Arc<AtomicBool> {
        self.interrupt.clone()
    }

    /// Whether a timeout has stopped this run.
    pub fn interrupted(&self) -> bool {
        self.interrupt.load(Ordering::Relaxed)
    }

    /// Runs `work`, setting the interrupt flag if it takes longer than
    /// `limit`. Nushell checks the flag between steps, so a stuck pipe
    /// stops with an "interrupted" error.
    pub fn with_timeout<T>(&self, limit: Duration, work: impl FnOnce() -> T) -> T {
        let (done, wait) = mpsc::channel::<()>();
        let flag = self.interrupt.clone();
        let watchdog = thread::spawn(move || {
            if let Err(RecvTimeoutError::Timeout) = wait.recv_timeout(limit) {
                flag.store(true, Ordering::Relaxed);
            }
        });
        let result = work();
        let _ = done.send(());
        let _ = watchdog.join();
        result
    }

    /// The stdlib registry, compiled once per host.
    pub fn registry(&self) -> Result<Arc<Registry>, SchemaError> {
        if let Some(registry) = self.registry.get() {
            return Ok(registry.clone());
        }
        let registry = Arc::new(Registry::stdlib()?);
        Ok(self.registry.get_or_init(|| registry).clone())
    }

    /// Adds a warning for the CLI (or embedder) to show.
    pub fn warn(&self, warning: String) {
        self.warnings.lock().expect("warnings lock").push(warning);
    }

    /// Returns, and forgets, the warnings added with `warn`.
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut *self.warnings.lock().expect("warnings lock"))
    }

    /// Marks `name` as running for the length of the returned guard, or
    /// returns the chain of pipes if `name` is already running.
    pub fn enter(self: &Arc<Self>, name: &str) -> Result<Running, Vec<String>> {
        let mut running = self.running.lock().expect("running lock");
        if running.iter().any(|n| n == name) {
            let mut chain = running.clone();
            chain.push(name.to_string());
            return Err(chain);
        }
        running.push(name.to_string());
        Ok(Running { host: self.clone() })
    }
}

/// While alive, the pipe it was made for counts as running.
pub struct Running {
    host: Arc<Host>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.host.running.lock().expect("running lock").pop();
    }
}
```

`crates/ppp-core/src/cache.rs`:

```rust
//! The on-disk HTTP cache behind `http get`.
//!
//! Each URL is stored as two files named by the SHA-256 of the URL: the body,
//! and a small JSON file with what is needed to decide freshness and to
//! revalidate (`ETag`, `Last-Modified`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What the cache knows about one response, besides its body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub url: String,
    pub fetched: Timestamp,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// From `Cache-Control: max-age`.
    pub max_age: Option<SignedDuration>,
}

impl Meta {
    /// Whether the response can be used without asking the server. The
    /// pipe's `refresh`, when it has one, wins over the server's `max-age`;
    /// with neither, a response is always revalidated.
    pub fn is_fresh(&self, now: Timestamp, refresh: Option<SignedDuration>) -> bool {
        let lifetime = refresh.or(self.max_age).unwrap_or(SignedDuration::ZERO);
        now.duration_since(self.fetched) < lifetime
    }
}

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: &Path) -> Cache {
        Cache {
            dir: dir.to_path_buf(),
        }
    }

    /// The stored response for `url`, if there is a readable one.
    pub fn get(&self, url: &str) -> Option<(Meta, Vec<u8>)> {
        let (meta_path, body_path) = self.paths(url);
        let meta: Meta = serde_json::from_slice(&fs::read(meta_path).ok()?).ok()?;
        if meta.url != url {
            return None;
        }
        Some((meta, fs::read(body_path).ok()?))
    }

    /// Stores a response, replacing any earlier one for the same URL. The
    /// body is written before the metadata, so a reader never pairs new
    /// metadata with an old body.
    pub fn put(&self, meta: &Meta, body: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let (meta_path, body_path) = self.paths(&meta.url);
        write_atomically(&body_path, body)?;
        write_atomically(
            &meta_path,
            &serde_json::to_vec_pretty(meta).expect("metadata serializes"),
        )
    }

    fn paths(&self, url: &str) -> (PathBuf, PathBuf) {
        let key: String = Sha256::digest(url.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        (
            self.dir.join(format!("{key}.json")),
            self.dir.join(format!("{key}.body")),
        )
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let partial = path.with_extension("partial");
    fs::write(&partial, bytes)?;
    fs::rename(partial, path)
}

/// The `max-age` of a `Cache-Control` header, unless it also says
/// `no-store` or `no-cache`.
pub fn max_age(cache_control: &str) -> Option<SignedDuration> {
    let directives: Vec<String> = cache_control
        .split(',')
        .map(|d| d.trim().to_ascii_lowercase())
        .collect();
    if directives
        .iter()
        .any(|d| d == "no-store" || d == "no-cache")
    {
        return None;
    }
    directives
        .iter()
        .find_map(|d| d.strip_prefix("max-age=")?.parse::<i64>().ok())
        .map(SignedDuration::from_secs)
}
```

`crates/ppp-core/src/http.rs`:

```rust
//! Fetching over HTTP for `http get`, through the cache (spec §4.1, §8.1).

use jiff::{SignedDuration, Timestamp};

use crate::cache::{Cache, Meta, max_age};
use crate::host::Host;

/// A response body and how it was obtained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub from_cache: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    #[error("GET {url} returned HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("GET {url} failed: {message}")]
    Network { url: String, message: String },
    #[error("GET {url}: the response is larger than {limit} bytes")]
    TooLarge { url: String, limit: u64 },
}

/// Gets `url`, using a fresh cached copy when there is one, revalidating a
/// stale one, and falling back to a stale one (with a warning) when the
/// network fails. `refresh` is the calling pipe's `refresh`.
pub fn fetch(
    host: &Host,
    url: &str,
    refresh: Option<SignedDuration>,
) -> Result<Fetched, FetchError> {
    let cache = host.config.cache_dir.as_deref().map(Cache::new);
    let cached = cache.as_ref().and_then(|cache| cache.get(url));
    let now = Timestamp::now();
    if let Some((meta, body)) = &cached
        && meta.is_fresh(now, refresh)
    {
        return Ok(Fetched {
            body: body.clone(),
            content_type: meta.content_type.clone(),
            from_cache: true,
        });
    }

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(host.config.http_timeout))
        .http_status_as_error(false)
        .user_agent("ppp-pipes")
        .build()
        .into();
    let mut request = agent.get(url);
    if let Some((meta, _)) = &cached {
        if let Some(etag) = &meta.etag {
            request = request.header("If-None-Match", etag);
        }
        if let Some(modified) = &meta.last_modified {
            request = request.header("If-Modified-Since", modified);
        }
    }
    let network = |err: ureq::Error| match err {
        ureq::Error::BodyExceedsLimit(limit) => FetchError::TooLarge {
            url: url.to_string(),
            limit,
        },
        other => FetchError::Network {
            url: url.to_string(),
            message: other.to_string(),
        },
    };
    let result = request.call().map_err(network).and_then(|mut response| {
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let meta = Meta {
            url: url.to_string(),
            fetched: now,
            content_type: header("content-type"),
            etag: header("etag"),
            last_modified: header("last-modified"),
            max_age: header("cache-control").as_deref().and_then(max_age),
        };
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(host.config.max_response_bytes)
            .read_to_vec()
            .map_err(network)?;
        Ok((status, meta, body))
    });

    match (result, cached) {
        (Ok((304, fresh, _)), Some((old, body))) => {
            let meta = Meta {
                content_type: old.content_type.clone(),
                ..fresh
            };
            if let Some(cache) = &cache {
                let _ = cache.put(&meta, &body);
            }
            Ok(Fetched {
                body,
                content_type: meta.content_type,
                from_cache: true,
            })
        }
        (Ok((status, meta, body)), _) if (200..300).contains(&status) => {
            if let Some(cache) = &cache
                && let Err(err) = cache.put(&meta, &body)
            {
                host.warn(format!("Warning: could not cache {url}: {err}"));
            }
            Ok(Fetched {
                body,
                content_type: meta.content_type,
                from_cache: false,
            })
        }
        (Ok((status, _, _)), _) => Err(FetchError::Status {
            url: url.to_string(),
            status,
        }),
        (Err(err @ FetchError::Network { .. }), Some((meta, body))) => {
            host.warn(format!(
                "Warning: {err}; using the copy cached at {}",
                meta.fetched
            ));
            Ok(Fetched {
                body,
                content_type: meta.content_type,
                from_cache: true,
            })
        }
        (Err(err), _) => Err(err),
    }
}
```

`crates/ppp-core/src/lib.rs`:

```rust
pub mod cache;
pub mod commands;
pub mod engine;
pub mod host;
pub mod http;
pub mod ics;
pub mod pipe;
pub mod recur;
pub mod schema;
pub mod serialize;
pub mod time;
pub mod time_fields;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ppp-core --test http`
Expected: `test result: ok. 9 passed`

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add Cargo.toml Cargo.lock crates/ppp-core
git commit -m "Add an on-disk HTTP cache and fetching through it

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---

### Task 2: Our `http get`, `pipe get`, timeouts and pipe metadata

**Files:** Modify `crates/ppp-core/src/engine.rs` (`Engine::with_host`, signals, `set_refresh`; nushell's `HttpGet` no longer registered), `crates/ppp-core/src/commands/mod.rs` (`Context`), `crates/ppp-core/src/pipe.rs` (`load_with`, refresh/category/search terms/examples, `PipeError::Cycle`); Create `crates/ppp-core/src/commands/http_get.rs`, `crates/ppp-core/src/commands/pipe_get.rs`, `fixtures/pipes/{gets-two-events,cycle-a,cycle-b,gets-invalid,slow,fetches}.nu`; Test `crates/ppp-core/tests/host.rs`.

**Interfaces:** Consumes Task 1. Produces `Engine::with_host(Arc<Host>)`, `Engine::host()`, `Engine::set_refresh(Option<SignedDuration>)`; `Pipe::load_with(&Path, Arc<Host>)`; `PipeInfo { refresh, category, search_terms, examples: Vec<Example { description, example }>, .. }`; `PipeError::Cycle(Vec<String>)`; commands `http get <url> [--raw]` and `pipe get <name> [args]`.

- [ ] **Step 1: Write the failing test**

`fixtures/pipes/gets-two-events.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that gets another pipe's records.
export def main []: nothing -> table {
    pipe get two-events {count: 1} | update summary {|row| $"Got: ($row.summary)" }
}
```

`fixtures/pipes/cycle-a.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# Half of a loop: gets cycle-b.
export def main []: nothing -> table { pipe get cycle-b }
```

`fixtures/pipes/cycle-b.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# The other half: gets cycle-a.
export def main []: nothing -> table { pipe get cycle-a }
```

`fixtures/pipes/gets-invalid.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that gets a pipe whose records fail the check.
export def main []: nothing -> table { pipe get invalid-event }
```

`fixtures/pipes/slow.nu`:

```nu
export const pipe = { output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"] }

# A pipe that never finishes.
export def main []: nothing -> table {
    loop { }
    []
}
```

`fixtures/pipes/fetches.nu`:

```nu
export const pipe = {
    output: ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    refresh: 1hr
}

# Events from a URL
#
# For tests: reads an iCalendar feed from --url.
@category pipes
@search-terms calendar test
@example "A local feed" { fetches --url http://localhost:8080/feed }
export def main [
    --url: string  # The feed to read
]: nothing -> table {
    use pipes *
    http get --raw $url | from ics | into event --source $url
}
```

`crates/ppp-core/tests/host.rs`:

```rust
//! What a run shares through its `Host`: our `http get`, `pipe get`,
//! the timeout, and the pipe's `refresh`.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nu_protocol::{Record, Value, record};
use ppp_core::engine::Engine;
use ppp_core::host::{Host, HostConfig};
use ppp_core::pipe::{Pipe, PipeError};
use support::{reply, serve, temp_dir};

const ICS: &str = "BEGIN:VCALENDAR\r\nX-WR-TIMEZONE:America/Chicago\r\nBEGIN:VEVENT\r\nUID:a@example.edu\r\nSUMMARY:Chess Club\r\nDTSTART:20261013T180000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

fn pipes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pipes")
}

fn host(cache: Option<&str>) -> Arc<Host> {
    Arc::new(Host::new(HostConfig {
        cache_dir: cache.map(temp_dir),
        pipes_dir: pipes_dir(),
        ..HostConfig::default()
    }))
}

fn eval(engine: &mut Engine, source: &str) -> String {
    let value = engine.eval(source, Value::test_nothing()).unwrap();
    engine
        .eval("$in | to nuon", value)
        .unwrap()
        .coerce_into_string()
        .unwrap()
}

#[test]
fn http_get_parses_json_by_content_type() {
    let server = serve(vec![
        reply(
            "200 OK",
            &[("Content-Type", "application/json")],
            r#"{"events": [{"id": 1}], "next": null}"#,
        ),
        reply(
            "200 OK",
            &[("Content-Type", "application/json")],
            r#"{"a": 1}"#,
        ),
    ]);
    let mut engine = Engine::with_host(host(None));
    assert_eq!(
        eval(&mut engine, &format!("http get {}", server.url)),
        "{events: [[id]; [1]], next: null}"
    );
    assert_eq!(
        eval(&mut engine, &format!("http get --raw {}", server.url)),
        r#""{\"a\": 1}""#
    );
}

#[test]
fn http_get_returns_text_or_binary() {
    let server = serve(vec![
        reply(
            "200 OK",
            &[("Content-Type", "text/calendar")],
            "BEGIN:VCALENDAR",
        ),
        support::Reply {
            status: "200 OK",
            headers: vec![],
            body: vec![0xff, 0x00],
        },
    ]);
    let mut engine = Engine::with_host(host(None));
    assert_eq!(
        eval(&mut engine, &format!("http get {} | describe", server.url)),
        r#""string""#
    );
    assert_eq!(
        eval(&mut engine, &format!("http get {} | describe", server.url)),
        r#""binary""#
    );
}

#[test]
fn http_get_refuses_other_schemes() {
    let mut engine = Engine::with_host(host(None));
    let err = engine
        .eval("http get file:///etc/hosts", Value::test_nothing())
        .unwrap_err();
    assert!(
        err.0.contains("only http and https URLs can be fetched"),
        "{err}"
    );
}

#[test]
fn http_errors_are_engine_errors() {
    let server = serve(vec![reply("503 Service Unavailable", &[], "")]);
    let mut engine = Engine::with_host(host(None));
    let err = engine
        .eval(&format!("http get {}", server.url), Value::test_nothing())
        .unwrap_err();
    assert!(err.0.contains("returned HTTP 503"), "{err}");
}

#[test]
fn a_pipes_refresh_keeps_its_fetches_fresh() {
    let server = serve(vec![reply("200 OK", &[], ICS)]);
    let shared = host(Some("host-refresh"));
    let args = record! { "url" => Value::test_string(server.url.clone()) };
    let mut pipe = Pipe::load_with(&pipes_dir().join("fetches.nu"), shared.clone()).unwrap();
    assert_eq!(
        pipe.info().refresh,
        Some(jiff::SignedDuration::from_hours(1))
    );
    assert_eq!(pipe.run(&args).unwrap().len(), 1);
    // The server has served its one reply; the second run must use the cache.
    assert_eq!(pipe.run(&args).unwrap().len(), 1);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn pipe_get_returns_another_pipes_records() {
    let mut pipe = Pipe::load_with(&pipes_dir().join("gets-two-events.nu"), host(None)).unwrap();
    let records = pipe.run(&Record::new()).unwrap();
    assert_eq!(records.len(), 1);
    let summary = records[0]
        .get_data_by_key("summary")
        .unwrap()
        .coerce_into_string()
        .unwrap();
    assert_eq!(summary, "Got: Chess Club");
    // Datetimes stay datetimes.
    assert!(matches!(
        records[0].get_data_by_key("start"),
        Some(Value::Date { .. })
    ));
}

#[test]
fn pipe_get_loops_are_errors() {
    let mut pipe = Pipe::load_with(&pipes_dir().join("cycle-a.nu"), host(None)).unwrap();
    let err = pipe.run(&Record::new()).unwrap_err().to_string();
    assert!(
        err.contains("pipes get each other in a loop: cycle-a → cycle-b → cycle-a"),
        "{err}"
    );
}

#[test]
fn pipe_get_checks_the_other_pipes_records() {
    let mut pipe = Pipe::load_with(&pipes_dir().join("gets-invalid.nu"), host(None)).unwrap();
    let err = pipe.run(&Record::new()).unwrap_err().to_string();
    assert!(err.contains("pipe get invalid-event"), "{err}");
    assert!(err.contains("\"start\" is a required property"), "{err}");
}

#[test]
fn a_run_past_its_time_limit_stops() {
    let shared = host(None);
    let mut pipe = Pipe::load_with(&pipes_dir().join("slow.nu"), shared.clone()).unwrap();
    let started = std::time::Instant::now();
    let result = shared.with_timeout(Duration::from_millis(200), || pipe.run(&Record::new()));
    assert!(matches!(result, Err(PipeError::Engine(_))), "{result:?}");
    assert!(shared.interrupted());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_run_inside_its_time_limit_is_untouched() {
    let shared = host(None);
    let mut pipe = Pipe::load_with(&pipes_dir().join("two-events.nu"), shared.clone()).unwrap();
    let result = shared.with_timeout(Duration::from_secs(30), || pipe.run(&Record::new()));
    assert_eq!(result.unwrap().len(), 2);
    assert!(!shared.interrupted());
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-core --test host`
Expected: compile error — `Engine::with_host`, `HostConfig` in `ppp_core::host` exists but `Pipe::load_with` and `PipeError` variants don't.

- [ ] **Step 3: Write the code**

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

use std::sync::{Arc, Mutex};

use jiff::SignedDuration;
use nu_engine::eval_block;
use nu_protocol::debugger::WithoutDebug;
use nu_protocol::engine::{EngineState, Stack, StateWorkingSet, VirtualPath};
use nu_protocol::{PipelineData, Signals, Span, Value, format_cli_error};

use crate::commands::{self, Context};
use crate::host::Host;

/// The embedded `pipes` nushell module.
const PIPES_MODULE: &str = include_str!("../../../stdlib/pipes/mod.nu");

/// A rendered nushell diagnostic, ready to print.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EngineError(pub String);

pub struct Engine {
    state: EngineState,
    stack: Stack,
    /// Rendered parse warnings not yet taken by `take_warnings`.
    warnings: Vec<String>,
    context: Context,
}

impl Engine {
    /// An engine with its own default `Host`: no cache, pipes in `./pipes`.
    pub fn new() -> Engine {
        Engine::with_host(Arc::new(Host::default()))
    }

    /// An engine sharing `host` with every other engine of the same run.
    pub fn with_host(host: Arc<Host>) -> Engine {
        let context = Context {
            host: host.clone(),
            refresh: Arc::new(Mutex::new(None)),
        };
        let mut state = nu_cmd_lang::create_default_context();
        let delta = {
            let mut working_set = StateWorkingSet::new(&state);
            bind_data_commands(&mut working_set);
            commands::bind(&mut working_set, &context);
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
        // Some nushell commands expect a working directory. Pipes have no
        // filesystem access, so the root is a neutral choice that exists on
        // every platform.
        state.add_env_var("PWD".into(), Value::string("/", Span::unknown()));
        // A timeout sets the host's flag, which stops every engine of the run.
        state.set_signals(Signals::new(host.interrupt()));
        Engine {
            state,
            stack: Stack::new(),
            warnings: Vec::new(),
            context,
        }
    }

    /// The host this engine shares with the rest of its run.
    pub fn host(&self) -> &Arc<Host> {
        &self.context.host
    }

    /// Sets the running pipe's `refresh`, which `http get` uses to decide
    /// whether a cached response is fresh.
    pub fn set_refresh(&self, refresh: Option<SignedDuration>) {
        *self.context.refresh.lock().expect("refresh lock") = refresh;
    }

    /// Parses and runs `source` with `input` as its pipeline input, and
    /// collects the result. Definitions and `use`s persist for later calls.
    pub fn eval(&mut self, source: &str, input: Value) -> Result<Value, EngineError> {
        let block = {
            let mut working_set = StateWorkingSet::new(&self.state);
            // `parse` would reuse a cached block for repeated source text, and
            // that block comes back without its compiled form.
            let block =
                nu_parser::parse_fresh(&mut working_set, Some("source"), source.as_bytes(), false);
            if let Some(err) = working_set.parse_errors.first() {
                return Err(EngineError(format_cli_error(None, &working_set, err, None)));
            }
            if let Some(err) = working_set.compile_errors.first() {
                return Err(EngineError(format_cli_error(None, &working_set, err, None)));
            }
            // nushell formats warnings and errors the same way; only the
            // leading word differs.
            for warning in &working_set.parse_warnings {
                let text = format_cli_error(None, &working_set, warning, None);
                self.warnings.push(text.replacen("Error: ", "Warning: ", 1));
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

    /// Returns, and forgets, the warnings nushell reported while parsing
    /// since the last call (deprecations, for example), rendered.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
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
/// no plugins. Network access is our own `http get` (in `commands`).
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
        StrLowercase,
        StrEndswith,
        StrJoin,
        StrLength,
        StrReplace,
        StrStartsWith,
        StrSubstring,
        StrTrim,
        StrUppercase,
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
        // URLs
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

use std::sync::{Arc, Mutex};

use jiff::SignedDuration;
use nu_protocol::engine::StateWorkingSet;

use crate::host::Host;

pub mod expand_rrule;
mod from_ics;
mod http_get;
mod pipe_get;

pub use expand_rrule::ExpandRrule;
pub use from_ics::FromIcs;
pub use http_get::HttpGet;
pub use pipe_get::PipeGet;

/// What our commands share with the engine that runs them.
#[derive(Clone)]
pub struct Context {
    pub host: Arc<Host>,
    /// The running pipe's `refresh`, set when the pipe is loaded.
    pub refresh: Arc<Mutex<Option<SignedDuration>>>,
}

/// Registers every command in this module.
pub fn bind(working_set: &mut StateWorkingSet, context: &Context) {
    working_set.add_decl(Box::new(ExpandRrule));
    working_set.add_decl(Box::new(FromIcs));
    working_set.add_decl(Box::new(HttpGet {
        context: context.clone(),
    }));
    working_set.add_decl(Box::new(PipeGet {
        context: context.clone(),
    }));
}
```

`crates/ppp-core/src/commands/http_get.rs`:

```rust
//! `http get`: our replacement for nushell's, through the cache and limits.

use nu_engine::command_prelude::*;
use nu_protocol::shell_error::generic::GenericError;

use super::Context;
use crate::http::fetch;

#[derive(Clone)]
pub struct HttpGet {
    pub context: Context,
}

impl Command for HttpGet {
    fn name(&self) -> &str {
        "http get"
    }

    fn description(&self) -> &str {
        "Fetch a URL, using the run's HTTP cache."
    }

    fn extra_description(&self) -> &str {
        "A JSON response (by Content-Type) is parsed unless --raw is given; anything else is \
         returned as text when it is UTF-8, or as binary. A cached copy is used while it is \
         fresh by the pipe's `refresh` or the response's Cache-Control, and when the network \
         fails."
    }

    fn signature(&self) -> Signature {
        Signature::build("http get")
            .input_output_types(vec![(Type::Nothing, Type::Any)])
            .required("url", SyntaxShape::String, "The http(s) URL to fetch")
            .switch("raw", "Return the body without parsing JSON", None)
            .category(Category::Network)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let url: Spanned<String> = call.req(engine_state, stack, 0)?;
        let raw = call.has_flag(engine_state, stack, "raw")?;
        if !(url.item.starts_with("http://") || url.item.starts_with("https://")) {
            return Err(ShellError::Generic(GenericError::new(
                "only http and https URLs can be fetched",
                "not an http(s) URL",
                url.span,
            )));
        }
        let refresh = *self.context.refresh.lock().expect("refresh lock");
        let fetched = fetch(&self.context.host, &url.item, refresh).map_err(|err| {
            ShellError::Generic(GenericError::new("cannot fetch", err.to_string(), url.span))
        })?;
        let is_json = fetched
            .content_type
            .as_deref()
            .is_some_and(|t| t.contains("json"));
        let value = if is_json && !raw {
            let json: serde_json::Value = serde_json::from_slice(&fetched.body).map_err(|err| {
                ShellError::Generic(GenericError::new(
                    "cannot parse JSON response",
                    err.to_string(),
                    url.span,
                ))
            })?;
            json_to_value(json, head)
        } else {
            match String::from_utf8(fetched.body) {
                Ok(text) => Value::string(text, head),
                Err(err) => Value::binary(err.into_bytes(), head),
            }
        };
        Ok(value.into_pipeline_data())
    }
}

fn json_to_value(json: serde_json::Value, span: Span) -> Value {
    match json {
        serde_json::Value::Null => Value::nothing(span),
        serde_json::Value::Bool(b) => Value::bool(b, span),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::int(i, span),
            None => Value::float(n.as_f64().expect("JSON numbers convert to f64"), span),
        },
        serde_json::Value::String(s) => Value::string(s, span),
        serde_json::Value::Array(items) => Value::list(
            items
                .into_iter()
                .map(|item| json_to_value(item, span))
                .collect(),
            span,
        ),
        serde_json::Value::Object(map) => {
            let mut record = Record::new();
            for (key, value) in map {
                record.push(key, json_to_value(value, span));
            }
            Value::record(record, span)
        }
    }
}
```

`crates/ppp-core/src/commands/pipe_get.rs`:

```rust
//! `pipe get`: another pipe's checked records, as nushell values (spec §5.5).

use nu_engine::command_prelude::*;
use nu_protocol::shell_error::generic::GenericError;

use super::Context;
use crate::pipe::{Pipe, check_records};

#[derive(Clone)]
pub struct PipeGet {
    pub context: Context,
}

/// How many of another pipe's failures to show in the error.
const SHOWN: usize = 5;

impl Command for PipeGet {
    fn name(&self) -> &str {
        "pipe get"
    }

    fn description(&self) -> &str {
        "Run another pipe and return its records, checked against its declared types."
    }

    fn extra_description(&self) -> &str {
        "The pipe is looked up as <name>.nu in the run's pipes directory, runs in its own \
         sandboxed engine sharing this run's cache and timeout, and returns nushell values \
         (datetimes stay datetimes). A pipe that, directly or not, gets itself is an error."
    }

    fn signature(&self) -> Signature {
        Signature::build("pipe get")
            .input_output_types(vec![(Type::Nothing, Type::table())])
            .required("name", SyntaxShape::String, "The pipe's name")
            .optional(
                "args",
                SyntaxShape::Record(Default::default()),
                "The pipe's flags, e.g. {within: 3day}",
            )
            .category(Category::Custom("pipes".into()))
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let name: Spanned<String> = call.req(engine_state, stack, 0)?;
        let args: Option<Record> = call.opt(engine_state, stack, 1)?;
        let fail = |message: String| {
            ShellError::Generic(GenericError::new(
                format!("pipe get {}", name.item),
                message,
                name.span,
            ))
        };
        let host = &self.context.host;
        let path = host.config.pipes_dir.join(format!("{}.nu", name.item));
        let mut pipe = Pipe::load_with(&path, host.clone()).map_err(|e| fail(e.to_string()))?;
        let records = pipe.run(&args.unwrap_or_default());
        for warning in pipe.take_warnings() {
            host.warn(warning);
        }
        let records = records.map_err(|e| fail(e.to_string()))?;
        let registry = host.registry().map_err(|e| fail(e.to_string()))?;
        if let Err(errors) = check_records(&registry, pipe.info(), &records) {
            let mut lines: Vec<String> = errors
                .iter()
                .take(SHOWN)
                .map(|e| format!("record {} {}: {}", e.index, e.pointer, e.message))
                .collect();
            if errors.len() > SHOWN {
                lines.push(format!("…and {} more", errors.len() - SHOWN));
            }
            return Err(fail(format!(
                "its records failed the check:\n{}",
                lines.join("\n")
            )));
        }
        Ok(Value::list(records, head).into_pipeline_data())
    }
}
```

`crates/ppp-core/src/pipe.rs`:

```rust
//! Loading a pipe file, reading its metadata, running it, and checking its
//! output at the boundary (spec §5.1).

use std::path::Path;
use std::sync::Arc;

use jiff::SignedDuration;
use nu_protocol::{Record, Span, Value};
use serde_json::Value as Json;

use crate::engine::{Engine, EngineError};
use crate::host::Host;
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
    #[error("pipes get each other in a loop: {}", .0.join(" → "))]
    Cycle(Vec<String>),
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
    /// How long a fetched response stays fresh, from `pipe.refresh`.
    pub refresh: Option<SignedDuration>,
    /// From `@category`.
    pub category: String,
    /// From `@search-terms`.
    pub search_terms: Vec<String>,
    /// From `@example`.
    pub examples: Vec<Example>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub description: String,
    pub example: String,
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
    /// Loads the pipe at `path` with a default `Host` (no cache).
    pub fn load(path: &Path) -> Result<Pipe, PipeError> {
        Pipe::load_with(path, Arc::new(Host::default()))
    }

    /// Loads the pipe at `path` into a fresh engine sharing `host`, and reads
    /// its metadata. Nothing in the pipe runs except its constant definitions.
    pub fn load_with(path: &Path, host: Arc<Host>) -> Result<Pipe, PipeError> {
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
        // The name becomes the command to call and `$<name>.pipe`, both
        // spliced into nushell source, so it must be a plain identifier.
        let plain = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if name.is_empty() || !name.chars().all(plain) {
            return Err(metadata(format!(
                "pipe file names may use only letters, digits, `-` and `_`, not \"{name}\""
            )));
        }
        let mut engine = Engine::with_host(host);
        engine.eval(
            &format!("use {}", nu_string(&full.to_string_lossy())),
            Value::nothing(Span::unknown()),
        )?;

        let found = engine.eval(
            &format!("scope commands | where name == {} | select description extra_description category search_terms examples signatures", nu_string(&name)),
            Value::nothing(Span::unknown()),
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
                Value::nothing(Span::unknown()),
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

        let refresh = match engine.eval(
            &format!("{constant}.refresh? | default null"),
            Value::nothing(Span::unknown()),
        )? {
            Value::Duration { val, .. } => Some(SignedDuration::from_nanos(val)),
            Value::Nothing { .. } => None,
            _ => {
                return Err(metadata(
                    "`pipe.refresh` must be a duration, e.g. 1hr".into(),
                ));
            }
        };
        engine.set_refresh(refresh);
        let search_terms = field("search_terms")
            .split(',')
            .map(|term| term.trim().to_string())
            .filter(|term| !term.is_empty())
            .collect();
        let examples = match command.get_data_by_key("examples") {
            Some(Value::List { vals, .. }) => vals
                .iter()
                .map(|example| {
                    let text = |key: &str| {
                        example
                            .get_data_by_key(key)
                            .and_then(|v| v.coerce_into_string().ok())
                            .unwrap_or_default()
                    };
                    Example {
                        description: text("description"),
                        example: text("example"),
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        let info = PipeInfo {
            name,
            title: field("description"),
            description: field("extra_description"),
            params,
            output,
            refresh,
            category: field("category"),
            search_terms,
            examples,
        };
        Ok(Pipe { engine, info })
    }

    pub fn info(&self) -> &PipeInfo {
        &self.info
    }

    /// Returns, and forgets, nushell's warnings from loading and running
    /// the pipe (deprecations, for example), rendered.
    pub fn take_warnings(&mut self) -> Vec<String> {
        self.engine.take_warnings()
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
        self.call(&call, Value::record(args.clone(), Span::unknown()))
    }

    /// Runs `main` with command-line arguments, the way `nu <pipe>.nu
    /// <args>` does: each argument is quoted as needed and nushell's parser
    /// checks them against `main`'s signature. For a CLI only; embedders
    /// pass typed values to `run`.
    pub fn run_command_line(&mut self, args: &[String]) -> Result<Vec<Value>, PipeError> {
        let mut call = self.info.name.clone();
        for arg in args {
            call.push(' ');
            call.push_str(&nu_parser::escape_for_script_arg(arg));
        }
        self.call(&call, Value::nothing(Span::unknown()))
    }

    fn call(&mut self, source: &str, input: Value) -> Result<Vec<Value>, PipeError> {
        let host = self.engine.host().clone();
        let _running = host.enter(&self.info.name).map_err(PipeError::Cycle)?;
        Ok(match self.engine.eval(source, input)? {
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

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: every suite passes; `host` 10.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add crates/ppp-core fixtures/pipes
git commit -m "Add http get with the cache, pipe get, timeouts and pipe metadata

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---

### Task 3: `ppp run` options and `ppp info`

**Files:** Modify `crates/ppp-cli/Cargo.toml`, `crates/ppp-cli/src/main.rs`, `crates/ppp-cli/src/run.rs`, `crates/ppp-cli/tests/run.rs`; Create `crates/ppp-cli/src/info.rs`; Test `crates/ppp-cli/tests/info.rs`.

**Interfaces:** Consumes Tasks 1–2. Produces `ppp run [--pipes-dir] [--cache-dir DIR | --no-cache] [--timeout SECS] <pipe> [flags…]` and `ppp info [--pipes-dir] <pipe>`.

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
    let out = ppp(&["run", "--no-cache", "campus-week", "--within", "3day"]);
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

#[test]
fn a_pipe_past_its_timeout_is_stopped() {
    let started = std::time::Instant::now();
    let out = ppp(&["run", "--timeout", "1", "fixtures/pipes/slow.nu"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(text(&out.stdout), "");
    assert_eq!(
        text(&out.stderr),
        "ppp: slow ran longer than 1s and was stopped\n"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn pipe_get_finds_pipes_in_the_pipes_dir() {
    let out = ppp(&["run", "--pipes-dir", "fixtures/pipes", "gets-two-events"]);
    let events = records(&out);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["summary"], "Got: Chess Club");
}

#[test]
fn cache_dir_and_no_cache_conflict() {
    let out = ppp(&[
        "run",
        "--no-cache",
        "--cache-dir",
        "x",
        "fixtures/pipes/two-events.nu",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("cannot be used with"),
        "{}",
        text(&out.stderr)
    );
}
```

`crates/ppp-cli/tests/info.rs`:

```rust
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ppp-cli`
Expected: the new `run` tests and every `info` test FAIL (clap rejects `info`, `--timeout`, `--no-cache`).

- [ ] **Step 3: Write the code**

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
dirs.workspace = true
ppp-core = { path = "../ppp-core" }
serde_json.workspace = true
```

`crates/ppp-cli/src/main.rs`:

```rust
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

mod info;
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
        #[command(flatten)]
        options: run::Options,
        /// A pipe name (looked up as <pipes-dir>/<name>.nu) or a path to a .nu file
        pipe: String,
        /// The pipe's own flags, e.g. --within 3day
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Print what a pipe declares (title, flags, output types) as JSON, without running it
    Info {
        #[command(flatten)]
        lookup: Lookup,
        /// A pipe name or a path to a .nu file
        pipe: String,
    },
    /// Check a JSON array or JSONL file of records against the stdlib schemas
    Validate {
        /// A `.json` file holding an array of records, or a `.jsonl` file with one record per line
        file: PathBuf,
    },
}

/// Where pipe names are looked up.
#[derive(Args, Clone)]
pub struct Lookup {
    /// Directory that pipe names are looked up in
    #[arg(long, default_value = "pipes")]
    pub pipes_dir: PathBuf,
}

impl Lookup {
    /// A pipe name is looked up as `<pipes-dir>/<name>.nu`; anything with a
    /// `/` or ending in `.nu` is a path.
    pub fn path(&self, pipe: &str) -> PathBuf {
        if pipe.contains('/') || pipe.ends_with(".nu") {
            PathBuf::from(pipe)
        } else {
            self.pipes_dir.join(format!("{pipe}.nu"))
        }
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Run {
            options,
            pipe,
            args,
        } => run::run(&options, &pipe, &args),
        Command::Info { lookup, pipe } => info::run(&lookup, &pipe),
        Command::Validate { file } => validate::run(&file),
    }
}
```

`crates/ppp-cli/src/run.rs`:

```rust
//! `ppp run`: runs a pipe and prints its records as a JSON array.
//!
//! Exit codes: 0 when the pipe ran and every record is valid, 1 when any
//! record fails the boundary check, 2 when the pipe cannot be loaded or
//! run, or runs past `--timeout`. Flags are checked by nushell, as for
//! `nu <pipe>.nu <flags>`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Args;
use ppp_core::host::{Host, HostConfig};
use ppp_core::pipe::{Pipe, check_records};

use crate::Lookup;
use crate::report::{Numbering, print_errors};

#[derive(Args)]
pub struct Options {
    #[command(flatten)]
    lookup: Lookup,
    /// Where fetched responses are cached [default: the user cache directory]
    #[arg(long, conflicts_with = "no_cache")]
    cache_dir: Option<PathBuf>,
    /// Fetch everything, keeping nothing
    #[arg(long)]
    no_cache: bool,
    /// Stop the pipe after this many seconds
    #[arg(long, default_value_t = 60)]
    timeout: u64,
}

pub fn run(options: &Options, pipe: &str, args: &[String]) -> ExitCode {
    let host = Arc::new(Host::new(HostConfig {
        cache_dir: cache_dir(options),
        pipes_dir: options.lookup.pipes_dir.clone(),
        ..HostConfig::default()
    }));
    let code = match try_run(options, &host, pipe, args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    };
    for warning in host.take_warnings() {
        eprintln!("{warning}");
    }
    code
}

fn cache_dir(options: &Options) -> Option<PathBuf> {
    if options.no_cache {
        return None;
    }
    options
        .cache_dir
        .clone()
        .or_else(|| dirs::cache_dir().map(|dir| dir.join("ppp-pipes").join("http")))
}

fn try_run(
    options: &Options,
    host: &Arc<Host>,
    pipe: &str,
    args: &[String],
) -> Result<ExitCode, String> {
    let path = options.lookup.path(pipe);
    let mut loaded = Pipe::load_with(&path, host.clone()).map_err(|e| format!("ppp: {e}"))?;
    let limit = Duration::from_secs(options.timeout);
    let records = host.with_timeout(limit, || loaded.run_command_line(args));
    for warning in loaded.take_warnings() {
        eprintln!("{warning}");
    }
    let records = records.map_err(|e| {
        if host.interrupted() {
            format!(
                "ppp: {} ran longer than {}s and was stopped",
                loaded.info().name,
                options.timeout
            )
        } else {
            format!("ppp: {e}")
        }
    })?;
    let registry = host
        .registry()
        .map_err(|e| format!("ppp: stdlib schemas: {e}"))?;
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
```

`crates/ppp-cli/src/info.rs`:

```rust
//! `ppp info`: what a pipe declares, as JSON, without running it. This is
//! the metadata a UI turns into a form (spec §4.4).

use std::process::ExitCode;

use ppp_core::pipe::{Pipe, PipeInfo};
use ppp_core::serialize::record_to_json;
use ppp_core::time_fields::TimeTree;
use serde_json::{Value as Json, json};

use crate::Lookup;

pub fn run(lookup: &Lookup, pipe: &str) -> ExitCode {
    match Pipe::load(&lookup.path(pipe)) {
        Ok(loaded) => {
            let text = serde_json::to_string_pretty(&to_json(loaded.info()))
                .expect("JSON values always serialize");
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("ppp: {err}");
            ExitCode::from(2)
        }
    }
}

fn to_json(info: &PipeInfo) -> Json {
    let params: Vec<Json> = info
        .params
        .iter()
        .map(|p| {
            let default = p.default.as_ref().map_or(Json::Null, |value| {
                record_to_json(value, &TimeTree::default()).unwrap_or(Json::Null)
            });
            json!({ "name": p.name, "type": p.shape, "description": p.description, "default": default })
        })
        .collect();
    let examples: Vec<Json> = info
        .examples
        .iter()
        .map(|e| json!({ "description": e.description, "example": e.example }))
        .collect();
    json!({
        "name": info.name,
        "title": info.title,
        "description": info.description,
        "category": info.category,
        "search_terms": info.search_terms,
        "examples": examples,
        "params": params,
        "output": info.output,
        "refresh": info.refresh.map(|r| r.to_string()),
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: every suite passes, 168 tests; then `cargo test -p ppp-cli --test run -- --ignored` passes against the live feed.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets` — expect no output.

```bash
git add crates/ppp-cli Cargo.lock
git commit -m "Add ppp run's cache and timeout options and ppp info

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_011RdJ8A2KKQKPcdnzTqtMDu"
```

---


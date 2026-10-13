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
        body_sha256: None,
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
    assert_eq!(
        max_age("max-age=\"60\""),
        Some(SignedDuration::from_secs(60))
    );
    assert_eq!(max_age("no-store"), None);
    assert_eq!(max_age("private"), None);
}

#[test]
fn the_size_cap_counts_decompressed_bytes() {
    use std::io::Write;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(&vec![b'x'; 100_000]).unwrap();
    let body = gz.finish().unwrap();
    assert!(body.len() < 1_000);
    let server = serve(vec![support::Reply {
        status: "200 OK",
        headers: vec![("Content-Encoding", "gzip".to_string())],
        body,
    }]);
    let host = Host::new(HostConfig {
        max_response_bytes: 10_000,
        ..HostConfig::default()
    });
    assert!(matches!(
        fetch(&host, &server.url, None),
        Err(FetchError::TooLarge { limit: 10_000, .. })
    ));
}

#[test]
fn concurrent_writers_never_pair_a_body_with_the_wrong_metadata() {
    use ppp_core::cache::Cache;
    let dir = temp_dir("cache-race");
    let url = "https://example.edu/feed";
    let writers: Vec<_> = (0..8)
        .map(|n| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                let cache = Cache::new(&dir);
                for round in 0..50 {
                    let body = format!("body {n} {round} {}", "x".repeat(n * 100));
                    let meta = Meta {
                        url: url.into(),
                        fetched: Timestamp::now(),
                        content_type: Some(format!("text/plain; writer={n}; round={round}")),
                        etag: None,
                        last_modified: None,
                        max_age: None,
                        body_sha256: None,
                    };
                    cache.put(&meta, body.as_bytes()).unwrap();
                }
            })
        })
        .collect();
    let reader = Cache::new(&dir);
    for _ in 0..500 {
        if let Some((meta, body)) = reader.get(url) {
            let body = String::from_utf8(body).unwrap();
            let tag = meta.content_type.unwrap();
            let (n, round) = (
                tag.split("writer=").nth(1).unwrap(),
                tag.split("round=").nth(1).unwrap(),
            );
            let n = n.split(';').next().unwrap();
            assert!(
                body.starts_with(&format!("body {n} {round} ")),
                "{tag} paired with {body:.20}"
            );
        }
    }
    for writer in writers {
        writer.join().unwrap();
    }
}

#[test]
fn the_run_timeout_cuts_a_stuck_request_short() {
    let url = support::serve_silence();
    let host = Host::default();
    let started = std::time::Instant::now();
    let result = host.with_timeout(std::time::Duration::from_secs(1), || {
        fetch(&host, &url, None)
    });
    assert!(
        matches!(result, Err(FetchError::Network { .. })),
        "{result:?}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn nothing_is_fetched_after_the_run_timed_out() {
    let server = serve(vec![reply("200 OK", &[], "late")]);
    let host = Host::default();
    let result = host.with_timeout(std::time::Duration::from_millis(10), || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        fetch(&host, &server.url, None)
    });
    assert!(matches!(result, Err(FetchError::Network { .. })));
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn the_default_cache_is_under_the_user_cache_dir() {
    let dir = ppp_core::cache::default_dir().unwrap();
    assert!(dir.ends_with("ppp-pipes/http"), "{}", dir.display());
}

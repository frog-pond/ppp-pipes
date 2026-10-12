//! Fetching over HTTP for `http get`, through the cache (spec §4.1, §8.1).

use std::io::Read;

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

    if host.interrupted() {
        return Err(FetchError::Network {
            url: url.to_string(),
            message: "the run's time limit has passed".into(),
        });
    }
    // A request may not outlast the run's time limit.
    let timeout = match host.time_left() {
        Some(left) => left.min(host.config.http_timeout),
        None => host.config.http_timeout,
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
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
            body_sha256: None,
        };
        let status = response.status().as_u16();
        // The cap counts decoded bytes: ureq's own limit counts the bytes
        // on the wire, so a small gzip body could expand without bound.
        let limit = host.config.max_response_bytes;
        let mut body = Vec::new();
        response
            .body_mut()
            .with_config()
            .reader()
            .take(limit.saturating_add(1))
            .read_to_end(&mut body)
            .map_err(|err| FetchError::Network {
                url: url.to_string(),
                message: err.to_string(),
            })?;
        if body.len() as u64 > limit {
            return Err(FetchError::TooLarge {
                url: url.to_string(),
                limit,
            });
        }
        Ok((status, meta, body))
    });

    match (result, cached) {
        (Ok((304, fresh, _)), Some((old, body))) => {
            // A 304 updates only the headers it sends (RFC 9111 §4.3.4).
            let meta = Meta {
                content_type: old.content_type.clone(),
                etag: fresh.etag.or(old.etag),
                last_modified: fresh.last_modified.or(old.last_modified),
                max_age: fresh.max_age.or(old.max_age),
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

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

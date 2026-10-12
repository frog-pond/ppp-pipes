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

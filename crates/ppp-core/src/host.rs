//! What every engine in one run shares: where the HTTP cache lives, where
//! `pipe get` finds pipes, the limits, and the flag a timeout sets.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

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
    timed_out: AtomicBool,
    deadline: Mutex<Option<Instant>>,
    /// Each running pipe's file and name, outermost first.
    running: Mutex<Vec<(PathBuf, String)>>,
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

    /// This host, using `registry` instead of compiling its own. A
    /// long-lived embedder (the plugin) compiles the stdlib once and hands
    /// it to each run's host.
    pub fn with_registry(self, registry: Arc<Registry>) -> Host {
        let _ = self.registry.set(registry);
        self
    }

    /// The flag every engine of this host checks; setting it stops them.
    pub fn interrupt(&self) -> Arc<AtomicBool> {
        self.interrupt.clone()
    }

    /// Whether the interrupt flag is set: the running pipe must stop.
    pub fn interrupted(&self) -> bool {
        self.interrupt.load(Ordering::Relaxed)
    }

    /// Whether the last run under `with_timeout` ran out of time.
    pub fn timed_out(&self) -> bool {
        self.timed_out.load(Ordering::Relaxed)
    }

    /// Runs `work`, setting the interrupt flag if it takes longer than
    /// `limit`. Nushell checks the flag between steps, so a stuck pipe
    /// stops with an "interrupted" error. Once `work` returns the flag and
    /// the deadline are cleared, so the host can run again.
    pub fn with_timeout<T>(&self, limit: Duration, work: impl FnOnce() -> T) -> T {
        *self.deadline.lock().expect("deadline lock") = Some(Instant::now() + limit);
        let (done, wait) = mpsc::channel::<()>();
        let flag = self.interrupt.clone();
        let watchdog = thread::spawn(move || {
            let fired = matches!(wait.recv_timeout(limit), Err(RecvTimeoutError::Timeout));
            if fired {
                flag.store(true, Ordering::Relaxed);
            }
            fired
        });
        let result = work();
        let _ = done.send(());
        let fired = watchdog.join().expect("the watchdog does not panic");
        self.timed_out.store(fired, Ordering::Relaxed);
        self.interrupt.store(false, Ordering::Relaxed);
        *self.deadline.lock().expect("deadline lock") = None;
        result
    }

    /// How long the run has left, if it has a time limit. Zero once the
    /// limit has passed.
    pub fn time_left(&self) -> Option<Duration> {
        let deadline = *self.deadline.lock().expect("deadline lock");
        deadline.map(|at| at.saturating_duration_since(Instant::now()))
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

    /// Marks the pipe in `file` as running for the length of the returned
    /// guard, or returns the chain of pipe names if that file is already
    /// running. Two files may share a name, so the file is what counts.
    pub fn enter(self: &Arc<Self>, file: &Path, name: &str) -> Result<Running, Vec<String>> {
        let mut running = self.running.lock().expect("running lock");
        if running.iter().any(|(f, _)| f == file) {
            let mut chain: Vec<String> = running.iter().map(|(_, n)| n.clone()).collect();
            chain.push(name.to_string());
            return Err(chain);
        }
        running.push((file.to_path_buf(), name.to_string()));
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

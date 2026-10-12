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
        .or_else(ppp_core::cache::default_dir)
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

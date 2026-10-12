//! `ppp run`: runs a pipe and prints its records as a JSON array.
//!
//! Exit codes: 0 when the pipe ran and every record is valid, 1 when any
//! record fails the boundary check, 2 when the pipe cannot be loaded or
//! run. Flags are checked by nushell, as for `nu <pipe>.nu <flags>`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ppp_core::pipe::{Pipe, check_records};
use ppp_core::schema::Registry;

use crate::report::{Numbering, print_errors};

pub fn run(pipe: &str, pipes_dir: &Path, args: &[String]) -> ExitCode {
    match try_run(pipe, pipes_dir, args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn try_run(pipe: &str, pipes_dir: &Path, args: &[String]) -> Result<ExitCode, String> {
    let path = pipe_path(pipe, pipes_dir);
    let mut loaded = Pipe::load(&path).map_err(|e| format!("ppp: {e}"))?;
    let records = loaded
        .run_command_line(args)
        .map_err(|e| format!("ppp: {e}"))?;
    for warning in loaded.take_warnings() {
        eprintln!("{warning}");
    }
    let registry = Registry::stdlib().map_err(|e| format!("ppp: stdlib schemas: {e}"))?;
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

/// A pipe name is looked up as `<pipes_dir>/<name>.nu`; anything with a `/`
/// or ending in `.nu` is a path.
fn pipe_path(pipe: &str, pipes_dir: &Path) -> PathBuf {
    if pipe.contains('/') || pipe.ends_with(".nu") {
        PathBuf::from(pipe)
    } else {
        pipes_dir.join(format!("{pipe}.nu"))
    }
}

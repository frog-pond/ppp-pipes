use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
        /// Directory that pipe names are looked up in
        #[arg(long, default_value = "pipes")]
        pipes_dir: PathBuf,
        /// A pipe name (looked up as <pipes-dir>/<name>.nu) or a path to a .nu file
        pipe: String,
        /// The pipe's own flags, e.g. --days 3
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Check a JSON array or JSONL file of records against the stdlib schemas
    Validate {
        /// A `.json` file holding an array of records, or a `.jsonl` file with one record per line
        file: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Run {
            pipes_dir,
            pipe,
            args,
        } => run::run(&pipe, &pipes_dir, &args),
        Command::Validate { file } => validate::run(&file),
    }
}

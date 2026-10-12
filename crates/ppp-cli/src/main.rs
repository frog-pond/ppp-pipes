use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
    /// Check a JSON array or JSONL file of records against the stdlib schemas
    Validate {
        /// A `.json` file holding an array of records, or a `.jsonl` file with one record per line
        file: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Validate { file } => validate::run(&file),
    }
}

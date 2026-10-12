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

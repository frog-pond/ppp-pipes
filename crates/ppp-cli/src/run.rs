//! `ppp run`: runs a pipe and prints its records as a JSON array.
//!
//! Exit codes: 0 when the pipe ran and every record is valid, 1 when any
//! record fails the boundary check, 2 when the pipe cannot be loaded or
//! run, or its flags are wrong.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nu_protocol::{Record, Span, Value};
use ppp_core::pipe::{Param, Pipe, check_records};
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
    let flags = parse_flags(loaded.info().name.as_str(), &loaded.info().params, args)?;
    let records = loaded.run(&flags).map_err(|e| format!("ppp: {e}"))?;
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

/// Reads `--flag value`, `--flag=value` and bare `--switch` arguments into
/// typed values, using the pipe's declared flags.
fn parse_flags(pipe: &str, params: &[Param], args: &[String]) -> Result<Record, String> {
    let usage = || {
        let flags: Vec<String> = params
            .iter()
            .map(|p| {
                if p.shape == "switch" {
                    format!("--{}", p.name)
                } else {
                    format!("--{} <{}>", p.name, p.shape)
                }
            })
            .collect();
        if flags.is_empty() {
            format!("{pipe} takes no flags")
        } else {
            format!("{pipe} takes: {}", flags.join(", "))
        }
    };
    let mut record = Record::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let Some(flag) = arg.strip_prefix("--") else {
            return Err(format!("ppp: unexpected argument \"{arg}\"; {}", usage()));
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (flag, None),
        };
        let param = params
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("ppp: {pipe} has no flag --{name}; {}", usage()))?;
        if param.shape == "switch" {
            record.push(name, Value::bool(true, Span::unknown()));
            continue;
        }
        let text = match inline {
            Some(text) => text,
            None => rest
                .next()
                .cloned()
                .ok_or_else(|| format!("ppp: --{name} needs a value"))?,
        };
        record.push(name, typed_value(param, &text)?);
    }
    Ok(record)
}

fn typed_value(param: &Param, text: &str) -> Result<Value, String> {
    let span = Span::unknown();
    let bad = || {
        format!(
            "ppp: --{} expects {}, got \"{text}\"",
            param.name, param.shape
        )
    };
    Ok(match param.shape.as_str() {
        "string" => Value::string(text, span),
        "int" => Value::int(text.parse().map_err(|_| bad())?, span),
        "float" => Value::float(text.parse().map_err(|_| bad())?, span),
        "number" => match text.parse::<i64>() {
            Ok(n) => Value::int(n, span),
            Err(_) => Value::float(text.parse().map_err(|_| bad())?, span),
        },
        "bool" => Value::bool(text.parse().map_err(|_| bad())?, span),
        other => {
            return Err(format!(
                "ppp: --{} has type {other}, which can't be given on the command line yet",
                param.name
            ));
        }
    })
}

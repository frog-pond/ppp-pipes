//! `ppp validate`: checks a file of records against the stdlib schemas.
//!
//! Exit codes: 0 when every record is valid, 1 when any record is invalid,
//! 2 when the file cannot be read or parsed.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use ppp_core::schema::{RecordError, Registry};
use serde_json::Value as Json;

/// How many failures to print before summarizing the rest.
const SHOWN: usize = 20;

pub fn run(file: &Path) -> ExitCode {
    let (records, unit) = match read_records(file) {
        Ok(found) => found,
        Err(message) => {
            eprintln!("ppp: {}: {message}", file.display());
            return ExitCode::from(2);
        }
    };
    let registry = match Registry::stdlib() {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!("ppp: stdlib schemas: {e}");
            return ExitCode::from(2);
        }
    };

    let errors: Vec<RecordError> = records
        .iter()
        .enumerate()
        .flat_map(|(i, record)| registry.validate(i, record))
        .collect();

    if errors.is_empty() {
        println!(
            "{}: {} valid",
            file.display(),
            count(records.len(), "record")
        );
        return ExitCode::SUCCESS;
    }
    for e in errors.iter().take(SHOWN) {
        eprintln!("{}: {}", file.display(), describe(e, unit));
    }
    if errors.len() > SHOWN {
        eprintln!("{}: …and {} more", file.display(), errors.len() - SHOWN);
    }
    eprintln!(
        "{}: {} in {}",
        file.display(),
        count(errors.len(), "error"),
        count(records.len(), "record")
    );
    ExitCode::from(1)
}

/// How records are numbered in messages.
#[derive(Clone, Copy)]
enum Unit {
    /// 0-based position in a JSON array.
    Record,
    /// 1-based line in a JSONL file.
    Line,
}

fn read_records(file: &Path) -> Result<(Vec<Json>, Unit), String> {
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    if file.extension().is_some_and(|ext| ext == "jsonl") {
        let mut records = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            records.push(serde_json::from_str(line).map_err(|e| format!("line {}: {e}", i + 1))?);
        }
        return Ok((records, Unit::Line));
    }
    match serde_json::from_str(&text).map_err(|e| e.to_string())? {
        Json::Array(records) => Ok((records, Unit::Record)),
        _ => Err("expected a JSON array of records".to_string()),
    }
}

fn describe(e: &RecordError, unit: Unit) -> String {
    let place = match unit {
        Unit::Record => format!("record {}", e.index),
        Unit::Line => format!("line {}", e.index + 1),
    };
    let pointer = if e.pointer.is_empty() {
        String::new()
    } else {
        format!(" {}", e.pointer)
    };
    format!("{place}{pointer}: {}\n    value: {}", e.message, e.excerpt)
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

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
    let (records, numbering) = match read_records(file) {
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
        eprintln!("{}: {}", file.display(), describe(e, &numbering));
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
enum Numbering {
    /// 0-based position in a JSON array.
    Record,
    /// The 1-based line each record came from in a JSONL file.
    Lines(Vec<usize>),
}

fn read_records(file: &Path) -> Result<(Vec<Json>, Numbering), String> {
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    if file.extension().is_some_and(|ext| ext == "jsonl") {
        let mut records = Vec::new();
        let mut lines = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            records.push(serde_json::from_str(line).map_err(|e| format!("line {}: {e}", i + 1))?);
            lines.push(i + 1);
        }
        return Ok((records, Numbering::Lines(lines)));
    }
    match serde_json::from_str(&text).map_err(|e| e.to_string())? {
        Json::Array(records) => Ok((records, Numbering::Record)),
        _ => Err("expected a JSON array of records".to_string()),
    }
}

fn describe(e: &RecordError, numbering: &Numbering) -> String {
    let place = match numbering {
        Numbering::Record => format!("record {}", e.index),
        Numbering::Lines(lines) => format!("line {}", lines[e.index]),
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

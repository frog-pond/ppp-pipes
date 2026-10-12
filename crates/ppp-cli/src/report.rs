//! Printing validation failures, shared by `ppp validate` and `ppp run`.

use ppp_core::schema::RecordError;

/// How many failures to print before summarizing the rest.
const SHOWN: usize = 20;

/// How records are numbered in messages.
pub enum Numbering {
    /// 0-based position in a list.
    Record,
    /// The 1-based line each record came from in a JSONL file.
    Lines(Vec<usize>),
}

/// Prints up to `SHOWN` failures to stderr, each prefixed with `label`,
/// then a count line.
pub fn print_errors(label: &str, errors: &[RecordError], numbering: &Numbering, total: usize) {
    for e in errors.iter().take(SHOWN) {
        eprintln!("{label}: {}", describe(e, numbering));
    }
    if errors.len() > SHOWN {
        eprintln!("{label}: …and {} more", errors.len() - SHOWN);
    }
    eprintln!(
        "{label}: {} in {}",
        count(errors.len(), "error"),
        count(total, "record")
    );
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
    let value = if e.excerpt.is_empty() {
        String::new()
    } else {
        format!("\n    value: {}", e.excerpt)
    };
    format!("{place}{pointer}: {}{value}", e.message)
}

pub fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

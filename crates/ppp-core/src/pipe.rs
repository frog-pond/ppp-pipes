//! Loading a pipe file, reading its metadata, running it, and checking its
//! output at the boundary (spec §5.1).

use std::path::Path;

use nu_protocol::{Record, Span, Value};
use serde_json::Value as Json;

use crate::engine::{Engine, EngineError};
use crate::schema::{RecordError, Registry};
use crate::serialize::record_to_json;

#[derive(Debug, thiserror::Error)]
pub enum PipeError {
    #[error("{0}")]
    Engine(#[from] EngineError),
    #[error("{path}: {message}")]
    Metadata { path: String, message: String },
    #[error("{pipe} has no flag --{flag}")]
    UnknownFlag { pipe: String, flag: String },
}

/// What the host knows about a pipe without running it.
#[derive(Debug, Clone, PartialEq)]
pub struct PipeInfo {
    /// The command name: the file name without `.nu`.
    pub name: String,
    /// First line of `main`'s doc comment.
    pub title: String,
    /// The rest of `main`'s doc comment.
    pub description: String,
    /// `main`'s flags, in order.
    pub params: Vec<Param>,
    /// Record type URIs from `export const pipe = { output: [...] }`.
    pub output: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    /// Nushell's name for the flag's type, e.g. `int` or `string`. Switches
    /// (flags with no value) are `switch`.
    pub shape: String,
    pub description: String,
    pub default: Option<Value>,
}

pub struct Pipe {
    engine: Engine,
    info: PipeInfo,
}

impl Pipe {
    /// Loads the pipe at `path` into a fresh engine and reads its metadata.
    /// Nothing in the pipe runs except its constant definitions.
    pub fn load(path: &Path) -> Result<Pipe, PipeError> {
        let metadata = |message: String| PipeError::Metadata {
            path: path.display().to_string(),
            message,
        };
        let full = path.canonicalize().map_err(|e| metadata(e.to_string()))?;
        let name = full
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| metadata("file name is not valid UTF-8".into()))?
            .to_string();
        let mut engine = Engine::new();
        engine.eval(
            &format!("use {}", nu_string(&full.to_string_lossy())),
            Value::test_nothing(),
        )?;

        let found = engine.eval(
            &format!("scope commands | where name == {} | select description extra_description signatures", nu_string(&name)),
            Value::test_nothing(),
        )?;
        let command = found
            .as_list()
            .ok()
            .and_then(|rows| rows.first())
            .ok_or_else(|| metadata("the file does not export a `main` command".into()))?;
        let field = |key: &str| {
            command
                .get_data_by_key(key)
                .and_then(|v| v.coerce_into_string().ok())
                .unwrap_or_default()
        };
        let params = read_params(command).map_err(metadata)?;

        let constant = format!("${}.pipe", name.replace('-', "_"));
        let output = engine
            .eval(
                &format!("{constant}.output? | default []"),
                Value::test_nothing(),
            )
            .map_err(|_| metadata("the file has no `export const pipe = {...}`".into()))?;
        let output = output
            .as_list()
            .map_err(|_| metadata("`pipe.output` must be a list of record type URIs".into()))?
            .iter()
            .map(|v| {
                v.coerce_string()
                    .map_err(|_| metadata("`pipe.output` must be a list of strings".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if output.is_empty() {
            return Err(metadata(
                "`pipe.output` must name at least one record type".into(),
            ));
        }

        let info = PipeInfo {
            name,
            title: field("description"),
            description: field("extra_description"),
            params,
            output,
        };
        Ok(Pipe { engine, info })
    }

    pub fn info(&self) -> &PipeInfo {
        &self.info
    }

    /// Runs `main` with the given flag values and returns its records.
    /// Flags must be names from `info().params`; a switch is passed when its
    /// value is `true`.
    pub fn run(&mut self, args: &Record) -> Result<Vec<Value>, PipeError> {
        let mut call = format!("let args = $in; {}", self.info.name);
        for (flag, value) in args.iter() {
            let param = self
                .info
                .params
                .iter()
                .find(|p| &p.name == flag)
                .ok_or_else(|| PipeError::UnknownFlag {
                    pipe: self.info.name.clone(),
                    flag: flag.clone(),
                })?;
            if param.shape == "switch" {
                if value.as_bool().unwrap_or(false) {
                    call.push_str(&format!(" --{flag}"));
                }
            } else {
                call.push_str(&format!(" --{flag} $args.{flag}"));
            }
        }
        let output = self
            .engine
            .eval(&call, Value::record(args.clone(), Span::unknown()))?;
        Ok(match output {
            Value::List { vals, .. } => vals.to_vec(),
            Value::Nothing { .. } => Vec::new(),
            other => vec![other],
        })
    }
}

/// Checks a pipe's records at the boundary: each must have a type the pipe
/// declares, serialize for that type, and validate. Returns the JSON
/// records, or every failure.
pub fn check_records(
    registry: &Registry,
    info: &PipeInfo,
    records: &[Value],
) -> Result<Vec<Json>, Vec<RecordError>> {
    let mut out = Vec::with_capacity(records.len());
    let mut errors = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let type_name = record
            .get_data_by_key("type")
            .and_then(|v| v.coerce_string().ok());
        let declared = type_name
            .as_deref()
            .and_then(|name| registry.record_type(name))
            .filter(|record_type| info.output.contains(&record_type.uri));
        let Some(record_type) = declared else {
            let message = match type_name {
                Some(name) => format!(
                    "type \"{name}\" is not one of the pipe's output types: {}",
                    info.output.join(", ")
                ),
                None => "record has no \"type\"".to_string(),
            };
            errors.push(RecordError {
                index,
                pointer: "/type".into(),
                message,
                excerpt: String::new(),
            });
            continue;
        };
        match record_to_json(record, &record_type.time_fields) {
            Ok(json) => {
                errors.extend(registry.validate(index, &json));
                out.push(json);
            }
            Err(err) => errors.push(RecordError {
                index,
                pointer: String::new(),
                message: err.to_string(),
                excerpt: String::new(),
            }),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

fn read_params(command: &Value) -> Result<Vec<Param>, String> {
    let signatures = command
        .get_data_by_key("signatures")
        .and_then(|s| s.as_record().ok().cloned())
        .ok_or("cannot read `main`'s signature")?;
    let Some((_, rows)) = signatures.iter().next() else {
        return Ok(Vec::new());
    };
    let mut params = Vec::new();
    for row in rows.as_list().map_err(|e| e.to_string())? {
        let text = |key: &str| {
            row.get_data_by_key(key)
                .and_then(|v| v.coerce_string().ok())
                .unwrap_or_default()
        };
        match text("parameter_type").as_str() {
            "input" | "output" => {}
            "named" | "switch" if text("parameter_name") == "help" => {}
            kind @ ("named" | "switch") => params.push(Param {
                name: text("parameter_name"),
                shape: if kind == "switch" {
                    "switch".into()
                } else {
                    text("syntax_shape")
                },
                description: text("description"),
                default: row
                    .get_data_by_key("parameter_default")
                    .filter(|v| !v.is_nothing()),
            }),
            other => {
                return Err(format!(
                    "`main` has a {other} parameter `{}`; pipe parameters must be flags",
                    text("parameter_name")
                ));
            }
        }
    }
    Ok(params)
}

/// A nushell double-quoted string literal for `text`.
fn nu_string(text: &str) -> String {
    serde_json::to_string(text).expect("strings always serialize")
}

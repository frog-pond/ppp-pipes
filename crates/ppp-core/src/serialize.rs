//! Turns a nushell record into JSON for its record type.
//!
//! Inside a pipe, times are nushell datetimes, which always carry an
//! offset. The schema decides how each one is written (spec §5.3): as an
//! instant, as a date in the record's `tz`, or as a wall-clock time in the
//! record's `tz`.

use jiff::tz::TimeZone;
use nu_protocol::Value;
use serde_json::{Map, Number, Value as Json};

use crate::time;
use crate::time_fields::{TimeKind, TimeTree};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SerializeError {
    #[error("{pointer}: {type_name} values cannot be written as JSON")]
    Unsupported { pointer: String, type_name: String },
    #[error("{pointer}: {value} is not a finite number")]
    NonFinite { pointer: String, value: String },
    #[error("{pointer}: writing a {form} needs the record's \"tz\", and the record has none")]
    MissingTimeZone { pointer: String, form: &'static str },
    #[error("{pointer}: unknown time zone \"{tz}\"")]
    UnknownTimeZone { pointer: String, tz: String },
    #[error("{pointer}: datetime is outside the years -9999 to 9999")]
    OutOfRange { pointer: String },
}

/// Serializes one record, formatting the fields in `time_fields`.
/// Datetimes elsewhere are written as instants.
pub fn record_to_json(record: &Value, time_fields: &TimeTree) -> Result<Json, SerializeError> {
    let context = match record {
        Value::Record { val, .. } => Context {
            tz: match val.get("tz") {
                Some(Value::String { val, .. }) => Some(val.clone()),
                _ => None,
            },
            all_day: matches!(val.get("all_day"), Some(Value::Bool { val: true, .. })),
        },
        _ => Context {
            tz: None,
            all_day: false,
        },
    };
    context.convert(record, Some(time_fields), &mut String::new())
}

struct Context {
    tz: Option<String>,
    all_day: bool,
}

impl Context {
    fn convert(
        &self,
        value: &Value,
        tree: Option<&TimeTree>,
        pointer: &mut String,
    ) -> Result<Json, SerializeError> {
        Ok(match value {
            Value::Nothing { .. } => Json::Null,
            Value::Bool { val, .. } => Json::Bool(*val),
            Value::Int { val, .. } => Json::Number((*val).into()),
            Value::Filesize { val, .. } => Json::Number(val.get().into()),
            Value::Float { val, .. } => {
                Json::Number(
                    Number::from_f64(*val).ok_or_else(|| SerializeError::NonFinite {
                        pointer: pointer.clone(),
                        value: val.to_string(),
                    })?,
                )
            }
            Value::String { val, .. } | Value::Glob { val, .. } => Json::String(val.clone()),
            Value::Duration { val, .. } => {
                Json::String(jiff::SignedDuration::from_nanos(*val).to_string())
            }
            Value::Date { val, .. } => {
                let ts = time::timestamp(val).map_err(|_| SerializeError::OutOfRange {
                    pointer: pointer.clone(),
                })?;
                Json::String(self.format_time(ts, tree.and_then(|t| t.kind), pointer)?)
            }
            Value::Record { val, .. } => {
                let mut out = Map::new();
                for (key, child) in val.iter() {
                    let len = pointer.len();
                    pointer.push('/');
                    pointer.push_str(&escape_pointer(key));
                    let subtree = tree.and_then(|t| t.fields.get(key));
                    out.insert(key.clone(), self.convert(child, subtree, pointer)?);
                    pointer.truncate(len);
                }
                Json::Object(out)
            }
            Value::List { vals, .. } => {
                let subtree = tree.and_then(|t| t.each.as_deref());
                let mut out = Vec::with_capacity(vals.len());
                for (i, child) in vals.iter().enumerate() {
                    let len = pointer.len();
                    pointer.push('/');
                    pointer.push_str(&i.to_string());
                    out.push(self.convert(child, subtree, pointer)?);
                    pointer.truncate(len);
                }
                Json::Array(out)
            }
            other => {
                return Err(SerializeError::Unsupported {
                    pointer: pointer.clone(),
                    type_name: other.get_type().to_string(),
                });
            }
        })
    }

    fn format_time(
        &self,
        ts: jiff::Timestamp,
        kind: Option<TimeKind>,
        pointer: &str,
    ) -> Result<String, SerializeError> {
        let kind = match kind {
            Some(TimeKind::When) if self.all_day => TimeKind::Date,
            Some(TimeKind::When) if self.tz.is_some() => TimeKind::LocalDateTime,
            Some(TimeKind::When) | None => TimeKind::Instant,
            Some(kind) => kind,
        };
        match kind {
            TimeKind::Instant | TimeKind::When => Ok(time::format_instant(ts)),
            TimeKind::Date => Ok(time::format_date(ts, &self.zone(pointer, "date")?)),
            TimeKind::LocalDateTime => Ok(time::format_local(
                ts,
                &self.zone(pointer, "local date-time")?,
            )),
        }
    }

    fn zone(&self, pointer: &str, form: &'static str) -> Result<TimeZone, SerializeError> {
        let name = self
            .tz
            .as_deref()
            .ok_or_else(|| SerializeError::MissingTimeZone {
                pointer: pointer.to_string(),
                form,
            })?;
        TimeZone::get(name).map_err(|_| SerializeError::UnknownTimeZone {
            pointer: pointer.to_string(),
            tz: name.to_string(),
        })
    }
}

/// Escapes a key for use in a JSON Pointer (RFC 6901).
fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

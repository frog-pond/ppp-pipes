//! The stdlib schema registry: compiles a validator and a time-field tree
//! for every record type, and validates records against them.
//!
//! Schemas are embedded in the binary and resolved by `$id`. Nothing is ever
//! fetched, so validation is offline and deterministic.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::time_fields::{self, Documents, TimeTree};

pub const COMMON_ID: &str = "https://pipes.frogpond.tech/stdlib/common/1";

/// The stdlib schema documents, in the order they are registered.
const STDLIB: &[&str] = &[
    include_str!("../../../stdlib/common.schema.json"),
    include_str!("../../../stdlib/calendar.schema.json"),
];

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("schema document is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("schema document has no \"$id\"")]
    MissingId,
    #[error("could not register schema {id}: {message}")]
    Register { id: String, message: String },
    #[error("could not compile record type {name}: {message}")]
    Compile { name: String, message: String },
    #[error("cannot resolve $ref {reference}")]
    UnresolvedRef { reference: String },
    #[error(
        "time field at {path} sits under \"{keyword}\", which the time-field walker does not follow"
    )]
    UnsupportedTimeKeyword { keyword: String, path: String },
    #[error("time field at {path} is declared with two different time forms")]
    ConflictingTimeKinds { path: String },
}

/// A validation failure for one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordError {
    /// Position of the record in its list (0-based).
    pub index: usize,
    /// JSON Pointer to the failing value inside the record; empty for the whole record.
    pub pointer: String,
    pub message: String,
    /// The failing value as compact JSON, cut to 80 characters.
    pub excerpt: String,
}

/// One record type: a `$def` in a stdlib schema that declares an `$anchor`.
pub struct RecordType {
    /// The value of the record's `type` field, e.g. `event`.
    pub name: String,
    /// `<schema $id>#<anchor>`, e.g. `https://pipes.frogpond.tech/stdlib/calendar/1#event`.
    pub uri: String,
    pub time_fields: TimeTree,
    validator: jsonschema::Validator,
}

pub struct Registry {
    types: BTreeMap<String, RecordType>,
}

impl Documents for BTreeMap<String, Json> {
    fn document(&self, id: &str) -> Option<&Json> {
        self.get(id)
    }
}

impl Registry {
    /// The registry of embedded stdlib schemas.
    pub fn stdlib() -> Result<Registry, SchemaError> {
        let documents = STDLIB
            .iter()
            .map(|text| serde_json::from_str(text))
            .collect::<Result<Vec<Json>, _>>()?;
        Registry::from_documents(documents)
    }

    /// Builds a registry from schema documents. Each must have an `$id`.
    pub fn from_documents(documents: Vec<Json>) -> Result<Registry, SchemaError> {
        let mut by_id = BTreeMap::new();
        for doc in documents {
            let id = doc
                .get("$id")
                .and_then(Json::as_str)
                .ok_or(SchemaError::MissingId)?
                .to_string();
            by_id.insert(id, doc);
        }

        let mut builder = jsonschema::Registry::new();
        for (id, doc) in &by_id {
            builder = builder
                .add(id.as_str(), doc.clone())
                .map_err(|e| SchemaError::Register {
                    id: id.clone(),
                    message: e.to_string(),
                })?;
        }
        let prepared = builder.prepare().map_err(|e| SchemaError::Register {
            id: "(registry)".into(),
            message: e.to_string(),
        })?;

        let mut types = BTreeMap::new();
        for (id, doc) in &by_id {
            let Some(defs) = doc.get("$defs").and_then(Json::as_object) else {
                continue;
            };
            for def in defs.values() {
                let Some(anchor) = def.get("$anchor").and_then(Json::as_str) else {
                    continue;
                };
                let uri = format!("{id}#{anchor}");
                // Draft 2020-12, formats asserted, no network or file access,
                // plus our two custom formats.
                let validator = jsonschema::draft202012::options()
                    .with_registry(&prepared)
                    .offline()
                    .should_validate_formats(true)
                    .with_format("iana-tz", |s: &str| jiff::tz::db().get(s).is_ok())
                    .with_format("local-date-time", |s: &str| {
                        s.parse::<jiff::civil::DateTime>().is_ok()
                    })
                    .build(&serde_json::json!({ "$ref": uri }))
                    .map_err(|e| SchemaError::Compile {
                        name: anchor.into(),
                        message: e.to_string(),
                    })?;
                let time_fields = time_fields::walk(&by_id, id, def)?;
                types.insert(
                    anchor.to_string(),
                    RecordType {
                        name: anchor.to_string(),
                        uri,
                        time_fields,
                        validator,
                    },
                );
            }
        }

        Ok(Registry { types })
    }

    /// Looks up a record type by the value of a record's `type` field.
    pub fn record_type(&self, name: &str) -> Option<&RecordType> {
        self.types.get(name)
    }

    /// Validates one record against the record type its `type` field names.
    pub fn validate(&self, index: usize, record: &Json) -> Vec<RecordError> {
        let type_name = match record.get("type") {
            Some(Json::String(name)) => name,
            Some(other) => return vec![error(index, "/type", "\"type\" must be a string", other)],
            None => return vec![error(index, "", "record has no \"type\"", record)],
        };
        let Some(record_type) = self.record_type(type_name) else {
            let message = format!("unknown record type \"{type_name}\"");
            return vec![error(index, "/type", &message, &record["type"])];
        };
        record_type
            .validator
            .iter_errors(record)
            .map(|e| {
                error(
                    index,
                    &e.instance_path().to_string(),
                    &e.to_string(),
                    e.instance(),
                )
            })
            .collect()
    }
}

fn error(index: usize, pointer: &str, message: &str, value: &Json) -> RecordError {
    RecordError {
        index,
        pointer: pointer.to_string(),
        message: message.to_string(),
        excerpt: excerpt(value),
    }
}

fn excerpt(value: &Json) -> String {
    const LIMIT: usize = 80;
    let text = value.to_string();
    if text.chars().count() <= LIMIT {
        return text;
    }
    let cut: String = text.chars().take(LIMIT - 1).collect();
    format!("{cut}…")
}

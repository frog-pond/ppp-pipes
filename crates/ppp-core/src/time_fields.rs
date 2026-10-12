//! Finds the time fields of a record type by walking its JSON Schema.
//!
//! The serializer needs to know, for each record type, which fields hold
//! dates, instants or local times. The schema already says so: those fields
//! `$ref` one of the time definitions in `common`. This module walks a
//! record type's `$def` and builds a tree of those paths.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::schema::{COMMON_ID, SchemaError};

/// How a time field is written in JSON (brainstorm §0.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeKind {
    /// RFC 3339 with an offset, written in UTC: `2026-10-15T00:30:00Z`.
    Instant,
    /// A calendar date: `2026-10-14`.
    Date,
    /// A wall-clock time with no offset, read in the record's `tz`.
    LocalDateTime,
    /// Any of the three, chosen per record (spec §5.3).
    When,
}

impl TimeKind {
    fn from_common_def(name: &str) -> Option<TimeKind> {
        match name {
            "instant" => Some(TimeKind::Instant),
            "date" => Some(TimeKind::Date),
            "local-date-time" => Some(TimeKind::LocalDateTime),
            "when" => Some(TimeKind::When),
            _ => None,
        }
    }
}

/// The time fields of a record type, as a tree that mirrors the record.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TimeTree {
    /// Set when the value at this node is itself a time.
    pub kind: Option<TimeKind>,
    /// Object properties that are, or contain, time fields.
    pub fields: BTreeMap<String, TimeTree>,
    /// Array items that are, or contain, time fields.
    pub each: Option<Box<TimeTree>>,
}

/// One step in a path through a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Each,
}

impl TimeTree {
    pub fn is_empty(&self) -> bool {
        self.kind.is_none() && self.fields.is_empty() && self.each.is_none()
    }

    /// Lists every time field as a path string, e.g. `start` or `exdates[]`.
    pub fn paths(&self) -> Vec<(String, TimeKind)> {
        let mut out = Vec::new();
        self.collect_paths(String::new(), &mut out);
        out
    }

    fn collect_paths(&self, prefix: String, out: &mut Vec<(String, TimeKind)>) {
        if let Some(kind) = self.kind {
            out.push((prefix.clone(), kind));
        }
        for (key, child) in &self.fields {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            child.collect_paths(path, out);
        }
        if let Some(child) = &self.each {
            child.collect_paths(format!("{prefix}[]"), out);
        }
    }

    fn insert(&mut self, path: &[Seg], kind: TimeKind) -> Result<(), SchemaError> {
        let mut node = self;
        for seg in path {
            node = match seg {
                Seg::Key(key) => node.fields.entry(key.clone()).or_default(),
                Seg::Each => node.each.get_or_insert_with(Default::default),
            };
        }
        match node.kind {
            Some(existing) if existing != kind => Err(SchemaError::ConflictingTimeKinds {
                path: path_string(path),
            }),
            _ => {
                node.kind = Some(kind);
                Ok(())
            }
        }
    }
}

/// Keywords whose subschemas the walker does not follow. If one of them
/// leads to a time field, the walker cannot know which form applies, so it
/// fails instead of guessing.
const UNSUPPORTED: &[&str] = &[
    "anyOf",
    "oneOf",
    "not",
    "prefixItems",
    "contains",
    "patternProperties",
    "additionalProperties",
    "dependentSchemas",
    "propertyNames",
    "unevaluatedProperties",
    "unevaluatedItems",
];

/// Looks up schema documents by `$id`.
pub trait Documents {
    fn document(&self, id: &str) -> Option<&Json>;
}

/// Builds the time tree for the schema at `schema`, found in document `doc_id`.
pub fn walk(docs: &dyn Documents, doc_id: &str, schema: &Json) -> Result<TimeTree, SchemaError> {
    let mut tree = TimeTree::default();
    let mut walker = Walker {
        docs,
        tree: &mut tree,
        stack: Vec::new(),
    };
    walker.walk(doc_id, schema, &mut Vec::new())?;
    Ok(tree)
}

struct Walker<'a> {
    docs: &'a dyn Documents,
    tree: &'a mut TimeTree,
    /// The `$ref` targets being walked, so recursive schemas end.
    stack: Vec<String>,
}

impl Walker<'_> {
    fn walk(
        &mut self,
        doc_id: &str,
        schema: &Json,
        path: &mut Vec<Seg>,
    ) -> Result<(), SchemaError> {
        let Some(obj) = schema.as_object() else {
            return Ok(());
        };

        if let Some(Json::String(reference)) = obj.get("$ref") {
            let (target_doc, pointer) = split_ref(doc_id, reference);
            if target_doc == COMMON_ID
                && let Some(kind) = pointer
                    .strip_prefix("/$defs/")
                    .and_then(TimeKind::from_common_def)
            {
                return self.tree.insert(path, kind);
            }
            let key = format!("{target_doc}#{pointer}");
            if !self.stack.contains(&key) {
                let target = resolve(self.docs, &target_doc, &pointer)
                    .ok_or_else(|| SchemaError::UnresolvedRef {
                        reference: reference.clone(),
                    })?
                    .clone();
                self.stack.push(key);
                self.walk(&target_doc, &target, path)?;
                self.stack.pop();
            }
        }

        if let Some(Json::Array(parts)) = obj.get("allOf") {
            for part in parts {
                self.walk(doc_id, part, path)?;
            }
        }

        if let Some(Json::Object(props)) = obj.get("properties") {
            for (key, sub) in props {
                path.push(Seg::Key(key.clone()));
                self.walk(doc_id, sub, path)?;
                path.pop();
            }
        }

        if let Some(items) = obj.get("items") {
            path.push(Seg::Each);
            self.walk(doc_id, items, path)?;
            path.pop();
        }

        // `if`, `then` and `else` only narrow fields that `properties`
        // already declares, so the walker skips them.

        for keyword in UNSUPPORTED {
            let Some(value) = obj.get(*keyword) else {
                continue;
            };
            let subschemas: Vec<&Json> = match value {
                Json::Array(list) => list.iter().collect(),
                Json::Object(map)
                    if *keyword == "patternProperties" || *keyword == "dependentSchemas" =>
                {
                    map.values().collect()
                }
                other => vec![other],
            };
            for sub in subschemas {
                let probe = walk_with_stack(self.docs, doc_id, sub, &self.stack)?;
                if !probe.is_empty() {
                    return Err(SchemaError::UnsupportedTimeKeyword {
                        keyword: keyword.to_string(),
                        path: path_string(path),
                    });
                }
            }
        }

        Ok(())
    }
}

fn walk_with_stack(
    docs: &dyn Documents,
    doc_id: &str,
    schema: &Json,
    stack: &[String],
) -> Result<TimeTree, SchemaError> {
    let mut tree = TimeTree::default();
    let mut walker = Walker {
        docs,
        tree: &mut tree,
        stack: stack.to_vec(),
    };
    walker.walk(doc_id, schema, &mut Vec::new())?;
    Ok(tree)
}

/// Splits a `$ref` into the target document's `$id` and a JSON Pointer.
/// An anchor (`…/calendar/1#event`) becomes the pointer to the `$def` that
/// declares it.
fn split_ref(doc_id: &str, reference: &str) -> (String, String) {
    let (base, fragment) = reference.split_once('#').unwrap_or((reference, ""));
    let target_doc = if base.is_empty() {
        doc_id.to_string()
    } else {
        base.to_string()
    };
    (target_doc, fragment.to_string())
}

fn resolve<'a>(docs: &'a dyn Documents, doc_id: &str, fragment: &str) -> Option<&'a Json> {
    let doc = docs.document(doc_id)?;
    if fragment.is_empty() || fragment.starts_with('/') {
        return doc.pointer(fragment);
    }
    doc.get("$defs")?
        .as_object()?
        .values()
        .find(|def| def.get("$anchor").and_then(Json::as_str) == Some(fragment))
}

fn path_string(path: &[Seg]) -> String {
    let mut out = String::new();
    for seg in path {
        match seg {
            Seg::Key(key) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(key);
            }
            Seg::Each => out.push_str("[]"),
        }
    }
    out
}

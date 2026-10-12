//! `ppp info`: what a pipe declares, as JSON, without running it. This is
//! the metadata a UI turns into a form (spec §4.4).

use std::process::ExitCode;

use ppp_core::pipe::{Pipe, PipeInfo};
use ppp_core::serialize::record_to_json;
use ppp_core::time_fields::TimeTree;
use serde_json::{Value as Json, json};

use crate::Lookup;

pub fn run(lookup: &Lookup, pipe: &str) -> ExitCode {
    match Pipe::load(&lookup.path(pipe)) {
        Ok(loaded) => {
            let text = serde_json::to_string_pretty(&to_json(loaded.info()))
                .expect("JSON values always serialize");
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("ppp: {err}");
            ExitCode::from(2)
        }
    }
}

fn to_json(info: &PipeInfo) -> Json {
    let params: Vec<Json> = info
        .params
        .iter()
        .map(|p| {
            let default = p.default.as_ref().map_or(Json::Null, |value| {
                record_to_json(value, &TimeTree::default()).unwrap_or(Json::Null)
            });
            json!({ "name": p.name, "type": p.shape, "description": p.description, "default": default })
        })
        .collect();
    let examples: Vec<Json> = info
        .examples
        .iter()
        .map(|e| json!({ "description": e.description, "example": e.example }))
        .collect();
    json!({
        "name": info.name,
        "title": info.title,
        "description": info.description,
        "category": info.category,
        "search_terms": info.search_terms,
        "examples": examples,
        "params": params,
        "output": info.output,
        "refresh": info.refresh.map(|r| r.to_string()),
    })
}

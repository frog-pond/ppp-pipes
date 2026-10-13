//! `http get`: our replacement for nushell's, through the cache and limits.

use nu_engine::command_prelude::*;
use nu_protocol::shell_error::generic::GenericError;

use super::Context;
use crate::http::fetch;

#[derive(Clone)]
pub struct HttpGet {
    pub context: Context,
}

impl Command for HttpGet {
    fn name(&self) -> &str {
        "http get"
    }

    fn description(&self) -> &str {
        "Fetch a URL, using the run's HTTP cache."
    }

    fn extra_description(&self) -> &str {
        "A JSON response (by Content-Type) is parsed unless --raw is given; anything else is \
         returned as text when it is UTF-8, or as binary. A cached copy is used while it is \
         fresh by the pipe's `refresh` or the response's Cache-Control, and when the network \
         fails."
    }

    fn signature(&self) -> Signature {
        Signature::build("http get")
            .input_output_types(vec![(Type::Nothing, Type::Any)])
            .required("url", SyntaxShape::String, "The http(s) URL to fetch")
            .switch("raw", "Return the body without parsing JSON", None)
            .category(Category::Network)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let url: Spanned<String> = call.req(engine_state, stack, 0)?;
        let raw = call.has_flag(engine_state, stack, "raw")?;
        if !(url.item.starts_with("http://") || url.item.starts_with("https://")) {
            return Err(ShellError::Generic(GenericError::new(
                "only http and https URLs can be fetched",
                "not an http(s) URL",
                url.span,
            )));
        }
        let refresh = *self.context.refresh.lock().expect("refresh lock");
        let fetched = fetch(&self.context.host, &url.item, refresh).map_err(|err| {
            ShellError::Generic(GenericError::new("cannot fetch", err.to_string(), url.span))
        })?;
        let is_json = fetched
            .content_type
            .as_deref()
            .is_some_and(|t| t.contains("json"));
        let value = if is_json && !raw && fetched.body.trim_ascii().is_empty() {
            Value::nothing(head)
        } else if is_json && !raw {
            let json: serde_json::Value = serde_json::from_slice(&fetched.body).map_err(|err| {
                ShellError::Generic(GenericError::new(
                    "cannot parse JSON response",
                    err.to_string(),
                    url.span,
                ))
            })?;
            json_to_value(json, head)
        } else {
            match String::from_utf8(fetched.body) {
                Ok(text) => Value::string(text, head),
                Err(err) => Value::binary(err.into_bytes(), head),
            }
        };
        Ok(value.into_pipeline_data())
    }
}

fn json_to_value(json: serde_json::Value, span: Span) -> Value {
    match json {
        serde_json::Value::Null => Value::nothing(span),
        serde_json::Value::Bool(b) => Value::bool(b, span),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::int(i, span),
            None => Value::float(n.as_f64().expect("JSON numbers convert to f64"), span),
        },
        serde_json::Value::String(s) => Value::string(s, span),
        serde_json::Value::Array(items) => Value::list(
            items
                .into_iter()
                .map(|item| json_to_value(item, span))
                .collect(),
            span,
        ),
        serde_json::Value::Object(map) => {
            let mut record = Record::new();
            for (key, value) in map {
                record.push(key, json_to_value(value, span));
            }
            Value::record(record, span)
        }
    }
}

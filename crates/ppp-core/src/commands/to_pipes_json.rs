//! `to pipes-json`: records as the canonical JSON the host would emit.

use nu_engine::command_prelude::*;

use super::{CommandError, Context, describe_failures};
use crate::pipe::check_values;
use crate::schema::Registry;

#[derive(Clone)]
pub struct ToPipesJson {
    pub context: Context,
}

impl Command for ToPipesJson {
    fn name(&self) -> &str {
        "to pipes-json"
    }

    fn description(&self) -> &str {
        "Write records as JSON with each time field in its schema's form, after checking them."
    }

    fn extra_description(&self) -> &str {
        "Unlike `to json`, dates come out as dates, local times without an offset (in the \
         record's `tz`) and instants in UTC, exactly as `ppp run` prints them."
    }

    fn signature(&self) -> Signature {
        Signature::build("to pipes-json")
            .input_output_types(vec![
                (Type::table(), Type::String),
                (Type::record(), Type::String),
            ])
            .category(Category::Custom("pipes".into()))
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let registry = self.context.host.registry().map_err(|e| {
            CommandError::new("stdlib schemas", e.to_string()).into_shell_error(head)
        })?;
        render(&registry, input.into_value(head)?, head)
            .map(|value| value.into_pipeline_data())
            .map_err(|err| err.into_shell_error(head))
    }
}

/// The records in `input` (a record, or a list of records) as a pretty JSON
/// array; a single record gives a single object.
pub fn render(registry: &Registry, input: Value, head: Span) -> Result<Value, CommandError> {
    let single = !matches!(input, Value::List { .. });
    let records = match input {
        Value::List { vals, .. } => vals.to_vec(),
        other => vec![other],
    };
    let json = check_values(registry, None, &records)
        .map_err(|errors| CommandError::new("records are not valid", describe_failures(&errors)))?;
    let text = if single {
        serde_json::to_string_pretty(&json[0])
    } else {
        serde_json::to_string_pretty(&json)
    }
    .expect("JSON values always serialize");
    Ok(Value::string(text, head))
}

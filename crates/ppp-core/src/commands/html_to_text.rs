//! `html to-text`: plain text from an HTML string.

use nu_engine::command_prelude::*;

use super::CommandError;
use crate::html::to_text;

#[derive(Clone)]
pub struct HtmlToText;

impl Command for HtmlToText {
    fn name(&self) -> &str {
        "html to-text"
    }

    fn description(&self) -> &str {
        "Turn HTML into plain text: drop tags, break lines at block tags, decode entities."
    }

    fn signature(&self) -> Signature {
        Signature::build("html to-text")
            .input_output_types(vec![
                (Type::String, Type::String),
                (Type::Nothing, Type::Nothing),
            ])
            .category(Category::Strings)
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        convert(input.into_value(head)?, head)
            .map(|value| value.into_pipeline_data())
            .map_err(|err| err.into_shell_error(head))
    }
}

/// Plain text from an HTML string; `null` stays `null`.
pub fn convert(input: Value, head: Span) -> Result<Value, CommandError> {
    Ok(match input {
        Value::Nothing { .. } => Value::nothing(head),
        value => {
            let kind = value.get_type();
            let html = value.coerce_into_string().map_err(|_| {
                CommandError::new("html to-text", format!("expected text, got {kind}"))
            })?;
            Value::string(to_text(&html), head)
        }
    })
}

//! `html to-text`: plain text from an HTML string.

use nu_engine::command_prelude::*;

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
        match input.into_value(head)? {
            Value::Nothing { .. } => Ok(Value::nothing(head).into_pipeline_data()),
            value => {
                Ok(Value::string(to_text(&value.coerce_into_string()?), head).into_pipeline_data())
            }
        }
    }
}

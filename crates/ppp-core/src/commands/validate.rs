//! `validate`: checks records against the stdlib schemas and passes them on.

use nu_engine::command_prelude::*;

use super::{CommandError, Context, describe_failures};
use crate::pipe::check_values;
use crate::schema::Registry;

#[derive(Clone)]
pub struct Validate {
    pub context: Context,
}

impl Command for Validate {
    fn name(&self) -> &str {
        "validate"
    }

    fn description(&self) -> &str {
        "Check records against the stdlib schemas; pass them on unchanged if they are valid."
    }

    fn signature(&self) -> Signature {
        Signature::build("validate")
            .input_output_types(vec![
                (Type::table(), Type::table()),
                (Type::record(), Type::record()),
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
        check(&registry, input.into_value(head)?)
            .map(|value| value.into_pipeline_data())
            .map_err(|err| err.into_shell_error(head))
    }
}

/// Returns `input` (a record, or a list of records) unchanged if every
/// record is valid.
pub fn check(registry: &Registry, input: Value) -> Result<Value, CommandError> {
    let records = match &input {
        Value::List { vals, .. } => vals.to_vec(),
        other => vec![other.clone()],
    };
    match check_values(registry, None, &records) {
        Ok(_) => Ok(input),
        Err(errors) => Err(CommandError::new(
            "records are not valid",
            describe_failures(&errors),
        )),
    }
}

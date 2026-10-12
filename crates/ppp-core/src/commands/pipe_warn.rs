//! `pipe warn`: a warning for whoever runs the pipe, without failing it.

use nu_engine::command_prelude::*;

use super::Context;

#[derive(Clone)]
pub struct PipeWarn {
    pub context: Context,
}

impl Command for PipeWarn {
    fn name(&self) -> &str {
        "pipe warn"
    }

    fn description(&self) -> &str {
        "Report a warning (for example, an optional source that failed) and carry on."
    }

    fn signature(&self) -> Signature {
        Signature::build("pipe warn")
            .input_output_types(vec![(Type::Any, Type::Any)])
            .required("message", SyntaxShape::String, "What went wrong")
            .category(Category::Custom("pipes".into()))
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let message: String = call.req(engine_state, stack, 0)?;
        self.context.host.warn(format!("Warning: {message}"));
        Ok(input)
    }
}

//! `pipe get`: another pipe's checked records, as nushell values (spec §5.5).

use nu_engine::command_prelude::*;
use nu_protocol::shell_error::generic::GenericError;

use super::Context;
use crate::pipe::{Pipe, check_records, is_pipe_name};

#[derive(Clone)]
pub struct PipeGet {
    pub context: Context,
}

/// How many of another pipe's failures to show in the error.
const SHOWN: usize = 5;

impl Command for PipeGet {
    fn name(&self) -> &str {
        "pipe get"
    }

    fn description(&self) -> &str {
        "Run another pipe and return its records, checked against its declared types."
    }

    fn extra_description(&self) -> &str {
        "The pipe is looked up as <name>.nu in the run's pipes directory, runs in its own \
         sandboxed engine sharing this run's cache and timeout, and returns nushell values \
         (datetimes stay datetimes). A pipe that, directly or not, gets itself is an error."
    }

    fn signature(&self) -> Signature {
        Signature::build("pipe get")
            .input_output_types(vec![(Type::Nothing, Type::table())])
            .required("name", SyntaxShape::String, "The pipe's name")
            .optional(
                "args",
                SyntaxShape::Record(Default::default()),
                "The pipe's flags, e.g. {within: 3day}",
            )
            .category(Category::Custom("pipes".into()))
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let name: Spanned<String> = call.req(engine_state, stack, 0)?;
        let args: Option<Record> = call.opt(engine_state, stack, 1)?;
        let fail = |message: String| {
            ShellError::Generic(GenericError::new(
                format!("pipe get {}", name.item),
                message,
                name.span,
            ))
        };
        if !is_pipe_name(&name.item) {
            return Err(fail(
                "pipe names may use only letters, digits, `-` and `_`".into(),
            ));
        }
        let host = &self.context.host;
        let path = host.config.pipes_dir.join(format!("{}.nu", name.item));
        let mut pipe = Pipe::load_with(&path, host.clone()).map_err(|e| fail(e.to_string()))?;
        let records = pipe.run(&args.unwrap_or_default());
        for warning in pipe.take_warnings() {
            host.warn(warning);
        }
        let records = records.map_err(|e| fail(e.to_string()))?;
        let registry = host.registry().map_err(|e| fail(e.to_string()))?;
        if let Err(errors) = check_records(&registry, pipe.info(), &records) {
            let mut lines: Vec<String> = errors
                .iter()
                .take(SHOWN)
                .map(|e| format!("record {} {}: {}", e.index, e.pointer, e.message))
                .collect();
            if errors.len() > SHOWN {
                lines.push(format!("…and {} more", errors.len() - SHOWN));
            }
            return Err(fail(format!(
                "its records failed the check:\n{}",
                lines.join("\n")
            )));
        }
        Ok(Value::list(records, head).into_pipeline_data())
    }
}

//! `query web`: nushell's own HTML query command (from `nu_plugin_query`),
//! run inside the engine.
//!
//! `nu_plugin_query` is a plugin, but its `query web` does its work in a
//! public function that needs no plugin connection. Calling it here gives
//! pipes the same command, with the same flags and results, as stock `nu`
//! with that plugin installed.

use nu_engine::command_prelude::*;
use nu_engine::eval_expression;
use nu_plugin::SimplePluginCommand;
use nu_plugin_protocol::EvaluatedCall;
use nu_protocol::debugger::WithoutDebug;

#[derive(Clone)]
pub struct QueryWeb;

impl Command for QueryWeb {
    fn name(&self) -> &str {
        SimplePluginCommand::name(&nu_plugin_query::QueryWeb)
    }

    fn description(&self) -> &str {
        SimplePluginCommand::description(&nu_plugin_query::QueryWeb)
    }

    fn signature(&self) -> Signature {
        SimplePluginCommand::signature(&nu_plugin_query::QueryWeb)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;
        let evaluated = EvaluatedCall::try_from_call(
            call,
            engine_state,
            stack,
            eval_expression::<WithoutDebug>,
        )?;
        let input = input.into_value(head)?;
        nu_plugin_query::parse_selector_params(&evaluated, &input)
            .map(|value| value.into_pipeline_data())
            .map_err(ShellError::from)
    }
}

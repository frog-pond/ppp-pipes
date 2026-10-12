//! `query html`: CSS selector queries on HTML, with the same flags and
//! results as nushell's `query web` (see `html_query`).

use nu_engine::command_prelude::*;
use nu_engine::eval_expression;
use nu_plugin_protocol::EvaluatedCall;
use nu_protocol::debugger::WithoutDebug;

use super::Context;
use crate::html_query::parse_selector_params;

#[derive(Clone)]
pub struct QueryHtml {
    pub context: Context,
}

impl Command for QueryHtml {
    fn name(&self) -> &str {
        "query html"
    }

    fn description(&self) -> &str {
        "Run a CSS selector query on HTML."
    }

    fn extra_description(&self) -> &str {
        "Takes the same flags and gives the same results as nushell's `query web`, without \
         needing the `nu_plugin_query` plugin. `--inspect` output goes where warnings go."
    }

    fn signature(&self) -> Signature {
        Signature::build("query html")
            .named("query", SyntaxShape::String, "Selector query.", Some('q'))
            .switch("as-html", "Return the query output as html.", Some('m'))
            // A string or a list of strings.
            .named(
                "attribute",
                SyntaxShape::Any,
                "Downselect based on the given attribute.",
                Some('a'),
            )
            .named(
                "as-table",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Find table based on column header list.",
                Some('t'),
            )
            .switch(
                "inspect",
                "Run in inspect mode to provide more information for determining column headers.",
                Some('i'),
            )
            .switch(
                "document",
                "Parse the input as a full HTML document instead of a fragment",
                Some('d'),
            )
            .category(Category::Network)
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
        let host = &self.context.host;
        parse_selector_params(&evaluated, &input, &mut |message| host.warn(message))
            .map(|value| value.into_pipeline_data())
            .map_err(ShellError::from)
    }
}

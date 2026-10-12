//! The sandboxed nushell engine that runs pipes.
//!
//! The engine registers a hand-picked command set: nushell's language
//! commands, the data commands listed in `bind_data_commands`, and our own.
//! Nothing that runs external programs or touches the filesystem is
//! registered, so a pipe that tries gets "command not found". The `pipes`
//! module is embedded and loaded as a virtual directory, so `use pipes *`
//! works without files on disk.

use nu_engine::eval_block;
use nu_protocol::debugger::WithoutDebug;
use nu_protocol::engine::{EngineState, Stack, StateWorkingSet, VirtualPath};
use nu_protocol::{PipelineData, Span, Value, format_cli_error};

use crate::commands;

/// The embedded `pipes` nushell module.
const PIPES_MODULE: &str = include_str!("../../../stdlib/pipes/mod.nu");

/// A rendered nushell diagnostic, ready to print.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EngineError(pub String);

pub struct Engine {
    state: EngineState,
    stack: Stack,
    /// Rendered parse warnings not yet taken by `take_warnings`.
    warnings: Vec<String>,
}

impl Engine {
    pub fn new() -> Engine {
        // `http get` uses rustls, which needs a crypto provider installed
        // once per process. Later calls are no-ops.
        nu_command::tls::CRYPTO_PROVIDER.default();
        let mut state = nu_cmd_lang::create_default_context();
        let delta = {
            let mut working_set = StateWorkingSet::new(&state);
            bind_data_commands(&mut working_set);
            commands::bind(&mut working_set);
            let file = working_set.add_file("pipes/mod.nu", PIPES_MODULE.as_bytes());
            let module_file =
                working_set.add_virtual_path("pipes/mod.nu".into(), VirtualPath::File(file));
            let _ =
                working_set.add_virtual_path("pipes".into(), VirtualPath::Dir(vec![module_file]));
            working_set.render()
        };
        state
            .merge_delta(delta)
            .expect("the built-in command set and pipes module always merge");
        // Some commands (`http get` among them) need a working directory.
        // Pipes have no filesystem access, so the root is a neutral choice
        // that exists on every platform.
        state.add_env_var("PWD".into(), Value::string("/", Span::unknown()));
        Engine {
            state,
            stack: Stack::new(),
            warnings: Vec::new(),
        }
    }

    /// Parses and runs `source` with `input` as its pipeline input, and
    /// collects the result. Definitions and `use`s persist for later calls.
    pub fn eval(&mut self, source: &str, input: Value) -> Result<Value, EngineError> {
        let block = {
            let mut working_set = StateWorkingSet::new(&self.state);
            let block =
                nu_parser::parse(&mut working_set, Some("source"), source.as_bytes(), false);
            if let Some(err) = working_set.parse_errors.first() {
                return Err(EngineError(format_cli_error(None, &working_set, err, None)));
            }
            if let Some(err) = working_set.compile_errors.first() {
                return Err(EngineError(format_cli_error(None, &working_set, err, None)));
            }
            // nushell formats warnings and errors the same way; only the
            // leading word differs.
            for warning in &working_set.parse_warnings {
                let text = format_cli_error(None, &working_set, warning, None);
                self.warnings.push(text.replacen("Error: ", "Warning: ", 1));
            }
            let delta = working_set.render();
            self.state
                .merge_delta(delta)
                .map_err(|err| self.render(&err))?;
            block
        };
        let input = PipelineData::Value(input, None);
        let result = match eval_block::<WithoutDebug>(&self.state, &mut self.stack, &block, input) {
            Ok(out) => out.body.into_value(Span::unknown()),
            Err(err) => Err(err),
        };
        match result {
            Ok(Value::Error { error, .. }) => Err(self.render(&error)),
            Ok(value) => Ok(value),
            Err(err) => Err(self.render(&err)),
        }
    }

    /// Returns, and forgets, the warnings nushell reported while parsing
    /// since the last call (deprecations, for example), rendered.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Whether a command with this name is registered.
    pub fn has_command(&self, name: &str) -> bool {
        self.state.find_decl(name.as_bytes(), &[]).is_some()
    }

    fn render(&self, err: &nu_protocol::ShellError) -> EngineError {
        let working_set = StateWorkingSet::new(&self.state);
        EngineError(format_cli_error(Some(&self.stack), &working_set, err, None))
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

/// The nushell data commands a pipe may use: no externals, no filesystem,
/// no plugins. Network access is `http get` only.
fn bind_data_commands(working_set: &mut StateWorkingSet) {
    use nu_command::*;
    macro_rules! bind {
        ( $( $command:expr ),* $(,)? ) => { $( working_set.add_decl(Box::new($command)); )* };
    }
    bind!(
        // Filters
        All,
        Any,
        Append,
        Chunks,
        Columns,
        Compact,
        Default,
        Drop,
        Each,
        Enumerate,
        Find,
        First,
        Flatten,
        Get,
        GroupBy,
        IsEmpty,
        IsNotEmpty,
        Items,
        Last,
        Length,
        Merge,
        Move,
        Prepend,
        Reduce,
        Reject,
        Rename,
        Reverse,
        Select,
        Skip,
        SkipWhile,
        Sort,
        SortBy,
        Take,
        TakeWhile,
        Transpose,
        Uniq,
        UniqBy,
        Update,
        Upsert,
        Insert,
        Values,
        Where,
        Wrap,
        Zip,
        // Strings
        Parse,
        Split,
        SplitColumn,
        SplitList,
        SplitRow,
        StrContains,
        StrLowercase,
        StrEndswith,
        StrJoin,
        StrLength,
        StrReplace,
        StrStartsWith,
        StrSubstring,
        StrTrim,
        StrUppercase,
        Lines,
        // Conversions
        IntoBool,
        IntoDatetime,
        IntoDuration,
        IntoFloat,
        IntoInt,
        IntoRecord,
        IntoString,
        // Dates
        DateNow,
        DateToTimezone,
        FormatDate,
        // Formats
        FromJson,
        FromNuon,
        ToJson,
        ToNuon,
        // Math and generators
        MathMax,
        MathMin,
        MathSum,
        Generate,
        Seq,
        // URLs and network
        HttpGet,
        UrlBuildQuery,
        UrlDecode,
        UrlEncode,
        UrlJoin,
        UrlParse,
    );
}

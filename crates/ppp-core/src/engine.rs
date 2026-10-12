//! The sandboxed nushell engine that runs pipes.
//!
//! The engine registers a hand-picked command set: nushell's language
//! commands, the data commands listed in `bind_data_commands`, and our own.
//! Nothing that runs external programs or touches the filesystem is
//! registered, so a pipe that tries gets "command not found". The `pipes`
//! module is embedded and loaded as a virtual directory, so `use ppp *`
//! works without files on disk.

use std::sync::{Arc, Mutex};

use jiff::SignedDuration;
use nu_engine::eval_block;
use nu_protocol::debugger::WithoutDebug;
use nu_protocol::engine::{EngineState, Stack, StateWorkingSet, VirtualPath};
use nu_protocol::{PipelineData, Signals, Span, Value, VirtualPathId, format_cli_error};

use crate::commands::{self, Context};
use crate::host::Host;

/// The embedded `ppp` nushell module.
const PPP_MODULE: &str = include_str!("../../../stdlib/ppp/mod.nu");

/// A rendered nushell diagnostic, ready to print.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EngineError(pub String);

pub struct Engine {
    state: EngineState,
    stack: Stack,
    /// Rendered parse warnings not yet taken by `take_warnings`.
    warnings: Vec<String>,
    context: Context,
}

impl Engine {
    /// An engine with its own default `Host`: no cache, pipes in `./pipes`.
    pub fn new() -> Engine {
        Engine::with_host(Arc::new(Host::default()))
    }

    /// An engine sharing `host` with every other engine of the same run.
    pub fn with_host(host: Arc<Host>) -> Engine {
        let context = Context {
            host: host.clone(),
            refresh: Arc::new(Mutex::new(None)),
        };
        let mut state = nu_cmd_lang::create_default_context();
        let delta = {
            let mut working_set = StateWorkingSet::new(&state);
            bind_data_commands(&mut working_set);
            commands::bind(&mut working_set, &context);
            let module_file = add_virtual_file(&mut working_set, "ppp/mod.nu", PPP_MODULE);
            let _ = working_set.add_virtual_path("ppp".into(), VirtualPath::Dir(vec![module_file]));
            working_set.render()
        };
        state
            .merge_delta(delta)
            .expect("the built-in command set and ppp module always merge");
        // Some nushell commands expect a working directory. Pipes have no
        // filesystem access, so the root is a neutral choice that exists on
        // every platform.
        state.add_env_var("PWD".into(), Value::string("/", Span::unknown()));
        // A timeout sets the host's flag, which stops every engine of the run.
        state.set_signals(Signals::new(host.interrupt()));
        Engine {
            state,
            stack: Stack::new(),
            warnings: Vec::new(),
            context,
        }
    }

    /// Makes `source` the module that `use <name>` loads. Pipes can't load
    /// modules from disk, so this is how a pipe's own file gets in.
    pub fn add_module(&mut self, name: &str, source: &str) -> Result<(), EngineError> {
        let mut working_set = StateWorkingSet::new(&self.state);
        add_virtual_file(&mut working_set, name, source);
        let delta = working_set.render();
        self.state
            .merge_delta(delta)
            .map_err(|err| self.render(&err))
    }

    /// The host this engine shares with the rest of its run.
    pub fn host(&self) -> &Arc<Host> {
        &self.context.host
    }

    /// Sets the running pipe's `refresh`, which `http get` uses to decide
    /// whether a cached response is fresh.
    pub fn set_refresh(&self, refresh: Option<SignedDuration>) {
        *self.context.refresh.lock().expect("refresh lock") = refresh;
    }

    /// Parses and runs `source` with `input` as its pipeline input, and
    /// collects the result. Definitions and `use`s persist for later calls.
    pub fn eval(&mut self, source: &str, input: Value) -> Result<Value, EngineError> {
        let block = {
            let mut working_set = StateWorkingSet::new(&self.state);
            // `parse` would reuse a cached block for repeated source text, and
            // that block comes back without its compiled form.
            let block =
                nu_parser::parse_fresh(&mut working_set, Some("source"), source.as_bytes(), false);
            // nushell's parser reads `use`d files straight from disk, and
            // nothing turns that off. Refuse any file it read before its
            // contents can show up in an error.
            let first_new = self.state.num_files();
            if let Some(file) = working_set
                .files()
                .skip(first_new)
                .find(|file| &*file.name != "source")
            {
                return Err(EngineError(format!(
                    "Error: pipes can't load files; `{}` is not one of the modules a pipe may use",
                    file.name
                )));
            }
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

/// Adds `contents` as a file that `use <name>` finds without touching disk.
fn add_virtual_file(
    working_set: &mut StateWorkingSet,
    name: &str,
    contents: &str,
) -> VirtualPathId {
    let file = working_set.add_file(name, contents.as_bytes());
    working_set.add_virtual_path(name.into(), VirtualPath::File(file))
}

/// The nushell data commands a pipe may use: no externals, no filesystem,
/// no plugins. Network access is our own `http get` (in `commands`).
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
        // URLs
        UrlBuildQuery,
        UrlDecode,
        UrlEncode,
        UrlJoin,
        UrlParse,
    );
}

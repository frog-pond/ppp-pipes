//! `nu_plugin_pipes`: ppp-pipes' commands for stock `nu` (spec §4.5).
//!
//! Each command here is a thin wrapper around the same function the
//! sandboxed engine's command calls; it borrows that command's name,
//! signature and description, so the two can't drift apart.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use nu_plugin::{EngineInterface, EvaluatedCall, Plugin, PluginCommand, SimplePluginCommand};
use nu_protocol::engine::Command;
use nu_protocol::{LabeledError, Record, SignalAction, Signature, Spanned, SyntaxShape, Value};
use ppp_core::commands::{self, CommandError, Context};
use ppp_core::host::{Host, HostConfig};
use ppp_core::schema::Registry;

/// How long `pipe get` lets a pipe run.
const PIPE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct PipesPlugin {
    registry: OnceLock<Result<Arc<Registry>, String>>,
}

impl PipesPlugin {
    fn registry(&self) -> Result<Arc<Registry>, LabeledError> {
        self.registry
            .get_or_init(|| Registry::stdlib().map(Arc::new).map_err(|e| e.to_string()))
            .clone()
            .map_err(|e| {
                LabeledError::new("stdlib schemas").with_label(e, nu_protocol::Span::unknown())
            })
    }
}

impl Plugin for PipesPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
        let context = Context {
            host: Arc::new(Host::default()),
            refresh: Arc::new(Mutex::new(None)),
        };
        vec![
            Box::new(FromIcs(commands::FromIcs)),
            Box::new(ExpandRrule(commands::ExpandRrule)),
            Box::new(HtmlToText(commands::HtmlToText)),
            Box::new(Validate(commands::Validate {
                context: context.clone(),
            })),
            Box::new(ToPipesJson(commands::ToPipesJson {
                context: context.clone(),
            })),
            Box::new(PipeWarn(commands::PipeWarn {
                context: context.clone(),
            })),
            Box::new(QueryHtml(commands::QueryHtml {
                context: context.clone(),
            })),
            Box::new(PipeGet(commands::PipeGet { context })),
        ]
    }
}

/// The parts every wrapper takes from the engine command it wraps.
macro_rules! borrow_from_core {
    () => {
        type Plugin = PipesPlugin;
        fn name(&self) -> &str {
            self.0.name()
        }
        fn description(&self) -> &str {
            self.0.description()
        }
        fn extra_description(&self) -> &str {
            self.0.extra_description()
        }
    };
}

fn labeled(head: nu_protocol::Span) -> impl Fn(CommandError) -> LabeledError {
    move |err| err.into_labeled_error(head)
}

pub struct FromIcs(commands::FromIcs);

impl SimplePluginCommand for FromIcs {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        _: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        let tz: Option<String> = call.get_flag("tz")?;
        commands::from_ics::read(input.clone(), tz.as_deref(), call.head)
            .map_err(labeled(call.head))
    }
}

pub struct ExpandRrule(commands::ExpandRrule);

impl SimplePluginCommand for ExpandRrule {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        _: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        let from: Value = call.get_flag_value("from").expect("required flag");
        let until: Value = call.get_flag_value("until").expect("required flag");
        commands::expand_rrule::expand_values(input.clone(), &from, &until, call.head)
            .map_err(labeled(call.head))
    }
}

pub struct HtmlToText(commands::HtmlToText);

impl SimplePluginCommand for HtmlToText {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        _: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        commands::html_to_text::convert(input.clone(), call.head).map_err(labeled(call.head))
    }
}

/// In stock `nu` a warning goes straight to stderr.
pub struct PipeWarn(commands::PipeWarn);

impl SimplePluginCommand for PipeWarn {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        _: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        let message: String = call.req(0)?;
        eprintln!("Warning: {message}");
        Ok(input.clone())
    }
}

/// `--inspect` output goes to stderr, as `pipe warn`'s does.
pub struct QueryHtml(commands::QueryHtml);

impl SimplePluginCommand for QueryHtml {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        _: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        ppp_core::html_query::parse_selector_params(call, input, &mut |message| {
            eprintln!("{message}")
        })
    }
}

pub struct Validate(commands::Validate);

impl SimplePluginCommand for Validate {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        plugin: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        commands::validate::check(&*plugin.registry()?, input.clone()).map_err(labeled(call.head))
    }
}

pub struct ToPipesJson(commands::ToPipesJson);

impl SimplePluginCommand for ToPipesJson {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0.signature()
    }
    fn run(
        &self,
        plugin: &PipesPlugin,
        _: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        commands::to_pipes_json::render(&*plugin.registry()?, input.clone(), call.head)
            .map_err(labeled(call.head))
    }
}

/// `pipe get` in stock `nu`: runs the pipe in the full sandboxed engine, with
/// the cache and a time limit, and returns its checked records.
pub struct PipeGet(commands::PipeGet);

impl SimplePluginCommand for PipeGet {
    borrow_from_core!();
    fn signature(&self) -> Signature {
        self.0
            .signature()
            .named(
                "pipes-dir",
                SyntaxShape::String,
                "Directory that pipe names are looked up in (default: ./pipes)",
                None,
            )
            .named(
                "timeout",
                SyntaxShape::Duration,
                "Stop the pipe after this long (default: 1min)",
                None,
            )
    }
    fn run(
        &self,
        plugin: &PipesPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _: &Value,
    ) -> Result<Value, LabeledError> {
        let Spanned { item: name, span } = call.req::<Spanned<String>>(0)?;
        let args: Option<Record> = call.opt(1)?;
        let pipes_dir: Option<String> = call.get_flag("pipes-dir")?;
        let limit = match call.get_flag_value("timeout") {
            Some(Value::Duration { val, .. }) => Duration::from_nanos(val.max(0) as u64),
            _ => PIPE_TIMEOUT,
        };
        let cwd = PathBuf::from(engine.get_current_dir()?);
        let host = Arc::new(
            Host::new(HostConfig {
                cache_dir: ppp_core::cache::default_dir(),
                pipes_dir: cwd.join(pipes_dir.unwrap_or_else(|| "pipes".into())),
                ..HostConfig::default()
            })
            .with_registry(plugin.registry()?),
        );
        // The plugin runs in its own process: Ctrl-C reaches it only as a
        // signal from nu, which must stop the sandboxed engine too.
        let pressed = Arc::new(AtomicBool::new(false));
        let _handler = {
            let (pressed, interrupt) = (pressed.clone(), host.interrupt());
            engine.register_signal_handler(Box::new(move |action| {
                if matches!(action, SignalAction::Interrupt) {
                    pressed.store(true, Ordering::Relaxed);
                    interrupt.store(true, Ordering::Relaxed);
                }
            }))?
        };
        let result = host.with_timeout(limit, || {
            commands::pipe_get::get(&host, &name, args.unwrap_or_default(), call.head)
        });
        for warning in host.take_warnings() {
            eprintln!("{warning}");
        }
        result.map_err(|err| {
            let err = if pressed.load(Ordering::Relaxed) {
                CommandError::new(format!("pipe get {name}"), "the pipe was interrupted")
            } else if host.timed_out() {
                let limit = humantime(limit);
                CommandError::new(
                    format!("pipe get {name}"),
                    format!("the pipe ran longer than {limit} and was stopped"),
                )
            } else {
                err
            };
            // Point at the pipe's name, as the engine's `pipe get` does.
            err.into_labeled_error(span)
        })
    }
}

/// A duration as nushell writes one, e.g. `1sec` or `1min`.
fn humantime(limit: Duration) -> String {
    let secs = limit.as_secs();
    if secs > 0 && secs.is_multiple_of(60) {
        format!("{}min", secs / 60)
    } else {
        format!("{secs}sec")
    }
}

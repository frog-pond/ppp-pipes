//! Our own nushell commands.

use std::sync::{Arc, Mutex};

use jiff::SignedDuration;
use nu_protocol::engine::StateWorkingSet;
use nu_protocol::shell_error::generic::GenericError;
use nu_protocol::{LabeledError, ShellError, Span};

use crate::host::Host;
use crate::schema::RecordError;

pub mod expand_rrule;
pub mod from_ics;
mod html_to_text;
mod http_get;
pub mod pipe_get;
mod pipe_warn;
mod query_html;
pub mod to_pipes_json;
pub mod validate;

pub use expand_rrule::ExpandRrule;
pub use from_ics::FromIcs;
pub use html_to_text::HtmlToText;
pub use http_get::HttpGet;
pub use pipe_get::PipeGet;
pub use pipe_warn::PipeWarn;
pub use query_html::QueryHtml;
pub use to_pipes_json::ToPipesJson;
pub use validate::Validate;

/// What our commands share with the engine that runs them.
#[derive(Clone)]
pub struct Context {
    pub host: Arc<Host>,
    /// The running pipe's `refresh`, set when the pipe is loaded.
    pub refresh: Arc<Mutex<Option<SignedDuration>>>,
}

/// Registers every command in this module.
pub fn bind(working_set: &mut StateWorkingSet, context: &Context) {
    working_set.add_decl(Box::new(ExpandRrule));
    working_set.add_decl(Box::new(FromIcs));
    working_set.add_decl(Box::new(HtmlToText));
    working_set.add_decl(Box::new(HttpGet {
        context: context.clone(),
    }));
    working_set.add_decl(Box::new(PipeGet {
        context: context.clone(),
    }));
    working_set.add_decl(Box::new(PipeWarn {
        context: context.clone(),
    }));
    working_set.add_decl(Box::new(QueryHtml {
        context: context.clone(),
    }));
    working_set.add_decl(Box::new(ToPipesJson {
        context: context.clone(),
    }));
    working_set.add_decl(Box::new(Validate {
        context: context.clone(),
    }));
}

/// Why one of our commands failed, in a form both the engine's commands
/// and the plugin's can report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub title: String,
    pub message: String,
    /// What the error is about, such as the input or a flag's value, when
    /// that isn't the command itself.
    pub span: Option<Span>,
}

impl CommandError {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> CommandError {
        CommandError {
            title: title.into(),
            message: message.into(),
            span: None,
        }
    }

    /// Points the error at `span` instead of at the command.
    pub fn at(self, span: Span) -> CommandError {
        CommandError {
            span: Some(span),
            ..self
        }
    }

    /// The error, labeled at its own span or else at `head`.
    pub fn into_shell_error(self, head: Span) -> ShellError {
        let span = self.span.unwrap_or(head);
        ShellError::Generic(GenericError::new(self.title, self.message, span))
    }

    /// The error, labeled at its own span or else at `head`.
    pub fn into_labeled_error(self, head: Span) -> LabeledError {
        let span = self.span.unwrap_or(head);
        LabeledError::new(self.title).with_label(self.message, span)
    }
}

/// How many record failures an error message shows.
const SHOWN: usize = 5;

/// The first few record failures, one per line.
pub fn describe_failures(errors: &[RecordError]) -> String {
    let mut lines: Vec<String> = errors
        .iter()
        .take(SHOWN)
        .map(|e| format!("record {} {}: {}", e.index, e.pointer, e.message))
        .collect();
    if errors.len() > SHOWN {
        lines.push(format!("…and {} more", errors.len() - SHOWN));
    }
    lines.join("\n")
}

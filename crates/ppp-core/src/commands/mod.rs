//! Our own nushell commands.

use std::sync::{Arc, Mutex};

use jiff::SignedDuration;
use nu_protocol::engine::StateWorkingSet;

use crate::host::Host;

pub mod expand_rrule;
mod from_ics;
mod html_to_text;
mod http_get;
mod pipe_get;

pub use expand_rrule::ExpandRrule;
pub use from_ics::FromIcs;
pub use html_to_text::HtmlToText;
pub use http_get::HttpGet;
pub use pipe_get::PipeGet;

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
}

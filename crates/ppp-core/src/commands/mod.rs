//! Our own nushell commands.

use nu_protocol::engine::StateWorkingSet;

mod from_ics;

pub use from_ics::FromIcs;

/// Registers every command in this module.
pub fn bind(working_set: &mut StateWorkingSet) {
    working_set.add_decl(Box::new(FromIcs));
}

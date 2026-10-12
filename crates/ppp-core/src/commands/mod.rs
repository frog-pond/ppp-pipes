//! Our own nushell commands.

use nu_protocol::engine::StateWorkingSet;

mod expand_rrule;
mod from_ics;

pub use expand_rrule::ExpandRrule;
pub use from_ics::FromIcs;

/// Registers every command in this module.
pub fn bind(working_set: &mut StateWorkingSet) {
    working_set.add_decl(Box::new(ExpandRrule));
    working_set.add_decl(Box::new(FromIcs));
}

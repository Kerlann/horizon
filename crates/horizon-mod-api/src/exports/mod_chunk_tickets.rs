// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod mod_chunk_tickets;
pub use crate::mod_chunk_tickets::TicketOwner as NativeTicketOwner;
pub use crate::mod_chunk_tickets::{TicketSet, TicketValidation};

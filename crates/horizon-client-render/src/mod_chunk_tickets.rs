//! The chunk tickets native mods hold (NeoForge's `TicketController`): one
//! ticket per (mod, owner, chunk), so two machines in one chunk don't release
//! each other's. `Streaming` keeps the union with `/forceload`'s chunks in its
//! `forced` set, which is all loading and ticking read.
use std::collections::{BTreeSet, HashMap};

/// What a ticket belongs to: a block (a machine) or an entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TicketOwner {
    /// The block at `[x, y, z]`.
    Block([i32; 3]),
    /// The entity with this UUID.
    Entity(u128),
}

/// One chunk a mod holds loaded for an owner.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModTicket {
    /// The holding mod's `[mod] id`.
    pub mod_id: String,
    pub owner: TicketOwner,
    pub chunk: (i32, i32),
}

/// The tickets of one level.
#[derive(Debug, Default)]
pub struct ModTickets {
    held: BTreeSet<ModTicket>,
    per_chunk: HashMap<(i32, i32), u32>,
}

impl ModTickets {
    /// Tickets read from a save: those of a mod `loaded` accepts are held, the
    /// others are discarded (`TicketController`: a controller that isn't
    /// registered loses its tickets when the world loads).
    pub fn from_saved(saved: Vec<ModTicket>, loaded: impl Fn(&str) -> bool) -> Self {
        let mut tickets = Self::default();
        for ticket in saved {
            if loaded(&ticket.mod_id) {
                tickets.insert(ticket);
            }
        }
        tickets
    }
    /// Hold `ticket`: true when it was new.
    pub fn insert(&mut self, ticket: ModTicket) -> bool {
        let chunk = ticket.chunk;
        let new = self.held.insert(ticket);
        if new {
            *self.per_chunk.entry(chunk).or_default() += 1;
        }
        new
    }
    /// Release `ticket`: true when it was held.
    pub fn remove(&mut self, ticket: &ModTicket) -> bool {
        let held = self.held.remove(ticket);
        if held && let Some(count) = self.per_chunk.get_mut(&ticket.chunk) {
            *count -= 1;
            if *count == 0 {
                self.per_chunk.remove(&ticket.chunk);
            }
        }
        held
    }
    /// Whether any mod holds `chunk`.
    pub fn holds(&self, chunk: (i32, i32)) -> bool {
        self.per_chunk.contains_key(&chunk)
    }
    /// The chunks any mod holds.
    pub fn chunks(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.per_chunk.keys().copied()
    }
    /// The tickets mod `mod_id` holds, in order.
    pub fn of_mod<'a>(&'a self, mod_id: &'a str) -> impl Iterator<Item = &'a ModTicket> {
        self.held.iter().filter(move |t| t.mod_id == mod_id)
    }
    /// Every ticket to save.
    pub fn to_save(&self) -> impl Iterator<Item = &ModTicket> {
        self.held.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(mod_id: &str, owner: TicketOwner, chunk: (i32, i32)) -> ModTicket {
        ModTicket {
            mod_id: mod_id.into(),
            owner,
            chunk,
        }
    }

    #[test]
    fn a_chunk_is_held_while_any_ticket_holds_it() {
        let mut tickets = ModTickets::default();
        let (a, b) = (TicketOwner::Block([1, 2, 3]), TicketOwner::Entity(9));
        assert!(tickets.insert(ticket("m", a, (0, 0))));
        assert!(!tickets.insert(ticket("m", a, (0, 0))));
        assert!(tickets.insert(ticket("m", b, (0, 0))));
        // The same owner under another mod is another ticket.
        assert!(tickets.insert(ticket("n", a, (0, 0))));
        assert!(tickets.remove(&ticket("m", a, (0, 0))));
        assert!(!tickets.remove(&ticket("m", a, (0, 0))));
        assert!(tickets.holds((0, 0)));
        assert_eq!(tickets.of_mod("m").count(), 1);
        assert!(tickets.remove(&ticket("m", b, (0, 0))));
        assert!(tickets.remove(&ticket("n", a, (0, 0))));
        assert!(!tickets.holds((0, 0)));
        assert_eq!(tickets.chunks().count(), 0);
    }

    #[test]
    fn tickets_of_mods_not_loaded_are_discarded() {
        let saved = vec![
            ticket("here", TicketOwner::Entity(1), (4, 4)),
            ticket("gone", TicketOwner::Block([0, 0, 0]), (8, 8)),
        ];
        let tickets = ModTickets::from_saved(saved, |id| id == "here");
        assert!(tickets.holds((4, 4)));
        assert!(!tickets.holds((8, 8)));
        assert_eq!(tickets.to_save().count(), 1);
    }
}

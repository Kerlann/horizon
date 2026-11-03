//! The chunk tickets native mods hold (NeoForge's `TicketController`): one
//! ticket per (mod, owner, chunk), so two machines in one chunk don't release
//! each other's. `Streaming` keeps the union with `/forceload`'s chunks in its
//! `forced` set, which is all loading and ticking read.
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

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
    /// Whether the chunk joins the spawning chunks (inhabited time, thunder,
    /// a spawn attempt) with no player within 128 blocks
    /// (`forceChunk`'s `forceNaturalSpawning`). One owner holds the two kinds
    /// apart.
    pub natural_spawning: bool,
}

/// The chunks one owner holds, the two kinds apart (NeoForge's `TicketSet`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TicketSet {
    /// Chunks held without natural spawning.
    pub non_natural_spawning: Vec<[i32; 2]>,
    /// Chunks held with natural spawning forced.
    pub natural_spawning: Vec<[i32; 2]>,
}

impl TicketSet {
    fn push(&mut self, chunk: [i32; 2], natural_spawning: bool) {
        if natural_spawning {
            self.natural_spawning.push(chunk);
        } else {
            self.non_natural_spawning.push(chunk);
        }
    }
}

/// The tickets of one level.
#[derive(Debug, Default)]
pub struct ModTickets {
    held: BTreeSet<ModTicket>,
    /// Read from the save, not yet validated: they hold nothing until
    /// `take_loaded` hands them out and the survivors are inserted.
    loaded: Vec<ModTicket>,
    /// Per chunk: how many tickets hold it, and how many of those force natural spawning.
    per_chunk: HashMap<(i32, i32), (u32, u32)>,
}

impl ModTickets {
    /// Tickets read from a save: those of a mod `loaded` accepts wait in the
    /// loaded set for validation, the others are discarded (`TicketController`: a controller that isn't
    /// registered loses its tickets when the world loads).
    pub fn from_saved(saved: Vec<ModTicket>, loaded: impl Fn(&str) -> bool) -> Self {
        let mut tickets = Self::default();
        for ticket in saved {
            if loaded(&ticket.mod_id) {
                tickets.loaded.push(ticket);
            }
        }
        tickets
    }
    /// Hold `ticket`: true when it was new.
    pub fn insert(&mut self, ticket: ModTicket) -> bool {
        let (chunk, spawning) = (ticket.chunk, ticket.natural_spawning);
        let new = self.held.insert(ticket);
        if new {
            let counts = self.per_chunk.entry(chunk).or_default();
            counts.0 += 1;
            counts.1 += u32::from(spawning);
        }
        new
    }
    /// Release `ticket`: true when it was held.
    pub fn remove(&mut self, ticket: &ModTicket) -> bool {
        let held = self.held.remove(ticket);
        if held && let Some(counts) = self.per_chunk.get_mut(&ticket.chunk) {
            counts.0 -= 1;
            counts.1 -= u32::from(ticket.natural_spawning);
            if counts.0 == 0 {
                self.per_chunk.remove(&ticket.chunk);
            }
        }
        held
    }
    /// Whether any mod holds `chunk`.
    pub fn holds(&self, chunk: (i32, i32)) -> bool {
        self.per_chunk.contains_key(&chunk)
    }
    /// Whether a ticket that forces natural spawning holds `chunk`
    /// (`TicketStorage.shouldForceNaturalSpawning`).
    pub fn forces_natural_spawning(&self, chunk: (i32, i32)) -> bool {
        self.per_chunk
            .get(&chunk)
            .is_some_and(|counts| counts.1 > 0)
    }
    /// The chunks any mod holds.
    pub fn chunks(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.per_chunk.keys().copied()
    }
    /// The loaded tickets still to validate, taken out of the store.
    pub fn take_loaded(&mut self) -> Vec<ModTicket> {
        std::mem::take(&mut self.loaded)
    }
    /// Whether tickets loaded from the save wait for validation.
    pub fn has_loaded(&self) -> bool {
        !self.loaded.is_empty()
    }
    /// The tickets mod `mod_id` holds, in order.
    pub fn of_mod<'a>(&'a self, mod_id: &'a str) -> impl Iterator<Item = &'a ModTicket> {
        self.held.iter().filter(move |t| t.mod_id == mod_id)
    }
    /// Every ticket to save, the loaded ones not yet validated included.
    pub fn to_save(&self) -> impl Iterator<Item = &ModTicket> {
        self.held.iter().chain(&self.loaded)
    }
}

/// The tickets a level loaded, as `chunk_tickets_validate` hands them out
/// (NeoForge's `TicketHelper`). Every call acts as the mod marked as running:
/// it lists and removes only that mod's own tickets, block owners and entity
/// owners apart. The tickets of all mods and the removals made are the
/// game's: a removal goes to a [`TicketRemovals`] queue only the game holds.
pub struct TicketValidation {
    loaded: Vec<ModTicket>,
    removed: Arc<Mutex<Vec<ModTicket>>>,
}

impl std::fmt::Debug for TicketValidation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TicketValidation")
            .field("own", &self.own().count())
            .finish()
    }
}

/// The tickets the mods removed in one validation, for the game to release.
#[derive(Debug)]
pub struct TicketRemovals(Arc<Mutex<Vec<ModTicket>>>);

impl TicketRemovals {
    /// The tickets removed.
    pub fn into_removed(self) -> Vec<ModTicket> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl TicketValidation {
    /// A validation of `loaded` tickets (all mods'), and the queue of what is
    /// removed. For the game.
    #[doc(hidden)]
    pub fn new(loaded: Vec<ModTicket>) -> (Self, TicketRemovals) {
        let removed = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                loaded,
                removed: removed.clone(),
            },
            TicketRemovals(removed),
        )
    }
    /// The calling mod's block tickets: each block position with the chunks it
    /// holds without and with natural spawning (`TicketHelper.getBlockTickets`'
    /// `TicketSet`: `nonNaturalSpawning`, `naturalSpawning`), in position order.
    pub fn block_tickets(&self) -> Vec<([i32; 3], TicketSet)> {
        let mut grouped = std::collections::BTreeMap::<[i32; 3], TicketSet>::new();
        for (owner, chunk, spawning) in self.own() {
            if let TicketOwner::Block(pos) = owner {
                grouped.entry(pos).or_default().push(chunk, spawning);
            }
        }
        grouped.into_iter().collect()
    }
    /// The calling mod's entity tickets: each entity UUID with the chunks it
    /// holds without and with natural spawning (`TicketHelper.getEntityTickets`),
    /// in UUID order.
    pub fn entity_tickets(&self) -> Vec<(u128, TicketSet)> {
        let mut grouped = std::collections::BTreeMap::<u128, TicketSet>::new();
        for (owner, chunk, spawning) in self.own() {
            if let TicketOwner::Entity(uuid) = owner {
                grouped.entry(uuid).or_default().push(chunk, spawning);
            }
        }
        grouped.into_iter().collect()
    }
    /// Remove every ticket the block holds, of both kinds
    /// (`removeAllTickets(BlockPos)`): how many went.
    pub fn remove_block_tickets(&mut self, pos: [i32; 3]) -> usize {
        self.remove(TicketOwner::Block(pos), None)
    }
    /// Remove every ticket the entity holds, of both kinds
    /// (`removeAllTickets(UUID)`).
    pub fn remove_entity_tickets(&mut self, uuid: u128) -> usize {
        self.remove(TicketOwner::Entity(uuid), None)
    }
    /// Remove the block's ticket of one kind on one chunk
    /// (`removeTicket(BlockPos, ChunkPos, forceNaturalSpawning)`): whether it
    /// held one. The other kind on the chunk stays.
    pub fn remove_block_ticket(
        &mut self,
        pos: [i32; 3],
        chunk: [i32; 2],
        natural_spawning: bool,
    ) -> bool {
        self.remove(TicketOwner::Block(pos), Some((chunk, natural_spawning))) > 0
    }
    /// Remove the entity's ticket of one kind on one chunk
    /// (`removeTicket(UUID, ChunkPos, forceNaturalSpawning)`).
    pub fn remove_entity_ticket(
        &mut self,
        uuid: u128,
        chunk: [i32; 2],
        natural_spawning: bool,
    ) -> bool {
        self.remove(TicketOwner::Entity(uuid), Some((chunk, natural_spawning))) > 0
    }
    fn own(&self) -> impl Iterator<Item = (TicketOwner, [i32; 2], bool)> + '_ {
        let me = crate::crash::current_native_mod();
        self.loaded
            .iter()
            .filter(move |t| me == Some(t.mod_id.as_str()))
            .map(|t| (t.owner, [t.chunk.0, t.chunk.1], t.natural_spawning))
    }
    fn remove(&mut self, owner: TicketOwner, kind: Option<([i32; 2], bool)>) -> usize {
        let Some(me) = crate::crash::current_native_mod() else {
            return 0;
        };
        let (gone, kept): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.loaded).into_iter().partition(|t| {
                t.mod_id == me
                    && t.owner == owner
                    && kind.is_none_or(|([x, z], spawning)| {
                        t.chunk == (x, z) && t.natural_spawning == spawning
                    })
            });
        self.loaded = kept;
        let count = gone.len();
        self.removed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(gone);
        count
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
            natural_spawning: false,
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
    fn natural_spawning_is_forced_while_any_such_ticket_holds_the_chunk() {
        let mut tickets = ModTickets::default();
        let owner = TicketOwner::Entity(1);
        let spawning = ModTicket {
            natural_spawning: true,
            ..ticket("m", owner, (2, 2))
        };
        assert!(tickets.insert(ticket("m", owner, (2, 2))));
        assert!(tickets.holds((2, 2)) && !tickets.forces_natural_spawning((2, 2)));
        // The same owner holds both kinds apart.
        assert!(tickets.insert(spawning.clone()));
        assert!(tickets.forces_natural_spawning((2, 2)));
        assert!(tickets.remove(&spawning));
        assert!(tickets.holds((2, 2)) && !tickets.forces_natural_spawning((2, 2)));
    }

    #[test]
    fn tickets_of_mods_not_loaded_are_discarded() {
        let saved = vec![
            ticket("here", TicketOwner::Entity(1), (4, 4)),
            ticket("gone", TicketOwner::Block([0, 0, 0]), (8, 8)),
        ];
        let mut tickets = ModTickets::from_saved(saved, |id| id == "here");
        // Loaded tickets wait for validation and hold nothing yet.
        assert!(tickets.has_loaded() && !tickets.holds((4, 4)));
        assert_eq!(tickets.to_save().count(), 1);
        for ticket in tickets.take_loaded() {
            tickets.insert(ticket);
        }
        assert!(tickets.holds((4, 4)));
        assert!(!tickets.holds((8, 8)));
    }

    fn set(plain: &[[i32; 2]], spawning: &[[i32; 2]]) -> TicketSet {
        TicketSet {
            non_natural_spawning: plain.to_vec(),
            natural_spawning: spawning.to_vec(),
        }
    }

    fn in_mod(id: &'static str) -> crate::crash::NativeModGuard {
        crate::crash::enter_native_mod(crate::crash::NativeModCall {
            id,
            hook: "test",
            part: None,
        })
    }

    #[test]
    fn validation_shows_and_removes_only_the_callers_tickets() {
        let (block, entity) = (TicketOwner::Block([1, 2, 3]), TicketOwner::Entity(9));
        let (mut v, removals) = TicketValidation::new(vec![
            ticket("m", block, (0, 0)),
            ticket("m", block, (0, 1)),
            ticket("m", entity, (5, 5)),
            ticket("n", block, (0, 0)),
        ]);
        assert!(v.block_tickets().is_empty(), "no caller, no tickets");
        let guard = in_mod("m");
        assert_eq!(
            v.block_tickets(),
            vec![([1, 2, 3], set(&[[0, 0], [0, 1]], &[]))]
        );
        assert_eq!(v.entity_tickets(), vec![(9, set(&[[5, 5]], &[]))]);
        assert!(v.remove_block_ticket([1, 2, 3], [0, 1], false));
        assert!(!v.remove_block_ticket([1, 2, 3], [0, 1], false));
        assert_eq!(v.remove_entity_tickets(9), 1);
        assert_eq!(v.remove_block_tickets([1, 2, 3]), 1);
        drop(guard);
        let _guard = in_mod("n");
        assert_eq!(v.remove_block_tickets([1, 2, 3]), 1, "its own, not m's");
        assert_eq!(removals.into_removed().len(), 4);
    }

    #[test]
    fn one_mod_cannot_reach_another_mods_tickets_or_removals() {
        let owner = TicketOwner::Block([1, 2, 3]);
        let (mut v, removals) =
            TicketValidation::new(vec![ticket("a", owner, (0, 0)), ticket("b", owner, (1, 1))]);
        {
            let _a = in_mod("a");
            assert_eq!(v.remove_block_tickets([1, 2, 3]), 1);
        }
        let _b = in_mod("b");
        // b sees only its own, however it asks, and a's removal is not its to see.
        assert_eq!(v.block_tickets(), vec![([1, 2, 3], set(&[[1, 1]], &[]))]);
        assert!(!format!("{v:?}").contains("(0, 0)"));
        assert_eq!(v.remove_block_tickets([1, 2, 3]), 1);
        assert_eq!(v.remove_block_tickets([1, 2, 3]), 0);
        let removed = removals.into_removed();
        assert_eq!(removed.len(), 2);
        assert_eq!(removed[0].mod_id, "a");
    }

    #[test]
    fn validation_removes_either_kind_apart_and_remove_all_removes_both() {
        let owner = TicketOwner::Block([4, 5, 6]);
        let spawning = ModTicket {
            natural_spawning: true,
            ..ticket("m", owner, (3, 3))
        };
        let both = vec![ticket("m", owner, (3, 3)), spawning];
        let (mut v, removals) = TicketValidation::new(both.clone());
        let _m = in_mod("m");
        assert_eq!(
            v.block_tickets(),
            vec![([4, 5, 6], set(&[[3, 3]], &[[3, 3]]))]
        );
        // A singular removal takes the kind it names and leaves the other.
        assert!(v.remove_block_ticket([4, 5, 6], [3, 3], true));
        assert!(!v.remove_block_ticket([4, 5, 6], [3, 3], true));
        assert_eq!(v.block_tickets(), vec![([4, 5, 6], set(&[[3, 3]], &[]))]);
        assert!(v.remove_block_ticket([4, 5, 6], [3, 3], false));
        assert!(v.block_tickets().is_empty());
        // The other order, and the entity side.
        let (mut v, r2) = TicketValidation::new(both.clone());
        assert!(v.remove_block_ticket([4, 5, 6], [3, 3], false));
        assert_eq!(v.block_tickets(), vec![([4, 5, 6], set(&[], &[[3, 3]]))]);
        // Remove-all takes both.
        let (mut v, r3) = TicketValidation::new(both);
        assert_eq!(v.remove_block_tickets([4, 5, 6]), 2);
        assert_eq!(removals.into_removed().len(), 2);
        assert_eq!(r2.into_removed().len(), 1);
        assert_eq!(r3.into_removed().len(), 2);
        let entity = TicketOwner::Entity(7);
        let (mut v, removals) = TicketValidation::new(vec![
            ticket("m", entity, (1, 1)),
            ModTicket {
                natural_spawning: true,
                ..ticket("m", entity, (1, 1))
            },
        ]);
        assert!(v.remove_entity_ticket(7, [1, 1], true));
        assert_eq!(v.entity_tickets(), vec![(7, set(&[[1, 1]], &[]))]);
        assert!(removals.into_removed()[0].natural_spawning);
    }
}

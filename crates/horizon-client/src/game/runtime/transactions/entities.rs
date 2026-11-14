//! Entities a transaction adds and removes (#796). An abort takes out every entity that joined
//! since the transaction opened, with its attachment map, residency and saved memory, and tells
//! no one. A removal inside one only marks the entity, whose handle reads stale at once; the root
//! commit removes it as a removal outside one does, and an abort unmarks it. While one is open in
//! the level the game is in, mods hear of joins and leaves at the root commit only (the joins, in
//! the order the entities were added, before the removals' effects), and an abort tells them
//! nothing. Construction (`entity_constructing`, a mob's finalize) stays at the spawn
//! call.
//!
//! Join numbers, not ids, name what a transaction added: every kind takes one as it is put in its
//! list, the kinds whose id comes late too, so an aborted spawn just burns its id. Bolts and
//! thrown potions have none and are kept by their lists' lengths. A traveller that arrived from
//! another level inside the transaction is left where it arrived: travel is not journaled.
use super::super::{Game, commands::Target};
use horizon_client::join_seq::{JoinSeq, Joined as _};
use mod_api::EntityRef;
use std::collections::{HashMap, HashSet};

/// A raw record of `persist.anon` (kind and position bits) as it was: what an aborted spawn
/// puts back.
pub(in crate::game) type KeptAnon = (
    (String, [u64; 3]),
    Option<(horizon_core::geometry::Vec3, horizon_core::nbt::Tag)>,
);

/// What a transaction did to entities, for its close.
#[derive(Default)]
pub(super) struct EntityJournal {
    /// Below the join number of every entity added in the transaction (`JoinSeq::upcoming` as
    /// it opened).
    mark: JoinSeq,
    /// How many bolts and thrown potions there were as it opened.
    bolts: usize,
    potions: usize,
    /// The travellers that arrived from another level, by the ids they arrived under.
    arrivals: HashSet<u32>,
    /// The removals asked for, in order: each runs at the root commit.
    removals: Vec<Removal>,
    /// Each mob's `removed` flag before the transaction first removed it.
    mob_removed: HashMap<u32, bool>,
    /// The join and leave announcements held for the root commit, in order.
    announcements: Vec<Announcement>,
    /// Each raw record a spawn wrote over in `persist.anon` (by kind and position: an end
    /// crystal standing there keeps its own there), as it was before the first such spawn.
    anon: Vec<KeptAnon>,
}

/// A removal waiting for the root commit.
enum Removal {
    /// The mob is marked already; a killed one's effects that act on removal wait.
    Mob {
        id: u32,
        killed: bool,
    },
    Other(EntityRef),
}

/// What mods are told of entities joining and leaving, held while a transaction is open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::game) enum Announcement {
    /// `native_joins_flush` of the entities that joined through `through`, as the caller said
    /// whether they came with a chunk.
    Joins {
        loaded_from_disk: bool,
        through: JoinSeq,
    },
    /// `native_entity_left`.
    Left(u32),
    /// `native_leaves_flush`.
    Leaves,
}

impl EntityJournal {
    /// A nested commit: the parent's mark is earlier and stays.
    pub(super) fn merge(&mut self, child: Self) {
        self.arrivals.extend(child.arrivals);
        self.removals.extend(child.removals);
        for (id, was) in child.mob_removed {
            self.mob_removed.entry(id).or_insert(was);
        }
        for kept in child.anon {
            self.keep_anon(kept);
        }
        for announcement in child.announcements {
            self.announce(announcement);
        }
    }
    /// A nested abort: its arrivals stay out of the parent's abort too.
    pub(super) fn keep_arrivals(&mut self, arrivals: HashSet<u32>) {
        self.arrivals.extend(arrivals);
    }
    /// Keeps the first record each key had: a later spawn there wrote over this one's.
    fn keep_anon(&mut self, kept: KeptAnon) {
        if !self.anon.iter().any(|(key, _)| *key == kept.0) {
            self.anon.push(kept);
        }
    }
    fn announce(&mut self, announcement: Announcement) {
        match (self.announcements.last_mut(), announcement) {
            // One flush covers both when they were told alike.
            (
                Some(Announcement::Joins {
                    loaded_from_disk: held,
                    through: held_through,
                }),
                Announcement::Joins {
                    loaded_from_disk,
                    through,
                },
            ) if *held == loaded_from_disk => *held_through = (*held_through).max(through),
            (Some(last), announcement) if *last == announcement => {}
            _ => self.announcements.push(announcement),
        }
    }
}

impl Game<'_> {
    /// The journal a transaction opening now starts with.
    pub(super) fn entity_journal_open(&self) -> EntityJournal {
        EntityJournal {
            mark: JoinSeq::upcoming(),
            bolts: self.bolts.len(),
            potions: self.potions.thrown.len(),
            ..EntityJournal::default()
        }
    }
    /// Holds `announcement` for the root commit when a transaction is open in the level the game
    /// is in: true when it waits.
    pub(in crate::game) fn hold_announcement(&mut self, announcement: Announcement) -> bool {
        let dim = self.dim;
        match self.transactions.last_mut() {
            Some(level) if level.dimension == Some(dim) => {
                level.lifetimes.announce(announcement);
                true
            }
            _ => false,
        }
    }
    /// A spawn wrote over `was` in `persist.anon` under `key`: an abort puts it back.
    pub(in crate::game) fn journal_anon(&mut self, kept: KeptAnon) {
        if let Some(level) = self.transactions.last_mut() {
            level.lifetimes.keep_anon(kept);
        }
    }
    /// Entity `id` arrived from another level: an abort leaves it where it is.
    pub(in crate::game) fn journal_arrival(&mut self, id: u32) {
        if let Some(level) = self.transactions.last_mut() {
            level.lifetimes.arrivals.insert(id);
        }
    }
    /// Whether an open transaction removes `target`, which reads as gone until it closes.
    pub(in crate::game) fn removal_pending(&self, target: &Target) -> bool {
        let id = super::super::native_entities::target_id(*target);
        self.transactions.iter().any(|level| {
            (level.lifetimes.removals.iter())
                .any(|removal| matches!(removal, Removal::Other(e) if Some(e.id()) == id))
        })
    }
    /// `entity_remove` of `target`, which `entity` names, inside a transaction: marked now and
    /// removed at the root commit. False, with nothing done, when none is open.
    pub(in crate::game) fn journal_removal(
        &mut self,
        entity: EntityRef,
        target: Target,
        killed: bool,
    ) -> bool {
        let Some(level) = self.transactions.last_mut() else {
            return false;
        };
        match target {
            Target::Mob(id) => {
                for mob in self.mobs.iter_mut().filter(|m| m.id == id) {
                    level.lifetimes.mob_removed.entry(id).or_insert(mob.removed);
                    mob.removed = true;
                }
                level.lifetimes.removals.push(Removal::Mob { id, killed });
            }
            _ => level.lifetimes.removals.push(Removal::Other(entity)),
        }
        true
    }
    /// What `journal` left for the root commit: the joins it held, then its removals, in order,
    /// then the leaves it held. An entity added and removed in it is never announced, so it never
    /// leaves.
    pub(super) fn commit_entity_journal(&mut self, journal: EntityJournal) {
        let removed: Vec<u32> = (journal.removals.iter())
            .map(|removal| match removal {
                Removal::Mob { id, .. } => *id,
                Removal::Other(entity) => entity.id(),
            })
            .collect();
        for announcement in &journal.announcements {
            if let Announcement::Joins {
                loaded_from_disk,
                through,
            } = *announcement
            {
                self.flush_joins(loaded_from_disk, 0, &removed, Some(through));
            }
        }
        for removal in journal.removals {
            match removal {
                Removal::Mob { id, killed } => {
                    if killed {
                        self.mob_removal_effects(id);
                    }
                }
                Removal::Other(entity) => {
                    if let Some(target) = self.native_entity_target(entity) {
                        self.remove_target_now(entity, target);
                    }
                }
            }
        }
        for announcement in journal.announcements {
            match announcement {
                Announcement::Joins { .. } => {}
                Announcement::Left(id) => self.native_entity_left(id),
                Announcement::Leaves => self.native_leaves_flush(),
            }
        }
    }
    /// Undoes `journal`'s entity changes, after the rest of its transaction rolled back: the
    /// removed mobs back, the entities it added out. Its arrivals come back.
    pub(super) fn roll_back_entity_journal(&mut self, journal: EntityJournal) -> HashSet<u32> {
        for (id, was) in journal.mob_removed {
            for mob in self.mobs.iter_mut().filter(|m| m.id == id) {
                mob.removed = was;
            }
        }
        self.bolts.truncate(journal.bolts);
        self.potions.thrown.truncate(journal.potions);
        // One load when nothing joined anywhere since the transaction opened.
        if JoinSeq::latest() >= journal.mark {
            self.discard_joined_since(journal.mark, &journal.arrivals);
        }
        // After the discard: forgetting an aborted entity reads nothing keyed by position.
        for kept in journal.anon {
            self.persist.put_back_anon(kept);
        }
        journal.arrivals
    }
    /// Takes every entity that joined at `mark` or later out of its list, but the `arrivals`,
    /// with no hook (as a join a mod cancels), and forgets what was kept of it.
    fn discard_joined_since(&mut self, mark: JoinSeq, arrivals: &HashSet<u32>) {
        use super::super::entity::{Entity as _, with_entity};
        let added = |id: u32, join: JoinSeq| join >= mark && !arrivals.contains(&id);
        let mut gone = Vec::new();
        for handle in self.entity_handles_rev() {
            let (id, join) = with_entity!(self, handle, e => (e.id(), e.join_seq()));
            if added(id, join) {
                self.discard_entity(handle);
                gone.push(id);
            }
        }
        macro_rules! apart {
            ($($list:expr),* $(,)?) => {$(
                $list.retain(|e| {
                    let out = added(e.id, e.join_seq());
                    if out {
                        gone.push(e.id);
                    }
                    !out
                });
            )*};
        }
        apart!(
            self.crystals,
            self.cushions,
            self.paintings,
            self.item_frames,
            self.others,
            self.breath_clouds,
            self.fangs.list,
            self.ominous_spawners,
            self.resident_entities.knots,
        );
        if let Some(id) = (self.dragon.as_ref())
            .filter(|dragon| added(dragon.id, dragon.join_seq()))
            .map(|dragon| dragon.id)
        {
            self.discard_resident_dragon();
            gone.push(id);
        }
        if let Some(id) = (self.hook.as_ref())
            .filter(|hook| added(hook.id(), hook.join_seq()))
            .map(|hook| hook.id())
        {
            self.hook.discard();
            gone.push(id);
        }
        (self.native_lifecycle.pending_item_spawns).retain(|spawn| !gone.contains(&spawn.0));
        for id in gone.into_iter().filter(|&id| id != 0) {
            self.forget_entity(id);
        }
    }
}

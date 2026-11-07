//! Structural events from the numbered entity owners of one game instance.
//! Bindings ride with storage when dimensions are parked; no event holds Game.
use super::JoinSeq;
use mod_api::DimensionKey;
use std::{
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[path = "residency/table.rs"]
pub mod table;

/// Authoritative entity storage families, excluding the dragon's eight parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Mob,
    Item,
    Orb,
    Projectile,
    Stand,
    Frame,
    Painting,
    Boat,
    Cart,
    Other,
    Fangs,
    Crystal,
    Dragon,
    Tnt,
    Falling,
    Rocket,
    Trident,
    Spawner,
    Cloud,
    Cushion,
}

/// The dimension and family owning a list or singleton.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Owner {
    pub dimension: DimensionKey,
    pub kind: Kind,
}

/// Existing runtime identity, not a new residency token or attachment slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    pub id: NonZeroU32,
    pub join: JoinSeq,
}
impl Identity {
    /// A pending id of zero cannot name an entity in the lookup table.
    pub fn new(id: u32, join: JoinSeq) -> Option<Self> {
        Some(Self {
            id: NonZeroU32::new(id)?,
            join,
        })
    }
}

/// A structural operation; state-only writes emit nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Install(Identity),
    Remove(Identity),
    Extract(Identity),
    PutBack(Identity),
    Lifted,
    Restored,
}

/// One operation on one authoritative owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub owner: Owner,
    pub change: Change,
}

#[derive(Debug, Default)]
struct Pending {
    events: Mutex<Vec<Event>>,
    ready: AtomicBool,
}

/// Shared only by the owners of one game instance and its parked dimensions.
/// Cloning shares the sink, never an entity or a reference back to the game.
#[derive(Clone, Debug, Default)]
pub struct Journal(Arc<Pending>);
impl Journal {
    /// Record an operation at the storage boundary, in mutation order.
    pub fn push(&self, event: Event) {
        let mut events = self.0.events.lock().unwrap();
        debug_assert!(events.len() < 1_000_000, "entity journal was not drained");
        events.push(event);
        self.0.ready.store(true, Ordering::Release);
    }

    /// Exchange the pending batch with an empty reusable scratch buffer.
    /// Consumers run after releasing the lock and can safely produce events.
    /// An idle drain takes no mutex and performs no allocation.
    pub fn drain_into(&self, scratch: &mut Vec<Event>) {
        assert!(scratch.is_empty(), "previous entity batch was not consumed");
        if !self.0.ready.load(Ordering::Acquire) {
            return;
        }
        let mut events = self.0.events.lock().unwrap();
        std::mem::swap(&mut *events, scratch);
        self.0.ready.store(false, Ordering::Release);
    }

    /// Forget queued operations when the owning game resets its table/epoch.
    /// Existing storage bindings continue using this sink after the reset.
    pub fn clear(&self) {
        let mut events = self.0.events.lock().unwrap();
        events.clear();
        self.0.ready.store(false, Ordering::Release);
    }
}

/// Storage-local identity accessor and journal sink, never a Game reference.
#[derive(Debug)]
pub(super) struct Binding<T> {
    journal: Journal,
    owner: Owner,
    identity: fn(&T) -> (u32, JoinSeq),
}
impl Journal {
    pub(super) fn bind<T>(&self, owner: Owner, identity: fn(&T) -> (u32, JoinSeq)) -> Binding<T> {
        Binding {
            journal: self.clone(),
            owner,
            identity,
        }
    }
}
impl<T> Clone for Binding<T> {
    fn clone(&self) -> Self {
        Self {
            journal: self.journal.clone(),
            owner: self.owner,
            identity: self.identity,
        }
    }
}
impl<T> Binding<T> {
    pub(super) fn same(&self, other: &Self) -> bool {
        self.owner == other.owner && Arc::ptr_eq(&self.journal.0, &other.journal.0)
    }
    pub(super) fn marker(&self, change: Change) {
        self.journal.push(Event {
            owner: self.owner,
            change,
        });
    }
    pub(super) fn identity(&self, id: u32, join: JoinSeq, change: fn(Identity) -> Change) {
        if let Some(identity) = Identity::new(id, join) {
            self.marker(change(identity));
        }
    }
    pub(super) fn entity(&self, entity: &T, change: fn(Identity) -> Change) {
        let (id, join) = (self.identity)(entity);
        self.identity(id, join, change);
    }
    pub(super) fn all(&self, entities: &[T], change: fn(Identity) -> Change) {
        for entity in entities {
            self.entity(entity, change);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(dimension: DimensionKey, kind: Kind, change: Change) -> Event {
        Event {
            owner: Owner { dimension, kind },
            change,
        }
    }

    #[test]
    fn drains_keep_cross_dimension_order_and_events_produced_by_consumers() {
        let journal = Journal::default();
        let other_dimension = journal.clone();
        let owner = Identity::new(7, JoinSeq::next()).unwrap();
        let install = event(DimensionKey::OVERWORLD, Kind::Other, Change::Install(owner));
        let extract = event(DimensionKey::NETHER, Kind::Falling, Change::Extract(owner));
        let terminal = event(DimensionKey::NETHER, Kind::Falling, Change::PutBack(owner));
        journal.push(install);
        other_dimension.push(extract);
        let mut scratch = Vec::new();
        journal.drain_into(&mut scratch);
        assert_eq!(scratch, [install, extract]);
        // A consumer emitting an event cannot deadlock or lose that next batch.
        for _ in scratch.drain(..) {
            other_dimension.push(terminal);
        }
        journal.drain_into(&mut scratch);
        assert_eq!(scratch, [terminal, terminal]);
        scratch.clear();
        let capacity = scratch.capacity();
        journal.drain_into(&mut scratch);
        assert!(scratch.is_empty());
        assert_eq!(scratch.capacity(), capacity);
    }

    #[test]
    fn reset_discards_old_operations_without_orphaning_parked_producers() {
        let journal = Journal::default();
        let parked = journal.clone();
        let old = event(DimensionKey::OVERWORLD, Kind::Mob, Change::Lifted);
        let new = event(DimensionKey::END, Kind::Dragon, Change::Restored);
        journal.push(old);
        journal.clear();
        let mut scratch = Vec::new();
        journal.drain_into(&mut scratch);
        assert!(scratch.is_empty());
        parked.push(new);
        journal.drain_into(&mut scratch);
        assert_eq!(scratch, [new]);
    }

    #[test]
    fn concurrent_producers_are_delivered_once_in_each_producers_order() {
        let journal = Journal::default();
        let threads: Vec<_> = [DimensionKey::OVERWORLD, DimensionKey::NETHER]
            .into_iter()
            .map(|dimension| {
                let producer = journal.clone();
                std::thread::spawn(move || {
                    for id in 1..=200 {
                        producer.push(event(
                            dimension,
                            Kind::Mob,
                            Change::Install(Identity::new(id, JoinSeq::next()).unwrap()),
                        ));
                    }
                })
            })
            .collect();
        let mut received = Vec::new();
        let mut scratch = Vec::new();
        while threads.iter().any(|t| !t.is_finished()) {
            journal.drain_into(&mut scratch);
            received.append(&mut scratch);
            std::thread::yield_now();
        }
        for thread in threads {
            thread.join().unwrap();
        }
        journal.drain_into(&mut scratch);
        received.append(&mut scratch);
        for dimension in [DimensionKey::OVERWORLD, DimensionKey::NETHER] {
            let ids: Vec<_> = received
                .iter()
                .filter_map(|e| {
                    if let Change::Install(entity) = e.change
                        && e.owner.dimension == dimension
                    {
                        Some(entity.id.get())
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(ids, (1..=200).collect::<Vec<_>>());
        }
    }
}

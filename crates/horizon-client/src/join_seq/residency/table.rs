//! A journal-fed index. Every hit is checked against its named storage owner;
//! there is no owner reconciliation scan or reference back to the game.
use super::{Change, Event, Identity, Journal, Owner};
use crate::join_seq::JoinSeq;
use horizon_core::fast_hash::{FxHashMap, FxHashSet};
use std::num::NonZeroU32;

/// Stable storage identity, never a cached element index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub owner: Owner,
    pub join: JoinSeq,
}

/// A candidate obtained by locating the join in exactly the named owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceEntity<T> {
    pub id: u32,
    pub value: T,
}

/// A validated result or a narrowly scoped temporary absence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup<T> {
    Missing,
    Taken,
    Present { location: Location, value: T },
}

/// One game's index, shared by active and parked dimensions through the journal.
/// Runtime ids are allocated by the game's one counter, not per dimension.
pub struct EntityTable {
    journal: Journal,
    pending: Vec<Event>,
    rows: FxHashMap<NonZeroU32, Location>,
    extracted: FxHashMap<NonZeroU32, Location>,
    lifted: FxHashSet<Owner>,
}
impl EntityTable {
    /// Observe this game's existing structural-event sink.
    pub fn new(journal: Journal) -> Self {
        Self {
            journal,
            pending: Vec::new(),
            rows: FxHashMap::default(),
            extracted: FxHashMap::default(),
            lifted: FxHashSet::default(),
        }
    }
    /// Check the current owner and id once, returning its already located value.
    /// The callback must only inspect the supplied owner, not scan other kinds
    /// or dimensions. A missing or mismatched source retires the stale cache row.
    pub fn lookup<T>(
        &mut self,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> Lookup<T> {
        let Some(id) = NonZeroU32::new(id) else {
            return Lookup::Missing;
        };
        self.drain();
        let Some(&location) = self.rows.get(&id) else {
            return Lookup::Missing;
        };
        if let Some(source) = locate(location) {
            if source.id == id.get() {
                return Lookup::Present {
                    location,
                    value: source.value,
                };
            }
            // Finding a different entity is corruption, not a lifted absence.
        } else if self.extracted.get(&id) == Some(&location)
            || self.lifted.contains(&location.owner)
        {
            return Lookup::Taken;
        }
        forget(&mut self.rows, id, location);
        forget(&mut self.extracted, id, location);
        Lookup::Missing
    }
    /// Apply pending operations in mutation order, reusing the event buffer.
    pub fn drain(&mut self) {
        self.journal.drain_into(&mut self.pending);
        for &event in &self.pending {
            let location = |identity: Identity| Location {
                owner: event.owner,
                join: identity.join,
            };
            match event.change {
                Change::Install(identity) | Change::PutBack(identity) => {
                    let location = location(identity);
                    if let Some(old) = self.rows.insert(identity.id, location) {
                        debug_assert_eq!(old, location, "duplicate loaded entity id");
                    }
                    self.extracted.remove(&identity.id);
                }
                Change::Remove(identity) => {
                    let location = location(identity);
                    forget(&mut self.rows, identity.id, location);
                    forget(&mut self.extracted, identity.id, location);
                }
                Change::Extract(identity) => {
                    let location = location(identity);
                    if self.rows.get(&identity.id) == Some(&location) {
                        self.extracted.insert(identity.id, location);
                    }
                }
                Change::Lifted => {
                    self.lifted.insert(event.owner);
                }
                Change::Restored => {
                    self.lifted.remove(&event.owner);
                }
            }
        }
        self.pending.clear();
    }
    /// Expire every open marker before the debug leak assertion. Release builds
    /// therefore validate a leaked owner as missing, never permanently Taken.
    /// Work is proportional to open markers, not the number of resident rows.
    pub fn end_tick(&mut self) {
        self.drain();
        let leaked = self.extracted.len() + self.lifted.len();
        self.extracted.clear();
        self.lifted.clear();
        debug_assert_eq!(leaked, 0, "entity residency operation did not finish");
    }
    /// Clear the old epoch without detaching active or parked producer bindings.
    pub fn reset(&mut self) {
        self.journal.clear();
        self.pending.clear();
        self.rows.clear();
        self.extracted.clear();
        self.lifted.clear();
    }
}
fn forget(map: &mut FxHashMap<NonZeroU32, Location>, id: NonZeroU32, location: Location) {
    if map.get(&id) == Some(&location) {
        map.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::join_seq::residency::{DimensionKey, Kind};
    const HOME: Owner = Owner {
        dimension: DimensionKey::OVERWORLD,
        kind: Kind::Mob,
    };

    struct Fixture {
        journal: Journal,
        table: EntityTable,
        source: FxHashMap<Location, SourceEntity<usize>>,
    }
    impl Fixture {
        fn new() -> Self {
            let journal = Journal::default();
            Self {
                table: EntityTable::new(journal.clone()),
                journal,
                source: FxHashMap::default(),
            }
        }
        fn admit(&mut self, id: u32, owner: Owner) -> Location {
            let location = Location {
                owner,
                join: JoinSeq::next(),
            };
            self.source.insert(location, SourceEntity { id, value: 42 });
            self.emit(
                location,
                Change::Install(Identity::new(id, location.join).unwrap()),
            );
            location
        }
        fn emit(&self, location: Location, change: Change) {
            self.journal.push(Event {
                owner: location.owner,
                change,
            });
        }
        fn lookup(&mut self, id: u32) -> Lookup<usize> {
            self.table
                .lookup(id, |location| self.source.get(&location).copied())
        }
    }

    #[test]
    fn validated_hits_keep_the_located_value_and_purge_both_kinds_of_stale_row() {
        let mut f = Fixture::new();
        let location = f.admit(7, HOME);
        assert_eq!(
            f.lookup(7),
            Lookup::Present {
                location,
                value: 42
            }
        );
        for wrong_source in [None, Some(SourceEntity { id: 8, value: 99 })] {
            match wrong_source {
                None => {
                    f.source.remove(&location);
                }
                Some(source) => {
                    f.source.insert(location, source);
                }
            }
            assert_eq!(f.lookup(7), Lookup::Missing);
            // A physical row alone must not resurrect a purged cache entry.
            f.source.insert(location, SourceEntity { id: 7, value: 42 });
            assert_eq!(f.lookup(7), Lookup::Missing);
            f.emit(
                location,
                Change::Install(Identity::new(7, location.join).unwrap()),
            );
            assert_eq!(
                f.lookup(7),
                Lookup::Present {
                    location,
                    value: 42
                }
            );
        }
        f.admit(1, HOME);
        assert_eq!(
            f.table
                .lookup::<()>(0, |_| panic!("zero id inspected storage")),
            Lookup::Missing
        );
    }

    #[test]
    fn extraction_and_lift_only_hide_missing_sources_and_finish_in_order() {
        let mut f = Fixture::new();
        let location = f.admit(7, HOME);
        let identity = Identity::new(7, location.join).unwrap();
        let source = f.source.remove(&location).unwrap();
        f.emit(location, Change::Extract(identity));
        assert_eq!(f.lookup(7), Lookup::Taken);
        f.source.insert(location, source);
        // Exposed replay storage is readable even before its marker closes.
        assert_eq!(
            f.lookup(7),
            Lookup::Present {
                location,
                value: 42
            }
        );
        f.emit(location, Change::PutBack(identity));
        f.table.end_tick();
        f.source.remove(&location);
        f.emit(location, Change::Lifted);
        let spawned = f.admit(8, HOME);
        assert_eq!(f.lookup(7), Lookup::Taken);
        assert_eq!(
            f.lookup(8),
            Lookup::Present {
                location: spawned,
                value: 42
            }
        );
        f.source.insert(location, source);
        f.emit(location, Change::Restored);
        f.table.end_tick();
        assert_eq!(
            f.lookup(7),
            Lookup::Present {
                location,
                value: 42
            }
        );
        // Retirement is authoritative, even if an unreported old view remains.
        f.emit(location, Change::Extract(identity));
        f.emit(location, Change::Remove(identity));
        assert_eq!(f.lookup(7), Lookup::Missing);
        f.table.end_tick();
        f.source.remove(&location);
    }

    #[test]
    fn an_id_mismatch_is_not_disguised_as_a_temporary_absence() {
        for lift in [false, true] {
            let mut f = Fixture::new();
            let location = f.admit(7, HOME);
            f.source.get_mut(&location).unwrap().id = 8;
            let identity = Identity::new(7, location.join).unwrap();
            f.emit(
                location,
                if lift {
                    Change::Lifted
                } else {
                    Change::Extract(identity)
                },
            );
            assert_eq!(f.lookup(7), Lookup::Missing);
            f.source.get_mut(&location).unwrap().id = 7;
            assert_eq!(f.lookup(7), Lookup::Missing);
            if lift {
                f.emit(location, Change::Restored);
            }
            f.table.end_tick();
        }
    }

    #[test]
    fn parked_transfer_and_late_retirement_cannot_erase_a_new_owner() {
        let mut f = Fixture::new();
        let old = f.admit(7, HOME);
        assert!(matches!(f.lookup(7), Lookup::Present { .. }));
        let new = Location {
            owner: Owner {
                dimension: DimensionKey::NETHER,
                kind: Kind::Other,
            },
            ..old
        };
        let source = f.source.remove(&old).unwrap();
        let identity = Identity::new(7, old.join).unwrap();
        f.emit(old, Change::Remove(identity));
        f.source.insert(new, source);
        f.emit(new, Change::Install(identity));
        assert_eq!(
            f.lookup(7),
            Lookup::Present {
                location: new,
                value: 42
            }
        );
        f.emit(old, Change::Remove(identity));
        assert_eq!(
            f.lookup(7),
            Lookup::Present {
                location: new,
                value: 42
            }
        );
        f.source.remove(&new);
        f.emit(new, Change::Remove(identity));
        let replacement = f.admit(7, new.owner);
        f.emit(new, Change::Remove(identity));
        assert_eq!(
            f.lookup(7),
            Lookup::Present {
                location: replacement,
                value: 42
            }
        );
    }

    #[test]
    fn end_tick_expires_leaks_before_debug_panics_and_release_lookups_miss() {
        for lift in [false, true] {
            let mut f = Fixture::new();
            let location = f.admit(7, HOME);
            f.source.remove(&location);
            let identity = Identity::new(7, location.join).unwrap();
            f.emit(
                location,
                if lift {
                    Change::Lifted
                } else {
                    Change::Extract(identity)
                },
            );
            assert_eq!(f.lookup(7), Lookup::Taken);
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.table.end_tick()));
            assert_eq!(result.is_err(), cfg!(debug_assertions));
            assert_eq!(f.lookup(7), Lookup::Missing);
            f.table.end_tick();
        }
    }

    #[test]
    fn reset_clears_pending_rows_and_markers_but_keeps_producers_connected() {
        let mut f = Fixture::new();
        let old = f.admit(7, HOME);
        assert!(matches!(f.lookup(7), Lookup::Present { .. }));
        f.emit(old, Change::Extract(Identity::new(7, old.join).unwrap()));
        f.emit(old, Change::Lifted);
        f.table.drain();
        f.admit(9, HOME);
        f.table.reset();
        assert_eq!(f.lookup(7), Lookup::Missing);
        assert_eq!(f.lookup(9), Lookup::Missing);
        f.table.end_tick();
        let new = f.admit(8, HOME);
        assert_eq!(
            f.lookup(8),
            Lookup::Present {
                location: new,
                value: 42
            }
        );
    }
}

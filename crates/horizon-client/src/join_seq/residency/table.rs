//! A journal-fed index. Every hit is checked against its named storage owner;
//! there is no owner reconciliation scan or reference back to the game.
use super::{Change, DimensionKey, Event, Identity, IdentityDetails, Journal, Owner};
use crate::join_seq::JoinSeq;
use horizon_core::fast_hash::FxHashMap;
use std::num::NonZeroU32;

/// Stable storage identity, never a cached element index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub owner: Owner,
    pub join: JoinSeq,
    pub source: super::Source,
}

/// A candidate obtained by locating the join in exactly the named owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceEntity<T> {
    pub id: u32,
    pub value: T,
}

/// A validated result or a narrowly scoped temporary absence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityLookup<T> {
    Missing,
    Taken,
    Found { location: Location, value: T },
}

/// Unfinished operations expired at the tick boundary, never retained as Taken.
/// `lifts` counts outstanding passes, including nested passes of the same owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnbalancedPass {
    pub extractions: usize,
    pub lifts: usize,
}

/// One game's index, shared by active and parked dimensions through the journal.
/// Runtime ids are allocated by the game's one counter, not per dimension.
struct ResidentRow<R> {
    location: Location,
    details: IdentityDetails,
    cached: Option<R>,
}

pub struct EntityTable<R = ()> {
    journal: Journal,
    pending: Vec<Event>,
    rows: FxHashMap<NonZeroU32, ResidentRow<R>>,
    extracted: FxHashMap<NonZeroU32, Location>,
    extracted_by_owner: FxHashMap<Owner, usize>,
    lifted: FxHashMap<Owner, usize>,
}
impl<R> EntityTable<R> {
    /// Observe this game's existing structural-event sink.
    pub fn new(journal: Journal) -> Self {
        Self {
            journal,
            pending: Vec::new(),
            rows: FxHashMap::default(),
            extracted: FxHashMap::default(),
            extracted_by_owner: FxHashMap::default(),
            lifted: FxHashMap::default(),
        }
    }
    /// Check the current owner and id once, returning its already located value.
    /// The callback must only inspect the supplied owner, not scan other kinds
    /// or dimensions. A missing or mismatched source retires the stale cache row.
    pub fn entity_lookup<T>(
        &mut self,
        dimension: &DimensionKey,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> EntityLookup<T> {
        self.lookup(Some(dimension), id, locate)
    }
    /// Resolve an id across all loaded dimensions. The resolver still validates
    /// exactly the named owner; it must not swap levels or search other owners.
    pub fn server_entity_lookup<T>(
        &mut self,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> EntityLookup<T> {
        self.lookup(None, id, locate)
    }
    fn lookup<T>(
        &mut self,
        dimension: Option<&DimensionKey>,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> EntityLookup<T> {
        let Some(id) = NonZeroU32::new(id) else {
            return EntityLookup::Missing;
        };
        self.drain();
        let Some(location) = self.rows.get(&id).map(|row| row.location) else {
            return EntityLookup::Missing;
        };
        if dimension.is_some_and(|dimension| location.owner.dimension != *dimension) {
            return EntityLookup::Missing;
        }
        if let Some(source) = locate(location) {
            if source.id == id.get() {
                return EntityLookup::Found {
                    location,
                    value: source.value,
                };
            }
            // Finding a different entity is corruption, not a lifted absence.
        } else if self.extracted.get(&id) == Some(&location)
            || self.lifted.contains_key(&location.owner)
        {
            return EntityLookup::Taken;
        }
        forget_row(&mut self.rows, id, location);
        forget_extraction(
            &mut self.extracted,
            &mut self.extracted_by_owner,
            id,
            Some(location),
        );
        EntityLookup::Missing
    }
    /// Validate the source once, then borrow its cached publication. A lifted
    /// source may have a row; a missing or corrupt source never does. The cache
    /// shares the authoritative identity record, so retirement needs no second
    /// journal consumer, tombstones or reconciliation sweep.
    pub fn entity_read_lookup<T>(
        &mut self,
        dimension: &DimensionKey,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> (EntityLookup<T>, Option<&R>) {
        self.read_lookup(Some(dimension), id, locate)
    }

    /// Cached counterpart of server-wide validated lookup.
    pub fn server_entity_read_lookup<T>(
        &mut self,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> (EntityLookup<T>, Option<&R>) {
        self.read_lookup(None, id, locate)
    }

    /// Resolve one admitted source and borrow its exact generation's read
    /// metadata in the same lookup. Unlike a successful-source-only lookup,
    /// this retains the admitted address when the source is physically taken.
    /// Family/UUID and pending-dependency checks can then inspect the borrowed
    /// metadata without resolving the identity or touching its storage again.
    pub fn server_admitted_read_lookup<T>(
        &mut self,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> Option<(Location, EntityLookup<T>, Option<&R>)> {
        let state = self.lookup(None, id, locate);
        if matches!(state, EntityLookup::Missing) {
            return None;
        }
        let row = self.rows.get(&NonZeroU32::new(id)?)?;
        Some((row.location, state, row.cached.as_ref()))
    }

    fn read_lookup<T>(
        &mut self,
        dimension: Option<&DimensionKey>,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> (EntityLookup<T>, Option<&R>) {
        let result = self.lookup(dimension, id, locate);
        let cached = match &result {
            EntityLookup::Missing => None,
            _ => NonZeroU32::new(id)
                .and_then(|id| self.rows.get(&id))
                .and_then(|row| row.cached.as_ref()),
        };
        (result, cached)
    }

    /// Validate one admitted source and return its canonical location, including
    /// an explicitly lifted source. The caller separately validates world epoch
    /// and authority. This never reconstructs generation from cached payloads.
    pub fn server_admitted_location<T>(
        &mut self,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> Option<(Location, EntityLookup<T>)> {
        let state = self.lookup(None, id, locate);
        if matches!(state, EntityLookup::Missing) {
            return None;
        }
        let row = self.rows.get(&NonZeroU32::new(id)?)?;
        Some((row.location, state))
    }

    /// Publish captures of already admitted sources. Exact owner/join agreement
    /// prevents a delayed publication from replacing a transferred generation.
    /// Drain once per batch; no source scan or per-removal metadata is needed.
    pub fn publish_rows(&mut self, rows: impl IntoIterator<Item = (u32, Location, R)>) -> usize {
        self.publish_rows_merging(rows, |_, prepared| prepared)
    }

    /// Install fully prepared captures and merge retained patches only AFTER
    /// final generation validation. Capture failure must occur before this call,
    /// so an accepted overlay cannot be lost by a fallible source projection.
    /// The merge is pure payload work: no source/Game callbacks or effects.
    pub fn publish_rows_merging(
        &mut self,
        rows: impl IntoIterator<Item = (u32, Location, R)>,
        mut merge: impl FnMut(Option<R>, R) -> R,
    ) -> usize {
        self.drain();
        let mut published = 0;
        for (id, location, cached) in rows {
            let Some(row) = NonZeroU32::new(id).and_then(|id| self.rows.get_mut(&id)) else {
                continue;
            };
            if row.location == location {
                row.cached = Some(merge(row.cached.take(), cached));
                published += 1;
            }
        }
        published
    }

    /// Edit a publication only while its exact admitted generation still owns
    /// the id. Used for dependent projections and pending light overlays; the
    /// caller owns patch semantics and must not perform engine effects here.
    pub fn edit_cached_row(
        &mut self,
        id: u32,
        location: Location,
        edit: impl FnOnce(&mut R),
    ) -> bool {
        self.drain();
        let Some(row) = NonZeroU32::new(id).and_then(|id| self.rows.get_mut(&id)) else {
            return false;
        };
        if row.location != location {
            return false;
        }
        let Some(cached) = row.cached.as_mut() else {
            return false;
        };
        edit(cached);
        true
    }

    /// Whether a source collection can be scanned without silently omitting a
    /// loan. `covered` names an individually extracted source actually supplied
    /// by the caller's explicit loan, never an arbitrary disjoint live view.
    /// Whole-owner passes remain unavailable; one source cannot cover a list.
    /// Journal-derived counts make both checks O(1), without a resident scan.
    pub fn owner_physically_available(
        &mut self,
        owner: Owner,
        covered: Option<(u32, Location)>,
    ) -> bool {
        self.drain();
        if self.lifted.contains_key(&owner) {
            return false;
        }
        let covered = usize::from(covered.is_some_and(|(id, at)| {
            at.owner == owner
                && NonZeroU32::new(id).and_then(|id| self.extracted.get(&id)) == Some(&at)
        }));
        self.extracted_by_owner.get(&owner).copied().unwrap_or(0) == covered
    }

    /// Apply pending operations in mutation order, reusing the event buffer.
    pub fn drain(&mut self) {
        self.journal.drain_into(&mut self.pending);
        for event in self.pending.drain(..) {
            let location = |identity: Identity| Location {
                owner: event.owner,
                join: identity.join,
                source: identity.source,
            };
            match event.change {
                Change::Install(identity) | Change::PutBack(identity) => {
                    let location = location(identity);
                    let row = self.rows.entry(identity.id).or_insert(ResidentRow {
                        location,
                        details: identity.details,
                        cached: None,
                    });
                    debug_assert_eq!(row.location, location, "duplicate loaded entity id");
                    if row.location != location || row.details != identity.details {
                        row.location = location;
                        row.details = identity.details;
                        row.cached = None;
                    }
                    forget_extraction(
                        &mut self.extracted,
                        &mut self.extracted_by_owner,
                        identity.id,
                        None,
                    );
                }
                Change::Remove(identity) => {
                    let location = location(identity);
                    forget_row(&mut self.rows, identity.id, location);
                    forget_extraction(
                        &mut self.extracted,
                        &mut self.extracted_by_owner,
                        identity.id,
                        Some(location),
                    );
                }
                Change::Extract(identity) => {
                    let location = location(identity);
                    if self.rows.get(&identity.id).map(|row| row.location) == Some(location)
                        && self.extracted.get(&identity.id) != Some(&location)
                    {
                        forget_extraction(
                            &mut self.extracted,
                            &mut self.extracted_by_owner,
                            identity.id,
                            None,
                        );
                        self.extracted.insert(identity.id, location);
                        *self.extracted_by_owner.entry(location.owner).or_default() += 1;
                    }
                }
                Change::Lifted => {
                    *self.lifted.entry(event.owner).or_default() += 1;
                }
                Change::Restored => {
                    if let Some(depth) = self.lifted.get_mut(&event.owner) {
                        *depth -= 1;
                        if *depth == 0 {
                            self.lifted.remove(&event.owner);
                        }
                    }
                }
            }
        }
    }
    /// Drain and expire open markers before reporting an unfinished operation.
    /// Log the counts in every build; debug builds then assert, while release
    /// returns a typed error. Later lookups never remain permanently Taken.
    /// Valid ticks allocate nothing; work scales with markers, not resident rows.
    pub fn end_tick(&mut self) -> Result<(), UnbalancedPass> {
        self.drain();
        let leaked = self.extracted.len() + self.lifted.len();
        let unfinished = (leaked != 0).then(|| UnbalancedPass {
            extractions: self.extracted.len(),
            lifts: self.lifted.values().sum(),
        });
        self.extracted.clear();
        self.extracted_by_owner.clear();
        self.lifted.clear();
        if let Some(error) = unfinished {
            log::error!("entity residency operation did not finish: {error:?}");
            debug_assert_eq!(leaked, 0, "entity residency operation did not finish");
            return Err(error);
        }
        Ok(())
    }
    /// Resolve once and return immutable metadata from that same primary row.
    /// Real loans keep metadata even without an available physical payload.
    pub fn server_admitted_identity<T>(
        &mut self,
        id: u32,
        locate: impl FnOnce(Location) -> Option<SourceEntity<T>>,
    ) -> Option<(Location, EntityLookup<T>, IdentityDetails)> {
        let state = self.lookup(None, id, locate);
        if matches!(state, EntityLookup::Missing) {
            return None;
        }
        let row = self.rows.get(&NonZeroU32::new(id)?)?;
        Some((row.location, state, row.details))
    }

    /// Candidate-only metadata, including Taken. Collect into a reusable buffer
    /// and release this borrow before invoking any source or UUID resolver.
    pub fn admitted_metadata(
        &mut self,
    ) -> impl Iterator<Item = (u32, Location, IdentityDetails)> + '_ {
        self.drain();
        self.rows
            .iter()
            .map(|(id, row)| (id.get(), row.location, row.details))
    }

    /// Clone the current producer authority when binding storage.
    pub fn producer(&self) -> Journal {
        self.journal.clone()
    }
    /// Retire the epoch and clear cached state. Rebind every retained owner.
    pub fn reset(&mut self) {
        self.journal.reset_epoch();
        self.pending.clear();
        self.rows.clear();
        self.extracted.clear();
        self.extracted_by_owner.clear();
        self.lifted.clear();
    }
}
fn forget_row<R>(
    map: &mut FxHashMap<NonZeroU32, ResidentRow<R>>,
    id: NonZeroU32,
    location: Location,
) {
    if map.get(&id).map(|row| row.location) == Some(location) {
        map.remove(&id);
    }
}
fn forget_extraction(
    map: &mut FxHashMap<NonZeroU32, Location>,
    by_owner: &mut FxHashMap<Owner, usize>,
    id: NonZeroU32,
    expected: Option<Location>,
) {
    if expected.is_some_and(|at| map.get(&id) != Some(&at)) {
        return;
    }
    if let Some(at) = map.remove(&id) {
        let count = by_owner
            .get_mut(&at.owner)
            .expect("every extraction has an owner count");
        *count -= 1;
        if *count == 0 {
            by_owner.remove(&at.owner);
        }
    }
}

#[cfg(test)]
#[path = "table/completion_tests.rs"]
mod completion_tests;
#[cfg(test)]
#[path = "table/metadata_tests.rs"]
mod metadata_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::join_seq::residency::{DimensionKey, Kind};
    use crate::join_seq::{JoinList, Joined, RestoreOrder};
    const HOME: Owner = Owner {
        dimension: DimensionKey::OVERWORLD,
        kind: Kind::Mob,
    };

    struct Fixture {
        journal: Journal,
        table: EntityTable,
        source: FxHashMap<Location, SourceEntity<usize>>,
        dimension: DimensionKey,
    }
    impl Fixture {
        fn new() -> Self {
            let journal = Journal::default();
            Self {
                table: EntityTable::new(journal.clone()),
                journal,
                source: FxHashMap::default(),
                dimension: HOME.dimension,
            }
        }
        fn admit(&mut self, id: u32, owner: Owner) -> Location {
            let location = Location {
                owner,
                join: JoinSeq::next(),
                source: Default::default(),
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
        fn entity_lookup(&mut self, id: u32) -> EntityLookup<usize> {
            self.table.entity_lookup(&self.dimension, id, |location| {
                self.source.get(&location).copied()
            })
        }
    }

    #[test]
    fn collection_availability_counts_partial_loans_without_hiding_live_members() {
        let mut list = JoinList::default();
        list.push(Stored(7, JoinSeq::UNJOINED));
        list.push(Stored(8, JoinSeq::UNJOINED));
        let mut table = EntityTable::<()>::new(Journal::default());
        list.bind_residency(&table.producer(), HOME, |v| (v.0, v.1));
        let first_at = Location {
            owner: HOME,
            join: list[0].1,
        };
        let second_at = Location {
            owner: HOME,
            join: list[1].1,
        };
        assert!(table.owner_physically_available(HOME, None));
        let first = list.extract(first_at.join).unwrap();
        assert!(!table.owner_physically_available(HOME, None));
        assert!(table.owner_physically_available(HOME, Some((7, first_at))));
        assert!(!table.owner_physically_available(HOME, Some((8, second_at))));
        assert!(matches!(
            table.server_entity_lookup(8, |at| locate(&list, at)),
            EntityLookup::Found { .. }
        ));
        let second = list.extract(second_at.join).unwrap();
        assert!(!table.owner_physically_available(HOME, Some((7, first_at))));
        list.put_back(0, first);
        assert!(!table.owner_physically_available(HOME, None));
        assert!(table.owner_physically_available(HOME, Some((8, second_at))));
        list.put_back(1, second);
        assert!(table.owner_physically_available(HOME, None));
        let held = list.lift();
        assert!(!table.owner_physically_available(HOME, Some((7, first_at))));
        assert!(table.owner_physically_available(
            Owner {
                dimension: DimensionKey::NETHER,
                ..HOME
            },
            None
        ));
        list.restore(held, RestoreOrder::SurvivorsFirst);
        assert!(table.owner_physically_available(HOME, None));
        let retired = list.extract(first_at.join).unwrap();
        list.retire_extracted(retired.0, retired.1);
        assert!(table.owner_physically_available(HOME, None));
        let _held = list.extract(second_at.join).unwrap();
        assert!(!table.owner_physically_available(HOME, None));
        table.reset();
        assert!(table.owner_physically_available(HOME, None));
    }

    #[test]
    fn admitted_read_returns_taken_address_and_metadata_with_one_resolution() {
        use std::cell::Cell;
        let mut list = JoinList::default();
        list.push(Stored(7, JoinSeq::UNJOINED));
        let mut table = EntityTable::<u32>::new(Journal::default());
        list.bind_residency(&table.producer(), HOME, |v| (v.0, v.1));
        let address = Location {
            owner: HOME,
            join: list[0].1,
        };
        assert_eq!(table.publish_rows([(7, address, 19)]), 1);
        let calls = Cell::new(0);
        {
            let (at, state, metadata) = table
                .server_admitted_read_lookup(7, |at| {
                    calls.set(calls.get() + 1);
                    locate(&list, at)
                })
                .unwrap();
            assert_eq!(at, address);
            assert!(matches!(state, EntityLookup::Found { .. }));
            assert_eq!(metadata, Some(&19));
            assert_eq!(calls.get(), 1);
        }
        let held = list.lift();
        calls.set(0);
        let (at, state, metadata) = table
            .server_admitted_read_lookup(7, |at| {
                calls.set(calls.get() + 1);
                locate(&list, at)
            })
            .unwrap();
        assert_eq!(at, address);
        assert_eq!(state, EntityLookup::Taken);
        assert_eq!(metadata, Some(&19));
        assert_eq!(calls.get(), 1);
        list.restore(held, RestoreOrder::SurvivorsFirst);
        assert!(
            table
                .server_admitted_read_lookup(7, |_| Some(SourceEntity {
                    id: 8,
                    value: 0usize
                }))
                .is_none()
        );
        assert!(
            table
                .server_admitted_read_lookup::<usize>(7, |_| panic!(
                    "retired identity cannot resolve"
                ))
                .is_none()
        );
        assert!(
            table
                .server_admitted_read_lookup::<usize>(0, |_| panic!("zero cannot resolve"))
                .is_none()
        );
    }

    #[test]
    fn admitted_generation_and_prepared_merge_preserve_pending_work() {
        let mut list = JoinList::default();
        list.push(Stored(7, JoinSeq::UNJOINED));
        let mut table = EntityTable::<(u32, Vec<u32>)>::new(Journal::default());
        list.bind_residency(&table.producer(), HOME, |v| (v.0, v.1));
        let at = Location {
            owner: HOME,
            join: list[0].1,
        };
        assert_eq!(table.publish_rows([(7, at, (10, vec![11, 12]))]), 1);
        let owned = list.lift();
        let (admitted, state) = table
            .server_admitted_location(7, |at| locate(&list, at))
            .unwrap();
        assert_eq!(admitted, at);
        assert_eq!(state, EntityLookup::Taken);
        assert_eq!(
            table.publish_rows_merging([(7, at, (20, Vec::new()))], |old, mut prepared| {
                prepared.1 = old.unwrap().1;
                prepared
            }),
            1
        );
        list.restore(owned, RestoreOrder::SurvivorsFirst);
        assert_eq!(
            table
                .entity_read_lookup(&HOME.dimension, 7, |at| locate(&list, at))
                .1,
            Some(&(20, vec![11, 12]))
        );
        list.remove(0);
        list.push(Stored(7, JoinSeq::UNJOINED));
        assert_eq!(
            table.publish_rows_merging([(7, at, (30, Vec::new()))], |_, _| panic!(
                "stale generation reached merge"
            )),
            0
        );
        let (replacement, _) = table
            .server_admitted_location(7, |at| locate(&list, at))
            .unwrap();
        assert_ne!(replacement, at);
        assert!(
            table
                .server_admitted_location(8, |at| locate(&list, at))
                .is_none()
        );
    }

    #[test]
    fn cached_publications_survive_lifts_but_not_replaced_generations() {
        let mut list = JoinList::default();
        list.push(Stored(7, JoinSeq::UNJOINED));
        let mut table = EntityTable::<u32>::new(Journal::default());
        list.bind_residency(&table.producer(), HOME, |v| (v.0, v.1));
        let old = Location {
            owner: HOME,
            join: list[0].1,
        };
        assert_eq!(table.publish_rows([(7, old, 10)]), 1);
        let taken = list.lift();
        let (state, row) = table.entity_read_lookup(&HOME.dimension, 7, |at| locate(&list, at));
        assert_eq!(state, EntityLookup::Taken);
        assert_eq!(row, Some(&10));
        list.restore(taken, RestoreOrder::SurvivorsFirst);
        assert_eq!(
            table
                .entity_read_lookup(&HOME.dimension, 7, |at| locate(&list, at))
                .1,
            Some(&10)
        );
        list.remove(0);
        list.push(Stored(7, JoinSeq::UNJOINED));
        let replacement = Location {
            owner: HOME,
            join: list[0].1,
        };
        assert_ne!(old.join, replacement.join);
        assert_eq!(table.publish_rows([(7, old, 11)]), 0);
        assert_eq!(
            table
                .entity_read_lookup(&HOME.dimension, 7, |at| locate(&list, at))
                .1,
            None
        );
        assert_eq!(table.publish_rows([(7, replacement, 20)]), 1);
        assert!(!table.edit_cached_row(7, old, |_| panic!(
            "edited a replacement through its old generation"
        )));
        assert!(table.edit_cached_row(7, replacement, |row| *row += 1));
        table.producer().push(Event {
            owner: HOME,
            change: Change::Remove(Identity::new(7, old.join).unwrap()),
        });
        assert_eq!(
            table
                .entity_read_lookup(&HOME.dimension, 7, |at| locate(&list, at))
                .1,
            Some(&21)
        );
        table.end_tick().unwrap();
    }

    #[test]
    fn cached_payloads_retire_with_source_corruption_removal_and_epoch() {
        use std::rc::Rc;
        let mut list: JoinList<Stored> = JoinList::default();
        let mut table = EntityTable::<Rc<()>>::new(Journal::default());
        list.bind_residency(&table.producer(), HOME, |v: &Stored| (v.0, v.1));
        for action in 0..3 {
            list.push(Stored(7, JoinSeq::UNJOINED));
            let at = Location {
                owner: HOME,
                join: list[0].1,
            };
            let payload = Rc::new(());
            assert_eq!(table.publish_rows([(7, at, payload.clone())]), 1);
            assert_eq!(Rc::strong_count(&payload), 2);
            match action {
                0 => {
                    let (state, row) = table.entity_read_lookup(&HOME.dimension, 7, |_| {
                        Some(SourceEntity { id: 8, value: () })
                    });
                    assert_eq!(state, EntityLookup::Missing);
                    assert!(row.is_none());
                }
                1 => {
                    list.clear();
                    table.drain();
                }
                _ => table.reset(),
            }
            assert_eq!(Rc::strong_count(&payload), 1);
            list.clear();
            list.bind_residency(&table.producer(), HOME, |v| (v.0, v.1));
        }
    }

    #[test]
    fn cached_read_validates_once_and_never_exposes_wrong_dimension() {
        let mut list = JoinList::default();
        list.push(Stored(7, JoinSeq::UNJOINED));
        let mut table = EntityTable::<u32>::new(Journal::default());
        list.bind_residency(&table.producer(), HOME, |v| (v.0, v.1));
        let at = Location {
            owner: HOME,
            join: list[0].1,
        };
        assert_eq!(table.publish_rows([(7, at, 99)]), 1);
        let mut calls = 0;
        let (state, row) = table.entity_read_lookup(&HOME.dimension, 7, |at| {
            calls += 1;
            locate(&list, at)
        });
        assert!(matches!(state, EntityLookup::Found { value: 0, .. }));
        assert_eq!(row, Some(&99));
        assert_eq!(calls, 1);
        let (state, row) = table.entity_read_lookup::<()>(&DimensionKey::NETHER, 7, |_| {
            panic!("wrong dimension inspected storage")
        });
        assert_eq!(state, EntityLookup::Missing);
        assert!(row.is_none());
        assert_eq!(
            table
                .entity_read_lookup(&HOME.dimension, 7, |at| locate(&list, at))
                .1,
            Some(&99)
        );
    }

    #[test]
    fn server_lookup_follows_a_transfer_without_a_dimension_scan() {
        let mut f = Fixture::new();
        let old = f.admit(7, HOME);
        let away = Owner {
            dimension: DimensionKey::NETHER,
            kind: Kind::Boat,
        };
        f.emit(old, Change::Remove(Identity::new(7, old.join).unwrap()));
        f.source.remove(&old);
        let arrived = f.admit(7, away);
        assert_eq!(f.entity_lookup(7), EntityLookup::Missing);
        assert_eq!(
            f.table.server_entity_lookup(7, |at| {
                assert_eq!(at, arrived);
                f.source.get(&at).copied()
            }),
            EntityLookup::Found {
                location: arrived,
                value: 42
            }
        );
        f.emit(arrived, Change::Lifted);
        f.source.remove(&arrived);
        assert_eq!(
            f.table.server_entity_lookup::<()>(7, |_| None),
            EntityLookup::Taken
        );
        assert_eq!(
            f.table
                .server_entity_lookup::<()>(0, |_| panic!("zero reached storage")),
            EntityLookup::Missing
        );
    }

    struct Stored(u32, JoinSeq);
    impl Joined for Stored {
        fn join_seq(&self) -> JoinSeq {
            self.1
        }
        fn set_join_seq(&mut self, join: JoinSeq) {
            self.1 = join;
        }
    }
    fn locate(list: &JoinList<Stored>, at: Location) -> Option<SourceEntity<usize>> {
        list.locate(at.join).map(|i| SourceEntity {
            id: list[i].0,
            value: i,
        })
    }

    #[test]
    fn a_spawn_during_a_real_lift_is_found_while_the_survivor_is_taken() {
        let mut list = JoinList::default();
        list.push(Stored(7, JoinSeq::UNJOINED));
        let mut table = EntityTable::<()>::new(Journal::default());
        list.bind_residency(&table.producer(), HOME, |v| (v.0, v.1));
        let first = Location {
            owner: HOME,
            join: list[0].1,
            source: Default::default(),
        };
        let taken = list.lift();
        assert_eq!(
            table.entity_lookup(&HOME.dimension, 7, |at| locate(&list, at)),
            EntityLookup::Taken
        );
        list.push(Stored(8, JoinSeq::UNJOINED));
        let second = Location {
            owner: HOME,
            join: list[0].1,
            source: Default::default(),
        };
        assert_eq!(
            table.entity_lookup(&HOME.dimension, 8, |at| locate(&list, at)),
            EntityLookup::Found {
                location: second,
                value: 0
            }
        );
        list.restore(taken, RestoreOrder::SurvivorsFirst);
        table.end_tick().unwrap();
        assert_eq!(
            table.entity_lookup(&HOME.dimension, 7, |at| locate(&list, at)),
            EntityLookup::Found {
                location: first,
                value: 0
            }
        );
        assert_eq!(
            table.entity_lookup(&HOME.dimension, 8, |at| locate(&list, at)),
            EntityLookup::Found {
                location: second,
                value: 1
            }
        );
    }

    #[test]
    fn nested_real_lifts_remain_taken_until_the_outer_restore() {
        let mut list = JoinList::default();
        list.push(Stored(7, JoinSeq::UNJOINED));
        let mut table = EntityTable::<()>::new(Journal::default());
        list.bind_residency(&table.producer(), HOME, |v| (v.0, v.1));
        let outer = list.lift();
        let inner = list.lift();
        list.restore(inner, RestoreOrder::SurvivorsFirst);
        assert_eq!(
            table.entity_lookup(&HOME.dimension, 7, |at| locate(&list, at)),
            EntityLookup::Taken
        );
        list.restore(outer, RestoreOrder::SurvivorsFirst);
        assert!(matches!(
            table.entity_lookup(&HOME.dimension, 7, |at| locate(&list, at)),
            EntityLookup::Found { value: 0, .. }
        ));
        table.end_tick().unwrap();
    }

    #[test]
    fn validated_hits_keep_the_located_value_and_purge_both_kinds_of_stale_row() {
        let mut f = Fixture::new();
        let location = f.admit(7, HOME);
        assert_eq!(
            f.entity_lookup(7),
            EntityLookup::Found {
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
            assert_eq!(f.entity_lookup(7), EntityLookup::Missing);
            // A physical row alone must not resurrect a purged cache entry.
            f.source.insert(location, SourceEntity { id: 7, value: 42 });
            assert_eq!(f.entity_lookup(7), EntityLookup::Missing);
            f.emit(
                location,
                Change::Install(Identity::new(7, location.join).unwrap()),
            );
            assert_eq!(
                f.entity_lookup(7),
                EntityLookup::Found {
                    location,
                    value: 42
                }
            );
        }
        f.admit(1, HOME);
        assert_eq!(
            f.table
                .entity_lookup::<()>(&HOME.dimension, 0, |_| panic!("zero id inspected storage")),
            EntityLookup::Missing
        );
    }

    #[test]
    fn extraction_and_lift_only_hide_missing_sources_and_finish_in_order() {
        let mut f = Fixture::new();
        let location = f.admit(7, HOME);
        let identity = Identity::new(7, location.join).unwrap();
        let source = f.source.remove(&location).unwrap();
        f.emit(location, Change::Extract(identity));
        assert_eq!(f.entity_lookup(7), EntityLookup::Taken);
        f.source.insert(location, source);
        // Exposed replay storage is readable even before its marker closes.
        assert!(
            matches!(f.entity_lookup(7), EntityLookup::Found { location: at, .. } if at == location)
        );
        f.emit(location, Change::PutBack(identity));
        f.table.end_tick().unwrap();
        f.source.remove(&location);
        f.emit(location, Change::Lifted);
        let spawned = f.admit(8, HOME);
        assert_eq!(f.entity_lookup(7), EntityLookup::Taken);
        assert_eq!(
            f.entity_lookup(8),
            EntityLookup::Found {
                location: spawned,
                value: 42
            }
        );
        f.source.insert(location, source);
        f.emit(location, Change::Restored);
        f.table.end_tick().unwrap();
        assert!(
            matches!(f.entity_lookup(7), EntityLookup::Found { location: at, .. } if at == location)
        );
        // Retirement is authoritative, even if an unreported old view remains.
        f.emit(location, Change::Extract(identity));
        f.emit(location, Change::Remove(identity));
        assert_eq!(f.entity_lookup(7), EntityLookup::Missing);
        f.table.end_tick().unwrap();
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
            assert_eq!(f.entity_lookup(7), EntityLookup::Missing);
            f.source.get_mut(&location).unwrap().id = 7;
            assert_eq!(f.entity_lookup(7), EntityLookup::Missing);
            if lift {
                f.emit(location, Change::Restored);
            }
            f.table.end_tick().unwrap();
        }
    }

    #[test]
    fn parked_transfer_and_late_retirement_cannot_erase_a_new_owner() {
        let mut f = Fixture::new();
        let old = f.admit(7, HOME);
        assert!(matches!(f.entity_lookup(7), EntityLookup::Found { .. }));
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
        f.dimension = new.owner.dimension;
        f.emit(new, Change::Install(identity));
        assert_eq!(
            f.table.entity_lookup::<()>(&HOME.dimension, 7, |_| panic!(
                "wrong dimension inspected storage"
            )),
            EntityLookup::Missing
        );
        assert!(
            matches!(f.entity_lookup(7), EntityLookup::Found { location, .. } if location == new)
        );
        f.emit(old, Change::Remove(identity));
        assert!(
            matches!(f.entity_lookup(7), EntityLookup::Found { location, .. } if location == new)
        );
        f.source.remove(&new);
        f.emit(new, Change::Remove(identity));
        let replacement = f.admit(7, new.owner);
        f.emit(new, Change::Remove(identity));
        assert!(
            matches!(f.entity_lookup(7), EntityLookup::Found { location, .. } if location == replacement)
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
            assert_eq!(f.entity_lookup(7), EntityLookup::Taken);
            f.admit(8, HOME);
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.table.end_tick()));
            assert_eq!(result.is_err(), cfg!(debug_assertions));
            if !cfg!(debug_assertions) {
                assert_eq!(
                    result.unwrap(),
                    Err(UnbalancedPass {
                        extractions: usize::from(!lift),
                        lifts: usize::from(lift),
                    })
                );
            }
            let mut leftover = Vec::new();
            f.journal.drain_into(&mut leftover);
            assert!(leftover.is_empty(), "end_tick did not drain the journal");
            assert!(f.table.pending.is_empty());
            assert_eq!(f.entity_lookup(7), EntityLookup::Missing);
            f.table.end_tick().unwrap();
        }
    }

    #[test]
    fn reset_discards_pending_state_and_requires_fresh_producers() {
        let mut f = Fixture::new();
        let old = f.admit(7, HOME);
        assert!(matches!(f.entity_lookup(7), EntityLookup::Found { .. }));
        f.emit(old, Change::Extract(Identity::new(7, old.join).unwrap()));
        f.emit(old, Change::Lifted);
        f.table.drain();
        f.admit(9, HOME);
        f.table.reset();
        assert_eq!(f.entity_lookup(7), EntityLookup::Missing);
        assert_eq!(f.entity_lookup(9), EntityLookup::Missing);
        f.table.end_tick().unwrap();
        f.admit(8, HOME);
        assert_eq!(f.entity_lookup(8), EntityLookup::Missing);
        f.journal = f.table.producer();
        let new = f.admit(8, HOME);
        assert_eq!(
            f.entity_lookup(8),
            EntityLookup::Found {
                location: new,
                value: 42
            }
        );
    }
}

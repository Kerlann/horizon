//! PURPOSE: the join sequence: when each entity joined the level, kept in one
//! counter that rises with every join, whatever the kind.
//! WHY: ids do not say it (primed TNT, falling blocks, projectiles, tridents and
//! rockets take theirs late, in kind-list order), and vanilla ticks entities in
//! the order they were added, so a walk in that order needs a number that does
//! (#6509, tick order slice).
//! REFS: vanilla `EntityLookup` / `EntitySectionStorage` insertion order,
//! `PersistentEntitySectionManager.addEntity`; issue #6509; PR #7808.
//! LOGS: none. READS-BY: `game/entities/entity.rs` (the join-order walk, the save order).
//! ENDS-WHEN: kinds share one stored entity list, which is its own order.

use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    ops::{Deref, Index, IndexMut, RangeBounds},
    slice::SliceIndex,
};

#[path = "join_seq/residency.rs"]
pub mod residency;

static NEXT: AtomicU64 = AtomicU64::new(1);

/// When an entity joined: a number above every earlier join's. Zero means it
/// has not joined a list yet. One counter for the process, so entities of
/// different levels compare too; only the order of the numbers means anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JoinSeq(u64);

impl JoinSeq {
    /// Not joined yet.
    pub const UNJOINED: Self = Self(0);
    /// Assign the next number when an entity joins, with one relaxed add.
    pub fn next() -> Self {
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
    /// The latest sequence assigned anywhere in this process.
    pub fn latest() -> Self {
        Self(NEXT.load(Ordering::Relaxed) - 1)
    }

    /// Above every number a join can get.
    pub const MAX: Self = Self(u64::MAX);
    /// The number the next join will get or a later one: every entity that has
    /// joined so far is below it.
    pub fn upcoming() -> Self {
        Self(NEXT.load(Ordering::Relaxed))
    }
    pub fn is_joined(self) -> bool {
        self.0 != 0
    }
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A kind that stores when it joined.
pub trait Joined {
    fn join_seq(&self) -> JoinSeq;
    fn set_join_seq(&mut self, seq: JoinSeq);
}

/// The existing pass-specific order of survivors and nested spawns.
#[derive(Clone, Copy, Debug)]
pub enum RestoreOrder {
    /// Keep ticked survivors before entities created during the pass.
    SurvivorsFirst,
    /// Keep nested spawns before the ticked survivors.
    SpawnsFirst,
}

/// Implements [`Joined`] for a struct with a `join: JoinSeq` field.
#[macro_export]
macro_rules! impl_joined {
    ($($ty:ty),* $(,)?) => {$(
        impl $crate::join_seq::Joined for $ty {
            fn join_seq(&self) -> $crate::join_seq::JoinSeq {
                self.join
            }
            fn set_join_seq(&mut self, seq: $crate::join_seq::JoinSeq) {
                self.join = seq;
            }
        }
    )*};
}

/// One kind's entity list, with explicit structural operations and immutable slice access.
/// `DIRECT = false` closes mutable element access: state edits are scoped and
/// whole-value replacement must carry the attachment slot through `store_back`.
///
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let mut list = JoinList::<u32, false>::default();
/// list[0] = 1;
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let mut list = JoinList::<u32, false>::default();
/// let _ = list.get_mut(0);
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let mut list = JoinList::<u32, false>::default();
/// let _ = list.iter_mut();
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let mut list = JoinList::<u32, false>::default();
/// let _ = list.get_disjoint_mut([0, 1]);
/// ```
/// An entity put in without a join number gets the next one; a numbered entity
/// keeps its number when taken out and put back. Mutable element access is for
/// entity state; identity changes and whole-entity replacement remain audited
/// at their owning game boundaries.
///
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let mut list = JoinList::<u32>::default();
/// let _: &mut Vec<u32> = &mut list;
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let mut list = JoinList::<u32>::default();
/// let _: &mut [u32] = &mut list;
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let mut list = JoinList::<u32>::default();
/// list.iter_mut().as_mut_slice().reverse();
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let mut list = JoinList::<u32>::default();
/// (&mut list).into_iter().as_mut_slice().reverse();
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let _: JoinList<u32> = vec![1].into();
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::JoinList;
/// let list = JoinList::<u32>::default();
/// let _: Vec<u32> = list.into();
/// ```
#[derive(Debug)]
pub struct JoinList<T, const DIRECT: bool = true>(Vec<T>, Option<residency::Binding<T>>);
impl<T: Clone, const DIRECT: bool> Clone for JoinList<T, DIRECT> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), None)
    }
}
impl<T, const DIRECT: bool> Drop for JoinList<T, DIRECT> {
    fn drop(&mut self) {
        self.clear();
    }
}

/// A singleton resident owner with explicit membership operations.
/// State borrows remain audited: callers must not replace the whole entity or
/// change its identity through [`Self::as_mut`]. No mutable `Option` escapes.
///
/// ```compile_fail
/// use horizon_client_lib::join_seq::ResidentSlot;
/// let mut owner = ResidentSlot::<u32>::default();
/// owner = Some(1);
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::ResidentSlot;
/// let mut owner = ResidentSlot::<u32>::default();
/// let _: &mut Option<u32> = &mut owner;
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::ResidentSlot;
/// let mut owner = ResidentSlot::<u32>::default();
/// owner.take();
/// ```
pub struct ResidentSlot<T>(Option<T>);

impl<T> Default for ResidentSlot<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T: Joined> ResidentSlot<T> {
    /// Find the singleton owner by its stable join number.
    pub fn locate(&self, join: JoinSeq) -> Option<usize> {
        self.0
            .as_ref()
            .filter(|entity| entity.join_seq() == join)
            .map(|_| 0)
    }
    /// Install only into an empty owner, returning a refused entity unchanged.
    pub fn install(&mut self, entity: T) -> Result<(), T> {
        if self.0.is_some() {
            return Err(entity);
        }
        self.replace(entity);
        Ok(())
    }
    /// Explicitly retire any previous owner and install its replacement.
    pub fn replace(&mut self, mut entity: T) {
        JoinList::<T>::number(&mut entity);
        self.0 = Some(entity);
    }
    /// Move the owner out without dropping it.
    pub fn extract(&mut self) -> Option<T> {
        self.0.take()
    }
    /// Physically discard the owner, if present.
    pub fn discard(&mut self) {
        drop(self.extract());
    }
    /// Borrow entity state; identity and whole-value replacement stay audited.
    pub fn as_mut(&mut self) -> Option<&mut T> {
        self.0.as_mut()
    }
}

impl<T> Deref for ResidentSlot<T> {
    type Target = Option<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Off-walk entity storage using the same numbering and boundary as [`JoinList`].
/// No mutable vector, slice, element reference or iterator escapes. State edits
/// are scoped; identity and replacement remain audited by their callers.
///
/// ```compile_fail
/// use horizon_client_lib::join_seq::ResidentList;
/// let mut list = ResidentList::<u32>::default();
/// let _: &mut Vec<u32> = &mut list;
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::ResidentList;
/// let mut list = ResidentList::<u32>::default();
/// list[0] = 1;
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::ResidentList;
/// let mut list = ResidentList::<u32>::default();
/// let _: &mut [u32] = &mut list;
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::ResidentList;
/// let mut list = ResidentList::<u32>::default();
/// list.iter_mut().for_each(|entity| *entity = 1);
/// ```
/// ```compile_fail
/// use horizon_client_lib::join_seq::ResidentList;
/// let mut list = ResidentList::<u32>::default();
/// let escaped = list.update(0, |entity| entity);
/// ```
#[derive(Clone, Debug)]
pub struct ResidentList<T>(JoinList<T, false>);

impl<T> Default for ResidentList<T> {
    fn default() -> Self {
        Self(JoinList::default())
    }
}

impl<T: Joined> ResidentList<T> {
    /// Find an owner using the same lookup as the joined entity lists.
    pub fn locate(&self, join: JoinSeq) -> Option<usize> {
        self.0.locate(join)
    }
    /// Install an owner, assigning its stable join number only on first entry.
    pub fn push(&mut self, entity: T) {
        self.0.push(entity);
    }
    /// Append a batch after any owners already in this storage.
    pub fn append(&mut self, other: &mut Self) {
        self.0.append(&mut other.0);
    }
    pub fn remove(&mut self, index: usize) -> T {
        self.0.remove(index)
    }
    pub fn retain(&mut self, keep: impl FnMut(&T) -> bool) {
        self.0.retain(keep);
    }
    pub fn retain_mut(&mut self, keep: impl FnMut(&mut T) -> bool) {
        self.0.retain_where(keep);
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn drain(&mut self, range: impl RangeBounds<usize>) -> std::vec::Drain<'_, T> {
        self.0.drain(range)
    }
    pub fn update<R>(&mut self, index: usize, edit: impl FnOnce(&mut T) -> R) -> R {
        edit(&mut self.0.0[index])
    }
    pub fn for_each_mut(&mut self, edit: impl FnMut(&mut T)) {
        self.0.0.iter_mut().for_each(edit);
    }
    /// Take the owners for a pass, leaving a spawn buffer in their place.
    pub fn lift(&mut self) -> Self {
        Self(self.0.lift())
    }
    /// Restore survivors before entities spawned while the pass was lifted.
    pub fn restore(&mut self, taken: Self, order: RestoreOrder) {
        self.0.restore(taken.0, order);
    }
}

impl<T: Joined> FromIterator<T> for ResidentList<T> {
    fn from_iter<I: IntoIterator<Item = T>>(entities: I) -> Self {
        Self(entities.into_iter().collect())
    }
}

impl<T> Deref for ResidentList<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<'a, T> IntoIterator for &'a ResidentList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T, const DIRECT: bool> Default for JoinList<T, DIRECT> {
    fn default() -> Self {
        Self(Vec::new(), None)
    }
}

impl<T, const DIRECT: bool> JoinList<T, DIRECT> {
    /// Associate storage with its owning dimension; old membership leaves first.
    pub fn bind_residency(
        &mut self,
        journal: &residency::Journal,
        owner: residency::Owner,
        identity: fn(&T) -> (u32, JoinSeq),
    ) {
        self.bind_to(journal.bind(owner, identity));
    }
    fn bind_to(&mut self, binding: residency::Binding<T>) {
        if let Some(old) = &self.1 {
            old.all(&self.0, residency::Change::Remove);
        }
        binding.all(&self.0, residency::Change::Install);
        self.1 = Some(binding);
    }
    /// Report the existing allocator's delayed zero-to-live id assignment.
    pub fn id_assigned(&self, index: usize) {
        if let Some(binding) = &self.1 {
            binding.entity(&self.0[index], residency::Change::Install);
        }
    }
    /// Finish a consumed extraction after its terminal work has completed.
    pub fn retire_extracted(&self, id: u32, join: JoinSeq) {
        if let Some(binding) = &self.1 {
            binding.identity(id, join, residency::Change::Remove);
        }
    }
    /// Lift a whole pass, leaving an empty spawn buffer behind.
    pub fn lift(&mut self) -> Self {
        let taken = std::mem::take(self);
        self.1 = taken.1.clone();
        if let Some(binding) = &self.1 {
            binding.marker(residency::Change::Lifted);
        }
        taken
    }
    /// Restore a lifted pass using its established spawn-buffer ordering.
    pub fn restore(&mut self, mut taken: Self, order: RestoreOrder) {
        match (&self.1, &taken.1) {
            (Some(a), Some(b)) => assert!(a.same(b), "restoring a different entity owner"),
            (Some(binding), None) => taken.bind_to(binding.clone()),
            (None, Some(binding)) => self.bind_to(binding.clone()),
            (None, None) => {}
        }
        match order {
            RestoreOrder::SurvivorsFirst => {
                taken.append(self);
                *self = taken;
            }
            RestoreOrder::SpawnsFirst if self.is_empty() => *self = taken,
            RestoreOrder::SpawnsFirst => self.append(&mut taken),
        }
        if let Some(binding) = &self.1 {
            binding.marker(residency::Change::Restored);
        }
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity), None)
    }
    /// Move a batch back without changing its join numbers or order.
    pub fn append(&mut self, other: &mut Self) {
        let same = match (&self.1, &other.1) {
            (Some(a), Some(b)) => a.same(b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            if let Some(binding) = &other.1 {
                binding.all(&other.0, residency::Change::Remove);
            }
            if let Some(binding) = &self.1 {
                binding.all(&other.0, residency::Change::Install);
            }
        }
        self.0.append(&mut other.0);
    }
    pub fn remove(&mut self, index: usize) -> T {
        let entity = self.0.remove(index);
        if let Some(binding) = &self.1 {
            binding.entity(&entity, residency::Change::Remove);
        }
        entity
    }
    /// Extract selected owners in list order, compacting survivors in place.
    /// The predicate cannot replace an entity or change its identity.
    pub fn extract_if(&mut self, mut take: impl FnMut(&T) -> bool) -> impl Iterator<Item = T> {
        let binding = &self.1;
        self.0.extract_if(.., move |entity| {
            let removed = take(entity);
            if removed && let Some(binding) = binding {
                binding.entity(entity, residency::Change::Remove);
            }
            removed
        })
    }
    pub fn swap_remove(&mut self, index: usize) -> T {
        let entity = self.0.swap_remove(index);
        if let Some(binding) = &self.1 {
            binding.entity(&entity, residency::Change::Remove);
        }
        entity
    }
    pub fn pop(&mut self) -> Option<T> {
        let entity = self.0.pop()?;
        if let Some(binding) = &self.1 {
            binding.entity(&entity, residency::Change::Remove);
        }
        Some(entity)
    }
    pub fn clear(&mut self) {
        self.truncate(0);
    }
    pub fn truncate(&mut self, len: usize) {
        if let Some(binding) = &self.1 {
            binding.all(&self.0[len.min(self.0.len())..], residency::Change::Remove);
        }
        self.0.truncate(len);
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.retain_where(|entity| keep(entity));
    }
    fn retain_where(&mut self, mut keep: impl FnMut(&mut T) -> bool) {
        let binding = &self.1;
        self.0.retain_mut(|entity| {
            let before = binding.as_ref().map(|b| b.read(entity));
            let retained = keep(entity);
            if !retained && let (Some(binding), Some((id, join))) = (binding, before) {
                binding.identity(id, join, residency::Change::Remove);
            }
            retained
        });
    }
    pub fn drain(&mut self, range: impl RangeBounds<usize>) -> std::vec::Drain<'_, T> {
        let drained = self.0.drain(range);
        if let Some(binding) = &self.1 {
            binding.all(drained.as_slice(), residency::Change::Remove);
        }
        drained
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        self.0.swap(a, b);
    }
}

impl<T> JoinList<T> {
    pub fn retain_mut(&mut self, keep: impl FnMut(&mut T) -> bool) {
        self.retain_where(keep);
    }
    /// Run an audited state edit without returning a mutable entity reference.
    /// Whole-entity replacement belongs to the owning residency boundary.
    ///
    /// ```compile_fail
    /// use horizon_client_lib::join_seq::JoinList;
    /// let mut list = JoinList::<u32>::default();
    /// let escaped = list.update(0, |entity| entity);
    /// ```
    pub fn update<R>(&mut self, index: usize, edit: impl FnOnce(&mut T) -> R) -> R {
        edit(&mut self.0[index])
    }
    /// Edit each entity in list order without lending out mutable iteration.
    pub fn for_each_mut(&mut self, edit: impl FnMut(&mut T)) {
        self.0.iter_mut().for_each(edit);
    }
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut(self.0.iter_mut())
    }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.0.get_mut(index)
    }
    pub fn last_mut(&mut self) -> Option<&mut T> {
        self.0.last_mut()
    }
    /// Borrow distinct entities without lending out a mutable slice.
    pub fn get_disjoint_mut<const N: usize>(
        &mut self,
        indices: [usize; N],
    ) -> Result<[&mut T; N], std::slice::GetDisjointMutError> {
        self.0.get_disjoint_mut(indices)
    }
}

impl<T: horizon_mod_runtime::attachments::HasAttachmentLifetime, const DIRECT: bool>
    JoinList<T, DIRECT>
{
    /// Remove an owner leaving this level, retiring its source registration now.
    /// The retained token records whether arrival must rebind; it owns no live row.
    pub fn remove_for_transfer(&mut self, index: usize) -> T {
        let mut entity = self.remove(index);
        entity.attachment_lifetime().retire();
        entity
    }
}

impl<T: horizon_mod_runtime::attachments::HasAttachmentLifetime> JoinList<T, false> {
    /// Audited state edit; debug assertions detect a changed bound registration.
    pub fn update<R>(&mut self, index: usize, edit: impl FnOnce(&mut T) -> R) -> R {
        checked_edit(&mut self.0[index], edit)
    }
    pub fn for_each_mut(&mut self, mut edit: impl FnMut(&mut T)) {
        for entity in &mut self.0 {
            checked_edit(entity, &mut edit);
        }
    }
    /// Store a tick clone without dropping the owner's lifetime slot.
    pub fn store_back(&mut self, index: usize, entity: T) -> bool
    where
        T: Joined,
    {
        let join = entity.join_seq();
        let Some(index) = Some(index)
            .filter(|&i| self.0.get(i).is_some_and(|old| old.join_seq() == join))
            .or_else(|| self.locate(join))
        else {
            return false;
        };
        store_back(&mut self.0[index], entity);
        true
    }
}

fn checked_edit<T: horizon_mod_runtime::attachments::HasAttachmentLifetime, R>(
    entity: &mut T,
    edit: impl FnOnce(&mut T) -> R,
) -> R {
    #[cfg(debug_assertions)]
    let before = entity.attachment_lifetime().registration();
    let result = edit(entity);
    #[cfg(debug_assertions)]
    debug_assert!(
        before.is_none_or(|old| entity
            .attachment_lifetime()
            .registration()
            .is_some_and(|current| old.same_registration(&current))),
        "entity edit overwrote its registration"
    );
    result
}

/// Carry lifetime authority from a stored owner into its unbound tick clone.
pub fn store_back<T: horizon_mod_runtime::attachments::HasAttachmentLifetime>(
    old: &mut T,
    mut new: T,
) {
    debug_assert!(!new.attachment_lifetime().is_bound());
    std::mem::swap(old.attachment_lifetime(), new.attachment_lifetime());
    *old = new;
}

impl<T, const DIRECT: bool> Deref for JoinList<T, DIRECT> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0
    }
}
impl<T, const DIRECT: bool> AsRef<[T]> for JoinList<T, DIRECT> {
    fn as_ref(&self) -> &[T] {
        self
    }
}
impl<T, I: SliceIndex<[T]>, const DIRECT: bool> Index<I> for JoinList<T, DIRECT> {
    type Output = I::Output;
    fn index(&self, index: I) -> &Self::Output {
        &self.0[index]
    }
}
impl<T> IndexMut<usize> for JoinList<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut self.0[index]
    }
}

impl<T: Joined, const DIRECT: bool> JoinList<T, DIRECT> {
    /// Find a resident by its stable join number, or return `None` while absent.
    /// Usually lists are in join order: search them in logarithmic time. Lists
    /// restored out of order, and missing joins, fall back to a linear walk.
    pub fn locate(&self, join: JoinSeq) -> Option<usize> {
        match self.0.binary_search_by_key(&join, Joined::join_seq) {
            Ok(i) => Some(i),
            Err(_) => self.0.iter().position(|entity| entity.join_seq() == join),
        }
    }

    /// Temporarily lift one numbered owner without treating it as a retirement.
    pub fn extract(&mut self, join: JoinSeq) -> Option<T> {
        let index = self.locate(join)?;
        let entity = self.0.remove(index);
        if let Some(binding) = &self.1 {
            binding.entity(&entity, residency::Change::Extract);
        }
        Some(entity)
    }

    /// Return an extracted owner at its former position, preserving its number.
    /// If other mutations shortened the list, restore it at the end instead.
    pub fn put_back(&mut self, index: usize, entity: T) {
        let index = index.min(self.0.len());
        self.0.insert(index, entity);
        if let Some(binding) = &self.1 {
            binding.entity(&self.0[index], residency::Change::PutBack);
        }
    }

    fn number(entity: &mut T) {
        if !entity.join_seq().is_joined() {
            entity.set_join_seq(JoinSeq::next());
        }
    }
    pub fn push(&mut self, mut entity: T) {
        Self::number(&mut entity);
        self.0.push(entity);
        if let Some(binding) = &self.1 {
            binding.entity(self.0.last().unwrap(), residency::Change::Install);
        }
    }
    pub fn insert(&mut self, index: usize, mut entity: T) {
        Self::number(&mut entity);
        self.0.insert(index, entity);
        if let Some(binding) = &self.1 {
            binding.entity(&self.0[index], residency::Change::Install);
        }
    }
    pub fn extend(&mut self, entities: impl IntoIterator<Item = T>) {
        for entity in entities {
            self.push(entity);
        }
    }
}

impl<T: Joined, const DIRECT: bool> FromIterator<T> for JoinList<T, DIRECT> {
    fn from_iter<I: IntoIterator<Item = T>>(entities: I) -> Self {
        let entities = entities.into_iter();
        let mut list = Self::with_capacity(entities.size_hint().0);
        list.extend(entities);
        list
    }
}

impl<'a, T, const DIRECT: bool> IntoIterator for &'a JoinList<T, DIRECT> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
/// Mutable entity iteration without lending out the backing slice.
/// Unlike `slice::IterMut`, this cannot reorder the remaining list as a slice.
pub struct IterMut<'a, T>(std::slice::IterMut<'a, T>);

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.0.nth(n)
    }
}
impl<T> DoubleEndedIterator for IterMut<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back()
    }
    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        self.0.nth_back(n)
    }
}
impl<T> ExactSizeIterator for IterMut<'_, T> {}
impl<T> std::iter::FusedIterator for IterMut<'_, T> {}

impl<'a, T> IntoIterator for &'a mut JoinList<T> {
    type Item = &'a mut T;
    type IntoIter = IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
impl<T, const DIRECT: bool> IntoIterator for JoinList<T, DIRECT> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(mut self) -> Self::IntoIter {
        if let Some(binding) = &self.1 {
            binding.all(&self.0, residency::Change::Remove);
        }
        std::mem::take(&mut self.0).into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct Entity {
        join: JoinSeq,
        value: u32,
    }
    crate::impl_joined!(Entity);

    fn entity(value: u32) -> Entity {
        Entity {
            join: JoinSeq::UNJOINED,
            value,
        }
    }

    fn snapshot(list: &JoinList<Entity>) -> Vec<(u32, JoinSeq)> {
        list.iter().map(|e| (e.value, e.join)).collect()
    }

    #[test]
    fn locate_finds_ordered_reordered_and_temporarily_absent_owners() {
        let mut list = JoinList::<Entity>::default();
        assert_eq!(list.locate(JoinSeq::UNJOINED), None);
        list.extend((0..5).map(entity));
        let joins: Vec<_> = list.iter().map(Joined::join_seq).collect();
        for (i, &join) in joins.iter().enumerate() {
            assert_eq!(list.locate(join), Some(i));
        }
        list.swap(0, 4);
        list.swap(1, 3);
        for (i, owner) in list.iter().enumerate() {
            assert_eq!(list.locate(owner.join), Some(i));
        }
        let taken = list.remove(2);
        assert_eq!(list.locate(taken.join), None);
        let join = taken.join;
        list.insert(2, taken);
        assert_eq!(list.locate(join), Some(2));
        let mut taken = std::mem::take(&mut list);
        assert_eq!(list.locate(join), None);
        list.push(entity(99));
        list.append(&mut taken);
        assert_eq!(list.locate(join), Some(3));
    }

    #[test]
    fn locate_keeps_binary_hit_selection_for_duplicate_joins() {
        let mut list: JoinList<_> = (0..16).map(entity).collect();
        let duplicate = list[13].clone();
        list[14] = duplicate;
        // Preserve the old helper's binary hit, not the linear first match.
        assert_eq!(list.locate(list[13].join), Some(14));
    }

    #[test]
    fn store_back_uses_its_supplied_slot_without_searching() {
        use horizon_mod_runtime::attachments::{AttachmentLifetime, HasAttachmentLifetime};
        use std::cell::Cell;
        #[derive(Clone, Default)]
        struct Counted {
            join: JoinSeq,
            reads: Cell<usize>,
            lifetime: AttachmentLifetime,
        }
        impl Joined for Counted {
            fn join_seq(&self) -> JoinSeq {
                self.reads.set(self.reads.get() + 1);
                self.join
            }
            fn set_join_seq(&mut self, join: JoinSeq) {
                self.join = join;
            }
        }
        impl HasAttachmentLifetime for Counted {
            fn attachment_lifetime(&mut self) -> &mut AttachmentLifetime {
                &mut self.lifetime
            }
        }
        let mut list: JoinList<_, false> = (0..16).map(|_| Counted::default()).collect();
        for owner in &list {
            owner.reads.set(0);
        }
        let clone = list[7].clone();
        assert!(list.store_back(7, clone));
        // The clone's key is read once. The direct candidate is checked once;
        // replacement stores the clone's count, and no peers are consulted.
        assert_eq!(list[7].reads.get(), 1);
        assert!(
            list.iter()
                .enumerate()
                .all(|(i, owner)| i == 7 || owner.reads.get() == 0)
        );
    }

    #[derive(Clone)]
    struct Bound {
        join: JoinSeq,
        lifetime: horizon_mod_runtime::attachments::AttachmentLifetime,
    }
    crate::impl_joined!(Bound);
    impl horizon_mod_runtime::attachments::HasAttachmentLifetime for Bound {
        fn attachment_lifetime(
            &mut self,
        ) -> &mut horizon_mod_runtime::attachments::AttachmentLifetime {
            &mut self.lifetime
        }
    }

    #[test]
    fn store_back_preserves_join_and_registration_after_reordering() {
        use horizon_mod_runtime::attachments::{
            AttachmentLifetime, Attachments, index::AttachmentIndex,
        };
        use std::sync::{Arc, Mutex};
        let index = Arc::new(Mutex::new(AttachmentIndex::default()));
        let mut map = Attachments::default();
        let lifetime = AttachmentLifetime::new(map.bind_index(&index, 1u32));
        let registration = lifetime.registration().unwrap();
        let mut list = JoinList::<_, false>::default();
        list.push(Bound {
            join: JoinSeq::UNJOINED,
            lifetime,
        });
        list.push(Bound {
            join: JoinSeq::UNJOINED,
            lifetime: AttachmentLifetime::default(),
        });
        let join = list[0].join;
        let other = list[1].join;
        assert_ne!(join, other);
        let tick_clone = list[0].clone();
        list.swap(0, 1);
        assert!(list.store_back(0, tick_clone));
        assert_eq!(list.locate(join), Some(1));
        assert_eq!(list[0].join, other);
        assert!(registration.same_registration(&list[1].lifetime.registration().unwrap()));
        let mut gone = list.remove(1).clone();
        assert_eq!(list.locate(join), None);
        assert!(!list.store_back(0, gone.clone()));
        gone.join = JoinSeq::UNJOINED;
        list.push(gone);
        assert_ne!(list[1].join, join);
        assert_eq!(list.locate(join), None);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "entity edit overwrote its registration")]
    fn bound_update_rejects_whole_entity_overwrite() {
        use horizon_mod_runtime::attachments::{
            AttachmentLifetime, Attachments, index::AttachmentIndex,
        };
        use std::sync::{Arc, Mutex};
        let index = Arc::new(Mutex::new(AttachmentIndex::default()));
        let mut map = Attachments::default();
        let mut list = JoinList::<_, false>::default();
        list.push(Bound {
            join: JoinSeq::UNJOINED,
            lifetime: AttachmentLifetime::new(map.bind_index(&index, 1u32)),
        });
        list.update(0, |owner| *owner = owner.clone());
    }

    #[test]
    fn scoped_edits_preserve_order_and_keep_results_local_to_each_entity() {
        let mut list: JoinList<_> = (1..=3).map(entity).collect();
        let joins: Vec<_> = list.iter().map(Joined::join_seq).collect();
        assert_eq!(
            list.update(1, |e| {
                e.value += 10;
                e.value
            }),
            12
        );
        let mut visited = Vec::new();
        list.for_each_mut(|e| {
            if e.value == 12 {
                return;
            }
            visited.push(e.value);
            e.value *= 2;
        });
        assert_eq!(visited, [1, 3]);
        assert_eq!(
            snapshot(&list),
            [(2, joins[0]), (12, joins[1]), (6, joins[2])]
        );
    }

    #[test]
    fn collection_insert_and_extend_only_number_new_entities() {
        let old = JoinSeq::next();
        let another = JoinSeq::next();
        let mut list: JoinList<_> = [
            entity(1),
            Entity {
                join: old,
                value: 2,
            },
        ]
        .into_iter()
        .collect();
        let first = list[0].join;
        list.insert(1, entity(3));
        let inserted = list[1].join;
        list.extend([
            entity(4),
            Entity {
                join: another,
                value: 5,
            },
        ]);
        assert!(old < first && first < inserted && inserted < list[3].join);
        assert_eq!(
            snapshot(&list),
            [
                (1, first),
                (3, inserted),
                (2, old),
                (4, list[3].join),
                (5, another)
            ]
        );
    }

    #[test]
    fn removals_and_retention_preserve_survivor_numbers_and_order() {
        let mut list: JoinList<_> = (1..=7).map(entity).collect();
        let before = snapshot(&list);
        assert_eq!(list.remove(1).value, 2);
        assert_eq!(list.swap_remove(1).value, 3);
        list.retain(|e| e.value != 4);
        list.retain_mut(|e| {
            e.value *= 10;
            e.value != 50
        });
        assert_eq!(
            snapshot(&list),
            [(10, before[0].1), (70, before[6].1), (60, before[5].1)]
        );
        let [first, last] = list.get_disjoint_mut([0, 2]).unwrap();
        first.value += 1;
        last.value += 1;
        assert_eq!(
            snapshot(&list),
            [(11, before[0].1), (70, before[6].1), (61, before[5].1)]
        );
    }

    #[test]
    fn partial_drain_and_swap_keep_the_remaining_joins() {
        let mut list: JoinList<_> = (1..=6).map(entity).collect();
        let before = snapshot(&list);
        {
            let mut drain = list.drain(1..4);
            assert_eq!(drain.next().unwrap().join, before[1].1);
            // Dropping a partially consumed drain still removes its whole range.
        }
        list.swap(0, 2);
        assert_eq!(snapshot(&list), [before[5], before[4], before[0]]);
        assert_eq!(list.pop().unwrap().join, before[0].1);
        list.truncate(1);
        assert_eq!(snapshot(&list), [before[5]]);
        list.clear();
        assert!(list.is_empty());
    }

    #[test]
    fn append_preserves_batch_numbers_and_receiver_order() {
        let mut list = JoinList::<Entity>::default();
        for value in 1..=2 {
            list.push(Entity {
                join: JoinSeq::UNJOINED,
                value,
            });
        }
        let joins: Vec<_> = list.iter().map(Joined::join_seq).collect();
        let mut taken = std::mem::take(&mut list);
        list.push(Entity {
            join: JoinSeq::UNJOINED,
            value: 3,
        });
        let fresh = list[0].join;
        list.append(&mut taken);
        assert!(taken.is_empty());
        assert_eq!(list.iter().map(|e| e.value).collect::<Vec<_>>(), [3, 1, 2]);
        assert_eq!(
            list.iter().map(Joined::join_seq).collect::<Vec<_>>(),
            [fresh, joins[0], joins[1]]
        );
        assert!(joins.into_iter().all(|join| join < fresh));
    }

    fn journal_list() -> (JoinList<Entity>, residency::Journal) {
        let journal = residency::Journal::default();
        let mut list: JoinList<_> = (1..=3).map(entity).collect();
        list.bind_residency(&journal, journal_owner(), |e| (e.value, e.join));
        (list, journal)
    }
    fn journal_owner() -> residency::Owner {
        residency::Owner {
            dimension: mod_api::DimensionKey::OVERWORLD,
            kind: residency::Kind::Mob,
        }
    }
    fn key(entity: &Entity) -> residency::Identity {
        residency::Identity::new(entity.value, entity.join).unwrap()
    }
    fn changes(journal: &residency::Journal) -> Vec<residency::Change> {
        let mut events = Vec::new();
        journal.drain_into(&mut events);
        events
            .into_iter()
            .map(|e| {
                assert_eq!(e.owner, journal_owner());
                e.change
            })
            .collect()
    }

    #[test]
    fn journal_records_every_permanent_list_operation_and_only_removed_owners() {
        use residency::Change::{Install, Remove};
        let (mut list, journal) = journal_list();
        let keys: Vec<_> = list.iter().map(key).collect();
        assert_eq!(
            changes(&journal),
            keys.iter().copied().map(Install).collect::<Vec<_>>()
        );
        list.insert(1, entity(4));
        let inserted = key(&list[1]);
        assert_eq!(list.remove(1).value, 4);
        assert_eq!(list.swap_remove(1).value, 2);
        assert_eq!(list.pop().unwrap().value, 3);
        list.truncate(100);
        list.clear();
        assert!(list.pop().is_none());
        assert_eq!(
            changes(&journal),
            [
                Install(inserted),
                Remove(inserted),
                Remove(keys[1]),
                Remove(keys[2]),
                Remove(keys[0])
            ]
        );
        list.extend((5..=8).map(entity));
        let keys: Vec<_> = list.iter().map(key).collect();
        changes(&journal);
        list.retain_mut(|e| e.value != 6);
        assert_eq!(list.extract_if(|e| e.value == 7).next().unwrap().value, 7);
        drop(list.drain(..1));
        list.retain(|_| false);
        assert!(list.is_empty());
        assert_eq!(
            changes(&journal),
            [
                Remove(keys[1]),
                Remove(keys[2]),
                Remove(keys[0]),
                Remove(keys[3])
            ]
        );
        list.push(entity(9));
        let last = key(&list[0]);
        assert_eq!(list.into_iter().next().unwrap().value, 9);
        assert_eq!(changes(&journal), [Install(last), Remove(last)]);
        let (list, journal) = journal_list();
        let keys: Vec<_> = list.iter().map(key).collect();
        changes(&journal);
        drop(list);
        assert_eq!(
            changes(&journal),
            keys.into_iter().map(Remove).collect::<Vec<_>>()
        );
        for id in [0, 7] {
            let mut list = JoinList::default();
            list.push(entity(id));
            list.bind_residency(&journal, journal_owner(), |e| (e.value, e.join));
            let expected: Vec<_> = residency::Identity::new(id, list[0].join)
                .map(Remove)
                .into_iter()
                .collect();
            changes(&journal);
            list.retain_mut(|e| {
                e.value = 70;
                e.join = JoinSeq::next();
                false
            });
            assert_eq!(changes(&journal), expected);
        }
    }

    #[test]
    fn journal_extraction_has_distinct_put_back_and_retirement_terminals() {
        use residency::Change::{Extract, PutBack, Remove};
        let (mut list, journal) = journal_list();
        let keys: Vec<_> = list.iter().map(key).collect();
        changes(&journal);
        let first = list.extract(keys[0].join).unwrap();
        assert_eq!(list.locate(keys[0].join), None);
        list.put_back(0, first);
        assert_eq!(
            snapshot(&list).iter().map(|e| e.0).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        let _ = list.extract(keys[1].join).unwrap();
        list.retire_extracted(2, keys[1].join);
        assert_eq!(
            changes(&journal),
            [
                Extract(keys[0]),
                PutBack(keys[0]),
                Extract(keys[1]),
                Remove(keys[1])
            ]
        );
        assert_eq!(list.locate(keys[1].join), None);
    }

    #[test]
    fn journal_lift_and_both_restore_orders_never_reinstall_survivors() {
        use residency::Change::{Install, Lifted, Remove, Restored};
        for (order, expected) in [
            (RestoreOrder::SurvivorsFirst, [1, 3, 4]),
            (RestoreOrder::SpawnsFirst, [4, 1, 3]),
        ] {
            let (mut list, journal) = journal_list();
            let removed = key(&list[1]);
            changes(&journal);
            let mut taken = list.lift();
            list.push(entity(4));
            let spawned = key(&list[0]);
            let _ = taken.remove(1);
            list.restore(taken, order);
            assert_eq!(
                changes(&journal),
                [Lifted, Install(spawned), Remove(removed), Restored]
            );
            assert_eq!(list.iter().map(|e| e.value).collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn journal_restore_admits_an_unbound_batch_without_reinstalling_the_spawn_buffer() {
        use residency::Change::{Install, Restored};
        for (order, expected) in [
            (RestoreOrder::SurvivorsFirst, [4, 5, 1, 2, 3]),
            (RestoreOrder::SpawnsFirst, [1, 2, 3, 4, 5]),
        ] {
            let (mut list, journal) = journal_list();
            changes(&journal);
            let staged: JoinList<_> = (4..=5).map(entity).collect();
            let admitted: Vec<_> = staged.iter().map(key).collect();
            list.restore(staged, order);
            assert_eq!(
                changes(&journal),
                [Install(admitted[0]), Install(admitted[1]), Restored]
            );
            assert_eq!(list.iter().map(|e| e.value).collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn journal_snapshots_are_detached_and_delayed_ids_install_only_after_assignment() {
        use residency::Change::{Install, Remove};
        let (mut list, journal) = journal_list();
        changes(&journal);
        let mut snapshot = list.clone();
        snapshot.clear();
        snapshot.push(entity(99));
        drop(snapshot.into_iter());
        assert!(changes(&journal).is_empty());
        list.clear();
        changes(&journal);
        list.push(entity(0));
        let join = list[0].join;
        assert!(changes(&journal).is_empty());
        list.update(0, |e| e.value = 7);
        list.id_assigned(0);
        let assigned = residency::Identity::new(7, join).unwrap();
        list.clear();
        assert_eq!(changes(&journal), [Install(assigned), Remove(assigned)]);
    }

    #[test]
    fn journal_epoch_rebind_reinstalls_live_members_and_updates_the_producer() {
        use residency::Change::Install;
        let (mut list, mut journal) = journal_list();
        let stale = journal.clone();
        journal.reset_epoch();
        list.push(entity(4));
        assert!(changes(&journal).is_empty());
        assert!(changes(&stale).is_empty());
        let keys: Vec<_> = list.iter().map(key).collect();
        list.bind_residency(&journal, journal_owner(), |e| (e.value, e.join));
        assert_eq!(
            changes(&journal),
            keys.into_iter().map(Install).collect::<Vec<_>>()
        );
        assert!(changes(&stale).is_empty());
        list.push(entity(5));
        assert_eq!(changes(&journal), [Install(key(&list[4]))]);
    }

    #[test]
    fn journal_same_owner_transfer_between_games_updates_both_sinks() {
        use residency::Change::{Install, Remove};
        let (mut source, old_journal) = journal_list();
        let keys: Vec<_> = source.iter().map(key).collect();
        changes(&old_journal);
        let new_journal = residency::Journal::default();
        let mut target = JoinList::default();
        target.bind_residency(&new_journal, journal_owner(), |e: &Entity| {
            (e.value, e.join)
        });
        target.append(&mut source);
        assert!(source.is_empty());
        assert_eq!(target.iter().map(key).collect::<Vec<_>>(), keys);
        assert_eq!(
            changes(&old_journal),
            keys.iter().copied().map(Remove).collect::<Vec<_>>()
        );
        assert_eq!(
            changes(&new_journal),
            keys.into_iter().map(Install).collect::<Vec<_>>()
        );
    }

    #[test]
    fn journal_batch_transfer_changes_dimension_without_changing_identity() {
        use residency::{
            Change::{Install, Remove},
            Event, Kind, Owner,
        };
        let (mut source, journal) = journal_list();
        let keys: Vec<_> = source.iter().map(key).collect();
        changes(&journal);
        let destination = Owner {
            dimension: mod_api::DimensionKey::NETHER,
            kind: Kind::Other,
        };
        let mut target = JoinList::default();
        target.bind_residency(&journal, destination, |e: &Entity| (e.value, e.join));
        target.append(&mut source);
        assert!(source.is_empty());
        assert_eq!(target.iter().map(key).collect::<Vec<_>>(), keys);
        let expected: Vec<_> = keys
            .iter()
            .map(|&k| Event {
                owner: journal_owner(),
                change: Remove(k),
            })
            .chain(keys.iter().map(|&k| Event {
                owner: destination,
                change: Install(k),
            }))
            .collect();
        let mut events = Vec::new();
        journal.drain_into(&mut events);
        assert_eq!(events, expected);
        // Opening/resetting a level may rebind retained storage to its dimension.
        target.bind_residency(&journal, journal_owner(), |e| (e.value, e.join));
        let expected: Vec<_> = keys
            .iter()
            .map(|&k| Event {
                owner: destination,
                change: Remove(k),
            })
            .chain(keys.iter().map(|&k| Event {
                owner: journal_owner(),
                change: Install(k),
            }))
            .collect();
        events.clear();
        journal.drain_into(&mut events);
        assert_eq!(events, expected);
        assert_eq!(target.iter().map(key).collect::<Vec<_>>(), keys);
    }
}

#[cfg(test)]
mod resident_tests {
    use super::{JoinSeq, Joined, ResidentList, ResidentSlot};

    #[derive(Clone, Debug)]
    struct Numbered {
        value: i32,
        join: JoinSeq,
    }
    impl Numbered {
        fn new(value: i32) -> Self {
            Self {
                value,
                join: JoinSeq::UNJOINED,
            }
        }
    }
    crate::impl_joined!(Numbered);

    use std::{cell::Cell, rc::Rc};

    #[test]
    fn joined_lift_leaves_a_spawn_buffer_and_keeps_the_original_numbers() {
        fn check<const DIRECT: bool>() {
            let mut owners: super::JoinList<_, DIRECT> = (1..=2).map(Numbered::new).collect();
            let joins: Vec<_> = owners.iter().map(Joined::join_seq).collect();
            let taken = owners.lift();
            assert!(owners.is_empty());
            assert_eq!(taken.iter().map(|e| e.value).collect::<Vec<_>>(), [1, 2]);
            owners.push(Numbered::new(3));
            let spawned = owners[0].join;
            assert!(spawned > joins[1]);
            owners.restore(taken, super::RestoreOrder::SurvivorsFirst);
            assert_eq!(
                owners.iter().map(|e| e.value).collect::<Vec<_>>(),
                [1, 2, 3]
            );
            for (index, join) in joins.into_iter().chain([spawned]).enumerate() {
                assert_eq!(owners.locate(join), Some(index));
            }
        }
        check::<true>();
        check::<false>();
    }

    #[test]
    fn restore_respects_both_pass_orders_with_and_without_spawns() {
        for (order, expected) in [
            (super::RestoreOrder::SurvivorsFirst, [1, 2, 3]),
            (super::RestoreOrder::SpawnsFirst, [3, 1, 2]),
        ] {
            let mut owners: super::JoinList<_> = (1..=2).map(Numbered::new).collect();
            let joins: Vec<_> = owners.iter().map(Joined::join_seq).collect();
            let taken = owners.lift();
            owners.push(Numbered::new(3));
            owners.restore(taken, order);
            assert_eq!(owners.iter().map(|e| e.value).collect::<Vec<_>>(), expected);
            for join in joins {
                assert!(owners.locate(join).is_some());
            }
            let taken = owners.lift();
            owners.restore(taken, order);
            assert_eq!(owners.iter().map(|e| e.value).collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn extraction_keeps_identity_and_restores_the_original_position() {
        let mut list: super::JoinList<_> = (1..=3).map(Numbered::new).collect();
        let joins: Vec<_> = list.iter().map(Joined::join_seq).collect();
        assert!(list.extract(JoinSeq::UNJOINED).is_none());
        let mut owner = list.extract(joins[1]).unwrap();
        assert_eq!(owner.value, 2);
        assert_eq!(list.locate(joins[1]), None);
        assert_eq!(list.iter().map(|e| e.value).collect::<Vec<_>>(), [1, 3]);
        owner.value = 20;
        list.put_back(1, owner);
        assert_eq!(list.iter().map(|e| e.value).collect::<Vec<_>>(), [1, 20, 3]);
        for (index, join) in joins.into_iter().enumerate() {
            assert_eq!(list.locate(join), Some(index));
        }
    }

    #[test]
    fn put_back_clamps_a_position_invalidated_by_other_removals() {
        let mut list: super::JoinList<_> = (1..=3).map(Numbered::new).collect();
        let join = list[2].join;
        let owner = list.extract(join).unwrap();
        list.remove(0);
        list.put_back(2, owner);
        assert_eq!(list.iter().map(|e| e.value).collect::<Vec<_>>(), [2, 3]);
        assert_eq!(list.locate(join), Some(1));
        let owner = list.extract(join).unwrap();
        list.clear();
        list.put_back(usize::MAX, owner);
        assert_eq!(list.locate(join), Some(0));
    }

    #[test]
    fn resident_lookup_tracks_numbered_owners_across_membership_changes() {
        let mut list: ResidentList<_> = (1..=3).map(Numbered::new).collect();
        let joins: Vec<_> = list.iter().map(Joined::join_seq).collect();
        assert!(joins.iter().all(|join| join.is_joined()));
        assert!(joins.windows(2).all(|pair| pair[0] < pair[1]));
        for (index, &join) in joins.iter().enumerate() {
            assert_eq!(list.locate(join), Some(index));
        }
        let removed = list.remove(0);
        assert_eq!(list.locate(joins[0]), None);
        list.retain(|entity| entity.value != 2);
        assert_eq!(list.locate(joins[1]), None);
        assert_eq!(list.locate(joins[2]), Some(0));
        let taken = list.lift();
        list.push(Numbered::new(4));
        let spawned = list[0].join;
        assert!(spawned > joins[2]);
        assert_eq!(list.locate(joins[2]), None);
        list.restore(taken, super::RestoreOrder::SurvivorsFirst);
        assert_eq!(list.locate(joins[2]), Some(0));
        assert_eq!(list.locate(spawned), Some(1));
        list.push(removed);
        assert_eq!(list.locate(joins[0]), Some(2));
        list.clear();
        for join in joins.into_iter().chain([spawned]) {
            assert_eq!(list.locate(join), None);
        }
    }

    #[test]
    fn singleton_lookup_survives_extract_and_rejects_replaced_owners() {
        let mut slot = ResidentSlot::default();
        slot.install(Numbered::new(1)).unwrap();
        let join = slot.as_ref().unwrap().join;
        assert!(join.is_joined());
        assert_eq!(slot.locate(join), Some(0));
        let owner = slot.extract().unwrap();
        assert_eq!(slot.locate(join), None);
        slot.install(owner).unwrap();
        assert_eq!(slot.as_ref().unwrap().join, join);
        assert_eq!(slot.locate(join), Some(0));
        slot.replace(Numbered::new(2));
        let replacement = slot.as_ref().unwrap().join;
        assert!(replacement > join);
        assert_eq!(slot.locate(join), None);
        assert_eq!(slot.locate(replacement), Some(0));
        slot.discard();
        assert_eq!(slot.locate(replacement), None);
    }

    #[test]
    fn resident_restore_keeps_spawn_buffer_and_snapshot_order() {
        let mut list = ResidentList::default();
        for value in [1, 2, 3] {
            list.push(Numbered::new(value));
        }
        let snapshot = list.to_vec();
        let mut taken = list.lift();
        list.push(Numbered::new(4));
        taken.retain_mut(|value| {
            value.value *= 10;
            value.value != 20
        });
        list.restore(taken, super::RestoreOrder::SurvivorsFirst);
        assert_eq!(
            list.iter().map(|e| e.value).collect::<Vec<_>>(),
            [10, 30, 4]
        );
        assert_eq!(
            snapshot.iter().map(|e| e.value).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(list.remove(1).value, 30);
        assert_eq!(list.drain(..1).map(|e| e.value).collect::<Vec<_>>(), [10]);
        assert_eq!(list[0].value, 4);
    }

    #[test]
    fn resident_append_preserves_spawn_buffer_before_survivors() {
        let mut list: ResidentList<_> = [1, 2].into_iter().map(Numbered::new).collect();
        let mut taken = list.lift();
        list.push(Numbered::new(3));
        taken.retain(|e| e.value != 1);
        list.append(&mut taken);
        assert_eq!(list.iter().map(|e| e.value).collect::<Vec<_>>(), [3, 2]);
        assert!(taken.is_empty());
    }

    #[test]
    fn resident_owners_drop_only_on_discard() {
        struct Owner(Rc<Cell<usize>>, JoinSeq);
        impl Joined for Owner {
            fn join_seq(&self) -> JoinSeq {
                self.1
            }
            fn set_join_seq(&mut self, join: JoinSeq) {
                self.1 = join;
            }
        }
        impl Drop for Owner {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let dropped = Rc::new(Cell::new(0));
        let mut list = ResidentList::default();
        for _ in 0..3 {
            list.push(Owner(dropped.clone(), JoinSeq::UNJOINED));
        }
        let taken = list.lift();
        list.push(Owner(dropped.clone(), JoinSeq::UNJOINED));
        assert_eq!(dropped.get(), 0);
        list.restore(taken, super::RestoreOrder::SurvivorsFirst);
        assert_eq!(dropped.get(), 0);
        list.clear();
        assert_eq!(dropped.get(), 4);
        list.push(Owner(dropped.clone(), JoinSeq::UNJOINED));
        drop(list);
        assert_eq!(dropped.get(), 5);
    }
}

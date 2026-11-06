//! Dense attachment membership with immutable, shared-entry snapshots.
use std::{collections::HashMap, hash::Hash, sync::Arc};

use super::AttachmentEntry;
use crate::nbt::JavaString;

/// An already-resolved attachment namespace and name.
pub type AttachmentKey = (JavaString, JavaString);

/// A loaded holder and the immutable value it had when a snapshot was acquired.
#[derive(Clone)]
#[non_exhaustive]
pub struct AttachmentRow<H> {
    /// The ready-to-use holder identity; iteration needs no lookup or conversion.
    pub holder: H,
    /// Shared with the holder's attachment map, never copied by snapshot acquisition.
    pub entry: Arc<AttachmentEntry>,
}

/// Authority to publish one registration's changes. Cloning or dropping this token has
/// no membership effect. Re-registering the holder invalidates all earlier tokens.
#[derive(Clone)]
#[non_exhaustive]
pub struct AttachmentOwner<H> {
    holder: H,
    generation: Arc<()>,
}

struct Owner {
    generation: Arc<()>,
    slots: HashMap<Arc<AttachmentKey>, usize>,
}

fn reclaim_sparse_map<K: Eq + Hash, V>(map: &mut HashMap<K, V>) {
    if map.is_empty() {
        *map = HashMap::new();
    } else if map.len() < map.capacity() / 4 {
        map.shrink_to_fit();
    }
}

/// A dimension's loaded attachment memberships, independent of holder storage.
///
/// Snapshots clone one Arc, and borrowed row iteration needs no locks or allocations.
/// Mutation copies a bucket's row vector only when a snapshot retains that version;
/// entries themselves remain shared. With no retained version, updates are in place.
/// All key/holder lookups are expected O(1). An owner's key-to-slot map is the only
/// reverse index; unregistration visits only that holder's keys, not all types.
/// Key strings are shared across every membership in a bucket. Sparse tables and
/// row vectors shrink below quarter capacity, amortizing reclamation over removals.
/// Registration and unload are explicit lifecycle operations, never Drop effects.
///
/// Rows append on first insertion. Replacement preserves the slot; removal fills it
/// from the final row. Order is deterministic given the ordered registration/mutation
/// history, but is neither sorted nor stable across removals. Callers restoring many
/// holders must supply a defined registration order rather than HashMap iteration.
#[non_exhaustive]
pub struct AttachmentIndex<H> {
    buckets: HashMap<Arc<AttachmentKey>, Arc<Vec<AttachmentRow<H>>>>,
    owners: HashMap<H, Owner>,
    empty: Arc<Vec<AttachmentRow<H>>>,
}

impl<H> Default for AttachmentIndex<H> {
    fn default() -> Self {
        Self {
            buckets: HashMap::new(),
            owners: HashMap::new(),
            empty: Arc::new(Vec::new()),
        }
    }
}

impl<H: Copy + Eq + Hash> AttachmentIndex<H> {
    /// Install the authoritative map, replacing the holder's previous membership.
    /// Only lifecycle code may register; a bound map publishes through its token.
    /// Rebinding invalidates stale save/COW bindings, even when their keys match.
    /// Shared keys keep their dense slots; unchanged entries do not copy row vectors.
    pub fn register(
        &mut self,
        holder: H,
        entries: impl IntoIterator<Item = (AttachmentKey, Arc<AttachmentEntry>)>,
    ) -> AttachmentOwner<H> {
        let mut omitted = self
            .owners
            .remove(&holder)
            .map_or_else(HashMap::new, |owner| owner.slots);
        // A live stale token retains its identity, so generations cannot wrap or be
        // reused, including across distinct indexes.
        let generation = Arc::new(());
        let token = AttachmentOwner { holder, generation };
        self.owners.insert(
            holder,
            Owner {
                generation: token.generation.clone(),
                slots: omitted.clone(),
            },
        );
        for (key, entry) in entries {
            omitted.remove(&key);
            self.set(&token, &key, entry);
        }
        for (key, _) in omitted {
            self.remove(&token, &key);
        }
        token
    }

    /// Capture current membership and values. Missing keys share the same empty Arc;
    /// acquiring an empty snapshot neither allocates nor registers the key.
    pub fn snapshot(&self, key: &AttachmentKey) -> Arc<Vec<AttachmentRow<H>>> {
        self.buckets.get(key).unwrap_or(&self.empty).clone()
    }

    /// Publish insertion or replacement. Return false for a stale or foreign token,
    /// leaving the index unchanged. Replacement does not reorder the holder's row.
    pub fn set(
        &mut self,
        token: &AttachmentOwner<H>,
        key: &AttachmentKey,
        entry: Arc<AttachmentEntry>,
    ) -> bool {
        let Some(owner) = self.owners.get_mut(&token.holder) else {
            return false;
        };
        if !Arc::ptr_eq(&owner.generation, &token.generation) {
            return false;
        }
        let row = AttachmentRow {
            holder: token.holder,
            entry,
        };
        if let Some(&slot) = owner.slots.get(key) {
            let rows = self.buckets.get_mut(key).unwrap();
            if !Arc::ptr_eq(&rows[slot].entry, &row.entry) {
                Arc::make_mut(rows)[slot] = row;
            }
        } else {
            let shared_key = self
                .buckets
                .get_key_value(key)
                .map_or_else(|| Arc::new(key.clone()), |(key, _)| key.clone());
            let rows = self.buckets.entry(shared_key.clone()).or_default();
            owner.slots.insert(shared_key, rows.len());
            Arc::make_mut(rows).push(row);
        }
        true
    }

    /// Remove one membership. Return false only for a stale or foreign token.
    pub fn remove(&mut self, token: &AttachmentOwner<H>, key: &AttachmentKey) -> bool {
        let Some(owner) = self.owners.get_mut(&token.holder) else {
            return false;
        };
        if !Arc::ptr_eq(&owner.generation, &token.generation) {
            return false;
        }
        if let Some(slot) = owner.slots.remove(key) {
            reclaim_sparse_map(&mut owner.slots);
            self.remove_membership(key, slot);
        }
        true
    }

    /// Explicitly unload this registration. Stale or foreign tokens cannot unload
    /// a replacement owner. Retained snapshots keep their original rows and values.
    pub fn unregister(&mut self, token: &AttachmentOwner<H>) -> bool {
        let Some(owner) = self.owners.get(&token.holder) else {
            return false;
        };
        if !Arc::ptr_eq(&owner.generation, &token.generation) {
            return false;
        }
        self.unload(token.holder)
    }

    /// Remove the current registration at an authoritative storage boundary, even
    /// when the live payload has detached from its original map binding through COW.
    /// Unlike token-checked unregistration, this is only for trusted holder unload,
    /// never for a map observer or Drop. Return false if the holder was not registered.
    pub fn unload(&mut self, holder: H) -> bool {
        let Some(owner) = self.owners.remove(&holder) else {
            return false;
        };
        for (key, slot) in owner.slots {
            self.remove_membership(&key, slot);
        }
        reclaim_sparse_map(&mut self.owners);
        true
    }

    fn remove_membership(&mut self, key: &AttachmentKey, slot: usize) {
        let rows = Arc::make_mut(self.buckets.get_mut(key).unwrap());
        rows.swap_remove(slot);
        if let Some(moved) = rows.get(slot) {
            *self
                .owners
                .get_mut(&moved.holder)
                .unwrap()
                .slots
                .get_mut(key)
                .unwrap() = slot;
        }
        if rows.is_empty() {
            self.buckets.remove(key);
            reclaim_sparse_map(&mut self.buckets);
        } else if rows.len() < rows.capacity() / 4 {
            rows.shrink_to_fit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attachments::{
        AttachmentCodec, AttachmentError, AttachmentType, Attachments, Holder,
    };
    use crate::nbt::Tag;

    static VALUE: AttachmentType<i32> = AttachmentType {
        namespace: "test",
        name: "value",
        default: || 0,
        codec: Some(AttachmentCodec {
            encode: |n| Ok([("value".into(), Tag::Int(*n))].into()),
            decode: |form| match form.get(&"value".into()) {
                Some(Tag::Int(n)) => Ok(*n),
                _ => Err(AttachmentError::Decode),
            },
        }),
        copy_on_death: false,
        sync: false,
    };

    fn key(name: &str) -> AttachmentKey {
        ("test".into(), name.into())
    }

    fn entry(n: i32) -> Arc<AttachmentEntry> {
        let mut map = Attachments::default();
        VALUE.set(&mut map, Holder::Level, n).unwrap();
        map.entry("test", "value").unwrap().unwrap().clone()
    }

    fn values(rows: &[AttachmentRow<u32>]) -> Vec<(u32, i32)> {
        rows.iter()
            .map(|row| (row.holder, *row.entry.read(&VALUE).unwrap()))
            .collect()
    }

    #[test]
    fn snapshots_keep_membership_and_values_while_current_rows_change() {
        let mut index = AttachmentIndex::default();
        let original = entry(10);
        let a = index.register(1, [(key("value"), original.clone())]);
        let b = index.register(2, [(key("value"), entry(20))]);
        let before = index.snapshot(&key("value"));
        assert!(Arc::ptr_eq(&before[0].entry, &original));
        assert!(index.set(&a, &key("value"), entry(11)));
        assert!(index.remove(&b, &key("value")));
        index.register(3, [(key("value"), entry(30))]);
        assert_eq!(values(&before), [(1, 10), (2, 20)]);
        assert_eq!(values(&index.snapshot(&key("value"))), [(1, 11), (3, 30)]);
    }

    #[test]
    fn saved_entries_join_without_a_typed_read_and_remain_snapshot_values() {
        let mut source = Attachments::default();
        VALUE.set(&mut source, Holder::Level, 42).unwrap();
        let nbt = source.to_nbt();
        let loaded = Attachments::from_nbt(nbt.clone());
        let raw = loaded.entry("test", "value").unwrap().unwrap().clone();
        let mut index = AttachmentIndex::default();
        let owner = index.register(1, [(key("value"), raw.clone())]);
        let before = index.snapshot(&key("value"));
        assert!(Arc::ptr_eq(&raw, &before[0].entry));
        index.set(&owner, &key("value"), entry(99));
        assert_eq!(values(&before), [(1, 42)]);
        assert_eq!(values(&index.snapshot(&key("value"))), [(1, 99)]);
        assert_eq!(loaded.to_nbt(), nbt);
    }

    #[test]
    fn writes_copy_rows_once_per_retained_version_and_keep_other_entries_shared() {
        let mut index = AttachmentIndex::default();
        let a = index.register(1, [(key("value"), entry(1))]);
        let untouched = entry(2);
        index.register(2, [(key("value"), untouched.clone())]);
        let before = index.snapshot(&key("value"));
        index.set(&a, &key("value"), entry(10));
        let current = index.snapshot(&key("value"));
        assert!(!Arc::ptr_eq(&before, &current));
        assert!(Arc::ptr_eq(&untouched, &current[1].entry));
        let current_version = Arc::as_ptr(&current);
        drop(current);
        index.set(&a, &key("value"), entry(11));
        let after = index.snapshot(&key("value"));
        assert_eq!(Arc::as_ptr(&after), current_version);
        assert_eq!(values(&before), [(1, 1), (2, 2)]);
        assert_eq!(values(&after), [(1, 11), (2, 2)]);
    }

    #[test]
    fn swap_removal_repairs_slots_and_replacement_preserves_order() {
        let mut index = AttachmentIndex::default();
        let a = index.register(1, [(key("value"), entry(1))]);
        let b = index.register(2, [(key("value"), entry(2))]);
        let c = index.register(3, [(key("value"), entry(3))]);
        let d = index.register(4, [(key("value"), entry(4))]);
        assert!(index.remove(&b, &key("value")));
        assert!(index.set(&d, &key("value"), entry(40)));
        assert_eq!(
            values(&index.snapshot(&key("value"))),
            [(1, 1), (4, 40), (3, 3)]
        );
        assert!(index.unregister(&a));
        assert!(index.set(&c, &key("value"), entry(30)));
        assert_eq!(values(&index.snapshot(&key("value"))), [(3, 30), (4, 40)]);
        assert!(index.remove(&d, &key("value")));
        assert_eq!(values(&index.snapshot(&key("value"))), [(3, 30)]);
    }

    #[test]
    fn rebind_rejects_stale_tokens_and_unloads_only_the_owners_keys() {
        let mut index = AttachmentIndex::default();
        let old = index.register(1, [(key("a"), entry(1)), (key("b"), entry(2))]);
        index.register(2, [(key("b"), entry(20))]);
        let retained = index.snapshot(&key("a"));
        let current = index.register(1, [(key("b"), entry(22)), (key("c"), entry(3))]);
        assert!(!index.set(&old, &key("b"), entry(99)));
        assert!(!index.remove(&old, &key("b")));
        assert!(!index.unregister(&old));
        assert_eq!(values(&retained), [(1, 1)]);
        assert!(index.snapshot(&key("a")).is_empty());
        assert_eq!(values(&index.snapshot(&key("b"))), [(1, 22), (2, 20)]);
        assert_eq!(values(&index.snapshot(&key("c"))), [(1, 3)]);
        let clone = current.clone();
        drop(current);
        assert_eq!(values(&index.snapshot(&key("c"))), [(1, 3)]);
        assert!(index.unregister(&clone));
        assert!(!index.set(&clone, &key("a"), entry(9)));
        assert_eq!(values(&index.snapshot(&key("b"))), [(2, 20)]);
    }

    #[test]
    fn multi_key_mass_unload_repairs_survivors_reverse_slots() {
        let mut index = AttachmentIndex::default();
        let owners: Vec<_> = (0..64)
            .map(|i| {
                let mut entries = vec![(key("a"), entry(i))];
                if i % 2 == 0 {
                    entries.push((key("b"), entry(i + 100)));
                }
                index.register(i as u32, entries)
            })
            .collect();
        let mut alive = [true; 64];
        for step in 0..64 {
            let removed = step * 17 % 64;
            alive[removed] = false;
            assert!(index.unload(removed as u32));
            for (i, owner) in owners.iter().enumerate().filter(|(i, _)| alive[*i]) {
                assert!(index.set(owner, &key("a"), entry(i as i32 + 1000)));
            }
            for (name, offset) in [("a", 1000), ("b", 100)] {
                let mut actual = values(&index.snapshot(&key(name)));
                actual.sort_unstable();
                let expected: Vec<_> = (0..64)
                    .filter(|&i| alive[i] && (name == "a" || i % 2 == 0))
                    .map(|i| (i as u32, i as i32 + offset))
                    .collect();
                assert_eq!(actual, expected);
            }
        }
        assert!(index.snapshot(&key("a")).is_empty());
        assert!(index.snapshot(&key("b")).is_empty());
    }

    #[test]
    fn same_key_rebind_preserves_slots_and_only_changed_entries_copy_rows() {
        let mut index = AttachmentIndex::default();
        let original = entry(1);
        let old = index.register(1, [(key("value"), original.clone())]);
        index.register(2, [(key("value"), entry(2))]);
        let retained = index.snapshot(&key("value"));
        let refreshed = index.register(1, [(key("value"), original)]);
        assert!(Arc::ptr_eq(&retained, &index.snapshot(&key("value"))));
        assert!(!index.set(&old, &key("value"), entry(99)));
        let current = index.register(1, [(key("value"), entry(10))]);
        assert!(!index.unregister(&refreshed));
        assert_eq!(values(&retained), [(1, 1), (2, 2)]);
        assert_eq!(values(&index.snapshot(&key("value"))), [(1, 10), (2, 2)]);
        assert!(index.unregister(&current));
    }

    #[test]
    fn trusted_unload_removes_detached_registration_but_preserves_snapshots() {
        let mut index = AttachmentIndex::default();
        let binding = index.register(1, [(key("value"), entry(7))]);
        let stale = binding.clone();
        let retained = index.snapshot(&key("value"));
        drop(binding);
        assert!(index.unload(1));
        assert!(!index.unload(1));
        assert!(index.snapshot(&key("value")).is_empty());
        assert_eq!(values(&retained), [(1, 7)]);
        assert!(!index.set(&stale, &key("value"), entry(99)));
        assert!(!index.unregister(&stale));
        index.register(1, [(key("value"), entry(8))]);
        assert!(!index.set(&stale, &key("value"), entry(99)));
        assert!(!index.unregister(&stale));
        assert_eq!(values(&index.snapshot(&key("value"))), [(1, 8)]);
    }

    #[test]
    fn tokens_from_another_index_cannot_publish_or_unload() {
        let mut first = AttachmentIndex::default();
        let foreign = first.register(1, [(key("value"), entry(1))]);
        let mut second = AttachmentIndex::default();
        second.register(1, [(key("value"), entry(2))]);
        assert!(!second.set(&foreign, &key("value"), entry(99)));
        assert!(!second.remove(&foreign, &key("value")));
        assert!(!second.unregister(&foreign));
        assert_eq!(values(&second.snapshot(&key("value"))), [(1, 2)]);
    }

    #[test]
    fn missing_keys_reuse_empty_snapshot_and_do_not_become_memberships() {
        let mut index = AttachmentIndex::<u32>::default();
        let empty = index.snapshot(&key("missing"));
        assert!(empty.is_empty());
        assert!(Arc::ptr_eq(&empty, &index.snapshot(&key("other"))));
        let owner = index.register(1, []);
        assert!(index.remove(&owner, &key("missing")));
        assert!(index.unregister(&owner));
        assert!(Arc::ptr_eq(&empty, &index.snapshot(&key("missing"))));
        index.register(1, [(key("missing"), entry(7)), (key("missing"), entry(8))]);
        assert_eq!(values(&index.snapshot(&key("missing"))), [(1, 8)]);
        assert!(empty.is_empty());
    }
}

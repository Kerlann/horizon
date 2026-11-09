//! Physics-only merge keys. Stable ordinals order candidates; joins resolve
//! current storage. Neither mode caches resident element positions.
use horizon_client::{
    entities::{ItemCells, ItemEntity, ItemMergeRemovals},
    fast_hash::{FxHashMap, FxHashSet},
    items::ItemStack,
    join_seq::{JoinList, JoinSeq},
};
use horizon_core::geometry::Vec3;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Key {
    order: u128,
    join: JoinSeq,
}
#[derive(Default)]
struct Removed {
    keys: FxHashSet<Key>,
    pending: Vec<JoinSeq>,
}
impl ItemMergeRemovals<Key> for Removed {
    fn is_removed(&self, key: Key) -> bool {
        self.keys.contains(&key)
    }
    fn mark_removed(&mut self, key: Key) {
        if self.keys.insert(key) {
            self.pending.push(key.join);
        }
    }
}
enum Mode {
    Compact {
        removed: Vec<bool>,
        cells: ItemCells,
    },
    Joined {
        keys: FxHashMap<JoinSeq, Key>,
        ids: FxHashMap<u32, JoinSeq>,
        removed: Removed,
        cells: ItemCells<Key>,
    },
}
pub(super) struct ItemMergeIndex(Mode);
impl ItemMergeIndex {
    pub(super) fn new(items: &[ItemEntity], keyed: bool) -> Self {
        Self(if keyed {
            let mut keys = FxHashMap::default();
            let mut ids = FxHashMap::default();
            let cells = ItemCells::with_keys(items, |order, item| {
                let key = Key {
                    order: order as u128,
                    join: item.join,
                };
                keys.insert(item.join, key);
                ids.insert(item.id, item.join);
                key
            });
            Mode::Joined {
                keys,
                ids,
                cells,
                removed: Removed::default(),
            }
        } else {
            Mode::Compact {
                removed: vec![false; items.len()],
                cells: ItemCells::new(items),
            }
        })
    }
    pub(super) fn remove_id(&mut self, id: u32) {
        if let Mode::Joined {
            keys,
            ids,
            removed,
            cells,
        } = &mut self.0
            && let Some(&join) = ids.get(&id)
        {
            let key = keys[&join];
            removed.mark_removed(key);
            cells.forget_key(key);
        }
    }
    pub(super) fn take_retired(&mut self) -> Vec<JoinSeq> {
        match &mut self.0 {
            Mode::Joined { removed, .. } => std::mem::take(&mut removed.pending),
            Mode::Compact { .. } => Vec::new(),
        }
    }
    pub(super) fn refresh(&mut self, join: JoinSeq, source: Option<&ItemEntity>) {
        if let Mode::Joined {
            keys,
            ids,
            removed,
            cells,
        } = &mut self.0
        {
            if let Some(source) = source {
                ids.insert(source.id, join);
                let order = keys.len() as u128;
                let key = *keys.entry(join).or_insert(Key { order, join });
                if !removed.is_removed(key) {
                    cells.refresh_key(key, source.position);
                }
            } else if let Some(&key) = keys.get(&join) {
                cells.forget_key(key);
            }
        }
    }
    pub(super) fn is_removed(&self, i: usize, join: JoinSeq) -> bool {
        match &self.0 {
            Mode::Compact { removed, .. } => removed[i],
            Mode::Joined { keys, removed, .. } => removed.is_removed(keys[&join]),
        }
    }
    pub(super) fn discard(&mut self, i: usize, join: JoinSeq, at: Vec3) {
        match &mut self.0 {
            Mode::Compact { removed, cells } => {
                removed[i] = true;
                cells.remove(i, at);
            }
            Mode::Joined {
                keys,
                removed,
                cells,
                ..
            } => {
                let key = keys[&join];
                removed.mark_removed(key);
                cells.forget_key(key);
            }
        }
    }
    pub(super) fn moved(&mut self, i: usize, join: JoinSeq, from: Vec3, to: Vec3) {
        match &mut self.0 {
            Mode::Compact { cells, .. } => cells.moved(i, from, to),
            Mode::Joined { keys, cells, .. } => cells.moved_key(keys[&join], from, to),
        }
    }
    pub(super) fn merge(
        &mut self,
        items: &mut JoinList<ItemEntity>,
        i: usize,
        max_stack: &dyn Fn(&ItemStack) -> u32,
    ) {
        match &mut self.0 {
            Mode::Compact { removed, cells } => {
                horizon_client::entities::merge_with_neighbours(items, removed, cells, i, max_stack)
            }
            Mode::Joined {
                keys,
                removed,
                cells,
                ..
            } => {
                let key = keys[&items[i].join];
                horizon_client::entities::merge_with_neighbours_keyed(
                    items,
                    removed,
                    cells,
                    key,
                    |items, key| items.locate(key.join),
                    max_stack,
                );
            }
        }
    }
}

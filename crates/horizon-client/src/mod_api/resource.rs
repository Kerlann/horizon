//! NeoForge 26.3's transfer API (#41 §7): a resource is what a handler holds, without an amount
//! ([`ItemResource`]), and a [`ResourceHandler`] is the indexed contents of one holder, written
//! only inside a transaction.
use super::{Host, ItemStack, Transaction};
use std::fmt;

/// An item and its components, without a count, as NeoForge's `ItemResource`: what a slot holds.
/// An empty slot holds none.
#[derive(Clone)]
pub struct ItemResource(ItemStack);

impl ItemResource {
    /// What `stack` is, whatever its count.
    pub fn of(stack: &ItemStack) -> Self {
        Self(ItemStack {
            count: 1,
            ..stack.clone()
        })
    }
    /// The item's id (`minecraft:apple`).
    pub fn item(&self) -> &str {
        &self.0.item
    }
    /// `count` of it as a stack.
    pub fn to_stack(&self, count: u32) -> ItemStack {
        ItemStack {
            count,
            ..self.0.clone()
        }
    }
    /// Whether `stack` is this item with these components.
    pub fn matches(&self, stack: &ItemStack) -> bool {
        self.0.same_item_same_components(stack)
    }
}

impl PartialEq for ItemResource {
    fn eq(&self, other: &Self) -> bool {
        self.matches(&other.0)
    }
}
impl Eq for ItemResource {}

impl fmt::Debug for ItemResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ItemResource").field(&self.0.item).finish()
    }
}

/// The contents of one holder (a block entity as one side reaches it, an item where it lives),
/// numbered from 0, as NeoForge's `ResourceHandler`: a provider makes one for its holder, so it
/// knows where its contents live. It holds no borrow of the game: each call is given the host,
/// and its state is the game's (slots, a saved compound, attachments, a stack's components), so
/// the transaction `tx` a write runs in puts it back on an abort. Amounts never exceed what was
/// asked; a handler answering more is a mod bug, and the move it was part of is aborted.
pub trait ResourceHandler<R> {
    /// How many indices it has.
    fn size(&self, host: &mut dyn Host) -> usize;
    /// What index `index` holds, if anything.
    fn resource(&self, host: &mut dyn Host, index: usize) -> Option<R>;
    /// How much of it.
    fn amount(&self, host: &mut dyn Host, index: usize) -> u32;
    /// How much of `resource` index `index` holds when it holds that, whatever it holds now: none
    /// where `resource` may not go (`getCapacityAsLong`).
    fn capacity(&self, host: &mut dyn Host, index: usize, resource: &R) -> u32;
    /// Whether `resource` may go in index `index` at all (`isValid`).
    fn is_valid(&self, host: &mut dyn Host, index: usize, resource: &R) -> bool {
        self.capacity(host, index, resource) > 0
    }
    /// Puts up to `amount` of `resource` into index `index`: how much went in.
    fn insert(
        &self,
        host: &mut dyn Host,
        tx: &Transaction,
        index: usize,
        resource: &R,
        amount: u32,
    ) -> u32;
    /// Takes up to `amount` of `resource` out of index `index`: how much came out. None of
    /// another resource.
    fn extract(
        &self,
        host: &mut dyn Host,
        tx: &Transaction,
        index: usize,
        resource: &R,
        amount: u32,
    ) -> u32;
    /// Puts up to `amount` of `resource` in wherever it goes (NeoForge's `insert` without an
    /// index): by default index by index from 0. How much went in.
    fn insert_any(&self, host: &mut dyn Host, tx: &Transaction, resource: &R, amount: u32) -> u32 {
        let mut went = 0u32;
        for index in 0..self.size(host) {
            if went >= amount {
                break;
            }
            went = went.saturating_add(self.insert(host, tx, index, resource, amount - went));
        }
        went
    }
    /// Takes up to `amount` of `resource` out from wherever it is (`extract` without an index):
    /// by default index by index from 0. How much came out.
    fn extract_any(&self, host: &mut dyn Host, tx: &Transaction, resource: &R, amount: u32) -> u32 {
        let mut came = 0u32;
        for index in 0..self.size(host) {
            if came >= amount {
                break;
            }
            came = came.saturating_add(self.extract(host, tx, index, resource, amount - came));
        }
        came
    }
}

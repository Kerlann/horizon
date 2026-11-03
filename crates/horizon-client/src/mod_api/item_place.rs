//! Items as holders (arismoko/horizon#41 §7): an item's own capability, such as
//! [`ITEM_ITEMS`] for a backpack's slots, reached where its stack lives. NeoForge 26.3 reaches
//! an item through an `ItemAccess`; here the place is an [`ItemPlace`], and every read and write
//! of a place goes through [`place_stack`](dyn Host::place_stack) and
//! [`set_place_stack`](dyn Host::set_place_stack), so the inventory holding the stack is the one
//! changed. Handlers hold no borrow of the game, as block entity capabilities don't (#71).
//!
//! A write plans with `simulate`, then commits: the old stack must come out whole and the new
//! one go in whole, or it refuses before anything moves and hands the stack back. That holds
//! while every handler's `simulate` answers as its real call does; one that doesn't is a mod bug,
//! and what it leaves out is put back, else dropped where it was, never lost or made.
use super::{Direction, Hand, Host, ITEMS, ItemHandler, ItemStack};
use std::{
    cell::RefCell,
    collections::HashSet,
    marker::PhantomData,
    sync::{Arc, LazyLock, Mutex},
};

/// Where a stack lives.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemPlace {
    /// Slot `slot` of a player's inventory, by `native_inventory_tick`'s numbers: 0..36 (0..9
    /// the hotbar), the armour 100..=103 (feet to head), the off hand -106
    /// (`ItemAccess.forPlayerSlot`).
    PlayerSlot { player: u128, slot: i32 },
    /// What a player holds in `hand` (`forPlayerInteraction`).
    PlayerHand { player: u128, hand: Hand },
    /// The stack on a player's cursor in a menu (`forPlayerCursor`).
    PlayerCursor { player: u128 },
    /// Slot `slot` of the block entity at `pos` as `side` reaches it, through its [`ITEMS`]
    /// handler (`forHandlerIndex`).
    BlockEntity {
        pos: [i32; 3],
        side: Option<Direction>,
        slot: usize,
    },
    /// The stack of item entity `id` in the level in place.
    ItemEntity { id: u32 },
    /// Slot `slot` of the item at `parent`, through its [`ITEM_ITEMS`] handler: a backpack in a
    /// backpack.
    Nested { parent: Box<ItemPlace>, slot: usize },
    /// A place a mod keeps, such as an accessory slot: read and written by the [`PlaceProvider`]
    /// it registered as `provider`, which alone knows what `key` means.
    Mod { provider: &'static str, key: u64 },
}

/// A mod's own places ([`ItemPlace::Mod`]), registered with
/// [`Registrar::place_provider`](super::Registrar::place_provider). Like a handler, it holds no
/// borrow: its stacks live where the mod keeps them (a player's attachments).
pub trait PlaceProvider: Send + Sync + 'static {
    /// The stack at `key`, if any.
    fn stack(&self, host: &mut dyn Host, key: u64) -> Option<ItemStack>;
    /// Puts `stack` in place of what is at `key` (a count of 0 empties it). With `one_by_one`,
    /// `stack` stands for one item of what is there: from a stack of more than one, `key` keeps
    /// the others and `stack` goes where the mod puts a split-off item, such as the player's
    /// inventory, else the ground (`host.set_game_stack` does both for a player's place). All
    /// or nothing: false when nothing changed, as where `key` is no longer a place. `simulate`
    /// changes nothing and answers as the real call would.
    fn set_stack(
        &self,
        host: &mut dyn Host,
        key: u64,
        stack: ItemStack,
        one_by_one: bool,
        simulate: bool,
    ) -> bool;
}

/// An item capability: its name, the handler trait `T` it answers with, and the game's own
/// answer for a stack where no part is registered for its item, if the game has one.
pub struct ItemCapabilityKey<T: ?Sized + 'static> {
    pub name: &'static str,
    pub(super) default: Option<fn(&mut dyn Host, &ItemStack) -> Option<Arc<T>>>,
    pub(super) handler: PhantomData<fn() -> Arc<T>>,
}

impl<T: ?Sized + 'static> ItemCapabilityKey<T> {
    /// A mod's own item capability, answered only by the parts registered for it.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            default: None,
            handler: PhantomData,
        }
    }
}

/// Items held by an item, as NeoForge 26.3's `Capabilities.Item.ITEM`. The game answers it for
/// an item with `item_container` slots, kept in its `minecraft:container`.
pub const ITEM_ITEMS: ItemCapabilityKey<dyn StackItemHandler> = ItemCapabilityKey {
    name: "horizon:item_items",
    default: Some(|host, stack| {
        static GAME: LazyLock<Arc<dyn StackItemHandler>> =
            LazyLock::new(|| Arc::new(GameItemItems));
        host.item_container_slots(stack).map(|_| GAME.clone())
    }),
    handler: PhantomData,
};

/// The slots of the item at `at`, numbered from 0. A change rewrites the item at `at` through
/// [`set_place_stack`](dyn Host::set_place_stack) with `one_by_one`: from a stack of more than
/// one, the changed item is split off. When that hands the item back, nothing changed, and the
/// handler reports nothing moved. `simulate` changes nothing and must answer exactly as the
/// real call would, as [`ItemHandler`]'s must: pass it on to `set_place_stack`.
pub trait StackItemHandler: Send + Sync + 'static {
    fn slots(&self, host: &mut dyn Host, at: &ItemPlace) -> usize;
    fn stack(&self, host: &mut dyn Host, at: &ItemPlace, slot: usize) -> Option<ItemStack>;
    /// Puts as much of `stack` as slot `slot` takes; what didn't fit.
    fn insert(
        &self,
        host: &mut dyn Host,
        at: &ItemPlace,
        slot: usize,
        stack: ItemStack,
        simulate: bool,
    ) -> Option<ItemStack>;
    /// Takes up to `max` out of slot `slot`; what came out.
    fn extract(
        &self,
        host: &mut dyn Host,
        at: &ItemPlace,
        slot: usize,
        max: u32,
        simulate: bool,
    ) -> Option<ItemStack>;
    /// How many of `stack` slot `slot` holds when empty, as [`ItemHandler::capacity`].
    fn capacity(&self, host: &mut dyn Host, at: &ItemPlace, slot: usize, stack: &ItemStack) -> u32;
}

/// The game's own [`ITEM_ITEMS`], over the `item_container` helpers.
struct GameItemItems;
impl StackItemHandler for GameItemItems {
    fn slots(&self, host: &mut dyn Host, at: &ItemPlace) -> usize {
        host.place_stack(at)
            .and_then(|held| host.item_container_slots(&held))
            .map_or(0, |slots| slots.len())
    }
    fn stack(&self, host: &mut dyn Host, at: &ItemPlace, slot: usize) -> Option<ItemStack> {
        let held = host.place_stack(at)?;
        host.item_container_slots(&held)?.get(slot)?.clone()
    }
    fn insert(
        &self,
        host: &mut dyn Host,
        at: &ItemPlace,
        slot: usize,
        stack: ItemStack,
        simulate: bool,
    ) -> Option<ItemStack> {
        let Some((went, changed)) = host
            .place_stack(at)
            .and_then(|held| host.item_container_insert_slot(&held, slot, &stack))
        else {
            return Some(stack);
        };
        if went == 0 || host.set_place_stack(at, changed, true, simulate).is_some() {
            return Some(stack);
        }
        (went < stack.count).then(|| ItemStack {
            count: stack.count - went,
            ..stack
        })
    }
    fn extract(
        &self,
        host: &mut dyn Host,
        at: &ItemPlace,
        slot: usize,
        max: u32,
        simulate: bool,
    ) -> Option<ItemStack> {
        let held = host.place_stack(at)?;
        let (out, changed) = host.item_container_extract(&held, slot, max)?;
        let out = out?;
        if host.set_place_stack(at, changed, true, simulate).is_some() {
            return None;
        }
        Some(out)
    }
    fn capacity(&self, host: &mut dyn Host, at: &ItemPlace, slot: usize, stack: &ItemStack) -> u32 {
        host.place_stack(at)
            .map_or(0, |held| host.item_container_capacity(&held, slot, stack))
    }
}

thread_local! {
    /// One item of the stack at a place, held here while a write into one of its slots
    /// (`set_nested`) changes it: reads and writes of that place come here, so nothing real
    /// changes until the whole write is known to go through.
    static STAGED: RefCell<Vec<(ItemPlace, ItemStack)>> = const { RefCell::new(Vec::new()) };
}

/// Drops the newest staged item, also when a handler panics.
struct Unstage;
impl Drop for Unstage {
    fn drop(&mut self) {
        STAGED.with_borrow_mut(Vec::pop);
    }
}

/// The owners of item handlers whose real calls disagreed with their simulation, warned once.
static BROKEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// `owner`'s item handler at `at` moved other than it was asked or its simulation said: a mod
/// bug, warned once per mod.
pub(crate) fn warn_broken(owner: Option<usize>, at: &dyn std::fmt::Debug) {
    let owner = owner.map_or_else(|| "the game".to_owned(), super::mod_name);
    let mut warned = BROKEN.lock().unwrap_or_else(|e| e.into_inner());
    if warned.get_or_insert_default().insert(owner.clone()) {
        log::warn!(
            "Horizon: {owner}'s item handler at {at:?} moved other than it was asked or its \
             simulation said; what it left out is put back or dropped there. Its simulate must \
             answer as its real call does."
        );
    }
}

#[cfg(test)]
pub(super) fn warned_about(owner: &str) -> bool {
    let broken = BROKEN.lock().unwrap_or_else(|e| e.into_inner());
    broken.as_ref().is_some_and(|owners| owners.contains(owner))
}

/// The slots a place sits in: a block entity's, through [`ITEMS`], or an item's, through
/// [`ITEM_ITEMS`] (whose writes are staged).
enum Container<'p> {
    Block(Arc<dyn ItemHandler>, [i32; 3], Option<Direction>),
    Item(Arc<dyn StackItemHandler>, &'p ItemPlace),
}

/// Whether `out` is exactly `count` of `old`.
fn is_exactly(out: Option<&ItemStack>, old: &ItemStack, count: u32) -> bool {
    out.is_some_and(|out| out.count == count && out.same_item_same_components(old))
}

impl Container<'_> {
    /// The mod that registered the handler; none for the game's own.
    fn owner(&self) -> Option<usize> {
        match self {
            Self::Block(h, ..) => super::capability_mod(h),
            Self::Item(h, _) => super::capability_mod(h),
        }
    }
    fn slots(&self, host: &mut dyn Host) -> usize {
        match self {
            Self::Block(h, pos, side) => h.slots(host, *pos, *side),
            Self::Item(h, at) => h.slots(host, at),
        }
    }
    fn stack(&self, host: &mut dyn Host, slot: usize) -> Option<ItemStack> {
        match self {
            Self::Block(h, pos, side) => h.stack(host, *pos, *side, slot),
            Self::Item(h, at) => h.stack(host, at, slot),
        }
    }
    fn insert(
        &self,
        host: &mut dyn Host,
        slot: usize,
        stack: ItemStack,
        simulate: bool,
    ) -> Option<ItemStack> {
        match self {
            Self::Block(h, pos, side) => h.insert(host, *pos, *side, slot, stack, simulate),
            Self::Item(h, at) => h.insert(host, at, slot, stack, simulate),
        }
    }
    fn extract(
        &self,
        host: &mut dyn Host,
        slot: usize,
        max: u32,
        simulate: bool,
    ) -> Option<ItemStack> {
        match self {
            Self::Block(h, pos, side) => h.extract(host, *pos, *side, slot, max, simulate),
            Self::Item(h, at) => h.extract(host, at, slot, max, simulate),
        }
    }
    /// Inserts `stack` into `slot` for real, in passes until one takes nothing, as
    /// `Inventory.add` and NeoForge's insert loops do: a handler may take less per call than
    /// it holds. What is left: of the offered item, and never more than was offered. A leftover
    /// of another item, or of other components, is a mod bug, warned once naming its mod: the
    /// passes end, and the leftover is taken as the offered item.
    fn insert_all(&self, host: &mut dyn Host, slot: usize, stack: ItemStack) -> Option<ItemStack> {
        let mut left = stack;
        loop {
            let count = left.count;
            match self.insert(host, slot, left.clone(), false) {
                Some(rest) if !rest.same_item_same_components(&left) => {
                    match self {
                        Self::Block(_, pos, _) => warn_broken(self.owner(), pos),
                        Self::Item(_, place) => warn_broken(self.owner(), place),
                    }
                    left.count = rest.count.min(count);
                    return Some(left).filter(|left| left.count > 0);
                }
                Some(rest) if rest.count >= count => return Some(left),
                Some(rest) if rest.count > 0 => left.count = rest.count,
                _ => return None,
            }
        }
    }
    fn capacity(&self, host: &mut dyn Host, slot: usize, stack: &ItemStack) -> u32 {
        match self {
            Self::Block(h, pos, side) => h.capacity(host, *pos, *side, slot, stack),
            Self::Item(h, at) => h.capacity(host, at, slot, stack),
        }
    }

    /// Puts `stack` in `slot` in place of what is there, planned with `simulate` first: all of
    /// the old stack must come out, and `stack` must fit the emptied slot. An occupied slot
    /// can't be asked that before it is emptied, so its capacity (a hint, as NeoForge's) is
    /// asked first; then the old stack comes out, the empty slot is asked by a simulated insert,
    /// and where that falls short the old stack goes back and the write is refused, with
    /// nothing moved. With `one_by_one`, from a stack of more than one, one item comes out and
    /// `stack` goes into the other slots (`ItemAccess.forHandlerIndex`). With `simulate`, only
    /// the plan. All or nothing, while the handler's real calls answer as its simulations did
    /// (`broken` when they don't). Real inserts go in passes (`insert_all`).
    fn set(
        &self,
        host: &mut dyn Host,
        place: &ItemPlace,
        slot: usize,
        stack: ItemStack,
        one_by_one: bool,
        simulate: bool,
    ) -> bool {
        let old = self.stack(host, slot);
        if one_by_one && let Some(old) = old.as_ref().filter(|old| old.count > 1) {
            let out = self.extract(host, slot, 1, true);
            if !is_exactly(out.as_ref(), old, 1) {
                return false;
            }
            let Some(plan) = self.plan_into_others(host, slot, &stack) else {
                return false;
            };
            if simulate {
                return true;
            }
            let out = self.extract(host, slot, 1, false);
            if !is_exactly(out.as_ref(), old, 1) {
                self.broken(host, place, slot, out);
                return false;
            }
            let mut left = 0;
            for (to, count) in plan {
                let some = ItemStack {
                    count,
                    ..stack.clone()
                };
                left += self.insert_all(host, to, some).map_or(0, |rest| rest.count);
            }
            let left = (left > 0).then_some(ItemStack {
                count: left,
                ..stack
            });
            return left.is_none() || self.broken(host, place, slot, left);
        }
        if let Some(old) = &old
            && !is_exactly(
                self.extract(host, slot, old.count, true).as_ref(),
                old,
                old.count,
            )
        {
            return false;
        }
        // An occupied slot can't be asked whether `stack` fits it once emptied: its capacity
        // answers that, NeoForge's `isValid` and `getCapacityAsLong`.
        let fits = stack.count == 0
            || match &old {
                Some(_) => self.capacity(host, slot, &stack) >= stack.count,
                None => self.insert(host, slot, stack.clone(), true).is_none(),
            };
        if !fits {
            return false;
        }
        if simulate {
            return true;
        }
        if let Some(old) = &old {
            let out = self.extract(host, slot, old.count, false);
            if !is_exactly(out.as_ref(), old, old.count) {
                self.broken(host, place, slot, out);
                return false;
            }
            // Emptied, the slot answers for itself. Short of room, the old stack goes back, in
            // passes, and the write is refused cleanly; only what the slot then won't take
            // back is `broken`.
            if stack.count > 0 && self.insert(host, slot, stack.clone(), true).is_some() {
                let rest = self.insert_all(host, slot, old.clone());
                if rest.is_some() {
                    self.broken(host, place, slot, rest);
                }
                return false;
            }
        }
        if stack.count == 0 {
            return true;
        }
        match self.insert_all(host, slot, stack.clone()) {
            None => true,
            // Nothing went in: the old stack goes back, and the write is refused.
            Some(rest) if rest.count >= stack.count => {
                self.broken(host, place, slot, old);
                false
            }
            rest => self.broken(host, place, slot, rest),
        }
    }

    /// Where all of `stack` goes in the slots but `skip`, simulated slot by slot: how many into
    /// which. None when it doesn't all fit.
    fn plan_into_others(
        &self,
        host: &mut dyn Host,
        skip: usize,
        stack: &ItemStack,
    ) -> Option<Vec<(usize, u32)>> {
        let mut plan = Vec::new();
        let mut left = stack.count;
        for slot in (0..self.slots(host)).filter(|slot| *slot != skip) {
            if left == 0 {
                break;
            }
            let some = ItemStack {
                count: left,
                ..stack.clone()
            };
            let rest = self
                .insert(host, slot, some, true)
                .map_or(0, |rest| rest.count);
            if rest < left {
                plan.push((slot, left - rest));
                left = rest;
            }
        }
        (left == 0).then_some(plan)
    }

    /// The handler's real call disagreed with its simulation: a mod bug, warned once per mod.
    /// In a block entity, `loose` goes back into `slot`, and what it won't take is dropped at
    /// the block: the write stands as far as it went (true). In an item, the write is staged,
    /// so it is dropped whole and nothing real changed (false).
    fn broken(
        &self,
        host: &mut dyn Host,
        place: &ItemPlace,
        slot: usize,
        loose: Option<ItemStack>,
    ) -> bool {
        warn_broken(self.owner(), place);
        let Self::Block(_, pos, _) = self else {
            return false;
        };
        if let Some(rest) = loose.and_then(|loose| self.insert_all(host, slot, loose)) {
            host.drop_at(*pos, rest);
        }
        true
    }
}

impl dyn Host + '_ {
    /// `key`'s handler for the item at `place`: the part a mod registered for its item, else
    /// the game's own answer, if the game has one.
    pub fn item_capability<T: ?Sized + 'static>(
        &mut self,
        key: &ItemCapabilityKey<T>,
        place: &ItemPlace,
    ) -> Option<Arc<T>> {
        let stack = self.place_stack(place)?;
        super::item_capability(key, || Some(&*stack.item))
            .or_else(|| key.default.and_then(|default| default(self, &stack)))
    }

    /// The stack at `place`, if any: the one place every item handler reads.
    pub fn place_stack(&mut self, place: &ItemPlace) -> Option<ItemStack> {
        let staged = STAGED.with_borrow(|staged| {
            staged
                .iter()
                .rev()
                .find(|(at, _)| at == place)
                .map(|(_, stack)| stack.clone())
        });
        match (staged, place) {
            (Some(stack), _) => Some(stack),
            (None, ItemPlace::BlockEntity { pos, side, slot }) => self
                .capability(&ITEMS, *pos)?
                .stack(self, *pos, *side, *slot),
            (None, ItemPlace::Nested { parent, slot }) => self
                .item_capability(&ITEM_ITEMS, parent)?
                .stack(self, parent, *slot),
            (None, ItemPlace::Mod { provider, key }) => {
                super::place_provider(provider)?.stack(self, *key)
            }
            (None, _) => self.game_stack(place),
        }
        .filter(|stack| stack.count > 0)
    }

    /// Puts `stack` at `place` (a count of 0 empties it): the one place every item handler
    /// writes. With `one_by_one`, `stack` stands for one item of what is there, as
    /// `ItemAccess.oneByOne`: from a stack of more than one, the place keeps the others and
    /// `stack` goes elsewhere. From a player's hand it goes as vanilla's
    /// `ItemUtils.createFilledResult` puts a filled bucket: into their inventory, else dropped,
    /// and in creative the hand is kept. From a player's other places the same, without
    /// creative's rule. From an item entity, it's dropped there. From a block entity's slot or
    /// an item's, into that container's other slots. From a mod's place, where its
    /// [`PlaceProvider`] puts it. A stack of items holding the place splits the same way: one
    /// of them is rewritten holding the change, and the others stay as they were.
    ///
    /// Planned with `simulate` first, then done: None when it's done, Some(`stack`) when it
    /// can't be, and nothing moved. `simulate` answers the same, changing nothing.
    pub fn set_place_stack(
        &mut self,
        place: &ItemPlace,
        stack: ItemStack,
        one_by_one: bool,
        simulate: bool,
    ) -> Option<ItemStack> {
        let staged = STAGED.with_borrow_mut(|staged| {
            let at = staged.iter_mut().rev().find(|(at, _)| at == place);
            at.map(|(_, held)| {
                if !simulate {
                    *held = stack.clone();
                }
            })
            .is_some()
        });
        let done = staged
            || match place {
                ItemPlace::BlockEntity { pos, side, slot } => {
                    self.capability(&ITEMS, *pos).is_some_and(|handler| {
                        let container = Container::Block(handler, *pos, *side);
                        container.set(self, place, *slot, stack.clone(), one_by_one, simulate)
                    })
                }
                ItemPlace::Nested { parent, slot } => {
                    self.set_nested(parent, *slot, stack.clone(), one_by_one, simulate)
                }
                ItemPlace::Mod { provider, key } => {
                    super::place_provider(provider).is_some_and(|provider| {
                        provider.set_stack(self, *key, stack.clone(), one_by_one, simulate)
                    })
                }
                _ => self.set_game_stack(place, stack.clone(), one_by_one, simulate),
            };
        (!done).then_some(stack)
    }

    /// Writes slot `slot` of the item at `parent` on one item of it, staged, then writes that
    /// one item back to `parent` with `one_by_one`: from a stack of them, it splits off holding
    /// the change. Nothing real changes before that last write, so a refusal anywhere changes
    /// nothing, and `simulate` drops the staged change.
    fn set_nested(
        &mut self,
        parent: &ItemPlace,
        slot: usize,
        stack: ItemStack,
        one_by_one: bool,
        simulate: bool,
    ) -> bool {
        let Some(handler) = self.item_capability(&ITEM_ITEMS, parent) else {
            return false;
        };
        let Some(before) = self.place_stack(parent) else {
            return false;
        };
        let one = ItemStack { count: 1, ..before };
        STAGED.with_borrow_mut(|staged| staged.push((parent.clone(), one.clone())));
        let unstage = Unstage;
        let place = ItemPlace::Nested {
            parent: Box::new(parent.clone()),
            slot,
        };
        let done =
            Container::Item(handler, parent).set(self, &place, slot, stack, one_by_one, false);
        let after = STAGED.with_borrow(|staged| staged.last().map(|(_, held)| held.clone()));
        drop(unstage);
        match after {
            Some(after) if done => {
                after == one
                    || self
                        .set_place_stack(parent, after, true, simulate)
                        .is_none()
            }
            _ => false,
        }
    }
}

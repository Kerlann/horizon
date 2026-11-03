//! Capability parts (arismoko/horizon#41 §7): providers a mod registers for its block entity
//! type to answer a capability, such as [`ITEMS`], beside the game's own (NeoForge's
//! `RegisterCapabilitiesEvent`). A handler holds no borrow of the game: each call is given the
//! host, so a handler may look up and call a neighbour's capability (a pipe), and it keeps its
//! state where the game keeps the block entity's (its slots, its saved compound, its
//! attachments).
use super::{Direction, Host, ItemResource, ResourceHandler, Transaction};
use std::{fmt::Debug, marker::PhantomData, ops::Deref};

/// A capability: its name (`horizon:items`, `mymod:chemicals`), the handler trait `T` it answers
/// with, and the game's own provider, if the game has one.
pub struct CapabilityKey<T: ?Sized + 'static> {
    pub name: &'static str,
    pub(super) default: Option<fn(&mut dyn Host, BlockSide) -> Option<Box<T>>>,
    pub(super) handler: PhantomData<fn() -> Box<T>>,
}

/// A block entity a capability is asked of, and the side it is reached through (none: from no
/// particular side): NeoForge's block capability context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockSide {
    pub pos: [i32; 3],
    pub side: Option<Direction>,
}

/// A capability's handler, with who answered: a mod's provider or the game's own.
pub struct Capability<T: ?Sized> {
    pub(super) handler: Box<T>,
    /// The mod whose provider answered; none for the game.
    pub(super) owner: Option<usize>,
}

impl<T: ?Sized> Deref for Capability<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.handler
    }
}

impl<T: ?Sized> Capability<T> {
    /// The id of the mod whose provider answered, or "the game", for warnings.
    pub fn owner(&self) -> String {
        self.owner
            .map_or_else(|| "the game".to_owned(), super::mod_name)
    }
    /// `answer`, the handler's amount for a request of `asked` at `at`: itself, or none beyond
    /// `asked`, a mod bug warned once naming the mod. The caller aborts the move.
    pub(crate) fn checked(&self, answer: u32, asked: u32, at: &dyn Debug) -> Option<u32> {
        if answer > asked {
            super::item_place::warn_broken(self.owner, at);
            return None;
        }
        Some(answer)
    }
}

impl<T: ?Sized + 'static> CapabilityKey<T> {
    /// A mod's own capability, answered only by the parts registered for it.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            default: None,
            handler: PhantomData,
        }
    }
}

/// Item slots, as NeoForge 26.3's `Capabilities.Item.BLOCK`: a block entity's slots as one side
/// reaches them. The game answers it for every block entity with slots: chests (a double chest's
/// two halves as one), furnaces, hoppers and the other vanilla containers, by their
/// `WorldlyContainer` sides, and a mod block entity's `block_entity_container`.
pub const ITEMS: CapabilityKey<dyn ResourceHandler<ItemResource>> = CapabilityKey {
    name: "horizon:items",
    default: Some(|host, at| {
        host.container_size(at.pos, None)
            .map(|_| Box::new(GameItems(at)) as Box<dyn ResourceHandler<ItemResource>>)
    }),
    handler: PhantomData,
};

/// The game's own [`ITEMS`]: the block entity's slots `.side` reaches, by the rules hoppers
/// follow.
struct GameItems(BlockSide);
impl ResourceHandler<ItemResource> for GameItems {
    fn size(&self, host: &mut dyn Host) -> usize {
        host.container_size(self.0.pos, self.0.side).unwrap_or(0)
    }
    fn resource(&self, host: &mut dyn Host, index: usize) -> Option<ItemResource> {
        let stack = host.container_stack(self.0.pos, self.0.side, index)?;
        Some(ItemResource::of(&stack))
    }
    fn amount(&self, host: &mut dyn Host, index: usize) -> u32 {
        host.container_stack(self.0.pos, self.0.side, index)
            .map_or(0, |stack| stack.count)
    }
    fn capacity(&self, host: &mut dyn Host, index: usize, resource: &ItemResource) -> u32 {
        host.container_capacity(self.0.pos, self.0.side, index, &resource.to_stack(1))
    }
    fn insert(
        &self,
        host: &mut dyn Host,
        tx: &Transaction,
        index: usize,
        resource: &ItemResource,
        amount: u32,
    ) -> u32 {
        let stack = resource.to_stack(amount);
        let rest = host.container_insert(self.0.pos, self.0.side, index, stack, tx);
        amount - rest.map_or(0, |rest| rest.count.min(amount))
    }
    fn extract(
        &self,
        host: &mut dyn Host,
        tx: &Transaction,
        index: usize,
        resource: &ItemResource,
        amount: u32,
    ) -> u32 {
        let held = host.container_stack(self.0.pos, self.0.side, index);
        if !held.is_some_and(|held| resource.matches(&held)) {
            return 0;
        }
        host.container_extract(self.0.pos, self.0.side, index, amount, tx)
            .map_or(0, |out| out.count.min(amount))
    }
}

impl dyn Host + '_ {
    /// `key`'s handler for the block entity at `pos`, reached through `side`: the first provider
    /// to answer, of those mods registered for its type `before_game`, the game's own, and the
    /// mods' others, each in registration order.
    pub fn capability<T: ?Sized + 'static>(
        &mut self,
        key: &CapabilityKey<T>,
        pos: [i32; 3],
        side: Option<Direction>,
    ) -> Option<Capability<T>> {
        let at = BlockSide { pos, side };
        let mut game = None;
        let parts = super::providers(false, key.name, self, |host| host.block_entity_type(pos));
        let answer = super::ask_providers::<super::BlockProvider<T>, T>(
            parts,
            self,
            |provider, host| provider(host, at),
            |host| {
                game = key.default.and_then(|default| default(host, at));
                game.is_some()
            },
        );
        match answer {
            super::Answer::Mod(found) => Some(found),
            super::Answer::Game => game.map(|handler| Capability {
                handler,
                owner: None,
            }),
            super::Answer::Nobody => None,
        }
    }
}

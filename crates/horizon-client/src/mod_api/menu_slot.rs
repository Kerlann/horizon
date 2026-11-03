//! A player's open menu's slots as item places (arismoko/horizon#41 §7), a fork addition beside
//! NeoForge 26.3's `ItemAccess.forPlayerSlot`. A slot is the storage it shows, so a write through
//! [`ItemPlace::MenuSlot`] is a write to that storage, journaled once, through its own handler,
//! under the slot's own rules.
use super::{EntityRef, ItemPlace, LevelRef, MenuRef};

/// What a menu slot shows: where [`MenuRef::slot_backing`] says its stack lives.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSlotBacking {
    /// Index `.0` of the menu's player's [`ENTITY_ITEMS`](super::ENTITY_ITEMS): the inventory
    /// 0..36, the armour 36..40 from feet to head, the off hand 40.
    Player(usize),
    /// Slot `slot` of the block entity at `pos`, through its [`ITEMS`](super::ITEMS) handler.
    BlockEntity {
        level: LevelRef,
        pos: [i32; 3],
        slot: usize,
    },
    /// Slot `slot` of `entity`'s [`ENTITY_ITEMS`](super::ENTITY_ITEMS): a chest minecart's, a
    /// chest boat's or a chested mount's.
    Entity { entity: EntityRef, slot: usize },
    /// Slot `.0` of the player's ender chest.
    Ender(usize),
    /// Slot `.0` of the player's inventory crafting grid, 0..4.
    CraftGrid(usize),
    /// The inventory crafting grid's result, which nothing writes: a craft is a click.
    CraftResult,
    /// Slot `.0` of a mod's chest menu (`native_open_menu`).
    Native(usize),
}

impl MenuRef {
    /// The stack on this menu's cursor, as a place (`ItemAccess.forPlayerCursor`): nothing once
    /// the menu is gone.
    pub fn cursor(self) -> ItemPlace {
        ItemPlace::PlayerCursor { menu: self }
    }
}

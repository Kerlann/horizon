pub use horizon_world::consumable::UseAnimation;
#[horizon_mod_api_macros::wasm(via = String)]
pub use horizon_world::identifier::Identifier;
#[horizon_mod_api_macros::wasm(via = Tag)]
pub use horizon_world::nbt::Compound;
#[horizon_mod_api_macros::wasm(via = Vec<u8>)]
pub use horizon_world::nbt::Tag;
pub use horizon_world::player_inventory::EquipmentSlot;
pub use horizon_world::{
    attachments::{
        AttachmentCodec, AttachmentError, AttachmentForm, AttachmentPolicy, AttachmentStore,
        AttachmentType, AttachmentValue, AttachmentWrite, Holder,
    },
    level::{
        BlockId, ConnectionKind, Dimension as LevelDimension, LandingWrite, NeighbourUpdate,
        PlacementContext,
    },
    loot::{LootStack, NativeLootContext},
    registry::BlockStateId,
};
// hidden: the game's maps behind the store, not part of the mod surface
#[doc(hidden)]
pub use horizon_world::attachments::{AttachmentHost, Attachments};
/// The structure box a piece's `bounding_box` answers, in world blocks.
pub use horizon_world::worldgen::structure::BoundingBox;

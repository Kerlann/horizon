//! Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The handles' reads of the `keys` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[horizon_mod_api_macros::host_area]
pub trait KeysBackend {
    /// Side: both.
    /// The canonical block id retained by the load-owned registration slot. An invalid slot
    /// is a boundary error, including when a wasm guest supplies it.
    #[access(Metadata)]
    fn block_key_id(&self, key: BlockKey) -> String;
    /// Side: both.
    /// The canonical item id retained by the load-owned registration slot.
    #[access(Metadata)]
    fn item_key_id(&self, key: ItemKey) -> String;
    /// Side: both.
    /// The canonical effect id retained by the load-owned registration slot.
    #[access(Metadata)]
    fn effect_key_id(&self, key: EffectKey) -> String;
    /// Side: both.
    /// The canonical fluid id of a key from the completed registry, `minecraft:water`. An
    /// invalid slot is a boundary error, including when a wasm guest supplies it.
    #[access(Metadata)]
    fn fluid_key_id(&self, key: FluidKey) -> String;
    /// Side: both.
    /// The key of the block named `id`, for a name that only arrives at run time (data, config,
    /// a command, a network message). `id` is the full registry name, `minecraft:stone` or
    /// `mymod:machine`. None for a name the registry does not hold, and before the registry is
    /// complete (during registration, use `Registrar::block_key`). Does not allocate.
    #[access(Metadata)]
    fn block_key(&self, id: &str) -> Option<BlockKey>;
    /// Side: both.
    /// The key of the effect named `id`: the full registry name, `minecraft:speed` or
    /// `mymod:rage`. None for a name the registry does not hold, and before the registry is
    /// complete (during registration, use `Registrar::effect_key`). Does not allocate.
    #[access(Metadata)]
    fn effect_key(&self, id: &str) -> Option<EffectKey>;
    /// Side: both.
    /// The canonical block entity type id retained by the load-owned slot.
    #[access(WorldOnly)]
    fn block_entity_type_key_id(&self, key: BlockEntityTypeKey) -> String;
}

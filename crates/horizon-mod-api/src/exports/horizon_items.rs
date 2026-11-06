#[horizon_mod_api_macros::wasm(via = horizon_world::nbt::Tag)]
pub use horizon_items::item_data::StackData;
#[horizon_mod_api_macros::wasm(via = horizon_world::nbt::Tag)]
pub use horizon_items::items::ItemStack;
pub use horizon_items::items::TagRegistry;

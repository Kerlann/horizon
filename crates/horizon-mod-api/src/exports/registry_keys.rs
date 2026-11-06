mod registry_keys;
pub use registry_keys::UnknownFluid;
mod __private {
    pub use crate::registry_keys::resolve_block_entity_types;
    // hidden: load-owned key resolution, used by the game at the registry boundaries
    pub use crate::registry_keys::{
        ResolvedBlock, block as resolved_block, effect as resolved_effect, resolve_blocks,
        resolve_effects,
    };
    pub use crate::registry_keys::{
        block_entity_type_key, block_entity_type_name, block_id as block_key_id,
        effect_id as effect_key_id, item_id as item_key_id,
    };
    pub use crate::registry_keys::{
        effect_key, fluid_id, fluid_key, fluid_key_utf16, resolve_fluids, world_fluid_key,
    };
}

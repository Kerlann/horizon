mod part_table;
#[cfg(any(test, feature = "testing"))]
pub(crate) use part_table::install_parts;
#[cfg(test)]
pub(crate) use part_table::mark_game_thread;
pub use part_table::{
    Attached, BlockCapabilityProvider, BlockEntityProviders, BlockProvider, BlockProviderOf,
    CapabilityProviders, EntityCapabilityProvider, EntityProvider, EntityProviderOf,
    EntityProviders, ItemCapabilityProvider, ItemProvider, ItemProviderOf, OwnProvider, Parts,
    ProvidedBy, Registrar, block_entity_providers, entity_providers, has_entity_providers,
};
pub(crate) use part_table::{InstalledProviders, answered, ask_providers, has_part_hook, parts};
mod __private {
    pub use crate::part_table::{
        HookParts, ItemKeyError, KnownIds, PartsRef, ResolvePartsError, block_entity_types,
        block_listens, entity_parts, game_rule_bool_slot, game_rule_registry, has_entity_parts,
        has_part_hook, has_scoped_hook, install_parts, key_mappings, mark_game_thread, parts,
        resolve_parts,
    };
    #[cfg(any(test, feature = "testing"))]
    pub use crate::{part_table::tests as part_tests, phase::with_test_packages};
}

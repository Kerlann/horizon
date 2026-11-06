//! The game's side of the mod API: the installed tables, the reload machinery, the structs the
//! game fills for its hooks (`built`) and what it reads of handles and the API's own types
//! (`game`). None of it is part of the API a mod is written against.
use crate::*;

/// Switch an installed mod's dispatches on or off, returning whether it was found.
pub fn set_mod_enabled(id: &str, on: bool) -> bool {
    crate::set_mod_enabled(id, on)
}

/// Resolve a listener's owner once at load, for dispatch through [`mod_index_on`].
pub fn mod_index(id: &str) -> Option<usize> {
    crate::mods().iter().position(|entry| entry.id == id)
}

/// Whether a resolved listener's owner is currently enabled.
pub fn mod_index_on(index: usize) -> bool {
    crate::mod_index_on(index)
}

/// The installed mod `id`'s hooks, for the game to hand its other faces (its `impl Points`) on.
pub fn hooks_of(id: &str) -> Option<&'static dyn Hooks> {
    crate::mods()
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| &*entry.hooks)
}

/// Marks the thread as inside mod `id`'s `call` until the guard drops, as a hook's dispatch does,
/// so a panic in it names both.
pub fn enter(id: &'static str, call: &'static str) -> crate::crash::NativeModGuard {
    crate::mark_mod(id, call)
}

/// Install the process-wide ordered list of mods.
pub fn install(mods: Mods) {
    crate::install(mods);
}

/// Whether an installed mod subscribes to `hook`.
pub fn listened(hook: HookId) -> bool {
    crate::listened(hook)
}

pub use crate::generated::calls::*;
pub use crate::part_table::{
    HookParts, KnownIds, PartsRef, block_entity_types, block_listens, has_part_hook,
    has_scoped_hook, install_parts, mark_game_thread, parts, resolve_parts,
};
pub use crate::phase::{
    LoadAccess, ModPhase, PhaseError, PhaseOwner, check_access, current_phase, install_packages,
};
pub use crate::registry_keys::resolve_block_entity_types;
/// The game's side of wasm mods, for `horizon-mod-wasm`.
#[cfg(all(feature = "wasm-host", not(target_arch = "wasm32")))]
pub mod wasm_host {
    pub use crate::wasm_host::{
        Budgets, CallHost, CallRefused, EPOCH, Exports, Fuel, Instantiated, LoadError, Member,
        State, instantiate, live_groups,
    };
}
/// The registered structure types, for the executable to hand to the engine's
/// `StructureRegistry` before any world opens.
pub use crate::structure_type::installed_structure_types;
/// The mod's side of the exports other crates add to its world (the points crate's listeners).
#[cfg(target_arch = "wasm32")]
pub use crate::wasm::exported as wasm_guest;
#[cfg(any(test, feature = "testing"))]
pub use crate::{part_table::tests as part_tests, phase::with_test_packages};

/// The trait mods installed, in dispatch order: the game's own list, not part of the mod surface.
pub fn mods() -> &'static [ModEntry] {
    crate::mods()
}

/// The startup package inventory at an owning engine load boundary.
pub fn loaded_mods() -> LoadedMods {
    LoadedMods::new()
}

/// The data-pack and resource reloads the game runs.
pub mod reload {
    pub use crate::reload::{
        PreparedStep, RegisteredListener, ReloadFiles, ReloadStageError, apply_listeners,
        prepare_listeners,
    };
    pub use crate::server_reload::{OrderedServerReload, RetainedEntry};
}

/// The structs the game fills for its hooks, each turned into the API's own with `From`.
pub mod built {
    pub use crate::built::{
        ChannelRef, NativeActiveEffect, NativeBlockClick, NativeExplosionStart, NativeItemClick,
        NativeItemFished, NativeLifecycleContext, NativeLootDrops, NativePlayerAbilities,
        NativePlayerExperience, NativePlayerFood, NativePortalShape, NativeScreenView,
        NativeServerPlayer, SoundPlay, ToolUse,
    };
    pub use crate::config_files::built::{ConfigEntry, ConfigStat};
    pub use crate::game::built::NativeConfigFile;
    pub use crate::render::built::{BlockSpriteArrays, HandDrawn, WorldDrawn};
}

/// What the game reads of handles and calls on the API's own types.
pub mod game {
    use crate::*;
    use std::fmt::Debug;

    /// A type's fields the game reads, as `value.fields().name`.
    pub trait Fields {
        type Fields<'a>
        where
            Self: 'a;
        fn fields(&self) -> Self::Fields<'_>;
    }

    /// `Ingredient::test` with the loop context the game's own matching holds.
    pub trait IngredientTest {
        fn test_in(
            &self,
            context: &horizon_world::native_load_context::LoopContext,
            stack: &ItemStack,
        ) -> bool;
    }
    impl IngredientTest for Ingredient {
        fn test_in(
            &self,
            context: &horizon_world::native_load_context::LoopContext,
            stack: &ItemStack,
        ) -> bool {
            Ingredient::test_in(self, context, stack)
        }
    }

    /// A block entity handle the game names.
    pub fn block_entity_ref(level: LevelRef, position: [i32; 3]) -> BlockEntityRef {
        BlockEntityRef { level, position }
    }
    /// A boss bar handle the game names.
    pub fn boss_bar_ref(epoch: NativeWorldEpoch, uuid: u128) -> BossBarRef {
        BossBarRef { epoch, uuid }
    }
    /// A chunk handle the game names.
    pub fn chunk_ref(level: LevelRef, pos: [i32; 2]) -> ChunkRef {
        ChunkRef { level, pos }
    }
    /// An open menu's handle the game names.
    pub fn menu_ref(player: PlayerRef, window: i32, generation: u64) -> MenuRef {
        MenuRef {
            player,
            window,
            generation,
        }
    }
    /// A player handle the game names.
    pub fn player_ref(entity: EntityRef, uuid: u128) -> PlayerRef {
        PlayerRef { entity, uuid }
    }
    /// A stack's modifiers for `item_attribute_modifier`, as its component
    /// gives them.
    pub fn item_attribute_modifiers(entries: Vec<ItemAttributeModifier>) -> ItemAttributeModifiers {
        ItemAttributeModifiers {
            entries,
            changed: false,
        }
    }
    /// The modifiers the hook left, or `None` when no mod changed them.
    pub fn changed_item_attribute_modifiers(
        modifiers: ItemAttributeModifiers,
    ) -> Option<Vec<ItemAttributeModifier>> {
        modifiers.changed.then_some(modifiers.entries)
    }

    /// A stack's enchantments for `get_enchantment_level`, as its
    /// component gives them.
    pub fn item_enchantments(entries: Vec<(Identifier, u32)>) -> ItemEnchantments {
        ItemEnchantments {
            entries,
            changed: false,
        }
    }
    /// The enchantments the hook left, and whether a mod changed them.
    pub fn item_enchantments_entries(
        enchantments: ItemEnchantments,
    ) -> (Vec<(Identifier, u32)>, bool) {
        (enchantments.entries, enchantments.changed)
    }

    pub struct BlockEntityRefFields {
        pub level: LevelRef,
        pub position: [i32; 3],
    }
    impl Fields for BlockEntityRef {
        type Fields<'a> = BlockEntityRefFields;
        fn fields(&self) -> BlockEntityRefFields {
            BlockEntityRefFields {
                level: self.level,
                position: self.position,
            }
        }
    }

    pub struct BossBarRefFields {
        pub epoch: NativeWorldEpoch,
        pub uuid: u128,
    }
    impl Fields for BossBarRef {
        type Fields<'a> = BossBarRefFields;
        fn fields(&self) -> BossBarRefFields {
            BossBarRefFields {
                epoch: self.epoch,
                uuid: self.uuid,
            }
        }
    }

    pub struct MenuRefFields {
        pub player: PlayerRef,
        pub window: i32,
        pub generation: u64,
    }
    impl Fields for MenuRef {
        type Fields<'a> = MenuRefFields;
        fn fields(&self) -> MenuRefFields {
            MenuRefFields {
                player: self.player,
                window: self.window,
                generation: self.generation,
            }
        }
    }

    pub struct PlayerRefFields {
        pub entity: EntityRef,
        pub uuid: u128,
    }
    impl Fields for PlayerRef {
        type Fields<'a> = PlayerRefFields;
        fn fields(&self) -> PlayerRefFields {
            PlayerRefFields {
                entity: self.entity,
                uuid: self.uuid,
            }
        }
    }

    pub struct ModEntryFields<'a> {
        pub id: &'static str,
        pub hooks: &'a dyn Hooks,
    }
    impl Fields for ModEntry {
        type Fields<'a> = ModEntryFields<'a>;
        fn fields(&self) -> ModEntryFields<'_> {
            ModEntryFields {
                id: self.id,
                hooks: &*self.hooks,
            }
        }
    }

    pub trait PartsLookup {
        fn for_hook(&self, hook: HookId) -> Option<&crate::part_table::HookParts>;
    }
    impl PartsLookup for Parts {
        fn for_hook(&self, hook: HookId) -> Option<&crate::part_table::HookParts> {
            Parts::for_hook(self, hook)
        }
    }

    pub trait CapabilityChecked {
        fn checked(&self, answer: u32, asked: u32, at: &dyn Debug) -> Option<u32>;
    }
    impl<T: ?Sized> CapabilityChecked for Capability<T> {
        fn checked(&self, answer: u32, asked: u32, at: &dyn Debug) -> Option<u32> {
            Capability::checked(self, answer, asked, at)
        }
    }

    pub trait FileToIdConverterPaths {
        fn directory(&self) -> &str;
        fn file_to_id(&self, file: &str) -> Option<Identifier>;
    }
    impl FileToIdConverterPaths for FileToIdConverter {
        fn directory(&self) -> &str {
            FileToIdConverter::directory(self)
        }
        fn file_to_id(&self, file: &str) -> Option<Identifier> {
            FileToIdConverter::file_to_id(self, file)
        }
    }

    pub trait RegistrationContextNew {
        fn new() -> Self;
    }
    impl RegistrationContextNew for RegistrationContext {
        fn new() -> Self {
            RegistrationContext::new()
        }
    }

    pub trait ReloadContextNew {
        fn new(files: crate::reload::ReloadFiles) -> Self;
    }
    impl ReloadContextNew for ServerReloadContext {
        fn new(files: crate::reload::ReloadFiles) -> Self {
            ServerReloadContext::new(files)
        }
    }
    impl ReloadContextNew for ClientReloadContext {
        fn new(files: crate::reload::ReloadFiles) -> Self {
            ClientReloadContext::new(files)
        }
    }

    pub trait ServerReloadListenersOrder {
        fn new() -> Self;
        fn finish(self) -> Result<crate::server_reload::OrderedServerReload, ReloadOrderError>;
    }
    impl ServerReloadListenersOrder for crate::server_reload::ServerListenerRegistry {
        fn new() -> Self {
            crate::server_reload::ServerListenerRegistry::new()
        }
        fn finish(self) -> Result<crate::server_reload::OrderedServerReload, ReloadOrderError> {
            crate::server_reload::ServerListenerRegistry::finish(self)
        }
    }

    pub trait ClientReloadListenersOrder {
        fn new() -> Self;
        #[allow(clippy::type_complexity)]
        fn finish(
            self,
        ) -> Result<
            Vec<crate::reload::RegisteredListener<dyn ClientReloadListener>>,
            ReloadOrderError,
        >;
    }
    impl ClientReloadListenersOrder for crate::client_reload::ClientListenerRegistry {
        fn new() -> Self {
            crate::client_reload::ClientListenerRegistry::new()
        }
        fn finish(
            self,
        ) -> Result<
            Vec<crate::reload::RegisteredListener<dyn ClientReloadListener>>,
            ReloadOrderError,
        > {
            crate::client_reload::ClientListenerRegistry::finish(self)
        }
    }

    pub trait ServerReloadResourcesInstall {
        fn install(&mut self, retained: Vec<crate::server_reload::RetainedEntry>);
    }
    impl ServerReloadResourcesInstall for crate::server_reload::RetainedListeners {
        fn install(&mut self, retained: Vec<crate::server_reload::RetainedEntry>) {
            crate::server_reload::RetainedListeners::install(self, retained)
        }
    }

    pub trait BackendRefOf<'a> {
        fn of(inner: &'a dyn Backend) -> Self;
    }
    impl<'a> BackendRefOf<'a> for BackendRef<'a> {
        fn of(inner: &'a dyn Backend) -> Self {
            BackendRef::of(inner)
        }
    }

    pub trait BackendMutRefOf<'a> {
        fn of(inner: &'a mut dyn BackendMut) -> Self;
    }
    impl<'a> BackendMutRefOf<'a> for BackendMutRef<'a> {
        fn of(inner: &'a mut dyn BackendMut) -> Self {
            BackendMutRef::of(inner)
        }
    }

    pub trait ClientBackendMutRefOf<'a> {
        fn of(inner: &'a mut dyn ClientBackendMut) -> Self;
    }
    impl<'a> ClientBackendMutRefOf<'a> for ClientBackendMutRef<'a> {
        fn of(inner: &'a mut dyn ClientBackendMut) -> Self {
            ClientBackendMutRef::of(inner)
        }
    }

    pub use self::modblockentityviewnew::ModBlockEntityViewNew;
    mod modblockentityviewnew {
        #[allow(unused_imports)]
        use crate::*;
        #[allow(unused_imports)]
        use serde_json::{Map, Value};
        #[allow(unused_imports)]
        use std::sync::{Arc, OnceLock};
        #[allow(unused_imports)]
        use std::time::Duration;
        pub trait ModBlockEntityViewNew<'a>: Sized {
            fn new(entity: &'a mut ModBlockEntity) -> Self;
        }
        impl<'a> ModBlockEntityViewNew<'a> for ModBlockEntityView<'a> {
            fn new(entity: &'a mut ModBlockEntity) -> Self {
                ModBlockEntityView::new(entity)
            }
        }
    }

    pub use self::reloadfilesnew::ReloadFilesNew;
    mod reloadfilesnew {
        #[allow(unused_imports)]
        use crate::reload::*;
        #[allow(unused_imports)]
        use crate::*;
        #[allow(unused_imports)]
        use crate::{
            FileToIdConverter, Identifier,
            reload_order::{ReloadOrder, ReloadOrderError},
        };
        #[allow(unused_imports)]
        use std::{collections::BTreeMap, sync::Arc};
        pub trait ReloadFilesNew: Sized {
            fn new(files: BTreeMap<Identifier, Vec<u8>>) -> Self;
        }
        impl ReloadFilesNew for ReloadFiles {
            fn new(files: BTreeMap<Identifier, Vec<u8>>) -> Self {
                ReloadFiles::new(files)
            }
        }
    }
}

// hidden: load-owned key resolution, used by the game at the registry boundaries
pub use crate::registry_keys::{
    ResolvedBlock, block as resolved_block, effect as resolved_effect, resolve_blocks,
    resolve_effects,
};
pub use crate::registry_keys::{
    block_entity_type_key, block_entity_type_name, block_id as block_key_id,
    effect_id as effect_key_id,
};
pub use crate::registry_keys::{effect_key, fluid_id, fluid_key, resolve_fluids};
pub use crate::unresolved_fluid::{
    identity_name as fluid_identity_name, intern as intern_unresolved_fluid,
    lookup as lookup_unresolved_fluid, name as unresolved_fluid_name,
};

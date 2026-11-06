//! Read-only block, tag and fluid queries for native mods.
use super::{Dim, Game};
use horizon_client::{
    items::{ItemStack, Items},
    world::World,
};
use horizon_world::{
    chunk::BlockState,
    fluid::{FluidKind, FluidState},
    level::Behaviours,
    registry::{BlockRegistry, BlockStateId},
};
use mod_api::{LevelRef, NativeLifecycleContext, TagRegistry};

/// Why a server block cannot be read.
#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum NativeBlockReadError {
    /// The context is stale or the call is outside its server tick hook.
    InactiveContext,
    /// The dimension is unknown or is not currently open on the server.
    UnavailableDimension,
    OutOfBounds,
    Unloaded,
    InvalidState,
}

/// Whether a block registry name belongs to a merged block tag. Namespaced names need no allocation.
pub(super) fn block_in_tag(items: &Items, block: &str, tag: &str) -> bool {
    items.block_in_tag(
        &horizon_client::command::args::normalized(tag),
        &horizon_client::command::args::normalized(block),
    )
}

/// `ItemStack.is(TagKey)` over the merged item tags. An empty stack tests `minecraft:air`.
pub(super) fn stack_in_tag(items: &Items, stack: &ItemStack, tag: &str) -> bool {
    let item = if stack.count == 0 {
        "minecraft:air"
    } else {
        &stack.item
    };
    items.item_in_tag(&horizon_client::command::args::normalized(tag), item)
}

/// `BlockState.getFluidState`. None for an id no state has.
pub(super) fn state_fluid(
    behaviours: &Behaviours,
    registry: &BlockRegistry,
    state: BlockStateId,
) -> Option<FluidState> {
    registry.state(state).map(|_| behaviours.fluid(state))
}

/// `FluidState.is(TagKey)` over the merged fluid tags. The empty fluid is in none.
pub(super) fn fluid_in_tag(items: &Items, fluid: &str, tag: &str) -> bool {
    items.fluid_in_tag(
        &horizon_client::command::args::normalized(tag),
        &horizon_client::command::args::normalized(fluid),
    )
}

/// The fluid a registry id names, with or without `minecraft:`.
fn fluid_kind(fluid: &str) -> Option<FluidKind> {
    FluidKind::from_name(&horizon_client::command::args::normalized(fluid))
}

impl Game<'_> {
    /// The merged tag tables, once the world has built them. None on a server's client, whose
    /// tags are not the server's, and before the world has loaded its tags (the world lock is
    /// installed after they are built).
    pub(super) fn native_tag_items(&self) -> Option<&Items> {
        (!self.options.execution_role.is_client() && self.session_lock.is_some())
            .then(|| self.items())
    }

    /// Whether the block registry name is in the world's merged block tag. Unknown names or tags
    /// return false; unavailable authoritative tags return None.
    pub(super) fn native_block_in_tag(&self, block: &str, tag: &str) -> Option<bool> {
        Some(block_in_tag(self.native_tag_items()?, block, tag))
    }

    /// The members of a tag in vanilla's order (`HolderSet.Named` iteration: file order, nested
    /// tags in place, first occurrence kept), over the merged tags. Empty for an unknown tag.
    /// None as for `native_block_in_tag`.
    pub(super) fn native_tag_members(
        &self,
        registry: TagRegistry,
        tag: &str,
    ) -> Option<Vec<String>> {
        let members = self
            .native_tag_items()?
            .tag_members(registry, &horizon_client::command::args::normalized(tag));
        Some(members.map(<[String]>::to_vec).unwrap_or_default())
    }

    /// The outline of `state` as `BlockState.getShape` answers it with an empty getter at the
    /// origin (`EmptyBlockGetter.INSTANCE`, `BlockPos.ZERO`): its boxes, each `[min x, y, z,
    /// max x, y, z]` in the block's own space, with no random offset and none of a world's
    /// context (a state that reads its neighbours or block entity answers what its state alone
    /// gives). Empty for a state with no outline, such as air. None for an id no state has.
    pub(super) fn native_state_outline(&self, state: BlockStateId) -> Option<Vec<[f64; 6]>> {
        self.pack.assets.registry.state(state)?;
        Some(self.res.facts().get(state).outline.to_vec())
    }

    /// The fluid state `state` holds (`BlockState.getFluidState`): water for a waterlogged state,
    /// the empty fluid for most. None for an id no state has.
    pub(super) fn native_state_fluid_state(&self, state: BlockStateId) -> Option<FluidState> {
        state_fluid(self.res.behaviours(), self.pack.assets.registry, state)
    }

    /// Whether `fluid` is in the fluid tag `tag` (`FluidState.is(TagKey)`, `minecraft:water` or
    /// `water`), over the merged fluid tags. An unknown tag is false. None as for
    /// `native_block_in_tag`.
    pub(super) fn native_fluid_in_tag(&self, fluid: &str, tag: &str) -> Option<bool> {
        Some(fluid_in_tag(self.native_tag_items()?, fluid, tag))
    }

    /// The fluid state at `pos` of the level `level` names (`BlockGetter.getFluidState`). None
    /// for a stale handle, a chunk that isn't loaded or a `y` outside the world: a read never
    /// loads a chunk.
    pub(super) fn native_level_fluid_state(
        &self,
        level: LevelRef,
        pos: [i32; 3],
    ) -> Option<FluidState> {
        let state = self.native_level_state(level, pos)?;
        self.native_state_fluid_state(state)
    }

    /// Whether `fluid` and `other` are the same fluid (`Fluid.isSame`): a source and its
    /// flowing form are, so `minecraft:water` and `minecraft:flowing_water` are the same, water
    /// and lava not. Names are fluid ids, with or without `minecraft:`; false when either
    /// names no fluid.
    pub(super) fn native_fluid_is_same(&self, fluid: &str, other: &str) -> bool {
        match (fluid_kind(fluid), fluid_kind(other)) {
            (Some(fluid), Some(other)) => fluid.same_family(other),
            _ => false,
        }
    }

    /// The bucket item that picks `fluid` up (`Fluid.getBucket`), `minecraft:water_bucket` for
    /// water and flowing water. None for `minecraft:empty`, whose bucket is air, and for a name
    /// no fluid has.
    pub(super) fn native_fluid_bucket(&self, fluid: &str) -> Option<&'static str> {
        horizon_world::level::fluid_bucket(fluid_kind(fluid)?)
    }

    /// The sound event a bucket plays as it picks `fluid` up (`Fluid.getPickupSound`),
    /// `minecraft:item.bucket.fill_lava` for lava and flowing lava. None for
    /// `minecraft:empty`, which has none, and for a name no fluid has.
    pub(super) fn native_fluid_pickup_sound(&self, fluid: &str) -> Option<&'static str> {
        horizon_world::level::fluid_pickup_sound(fluid_kind(fluid)?)
    }

    /// Read a loaded block in a server tick hook, without loading a chunk.
    /// The returned name and properties are owned and remain valid after the hook.
    #[allow(dead_code)]
    pub(super) fn native_server_block(
        &self,
        context: &NativeLifecycleContext,
        dimension: &str,
        pos: [i32; 3],
    ) -> Result<BlockState, NativeBlockReadError> {
        if !self.native_lifecycle.permits_read(context) {
            return Err(NativeBlockReadError::InactiveContext);
        }
        let dim = match dimension {
            "minecraft:overworld" => Dim::Overworld,
            "minecraft:the_nether" => Dim::Nether,
            "minecraft:the_end" => Dim::End,
            _ => return Err(NativeBlockReadError::UnavailableDimension),
        };
        let world = if dim == self.dim {
            Some(&self.world)
        } else {
            self.levels.parked_world(dim)
        }
        .ok_or(NativeBlockReadError::UnavailableDimension)?;
        read_block(world, self.pack.assets.registry, pos)
    }
}

/// Resolve an existing world state, preserving unloaded and invalid states.
fn read_block(
    world: &World,
    registry: &BlockRegistry,
    [x, y, z]: [i32; 3],
) -> Result<BlockState, NativeBlockReadError> {
    if !(-30_000_000..30_000_000).contains(&x)
        || !(-30_000_000..30_000_000).contains(&z)
        || !world.shape.contains_y(y)
    {
        return Err(NativeBlockReadError::OutOfBounds);
    }
    if !world.loaded(x, z) {
        return Err(NativeBlockReadError::Unloaded);
    }
    registry
        .state(world.block(x, y, z))
        .cloned()
        .ok_or(NativeBlockReadError::InvalidState)
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::*;
    use horizon_client::chunk::Chunk;
    use mod_api::LevelHost;
    use std::sync::Arc;

    #[test]
    fn reads_only_loaded_in_bounds_states() {
        let facts = br#"{"defaults":{"minecraft:air":0,"test:pillar":1},"states":[
            ["air",true,false,"","minecraft:air"],
            ["pillar",false,false,"",{"id":"test:pillar","properties":{"axis":"x"}}]
        ]}"#;
        let registry = BlockRegistry::from_facts_json(&facts[..]).unwrap();
        let mut world = World::new(BlockStateId(0), 0);
        world.chunks.insert((-1, 0), Arc::new(Chunk::empty(-1, 0)));
        world.set(-1, 64, 1, BlockStateId(1)).unwrap();
        // The Nether's build range starts at y 0 (its level's shape).
        let mut nether = World::new(BlockStateId(0), 0);
        nether.shape = horizon_client::chunk::LevelShape::NETHER;
        nether.chunks.insert((-1, 0), Arc::new(Chunk::empty(-1, 0)));
        let block = read_block(&world, &registry, [-1, 64, 1]).unwrap();
        assert_eq!(block.name, "test:pillar");
        assert_eq!(block.properties["axis"], "x");
        assert_eq!(
            read_block(&world, &registry, [-1, 65, 1]).unwrap().name,
            "minecraft:air"
        );
        assert_eq!(
            read_block(&world, &registry, [0, 64, 1]),
            Err(NativeBlockReadError::Unloaded)
        );
        assert_eq!(
            read_block(&nether, &registry, [-1, -1, 1]),
            Err(NativeBlockReadError::OutOfBounds)
        );
        assert_eq!(
            read_block(&world, &registry, [30_000_000, 64, 1]),
            Err(NativeBlockReadError::OutOfBounds)
        );
        world.set(-1, 64, 1, BlockStateId(99)).unwrap();
        assert_eq!(
            read_block(&world, &registry, [-1, 64, 1]),
            Err(NativeBlockReadError::InvalidState)
        );
        assert_eq!(world.chunks.len(), 1, "reads must not load chunks");
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn recipes_of_type_answers_vanilla_types_from_the_live_table() {
        with_game(|game| {
            let smelting = game.native_recipes_of_type("minecraft:smelting");
            let iron = smelting
                .iter()
                .find(|recipe| recipe.id == "minecraft:iron_ingot_from_smelting_raw_iron")
                .expect("vanilla's smelting recipe of raw iron");
            assert_eq!(iron.json["cookingtime"], 200);
            assert_eq!(iron.cooking_time, Some(200));
            assert_eq!(iron.experience, Some(0.7));
            assert_eq!(iron.ingredients.len(), 1);
            assert!(iron.ingredients[0].items.contains("minecraft:raw_iron"));
            assert_eq!(iron.results[0].item.as_ref(), "minecraft:iron_ingot");
            let shaped = game.native_recipes_of_type("minecraft:crafting_shaped");
            let bucket = shaped
                .iter()
                .find(|recipe| recipe.id == "minecraft:bucket")
                .expect("vanilla's bucket recipe");
            assert_eq!(bucket.ingredients.len(), 3, "its pattern's three cells");
            assert_eq!(bucket.results[0].item.as_ref(), "minecraft:bucket");
            assert_eq!(bucket.cooking_time, None);
            assert!(
                !game
                    .native_recipes_of_type("minecraft:campfire_cooking")
                    .is_empty()
            );
            assert!(game.native_recipes_of_type("nope:missing").is_empty());
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn tag_and_fluid_reads_follow_vanilla_and_load_nothing() {
        with_game(|game| {
            let dir =
                std::env::temp_dir().join(format!("horizon-tag-reads-{}", std::process::id()));
            game.session_lock = Some(horizon_client::save::SessionLock::acquire(&dir).unwrap());
            let registry = game.pack.assets.registry;
            let stone = registry.default_state("minecraft:stone").unwrap();
            let water = registry.default_state("minecraft:water").unwrap();
            let slab = game
                .native_block_state_id("minecraft:oak_slab[waterlogged=true]")
                .unwrap();
            let dry_slab = game.native_block_state_id("minecraft:oak_slab").unwrap();
            let lava = registry.default_state("minecraft:lava").unwrap();

            // Members come in the tag file's order, in each registry, with or without the
            // namespace; an unknown tag lists nothing.
            let oak = [
                "oak_log",
                "oak_wood",
                "stripped_oak_log",
                "stripped_oak_wood",
            ]
            .map(|name| format!("minecraft:{name}"));
            for registry in [TagRegistry::Block, TagRegistry::Item] {
                assert_eq!(
                    game.native_tag_members(registry, "oak_logs").as_deref(),
                    Some(&oak[..])
                );
            }
            {
                let host: &dyn mod_api::Host = game;
                assert_eq!(
                    host.tag_members(TagRegistry::Item, "oak_logs").as_deref(),
                    Some(&oak[..])
                );
                assert_eq!(
                    host.item_stack_in_tag(&ItemStack::new("minecraft:oak_log", 1), "oak_logs"),
                    Some(true)
                );
            }
            assert_eq!(
                game.native_tag_members(TagRegistry::Fluid, "minecraft:water")
                    .unwrap(),
                ["minecraft:water", "minecraft:flowing_water"]
            );
            assert_eq!(
                game.native_tag_members(TagRegistry::Item, "nope:missing"),
                Some(Vec::new())
            );

            // A full block is one unit box, a bottom slab half of it, air none, and an id no
            // state has is unknown.
            assert_eq!(
                game.native_state_outline(stone),
                Some(vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]])
            );
            assert_eq!(
                game.native_state_outline(dry_slab),
                Some(vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]])
            );
            let air = registry.default_state("minecraft:air").unwrap();
            assert_eq!(game.native_state_outline(air), Some(Vec::new()));
            assert_eq!(game.native_state_outline(BlockStateId(u32::MAX)), None);

            assert_eq!(
                game.native_block_in_tag("minecraft:stone", "minecraft:base_stone_overworld"),
                Some(true)
            );
            assert_eq!(
                game.native_block_in_tag("stone", "base_stone_overworld"),
                Some(true)
            );
            assert_eq!(
                game.native_block_in_tag("minecraft:stone", "minecraft:logs"),
                Some(false)
            );
            assert_eq!(
                game.native_block_in_tag("minecraft:stone", "nope:missing"),
                Some(false)
            );
            assert_eq!(
                game.native_block_in_tag("missing:unknown", "minecraft:logs"),
                Some(false)
            );

            assert_eq!(
                game.native_state_fluid_state(water),
                Some(FluidState::WATER)
            );
            assert_eq!(
                game.native_fluid_in_tag("minecraft:water", "minecraft:water"),
                Some(true)
            );
            assert_eq!(game.native_fluid_in_tag("water", "water"), Some(true));
            assert_eq!(
                game.native_fluid_in_tag("minecraft:water", "minecraft:lava"),
                Some(false)
            );
            assert_eq!(
                game.native_state_fluid_state(lava).unwrap().kind().name(),
                "minecraft:lava"
            );
            assert_eq!(
                game.native_fluid_in_tag("minecraft:lava", "minecraft:lava"),
                Some(true)
            );
            assert_eq!(
                game.native_fluid_in_tag("minecraft:lava", "minecraft:water"),
                Some(false)
            );
            assert_eq!(
                game.native_fluid_in_tag("minecraft:empty", "minecraft:water"),
                Some(false)
            );
            assert_eq!(game.native_state_fluid_state(slab), Some(FluidState::WATER));
            assert_eq!(
                game.native_state_fluid_state(dry_slab),
                Some(FluidState::EMPTY)
            );
            assert_eq!(
                game.native_state_fluid_state(stone),
                Some(FluidState::EMPTY)
            );
            assert_eq!(game.native_state_fluid_state(BlockStateId(u32::MAX)), None);

            // The world holds a chunk with water in it; a neighbour is not loaded.
            let mut chunk = Chunk::empty(0, 0);
            chunk.set(1, 64, 1, slab.0);
            game.world.chunks.insert((0, 0), Arc::new(chunk));
            let level = game.native_level(game.dim.id()).unwrap();
            let chunks = game.world.chunks.len();
            assert_eq!(
                game.native_level_fluid_state(level, [1, 64, 1]),
                Some(FluidState::WATER)
            );
            assert_eq!(
                game.native_level_fluid_state(level, [2, 64, 1]),
                Some(FluidState::EMPTY)
            );
            assert_eq!(game.native_level_fluid_state(level, [16, 64, 1]), None);
            assert_eq!(game.native_level_fluid_state(level, [1, 4000, 1]), None);
            game.with_level(|_, host, _| {
                // Not an authoritative level: tags are not the server's, so they read None.
                assert_eq!(
                    host.block_in_tag("minecraft:stone", "minecraft:base_stone_overworld"),
                    None
                );
                assert_eq!(
                    host.fluid_in_tag("minecraft:water", "minecraft:water"),
                    None
                );
                assert_eq!(host.fluid_state([1, 64, 1]), Some(FluidState::WATER));
            });
            game.options.dedicated = Some(Vec::new());
            let bounds = game.native_level_height(level).unwrap();
            let parsed = game.native_block_state_id("minecraft:oak_slab[waterlogged=true]");
            let default = game.native_block_state_default("minecraft:oak_slab");
            game.with_level(|_, host, ticks| {
                assert_eq!(
                    host.block_state_id("minecraft:oak_slab[waterlogged=true]"),
                    parsed
                );
                assert_eq!(host.block_state_default("minecraft:oak_slab"), default);
                assert_eq!(host.block_state_id("minecraft:oak_slab[bad=true]"), None);
                assert_eq!(host.block_state_default("missing:unknown"), None);
                let wrapped = super::super::mod_api_host::LevelTicks {
                    level: &*host,
                    ticks,
                };
                assert_eq!(
                    wrapped.block_state_id("minecraft:oak_slab[waterlogged=true]"),
                    parsed
                );
                assert_eq!(wrapped.block_state_default("minecraft:oak_slab"), default);
                assert_eq!(wrapped.block_state_id("minecraft:oak_slab[bad=true]"), None);
                assert_eq!(wrapped.block_state_default("missing:unknown"), None);
                assert_eq!(host.state_string(stone).as_deref(), Some("minecraft:stone"));
                assert!(
                    host.state_string(slab)
                        .unwrap()
                        .contains("waterlogged=true")
                );
                assert_eq!(host.state_string(BlockStateId(u32::MAX)), None);
                assert_eq!((host.min_y(), host.height()), bounds);
                assert_eq!(
                    host.block_in_tag("minecraft:stone", "minecraft:base_stone_overworld"),
                    Some(true)
                );
                assert_eq!(
                    host.block_in_tag("minecraft:stone", "minecraft:logs"),
                    Some(false)
                );
                assert_eq!(host.fluid_state([1, 64, 1]), Some(FluidState::WATER));
                assert_eq!(host.fluid_state([16, 64, 1]), None);
                assert_eq!(host.state_fluid_state(slab), Some(FluidState::WATER));
                assert_eq!(
                    host.fluid_in_tag("minecraft:water", "minecraft:water"),
                    Some(true)
                );
                assert_eq!(
                    host.fluid_in_tag("minecraft:empty", "minecraft:water"),
                    Some(false)
                );
            });
            {
                use mod_api::LevelHost;
                let hit =
                    super::super::native_hooks::HitLevel(horizon_client::health::DamageLevel::of(
                        &mut game.world,
                        game.pack.assets.registry,
                        Some(world_items!(game)),
                        Some(&game.pack.block_registries),
                        &game.damage_hooks,
                    ));
                assert_eq!(
                    hit.block_state_id("minecraft:oak_slab[waterlogged=true]"),
                    parsed
                );
                assert_eq!(hit.block_state_default("minecraft:oak_slab"), default);
                assert_eq!(hit.block_state_id("minecraft:oak_slab[bad=true]"), None);
                assert_eq!(hit.block_state_default("missing:unknown"), None);
                assert_eq!(hit.state_string(stone).as_deref(), Some("minecraft:stone"));
                assert!(hit.state_string(slab).unwrap().contains("waterlogged=true"));
                assert_eq!(hit.state_string(BlockStateId(u32::MAX)), None);
                assert_eq!((hit.min_y(), hit.height()), bounds);
                assert_eq!(
                    hit.block_in_tag("minecraft:stone", "minecraft:base_stone_overworld"),
                    Some(true)
                );
                assert_eq!(
                    hit.fluid_in_tag("minecraft:water", "minecraft:water"),
                    Some(true)
                );
                assert_eq!(
                    hit.block_in_tag("missing:unknown", "minecraft:logs"),
                    Some(false)
                );
            }
            {
                use mod_api::LevelHost;
                let hit =
                    super::super::native_hooks::HitLevel(horizon_client::health::DamageLevel::of(
                        &mut game.world,
                        game.pack.assets.registry,
                        None,
                        None,
                        &game.damage_hooks,
                    ));
                assert_eq!(hit.block_state_id("minecraft:stone"), None);
                assert_eq!(hit.block_state_default("minecraft:stone"), None);
                assert_eq!(hit.block_in_tag("minecraft:stone", "minecraft:logs"), None);
                assert_eq!(hit.fluid_in_tag("minecraft:water", "minecraft:water"), None);
            }
            assert_eq!(game.world.chunks.len(), chunks, "nothing was loaded");
            game.options.execution_role = super::super::execution_role::ExecutionRole::Client;
            let host: &dyn mod_api::Host = game;
            assert_eq!(host.tag_members(TagRegistry::Item, "oak_logs"), None);
            assert_eq!(
                host.item_stack_in_tag(&ItemStack::new("minecraft:oak_log", 1), "oak_logs"),
                None
            );
            game.session_lock = None;
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// Sources and their flowing forms are one fluid, the empty fluid has no bucket and no
    /// pickup sound, and a name no fluid has answers nothing, through a mod's `Host`.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn fluid_sameness_buckets_and_pickup_sounds_follow_vanilla() {
        with_game(|game| {
            let host: &dyn mod_api::Host = game;
            assert!(host.fluid_is_same("minecraft:water", "minecraft:flowing_water"));
            assert!(host.fluid_is_same("flowing_lava", "lava"));
            assert!(host.fluid_is_same("minecraft:empty", "minecraft:empty"));
            assert!(!host.fluid_is_same("minecraft:water", "minecraft:lava"));
            assert!(!host.fluid_is_same("minecraft:water", "minecraft:empty"));
            assert!(!host.fluid_is_same("nope:missing", "nope:missing"));

            assert_eq!(
                host.fluid_bucket("minecraft:flowing_water"),
                Some("minecraft:water_bucket")
            );
            assert_eq!(host.fluid_bucket("lava"), Some("minecraft:lava_bucket"));
            assert_eq!(host.fluid_bucket("minecraft:empty"), None);
            assert_eq!(host.fluid_bucket("nope:missing"), None);

            assert_eq!(
                host.fluid_pickup_sound("minecraft:water"),
                Some("minecraft:item.bucket.fill")
            );
            assert_eq!(
                host.fluid_pickup_sound("minecraft:flowing_lava"),
                Some("minecraft:item.bucket.fill_lava")
            );
            assert_eq!(host.fluid_pickup_sound("minecraft:empty"), None);
            assert_eq!(host.fluid_pickup_sound("nope:missing"), None);
        });
    }

    #[test]
    fn an_empty_stack_tests_air() {
        let mut tags = std::collections::HashMap::new();
        tags.insert(
            "minecraft:x".to_owned(),
            ["minecraft:air".to_owned()].into_iter().collect(),
        );
        let items = Items::for_tests(tags);
        let stack = |item: &str, count| ItemStack {
            item: item.into(),
            count,
            damage: 0,
            extra: None,
        };
        assert!(stack_in_tag(&items, &stack("minecraft:stone", 0), "x"));
        assert!(!stack_in_tag(&items, &stack("minecraft:stone", 1), "x"));
        assert!(!stack_in_tag(
            &items,
            &stack("minecraft:stone", 0),
            "minecraft:y"
        ));
    }
}

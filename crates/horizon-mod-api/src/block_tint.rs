//! Block tint sources, as a client mod registers them in `register_block_tint_sources` (NeoForge's
//! `RegisterColorHandlersEvent.BlockTintSources`, vanilla's `BlockColors.register`).
//!
//! A model face with `tintindex` i is multiplied by layer i of its block's list. Vanilla's own
//! sources ([`VanillaTint`], vanilla's `BlockTintSources`) are the game's, evaluated as it evaluates
//! its own blocks. A mod's source is its own [`BlockTintSource`], held by the game and asked on the
//! meshing workers: like a structure type on the worldgen workers, it gets no `Host` and keeps no
//! mutable state, since two workers can mesh neighbouring sections at once.
use super::{BlockStateId, Direction, FluidState, LightLayer};
use std::sync::Arc;

/// A block state as a tint source reads it, vanilla's `BlockState`: its id, its block and its
/// property values. The game keeps one per state while a mod's source is registered and hands out
/// shared copies, so reading one allocates nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
// exhaustive: the game builds it; mods only read it
pub struct BlockState {
    /// Its id in the game's block-state registry.
    pub id: BlockStateId,
    /// Its block (`BlockState.getBlock()`): `mymod:cable`, `minecraft:oak_leaves`.
    pub block: Arc<str>,
    /// Its property values (`BlockState.getValues()`), by name: `("color", "red")`.
    pub properties: Arc<[(Arc<str>, Arc<str>)]>,
}

impl BlockState {
    /// `BlockState.getValue`: the value of `property`, as its string; None where the block has no
    /// such property.
    pub fn value(&self, property: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(name, _)| &**name == property)
            .map(|(_, value)| &**value)
    }
}

/// The colour calculations block tints blend over the biomes around a block (vanilla's
/// `BiomeColors` resolvers, `ColorResolver`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
// exhaustive: the game blends every resolver, so a new one must be handled there
pub enum ColorResolver {
    /// `BiomeColors.GRASS_COLOR_RESOLVER`: `Biome.getGrassColor`.
    Grass,
    /// `BiomeColors.FOLIAGE_COLOR_RESOLVER`: `Biome.getFoliageColor`.
    Foliage,
    /// `BiomeColors.DRY_FOLIAGE_COLOR_RESOLVER`: `Biome.getDryFoliageColor`.
    DryFoliage,
    /// `BiomeColors.WATER_COLOR_RESOLVER`: `Biome.getWaterColor`.
    Water,
}

/// What a tint source reads of the world around the block it colours: vanilla's
/// `BlockAndTintGetter`, over the region a section is meshed from (`RenderSectionRegion`: the
/// section's chunk and its eight neighbours). A position outside that region reads as air, no
/// fluid and the light of an unloaded chunk. Block entities are not part of the region.
pub trait BlockAndTintGetter {
    /// `BlockGetter.getBlockState`: the state at `pos`.
    fn block_state(&self, pos: [i32; 3]) -> BlockState;
    /// `BlockGetter.getFluidState`: the fluid at `pos`, the empty fluid where there is none.
    /// None outside the region, as `LevelHost::fluid_state` reads an unloaded position.
    fn fluid_state(&self, pos: [i32; 3]) -> Option<FluidState>;
    /// `BlockAndLightGetter.getBrightness`: the stored light of one layer at `pos`, 0 to 15.
    fn brightness(&self, layer: LightLayer, pos: [i32; 3]) -> i32;
    /// `BlockAndLightGetter.getRawBrightness(pos, darkening)`: the brighter of the block light
    /// and the sky light less `sky_darken`.
    fn raw_brightness(&self, pos: [i32; 3], sky_darken: i32) -> i32;
    /// `BlockAndLightGetter.canSeeSky`: the sky light at `pos` is full.
    fn can_see_sky(&self, pos: [i32; 3]) -> bool;
    /// `cardinalLighting().byFace(face)`: how much a face toward `face` is shaded, 0 to 1 (the
    /// Nether's flat lighting in the Nether).
    fn shade(&self, face: Direction) -> f32;
    /// `getBlockTint(pos, resolver)`: the resolver's colour at `pos` blended over the biome blend
    /// radius, as opaque ARGB.
    fn block_tint(&self, pos: [i32; 3], resolver: ColorResolver) -> u32;
    /// `LevelHeightAccessor.getMinY`: the lowest block Y.
    fn min_y(&self) -> i32;
    /// `LevelHeightAccessor.getHeight`: the build height.
    fn height(&self) -> i32;
}

/// A mod's block tint source: vanilla's `BlockTintSource`. Colours are ARGB, and a face it colours
/// is multiplied by their RGB, as vanilla's block renderer does. Implemented once, held by the game, asked on the meshing workers, so it
/// keeps no mutable state.
pub trait BlockTintSource: Send + Sync + 'static {
    /// `color(state)`: the colour without a world, where the block is drawn away from a level (a
    /// block in a minecart, a mod's drawn block). Also the default of the two below.
    fn color(&self, state: &BlockState) -> u32;
    /// `colorInWorld(state, level, pos)`: the colour of the block at `pos`, as the chunk mesher
    /// draws it. The mesher asks once per block and tint index each time it meshes the section.
    fn color_in_world(
        &self,
        state: &BlockState,
        level: &dyn BlockAndTintGetter,
        pos: [i32; 3],
    ) -> u32 {
        let _ = (level, pos);
        self.color(state)
    }
    /// `colorAsTerrainParticle(state, level, pos)`: the colour of the bits the block breaks into
    /// and of falling dust.
    fn color_as_terrain_particle(
        &self,
        state: &BlockState,
        level: &dyn BlockAndTintGetter,
        pos: [i32; 3],
    ) -> u32 {
        self.color_in_world(state, level, pos)
    }
    /// `relevantProperties()`: the property names the colour depends on. Vanilla groups states
    /// for shared baked models by them (`ModelGroupCollector`); Horizon bakes every state, so the
    /// game does not read it.
    fn relevant_properties(&self) -> Vec<String> {
        Vec::new()
    }
}

/// One of vanilla's tint sources (`BlockTintSources`), as the game colours its own blocks with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
// exhaustive: vanilla's closed set, each evaluated by the game
pub enum VanillaTint {
    /// `constant(color)`: one ARGB colour everywhere.
    Constant(u32),
    /// `constant(colorInHand, colorInWorld)`: `hand` without a world, `world` in it (the lily pad).
    ConstantSplit {
        /// The colour without a world.
        hand: u32,
        /// The colour in the world and of its particles.
        world: u32,
    },
    /// `grass()`: the biome's grass colour.
    Grass,
    /// `grassBlock()`: the biome's grass colour, but none on its particles.
    GrassBlock,
    /// `doubleTallGrass()`: grass colour, the upper half's taken at the lower half.
    DoubleTallGrass,
    /// `sugarCane()`: grass colour in the world, none without one.
    SugarCane,
    /// `foliage()`: the biome's foliage colour.
    Foliage,
    /// `dryFoliage()`: the biome's dry foliage colour.
    DryFoliage,
    /// `water()`: the biome's water colour in the world, none without one.
    Water,
    /// `waterParticles()`: no colour in the world; the water colour on particles.
    WaterParticles,
    /// `redstone()`: the colour of the state's `power` (0 to 15).
    Redstone,
    /// `stem()`: the colour of the state's `age` (0 to 7).
    Stem,
}

/// One layer of a block's tint list: one of vanilla's sources or a mod's own.
#[derive(Clone)]
// exhaustive: mods build one; the game evaluates both kinds
pub enum TintLayer {
    /// A vanilla source, evaluated by the game as for its own blocks.
    Vanilla(VanillaTint),
    /// A mod's source.
    Source(Arc<dyn BlockTintSource>),
}

impl From<VanillaTint> for TintLayer {
    fn from(tint: VanillaTint) -> Self {
        Self::Vanilla(tint)
    }
}

impl<T: BlockTintSource> From<Arc<T>> for TintLayer {
    fn from(source: Arc<T>) -> Self {
        Self::Source(source)
    }
}

impl std::fmt::Debug for TintLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vanilla(tint) => f.debug_tuple("Vanilla").field(tint).finish(),
            Self::Source(_) => f.write_str("Source"),
        }
    }
}

/// The block tint registry `register_block_tint_sources` hands out: NeoForge's
/// `RegisterColorHandlersEvent.BlockTintSources`.
pub trait BlockTintSources {
    /// `register(sources, blocks...)`, vanilla's `BlockColors.register`: `layers[i]` colours the
    /// faces of `blocks` whose `tintindex` is i, and a face whose index is past the list is not
    /// tinted. Blocks are ids, a mod's (`mymod:cable`) or vanilla's (`minecraft:oak_leaves`); a
    /// later registration for a block replaces the earlier one, vanilla's own included.
    fn register(&mut self, layers: Vec<TintLayer>, blocks: Vec<String>);
}

/// The game's [`BlockTintSources`]: the registrations in order.
#[derive(Default)]
// exhaustive: its field is private; the game builds it
pub struct BlockTintRegistry {
    registrations: Vec<(Vec<TintLayer>, Vec<String>)>,
}

impl BlockTintSources for BlockTintRegistry {
    fn register(&mut self, layers: Vec<TintLayer>, blocks: Vec<String>) {
        self.registrations.push((layers, blocks));
    }
}

impl BlockTintRegistry {
    /// Each block's layers, the last registration for a block winning, in block-id order.
    pub fn into_layers(self) -> std::collections::BTreeMap<String, Arc<[TintLayer]>> {
        let mut by_block = std::collections::BTreeMap::new();
        for (layers, blocks) in self.registrations {
            let layers: Arc<[TintLayer]> = layers.into();
            for block in blocks {
                by_block.insert(block, layers.clone());
            }
        }
        by_block
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(u32);
    impl BlockTintSource for Fixed {
        fn color(&self, _: &BlockState) -> u32 {
            self.0
        }
    }

    #[test]
    fn a_later_registration_for_a_block_replaces_the_earlier_one() {
        let mut registry = BlockTintRegistry::default();
        registry.register(
            vec![VanillaTint::Grass.into()],
            vec!["mymod:a".into(), "mymod:b".into()],
        );
        registry.register(
            vec![
                Arc::new(Fixed(0x12_3456)).into(),
                VanillaTint::Foliage.into(),
            ],
            vec!["mymod:b".into()],
        );
        let layers = registry.into_layers();
        assert!(matches!(
            &*layers["mymod:a"],
            [TintLayer::Vanilla(VanillaTint::Grass)]
        ));
        assert!(matches!(
            &*layers["mymod:b"],
            [
                TintLayer::Source(_),
                TintLayer::Vanilla(VanillaTint::Foliage)
            ]
        ));
    }
}

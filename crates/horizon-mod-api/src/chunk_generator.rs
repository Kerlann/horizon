//! Chunk generators a native mod registers: vanilla's `ChunkGenerator` with
//! its own terrain, as
//! [`register_chunk_generator`](super::Registrar::register_chunk_generator)
//! hands it over once at startup. A level whose stem names `modid:void`, one
//! [`register_dimension`](super::Registrar::register_dimension) made or a data
//! pack's, then generates by the mod's code.
//!
//! There is no `Host` here: chunks fill on the worldgen worker threads, so the
//! mod sees only the [`ChunkAccess`] of the chunk it fills, and a generator
//! keeps no mutable state because two workers can fill neighbouring chunks at
//! once. The level gets terrain only: no surface, carvers or features.
use crate::ChunkPos;
use horizon_world::registry::BlockStateId;
use horizon_world::worldgen::mod_generators::{
    ChunkAccess as EngineChunk, ModChunkGenerator, ModChunkGenerators,
};
use std::sync::Arc;

/// A block state a generator writes: the handle [`ChunkAccess::state`] gives and
/// [`ChunkAccess::set_block`] takes. It means something only for the chunk that gave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TerrainState(pub(crate) u32);

/// The chunk a generator fills: vanilla's `ChunkAccess`, as far as terrain needs it. Positions
/// are world blocks.
pub trait ChunkAccess {
    /// `ChunkAccess.getPos`.
    fn pos(&self) -> ChunkPos;
    /// The world's seed.
    fn seed(&self) -> i64;
    /// `getMinY`: the level's lowest block Y.
    fn min_y(&self) -> i32;
    /// `getHeight`: the level's build height, from its dimension type.
    fn height(&self) -> i32;
    /// A block state by block name (`stone`, `minecraft:oak_log`), with its default
    /// properties; air for a block the game doesn't have.
    fn state(&self, name: &str) -> TerrainState;
    /// A block state by block name with properties (`oak_log` with `axis=y`); missing
    /// properties keep defaults.
    fn state_with(&self, name: &str, properties: &[(&str, &str)]) -> TerrainState;
    /// `setBlockState`: write `state` at `pos`, answering whether it was written. A position
    /// outside the chunk or the build height is skipped.
    fn set_block(&mut self, pos: [i32; 3], state: TerrainState) -> bool;
}

/// A mod's chunk generator: vanilla's `ChunkGenerator`, whose code answers for
/// a level stem with `"generator": {"type": "modid:void"}`. Implemented once,
/// kept by the game; deterministic and stateless.
pub trait ChunkGenerator: Send + Sync + 'static {
    /// `ChunkGenerator.buildTerrain`: write the terrain of `chunk`, which
    /// starts as air. The same seed and chunk give the same blocks.
    fn build_terrain(&self, chunk: &mut dyn ChunkAccess);
}

/// A mod's generator as the engine's: the engine's chunk reaches the mod as a [`ChunkAccess`],
/// its block states converted to the API's handles and back.
pub(crate) struct Adapter(pub(crate) Arc<dyn ChunkGenerator>);

impl ModChunkGenerator for Adapter {
    fn build_terrain(&self, chunk: &mut dyn EngineChunk) {
        self.0.build_terrain(&mut Converting(chunk));
    }
}

/// The engine's chunk, as the mod's [`ChunkAccess`].
struct Converting<'a>(&'a mut dyn EngineChunk);

impl ChunkAccess for Converting<'_> {
    fn pos(&self) -> ChunkPos {
        self.0.pos()
    }
    fn seed(&self) -> i64 {
        self.0.seed()
    }
    fn min_y(&self) -> i32 {
        self.0.min_y()
    }
    fn height(&self) -> i32 {
        self.0.height()
    }
    fn state(&self, name: &str) -> TerrainState {
        TerrainState(self.0.state(name).0)
    }
    fn state_with(&self, name: &str, properties: &[(&str, &str)]) -> TerrainState {
        TerrainState(self.0.state_with(name, properties).0)
    }
    fn set_block(&mut self, pos: [i32; 3], state: TerrainState) -> bool {
        self.0.set_block(pos, BlockStateId(state.0))
    }
}

/// What the game knows about a registered generator: its namespaced id
/// (`modid:void`), which a level stem's `generator` names, and its engine side.
pub(crate) struct ChunkGeneratorInfo {
    pub(crate) id: Box<str>,
    pub(crate) generator: Arc<dyn ModChunkGenerator>,
}

/// The generators the mods registered at startup, as the engine reads them:
/// `modid:void` → generator. Empty before the parts are installed.
pub fn installed_chunk_generators() -> ModChunkGenerators {
    super::parts::installed_chunk_generator_list()
        .iter()
        .map(|info| (info.id.to_string(), info.generator.clone()))
        .collect()
}

//! A level's blocks and time for native server mods, through a `LevelRef`
//! (docs/native-mods.md, "Entity and level handles", #7387). They read any
//! level the server has loaded, the one in place or another, and return
//! `None` (or `false`) on a client, for a handle of an earlier open, or for a
//! block in a chunk that isn't loaded.
use super::Game;
use horizon_client::mod_api::{ChunkRef, ChunkStatus, LevelRef};
use horizon_client::world::World;

impl Game<'_> {
    /// The world a handle names now, or None for a stale or foreign one.
    pub(super) fn native_level_world(&self, level: LevelRef) -> Option<&World> {
        if !self.is_logical_server() || self.native_lifecycle.epoch() != Some(level.epoch()) {
            return None;
        }
        if level.dimension() == self.dim.id() {
            return Some(&self.world);
        }
        self.levels.parked_world_of(level.dimension())
    }
    /// Find a loaded chunk in an open server level. API @API:chunk_refs@.
    pub(super) fn native_chunk(&self, level: LevelRef, pos: [i32; 2]) -> Option<ChunkRef> {
        self.native_level_world(level)?
            .chunks
            .contains_key(&(pos[0], pos[1]))
            .then_some(ChunkRef { level, pos })
    }
    /// Whether this chunk is loaded; false on a client or for a stale handle.
    /// API @API:chunk_refs@.
    pub(super) fn native_chunk_ref_loaded(&self, chunk: ChunkRef) -> bool {
        self.native_chunk(chunk.level(), chunk.pos()).is_some()
    }
    /// The status of a loaded chunk. Proto chunks are not exposed here.
    /// API @API:chunk_refs@.
    pub(super) fn native_chunk_status(&self, chunk: ChunkRef) -> Option<ChunkStatus> {
        self.native_chunk(chunk.level(), chunk.pos())
            .map(|_| ChunkStatus::Full)
    }
    /// The state at `pos`, if its chunk is loaded and `y` is inside the world.
    pub(super) fn native_level_state(
        &self,
        level: LevelRef,
        pos: [i32; 3],
    ) -> Option<horizon_world::registry::BlockStateId> {
        let world = self.native_level_world(level)?;
        let [x, y, z] = pos;
        let inside = world.shape.contains_y(y);
        (inside && world.chunks.contains_key(&(x >> 4, z >> 4))).then(|| world.block(x, y, z))
    }
    /// The block state at `pos` (`Level.getBlockState`) as `/setblock` takes
    /// it: `minecraft:oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]`.
    /// None for a chunk that isn't loaded or a `y` outside the world. API
    /// @API:level_reads@.
    pub(super) fn native_level_block_state(
        &self,
        level: LevelRef,
        pos: [i32; 3],
    ) -> Option<String> {
        self.native_level_state(level, pos)
            .map(|state| self.state_string(state))
    }
    /// The block at `pos` without its properties: `minecraft:oak_stairs`.
    /// None as for `block_state`. API @API:level_reads@.
    pub(super) fn native_level_block(&self, level: LevelRef, pos: [i32; 3]) -> Option<String> {
        self.native_level_state(level, pos)
            .map(|state| self.res.facts().get(state).name.to_string())
    }
    /// Property `key` of the block state at `pos` (`BlockState.getValue`):
    /// `east` for `facing`. None for a block without it, or as for
    /// `block_state`. API @API:level_reads@.
    pub(super) fn native_level_block_property(
        &self,
        level: LevelRef,
        pos: [i32; 3],
        key: &str,
    ) -> Option<String> {
        let state = self.native_level_state(level, pos)?;
        self.pack
            .assets
            .registry
            .state(state)?
            .properties
            .get(key)
            .cloned()
    }
    /// Whether this is the level in place: the one the server ticks now, whose block entities
    /// the game's own capabilities read and write and whose capabilities
    /// `LevelRef::capability` looks up. False for a level parked while the server ticks
    /// another (its blocks can still be read), for a handle of an earlier open, and on a
    /// client. API @API:level_reads@.
    pub(super) fn native_level_in_place(&self, level: LevelRef) -> bool {
        self.is_logical_server()
            && self.native_lifecycle.epoch() == Some(level.epoch())
            && self.dim.id() == level.dimension()
    }
    /// Whether `pos` is inside the world's height and its chunk is loaded
    /// (`Level.isLoaded`, which is false outside `isInValidBounds`): a block
    /// read or write there finds the world. API @API:level_reads@.
    pub(super) fn native_level_is_loaded(&self, level: LevelRef, pos: [i32; 3]) -> bool {
        self.native_level_world(level).is_some_and(|w| {
            w.shape.contains_y(pos[1]) && w.chunks.contains_key(&(pos[0] >> 4, pos[2] >> 4))
        })
    }
    /// The lowest block `y` and the height of the level
    /// (`LevelHeightAccessor.getMinY`, `getHeight`). API @API:level_reads@.
    pub(super) fn native_level_height(&self, level: LevelRef) -> Option<(i32, i32)> {
        self.native_level_world(level)
            .map(|w| (w.shape.min_y, w.shape.height))
    }
    /// The ticks the server has run (`Level.getGameTime`), shared by every
    /// level as in vanilla. API @API:level_reads@.
    pub(super) fn native_level_game_time(&self, level: LevelRef) -> Option<i64> {
        self.native_level_world(level)?;
        Some(self.world.game_time)
    }
    /// The time of day in ticks (`Level.getOverworldClockTime`), shared by
    /// every level; it keeps counting past 24000. API @API:level_reads@.
    pub(super) fn native_level_day_time(&self, level: LevelRef) -> Option<i64> {
        self.native_level_world(level)?;
        Some(self.world.day_time)
    }
}

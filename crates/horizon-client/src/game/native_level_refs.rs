//! A level's blocks and time for native server mods, through a `LevelRef`
//! (docs/native-mods.md, "Entity and level handles", #7387). They read any
//! level the server has loaded, the one in place or another, and return
//! `None` (or `false`) on a client, for a handle of an earlier open, or for a
//! block in a chunk that isn't loaded.
use super::Game;
use horizon_client::mod_api::LevelRef;
use horizon_client::world::World;

fn level_state(
    world: &World,
    [x, y, z]: [i32; 3],
) -> Option<horizon_world::registry::BlockStateId> {
    let inside = (world.bottom_y()..world.top_y()).contains(&y);
    (inside && world.chunks.contains_key(&(x >> 4, z >> 4))).then(|| world.block(x, y, z))
}

impl Game<'_> {
    /// The world a handle names now, or None for a stale or foreign one.
    pub(super) fn native_level_world(&self, level: LevelRef) -> Option<&World> {
        if self.remote.is_some() || self.native_lifecycle.epoch() != Some(level.epoch()) {
            return None;
        }
        if level.dimension() == self.dim.id() {
            return Some(&self.world);
        }
        let dim = self
            .levels
            .parked_dims()
            .into_iter()
            .find(|d| d.id() == level.dimension())?;
        self.levels.parked_world(dim)
    }
    /// The state at `pos`, if its chunk is loaded and `y` is inside the world.
    fn native_level_state(
        &self,
        level: LevelRef,
        pos: [i32; 3],
    ) -> Option<horizon_world::registry::BlockStateId> {
        level_state(self.native_level_world(level)?, pos)
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
    /// Whether the chunk holding `pos` is loaded (`Level.isLoaded`): a block
    /// read or write there finds the world. API @API:level_reads@.
    pub(super) fn native_level_is_loaded(&self, level: LevelRef, pos: [i32; 3]) -> bool {
        self.native_level_state(level, pos).is_some()
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

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::chunk::Chunk;
    use horizon_world::registry::BlockStateId;
    use std::sync::Arc;

    #[test]
    fn loaded_chunk_does_not_make_out_of_height_blocks_loaded() {
        let mut world = World::new(BlockStateId(0), 0);
        world.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        assert!(level_state(&world, [0, 64, 0]).is_some());
        assert!(level_state(&world, [0, 400, 0]).is_none());
        assert!(level_state(&world, [16, 64, 0]).is_none());
    }
}

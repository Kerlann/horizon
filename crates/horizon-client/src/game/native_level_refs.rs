//! A level's blocks and time for native server mods, through a `LevelRef`
//! (docs/native-mods.md, "Entity and level handles", #7387). They read any
//! level the server has loaded, the one in place or another, and return
//! `None` (or `false`) on a client, for a handle of an earlier open, or for a
//! block in a chunk that isn't loaded.
use super::Game;
use horizon_client::mod_api::{ChunkRef, ChunkStatus, LevelRef};
use horizon_client::{streaming::Streaming, world::World};
use horizon_core::position::ChunkPos;

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
    /// Find a loaded chunk in an open server level.
    pub(super) fn native_chunk(&self, level: LevelRef, pos: [i32; 2]) -> Option<ChunkRef> {
        self.native_level_world(level)?
            .chunks
            .contains_key(&(pos[0], pos[1]))
            .then_some(ChunkRef { level, pos })
    }
    /// Whether this chunk is loaded; false on a client or for a stale handle.
    pub(super) fn native_chunk_ref_loaded(&self, chunk: ChunkRef) -> bool {
        self.native_chunk(chunk.level(), chunk.pos()).is_some()
    }
    /// The status of a loaded chunk. Proto chunks are not exposed here.
    pub(super) fn native_chunk_status(&self, chunk: ChunkRef) -> Option<ChunkStatus> {
        self.native_chunk(chunk.level(), chunk.pos())
            .map(|_| ChunkStatus::Full)
    }
    /// A handle for chunk `pos` of an open level, loaded or not, so a mod can
    /// force an unloaded chunk. None for a stale handle, on a client, and for
    /// a chunk outside the world (`ChunkPos` past 30 million blocks).
    pub(super) fn native_level_chunk_ref(
        &self,
        level: LevelRef,
        pos: [i32; 2],
    ) -> Option<ChunkRef> {
        const LIMIT: i32 = 30_000_000 >> 4;
        self.native_level_world(level)?;
        let inside = pos.iter().all(|c| (-LIMIT..LIMIT).contains(c));
        inside.then_some(ChunkRef::new(level, pos))
    }
    /// The streaming of the level a handle names now, or None for a stale or
    /// foreign one.
    fn native_level_streaming(&self, level: LevelRef) -> Option<&Streaming> {
        self.native_level_world(level)?;
        if level.dimension() == self.dim.id() {
            return Some(&self.streaming);
        }
        self.levels.parked_streaming_of(level.dimension())
    }
    /// Whether the chunk is force-loaded (`ServerLevel.getForcedChunks`
    /// holds it), loaded or not. None for a stale handle or on a client.
    pub(super) fn native_chunk_forced(&self, chunk: ChunkRef) -> Option<bool> {
        let [x, z] = chunk.pos();
        Some(
            self.native_level_streaming(chunk.level())?
                .forced
                .contains(&(x, z)),
        )
    }
    /// `/forceload add` or `remove` of one chunk (`ServerLevel.setChunkForced`):
    /// true when the forced set changed. A forced chunk is kept loaded and
    /// entity ticking, saved with the level, and the level keeps ticking
    /// without a player. This is the world's own `/forceload` state, not a
    /// ticket the mod owns: the chunk stays forced across restarts, and after
    /// the mod is unloaded or removed, until `set_forced(false)` or
    /// `/forceload remove` clears it. Unlike vanilla, the chunk is not loaded before
    /// this returns: it loads over the next ticks, as after `/forceload`.
    /// False for a stale handle, on a client, and for a level the server
    /// has not opened.
    pub(super) fn native_chunk_set_forced(&mut self, chunk: ChunkRef, forced: bool) -> bool {
        if self.native_level_streaming(chunk.level()).is_none() {
            return false;
        }
        let [x, z] = chunk.pos();
        let streaming = if chunk.level().dimension() == self.dim.id() {
            &mut self.streaming
        } else if let Some(streaming) = self
            .levels
            .parked_streaming_of_mut(chunk.level().dimension())
        {
            streaming
        } else {
            return false;
        };
        streaming.set_forced(x, z, forced)
    }
    /// The level's force-loaded chunks (`/forceload query`), sorted as
    /// the command lists them. None for a stale handle or on a client.
    pub(super) fn native_level_forced_chunks(&self, level: LevelRef) -> Option<Vec<[i32; 2]>> {
        let mut forced: Vec<(i32, i32)> = self
            .native_level_streaming(level)?
            .forced
            .iter()
            .copied()
            .collect();
        forced.sort_unstable_by_key(|&(x, z)| ChunkPos::new(x, z).pack());
        Some(forced.into_iter().map(|(x, z)| [x, z]).collect())
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
    /// None for a chunk that isn't loaded or a `y` outside the world.
    pub(super) fn native_level_block_state(
        &self,
        level: LevelRef,
        pos: [i32; 3],
    ) -> Option<String> {
        self.native_level_state(level, pos)
            .map(|state| self.state_string(state))
    }
    /// The block at `pos` without its properties: `minecraft:oak_stairs`.
    /// None as for `block_state`.
    pub(super) fn native_level_block(&self, level: LevelRef, pos: [i32; 3]) -> Option<String> {
        self.native_level_state(level, pos)
            .map(|state| self.res.facts().get(state).name.to_string())
    }
    /// Property `key` of the block state at `pos` (`BlockState.getValue`):
    /// `east` for `facing`. None for a block without it, or as for
    /// `block_state`.
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
    /// client.
    pub(super) fn native_level_in_place(&self, level: LevelRef) -> bool {
        self.is_logical_server()
            && self.native_lifecycle.epoch() == Some(level.epoch())
            && self.dim.id() == level.dimension()
    }
    /// Whether `pos` is inside the world's height and its chunk is loaded
    /// (`Level.isLoaded`, which is false outside `isInValidBounds`): a block
    /// read or write there finds the world. False on a client or for a stale
    /// handle: the position is not loaded here.
    pub(super) fn native_level_is_loaded(&self, level: LevelRef, pos: [i32; 3]) -> bool {
        self.native_level_world(level).is_some_and(|w| {
            w.shape.contains_y(pos[1]) && w.chunks.contains_key(&(pos[0] >> 4, pos[2] >> 4))
        })
    }
    /// The lowest block `y` and the height of the level
    /// (`LevelHeightAccessor.getMinY`, `getHeight`).
    pub(super) fn native_level_height(&self, level: LevelRef) -> Option<(i32, i32)> {
        self.native_level_world(level)
            .map(|w| (w.shape.min_y, w.shape.height))
    }
    /// The ticks the server has run (`Level.getGameTime`), shared by every
    /// level as in vanilla.
    pub(super) fn native_level_game_time(&self, level: LevelRef) -> Option<i64> {
        self.native_level_world(level)?;
        Some(self.world.game_time)
    }
    /// The time of day in ticks (`Level.getOverworldClockTime`), shared by
    /// every level; it keeps counting past 24000.
    pub(super) fn native_level_day_time(&self, level: LevelRef) -> Option<i64> {
        self.native_level_world(level)?;
        Some(self.world.day_time)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Dim, native_entity_refs::tests::with_game};
    use horizon_client::mod_api::{ChunkRef, NativeWorldEpoch};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn forced_chunks_set_clear_and_query_like_forceload() {
        with_game(|game| {
            let level = game.native_level(game.dim.id()).unwrap();
            let far = game.native_level_chunk_ref(level, [40, -7]).unwrap();
            let near = game.native_level_chunk_ref(level, [-3, 2]).unwrap();
            assert_eq!(game.native_chunk_forced(far), Some(false));
            assert_eq!(game.native_level_forced_chunks(level), Some(vec![]));
            assert!(game.native_chunk_set_forced(far, true));
            assert!(!game.native_chunk_set_forced(far, true), "already forced");
            assert!(game.native_chunk_set_forced(near, true));
            assert_eq!(game.native_chunk_forced(far), Some(true));
            // The command's own set and order.
            assert!(game.streaming.forced.contains(&(40, -7)));
            let listed = game.native_level_forced_chunks(level).unwrap();
            // Signed packed positions: z first, then x.
            assert_eq!(listed, vec![[40, -7], [-3, 2]]);
            assert!(game.native_chunk_set_forced(far, false));
            assert!(!game.native_chunk_set_forced(far, false), "already clear");
            assert_eq!(game.native_level_forced_chunks(level), Some(vec![[-3, 2]]));
            assert!(!game.streaming.forced.contains(&(40, -7)));
            // Outside the world.
            assert!(game.native_level_chunk_ref(level, [1_875_000, 0]).is_none());
            assert!(
                game.native_level_chunk_ref(level, [0, -1_875_001])
                    .is_none()
            );
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn forced_chunk_changes_the_ticket_sources_and_the_saved_set() {
        with_game(|game| {
            let level = game.native_level(game.dim.id()).unwrap();
            let chunk = game.native_level_chunk_ref(level, [5, 6]).unwrap();
            assert!(game.native_chunk_set_forced(chunk, true));
            assert!(game.levels_to_tick().contains(&game.dim));
            game.save
                .save_forced_chunks(&game.streaming.forced)
                .unwrap();
            assert!(game.save.load_forced_chunks().contains(&(5, 6)));
            assert!(game.native_chunk_set_forced(chunk, false));
            game.save
                .save_forced_chunks(&game.streaming.forced)
                .unwrap();
            assert!(game.save.load_forced_chunks().is_empty());
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn forced_chunks_of_a_parked_level_and_stale_handles() {
        with_game(|game| {
            let stale_epoch =
                NativeWorldEpoch::new(game.native_lifecycle.epoch().unwrap().get() + 1);
            let level = game.native_level(game.dim.id()).unwrap();
            let stale_level =
                horizon_client::mod_api::LevelRef::new(stale_epoch, level.dimension());
            let stale = ChunkRef::new(stale_level, [0, 0]);
            assert_eq!(game.native_chunk_forced(stale), None);
            assert!(!game.native_chunk_set_forced(stale, true));
            assert_eq!(game.native_level_forced_chunks(stale_level), None);
            assert!(game.native_level_chunk_ref(stale_level, [0, 0]).is_none());
            // A level the server has not opened.
            let other = if game.dim == Dim::Nether {
                Dim::End
            } else {
                Dim::Nether
            };
            let parked = horizon_client::mod_api::LevelRef::new(level.epoch(), other.id());
            let chunk = ChunkRef::new(parked, [1, 1]);
            assert_eq!(game.native_chunk_forced(chunk), None);
            assert!(!game.native_chunk_set_forced(chunk, true));
            // Opened, then parked.
            let back = game.dim;
            game.enter_level(None, other).unwrap();
            game.enter_level(None, back).unwrap();
            assert_eq!(game.native_chunk_forced(chunk), Some(false));
            assert!(game.native_chunk_set_forced(chunk, true));
            assert_eq!(game.native_level_forced_chunks(parked), Some(vec![[1, 1]]));
            assert_eq!(game.native_level_forced_chunks(level), Some(vec![]));
            assert!(game.levels_to_tick().contains(&other));
            assert!(game.native_chunk_set_forced(chunk, false));
            assert!(!game.levels_to_tick().contains(&other));
        });
    }
}

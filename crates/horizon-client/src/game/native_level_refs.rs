//! A level's blocks and time for native server mods, through a `LevelRef`
//! (docs/native-mods.md, "Entity and level handles", #7387). They read any
//! level the server has loaded, the one in place or another, and return
//! `None` (or `false`) on a client, for a handle of an earlier open, or for a
//! block in a chunk that isn't loaded.
use super::Game;
use horizon_client::mod_api::{ChunkRef, ChunkStatus, LevelRef, NativeTicketOwner};
use horizon_client::mod_chunk_tickets::ModTicket;
use horizon_client::{streaming::Streaming, world::World};
use horizon_core::position::ChunkPos;

/// Whether chunk `pos` lies inside the world (`/forceload`'s 30 million block bound).
fn chunk_in_world(pos: [i32; 2]) -> bool {
    const LIMIT: i32 = 30_000_000 >> 4;
    pos.iter().all(|c| (-LIMIT..LIMIT).contains(c))
}

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
        self.native_level_world(level)?;
        chunk_in_world(pos).then_some(ChunkRef::new(level, pos))
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
    /// Whether `/forceload` holds the chunk (`ServerLevel.getForcedChunks`),
    /// loaded or not; a mod's own ticket on it does not count. None for a stale handle or on a client.
    pub(super) fn native_chunk_forced(&self, chunk: ChunkRef) -> Option<bool> {
        let [x, z] = chunk.pos();
        Some(
            self.native_level_streaming(chunk.level())?
                .commanded()
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
    /// The mods validate the chunk tickets `dim`'s level loaded from its save,
    /// and the survivors take hold. Tickets a mod added since the load (in
    /// `level_load`, say) are not candidates. Nothing is built while no mod
    /// listens.
    pub(super) fn validate_mod_chunk_tickets(&mut self, dim: super::Dim) {
        let loaded = self.streaming.take_loaded_tickets();
        let loaded = if loaded.is_empty()
            || !horizon_client::mod_api::listened(
                horizon_client::mod_api::HookId::ChunkTicketsValidate,
            ) {
            loaded
        } else {
            let (mut validation, removals) =
                horizon_client::mod_chunk_tickets::TicketValidation::new(loaded.clone());
            self.native_chunk_tickets_validate(dim.id(), &mut validation);
            let removed = removals.into_removed();
            loaded
                .into_iter()
                .filter(|t| !removed.contains(t))
                .collect()
        };
        for ticket in loaded {
            self.streaming.set_mod_forced(ticket, true);
        }
    }
    /// The calling mod's chunk ticket for `owner` on `chunk`
    /// (`TicketController.forceChunk`): true when the mod's tickets changed.
    /// A chunk stays forced, entity ticking and saved, and its level keeps
    /// ticking, while any ticket or `/forceload` holds it. With
    /// `natural_spawning` the chunk joins the spawning chunks (inhabited
    /// time, thunder, a spawn attempt) while it lies in the spawning area of a
    /// player (`ChunkMap.collectSpawningChunks`); the local mob cap still lets
    /// mobs spawn only with a non-spectator within 128 blocks. One
    /// owner holds a plain and a natural-spawning ticket on a chunk apart. Tickets are
    /// scoped by (mod, owner, chunk), so two owners in one chunk release
    /// separately. They are saved in `data/horizon/mod_chunk_tickets.dat`;
    /// when the world loads, those of a mod that isn't installed are discarded.
    /// False outside a mod's hook, for a stale handle, on a client, for a
    /// chunk outside the world and for a level the server has not opened.
    pub(super) fn native_chunk_set_mod_forced(
        &mut self,
        chunk: ChunkRef,
        owner: NativeTicketOwner,
        forced: bool,
        natural_spawning: bool,
    ) -> bool {
        let Some(mod_id) = horizon_client::crash::current_native_mod() else {
            return false;
        };
        if self.native_level_streaming(chunk.level()).is_none() || !chunk_in_world(chunk.pos()) {
            return false;
        }
        let [x, z] = chunk.pos();
        let ticket = ModTicket {
            mod_id: mod_id.to_owned(),
            owner,
            chunk: (x, z),
            natural_spawning,
        };
        let dimension = chunk.level().dimension();
        let streaming = if dimension == self.dim.id() {
            &mut self.streaming
        } else if let Some(streaming) = self.levels.parked_streaming_of_mut(dimension) {
            streaming
        } else {
            return false;
        };
        streaming.set_mod_forced(ticket, forced)
    }
    /// The calling mod's chunk tickets in the level, as (owner, chunk
    /// position, natural_spawning), in order. None for a stale handle, on a client and outside
    /// a mod's hook.
    pub(super) fn native_level_mod_forced_chunks(
        &self,
        level: LevelRef,
    ) -> Option<Vec<(NativeTicketOwner, [i32; 2], bool)>> {
        let mod_id = horizon_client::crash::current_native_mod()?;
        let tickets = self.native_level_streaming(level)?.mod_tickets();
        Some(
            tickets
                .of_mod(mod_id)
                .map(|t| (t.owner, [t.chunk.0, t.chunk.1], t.natural_spawning))
                .collect(),
        )
    }
    /// The level's force-loaded chunks (`/forceload query`), sorted as
    /// the command lists them. None for a stale handle or on a client.
    pub(super) fn native_level_forced_chunks(&self, level: LevelRef) -> Option<Vec<[i32; 2]>> {
        let mut forced: Vec<(i32, i32)> = self
            .native_level_streaming(level)?
            .commanded()
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
            game.enter_level(other).unwrap();
            game.enter_level(back).unwrap();
            assert_eq!(game.native_chunk_forced(chunk), Some(false));
            assert!(game.native_chunk_set_forced(chunk, true));
            assert_eq!(game.native_level_forced_chunks(parked), Some(vec![[1, 1]]));
            assert_eq!(game.native_level_forced_chunks(level), Some(vec![]));
            assert!(game.levels_to_tick().contains(&other));
            assert!(game.native_chunk_set_forced(chunk, false));
            assert!(!game.levels_to_tick().contains(&other));
        });
    }

    fn in_mod(id: &'static str) -> horizon_client::crash::NativeModGuard {
        horizon_client::crash::enter_native_mod(horizon_client::crash::NativeModCall {
            id,
            hook: "test",
            part: None,
        })
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mod_tickets_are_scoped_by_owner_and_share_the_forceload_store() {
        use horizon_client::mod_api::NativeTicketOwner::{Block, Entity};
        with_game(|game| {
            let level = game.native_level(game.dim.id()).unwrap();
            let chunk = game.native_level_chunk_ref(level, [5, 6]).unwrap();
            let (a, b) = (Block([80, 64, 96]), Entity(7));
            // Outside a mod's hook there is no caller to own a ticket.
            assert!(!game.native_chunk_set_mod_forced(chunk, a, true, false));
            let guard = in_mod("machines");
            assert!(game.native_chunk_set_mod_forced(chunk, a, true, false));
            assert!(
                !game.native_chunk_set_mod_forced(chunk, a, true, false),
                "held"
            );
            assert!(game.native_chunk_set_mod_forced(chunk, b, true, false));
            assert!(game.streaming.forced.contains(&(5, 6)));
            assert_eq!(
                game.native_chunk_forced(chunk),
                Some(false),
                "not /forceload's"
            );
            assert_eq!(game.native_level_forced_chunks(level), Some(vec![]));
            // Two owners in one chunk release separately.
            assert!(game.native_chunk_set_mod_forced(chunk, a, false, false));
            assert!(game.streaming.forced.contains(&(5, 6)));
            assert_eq!(
                game.native_level_mod_forced_chunks(level),
                Some(vec![(b, [5, 6], false)])
            );
            // Another mod's tickets are not listed, and its release leaves ours.
            drop(guard);
            let guard = in_mod("other");
            assert_eq!(game.native_level_mod_forced_chunks(level), Some(vec![]));
            assert!(!game.native_chunk_set_mod_forced(chunk, b, false, false));
            drop(guard);
            // /forceload and a ticket on one chunk: the chunk stays until both go.
            assert!(game.native_chunk_set_forced(chunk, true));
            assert!(game.native_chunk_set_forced(chunk, false));
            assert!(
                game.streaming.forced.contains(&(5, 6)),
                "the ticket holds it"
            );
            assert!(game.native_chunk_set_forced(chunk, true));
            let _guard = in_mod("machines");
            assert!(game.native_chunk_set_mod_forced(chunk, b, false, false));
            assert!(
                game.streaming.forced.contains(&(5, 6)),
                "/forceload holds it"
            );
            game.streaming.clear_forced();
            assert!(game.streaming.forced.is_empty());
            // The world's bounds, as /forceload checks them.
            let outside = horizon_client::mod_api::ChunkRef::new(level, [1_875_000, 0]);
            assert!(!game.native_chunk_set_mod_forced(outside, a, true, false));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mod_tickets_save_apart_from_vanillas_tickets_and_release_with_the_world() {
        use horizon_client::mod_api::NativeTicketOwner::{Block, Entity};
        with_game(|game| {
            let level = game.native_level(game.dim.id()).unwrap();
            let chunk = game.native_level_chunk_ref(level, [-9, 4]).unwrap();
            let _guard = in_mod("machines");
            assert!(game.native_chunk_set_mod_forced(chunk, Block([-140, 70, 70]), true, false));
            assert!(game.native_chunk_set_mod_forced(chunk, Entity(u128::MAX - 3), true, false));
            game.save_chunk_tickets().unwrap();
            let vanilla = game.save.load_forced_chunks();
            assert!(vanilla.is_empty(), "vanilla's file has no mod ticket");
            // The world closes: its streaming goes, the next opening reads the file.
            let reopen = |game: &mut super::super::Game<'_>, loaded: fn(&str) -> bool| {
                game.streaming.load_forced(
                    game.save.load_forced_chunks(),
                    game.save.load_mod_tickets(),
                    loaded,
                );
                game.validate_mod_chunk_tickets(game.dim);
            };
            reopen(game, |id| id == "machines");
            assert_eq!(game.native_level_mod_forced_chunks(level).unwrap().len(), 2);
            assert!(game.streaming.forced.contains(&(-9, 4)));
            // Without the mod, its tickets are discarded: nothing is forced, and
            // the next save drops them from the file.
            reopen(game, |_| false);
            assert!(game.streaming.forced.is_empty());
            game.save_chunk_tickets().unwrap();
            reopen(game, |id| id == "machines");
            assert!(game.streaming.forced.is_empty());
            assert!(game.save.load_mod_tickets().is_empty());
            // Released tickets leave the file.
            assert!(game.native_chunk_set_mod_forced(chunk, Block([-140, 70, 70]), true, false));
            assert!(game.native_chunk_set_mod_forced(chunk, Block([-140, 70, 70]), false, false));
            game.save_chunk_tickets().unwrap();
            assert!(game.save.load_mod_tickets().is_empty());
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_mod_ticket_keeps_a_parked_level_active() {
        with_game(|game| {
            let level = game.native_level(game.dim.id()).unwrap();
            let other = if game.dim == Dim::Nether {
                Dim::End
            } else {
                Dim::Nether
            };
            let back = game.dim;
            game.enter_level(other).unwrap();
            game.enter_level(back).unwrap();
            let parked = horizon_client::mod_api::LevelRef::new(level.epoch(), other.id());
            let chunk = ChunkRef::new(parked, [2, 2]);
            let _guard = in_mod("machines");
            let owner = horizon_client::mod_api::NativeTicketOwner::Block([32, 64, 32]);
            assert!(!game.levels_to_tick().contains(&other));
            assert!(game.native_chunk_set_mod_forced(chunk, owner, true, false));
            assert!(game.levels_to_tick().contains(&other));
            assert!(game.native_chunk_set_mod_forced(chunk, owner, false, false));
            assert!(!game.levels_to_tick().contains(&other));
        });
    }

    /// The result `/forceload query` gives: the chunks listed, or 0 when it reports none.
    fn query_result(game: &mut super::super::Game<'_>, command: &str) -> Option<i32> {
        let d = horizon_client::command::vanilla::game_dispatcher_with_features(
            &game.level.enabled_features,
        );
        let parse = d.parse(command, &game.ui.registries);
        let (ctx, _) = parse.executable().ok()?;
        let (ctx, root) = (ctx.clone(), parse.context.clone());
        game.run_chain(&root, &ctx).ok()
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn forceload_query_lists_only_command_tickets() {
        with_game(|game| {
            let level = game.native_level(game.dim.id()).unwrap();
            let chunk = game.native_level_chunk_ref(level, [3, 3]).unwrap();
            game.ui.registries.permission = 4;
            let _guard = in_mod("machines");
            let owner = horizon_client::mod_api::NativeTicketOwner::Block([48, 64, 48]);
            assert!(game.native_chunk_set_mod_forced(chunk, owner, true, false));
            assert!(game.streaming.forced.contains(&(3, 3)));
            assert_eq!(query_result(game, "forceload query"), Some(0), "list form");
            assert_eq!(
                query_result(game, "forceload query 48 48"),
                None,
                "position form"
            );
            assert!(game.native_chunk_set_forced(chunk, true));
            assert_eq!(query_result(game, "forceload query"), Some(1));
            assert_eq!(query_result(game, "forceload query 48 48"), Some(1));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn natural_spawning_tickets_and_plain_ones_save_and_load_apart() {
        use horizon_client::mod_api::NativeTicketOwner::Block;
        with_game(|game| {
            let level = game.native_level(game.dim.id()).unwrap();
            let plain = game.native_level_chunk_ref(level, [11, 12]).unwrap();
            let spawning = game.native_level_chunk_ref(level, [13, 12]).unwrap();
            let _guard = in_mod("machines");
            let owner = Block([176, 64, 192]);
            assert!(game.native_chunk_set_mod_forced(plain, owner, true, false));
            assert!(game.native_chunk_set_mod_forced(spawning, owner, true, true));
            let tickets = game.streaming.mod_tickets();
            assert!(!tickets.forces_natural_spawning((11, 12)));
            assert!(tickets.forces_natural_spawning((13, 12)));
            // Both kinds load and tick: they are forced chunks.
            assert!(game.streaming.forced.contains(&(11, 12)));
            assert!(game.streaming.forced.contains(&(13, 12)));
            assert!(game.levels_to_tick().contains(&game.dim));
            assert_eq!(
                game.native_level_mod_forced_chunks(level),
                Some(vec![(owner, [11, 12], false), (owner, [13, 12], true)])
            );
            // One owner holds both kinds on one chunk apart.
            assert!(game.native_chunk_set_mod_forced(plain, owner, true, true));
            assert!(game.native_chunk_set_mod_forced(plain, owner, false, true));
            assert!(game.streaming.forced.contains(&(11, 12)));
            assert!(
                !game
                    .streaming
                    .mod_tickets()
                    .forces_natural_spawning((11, 12))
            );
            game.save_chunk_tickets().unwrap();
            let saved = game.save.load_mod_tickets();
            game.streaming
                .load_forced(Default::default(), saved, |_| true);
            game.validate_mod_chunk_tickets(game.dim);
            let tickets = game.streaming.mod_tickets();
            assert!(!tickets.forces_natural_spawning((11, 12)));
            assert!(tickets.forces_natural_spawning((13, 12)));
            assert!(tickets.holds((11, 12)));
        });
    }

    /// The flag adds a chunk to the spawning chunks (inhabited time, a spawn
    /// attempt); the local mob cap still wants a non-spectator within 128
    /// blocks of its centre, as NeoForge (which leaves `LocalMobCapCalculator`
    /// alone) does.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_natural_spawning_ticket_far_from_every_player_runs_the_attempt_but_the_local_cap_refuses()
    {
        use horizon_client::chunk::Chunk;
        use horizon_client::mobs::spawn_selection::{Category, SpawnPlayer, SpawnState};
        use horizon_client::mod_api::NativeTicketOwner::Block;
        use horizon_client::motion::PlayerMotion;
        use horizon_core::geometry::Vec3;
        use std::sync::Arc;
        with_game(|game| {
            game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
            for cx in 6..=10 {
                for cz in 6..=10 {
                    let mut chunk = Chunk::empty(cx, cz);
                    chunk.lit = true;
                    game.world.chunks.insert((cx, cz), Arc::new(chunk));
                }
            }
            let level = game.native_level(game.dim.id()).unwrap();
            // Both chunks lie in the player's 8-chunk area and past 128 blocks of it.
            let flagged = game.native_level_chunk_ref(level, [8, 8]).unwrap();
            let plain = game.native_level_chunk_ref(level, [8, 7]).unwrap();
            {
                let _guard = in_mod("machines");
                let owner = Block([128, 64, 128]);
                assert!(game.native_chunk_set_mod_forced(flagged, owner, true, true));
                assert!(game.native_chunk_set_mod_forced(plain, owner, true, false));
            }
            game.spawn_natural();
            assert_eq!(game.world.inhabited.get(&(8, 8)), Some(&1));
            assert!(
                !game.world.inhabited.contains_key(&(8, 7)),
                "without the flag it is no spawning chunk"
            );
            assert!(game.mobs.is_empty(), "the local cap refuses");
            // The cap itself: no player within 128 blocks of the chunk.
            let players: Vec<SpawnPlayer> = game
                .level_players()
                .iter()
                .map(|p| SpawnPlayer {
                    position: [p.position.x, p.position.y, p.position.z],
                    spectator: p.spectator,
                })
                .collect();
            let natural = game.natural_level();
            let mut state = SpawnState::new(&natural, 1, players, &[], |_, _| true);
            assert!(!state.can_spawn_local(Category::Monster, 8, 8));
            assert!(state.can_spawn_local(Category::Monster, 0, 0));
        });
    }

    use horizon_client::mod_api::{ChunkTicketsValidateEvent, Hooks, ModEntry, ServerHost};
    use horizon_client::mod_chunk_tickets::{ModTicket, TicketOwner};

    /// Removes the block at [1, 64, 1] from the tickets it is shown.
    struct Prune;
    impl Hooks for Prune {
        fn chunk_tickets_validate(
            &self,
            _: &mut dyn ServerHost,
            e: &mut ChunkTicketsValidateEvent<'_>,
        ) {
            e.validation.remove_block_tickets([1, 64, 1]);
        }
    }
    /// Subscribes to nothing.
    struct Idle;
    impl Hooks for Idle {}

    fn saved(mod_id: &str, pos: [i32; 3], chunk: (i32, i32)) -> ModTicket {
        ModTicket {
            mod_id: mod_id.into(),
            owner: TicketOwner::Block(pos),
            chunk,
            natural_spawning: false,
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn validation_drops_a_removed_ticket_and_leaves_the_rest() {
        let mods: horizon_client::mod_api::Mods = vec![
            ModEntry::new("machines", Box::new(Prune)),
            ModEntry::new("idle", Box::new(Idle)),
        ]
        .into();
        horizon_client::mod_api::testing::with_mods(mods, || {
            with_game(|game| {
                let tickets = vec![
                    saved("machines", [1, 64, 1], (7, 7)),
                    saved("machines", [9, 64, 9], (8, 8)),
                    saved("idle", [1, 64, 1], (9, 9)),
                ];
                game.streaming
                    .load_forced(Default::default(), tickets, |_| true);
                assert!(
                    game.streaming.forced.is_empty(),
                    "loaded, not yet validated"
                );
                game.validate_mod_chunk_tickets(game.dim);
                assert!(!game.streaming.forced.contains(&(7, 7)), "removed");
                assert!(game.streaming.forced.contains(&(8, 8)));
                assert!(
                    game.streaming.forced.contains(&(9, 9)),
                    "idle keeps its own"
                );
                game.save_chunk_tickets().unwrap();
                let file = game.save.load_mod_tickets();
                assert_eq!(file.len(), 2);
                assert!(!file.iter().any(|t| t.chunk == (7, 7)));
            });
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn validation_runs_when_a_parked_level_opens_and_not_without_a_listener() {
        let mods: horizon_client::mod_api::Mods = vec![
            ModEntry::new("machines", Box::new(Prune)),
            ModEntry::new("idle", Box::new(Idle)),
        ]
        .into();
        horizon_client::mod_api::testing::with_mods(mods, || {
            with_game(|game| {
                let other = if game.dim == Dim::Nether {
                    Dim::End
                } else {
                    Dim::Nether
                };
                let world =
                    std::env::temp_dir().join(format!("horizon-validate-{}", std::process::id()));
                game.world_root = world.clone();
                let root = horizon_client::save::dimension_dir(&world, other.id());
                let save = horizon_client::save::SaveDir::new(root, other.shape());
                let mut tickets = horizon_client::mod_chunk_tickets::ModTickets::default();
                tickets.insert(saved("machines", [1, 64, 1], (7, 7)));
                tickets.insert(saved("machines", [9, 64, 9], (8, 8)));
                save.save_mod_tickets(&tickets).unwrap();
                let back = game.dim;
                game.enter_level(other).unwrap();
                game.enter_level(back).unwrap();
                let parked = game.levels.parked_streaming_of(other.id()).unwrap();
                assert!(!parked.forced.contains(&(7, 7)));
                assert!(parked.forced.contains(&(8, 8)));
                assert!(game.levels_to_tick().contains(&other));
                let _ = std::fs::remove_dir_all(&world);
            });
        });
        // No mod listens: the tickets stay as loaded.
        let mods: horizon_client::mod_api::Mods =
            vec![ModEntry::new("idle", Box::new(Idle))].into();
        horizon_client::mod_api::testing::with_mods(mods, || {
            with_game(|game| {
                let tickets = vec![saved("idle", [1, 64, 1], (7, 7))];
                game.streaming
                    .load_forced(Default::default(), tickets, |_| true);
                game.validate_mod_chunk_tickets(game.dim);
                assert!(game.streaming.forced.contains(&(7, 7)));
            });
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn validation_sees_the_saved_tickets_not_ones_added_after_the_load() {
        let mods: horizon_client::mod_api::Mods =
            vec![ModEntry::new("machines", Box::new(Prune))].into();
        horizon_client::mod_api::testing::with_mods(mods, || {
            with_game(|game| {
                let level = game.native_level(game.dim.id()).unwrap();
                let tickets = vec![saved("machines", [1, 64, 1], (7, 7))];
                game.streaming
                    .load_forced(Default::default(), tickets, |_| true);
                // `level_load` runs after the load: its hold is the same owner's.
                let fresh = game.native_level_chunk_ref(level, [6, 6]).unwrap();
                {
                    let _guard = in_mod("machines");
                    let owner = horizon_client::mod_api::NativeTicketOwner::Block([1, 64, 1]);
                    assert!(game.native_chunk_set_mod_forced(fresh, owner, true, false));
                }
                game.validate_mod_chunk_tickets(game.dim);
                assert!(!game.streaming.forced.contains(&(7, 7)), "saved one pruned");
                assert!(game.streaming.forced.contains(&(6, 6)), "fresh one kept");
            });
        });
    }
}

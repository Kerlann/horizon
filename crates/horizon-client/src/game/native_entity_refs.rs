//! Entity and level handles for native server mods (docs/native-mods.md,
//! "Entity and level handles", #7387): `native_entity(id)` turns an event's
//! entity id into an `EntityRef`, and each `native_entity_*` helper reads the
//! entity a handle names. They read where the world is simulated: on a client,
//! with no world open, or for a handle of an earlier open, they return `None`.
//! The entities are those of the level in place (the one a hook runs in) and
//! the server's players, whatever their level.
use super::{Dim, Game, commands::Target};
use horizon_client::mod_api::{EntityRef, LevelRef, NativeBlockUpdateFlags};
use horizon_world::registry::BlockStateId;

impl Game<'_> {
    /// What a handle names now, or None for a stale or foreign one.
    pub(super) fn native_entity_target(&self, entity: EntityRef) -> Option<Target> {
        if self.remote.is_some() || self.native_lifecycle.epoch() != Some(entity.epoch()) {
            return None;
        }
        let id = i32::try_from(entity.id()).ok()?;
        let player = self
            .lan
            .iter()
            .flat_map(|h| h.hub.guests.iter())
            .find(|g| g.placed && g.entity_id == id);
        if let Some(g) = player {
            // `as_guest` swaps the acting player's state into the game.
            let acting = self.lan_acting == Some(g.conn) && self.player.is_some();
            return Some(if acting {
                Target::Player
            } else {
                Target::Guest(g.conn)
            });
        }
        match self.level_entity(id)? {
            Target::Mob(id) if !self.mobs.iter().any(|m| m.id == id && !m.removed) => None,
            t => Some(t),
        }
    }
    /// The entity `id` (the id events carry) as a handle for the `entity_*`
    /// methods: None on a client, with no world open, or for no such entity
    /// in the level in place or among the server's players. API
    /// @API:entity_handles@.
    pub(super) fn native_entity(&self, id: u32) -> Option<EntityRef> {
        let entity = EntityRef::new(self.native_lifecycle.epoch()?, id);
        self.native_entity_target(entity).map(|_| entity)
    }
    /// The level `dimension` (`minecraft:the_nether`) as a handle: None on a
    /// client, with no world open, or for a dimension the server hasn't
    /// loaded. API @API:entity_handles@.
    pub(super) fn native_level(&self, dimension: &str) -> Option<LevelRef> {
        if self.remote.is_some() {
            return None;
        }
        let epoch = self.native_lifecycle.epoch()?;
        let dim = std::iter::once(self.dim)
            .chain(self.levels.parked_dims())
            .find(|d| d.id() == dimension)?;
        Some(LevelRef::new(epoch, dim.id()))
    }
    /// Set a block in the level in place (`Level.setBlock`). Parked levels,
    /// stale handles, clients and unloaded or out-of-height positions return
    /// false. Horizon sends every change to clients. API @API:level_block_writes@.
    pub(super) fn native_level_set_block(
        &mut self,
        level: LevelRef,
        pos: [i32; 3],
        state: BlockStateId,
        flags: NativeBlockUpdateFlags,
    ) -> bool {
        if level.dimension() != self.dim.id() || !self.native_level_is_loaded(level, pos) {
            return false;
        }
        self.native_set_block(level.dimension(), pos, state, flags.bits())
    }
    /// Destroy a block in the level in place (`Level.destroyBlock`), optionally
    /// dropping its resources. Parked levels and invalid positions return false.
    /// API @API:level_block_writes@.
    pub(super) fn native_level_destroy_block(
        &mut self,
        level: LevelRef,
        pos: [i32; 3],
        drop: bool,
    ) -> bool {
        if level.dimension() != self.dim.id()
            || !self.native_level_is_loaded(level, pos)
            || (self.serving.is_none() && self.options.dedicated.is_none())
        {
            return false;
        }
        let at = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
        let changed = self.with_level(|b, level, ticks| b.destroy_block(level, ticks, at, drop));
        let _ = self.process_level_events();
        changed
    }
    /// The entity's position (`Entity.position`): its feet, the middle of
    /// its box for a painting or an item frame. API @API:entity_handles@.
    pub(super) fn native_entity_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        self.target_position(self.native_entity_target(entity)?)
    }
    /// The block the entity's position is in (`Entity.blockPosition`). API
    /// @API:entity_handles@.
    pub(super) fn native_entity_block_position(&self, entity: EntityRef) -> Option<[i32; 3]> {
        self.native_entity_position(entity)
            .map(|p| p.map(|v| v.floor() as i32))
    }
    /// The entity's yaw and pitch in degrees (`Entity.getYRot`, `getXRot`); 0
    /// for the kinds that don't turn. API @API:entity_handles@.
    pub(super) fn native_entity_rotation(&self, entity: EntityRef) -> Option<[f32; 2]> {
        Some(self.target_rotation(self.native_entity_target(entity)?))
    }
    /// The entity's type (`Entity.getType`): `minecraft:zombie`. API
    /// @API:entity_handles@.
    pub(super) fn native_entity_kind(&self, entity: EntityRef) -> Option<String> {
        Some(self.target_type(self.native_entity_target(entity)?))
    }
    /// The entity's UUID (`Entity.getUUID`), the one it is saved with; an
    /// entity that has none yet gets it now, as a new vanilla entity does.
    /// API @API:entity_handles@.
    pub(super) fn native_entity_uuid(&mut self, entity: EntityRef) -> Option<u128> {
        let words = match self.native_entity_target(entity)? {
            Target::Player => return self.command_guest(self.lan_acting?).map(|g| g.uuid),
            Target::Guest(conn) => return self.command_guest(conn).map(|g| g.uuid),
            _ => self.uuid_for(entity.id()),
        };
        Some(uuid_from_words(words))
    }
    /// The level the entity is in (`Entity.level`). API @API:entity_handles@.
    pub(super) fn native_entity_level(&self, entity: EntityRef) -> Option<LevelRef> {
        let dim: Dim = self.target_level(self.native_entity_target(entity)?);
        Some(LevelRef::new(entity.epoch(), dim.id()))
    }
}

/// A saved UUID's four ints (`UUIDUtil.uuidFromIntArray`) as one number.
fn uuid_from_words(words: [i32; 4]) -> u128 {
    words
        .iter()
        .fold(0, |uuid, w| (uuid << 32) | u128::from(*w as u32))
}

#[cfg(test)]
mod tests {
    #[test]
    fn uuid_words_are_most_significant_first() {
        let words = [0x0102_0304, -1, 0, 0x7fff_ffff];
        assert_eq!(
            super::uuid_from_words(words),
            0x0102_0304_ffff_ffff_0000_0000_7fff_ffff
        );
    }

    #[test]
    #[ignore = "needs prepared resources"]
    fn level_block_writes_validate_and_change_world() {
        use super::super::{Audio, Game, Options, Progress, Settings, load_resources};
        use horizon_client::{
            chunk::Chunk,
            mod_api::{NativeAuthority, NativeBlockUpdateFlags},
        };
        use std::{sync::Arc, time::Instant};

        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = super::super::execution_role::ExecutionRole::IntegratedServer;
        options.dedicated = Some(Vec::new());
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.native_lifecycle
            .begin_open(NativeAuthority::IntegratedServer);
        let level = game.native_level("minecraft:overworld").unwrap();
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        let chest = game
            .pack
            .assets
            .registry
            .default_state("minecraft:chest")
            .unwrap();
        let air = game
            .pack
            .assets
            .registry
            .default_state("minecraft:air")
            .unwrap();
        let flags = NativeBlockUpdateFlags::NEIGHBORS;
        assert!(game.native_level_set_block(level, [0, 64, 0], stone, flags));
        assert_eq!(game.world.block(0, 64, 0), stone);
        assert!(!game.native_level_set_block(level, [16, 64, 0], stone, flags));
        assert!(!game.native_level_set_block(level, [0, 10000, 0], stone, flags));
        let stale = horizon_client::mod_api::LevelRef::new(
            horizon_client::mod_api::NativeWorldEpoch::new(level.epoch().get() + 1),
            level.dimension(),
        );
        assert!(!game.native_level_set_block(stale, [0, 64, 0], stone, flags));
        assert!(!game.native_level_destroy_block(stale, [0, 64, 0], true));
        let fill_chest = |game: &mut Game<'_>| {
            let Some(super::super::BlockEntity::Chest(items)) =
                game.world.block_entities.get_mut(&(0, 64, 0))
            else {
                panic!("chest block entity missing")
            };
            items[0] = Some(horizon_client::items::ItemStack {
                item: "minecraft:diamond".into(),
                count: 3,
                damage: 0,
                extra: None,
            });
        };
        assert!(game.native_level_set_block(level, [0, 64, 0], chest, flags));
        fill_chest(&mut game);
        assert!(game.native_level_set_block(level, [0, 64, 0], air, flags));
        assert_eq!(
            game.dropped
                .iter()
                .filter(|item| item.stack.item.as_ref() == "minecraft:diamond")
                .map(|item| item.stack.count)
                .sum::<u32>(),
            3
        );
        game.dropped.clear();
        assert!(game.native_level_set_block(level, [0, 64, 0], chest, flags));
        fill_chest(&mut game);
        assert!(game.native_level_set_block(
            level,
            [0, 64, 0],
            air,
            NativeBlockUpdateFlags::SKIP_BLOCK_ENTITY_SIDE_EFFECTS
        ));
        assert!(!game.world.block_entities.contains_key(&(0, 64, 0)));
        assert!(game.dropped.is_empty());
        assert!(game.native_level_set_block(level, [0, 64, 0], stone, flags));
        assert!(game.native_level_destroy_block(level, [0, 64, 0], false));
        assert!(game.dropped.is_empty());
        assert_eq!(game.world.block(0, 64, 0), air);
        assert!(game.native_level_set_block(level, [0, 64, 0], stone, flags));
        assert!(game.native_level_destroy_block(level, [0, 64, 0], true));
        assert!(
            game.dropped
                .iter()
                .any(|item| item.stack.item.as_ref() == "minecraft:cobblestone")
        );
        assert!(!game.native_level_destroy_block(level, [0, 64, 0], true));
        game.dropped.clear();
        let copper = game
            .pack
            .assets
            .registry
            .default_state("minecraft:copper_chest")
            .unwrap();
        let exposed = game
            .pack
            .assets
            .registry
            .default_state("minecraft:exposed_copper_chest")
            .unwrap();
        assert!(game.native_level_set_block(level, [0, 64, 0], copper, flags));
        fill_chest(&mut game);
        assert!(game.native_level_set_block(level, [0, 64, 0], exposed, flags));
        let Some(super::super::BlockEntity::Chest(items)) =
            game.world.block_entities.get(&(0, 64, 0))
        else {
            panic!("copper chest entity missing")
        };
        assert_eq!(items[0].as_ref().unwrap().count, 3);
        assert!(game.dropped.is_empty());
        let sensor = game
            .pack
            .assets
            .registry
            .default_state("minecraft:sculk_sensor")
            .unwrap();
        assert!(game.native_level_set_block(level, [0, 64, 0], sensor, flags));
        assert!(matches!(
            game.world.block_entities.get(&(0, 64, 0)),
            Some(super::super::BlockEntity::Sculk(_))
        ));
        assert!(game.native_level_set_block(level, [0, 64, 0], chest, flags));
        assert!(matches!(
            game.world.block_entities.get(&(0, 64, 0)),
            Some(super::super::BlockEntity::Chest(_))
        ));
        assert!(
            game.enter_command_level(super::super::Dim::Nether)
                .is_some()
        );
        assert!(!game.native_level_set_block(level, [0, 64, 0], stone, flags));
        assert!(!game.native_level_destroy_block(level, [0, 64, 0], true));
    }
}

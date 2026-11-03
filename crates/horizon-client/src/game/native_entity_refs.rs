//! Entity and level handles for native server mods (docs/native-mods.md,
//! "Entity and level handles", #7387): `native_entity(id)` turns an event's
//! entity id into an `EntityRef`, and each `native_entity_*` helper reads the
//! entity a handle names. They read where the world is simulated: on a client,
//! with no world open, or for a handle of an earlier open, they return `None`.
//! The entities are those of the level in place (the one a hook runs in) and
//! the server's players, whatever their level.
use super::{
    Dim, Game,
    commands::Target,
    entity::{Entity as _, entity_lists_of, for_each_entity},
};
use horizon_client::mod_api::{EntityRef, LevelRef};
use horizon_core::geometry::{Aabb, Vec3};

impl Game<'_> {
    /// What a handle names now, or None for a stale or foreign one.
    pub(super) fn native_entity_target(&self, entity: EntityRef) -> Option<Target> {
        if self.remote.is_some() || self.native_lifecycle.epoch() != Some(entity.epoch()) {
            return None;
        }
        let id = i32::try_from(entity.id()).ok()?;
        if id == 0 {
            return None;
        }
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
    /// Entities whose bounding boxes intersect this box (`Level.getEntities`).
    /// Face-only contact does not intersect; spectator players are excluded.
    /// Returns None on a client, for a stale handle, or for a parked level:
    /// its entity handles cannot resolve until the level is in place. The walk
    /// is in entity-list order followed by players; each overlap requires a
    /// handle lookup, so cost grows with the box's contents. Tridents cannot
    /// yet resolve; frames, paintings, other special entities and the dragon
    /// are not on the entity-list walk.
    /// During a player's action with the hub taken out, players including the
    /// acting player are not listed: their handles cannot resolve there yet.
    /// During `hears_projectile_impacts`, the projectile list is taken out of
    /// the game and is not visible; it is restored for `projectile_impact`.
    /// API @API:level_entities_in@.
    pub(super) fn native_level_entities_in(
        &self,
        level: LevelRef,
        min: [f64; 3],
        max: [f64; 3],
    ) -> Option<Vec<EntityRef>> {
        if self.remote.is_some()
            || self.native_lifecycle.epoch() != Some(level.epoch())
            || level.dimension() != self.dim.id()
        {
            return None;
        }
        let bounds = Aabb::new(
            Vec3::new(min[0], min[1], min[2]),
            Vec3::new(max[0], max[1], max[2]),
        );
        let mut ids = Vec::new();
        for_each_entity!(entity_lists_of!(self), entity => {
            // The join flush assigns zero IDs later; a handle could alias another entity.
            if entity.id() != 0 && !entity.is_removed() && bounds.intersects(entity.bounds()) {
                ids.push(entity.id());
            }
        });
        for guest in self.lan.iter().flat_map(|h| &h.hub.guests) {
            if !guest.placed || guest.dimension != level.dimension() {
                continue;
            }
            let body = if self.lan_acting == Some(guest.conn)
                && let Some(player) = &self.player
            {
                if self.mode_id() == 3 {
                    continue;
                }
                player.bounds()
            } else {
                if guest.game_mode == 3 {
                    continue;
                }
                guest.bounds()
            };
            if bounds.intersects(body)
                && let Ok(id) = u32::try_from(guest.entity_id)
            {
                ids.push(id);
            }
        }
        Some(
            ids.into_iter()
                .map(|id| EntityRef::new(level.epoch(), id))
                .filter(|handle| self.native_entity_target(*handle).is_some())
                .collect(),
        )
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
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn entities_in_returns_readable_handles_and_intersects_boxes() {
        use super::super::{
            Audio, Game, Options, Progress, Settings, execution_role::ExecutionRole, load_resources,
        };
        use horizon_client::entities::ArmorStand;
        use horizon_client::mod_api::{LevelRef, NativeAuthority, NativeWorldEpoch};
        use horizon_core::geometry::Vec3;

        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = ExecutionRole::IntegratedServer;
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
        game.native_lifecycle
            .begin_open(NativeAuthority::IntegratedServer);
        let level = game.native_level(game.dim.id()).unwrap();
        game.armor_stands
            .push(ArmorStand::new(0, Vec3::new(0.5, 0.0, 0.0), 0.0));
        assert!(game.native_entity(0).is_none());
        game.armor_stands
            .push(ArmorStand::new(12, Vec3::new(1.0, 0.0, 0.0), 0.0));
        let found = game
            .native_level_entities_in(level, [0.0, 0.0, -1.0], [0.8, 1.0, 1.0])
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(game.native_entity_position(found[0]), Some([1.0, 0.0, 0.0]));
        assert_eq!(
            game.native_entity_kind(found[0]).as_deref(),
            Some("minecraft:armor_stand")
        );
        assert!(
            game.native_level_entities_in(level, [0.0, 0.0, -1.0], [0.75, 1.0, 1.0])
                .unwrap()
                .is_empty()
        );
        assert!(
            game.native_level_entities_in(
                LevelRef::new(NativeWorldEpoch::new(0), game.dim.id()),
                [0.0; 3],
                [2.0; 3]
            )
            .is_none()
        );
    }

    #[test]
    fn uuid_words_are_most_significant_first() {
        let words = [0x0102_0304, -1, 0, 0x7fff_ffff];
        assert_eq!(
            super::uuid_from_words(words),
            0x0102_0304_ffff_ffff_0000_0000_7fff_ffff
        );
    }
}

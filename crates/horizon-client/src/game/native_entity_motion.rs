//! An entity's motion and shape for native server mods, through an
//! `EntityRef` (docs/modding/src/how-to/native-mods.md, "Entity and level handles", #7387):
//! velocity, ground, box, eyes and view. They read the entity a handle names
//! as `native_entity_refs.rs` finds it, and return `None` (or `false`) for a
//! stale handle. On a client (API @API:client_entity_refs@) they read the
//! entities the server shows the window, and nothing moves one.
use super::entity::{Entity as _, EntityHandle, with_entity, with_entity_mut};
use super::{Game, commands::Target};
use horizon_core::geometry::{Aabb, Axis, Vec3};
use mod_api::EntityRef;

fn corners(b: Aabb) -> [[f64; 3]; 2] {
    let axes = [Axis::X, Axis::Y, Axis::Z];
    [axes.map(|a| b.min(a)), axes.map(|a| b.max(a))]
}

impl Game<'_> {
    /// The simulated entity (one of the shared entity walk's lists) with `id`.
    pub(super) fn native_walk_entity(&self, id: u32) -> Option<EntityHandle> {
        self.entity_handles()
            .find(|h| with_entity!(self, *h, e => e.id() == id))
    }
    /// This window's own player, when a client's target names it.
    fn native_client_player(&self, t: Target) -> Option<&horizon_client::motion::PlayerMotion> {
        self.remote.as_ref()?;
        (t == Target::Player).then_some(())?;
        self.player.as_ref()
    }
    /// The server's copy of the player a target names.
    pub(super) fn native_target_guest(
        &self,
        t: Target,
    ) -> Option<&horizon_client::lan::guest::Guest> {
        match t {
            Target::Player => self.command_guest(self.lan_acting?),
            Target::Guest(conn) => self.command_guest(conn),
            _ => None,
        }
    }
    /// The entity's velocity in blocks a tick (`Entity.getDeltaMovement`). A
    /// player's is the move its client last reported (`getKnownMovement`); an
    /// entity Horizon doesn't simulate (a display, a painting) has none, so
    /// zero. On a client, another entity's is the motion the server last
    /// sent it (`Entity.lerpMotion`). API 379.
    pub(super) fn native_entity_velocity(&self, entity: EntityRef) -> Option<[f64; 3]> {
        let t = self.native_entity_read_target(entity)?;
        if !self.is_logical_server() {
            // The client's copy: its own player's motion, and the motion the
            // server sent for an entity it tracks.
            let v = match t {
                Target::Player => self.player.as_ref()?.velocity,
                _ => match self.native_walk_entity(entity.id()) {
                    Some(h) => with_entity!(self, h, e => e.velocity()),
                    None => Vec3::ZERO,
                },
            };
            return Some([v.x, v.y, v.z]);
        }
        if let Some(g) = self.native_target_guest(t) {
            return Some(std::array::from_fn(|i| {
                g.position[i] - g.previous_position[i]
            }));
        }
        let v = match self.native_walk_entity(entity.id()) {
            Some(h) => with_entity!(self, h, e => e.velocity()),
            None => Vec3::ZERO,
        };
        Some([v.x, v.y, v.z])
    }
    /// `Entity.getKnownSpeed`, in blocks a tick: a player's reported move; a
    /// vehicle with a live controlling player, that player's; else what the
    /// entity moved in its last tick (`computeSpeed`: position minus the
    /// previous tick's). Every list the walk keeps has a previous position.
    /// A frame, painting, crystal, spawner, cloud, fangs or marker never
    /// moves, so zero; the dragon uses its own previous position.
    pub(super) fn native_entity_known_speed(&self, entity: EntityRef) -> Option<Vec3> {
        let t = self.native_entity_target(entity)?;
        if let Some(g) = self.native_target_guest(t) {
            let [x, y, z] = g.known_movement;
            return Some(Vec3::new(x, y, z));
        }
        if matches!(t, Target::Player) {
            return Some(self.known_player_movement());
        }
        if let Some(rider) = self.native_controlling_player(t, entity) {
            return self.native_entity_known_speed(rider);
        }
        if let Target::Dragon(_) = t {
            let d = self.dragon.as_ref()?;
            return Some(d.position.subtract(d.previous));
        }
        Some(match self.native_walk_entity(entity.id()) {
            Some(h) => with_entity!(self, h, e => e.position().subtract(e.previous_position())),
            None => Vec3::ZERO,
        })
    }
    /// Whether `Entity.getControllingPassenger` can be a player: a boat's
    /// first passenger, or a mount's rider as `equine::controlled` decides
    /// (saddled; a pig or strider's rider holding the stick; a harnessed
    /// happy ghast), the rule the mount code steers by.
    fn native_steered(&self, t: Target) -> bool {
        match t {
            Target::Boat(_) => true,
            Target::Mob(id) => self
                .mobs
                .iter()
                .any(|m| m.id == id && horizon_client::mobs::equine::controlled(m)),
            _ => false,
        }
    }
    /// `Entity.getControllingPassenger` when it is a player.
    fn native_controlling_player(&self, t: Target, entity: EntityRef) -> Option<EntityRef> {
        let steered = self.native_steered(t);
        let first = *self
            .native_entity_passengers(entity)
            .ok_or(())
            .ok()?
            .first()?;
        let player = matches!(
            self.native_entity_target(first)?,
            Target::Player | Target::Guest(_)
        );
        (steered && player).then_some(first)
    }
    /// Sets the entity's velocity (`Entity.setDeltaMovement`); clients see it
    /// as they see the entity move. False for a player (its client moves
    /// it), an entity Horizon doesn't simulate, a value that isn't finite,
    /// or on a client. API 379.
    pub(super) fn native_entity_set_velocity(
        &mut self,
        entity: EntityRef,
        velocity: [f64; 3],
    ) -> bool {
        if self.remote.is_some()
            || !velocity.iter().all(|v| v.is_finite())
            || self.native_entity_target(entity).is_none()
        {
            return false;
        }
        let Some(h) = self.native_walk_entity(entity.id()) else {
            return false;
        };
        let [x, y, z] = velocity;
        with_entity_mut!(self, h, e => *e.velocity_mut() = Vec3::new(x, y, z));
        if let EntityHandle::Mob(i) = h {
            self.mobs[i].needs_sync = true;
        }
        true
    }
    /// `Entity.resetFallDistance`: how far the entity has fallen is forgotten, so its next
    /// landing hurts it as a step does (a fan's or a launcher's push, a mod's soft block).
    /// Players too, the acting one and guests. False for a stale handle or an entity not in
    /// the level in place; true for a kind that keeps no fall distance, which has none to forget.
    pub(super) fn native_entity_reset_fall_distance(&mut self, entity: EntityRef) -> bool {
        let Some(target) = self.native_entity_target(entity) else {
            return false;
        };
        if matches!(target, Target::Player | Target::Guest(_)) {
            let motion = match target {
                Target::Player => self.player.as_mut(),
                Target::Guest(conn) => self
                    .lan
                    .as_mut()
                    .and_then(|host| host.hub.guest_mut(conn))
                    .and_then(|g| g.motion.as_deref_mut()),
                _ => None,
            };
            let Some(motion) = motion else {
                return false;
            };
            motion.fall_distance = 0.0;
            return true;
        }
        let Some(h) = self.native_walk_entity(entity.id()) else {
            return false;
        };
        with_entity_mut!(self, h, e => e.reset_fall_distance());
        true
    }
    /// Whether the entity stands on a block (`Entity.onGround`).
    /// Whether the entity stands on a block (`Entity.onGround`). API
    /// 379.
    pub(super) fn native_entity_on_ground(&self, entity: EntityRef) -> Option<bool> {
        let t = self.native_entity_target(entity)?;
        if let Some(p) = self.native_client_player(t) {
            return Some(p.ground.on_ground);
        }
        if let Some(g) = self.native_target_guest(t) {
            return Some(g.on_ground);
        }
        Some(
            self.native_walk_entity(entity.id())
                .is_some_and(|h| with_entity!(self, h, e => e.on_ground())),
        )
    }
    /// The entity's box (`Entity.getBoundingBox`): its lowest and highest
    /// corner. None for the kinds whose box Horizon doesn't keep (displays,
    /// markers, End crystals, the dragon). API 379.
    pub(super) fn native_entity_bounding_box(&self, entity: EntityRef) -> Option<[[f64; 3]; 2]> {
        let t = self.native_entity_read_target(entity)?;
        if let Some(p) = self.native_client_player(t) {
            return Some(corners(p.bounds()));
        }
        if let Some(g) = self.native_target_guest(t) {
            return Some(corners(g.bounds()));
        }
        if let Some(h) = self.native_walk_entity(entity.id()) {
            return Some(corners(with_entity!(self, h, e => e.bounds())));
        }
        let (lo, hi) = match t {
            Target::Frame(id) => self.item_frames.iter().find(|f| f.id == id)?.bounds(),
            Target::Painting(id) => self.paintings.iter().find(|p| p.id == id)?.bounds(),
            _ => return None,
        };
        Some([lo, hi])
    }
    /// The height of the entity's eyes above its feet (`Entity.getEyeHeight`).
    /// API 379.
    pub(super) fn native_entity_eye_height(&self, entity: EntityRef) -> Option<f64> {
        Some(self.target_eye_height(self.native_entity_target(entity)?))
    }
    /// Where the entity's eyes are (`Entity.getEyePosition`). API
    /// 379.
    pub(super) fn native_entity_eye_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        let [x, y, z] = self.native_entity_position(entity)?;
        Some([x, y + self.native_entity_eye_height(entity)?, z])
    }
    /// The unit vector the entity looks along (`Entity.getViewVector`, from
    /// its pitch and yaw). API 379.
    pub(super) fn native_entity_view_vector(&self, entity: EntityRef) -> Option<[f64; 3]> {
        let [yaw, pitch] = self.native_entity_rotation(entity)?;
        let v = horizon_world::view_direction::view_vector(pitch, yaw);
        Some([v.x, v.y, v.z])
    }
    /// The square of the distance from the entity's feet to `pos`
    /// (`Entity.distanceToSqr`). API 379.
    pub(super) fn native_entity_distance_to_sqr(
        &self,
        entity: EntityRef,
        pos: [f64; 3],
    ) -> Option<f64> {
        let p = self.native_entity_position(entity)?;
        let [dx, dy, dz] = std::array::from_fn(|i| p[i] - pos[i]);
        Some(dx * dx + dy * dy + dz * dz)
    }
}

#[cfg(test)]
mod tests {
    use horizon_core::geometry::{Aabb, Vec3};

    #[test]
    fn corners_are_lowest_then_highest() {
        let b = Aabb::new(Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0));
        assert_eq!(super::corners(b), [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn reset_fall_distance_forgets_a_fall_for_mobs_carts_and_players() {
        use super::super::minecarts::{CartKind, Minecart};
        use horizon_client::mobs::Mob;
        fn fake_fall<'a>(game: &'a mut super::super::Game<'_>) -> &'a mut f64 {
            let host = game.lan.as_mut().unwrap();
            let g = host.hub.guests.iter_mut().find(|g| g.uuid == 2).unwrap();
            &mut g
                .motion
                .as_deref_mut()
                .expect("a fake has a motion")
                .fall_distance
        }
        super::super::native_entity_refs::tests::with_game(|game| {
            super::super::bossbars::audience_tests::add_real_and_fake(game);
            game.options.dedicated = Some(Vec::new());
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            let mut zombie = Mob::new(40, kind, Vec3::ZERO, 0.0);
            zombie.fall_distance = 12.0;
            game.mobs.push(zombie);
            let mut cart = Minecart::new(41, CartKind::Rideable, Vec3::ZERO);
            cart.fall_distance = 7.0;
            game.carts.push(cart);
            let (zombie, cart) = (
                game.native_entity(40).unwrap(),
                game.native_entity(41).unwrap(),
            );
            assert!(game.native_entity_reset_fall_distance(zombie));
            assert!(game.native_entity_reset_fall_distance(cart));
            assert_eq!(game.mobs[0].fall_distance, 0.0);
            assert_eq!(game.carts[0].fall_distance, 0.0);
            // A player: the fake guest's own motion.
            let level = game.native_level(game.dim.id()).unwrap();
            let fake = game.native_fake_player(level, 2, "fake").unwrap().entity();
            *fake_fall(game) = 9.0;
            assert!(game.native_entity_reset_fall_distance(fake));
            assert_eq!(*fake_fall(game), 0.0);
            // An entity that is gone: nothing.
            game.mobs.clear();
            assert!(!game.native_entity_reset_fall_distance(zombie));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn only_a_mount_its_rider_controls_takes_the_riders_speed() {
        use super::super::commands::Target;
        use horizon_client::mobs::{Mob, equine::Rider};
        super::super::native_entity_refs::tests::with_game(|game| {
            for (id, name) in [(30, "minecraft:horse"), (31, "minecraft:pig")] {
                let kind = game.res.mob_types().get(name).unwrap().clone();
                let mut m = Mob::new(id, kind, Vec3::ZERO, 0.0);
                m.rider = Some(Rider::default());
                game.mobs.push(m);
            }
            // A horse being broken in, unsaddled: its own speed (AbstractHorse.java:962).
            assert!(!game.native_steered(Target::Mob(30)));
            game.mobs[0].extra.set_saddled(true);
            assert!(game.native_steered(Target::Mob(30)));
            // A saddled pig whose rider holds no stick (Pig.java:97-100).
            game.mobs[1].extra.set_saddled(true);
            assert!(!game.native_steered(Target::Mob(31)));
            game.mobs[1].rider.as_mut().unwrap().steer = true;
            assert!(game.native_steered(Target::Mob(31)));
        });
    }
}

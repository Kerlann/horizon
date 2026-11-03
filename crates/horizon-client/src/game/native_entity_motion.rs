//! An entity's motion and shape for native server mods, through an
//! `EntityRef` (docs/native-mods.md, "Entity and level handles", #7387):
//! velocity, ground, box, eyes and view. They read the entity a handle names
//! as `native_entity_refs.rs` finds it, and return `None` (or `false`) for a
//! stale handle or on a client.
use super::entity::{Entity as _, EntityHandle, with_entity, with_entity_mut};
use super::{Game, commands::Target};
use horizon_client::mod_api::EntityRef;
use horizon_core::geometry::{Aabb, Axis, Vec3};

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
    /// zero.
    pub(super) fn native_entity_velocity(&self, entity: EntityRef) -> Option<[f64; 3]> {
        let t = self.native_entity_target(entity)?;
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
    /// it), an entity Horizon doesn't simulate, or a value that isn't finite.
    pub(super) fn native_entity_set_velocity(
        &mut self,
        entity: EntityRef,
        velocity: [f64; 3],
    ) -> bool {
        if !velocity.iter().all(|v| v.is_finite()) || self.native_entity_target(entity).is_none() {
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
    /// Whether the entity stands on a block (`Entity.onGround`).
    pub(super) fn native_entity_on_ground(&self, entity: EntityRef) -> Option<bool> {
        let t = self.native_entity_target(entity)?;
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
    /// markers, End crystals, the dragon).
    pub(super) fn native_entity_bounding_box(&self, entity: EntityRef) -> Option<[[f64; 3]; 2]> {
        let t = self.native_entity_target(entity)?;
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
    pub(super) fn native_entity_eye_height(&self, entity: EntityRef) -> Option<f64> {
        Some(self.target_eye_height(self.native_entity_target(entity)?))
    }
    /// Where the entity's eyes are (`Entity.getEyePosition`).
    pub(super) fn native_entity_eye_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        let [x, y, z] = self.native_entity_position(entity)?;
        Some([x, y + self.native_entity_eye_height(entity)?, z])
    }
    /// The unit vector the entity looks along (`Entity.getViewVector`, from
    /// its pitch and yaw).
    pub(super) fn native_entity_view_vector(&self, entity: EntityRef) -> Option<[f64; 3]> {
        let [yaw, pitch] = self.native_entity_rotation(entity)?;
        let v = horizon_world::view_direction::view_vector(pitch, yaw);
        Some([v.x, v.y, v.z])
    }
    /// The square of the distance from the entity's feet to `pos`
    /// (`Entity.distanceToSqr`).
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
            game.mobs[0].extra.saddled = true;
            assert!(game.native_steered(Target::Mob(30)));
            // A saddled pig whose rider holds no stick (Pig.java:97-100).
            game.mobs[1].extra.saddled = true;
            assert!(!game.native_steered(Target::Mob(31)));
            game.mobs[1].rider.as_mut().unwrap().steer = true;
            assert!(game.native_steered(Target::Mob(31)));
        });
    }
}

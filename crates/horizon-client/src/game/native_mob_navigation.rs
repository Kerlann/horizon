//! Navigation helpers for native mod mob handles.
use super::{Game, commands::Target};
use mod_api::MobRef;

/// The common boundary for mod-supplied navigation and look-control coordinates.
pub(super) fn safe_control_position(at: [f64; 3]) -> bool {
    at.into_iter()
        .all(|v| v.is_finite() && v.abs() <= 2_000_000_000.0)
}

impl Game<'_> {
    /// `PathNavigation.moveTo(x, y, z, speed)`: true if a path was started.
    /// Refuses a stale or unavailable mob, a client, non-finite arguments,
    /// and coordinates too large for safe block arithmetic. As in vanilla,
    /// the mob's own goals may replace the path next tick. False for the
    /// dragon, which flies by its phase and keeps no navigation.
    pub(super) fn native_mob_move_to(&mut self, mob: MobRef, at: [f64; 3], speed: f64) -> bool {
        if !speed.is_finite() || !safe_control_position(at) {
            return false;
        }
        let Some(Target::Mob(id)) = self.native_entity_target(mob.entity()) else {
            return false;
        };
        let Some(index) = self.mobs.iter().position(|m| m.id == id) else {
            return false;
        };
        // The pathfinder reads terrain only. Take the mob list so its selected
        // mob can be mutably borrowed alongside the shared MobWorld, just as
        // the mob tick does. No mod hook fires during pathfinding.
        let mut mobs = std::mem::take(&mut self.mobs);
        let moved = mobs[index].nav_move_to(&self.mob_world_base(&[]), at, speed);
        self.mobs = mobs;
        moved
    }

    /// `PathNavigation.stop`. False for a stale or unavailable mob, or on a
    /// client, and for the dragon (no navigation).
    pub(super) fn native_mob_stop(&mut self, mob: MobRef) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(mob.entity()) else {
            return false;
        };
        let Some(mob) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return false;
        };
        mob.nav_stop();
        true
    }

    /// `PathNavigation.isDone`; None for a stale or unavailable mob, or on a
    /// client, and for the dragon (no navigation).
    pub(super) fn native_mob_is_done(&self, mob: MobRef) -> Option<bool> {
        let Target::Mob(id) = self.native_entity_target(mob.entity())? else {
            return None;
        };
        Some(self.mobs.iter().find(|m| m.id == id)?.ai.nav.is_done())
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use horizon_client::{chunk::Chunk, mobs::Mob};
    use horizon_core::geometry::Vec3;
    use mod_api::{EntityRef, MobRef, NativeWorldEpoch};
    use std::sync::Arc;

    #[test]
    #[ignore = "needs prepared resources"]
    fn navigation_starts_a_real_path_and_refuses_stale_handles() {
        with_game(|game| {
            game.world
                .chunks
                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
            let stone = game
                .pack
                .assets
                .registry
                .default_state("minecraft:stone")
                .unwrap();
            for x in 0..16 {
                for z in 0..16 {
                    game.world.set(x, 64, z, stone).unwrap();
                }
            }
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            let mut zombie = Mob::new(99, kind, Vec3::new(3.5, 65.0, 3.5), 0.0);
            zombie.on_ground = true;
            game.mobs.push(zombie);
            let entity = game.native_entity(99).unwrap();
            let mob = game.native_mob_ref(entity).unwrap();
            assert_eq!(mob.is_done(game), Some(true));
            assert!(mob.move_to(game, [10.5, 65.0, 3.5], 1.25));
            assert_eq!(mob.is_done(game), Some(false));
            assert_eq!(game.mobs[0].ai.nav.speed_modifier, 1.25);
            assert!(!mob.move_to(game, [f64::NAN, 65.0, 3.5], 1.0));
            assert_eq!(mob.is_done(game), Some(false));
            assert!(mob.stop(game));
            assert_eq!(mob.is_done(game), Some(true));
            assert!(!mob.move_to(game, [1000.0, 65.0, 0.0], 1.0));
            let compare_extreme =
                |game: &mut super::super::Game<'_>, extreme: f64, boundary: f64| {
                    let result = mob.move_to(game, [10.5, boundary, 3.5], 1.0);
                    let path = game.mobs[0].ai.nav.path.clone();
                    mob.stop(game);
                    assert_eq!(mob.move_to(game, [10.5, extreme, 3.5], 1.0), result);
                    assert_eq!(game.mobs[0].ai.nav.path, path);
                    mob.stop(game);
                };
            let top = f64::from(game.world.top_y() - 1);
            let bottom = f64::from(game.world.bottom_y());
            compare_extreme(game, 2_000_000_000.0, top);
            compare_extreme(game, -2_000_000_000.0, bottom);
            game.world.shape =
                horizon_client::chunk::LevelShape::new(0, 128, 128, true, false).unwrap();
            assert_eq!(game.world.shape.max_y(), 128);
            compare_extreme(game, 2_000_000_000.0, 127.0);
            compare_extreme(game, -2_000_000_000.0, 0.0);
            game.world.shape = horizon_client::chunk::LevelShape::OVERWORLD;
            let stale = MobRef::new(EntityRef::new(
                NativeWorldEpoch::new(entity.epoch().get() + 1),
                99,
            ));
            assert!(!stale.move_to(game, [5.0, 65.0, 3.0], 1.0));
            assert!(!stale.stop(game));
            assert_eq!(stale.is_done(game), None);
            let taken = std::mem::take(&mut game.mobs);
            assert_eq!(mob.is_done(game), None);
            assert!(!mob.stop(game));
            game.mobs = taken;
            game.armor_stands
                .push(horizon_client::entities::ArmorStand::new(
                    102,
                    Vec3::ZERO,
                    0.0,
                ));
            let wrong = MobRef::new(game.native_entity(102).unwrap());
            assert!(!wrong.move_to(game, [5.0, 65.0, 3.0], 1.0));
            assert!(!wrong.stop(game));
            assert_eq!(wrong.is_done(game), None);
            let parrot = game
                .res
                .mob_types()
                .get("minecraft:parrot")
                .unwrap()
                .clone();
            game.mobs
                .push(Mob::new(100, parrot, Vec3::new(3.5, 66.0, 5.5), 0.0));
            let flying = game
                .native_mob_ref(game.native_entity(100).unwrap())
                .unwrap();
            assert!(flying.move_to(game, [9.5, 66.0, 5.5], 1.0));
            assert_eq!(flying.is_done(game), Some(false));
            let cod = game.res.mob_types().get("minecraft:cod").unwrap().clone();
            game.mobs
                .push(Mob::new(101, cod, Vec3::new(3.5, 65.0, 7.5), 0.0));
            let fish = game
                .native_mob_ref(game.native_entity(101).unwrap())
                .unwrap();
            assert!(!fish.move_to(game, [9.5, 65.0, 7.5], 1.0));
            assert_eq!(fish.is_done(game), Some(true));
        });
    }
}

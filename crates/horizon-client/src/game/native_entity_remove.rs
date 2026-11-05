//! Removing an entity for native server mods, through an `EntityRef`
//! (docs/modding/src/how-to/native-mods.md, "Entity and level handles", #7387): `Entity.discard`
//! and `Entity.remove` with a reason. Nothing dies and nothing drops loot.
//! The handle reads as gone from the call on, as `isRemoved` does. A mob
//! leaves at once; other kinds are marked and announced at once, while their
//! physical cleanup waits for the next sweep so active walks stay valid.
//! A vehicle with a chest spills it, as vanilla's `remove` does.
use super::{Game, commands::Target};
use horizon_core::position::BlockPos;
use mod_api::{EntityRef, NativeRemovalReason};

impl Game<'_> {
    /// Removes the entity with no death and no loot (`Entity.discard`): a
    /// chest boat or a minecart with a chest still drops what it holds, and
    /// a boat its lead, as vanilla's `remove` does. False for a player, or a
    /// stale handle.
    pub(super) fn native_entity_discard(&mut self, entity: EntityRef) -> bool {
        self.native_entity_remove(entity, NativeRemovalReason::Discarded)
    }
    /// Removes the entity for `reason` (`Entity.remove`): `Discarded`, as
    /// `discard`, or `Killed`, which also runs a mob's effects that act on
    /// removal (Oozing's slimes, Weaving's cobwebs, Wind Charged's burst),
    /// still with no death and no loot. False for a player, or a stale
    /// handle.
    pub(super) fn native_entity_remove(
        &mut self,
        entity: EntityRef,
        reason: NativeRemovalReason,
    ) -> bool {
        let killed = reason == NativeRemovalReason::Killed;
        match self.native_entity_target(entity) {
            None | Some(Target::Player | Target::Guest(_)) => false,
            Some(Target::Mob(id)) => {
                self.remove_mob_now(id, killed);
                true
            }
            Some(t) => {
                let passengers = self.native_entity_passengers(entity).unwrap_or_default();
                self.persist.removals.push(t);
                for passenger in passengers {
                    self.native_entity_stop_riding(passenger);
                }
                self.native_entity_left(entity.id());
                true
            }
        }
    }
    /// A mob leaves the level: its effects act first when it was killed.
    fn remove_mob_now(&mut self, id: u32, killed: bool) {
        let Some(i) = self.mobs.iter().position(|m| m.id == id && !m.removed) else {
            return;
        };
        if killed && !self.mobs[i].extra.effects.is_empty() {
            let m = &self.mobs[i];
            let names: Vec<&'static str> = m
                .extra
                .effects
                .iter()
                .map(|e| e.0.trim_start_matches("minecraft:"))
                .collect();
            let (bounds, mut random) = (m.bounds(), m.random.clone());
            self.effects_on_death(&names, bounds, false, Some(id), Some(&mut random));
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                m.random = random;
            }
        }
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            m.removed = true;
        }
    }
    /// `AbstractMinecartContainer.remove` and `AbstractChestBoat.remove`: a
    /// chest or hopper minecart or a chest boat spills what it holds
    /// (`Containers.dropContents`) for every destroying reason.
    fn spill_vehicle(&mut self, t: Target) {
        let id = match t {
            Target::Cart(id) | Target::Boat(id) => id,
            _ => return,
        };
        // What an open menu edited goes back to the vehicle first.
        self.release_vehicle_container(id);
        let held = match t {
            Target::Cart(_) => self
                .carts
                .iter_mut()
                .find(|c| c.id == id)
                .map(|c| (c.position, &mut c.items)),
            _ => self
                .boats
                .iter_mut()
                .find(|b| b.id == id)
                .map(|b| (b.position, &mut b.items)),
        };
        let Some((at, items)) = held else {
            return;
        };
        if items.iter().all(Option::is_none) {
            return;
        }
        let items = std::mem::take(items);
        let at = BlockPos {
            x: at.x.floor() as i32,
            y: at.y.floor() as i32,
            z: at.z.floor() as i32,
        };
        self.with_level(|_, level, _| level.drop_lost_contents(at, items));
        self.flush_spill();
    }
    /// Takes out the entities mods removed since the last sweep.
    pub(super) fn native_removals_flush(&mut self) {
        if self.persist.removals.is_empty() {
            return;
        }
        for t in std::mem::take(&mut self.persist.removals) {
            match t {
                Target::Crystal(id) => self.crystals.retain(|c| c.id != id),
                Target::Dragon(id) => {
                    if self.dragon.as_ref().is_some_and(|d| d.id == id) {
                        self.dragon = None;
                    }
                }
                Target::Player | Target::Guest(_) | Target::Mob(_) => {}
                // The kinds `/kill` already removes with no death.
                t => {
                    self.spill_vehicle(t);
                    self.kill_target_in_level(t);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "needs prepared resources"]
    fn discard_other_clears_nbt_and_saved_entity() {
        use super::super::{commands::OtherEntity, native_entity_refs::tests::with_game};
        use horizon_client::{chunk::Chunk, save::SaveDir};
        use horizon_core::geometry::Vec3;
        use horizon_world::nbt::{Compound, Tag};
        use std::sync::Arc;

        with_game(|game| {
            let root = std::path::PathBuf::from(format!(
                "target/discard-save-test-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            let shape = game.save.shape;
            game.save = Arc::new(SaveDir::new(root.clone(), shape));
            game.world
                .chunks
                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
            game.others.push(OtherEntity {
                id: 12,
                kind: "minecraft:text_display".into(),
                position: Vec3::new(1.0, 70.0, 1.0),
                yaw: 0.0,
                pitch: 0.0,
            });
            let mut tag = Compound::new();
            tag.insert("teleport_duration".into(), Tag::Int(4));
            game.persist.raw.insert(12, Tag::Compound(tag));
            game.save_entities(true).unwrap();
            assert_eq!(
                SaveDir::new(root.clone(), shape).load_entities(0, 0).len(),
                1
            );
            let entity = game.native_entity(12).unwrap();
            let selector = horizon_client::command::args::Selector {
                kind: 'e',
                limit: usize::MAX,
                ..Default::default()
            };
            assert!(game.select(&selector).contains(&super::Target::Other(12)));
            assert!(entity.discard(game));
            assert!(!game.select(&selector).contains(&super::Target::Other(12)));
            assert!(!entity.discard(game));
            game.native_removals_flush();
            assert!(!game.persist.raw.contains_key(&12));
            game.save_entities(true).unwrap();
            assert!(
                SaveDir::new(root.clone(), shape)
                    .load_entities(0, 0)
                    .is_empty()
            );
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    #[ignore = "needs prepared resources"]
    fn discard_mob_and_refuse_players_and_stale_handles() {
        use super::super::native_entity_refs::tests::with_game;
        use horizon_client::{mobs::Mob, motion::PlayerMotion};
        use horizon_core::geometry::Vec3;

        with_game(|game| {
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(12, kind, Vec3::ZERO, 0.0));
            let mob = game.native_entity(12).unwrap();
            assert!(mob.discard(game));
            assert!(game.mobs[0].removed);
            assert!(!mob.discard(game));
            game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
            game.lan_acting_uuid = Some(123);
            game.lan_acting_entity = Some(20);
            let player = game.native_entity(20).unwrap();
            assert!(!player.discard(game));
            assert!(game.player.is_some());
        });
    }

    #[test]
    #[ignore = "needs prepared resources"]
    fn kill_command_forgets_other_entity_data() {
        use super::super::{commands::OtherEntity, native_entity_refs::tests::with_game};
        use horizon_core::geometry::Vec3;
        use horizon_world::nbt::{Compound, Tag};

        with_game(|game| {
            game.others.push(OtherEntity {
                id: 12,
                kind: "minecraft:marker".into(),
                position: Vec3::ZERO,
                yaw: 0.0,
                pitch: 0.0,
            });
            game.persist.raw.insert(12, Tag::Compound(Compound::new()));
            game.ui.registries.permission = 4;
            assert!(game.run_command("kill @e[type=minecraft:marker]"));
            assert!(game.others.is_empty());
            assert!(!game.persist.raw.contains_key(&12));
        });
    }
}

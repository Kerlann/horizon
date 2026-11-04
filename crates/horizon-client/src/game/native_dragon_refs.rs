//! Ender dragon handles for native server mods: the `Mob` state the dragon
//! keeps, its phase and the fight it belongs to.
use super::{
    Dim, Game,
    commands::Target,
    dragon::{Dragon, Fight},
};
use mod_api::{DragonRef, EntityRef, LivingRef, NativeDragonPhase};

impl Game<'_> {
    /// The dragon an entity handle names now.
    pub(super) fn native_dragon(&self, entity: EntityRef) -> Option<&Dragon> { if let Some(__l) = crate::points_generated::P3042.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        let Target::Dragon(id) = self.native_entity_target(entity)? else {
            return None;
        };
        self.dragon.as_ref().filter(|d| d.id == id)
    }
    pub(super) fn native_dragon_mut(&mut self, entity: EntityRef) -> Option<&mut Dragon> { if let Some(__l) = crate::points_generated::P3041.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        let Target::Dragon(id) = self.native_entity_target(entity)? else {
            return None;
        };
        self.dragon.as_mut().filter(|d| d.id == id)
    }

    /// The entity as the ender dragon, or None for another kind, a stale
    /// handle or a client.
    pub(super) fn native_dragon_ref(&self, entity: EntityRef) -> Option<DragonRef> { if let Some(__l) = crate::points_generated::P3040.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.native_dragon(entity)?;
        Some(DragonRef::new(entity))
    }

    /// `EnderDragonPhaseManager.getCurrentPhase`.
    pub(super) fn native_dragon_phase(&self, dragon: DragonRef) -> Option<NativeDragonPhase> { if let Some(__l) = crate::points_generated::P3039.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(self.native_dragon(dragon.entity())?.native_phase())
    }

    /// `EnderDragonPhaseManager.setPhase`: the dragon ends its phase and
    /// begins the new one fresh (targets and counters cleared), and the
    /// clients get the synced `DATA_PHASE`. Setting the phase it holds does
    /// nothing, as in vanilla (a strafing dragon keeps its player). No
    /// random draw. False for a stale handle or a client. The new phase
    /// runs from the dragon's next tick.
    pub(super) fn native_dragon_set_phase(
        &mut self,
        dragon: DragonRef,
        phase: NativeDragonPhase,
    ) -> bool { if let Some(__l) = crate::points_generated::P3038.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        let Some(d) = self.native_dragon_mut(dragon.entity()) else {
            return false;
        };
        d.set_native_phase(phase);
        self.end_dragon_flame();
        true
    }

    /// The dragon's fight: Horizon keeps the End's fight only, so it is the
    /// fight of a dragon in the level in place when that is the End.
    fn native_dragon_fight(&self, dragon: DragonRef) -> Option<&Fight> { if let Some(__l) = crate::points_generated::P3037.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.native_dragon(dragon.entity())?;
        (self.dim == Dim::End && self.fight.loaded).then_some(&self.fight)
    }

    /// `EnderDragonFight.aliveCrystals`: the crystals the last scan found
    /// (every 100 ticks). None for a stale handle, a client, or a dragon
    /// outside the End or whose fight is not loaded.
    pub(super) fn native_dragon_alive_crystals(&self, dragon: DragonRef) -> Option<i32> { if let Some(__l) = crate::points_generated::P3036.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(self.native_dragon_fight(dragon)?.alive_crystals)
    }

    /// `EnderDragonFight.hasPreviouslyKilledDragon`. None as
    /// `alive_crystals` is.
    pub(super) fn native_dragon_previously_killed(&self, dragon: DragonRef) -> Option<bool> { if let Some(__l) = crate::points_generated::P3035.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(self.native_dragon_fight(dragon)?.previously_killed)
    }

    /// `EnderDragon.canAttack` (`canBeSeenAsEnemy`) and `Mob.asValidTarget`
    /// for the targets Horizon models: mobs and players. Armor stands, end
    /// crystals and the like are not supported.
    fn native_dragon_valid_target(&self, target: Target) -> bool { if let Some(__l) = crate::points_generated::P3034.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        match target {
            Target::Mob(id) => self
                .mobs
                .iter()
                .any(|m| m.id == id && !m.removed && m.alive() && !m.invulnerable_flag),
            Target::Player | Target::Guest(_) => self
                .native_player_facts(target)
                .is_some_and(|p| p.alive && !p.invulnerable && !matches!(p.mode, 1 | 3)),
            _ => false,
        }
    }

    /// `Mob.getTarget` for the dragon: the mob or player a mod set, None once
    /// it is no longer a valid target. The dragon's flight never sets or
    /// reads it.
    pub(super) fn native_dragon_target(&self, entity: EntityRef) -> Option<LivingRef> { if let Some(__l) = crate::points_generated::P3033.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        let target = self.native_entity(self.native_dragon(entity)?.target?)?;
        self.native_dragon_valid_target(self.native_entity_target(target)?)
            .then(|| self.native_living(target))?
    }

    /// `Mob.setTarget` for the dragon; None clears it. An invalid target is
    /// refused (false, unchanged) where vanilla stores None. False too for a
    /// stale handle or a target that is not a mob or a player.
    pub(super) fn native_dragon_set_target(
        &mut self,
        entity: EntityRef,
        target: Option<LivingRef>,
    ) -> bool { if let Some(__l) = crate::points_generated::P3032.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        let id = match target {
            None => None,
            Some(target) => {
                let valid = self
                    .native_entity_target(target.entity())
                    .is_some_and(|t| self.native_dragon_valid_target(t));
                if !valid {
                    return false;
                }
                Some(target.entity().id())
            }
        };
        let Some(dragon) = self.native_dragon_mut(entity) else {
            return false;
        };
        dragon.target = id;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::Dim;
    use super::super::native_entity_refs::tests::with_game;
    use super::*;
    use horizon_client::{entities::ArmorStand, mobs::Mob};
    use horizon_core::geometry::Vec3;
    use horizon_protocol::entity_data::{Value, serializer};
    use horizon_world::nbt::Tag;
    use mod_api::{MobRef, NativeWorldEpoch};

    fn dragon_in(game: &mut Game<'_>) -> (EntityRef, DragonRef) {
        let mut dragon = Dragon::new(Vec3::ZERO, 0.0, 0);
        dragon.id = 15;
        game.dragon = Some(dragon);
        let entity = game.native_entity(15).unwrap();
        (entity, game.native_dragon_ref(entity).unwrap())
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn the_dragon_is_a_mob_with_mob_state() {
        with_game(|game| {
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(12, kind.clone(), Vec3::ZERO, 0.0));
            let mut shielded = Mob::new(13, kind, Vec3::ZERO, 0.0);
            shielded.invulnerable_flag = true;
            game.mobs.push(shielded);
            game.armor_stands.push(ArmorStand::new(14, Vec3::ZERO, 0.0));
            let (entity, dragon) = dragon_in(game);
            assert!(
                game.native_dragon_ref(game.native_entity(12).unwrap())
                    .is_none()
            );
            let mob = game.native_mob_ref(entity).unwrap();
            assert_eq!(mob, dragon.mob());
            assert_eq!(mob.living().entity(), entity);

            // Aggression is a synced mob flag; persistence is saved.
            assert_eq!(mob.is_aggressive(game), Some(false));
            assert!(mob.set_aggressive(game, true));
            assert_eq!(mob.is_aggressive(game), Some(true));
            let state = game.lan_dragon_state().unwrap();
            assert!(state.meta.contains(&(15, serializer::BYTE, Value::Byte(4))));
            assert!(mob.set_aggressive(game, false));
            let state = game.lan_dragon_state().unwrap();
            assert!(state.meta.iter().all(|m| m.0 != 15));
            assert_eq!(mob.persistence_required(game), Some(false));
            assert!(mob.set_persistence_required(game));
            assert_eq!(mob.persistence_required(game), Some(true));
            let saved = game.dragon.as_ref().unwrap().to_nbt([0; 4], None);
            assert_eq!(saved.get("PersistenceRequired"), Some(&Tag::Byte(1)));
            assert!(Dragon::from_nbt(&saved, 0).unwrap().persistent);
            // `getBooleanOr` takes any nonzero number as true.
            for (value, want) in [
                (Tag::Byte(2), true),
                (Tag::Int(1), true),
                (Tag::Byte(0), false),
            ] {
                let mut tag = saved.clone();
                if let Tag::Compound(map) = &mut tag {
                    map.insert("PersistenceRequired".into(), value);
                }
                assert_eq!(Dragon::from_nbt(&tag, 0).unwrap().persistent, want);
            }

            // A target a mod set: valid mobs only, and it goes stale.
            let zombie = game.native_living(game.native_entity(12).unwrap()).unwrap();
            let shield = game.native_living(game.native_entity(13).unwrap()).unwrap();
            let stand = game.native_living(game.native_entity(14).unwrap()).unwrap();
            assert_eq!(mob.target(game), None);
            assert!(mob.set_target(game, Some(zombie)));
            assert_eq!(mob.target(game), Some(zombie));
            assert!(!mob.set_target(game, Some(shield)));
            assert!(!mob.set_target(game, Some(stand)));
            assert!(!mob.set_target(game, Some(dragon.mob().living())));
            assert_eq!(mob.target(game), Some(zombie));
            game.mobs[0].invulnerable_flag = true;
            assert_eq!(mob.target(game), None);
            game.mobs[0].invulnerable_flag = false;
            assert_eq!(mob.target(game), Some(zombie));
            assert!(mob.set_target(game, None));
            assert_eq!(mob.target(game), None);

            // No PathNavigation: the dragon flies by its phase.
            assert!(!mob.move_to(game, [1.0, 1.0, 1.0], 1.0));
            assert!(!mob.stop(game));
            assert_eq!(mob.is_done(game), None);

            let stale = MobRef::new(EntityRef::new(
                NativeWorldEpoch::new(entity.epoch().get() + 1),
                15,
            ));
            assert!(!stale.set_aggressive(game, true));
            assert_eq!(stale.persistence_required(game), None);
            assert!(!stale.set_target(game, None));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn the_phase_is_read_set_synced_and_saved() {
        with_game(|game| {
            let (entity, dragon) = dragon_in(game);
            assert_eq!(dragon.phase(game), Some(NativeDragonPhase::HoldingPattern));
            for (i, phase) in [
                NativeDragonPhase::HoldingPattern,
                NativeDragonPhase::StrafePlayer,
                NativeDragonPhase::LandingApproach,
                NativeDragonPhase::Landing,
                NativeDragonPhase::Takeoff,
                NativeDragonPhase::SittingFlaming,
                NativeDragonPhase::SittingScanning,
                NativeDragonPhase::SittingAttacking,
                NativeDragonPhase::ChargingPlayer,
                NativeDragonPhase::Dying,
                NativeDragonPhase::Hover,
            ]
            .into_iter()
            .enumerate()
            {
                assert_eq!(phase.id(), i as i32);
                assert_eq!(NativeDragonPhase::from_id(i as i32), Some(phase));
                assert!(dragon.set_phase(game, phase));
                assert_eq!(dragon.phase(game), Some(phase));
                let state = game.lan_dragon_state().unwrap();
                let synced = state.meta.iter().find(|m| m.0 == 16);
                if phase == NativeDragonPhase::Hover {
                    assert_eq!(synced, None);
                } else {
                    assert_eq!(synced, Some(&(16, serializer::INT, Value::Int(i as i32))));
                }
                let saved = game.dragon.as_ref().unwrap().to_nbt([0; 4], None);
                assert_eq!(saved.get("DragonPhase"), Some(&Tag::Int(i as i32)));
                let loaded = Dragon::from_nbt(&saved, 0).unwrap();
                assert_eq!(loaded.native_phase(), phase);
            }
            assert_eq!(NativeDragonPhase::from_id(11), None);
            assert_eq!(NativeDragonPhase::from_id(-1), None);

            // Setting the phase it holds changes nothing, a strafe included.
            assert!(dragon.set_phase(game, NativeDragonPhase::Hover));
            assert_eq!(dragon.phase(game), Some(NativeDragonPhase::Hover));

            // The fight: unloaded until the End loads it, and only the End's.
            assert_eq!(dragon.alive_crystals(game), None);
            assert_eq!(dragon.previously_killed(game), None);
            game.fight.loaded = true;
            game.fight.alive_crystals = 3;
            game.fight.previously_killed = true;
            assert_eq!(dragon.alive_crystals(game), None, "not in the End");
            assert_eq!(dragon.previously_killed(game), None);
            game.dim = Dim::End;
            assert_eq!(dragon.alive_crystals(game), Some(3));
            assert_eq!(dragon.previously_killed(game), Some(true));

            let stale = DragonRef::new(EntityRef::new(
                NativeWorldEpoch::new(entity.epoch().get() + 1),
                15,
            ));
            assert!(!stale.set_phase(game, NativeDragonPhase::Landing));
            assert_eq!(stale.phase(game), None);
            assert_eq!(stale.alive_crystals(game), None);
            game.dragon = None;
            assert_eq!(dragon.phase(game), None);
            assert!(!dragon.set_phase(game, NativeDragonPhase::Landing));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn leaving_flaming_discards_the_cloud_at_once() {
        use super::super::dragon::Breath;
        with_game(|game| {
            let (_, dragon) = dragon_in(game);
            let cloud = |from_dragon| Breath {
                id: 0,
                at: [0.0; 3],
                radius: 5.0,
                age: 0,
                duration: 200,
                from_dragon,
                victims: Vec::new(),
                potion: None,
            };
            assert!(dragon.set_phase(game, NativeDragonPhase::SittingFlaming));
            game.breath_clouds.push(cloud(true));
            game.breath_clouds.push(cloud(false));
            assert!(dragon.set_phase(game, NativeDragonPhase::Hover));
            assert_eq!(game.breath_clouds.len(), 1, "the flame cloud is gone");
            assert!(!game.breath_clouds[0].from_dragon);
            // Back in and out within one hook leaves nothing of the old cloud.
            assert!(dragon.set_phase(game, NativeDragonPhase::SittingFlaming));
            game.breath_clouds.push(cloud(true));
            assert!(dragon.set_phase(game, NativeDragonPhase::Hover));
            assert!(dragon.set_phase(game, NativeDragonPhase::SittingFlaming));
            assert_eq!(game.breath_clouds.len(), 1);
            // The gameplay path leaves the flag for the tick.
            game.breath_clouds.push(cloud(true));
            game.dragon
                .as_mut()
                .unwrap()
                .set_native_phase(NativeDragonPhase::Hover);
            game.dragon
                .as_mut()
                .unwrap()
                .set_native_phase(NativeDragonPhase::SittingFlaming);
            game.tick_breath();
            assert!(game.breath_clouds.iter().all(|c| !c.from_dragon));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn an_invulnerable_player_is_not_a_target() {
        with_game(|game| {
            let (entity, _) = dragon_in(game);
            let mob = game.native_mob_ref(entity).unwrap();
            game.player =
                Some(horizon_client::motion::PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
            game.lan_acting_uuid = Some(7);
            game.lan_acting_entity = Some(30);
            let target = game.native_entity(30).unwrap();
            let living = game.native_living(target).unwrap();
            assert!(mob.set_target(game, Some(living)));
            game.player.as_mut().unwrap().invulnerable = true;
            assert_eq!(mob.target(game), None);
            assert!(!mob.set_target(game, Some(living)));
        });
    }
}

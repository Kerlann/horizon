//! An entity's state flags for native server mods, through an `EntityRef`
//! (docs/modding/src/how-to/native-mods.md, "Entity and level handles", #7387): sneaking and
//! sprinting, water and lava, fire and air, silence, invisibility and pose.
//! Players and mobs have them all; the other kinds have the ones Horizon
//! keeps for them (an armor stand's fire and invisibility, an item's fire),
//! and `None` for the rest, as for a stale handle. Secondary use also reads
//! the local client's live semantic sneak state; the other flags are server-only.
use super::{Game, commands::Target};
use horizon_client::mobs::{Mob, Species, sync};
use horizon_client::motion::PlayerMotion;
use horizon_client::pose::Pose;
use mod_api::EntityRef;

/// `Pose`'s serialized names by id (`Pose.STANDING` is 0).
const POSES: [&str; 18] = [
    "standing",
    "fall_flying",
    "sleeping",
    "swimming",
    "spin_attack",
    "crouching",
    "long_jumping",
    "dying",
    "croaking",
    "using_tongue",
    "sitting",
    "roaring",
    "sniffing",
    "emerging",
    "digging",
    "sliding",
    "shooting",
    "inhaling",
];

/// `Entity.getMaxAirSupply`: a dolphin's 4800, an axolotl's 6000, else 300.
fn max_air(species: Option<Species>) -> i32 {
    match species {
        Some(Species::Dolphin) => 4800,
        Some(Species::Axolotl) => horizon_client::mobs::axolotl::MAX_AIR,
        _ => 300,
    }
}

/// A player's pose as `Pose` names it.
fn player_pose(pose: Pose) -> &'static str {
    match pose {
        Pose::Standing => "standing",
        Pose::Crouching => "crouching",
        Pose::Swimming => "swimming",
        Pose::FallFlying => "fall_flying",
        Pose::SpinAttack => "spin_attack",
        Pose::Sleeping => "sleeping",
    }
}

/// What a handle names, as the flags read it.
enum Body<'a> {
    Player(&'a PlayerMotion),
    Mob(&'a Mob),
    Other(Target),
}

impl Game<'_> {
    /// The player's body or the mob a handle names, or its target.
    fn native_entity_body(&self, entity: EntityRef) -> Option<Body<'_>> {
        Some(match self.native_entity_target(entity)? {
            Target::Player => Body::Player(self.player.as_ref()?),
            Target::Guest(conn) => Body::Player(self.command_guest(conn)?.motion.as_deref()?),
            Target::Mob(id) => Body::Mob(self.mobs.iter().find(|m| m.id == id)?),
            t => Body::Other(t),
        })
    }
    /// `isSecondaryUseActive`: the local client's live semantic sneak state (not the
    /// physical key binding or last tick's crouching pose); the server's shift-key flag.
    /// False for non-player entities, None for a stale or unreachable handle.
    pub(super) fn native_entity_is_shift_key_down(&self, entity: EntityRef) -> Option<bool> {
        if !self.is_logical_server() {
            return Some(match self.native_client_entity_target(entity)? {
                Target::Player => self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft),
                _ => false,
            });
        }
        Some(match self.native_entity_body(entity)? {
            Body::Player(p) => p.shift_key_down,
            _ => false,
        })
    }
    /// Whether the entity crouches (`Entity.isCrouching`: the crouching
    /// pose).
    pub(super) fn native_entity_is_crouching(&self, entity: EntityRef) -> Option<bool> {
        Some(self.native_entity_pose(entity)? == "crouching")
    }
    /// Whether the entity sprints (`Entity.isSprinting`): a player, or a
    /// cat or ocelot dashing after its target (their shared sprinting flag);
    /// false for the other kinds.
    pub(super) fn native_entity_is_sprinting(&self, entity: EntityRef) -> Option<bool> {
        Some(match self.native_entity_body(entity)? {
            Body::Player(p) => p.sprinting,
            Body::Mob(m) => m.extra.sprinting,
            _ => false,
        })
    }
    /// Whether the entity touched water at its last fluid update
    /// (`Entity.isInWater`): players and mobs.
    pub(super) fn native_entity_is_in_water(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Player(p) => Some(p.fluids.was_touching_water),
            Body::Mob(m) => Some(m.fluids.in_water),
            Body::Other(_) => None,
        }
    }
    /// Whether the entity is in lava (`Entity.isInLava`): players and mobs.
    pub(super) fn native_entity_is_in_lava(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Player(p) => Some(p.is_in_lava()),
            Body::Mob(m) => Some(m.fluids.in_lava),
            Body::Other(_) => None,
        }
    }
    /// Whether the entity's type never burns (`EntityType.fireImmune`):
    /// players and mobs.
    pub(super) fn native_entity_fire_immune(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Player(_) => Some(false),
            Body::Mob(m) => Some(horizon_client::mobs::spawn_rules::fire_immune(m.species())),
            Body::Other(_) => None,
        }
    }
    /// The ticks the entity burns for (`Entity.getRemainingFireTicks`):
    /// players, mobs, armor stands and items.
    pub(super) fn native_entity_fire_ticks(&self, entity: EntityRef) -> Option<i32> {
        match self.native_entity_body(entity)? {
            Body::Player(p) => Some(p.fire_ticks),
            Body::Mob(m) => Some(m.fire_ticks),
            Body::Other(Target::Stand(id)) => {
                Some(self.armor_stands.iter().find(|s| s.id == id)?.fire_ticks)
            }
            Body::Other(Target::Item(id)) => {
                Some(self.dropped.iter().find(|i| i.id == id)?.fire_ticks)
            }
            Body::Other(_) => None,
        }
    }
    /// Sets the remaining fire ticks (`Entity.setRemainingFireTicks`).
    pub(super) fn native_entity_set_remaining_fire_ticks(
        &mut self,
        entity: EntityRef,
        ticks: i32,
    ) -> bool {
        match self.native_entity_target(entity) {
            Some(Target::Player) => self.player.as_mut().is_some_and(|p| {
                p.fire_ticks = ticks;
                true
            }),
            Some(Target::Guest(conn)) => self
                .lan
                .as_mut()
                .and_then(|lan| lan.hub.guest_mut(conn))
                .and_then(|guest| guest.motion.as_deref_mut())
                .is_some_and(|p| {
                    p.fire_ticks = ticks;
                    true
                }),
            Some(Target::Mob(id)) => self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
                m.fire_ticks = ticks;
                m.needs_sync = true;
                true
            }),
            Some(Target::Stand(id)) => self
                .armor_stands
                .iter_mut()
                .find(|s| s.id == id)
                .is_some_and(|s| {
                    s.fire_ticks = ticks;
                    true
                }),
            Some(Target::Item(id)) => {
                self.dropped
                    .iter_mut()
                    .find(|i| i.id == id)
                    .is_some_and(|i| {
                        i.fire_ticks = ticks;
                        true
                    })
            }
            _ => false,
        }
    }
    /// Clears positive fire ticks while retaining fire immunity (`Entity.clearFire`).
    pub(super) fn native_entity_clear_fire(&mut self, entity: EntityRef) -> bool {
        let Some(ticks) = self.native_entity_fire_ticks(entity) else {
            return false;
        };
        self.native_entity_set_remaining_fire_ticks(entity, ticks.min(0))
    }
    /// The entity's air (`Entity.getAirSupply`): players and mobs.
    pub(super) fn native_entity_air_supply(&self, entity: EntityRef) -> Option<i32> {
        match self.native_entity_body(entity)? {
            Body::Player(p) => Some(p.air),
            Body::Mob(m) => Some(m.air),
            Body::Other(_) => None,
        }
    }
    /// Sets the entity's air (`Entity.setAirSupply`).
    pub(super) fn native_entity_set_air_supply(&mut self, entity: EntityRef, air: i32) -> bool {
        match self.native_entity_target(entity) {
            Some(Target::Player) => match self.player.as_mut() {
                Some(player) => {
                    player.air = air;
                    true
                }
                None => false,
            },
            Some(Target::Guest(conn)) => match self
                .lan
                .as_mut()
                .and_then(|lan| lan.hub.guest_mut(conn))
                .and_then(|guest| guest.motion.as_deref_mut())
            {
                Some(player) => {
                    player.air = air;
                    true
                }
                None => false,
            },
            Some(Target::Mob(id)) => match self.mobs.iter_mut().find(|mob| mob.id == id) {
                Some(mob) => {
                    mob.air = air;
                    mob.needs_sync = true;
                    true
                }
                None => false,
            },
            _ => false,
        }
    }
    /// The most air the entity holds (`Entity.getMaxAirSupply`): 300, a
    /// dolphin's 4800, an axolotl's 6000.
    pub(super) fn native_entity_max_air_supply(&self, entity: EntityRef) -> Option<i32> {
        match self.native_entity_body(entity)? {
            Body::Mob(m) => Some(max_air(Some(m.species()))),
            _ => Some(max_air(None)),
        }
    }
    /// Whether the mob makes no sounds (`Entity.isSilent`, its `Silent`
    /// tag); false for a player.
    pub(super) fn native_entity_is_silent(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Player(_) => Some(false),
            Body::Mob(m) => Some(m.silent),
            Body::Other(_) => None,
        }
    }
    /// Silences the mob or lets it make sounds again (`Entity.setSilent`).
    /// False for the other kinds.
    pub(super) fn native_entity_set_silent(&mut self, entity: EntityRef, silent: bool) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(entity) else {
            return false;
        };
        match self.mobs.iter_mut().find(|m| m.id == id) {
            Some(m) => {
                m.silent = silent;
                true
            }
            None => false,
        }
    }
    /// Whether the entity's model is hidden (`Entity.isInvisible`): the
    /// invisibility effect on a player or a mob, a mob's or an armor stand's
    /// flag.
    pub(super) fn native_entity_is_invisible(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_target(entity)? {
            Target::Player => Some(self.potions.effects.get("invisibility").is_some()),
            Target::Guest(conn) => Some(
                self.command_guest(conn)?
                    .effects
                    .get("invisibility")
                    .is_some(),
            ),
            Target::Mob(id) => {
                let m = self.mobs.iter().find(|m| m.id == id)?;
                Some(m.is_invisible())
            }
            Target::Stand(id) => Some(self.armor_stands.iter().find(|s| s.id == id)?.invisible),
            _ => None,
        }
    }
    /// Sets the entity's frozen ticks (`Entity.setTicksFrozen`).
    pub(super) fn native_entity_set_ticks_frozen(&mut self, entity: EntityRef, ticks: i32) -> bool {
        match self.native_entity_target(entity) {
            Some(Target::Player) => self.player.as_mut().is_some_and(|p| {
                p.ticks_frozen = ticks;
                true
            }),
            Some(Target::Guest(conn)) => self
                .lan
                .as_mut()
                .and_then(|lan| lan.hub.guest_mut(conn))
                .and_then(|guest| guest.motion.as_deref_mut())
                .is_some_and(|p| {
                    p.ticks_frozen = ticks;
                    true
                }),
            Some(Target::Mob(id)) => self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
                m.ticks_frozen = ticks;
                m.needs_sync = true;
                true
            }),
            _ => false,
        }
    }
    /// Returns the mob's persistent glowing tag (`Entity.hasGlowingTag`).
    pub(super) fn native_entity_glowing(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Mob(m) => Some(m.glowing),
            _ => None,
        }
    }
    /// Returns the mob's no-gravity flag (`Entity.isNoGravity`).
    pub(super) fn native_entity_no_gravity(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_body(entity)? {
            Body::Mob(m) => Some(m.no_gravity),
            _ => None,
        }
    }
    /// Returns the entity's frozen ticks (`Entity.getTicksFrozen`).
    pub(super) fn native_entity_ticks_frozen(&self, entity: EntityRef) -> Option<i32> {
        match self.native_entity_body(entity)? {
            Body::Player(p) => Some(p.ticks_frozen),
            Body::Mob(m) => Some(m.ticks_frozen),
            _ => None,
        }
    }
    /// Sets the mob's persistent glowing tag (`Entity.setGlowingTag`). Returns false for players and guests.
    pub(super) fn native_entity_set_glowing(&mut self, entity: EntityRef, glowing: bool) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(entity) else {
            return false;
        };
        self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
            m.glowing = glowing;
            m.needs_sync = true;
            true
        })
    }
    /// Sets the mob's no-gravity flag (`Entity.setNoGravity`). Returns false for players and guests.
    pub(super) fn native_entity_set_no_gravity(
        &mut self,
        entity: EntityRef,
        no_gravity: bool,
    ) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(entity) else {
            return false;
        };
        self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
            m.no_gravity = no_gravity;
            m.needs_sync = true;
            true
        })
    }
    /// Sets the entity's shared invisibility flag (`Entity.setInvisible`). Returns false for players and guests.
    pub(super) fn native_entity_set_invisible(
        &mut self,
        entity: EntityRef,
        invisible: bool,
    ) -> bool {
        match self.native_entity_target(entity) {
            Some(Target::Mob(id)) => self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
                m.invisible = invisible;
                m.needs_sync = true;
                true
            }),
            Some(Target::Stand(id)) => self
                .armor_stands
                .iter_mut()
                .find(|s| s.id == id)
                .is_some_and(|s| {
                    s.invisible = invisible;
                    true
                }),
            _ => false,
        }
    }
    /// The entity's pose (`Entity.getPose`) as `Pose` names it: `standing`,
    /// `crouching`, `swimming`, `sleeping`, a warden's `roaring`... Kinds
    /// with no pose stand.
    pub(super) fn native_entity_pose(&self, entity: EntityRef) -> Option<&'static str> {
        Some(match self.native_entity_body(entity)? {
            Body::Player(p) => player_pose(p.pose),
            Body::Mob(m) => POSES.get(usize::from(sync::pose_id(m))).copied()?,
            Body::Other(_) => POSES[0],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses_keep_vanillas_ids() {
        assert_eq!(POSES[usize::from(sync::SITTING)], "sitting");
        assert_eq!(POSES[usize::from(sync::INHALING)], "inhaling");
        for pose in [
            Pose::Standing,
            Pose::Crouching,
            Pose::Swimming,
            Pose::FallFlying,
            Pose::SpinAttack,
            Pose::Sleeping,
        ] {
            let id = POSES.iter().position(|p| *p == player_pose(pose));
            assert_eq!(id.map(|i| Pose::from_id(i as i32)), Some(pose));
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mob_setters_update_stored_state() {
        use super::super::native_entity_refs::tests::with_game;
        use horizon_core::geometry::Vec3;
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(42, kind, Vec3::ZERO, 0.0));
            let entity = game.native_entity(42).unwrap();
            assert!(entity.set_air_supply(game, -7));
            assert_eq!(game.mobs[0].air, -7);
            assert!(entity.set_remaining_fire_ticks(game, 37));
            assert_eq!(game.mobs[0].fire_ticks, 37);
            assert!(entity.clear_fire(game));
            assert_eq!(game.mobs[0].fire_ticks, 0);
            assert!(game.native_entity_set_remaining_fire_ticks(entity, -12));
            assert!(game.native_entity_clear_fire(entity));
            assert_eq!(game.mobs[0].fire_ticks, -12);
            assert!(entity.set_ticks_frozen(game, 31));
            assert_eq!(game.mobs[0].ticks_frozen, 31);
            assert!(entity.set_invisible(game, true));
            assert!(game.mobs[0].invisible);
            game.mobs[0].add_effect("minecraft:invisibility", 100, 0);
            assert!(entity.set_invisible(game, false));
            assert_eq!(entity.is_invisible(game), Some(false));
            assert!(entity.set_glowing(game, true));
            assert!(game.mobs[0].glowing);
            assert!(entity.set_no_gravity(game, true));
            assert!(game.mobs[0].no_gravity);
            assert!(game.mobs[0].needs_sync);
        });
    }

    fn with_player(test: impl FnOnce(&mut Game<'_>, EntityRef)) {
        use super::super::native_entity_refs::tests::with_game;
        use horizon_core::geometry::Vec3;
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
            game.lan_acting_uuid = Some(123);
            game.lan_acting_entity = Some(20);
            let player = game.native_entity(20).unwrap();
            test(game, player);
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn player_invisibility_write_is_refused() {
        with_player(|game, player| assert!(!player.set_invisible(game, true)));
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn player_glowing_write_is_refused() {
        with_player(|game, player| assert!(!player.set_glowing(game, true)));
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn player_no_gravity_write_is_refused() {
        with_player(|game, player| assert!(!player.set_no_gravity(game, true)));
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn player_frozen_write_updates_server_state() {
        with_player(|game, player| {
            assert!(player.set_ticks_frozen(game, 53));
            assert_eq!(game.player.as_ref().unwrap().ticks_frozen, 53);
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn secondary_use_reads_reachable_client_handles_and_keeps_server_flags() {
        use super::super::execution_role::ExecutionRole;
        use horizon_core::geometry::Vec3;
        use horizon_protocol::connection::Pipe;
        use mod_api::{ClientHost, Host};
        use winit::keyboard::KeyCode;
        super::super::native_entity_refs::tests::with_game(|game| {
            game.options.execution_role = ExecutionRole::Client;
            game.options.dedicated = None;
            let (client, _server) = Pipe::pair();
            game.connect_local(client, "SneakReadTest").unwrap();
            // The joined client's state; the local-player and entity constructors must both
            // resolve it, not just a hand-built entity id accepted by an unreachable stub.
            let remote = game.remote.as_mut().unwrap();
            remote.logged_in = true;
            remote.entity_id = 7;
            remote.profile = 42;
            game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
            let local = game.local_player().unwrap();
            let entity = game.entity(7).unwrap();
            assert_eq!(local.entity(), entity);
            game.keys.insert(KeyCode::ShiftLeft);
            assert!(!game.player.as_ref().unwrap().shift_key_down);
            assert_eq!(entity.is_shift_key_down(game), Some(true));
            game.keys.clear();
            game.player.as_mut().unwrap().shift_key_down = true;
            assert_eq!(
                entity.is_shift_key_down(game),
                Some(false),
                "live secondary use, not last tick's body flag"
            );
            let stale = EntityRef::new(mod_api::NativeWorldEpoch::new(0), 7);
            assert_eq!(stale.is_shift_key_down(game), None);
            game.remote.as_mut().unwrap().logged_in = false;
            assert!(game.local_player().is_none());
            assert_eq!(entity.is_shift_key_down(game), None);
        });
        with_player(|game, player| {
            game.keys.clear();
            game.player.as_mut().unwrap().shift_key_down = true;
            assert_eq!(
                player.is_shift_key_down(game),
                Some(true),
                "server keeps its semantic entity flag"
            );
        });
    }

    #[test]
    fn max_air_follows_the_species() {
        assert_eq!(max_air(None), 300);
        assert_eq!(max_air(Some(Species::Dolphin)), 4800);
        assert_eq!(max_air(Some(Species::Axolotl)), 6000);
        assert_eq!(max_air(Some(Species::Zombie)), 300);
    }
}

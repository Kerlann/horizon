//! Mob handles for native server mods.
use super::{Game, commands::Target};
use horizon_client::mobs::{self, Mob, Species, Who, ai::brain_target};
use mod_api::{EntityRef, LivingRef, MobRef};

/// What decides whether a player can be a mob's target.
pub(super) struct PlayerFacts {
    pub mode: i32,
    pub alive: bool,
    pub invulnerable: bool,
    pub uuid: [i32; 4],
}

impl Game<'_> {
    /// A player target's game mode, liveness, invulnerability ability and uuid.
    pub(super) fn native_player_facts(&self, target: Target) -> Option<PlayerFacts> {
        match target {
            Target::Guest(conn) => {
                let g = self.command_guest(conn)?;
                if g.is_fake() {
                    return None;
                }
                Some(PlayerFacts {
                    mode: g.game_mode,
                    alive: !g.dead && g.health > 0.0,
                    invulnerable: g.motion.as_ref().is_some_and(|p| p.invulnerable),
                    uuid: horizon_client::lan::playerdata::uuid_ints(g.uuid),
                })
            }
            Target::Player => {
                if self.lan_acting_fake {
                    return None;
                }
                let player = self.player.as_ref()?;
                Some(PlayerFacts {
                    mode: self.mode_id(),
                    alive: !player.health.dead_or_dying(),
                    invulnerable: player.invulnerable,
                    uuid: self
                        .lan_acting_uuid
                        .map(horizon_client::lan::playerdata::uuid_ints)
                        .or(self.level.player_uuid)
                        .unwrap_or_default(),
                })
            }
            _ => None,
        }
    }
}

/// The target setter cannot model these species' extra side effects yet.
pub(super) fn target_setter_available(mob: &Mob) -> bool {
    brain_target(mob).is_none()
        && !matches!(
            mob.species(),
            Species::Fox | Species::Enderman | Species::ZombifiedPiglin
        )
}

/// Target admission shared by handles and a goal's selected-player snapshot.
pub(super) fn can_target_player(mob: &Mob, facts: PlayerFacts, difficulty: i32) -> bool {
    if !facts.alive || facts.invulnerable || matches!(facts.mode, 1 | 3) || difficulty == 0 {
        return false;
    }
    if mob.built_by_player() || (mob.species() == Species::Dolphin && mob.is_baby()) {
        return false;
    }
    !(mob.is_tamable_animal()
        && mob.extra.tame
        && mobs::tame::owned_by(mob.extra.owner, facts.uuid))
}

/// Only the facts target admission needs, borrowed from a live mob or its existing tick snapshot.
pub(super) struct MobTargetFacts {
    id: u32,
    species: Species,
    alive: bool,
    invulnerable: bool,
    baby: bool,
}
impl From<&Mob> for MobTargetFacts {
    fn from(mob: &Mob) -> Self {
        Self {
            id: mob.id,
            species: mob.species(),
            alive: mob.alive(),
            invulnerable: mob.invulnerable_flag,
            baby: mob.is_baby(),
        }
    }
}
impl From<&mobs::Seen> for MobTargetFacts {
    fn from(mob: &mobs::Seen) -> Self {
        Self {
            id: mob.id,
            species: mob.species,
            alive: mob.alive,
            invulnerable: mob.invulnerable,
            baby: mob.baby,
        }
    }
}

/// Target admission over the same facts while the live mob list is taken for ticking.
pub(super) fn can_target_mob(mob: &Mob, other: MobTargetFacts, world: &mobs::MobWorld<'_>) -> bool {
    if !other.alive || other.invulnerable || other.species == Species::Ghast {
        return false;
    }
    if mob.species() == Species::Breeze && other.species != Species::IronGolem {
        return false;
    }
    if mob.species() == Species::Creeper && other.species == Species::Goat {
        return false;
    }
    if matches!(
        mob.species(),
        Species::Evoker | Species::Vindicator | Species::Illusioner | Species::Pillager
    ) && matches!(other.species, Species::Villager | Species::WanderingTrader)
        && other.baby
    {
        return false;
    }
    !mob.spares_target(world, Who::Mob(other.id))
}

#[derive(Clone, Copy)]
enum MobFlag {
    Aggressive,
    Persistent,
}

impl Game<'_> {
    /// `Mob.asValidTarget` and the modeled `canAttack` overrides.
    fn native_mob_valid_target(&self, mob: &Mob, target: Target) -> bool {
        if mob.species() == Species::Warden {
            return self.native_warden_valid_target(mob, target);
        }
        match target {
            Target::Mob(id) => {
                let Some(other) = self.mobs.iter().find(|m| m.id == id && !m.removed) else {
                    return false;
                };
                let seen = [super::creatures::seen(other, self.level.player_uuid)];
                can_target_mob(mob, (&seen[0]).into(), &self.mob_world_base(&seen))
            }
            Target::Player | Target::Guest(_) => self
                .native_player_facts(target)
                .is_some_and(|facts| can_target_player(mob, facts, self.difficulty_id())),
            _ => false,
        }
    }
    /// `Warden.canTargetEntity`, which overrides the usual `Mob.canAttack`.
    fn native_warden_valid_target(&self, mob: &Mob, target: Target) -> bool {
        use horizon_core::geometry::Axis;
        let (bounds, team) = match target {
            Target::Mob(id) => {
                let Some(other) = self.mobs.iter().find(|m| m.id == id && !m.removed) else {
                    return false;
                };
                if !other.alive() || other.invulnerable_flag || other.species() == Species::Warden {
                    return false;
                }
                (other.bounds(), self.mob_team(id))
            }
            Target::Guest(conn) => {
                let Some(g) = self.command_guest(conn) else {
                    return false;
                };
                if g.dimension != self.dim.id()
                    || g.dead
                    || g.health <= 0.0
                    || matches!(g.game_mode, 1 | 3)
                    || g.motion.as_ref().is_some_and(|p| p.invulnerable)
                {
                    return false;
                }
                (g.bounds(), self.score.board.players_team(&g.name))
            }
            Target::Player => {
                let Some(player) = &self.player else {
                    return false;
                };
                if matches!(self.mode_id(), 1 | 3)
                    || player.health.dead_or_dying()
                    || player.invulnerable
                {
                    return false;
                }
                (player.bounds(), self.player_team())
            }
            _ => return false,
        };
        let border = self.border.geometry();
        !horizon_client::scoreboard::teams_allied(self.mob_team(mob.id), team)
            && border.is_within_bounds(bounds.min(Axis::X), bounds.min(Axis::Z), 0.0)
            && border.is_within_bounds(
                bounds.max(Axis::X) - f64::from(1.0e-5_f32),
                bounds.max(Axis::Z) - f64::from(1.0e-5_f32),
                0.0,
            )
    }

    /// The entity as a vanilla Mob: a mob or the ender dragon. Players, armor
    /// stands, stale handles and clients are rejected. Mobs taken out for
    /// ticking are unavailable (a #29 follow-up).
    pub(super) fn native_mob_ref(&self, entity: EntityRef) -> Option<MobRef> {
        match self.native_entity_target(entity)? {
            Target::Mob(_) | Target::Dragon(_) => Some(MobRef::new(entity)),
            _ => None,
        }
    }

    /// `Mob.getTarget`. Brain-backed mobs read their attack memory; piglins
    /// and hoglins use Horizon's `ai.target` stand-in. Invalid targets, an
    /// unavailable mob or player binding, or a target without a living handle
    /// read as None. The dragon reads what a mod set, while it stays a valid
    /// target (`EnderDragon.canAttack`); its flight never sets one.
    pub(super) fn native_mob_target(&self, mob: MobRef) -> Option<LivingRef> {
        let Target::Mob(id) = self.native_entity_target(mob.entity())? else {
            return self.native_dragon_target(mob.entity());
        };
        let mob = self.mobs.iter().find(|m| m.id == id)?;
        let target = brain_target(mob).unwrap_or(mob.ai.target)?;
        let target_id = match target {
            Who::Mob(id) => id,
            Who::Player => {
                let host = self.lan.as_ref()?;
                let conn = host.mob_players.get(&id)?;
                u32::try_from(host.hub.guest(*conn)?.entity_id).ok()?
            }
        };
        let entity = self.native_entity(target_id)?;
        self.native_mob_valid_target(mob, self.native_entity_target(entity)?)
            .then(|| self.native_living(entity))?
    }

    /// `Mob.setTarget`; None clears it. Only mobs and players whose binding
    /// can be stored in the current level are supported; armor stands are
    /// refused. The dragon stores a mob or a player, which its flight ignores
    /// and its save does not keep (vanilla keeps no target either). Brain-backed mobs refuse writes: vanilla's setter
    /// does not update their brain memory. Foxes, endermen and zombified
    /// piglins also refuse writes until their setter side effects are modeled.
    /// Invalid targets are refused (false, mob unchanged); vanilla instead
    /// stores `asValidTarget(target)`, clearing the target when it is invalid.
    /// False also means a stale handle, client, or an unavailable mob/player.
    pub(super) fn native_mob_set_target(&mut self, mob: MobRef, target: Option<LivingRef>) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(mob.entity()) else {
            return self.native_dragon_set_target(mob.entity(), target);
        };
        let Some(mob_state) = self.mobs.iter().find(|m| m.id == id) else {
            return false;
        };
        if !target_setter_available(mob_state) {
            return false;
        }
        if let Some(target) = target {
            let Some(target_kind) = self.native_entity_target(target.entity()) else {
                return false;
            };
            if !self.native_mob_valid_target(mob_state, target_kind) {
                return false;
            }
        }
        let (who, player) = match target {
            None => (None, None),
            Some(target) => match self.native_entity_target(target.entity()) {
                Some(Target::Mob(id)) => (Some(Who::Mob(id)), None),
                Some(Target::Guest(conn)) if self.lan_acting_uuid.is_none() => {
                    let Some(guest) = self.lan.as_ref().and_then(|h| h.hub.guest(conn)) else {
                        return false;
                    };
                    if guest.dimension != self.dim.id() {
                        return false;
                    }
                    (Some(Who::Player), Some(conn))
                }
                Some(Target::Player) => match self.lan_acting {
                    Some(conn) if self.lan.is_some() => (Some(Who::Player), Some(conn)),
                    _ => return false,
                },
                _ => return false,
            },
        };
        if let Some(conn) = player {
            let Some(host) = self.lan.as_mut() else {
                return false;
            };
            host.mob_players.insert(id, conn);
        }
        let Some(mob) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return false;
        };
        mob.ai.target = who;
        true
    }

    /// The mob flag a handle names: a mob's, or the dragon's own.
    fn native_mob_flag(&self, mob: MobRef, flag: MobFlag) -> Option<bool> {
        match self.native_entity_target(mob.entity())? {
            Target::Mob(id) => {
                let mob = self.mobs.iter().find(|m| m.id == id)?;
                Some(match flag {
                    MobFlag::Aggressive => mob.ai.aggressive,
                    MobFlag::Persistent => mob.persistent,
                })
            }
            Target::Dragon(_) => {
                let dragon = self.native_dragon(mob.entity())?;
                Some(match flag {
                    MobFlag::Aggressive => dragon.aggressive,
                    MobFlag::Persistent => dragon.persistent,
                })
            }
            _ => None,
        }
    }
    fn native_mob_set_flag(&mut self, mob: MobRef, flag: MobFlag, value: bool) -> bool {
        let slot = match self.native_entity_target(mob.entity()) {
            Some(Target::Mob(id)) => {
                self.mobs
                    .iter_mut()
                    .find(|m| m.id == id)
                    .map(|mob| match flag {
                        MobFlag::Aggressive => &mut mob.ai.aggressive,
                        MobFlag::Persistent => &mut mob.persistent,
                    })
            }
            Some(Target::Dragon(_)) => self.native_dragon_mut(mob.entity()).map(|d| match flag {
                MobFlag::Aggressive => &mut d.aggressive,
                MobFlag::Persistent => &mut d.persistent,
            }),
            _ => None,
        };
        slot.map(|slot| *slot = value).is_some()
    }

    /// `Mob.isAggressive`, the synced mob flag (the dragon's reaches its
    /// clients as `DATA_MOB_FLAGS_ID`).
    pub(super) fn native_mob_is_aggressive(&self, mob: MobRef) -> Option<bool> {
        self.native_mob_flag(mob, MobFlag::Aggressive)
    }

    /// `Mob.setAggressive`. False for an unavailable mob.
    pub(super) fn native_mob_set_aggressive(&mut self, mob: MobRef, value: bool) -> bool {
        self.native_mob_set_flag(mob, MobFlag::Aggressive, value)
    }

    /// `Mob.isPersistenceRequired`. The dragon never despawns
    /// (`EnderDragon.checkDespawn` is empty), but keeps and saves the flag
    /// (`PersistenceRequired`).
    pub(super) fn native_mob_persistence_required(&self, mob: MobRef) -> Option<bool> {
        self.native_mob_flag(mob, MobFlag::Persistent)
    }

    /// `Mob.setPersistenceRequired`, which sets the flag and cannot clear it.
    /// False for an unavailable mob.
    pub(super) fn native_mob_set_persistence_required(&mut self, mob: MobRef) -> bool {
        self.native_mob_set_flag(mob, MobFlag::Persistent, true)
    }

    /// `Mob.requiresCustomPersistence` and its species overrides: passengers,
    /// leashed mobs and species-specific reasons to stay, such as an enderman
    /// carrying a block. Independent of `PersistenceRequired` and `NoAI`.
    /// None for an unavailable mob or the separately modeled dragon.
    pub(super) fn native_mob_requires_custom_persistence(&self, mob: MobRef) -> Option<bool> {
        let Target::Mob(id) = self.native_entity_target(mob.entity())? else {
            return None;
        };
        self.mobs
            .iter()
            .find(|m| m.id == id)
            .map(mobs::ai::requires_custom_persistence)
    }

    /// `Mob.isNoAi`: the mob stands where it is and its goals do not run,
    /// while what NoAI keeps running in vanilla (the daylight burn of the
    /// undead, a horse's tail) still does. Saved with it as `NoAI`. None
    /// for anything but a mob.
    pub(super) fn native_mob_no_ai(&self, mob: MobRef) -> Option<bool> {
        let Target::Mob(id) = self.native_entity_target(mob.entity())? else {
            return None;
        };
        Some(self.mobs.iter().find(|m| m.id == id && !m.removed)?.no_ai)
    }

    /// `Mob.setNoAi`: whether the mob's own AI runs. With no AI the mob
    /// stands (what NoAI keeps in vanilla still runs), and the flag is
    /// saved with it as `NoAI`. False for anything but a mob.
    pub(super) fn native_mob_set_no_ai(&mut self, mob: MobRef, no_ai: bool) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(mob.entity()) else {
            return false;
        };
        self.mobs
            .iter_mut()
            .find(|m| m.id == id && !m.removed)
            .map(|m| m.no_ai = no_ai)
            .is_some()
    }

    /// `Mob.setBaby` through the species' setter: ageable mobs use their
    /// baby start age (`-48000` for sniffers, otherwise `-24000`) or `0`,
    /// including age-boundary attribute and AI setup changes. Non-ageable
    /// babies, such as zombies, keep their non-growing baby state instead.
    /// Leaves `NoAI` and `PersistenceRequired` unchanged. False, with no
    /// changes, for an unavailable mob or a species with no baby form.
    pub(super) fn native_mob_set_baby(&mut self, mob: MobRef, baby: bool) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(mob.entity()) else {
            return false;
        };
        let Some(mob) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return false;
        };
        if !mobs::drops::can_be_baby(mob.species()) {
            return false;
        }
        mobs::finalize::set_baby(mob, baby);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::*;
    use horizon_client::{entities::ArmorStand, mobs::Mob};
    use horizon_core::geometry::Vec3;
    use mod_api::NativeWorldEpoch;

    fn add_mob(game: &mut Game<'_>, id: u32, name: &str) -> MobRef {
        let kind = game.res.mob_types().get(name).unwrap().clone();
        game.mobs.push(Mob::new(id, kind, Vec3::ZERO, 0.0));
        game.native_mob_ref(game.native_entity(id).unwrap())
            .unwrap()
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mob_flags_custom_persistence_is_independent_of_stored_flags() {
        with_game(|game| {
            let mob = add_mob(game, 12, "minecraft:cow");
            for no_ai in [false, true] {
                assert!(mob.set_no_ai(game, no_ai));
                for (persistent, leash, expected) in [
                    (false, None, false),
                    (false, Some(mobs::leash::Holder::Knot([5, 64, 5])), true),
                    (true, None, false),
                    (true, Some(mobs::leash::Holder::Knot([5, 64, 5])), true),
                ] {
                    game.mobs[0].persistent = persistent;
                    game.mobs[0].extra.leash = leash;
                    assert_eq!(mob.requires_custom_persistence(game), Some(expected));
                    assert_eq!(mob.persistence_required(game), Some(persistent));
                    assert_eq!(mob.no_ai(game), Some(no_ai));
                }
            }
            let enderman = add_mob(game, 13, "minecraft:enderman");
            assert_eq!(enderman.requires_custom_persistence(game), Some(false));
            game.mobs[1].extra.carried = Some(
                game.pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap(),
            );
            assert_eq!(enderman.requires_custom_persistence(game), Some(true));
            assert_eq!(enderman.persistence_required(game), Some(false));
            game.mobs[1].extra.carried = None;
            assert!(enderman.set_persistence_required(game));
            assert_eq!(enderman.requires_custom_persistence(game), Some(false));
            assert_eq!(enderman.persistence_required(game), Some(true));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mob_flags_baby_setter_uses_species_age_and_attribute_boundaries() {
        use mobs::finalize::{Attr, base_value};

        with_game(|game| {
            let mut id = 12;
            for (no_ai, persistent) in [(false, false), (false, true), (true, false), (true, true)]
            {
                for (name, baby_age, damage) in [
                    ("minecraft:sniffer", -48000, None),
                    ("minecraft:goat", -24000, Some((1.0, 2.0))),
                    ("minecraft:hoglin", -24000, Some((0.5, 6.0))),
                    // Non-ageable babies keep a sentinel age instead of growing up.
                    ("minecraft:zombie", -1073741824, None),
                    ("minecraft:piglin", -1073741824, None),
                ] {
                    let mob = add_mob(game, id, name);
                    id += 1;
                    assert!(mob.set_no_ai(game, no_ai));
                    if persistent {
                        assert!(mob.set_persistence_required(game));
                    }
                    for baby in [true, false, true, false] {
                        assert!(mob.set_baby(game, baby), "{name}");
                        let state = game.mobs.last().unwrap();
                        assert_eq!(state.extra.age, if baby { baby_age } else { 0 }, "{name}");
                        assert_eq!(mob.living().baby(game), Some(baby), "{name}");
                        if let Some((baby_damage, adult_damage)) = damage {
                            assert_eq!(
                                base_value(state, Attr::AttackDamage),
                                if baby { baby_damage } else { adult_damage },
                                "{name}"
                            );
                        }
                        assert_eq!(mob.no_ai(game), Some(no_ai), "{name}");
                        assert_eq!(mob.persistence_required(game), Some(persistent), "{name}");
                    }
                }
            }
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mob_flags_baby_setter_runs_ghastling_setup_in_both_directions() {
        use mobs::{control::MoveKind, nav::NavKind};

        with_game(|game| {
            let ghast = add_mob(game, 12, "minecraft:happy_ghast");
            assert!(ghast.set_no_ai(game, true));
            assert!(ghast.set_persistence_required(game));
            assert!(game.mobs[0].ai.goals.has("HappyGhastFloatGoal"));
            for baby in [true, false, true, false] {
                assert!(ghast.set_baby(game, baby));
                let state = &game.mobs[0];
                assert_eq!(ghast.living().baby(game), Some(baby));
                assert_eq!(state.ai.goals.has("HappyGhastFloatGoal"), !baby);
                assert_eq!(state.ai.goals.has("RandomFloatAroundGoal"), !baby);
                if baby {
                    assert!(matches!(state.ai.nav.kind, NavKind::Flying));
                    assert!(matches!(state.ai.mov.kind, MoveKind::Flying { .. }));
                } else {
                    assert!(matches!(state.ai.nav.kind, NavKind::Ground));
                    assert!(matches!(state.ai.mov.kind, MoveKind::Custom));
                }
                assert_eq!(ghast.no_ai(game), Some(true));
                assert_eq!(ghast.persistence_required(game), Some(true));
            }
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mob_flags_baby_setter_refuses_no_baby_species_and_unavailable_mobs() {
        with_game(|game| {
            for (id, name) in [(12, "minecraft:skeleton"), (13, "minecraft:parrot")] {
                let mob = add_mob(game, id, name);
                assert!(mob.set_no_ai(game, true));
                assert!(mob.set_persistence_required(game));
                for baby in [true, false] {
                    assert!(!mob.set_baby(game, baby), "{name}");
                    assert_eq!(game.mobs.last().unwrap().extra.age, 0, "{name}");
                    assert_eq!(mob.living().baby(game), Some(false), "{name}");
                    assert_eq!(mob.no_ai(game), Some(true), "{name}");
                    assert_eq!(mob.persistence_required(game), Some(true), "{name}");
                }
            }
            let mob = add_mob(game, 14, "minecraft:sniffer");
            let stale = MobRef::new(EntityRef::new(
                NativeWorldEpoch::new(mob.entity().epoch().get() + 1),
                14,
            ));
            assert_eq!(stale.requires_custom_persistence(game), None);
            assert!(!stale.set_baby(game, true));
            game.held_out.push(14);
            assert_eq!(mob.requires_custom_persistence(game), None);
            assert!(!mob.set_baby(game, true));
            game.held_out.pop();
            game.mobs.last_mut().unwrap().removed = true;
            assert_eq!(mob.requires_custom_persistence(game), None);
            assert!(!mob.set_baby(game, true));
            assert_eq!(game.mobs.last().unwrap().extra.age, 0);
            game.mobs.last_mut().unwrap().removed = false;
            let taken = std::mem::take(&mut game.mobs);
            assert_eq!(mob.requires_custom_persistence(game), None);
            assert!(!mob.set_baby(game, true));
            game.mobs = taken;
            assert!(mob.set_baby(game, true));

            let mut dragon = super::super::dragon::Dragon::new(Vec3::ZERO, 0.0, 0);
            dragon.id = 15;
            game.dragon = Some(dragon);
            let dragon = game
                .native_mob_ref(game.native_entity(15).unwrap())
                .unwrap();
            assert_eq!(dragon.requires_custom_persistence(game), None);
            assert!(!dragon.set_baby(game, true));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn player_target_round_trips_through_the_mob_binding() {
        use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
        use horizon_protocol::packets::configuration::ClientInformation;
        use std::sync::{Arc, atomic::AtomicI32, mpsc};

        with_game(|game| {
            let settings = super::super::lan::LanSettings {
                port: 0,
                guest_commands: false,
                force_game_mode: false,
            };
            let net = NetConfig {
                announce: false,
                ..NetConfig::lan(0, "MobRefTest".into(), 2)
            };
            let config = HubConfig {
                default_game_mode: 0,
                guest_commands: false,
                force_game_mode: false,
                hardcore: false,
                view_distance: 10,
                simulation_distance: 10,
                dimension: "minecraft:overworld".into(),
                allow_flight: false,
                seed: 0,
                max_players: 2,
                integrated: false,
                levels: Default::default(),
            };
            game.lan_host(settings, net, config).unwrap();
            let (tx, _rx) = mpsc::channel();
            let mut guest = Guest::new(
                Joined {
                    conn: 1,
                    uuid: 123,
                    name: "guest".into(),
                    properties: Vec::new(),
                    info: ClientInformation::new(8),
                    brand: String::new(),
                    sender: PlayerSender::new(tx, Arc::default()),
                    latency: Arc::new(AtomicI32::new(0)),
                    address: "127.0.0.1:1".parse().unwrap(),
                    expected: false,
                },
                20,
                [0.0; 3],
                8,
            );
            guest.placed = true;
            game.lan.as_mut().unwrap().hub.guests.push(guest);
            let zombie = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(12, zombie, Vec3::ZERO, 0.0));
            let mob = game
                .native_mob_ref(game.native_entity(12).unwrap())
                .unwrap();
            let player = game.native_living(game.native_entity(20).unwrap()).unwrap();
            assert!(mob.set_target(game, Some(player)));
            assert_eq!(mob.target(game), Some(player));
            assert_eq!(game.lan.as_ref().unwrap().mob_players.get(&12), Some(&1));
            game.lan.as_mut().unwrap().hub.guests[0].game_mode = 1;
            assert!(!mob.set_target(game, Some(player)));
            assert_eq!(mob.target(game), None);
            assert_eq!(game.mobs[0].ai.target, Some(Who::Player));
            game.lan.as_mut().unwrap().hub.guests[0].game_mode = 0;
            game.lan.as_mut().unwrap().hub.guests[0].dimension = "minecraft:the_nether".into();
            assert!(!mob.set_target(game, Some(player)));
            assert_eq!(game.mobs[0].ai.target, Some(Who::Player));
            game.lan.as_mut().unwrap().mob_players.clear();
            assert_eq!(mob.target(game), None);
            assert_eq!(game.mobs[0].ai.target, Some(Who::Player));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mob_handles_read_write_and_refuse_unavailable_entities() {
        with_game(|game| {
            let zombie = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs
                .push(Mob::new(12, zombie.clone(), Vec3::ZERO, 0.0));
            game.mobs.push(Mob::new(13, zombie, Vec3::ZERO, 0.0));
            game.armor_stands.push(ArmorStand::new(14, Vec3::ZERO, 0.0));
            let mut dragon = super::super::dragon::Dragon::new(Vec3::ZERO, 0.0, 0);
            dragon.id = 15;
            game.dragon = Some(dragon);
            assert!(
                game.native_mob_ref(game.native_entity(15).unwrap())
                    .is_some(),
                "the dragon is a Mob"
            );
            let entity = game.native_entity(12).unwrap();
            let mob = game.native_mob_ref(entity).unwrap();
            let other = game.native_living(game.native_entity(13).unwrap()).unwrap();
            let stand = game.native_living(game.native_entity(14).unwrap()).unwrap();
            for (id, name) in [
                (16, "minecraft:creaking"),
                (17, "minecraft:creeper"),
                (18, "minecraft:goat"),
                (19, "minecraft:warden"),
                (21, "minecraft:ghast"),
            ] {
                let kind = game.res.mob_types().get(name).unwrap().clone();
                game.mobs.push(Mob::new(id, kind, Vec3::ZERO, 0.0));
            }
            let brain = game
                .native_mob_ref(game.native_entity(16).unwrap())
                .unwrap();
            mobs::monster_brain::set_attack_target(
                game.mobs.iter_mut().find(|m| m.id == 16).unwrap(),
                Some(Who::Mob(13)),
            );
            assert_eq!(brain.target(game), Some(other));
            assert!(!brain.set_target(game, None));
            assert_eq!(brain.target(game), Some(other));
            let warden = game
                .native_mob_ref(game.native_entity(19).unwrap())
                .unwrap();
            let ghast = game.native_living(game.native_entity(21).unwrap()).unwrap();
            let warden_mob = game.mobs.iter_mut().find(|m| m.id == 19).unwrap();
            warden_mob
                .extra
                .warden
                .as_mut()
                .unwrap()
                .brain
                .mem
                .attack_target
                .set(Who::Mob(21));
            assert_eq!(
                warden.target(game),
                Some(ghast),
                "warden can target a ghast, unlike Mob.canAttack"
            );
            assert!(!warden.set_target(game, None));
            let creeper = game
                .native_mob_ref(game.native_entity(17).unwrap())
                .unwrap();
            let goat = game.native_living(game.native_entity(18).unwrap()).unwrap();
            assert!(creeper.set_target(game, Some(other)));
            assert!(!creeper.set_target(game, Some(goat)));
            assert_eq!(creeper.target(game), Some(other));
            assert!(game.native_mob_ref(stand.entity()).is_none());
            assert_eq!(mob.living().entity(), entity);
            assert_eq!(game.native_mob_is_aggressive(mob), Some(false));
            assert!(game.native_mob_set_aggressive(mob, true));
            assert_eq!(game.native_mob_is_aggressive(mob), Some(true));
            assert_eq!(game.native_mob_persistence_required(mob), Some(false));
            assert!(game.native_mob_set_persistence_required(mob));
            assert_eq!(game.native_mob_persistence_required(mob), Some(true));
            assert!(mob.set_target(game, Some(other)));
            assert_eq!(mob.target(game), Some(other));
            assert!(!game.native_mob_set_target(mob, Some(stand)));
            assert_eq!(game.native_mob_target(mob), Some(other));
            let stale = MobRef::new(EntityRef::new(
                NativeWorldEpoch::new(entity.epoch().get() + 1),
                12,
            ));
            assert!(game.native_mob_ref(stale.entity()).is_none());
            assert!(!game.native_mob_set_target(stale, None));
            assert_eq!(game.native_mob_is_aggressive(stale), None);
            let taken = std::mem::take(&mut game.mobs);
            assert_eq!(game.native_mob_target(mob), None);
            assert!(!game.native_mob_set_target(mob, None));
            assert_eq!(taken[0].ai.target, Some(Who::Mob(13)));
            game.mobs = taken;
            assert!(game.native_mob_set_target(mob, None));
            assert_eq!(game.native_mob_target(mob), None);
        });
    }
}

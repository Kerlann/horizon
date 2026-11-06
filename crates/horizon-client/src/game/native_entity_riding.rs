//! Vehicles and passengers for native server mods, through an `EntityRef`
//! (docs/modding/src/how-to/native-mods.md, "Entity and level handles", #7387): what an entity
//! rides, who rides it, and getting off. Players ride boats, minecarts and
//! mounts; mobs ride mobs, boats and minecarts. Reads return `None` for a
//! stale handle or on a client.
use super::{Game, commands::Target};
use mod_api::EntityRef;

impl Game<'_> {
    /// The id of the entity `t` rides, if it rides one.
    fn native_vehicle_id(&self, t: Target) -> Option<u32> {
        match t {
            Target::Player => self.riding,
            Target::Guest(conn) => self.command_guest(conn)?.vehicle,
            Target::Mob(id) => {
                self.mobs
                    .iter()
                    .find(|m| m.id == id && !m.removed)?
                    .extra
                    .mount
            }
            _ => None,
        }
    }
    /// The entity's vehicle as a handle, if it rides one in this level.
    fn native_vehicle(&self, entity: EntityRef) -> Option<Option<EntityRef>> {
        let t = self.native_entity_target(entity)?;
        Some(
            self.native_vehicle_id(t)
                .and_then(|v| self.native_entity(v)),
        )
    }
    /// What the entity rides (`Entity.getVehicle`): `None` when it rides
    /// nothing, or for a stale handle.
    pub(super) fn native_entity_vehicle(&self, entity: EntityRef) -> Option<EntityRef> {
        self.native_vehicle(entity).flatten()
    }
    /// Whether the entity rides another (`Entity.isPassenger`).
    pub(super) fn native_entity_is_passenger(&self, entity: EntityRef) -> Option<bool> {
        Some(self.native_vehicle(entity)?.is_some())
    }
    /// The lowest vehicle under the entity (`Entity.getRootVehicle`): the
    /// entity itself when it rides nothing.
    pub(super) fn native_entity_root_vehicle(&self, entity: EntityRef) -> Option<EntityRef> {
        let mut root = entity;
        // A chain of riders is short; the bound only guards against a loop.
        for _ in 0..64 {
            match self.native_vehicle(root)? {
                Some(v) => root = v,
                None => break,
            }
        }
        Some(root)
    }
    /// The entities riding this one (`Entity.getPassengers`): its players
    /// in the order they got in, then its mobs.
    pub(super) fn native_entity_passengers(&self, entity: EntityRef) -> Option<Vec<EntityRef>> {
        self.native_entity_target(entity)?;
        let id = entity.id();
        let acting = self.lan_acting.filter(|_| self.player.is_some());
        let mut acting_listed = false;
        let mut players: Vec<(u64, u32)> = self
            .lan
            .iter()
            .flat_map(|h| h.hub.guests.iter())
            .filter(|g| g.placed && g.dimension == self.dim.id())
            .filter(|g| {
                let vehicle = if acting == Some(g.conn) {
                    acting_listed = true;
                    self.riding
                } else {
                    g.vehicle
                };
                vehicle == Some(id)
            })
            .map(|g| (g.vehicle_order, g.entity_id as u32))
            .collect();
        players.sort_unstable();
        // An acting player absent from the placed list still rides through
        // `riding`; `addPassenger` puts that player first.
        if let Some(me) = self.lan_acting_entity
            && self.lan_acting_uuid.is_some()
            && !acting_listed
            && self.player.is_some()
            && self.riding == Some(id)
        {
            players.insert(0, (0, me));
        }
        let mobs = self
            .mobs
            .iter()
            .filter(|m| m.extra.mount == Some(id) && !m.removed && m.alive())
            .map(|m| m.id);
        let ids = players.into_iter().map(|(_, id)| id).chain(mobs);
        Some(ids.filter_map(|id| self.native_entity(id)).collect())
    }
    /// Whether anything rides the entity (`Entity.isVehicle`).
    pub(super) fn native_entity_is_vehicle(&self, entity: EntityRef) -> Option<bool> {
        Some(!self.native_entity_passengers(entity)?.is_empty())
    }
    /// Attempts one mount, shared by handle writes and the ride command.
    pub(super) fn start_riding_target(
        &mut self,
        rider: Target,
        vehicle: Target,
        force: bool,
    ) -> bool {
        if let Target::Guest(conn) = rider {
            if self
                .command_guest(conn)
                .is_none_or(|guest| guest.dimension != self.dim.id())
            {
                return false;
            }
            return self
                .for_player(rider, |game| {
                    game.start_riding_target(Target::Player, vehicle, force)
                })
                .unwrap_or(false);
        }
        let vehicle_id = match vehicle {
            Target::Mob(id) | Target::Boat(id) | Target::Cart(id) => id,
            _ => return false,
        };
        if !matches!(rider, Target::Player | Target::Mob(_))
            || rider == vehicle
            || self.native_vehicle_id(rider) == Some(vehicle_id)
        {
            return false;
        }
        let mut current = Some(vehicle);
        for _ in 0..64 {
            let Some(target) = current else { break };
            if target == rider {
                return false;
            }
            current = self
                .native_vehicle_id(target)
                .and_then(|id| self.native_entity(id))
                .and_then(|e| self.native_entity_target(e));
        }
        if current.is_some() {
            return false;
        }
        if !force {
            match rider {
                Target::Player if self.keys.held(horizon_client::actions::Action::Sneak) => {
                    return false;
                }
                Target::Mob(id)
                    if self.mobs.iter().find(|m| m.id == id).is_none_or(|m| {
                        m.extra.boarding_cooldown > 0
                            || matches!(
                                m.species(),
                                horizon_client::mobs::Species::Warden
                                    | horizon_client::mobs::Species::Wither
                            )
                    }) =>
                {
                    return false;
                }
                _ => {}
            }
        }
        match vehicle {
            Target::Boat(id) => {
                let Some(index) = self.boats.iter().position(|b| b.id == id) else {
                    return false;
                };
                if self.boat_passengers(id) >= self.boats[index].max_passengers()
                    || (!force && !self.boat_has_room(index))
                {
                    return false;
                }
            }
            Target::Cart(id) => {
                if !self.carts.iter().any(|cart| cart.id == id)
                    || self.riding == Some(id)
                    || self.ridden_by_guest(id)
                    || self
                        .mobs
                        .iter()
                        .any(|m| !m.removed && m.extra.mount == Some(id))
                {
                    return false;
                }
            }
            Target::Mob(id) => {
                let Some(mob) = self.mobs.iter().find(|m| m.id == id && m.alive()) else {
                    return false;
                };
                let players = self.boat_players(id).max(usize::from(mob.rider.is_some()));
                let mobs = self
                    .mobs
                    .iter()
                    .filter(|m| !m.removed && m.alive() && m.extra.mount == Some(id))
                    .count()
                    .max(usize::from(mob.extra.rider.is_some()));
                if !horizon_client::mobs::riders::can_add_passenger(mob, players + mobs) {
                    return false;
                }
            }
            _ => return false,
        }
        let allowed = match rider {
            Target::Player => self.native_may_board(vehicle_id),
            Target::Mob(id) => self.native_mob_may_ride(id, vehicle_id, true),
            _ => false,
        };
        if !allowed {
            return false;
        }
        if self.native_vehicle_id(rider).is_some() {
            match rider {
                Target::Player => self.stop_riding(),
                Target::Mob(id) => self.dismount_mob(id),
                _ => return false,
            }
            if self.native_vehicle_id(rider).is_some() {
                return false;
            }
        }
        match rider {
            Target::Player => {
                match vehicle {
                    Target::Boat(id) => {
                        if let Some(boat) = self.boats.iter().find(|b| b.id == id) {
                            self.yaw = boat.yaw;
                        }
                    }
                    Target::Mob(id) => {
                        if let Some(mob) = self.mobs.iter_mut().find(|m| m.id == id) {
                            mob.extra.flag = false;
                            mob.extra.counter = 0;
                            self.yaw = mob.yaw;
                        }
                        self.mount.jump_ticks = 0;
                        self.mount.jump_scale = 0.0;
                    }
                    _ => {}
                }
                self.riding = Some(vehicle_id);
                self.trigger_player("minecraft:started_riding");
            }
            Target::Mob(id) => horizon_client::mobs::riders::link(&mut self.mobs, id, vehicle_id),
            _ => return false,
        }
        self.native_vehicle_id(rider) == Some(vehicle_id)
    }

    /// Attempts to board a vehicle through the shared server mount path.
    ///
    /// Players and mobs can ride a mob, boat or minecart. Returns false for
    /// unsupported entity pairs, stale or held-out handles, a full stored
    /// vehicle, a cycle, or a canceled mount or dismount. Mob vehicle capacity
    /// follows its species; minecarts store one passenger and boats store one
    /// mob and up to two passengers in total. `force` bypasses normal
    /// admission checks, not these storage limits. Player boarding cooldown and the strider's
    /// eye-in-lava admission check are not yet represented by the game.
    pub(super) fn native_entity_start_riding(
        &mut self,
        entity: EntityRef,
        vehicle: EntityRef,
        force: bool,
    ) -> bool {
        let Some(rider) = self.native_entity_target(entity) else {
            return false;
        };
        let Some(vehicle) = self.native_entity_target(vehicle) else {
            return false;
        };
        self.start_riding_target(rider, vehicle, force)
    }

    /// Gets the entity off its vehicle (`Entity.stopRiding`), where the
    /// vehicle lets it out, as `/ride ... dismount` does. False when it rode
    /// nothing, a mod's `mob_ride` hook kept it on, or for a stale handle.
    pub(super) fn native_entity_stop_riding(&mut self, entity: EntityRef) -> bool {
        let Some(t) = self.native_entity_target(entity) else {
            return false;
        };
        if self.native_vehicle_id(t).is_none() {
            return false;
        }
        match t {
            Target::Mob(id) => self.dismount_mob(id),
            Target::Player | Target::Guest(_) => {
                self.for_player(t, |g| g.stop_riding());
            }
            _ => return false,
        }
        self.native_vehicle_id(t).is_none()
    }
    /// Gets every passenger off the entity (`Entity.ejectPassengers`), last
    /// first as vanilla walks the list, so `mob_ride` hears them in its
    /// order. False when nothing rode it, or for a stale handle.
    pub(super) fn native_entity_eject_passengers(&mut self, entity: EntityRef) -> bool {
        let Some(passengers) = self.native_entity_passengers(entity) else {
            return false;
        };
        let mut any = false;
        for p in passengers.into_iter().rev() {
            any |= self.native_entity_stop_riding(p);
        }
        any
    }
}

#[cfg(test)]
mod tests {
    use super::super::commands::Target;
    use horizon_client::mobs::Mob;
    use horizon_core::geometry::Vec3;
    use mod_api::{self, EntityMountEvent, Flow, Hooks, ServerHost};
    use std::sync::{Arc, Mutex};

    struct MountObserver(Arc<Mutex<Vec<bool>>>);
    #[mod_api::__subscriptions]
    impl Hooks for MountObserver {
        fn entity_mount(&self, _: &mut dyn ServerHost, event: &mut EntityMountEvent<'_>) -> Flow {
            self.0.lock().unwrap().push(event.mounting);
            if event.mounting {
                Flow::Continue
            } else {
                Flow::Cancel
            }
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn canceled_dismount_keeps_old_mount_and_mount_hook_fires_once() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "mount_observer",
            Box::new(MountObserver(events.clone())),
        )]
        .into();
        mod_api::testing::with_mods(mods, || {
            super::super::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                for id in [40, 41, 42] {
                    let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                    game.mobs.push(Mob::new(id, kind, Vec3::ZERO, 0.0));
                }
                assert!(game.start_riding_target(Target::Mob(40), Target::Mob(41), true));
                assert!(!game.start_riding_target(Target::Mob(40), Target::Mob(42), true));
                assert_eq!(game.mobs[0].extra.mount, Some(41));
                assert_eq!(*events.lock().unwrap(), [true, true, false]);
            });
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn full_vehicles_do_not_fire_mount_hooks_for_commands_or_handles() {
        use super::super::boats::Boat;
        let events = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "mount_observer",
            Box::new(MountObserver(events.clone())),
        )]
        .into();
        mod_api::testing::with_mods(mods, || {
            super::super::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                game.ui.registries.permission = 4;
                for (id, name) in [
                    (40, "minecraft:zombie"),
                    (41, "minecraft:pig"),
                    (42, "minecraft:sheep"),
                    (43, "minecraft:cow"),
                ] {
                    let kind = game.res.mob_types().get(name).unwrap().clone();
                    game.mobs.push(Mob::new(id, kind, Vec3::ZERO, 0.0));
                }
                game.boats
                    .push(Boat::new(44, "oak".into(), Vec3::ZERO, 0.0));
                let pig =
                    "ride @e[type=minecraft:zombie,limit=1] mount @e[type=minecraft:pig,limit=1]";
                let boat = "ride @e[type=minecraft:zombie,limit=1] mount @e[type=minecraft:oak_boat,limit=1]";
                assert!(game.run_command(pig));
                game.mobs[0].extra.mount = None;
                game.mobs[1].extra.rider = None;
                assert!(game.run_command(boat));
                game.mobs[0].extra.mount = None;
                game.mobs[2].extra.mount = Some(41);
                game.mobs[1].extra.rider = Some(42);
                game.mobs[3].extra.mount = Some(44);
                game.riding = Some(44);
                events.lock().unwrap().clear();
                assert!(!game.run_command(pig));
                assert!(!game.run_command(boat));
                let rider = game.native_entity(40).unwrap();
                for id in [41, 44] {
                    let vehicle = game.native_entity(id).unwrap();
                    assert!(!game.native_entity_start_riding(rider, vehicle, true));
                }
                assert!(events.lock().unwrap().is_empty());
            });
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn camel_admits_multiple_mob_passengers_but_not_a_fourth() {
        super::super::native_entity_refs::tests::with_game(|game| {
            for (id, name) in [
                (70, "minecraft:camel"),
                (71, "minecraft:camel_husk"),
                (72, "minecraft:happy_ghast"),
            ] {
                let kind = game.res.mob_types().get(name).unwrap().clone();
                game.mobs.push(Mob::new(id, kind, Vec3::ZERO, 0.0));
            }
            for id in 73..=76 {
                let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                game.mobs.push(Mob::new(id, kind, Vec3::ZERO, 0.0));
            }
            game.riding = Some(70);
            assert!(game.start_riding_target(Target::Mob(73), Target::Mob(70), true));
            assert!(game.start_riding_target(Target::Mob(74), Target::Mob(70), true));
            assert!(!game.start_riding_target(Target::Mob(75), Target::Mob(70), true));
            let mut riding = horizon_client::lan::entity::Riding::default();
            game.lan_riding(&[(70, 1)], &mut riding);
            assert_eq!(riding.passengers(70), &[1, 73, 74]);
            game.dismount_mob(73);
            game.dismount_mob(74);
            game.riding = None;
            for vehicle in [70, 71, 72] {
                let capacity = if vehicle == 72 { 4 } else { 3 };
                for rider in 73..73 + capacity {
                    assert!(game.start_riding_target(
                        Target::Mob(rider),
                        Target::Mob(vehicle),
                        true
                    ));
                }
                if capacity == 3 {
                    assert!(!game.start_riding_target(Target::Mob(76), Target::Mob(vehicle), true));
                }
                for rider in 73..73 + capacity {
                    game.dismount_mob(rider);
                }
            }
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn boat_and_cart_interactions_use_shared_mount_path() {
        use super::super::{
            InteractTarget,
            boats::Boat,
            minecarts::{CartKind, Minecart},
        };
        use horizon_client::motion::PlayerMotion;
        super::super::native_entity_refs::tests::with_game(|game| {
            game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
            game.boats
                .push(Boat::new(60, "oak".into(), Vec3::ZERO, 45.0));
            assert!(game.interact_boat(Some(InteractTarget {
                id: 60,
                location: [0.0; 3]
            })));
            assert_eq!(game.riding, Some(60));
            assert_eq!(game.yaw, 45.0);
            game.riding = None;
            game.carts
                .push(Minecart::new(61, CartKind::Rideable, Vec3::ZERO));
            assert!(game.interact_cart(Some(InteractTarget {
                id: 61,
                location: [0.0; 3]
            })));
            assert_eq!(game.riding, Some(61));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn handles_reject_held_out_and_stale_entities() {
        super::super::native_entity_refs::tests::with_game(|game| {
            for id in [50, 51] {
                let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                game.mobs.push(Mob::new(id, kind, Vec3::ZERO, 0.0));
            }
            let rider = game.native_entity(50).unwrap();
            let vehicle = game.native_entity(51).unwrap();
            let stale = mod_api::EntityRef::new(
                mod_api::NativeWorldEpoch::new(rider.epoch().get().wrapping_add(1)),
                rider.id(),
            );
            assert!(!game.native_entity_start_riding(stale, vehicle, true));
            game.held_out.push(51);
            assert!(!game.native_entity_start_riding(rider, vehicle, true));
            game.held_out.pop();
            assert!(game.native_entity_start_riding(rider, vehicle, true));
            assert_eq!(game.mobs[0].extra.mount, Some(51));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn mount_rejects_cycles_and_respects_cooldown_unless_forced() {
        super::super::native_entity_refs::tests::with_game(|game| {
            for id in [30, 31, 32] {
                let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                game.mobs.push(Mob::new(id, kind, Vec3::ZERO, 0.0));
            }
            game.mobs[0].extra.boarding_cooldown = 10;
            assert!(!game.start_riding_target(Target::Mob(30), Target::Mob(31), false));
            assert!(game.start_riding_target(Target::Mob(30), Target::Mob(31), true));
            assert!(!game.start_riding_target(Target::Mob(30), Target::Mob(31), true));
            assert!(!game.start_riding_target(Target::Mob(31), Target::Mob(30), true));
            assert!(!game.start_riding_target(Target::Mob(32), Target::Mob(31), true));
            assert_eq!(game.mobs[0].extra.mount, Some(31));
            assert_eq!(game.mobs[1].extra.rider, Some(30));
            assert_eq!(game.mobs[2].extra.mount, None);
            game.dismount_mob(30);
            assert_eq!(game.mobs[0].extra.mount, None);
            assert_eq!(game.mobs[0].extra.boarding_cooldown, 60);
            assert!(!game.start_riding_target(Target::Mob(30), Target::Mob(32), false));
            assert!(game.start_riding_target(Target::Mob(30), Target::Mob(32), true));
            assert_eq!(game.mobs[0].extra.mount, Some(32));
        });
    }
}

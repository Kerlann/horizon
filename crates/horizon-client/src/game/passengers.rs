//! Mobs riding mobs: boarding and dismounting, keeping riders on their
//! vehicles, the jockeys that spawn mounted (chicken jockeys, spider
//! jockeys, strider jockeys, zombie horsemen) and the skeleton horse trap.
//! Saving riders under `Passengers` lives in `persist.rs`.
use super::Game;
use horizon_client::mobs::{Species, riders};
use horizon_core::{geometry::Vec3, random::RandomSource};
use std::collections::HashMap;

/// The seconds a skeleton horse trap waits for a player: 15 minutes.
const TRAP_LIFETIME: i32 = 18000;
/// `SkeletonTrapGoal`: a player this close springs the trap.
const TRAP_RANGE: f64 = 10.0;
/// Ticks of invulnerability the trap's riders and horses start with.
const TRAP_INVULNERABLE: i32 = 60;

impl Game<'_> {
    /// `Entity.startRiding` for two mobs.
    pub(super) fn mount_mob(&mut self, rider: u32, vehicle: u32) {
        self.try_mount_mob(rider, vehicle);
    }
    /// `Entity.startRiding` for two mobs: false when a mod canceled it.
    pub(super) fn try_mount_mob(&mut self, rider: u32, vehicle: u32) -> bool {
        let vehicle = if self.boats.iter().any(|b| b.id == vehicle) {
            super::commands::Target::Boat(vehicle)
        } else if self.carts.iter().any(|c| c.id == vehicle) {
            super::commands::Target::Cart(vehicle)
        } else {
            super::commands::Target::Mob(vehicle)
        };
        self.start_riding_target(super::commands::Target::Mob(rider), vehicle, true)
    }
    /// Riders sit on their vehicles (`Entity.positionRider`), mobs or boats,
    /// and let go when the vehicle dies or is gone.
    pub(super) fn sync_riders(&mut self) {
        // Entity.baseTick.
        for m in &mut self.mobs {
            if m.extra.boarding_cooldown > 0 {
                m.extra.boarding_cooldown -= 1;
            }
        }
        let boats: std::collections::HashSet<u32> = self.boats.iter().map(|b| b.id).collect();
        let carts: std::collections::HashSet<u32> = self.carts.iter().map(|c| c.id).collect();
        let tnts: std::collections::HashSet<u32> = self
            .tnts
            .iter()
            .map(|t| t.id)
            .filter(|&id| id != 0)
            .collect();
        let index: HashMap<u32, usize> = self
            .mobs
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id, i))
            .collect();
        let mut riding: HashMap<u32, u32> = HashMap::new();
        // If the vehicle already has a first rider that is still validly riding it, preserve it.
        for m in &self.mobs {
            if let Some(r) = m.extra.rider
                && let Some(&j) = index.get(&r)
                && self.mobs[j].extra.mount == Some(m.id)
                && self.mobs[j].alive()
                && !self.mobs[j].removed
            {
                riding.insert(m.id, r);
            }
        }
        for i in 0..self.mobs.len() {
            let Some(v) = self.mobs[i].extra.mount else {
                continue;
            };
            // Not a text display: a server's display mounts come through
            // `remote_set_passengers`, which seats them in `position_riders`.
            let valid = self.mobs[i].alive()
                && !self.mobs[i].removed
                && (boats.contains(&v)
                    || carts.contains(&v)
                    || tnts.contains(&v)
                    || index
                        .get(&v)
                        .is_some_and(|&j| self.mobs[j].alive() && !self.mobs[j].removed));
            if valid {
                riding.entry(v).or_insert(self.mobs[i].id);
            } else {
                let m = &mut self.mobs[i];
                m.extra.mount = None;
                m.extra.passenger = false;
            }
        }
        for m in &mut self.mobs {
            m.extra.rider = riding.get(&m.id).copied();
        }
        self.position_riders();
    }
    /// `Entity.positionRider` for every mob passenger: in its boat, then on
    /// its mob. A server's client places them itself too, as the server sends
    /// no passenger's position.
    pub(super) fn position_riders(&mut self) {
        self.seat_boat_mobs();
        self.seat_cart_mobs();
        self.seat_tnt_mobs();
        let index: HashMap<u32, usize> = self
            .mobs
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id, i))
            .collect();
        // Riders of riders settle over a few passes.
        for pass in 0..3 {
            for i in 0..self.mobs.len() {
                let Some(v) = self.mobs[i].extra.mount else {
                    continue;
                };
                let Some(&j) = index.get(&v) else {
                    // The display doesn't move between passes.
                    if pass > 0 {
                        continue;
                    }
                    // A mob on a text display: NPC plugins seat their sitting
                    // NPCs on one. The server's position for a passenger
                    // gives way to its seat, as `rideTick` places it.
                    let Some((at, before)) = self.text_displays.get(&(v as i32)).map(|d| d.seat())
                    else {
                        continue;
                    };
                    let m = &mut self.mobs[i];
                    // EntityType.TEXT_DISPLAY's box is zero wide.
                    let offset = riders::vehicle_attachment(m, 0.0);
                    m.position = Vec3::new(at.x, at.y - offset, at.z);
                    m.previous = Vec3::new(before.x, before.y - offset, before.z);
                    m.velocity = Vec3::ZERO;
                    m.fall_distance = 0.0;
                    m.on_ground = false;
                    m.extra.passenger = true;
                    m.ai.nav.stop();
                    continue;
                };
                let vehicle = &self.mobs[j];
                let mut attach = riders::passenger_attachment(vehicle);
                if horizon_client::mobs::camel::is_camel(vehicle.species()) {
                    // Camel.getPassengerAttachmentPoint: the seat follows the
                    // rider's place in `getPassengers` (load order).
                    let mut aboard: Vec<usize> = (0..self.mobs.len())
                        .filter(|&k| self.mobs[k].extra.mount == Some(v))
                        .collect();
                    let player_aboard = vehicle.rider.is_some() || self.riding == Some(v);
                    let (seat, count) = if player_aboard {
                        let mob_seat = aboard.iter().position(|&k| k == i).unwrap_or(0);
                        (mob_seat + 1, aboard.len() + 1)
                    } else {
                        if let Some(driver) = vehicle.extra.rider
                            && let Some(pos) =
                                aboard.iter().position(|&k| self.mobs[k].id == driver)
                            && pos != 0
                        {
                            let driver_idx = aboard.remove(pos);
                            aboard.insert(0, driver_idx);
                        }
                        let seat = aboard.iter().position(|&k| k == i).unwrap_or(0);
                        (seat, aboard.len())
                    };
                    let animal = horizon_client::mobs::drops::is_animal(self.mobs[i].species());
                    attach = horizon_client::mobs::camel::passenger_attachment_at(
                        vehicle, seat, count, animal,
                    );
                }
                if vehicle.species() == horizon_client::mobs::Species::HappyGhast {
                    // EntityType.HAPPY_GHAST: four seats, by the rider's place
                    // in `getPassengers`.
                    let seat = self.mobs[..i]
                        .iter()
                        .filter(|r| r.extra.mount == Some(v))
                        .count();
                    attach = riders::happy_ghast_attachment(seat);
                }
                let (pos, prev) = (vehicle.position, vehicle.previous);
                let (yaw, prev_yaw) = (vehicle.yaw, vehicle.previous_yaw);
                let (body, prev_body) = (vehicle.body_yaw, vehicle.previous_body_yaw);
                let vehicle_width = vehicle.width();
                let m = &mut self.mobs[i];
                let offset = riders::vehicle_attachment(m, vehicle_width);
                m.position = riders::seat(attach, offset, pos, yaw);
                m.previous = riders::seat(attach, offset, prev, prev_yaw);
                m.velocity = Vec3::ZERO;
                m.fall_distance = 0.0;
                m.on_ground = false;
                m.body_yaw = body;
                m.previous_body_yaw = prev_body;
                m.extra.passenger = true;
                // A rider never walks off on its own.
                m.ai.nav.stop();
            }
        }
        // Displays and interaction boxes riding anything the client tracks,
        // as NPC plugins put a name over a head or a model's bones on a
        // pivot: `Entity.rideTick` -> `positionRider`, the vehicle's passenger
        // attachment (the top of its box in its pose and scale) less the rider's own,
        // which is 0 for both (Display and Interaction are 0 by 0). On the
        // local player, the top of its box in its pose and scale
        // (`Avatar.POSES`).
        let me = self.remote.as_ref().map(|r| r.entity_id as u32);
        let own_seat = self.player.as_ref().map(|p| {
            p.position
                .add_vector(Vec3::new(0.0, f64::from(p.dimensions.height), 0.0))
        });
        // A rider of a rider settles over a few passes, as the mobs above.
        for _ in 0..3 {
            let mut vehicles: Vec<u32> = self
                .text_displays
                .values()
                .filter_map(|d| d.vehicle)
                .chain(self.interactions.values().filter_map(|i| i.vehicle))
                .collect();
            vehicles.sort_unstable();
            vehicles.dedup();
            let seats: HashMap<u32, glam::DVec3> = vehicles
                .into_iter()
                .filter_map(|v| {
                    self.passenger_riding_position(v, me, own_seat, &index)
                        .map(|at| (v, at))
                })
                .collect();
            for display in self.text_displays.values_mut() {
                if let Some(at) = display.vehicle.and_then(|v| seats.get(&v)) {
                    display.ride(*at);
                }
            }
            for interaction in self.interactions.values_mut() {
                if let Some(at) = interaction.vehicle.and_then(|v| seats.get(&v)) {
                    interaction.ride_to(*at);
                }
            }
        }
        // Armor stands riding a mob (a server's name tag hologram): seated
        // like the displays above, so the name follows its vehicle instead of
        // staying where the stand spawned. The stand's own vehicle attachment
        // is the default 0, so its feet sit on the seat. The seat at the
        // start of the tick too (`commonTick` runs before `rideTick`), so the
        // stand is drawn between the two even while the level is frozen and
        // its vehicle is not. Those riding the local player are seated after
        // its move (`seat_own_stands`).
        for i in 0..self.armor_stands.len() {
            let Some(vehicle) = self.armor_stands[i].vehicle else {
                continue;
            };
            let Some(&j) = index.get(&vehicle).filter(|_| Some(vehicle) != me) else {
                continue;
            };
            let vehicle = &self.mobs[j];
            let attach = riders::passenger_attachment(vehicle);
            let at = riders::seat(attach, 0.0, vehicle.position, vehicle.yaw);
            let before = riders::seat(attach, 0.0, vehicle.previous, vehicle.previous_yaw);
            let stand = &mut self.armor_stands[i];
            stand.position = at;
            stand.previous = before;
            stand.velocity = Vec3::ZERO;
        }
    }

    /// Armor stands riding the local player, on the top of its box. Vanilla
    /// ticks the player (`tickNonPassenger`) before its passengers
    /// (`tickPassenger` -> `positionRider`), so a rider's `xo` and `x` are the
    /// seats before and after the same move: call this after the player's.
    pub(super) fn seat_own_stands(&mut self) {
        let (Some(me), Some(p)) = (
            self.remote.as_ref().map(|r| r.entity_id as u32),
            self.player.as_ref(),
        ) else {
            return;
        };
        let up = Vec3::new(0.0, f64::from(p.dimensions.height), 0.0);
        let (at, before) = (
            p.position.add_vector(up),
            self.previous_position.add_vector(up),
        );
        for stand in self
            .armor_stands
            .iter_mut()
            .filter(|a| a.vehicle == Some(me))
        {
            stand.position = at;
            stand.previous = before;
            stand.velocity = Vec3::ZERO;
        }
    }

    /// `Entity.getPassengerRidingPosition` of a vehicle: where the feet of a
    /// passenger whose own vehicle attachment is 0 land.
    fn passenger_riding_position(
        &self,
        vehicle: u32,
        me: Option<u32>,
        own_seat: Option<Vec3>,
        index: &HashMap<u32, usize>,
    ) -> Option<glam::DVec3> {
        if Some(vehicle) == me {
            let at = own_seat?;
            return Some(glam::DVec3::new(at.x, at.y, at.z));
        }
        if let Some(&j) = index.get(&vehicle) {
            let v = &self.mobs[j];
            let at = riders::seat(riders::passenger_attachment(v), 0.0, v.position, v.yaw);
            return Some(glam::DVec3::new(at.x, at.y, at.z));
        }
        if let Some(d) = self.text_displays.get(&(vehicle as i32)) {
            return Some(d.position());
        }
        if let Some(i) = self.interactions.get(&(vehicle as i32)) {
            let at = i.position;
            return Some(glam::DVec3::new(at.x, at.y, at.z));
        }
        if let Some(b) = self.boats.iter().find(|b| b.id == vehicle) {
            let count = self.boat_players(vehicle).max(1) + self.boat_mobs(vehicle).len();
            let at = b.seat(0, count, false, 0.0, false);
            return Some(glam::DVec3::new(at.x, at.y, at.z));
        }
        if let Some(c) = self.carts.iter().find(|c| c.id == vehicle) {
            let at = c
                .position
                .add_vector(Vec3::new(0.0, super::minecarts::SEAT, 0.0));
            return Some(glam::DVec3::new(at.x, at.y, at.z));
        }
        if let Some(a) = self.armor_stands.iter().find(|a| a.id == vehicle) {
            // ArmorStand.getDefaultDimensions: a marker has no box to sit on.
            let height = if a.marker {
                0.0
            } else if a.small {
                0.9875
            } else {
                1.975
            };
            return Some(glam::DVec3::new(
                a.position.x,
                a.position.y + height,
                a.position.z,
            ));
        }
        // Any other tracked entity sits on its feet; an area effect cloud is
        // 0.5 tall, and ModelEngine parks a model's pivot half a block below
        // its feet for exactly that.
        let (kind, position) = self.remote.as_ref()?.tracked(vehicle as i32)?;
        let lift = if kind == "minecraft:area_effect_cloud" {
            0.5
        } else {
            0.0
        };
        Some(glam::DVec3::new(
            position[0],
            position[1] + lift,
            position[2],
        ))
    }

    /// `SkeletonHorse.customServerAiStep` and `SkeletonTrapGoal`: a trap
    /// horse is a trap for fifteen minutes; a player within ten blocks springs
    /// it.
    pub(super) fn tick_skeleton_traps(&mut self) {
        // `Level.hasNearbyAlivePlayer`: alive, not a spectator, in the level.
        let players: Vec<_> = self
            .level_players()
            .into_iter()
            .filter(|p| p.alive && !p.spectator)
            .map(|p| p.position)
            .collect();
        let mut sprung = Vec::new();
        for m in &mut self.mobs {
            if m.species() != Species::SkeletonHorse || !m.extra.trap || !m.alive() {
                continue;
            }
            if !m.persistent {
                m.extra.trap_time += 1;
                if m.extra.trap_time > TRAP_LIFETIME {
                    m.removed = true;
                    continue;
                }
            }
            if players
                .iter()
                .any(|&p| m.distance_sqr(p) < TRAP_RANGE * TRAP_RANGE)
            {
                sprung.push(m.id);
            }
        }
        for id in sprung {
            self.spring_skeleton_trap(id);
        }
    }
    fn spring_skeleton_trap(&mut self, id: u32) {
        let Some(horse) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        horse.extra.trap = false;
        horse.extra.tame = true;
        horse.extra.age = 0;
        let at = [horse.position.x, horse.position.y, horse.position.z];
        self.spawn_bolt(at, true);
        let Some(skeleton) = self.trap_skeleton(at) else {
            return;
        };
        self.mount_mob(skeleton, id);
        for _ in 0..3 {
            let Some(other) = self.spawn_mob(
                "minecraft:skeleton_horse",
                at,
                true,
                super::creatures::SpawnSite::Finalizing(
                    horizon_client::mobs::spawn_rules::Reason::Triggered,
                ),
            ) else {
                continue;
            };
            let (dx, dz) = (
                self.world.random.triangle(0.0, 1.1485),
                self.world.random.triangle(0.0, 1.1485),
            );
            if let Some(h) = self.mobs.iter_mut().find(|m| m.id == other) {
                h.invulnerable = TRAP_INVULNERABLE;
                h.extra.tame = true;
                h.extra.age = 0;
                h.velocity = Vec3::new(h.velocity.x + dx, h.velocity.y, h.velocity.z + dz);
            }
            if let Some(rider) = self.trap_skeleton(at) {
                self.mount_mob(rider, other);
            }
        }
    }
    /// `SkeletonTrapGoal.createSkeleton`: an iron helmet unless it already
    /// has a helm.
    fn trap_skeleton(&mut self, at: [f64; 3]) -> Option<u32> {
        let id = self.spawn_mob(
            "minecraft:skeleton",
            at,
            true,
            super::creatures::SpawnSite::Finalizing(
                horizon_client::mobs::spawn_rules::Reason::Triggered,
            ),
        )?;
        let m = self.mobs.iter_mut().find(|m| m.id == id)?;
        m.invulnerable = TRAP_INVULNERABLE;
        if m.extra.head_id().is_none() {
            m.extra.set_slot_id(
                horizon_items::equipment::HEAD,
                Some("minecraft:iron_helmet".into()),
            );
        }
        let finalize = horizon_client::mobs::finalize::item;
        let (weapon, helmet) = (
            finalize(m, horizon_client::mobs::finalize::MAINHAND),
            finalize(m, horizon_client::mobs::finalize::HEAD),
        );
        let special = self.local_difficulty(at).special();
        // `SkeletonTrapGoal.enchant`: the weapon and the helmet, always.
        for (slot, item) in [
            (horizon_client::mobs::gear::MAIN, weapon),
            (horizon_client::mobs::gear::HEAD, helmet),
        ] {
            if let Some(item) = item {
                self.enchant_item(id, slot, &item, special);
            }
        }
        Some(id)
    }
}

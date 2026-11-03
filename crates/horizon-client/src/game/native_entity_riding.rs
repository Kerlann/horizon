//! Vehicles and passengers for native server mods, through an `EntityRef`
//! (docs/native-mods.md, "Entity and level handles", #7387): what an entity
//! rides, who rides it, and getting off. Players ride boats, minecarts and
//! mounts; mobs ride mobs, boats and minecarts. Reads return `None` for a
//! stale handle or on a client.
use super::{Game, commands::Target};
use horizon_client::mod_api::EntityRef;

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

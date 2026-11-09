//! Passenger reads select canonical nominees and actual bodies before pending.
//! The call-local plan contains admitted addresses and seat ordering, not R data.
use super::{
    Game,
    native_base_read_rows::BaseReadSource,
    native_entity_body_provider::EntityBodyProvider,
    native_live_entity_reads::{EntityReadAdmission, EntityReadSelection, EntityReadSubject},
    native_read_dependencies::EntityReadDependency,
};
use mod_api::{EntityReadError, EntityRef, LevelRef, MobRef};

pub(super) struct PassengerReadInput {
    pub(super) admission: EntityReadAdmission,
    order: i64,
}

impl Game<'_> {
    /// Candidate indexes nominate identities only. Every candidate then passes
    /// the sole boundary and its current physical link is read from its body.
    pub(super) fn select_passenger_sources(
        &self,
        vehicle: EntityReadAdmission,
        provider: Option<&dyn EntityBodyProvider>,
        selection: &mut EntityReadSelection,
    ) -> Result<Vec<PassengerReadInput>, EntityReadError> {
        self.with_entity_read_storage(vehicle, provider, |_| {
            selection.include(vehicle);
            let level = LevelRef::new(vehicle.entity().epoch(), vehicle.location().owner.dimension);
            // Completeness is source-owned, independent of an R capture tick.
            let mobs = self
                .entity_read_state
                .borrow()
                .mob_relations
                .get(&level)
                .and_then(|pass| {
                    pass.index
                        .mob_rider_candidates(vehicle.entity().id())
                        .map(|ids| ids.collect::<Vec<_>>())
                })
                .ok_or(EntityReadError::Lifted)?;
            let mut nominees = Vec::with_capacity(mobs.len());
            for id in mobs {
                let entity = EntityRef::new(level.epoch(), id);
                if let Some(admitted) = self.entity_read_admission(
                    EntityReadSubject::OrdinaryMob(MobRef::new(entity)),
                    &[
                        EntityReadDependency::Riding,
                        EntityReadDependency::VitalState,
                    ],
                ) {
                    if admitted.location().owner.dimension == level.dimension() {
                        nominees.push(admitted);
                    }
                }
            }
            for conn in self
                .mob_guest_rider_connections(level, vehicle.entity().id())
                .ok_or(EntityReadError::Lifted)?
            {
                let Some(guest) = self.lan.as_ref().and_then(|host| host.hub.guest(conn)) else {
                    continue;
                };
                if !guest.placed || guest.dimension != level.dimension_id() {
                    continue;
                }
                // This is the actual record's entity ID, never the connection.
                let entity = EntityRef::new(level.epoch(), guest.entity_id as u32);
                let player = mod_api::__private::game::player_ref(entity, guest.uuid);
                if let Some(admitted) = self.entity_read_admission(
                    EntityReadSubject::Player(player),
                    &[EntityReadDependency::Riding],
                ) {
                    if admitted.location().source
                        == horizon_client::join_seq::residency::Source::Guest(conn)
                        && admitted.location().owner.dimension == level.dimension()
                    {
                        nominees.push(admitted);
                    }
                }
            }
            // An acting player outside the placed roster retains the engine's
            // front-seat policy; no profile or UUID is manufactured here.
            if self.riding == Some(vehicle.entity().id())
                && self.player.is_some()
                && self.dim.key() == level.dimension()
                && let (Some(id), Some(uuid)) = (self.lan_acting_entity, self.lan_acting_uuid)
            {
                let entity = EntityRef::new(level.epoch(), id);
                if !nominees.iter().any(|input| input.entity() == entity)
                    && let Some(input) = self.entity_read_admission(
                        EntityReadSubject::Player(mod_api::__private::game::player_ref(
                            entity, uuid,
                        )),
                        &[EntityReadDependency::Riding],
                    )
                {
                    nominees.push(input);
                }
            }
            let mut selected = Vec::with_capacity(nominees.len());
            for input in nominees {
                let order = self
                    .with_entity_read_storage(input, provider, |source| {
                        if self.source_vehicle_id(source.body) != Some(vehicle.entity().id()) {
                            return None;
                        }
                        selection.include(input);
                        match source.body {
                            BaseReadSource::Mob(mob) => {
                                (!mob.removed && mob.alive()).then_some(mob.extra.vehicle_order)
                            }
                            BaseReadSource::Guest(guest) => Some(guest.vehicle_order),
                            BaseReadSource::Player(_) => {
                                let guest = self.entity_read_guest(source.body, input.target());
                                Some(
                                    guest
                                        .filter(|guest| {
                                            guest.placed && guest.dimension == level.dimension_id()
                                        })
                                        .map_or(i64::MIN, |guest| guest.vehicle_order),
                                )
                            }
                            _ => None,
                        }
                    })
                    .ok_or(EntityReadError::Lifted)?;
                if let Some(order) = order {
                    selected.push(PassengerReadInput {
                        admission: input,
                        order,
                    });
                }
            }
            selected.sort_by_key(|input| input.order);
            Ok(selected)
        })
        .unwrap_or(Err(EntityReadError::Lifted))
    }
}

//! Pre-tombstone topology plans and forced post-tombstone application.
//! Removal owns lifecycle ordering; this module never publishes a tombstone.
use super::{
    Game,
    commands::Target,
    native_live_entity_reads::{EntityReadAdmission, EntityReadSelection, EntityReadSubject},
    native_read_dependencies::EntityReadDependency,
    native_topology_parts::{RelationSlot, TopologyRelation},
};
use horizon_client::join_seq::residency::table::EntityLookup;
use mod_api::EntityReadError;

pub(super) struct RemovalTopologyPlan {
    pub(super) own_ride: Option<RemovalDetachPlan>,
    pub(super) passengers: Vec<RemovalDetachPlan>,
}

pub(super) struct RemovalDetachPlan {
    removed: EntityReadAdmission,
    rider: TopologyRelation,
    vehicle: Option<EntityReadAdmission>,
    vehicle_id: u32,
}

impl Game<'_> {
    /// Select/append topology parts only. The public remove boundary probes
    /// its remaining effect parts, then projects Pending exactly once before
    /// tombstoning, detaching own ride, and detaching passengers.
    pub(super) fn preflight_removal_topology(
        &mut self,
        receiver: EntityReadAdmission,
        destroy_own_ride: bool,
        selection: &mut EntityReadSelection,
    ) -> Result<RemovalTopologyPlan, EntityReadError> {
        self.with_entity_read_storage(receiver, None, |_| ())
            .ok_or(EntityReadError::Lifted)?;
        selection.include(receiver);
        let own_ride = if destroy_own_ride {
            self.select_topology_stop_parts(receiver, None, selection)?
                .map(|stop| RemovalDetachPlan {
                    removed: receiver,
                    rider: stop.rider,
                    vehicle: stop.vehicle,
                    vehicle_id: stop.vehicle_id,
                })
        } else {
            None
        };
        let candidates = self.select_passenger_sources(receiver, None, selection)?;
        if !candidates.is_empty() {
            self.with_entity_read_storage(receiver, None, |source| self.live_source_geometry(source.body))
                .flatten().ok_or(EntityReadError::Lifted)?;
        }
        if matches!(receiver.target(), Target::Mob(_)) {
            self.select_topology_relation(receiver)?
                .ok_or(EntityReadError::Lifted)?;
        }
        let mut passengers = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let rider = self
                .select_topology_relation(candidate.admission)?
                .ok_or(EntityReadError::Lifted)?;
            passengers.push(RemovalDetachPlan {
                removed: receiver,
                rider,
                vehicle: Some(receiver),
                vehicle_id: receiver.entity().id(),
            });
        }
        // Player effect parts still need their direct owning-source adapter.
        // This explicit draft gap must refuse BEFORE the caller tombstones.
        if own_ride
            .iter()
            .chain(passengers.iter())
            .any(|plan| plan.rider.slot != RelationSlot::Mob)
        {
            return Err(EntityReadError::Lifted);
        }
        Ok(RemovalTopologyPlan {
            own_ride,
            passengers,
        })
    }

    /// `Entity.setRemoved`: removal detach does not fire the mount veto.
    /// Only this plan's exact expected tombstone may bypass logical admission.
    pub(super) fn detach_removed_topology(
        &mut self,
        plan: RemovalDetachPlan,
    ) -> Result<bool, EntityReadError> {
        let rider = if plan.rider.admission.entity() == plan.removed.entity() {
            plan.rider.admission
        } else {
            let Some(now) = self.entity_read_admission(
                EntityReadSubject::Entity(plan.rider.admission.entity()),
                &[EntityReadDependency::Riding, EntityReadDependency::Geometry],
                None,
            ) else {
                return Ok(false);
            };
            if now.location() != plan.rider.admission.location() {
                return Ok(false);
            }
            now
        };
        // This is the existing PHYSICAL canonical lookup, not public handle
        // re-admission. Its visibility bit is deliberately separate.
        let Some((at, state)) = self
            .entity_residency
            .borrow_mut()
            .server_admitted_location(plan.removed.entity().id(), |at| {
                self.locate_server_resident_owner(at)
            })
        else {
            return Err(EntityReadError::Lifted);
        };
        if at != plan.removed.location() {
            return Ok(false);
        }
        match state {
            EntityLookup::Taken => return Err(EntityReadError::Lifted),
            EntityLookup::Missing => return Err(EntityReadError::Lifted),
            EntityLookup::Found {
                value: (_, visible),
                ..
            } => {
                if visible
                    && self.server_read_target_visible(at.owner.dimension, plan.removed.target())
                {
                    return Ok(false);
                }
            }
        }
        let relation = self
            .select_topology_relation(rider)?
            .ok_or(EntityReadError::Lifted)?;
        if self.topology_vehicle(relation)? != Some(plan.vehicle_id) {
            return Ok(false);
        }
        let retained = plan
            .vehicle
            .filter(|vehicle| vehicle.entity() == plan.removed.entity());
        let mut selection = EntityReadSelection::default();
        let Some(fresh) = self.select_topology_stop_parts(rider, retained, &mut selection)? else {
            return Ok(false);
        };
        if fresh.vehicle.map(|vehicle| vehicle.location())
            != plan.vehicle.map(|vehicle| vehicle.location())
        {
            return Ok(false);
        }
        let placement = self.with_entity_read_storage(fresh.rider.admission, None, |rider| {
            let super::native_base_read_rows::BaseReadSource::Mob(mob) = rider.body else { return Err(EntityReadError::Lifted); };
            // LivingEntity.dismountVehicle keeps a removed rider where it is.
            if fresh.rider.admission.entity() == plan.removed.entity() {
                return selection.project(|| mob.position);
            }
            let vehicle = fresh.vehicle.ok_or(EntityReadError::Lifted)?;
            self.with_entity_read_storage(vehicle, None, |source| {
                let geometry = self.live_source_geometry(source.body).ok_or(EntityReadError::Lifted)?;
                // Required source geometry and owning world are known before
                // Pending; collision search runs only once the fence passes.
                selection.project(|| self.removed_vehicle_dismount_at(source.level.world,
                    source.level.border, mob.position, f64::from(mob.width()),
                    f64::from(mob.height()), geometry.position[1]))
            }).unwrap_or(Err(EntityReadError::Lifted))
        }).unwrap_or(Err(EntityReadError::Lifted))?;
        match fresh.rider.slot {
            RelationSlot::Mob => self.apply_topology_mob_stop(fresh, Some(placement)),
            _ => Err(EntityReadError::Lifted),
        }
    }
}

//! Operation-specific mutable relation storage. Plans retain addresses, never
//! bodies or iterator positions. Immutable H providers are not mutation authority.
use super::{
    Game, commands::Target, native_base_read_rows::BaseReadSource,
    native_live_entity_reads::EntityReadAdmission,
};
use horizon_client::{join_seq::Joined, mobs::Mob};
use mod_api::EntityReadError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RelationSlot {
    Mob,
    CurrentPlayer,
    Guest(u32),
}

#[derive(Clone, Copy)]
pub(super) struct TopologyRelation {
    pub(super) admission: EntityReadAdmission,
    pub(super) slot: RelationSlot,
}

impl Game<'_> {
    /// Preflight for eject and discard: qualify the receiver, select complete
    /// actual passenger links and every mutable relation, then choose Pending.
    /// Returned plans retain no body borrow or index; callers must check joins
    /// and decided links again after each callback.
    pub(super) fn preflight_topology_passengers(
        &mut self,
        entity: mod_api::EntityRef,
    ) -> Result<Option<Vec<TopologyRelation>>, EntityReadError> {
        use super::{
            native_live_entity_reads::{EntityReadSelection, EntityReadSubject},
            native_read_dependencies::EntityReadDependency,
        };
        let Some(vehicle) = self.entity_read_admission(
            EntityReadSubject::Entity(entity),
            &[EntityReadDependency::Riding, EntityReadDependency::Geometry],
            None,
        ) else {
            return Ok(None);
        };
        let mut selection = EntityReadSelection::default();
        let passengers = self.select_passenger_sources(vehicle, None, &mut selection)?;
        if matches!(vehicle.target(), Target::Mob(_)) {
            self.select_topology_relation(vehicle)?
                .ok_or(EntityReadError::Lifted)?;
        }
        let mut selected = Vec::with_capacity(passengers.len());
        for passenger in passengers {
            selected.push(
                self.select_topology_relation(passenger.admission)?
                    .ok_or(EntityReadError::Lifted)?,
            );
        }
        selection.project(|| Some(selected))
    }

    /// Select exclusive canonical storage without touching a mutation observer.
    /// Pending is deliberately not inspected until every required part exists.
    pub(super) fn select_topology_relation(
        &mut self,
        admission: EntityReadAdmission,
    ) -> Result<Option<TopologyRelation>, EntityReadError> {
        if !matches!(
            admission.target(),
            Target::Mob(_) | Target::Player | Target::Guest(_)
        ) {
            return Ok(None);
        }
        let at = admission.location();
        let id = admission.entity().id();
        let slot = if matches!(admission.target(), Target::Mob(_)) {
            admission.require_stored_source()?;
            let storage = self
                .topology_mobs_mut(at.owner.dimension)
                .ok_or(EntityReadError::Lifted)?;
            if !storage
                .iter()
                .any(|mob| mob.id == id && mob.join_seq() == at.join)
            {
                return Err(EntityReadError::Lifted);
            }
            RelationSlot::Mob
        } else {
            // This establishes the actual role of the canonical player slot,
            // not permission derived from an actor frame or an immutable loan.
            let source = self
                .resident_read_source(at)
                .ok_or(EntityReadError::Lifted)?;
            if source.id != id {
                return Err(EntityReadError::Lifted);
            }
            // A real guest turn can hold its extracted body in Game.player.
            // The role resolver checked exact residency and actual entity id;
            // that owned mutable field is proof, unlike a readonly H provider.
            let slot = match source.source {
                BaseReadSource::Player(_) => RelationSlot::CurrentPlayer,
                BaseReadSource::Guest(guest) => RelationSlot::Guest(guest.conn),
                _ => return Err(EntityReadError::Lifted),
            };
            match slot {
                RelationSlot::CurrentPlayer if self.player.is_none() => {
                    return Err(EntityReadError::Lifted);
                }
                RelationSlot::Guest(_) if admission.require_stored_source().is_err() => {
                    return Err(EntityReadError::Lifted);
                }
                RelationSlot::Guest(conn)
                    if self
                        .lan
                        .as_mut()
                        .and_then(|host| host.hub.guest(conn))
                        .is_none_or(|guest| guest.motion.is_none()) =>
                {
                    return Err(EntityReadError::Lifted);
                }
                _ => {}
            }
            slot
        };
        Ok(Some(TopologyRelation { admission, slot }))
    }

    /// Apply through a freshly located exact join, after pending and callback
    /// preconditions have been checked. No index survives a callback.
    pub(super) fn with_topology_mob_mut<R>(
        &mut self,
        admission: EntityReadAdmission,
        apply: impl FnOnce(&mut Mob) -> R,
    ) -> Result<R, EntityReadError> {
        admission.require_stored_source()?;
        let at = admission.location();
        let id = admission.entity().id();
        let result = {
            let storage = self
                .topology_mobs_mut(at.owner.dimension)
                .ok_or(EntityReadError::Lifted)?;
            let index = storage
                .iter()
                .position(|mob| mob.id == id && mob.join_seq() == at.join)
                .ok_or(EntityReadError::Lifted)?;
            apply(storage.get_mut(index).ok_or(EntityReadError::Lifted)?)
        };
        // Publish only source-owned relation metadata from the actual owning
        // level; never use the caller's current dimension as a hint.
        if let Some(level) = self.entity_read_level(at.owner.dimension)
            && let Some(mob) = level
                .mobs
                .iter()
                .find(|mob| mob.id == id && mob.join_seq() == at.join)
            && let Some(pass) =
                self.entity_read_state
                    .borrow_mut()
                    .mob_relations
                    .get_mut(&mod_api::LevelRef::new(
                        admission.entity().epoch(),
                        at.owner.dimension,
                    ))
        {
            pass.index.update_mob_rider(mob);
        }
        Ok(result)
    }

    pub(super) fn topology_vehicle(
        &mut self,
        relation: TopologyRelation,
    ) -> Result<Option<u32>, EntityReadError> {
        let selected = self
            .select_topology_relation(relation.admission)?
            .ok_or(EntityReadError::Lifted)?;
        if selected.slot != relation.slot {
            return Err(EntityReadError::Lifted);
        }
        match relation.slot {
            RelationSlot::Mob => {
                let at = relation.admission.location();
                let storage = self
                    .topology_mobs_mut(at.owner.dimension)
                    .ok_or(EntityReadError::Lifted)?;
                Ok(storage
                    .iter()
                    .find(|mob| {
                        mob.id == relation.admission.entity().id() && mob.join_seq() == at.join
                    })
                    .ok_or(EntityReadError::Lifted)?
                    .extra
                    .mount)
            }
            RelationSlot::CurrentPlayer => Ok(self.riding),
            RelationSlot::Guest(conn) => Ok(self
                .lan
                .as_ref()
                .and_then(|host| host.hub.guest(conn))
                .ok_or(EntityReadError::Lifted)?
                .vehicle),
        }
    }
}

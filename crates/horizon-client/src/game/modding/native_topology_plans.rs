//! Call-local topology admission and part selection, before any veto or effect.
use super::{
    Game,
    commands::Target,
    native_live_entity_reads::{EntityReadAdmission, EntityReadSelection, EntityReadSubject},
    native_read_dependencies::EntityReadDependency,
    native_topology_parts::TopologyRelation,
};
use mod_api::{EntityReadError, EntityRef};

pub(super) struct MountPlan {
    pub(super) rider: TopologyRelation,
    pub(super) vehicle: EntityReadAdmission,
    old_vehicle: Option<u32>,
    passengers: Vec<(EntityReadAdmission, i64)>,
}

pub(super) struct StopPlan {
    pub(super) rider: TopologyRelation,
    pub(super) vehicle: Option<EntityReadAdmission>,
    pub(super) vehicle_id: u32,
    pub(super) passengers: Vec<EntityReadAdmission>,
}

impl Game<'_> {
    fn topology_mob_mount_allowed(
        &self,
        plan: &MountPlan,
        force: bool,
    ) -> Result<bool, EntityReadError> {
        use super::native_base_read_rows::BaseReadSource;
        let rider_allowed = self
            .with_entity_read_storage(plan.rider.admission, None, |source| {
                let BaseReadSource::Mob(mob) = source.body else {
                    return false;
                };
                force
                    || (mob.extra.boarding_cooldown <= 0
                        && !matches!(
                            mob.species(),
                            horizon_client::mobs::Species::Warden
                                | horizon_client::mobs::Species::Wither
                        ))
            })
            .ok_or(EntityReadError::Lifted)?;
        if !rider_allowed {
            return Ok(false);
        }
        let mobs = plan
            .passengers
            .iter()
            .filter(|(passenger, _)| matches!(passenger.target(), Target::Mob(_)))
            .count();
        let players = plan.passengers.len() - mobs;
        self.with_entity_read_storage(plan.vehicle, None, |source| match source.body {
            BaseReadSource::Boat(boat) => {
                plan.passengers.len() < boat.max_passengers() && (force || !boat.under_water())
            }
            BaseReadSource::Cart(_) => plan.passengers.is_empty(),
            BaseReadSource::Mob(mob) => {
                force
                    || horizon_client::mobs::riders::can_add_passenger(
                        mob,
                        players.max(usize::from(mob.rider.is_some()))
                            + mobs.max(usize::from(mob.extra.rider.is_some())),
                    )
            }
            BaseReadSource::Stand(_) | BaseReadSource::Other(_) => {
                force || plan.passengers.is_empty()
            }
            _ => false,
        })
        .ok_or(EntityReadError::Lifted)
    }

    fn topology_mount_parts_unchanged(old: &MountPlan, new: &MountPlan) -> bool {
        old.rider.admission.location() == new.rider.admission.location()
            && old.vehicle.location() == new.vehicle.location()
            && old
                .passengers
                .iter()
                .map(|(part, order)| (part.entity(), part.location(), *order))
                .eq(new
                    .passengers
                    .iter()
                    .map(|(part, order)| (part.entity(), part.location(), *order)))
    }

    pub(super) fn execute_topology_mob_mount(
        &mut self,
        plan: MountPlan,
        force: bool,
    ) -> Result<bool, EntityReadError> {
        use super::native_base_read_source::base_read_source_kind;
        if !self.topology_mob_mount_allowed(&plan, force)? {
            return Ok(false);
        }
        let entity = plan.rider.admission.entity();
        let vehicle = plan.vehicle.entity();
        let kind = self
            .with_entity_read_storage(plan.rider.admission, None, |source| {
                base_read_source_kind(
                    source.body,
                    &mut self.entity_read_state.borrow_mut().kind_pool,
                )
            })
            .ok_or(EntityReadError::Lifted)?;
        let vehicle_kind = self
            .with_entity_read_storage(plan.vehicle, None, |source| {
                base_read_source_kind(
                    source.body,
                    &mut self.entity_read_state.borrow_mut().kind_pool,
                )
            })
            .ok_or(EntityReadError::Lifted)?;
        let level = mod_api::LevelRef::new(
            entity.epoch(),
            plan.rider.admission.location().owner.dimension,
        );
        let mut canceled = false;
        if self.native_hears_mounts() {
            self.native_entity_mount(mod_api::__private::EntityMountCall {
                dimension: level.dimension_id(),
                entity: entity.id(),
                kind: &kind,
                vehicle: vehicle.id(),
                vehicle_kind: &vehicle_kind,
                mounting: true,
                canceled: &mut canceled,
            });
        }
        if canceled {
            return Ok(false);
        }
        let Some(now) = self.entity_read_admission(EntityReadSubject::Entity(entity), &[], None)
        else {
            return Ok(false);
        };
        if now.location() != plan.rider.admission.location() {
            return Ok(false);
        }
        let relation = self
            .select_topology_relation(now)?
            .ok_or(EntityReadError::Lifted)?;
        if self.topology_vehicle(relation)? != plan.old_vehicle {
            return Ok(false);
        }
        let Some(fresh) = self.prepare_topology_mount(entity, vehicle)? else {
            return Ok(false);
        };
        if !Self::topology_mount_parts_unchanged(&plan, &fresh)
            || !self.topology_mob_mount_allowed(&fresh, force)?
        {
            return Ok(false);
        }
        let fresh = if plan.old_vehicle.is_some() {
            if !self.entity_stop_riding(entity)? {
                return Ok(false);
            }
            // Dismount has its own veto. Check the original destination
            // proposal again only when that additional callback ran.
            let Some(after) = self.prepare_topology_mount(entity, vehicle)? else {
                return Ok(false);
            };
            if !Self::topology_mount_parts_unchanged(&plan, &after)
                || self.topology_vehicle(after.rider)?.is_some()
                || !self.topology_mob_mount_allowed(&after, force)?
            {
                return Ok(false);
            }
            after
        } else {
            fresh
        };
        let order = fresh
            .passengers
            .last()
            .map_or(0, |(_, order)| order.wrapping_add(1));
        self.with_topology_mob_mut(fresh.rider.admission, |mob| {
            horizon_client::mobs::riders::link_rider(mob, vehicle.id());
            mob.extra.vehicle_order = order;
        })?;
        if matches!(fresh.vehicle.target(), Target::Mob(_)) {
            self.with_topology_mob_mut(fresh.vehicle, |mob| {
                horizon_client::mobs::riders::add_first_rider(mob, entity.id());
            })?;
        }
        Ok(true)
    }

    pub(super) fn execute_topology_mob_stop(
        &mut self,
        plan: StopPlan,
    ) -> Result<bool, EntityReadError> {
        use super::native_base_read_source::base_read_source_kind;
        let entity = plan.rider.admission.entity();
        let kind = self
            .with_entity_read_storage(plan.rider.admission, None, |source| {
                base_read_source_kind(
                    source.body,
                    &mut self.entity_read_state.borrow_mut().kind_pool,
                )
            })
            .ok_or(EntityReadError::Lifted)?;
        let vehicle_kind = match plan.vehicle {
            Some(vehicle) => self
                .with_entity_read_storage(vehicle, None, |source| {
                    base_read_source_kind(
                        source.body,
                        &mut self.entity_read_state.borrow_mut().kind_pool,
                    )
                })
                .ok_or(EntityReadError::Lifted)?,
            None => std::sync::Arc::from(""),
        };
        let level = mod_api::LevelRef::new(
            entity.epoch(),
            plan.rider.admission.location().owner.dimension,
        );
        let mut canceled = false;
        if self.native_hears_mounts() {
            self.native_entity_mount(mod_api::__private::EntityMountCall {
                dimension: level.dimension_id(),
                entity: entity.id(),
                kind: &kind,
                vehicle: plan.vehicle_id,
                vehicle_kind: &vehicle_kind,
                mounting: false,
                canceled: &mut canceled,
            });
        }
        if canceled {
            return Ok(false);
        }
        // Callback changes are refusal, never permission to choose a different
        // edge, body generation or owning level.
        let Some(now) = self.entity_read_admission(EntityReadSubject::Entity(entity), &[], None)
        else {
            return Ok(false);
        };
        if now.location() != plan.rider.admission.location() {
            return Ok(false);
        }
        let relation = self
            .select_topology_relation(now)?
            .ok_or(EntityReadError::Lifted)?;
        if self.topology_vehicle(relation)? != Some(plan.vehicle_id) {
            return Ok(false);
        }
        let Some(fresh) = self.prepare_topology_stop(entity)? else {
            return Ok(false);
        };
        if fresh.vehicle.map(|vehicle| vehicle.location())
            != plan.vehicle.map(|vehicle| vehicle.location())
        {
            return Ok(false);
        }
        self.apply_topology_mob_stop(fresh, None)
    }

    pub(super) fn apply_topology_mob_stop(
        &mut self,
        fresh: StopPlan,
        placement: Option<horizon_core::geometry::Vec3>,
    ) -> Result<bool, EntityReadError> {
        use super::native_base_read_rows::BaseReadSource;
        let entity = fresh.rider.admission.entity();
        self.with_topology_mob_mut(fresh.rider.admission, |mob| mob.extra.mount = None)?;
        if let Some(vehicle) = fresh
            .vehicle
            .filter(|vehicle| matches!(vehicle.target(), Target::Mob(_)))
        {
            let remaining = fresh
                .passengers
                .iter()
                .find(|passenger| {
                    passenger.entity() != entity && matches!(passenger.target(), Target::Mob(_))
                })
                .map(|passenger| passenger.entity().id());
            self.with_topology_mob_mut(vehicle, |mob| mob.extra.rider = remaining)?;
        }
        let at = if let Some(placement) = placement { placement } else { self
            .with_entity_read_storage(fresh.rider.admission, None, |rider| {
                let BaseReadSource::Mob(rider) = rider.body else {
                    return Err(EntityReadError::Lifted);
                };
                let Some(vehicle) = fresh.vehicle else {
                    return Ok(rider.position);
                };
                self.with_entity_read_storage(vehicle, None, |source| {
                    let world = source.level.world;
                    let border = source.level.border;
                    Ok(match source.body {
                        BaseReadSource::Boat(boat) => self.boat_dismount_target_at(
                            world,
                            boat,
                            f64::from(rider.width()),
                            rider.yaw,
                        ),
                        BaseReadSource::Cart(cart) => self
                            .cart_dismount_mob_at(world, border, cart, rider)
                            .unwrap_or(rider.position),
                        BaseReadSource::Mob(mount) => self
                            .horse_dismount_mobs_at(world, border, mount, rider)
                            .unwrap_or_else(|| super::mounts::on_top(mount)),
                        _ => rider.position,
                    })
                })
                .unwrap_or(Err(EntityReadError::Lifted))
            })
            .unwrap_or(Err(EntityReadError::Lifted))? };
        self.with_topology_mob_mut(fresh.rider.admission, |mob| {
            horizon_client::mobs::riders::finish_dismount(
                mob,
                at,
                super::boat_passengers::BOARDING_COOLDOWN,
            );
        })?;
        Ok(true)
    }

    pub(super) fn prepare_topology_stop(
        &mut self,
        entity: EntityRef,
    ) -> Result<Option<StopPlan>, EntityReadError> {
        let Some(rider) = self.entity_read_admission(
            EntityReadSubject::Entity(entity),
            &[EntityReadDependency::Riding, EntityReadDependency::Geometry],
            None,
        ) else {
            return Ok(None);
        };
        let mut selection = EntityReadSelection::default();
        let plan = self.select_topology_stop_parts(rider, None, &mut selection)?;
        selection.project(|| plan)
    }

    /// Select parts only. Removal combines every detach before one Pending projection.
    pub(super) fn select_topology_stop_parts(
        &mut self,
        rider: EntityReadAdmission,
        retained_vehicle: Option<EntityReadAdmission>,
        selection: &mut EntityReadSelection,
    ) -> Result<Option<StopPlan>, EntityReadError> {
        let Some(rider) = self.select_topology_relation(rider)? else {
            return Ok(None);
        };
        selection.include(rider.admission);
        let Some(vehicle_id) = self.topology_vehicle(rider)? else {
            return Ok(None);
        };
        if retained_vehicle.is_some_and(|vehicle| {
            vehicle.entity().id() != vehicle_id
                || vehicle.location().owner.dimension != rider.admission.location().owner.dimension
        }) {
            return Ok(None);
        }
        let vehicle = retained_vehicle.or_else(|| {
            self.entity_read_admission(
                EntityReadSubject::Entity(EntityRef::new(
                    rider.admission.entity().epoch(),
                    vehicle_id,
                )),
                &[EntityReadDependency::Riding, EntityReadDependency::Geometry],
                None,
            )
            .filter(|vehicle| {
                vehicle.location().owner.dimension == rider.admission.location().owner.dimension
            })
        });
        let mut passengers = Vec::new();
        if let Some(vehicle) = vehicle {
            self.with_entity_read_storage(vehicle, None, |_| ())
                .ok_or(EntityReadError::Lifted)?;
            selection.include(vehicle);
            if matches!(vehicle.target(), Target::Mob(_)) {
                self.select_topology_relation(vehicle)?
                    .ok_or(EntityReadError::Lifted)?;
                passengers = self
                    .select_passenger_sources(vehicle, None, selection)?
                    .into_iter()
                    .map(|passenger| passenger.admission)
                    .collect();
            }
        }
        Ok(Some(StopPlan {
            rider,
            vehicle,
            vehicle_id,
            passengers,
        }))
    }

    pub(super) fn prepare_topology_mount(
        &mut self,
        entity: EntityRef,
        vehicle: EntityRef,
    ) -> Result<Option<MountPlan>, EntityReadError> {
        let Some(rider) = self.entity_read_admission(
            EntityReadSubject::Entity(entity),
            &[
                EntityReadDependency::Riding,
                EntityReadDependency::Geometry,
                EntityReadDependency::PlayerState,
            ],
            None,
        ) else {
            return Ok(None);
        };
        let Some(vehicle) = self.entity_read_admission(
            EntityReadSubject::Entity(vehicle),
            &[EntityReadDependency::Riding, EntityReadDependency::Geometry],
            None,
        ) else {
            return Ok(None);
        };
        // Pair qualification precedes physical access, including an unrelated
        // Taken body on the other side of an unsupported pair.
        if !matches!(
            rider.target(),
            Target::Mob(_) | Target::Player | Target::Guest(_)
        ) || !matches!(
            vehicle.target(),
            Target::Mob(_)
                | Target::Boat(_)
                | Target::Cart(_)
                | Target::Stand(_)
                | Target::Other(_)
        ) || rider.entity() == vehicle.entity()
            || rider.location().owner.dimension != vehicle.location().owner.dimension
        {
            return Ok(None);
        }
        let rider = self
            .select_topology_relation(rider)?
            .ok_or(EntityReadError::Lifted)?;
        let mut selection = EntityReadSelection::default();
        selection.include(rider.admission);
        let old = self.topology_vehicle(rider)?;
        if old == Some(vehicle.entity().id()) {
            return selection.project(|| None);
        }
        // Capacity and ordering require complete actual source nominations.
        let passengers = self
            .select_passenger_sources(vehicle, None, &mut selection)?
            .into_iter()
            .map(|passenger| (passenger.admission, passenger.order))
            .collect();
        if matches!(vehicle.target(), Target::Mob(_)) {
            self.select_topology_relation(vehicle)?
                .ok_or(EntityReadError::Lifted)?;
        }
        if let Some(old) = old {
            if let Some(old) = self
                .entity_read_admission(
                    EntityReadSubject::Entity(EntityRef::new(entity.epoch(), old)),
                    &[EntityReadDependency::Riding, EntityReadDependency::Geometry],
                    None,
                )
                .filter(|old| {
                    old.location().owner.dimension == rider.admission.location().owner.dimension
                })
            {
                self.with_entity_read_storage(old, None, |_| ())
                    .ok_or(EntityReadError::Lifted)?;
                selection.include(old);
                if matches!(old.target(), Target::Mob(_)) {
                    self.select_topology_relation(old)?
                        .ok_or(EntityReadError::Lifted)?;
                    self.select_passenger_sources(old, None, &mut selection)?;
                }
            }
        }
        let mut current = Some(vehicle);
        let mut visited = Vec::new();
        while let Some(part) = current {
            if part.entity() == entity || visited.contains(&part.entity()) {
                return selection.project(|| None);
            }
            visited.push(part.entity());
            let next = self
                .with_entity_read_storage(part, None, |source| self.source_vehicle_id(source.body))
                .ok_or(EntityReadError::Lifted)?;
            selection.include(part);
            current = next
                .and_then(|id| {
                    self.entity_read_admission(
                        EntityReadSubject::Entity(EntityRef::new(entity.epoch(), id)),
                        &[EntityReadDependency::Riding],
                        None,
                    )
                })
                .filter(|next| {
                    next.location().owner.dimension == rider.admission.location().owner.dimension
                });
        }
        selection.project(|| ())?;
        Ok(Some(MountPlan {
            rider,
            vehicle,
            old_vehicle: old,
            passengers,
        }))
    }
}

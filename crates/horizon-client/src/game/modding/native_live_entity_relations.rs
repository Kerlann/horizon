//! Heavy relation kernels over actual bodies. Related headers use the primary
//! membership table; neither cached relationship payloads nor live AI peers supply them.
use super::{
    Game, native_base_read_rows::BaseReadSource, native_entity_body_provider::EntityBodyProvider,
    native_live_entity_reads::EntityReadSubject, native_read_dependencies::EntityReadDependency,
};
use mod_api::{DimensionKey, EntityReadError, EntityRef};

impl Game<'_> {
    pub(super) fn source_vehicle_id(&self, source: BaseReadSource<'_>) -> Option<u32> {
        match source {
            BaseReadSource::Player(_) => self.riding,
            BaseReadSource::Guest(guest) => guest.vehicle,
            BaseReadSource::Mob(mob) => mob.extra.mount,
            _ => None,
        }
    }

    /// A relation needs the other entity's identity, not permission to read its
    /// body. Admission validates that candidate without creating a UUID or R row.
    pub(super) fn admitted_relation_entity(
        &self,
        dimension: DimensionKey,
        id: u32,
    ) -> Option<EntityRef> {
        let entity = EntityRef::new(self.native_lifecycle.epoch()?, id);
        let admitted = self.entity_read_admission(EntityReadSubject::Entity(entity), &[])?;
        (admitted.location().owner.dimension == dimension).then_some(entity)
    }

    /// Outer absence is an invalid receiver; inner absence is a valid non-rider.
    pub(super) fn read_vehicle_relation(
        &self,
        provider: Option<&dyn EntityBodyProvider>,
        entity: EntityRef,
    ) -> Result<Option<Option<EntityRef>>, EntityReadError> {
        if !self.is_logical_server() {
            return Ok(None);
        }
        let Some(admitted) = self.entity_read_admission(
            EntityReadSubject::Entity(entity),
            &[EntityReadDependency::Riding],
        ) else {
            return Ok(None);
        };
        let dimension = admitted.location().owner.dimension;
        self.with_admitted_entity_source(admitted, provider, |source| {
            Some(
                self.source_vehicle_id(source.body)
                    .and_then(|id| self.admitted_relation_entity(dimension, id)),
            )
        })
    }

    /// Continue after a validated first link, without admitting the initial
    /// receiver a second time. Preserve the existing cycle guard.
    pub(super) fn root_vehicle_after_first(
        &self,
        provider: Option<&dyn EntityBodyProvider>,
        entity: EntityRef,
        first: Option<EntityRef>,
    ) -> Result<Option<EntityRef>, EntityReadError> {
        let Some(mut root) = first else {
            return Ok(Some(entity));
        };
        for _ in 1..64 {
            let Some(next) = self.read_vehicle_relation(provider, root)? else {
                return Ok(None);
            };
            match next {
                Some(vehicle) => root = vehicle,
                None => break,
            }
        }
        Ok(Some(root))
    }

    /// Resolve team inheritance from actual owner bodies. The existing UUID
    /// index discovers identities only; it never supplies tame/owner payloads.
    pub(super) fn native_source_team<'g>(
        &'g self,
        source: super::native_live_entity_sources::EntityReadSource<'_>,
        level: mod_api::LevelRef,
        provider: Option<&dyn EntityBodyProvider>,
    ) -> Result<Option<&'g horizon_client_core::scoreboard::Team>, EntityReadError> {
        use super::commands::Target;
        let mob = match source.body {
            BaseReadSource::Player(_) => return Ok(self.player_team()),
            BaseReadSource::Guest(guest) => return Ok(self.score.board.players_team(&guest.name)),
            BaseReadSource::Mob(mob) => mob,
            _ => return Ok(None),
        };
        let Some(&uuid) = source.level.persist.uuids.get(&mob.id) else {
            return Ok(None);
        };
        let own = horizon_client::save::uuid_text(uuid);
        if let Some(team) = self.score.board.players_team(&own) {
            return Ok(Some(team));
        }
        let Some(mut root) = mob.extra.tame.then_some(mob.extra.owner).flatten() else {
            return Ok(None);
        };
        // Preserve the engine's existing guard, including its final root name.
        let mut remaining = 64;
        loop {
            let Some((saved, profile)) = self.relation_identity_candidates_at(level, root) else {
                return Ok(None);
            };
            let local = self
                .local_player_residency
                .identity()
                .filter(|identity| identity.details.known_uuid == Some(root))
                .map(|identity| identity.id.get());
            let Some(id) = local.or(profile).or(saved) else {
                return Ok(None);
            };
            let entity = EntityRef::new(level.epoch(), id);
            let Some(admitted) = self.entity_read_admission(
                EntityReadSubject::Entity(entity),
                &[
                    EntityReadDependency::Teams,
                    EntityReadDependency::Ownership,
                    EntityReadDependency::Session,
                ],
            ) else {
                return Ok(None);
            };
            if matches!(admitted.target(), Target::Player | Target::Guest(_)) {
                if admitted.known_uuid() != Some(root) {
                    return Ok(None);
                }
                return self.with_admitted_entity_source(admitted, provider, |owner| {
                    match owner.body {
                        BaseReadSource::Player(_) => self.player_team(),
                        BaseReadSource::Guest(guest) => self.score.board.players_team(&guest.name),
                        _ => None,
                    }
                });
            }
            if admitted.location().owner.dimension != level.dimension() {
                return Ok(None);
            }
            let known = admitted.known_uuid().or_else(|| {
                self.entity_read_level(level.dimension())?
                    .persist
                    .uuids
                    .get(&id)
                    .copied()
            });
            if known != Some(root) {
                return Ok(None);
            }
            if remaining == 0 || !matches!(admitted.target(), Target::Mob(_)) {
                return Ok(self
                    .score
                    .board
                    .players_team(&horizon_client::save::uuid_text(root)));
            }
            remaining -= 1;
            let next =
                self.with_admitted_entity_source(admitted, provider, |owner| match owner.body {
                    BaseReadSource::Mob(mob) => mob.extra.tame.then_some(mob.extra.owner).flatten(),
                    _ => None,
                })?;
            match next {
                Some(next) => root = next,
                None => {
                    return Ok(self
                        .score
                        .board
                        .players_team(&horizon_client::save::uuid_text(root)));
                }
            }
        }
    }
}

//! The normalized NoGravity operation uses the world's existing FIFO envelope.
use super::{
    Game,
    commands::Target as EntityTarget,
    native_entity_commands::{EntityCommands, EntityOperation},
};
use horizon_client::join_seq::residency::{Kind, table::EntityLookup, write_queue::Target};
use mod_api::*;

fn family_target(kind: Kind, id: u32) -> Result<EntityTarget, EntityWriteError> {
    Ok(match kind {
        Kind::Mob => EntityTarget::Mob(id),
        Kind::Item => EntityTarget::Item(id),
        Kind::Orb => EntityTarget::Orb(id),
        Kind::Stand => EntityTarget::Stand(id),
        Kind::Boat => EntityTarget::Boat(id),
        Kind::Cart => EntityTarget::Cart(id),
        Kind::Projectile => EntityTarget::Projectile(id),
        Kind::Tnt => EntityTarget::Tnt(id),
        Kind::Trident => EntityTarget::Trident(id),
        Kind::Falling => EntityTarget::Falling(id),
        _ => return Err(EntityWriteError::Unsupported),
    })
}

impl Game<'_> {
    pub(super) fn entity_own_no_gravity(&self, entity: EntityRef) -> Option<Option<bool>> {
        let writer = self.active_entity_writer()?;
        if writer.actor.runtime_id != entity.id() {
            return None;
        }
        if self.native_lifecycle.epoch() != Some(entity.epoch()) {
            return Some(None);
        }
        let at = writer.actor.location;
        if self
            .locate_server_resident_owner(at)
            .is_none_or(|source| source.id != entity.id() || !source.value.1)
        {
            return Some(None);
        }
        let raw = if at.owner.dimension == self.dim.key() {
            self.resident_read_source(at)
        } else {
            self.levels
                .parked_of(LevelRef::new(entity.epoch(), at.owner.dimension).dimension_id())
                .and_then(|p| p.resident_read_source(at))
        };
        Some(
            raw.filter(|raw| raw.id == entity.id())
                .and_then(|raw| raw.source.no_gravity()),
        )
    }

    fn admitted_no_gravity_target(&self, entity: EntityRef) -> Result<Target, EntityWriteError> {
        if self.native_lifecycle.epoch() != Some(entity.epoch()) {
            return Err(EntityWriteError::StaleEpoch);
        }
        if !self.native_authoritative() {
            return Err(EntityWriteError::OutsideScope);
        }
        let (location, state) = self
            .entity_residency
            .borrow_mut()
            .server_admitted_location(entity.id(), |at| self.locate_server_resident_owner(at))
            .ok_or(EntityWriteError::Missing)?;
        let target = family_target(location.owner.kind, entity.id())?;
        if !self.server_read_target_visible(location.owner.dimension, target)
            || matches!(
                state,
                EntityLookup::Found {
                    value: (_, false),
                    ..
                }
            )
        {
            return Err(EntityWriteError::Removed);
        }
        Ok(Target {
            runtime_id: entity.id(),
            location,
        })
    }

    pub(super) fn publish_no_gravity_target(
        &self,
        target: Target,
        phase: EntityReadCapturePhase,
    ) -> bool {
        let at = target.location;
        if at.owner.kind != Kind::Mob {
            return self.publish_resident_read_location(at, phase);
        }
        let raw = if at.owner.dimension == self.dim.key() {
            self.resident_read_source(at)
        } else {
            self.native_lifecycle
                .epoch()
                .and_then(|epoch| {
                    self.levels
                        .parked_of(LevelRef::new(epoch, at.owner.dimension).dimension_id())
                })
                .and_then(|p| p.resident_read_source(at))
        };
        let Some(raw) = raw.filter(|raw| raw.id == target.runtime_id) else {
            return false;
        };
        let super::native_base_read_rows::BaseReadSource::Mob(mob) = raw.source else {
            return false;
        };
        self.publish_mob_read_source_at(mob, at, phase)
    }

    pub(super) fn entity_set_no_gravity(
        &mut self,
        entity: EntityRef,
        no_gravity: bool,
    ) -> WriteReceipt<bool, EntityWriteError> {
        let target = match self.admitted_no_gravity_target(entity) {
            Ok(target) => target,
            Err(error) => return WriteReceipt::Refused(error),
        };
        let writer = self
            .entity_read_state
            .borrow()
            .commands
            .as_ref()
            .and_then(EntityCommands::active);
        let Some(writer) = writer.filter(|writer| writer.actor != target) else {
            return match self.apply_no_gravity(target, no_gravity) {
                Ok(value) => WriteReceipt::Applied(value),
                Err(error) => WriteReceipt::Refused(error),
            };
        };
        if !self.transactions.is_empty() {
            return WriteReceipt::Refused(EntityWriteError::TransactionActive);
        }
        let Some(native) = mod_api::content::crash::current_native_mod() else {
            return WriteReceipt::Refused(EntityWriteError::OutsideScope);
        };
        let mut cached = false;
        self.entity_residency
            .borrow_mut()
            .edit_cached_row(entity.id(), target.location, |_| cached = true);
        if !cached
            && !self.publish_no_gravity_target(target, EntityReadCapturePhase::BeforeCallback)
        {
            return WriteReceipt::Refused(EntityWriteError::WorkLimit);
        }
        let mut state = self.entity_read_state.borrow_mut();
        let commands = state
            .commands
            .as_mut()
            .expect("active writer has command state");
        let issuer = match mod_api::__private::with_current_converted_package(|package| {
            commands.issuer(native, package)
        }) {
            Ok(issuer) => issuer,
            Err(error) => return WriteReceipt::Refused(error),
        };
        let id = match commands.accept(
            issuer,
            writer,
            target,
            EntityOperation::SetNoGravity(no_gravity),
        ) {
            Ok(id) => id,
            Err(_) => return WriteReceipt::Refused(EntityWriteError::WorkLimit),
        };
        let patched = self.entity_residency.borrow_mut().edit_cached_row(
            entity.id(),
            target.location,
            |row| row.queue_no_gravity_overlay(id, no_gravity),
        );
        assert!(
            patched,
            "validated command publication disappeared without an engine boundary"
        );
        WriteReceipt::Pending(id)
    }

    pub(super) fn apply_no_gravity(
        &mut self,
        target: Target,
        value: bool,
    ) -> Result<bool, EntityWriteError> {
        let epoch = self
            .native_lifecycle
            .epoch()
            .ok_or(EntityWriteError::StaleEpoch)?;
        let (location, state) = self
            .entity_residency
            .borrow_mut()
            .server_admitted_location(target.runtime_id, |at| {
                self.locate_server_resident_owner(at)
            })
            .ok_or(EntityWriteError::Removed)?;
        if location != target.location {
            return Err(EntityWriteError::GenerationChanged);
        }
        let kind_target = family_target(location.owner.kind, target.runtime_id)?;
        if !self.server_read_target_visible(location.owner.dimension, kind_target)
            || matches!(
                state,
                EntityLookup::Found {
                    value: (_, false),
                    ..
                }
            )
        {
            return Err(EntityWriteError::Removed);
        }
        let applied = if location.owner.dimension == self.dim.key() {
            self.set_resident_no_gravity(location, target.runtime_id, value)
        } else {
            self.levels
                .parked_mut_of(LevelRef::new(epoch, location.owner.dimension).dimension_id())
                .ok_or(EntityWriteError::Removed)?
                .set_resident_no_gravity(location, target.runtime_id, value)
        }?;
        if mod_api::__private::entity_reads_listened() {
            self.publish_no_gravity_target(target, EntityReadCapturePhase::AfterAppliedWrite);
        }
        Ok(applied)
    }
}

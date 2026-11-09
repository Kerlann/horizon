//! Real normalized Entity.setSilent operation and its after-writer executor.
use super::{
    Game,
    commands::Target as EntityTarget,
    native_entity_commands::{EntityCommands, EntityOperation},
};
use horizon_client::join_seq::residency::{
    Kind,
    table::EntityLookup,
    write_queue::{Target, WriterTurn},
};
use mod_api::*;

impl Game<'_> {
    /// Ordinary scalar OWN reads bypass foreign pending projections. Explicit
    /// cached_row retains its speculative metadata and is not a live OWN read.
    pub(super) fn entity_own_silent(&self, entity: EntityRef) -> Option<Option<bool>> {
        let writer = self.active_entity_writer()?;
        if writer.actor.runtime_id != entity.id() {
            return None;
        }
        if self.native_lifecycle.epoch() != Some(entity.epoch()) {
            return Some(None);
        }
        let at = writer.actor.location;
        let raw = if at.owner.dimension == self.dim.key() {
            self.resident_read_source(at)
        } else {
            let level = LevelRef::new(entity.epoch(), at.owner.dimension);
            self.levels
                .parked_of(level.dimension_id())
                .and_then(|parked| parked.resident_read_source(at))
        };
        Some(raw.and_then(|raw| match raw.source {
            super::native_base_read_rows::BaseReadSource::Mob(mob)
                if raw.id == entity.id() && !mob.removed =>
            {
                Some(mob.silent)
            }
            _ => None,
        }))
    }

    fn admitted_silent_target(&self, entity: EntityRef) -> Result<Target, EntityWriteError> {
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
        if location.owner.kind != Kind::Mob {
            return Err(EntityWriteError::WrongKind);
        }
        if !self
            .server_read_target_visible(location.owner.dimension, EntityTarget::Mob(entity.id()))
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

    pub(super) fn publish_silent_target(
        &self,
        target: Target,
        phase: EntityReadCapturePhase,
    ) -> bool {
        let raw = if target.location.owner.dimension == self.dim.key() {
            self.resident_read_source(target.location)
        } else {
            let level = LevelRef::new(
                self.native_lifecycle.epoch().expect("validated world"),
                target.location.owner.dimension,
            );
            self.levels
                .parked_of(level.dimension_id())
                .and_then(|parked| parked.resident_read_source(target.location))
        };
        let Some(raw) = raw else {
            return false;
        };
        if raw.id != target.runtime_id {
            return false;
        }
        let super::native_base_read_rows::BaseReadSource::Mob(mob) = raw.source else {
            return false;
        };
        self.publish_mob_read_source_at(mob, target.location, phase)
    }

    pub(super) fn entity_set_silent(
        &mut self,
        entity: EntityRef,
        silent: bool,
    ) -> WriteReceipt<bool, EntityWriteError> {
        let target = match self.admitted_silent_target(entity) {
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
            return match self.apply_silent(target, silent) {
                Ok(value) => WriteReceipt::Applied(value),
                Err(error) => WriteReceipt::Refused(error),
            };
        };
        // Item transactions cannot journal or silently discard accepted entity
        // work. This rejection creates neither command identity nor overlay.
        if !self.transactions.is_empty() {
            return WriteReceipt::Refused(EntityWriteError::TransactionActive);
        }
        let Some(native) = mod_api::crash::current_native_mod() else {
            return WriteReceipt::Refused(EntityWriteError::OutsideScope);
        };
        let mut cached = false;
        self.entity_residency
            .borrow_mut()
            .edit_cached_row(entity.id(), target.location, |_| cached = true);
        if !cached && !self.publish_silent_target(target, EntityReadCapturePhase::BeforeCallback) {
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
        let id = match commands.accept(issuer, writer, target, EntityOperation::SetSilent(silent)) {
            Ok(id) => id,
            Err(_) => return WriteReceipt::Refused(EntityWriteError::WorkLimit),
        };
        // No callback or source mutation occurs between cache validation and
        // this exact-generation patch. The canonical table owns the projection.
        let patched = self.entity_residency.borrow_mut().edit_cached_row(
            entity.id(),
            target.location,
            |row| row.queue_silent_overlay(id, silent),
        );
        assert!(
            patched,
            "validated command publication disappeared without an engine boundary"
        );
        WriteReceipt::Pending(id)
    }

    fn apply_silent(&mut self, target: Target, silent: bool) -> Result<bool, EntityWriteError> {
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
        // Compare the exact admission before interpreting the replacement's
        // kind or liveness. Reused ids never become a different operation.
        if location != target.location {
            return Err(EntityWriteError::GenerationChanged);
        }
        if !self.server_read_target_visible(
            location.owner.dimension,
            EntityTarget::Mob(target.runtime_id),
        ) || matches!(
            state,
            EntityLookup::Found {
                value: (_, false),
                ..
            }
        ) {
            return Err(EntityWriteError::Removed);
        }
        let list = if target.location.owner.dimension == self.dim.key() {
            &mut self.mobs
        } else {
            let level = LevelRef::new(epoch, target.location.owner.dimension);
            &mut self
                .levels
                .parked_mut_of(level.dimension_id())
                .ok_or(EntityWriteError::Removed)?
                .mobs
        };
        let index = list
            .locate(target.location.join)
            .ok_or(EntityWriteError::OutsideScope)?;
        let mob = &mut list[index];
        if mob.id != target.runtime_id {
            return Err(EntityWriteError::GenerationChanged);
        }
        if mob.removed {
            return Err(EntityWriteError::Removed);
        }
        mob.silent = silent;
        if mod_api::__private::entity_reads_listened() {
            self.publish_silent_target(target, EntityReadCapturePhase::AfterAppliedWrite);
        }
        Ok(true)
    }

    /// Caller has returned the writer's canonical owner and lent level to Game.
    /// Neither apply nor terminal callbacks run with a queue RefCell loan.
    pub(super) fn drain_entity_commands(&mut self, turn: WriterTurn) {
        let mut dispatch_panic = None;
        loop {
            let command = {
                let mut state = self.entity_read_state.borrow_mut();
                state
                    .commands
                    .as_mut()
                    .and_then(|commands| commands.queue.take_next(turn))
            };
            let Some(command) = command else {
                break;
            };
            let (completion, applied) = match command.operation {
                EntityOperation::SetSilent(silent) => {
                    let outcome = self.apply_silent(command.target, silent);
                    let applied = outcome.as_ref().ok().map(|_| silent);
                    (
                        EntityWriteCompletion::EntitySetSilent(CommandCompleted {
                            id: command.id,
                            outcome,
                        }),
                        applied,
                    )
                }
            };
            self.entity_residency.borrow_mut().edit_cached_row(
                command.target.runtime_id,
                command.target.location,
                |row| row.finish_silent_overlay(command.id, applied),
            );
            let (issuer, terminal) = {
                let mut state = self.entity_read_state.borrow_mut();
                let commands = state.commands.as_mut().expect("applying queue exists");
                let issuer = commands.identities[command.issuer.0 as usize].clone();
                let terminal = commands
                    .queue
                    .complete(command.id, completion)
                    .expect("owned command completes exactly once")
                    .clone();
                (issuer, terminal)
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                mod_api::__private::with_entity_command_recipient(
                    issuer.native,
                    issuer.package,
                    || self.native_entity_command_completed(&terminal),
                );
            }));
            if let Err(payload) = result
                && dispatch_panic.is_none()
            {
                dispatch_panic = Some(payload);
            }
        }
        // A failed notification cannot leave the turn's remaining accepted
        // work permanently blocking the world FIFO. Preserve its first panic.
        if let Some(payload) = dispatch_panic {
            std::panic::resume_unwind(payload);
        }
    }
}

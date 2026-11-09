//! One world FIFO and explicit writer ownership. No Game pointer or source index
//! survives acceptance; callbacks run only after releasing the state borrow.
use super::Game;
use horizon_client::join_seq::residency::write_queue::{Issuer, Target, WriteQueue, WriterTurn};
use mod_api::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct CommandIssuer {
    pub(super) native: &'static str,
    pub(super) package: Option<Arc<str>>,
}
#[derive(Default)]
struct RuntimeIssuers {
    native: Option<Issuer>,
    packages: HashMap<Arc<str>, Issuer>,
}

pub(super) enum EntityOperation {
    SetSilent(bool),
}
#[derive(Clone, Copy)]
pub(super) struct WriterFrame {
    pub(super) turn: WriterTurn,
    pub(super) actor: Target,
}
#[derive(Default)]
struct WriterClock {
    next: u64,
    active: Option<WriterFrame>,
    accepted: usize,
}
/// Cumulative accepted work, including commands appended by completion callbacks.
pub(super) const MAX_ENTITY_COMMANDS_PER_TURN: usize = 256;
/// An actor turn spans real Pre, physics, Post and completion callbacks. Nested
/// callbacks keep that enclosing turn; they do not create another actor frame.
pub(super) struct EntityWriterScope {
    clock: Rc<RefCell<WriterClock>>,
    previous: Option<WriterFrame>,
    pub(super) frame: WriterFrame,
}
impl Drop for EntityWriterScope {
    fn drop(&mut self) {
        self.clock.borrow_mut().active = self.previous;
    }
}

pub(super) struct EntityCommands {
    pub(super) epoch: NativeWorldEpoch,
    pub(super) queue: WriteQueue<EntityOperation, EntityWriteCompletion>,
    clock: Rc<RefCell<WriterClock>>,
    issuers: HashMap<&'static str, RuntimeIssuers>,
    pub(super) identities: Vec<CommandIssuer>,
}
impl EntityCommands {
    pub(super) fn new(epoch: NativeWorldEpoch) -> Self {
        Self {
            epoch,
            queue: WriteQueue::new(epoch),
            clock: Rc::default(),
            issuers: HashMap::new(),
            identities: Vec::new(),
        }
    }
    pub(super) fn active(&self) -> Option<WriterFrame> {
        self.clock.borrow().active
    }
    pub(super) fn issuer(
        &mut self,
        native: &'static str,
        package: Option<&str>,
    ) -> Result<Issuer, EntityWriteError> {
        if let Some(issuer) = self.known_issuer(native, package) {
            return Ok(issuer);
        }
        let issuer =
            Issuer(u32::try_from(self.identities.len()).map_err(|_| EntityWriteError::WorkLimit)?);
        let identity = CommandIssuer {
            native,
            package: package.map(Arc::from),
        };
        let runtime = self.issuers.entry(native).or_default();
        match &identity.package {
            Some(package) => {
                runtime.packages.insert(package.clone(), issuer);
            }
            None => runtime.native = Some(issuer),
        }
        self.identities.push(identity);
        Ok(issuer)
    }
    pub(super) fn known_issuer(&self, native: &str, package: Option<&str>) -> Option<Issuer> {
        let runtime = self.issuers.get(native)?;
        match package {
            Some(package) => runtime.packages.get(package).copied(),
            None => runtime.native,
        }
    }
    pub(super) fn accept(
        &mut self,
        issuer: Issuer,
        writer: WriterFrame,
        target: Target,
        operation: EntityOperation,
    ) -> Result<CommandId, EntityWriteError> {
        let mut clock = self.clock.borrow_mut();
        if clock.accepted >= MAX_ENTITY_COMMANDS_PER_TURN {
            return Err(EntityWriteError::WorkLimit);
        }
        let id = self
            .queue
            .accept(issuer, writer.turn, target, None, operation)
            .map_err(|_| EntityWriteError::WorkLimit)?;
        clock.accepted += 1;
        Ok(id)
    }

    fn callback_actor(&self, actor: Target) -> Option<EntityWriterScope> {
        let mut clock = self.clock.borrow_mut();
        let previous = clock.active?;
        let frame = WriterFrame {
            turn: previous.turn,
            actor,
        };
        clock.active = Some(frame);
        Some(EntityWriterScope {
            clock: self.clock.clone(),
            previous: Some(previous),
            frame,
        })
    }

    fn begin(&self, actor: Target) -> Option<EntityWriterScope> {
        let mut clock = self.clock.borrow_mut();
        let turn = WriterTurn(clock.next);
        clock.next = clock.next.checked_add(1)?;
        clock.accepted = 0;
        let frame = WriterFrame { turn, actor };
        let previous = clock.active.replace(frame);
        Some(EntityWriterScope {
            clock: self.clock.clone(),
            previous,
            frame,
        })
    }
}
impl Game<'_> {
    /// Native callback adapters own raw runtime-id validation at their boundary.
    pub(super) fn enter_native_callback_actor(&self, id: u32) -> Option<EntityWriterScope> {
        self.active_entity_writer()?;
        let (location, _) = self
            .entity_residency
            .borrow_mut()
            .server_admitted_location(id, |at| self.locate_server_resident_owner(at))?;
        self.enter_entity_callback_actor(Target {
            runtime_id: id,
            location,
        })
    }

    /// The callback boundary supplies its already validated explicit actor. This
    /// is not a new writer turn, and resident/disjoint sources never imply OWN.
    pub(super) fn enter_entity_callback_actor(&self, actor: Target) -> Option<EntityWriterScope> {
        self.entity_read_state
            .borrow()
            .commands
            .as_ref()?
            .callback_actor(actor)
    }

    pub(super) fn active_entity_writer(&self) -> Option<WriterFrame> {
        self.entity_read_state
            .borrow()
            .commands
            .as_ref()
            .and_then(EntityCommands::active)
    }

    pub(super) fn begin_entity_writer(&self, actor: Target) -> Option<EntityWriterScope> {
        if !self.native_authoritative() || !mod_api::__private::entity_reads_listened() {
            return None;
        }
        let epoch = self.native_lifecycle.epoch()?;
        let mut state = self.entity_read_state.borrow_mut();
        let commands = state
            .commands
            .get_or_insert_with(|| EntityCommands::new(epoch));
        if commands.epoch != epoch {
            *commands = EntityCommands::new(epoch);
        }
        commands.begin(actor)
    }

    /// The shared validated-read boundary supplies this exact admitted address.
    /// Physical residency never confers OWN; only an explicit enclosing writer
    /// frame can do so. Epoch and holder UUID validation belong to that boundary.
    pub(super) fn entity_read_actor_owns(
        &self,
        id: u32,
        location: horizon_client::join_seq::residency::table::Location,
    ) -> bool {
        self.active_entity_writer().is_some_and(|frame| {
            frame.actor
                == Target {
                    runtime_id: id,
                    location,
                }
        })
    }

    pub(super) fn entity_command_query(
        &self,
        id: CommandId,
    ) -> Result<EntityCommandStatus, CommandQueryError> {
        if self.native_lifecycle.epoch() != Some(id.epoch) {
            return Err(CommandQueryError::StaleEpoch);
        }
        let native =
            mod_api::crash::current_native_mod().ok_or(CommandQueryError::UnknownOrExpired)?;
        let state = self.entity_read_state.borrow();
        let commands = state
            .commands
            .as_ref()
            .ok_or(CommandQueryError::UnknownOrExpired)?;
        let issuer = mod_api::__private::with_current_converted_package(|package| {
            commands.known_issuer(native, package)
        })
        .ok_or(CommandQueryError::UnknownOrExpired)?;
        use horizon_client::join_seq::residency::write_queue::{Query, QueryError};
        match commands.queue.query(issuer, id) {
            Ok(Query::Pending) => Ok(EntityCommandStatus::Pending),
            Ok(Query::Terminal(completion)) => {
                Ok(EntityCommandStatus::Completed(completion.clone()))
            }
            Err(QueryError::StaleEpoch) => Err(CommandQueryError::StaleEpoch),
            Err(QueryError::UnknownOrExpired) => Err(CommandQueryError::UnknownOrExpired),
        }
    }
}

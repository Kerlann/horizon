//! Identity shared by pending light overlays and their eventual command completion.
use crate::*;

/// Identity of a world command, not authority or proof of completion.
/// The owning runtime validates the epoch and liveness when querying or applying it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Wire)]
pub struct CommandId {
    /// The world session that allocated the command.
    pub epoch: NativeWorldEpoch,
    /// Command sequence within that session.
    pub sequence: u64,
}

impl CommandId {
    /// Query this command in the current issuing mod/package scope. Pending work
    /// never expires. Terminal status agrees with `entity_command_completed` and
    /// remains available through the end of the following logical engine tick.
    /// Inaccessible ids are indistinguishable from unknown or expired ids.
    pub fn query(self, host: &dyn Host) -> Result<EntityCommandStatus, CommandQueryError> {
        crate::__private::backend_read(host, |backend| backend.entity_command_query(self))
            .unwrap_or(Err(CommandQueryError::UnknownOrExpired))
    }
}

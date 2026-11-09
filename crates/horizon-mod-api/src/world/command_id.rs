//! Identity shared by pending light overlays and their eventual command completion.
use crate::*;

/// Identity of a world command, not authority or proof of completion.
/// The owning runtime validates the epoch and liveness when querying or applying it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Wire)]
pub struct CommandId {
    /// The world session that allocated the command.
    pub epoch: WorldEpoch,
    /// Command sequence within that session.
    pub sequence: u64,
}

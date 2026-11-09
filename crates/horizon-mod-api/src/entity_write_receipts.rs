//! Entity write acceptance and terminal outcomes, shared by native and wasm mods.
use crate::*;

/// Whether a write was applied now, accepted for ordered execution, or refused.
///
/// `Pending` carries identity only: it is not an applied boolean, count, or
/// delivery guarantee. Validation available at enqueue runs before acceptance.
/// An accepted command later reports its actual result through the same typed
/// completion used by its issuing mod's event and command query.
#[derive(Clone, Debug, PartialEq, Eq, Wire)]
pub enum WriteReceipt<R, E> {
    /// The operation ran and produced this actual result.
    Applied(R),
    /// Accepted for the enclosing writer's FIFO, not yet applied.
    Pending(CommandId),
    /// Not accepted or applied; no successful result is fabricated.
    Refused(E),
}

/// A terminal outcome for one accepted command. Pending is unrepresentable.
///
/// An operation's concrete completion retains its original result and domain
/// error types. The issuing mod's completion event and non-destructive query
/// observe the same outcome after authoritative application/refusal and row
/// publication. The event is the primary delivery path; terminal query results
/// remain available through the end of the following logical engine tick.
#[derive(Clone, Debug, PartialEq, Eq, Wire)]
pub struct CommandCompleted<R, E> {
    /// The accepted command whose execution finished.
    pub id: CommandId,
    /// Actual applied result or typed final refusal, never another receipt.
    pub outcome: Result<R, E>,
}

/// Write-boundary or apply-time refusal, preserving the operation's domain error.
///
/// A handle or command identity supplies no authority. The runtime checks scope,
/// epoch, join generation and logical removal before touching a target; an ID
/// reused by another entity never retargets an accepted command.
#[derive(Clone, Debug, PartialEq, Eq, Wire)]
pub enum EntityWriteError<E = core::convert::Infallible> {
    /// The handle or accepted command belongs to another world epoch.
    StaleEpoch,
    /// The current host does not authorize this target or operation.
    OutsideScope,
    /// No admitted entity owns this identity.
    Missing,
    /// The accepted target has been logically removed.
    Removed,
    /// The target left its admitted generation, including dimension transfer.
    GenerationChanged,
    /// This operation does not accept the target's entity family.
    WrongKind,
    /// An argument fails the operation's validation rules.
    InvalidArgument,
    /// The target does not support the requested capability.
    Unsupported,
    /// Acceptance would exceed the runtime's bounded command/work allowance.
    WorkLimit,
    /// The enclosing transaction is no longer able to commit this command.
    TransactionClosed,
    /// The operation's original typed validation or application error.
    Domain(E),
}

/// A command query failed; neither case means that work is pending.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Wire)]
pub enum CommandQueryError {
    /// The command belongs to another world epoch.
    StaleEpoch,
    /// Unknown, inaccessible to this issuing-mod scope, or past terminal retention.
    /// These cases intentionally share one result rather than disclosing another
    /// mod's commands. Pending commands are not silently expired.
    UnknownOrExpired,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Reader, Writer};

    #[test]
    fn receipt_wire_distinguishes_acceptance_actual_false_and_typed_refusal() {
        // Independent bytes pin the boundary distinction, not a round-trip
        // whose expected answer is produced by the same encoder.
        let applied_false = [0, 0, 0, 0, 0];
        assert_eq!(
            Reader::decode::<WriteReceipt<bool, EntityWriteError>>(&applied_false).unwrap(),
            WriteReceipt::Applied(false)
        );
        let pending = [
            1u32.to_le_bytes().as_slice(),
            7u64.to_le_bytes().as_slice(),
            9u64.to_le_bytes().as_slice(),
        ]
        .concat();
        let receipt = Reader::decode::<WriteReceipt<bool, EntityWriteError>>(&pending).unwrap();
        assert_eq!(
            receipt,
            WriteReceipt::Pending(CommandId {
                epoch: NativeWorldEpoch::new(7),
                sequence: 9
            })
        );
        let refused = [2, 0, 0, 0, 3, 0, 0, 0];
        assert_eq!(
            Reader::decode::<WriteReceipt<bool, EntityWriteError>>(&refused).unwrap(),
            WriteReceipt::Refused(EntityWriteError::Removed)
        );
        let mut encoded = Writer::new();
        encoded.put_owned(WriteReceipt::<bool, EntityWriteError>::Applied(false));
        assert_eq!(encoded.error(), None);
        assert_eq!(encoded.bytes(), applied_false);
    }

    #[test]
    fn completion_has_terminal_result_and_preserves_domain_error() {
        let bytes = [
            7u64.to_le_bytes().as_slice(),
            9u64.to_le_bytes().as_slice(),
            &[1],           // Result::Err, not Pending
            &[10, 0, 0, 0], // EntityWriteError::Domain
            &[1],           // bool domain error
        ]
        .concat();
        let completed =
            Reader::decode::<CommandCompleted<u32, EntityWriteError<bool>>>(&bytes).unwrap();
        assert_eq!(completed.id.sequence, 9);
        assert_eq!(completed.outcome, Err(EntityWriteError::Domain(true)));
        // A receipt's Pending tag cannot decode as a terminal Result.
        let mut invalid = bytes;
        invalid[16] = 2;
        assert!(Reader::decode::<CommandCompleted<u32, EntityWriteError<bool>>>(&invalid).is_err());
    }
}

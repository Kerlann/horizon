//! Cached, potentially stale entity data materialized on request, not per-pass heap copies.
use crate::*;

/// Publication point of a cached entity row, not a claim about live residency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Wire)]
pub enum EntityReadCapturePhase {
    /// Capture at the start of the owner's pass.
    PassStart,
    /// Capture before invoking the owner's callback.
    BeforeCallback,
    /// Capture after ticking the entity.
    AfterEntityTick,
    /// Capture after an actually applied write, never enqueue acceptance.
    AfterAppliedWrite,
}

/// Identifies the publication from which an owned cached row was materialized.
/// Neither removal nor retained output is an entity-lift state. Capturing a row
/// does not promise its owner is resident or unchanged when the row is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Wire)]
pub struct EntityReadCapture {
    /// Logical engine tick, not vanilla world time.
    pub tick: NativeTickId,
    /// The owner's capture pass.
    pub pass: u64,
    /// Publication revision within that pass.
    pub revision: u64,
    /// When this publication was captured.
    pub phase: EntityReadCapturePhase,
    /// Latest speculative light overlay included in this publication, if any.
    /// The phase still describes the last engine-applied state, never the enqueue.
    /// Completion or refusal clears or rebases this marker.
    pub pending_overlay: Option<CommandId>,
}

/// Refusal of a foreign live entity read, or its unchanged typed domain error.
/// Cached scalar Option accessors do not use this error. Only the explicit
/// nested callback actor is OWN; a disjoint foreign live loan is not OWN.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Wire)]
pub enum EntityReadError<E = core::convert::Infallible> {
    /// The validated foreign owner is physically lifted from its storage.
    Lifted,
    /// The operation's original typed domain error.
    Domain(E),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Reader, Wire, WireError, Writer, layout_name};

    #[test]
    fn actual_generic_error_layout_and_uninhabited_domain() {
        assert_ne!(
            <EntityReadError>::layout(&mut Vec::new()),
            EntityReadError::<TextFormError>::layout(&mut Vec::new())
        );
        assert_eq!(
            <core::convert::Infallible as Wire>::LAYOUT_SEED,
            layout_name("core::convert::Infallible")
        );
        let lifted: EntityReadError = Reader::decode(&[0, 0, 0, 0]).unwrap();
        assert_eq!(lifted, EntityReadError::Lifted);
        let mut writer = Writer::new();
        writer.put(&lifted);
        assert_eq!(writer.bytes(), &[0, 0, 0, 0]);
        assert!(matches!(
            Reader::decode::<EntityReadError>(&[1, 0, 0, 0]),
            Err(WireError::Invalid(_))
        ));
        assert!(Reader::decode::<core::convert::Infallible>(&[]).is_err());
        // Independent tags: Domain is case 1, TextFormError::TooDeep is case 1.
        let domain: EntityReadError<TextFormError> =
            Reader::decode(&[1, 0, 0, 0, 1, 0, 0, 0]).unwrap();
        assert_eq!(domain, EntityReadError::Domain(TextFormError::TooDeep));
        writer.clear();
        writer.put(&domain);
        assert_eq!(writer.bytes(), &[1, 0, 0, 0, 1, 0, 0, 0]);
    }
}

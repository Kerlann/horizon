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
    /// A resident foreign source has a pending heavy dependency for this read.
    /// Identifies the latest relevant command, not an unrelated light overlay.
    /// Physical lifting remains Lifted; OWN live reads remain own reads.
    Pending(CommandId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Reader, Wire, WireError, Writer, layout_name};

    #[test]
    fn cached_payload_keeps_pending_identity_domain_absence_and_typed_error() {
        // Independent schema bytes: capture, optional overlay id, entity identity,
        // then the five retained projectile fields. No row encoder builds this input.
        let bytes = [
            1u64.to_le_bytes().as_slice(),  // logical tick
            2u64.to_le_bytes().as_slice(),  // pass
            3u64.to_le_bytes().as_slice(),  // revision
            &[1, 0, 0, 0], // BeforeCallback, not AfterEntityTick or AfterAppliedWrite
            &[1],          // pending overlay present
            7u64.to_le_bytes().as_slice(), // command epoch
            9u64.to_le_bytes().as_slice(), // command sequence
            7u64.to_le_bytes().as_slice(), // entity epoch
            11u32.to_le_bytes().as_slice(), // entity id
            &[0],          // no owner
            &[0],          // unknown/unsupported pickup mode
            &[1, 2, 0, 0, 0], // Err(PierceLevelError::NotArrow)
            &[0],          // unknown/unsupported base damage
            &[1, 0],       // known critical=false
        ]
        .concat();
        let row: ProjectileReadRow = Reader::decode(&bytes).unwrap();
        assert_eq!(row.capture.tick.get(), 1);
        assert_eq!(row.capture.phase, EntityReadCapturePhase::BeforeCallback);
        assert_eq!(row.capture.pending_overlay.unwrap().sequence, 9);
        assert_eq!(row.entity.id(), 11);
        assert_eq!(row.owner, None);
        assert_eq!(row.pickup, None);
        assert_eq!(row.pierce_level, Err(PierceLevelError::NotArrow));
        assert_eq!(row.critical, Some(false));
        let mut writer = Writer::new();
        writer.put(&row);
        assert_eq!(writer.bytes(), bytes);
    }

    #[test]
    fn pending_refusal_preserves_command_metadata_and_generic_domain() {
        // Independent wire input: Pending is case 2, followed by two full u64s.
        // Domain remains case 1, including when the default domain is uninhabited.
        let bytes = [
            2, 0, 0, 0, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 8, 7, 6, 5, 4, 3, 2, 1,
        ];
        let id = CommandId {
            epoch: NativeWorldEpoch::new(0x1122_3344_5566_7788),
            sequence: 0x0102_0304_0506_0708,
        };
        let pending: EntityReadError = Reader::decode(&bytes).unwrap();
        assert_eq!(pending, EntityReadError::Pending(id));
        let mut writer = Writer::new();
        writer.put(&pending);
        assert_eq!(writer.bytes(), bytes);
        let typed: EntityReadError<TextFormError> = Reader::decode(&bytes).unwrap();
        assert_eq!(typed, EntityReadError::Pending(id));
        writer.clear();
        writer.put(&typed);
        assert_eq!(writer.bytes(), bytes);
        assert!(Reader::decode::<EntityReadError>(&bytes[..19]).is_err());
        assert!(Reader::decode::<EntityReadError>(&[3, 0, 0, 0]).is_err());
    }

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

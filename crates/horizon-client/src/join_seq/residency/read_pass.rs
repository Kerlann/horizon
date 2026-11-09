//! Capture clocks for nested owner passes. Rows remain in EntityTable; closing
//! a pass never scans or clears the population it captured.
use super::Owner;
use mod_api::{EntityReadCapture, EntityReadCapturePhase, TickId};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadPass(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadPassError {
    Exhausted,
    NotCurrent,
    WrongOwner,
}

struct Frame {
    pass: ReadPass,
    owner: Owner,
    tick: TickId,
    revision: u64,
}

/// Owns only a capture clock, never Game or an entity reference. Unwinding a
/// callback closes its metadata frame without replacing the original panic.
pub struct ReadScope {
    clock: Rc<RefCell<ReadPasses>>,
    pass: ReadPass,
    owner: Owner,
    start: EntityReadCapture,
}

impl ReadScope {
    pub fn begin(
        clock: Rc<RefCell<ReadPasses>>,
        enabled: bool,
        owner: Owner,
        tick: TickId,
    ) -> Result<Option<Self>, ReadPassError> {
        let started = clock.borrow_mut().begin(enabled, owner, tick)?;
        Ok(started.map(|(pass, start)| Self {
            clock,
            pass,
            owner,
            start,
        }))
    }

    pub fn start(&self) -> EntityReadCapture {
        self.start
    }

    pub fn stamp(&self, phase: EntityReadCapturePhase) -> Result<EntityReadCapture, ReadPassError> {
        self.clock.borrow_mut().stamp(self.pass, self.owner, phase)
    }
}

impl Drop for ReadScope {
    fn drop(&mut self) {
        let mut clock = self.clock.borrow_mut();
        if clock.finish(self.pass).is_err()
            && let Some(index) = clock
                .frames
                .iter()
                .position(|frame| frame.pass == self.pass)
        {
            // An out-of-order active guard is an engine error. Invalidate that
            // suffix, but do not touch a newer epoch when this token is stale.
            log::error!("entity read capture scope closed out of order");
            clock.frames.truncate(index);
        }
    }
}

#[derive(Default)]
pub struct ReadPasses {
    next: u64,
    frames: Vec<Frame>,
}

impl ReadPasses {
    /// A disabled pass does not allocate or advance the capture clock. Callers
    /// must construct their source snapshots only after this returns Some.
    pub fn begin(
        &mut self,
        enabled: bool,
        owner: Owner,
        tick: TickId,
    ) -> Result<Option<(ReadPass, EntityReadCapture)>, ReadPassError> {
        if !enabled {
            return Ok(None);
        }
        self.next = self.next.checked_add(1).ok_or(ReadPassError::Exhausted)?;
        let frame = Frame {
            pass: ReadPass(self.next),
            owner,
            tick,
            revision: 0,
        };
        let capture = EntityReadCapture {
            tick,
            pass: frame.pass.0,
            revision: 0,
            phase: EntityReadCapturePhase::PassStart,
            pending_overlay: None,
        };
        self.frames.push(frame);
        Ok(Some((ReadPass(self.next), capture)))
    }

    /// Stamp the current matching source pass without retaining a Game loan.
    /// A different owner's callback must open its own nested scope instead.
    pub fn stamp_active_owner(
        &mut self,
        owner: Owner,
        phase: EntityReadCapturePhase,
    ) -> Result<Option<EntityReadCapture>, ReadPassError> {
        let Some(pass) = self
            .frames
            .last()
            .filter(|frame| frame.owner == owner)
            .map(|frame| frame.pass)
        else {
            return Ok(None);
        };
        self.stamp(pass, owner, phase).map(Some)
    }

    /// Publish a new authoritative state, not a speculative command patch.
    pub fn stamp(
        &mut self,
        pass: ReadPass,
        owner: Owner,
        phase: EntityReadCapturePhase,
    ) -> Result<EntityReadCapture, ReadPassError> {
        let frame = self
            .frames
            .last_mut()
            .filter(|frame| frame.pass == pass)
            .ok_or(ReadPassError::NotCurrent)?;
        if frame.owner != owner {
            return Err(ReadPassError::WrongOwner);
        }
        frame.revision = frame
            .revision
            .checked_add(1)
            .ok_or(ReadPassError::Exhausted)?;
        Ok(EntityReadCapture {
            tick: frame.tick,
            pass: pass.0,
            revision: frame.revision,
            phase,
            pending_overlay: None,
        })
    }

    pub fn finish(&mut self, pass: ReadPass) -> Result<(), ReadPassError> {
        if self.frames.last().map(|frame| frame.pass) != Some(pass) {
            return Err(ReadPassError::NotCurrent);
        }
        self.frames.pop();
        Ok(())
    }

    /// Unwind/epoch cleanup invalidates outstanding tokens, without reusing
    /// their pass numbers. This does not touch the authoritative row records.
    pub fn reset(&mut self) {
        self.frames.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::join_seq::residency::Kind;
    use mod_api::DimensionKey;

    const MOB: Owner = Owner {
        dimension: DimensionKey::OVERWORLD,
        kind: Kind::Mob,
    };
    const TNT: Owner = Owner {
        dimension: DimensionKey::OVERWORLD,
        kind: Kind::Tnt,
    };

    #[test]
    fn nested_passes_keep_independent_clocks_and_resume_the_outer_actor() {
        let mut passes = ReadPasses::default();
        let (outer, initial) = passes.begin(true, MOB, TickId::new(9)).unwrap().unwrap();
        assert_eq!(initial.phase, EntityReadCapturePhase::PassStart);
        let before = passes
            .stamp(outer, MOB, EntityReadCapturePhase::BeforeCallback)
            .unwrap();
        let (inner, nested) = passes.begin(true, TNT, TickId::new(9)).unwrap().unwrap();
        assert_ne!(before.pass, nested.pass);
        assert_eq!(passes.finish(outer), Err(ReadPassError::NotCurrent));
        assert_eq!(
            passes.stamp(inner, MOB, EntityReadCapturePhase::AfterEntityTick),
            Err(ReadPassError::WrongOwner)
        );
        let finished = passes
            .stamp(inner, TNT, EntityReadCapturePhase::AfterEntityTick)
            .unwrap();
        assert_eq!(finished.revision, 1);
        passes.finish(inner).unwrap();
        let resumed = passes
            .stamp(outer, MOB, EntityReadCapturePhase::AfterEntityTick)
            .unwrap();
        assert_eq!(resumed.pass, before.pass);
        assert_eq!(resumed.revision, 2);
        assert_eq!(resumed.tick, TickId::new(9));
        assert_eq!(resumed.pending_overlay, None);
        passes.finish(outer).unwrap();
    }

    #[test]
    fn callback_unwind_closes_only_its_nested_capture_frame() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let clock = Rc::new(RefCell::new(ReadPasses::default()));
        let outer = ReadScope::begin(clock.clone(), true, MOB, TickId::new(9))
            .unwrap()
            .unwrap();
        let panic = catch_unwind(AssertUnwindSafe(|| {
            let _inner = ReadScope::begin(clock.clone(), true, TNT, TickId::new(9))
                .unwrap()
                .unwrap();
            panic!("original callback failure");
        }))
        .unwrap_err();
        assert_eq!(
            panic.downcast_ref::<&str>(),
            Some(&"original callback failure")
        );
        assert_eq!(
            outer
                .stamp(EntityReadCapturePhase::AfterEntityTick)
                .unwrap()
                .pass,
            outer.start().pass
        );
        clock.borrow_mut().reset();
        let new = ReadScope::begin(clock.clone(), true, TNT, TickId::new(10))
            .unwrap()
            .unwrap();
        drop(outer);
        assert_eq!(
            new.stamp(EntityReadCapturePhase::BeforeCallback)
                .unwrap()
                .tick,
            TickId::new(10)
        );
        drop(new);
        assert!(clock.borrow().frames.is_empty());
    }

    #[test]
    fn disabled_capture_and_epoch_reset_never_reuse_a_publication_token() {
        let mut passes = ReadPasses::default();
        assert_eq!(passes.begin(false, MOB, TickId::new(1)).unwrap(), None);
        assert_eq!(passes.frames.capacity(), 0);
        let (old, _) = passes.begin(true, MOB, TickId::new(1)).unwrap().unwrap();
        passes.reset();
        let (new, capture) = passes.begin(true, MOB, TickId::new(2)).unwrap().unwrap();
        assert_ne!(old, new);
        assert_eq!(passes.finish(old), Err(ReadPassError::NotCurrent));
        assert_eq!(capture.tick, TickId::new(2));
        assert_eq!(capture.revision, 0);
        passes.finish(new).unwrap();
    }
}

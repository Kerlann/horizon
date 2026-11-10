//! Ordered scalar projections retained on the exact admitted identity row.
use super::native_entity_read_cache::{NativeReadDetails, NativeReadPublication};
use mod_api::CommandId;
use std::collections::VecDeque;

#[derive(Clone, Copy)]
enum ScalarPatch {
    Silent(bool),
    NoGravity(bool),
}
#[derive(Default)]
pub(super) struct ScalarOverlay {
    silent: Option<bool>,
    no_gravity: Option<bool>,
    pending: VecDeque<(CommandId, ScalarPatch)>,
}
impl NativeReadPublication {
    /// Bounded source dependency selection, never a command-queue scan. Nbt
    /// denotes whole-entity serialization, not independent name/tag getters.
    pub(super) fn pending_dependency(
        &self,
        dependencies: &[super::native_read_dependencies::EntityReadDependency],
    ) -> Option<CommandId> {
        dependencies
            .iter()
            .any(|dependency| {
                matches!(
                    dependency,
                    super::native_read_dependencies::EntityReadDependency::Nbt
                )
            })
            .then(|| self.scalar_overlay.pending.back().map(|(id, _)| *id))
            .flatten()
    }

    /// Pure prepared-row replacement; only called after final generation check.
    pub(super) fn merge_pending_scalars(previous: Option<Self>, mut prepared: Self) -> Self {
        prepared.scalar_overlay.silent = prepared.base.is_silent;
        prepared.scalar_overlay.no_gravity = prepared.base.no_gravity;
        if let Some(previous) = previous {
            prepared.scalar_overlay.pending = previous.scalar_overlay.pending;
        }
        prepared.replay_scalar_overlay();
        prepared
    }

    pub(super) fn queue_silent_overlay(&mut self, id: CommandId, silent: bool) {
        self.scalar_overlay
            .pending
            .push_back((id, ScalarPatch::Silent(silent)));
        self.replay_scalar_overlay();
    }

    pub(super) fn queue_no_gravity_overlay(&mut self, id: CommandId, value: bool) {
        self.scalar_overlay
            .pending
            .push_back((id, ScalarPatch::NoGravity(value)));
        self.replay_scalar_overlay();
    }

    pub(super) fn finish_silent_overlay(&mut self, id: CommandId, applied: Option<bool>) {
        if self
            .scalar_overlay
            .pending
            .front()
            .is_some_and(|(pending, _)| *pending == id)
        {
            self.scalar_overlay.pending.pop_front();
        }
        if let Some(silent) = applied {
            self.scalar_overlay.silent = Some(silent);
        }
        self.replay_scalar_overlay();
    }

    pub(super) fn finish_no_gravity_overlay(&mut self, id: CommandId, applied: Option<bool>) {
        if self
            .scalar_overlay
            .pending
            .front()
            .is_some_and(|(pending, _)| *pending == id)
        {
            self.scalar_overlay.pending.pop_front();
        }
        if let Some(value) = applied {
            self.scalar_overlay.no_gravity = Some(value);
        }
        self.replay_scalar_overlay();
    }

    fn replay_scalar_overlay(&mut self) {
        self.base.is_silent = self.scalar_overlay.silent;
        self.base.no_gravity = self.scalar_overlay.no_gravity;
        for (_, patch) in &self.scalar_overlay.pending {
            match *patch {
                ScalarPatch::Silent(value) => self.base.is_silent = Some(value),
                ScalarPatch::NoGravity(value) => self.base.no_gravity = Some(value),
            }
        }
        let pending = self.scalar_overlay.pending.back().map(|(id, _)| *id);
        self.capture.pending_overlay = pending;
        // Every materialized family header must describe the same overlay.
        match &mut self.details {
            NativeReadDetails::Base => {}
            NativeReadDetails::Living(row) => row.capture.pending_overlay = pending,
            NativeReadDetails::Mob {
                living,
                mob,
                merchant,
            } => {
                living.capture.pending_overlay = pending;
                mob.capture.pending_overlay = pending;
                if let Some(merchant) = merchant {
                    merchant.capture.pending_overlay = pending;
                }
            }
            NativeReadDetails::Player { living, player } => {
                living.capture.pending_overlay = pending;
                player.0.capture.pending_overlay = pending;
            }
            NativeReadDetails::Dragon { living, dragon } => {
                living.capture.pending_overlay = pending;
                dragon.capture.pending_overlay = pending;
            }
            NativeReadDetails::Display(row) => row.capture.pending_overlay = pending,
            NativeReadDetails::Item(row) => row.capture.pending_overlay = pending,
            NativeReadDetails::Projectile(row) => row.capture.pending_overlay = pending,
            NativeReadDetails::Hook(row) => row.capture.pending_overlay = pending,
        }
    }
}

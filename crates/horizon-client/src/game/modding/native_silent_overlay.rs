//! The first real light-write projection, retained on the admitted identity row.
use super::native_entity_read_cache::{NativeReadDetails, NativeReadPublication};
use mod_api::CommandId;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct SilentOverlay {
    pub(super) authoritative: Option<bool>,
    pending: VecDeque<(CommandId, bool)>,
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
            .then(|| self.silent_overlay.pending.back().map(|(id, _)| *id))
            .flatten()
    }

    /// Pure prepared-row replacement; only called after final generation check.
    pub(super) fn merge_pending_silent(previous: Option<Self>, mut prepared: Self) -> Self {
        prepared.silent_overlay.authoritative = prepared.base.is_silent;
        if let Some(previous) = previous {
            prepared.silent_overlay.pending = previous.silent_overlay.pending;
        }
        prepared.replay_silent_overlay();
        prepared
    }

    pub(super) fn queue_silent_overlay(&mut self, id: CommandId, silent: bool) {
        self.silent_overlay.pending.push_back((id, silent));
        self.replay_silent_overlay();
    }

    pub(super) fn finish_silent_overlay(&mut self, id: CommandId, applied: Option<bool>) {
        if self
            .silent_overlay
            .pending
            .front()
            .is_some_and(|(pending, _)| *pending == id)
        {
            self.silent_overlay.pending.pop_front();
        }
        if let Some(silent) = applied {
            self.silent_overlay.authoritative = Some(silent);
        }
        self.replay_silent_overlay();
    }

    fn replay_silent_overlay(&mut self) {
        let latest = self.silent_overlay.pending.back().copied();
        self.base.is_silent = latest
            .map(|(_, value)| value)
            .or(self.silent_overlay.authoritative);
        let pending = latest.map(|(id, _)| id);
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

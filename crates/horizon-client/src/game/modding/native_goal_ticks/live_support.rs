//! Reads combining the explicit actor loan with authoritative game state.
use super::GoalCx;

impl GoalCx<'_, '_> {
    /// Select the real actor-loan Host once; engine doubles or absent adapters
    /// fall back to the ordinary shared Host, never the world AI snapshots.
    pub(super) fn with_heavy_host<R>(&self, read: impl FnOnce(&dyn mod_api::Host) -> R) -> R {
        let mut read = Some(read);
        let mut result = None;
        if let Some(host) = self.w.goal_host {
            host.with_entity_read_host(self.m, &mut |host| {
                result = Some(read.take().expect("one callback loan Host")(host));
            });
        }
        result.unwrap_or_else(|| read.take().expect("one selected Host")(self.host))
    }

    pub(super) fn with_heavy_backend<R>(
        &self,
        read: impl FnOnce(&dyn mod_api::Backend) -> R,
    ) -> Option<R> {
        self.with_heavy_host(|host| mod_api::__private::backend_read(host, read))
    }
}

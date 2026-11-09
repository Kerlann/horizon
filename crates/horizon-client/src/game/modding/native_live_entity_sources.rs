//! Borrowed heavy-read sources. These views contain references to actual storage,
//! never an R publication or a second identity directory.
use super::{
    Game, Mob, boats, minecarts, native_base_read_rows::BaseReadSource,
    native_entity_body_provider::EntityBodyProvider, native_live_entity_reads::EntityReadTarget,
    persist::EntityMemory,
};
use horizon_client::{join_seq::JoinList, world::World};

/// The actual level storage selected by an admitted semantic address. Session
/// lanes remain on Game/Guest; per-level memory and collections come from here.
#[derive(Clone, Copy)]
pub(super) struct EntityReadLevel<'a> {
    pub(super) world: &'a World,
    pub(super) persist: &'a EntityMemory,
    pub(super) mobs: &'a JoinList<Mob>,
    pub(super) boats: &'a JoinList<boats::Boat, false>,
    pub(super) carts: &'a JoinList<minecarts::Minecart, false>,
    pub(super) parked: Option<&'a super::levels::Parked>,
}

pub(super) struct EntityReadSource<'a> {
    pub(super) body: BaseReadSource<'a>,
    pub(super) level: EntityReadLevel<'a>,
    pub(super) cart_spawner: Option<&'a horizon_client::mobs::spawner::Spawner>,
}

impl Game<'_> {
    /// Select the actual callback loan, if this admitted address is the current
    /// actor's. A provider does not grant permission for a foreign Taken body.
    /// The higher-ranked projection returns owned output, never a stored loan.
    pub(super) fn with_live_entity_source<R>(
        &self,
        read: EntityReadTarget,
        provider: Option<&dyn EntityBodyProvider>,
        project: impl for<'s> FnOnce(EntityReadSource<'s>) -> R,
    ) -> Option<R> {
        let mut project = Some(project);
        let mut result = None;
        if let Some(provider) = provider
            && let Some(frame) = self.active_entity_writer()
            && frame.actor.location == read.location
        {
            provider.with_read_source(frame.actor, &mut |source| {
                let Some(level) = self.entity_read_level(read.location.owner.dimension) else {
                    return;
                };
                let project = project
                    .take()
                    .expect("a body provider visits its source once");
                result = Some(project(EntityReadSource {
                    body: source.body,
                    level,
                    cart_spawner: source.cart_spawner,
                }));
            });
            if result.is_some() {
                return result;
            }
        }
        self.live_entity_source(read).map(|source| {
            project
                .take()
                .expect("a projection consumes one selected source")(source)
        })
    }

    /// Project actual storage only after admission. A missing concrete OWN loan
    /// is not ordinary payload absence: callers must use their lent adapter,
    /// and must never map a missing provider here to public Ok(None).
    pub(super) fn live_entity_source(
        &self,
        read: EntityReadTarget,
    ) -> Option<EntityReadSource<'_>> {
        use super::commands::Target;
        // Held-out controllers may leave an old copy in the canonical list.
        // Admission is already complete; this is source selection, not another
        // handle lookup. Only a concrete callback provider may supply that loan.
        if super::native_entities::target_id(read.target)
            .is_some_and(|id| self.held_out.contains(&id))
        {
            return None;
        }
        let level = self.entity_read_level(read.location.owner.dimension)?;
        // Player/session hooks are global slots, even while their world is
        // parked. Ordinary bodies and dragon parts live in level storage.
        let raw = if matches!(
            read.target,
            Target::Player | Target::Guest(_) | Target::Hook(_)
        ) {
            self.resident_read_source(read.location)
        } else {
            match level.parked {
                Some(parked) => parked.resident_read_source(read.location),
                None => self.resident_read_source(read.location),
            }
        }?;
        let cart_spawner = match raw.source {
            BaseReadSource::Cart(cart) => cart.spawner.as_deref(),
            _ => None,
        };
        Some(EntityReadSource {
            body: raw.source,
            level,
            cart_spawner,
        })
    }
}

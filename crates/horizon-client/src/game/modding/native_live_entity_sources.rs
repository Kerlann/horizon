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
    pub(super) selectors: horizon_client::mobs::goals::GoalSelectorSources<'a>,
}

impl Game<'_> {
    /// Fresh geometry from the selected body and immutable resource facts.
    /// The intern pool retains type identifiers only, never entity values.
    pub(super) fn live_source_geometry(
        &self,
        source: BaseReadSource<'_>,
    ) -> Option<super::native_base_read_rows::BaseReadGeometry> {
        use super::native_base_read_source::{base_read_source_geometry, base_read_source_kind};
        let kind =
            base_read_source_kind(source, &mut self.entity_read_state.borrow_mut().kind_pool);
        let facts = self.res.entity_type_geometry().get(&kind)?;
        let mut geometry = base_read_source_geometry(source, facts)?;
        if matches!(source, BaseReadSource::Player(_)) {
            geometry.rotation = [self.yaw, self.pitch];
        }
        Some(geometry)
    }

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
        if let Some(value) = self.with_borrowed_entity_source(read, provider, |source| {
            project.take().expect("one selected body projection")(source)
        }) {
            return Some(value);
        }
        self.live_entity_source(read)
            .map(|source| project.take().expect("one selected body projection")(source))
    }

    /// The provider owns one concrete callback loan. Matching its canonical
    /// admitted address selects that loan; no actor permission or live-peer
    /// directory can replace it, including a GoalCx without a writer frame.
    pub(super) fn with_borrowed_entity_source<R>(
        &self,
        read: EntityReadTarget,
        provider: Option<&dyn EntityBodyProvider>,
        project: impl for<'s> FnOnce(EntityReadSource<'s>) -> R,
    ) -> Option<R> {
        let provider = provider?;
        let address = horizon_client::join_seq::residency::write_queue::Target {
            runtime_id: read.entity.id(),
            location: read.location,
        };
        let mut project = Some(project);
        let mut result = None;
        provider.with_read_source(address, &mut |source| {
            let Some(level) = self.entity_read_level(read.location.owner.dimension) else {
                return;
            };
            result = Some(project.take().expect("one actual body loan")(
                EntityReadSource {
                    body: source.body,
                    level,
                    cart_spawner: source.cart_spawner,
                    selectors: source.selectors,
                },
            ));
        });
        result
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
        let selectors = match raw.source {
            BaseReadSource::Mob(mob) => horizon_client::mobs::goals::GoalSelectorSources {
                goals: mob.ai.goals.read_source(),
                targets: mob.ai.targets.read_source(),
                ..Default::default()
            },
            _ => Default::default(),
        };
        Some(EntityReadSource {
            body: raw.source,
            level,
            cart_spawner,
            selectors,
        })
    }
}

//! An owning temporary projectile loan, not a replacement body or a Host.
//! Callback handoffs return the complete actual body to its real Game owner.
use super::Game;
use horizon_client::join_seq::residency::{Kind, Owner, Source, table::Location};
use horizon_client::join_seq::{JoinList, JoinSeq, Joined};
use mod_api::EntityReadCapturePhase;

pub(super) trait ProjectileActor: Joined + Sized {
    const KIND: Kind;
    fn target(&self) -> super::commands::Target;
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut JoinList<Self>;
}

impl ProjectileActor for super::fireworks::Rocket {
    const KIND: Kind = Kind::Rocket;
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Rocket(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut JoinList<Self> {
        &mut game.rockets
    }
}
impl ProjectileActor for super::trident::ThrownTrident {
    const KIND: Kind = Kind::Trident;
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Trident(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut JoinList<Self> {
        &mut game.tridents
    }
}

pub(super) struct ProjectileLoan<T> {
    body: Option<T>,
    location: Location,
    // A restoration ordinal only. Identity is always reacquired by exact join.
    ordinal: usize,
}
impl<T: ProjectileActor> ProjectileLoan<T> {
    pub(super) fn extract(game: &mut Game<'_>, join: JoinSeq) -> Option<Self> {
        let owner = Owner {
            dimension: game.dim.key(),
            kind: T::KIND,
        };
        let list = T::owner(game);
        let ordinal = list.locate(join)?;
        let body = list.extract(join)?;
        Some(Self {
            body: Some(body),
            location: Location {
                source: Source::Ordinary,
                owner,
                join,
            },
            ordinal,
        })
    }
    pub(super) fn body_mut(&mut self) -> Option<&mut T> {
        self.body.as_mut()
    }
    pub(super) fn join(&self) -> JoinSeq {
        self.location.join
    }
    pub(super) fn retire(&mut self) {
        self.body = None;
    }
    pub(super) fn publish(&self, game: &Game<'_>, phase: EntityReadCapturePhase) {
        game.publish_resident_read_location(self.location, phase);
    }

    pub(super) fn restore(&mut self, game: &mut Game<'_>) {
        if let Some(body) = self.body.take() {
            T::owner(game).put_back(self.ordinal, body);
        }
    }
    /// On callback panic the body stays canonical while unwinding. The caller
    /// catches body faults, restores any held body and completes its writer.
    pub(super) fn with_game<R>(
        &mut self,
        game: &mut Game<'_>,
        callback: impl FnOnce(&mut Game<'_>) -> R,
    ) -> Option<R> {
        self.restore(game);
        game.publish_resident_read_location(self.location, EntityReadCapturePhase::BeforeCallback);
        let result = callback(game);
        // A callback may have removed the actor. Never substitute a new actor at
        // the same element position or overwrite its writes with a stale body.
        self.body = T::owner(game).extract(self.location.join);
        if self
            .body
            .as_ref()
            .is_some_and(|body| game.persist.removals.contains(&body.target()))
        {
            self.body = None;
        }
        self.body.as_ref().map(|_| result)
    }
}

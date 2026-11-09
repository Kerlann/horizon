//! An owning temporary entity actor loan, not a replacement body or a Host.
//! Callback handoffs return the complete actual body to its real Game owner.
use super::Game;
use horizon_client::join_seq::residency::{Kind, Owner, Source, table::Location};
use horizon_client::join_seq::{JoinList, JoinSeq, Joined};
use mod_api::EntityReadCapturePhase;

pub(super) trait EntityActor<const DIRECT: bool = true>: Joined + Sized {
    const KIND: Kind;
    fn id(&self) -> u32;
    fn target(&self) -> super::commands::Target;
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut JoinList<Self, DIRECT>;
}

impl EntityActor<false> for super::boats::Boat {
    const KIND: Kind = Kind::Boat;
    fn id(&self) -> u32 {
        self.id
    }
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Boat(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut JoinList<Self, false> {
        &mut game.boats
    }
}

impl EntityActor for super::fireworks::Rocket {
    const KIND: Kind = Kind::Rocket;
    fn id(&self) -> u32 {
        self.id
    }
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Rocket(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut JoinList<Self> {
        &mut game.rockets
    }
}
impl EntityActor for super::trident::ThrownTrident {
    const KIND: Kind = Kind::Trident;
    fn id(&self) -> u32 {
        self.id
    }
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Trident(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut JoinList<Self> {
        &mut game.tridents
    }
}

pub(super) struct EntityActorLoan<T, const DIRECT: bool = true> {
    body: Option<T>,
    location: Location,
    // A restoration ordinal only. Identity is always reacquired by exact join.
    ordinal: usize,
}
impl<T: EntityActor<DIRECT>, const DIRECT: bool> EntityActorLoan<T, DIRECT> {
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
    pub(super) fn retire(&mut self, game: &mut Game<'_>) {
        if let Some(body) = self.body.take() {
            T::owner(game).retire_extracted(body.id(), self.location.join);
        }
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
            self.retire(game);
        }
        self.body.as_ref().map(|_| result)
    }
}

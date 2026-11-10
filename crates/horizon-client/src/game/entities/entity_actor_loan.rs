//! An owning temporary entity actor loan, not a replacement body or a Host.
//! Callback handoffs return the complete actual body to its real Game owner.
use super::Game;
use horizon_client::join_seq::residency::{Kind, Owner, Source, table::Location};
use horizon_client::join_seq::{JoinList, JoinSeq, Joined};
use mod_api::EntityReadCapturePhase;

pub(super) trait ActorStorage<T: Joined> {
    fn extract_actor(&mut self, join: JoinSeq) -> Option<(usize, T)>;
    fn put_back_actor(&mut self, ordinal: usize, body: T);
    fn retire_actor(&mut self, id: u32, join: JoinSeq);
}
impl<T: Joined, const DIRECT: bool> ActorStorage<T> for JoinList<T, DIRECT> {
    fn extract_actor(&mut self, join: JoinSeq) -> Option<(usize, T)> {
        let ordinal = self.locate(join)?;
        self.extract(join).map(|body| (ordinal, body))
    }
    fn put_back_actor(&mut self, ordinal: usize, body: T) {
        self.put_back(ordinal, body);
    }
    fn retire_actor(&mut self, id: u32, join: JoinSeq) {
        self.retire_extracted(id, join);
    }
}
impl<T: Joined> ActorStorage<T> for horizon_client::join_seq::ResidentList<T> {
    fn extract_actor(&mut self, join: JoinSeq) -> Option<(usize, T)> {
        let ordinal = self.locate(join)?;
        self.extract(join).map(|body| (ordinal, body))
    }
    fn put_back_actor(&mut self, ordinal: usize, body: T) {
        self.put_back(ordinal, body);
    }
    fn retire_actor(&mut self, id: u32, join: JoinSeq) {
        self.retire_extracted(id, join);
    }
}

pub(super) trait EntityActor<const DIRECT: bool = true>: Joined + Sized {
    type Storage: ActorStorage<Self>;
    const KIND: Kind;
    fn id(&self) -> u32;
    fn target(&self) -> super::commands::Target;
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut Self::Storage;
}

impl EntityActor<false> for super::boats::Boat {
    type Storage = JoinList<Self, false>;
    const KIND: Kind = Kind::Boat;
    fn id(&self) -> u32 {
        self.id
    }
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Boat(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut Self::Storage {
        &mut game.boats
    }
}

impl EntityActor<false> for super::dragon::Breath {
    type Storage = horizon_client::join_seq::ResidentList<Self>;
    const KIND: Kind = Kind::Cloud;
    fn id(&self) -> u32 {
        self.id
    }
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Cloud(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut Self::Storage {
        &mut game.breath_clouds
    }
}

impl EntityActor<false> for super::minecarts::Minecart {
    type Storage = JoinList<Self, false>;
    const KIND: Kind = Kind::Cart;
    fn id(&self) -> u32 {
        self.id
    }
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Cart(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut Self::Storage {
        &mut game.carts
    }
}

impl EntityActor for super::fireworks::Rocket {
    type Storage = JoinList<Self>;
    const KIND: Kind = Kind::Rocket;
    fn id(&self) -> u32 {
        self.id
    }
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Rocket(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut Self::Storage {
        &mut game.rockets
    }
}
impl EntityActor for super::trident::ThrownTrident {
    type Storage = JoinList<Self>;
    const KIND: Kind = Kind::Trident;
    fn id(&self) -> u32 {
        self.id
    }
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Trident(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut Self::Storage {
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
        let (ordinal, body) = list.extract_actor(join)?;
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
            T::owner(game).retire_actor(body.id(), self.location.join);
        }
    }
    pub(super) fn publish(&self, game: &Game<'_>, phase: EntityReadCapturePhase) {
        game.publish_resident_read_location(self.location, phase);
    }

    pub(super) fn restore(&mut self, game: &mut Game<'_>) {
        if let Some(body) = self.body.take() {
            T::owner(game).put_back_actor(self.ordinal, body);
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
        self.body = T::owner(game)
            .extract_actor(self.location.join)
            .map(|(ordinal, body)| {
                self.ordinal = ordinal;
                body
            });
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

//! The real pickup body's owner, using the shared actor carrier.
use super::{Game, entity_actor_loan::EntityActor};
use horizon_client::entities::ExperienceOrb;
use horizon_client::join_seq::{JoinList, residency::Kind};
impl EntityActor for ExperienceOrb {
    type Storage = JoinList<Self>;
    const KIND: Kind = Kind::Orb;
    fn id(&self) -> u32 {
        self.id
    }
    fn target(&self) -> super::commands::Target {
        super::commands::Target::Orb(self.id)
    }
    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut Self::Storage {
        &mut game.orbs
    }
}

//! Side-effect-free dependencies from the same actual inputs used by saves.
use super::{native_mob_nbt_save::{mob_save_uuid_requests, ordered_mob_passengers}, native_nbt_save_owner::SaveError, persist::{EntityMemory, PassengerSlot}};
use horizon_client::{join_seq::{JoinList, residency::Kind}, mobs::{Mob, leash::Holder}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SaveRequirement {
    Body(Kind),
    Persistence,
    MobRoster,
    BlockRegistry,
    ItemRegistry,
    WorldClock,
    WorldPlayerUuid,
    CommandPlayerUuid,
    GuestUuid(u32),
    PortalStates,
    /// Parent physically admits this ID or explicitly proves logical absence.
    UuidReference(u32),
}
#[derive(Default, Debug)]
pub(super) struct SaveRequirements {
    pub(super) requests: Vec<SaveRequirement>,
}
impl SaveRequirements {
    pub(super) fn require(&mut self, request: SaveRequirement) {
        if !self.requests.contains(&request) { self.requests.push(request); }
    }
    pub(super) fn uuid(&mut self, id: u32) { self.require(SaveRequirement::UuidReference(id)); }

    /// Only actual Mob passengers are serialized. Saved tags are opaque data,
    /// not requests for live owners, and Player passenger storage is not read.
    pub(super) fn mob_passengers(&mut self, mobs: &JoinList<Mob>, persist: &EntityMemory, vehicle: u32) -> Result<(), SaveError> {
        self.require(SaveRequirement::MobRoster);
        self.require(SaveRequirement::Persistence);
        self.visit_passengers(mobs, persist, vehicle, &mut Vec::new())
    }
    fn visit_passengers(&mut self, mobs: &JoinList<Mob>, persist: &EntityMemory, vehicle: u32, path: &mut Vec<u32>) -> Result<(), SaveError> {
        for slot in ordered_mob_passengers(mobs, persist, vehicle) {
            if let PassengerSlot::Live(id) = slot { self.mob_tree(mobs, persist, id, path)?; }
        }
        Ok(())
    }
    fn mob_tree(&mut self, mobs: &JoinList<Mob>, persist: &EntityMemory, id: u32, path: &mut Vec<u32>) -> Result<(), SaveError> {
        if path.contains(&id) { return Err(SaveError::CyclicPassengers(id)); }
        let m = mobs.iter().find(|m| m.id == id).ok_or(SaveError::IncompleteReference(id))?;
        path.push(id);
        self.require(SaveRequirement::Body(Kind::Mob));
        self.require(SaveRequirement::BlockRegistry);
        self.uuid(id);
        let references = mob_save_uuid_requests(m, mobs);
        for id in references.suspects.into_iter().chain(references.target).chain(references.attacker) { self.uuid(id); }
        if horizon_client::mobs::persistent_anger(m.species()) || m.extra.anger > 0 || horizon_client::mobs::camel::is_camel(m.species()) {
            self.require(SaveRequirement::WorldClock);
        }
        match m.extra.leash {
            Some(Holder::Mob(id) | Holder::Boat(id)) => self.uuid(id),
            Some(Holder::Player) if m.extra.leash_player.is_none() => self.require(SaveRequirement::WorldPlayerUuid),
            _ => {}
        }
        self.visit_passengers(mobs, persist, id, path)?;
        path.pop();
        Ok(())
    }
}

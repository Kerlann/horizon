//! Disjoint actual storage and call-local admissions for owner-local saves.
use super::{native_mob_nbt_save::MobNbtOwner, persist::EntityMemory};
use horizon_client::{
    join_seq::{JoinList, residency::table::Location},
    mobs::Mob,
};
use horizon_core::nbt::Tag;
use horizon_world::registry::BlockRegistry;
use mod_api::DimensionKey;

/// Transport from completed physical preflight, never a UUID cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SaveAdmission {
    Present {
        id: u32,
        location: Location,
    },
    /// Preflight proved logical absence; retained persistence rules still apply.
    Absent {
        id: u32,
    },
}
impl SaveAdmission {
    fn id(self) -> u32 {
        match self {
            Self::Present { id, .. } | Self::Absent { id } => id,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SaveError {
    IncompleteReference(u32),
    CyclicPassengers(u32),
    StaleAdmission { id: u32, location: Location },
    UnavailableBody(Location),
}

/// The selected non-Mob body is borrowed separately from these actual fields.
/// The resolver releases its table borrow before returning. Real UUID writes
/// remain in EntityMemory; only their exact deltas escape on success or unwind.
pub(super) struct AdmittedSaveOwner<'a> {
    pub(super) mobs: &'a mut JoinList<Mob>,
    pub(super) persist: &'a mut EntityMemory,
    pub(super) game_time: i64,
    pub(super) registry: &'a BlockRegistry,
    pub(super) player_uuid: Option<[i32; 4]>,
    pub(super) dimension: DimensionKey,
    pub(super) admissions: &'a [SaveAdmission],
    pub(super) primary_uuid:
        &'a mut dyn FnMut(u32, Location) -> Result<Option<[i32; 4]>, SaveError>,
    pub(super) assignments: Vec<(u32, [i32; 4])>,
}
impl AdmittedSaveOwner<'_> {
    pub(super) fn into_assignments(self) -> Vec<(u32, [i32; 4])> {
        let mut assignments = self.assignments;
        // Candidates are staged before insertion so unwind cannot lose a real
        // write. Publish only candidates actually present in the owning map.
        assignments.retain(|(id, uuid)| self.persist.uuids.get(id) == Some(uuid));
        assignments
    }
    pub(super) fn validate_references(
        &mut self,
        requirements: &super::native_nbt_save_requirements::SaveRequirements,
    ) -> Result<(), SaveError> {
        use super::native_nbt_save_requirements::SaveRequirement;
        for request in &requirements.requests {
            if let SaveRequirement::UuidReference(id) = *request {
                let token = self.admissions.iter().copied().find(|token| token.id() == id)
                    .ok_or(SaveError::IncompleteReference(id))?;
                if let SaveAdmission::Present { id, location } = token {
                    (self.primary_uuid)(id, location)?;
                }
            }
        }
        Ok(())
    }
}
impl MobNbtOwner for AdmittedSaveOwner<'_> {
    type Error = SaveError;
    fn mobs(&self) -> &JoinList<Mob> {
        self.mobs
    }
    fn mobs_mut(&mut self) -> &mut JoinList<Mob> {
        self.mobs
    }
    fn persist(&self) -> &EntityMemory {
        self.persist
    }
    fn game_time(&self) -> i64 {
        self.game_time
    }
    fn registry(&self) -> &BlockRegistry {
        self.registry
    }
    fn uuid_for(&mut self, id: u32) -> Result<[i32; 4], SaveError> {
        let token = self
            .admissions
            .iter()
            .copied()
            .find(|token| token.id() == id)
            .ok_or(SaveError::IncompleteReference(id))?;
        let known = match token {
            SaveAdmission::Present { id, location } => {
                let known = (self.primary_uuid)(id, location)?;
                // Game::uuid_for only uses a primary UUID from its own level.
                if location.owner.dimension == self.dimension {
                    known
                } else {
                    None
                }
            }
            SaveAdmission::Absent { .. } => None,
        };
        if let Some(uuid) = known {
            return Ok(uuid);
        }
        if let Some(&uuid) = self.persist.uuids.get(&id) {
            return Ok(uuid);
        }
        let uuid = horizon_client::save::random_uuid();
        self.assignments.push((id, uuid));
        self.persist.uuids.insert(id, uuid);
        Ok(uuid)
    }
    fn write_leash(&mut self, tag: &mut Tag, index: usize) -> Result<(), SaveError> {
        let m = &self.mobs[index];
        let (holder, pending, player) =
            (m.extra.leash, m.extra.leash_pending, m.extra.leash_player);
        super::leads::write_mob_leash_tag(tag, holder, pending, player, self.player_uuid, |id| {
            self.uuid_for(id)
        })
    }
}

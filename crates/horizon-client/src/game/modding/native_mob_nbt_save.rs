//! Actual Mob save behavior shared by active and parked owner storage.
use super::{
    Game,
    persist::{EntityMemory, PassengerSlot, add_passengers},
};
use horizon_client::{
    entity_nbt,
    join_seq::JoinList,
    mobs::{Mob, Who},
};
use horizon_core::nbt::Tag;
use horizon_world::registry::BlockRegistry;
use std::collections::HashSet;

pub(super) trait MobNbtOwner {
    type Error;
    fn mobs(&self) -> &JoinList<Mob>;
    fn mobs_mut(&mut self) -> &mut JoinList<Mob>;
    fn persist(&self) -> &EntityMemory;
    fn game_time(&self) -> i64;
    fn registry(&self) -> &BlockRegistry;
    fn uuid_for(&mut self, id: u32) -> Result<[i32; 4], Self::Error>;
    fn write_leash(&mut self, tag: &mut Tag, index: usize) -> Result<(), Self::Error>;

    fn mob_tree_nbt(&mut self, i: usize) -> Result<Tag, Self::Error> {
        let id = self.mobs()[i].id;
        let uuid = self.uuid_for(id)?;
        // A warden's anger names each suspect by UUID. A mob that has never been
        // loaded or saved has no UUID yet, so give it one, or the save would
        // drop the anger at it (`who_uuid` writes nothing for a mob the map does
        // not know).
        let suspects: Vec<u32> = self.mobs()[i]
            .extra
            .warden
            .as_ref()
            .map(|st| {
                st.anger
                    .suspects()
                    .iter()
                    .filter_map(|who| match who {
                        Who::Mob(id) => Some(*id),
                        Who::Player => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        for suspect in suspects {
            self.uuid_for(suspect)?;
        }
        // `NeutralMob.addPersistentAngerSaveData`: the anger ends at an
        // absolute game time, so it goes on while the chunk is unloaded, and
        // the target is the entity the mob is angry at.
        let anger = self.mobs()[i].extra.anger;
        if horizon_client::mobs::persistent_anger(self.mobs()[i].species()) {
            let game_time = self.game_time();
            horizon_client::mobs::settle_anger_end(&mut self.mobs_mut()[i], game_time);
        } else {
            self.mobs_mut()[i].extra.anger_end = if anger > 0 {
                self.game_time() + i64::from(anger)
            } else {
                -1
            };
        }
        self.mobs_mut()[i].extra.angry_at = if anger > 0 {
            // A wolf and an iron golem keep their anger's target themselves,
            // also once the target is gone (`tame::update_persistent_anger`).
            if matches!(
                self.mobs()[i].species(),
                horizon_client::mobs::Species::Wolf | horizon_client::mobs::Species::IronGolem
            ) {
                self.mobs()[i].extra.angry_at
            } else {
                self.mobs()[i].ai.target
            }
        } else {
            None
        };
        // A mob gets its UUID when it is first saved (`uuid_for`): the anger
        // target still in the world gets it now, so `uuid_of` can name it.
        if let Some(horizon_client::mobs::Who::Mob(target)) = self.mobs()[i].extra.angry_at
            && target != 0
            && self.mobs().iter().any(|m| m.id == target)
        {
            self.uuid_for(target)?;
        }
        // The mob that last hurt it, still resident, gets its UUID now, as the
        // anger target does, so `put_last_hurt_by_mob` can name it.
        if let Some(horizon_client::mobs::Who::Mob(attacker)) = self.mobs()[i].ai.last_hurt_by
            && self.mobs().iter().any(|m| m.id == attacker)
        {
            self.uuid_for(attacker)?;
        }
        let mut tag = entity_nbt::mob_to_nbt(
            &self.mobs()[i],
            uuid,
            self.persist().raw.get(&id),
            Some(self.registry()),
            |other| self.persist().uuid_of(other),
        );
        self.put_last_hurt_by_mob(&self.mobs()[i], &mut tag);
        if horizon_client::mobs::camel::is_camel(self.mobs()[i].species())
            && let Tag::Compound(c) = &mut tag
        {
            let tick =
                horizon_client::mobs::camel::synced_pose_tick(&self.mobs()[i], self.game_time());
            c.insert("LastPoseTick".into(), Tag::Long(tick));
        }
        self.write_leash(&mut tag, i)?;
        self.put_mob_passengers(&mut tag, id)?;
        Ok(tag)
    }

    fn put_last_hurt_by_mob(&self, m: &horizon_client::mobs::Mob, tag: &mut Tag) {
        let Tag::Compound(c) = tag else { return };
        let since = i64::from(m.age) - i64::from(m.ai.last_hurt_by_time);
        let reference = match m.ai.last_hurt_by {
            Some(horizon_client::mobs::Who::Mob(attacker))
                if (0..=100).contains(&since)
                    && self.mobs().iter().any(|a| a.id == attacker && a.alive()) =>
            {
                self.persist().uuid_of(attacker)
            }
            _ => None,
        };
        match reference {
            Some(uuid) => {
                c.insert("last_hurt_by_mob".into(), Tag::IntArray(uuid.to_vec()));
                c.insert(
                    "ticks_since_last_hurt_by_mob".into(),
                    Tag::Int(since as i32),
                );
            }
            None => {
                c.remove(&horizon_core::nbt::JavaString::from("last_hurt_by_mob"));
                c.remove(&horizon_core::nbt::JavaString::from(
                    "ticks_since_last_hurt_by_mob",
                ));
            }
        }
    }

    fn put_mob_passengers(&mut self, tag: &mut Tag, id: u32) -> Result<(), Self::Error> {
        let mut riders: Vec<Tag> = Vec::new();
        let mut live_seen = HashSet::new();

        let slots = self.persist().passenger_slots.get(&id).cloned();
        if let Some(slots) = slots {
            for slot in slots {
                match slot {
                    PassengerSlot::Live(rider_id) => {
                        if let Some(pos) = self.mobs().iter().position(|m| {
                            m.id == rider_id && m.extra.mount == Some(id) && m.alive() && !m.removed
                        }) {
                            live_seen.insert(rider_id);
                            riders.push(self.mob_tree_nbt(pos)?);
                        }
                    }
                    PassengerSlot::Saved(saved_tag) => {
                        riders.push(saved_tag.clone());
                    }
                }
            }
        }

        let mut extra_riders: Vec<usize> = (0..self.mobs().len())
            .filter(|&j| {
                self.mobs()[j].extra.mount == Some(id)
                    && self.mobs()[j].alive()
                    && !self.mobs()[j].removed
                    && !live_seen.contains(&self.mobs()[j].id)
            })
            .collect();
        extra_riders.sort_by_key(|&j| self.mobs()[j].extra.vehicle_order);
        if let Some(driver) = self
            .mobs()
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.extra.rider)
            && let Some(pos) = extra_riders
                .iter()
                .position(|&j| self.mobs()[j].id == driver)
            && pos != 0
        {
            let d = extra_riders.remove(pos);
            extra_riders.insert(0, d);
        }
        for j in extra_riders {
            riders.push(self.mob_tree_nbt(j)?);
        }

        if let Tag::Compound(c) = tag {
            c.remove(&"Passengers".into());
        }
        add_passengers(tag, riders);
        Ok(())
    }
}

impl MobNbtOwner for Game<'_> {
    type Error = std::convert::Infallible;
    fn mobs(&self) -> &JoinList<Mob> {
        &self.mobs
    }
    fn mobs_mut(&mut self) -> &mut JoinList<Mob> {
        &mut self.mobs
    }
    fn persist(&self) -> &EntityMemory {
        &self.persist
    }
    fn game_time(&self) -> i64 {
        self.world.game_time
    }
    fn registry(&self) -> &BlockRegistry {
        self.pack.assets.registry
    }
    fn uuid_for(&mut self, id: u32) -> Result<[i32; 4], Self::Error> {
        Ok(Game::uuid_for(self, id))
    }
    fn write_leash(&mut self, tag: &mut Tag, index: usize) -> Result<(), Self::Error> {
        Game::write_leash(self, tag, index);
        Ok(())
    }
}

/// A genuine parked owner's storage; only new UUID assignments escape the loan.
pub(super) struct ParkedMobNbt<'a> {
    mobs: &'a mut JoinList<Mob>,
    persist: &'a mut EntityMemory,
    game_time: i64,
    registry: &'a BlockRegistry,
    player_uuid: Option<[i32; 4]>,
    assignments: Vec<(u32, [i32; 4])>,
}

impl<'a> ParkedMobNbt<'a> {
    pub(super) fn new(
        mobs: &'a mut JoinList<Mob>,
        persist: &'a mut EntityMemory,
        game_time: i64,
        registry: &'a BlockRegistry,
        player_uuid: Option<[i32; 4]>,
    ) -> Self {
        Self {
            mobs,
            persist,
            game_time,
            registry,
            player_uuid,
            assignments: Vec::new(),
        }
    }

    pub(super) fn into_assignments(self) -> Vec<(u32, [i32; 4])> {
        self.assignments
    }
}

impl MobNbtOwner for ParkedMobNbt<'_> {
    type Error = std::convert::Infallible;
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
    fn uuid_for(&mut self, id: u32) -> Result<[i32; 4], Self::Error> {
        if let Some(&uuid) = self.persist.uuids.get(&id) {
            return Ok(uuid);
        }
        let uuid = horizon_client::save::random_uuid();
        self.persist.uuids.insert(id, uuid);
        self.assignments.push((id, uuid));
        Ok(uuid)
    }
    fn write_leash(&mut self, tag: &mut Tag, index: usize) -> Result<(), Self::Error> {
        let m = &self.mobs[index];
        let (holder, pending, player) =
            (m.extra.leash, m.extra.leash_pending, m.extra.leash_player);
        super::leads::write_mob_leash_tag(tag, holder, pending, player, self.player_uuid, |id| {
            self.uuid_for(id)
        })
    }
}

impl Game<'_> {
    /// The actual Mob's saved tree in its owning dimension, including a newer
    /// live trade session. Command callers retain their existing Option result;
    /// mod-facing immediate refusal is a separate boundary.
    pub(super) fn mob_owner_nbt(&mut self, id: u32) -> Option<Tag> {
        use horizon_client::join_seq::residency::table::EntityLookup;
        let EntityLookup::Found {
            location,
            value: super::commands::Target::Mob(_),
        } = self.server_resident_entity(id)
        else {
            return None;
        };
        let mut tag = if location.owner.dimension == self.dim.key() {
            let index = self.mobs.locate(location.join)?;
            Game::mob_tree_nbt(self, index)
        } else {
            self.parked_mob_tree_nbt(location)?
        };
        let session = self
            .lan_trade_conn(id)
            .and_then(|conn| self.lan_trade_session(conn))
            .or_else(|| {
                self.station
                    .as_ref()
                    .and_then(|s| s.merchant.as_ref())
                    .filter(|m| m.mob == Some(id))
                    .map(|m| &*m.session)
            });
        if let (Some(session), Tag::Compound(data)) = (session, &mut tag) {
            data.insert("Offers".into(), entity_nbt::offers_to_nbt(&session.offers));
            data.insert("Xp".into(), Tag::Int(session.xp));
        }
        Some(tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn actual_parked_mob_save_keeps_owner_clock_passengers_and_caller_level() {
        crate::game::native_entity_refs::tests::with_game(|game| {
            game.enter_level(crate::game::Dim::Nether).unwrap();
            game.world.game_time = 9000;
            let wolf_kind = game.res.mob_types().get("minecraft:wolf").unwrap().clone();
            let pig_kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            game.persist.uuids.insert(8801, [11, 22, 33, 44]);
            game.persist.uuids.insert(8802, [55, 66, 77, 88]);
            let mut wolf = Mob::new(8801, wolf_kind, Vec3::new(2.0, 80.0, 0.5), 0.0);
            wolf.extra.anger = 25;
            wolf.extra.rider = Some(8802);
            let mut rider = Mob::new(8802, pig_kind, Vec3::new(2.0, 81.0, 0.5), 0.0);
            rider.extra.mount = Some(8801);
            game.mobs.push(wolf);
            game.mobs.push(rider);
            let join = game.mobs.iter().find(|m| m.id == 8801).unwrap().join;
            game.enter_level(crate::game::Dim::Overworld).unwrap();
            game.world.game_time = 50;
            let caller_level = game.dim.key();
            let caller_ids: Vec<_> = game.mobs.iter().map(|m| m.id).collect();

            let tag = game
                .command_target_nbt(crate::game::commands::Target::Mob(8801))
                .unwrap();
            assert_eq!(game.dim.key(), caller_level);
            assert_eq!(game.world.game_time, 50);
            assert_eq!(
                game.mobs.iter().map(|m| m.id).collect::<Vec<_>>(),
                caller_ids
            );
            assert_eq!(tag.get("UUID"), Some(&Tag::IntArray(vec![11, 22, 33, 44])));
            assert_eq!(tag.get("anger_end_time"), Some(&Tag::Long(9025)));
            let Some(Tag::List(passengers)) = tag.get("Passengers") else {
                panic!("actual rider was omitted");
            };
            assert_eq!(passengers.len(), 1);
            assert_eq!(
                passengers[0].get("UUID"),
                Some(&Tag::IntArray(vec![55, 66, 77, 88]))
            );
            let parked = game.levels.parked_of("minecraft:the_nether").unwrap();
            let at = parked.mobs.locate(join).unwrap();
            assert_eq!(parked.mobs[at].extra.anger_end, 9025);
        });
    }
}

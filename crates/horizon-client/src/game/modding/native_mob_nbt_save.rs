//! Actual Mob save behavior shared by active and parked owner storage.
use super::{
    Game,
    persist::{EntityMemory, PassengerSlot},
};
use horizon_client::{
    entity_nbt,
    join_seq::JoinList,
    mobs::{Mob, Who},
};
use horizon_core::nbt::Tag;
use std::collections::HashSet;

pub(super) trait MobNbtOwner {
    fn mob_tree_nbt(&mut self, i: usize) -> Tag {
        let id = self.mobs[i].id;
        let uuid = self.uuid_for(id);
        // A warden's anger names each suspect by UUID. A mob that has never been
        // loaded or saved has no UUID yet, so give it one, or the save would
        // drop the anger at it (`who_uuid` writes nothing for a mob the map does
        // not know).
        let suspects: Vec<u32> = self.mobs[i]
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
            self.uuid_for(suspect);
        }
        // `NeutralMob.addPersistentAngerSaveData`: the anger ends at an
        // absolute game time, so it goes on while the chunk is unloaded, and
        // the target is the entity the mob is angry at.
        let anger = self.mobs[i].extra.anger;
        // A loaded NoAI Enderman owns an absolute NeutralMob timestamp;
        // its skipped AI does not decrement the derived gameplay counter.
        if !(self.mobs[i].species() == horizon_client::mobs::Species::Enderman
            && self.mobs[i].no_ai)
        {
            self.mobs[i].extra.anger_end = if anger > 0 {
                self.world.game_time + i64::from(anger)
            } else {
                -1
            };
        }
        self.mobs[i].extra.angry_at = if anger > 0 {
            // A wolf and an iron golem keep their anger's target themselves,
            // also once the target is gone (`tame::update_persistent_anger`).
            if matches!(
                self.mobs[i].species(),
                horizon_client::mobs::Species::Wolf | horizon_client::mobs::Species::IronGolem
            ) {
                self.mobs[i].extra.angry_at
            } else {
                self.mobs[i].ai.target
            }
        } else {
            None
        };
        // A mob gets its UUID when it is first saved (`uuid_for`): the anger
        // target still in the world gets it now, so `uuid_of` can name it.
        if let Some(horizon_client::mobs::Who::Mob(target)) = self.mobs[i].extra.angry_at
            && target != 0
            && self.mobs.iter().any(|m| m.id == target)
        {
            self.uuid_for(target);
        }
        // The mob that last hurt it, still resident, gets its UUID now, as the
        // anger target does, so `put_last_hurt_by_mob` can name it.
        if let Some(horizon_client::mobs::Who::Mob(attacker)) = self.mobs[i].ai.last_hurt_by
            && self.mobs.iter().any(|m| m.id == attacker)
        {
            self.uuid_for(attacker);
        }
        let mut tag = entity_nbt::mob_to_nbt(
            &self.mobs[i],
            uuid,
            self.persist.raw.get(&id),
            Some(self.pack.assets.registry),
            |other| self.persist.uuid_of(other),
        );
        self.put_last_hurt_by_mob(&self.mobs[i], &mut tag);
        if horizon_client::mobs::camel::is_camel(self.mobs[i].species())
            && let Tag::Compound(c) = &mut tag
        {
            let tick =
                horizon_client::mobs::camel::synced_pose_tick(&self.mobs[i], self.world.game_time);
            c.insert("LastPoseTick".into(), Tag::Long(tick));
        }
        self.write_leash(&mut tag, i);
        self.put_mob_passengers(&mut tag, id);
        tag
    }

    fn put_last_hurt_by_mob(&self, m: &horizon_client::mobs::Mob, tag: &mut Tag) {
        let Tag::Compound(c) = tag else { return };
        let since = i64::from(m.age) - i64::from(m.ai.last_hurt_by_time);
        let reference = match m.ai.last_hurt_by {
            Some(horizon_client::mobs::Who::Mob(attacker))
                if (0..=100).contains(&since)
                    && self.mobs.iter().any(|a| a.id == attacker && a.alive()) =>
            {
                self.persist.uuid_of(attacker)
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

    fn put_mob_passengers(&mut self, tag: &mut Tag, id: u32) {
        let mut riders: Vec<Tag> = Vec::new();
        let mut live_seen = HashSet::new();

        let slots = self.persist.passenger_slots.get(&id).cloned();
        if let Some(slots) = slots {
            for slot in slots {
                match slot {
                    PassengerSlot::Live(rider_id) => {
                        if let Some(pos) = self.mobs.iter().position(|m| {
                            m.id == rider_id && m.extra.mount == Some(id) && m.alive() && !m.removed
                        }) {
                            live_seen.insert(rider_id);
                            riders.push(self.mob_tree_nbt(pos));
                        }
                    }
                    PassengerSlot::Saved(saved_tag) => {
                        riders.push(saved_tag.clone());
                    }
                }
            }
        }

        let mut extra_riders: Vec<usize> = (0..self.mobs.len())
            .filter(|&j| {
                self.mobs[j].extra.mount == Some(id)
                    && self.mobs[j].alive()
                    && !self.mobs[j].removed
                    && !live_seen.contains(&self.mobs[j].id)
            })
            .collect();
        extra_riders.sort_by_key(|&j| self.mobs[j].extra.vehicle_order);
        if let Some(driver) = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.extra.rider)
            && let Some(pos) = extra_riders.iter().position(|&j| self.mobs[j].id == driver)
            && pos != 0
        {
            let d = extra_riders.remove(pos);
            extra_riders.insert(0, d);
        }
        for j in extra_riders {
            riders.push(self.mob_tree_nbt(j));
        }

        if let Tag::Compound(c) = tag {
            c.remove(&"Passengers".into());
        }
        add_passengers(tag, riders);
    }
}

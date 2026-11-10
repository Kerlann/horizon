//! Reads only from the explicit mob actor loaned to the active goal.
use super::GoalCx;
use crate::mobs::Mob;
use horizon_client::mobs::villager;
use mod_api::{Hand, LivingRef};

impl GoalCx<'_, '_> {
    fn live_living_mob(&self, living: LivingRef) -> Option<&Mob> {
        self.entity_mob(living.entity())
    }

    pub(super) fn r_living_gliding_ticks(&self, living: LivingRef) -> Option<i32> {
        Some(self.live_living_mob(living)?.extra.fall_fly_ticks)
    }

    pub(super) fn r_living_is_gliding(&self, living: LivingRef) -> Option<bool> {
        Some(self.live_living_mob(living)?.extra.fall_flying)
    }

    pub(super) fn r_living_is_sleeping(&self, living: LivingRef) -> Option<bool> {
        Some(villager::sleeping(self.live_living_mob(living)?))
    }

    pub(super) fn r_living_sleeping_pos(&self, living: LivingRef) -> Option<[i32; 3]> {
        villager::sleeping_pos(self.live_living_mob(living)?)
    }

    pub(super) fn r_living_is_jumping(&self, living: LivingRef) -> Option<bool> {
        self.live_living_mob(living).map(|_| false)
    }

    pub(super) fn r_living_baby(&self, living: LivingRef) -> Option<bool> {
        Some(self.live_living_mob(living)?.extra.age < 0)
    }

    pub(super) fn r_living_is_tamed(&self, living: LivingRef) -> Option<bool> {
        let mob = self.live_living_mob(living)?;
        mob.is_tamable().then_some(mob.extra.tame)
    }

    pub(super) fn r_living_owner(&self, living: LivingRef) -> Option<u128> {
        let mob = self.live_living_mob(living)?;
        if !mob.is_tamable() {
            return None;
        }
        Some(horizon_client::lan::playerdata::ints_uuid(mob.extra.owner?))
    }

    pub(super) fn r_living_can_breathe_underwater(&self, living: LivingRef) -> Option<bool> {
        Some(self.live_living_mob(living)?.kind.flags.breathes_water)
    }

    fn live_mob_attribute(
        &self,
        living: LivingRef,
        id: &str,
        kind: horizon_world::attribute_map::ValueKind,
    ) -> Option<f64> {
        let m = self.live_living_mob(living)?;
        m.extra
            .live
            .read(self.w.attributes, &m.kind.id, id, kind, || {
                horizon_client::entity_nbt::attribute_entries(m)
                    .into_iter()
                    .map(|(_, entry)| entry)
                    .collect()
            })
    }

    pub(super) fn r_living_used_item_hand(&self, living: LivingRef) -> Option<Hand> {
        let mob = self.live_living_mob(living)?;
        horizon_client::mobs::spear_use::is_using_item(mob).then_some(if mob.extra.using_off_hand {
            Hand::Off
        } else {
            Hand::Main
        })
    }
    pub(super) fn r_living_is_climbing(&self, living: LivingRef) -> Option<bool> {
        let mob = self.live_living_mob(living)?;
        Some(self.w.goal_host?.mob_is_climbing(mob))
    }

    pub(super) fn r_living_last_attacker(&self, living: LivingRef) -> Option<mod_api::EntityRef> {
        let mob = self.live_living_mob(living)?;
        let who = mob
            .ai
            .last_hurt_by
            .filter(|_| mob.age as i32 - mob.ai.last_hurt_by_time <= 100)?;
        let id = match who {
            crate::mobs::Who::Mob(id) => id,
            crate::mobs::Who::Player => self.w.goal_host?.mob_player_credit(mob)??,
        };
        self.lookup_entity(id)
    }

    pub(super) fn r_living_kill_credit(&self, living: LivingRef) -> Option<mod_api::EntityRef> {
        let mob = self.live_living_mob(living)?;
        if let Some(credit) = self.w.goal_host?.mob_player_credit(mob) {
            return self.lookup_entity(credit?);
        }
        self.r_living_last_attacker(living)
    }
    pub(super) fn r_living_can_use_portal(&self, living: LivingRef) -> Option<bool> {
        Some(
            self.w
                .goal_host?
                .mob_can_use_portal(self.live_living_mob(living)?),
        )
    }
    pub(super) fn r_living_item_use_remaining(&self, living: LivingRef) -> Option<i32> {
        self.w
            .goal_host?
            .mob_item_use_remaining_lent(self.live_living_mob(living)?)
    }
}

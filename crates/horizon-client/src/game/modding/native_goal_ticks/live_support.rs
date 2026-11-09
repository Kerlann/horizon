//! Reads combining the explicit actor loan with authoritative game state.
use super::GoalCx;
use crate::mobs::Mob;
use mod_api::EntityRef;

impl GoalCx<'_, '_> {
    pub(super) fn r_entity_on_block(&self, entity: EntityRef) -> Option<[i32; 3]> {
        Some(self.w.goal_host?.mob_on_block(self.entity_mob(entity)?))
    }

    pub(super) fn r_entity_entities_team_damage_allowed(
        &self,
        entity: EntityRef,
        source: EntityRef,
    ) -> Option<bool> {
        let mob = |handle| -> Option<Option<&Mob>> {
            if self.is_own_entity(handle) {
                Some(Some(&*self.m))
            } else {
                (self.host.entity(handle.id()) == Some(handle)).then_some(None)
            }
        };
        self.w.goal_host?.entities_team_damage_allowed(
            entity,
            mob(entity)?,
            source,
            mob(source)?,
            self.m,
            self.w,
        )
    }
}

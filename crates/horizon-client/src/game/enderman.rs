//! Enderman leave-block attempts need the authoritative block simulation.
use super::Game;
use horizon_client::{Result, mobs::Species};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
};
use horizon_world::{
    level::{LevelAccess, flags},
    registry::BlockStateId,
};

impl Game<'_> {
    /// `EndermanLeaveBlockGoal.tick`: normalize, check, place, then clear.
    pub(super) fn enderman_place_block(
        &mut self,
        mob: u32,
        pos: [i32; 3],
        carried: BlockStateId,
        occupants: &[Aabb],
    ) -> Result<()> {
        // Runtime ids identify the actual mob in this level. An obsolete attempt
        // must not clear another mob's hand or a subsequently changed stack.
        if !self.mobs.iter().any(|m| {
            m.id == mob
                && m.alive()
                && !m.removed
                && m.species() == Species::Enderman
                && m.extra.carried == Some(carried)
        }) {
            return Ok(());
        }
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        let area = Aabb::new(
            Vec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])),
            Vec3::new(
                f64::from(pos[0] + 1),
                f64::from(pos[1] + 1),
                f64::from(pos[2] + 1),
            ),
        );
        struct Candidate {
            state: BlockStateId,
            target: BlockStateId,
            below: BlockStateId,
        }
        let candidate = self.with_level(|b, level, ticks| {
            let target = level.block_state(at);
            let below = level.block_state(at.offset(0, -1, 0));
            // Must run even when eligibility fails: updateShape can schedule
            // ticks and invoke native hooks on the real level.
            let state = b.update_from_neighbour_shapes(level, ticks, at, carried);
            let support = level.facts.get(below);
            if !level.facts.is_air(target)
                || level.facts.is_air(below)
                || &*support.name == "minecraft:bedrock"
                || !support.full_collision
                || occupants.iter().any(|bounds| bounds.intersects(area))
            {
                return None;
            }
            Some(Candidate {
                state,
                target,
                below,
            })
        });
        let Some(candidate) = candidate else {
            return self.process_level_events();
        };
        // Keep the fork's entity_place veto, with the normalized state. A cancel
        // leaves the original carried block intact, even when it normalized to AIR.
        if self.place_canceled(mob, at, candidate.state, candidate.below, None) {
            return self.process_level_events();
        }
        // A listener can change the candidate's blocks or the mob. Refuse that
        // stale attempt instead of overwriting its writes or normalizing twice.
        let kind = self
            .mobs
            .iter()
            .find(|m| {
                m.id == mob
                    && m.alive()
                    && !m.removed
                    && m.species() == Species::Enderman
                    && m.extra.carried == Some(carried)
            })
            .map(|m| m.kind.id.clone());
        let Some(kind) = kind else {
            return self.process_level_events();
        };
        if self.world.block(pos[0], pos[1], pos[2]) != candidate.target
            || self.world.block(pos[0], pos[1] - 1, pos[2]) != candidate.below
        {
            return self.process_level_events();
        }
        let facts = self.res.facts();
        let from = facts.get(candidate.target).name.clone();
        let to = facts.get(candidate.state).name.clone();
        // AIR is a legitimate normalized state. setBlock may report no
        // change for it; the eligible operation still empties the hand.
        let changed = self.with_level(|b, level, ticks| {
            b.set_block(level, ticks, at, candidate.state, flags::UPDATE_ALL)
        });
        if let Some(m) = self.mobs.iter_mut().find(|m| {
            m.id == mob
                && !m.removed
                && m.species() == Species::Enderman
                && m.extra.carried == Some(carried)
        }) {
            m.extra.carried = None;
        }
        self.process_level_events()?;
        if changed {
            self.native_entity_change_block(mob, &kind, pos, &from, &to);
        }
        Ok(())
    }
}

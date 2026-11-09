//! Invalidation of owned AI player/team projections between mutable mob hooks.
//!
//! Mutation owners must mark AFTER actual apply, including synchronous writes;
//! enqueue acceptance must not mark. `mark_mob_player_backing_dirty` covers player
//! position, visibility, inventory/equipment/effects, mode, lifecycle and guest
//! membership changes represented by PlayerView or LevelPlayer. Team membership,
//! collision/friendly-fire rules and tame/root-owner relationship changes use
//! `mark_mob_team_backing_dirty`; alliances are read canonically, not projected into every player. Canonical
//! scoreboard mutation and command paths must use that team marker too. Chat,
//! sounds and unrelated scalar entity writes such as set_silent mark neither.
//!
//! These are integration seams, not a claim all mutation paths are instrumented.
//! The loop compares revisions before each reborrow, retaining unchanged backing.
use super::Game;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct MobBackingRevision {
    pub(super) players: u64,
    pub(super) teams: u64,
}
impl MobBackingRevision {
    fn players_changed(&mut self) {
        self.players = self
            .players
            .checked_add(1)
            .expect("mob player backing revision exhausted");
    }
    fn teams_changed(&mut self) {
        self.teams = self
            .teams
            .checked_add(1)
            .expect("mob team backing revision exhausted");
    }
}
impl Game<'_> {
    pub(super) fn mark_mob_player_backing_dirty(&self) {
        self.entity_read_state
            .borrow_mut()
            .mob_backing
            .players_changed();
    }
    pub(super) fn mark_mob_team_backing_dirty(&self) {
        self.entity_read_state
            .borrow_mut()
            .mob_backing
            .teams_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_masks_invalidate_only_the_required_backing() {
        let mut revision = MobBackingRevision::default();
        revision.players_changed();
        assert_eq!(
            revision,
            MobBackingRevision {
                players: 1,
                teams: 0
            }
        );
        revision.teams_changed();
        assert_eq!(
            revision,
            MobBackingRevision {
                players: 1,
                teams: 1
            }
        );
    }
}

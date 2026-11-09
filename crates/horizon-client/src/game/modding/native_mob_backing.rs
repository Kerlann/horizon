//! Invalidation of owned AI player/team projections between mutable mob hooks.
//!
//! Mutation owners must mark AFTER actual apply, including synchronous writes;
//! enqueue acceptance must not mark. `mark_mob_player_backing_dirty` covers player
//! position, visibility, inventory/equipment/effects, mode, lifecycle and guest
//! membership changes represented by PlayerView or LevelPlayer. Team membership,
//! collision/friendly-fire rules and tame/root-owner relationship changes use
//! `mark_mob_team_backing_dirty` (which also invalidates player allies). Canonical
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
    pub(super) fn mark_mob_player_backing_dirty(&self, key: PlayerBackingKey) {
        let mut state = self.entity_read_state.borrow_mut();
        state.mob_player_writes.insert(key.uuid, key.conn);
        state.mob_backing.players_changed();
    }
    pub(super) fn mark_mob_team_backing_dirty(&self) {
        self.entity_read_state
            .borrow_mut()
            .mob_backing
            .teams_changed();
    }
}

impl Game<'_> {
    pub(super) fn ai_player_view(
        &self,
        teams: &horizon_client::mobs::TeamRulesSnapshot,
    ) -> Option<horizon_client::mobs::PlayerView> {
        let Some((facts, _)) = &teams.shared else {
            return self.player_view();
        };
        self.player_view_with_allies(std::sync::Arc::new(
            horizon_client::mobs::PlayerAllies::shared(facts.clone(), self.acting_uuid()),
        ))
    }
    /// A canonical tame/root-owner write emits identities only, after apply.
    pub(super) fn mark_mob_owner_backing_changed(
        &self,
        id: u32,
        owner: Option<([i32; 4], Option<[i32; 4]>)>,
    ) {
        if self.score.board.is_ai_tracking() {
            self.entity_read_state
                .borrow_mut()
                .mob_owner_writes
                .insert(id, owner);
        }
    }
    /// Apply only changed identities/rules. Descendants follow the bounded owner
    /// chain at query time: changing one root never walks the population.
    pub(super) fn refresh_mob_team_deltas(
        &mut self,
        teams: &horizon_client::mobs::TeamRulesSnapshot,
    ) -> usize {
        use horizon_client::scoreboard::TeamAiChange;
        let changes = self.score.board.take_ai_changes();
        let mut applied = 0;
        if let Some((shared, _)) = &teams.shared {
            let mut facts = shared.write().expect("AI team backing poisoned");
            for change in &changes {
                match change {
                    TeamAiChange::Member { member, team } => {
                        let uuid = facts.names.get(member).copied().or_else(|| {
                            horizon_client::lan::arrivals::parse_uuid(member)
                                .map(horizon_client::lan::playerdata::uuid_ints)
                                .filter(|uuid| !facts.players.contains(uuid))
                        });
                        if let Some(uuid) = uuid {
                            match team {
                                Some(team) => {
                                    facts.members.insert(uuid, *team);
                                }
                                None => {
                                    facts.members.remove(&uuid);
                                }
                            }
                            applied += 1;
                        }
                    }
                    TeamAiChange::Rules(rule) => {
                        facts.rules.insert(rule.identity, *rule);
                        applied += 1;
                    }
                    TeamAiChange::Removed(id) => {
                        facts.rules.remove(id);
                        applied += 1;
                    }
                }
            }
            let mut state = self.entity_read_state.borrow_mut();
            for (id, owner) in state.mob_owner_writes.drain() {
                if let Some(old) = facts.identities.remove(&id) {
                    facts.owners.remove(&old);
                }
                if let Some((uuid, owner)) = owner {
                    facts.identities.insert(id, uuid);
                    if let Some(owner) = owner {
                        facts.owners.insert(uuid, owner);
                    }
                }
                applied += 1;
            }
            if applied != 0 {
                state.mob_backing.teams_changed();
            }
        }
        self.score.board.recycle_ai_changes(changes);
        applied
    }
}

/// Exact represented facts at a canonical player turn/apply boundary. Equipment
/// identifiers are shared Arcs, effects are static IDs, and allies are excluded:
/// team membership has its own canonical delta producer.
#[derive(Debug, PartialEq)]
pub(in crate::game) struct PlayerAiFacts {
    view: Option<horizon_client::mobs::PlayerView>,
    health: Option<u32>,
    dimension: mod_api::DimensionKey,
    key: PlayerBackingKey,
    fake: bool,
}
impl Game<'_> {
    pub(in crate::game) fn player_ai_facts(&self) -> Option<PlayerAiFacts> {
        self.score.board.is_ai_tracking().then(|| PlayerAiFacts {
            view: self.player_view_with_allies(Default::default()),
            health: (!self.lan_acting_fake)
                .then(|| self.player.as_ref().map(|p| p.health.health().to_bits()))
                .flatten(),
            key: PlayerBackingKey {
                uuid: self.acting_uuid(),
                conn: self.lan_acting,
            },
            dimension: self.dim.key(),
            fake: self.lan_acting_fake,
        })
    }
    pub(in crate::game) fn player_ai_facts_delta(
        &self,
        before: &Option<PlayerAiFacts>,
    ) -> Option<(PlayerBackingKey, PlayerBackingKey)> {
        let before = before.as_ref()?;
        let after = self.player_ai_facts()?;
        (before != &after).then_some((before.key, after.key))
    }
    pub(in crate::game) fn publish_player_ai_delta(
        &self,
        delta: Option<(PlayerBackingKey, PlayerBackingKey)>,
    ) {
        if let Some((before, after)) = delta {
            self.mark_mob_player_backing_dirty(before);
            if before != after {
                self.mark_mob_player_backing_dirty(after);
            }
        }
    }
    pub(in crate::game) fn player_ai_facts_applied(&self, before: Option<PlayerAiFacts>) {
        self.publish_player_ai_delta(self.player_ai_facts_delta(&before));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::game) struct PlayerBackingKey {
    pub(super) uuid: [i32; 4],
    pub(super) conn: Option<u32>,
}

/// Indexes refer only to owned AI projection slots; canonical guest reads use
/// Hub::guest(conn), never these positions or retained source element offsets.
#[derive(Default)]
pub(super) struct PlayerBackingIndex {
    levels: std::collections::HashMap<u128, usize>,
    guests: std::collections::HashMap<u32, usize>,
    guest_ids: std::collections::HashMap<u32, usize>,
    pending: std::collections::HashMap<[i32; 4], Option<u32>>,
}
pub(super) struct PlayerBackingLoans<'a> {
    pub(super) local: &'a mut Option<horizon_client::mobs::PlayerView>,
    pub(super) players: &'a mut Vec<super::natural_spawning::LevelPlayer>,
    pub(super) nearest: &'a mut super::natural_spawning::NearestPlayers,
    pub(super) guests: &'a mut super::lan_mobs::GuestViews,
    pub(super) spare: &'a mut Option<horizon_client::mobs::PlayerView>,
    pub(super) guest_ids: &'a mut Vec<(u32, u32)>,
}
impl PlayerBackingIndex {
    pub(super) fn new(loans: &PlayerBackingLoans<'_>) -> Self {
        Self {
            levels: loans
                .players
                .iter()
                .enumerate()
                .map(|(slot, player)| (player.uuid, slot))
                .collect(),
            guests: loans
                .guests
                .iter()
                .enumerate()
                .map(|(slot, (conn, _))| (*conn, slot))
                .collect(),
            guest_ids: loans
                .guest_ids
                .iter()
                .enumerate()
                .map(|(slot, (conn, _))| (*conn, slot))
                .collect(),
            ..Default::default()
        }
    }
}
impl Game<'_> {
    /// Refresh changed bodies only. Ordinary writes update a single owned
    /// player and a single nearest-cell entry, independent of mob/player count.
    /// Membership removal/insertion alone rebuilds the affected owned ordering.
    pub(super) fn refresh_mob_player_deltas(
        &mut self,
        teams: &horizon_client::mobs::TeamRulesSnapshot,
        index: &mut PlayerBackingIndex,
        loans: PlayerBackingLoans<'_>,
    ) -> usize {
        {
            let mut state = self.entity_read_state.borrow_mut();
            if state.mob_player_writes.is_empty() {
                return 0;
            }
            std::mem::swap(&mut index.pending, &mut state.mob_player_writes);
        }
        let mut membership = false;
        let mut guest_order = false;
        let mut refreshed = 0;
        for (uuid, conn) in index.pending.drain() {
            let uuid128 = horizon_client::lan::playerdata::ints_uuid(uuid);
            let level = if let Some(conn) = conn {
                let view = self.ai_guest_view(conn, teams);
                match (index.guests.get(&conn).copied(), view) {
                    (Some(slot), Some(view)) => {
                        debug_assert_eq!(loans.guests[slot].0, conn);
                        loans.guests[slot].1 = view;
                    }
                    (Some(slot), None) => {
                        loans.guests.remove(slot);
                        guest_order = true;
                    }
                    (None, Some(view)) => {
                        loans.guests.push((conn, view));
                        guest_order = true;
                    }
                    (None, None) => {}
                }
                // Removal changes only rare membership ordering; normal per-body
                // writes never rebuild this index or another player's view.
                if guest_order {
                    index.guests = loans
                        .guests
                        .iter()
                        .enumerate()
                        .map(|(slot, (conn, _))| (*conn, slot))
                        .collect();
                    guest_order = false;
                }
                let guest = self.lan.as_ref().and_then(|host| host.hub.guest(conn));
                if let Some(guest) = guest {
                    if let Some(&slot) = index.guest_ids.get(&conn) {
                        loans.guest_ids[slot].1 = guest.entity_id as u32;
                    } else {
                        index.guest_ids.insert(conn, loans.guest_ids.len());
                        loans.guest_ids.push((conn, guest.entity_id as u32));
                    }
                }
                guest
                    .filter(|g| g.uuid == uuid128 && g.placed_in(self.dim.id()))
                    .map(|g| super::natural_spawning::LevelPlayer {
                        uuid: g.uuid,
                        position: horizon_core::geometry::Vec3::new(
                            g.position[0],
                            g.position[1],
                            g.position[2],
                        ),
                        bounds: super::lan_mobs::guest_bounds(g),
                        alive: !g.dead,
                        creative: g.creative(),
                        spectator: g.spectator(),
                    })
            } else if self.acting_uuid() == uuid {
                *loans.local = self.ai_player_view(teams);
                self.local_level_player()
            } else {
                None
            };
            match (index.levels.get(&uuid128).copied(), level) {
                (Some(slot), Some(level)) => {
                    debug_assert_eq!(loans.players[slot].uuid, uuid128);
                    loans.players[slot] = level;
                    loans.nearest.update(loans.players, slot);
                }
                (None, None) => {}
                _ => membership = true,
            }
            refreshed += 1;
        }
        if membership {
            *loans.players = self.level_players();
            *loans.nearest = super::natural_spawning::NearestPlayers::new(loans.players);
            index.levels = loans
                .players
                .iter()
                .enumerate()
                .map(|(slot, p)| (p.uuid, slot))
                .collect();
        }
        *loans.spare = loans.guests.first().map(|g| g.1.clone());
        refreshed
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

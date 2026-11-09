//! Applied AI backing changes between mutable mob hooks.
//!
//! Canonical Player turns compare represented facts after apply. Core scoreboard
//! mutations emit changed-only rule/member deltas; tame/identity owners enqueue
//! bounded UUID edges. Shared ally/rule backing follows those deltas without a
//! population rebuild. Unrelated writes (chat, sounds, Silent) mark neither.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MobOwnerFact {
    uuid: [i32; 4],
    owner: Option<[i32; 4]>,
}
impl Game<'_> {
    pub(super) fn mark_mob_player_backing_dirty(&self, key: PlayerBackingKey) {
        if !self.score.board.is_ai_tracking() {
            return;
        }
        let mut state = self.entity_read_state.borrow_mut();
        let Some(level) = state.mob_ai_level else {
            return;
        };
        let was_here = state
            .mob_ai_teams
            .read()
            .expect("AI team backing poisoned")
            .players
            .contains(&key.uuid);
        let here = match key.conn {
            Some(conn) if self.lan_acting == Some(conn) => {
                self.dim.key() == level && self.player.is_some() && !self.lan_acting_fake
            }
            Some(conn) => self
                .lan
                .as_ref()
                .and_then(|host| host.hub.guest(conn))
                .is_some_and(|guest| {
                    guest.participates_online()
                        && guest.placed
                        && super::Dim::from_id(&guest.dimension).key() == level
                }),
            None => self.dim.key() == level && self.player.is_some() && !self.lan_acting_fake,
        };
        if !was_here && !here {
            return;
        }
        state.mob_player_writes.insert(key.uuid, key.conn);
        state.mob_backing.players_changed();
    }
}

impl Game<'_> {
    pub(super) fn ai_player_view(
        &self,
        teams: &horizon_client::mobs::TeamRulesSnapshot,
        mobs: &horizon_client::join_seq::JoinList<horizon_client::mobs::Mob>,
    ) -> Option<horizon_client::mobs::PlayerView> {
        let Some((facts, _)) = &teams.shared else {
            return self.player_view();
        };
        let mut view = self.player_view_with_allies(std::sync::Arc::new(
            horizon_client::mobs::PlayerAllies::shared(facts.clone(), self.acting_uuid()),
        ))?;
        view.vehicle = view
            .riding_vehicle
            .and_then(|id| self.player_vehicle_view(id, mobs));
        Some(view)
    }
    /// Resolve a vehicle through its exact owning join, including the actual
    /// mob loan. This never scans mobs/boats or retains a source element offset.
    pub(super) fn player_vehicle_view(
        &self,
        id: u32,
        mobs: &horizon_client::join_seq::JoinList<horizon_client::mobs::Mob>,
    ) -> Option<horizon_client::mobs::Vehicle> {
        use horizon_client::join_seq::residency::{
            Kind,
            table::{EntityLookup, SourceEntity},
        };
        let result = self
            .entity_residency
            .borrow_mut()
            .server_entity_lookup(id, |at| {
                if at.owner.dimension == self.dim.key() {
                    match at.owner.kind {
                        Kind::Boat => {
                            let boat = &self.boats[self.boats.locate(at.join)?];
                            let p = boat.position;
                            return Some(SourceEntity {
                                id: boat.id,
                                value: Some(horizon_client::mobs::Vehicle {
                                    nautilus: false,
                                    bounds: horizon_core::geometry::Aabb::new(
                                        horizon_core::geometry::Vec3::new(
                                            p.x - 0.6875,
                                            p.y,
                                            p.z - 0.6875,
                                        ),
                                        horizon_core::geometry::Vec3::new(
                                            p.x + 0.6875,
                                            p.y + 0.5625,
                                            p.z + 0.6875,
                                        ),
                                    ),
                                    velocity: boat.velocity,
                                }),
                            });
                        }
                        Kind::Mob => {
                            let mob = &mobs[mobs.locate(at.join)?];
                            let value = (!mob.removed
                                && matches!(
                                    mob.species(),
                                    horizon_client::mobs::Species::Nautilus
                                        | horizon_client::mobs::Species::ZombieNautilus
                                ))
                            .then(|| {
                                horizon_client::mobs::Vehicle {
                                    nautilus: true,
                                    bounds: mob.bounds(),
                                    velocity: mob.velocity,
                                }
                            });
                            return Some(SourceEntity { id: mob.id, value });
                        }
                        _ => {}
                    }
                }
                self.locate_server_resident_owner(at)
                    .map(|source| SourceEntity {
                        id: source.id,
                        value: None,
                    })
            });
        match result {
            EntityLookup::Found { value, .. } => value,
            _ => None,
        }
    }
    pub(super) fn mob_owner_fact(
        &self,
        source: &horizon_client::mobs::Mob,
    ) -> Option<MobOwnerFact> {
        if source.removed {
            return None;
        }
        Some(MobOwnerFact {
            uuid: *self.persist.uuids.get(&source.id)?,
            owner: source.extra.tame.then_some(source.extra.owner).flatten(),
        })
    }
    pub(super) fn mob_owner_fact_at(&self, id: u32) -> Option<MobOwnerFact> {
        use horizon_client::join_seq::residency::{Kind, table::EntityLookup};
        let EntityLookup::Found { location, .. } = self.server_resident_entity(id) else {
            return None;
        };
        if location.owner.dimension != self.dim.key() || location.owner.kind != Kind::Mob {
            return None;
        }
        self.mob_owner_fact(&self.mobs[self.mobs.locate(location.join)?])
    }
    /// Compare a canonical source mutation with its pending/current bounded
    /// relation fact. A rejected/no-op tame operation cannot enqueue a refresh.
    pub(super) fn sync_mob_owner_backing(&self, source: &horizon_client::mobs::Mob) {
        if !self.score.board.is_ai_tracking() {
            return;
        }
        self.sync_mob_owner_fact(source.id, self.mob_owner_fact(source));
    }
    pub(super) fn sync_mob_owner_fact(&self, id: u32, after: Option<MobOwnerFact>) {
        self.sync_mob_owner_fact_in(self.dim.key(), id, after);
    }
    fn sync_mob_owner_fact_in(
        &self,
        dimension: mod_api::DimensionKey,
        id: u32,
        after: Option<MobOwnerFact>,
    ) {
        if !self.score.board.is_ai_tracking() {
            return;
        }
        let state = self.entity_read_state.borrow();
        if state.mob_ai_level != Some(dimension) {
            return;
        }
        let before = state.mob_owner_writes.get(&id).copied().unwrap_or_else(|| {
            let facts = state.mob_ai_teams.read().expect("AI team backing poisoned");
            facts.identities.get(&id).map(|uuid| MobOwnerFact {
                uuid: *uuid,
                owner: facts.owners.get(uuid).copied(),
            })
        });
        drop(state);
        if before != after {
            self.mark_mob_owner_backing_changed(id, after);
        }
    }
    pub(super) fn mark_mob_saved_identity_changed(
        &self,
        dimension: mod_api::DimensionKey,
        id: u32,
        old: Option<[i32; 4]>,
        new: Option<[i32; 4]>,
    ) {
        if self.entity_read_state.borrow().mob_ai_level != Some(dimension)
            || old == new
            || !self.score.board.is_ai_tracking()
        {
            return;
        }
        let owner = self
            .mob_owner_fact_at(id)
            .map(|fact| fact.owner)
            .unwrap_or_else(|| {
                let state = self.entity_read_state.borrow();
                if let Some(pending) = state.mob_owner_writes.get(&id) {
                    return pending.and_then(|fact| fact.owner);
                }
                let facts = state.mob_ai_teams.read().expect("AI team backing poisoned");
                facts
                    .identities
                    .get(&id)
                    .and_then(|uuid| facts.owners.get(uuid))
                    .copied()
            });
        self.sync_mob_owner_fact_in(dimension, id, new.map(|uuid| MobOwnerFact { uuid, owner }));
    }
    /// A canonical tame/root-owner write emits identities only, after apply.
    pub(super) fn mark_mob_owner_backing_changed(&self, id: u32, owner: Option<MobOwnerFact>) {
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
                if let Some(MobOwnerFact { uuid, owner }) = owner {
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
    pub(super) mobs: &'a horizon_client::join_seq::JoinList<horizon_client::mobs::Mob>,
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
    fn sync_ai_player_team_binding(
        &self,
        key: PlayerBackingKey,
        present: bool,
        teams: &horizon_client::mobs::TeamRulesSnapshot,
    ) {
        let Some((shared, _)) = &teams.shared else {
            return;
        };
        let name = present
            .then(|| match key.conn {
                Some(conn) => self
                    .lan
                    .as_ref()?
                    .hub
                    .guest(conn)
                    .map(|guest| guest.name.as_str()),
                None => Some(self.acting_player_name()),
            })
            .flatten();
        let mut facts = shared.write().expect("AI team backing poisoned");
        if name == facts.player_names.get(&key.uuid).map(|name| name.as_ref()) {
            return;
        }
        if let Some(old) = facts.player_names.remove(&key.uuid) {
            facts.names.remove(&old);
        }
        facts.players.remove(&key.uuid);
        let team = if let Some(name) = name {
            let name: std::sync::Arc<str> = name.into();
            facts.names.insert(name.clone(), key.uuid);
            facts.player_names.insert(key.uuid, name.clone());
            facts.players.insert(key.uuid);
            self.score
                .board
                .players_team(&name)
                .map(|team| team.ai_identity())
        } else {
            self.score
                .board
                .players_team(&horizon_client::save::uuid_text(key.uuid))
                .map(|team| team.ai_identity())
        };
        if let Some(team) = team {
            facts.members.insert(key.uuid, team);
        } else {
            facts.members.remove(&key.uuid);
        }
    }
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
                let view = self.ai_guest_view(conn, teams, loans.mobs);
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
                *loans.local = self.ai_player_view(teams, loans.mobs);
                self.local_level_player()
            } else {
                None
            };
            self.sync_ai_player_team_binding(PlayerBackingKey { uuid, conn }, level.is_some(), teams);
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

impl Game<'_> {
    /// A direct inactive-body mutator already owns its resolved target. Record
    /// that identity only after its represented scalar actually changed.
    pub(super) fn mark_player_target_backing_changed(&self, target: super::commands::Target) {
        use super::commands::Target;
        let key = match target {
            Target::Player => {
                if self.lan_acting_fake {
                    return;
                }
                PlayerBackingKey {
                    uuid: self.acting_uuid(),
                    conn: self.lan_acting,
                }
            }
            Target::Guest(conn) => {
                let Some(guest) = self.lan.as_ref().and_then(|host| host.hub.guest(conn)) else {
                    return;
                };
                if guest.is_fake() {
                    return;
                }
                PlayerBackingKey {
                    uuid: horizon_client::lan::playerdata::uuid_ints(guest.uuid),
                    conn: Some(conn),
                }
            }
            _ => return,
        };
        self.mark_mob_player_backing_dirty(key);
    }
}

#[cfg(test)]
#[path = "native_mob_backing/tests.rs"]
mod tests;

//! Per-pass relationship indexes and direct-source context construction.
//! Rebuild once per pass into retained capacity. Canonical relation writes update
//! affected keys; callbacks do not trigger population rescans. No retained loans.
use super::{
    Dim, Game,
    commands::Target,
    native_mob_read_rows::{MerchantReadContext, MobReadContext},
};
use horizon_client::{
    join_seq::residency::{
        IdentityDetails, Source,
        table::{EntityLookup, Location},
    },
    mobs::{self, Mob, Species, Who, ai::brain_target},
};
use horizon_core::geometry::{Aabb, Axis};
use mod_api::{EntityRef, LevelRef, LivingRef, WorldEpoch};
use std::collections::HashMap;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct BackingWork {
    pub(super) population_visits: usize,
    pub(super) source_lookups: usize,
    pub(super) owner_changes: usize,
    pub(super) player_projections: usize,
}
#[cfg(test)]
thread_local! {
    static BACKING_WORK: std::cell::Cell<BackingWork> = Default::default();
}
#[cfg(test)]
pub(super) fn record_backing_work(update: impl FnOnce(&mut BackingWork)) {
    BACKING_WORK.with(|work| {
        let mut value = work.get();
        update(&mut value);
        work.set(value);
    });
}
#[cfg(test)]
pub(super) fn backing_work() -> BackingWork {
    BACKING_WORK.with(std::cell::Cell::get)
}

/// The minimal extra mob facts retained on the authoritative publication.
#[derive(Clone, Copy)]
pub(super) struct NativeMobTargetFacts {
    pub id: u32,
    species: Species,
    alive: bool,
    invulnerable: bool,
    baby: bool,
    bounds: Aabb,
    owner: Option<[i32; 4]>,
    mount: Option<u32>,
    vehicle_order: i64,
    join: horizon_client::join_seq::JoinSeq,
}
impl NativeMobTargetFacts {
    pub(super) fn capture(source: &Mob) -> Self {
        Self {
            id: source.id,
            species: source.species(),
            alive: source.alive(),
            invulnerable: source.invulnerable_flag,
            baby: source.is_baby(),
            bounds: source.bounds(),
            owner: source.extra.tame.then_some(source.extra.owner).flatten(),
            mount: source.extra.mount,
            vehicle_order: source.extra.vehicle_order,
            join: horizon_client::join_seq::Joined::join_seq(source),
        }
    }
}
#[derive(Default)]
pub(super) struct NativeMobRelations {
    guest_riders: HashMap<u32, std::collections::HashSet<u32>>,
    guest_vehicles: HashMap<u32, u32>,
    merchant_by_conn: HashMap<u32, u32>,
    merchants: HashMap<u32, u32>,
    saved_ids: HashMap<[i32; 4], u32>,
    /// Minimal ownership edges, like rider_vehicles. AI still needs these when
    /// the real roster is lent and no mod requested an R publication.
    tame_owners: HashMap<u32, [i32; 4]>,
    identity_candidates: Vec<(u32, Location, IdentityDetails)>,
    mob_riders: HashMap<u32, std::collections::HashSet<u32>>,
    rider_vehicles: HashMap<u32, u32>,
    mob_riders_complete: bool,
}
impl NativeMobRelations {
    pub(super) fn clear(&mut self) {
        for riders in self.guest_riders.values_mut() {
            riders.clear();
        }
        self.guest_vehicles.clear();
        self.merchants.clear();
        self.merchant_by_conn.clear();
        self.saved_ids.clear();
        self.tame_owners.clear();
        self.identity_candidates.clear();
        for riders in self.mob_riders.values_mut() {
            riders.clear();
        }
        self.rider_vehicles.clear();
        self.mob_riders_complete = false;
    }
    /// None means the real roster was unavailable before any complete seed.
    /// Empty is meaningful only after a complete actual-source traversal.
    pub(super) fn mob_rider_candidates(
        &self,
        vehicle: u32,
    ) -> Option<impl Iterator<Item = u32> + '_> {
        self.mob_riders_complete.then(|| {
            self.mob_riders
                .get(&vehicle)
                .into_iter()
                .flat_map(|riders| riders.iter().copied())
        })
    }
    pub(super) fn guest_riders_of(&self, vehicle: u32) -> impl Iterator<Item = u32> + '_ {
        self.guest_riders
            .get(&vehicle)
            .into_iter()
            .flat_map(|riders| riders.iter().copied())
    }
    /// Call after canonical guest vehicle commits. Identity belongs to GuestList.
    pub(super) fn update_guest(&mut self, guest: &horizon_client::lan::guest::Guest) {
        self.update_guest_vehicle(guest.conn, guest.vehicle);
    }
    pub(super) fn update_guest_vehicle(&mut self, conn: u32, vehicle: Option<u32>) {
        if self.guest_vehicles.get(&conn).copied() == vehicle {
            return;
        }
        self.remove_guest(conn);
        if let Some(id) = vehicle {
            self.guest_vehicles.insert(conn, id);
            self.guest_riders.entry(id).or_default().insert(conn);
        }
    }
    /// Call before guest removal. Hub owns positional indexing.
    pub(super) fn remove_guest(&mut self, conn: u32) {
        if let Some(id) = self.guest_vehicles.remove(&conn)
            && let Some(count) = self.guest_riders.get_mut(&id)
        {
            count.remove(&conn);
        }
    }
    /// Call at merchant session open/close/rebinding, including disconnect.
    pub(super) fn update_merchant(&mut self, conn: u32, mob: Option<u32>) {
        if let Some(old) = self.merchant_by_conn.remove(&conn)
            && self.merchants.get(&old) == Some(&conn)
        {
            self.merchants.remove(&old);
        }
        if let Some(id) = mob {
            self.merchant_by_conn.insert(conn, id);
            self.merchants.insert(id, conn);
        }
    }
    pub(super) fn update_mob_owner(&mut self, source: &Mob) {
        let owner = (!source.removed && source.extra.tame)
            .then_some(source.extra.owner)
            .flatten();
        self.update_owner(source.id, owner);
    }

    fn update_owner(&mut self, id: u32, owner: Option<[i32; 4]>) {
        if self.tame_owners.get(&id).copied() == owner {
            return;
        }
        #[cfg(test)]
        record_backing_work(|work| work.owner_changes += 1);
        if let Some(owner) = owner {
            self.tame_owners.insert(id, owner);
        } else {
            self.tame_owners.remove(&id);
        }
    }

    /// Call in source publication order and at canonical mount/remove boundaries.
    /// The relation index contains identities only; current facts stay in the row.
    pub(super) fn update_mob_rider(&mut self, source: &Mob) {
        self.update_mob_owner(source);
        self.update_rider(
            source.id,
            (!source.removed).then_some(source.extra.mount).flatten(),
        );
    }
    fn update_rider(&mut self, id: u32, vehicle: Option<u32>) {
        if self.rider_vehicles.get(&id).copied() == vehicle {
            return;
        }
        self.remove_mob_rider(id);
        if let Some(vehicle) = vehicle {
            self.rider_vehicles.insert(id, vehicle);
            self.mob_riders.entry(vehicle).or_default().insert(id);
        }
    }
    pub(super) fn remove_mob_rider(&mut self, id: u32) {
        if let Some(vehicle) = self.rider_vehicles.remove(&id)
            && let Some(riders) = self.mob_riders.get_mut(&vehicle)
        {
            riders.remove(&id);
        }
    }
    /// Canonical persisted UUID assignment/removal/travel must update both levels.
    pub(super) fn update_saved_uuid(
        &mut self,
        id: u32,
        old: Option<[i32; 4]>,
        new: Option<[i32; 4]>,
    ) {
        if let Some(old) = old
            && self.saved_ids.get(&old) == Some(&id)
        {
            self.saved_ids.remove(&old);
        }
        if let Some(new) = new {
            self.saved_ids.insert(new, id);
        }
    }
}
struct PlayerTargetSource<'a> {
    facts: super::native_mob_refs::PlayerFacts,
    warden_invulnerable: bool,
    bounds: Aabb,
    team: Option<&'a horizon_client::scoreboard::Team>,
}
impl mobs::TeamLookup for Game<'_> {
    fn team(
        &self,
        actor: &Mob,
        who: Who,
        player: Option<[i32; 4]>,
    ) -> Option<&horizon_client::scoreboard::Team> {
        let level = LevelRef::new(self.native_lifecycle.epoch()?, self.dim.key());
        self.with_prepared_mob_relations(|index| match who {
            Who::Player => {
                let name = self.indexed_online_owner_name(player?, index)?;
                self.score.board.players_team(name)
            }
            Who::Mob(id) => {
                let owner = if id == actor.id {
                    if actor.removed {
                        return None;
                    }
                    actor.extra.tame.then_some(actor.extra.owner).flatten()
                } else {
                    index.tame_owners.get(&id).copied()
                };
                self.read_mob_team_owner(id, owner, level, index)
            }
        })?
    }
}

impl Game<'_> {
    /// Resolve a saved UUID only after canonical admission. A loose saved-map
    /// key, removed body, foreign level or dragon-part alias is not an identity.
    fn admitted_saved_uuid_at(
        &self,
        id: u32,
        dimension: mod_api::DimensionKey,
    ) -> Option<(Location, IdentityDetails, [i32; 4])> {
        #[cfg(test)]
        record_backing_work(|work| work.source_lookups += 1);
        let (at, state, details) = {
            let mut table = self.entity_residency.borrow_mut();
            let (at, state, _, details) = table
                .server_admitted_read_lookup(id, |at| self.locate_server_resident_owner(at))?;
            (at, state, details)
        };
        if at.owner.dimension != dimension
            || !matches!(at.source, Source::Ordinary | Source::Hook { .. })
            || !matches!(
                state,
                EntityLookup::Taken
                    | EntityLookup::Found {
                        value: (_, true),
                        ..
                    }
            )
        {
            return None;
        }
        let uuid = if dimension == self.dim.key() {
            self.persist.uuids.get(&id).copied()
        } else {
            self.levels
                .parked_of(LevelRef::new(self.native_lifecycle.epoch()?, dimension).dimension_id())?
                .saved_entity_uuid(id)
        }?;
        Some((at, details, uuid))
    }

    /// Seed only ownership edges from the actual pass roster. This also works
    /// without mods/read publications; no foreign H payload is materialized.
    pub(super) fn prepare_ai_mob_relations(&self, mobs: &[Mob]) {
        if self.with_prepared_mob_relations(|index| index.mob_riders_complete) != Some(false) {
            return;
        }
        let level = LevelRef::new(self.native_lifecycle.epoch().unwrap(), self.dim.key());
        let mut state = self.entity_read_state.borrow_mut();
        let pass = state.mob_relations.get_mut(&level).unwrap();
        #[cfg(test)]
        record_backing_work(|work| work.population_visits += mobs.len());
        for source in mobs {
            pass.index.update_mob_rider(source);
        }
        pass.index.mob_riders_complete = true;
    }

    pub(super) fn record_ai_mob_owner(&self, source: &Mob) {
        let Some(epoch) = self.native_lifecycle.epoch() else {
            return;
        };
        let level = LevelRef::new(epoch, self.dim.key());
        if let Some(pass) = self
            .entity_read_state
            .borrow_mut()
            .mob_relations
            .get_mut(&level)
        {
            pass.index.update_mob_rider(source);
        }
    }

    pub(super) fn record_ai_mob_owner_id(&self, id: u32) {
        let Some(epoch) = self.native_lifecycle.epoch() else {
            return;
        };
        let level = LevelRef::new(epoch, self.dim.key());
        let facts = self.read_mob_facts(id, level);
        if let Some(pass) = self
            .entity_read_state
            .borrow_mut()
            .mob_relations
            .get_mut(&level)
        {
            pass.index
                .update_owner(id, facts.and_then(|facts| facts.owner));
            pass.index
                .update_rider(id, facts.and_then(|facts| facts.mount));
        }
    }

    /// Canonical GuestList nominations plus only the explicit selected-body
    /// loan edges. Neither branch supplies identity proof or H data.
    pub(super) fn mob_guest_rider_connections(&self, vehicle: u32) -> Option<Vec<u32>> {
        let mut candidates = self
            .lan
            .as_ref()
            .into_iter()
            .flat_map(|host| host.hub.guests.rider_connections(vehicle))
            .filter(|conn| !self.lan_guest_swaps.contains(conn))
            .collect::<Vec<_>>();
        if !self.lan_guest_swaps.is_empty() {
            let level = LevelRef::new(self.native_lifecycle.epoch()?, self.dim.key());
            self.with_mob_read_relations(level, |index| {
                candidates.extend(
                    index
                        .guest_riders_of(vehicle)
                        .filter(|conn| self.lan_guest_swaps.contains(conn)),
                );
            })?;
        }
        Some(candidates)
    }

    pub(super) fn current_admitted_saved_uuid(&self, id: u32) -> Option<[i32; 4]> {
        self.admitted_saved_uuid_at(id, self.dim.key())
            .map(|(_, _, uuid)| uuid)
    }

    pub(super) fn current_mob_owner(&self, id: u32) -> Option<[i32; 4]> {
        let level = LevelRef::new(self.native_lifecycle.epoch()?, self.dim.key());
        self.with_prepared_mob_relations(|index| {
            self.read_saved_uuid(id, level)?;
            index.tame_owners.get(&id).copied()
        })?
    }

    pub(super) fn admitted_profile_uuid(&self, id: u32, source: Source, uuid: [i32; 4]) -> bool {
        #[cfg(test)]
        record_backing_work(|work| work.source_lookups += 1);
        let mut table = self.entity_residency.borrow_mut();
        table
            .server_admitted_read_lookup(id, |at| self.locate_server_resident_owner(at))
            .is_some_and(|(at, state, _, details)| {
                at.source == source
                    && details.known_uuid == Some(uuid)
                    && matches!(
                        state,
                        EntityLookup::Taken
                            | EntityLookup::Found {
                                value: (_, true),
                                ..
                            }
                    )
            })
    }

    /// Reuse the once-per-pass relation index, then validate its candidate at
    /// the primary table. Small writes update affected keys, not this population.
    /// Candidate metadata only: H never rebuilds or scans populations here.
    /// None explicitly means this owning level has no complete prepared index.
    pub(super) fn relation_identity_candidates_at(
        &self,
        level: LevelRef,
        uuid: [i32; 4],
    ) -> Option<(Option<u32>, Option<u32>)> {
        self.with_mob_read_relations(level, |index| {
            let saved = index.saved_ids.get(&uuid).copied();
            let profile = index
                .players_by_uuid
                .get(&horizon_client::lan::playerdata::ints_uuid(uuid))
                .and_then(|conn| index.guest_identities.get(conn))
                .map(|&(_, id)| id);
            (saved, profile)
        })
    }

    pub(super) fn current_admitted_saved_id(&self, uuid: [i32; 4]) -> Option<u32> {
        let level = LevelRef::new(self.native_lifecycle.epoch()?, self.dim.key());
        self.with_prepared_mob_relations(|index| {
            let id = *index.saved_ids.get(&uuid)?;
            (self.read_saved_uuid(id, level)? == uuid).then_some(id)
        })?
    }

    /// The primary table enumerates candidates only. Resolve each after dropping
    /// its iterator borrow, then require the same immutable admission. The reused
    /// scratch is returned empty; no second identity/facts authority is retained.
    pub(super) fn for_admitted_saved_uuids(
        &self,
        dimension: mod_api::DimensionKey,
        candidates: &mut Vec<(u32, Location, IdentityDetails)>,
        mut visit: impl FnMut(u32, [i32; 4]),
    ) {
        candidates.clear();
        candidates.extend(self.entity_residency.borrow_mut().admitted_metadata());
        #[cfg(test)]
        record_backing_work(|work| work.population_visits += candidates.len());
        for &(id, expected, expected_details) in candidates.iter() {
            if expected.owner.dimension != dimension {
                continue;
            }
            if let Some((at, details, uuid)) = self.admitted_saved_uuid_at(id, dimension)
                && at == expected
                && details == expected_details
            {
                visit(id, uuid);
            }
        }
        candidates.clear();
    }

    /// Rebuild once per pass, preserving allocation capacity. This is not a
    /// callback refresh: canonical changes use NativeMobRelations update methods.
    pub(super) fn rebuild_mob_read_relations(
        &self,
        level: LevelRef,
        index: &mut NativeMobRelations,
    ) {
        index.clear();
        let owner = horizon_client::join_seq::residency::Owner {
            dimension: level.dimension(),
            kind: horizon_client::join_seq::residency::Kind::Mob,
        };
        if self
            .entity_residency
            .borrow_mut()
            .owner_physically_available(owner, None)
        {
            let roster = if level.dimension() == self.dim.key() {
                Some(&self.mobs)
            } else {
                self.levels
                    .parked_of(level.dimension_id())
                    .map(|parked| &parked.mobs)
            };
            if let Some(roster) = roster {
                #[cfg(test)]
                record_backing_work(|work| work.population_visits += roster.len());
                for source in roster {
                    index.update_mob_rider(source);
                }
                index.mob_riders_complete = true;
            }
        }
        if let Some(host) = &self.lan {
            for guest in host.hub.guests.iter() {
                index.update_guest(guest);
            }
            for (&conn, play) in &host.play {
                index.update_merchant(conn, play.merchant.as_ref().and_then(|m| m.merchant.mob));
            }
        }
        // Saved keys are only candidates, never admission proof. Preserve keys
        // whose real source arrives later in this same pass; each lookup still
        // resolves the exact primary member and rereads its canonical UUID.
        let mut seed = |id, uuid| {
            #[cfg(test)]
            record_backing_work(|work| work.population_visits += 1);
            index.saved_ids.insert(uuid, id);
        };
        if level.dimension() == self.dim.key() {
            for (&id, &uuid) in &self.persist.uuids {
                seed(id, uuid);
            }
        } else if let Some(parked) = self.levels.parked_of(level.dimension_id()) {
            for (id, uuid) in parked.saved_entity_uuids() {
                seed(id, uuid);
            }
        }
        self.for_admitted_saved_uuids(
            level.dimension(),
            &mut index.identity_candidates,
            |id, uuid| {
                index.saved_ids.insert(uuid, id);
                if let Some(owner) = self.read_mob_facts(id, level).and_then(|facts| facts.owner) {
                    index.tame_owners.insert(id, owner);
                }
            },
        );
    }
    fn read_saved_uuid(&self, id: u32, level: LevelRef) -> Option<[i32; 4]> {
        self.admitted_saved_uuid_at(id, level.dimension())
            .map(|(_, _, uuid)| uuid)
    }
    fn read_guest<'a>(
        &'a self,
        _index: &NativeMobRelations,
        conn: u32,
    ) -> Option<&'a horizon_client::lan::guest::Guest> {
        let guest = self.lan.as_ref()?.hub.guest(conn)?;
        let id = u32::try_from(guest.entity_id).ok()?;
        (guest.participates_online()
            && self.admitted_profile_uuid(
                id,
                Source::Guest(conn),
                horizon_client::lan::playerdata::uuid_ints(guest.uuid),
            ))
        .then_some(guest)
    }
    fn read_mob_facts(&self, id: u32, level: LevelRef) -> Option<NativeMobTargetFacts> {
        let mut table = self.entity_residency.borrow_mut();
        let (state, cached) =
            table.server_entity_read_lookup(id, |at| self.locate_server_resident_owner(at));
        match state {
            EntityLookup::Taken => {
                let row = cached?;
                (row.base.level == level
                    && self.server_read_target_visible(level.dimension(), row.target))
                .then_some(row.mob_target_facts)
                .flatten()
            }
            EntityLookup::Found {
                location,
                value: (Target::Mob(_), true),
            } if location.owner.dimension == level.dimension() => {
                let list = if location.owner.dimension == self.dim.key() {
                    &self.mobs
                } else {
                    &self.levels.parked_of(level.dimension_id())?.mobs
                };
                let source = &list[list.locate(location.join)?];
                (source.id == id && !source.removed).then(|| NativeMobTargetFacts::capture(source))
            }
            _ => None,
        }
    }
    pub(super) fn indexed_online_owner_name<'a>(
        &'a self,
        uuid: [i32; 4],
        _index: &NativeMobRelations,
    ) -> Option<&'a str> {
        if self.player.is_some() && !self.lan_acting_fake && self.acting_uuid() == uuid {
            let id = u32::try_from(self.own_entity_id()).ok()?;
            let source = self.lan_acting.map_or(Source::LocalPlayer, Source::Guest);
            if self.admitted_profile_uuid(id, source, uuid) {
                return Some(self.acting_player_name());
            }
        }
        let guest = self
            .lan
            .as_ref()?
            .hub
            .guests
            .online_profile(horizon_client::lan::playerdata::ints_uuid(uuid))?;
        let id = u32::try_from(guest.entity_id).ok()?;
        self.admitted_profile_uuid(id, Source::Guest(guest.conn), uuid)
            .then_some(guest.name.as_str())
    }

    fn read_mob_team<'a>(
        &'a self,
        facts: NativeMobTargetFacts,
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> Option<&'a horizon_client::scoreboard::Team> {
        self.read_mob_team_owner(facts.id, facts.owner, level, index)
    }

    fn read_mob_team_owner<'a>(
        &'a self,
        id: u32,
        owner: Option<[i32; 4]>,
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> Option<&'a horizon_client::scoreboard::Team> {
        let uuid = self.read_saved_uuid(id, level)?;
        let mut text = [0; 36];
        let own = horizon_client_core::uuid::uuid_text_into(uuid, &mut text);
        if let Some(team) = self.score.board.players_team(own) {
            return Some(team);
        }
        let next_owner = |uuid| {
            if self.indexed_online_owner_name(uuid, index).is_some() {
                return None;
            }
            let id = *index.saved_ids.get(&uuid)?;
            (self.read_saved_uuid(id, level)? == uuid).then_some(())?;
            index.tame_owners.get(&id).copied()
        };
        let mut root = owner?;
        let mut fast = Some(root);
        loop {
            if let Some(name) = self.indexed_online_owner_name(root, index) {
                return self.score.board.players_team(name);
            }
            let id = *index.saved_ids.get(&root)?;
            (self.read_saved_uuid(id, level)? == root).then_some(())?;
            let Some(&next) = index.tame_owners.get(&id) else {
                return self
                    .score
                    .board
                    .players_team(horizon_client_core::uuid::uuid_text_into(root, &mut text));
            };
            root = next;
            // Floyd's guard needs no allocation or arbitrary chain-length limit.
            fast = fast.and_then(next_owner).and_then(next_owner);
            if fast == Some(root) {
                return None;
            }
        }
    }
    pub(super) fn mob_read_context(
        &self,
        source: &Mob,
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> MobReadContext {
        let target = brain_target(source)
            .unwrap_or(source.ai.target)
            .and_then(|who| self.read_mob_target(source, who, level, index));
        MobReadContext {
            target,
            local_player_riding: level.dimension() == self.dim.key()
                && self.riding == Some(source.id),
            guest_riding: index
                .guest_riders
                .get(&source.id)
                .is_some_and(|riders| !riders.is_empty()),
            carries_riders: self.options.dedicated.is_some(),
            raid: mobs::raider::state(source)
                .and_then(|s| s.raid_id)
                .and_then(|id| self.level_raid(level, id)),
        }
    }
    fn read_mob_target(
        &self,
        source: &Mob,
        who: Who,
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> Option<LivingRef> {
        let epoch = level.epoch();
        match who {
            Who::Mob(id) => {
                let facts = self.read_mob_facts(id, level)?;
                let valid = self.read_mob_target_valid(source, facts, level, index);
                valid.then(|| LivingRef::new(EntityRef::new(epoch, id)))
            }
            Who::Player => {
                let conn = *self.lan.as_ref()?.mob_players.get(&source.id)?;
                let guest = self.read_guest(index, conn)?;
                if !guest.placed || self.lan_guest_swaps.contains(&conn) {
                    return None;
                }
                let id = u32::try_from(guest.entity_id).ok()?;
                if guest.dimension != level.dimension_id() {
                    return None;
                }
                let valid = (source.species() == Species::Warden || !guest.is_fake())
                    && self.read_player_target_valid(
                        source,
                        self.guest_target_source(guest),
                        level,
                        index,
                    );
                valid.then(|| LivingRef::new(EntityRef::new(epoch, id)))
            }
        }
    }
    fn guest_target_source<'a>(
        &'a self,
        guest: &horizon_client::lan::guest::Guest,
    ) -> PlayerTargetSource<'a> {
        PlayerTargetSource {
            facts: super::native_mob_refs::PlayerFacts {
                mode: guest.game_mode,
                alive: !guest.dead && guest.health > 0.0,
                invulnerable: guest.motion.as_ref().is_some_and(|p| {
                    p.invulnerable
                        || p.health.permanently_invulnerable
                        || p.health.invulnerable_time > 0
                }),
                uuid: horizon_client::lan::playerdata::uuid_ints(guest.uuid),
            },
            warden_invulnerable: guest.motion.as_ref().is_some_and(|p| p.invulnerable),
            bounds: guest.bounds(),
            team: self.score.board.players_team(&guest.name),
        }
    }
    fn read_player_target_valid(
        &self,
        source: &Mob,
        player: PlayerTargetSource<'_>,
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> bool {
        let facts = player.facts;
        if source.species() == Species::Warden {
            !matches!(facts.mode, 1 | 3)
                && facts.alive
                && !player.warden_invulnerable
                && !horizon_client::scoreboard::teams_allied(
                    self.read_mob_team(NativeMobTargetFacts::capture(source), level, index),
                    player.team,
                )
                && self.read_target_in_border(player.bounds, level)
        } else {
            source.valid_target_facts(
                self.difficulty_id(),
                &mobs::goals::TargetFacts::Player {
                    alive: facts.alive,
                    invulnerable: facts.invulnerable,
                    creative: facts.mode == 1,
                    spectator: facts.mode == 3,
                    uuid: facts.uuid,
                },
                None,
            )
        }
    }
    fn read_mob_target_valid(
        &self,
        source: &Mob,
        facts: NativeMobTargetFacts,
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> bool {
        if source.species() == Species::Warden {
            facts.alive
                && !facts.invulnerable
                && facts.species != Species::Warden
                && !horizon_client::scoreboard::teams_allied(
                    self.read_mob_team(NativeMobTargetFacts::capture(source), level, index),
                    self.read_mob_team(facts, level, index),
                )
                && self.read_target_in_border(facts.bounds, level)
        } else {
            source.valid_target_facts(
                self.difficulty_id(),
                &mobs::goals::TargetFacts::Mob {
                    id: facts.id,
                    species: facts.species,
                    alive: facts.alive,
                    invulnerable: facts.invulnerable,
                    baby: facts.baby,
                },
                Some(facts.species),
            )
        }
    }
    fn read_target_in_border(&self, bounds: Aabb, level: LevelRef) -> bool {
        let border = if level.dimension() == self.dim.key() {
            &self.border
        } else {
            let Some(border) = self
                .levels
                .parked_border(Dim::from_id(level.dimension_id()))
            else {
                return false;
            };
            border
        };
        let border = border.geometry();
        border.is_within_bounds(bounds.min(Axis::X), bounds.min(Axis::Z), 0.0)
            && border.is_within_bounds(
                bounds.max(Axis::X) - f64::from(1.0e-5_f32),
                bounds.max(Axis::Z) - f64::from(1.0e-5_f32),
                0.0,
            )
    }
    pub(super) fn merchant_read_context<'a>(
        &'a self,
        source: &Mob,
        epoch: WorldEpoch,
        index: &NativeMobRelations,
    ) -> MerchantReadContext<'a> {
        let conn = index.merchants.get(&source.id).copied().filter(|conn| {
            self.lan
                .as_ref()
                .and_then(|host| host.play.get(conn))
                .and_then(|play| play.merchant.as_ref())
                .is_some_and(|m| m.merchant.mob == Some(source.id))
        });
        let session = conn
            .and_then(|conn| self.lan.as_ref()?.play.get(&conn)?.merchant.as_ref())
            .filter(|m| m.merchant.mob == Some(source.id))
            .map(|m| m.merchant.session.as_ref());
        let trading_player = conn
            .and_then(|conn| self.read_guest(index, conn))
            .and_then(|g| {
                if !g.placed || !g.participates_online() || self.lan_guest_swaps.contains(&g.conn) {
                    return None;
                }
                Some(mod_api::__private::game::player_ref(
                    EntityRef::new(epoch, u32::try_from(g.entity_id).ok()?),
                    g.uuid,
                ))
            });
        MerchantReadContext {
            session,
            trading_player,
        }
    }
}

impl Game<'_> {
    pub(super) fn read_relation_entity(
        &self,
        id: u32,
        level: LevelRef,
        _index: &NativeMobRelations,
    ) -> Option<EntityRef> {
        let mut table = self.entity_residency.borrow_mut();
        let (at, state, _, _) =
            table.server_admitted_read_lookup(id, |at| self.locate_server_resident_owner(at))?;
        let visible = match state {
            EntityLookup::Found {
                location,
                value: (target, true),
            } => {
                location.owner.dimension == level.dimension()
                    && self.server_read_target_visible(level.dimension(), target)
            }
            EntityLookup::Taken => {
                at.owner.dimension == level.dimension()
                    && !matches!(at.source, Source::DragonPart(_))
                    && self.server_read_target_visible(
                        level.dimension(),
                        super::entity_residency::residency_target_at(at, id),
                    )
            }
            _ => false,
        };
        visible.then(|| EntityRef::new(level.epoch(), id))
    }
    fn read_credit_id(
        &self,
        credit: super::combat::PlayerCredit,
        index: &NativeMobRelations,
        last_attacker: bool,
    ) -> Option<u32> {
        let uuid = horizon_client::lan::playerdata::uuid_ints(credit.0);
        if self.player.is_some() && uuid == self.acting_uuid() {
            if last_attacker && self.lan_acting.is_some() {
                return self.lan_acting_entity;
            }
            return u32::try_from(self.own_entity_id()).ok();
        }
        let conn = self.lan.as_ref()?.hub.guests.online_profile(credit.0)?.conn;
        if self.lan_acting == Some(conn) {
            return self.lan_acting_entity;
        }
        u32::try_from(self.read_guest(index, conn)?.entity_id).ok()
    }
    /// Credit is supplied from the source level's last_hurt_by_player map.
    /// A recorded but unresolved player credit does not fall back to a mob.
    pub(super) fn raw_mob_living_relations(
        &self,
        source: &Mob,
        level: LevelRef,
        index: &NativeMobRelations,
        credit: Option<super::combat::PlayerCredit>,
    ) -> (Option<EntityRef>, Option<EntityRef>) {
        let last_attacker = source
            .ai
            .last_hurt_by
            .filter(|_| source.age as i32 - source.ai.last_hurt_by_time <= 100)
            .and_then(|who| match who {
                Who::Mob(id) => Some(id),
                Who::Player => self.read_credit_id(credit?, index, true),
            })
            .and_then(|id| self.read_relation_entity(id, level, index));
        let kill_credit = match credit {
            Some(credit) => self
                .read_credit_id(credit, index, false)
                .and_then(|id| self.read_relation_entity(id, level, index)),
            None => last_attacker,
        };
        (kill_credit, last_attacker)
    }
    /// Borrow the actual equipment stack: duration needs no ItemStack clone.
    pub(super) fn raw_mob_item_use_remaining(&self, source: &Mob) -> i32 {
        use horizon_items::equipment::{MAINHAND, OFFHAND};
        if !horizon_client::mobs::spear_use::is_using_item(source) {
            return 0;
        }
        let slot = if source.extra.using_off_hand {
            OFFHAND
        } else {
            MAINHAND
        };
        source.extra.slot_stack(slot).map_or(0, |stack| {
            super::item_use::ticks_left(
                self.use_duration(stack),
                source.extra.using_item.unsigned_abs() - 1,
            )
        })
    }
}

impl Game<'_> {
    pub(super) fn resolve_lent_mob_goal_target(
        &self,
        actor: &Mob,
        world: &mobs::MobWorld<'_>,
        target: LivingRef,
        validate: bool,
    ) -> Option<Who> {
        let epoch = self.native_lifecycle.epoch()?;
        let entity = target.entity();
        if actor.removed || entity.epoch() != epoch {
            return None;
        }
        let level = LevelRef::new(epoch, mod_api::__private::dimension_key(world.level.id()));
        self.with_mob_read_relations(level, |index| {
            self.read_relation_entity(actor.id, level, index)?;
            let id = entity.id();
            if id == actor.id {
                return (!validate
                    || self.read_mob_target_valid(
                        actor,
                        NativeMobTargetFacts::capture(actor),
                        level,
                        index,
                    ))
                .then_some(Who::Mob(id));
            }
            if let Some(facts) = self.read_mob_facts(id, level) {
                return (!validate || self.read_mob_target_valid(actor, facts, level, index))
                    .then_some(Who::Mob(id));
            }
            // Player identity comes from the current source, not the goal's Seen
            // or Live view. Selection only binds Who after validation succeeds.
            let (uuid, valid) =
                if self.player.is_some() && u32::try_from(self.own_entity_id()).ok() == Some(id) {
                    if level.dimension() != self.dim.key() || self.lan_acting_fake {
                        return None;
                    }
                    let facts = self.native_player_facts(Target::Player)?;
                    let uuid = facts.uuid;
                    let valid = !validate
                        || self.read_player_target_valid(
                            actor,
                            PlayerTargetSource {
                                facts,
                                warden_invulnerable: self.player.as_ref()?.invulnerable,
                                bounds: self.player.as_ref()?.bounds(),
                                team: self.player_team(),
                            },
                            level,
                            index,
                        );
                    (uuid, valid)
                } else {
                    let conn = {
                        let mut table = self.entity_residency.borrow_mut();
                        let (at, _, _, _) = table.server_admitted_read_lookup(id, |at| {
                            self.locate_server_resident_owner(at)
                        })?;
                        let Source::Guest(conn) = at.source else {
                            return None;
                        };
                        conn
                    };
                    let guest = self.read_guest(index, conn)?;
                    self.read_relation_entity(id, level, index)?;
                    if guest.is_fake() {
                        return None;
                    }
                    let uuid = horizon_client::lan::playerdata::uuid_ints(guest.uuid);
                    let valid = !validate
                        || self.read_player_target_valid(
                            actor,
                            self.guest_target_source(guest),
                            level,
                            index,
                        );
                    (uuid, valid)
                };
            if validate && !valid {
                return None;
            }
            match world.player_selection {
                Some(selection) => {
                    let player = selection.get(id)?;
                    if player.view.uuid != uuid || !selection.select(id) {
                        return None;
                    }
                }
                None if world.selected_player_identity() != Some((uuid, id)) => return None,
                None => {}
            }
            Some(Who::Player)
        })?
    }
    /// Read passengers from authoritative rider relations. An unavailable lifted
    /// rider publication refuses the whole heavy read rather than dropping it.
    pub(super) fn lent_mob_passengers(&self, actor: &Mob) -> Option<Vec<EntityRef>> {
        if actor.removed {
            return None;
        }
        let epoch = self.native_lifecycle.epoch()?;
        let level = {
            let mut table = self.entity_residency.borrow_mut();
            let (state, cached) = table
                .server_entity_read_lookup(actor.id, |at| self.locate_server_resident_owner(at));
            match state {
                EntityLookup::Found {
                    location,
                    value: (Target::Mob(_), true),
                } => LevelRef::new(epoch, location.owner.dimension),
                EntityLookup::Taken => cached?.base.level,
                _ => return None,
            }
        };
        let guest_candidates = self.mob_guest_rider_connections(actor.id)?;
        self.with_mob_read_relations(level, |index| {
            self.read_relation_entity(actor.id, level, index)?;
            let mut seats = Vec::new();
            let acting = self.lan_acting.filter(|_| self.player.is_some());
            {
                for conn in guest_candidates {
                    if acting == Some(conn) {
                        continue;
                    }
                    let Some(guest) = self.read_guest(index, conn) else {
                        continue;
                    };
                    if guest.placed
                        && guest.dimension == level.dimension_id()
                        && guest.vehicle == Some(actor.id)
                    {
                        let id = u32::try_from(guest.entity_id).ok()?;
                        if let Some(entity) = self.read_relation_entity(id, level, index) {
                            seats.push((
                                guest.vehicle_order,
                                false,
                                self.lan.as_ref()?.hub.guests.connection_index(conn)? as u64,
                                entity,
                            ));
                        }
                    }
                }
            }
            let acting_guest = acting
                .and_then(|conn| self.lan.as_ref()?.hub.guest(conn))
                .filter(|g| g.placed && g.dimension == level.dimension_id());
            if self.riding == Some(actor.id) {
                if let Some(guest) = acting_guest {
                    seats.push((
                        guest.vehicle_order,
                        false,
                        self.lan.as_ref()?.hub.guests.connection_index(guest.conn)? as u64,
                        EntityRef::new(epoch, u32::try_from(guest.entity_id).ok()?),
                    ));
                } else if self.lan_acting_uuid.is_some() && self.player.is_some() {
                    seats.push((
                        i64::MIN,
                        false,
                        0,
                        EntityRef::new(epoch, self.lan_acting_entity?),
                    ));
                }
            }
            {
                for id in index.mob_rider_candidates(actor.id)? {
                    let facts = if id == actor.id {
                        NativeMobTargetFacts::capture(actor)
                    } else {
                        self.read_mob_facts(id, level)?
                    };
                    if facts.alive && facts.mount == Some(actor.id) {
                        let entity = self.read_relation_entity(id, level, index)?;
                        seats.push((facts.vehicle_order, true, facts.join.get(), entity));
                    }
                }
            }
            seats.sort_by_key(|&(order, mob, tie, _)| (order, mob, tie));
            Some(seats.into_iter().map(|(_, _, _, entity)| entity).collect())
        })?
    }
}

impl Game<'_> {
    pub(super) fn lent_mob_target(
        &self,
        mob: &Mob,
        actor: &Mob,
        world: &mobs::MobWorld<'_>,
    ) -> Option<LivingRef> {
        if mob.removed {
            return None;
        }
        let epoch = self.native_lifecycle.epoch()?;
        let level = LevelRef::new(epoch, mod_api::__private::dimension_key(world.level.id()));
        let who = brain_target(mob).unwrap_or(mob.ai.target)?;
        if who == Who::Player
            && mob.id == actor.id
            && let Some(selection) = world
                .player_selection
                .filter(|selection| selection.has_switched())
        {
            let player = selection.selected()?;
            let entity = EntityRef::new(epoch, player.entity_id);
            return self
                .resolve_lent_mob_goal_target(mob, world, LivingRef::new(entity), true)
                .map(|_| LivingRef::new(entity));
        }
        self.with_mob_read_relations(level, |index| {
            self.read_relation_entity(mob.id, level, index)?;
            if let Who::Mob(id) = who {
                let facts = if id == mob.id {
                    Some(NativeMobTargetFacts::capture(mob))
                } else if id == actor.id {
                    Some(NativeMobTargetFacts::capture(actor))
                } else {
                    None
                };
                if let Some(facts) = facts {
                    self.read_relation_entity(id, level, index)?;
                    return self
                        .read_mob_target_valid(mob, facts, level, index)
                        .then(|| LivingRef::new(EntityRef::new(epoch, id)));
                }
            }
            self.read_mob_target(mob, who, level, index)
        })?
    }
}

impl Game<'_> {
    /// Resolve a saved player UUID through canonical current identity, never by
    /// generating an entity UUID or scanning player/session populations.
    pub(super) fn resolve_mob_read_player(
        &self,
        uuid: [i32; 4],
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> Option<EntityRef> {
        if self.native_lifecycle.epoch() != Some(level.epoch()) {
            return None;
        }
        let acting_uuid = self
            .lan_acting_uuid
            .map(horizon_client::lan::playerdata::uuid_ints)
            .or(self.level.player_uuid);
        if self.player.is_some() && !self.lan_acting_fake && acting_uuid == Some(uuid) {
            return self.read_relation_entity(
                u32::try_from(self.own_entity_id()).ok()?,
                level,
                index,
            );
        }
        let guest = self
            .lan
            .as_ref()?
            .hub
            .guests
            .online_profile(horizon_client::lan::playerdata::ints_uuid(uuid))?;
        if !guest.participates_online()
            || horizon_client::lan::playerdata::uuid_ints(guest.uuid) != uuid
        {
            return None;
        }
        self.read_relation_entity(u32::try_from(guest.entity_id).ok()?, level, index)
    }
}

impl Game<'_> {
    /// Saved UUID resolution uses the retained identity index but validates its
    /// binding against the canonical level record before returning a handle.
    pub(super) fn resolve_read_uuid(
        &self,
        uuid: [i32; 4],
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> Option<EntityRef> {
        if self.native_lifecycle.epoch() != Some(level.epoch()) {
            return None;
        }
        if let Some(player) = self.resolve_mob_read_player(uuid, level, index) {
            return Some(player);
        }
        let id = *index.saved_ids.get(&uuid)?;
        if self.read_saved_uuid(id, level) != Some(uuid) {
            return None;
        }
        self.read_relation_entity(id, level, index)
    }
    pub(super) fn read_relation_guest(
        &self,
        conn: u32,
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> Option<EntityRef> {
        let guest = self.read_guest(index, conn)?;
        if !guest.participates_online() {
            return None;
        }
        self.read_relation_entity(u32::try_from(guest.entity_id).ok()?, level, index)
    }
}
#[cfg(test)]
mod tests {
    use super::super::{
        native_entity_refs::tests::with_game, native_guest_turn_tests::with_guests,
    };
    use super::*;
    use horizon_core::geometry::Vec3;

    fn mob(game: &Game<'_>, id: u32, name: &str) -> Mob {
        Mob::new(
            id,
            game.res.mob_types().get(name).unwrap().clone(),
            Vec3::new(0.0, 64.0, 0.0),
            0.0,
        )
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn shared_rules_preserve_species_baby_and_player_boundaries() {
        with_game(|game| {
            let world = game.mob_world_base(&[]);
            for (actor, target, baby, allowed) in [
                ("minecraft:breeze", Species::IronGolem, false, true),
                ("minecraft:breeze", Species::Cow, false, false),
                ("minecraft:creeper", Species::Goat, false, false),
                ("minecraft:creeper", Species::Cow, false, true),
                ("minecraft:pillager", Species::Villager, true, false),
                ("minecraft:pillager", Species::Villager, false, true),
                ("minecraft:iron_golem", Species::Creeper, false, true),
                ("minecraft:zombie", Species::Ghast, false, false),
            ] {
                let actor = mob(game, 40, actor);
                let facts = mobs::goals::TargetFacts::Mob {
                    id: 41,
                    species: target,
                    alive: true,
                    invulnerable: false,
                    baby,
                };
                // No Seen entry preserves the original wrapper's exact visibility contract.
                assert_eq!(actor.valid_target(&world, &facts), allowed);
                assert_eq!(
                    actor.valid_target_facts(i32::from(world.difficulty.base), &facts, None),
                    allowed
                );
                for (alive, invulnerable) in [(false, false), (true, true)] {
                    let facts = mobs::goals::TargetFacts::Mob {
                        id: 41,
                        species: target,
                        alive,
                        invulnerable,
                        baby,
                    };
                    assert!(!actor.valid_target(&world, &facts));
                    assert!(!actor.valid_target_facts(2, &facts, None));
                }
            }
            let golem = mob(game, 40, "minecraft:iron_golem");
            let creeper = mob(game, 41, "minecraft:creeper");
            let seen = [creeper.seen(None)];
            let world = game.mob_world_base(&seen);
            let facts = mobs::goals::TargetFacts::Mob {
                id: 41,
                species: Species::Creeper,
                alive: true,
                invulnerable: false,
                baby: false,
            };
            assert!(!golem.valid_target(&world, &facts));
            assert!(!golem.valid_target_facts(2, &facts, Some(Species::Creeper)));
            let mut dolphin = mob(game, 40, "minecraft:dolphin");
            dolphin.extra.age = -10;
            let facts = mobs::goals::TargetFacts::Mob {
                id: 41,
                species: Species::Cow,
                alive: true,
                invulnerable: false,
                baby: false,
            };
            assert!(!dolphin.valid_target(&world, &facts));
            assert!(!dolphin.valid_target_facts(2, &facts, Some(Species::Cow)));
            let zombie = mob(game, 40, "minecraft:zombie");
            for (difficulty, creative, spectator, alive, invulnerable, expected) in [
                (0, false, false, true, false, false),
                (2, false, false, true, false, true),
                (2, true, false, true, false, false),
                (2, false, true, true, false, false),
                (2, false, false, false, false, false),
                (2, false, false, true, true, false),
            ] {
                let facts = mobs::goals::TargetFacts::Player {
                    alive,
                    invulnerable,
                    creative,
                    spectator,
                    uuid: [0; 4],
                };
                assert_eq!(
                    zombie.valid_target_facts(difficulty, &facts, None),
                    expected
                );
                let mut world = game.mob_world_base(&[]);
                world.difficulty.base = difficulty as u8;
                assert_eq!(zombie.valid_target(&world, &facts), expected);
            }
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn context_resolves_resident_targets_and_indexed_trade_and_riders() {
        with_guests(|game| {
            let merchant =
                super::super::native_merchant_refs::tests::spawn(game, 40, "minecraft:villager");
            let level = game.entity_level(merchant.entity()).unwrap();
            let mut actor = mob(game, 42, "minecraft:iron_golem");
            actor.ai.target = Some(Who::Mob(40));
            let mut index = NativeMobRelations::default();
            game.rebuild_mob_read_relations(level, &mut index);
            assert_eq!(
                game.mob_read_context(&actor, level, &index)
                    .target
                    .unwrap()
                    .id(),
                40
            );
            game.mobs[0].invulnerable_flag = true;
            assert!(
                game.mob_read_context(&actor, level, &index)
                    .target
                    .is_none()
            );
            game.mobs[0].invulnerable_flag = false;
            let warden = mob(game, 42, "minecraft:warden");
            let warden_slot = game.mobs.len();
            game.mobs.push(warden);
            let join = game.mobs[warden_slot].join;
            let warden = game.mobs.extract(join).unwrap();
            assert!(
                game.read_mob_target(&warden, Who::Mob(40), level, &index)
                    .is_some()
            );
            game.mobs[0].position.x = 40_000_000.0;
            assert!(
                game.read_mob_target(&warden, Who::Mob(40), level, &index)
                    .is_none()
            );
            game.mobs[0].position.x = 0.0;
            game.set_saved_entity_uuid(40, [0, 0, 0, 40]);
            game.set_saved_entity_uuid(42, [0, 0, 0, 42]);
            index.update_saved_uuid(40, None, Some([0, 0, 0, 40]));
            index.update_saved_uuid(42, None, Some([0, 0, 0, 42]));
            game.score.board.add_team("allies");
            game.score
                .board
                .add_to_team(&horizon_client::save::uuid_text([0, 0, 0, 40]), "allies");
            game.score
                .board
                .add_to_team(&horizon_client::save::uuid_text([0, 0, 0, 42]), "allies");
            assert!(
                game.read_mob_target(&warden, Who::Mob(40), level, &index)
                    .is_none()
            );
            game.mobs.put_back(warden_slot, warden);
            let mut session = horizon_client::merchant::Session::new(Vec::new(), true);
            session.xp = 17;
            session.level = 3;
            game.lan_open_merchant(
                13,
                super::super::trading::MerchantState {
                    session: Box::new(session),
                    scroll: 0,
                    dragging: false,
                    focused: None,
                    mob: Some(40),
                    name: None,
                },
            );
            index.update_merchant(13, Some(40));
            let context = game.merchant_read_context(&game.mobs[0], level.epoch(), &index);
            assert_eq!(context.session.unwrap().xp, 17);
            assert_eq!(context.trading_player.unwrap().uuid(), 13);
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .update_guest(13, |guest| guest.vehicle = Some(42));
            index.update_guest(game.lan.as_ref().unwrap().hub.guest(13).unwrap());
            assert!(game.mob_read_context(&actor, level, &index).guest_riding);
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .update_guest(13, |guest| guest.vehicle = None);
            index.update_guest(game.lan.as_ref().unwrap().hub.guest(13).unwrap());
            assert!(!game.mob_read_context(&actor, level, &index).guest_riding);
            index.update_merchant(13, None);
            assert!(
                game.merchant_read_context(&game.mobs[0], level.epoch(), &index)
                    .session
                    .is_none()
            );
            index.remove_guest(13);
            assert!(
                game.read_guest(&index, 13).is_some(),
                "a candidate index is not identity authority"
            );
            let host = game.lan.as_mut().unwrap();
            let slot = host.hub.guests.connection_index(13).unwrap();
            let _removed = host.hub.guests.remove(slot);
            assert!(game.read_guest(&index, 13).is_none());
        });
    }
}

#[cfg(test)]
mod lent_tests {
    use super::super::native_guest_turn_tests::with_guests;
    use super::*;
    use horizon_client::{
        join_seq::RestoreOrder,
        mobs::player_selection::{PlayerSelection, TickPlayer},
    };
    use horizon_core::geometry::Vec3;
    use mod_api::EntityReadCapturePhase;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn real_publications_resolve_lifted_targets_and_every_ordered_passenger() {
        with_guests(|game| {
            game.options.dedicated = Some(Vec::new());
            for (id, kind) in [
                (40, "minecraft:zombie"),
                (41, "minecraft:cow"),
                (42, "minecraft:pig"),
                (43, "minecraft:sheep"),
            ] {
                game.mobs.push(Mob::new(
                    id,
                    game.res.mob_types().get(kind).unwrap().clone(),
                    Vec3::new(0.0, 64.0, 0.0),
                    0.0,
                ));
                assert!(game.entity(id).is_some());
            }
            game.mobs[0].ai.target = Some(Who::Mob(41));
            game.mobs[2].extra.mount = Some(40);
            game.mobs[2].extra.vehicle_order = 9;
            game.mobs[3].extra.mount = Some(40);
            game.mobs[3].extra.vehicle_order = 2;
            game.lan.as_mut().unwrap().hub.update_guest(13, |guest| {
                guest.vehicle = Some(40);
                guest.vehicle_order = 5;
            });
            let epoch = game.native_lifecycle.epoch().unwrap();
            let level = LevelRef::new(epoch, game.native_level_key());
            {
                let world = game.mob_world_base(&[]);
                for source in game.mobs.iter() {
                    assert!(game.publish_lent_mob_reads(
                        source,
                        &world,
                        EntityReadCapturePhase::BeforeCallback
                    ));
                }
            }
            let mut sources = game.mobs.lift();
            {
                let world = game.mob_world_base(&[]);
                assert!(world.live_mobs.is_none());
                assert!(world.others.is_empty());
                let target = LivingRef::new(EntityRef::new(epoch, 41));
                assert_eq!(
                    game.resolve_lent_mob_goal_target(&sources[0], &world, target, true),
                    Some(Who::Mob(41))
                );
                assert_eq!(
                    game.lent_mob_target(&sources[0], &sources[0], &world)
                        .unwrap()
                        .id(),
                    41
                );
                assert_eq!(
                    game.lent_mob_passengers(&sources[0])
                        .unwrap()
                        .iter()
                        .map(|e| e.id())
                        .collect::<Vec<_>>(),
                    [43, 13, 42]
                );
                assert!(
                    game.resolve_lent_mob_goal_target(
                        &sources[0],
                        &world,
                        LivingRef::new(EntityRef::new(WorldEpoch::new(epoch.get() + 1), 41)),
                        false
                    )
                    .is_none()
                );
            }
            sources[1].invulnerable_flag = true;
            {
                let world = game.mob_world_base(&[]);
                assert!(game.publish_lent_mob_reads(
                    &sources[1],
                    &world,
                    EntityReadCapturePhase::AfterAppliedWrite
                ));
                let target = LivingRef::new(EntityRef::new(epoch, 41));
                assert!(
                    game.resolve_lent_mob_goal_target(&sources[0], &world, target, true)
                        .is_none()
                );
                assert_eq!(
                    game.resolve_lent_mob_goal_target(&sources[0], &world, target, false),
                    Some(Who::Mob(41))
                );
                assert!(
                    game.lent_mob_target(&sources[0], &sources[0], &world)
                        .is_none()
                );
            }
            // Real retained pass freshness is part of the helper contract.
            game.entity_read_state
                .borrow_mut()
                .mob_relations
                .get_mut(&level)
                .unwrap()
                .tick = None;
            assert!(game.lent_mob_passengers(&sources[0]).is_none());
            game.mobs.restore(sources, RestoreOrder::SurvivorsFirst);
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn player_redirect_requires_exact_identity_and_selects_before_returning_who() {
        with_guests(|game| {
            game.options.dedicated = Some(Vec::new());
            for conn in [12, 13] {
                game.lan
                    .as_mut()
                    .unwrap()
                    .hub
                    .update_guest(conn, |guest| guest.loaded = true);
            }
            game.mobs.push(Mob::new(
                40,
                game.res
                    .mob_types()
                    .get("minecraft:zombie")
                    .unwrap()
                    .clone(),
                Vec3::new(0.0, 64.0, 0.0),
                0.0,
            ));
            assert!(game.entity(40).is_some());
            let epoch = game.native_lifecycle.epoch().unwrap();
            let level = LevelRef::new(epoch, game.native_level_key());
            {
                let world = game.mob_world_base(&[]);
                assert!(game.publish_lent_mob_reads(
                    &game.mobs[0],
                    &world,
                    EntityReadCapturePhase::BeforeCallback
                ));
            }
            let players = game
                .lan_guest_views()
                .into_iter()
                .filter_map(|(conn, view)| {
                    let (_, entity_id) = game.guest_identity(conn)?;
                    Some(TickPlayer {
                        entity_id,
                        connection: Some(conn),
                        view,
                    })
                })
                .collect();
            let selection = PlayerSelection::new(players, Some(12)).unwrap();
            let sources = game.mobs.lift();
            let target = LivingRef::new(EntityRef::new(epoch, 13));
            {
                let mut world = game.mob_world_base(&[]);
                world.player_identity = Some((horizon_client::lan::playerdata::uuid_ints(12), 12));
                assert!(
                    game.resolve_lent_mob_goal_target(&sources[0], &world, target, false)
                        .is_none()
                );
                world.player_selection = Some(&selection);
                assert_eq!(
                    game.resolve_lent_mob_goal_target(&sources[0], &world, target, true),
                    Some(Who::Player)
                );
                assert_eq!(selection.selected().unwrap().entity_id, 13);
            }
            let resolved = game
                .with_mob_read_relations(level, |index| {
                    game.resolve_mob_read_player(
                        horizon_client::lan::playerdata::uuid_ints(13),
                        level,
                        index,
                    )
                })
                .flatten();
            assert_eq!(resolved.unwrap().id(), 13);
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .update_guest(13, |guest| guest.uuid = 999);
            let world = game.mob_world_base(&[]);
            assert!(
                game.resolve_lent_mob_goal_target(&sources[0], &world, target, false)
                    .is_none()
            );
            drop(world);
            game.mobs.restore(sources, RestoreOrder::SurvivorsFirst);
        });
    }
}

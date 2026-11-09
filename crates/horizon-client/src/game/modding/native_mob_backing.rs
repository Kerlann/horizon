//! Changed-only invalidation of the minimal AI player projections.
//!
//! Team rules are queried from canonical sources at the decision. Player turn
//! owners compare their represented fields after apply, not enqueue acceptance;
//! unrelated callbacks and no-op writes do not enqueue projection work.
use super::Game;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct MobBackingRevision {
    pub(super) players: u64,
    pub(super) teams: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::game) struct PlayerBackingKey {
    pub(super) uuid: [i32; 4],
    pub(super) conn: Option<u32>,
    pub(super) dimension: mod_api::DimensionKey,
}

/// This is a turn-local comparison of the real selected player, not a retained
/// foreign H snapshot. Equipment identifiers are shared Arcs and effects are IDs.
#[derive(PartialEq)]
pub(in crate::game) struct PlayerAiFacts {
    view: Option<horizon_client::mobs::PlayerView>,
    health: Option<u32>,
    key: PlayerBackingKey,
}

impl Game<'_> {
    pub(super) fn ai_player_vehicle_view(
        &self,
        mobs: &horizon_client::join_seq::JoinList<horizon_client::mobs::Mob>,
    ) -> Option<horizon_client::mobs::Vehicle> {
        use horizon_client::join_seq::residency::Kind;
        use horizon_core::geometry::{Aabb, Vec3};
        let id = self.riding?;
        let (at, state, _) = self
            .entity_residency
            .borrow_mut()
            .server_admitted_identity(id, |at| self.locate_server_resident_owner(at))?;
        if at.owner.dimension != self.dim.key()
            || !matches!(
                state,
                horizon_client::join_seq::residency::table::EntityLookup::Taken
                    | horizon_client::join_seq::residency::table::EntityLookup::Found {
                        value: (_, true),
                        ..
                    }
            )
        {
            return None;
        }
        match at.owner.kind {
            Kind::Boat => {
                let boat = &self.boats[self.boats.locate(at.join)?];
                Some(horizon_client::mobs::Vehicle {
                    nautilus: false,
                    bounds: Aabb::new(
                        Vec3::new(
                            boat.position.x - 0.6875,
                            boat.position.y,
                            boat.position.z - 0.6875,
                        ),
                        Vec3::new(
                            boat.position.x + 0.6875,
                            boat.position.y + 0.5625,
                            boat.position.z + 0.6875,
                        ),
                    ),
                    velocity: boat.velocity,
                })
            }
            Kind::Mob => {
                let mob = &mobs[mobs.locate(at.join)?];
                matches!(
                    mob.species(),
                    horizon_client::mobs::Species::Nautilus
                        | horizon_client::mobs::Species::ZombieNautilus
                )
                .then(|| horizon_client::mobs::Vehicle {
                    nautilus: true,
                    bounds: mob.bounds(),
                    velocity: mob.velocity,
                })
            }
            _ => None,
        }
    }

    pub(super) fn ai_player_view_for_pass(
        &self,
        mobs: &horizon_client::join_seq::JoinList<horizon_client::mobs::Mob>,
    ) -> Option<horizon_client::mobs::PlayerView> {
        let mut view = self.ai_player_view()?;
        view.vehicle = self.ai_player_vehicle_view(mobs);
        Some(view)
    }

    /// A direct body writer already owns its resolved target. Call only when
    /// the represented scalar really changed, after its canonical apply.
    pub(super) fn mark_player_target_backing_changed(&self, target: super::commands::Target) {
        use super::commands::Target;
        let key = match target {
            Target::Player if !self.lan_acting_fake => PlayerBackingKey {
                uuid: self.acting_uuid(),
                conn: self.lan_acting,
                dimension: self.dim.key(),
            },
            Target::Guest(conn) => {
                let Some(guest) = self.lan.as_ref().and_then(|h| h.hub.guest(conn)) else {
                    return;
                };
                if !guest.participates_online() {
                    return;
                }
                PlayerBackingKey {
                    uuid: horizon_client::lan::playerdata::uuid_ints(guest.uuid),
                    conn: Some(conn),
                    dimension: super::Dim::from_id(&guest.dimension).key(),
                }
            }
            _ => return,
        };
        self.mark_mob_player_backing_dirty(key);
    }

    pub(in crate::game) fn player_ai_facts(&self) -> Option<PlayerAiFacts> {
        let state = self.entity_read_state.borrow();
        state.mob_ai_level?;
        if self.lan_acting_fake {
            return None;
        }
        Some(PlayerAiFacts {
            view: self.ai_player_view(),
            health: self.player.as_ref().map(|p| p.health.health().to_bits()),
            key: PlayerBackingKey {
                uuid: self.acting_uuid(),
                conn: self.lan_acting,
                dimension: self.dim.key(),
            },
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

    pub(super) fn mark_mob_player_backing_dirty(&self, key: PlayerBackingKey) {
        let mut state = self.entity_read_state.borrow_mut();
        if state.mob_ai_level != Some(key.dimension) {
            return;
        }
        state.mob_player_writes.insert(key);
        state.mob_backing.players = state
            .mob_backing
            .players
            .checked_add(1)
            .expect("mob player backing revision exhausted");
    }
}

/// Positions belong only to our owned AI views, never to Hub/source storage.
#[derive(Default)]
pub(super) struct PlayerBackingIndex {
    guests: std::collections::HashMap<u32, usize>,
    guest_ids: std::collections::HashMap<u32, usize>,
    pending: std::collections::HashSet<PlayerBackingKey>,
}
pub(super) struct PlayerBackingLoans<'a> {
    pub(super) mobs: &'a horizon_client::join_seq::JoinList<horizon_client::mobs::Mob>,
    pub(super) local: &'a mut Option<horizon_client::mobs::PlayerView>,
    pub(super) nearest: &'a mut super::natural_spawning::NearestPlayers,
    pub(super) guests: &'a mut super::lan_mobs::GuestViews,
    pub(super) spare: &'a mut Option<horizon_client::mobs::PlayerView>,
    pub(super) guest_ids: &'a mut Vec<(u32, u32)>,
}
impl PlayerBackingIndex {
    pub(super) fn new(guests: &super::lan_mobs::GuestViews, ids: &[(u32, u32)]) -> Self {
        Self {
            guests: guests
                .iter()
                .enumerate()
                .map(|(i, (conn, _))| (*conn, i))
                .collect(),
            guest_ids: ids
                .iter()
                .enumerate()
                .map(|(i, (conn, _))| (*conn, i))
                .collect(),
            ..Default::default()
        }
    }
}
impl Game<'_> {
    pub(super) fn refresh_ai_player_backing(
        &mut self,
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
        let mut refreshed = 0;
        for key in index.pending.drain() {
            let uuid = horizon_client::lan::playerdata::ints_uuid(key.uuid);
            let local = self.lan_acting == key.conn && self.acting_uuid() == key.uuid;
            let level = if local {
                *loans.local = self.ai_player_view_for_pass(loans.mobs);
                self.local_level_player()
            } else if let Some(conn) = key.conn {
                let view = self.ai_guest_view(conn, loans.mobs);
                let changed_membership = match (index.guests.get(&conn).copied(), view) {
                    (Some(slot), Some(view)) => {
                        loans.guests[slot].1 = view;
                        false
                    }
                    (Some(slot), None) => {
                        loans.guests.remove(slot);
                        true
                    }
                    (None, Some(view)) => {
                        loans.guests.push((conn, view));
                        true
                    }
                    (None, None) => false,
                };
                // Only structural membership changes shift the owned ordering;
                // ordinary scalar/equipment writes replace exactly one slot.
                if changed_membership {
                    index.guests = loans
                        .guests
                        .iter()
                        .enumerate()
                        .map(|(i, (c, _))| (*c, i))
                        .collect();
                }
                let guest = self.lan.as_ref().and_then(|h| h.hub.guest(conn));
                if let Some(guest) = guest {
                    if let Some(&slot) = index.guest_ids.get(&conn) {
                        loans.guest_ids[slot].1 = guest.entity_id as u32;
                    } else {
                        index.guest_ids.insert(conn, loans.guest_ids.len());
                        loans.guest_ids.push((conn, guest.entity_id as u32));
                    }
                } else if let Some(slot) = index.guest_ids.remove(&conn) {
                    loans.guest_ids.remove(slot);
                    index.guest_ids = loans
                        .guest_ids
                        .iter()
                        .enumerate()
                        .map(|(i, (c, _))| (*c, i))
                        .collect();
                }
                guest
                    .filter(|g| g.uuid == uuid && g.placed_in(self.dim.id()))
                    .map(super::natural_spawning::LevelPlayer::guest)
            } else {
                None
            };
            loans.nearest.update(uuid, level);
            refreshed += 1;
        }
        *loans.spare = loans.guests.first().map(|g| g.1.clone());
        refreshed
    }
}

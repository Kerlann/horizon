//! Per-pass relationship indexes and direct-source context construction.
//! Rebuild once per pass into retained capacity. Canonical relation writes update
//! affected keys; callbacks do not trigger population rescans. No retained loans.
use super::{
    Dim, Game,
    commands::Target,
    native_mob_read_rows::{MerchantReadContext, MobReadContext},
};
use horizon_client::{
    join_seq::residency::table::EntityLookup,
    mobs::{self, Mob, Species, Who, ai::brain_target},
};
use horizon_core::geometry::{Aabb, Axis};
use mod_api::{EntityRef, LevelRef, LivingRef, NativeWorldEpoch};
use std::collections::HashMap;

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
        }
    }
}
#[derive(Default)]
pub(super) struct NativeMobRelations {
    guests: HashMap<u32, usize>,
    players_by_uuid: HashMap<u128, u32>,
    players_by_id: HashMap<u32, u32>,
    guest_riders: HashMap<u32, usize>,
    guest_vehicles: HashMap<u32, u32>,
    guest_identities: HashMap<u32, (u128, u32)>,
    merchant_by_conn: HashMap<u32, u32>,
    merchants: HashMap<u32, u32>,
    saved_ids: HashMap<[i32; 4], u32>,
}
impl NativeMobRelations {
    pub(super) fn clear(&mut self) {
        self.guests.clear();
        self.players_by_uuid.clear();
        self.players_by_id.clear();
        self.guest_riders.clear();
        self.guest_vehicles.clear();
        self.guest_identities.clear();
        self.merchants.clear();
        self.merchant_by_conn.clear();
        self.saved_ids.clear();
    }
    /// Call at guest insert, identity/placement/vehicle writes and slot moves.
    pub(super) fn update_guest(&mut self, at: usize, guest: &horizon_client::lan::guest::Guest) {
        self.remove_guest(guest.conn);
        self.guests.insert(guest.conn, at);
        if guest.placed {
            self.players_by_uuid.insert(guest.uuid, guest.conn);
        }
        if let Ok(id) = u32::try_from(guest.entity_id) {
            self.players_by_id.insert(id, guest.conn);
            self.guest_identities.insert(guest.conn, (guest.uuid, id));
        }
        if let Some(id) = guest.vehicle {
            self.guest_vehicles.insert(guest.conn, id);
            *self.guest_riders.entry(id).or_default() += 1;
        }
    }
    /// Call before guest removal; update a swap-moved guest's slot afterwards.
    pub(super) fn remove_guest(&mut self, conn: u32) {
        self.guests.remove(&conn);
        if let Some((uuid, id)) = self.guest_identities.remove(&conn) {
            if self.players_by_uuid.get(&uuid) == Some(&conn) {
                self.players_by_uuid.remove(&uuid);
            }
            if self.players_by_id.get(&id) == Some(&conn) {
                self.players_by_id.remove(&id);
            }
        }
        if let Some(id) = self.guest_vehicles.remove(&conn) {
            if let Some(count) = self.guest_riders.get_mut(&id) {
                *count -= 1;
                if *count == 0 {
                    self.guest_riders.remove(&id);
                }
            }
        }
    }
    /// Call at merchant session open/close/rebinding, including disconnect.
    pub(super) fn update_merchant(&mut self, conn: u32, mob: Option<u32>) {
        if let Some(old) = self.merchant_by_conn.remove(&conn) {
            if self.merchants.get(&old) == Some(&conn) {
                self.merchants.remove(&old);
            }
        }
        if let Some(id) = mob {
            self.merchant_by_conn.insert(conn, id);
            self.merchants.insert(id, conn);
        }
    }
    /// Canonical persisted UUID assignment/removal/travel must update both levels.
    pub(super) fn update_saved_uuid(
        &mut self,
        id: u32,
        old: Option<[i32; 4]>,
        new: Option<[i32; 4]>,
    ) {
        if let Some(old) = old {
            if self.saved_ids.get(&old) == Some(&id) {
                self.saved_ids.remove(&old);
            }
        }
        if let Some(new) = new {
            self.saved_ids.insert(new, id);
        }
    }
}
impl Game<'_> {
    /// Rebuild once per pass, preserving allocation capacity. This is not a
    /// callback refresh: canonical changes use NativeMobRelations update methods.
    pub(super) fn rebuild_mob_read_relations(
        &self,
        level: LevelRef,
        index: &mut NativeMobRelations,
    ) {
        index.clear();
        if let Some(host) = &self.lan {
            for (at, guest) in host.hub.guests.iter().enumerate() {
                index.update_guest(at, guest);
            }
            for (&conn, play) in &host.play {
                index.update_merchant(conn, play.merchant.as_ref().and_then(|m| m.merchant.mob));
            }
        }
        if level.dimension() == self.dim.key() {
            index
                .saved_ids
                .extend(self.persist.uuids.iter().map(|(&id, &uuid)| (uuid, id)));
        } else if let Some(parked) = self.levels.parked_of(level.dimension_id()) {
            index
                .saved_ids
                .extend(parked.saved_entity_uuids().map(|(id, uuid)| (uuid, id)));
        }
    }
    fn read_saved_uuid(&self, id: u32, level: LevelRef) -> Option<[i32; 4]> {
        if level.dimension() == self.dim.key() {
            self.persist.uuids.get(&id).copied()
        } else {
            self.levels
                .parked_of(level.dimension_id())?
                .saved_entity_uuid(id)
        }
    }
    fn read_guest<'a>(
        &'a self,
        index: &NativeMobRelations,
        conn: u32,
    ) -> Option<&'a horizon_client::lan::guest::Guest> {
        let guest = self
            .lan
            .as_ref()?
            .hub
            .guests
            .get(*index.guests.get(&conn)?)?;
        (guest.conn == conn).then_some(guest)
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
    fn read_mob_team<'a>(
        &'a self,
        facts: NativeMobTargetFacts,
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> Option<&'a horizon_client::scoreboard::Team> {
        let uuid = self.read_saved_uuid(facts.id, level)?;
        let own = horizon_client::save::uuid_text(uuid);
        if let Some(team) = self.score.board.players_team(&own) {
            return Some(team);
        }
        let mut root = facts.owner?;
        for _ in 0..64 {
            let next = index
                .saved_ids
                .get(&root)
                .and_then(|&id| self.read_mob_facts(id, level))
                .and_then(|f| f.owner);
            match next {
                Some(next) => root = next,
                None => break,
            }
        }
        let uuid = horizon_client::lan::playerdata::ints_uuid(root);
        if self.player.is_some() && root == self.acting_uuid() {
            return self.score.board.players_team(self.acting_player_name());
        }
        if let Some(&conn) = index.players_by_uuid.get(&uuid) {
            return self
                .score
                .board
                .players_team(&self.read_guest(index, conn)?.name);
        }
        index.saved_ids.get(&root)?;
        self.score
            .board
            .players_team(&horizon_client::save::uuid_text(root))
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
            local_player_riding: self.riding == Some(source.id),
            guest_riding: index.guest_riders.contains_key(&source.id),
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
                let valid = if source.species() == Species::Warden {
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
                            id,
                            species: facts.species,
                            alive: facts.alive,
                            invulnerable: facts.invulnerable,
                            baby: facts.baby,
                        },
                        Some(facts.species),
                    )
                };
                valid.then(|| LivingRef::new(EntityRef::new(epoch, id)))
            }
            Who::Player => {
                let conn = *self.lan.as_ref()?.mob_players.get(&source.id)?;
                let guest = self.read_guest(index, conn)?;
                let id = u32::try_from(guest.entity_id).ok()?;
                let valid = if source.species() == Species::Warden {
                    guest.dimension == level.dimension_id()
                        && !guest.dead
                        && guest.health > 0.0
                        && !matches!(guest.game_mode, 1 | 3)
                        && !guest.motion.as_ref().is_some_and(|p| p.invulnerable)
                        && !horizon_client::scoreboard::teams_allied(
                            self.read_mob_team(NativeMobTargetFacts::capture(source), level, index),
                            self.score.board.players_team(&guest.name),
                        )
                        && self.read_target_in_border(guest.bounds(), level)
                } else {
                    !guest.is_fake()
                        && source.valid_target_facts(
                            self.difficulty_id(),
                            &mobs::goals::TargetFacts::Player {
                                alive: !guest.dead && guest.health > 0.0,
                                invulnerable: guest.motion.as_ref().is_some_and(|p| {
                                    p.invulnerable
                                        || p.health.permanently_invulnerable
                                        || p.health.invulnerable_time > 0
                                }),
                                creative: guest.game_mode == 1,
                                spectator: guest.game_mode == 3,
                                uuid: horizon_client::lan::playerdata::uuid_ints(guest.uuid),
                            },
                            None,
                        )
                };
                valid.then(|| LivingRef::new(EntityRef::new(epoch, id)))
            }
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
        epoch: NativeWorldEpoch,
        index: &NativeMobRelations,
    ) -> MerchantReadContext<'a> {
        let conn = index.merchants.get(&source.id).copied();
        let session = conn
            .and_then(|conn| self.lan.as_ref()?.play.get(&conn)?.merchant.as_ref())
            .map(|m| m.merchant.session.as_ref());
        let trading_player = conn
            .and_then(|conn| self.read_guest(index, conn))
            .and_then(|g| {
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
    fn read_relation_entity(&self, id: u32, level: LevelRef) -> Option<EntityRef> {
        let mut table = self.entity_residency.borrow_mut();
        let (state, cached) =
            table.server_entity_read_lookup(id, |at| self.locate_server_resident_owner(at));
        let visible = match state {
            EntityLookup::Found {
                location,
                value: (target, true),
            } => {
                location.owner.dimension == level.dimension()
                    && self.server_read_target_visible(level.dimension(), target)
            }
            EntityLookup::Taken => cached.is_some_and(|row| {
                row.base.level == level
                    && self.server_read_target_visible(level.dimension(), row.target)
            }),
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
        let conn = *index.players_by_uuid.get(&credit.0)?;
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
            .and_then(|id| self.read_relation_entity(id, level));
        let kill_credit = match credit {
            Some(credit) => self
                .read_credit_id(credit, index, false)
                .and_then(|id| self.read_relation_entity(id, level)),
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
            game.persist.uuids.insert(40, [0, 0, 0, 40]);
            game.persist.uuids.insert(42, [0, 0, 0, 42]);
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
            let at = *index.guests.get(&13).unwrap();
            let guest = &mut game.lan.as_mut().unwrap().hub.guests[at];
            guest.vehicle = Some(42);
            index.update_guest(at, guest);
            assert!(game.mob_read_context(&actor, level, &index).guest_riding);
            let guest = &mut game.lan.as_mut().unwrap().hub.guests[at];
            guest.vehicle = None;
            index.update_guest(at, guest);
            assert!(!game.mob_read_context(&actor, level, &index).guest_riding);
            index.update_merchant(13, None);
            assert!(
                game.merchant_read_context(&game.mobs[0], level.epoch(), &index)
                    .session
                    .is_none()
            );
            index.remove_guest(13);
            assert!(game.read_guest(&index, 13).is_none());
        });
    }
}

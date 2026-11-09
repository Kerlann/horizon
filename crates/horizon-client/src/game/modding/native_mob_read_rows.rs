//! Borrowed-source projections. Retained rows contain no per-publication owned collections.
use super::Game;
use horizon_client::mobs::{
    self, Mob, Species, Who,
    ai::brain_target,
    render::VILLAGER_TYPES,
    villager::{self, ProfessionState},
};
use mod_api::*;
use std::{collections::HashSet, sync::Arc};

const PATH_TYPES: [PathType; PathType::COUNT] = [
    PathType::Blocked,
    PathType::Open,
    PathType::Walkable,
    PathType::WalkableDoor,
    PathType::Trapdoor,
    PathType::PowderSnow,
    PathType::OnTopOfPowderSnow,
    PathType::Fence,
    PathType::Lava,
    PathType::Water,
    PathType::WaterBorder,
    PathType::Rail,
    PathType::UnpassableRail,
    PathType::FireInNeighbor,
    PathType::Fire,
    PathType::DamagingInNeighbor,
    PathType::Damaging,
    PathType::DoorOpen,
    PathType::DoorWoodClosed,
    PathType::DoorIronClosed,
    PathType::Breach,
    PathType::Leaves,
    PathType::StickyHoney,
    PathType::Cocoa,
    PathType::DamageCautious,
    PathType::OnTopOfTrapdoor,
    PathType::BigMobsCloseToDanger,
];

#[derive(Clone)]
pub(super) struct NativeMobReadRow {
    capture: EntityReadCapture,
    entity: EntityRef,
    is_done: Option<bool>,
    navigation_speed: Option<f64>,
    pathfinding_malus: [(PathType, f32); PathType::COUNT],
    can_use_portal: Option<bool>,
    target: Option<LivingRef>,
    is_aggressive: Option<bool>,
    persistence_required: Option<bool>,
    requires_custom_persistence: Option<bool>,
    no_ai: Option<bool>,
    restrict_center: Option<[i32; 3]>,
    restrict_radius: Option<i32>,
    has_restriction: Option<bool>,
    left_handed: Option<bool>,
    can_pick_up_loot: Option<bool>,
    no_action_time: Option<i32>,
    raid: Option<RaidRef>,
}
impl NativeMobReadRow {
    pub(super) fn materialize(&self) -> MobReadRow {
        MobReadRow {
            capture: self.capture,
            entity: self.entity,
            is_done: self.is_done,
            navigation_speed: self.navigation_speed,
            pathfinding_malus: self.pathfinding_malus.to_vec(),
            can_use_portal: self.can_use_portal,
            target: self.target,
            is_aggressive: self.is_aggressive,
            persistence_required: self.persistence_required,
            requires_custom_persistence: self.requires_custom_persistence,
            no_ai: self.no_ai,
            restrict_center: self.restrict_center,
            restrict_radius: self.restrict_radius,
            has_restriction: self.has_restriction,
            left_handed: self.left_handed,
            can_pick_up_loot: self.can_pick_up_loot,
            no_action_time: self.no_action_time,
            raid: self.raid,
        }
    }
}

#[derive(Clone)]
pub(super) struct NativeMerchantReadRow {
    capture: EntityReadCapture,
    entity: EntityRef,
    trading_player: Option<PlayerRef>,
    profession: Option<RetainedProfession>,
    villager_type: Option<&'static str>,
    level: Option<i32>,
    xp: Option<i32>,
}
#[derive(Clone)]
enum RetainedProfession {
    Vanilla(u8),
    Unresolved(Arc<str>),
}
impl NativeMerchantReadRow {
    pub(super) fn materialize(&self) -> MerchantReadRow {
        MerchantReadRow {
            capture: self.capture,
            entity: self.entity,
            trading_player: self.trading_player,
            profession: self.profession.as_ref().map(|p| match p {
                RetainedProfession::Vanilla(i) => {
                    format!("minecraft:{}", villager::profession(*i).id)
                }
                RetainedProfession::Unresolved(id) => id.to_string(),
            }),
            villager_type: self.villager_type.map(|name| format!("minecraft:{name}")),
            level: self.level,
            xp: self.xp,
        }
    }
}
impl Game<'_> {
    /// Identity is validated by the publisher; `source` may be lent out of Game.
    pub(super) fn capture_mob_read_row(
        &self,
        capture: EntityReadCapture,
        entity: EntityRef,
        level: LevelRef,
        source: &Mob,
    ) -> NativeMobReadRow {
        let target = brain_target(source)
            .unwrap_or(source.ai.target)
            .and_then(|who| {
                let id = match who {
                    Who::Mob(id) => id,
                    Who::Player => {
                        let host = self.lan.as_ref()?;
                        let conn = host.mob_players.get(&source.id)?;
                        u32::try_from(host.hub.guest(*conn)?.entity_id).ok()?
                    }
                };
                let entity = self.entity(id)?;
                self.native_mob_valid_target(source, self.native_entity_target(entity)?)
                    .then(|| self.living(entity))?
            });
        let raid = mobs::raider::state(source)
            .and_then(|state| state.raid_id)
            .and_then(|id| self.level_raid(level, id));
        NativeMobReadRow {
            capture,
            entity,
            is_done: Some(source.ai.nav.is_done()),
            navigation_speed: Some(source.ai.nav.speed_modifier),
            pathfinding_malus: PATH_TYPES.map(|kind| (kind, source.malus(kind))),
            can_use_portal: Some(self.mob_portal_eligible(source)),
            target,
            is_aggressive: Some(source.ai.aggressive),
            persistence_required: Some(source.persistent),
            requires_custom_persistence: Some(mobs::ai::requires_custom_persistence(source)),
            no_ai: Some(source.no_ai),
            restrict_center: Some(source.ai.home_position()),
            restrict_radius: Some(source.ai.home.map_or(-1, |(_, radius)| radius)),
            has_restriction: Some(source.ai.home.is_some()),
            left_handed: Some(source.extra.left_handed),
            can_pick_up_loot: Some(source.can_pick_up_loot()),
            no_action_time: Some(source.ai.no_action_time),
            raid,
        }
    }
    /// The publisher owns the identity pool, so unresolved saved ids allocate only once.
    pub(super) fn capture_merchant_read_row(
        &self,
        capture: EntityReadCapture,
        entity: EntityRef,
        source: &Mob,
        identities: &mut HashSet<Arc<str>>,
    ) -> Option<NativeMerchantReadRow> {
        let villager = match source.species() {
            Species::Villager => true,
            Species::WanderingTrader => false,
            _ => return None,
        };
        let profession = villager.then(|| match &source.extra.profession {
            ProfessionState::None => RetainedProfession::Vanilla(villager::NONE),
            ProfessionState::Vanilla(i) => RetainedProfession::Vanilla(*i),
            ProfessionState::Unresolved(id) => {
                let key = identities.get(id.as_str()).cloned().unwrap_or_else(|| {
                    let key: Arc<str> = Arc::from(id.as_str());
                    identities.insert(key.clone());
                    key
                });
                RetainedProfession::Unresolved(key)
            }
        });
        let (level, xp) = self.merchant_read_lent(source, |_, xp, level| (level.max(1), xp));
        Some(NativeMerchantReadRow {
            capture,
            entity,
            trading_player: self
                .lan_trade_conn(source.id)
                .and_then(|conn| self.lan_trade_player(conn))
                .and_then(|entity| self.player(entity)),
            profession,
            villager_type: villager
                .then(|| VILLAGER_TYPES[source.extra.variant as usize % VILLAGER_TYPES.len()]),
            level: villager.then_some(level),
            xp: villager.then_some(xp),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::*;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn borrowed_source_survives_mutation_and_materializes_owned_values() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let kind = game
                .res
                .mob_types()
                .get("minecraft:villager")
                .unwrap()
                .clone();
            game.mobs
                .push(Mob::new(40, kind, horizon_core::geometry::Vec3::ZERO, 0.0));
            let entity = game.entity(40).unwrap();
            let capture = EntityReadCapture {
                tick: NativeTickId::new(12),
                pass: 3,
                revision: 4,
                phase: EntityReadCapturePhase::BeforeCallback,
                pending_overlay: None,
            };
            let level = game.entity_level(entity).unwrap();
            let mut sources = game.mobs.lift();
            let source = &mut sources[0];
            source.no_ai = true;
            source.ai.no_action_time = 73;
            source.extra.profession = ProfessionState::Unresolved("example:engineer".into());
            source.extra.villager_level = 6;
            let mut identities = HashSet::new();
            let mob = game.capture_mob_read_row(capture, entity, level, source);
            let merchant = game
                .capture_merchant_read_row(capture, entity, source, &mut identities)
                .unwrap();
            let second = game
                .capture_merchant_read_row(capture, entity, source, &mut identities)
                .unwrap();
            match (&merchant.profession, &second.profession) {
                (
                    Some(RetainedProfession::Unresolved(a)),
                    Some(RetainedProfession::Unresolved(b)),
                ) => assert!(Arc::ptr_eq(a, b)),
                _ => panic!("unresolved profession was lost"),
            }
            source.no_ai = false;
            source.ai.no_action_time = 0;
            source.extra.profession = ProfessionState::None;
            let row = mob.materialize();
            assert_eq!(row.no_ai, Some(true));
            assert_eq!(row.no_action_time, Some(73));
            assert_eq!(row.pathfinding_malus.len(), 27);
            assert_eq!(
                row.pathfinding_malus
                    .iter()
                    .find(|(kind, _)| *kind == PathType::Blocked)
                    .unwrap()
                    .1,
                -1.0
            );
            let row = merchant.materialize();
            assert_eq!(row.profession.as_deref(), Some("example:engineer"));
            assert_eq!(row.level, Some(6));
            assert_eq!(row.villager_type.as_deref(), Some("minecraft:plains"));
            assert_eq!(identities.len(), 1);
            game.mobs.restore(
                sources,
                horizon_client::join_seq::RestoreOrder::SurvivorsFirst,
            );
        });
    }
}

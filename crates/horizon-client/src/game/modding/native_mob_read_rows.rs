//! Borrowed-source projections. Retained rows contain no per-publication owned collections.
//! Capture reads only source and publication context: no Game lookups or scans.
//! Mob work is O(PathType::COUNT); merchant work is constant except hashing an
//! unresolved profession's bytes. Interning allocates on the first distinct id
//! only, and the publisher must bound the pool lifetime (for example per world).
//! Context construction must index guests, sessions and target facts once per
//! pass. Those indexes and semantic target validation are the caller's cost.
use super::Game;
use horizon_client::mobs::{
    self, Mob, Species,
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
    pub(super) capture: EntityReadCapture,
    pub(super) entity: EntityRef,
    pub(super) is_done: Option<bool>,
    pub(super) navigation_speed: Option<f64>,
    pub(super) pathfinding_malus: [(PathType, f32); PathType::COUNT],
    pub(super) can_use_portal: Option<bool>,
    pub(super) target: Option<LivingRef>,
    pub(super) is_aggressive: Option<bool>,
    pub(super) persistence_required: Option<bool>,
    pub(super) requires_custom_persistence: Option<bool>,
    pub(super) no_ai: Option<bool>,
    pub(super) restrict_center: Option<[i32; 3]>,
    pub(super) restrict_radius: Option<i32>,
    pub(super) has_restriction: Option<bool>,
    pub(super) left_handed: Option<bool>,
    pub(super) can_pick_up_loot: Option<bool>,
    pub(super) no_action_time: Option<i32>,
    pub(super) raid: Option<RaidRef>,
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
    pub(super) capture: EntityReadCapture,
    pub(super) entity: EntityRef,
    pub(super) trading_player: Option<PlayerRef>,
    pub(super) profession: Option<RetainedProfession>,
    pub(super) villager_type: Option<&'static str>,
    pub(super) level: Option<i32>,
    pub(super) xp: Option<i32>,
}
#[derive(Clone)]
pub(super) enum RetainedProfession {
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
/// Publication-owned cross-entity facts, built once using indexed source lanes.
/// No field may be obtained by a live `Game.mobs` scan during capture.
pub(super) struct MobReadContext {
    /// Resolved `brain_target(source).unwrap_or(source.ai.target)`, validated
    /// against target source facts. For ordinary mobs use `Mob::valid_target`
    /// with a MobWorld containing the target's Seen facts (team/tame rules),
    /// or player alive/invulnerable/mode/UUID facts and difficulty.
    /// Warden instead needs alive, invulnerable, species (not Warden),
    /// player dimension/mode (guest same dimension; modes 1/3 excluded),
    /// bounds inside border (max minus 1e-5_f32),
    /// and source/target teams not allied: do not apply ordinary valid_target.
    /// Warden player invulnerability tests only motion.invulnerable, not the
    /// ordinary permanently_invulnerable/invulnerable_time union; guest fake
    /// filtering likewise follows native_warden_valid_target, not player facts.
    /// Player Who resolves through the source mob's bound player connection.
    pub target: Option<LivingRef>,
    /// Local player riding this source; authoritative guest rider presence.
    pub local_player_riding: bool,
    pub guest_riding: bool,
    pub carries_riders: bool,
    /// Raid membership resolved from source raider state and publication level.
    pub raid: Option<RaidRef>,
}
/// Guest sessions are indexed by mob id once per pass, not scanned per mob.
#[derive(Default)]
pub(super) struct MerchantReadContext<'a> {
    pub session: Option<&'a horizon_client::merchant::Session>,
    pub trading_player: Option<PlayerRef>,
}
impl Game<'_> {
    /// Identity and references are validated by the publisher; source may be lent.
    pub(super) fn capture_mob_read_row(
        &self,
        capture: EntityReadCapture,
        entity: EntityRef,
        source: &Mob,
        context: MobReadContext,
    ) -> NativeMobReadRow {
        NativeMobReadRow {
            capture,
            entity,
            is_done: Some(source.ai.nav.is_done()),
            navigation_speed: Some(source.ai.nav.speed_modifier),
            pathfinding_malus: PATH_TYPES.map(|kind| (kind, source.malus(kind))),
            can_use_portal: Some(
                mobs::mob_can_use_portal(source)
                    && source.extra.mount.is_none()
                    && !context.local_player_riding
                    && (!context.guest_riding || context.carries_riders),
            ),
            target: context.target,
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
            raid: context.raid,
        }
    }
    /// The publisher owns the identity pool, so unresolved saved ids allocate only once.
    pub(super) fn capture_merchant_read_row(
        &self,
        capture: EntityReadCapture,
        entity: EntityRef,
        source: &Mob,
        context: MerchantReadContext<'_>,
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
        // Match merchant_read_lent: unresolved professions suppress session data.
        let session = context
            .session
            .filter(|_| !villager || source.extra.profession.index().is_some());
        let level = session
            .map_or(source.extra.villager_level, |s| s.level)
            .max(1);
        let xp = session.map_or_else(
            || source.extra.trade.as_deref().map_or(0, |t| t.xp),
            |s| s.xp,
        );
        Some(NativeMerchantReadRow {
            capture,
            entity,
            trading_player: context.trading_player,
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
            let mut sources = game.mobs.lift();
            let source = &mut sources[0];
            source.no_ai = true;
            source.ai.no_action_time = 73;
            source.extra.profession = ProfessionState::Unresolved("example:engineer".into());
            source.extra.villager_level = 6;
            let mut identities = HashSet::new();
            let mob = game.capture_mob_read_row(
                capture,
                entity,
                source,
                MobReadContext {
                    // A previously resolved reference need not be resident in Game.
                    target: Some(LivingRef::new(EntityRef::new(entity.epoch(), 99))),
                    local_player_riding: false,
                    guest_riding: true,
                    carries_riders: false,
                    raid: None,
                },
            );
            let merchant = game
                .capture_merchant_read_row(
                    capture,
                    entity,
                    source,
                    MerchantReadContext::default(),
                    &mut identities,
                )
                .unwrap();
            let second = game
                .capture_merchant_read_row(
                    capture,
                    entity,
                    source,
                    MerchantReadContext::default(),
                    &mut identities,
                )
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
            assert_eq!(row.can_use_portal, Some(false));
            assert_eq!(row.no_action_time, Some(73));
            assert_eq!(row.target.unwrap().id(), 99);
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

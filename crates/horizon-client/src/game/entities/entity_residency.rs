//! One storage-family dispatch for the server's residency journal.
use super::{Game, commands::Target};
use horizon_client::join_seq::{
    Joined,
    residency::{
        Journal, Kind, Owner,
        table::{Location, SourceEntity},
    },
};
use mod_api::DimensionKey;

/// Assign a stored zero-id entity through its residency boundary exactly once.
macro_rules! assign_resident_id {
    ($list:expr, $index:expr, $entity:ident, $next:expr, $lifetime:expr) => {{
        let index = $index;
        if $list[index].id == 0 {
            let id = $list.update(index, |$entity| {
                $crate::game::entity::assign_stored_id($next, &mut $entity.id, $lifetime)
            });
            $list.id_assigned(index);
            id
        } else {
            $list[index].id
        }
    }};
}
pub(crate) use assign_resident_id;

macro_rules! assign_resident_ids {
    ($list:expr, $entity:ident, $next:expr, $lifetime:expr) => {
        for index in 0..$list.len() {
            $crate::game::entity_residency::assign_resident_id!(
                $list, index, $entity, $next, $lifetime
            );
        }
    };
}
pub(crate) use assign_resident_ids;

// Projectiles are nested in Game and direct in Parked. All other owners have
// the same field path; keep binding and resolution on this one family list.
macro_rules! resident_owners {
    ($then:ident, $this:ident, $($projectiles:ident).+) => {
        $then! {
            ($this, $($projectiles).+)
            Mob => Mob: mobs,
            Item => Item: dropped,
            Orb => Orb: orbs,
            Stand => Stand: armor_stands,
            Frame => Frame: item_frames,
            Painting => Painting: paintings,
            Boat => Boat: boats,
            Cart => Cart: carts,
            Other => Other: others,
            Fangs => Fangs: fangs.list,
            Crystal => Crystal: crystals,
            Tnt => Tnt: tnts,
            Cushion => Cushion: cushions,
            Knot => Knot: knots,
            Falling => Falling: falling,
            Rocket => Rocket: rockets,
            Trident => Trident: tridents,
            Spawner => Spawner: ominous_spawners,
            Cloud => Cloud: breath_clouds,
        }
    };
}
pub(crate) use resident_owners;

macro_rules! define_resident_targets {
    (($this:ident, $($projectiles:ident).+) $($kind:ident => $target:ident: $($field:ident).+,)*) => {
        pub(super) fn target_residency_kind(target: Target) -> Option<Kind> {
            match target {
                $(Target::$target(_) => Some(Kind::$kind),)*
                Target::Projectile(_) => Some(Kind::Projectile),
                Target::Dragon(_) => Some(Kind::Dragon),
                _ => None,
            }
        }
        pub(super) fn residency_target(kind: Kind, id: u32) -> Target {
            match kind {
                $(Kind::$kind => Target::$target(id),)*
                Kind::Projectile => Target::Projectile(id),
                Kind::Dragon => Target::Dragon(id),
            }
        }
    };
}
resident_owners!(define_resident_targets, unused, unused);

/// Canonical address mapping when an admitted payload is physically taken.
/// Session role and dragon-part identity come from the same immutable location.
pub(super) fn residency_target_at(at: Location, id: u32) -> Target {
    use horizon_client::join_seq::residency::Source;
    match at.source {
        Source::Ordinary => residency_target(at.owner.kind, id),
        Source::LocalPlayer => Target::Player,
        Source::Guest(conn) => Target::Guest(conn),
        Source::Hook { .. } => Target::Hook(id),
        Source::DragonPart(_) => Target::DragonPart(id),
    }
}

macro_rules! resident_visible {
    (Mob, $entity:expr) => {
        !$entity.removed
    };
    ($kind:ident, $entity:expr) => {
        true
    };
}
pub(crate) use resident_visible;

macro_rules! bind_resident_list {
    (Mob, $list:expr, $journal:expr, $owner:expr) => {
        $list.bind_residency_with_subtype(
            $journal,
            $owner,
            |entity| (entity.id, entity.join_seq()),
            |entity| {
                if matches!(
                    entity.species(),
                    horizon_client::mobs::Species::Villager
                        | horizon_client::mobs::Species::WanderingTrader
                ) {
                    horizon_client::join_seq::residency::EntitySubtype::Merchant
                } else {
                    horizon_client::join_seq::residency::EntitySubtype::Ordinary
                }
            },
        )
    };
    (Other, $list:expr, $journal:expr, $owner:expr) => {
        $list.bind_residency_with_subtype(
            $journal,
            $owner,
            |entity| (entity.id, entity.join_seq()),
            |entity| {
                use horizon_client::join_seq::residency::EntitySubtype;
                match entity.kind.as_str() {
                    "minecraft:block_display" => EntitySubtype::BlockDisplay,
                    "minecraft:item_display" => EntitySubtype::ItemDisplay,
                    "minecraft:text_display" => EntitySubtype::TextDisplay,
                    _ => EntitySubtype::Ordinary,
                }
            },
        )
    };
    (Cart, $list:expr, $journal:expr, $owner:expr) => {
        $list.bind_residency_with_subtype(
            $journal,
            $owner,
            |entity| (entity.id, entity.join_seq()),
            |entity| {
                use horizon_client::join_seq::residency::EntitySubtype;
                use $crate::game::minecarts::CartKind;
                match entity.kind {
                    CartKind::Chest => EntitySubtype::ChestCart,
                    CartKind::Furnace => EntitySubtype::FurnaceCart,
                    CartKind::Hopper => EntitySubtype::HopperCart,
                    CartKind::Tnt => EntitySubtype::TntCart,
                    CartKind::CommandBlock => EntitySubtype::CommandCart,
                    _ => EntitySubtype::Ordinary,
                }
            },
        )
    };
    ($kind:ident, $list:expr, $journal:expr, $owner:expr) => {
        $list.bind_residency($journal, $owner, |entity| (entity.id, entity.join_seq()))
    };
}
pub(crate) use bind_resident_list;

/// Physical lifetime access, not a cached publication or a delivery promise.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SlotAccess<R> {
    Done(R),
    Taken,
    Missing,
    UnsupportedKind(Kind),
}

macro_rules! resident_lifetime_slot {
    ($kind:ident, $list:expr, $join:expr, $visit:expr) => {{
        let Some(index) = $list.locate($join) else {
            return SlotAccess::Missing;
        };
        SlotAccess::Done($list.update(index, |entity| {
            $visit(
                horizon_mod_runtime::attachments::HasAttachmentLifetime::attachment_lifetime(
                    entity,
                ),
            )
        }))
    }};
}
pub(crate) use resident_lifetime_slot;

macro_rules! define_resident_access {
    (($this:ident, $($projectiles:ident).+) $($kind:ident => $target:ident: $($field:ident).+,)*) => {
        pub(super) fn bind_resident_owners(&mut $this, journal: &Journal, dimension: DimensionKey) {
            $($crate::game::entity_residency::bind_resident_list!($kind, $this$(.$field)+, journal, Owner { dimension, kind: Kind::$kind });)*
            $this$(.$projectiles)+.bind_residency(journal, Owner { dimension, kind: Kind::Projectile }, |entity| (entity.id, entity.join_seq()));
            $this.dragon.bind_residency(journal, Owner { dimension, kind: Kind::Dragon }, |entity| (entity.id, entity.join_seq()));
        }
        pub(super) fn locate_resident_owner(&$this, at: Location) -> Option<SourceEntity<(Target, bool)>> {
            let source = match at.owner.kind {
                $(Kind::$kind => {
                    let index = $this$(.$field)+.locate(at.join)?;
                    let entity = &$this$(.$field)+[index];
                    let visible = $crate::game::entity_residency::resident_visible!($kind, entity);
                    let id = entity.id;
                    Some(SourceEntity { id, value: (Target::$target(id), visible) })
                })*
                Kind::Projectile => {
                    let index = $this$(.$projectiles)+.locate(at.join)?;
                    let id = $this$(.$projectiles)+[index].id;
                    Some(SourceEntity { id, value: (Target::Projectile(id), true) })
                }
                Kind::Dragon => {
                    $this.dragon.locate(at.join)?;
                    let id = $this.dragon.as_ref()?.id;
                    Some(SourceEntity { id, value: (Target::Dragon(id), true) })
                }
            };
            // Logical visibility is not physical absence: filtering here would
            // incorrectly evict reversible conversion rows or report Taken.
            source.map(|mut source| {
                source.value.1 &= !$this.persist.removals.contains(&source.value.0);
                source
            })
        }
    };
}
pub(crate) use define_resident_access;

impl Game<'_> {
    resident_owners!(define_resident_access, self, creatures.projectiles);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::native_entity_refs::tests::with_game;
    use horizon_client::{
        join_seq::{RestoreOrder, residency::table::EntityLookup},
        mobs::Mob,
    };
    use horizon_core::{geometry::Vec3, position::BlockPos};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn game_residency_tracks_lift_spawn_restore_and_epoch_rebind() {
        with_game(|game| {
            game.reset_entity_residency();
            let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            game.mobs.push(Mob::new(410, kind.clone(), Vec3::ZERO, 0.0));
            let join = game.mobs[0].join_seq();
            let taken = game.mobs.lift();
            assert_eq!(game.server_resident_entity(410), EntityLookup::Taken);
            game.mobs.push(Mob::new(411, kind, Vec3::ZERO, 0.0));
            assert!(matches!(
                game.server_resident_entity(411),
                EntityLookup::Found {
                    value: Target::Mob(411),
                    ..
                }
            ));
            game.mobs.restore(taken, RestoreOrder::SurvivorsFirst);
            assert!(
                matches!(game.server_resident_entity(410), EntityLookup::Found { location, value: Target::Mob(410) } if location.join == join)
            );
            game.reset_entity_residency();
            assert!(matches!(
                game.server_resident_entity(410),
                EntityLookup::Found { .. }
            ));
            game.mobs.remove(0);
            assert_eq!(game.server_resident_entity(410), EntityLookup::Missing);
            assert!(matches!(
                game.server_resident_entity(411),
                EntityLookup::Found { .. }
            ));
            assert_eq!(game.entity_residency.get_mut().end_tick(), Ok(()));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn initial_non_overworld_owner_is_bound_before_level_notification() {
        use crate::game::Dim;
        with_game(|game| {
            game.dim = Dim::Nether;
            let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            game.mobs.push(Mob::new(410, kind, Vec3::ZERO, 0.0));
            game.native_level_opened(Dim::Nether);
            assert!(
                matches!(game.server_resident_entity(410), EntityLookup::Found { location, value: Target::Mob(410) } if location.owner.dimension == DimensionKey::NETHER)
            );
            game.native_lifecycle_world_closed();
            assert_eq!(game.server_resident_entity(410), EntityLookup::Missing);
            game.native_lifecycle_open_started();
            assert!(
                matches!(game.server_resident_entity(410), EntityLookup::Found { location, .. } if location.owner.dimension == DimensionKey::NETHER)
            );
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn parked_owners_rebind_and_route_without_trial_dimension_swaps() {
        use crate::game::Dim;
        with_game(|game| {
            game.reset_entity_residency();
            let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            game.mobs.push(Mob::new(410, kind.clone(), Vec3::ZERO, 0.0));
            game.enter_level(Dim::Nether).unwrap();
            game.mobs.push(Mob::new(411, kind, Vec3::ZERO, 0.0));
            assert!(
                matches!(game.server_resident_entity(410), EntityLookup::Found { location, .. } if location.owner.dimension == DimensionKey::OVERWORLD)
            );
            game.reset_entity_residency();
            assert!(matches!(
                game.server_resident_entity(410),
                EntityLookup::Found { .. }
            ));
            assert!(
                matches!(game.server_resident_entity(411), EntityLookup::Found { location, .. } if location.owner.dimension == DimensionKey::NETHER)
            );
            assert_eq!(game.level_entity(410), None);
            assert_eq!(game.level_entity(411), Some(Target::Mob(411)));
            assert_eq!(
                game.in_target_level(Target::Mob(410), |game| game.dim),
                Some(Dim::Overworld)
            );
            assert_eq!(game.dim, Dim::Nether);
            game.levels.clear();
            assert_eq!(game.server_resident_entity(410), EntityLookup::Missing);
            assert!(matches!(
                game.server_resident_entity(411),
                EntityLookup::Found { .. }
            ));
            assert_eq!(game.entity_residency.get_mut().end_tick(), Ok(()));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn logical_removal_is_not_lifted_absence_or_physical_retirement() {
        with_game(|game| {
            game.reset_entity_residency();
            let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            game.mobs.push(Mob::new(410, kind.clone(), Vec3::ZERO, 0.0));
            let taken = game.mobs.lift();
            game.mobs.push(Mob::new(411, kind, Vec3::ZERO, 0.0));
            game.mobs[0].removed = true;
            assert_eq!(game.server_resident_entity(411), EntityLookup::Missing);
            // Conversion can restore a resident after a pre hook. A read while
            // removed must not destroy its physical residency row.
            game.mobs[0].removed = false;
            assert!(matches!(
                game.server_resident_entity(411),
                EntityLookup::Found { .. }
            ));
            game.persist.removals.push(Target::Mob(411));
            assert_eq!(game.server_resident_entity(411), EntityLookup::Missing);
            game.persist.removals.clear();
            assert!(matches!(
                game.server_resident_entity(411),
                EntityLookup::Found { .. }
            ));
            game.mobs.restore(taken, RestoreOrder::SurvivorsFirst);
            assert_eq!(game.entity_residency.get_mut().end_tick(), Ok(()));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn late_tracking_admission_publishes_the_previously_zero_id() {
        with_game(|game| {
            game.reset_entity_residency();
            game.add_primed_tnt(BlockPos::new(0, 70, 0), 0.0, 80, None, None);
            let mut tnt = game.tnts.remove(0);
            let retired = tnt.id;
            tnt.id = 0;
            game.tnts.push(tnt);
            assert_eq!(game.server_resident_entity(retired), EntityLookup::Missing);
            assert_eq!(game.server_resident_entity(0), EntityLookup::Missing);
            game.lan_assign_ids();
            let id = game.tnts[0].id;
            assert_ne!(id, 0);
            assert!(
                matches!(game.server_resident_entity(id), EntityLookup::Found { value: Target::Tnt(found), .. } if found == id)
            );
            game.lan_assign_ids();
            assert_eq!(game.tnts[0].id, id);
            assert_eq!(game.entity_residency.get_mut().end_tick(), Ok(()));
        });
    }
}

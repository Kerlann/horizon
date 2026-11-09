//! Exact-location composition from borrowed resident storage. No scalar cache reads.
use super::api_values::TryIntoApi;
use super::{
    Game,
    native_base_read_rows::BaseReadSource,
    native_base_read_source::BaseReadOwner,
    native_entity_read_cache::{NativeReadDetails, NativeReadPublication},
    native_mob_read_context::NativeMobRelations,
};
use horizon_client::join_seq::residency::{Kind, read_pass::ReadScope, table::Location};
use mod_api::*;

/// Borrowed once by generation, including inside Parked's private storage.
pub(super) struct ResidentReadSource<'a> {
    pub(super) source: BaseReadSource<'a>,
    pub(super) id: u32,
    pub(super) persist: &'a super::persist::EntityMemory,
}
macro_rules! resident_read_access {
    (($this:ident, $($projectiles:ident).+) $($kind:ident => $target:ident: $($field:ident).+,)*) => {
        pub(super) fn resident_read_source(&$this, at: horizon_client::join_seq::residency::table::Location)
            -> Option<$crate::game::native_resident_read_publication::ResidentReadSource<'_>> {
            use horizon_client::join_seq::residency::Kind;
            use $crate::game::native_base_read_rows::BaseReadSource;
            let (source, id) = match at.owner.kind {
                $(Kind::$kind => { let i = $this$(.$field)+.locate(at.join)?;
                    let source = &$this$(.$field)+[i]; (BaseReadSource::$kind(source), source.id) },)*
                Kind::Projectile => { let i = $this$(.$projectiles)+.locate(at.join)?;
                    let source = &$this$(.$projectiles)+[i]; (BaseReadSource::Projectile(source), source.id) },
                Kind::Dragon => { $this.dragon.locate(at.join)?; let source = $this.dragon.as_ref()?;
                    (BaseReadSource::Dragon(source), source.id) },
            };
            Some($crate::game::native_resident_read_publication::ResidentReadSource { source, id, persist: &$this.persist })
        }
    };
}
pub(crate) use resident_read_access;
impl Game<'_> {
    super::entity_residency::resident_owners!(resident_read_access, self, creatures.projectiles);

    pub(super) fn publish_resident_read_location(
        &self,
        at: Location,
        phase: EntityReadCapturePhase,
    ) -> bool {
        if !self.is_logical_server() || at.owner.kind == Kind::Mob {
            return false;
        }
        let (Some(epoch), Some(tick)) = (
            self.native_lifecycle.epoch(),
            self.native_lifecycle.capture_tick(),
        ) else {
            return false;
        };
        let level = LevelRef::new(epoch, at.owner.dimension);
        let parked = if at.owner.dimension == self.dim.key() {
            None
        } else {
            let Some(parked) = self.levels.parked_of(level.dimension_id()) else {
                return false;
            };
            Some(parked)
        };
        let Some(raw) = (match parked {
            Some(p) => p.resident_read_source(at),
            None => self.resident_read_source(at),
        }) else {
            return false;
        };
        let target = super::entity_residency::residency_target(at.owner.kind, raw.id);
        if raw.id == 0 || !self.server_read_target_visible(at.owner.dimension, target) {
            return false;
        }
        let clock = self.entity_read_state.borrow().passes.clone();
        let mut scope = None;
        let current_stamp = clock.borrow_mut().stamp_active_owner(at.owner, phase);
        let capture = match current_stamp {
            Ok(Some(capture)) => capture,
            Ok(None) => {
                scope = match ReadScope::begin(clock.clone(), true, at.owner, tick) {
                    Ok(scope) => scope,
                    Err(_) => return false,
                };
                let Some(active) = &scope else {
                    return false;
                };
                match active.stamp(phase) {
                    Ok(capture) => capture,
                    Err(_) => return false,
                }
            }
            Err(_) => return false,
        };
        let entity = EntityRef::new(epoch, raw.id);
        let publication = {
            let mut state = self.entity_read_state.borrow_mut();
            let state = &mut *state;
            let pass = state.mob_relations.entry(level).or_default();
            if pass.tick != Some(tick) {
                self.rebuild_mob_read_relations(level, &mut pass.index);
                pass.tick = Some(tick);
            }
            let index = &pass.index;
            let resolve = |id| self.read_resident_identity(id);
            let player_leash = |uuid| self.resolve_mob_read_player(uuid, level, index);
            let pearl_cooldown = |id| self.pearl_portal_cooldown(id);
            let type_facts = |kind: &str| self.res.entity_type_geometry().get(kind);
            let owner = BaseReadOwner {
                level,
                epoch,
                logical_server: true,
                resolve: &resolve,
                player_leash: &player_leash,
                pearl_cooldown: &pearl_cooldown,
                type_facts: &type_facts,
                default_player_uuid: self
                    .lan_acting_uuid
                    .map(horizon_client::lan::playerdata::uuid_ints)
                    .or(self.level.player_uuid),
                local_rotation: [0.0; 2],
                local_head_yaw: 0.0,
                local_spectator: false,
                local_invisible: None,
                client_shift: None,
                player_portal_cooldown: None,
            };
            let base = match parked {
                Some(p) => p.snapshot_base_read_at(at, &mut state.kind_pool, owner),
                None => self.snapshot_base_read_at(at, &mut state.kind_pool, owner),
            };
            let Some(base) = base else {
                return false;
            };
            let Some(details) = self.resident_read_details(capture, entity, level, &raw, index)
            else {
                return false;
            };
            NativeReadPublication {
                capture,
                base,
                details,
                join_generation: at.join.get(),
                target,
                mob_target_facts: None,
            }
        };
        let published =
            self.entity_residency
                .borrow_mut()
                .publish_rows([(raw.id, at, publication)])
                == 1;
        drop(scope);
        published
    }

    fn resident_projectile_player_owner(
        &self,
        guest: Option<u32>,
        level: LevelRef,
        index: &NativeMobRelations,
    ) -> Option<EntityRef> {
        if let Some(conn) = guest {
            if self.lan_acting != Some(conn) {
                return self.read_relation_guest(conn, level, index);
            }
            let uuid = horizon_client::lan::playerdata::uuid_ints(self.lan_acting_uuid?);
            return self.resolve_mob_read_player(uuid, level, index);
        }
        self.resolve_mob_read_player(self.level.player_uuid?, level, index)
    }

    fn resident_read_details(
        &self,
        capture: EntityReadCapture,
        entity: EntityRef,
        level: LevelRef,
        raw: &ResidentReadSource<'_>,
        index: &NativeMobRelations,
    ) -> Option<NativeReadDetails> {
        let uuid = |uuid| self.resolve_read_uuid(uuid, level, index);
        Some(match raw.source {
            BaseReadSource::Stand(source) => NativeReadDetails::Living(
                self.snapshot_stand_living_read_row(capture, LivingRef::new(entity), source),
            ),
            BaseReadSource::Dragon(source) => NativeReadDetails::Dragon {
                living: self.snapshot_dragon_living_read_row(
                    capture,
                    LivingRef::new(entity),
                    source,
                )?,
                dragon: self.snapshot_dragon_read_row(
                    capture,
                    DragonRef::new(entity),
                    source,
                    (level.dimension() == DimensionKey::END && self.fight.loaded)
                        .then_some(&self.fight),
                )?,
            },
            BaseReadSource::Item(source) => {
                NativeReadDetails::Item(self.snapshot_item_entity_read_row(
                    capture,
                    ItemEntityRef::new(entity),
                    source,
                    source.thrower.and_then(uuid),
                )?)
            }
            BaseReadSource::Projectile(source) => {
                NativeReadDetails::Projectile(self.snapshot_projectile_read_row(
                    capture,
                    ProjectileRef::new(entity),
                    Some(source),
                    source.owner_uuid.and_then(uuid),
                )?)
            }
            BaseReadSource::Trident(source) => {
                NativeReadDetails::Projectile(self.snapshot_projectile_read_row(
                    capture,
                    ProjectileRef::new(entity),
                    None,
                    self.resident_projectile_player_owner(source.guest, level, index),
                )?)
            }
            BaseReadSource::Rocket(source) => {
                NativeReadDetails::Projectile(self.snapshot_projectile_read_row(
                    capture,
                    ProjectileRef::new(entity),
                    None,
                    match source.owner {
                        Some(horizon_client::mobs::Who::Mob(id)) => {
                            self.read_relation_entity(id, level, index)
                        }
                        Some(horizon_client::mobs::Who::Player) => {
                            self.resident_projectile_player_owner(source.guest, level, index)
                        }
                        None => source.owner_uuid.and_then(uuid),
                    },
                )?)
            }
            BaseReadSource::Other(source) => {
                let kind = match source.kind.as_str() {
                    "minecraft:text_display" => Some(NativeDisplayKind::Text),
                    "minecraft:item_display" => Some(NativeDisplayKind::Item),
                    "minecraft:block_display" => Some(NativeDisplayKind::Block),
                    _ => None,
                };
                if let Some(kind) = kind {
                    let empty =
                        horizon_core::nbt::Tag::compound([] as [(&str, horizon_core::nbt::Tag); 0]);
                    let tag = raw.persist.raw.get(&raw.id).unwrap_or(&empty);
                    let block = (kind == NativeDisplayKind::Block)
                        .then(|| {
                            tag.get("block_state")
                                .and_then(|t| self.state_from_nbt(t))
                                .or_else(|| {
                                    self.pack.assets.registry.default_state("minecraft:air")
                                })
                        })
                        .flatten()
                        .and_then(|state| state.try_into_api().ok());
                    NativeReadDetails::Display(self.snapshot_display_read_row(
                        capture,
                        DisplayRef::new(entity),
                        kind,
                        tag,
                        block,
                    )?)
                } else {
                    NativeReadDetails::Base
                }
            }
            _ => NativeReadDetails::Base,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Dim, commands::Target, native_entity_refs::tests::with_game};
    use super::*;
    use horizon_client::join_seq::{JoinSeq, RestoreOrder, residency::table::EntityLookup};
    use horizon_core::geometry::Vec3;

    fn location(game: &Game<'_>, id: u32) -> Location {
        let EntityLookup::Found { location, .. } = game.server_resident_entity(id) else {
            panic!("resident {id}");
        };
        location
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn every_nonmob_family_composes_from_source_and_survives_owner_lift() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            for (kind, nbt) in [
                (
                    "item",
                    "{Item:{id:\"minecraft:stone\",count:1},PickupDelay:0s}",
                ),
                ("experience_orb", "{}"),
                ("arrow", "{}"),
                ("armor_stand", "{Health:7.0f}"),
                ("item_frame", "{}"),
                ("painting", "{variant:\"minecraft:kebab\"}"),
                ("oak_boat", "{}"),
                ("minecart", "{}"),
                ("text_display", "{}"),
                ("block_display", "{block_state:{Name:\"minecraft:stone\"}}"),
                ("item_display", "{}"),
                ("evoker_fangs", "{}"),
                ("end_crystal", "{}"),
                ("ender_dragon", "{}"),
                ("tnt", "{fuse:80s}"),
                (
                    "falling_block",
                    "{BlockState:{Name:\"minecraft:sand\"},Time:1}",
                ),
                ("firework_rocket", "{LifeTime:20}"),
                ("trident", "{}"),
                (
                    "ominous_item_spawner",
                    "{item:{id:\"minecraft:stone\",count:1}}",
                ),
                ("area_effect_cloud", "{Duration:200,Radius:3.0f}"),
                ("cushion", "{}"),
            ] {
                game.summon_with_nbt(&format!("minecraft:{kind}"), [1.0, 70.0, 1.0], nbt)
                    .unwrap();
            }
            game.native_joins_skip();
            let ids = [
                game.dropped[0].id,
                game.orbs[0].id,
                game.creatures.projectiles[0].id,
                game.armor_stands[0].id,
                game.item_frames[0].id,
                game.paintings[0].id,
                game.boats[0].id,
                game.carts[0].id,
                game.others[0].id,
                game.others[1].id,
                game.others[2].id,
                game.fangs.list[0].id,
                game.crystals[0].id,
                game.dragon.as_ref().unwrap().id,
                game.tnts[0].id,
                game.falling[0].id,
                game.rockets[0].id,
                game.tridents[0].id,
                game.ominous_spawners[0].id,
                game.breath_clouds[0].id,
                game.cushions[0].id,
            ];
            let epoch = game.native_lifecycle.epoch().unwrap();
            let owner_uuid = [11, 22, 33, 44];
            game.persist
                .uuids
                .insert(game.armor_stands[0].id, owner_uuid);
            for id in ids {
                let at = location(game, id);
                assert!(
                    game.publish_resident_read_location(at, EntityReadCapturePhase::BeforeCallback),
                    "{at:?}"
                );
                let entity = EntityRef::new(epoch, id);
                let row = game.entity_cached_row(entity).unwrap();
                assert_eq!(row.position, [1.0, 70.0, 1.0]);
                assert_eq!(row.join_generation, at.join.get());
                assert_eq!(row.level.dimension(), game.dim.key());
                assert!(
                    game.with_entity_read_publication(entity, |row| Some(
                        match (&row.details, at.owner.kind) {
                            (NativeReadDetails::Living(living), Kind::Stand) =>
                                living.health == Some(7.0)
                                    && living.baby == Some(false)
                                    && living.is_climbing.is_none(),
                            (NativeReadDetails::Dragon { living, dragon }, Kind::Dragon) =>
                                living.baby == Some(false) && dragon.alive_crystals.is_none(),
                            (NativeReadDetails::Display(display), Kind::Other) =>
                                match row.base.kind.as_deref() {
                                    Some("minecraft:text_display") =>
                                        display.kind == Some(NativeDisplayKind::Text)
                                            && display.text_shadow == Some(false)
                                            && display.brightness.is_none()
                                            && display.item_context.is_none(),
                                    Some("minecraft:block_display") =>
                                        display.kind == Some(NativeDisplayKind::Block)
                                            && display.block_state.is_some()
                                            && display.text_shadow.is_none()
                                            && display.item_context.is_none(),
                                    Some("minecraft:item_display") =>
                                        display.kind == Some(NativeDisplayKind::Item)
                                            && display.item_context
                                                == Some(NativeItemDisplayContext::None)
                                            && display.text_shadow.is_none(),
                                    _ => false,
                                },
                            (NativeReadDetails::Item(item), Kind::Item) =>
                                item.pickup_delay == Some(0) && item.owner.is_none(),
                            (NativeReadDetails::Projectile(projectile), Kind::Projectile) =>
                                projectile.critical == Some(false)
                                    && projectile.pierce_level == Ok(0),
                            (
                                NativeReadDetails::Projectile(projectile),
                                Kind::Rocket | Kind::Trident,
                            ) =>
                                projectile.critical.is_none()
                                    && projectile.pierce_level == Err(PierceLevelError::NotArrow),
                            (
                                NativeReadDetails::Base,
                                Kind::Orb
                                | Kind::Frame
                                | Kind::Painting
                                | Kind::Boat
                                | Kind::Cart
                                | Kind::Fangs
                                | Kind::Crystal
                                | Kind::Tnt
                                | Kind::Falling
                                | Kind::Spawner
                                | Kind::Cloud
                                | Kind::Cushion,
                            ) => true,
                            _ => false,
                        }
                    ))
                    .unwrap()
                );
            }
            let stand = game.armor_stands[0].id;
            let at = location(game, stand);
            let entity = EntityRef::new(epoch, stand);
            game.armor_stands[0].health = 3.0;
            game.armor_stands[0].position = Vec3::new(9.0, 8.0, 7.0);
            assert_eq!(
                game.living_cached_row(LivingRef::new(entity))
                    .unwrap()
                    .health,
                Some(7.0)
            );
            assert!(
                game.publish_resident_read_location(at, EntityReadCapturePhase::AfterAppliedWrite)
            );
            assert_eq!(
                game.living_cached_row(LivingRef::new(entity))
                    .unwrap()
                    .health,
                Some(3.0)
            );
            assert_eq!(
                game.entity_cached_row(entity).unwrap().position,
                [9.0, 8.0, 7.0]
            );
            let lifted = game.armor_stands.lift();
            let item = game.dropped[0].id;
            game.dropped[0].thrower = Some(owner_uuid);
            let item_at = location(game, item);
            assert!(game.publish_resident_read_location(
                item_at,
                EntityReadCapturePhase::AfterAppliedWrite
            ));
            let item_ref = ItemEntityRef::new(EntityRef::new(epoch, item));
            assert_eq!(
                game.item_entity_cached_row(item_ref).unwrap().owner,
                Some(entity)
            );
            // The retained reverse identity is only a hint: changing the canonical
            // saved UUID must refuse the old binding, not name a different owner.
            game.persist.uuids.insert(stand, [99; 4]);
            assert!(game.publish_resident_read_location(
                item_at,
                EntityReadCapturePhase::AfterAppliedWrite
            ));
            assert_eq!(game.item_entity_cached_row(item_ref).unwrap().owner, None);
            assert_eq!(
                game.living_cached_row(LivingRef::new(entity))
                    .unwrap()
                    .health,
                Some(3.0)
            );
            assert!(
                !game.publish_resident_read_location(at, EntityReadCapturePhase::BeforeCallback)
            );
            game.armor_stands
                .restore(lifted, RestoreOrder::SurvivorsFirst);
            assert!(!game.publish_resident_read_location(
                Location {
                    join: JoinSeq::next(),
                    ..at
                },
                EntityReadCapturePhase::BeforeCallback
            ));
            game.persist.removals.push(Target::Stand(stand));
            assert!(
                !game.publish_resident_read_location(at, EntityReadCapturePhase::BeforeCallback)
            );
            assert!(game.entity_cached_row(entity).is_none());
            game.persist
                .removals
                .retain(|target| *target != Target::Stand(stand));
            game.armor_stands.clear();
            game.armor_stands
                .push(horizon_client::entities::ArmorStand::new(
                    stand,
                    Vec3::ZERO,
                    0.0,
                ));
            let replacement = location(game, stand);
            assert_ne!(replacement.join, at.join);
            assert!(game.entity_cached_row(entity).is_none());
            assert!(
                !game.publish_resident_read_location(at, EntityReadCapturePhase::BeforeCallback)
            );
            assert!(game.publish_resident_read_location(
                replacement,
                EntityReadCapturePhase::BeforeCallback
            ));
            assert_eq!(
                game.living_cached_row(LivingRef::new(entity))
                    .unwrap()
                    .health,
                Some(20.0)
            );
        });
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn parked_end_uses_its_source_geometry_and_loaded_global_fight() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            game.enter_level(Dim::End).unwrap();
            game.summon_with_nbt("minecraft:armor_stand", [23.0, 75.0, 9.0], "{Health:11.0f}")
                .unwrap();
            game.summon_with_nbt("minecraft:ender_dragon", [31.0, 80.0, 4.0], "{}")
                .unwrap();
            game.native_joins_skip();
            let stand = game.armor_stands[0].id;
            let dragon = game.dragon.as_ref().unwrap().id;
            let stand_at = location(game, stand);
            let dragon_at = location(game, dragon);
            game.enter_level(Dim::Nether).unwrap();
            game.fight.loaded = true;
            game.fight.alive_crystals = 6;
            game.fight.previously_killed = true;
            let epoch = game.native_lifecycle.epoch().unwrap();
            assert!(game.publish_resident_read_location(
                stand_at,
                EntityReadCapturePhase::AfterAppliedWrite
            ));
            assert!(game.publish_resident_read_location(
                dragon_at,
                EntityReadCapturePhase::AfterAppliedWrite
            ));
            assert_eq!(game.dim, Dim::Nether);
            let row = game
                .entity_cached_row(EntityRef::new(epoch, stand))
                .unwrap();
            assert_eq!(row.level.dimension(), DimensionKey::END);
            assert_eq!(row.position, [23.0, 75.0, 9.0]);
            assert_eq!(
                game.living_cached_row(LivingRef::new(row.entity))
                    .unwrap()
                    .health,
                Some(11.0)
            );
            let row = game
                .dragon_cached_row(DragonRef::new(EntityRef::new(epoch, dragon)))
                .unwrap();
            assert_eq!(row.alive_crystals, Some(6));
            assert_eq!(row.previously_killed, Some(true));
        });
    }
}

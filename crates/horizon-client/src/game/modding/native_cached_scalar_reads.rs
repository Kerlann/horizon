//! Scalar projections of the authoritative publication: never materialize an entire row.
//! The outer Option distinguishes no publication from a published unknown field.
//! Only the former takes the legacy resident-source fallback.
use super::api_values::TryIntoEngine;
use super::{
    Game, native_entity_read_cache::NativeReadDetails, native_mob_read_rows::RetainedProfession,
};
use horizon_client::first_person::Hand;
use horizon_world::registry::BlockStateId;
use mod_api::*;

impl Game<'_> {
    // entity
    pub(super) fn entity_custom_name_visible(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.custom_name_visible))
            .unwrap_or_else(|| self.uncached_entity_custom_name_visible(entity))
    }

    pub(super) fn entity_classification(&self, entity: EntityRef) -> Option<MobCategory> {
        self.with_entity_read_publication(entity, |row| Some(row.base.classification))
            .unwrap_or_else(|| self.uncached_entity_classification(entity))
    }

    pub(super) fn entity_added_to_level(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.added_to_level))
            .unwrap_or_else(|| self.uncached_entity_added_to_level(entity))
    }

    pub(super) fn entity_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        self.with_entity_read_publication(entity, |row| Some(Some(row.base.position)))
            .unwrap_or_else(|| self.uncached_entity_position(entity))
    }

    pub(super) fn entity_block_position(&self, entity: EntityRef) -> Option<[i32; 3]> {
        self.with_entity_read_publication(entity, |row| {
            Some(Some(row.base.position.map(|v| v.floor() as i32)))
        })
        .unwrap_or_else(|| self.uncached_entity_block_position(entity))
    }

    pub(super) fn entity_rotation(&self, entity: EntityRef) -> Option<[f32; 2]> {
        self.with_entity_read_publication(entity, |row| Some(Some(row.base.rotation)))
            .unwrap_or_else(|| self.uncached_entity_rotation(entity))
    }

    pub(super) fn entity_kind(&self, entity: EntityRef) -> Option<String> {
        self.with_entity_read_publication(entity, |row| {
            Some(row.base.kind.as_deref().map(str::to_owned))
        })
        .unwrap_or_else(|| self.uncached_entity_kind(entity))
    }

    pub(super) fn entity_level(&self, entity: EntityRef) -> Option<LevelRef> {
        self.with_entity_read_publication(entity, |row| Some(Some(row.base.level)))
            .unwrap_or_else(|| self.uncached_entity_level(entity))
    }

    pub(super) fn entity_permanently_invulnerable(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.permanently_invulnerable))
            .unwrap_or_else(|| self.uncached_entity_permanently_invulnerable(entity))
    }

    pub(super) fn entity_invulnerable_time(&self, entity: EntityRef) -> Option<i32> {
        self.with_entity_read_publication(entity, |row| Some(row.base.invulnerable_time))
            .unwrap_or_else(|| self.uncached_entity_invulnerable_time(entity))
    }

    pub(super) fn entity_damage_cooldown_time(&self, entity: EntityRef) -> Option<i32> {
        self.with_entity_read_publication(entity, |row| Some(row.base.damage_cooldown_time))
            .unwrap_or_else(|| self.uncached_entity_damage_cooldown_time(entity))
    }

    pub(super) fn entity_invulnerable_to_piercing_weapon(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| {
            Some(row.base.invulnerable_to_piercing_weapon)
        })
        .unwrap_or_else(|| self.uncached_entity_invulnerable_to_piercing_weapon(entity))
    }

    pub(super) fn entity_velocity(&self, entity: EntityRef) -> Option<[f64; 3]> {
        self.with_entity_read_publication(entity, |row| Some(Some(row.base.velocity)))
            .unwrap_or_else(|| self.uncached_entity_velocity(entity))
    }

    pub(super) fn entity_on_ground(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(Some(row.base.on_ground)))
            .unwrap_or_else(|| self.uncached_entity_on_ground(entity))
    }

    pub(super) fn entity_bounding_box(&self, entity: EntityRef) -> Option<[[f64; 3]; 2]> {
        self.with_entity_read_publication(entity, |row| Some(Some(row.base.bounding_box)))
            .unwrap_or_else(|| self.uncached_entity_bounding_box(entity))
    }

    pub(super) fn entity_eye_height(&self, entity: EntityRef) -> Option<f64> {
        self.with_entity_read_publication(entity, |row| Some(Some(row.base.eye_height)))
            .unwrap_or_else(|| self.uncached_entity_eye_height(entity))
    }

    pub(super) fn entity_eye_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        self.with_entity_read_publication(entity, |row| {
            Some(Some([
                row.base.position[0],
                row.base.position[1] + row.base.eye_height,
                row.base.position[2],
            ]))
        })
        .unwrap_or_else(|| self.uncached_entity_eye_position(entity))
    }

    pub(super) fn entity_view_vector(&self, entity: EntityRef) -> Option<[f64; 3]> {
        self.with_entity_read_publication(entity, |row| {
            Some(Some({
                let v = horizon_world::view_direction::view_vector(
                    row.base.rotation[1],
                    row.base.rotation[0],
                );
                [v.x, v.y, v.z]
            }))
        })
        .unwrap_or_else(|| self.uncached_entity_view_vector(entity))
    }

    pub(super) fn entity_up_vector(&self, entity: EntityRef) -> Option<[f64; 3]> {
        self.with_entity_read_publication(entity, |row| {
            Some(Some({
                let v = horizon_world::view_direction::view_vector(
                    row.base.rotation[1] - 90.0,
                    row.base.rotation[0],
                );
                [v.x, v.y, v.z]
            }))
        })
        .unwrap_or_else(|| self.uncached_entity_up_vector(entity))
    }

    pub(super) fn entity_is_multipart(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.is_multipart))
            .unwrap_or_else(|| self.uncached_entity_is_multipart(entity))
    }

    pub(super) fn entity_parts(&self, entity: EntityRef) -> Option<Vec<EntityRef>> {
        self.with_entity_read_publication(entity, |row| {
            Some(row.base.parts.as_ref().map(|p| p.materialize()))
        })
        .unwrap_or_else(|| self.uncached_entity_parts(entity))
    }

    pub(super) fn entity_head_yaw(&self, entity: EntityRef) -> Option<f32> {
        self.with_entity_read_publication(entity, |row| Some(row.base.head_yaw))
            .unwrap_or_else(|| self.uncached_entity_head_yaw(entity))
    }

    pub(super) fn entity_direction(&self, entity: EntityRef) -> Option<&'static str> {
        self.with_entity_read_publication(entity, |row| Some(row.base.direction))
            .unwrap_or_else(|| self.uncached_entity_direction(entity))
    }

    pub(super) fn entity_dimensions(&self, entity: EntityRef) -> Option<[f64; 2]> {
        self.with_entity_read_publication(entity, |row| {
            Some(Some([
                row.base.bounding_box[1][0] - row.base.bounding_box[0][0],
                row.base.bounding_box[1][1] - row.base.bounding_box[0][1],
            ]))
        })
        .unwrap_or_else(|| self.uncached_entity_dimensions(entity))
    }

    pub(super) fn entity_no_physics(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.no_physics))
            .unwrap_or_else(|| self.uncached_entity_no_physics(entity))
    }

    pub(super) fn entity_leash_holder(&self, entity: EntityRef) -> Option<LeashHolder> {
        self.with_entity_read_publication(entity, |row| Some(row.base.leash_holder))
            .unwrap_or_else(|| self.uncached_entity_leash_holder(entity))
    }

    pub(super) fn entity_sound_source(&self, entity: EntityRef) -> Option<SoundSource> {
        self.with_entity_read_publication(entity, |row| Some(row.base.sound_source))
            .unwrap_or_else(|| self.uncached_entity_sound_source(entity))
    }

    pub(super) fn entity_spectator(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.spectator))
            .unwrap_or_else(|| self.uncached_entity_spectator(entity))
    }

    pub(super) fn entity_is_shift_key_down(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.is_shift_key_down))
            .unwrap_or_else(|| self.uncached_entity_is_shift_key_down(entity))
    }

    pub(super) fn entity_is_crouching(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.is_crouching))
            .unwrap_or_else(|| self.uncached_entity_is_crouching(entity))
    }

    pub(super) fn entity_is_sprinting(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.is_sprinting))
            .unwrap_or_else(|| self.uncached_entity_is_sprinting(entity))
    }

    pub(super) fn entity_is_in_water(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.is_in_water))
            .unwrap_or_else(|| self.uncached_entity_is_in_water(entity))
    }

    pub(super) fn entity_is_in_lava(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.is_in_lava))
            .unwrap_or_else(|| self.uncached_entity_is_in_lava(entity))
    }

    pub(super) fn entity_fire_immune(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.fire_immune))
            .unwrap_or_else(|| self.uncached_entity_fire_immune(entity))
    }

    pub(super) fn entity_fire_ticks(&self, entity: EntityRef) -> Option<i32> {
        self.with_entity_read_publication(entity, |row| Some(row.base.fire_ticks))
            .unwrap_or_else(|| self.uncached_entity_fire_ticks(entity))
    }

    pub(super) fn entity_portal_cooldown(&self, entity: EntityRef) -> Option<i32> {
        self.with_entity_read_publication(entity, |row| Some(row.base.portal_cooldown))
            .unwrap_or_else(|| self.uncached_entity_portal_cooldown(entity))
    }

    pub(super) fn entity_air_supply(&self, entity: EntityRef) -> Option<i32> {
        self.with_entity_read_publication(entity, |row| Some(row.base.air_supply))
            .unwrap_or_else(|| self.uncached_entity_air_supply(entity))
    }

    pub(super) fn entity_max_air_supply(&self, entity: EntityRef) -> Option<i32> {
        self.with_entity_read_publication(entity, |row| Some(row.base.max_air_supply))
            .unwrap_or_else(|| self.uncached_entity_max_air_supply(entity))
    }

    pub(super) fn entity_is_silent(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.is_silent))
            .unwrap_or_else(|| self.uncached_entity_is_silent(entity))
    }

    pub(super) fn entity_glowing(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.glowing))
            .unwrap_or_else(|| self.uncached_entity_glowing(entity))
    }

    pub(super) fn entity_no_gravity(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.no_gravity))
            .unwrap_or_else(|| self.uncached_entity_no_gravity(entity))
    }

    pub(super) fn entity_ticks_frozen(&self, entity: EntityRef) -> Option<i32> {
        self.with_entity_read_publication(entity, |row| Some(row.base.ticks_frozen))
            .unwrap_or_else(|| self.uncached_entity_ticks_frozen(entity))
    }

    pub(super) fn entity_tick_count(&self, entity: EntityRef) -> Option<i32> {
        self.with_entity_read_publication(entity, |row| Some(row.base.tick_count))
            .unwrap_or_else(|| self.uncached_entity_tick_count(entity))
    }

    pub(super) fn entity_is_invisible(&self, entity: EntityRef) -> Option<bool> {
        self.with_entity_read_publication(entity, |row| Some(row.base.is_invisible))
            .unwrap_or_else(|| self.uncached_entity_is_invisible(entity))
    }

    pub(super) fn entity_pose(&self, entity: EntityRef) -> Option<&'static str> {
        self.with_entity_read_publication(entity, |row| Some(row.base.pose))
            .unwrap_or_else(|| self.uncached_entity_pose(entity))
    }

    pub(super) fn entity_distance_to_sqr(&self, entity: EntityRef, pos: [f64; 3]) -> Option<f64> {
        self.with_entity_read_publication(entity, |row| {
            Some({
                let [dx, dy, dz] = std::array::from_fn(|i| row.base.position[i] - pos[i]);
                Some(dx * dx + dy * dy + dz * dz)
            })
        })
        .unwrap_or_else(|| self.uncached_entity_distance_to_sqr(entity, pos))
    }

    // living
    pub(super) fn living_gliding_ticks(&self, living: LivingRef) -> Option<i32> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.gliding_ticks))
        })
        .unwrap_or_else(|| self.uncached_living_gliding_ticks(living))
    }

    pub(super) fn living_is_gliding(&self, living: LivingRef) -> Option<bool> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.is_gliding))
        })
        .unwrap_or_else(|| self.uncached_living_is_gliding(living))
    }

    pub(super) fn living_is_sleeping(&self, living: LivingRef) -> Option<bool> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.is_sleeping))
        })
        .unwrap_or_else(|| self.uncached_living_is_sleeping(living))
    }

    pub(super) fn living_sleeping_pos(&self, living: LivingRef) -> Option<[i32; 3]> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.sleeping_pos))
        })
        .unwrap_or_else(|| self.uncached_living_sleeping_pos(living))
    }

    pub(super) fn living_is_jumping(&self, living: LivingRef) -> Option<bool> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.is_jumping))
        })
        .unwrap_or_else(|| self.uncached_living_is_jumping(living))
    }

    pub(super) fn living_is_climbing(&self, living: LivingRef) -> Option<bool> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.is_climbing))
        })
        .unwrap_or_else(|| self.uncached_living_is_climbing(living))
    }

    pub(super) fn living_baby(&self, living: LivingRef) -> Option<bool> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.baby))
        })
        .unwrap_or_else(|| self.uncached_living_baby(living))
    }

    pub(super) fn living_health(&self, living: LivingRef) -> Option<f32> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.health))
        })
        .unwrap_or_else(|| self.uncached_living_health(living))
    }

    pub(super) fn living_max_health(&self, living: LivingRef) -> Option<f32> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.max_health))
        })
        .unwrap_or_else(|| self.uncached_living_max_health(living))
    }

    pub(super) fn living_alive(&self, living: LivingRef) -> Option<bool> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.alive))
        })
        .unwrap_or_else(|| self.uncached_living_alive(living))
    }

    pub(super) fn living_kill_credit(&self, living: LivingRef) -> Option<EntityRef> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.kill_credit))
        })
        .unwrap_or_else(|| self.uncached_living_kill_credit(living))
    }

    pub(super) fn living_last_attacker(&self, living: LivingRef) -> Option<EntityRef> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.last_attacker))
        })
        .unwrap_or_else(|| self.uncached_living_last_attacker(living))
    }

    pub(super) fn living_is_tamed(&self, living: LivingRef) -> Option<bool> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.is_tamed))
        })
        .unwrap_or_else(|| self.uncached_living_is_tamed(living))
    }

    pub(super) fn living_owner(&self, living: LivingRef) -> Option<u128> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.owner))
        })
        .unwrap_or_else(|| self.uncached_living_owner(living))
    }

    pub(super) fn living_item_use_remaining(&self, living: LivingRef) -> Option<i32> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.item_use_remaining))
        })
        .unwrap_or_else(|| self.uncached_living_item_use_remaining(living))
    }

    pub(in crate::game) fn living_used_item_hand(&self, living: LivingRef) -> Option<Hand> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.used_item_hand))
        })
        .unwrap_or_else(|| self.uncached_living_used_item_hand(living))
    }

    pub(super) fn living_can_use_portal(&self, living: LivingRef) -> Option<bool> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.can_use_portal))
        })
        .unwrap_or_else(|| self.uncached_living_can_use_portal(living))
    }

    pub(super) fn living_can_breathe_underwater(&self, living: LivingRef) -> Option<bool> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(
                row.details
                    .living()
                    .and_then(|row| row.can_breathe_underwater),
            )
        })
        .unwrap_or_else(|| self.uncached_living_can_breathe_underwater(living))
    }

    pub(super) fn living_arrow_count(&mut self, living: LivingRef) -> Option<i32> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.arrow_count))
        })
        .unwrap_or_else(|| self.uncached_living_arrow_count(living))
    }

    pub(super) fn living_stinger_count(&mut self, living: LivingRef) -> Option<i32> {
        self.with_entity_read_publication(living.entity(), |row| {
            Some(row.details.living().and_then(|row| row.stinger_count))
        })
        .unwrap_or_else(|| self.uncached_living_stinger_count(living))
    }

    // merchant
    pub(super) fn merchant_trading_player(&self, merchant: MerchantRef) -> Option<PlayerRef> {
        self.with_entity_read_publication(merchant.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob {
                    merchant: Some(merchant),
                    ..
                } => merchant.trading_player,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_merchant_trading_player(merchant))
    }

    pub(super) fn merchant_profession(&self, merchant: MerchantRef) -> Option<String> {
        self.with_entity_read_publication(merchant.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob {
                    merchant: Some(merchant),
                    ..
                } => merchant.profession.as_ref().map(|p| match p {
                    RetainedProfession::Vanilla(i) => format!(
                        "minecraft:{}",
                        horizon_client::mobs::villager::profession(*i).id
                    ),
                    RetainedProfession::Unresolved(id) => id.to_string(),
                }),
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_merchant_profession(merchant))
    }

    pub(super) fn merchant_level(&self, merchant: MerchantRef) -> Option<i32> {
        self.with_entity_read_publication(merchant.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob {
                    merchant: Some(merchant),
                    ..
                } => merchant.level,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_merchant_level(merchant))
    }

    pub(super) fn merchant_villager_type(&self, merchant: MerchantRef) -> Option<String> {
        self.with_entity_read_publication(merchant.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob {
                    merchant: Some(merchant),
                    ..
                } => merchant
                    .villager_type
                    .map(|name| format!("minecraft:{name}")),
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_merchant_villager_type(merchant))
    }

    pub(super) fn merchant_xp(&self, merchant: MerchantRef) -> Option<i32> {
        self.with_entity_read_publication(merchant.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob {
                    merchant: Some(merchant),
                    ..
                } => merchant.xp,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_merchant_xp(merchant))
    }

    // mob
    pub(super) fn mob_is_done(&self, mob: MobRef) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.is_done,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_is_done(mob))
    }

    pub(super) fn mob_navigation_speed(&self, mob: MobRef) -> Option<f64> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.navigation_speed,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_navigation_speed(mob))
    }

    pub(super) fn mob_pathfinding_malus(&self, mob: MobRef, kind: PathType) -> Option<f32> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob
                    .pathfinding_malus
                    .iter()
                    .find(|(path, _)| *path == kind)
                    .map(|(_, value)| *value),
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_pathfinding_malus(mob, kind))
    }

    pub(super) fn mob_can_use_portal(&self, mob: MobRef) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.can_use_portal,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_can_use_portal(mob))
    }

    pub(super) fn mob_target(&self, mob: MobRef) -> Option<LivingRef> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.target,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_target(mob))
    }

    pub(super) fn mob_is_aggressive(&self, mob: MobRef) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.is_aggressive,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_is_aggressive(mob))
    }

    pub(super) fn mob_persistence_required(&self, mob: MobRef) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.persistence_required,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_persistence_required(mob))
    }

    pub(super) fn mob_requires_custom_persistence(&self, mob: MobRef) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.requires_custom_persistence,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_requires_custom_persistence(mob))
    }

    pub(super) fn mob_no_ai(&self, mob: MobRef) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.no_ai,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_no_ai(mob))
    }

    pub(super) fn mob_restrict_center(&self, mob: MobRef) -> Option<[i32; 3]> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.restrict_center,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_restrict_center(mob))
    }

    pub(super) fn mob_restrict_radius(&self, mob: MobRef) -> Option<i32> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.restrict_radius,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_restrict_radius(mob))
    }

    pub(super) fn mob_has_restriction(&self, mob: MobRef) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.has_restriction,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_has_restriction(mob))
    }

    pub(super) fn mob_left_handed(&self, mob: MobRef) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.left_handed,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_left_handed(mob))
    }

    pub(super) fn mob_can_pick_up_loot(&self, mob: MobRef) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.can_pick_up_loot,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_can_pick_up_loot(mob))
    }

    pub(super) fn mob_no_action_time(&self, mob: MobRef) -> Option<i32> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.no_action_time,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_no_action_time(mob))
    }

    pub(super) fn mob_raid(&self, mob: MobRef) -> Option<RaidRef> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => mob.raid,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_raid(mob))
    }

    pub(super) fn mob_is_within_restriction(&self, mob: MobRef, at: [i32; 3]) -> Option<bool> {
        self.with_entity_read_publication(mob.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Mob { mob, .. } => match mob.has_restriction {
                    Some(false) => Some(true),
                    Some(true) => {
                        mob.restrict_center
                            .zip(mob.restrict_radius)
                            .map(|(center, radius)| {
                                horizon_client::mobs::goals::within_home(center, radius, at)
                            })
                    }
                    None => None,
                },
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_mob_is_within_restriction(mob, at))
    }

    // player
    pub(super) fn player_selected_slot(&self, player: PlayerRef) -> Option<usize> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.selected_slot,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_selected_slot(player))
    }

    pub(super) fn player_container_menu(&self, player: PlayerRef) -> Option<MenuRef> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.container_menu,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_container_menu(player))
    }

    pub(super) fn player_exp_cooldown(&self, player: PlayerRef) -> Option<u32> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.exp_cooldown,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_exp_cooldown(player))
    }

    pub(super) fn player_last_death_location(&self, player: PlayerRef) -> Option<GlobalPos> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.last_death_location.clone(),
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_last_death_location(player))
    }

    pub(super) fn player_needs_food(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.needs_food,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_needs_food(player))
    }

    pub(super) fn player_has_enough_food(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.has_enough_food,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_has_enough_food(player))
    }

    pub(super) fn player_can_eat(&self, player: PlayerRef, always: bool) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => Some(snapshot.0.can_eat[usize::from(always)]),
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_can_eat(player, always))
    }

    pub(super) fn player_experience(&self, player: PlayerRef) -> Option<NativePlayerExperience> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.experience,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_experience(player))
    }

    pub(super) fn player_xp_needed_for_next_level(&self, player: PlayerRef) -> Option<i32> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.xp_needed_for_next_level,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_xp_needed_for_next_level(player))
    }

    pub(super) fn player_permission_level(&self, player: PlayerRef) -> Option<u8> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.permission_level,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_permission_level(player))
    }

    pub(super) fn player_has_infinite_materials(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.has_infinite_materials,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_has_infinite_materials(player))
    }

    pub(super) fn player_can_use_game_master_blocks(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.can_use_game_master_blocks,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_can_use_game_master_blocks(player))
    }

    pub(super) fn player_previous_game_mode(&self, player: PlayerRef) -> Option<Option<GameMode>> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.previous_game_mode,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_previous_game_mode(player))
    }

    pub(super) fn player_is_secondary_use_active(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.is_secondary_use_active,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_is_secondary_use_active(player))
    }

    pub(super) fn player_is_fake(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.is_fake,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_is_fake(player))
    }

    pub(super) fn player_respawn_config(
        &self,
        player: PlayerRef,
    ) -> Option<Option<PlayerRespawnConfig>> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.respawn_config.clone(),
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_respawn_config(player))
    }

    pub(super) fn player_text_filtering(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.text_filtering,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_text_filtering(player))
    }

    pub(super) fn player_sleep_ignored(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.sleep_ignored,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_sleep_ignored(player))
    }

    pub(super) fn player_counts_for_sleep(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.counts_for_sleep,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_counts_for_sleep(player))
    }

    pub(super) fn player_sleep_timer(&self, player: PlayerRef) -> Option<u32> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.sleep_timer,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_sleep_timer(player))
    }

    pub(super) fn player_game_mode(&self, player: PlayerRef) -> Option<GameMode> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.game_mode,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_game_mode(player))
    }

    pub(super) fn player_is_creative(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.is_creative,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_is_creative(player))
    }

    pub(super) fn player_is_spectator(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.is_spectator,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_is_spectator(player))
    }

    pub(super) fn player_may_build(&self, player: PlayerRef) -> Option<bool> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.may_build,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_may_build(player))
    }

    pub(super) fn player_food(&self, player: PlayerRef) -> Option<NativePlayerFood> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.food,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_food(player))
    }

    pub(super) fn player_abilities(&self, player: PlayerRef) -> Option<NativePlayerAbilities> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.abilities,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_abilities(player))
    }

    pub(super) fn player_block_breaking(&self, player: PlayerRef) -> Option<BlockBreakingState> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.block_breaking,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_block_breaking(player))
    }

    pub(super) fn player_ambient_mood(&self, player: PlayerRef) -> Option<f32> {
        self.with_entity_read_publication(player.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Player {
                    player: snapshot, ..
                } if snapshot.1 == player => snapshot.0.ambient_mood,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_player_ambient_mood(player))
    }


}

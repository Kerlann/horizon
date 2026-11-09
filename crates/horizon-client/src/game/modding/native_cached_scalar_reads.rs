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


}

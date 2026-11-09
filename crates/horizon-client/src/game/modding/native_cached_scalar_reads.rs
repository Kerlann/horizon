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
    // display
    pub(super) fn display_kind(&self, display: DisplayRef) -> Option<NativeDisplayKind> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.kind,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_kind(display))
    }

    pub(super) fn display_transformation(
        &self,
        display: DisplayRef,
    ) -> Option<NativeTransformation> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.transformation,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_transformation(display))
    }

    pub(super) fn display_billboard(&self, display: DisplayRef) -> Option<NativeBillboard> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.billboard,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_billboard(display))
    }

    pub(super) fn display_brightness(&self, display: DisplayRef) -> Option<(u8, u8)> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.brightness,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_brightness(display))
    }

    pub(super) fn display_view_range(&self, display: DisplayRef) -> Option<f32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.view_range,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_view_range(display))
    }

    pub(super) fn display_shadow_radius(&self, display: DisplayRef) -> Option<f32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.shadow_radius,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_shadow_radius(display))
    }

    pub(super) fn display_shadow_strength(&self, display: DisplayRef) -> Option<f32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.shadow_strength,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_shadow_strength(display))
    }

    pub(super) fn display_width(&self, display: DisplayRef) -> Option<f32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.width,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_width(display))
    }

    pub(super) fn display_height(&self, display: DisplayRef) -> Option<f32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.height,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_height(display))
    }

    pub(super) fn display_glow_color_override(&self, display: DisplayRef) -> Option<i32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.glow_color_override,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_glow_color_override(display))
    }

    pub(super) fn display_interpolation_duration(&self, display: DisplayRef) -> Option<i32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.interpolation_duration,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_interpolation_duration(display))
    }

    pub(super) fn display_start_interpolation(&self, display: DisplayRef) -> Option<i32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.start_interpolation,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_start_interpolation(display))
    }

    pub(super) fn display_teleport_duration(&self, display: DisplayRef) -> Option<i32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.teleport_duration,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_teleport_duration(display))
    }

    pub(super) fn display_line_width(&self, display: DisplayRef) -> Option<i32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.line_width,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_line_width(display))
    }

    pub(super) fn display_background(&self, display: DisplayRef) -> Option<i32> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.background,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_background(display))
    }

    pub(super) fn display_text_opacity(&self, display: DisplayRef) -> Option<i8> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.text_opacity,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_text_opacity(display))
    }

    pub(super) fn display_text_shadow(&self, display: DisplayRef) -> Option<bool> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.text_shadow,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_text_shadow(display))
    }

    pub(super) fn display_see_through(&self, display: DisplayRef) -> Option<bool> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.see_through,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_see_through(display))
    }

    pub(super) fn display_default_background(&self, display: DisplayRef) -> Option<bool> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.default_background,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_default_background(display))
    }

    pub(super) fn display_alignment(&self, display: DisplayRef) -> Option<NativeTextAlignment> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.alignment,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_alignment(display))
    }

    pub(super) fn display_block_state(&self, display: DisplayRef) -> Option<BlockStateId> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row
                    .block_state
                    .and_then(|state| state.try_into_engine(&()).ok()),
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_block_state(display))
    }

    pub(super) fn display_item_context(
        &self,
        display: DisplayRef,
    ) -> Option<NativeItemDisplayContext> {
        self.with_entity_read_publication(display.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Display(row) => row.item_context,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_display_item_context(display))
    }

    // dragon
    pub(super) fn dragon_phase(&self, dragon: DragonRef) -> Option<NativeDragonPhase> {
        self.with_entity_read_publication(dragon.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Dragon { dragon: row, .. } => row.phase,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_dragon_phase(dragon))
    }

    pub(super) fn dragon_alive_crystals(&self, dragon: DragonRef) -> Option<i32> {
        self.with_entity_read_publication(dragon.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Dragon { dragon: row, .. } => row.alive_crystals,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_dragon_alive_crystals(dragon))
    }

    pub(super) fn dragon_previously_killed(&self, dragon: DragonRef) -> Option<bool> {
        self.with_entity_read_publication(dragon.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Dragon { dragon: row, .. } => row.previously_killed,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_dragon_previously_killed(dragon))
    }

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

    // fishing_hook
    pub(super) fn fishing_hook_player_owner(&self, hook: FishingHookRef) -> Option<EntityRef> {
        self.with_entity_read_publication(hook.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Hook(row) => row.player_owner,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_fishing_hook_player_owner(hook))
    }

    pub(super) fn fishing_hook_hooked_in(&self, hook: FishingHookRef) -> Option<EntityRef> {
        self.with_entity_read_publication(hook.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Hook(row) => row.hooked_in,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_fishing_hook_hooked_in(hook))
    }

    pub(super) fn fishing_hook_open_water(&self, hook: FishingHookRef) -> Option<bool> {
        self.with_entity_read_publication(hook.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Hook(row) => row.open_water,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_fishing_hook_open_water(hook))
    }

    // item_entity
    pub(super) fn item_entity_pickup_delay(&self, item: ItemEntityRef) -> Option<i32> {
        self.with_entity_read_publication(item.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Item(row) => row.pickup_delay,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_item_entity_pickup_delay(item))
    }

    pub(super) fn item_entity_age(&self, item: ItemEntityRef) -> Option<i32> {
        self.with_entity_read_publication(item.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Item(row) => row.age,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_item_entity_age(item))
    }

    pub(super) fn item_entity_target(&self, item: ItemEntityRef) -> Option<u128> {
        self.with_entity_read_publication(item.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Item(row) => row.target,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_item_entity_target(item))
    }

    pub(super) fn item_entity_owner(&self, item: ItemEntityRef) -> Option<EntityRef> {
        self.with_entity_read_publication(item.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Item(row) => row.owner,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_item_entity_owner(item))
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

    // projectile
    pub(super) fn projectile_owner(&self, projectile: ProjectileRef) -> Option<EntityRef> {
        self.with_entity_read_publication(projectile.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Projectile(row) => row.owner,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_projectile_owner(projectile))
    }

    pub(super) fn projectile_pickup(&self, projectile: ProjectileRef) -> Option<NativeArrowPickup> {
        self.with_entity_read_publication(projectile.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Projectile(row) => row.pickup,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_projectile_pickup(projectile))
    }

    pub(super) fn projectile_pierce_level(
        &self,
        projectile: ProjectileRef,
    ) -> Result<i8, PierceLevelError> {
        self.with_entity_read_publication(projectile.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Projectile(row) => row.pierce_level,
                _ => Err(PierceLevelError::Stale),
            })
        })
        .unwrap_or_else(|| self.uncached_projectile_pierce_level(projectile))
    }

    pub(super) fn projectile_base_damage(&self, projectile: ProjectileRef) -> Option<f64> {
        self.with_entity_read_publication(projectile.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Projectile(row) => row.base_damage,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_projectile_base_damage(projectile))
    }

    pub(super) fn projectile_critical(&self, projectile: ProjectileRef) -> Option<bool> {
        self.with_entity_read_publication(projectile.entity(), |row| {
            Some(match &row.details {
                NativeReadDetails::Projectile(row) => row.critical,
                _ => None,
            })
        })
        .unwrap_or_else(|| self.uncached_projectile_critical(projectile))
    }

    // raid
    pub(super) fn raid_center(&self, raid: RaidRef) -> Option<[i32; 3]> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.center);
        cached.unwrap_or_else(|| self.uncached_raid_center(raid))
    }

    pub(super) fn raid_omen_level(&self, raid: RaidRef) -> Option<i32> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.omen_level);
        cached.unwrap_or_else(|| self.uncached_raid_omen_level(raid))
    }

    pub(super) fn raid_groups_spawned(&self, raid: RaidRef) -> Option<i32> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.groups_spawned);
        cached.unwrap_or_else(|| self.uncached_raid_groups_spawned(raid))
    }

    pub(super) fn raid_num_groups(&self, raid: RaidRef) -> Option<i32> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.num_groups);
        cached.unwrap_or_else(|| self.uncached_raid_num_groups(raid))
    }

    pub(super) fn raid_has_bonus_wave(&self, raid: RaidRef) -> Option<bool> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.has_bonus_wave);
        cached.unwrap_or_else(|| self.uncached_raid_has_bonus_wave(raid))
    }

    pub(super) fn raid_is_started(&self, raid: RaidRef) -> Option<bool> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.is_started);
        cached.unwrap_or_else(|| self.uncached_raid_is_started(raid))
    }

    pub(super) fn raid_is_active(&self, raid: RaidRef) -> Option<bool> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.is_active);
        cached.unwrap_or_else(|| self.uncached_raid_is_active(raid))
    }

    pub(super) fn raid_status(&self, raid: RaidRef) -> Option<NativeRaidStatus> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.status);
        cached.unwrap_or_else(|| self.uncached_raid_status(raid))
    }

    pub(super) fn raid_total_health(&self, raid: RaidRef) -> Option<f32> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.total_health);
        cached.unwrap_or_else(|| self.uncached_raid_total_health(raid))
    }

    pub(super) fn raid_living_health(&self, raid: RaidRef) -> Option<f32> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.living_health);
        cached.unwrap_or_else(|| self.uncached_raid_living_health(raid))
    }

    pub(super) fn raid_bar_progress(&self, raid: RaidRef) -> Option<f32> {
        if self.native_lifecycle.epoch() != Some(raid.level().epoch()) {
            return None;
        }
        let cached = self
            .entity_read_state
            .borrow()
            .raids
            .get(&raid)
            .map(|row| row.bar_progress);
        cached.unwrap_or_else(|| self.uncached_raid_bar_progress(raid))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        commands::Target,
        native_base_read_rows::{
            BaseReadContext, BaseReadGeometry, BaseReadSource, CompactEntityReadRow,
        },
        native_entity_read_cache::NativeReadPublication,
        native_entity_refs::tests::with_game,
    };
    use super::*;
    use horizon_client::{join_seq::residency::table::EntityLookup, mobs::Mob};
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn cached_scalars_keep_unknown_false_geometry_and_lifted_identity() {
        with_game(|game| {
            game.reset_entity_residency();
            let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
            game.mobs.push(Mob::new(410, kind, Vec3::ZERO, 0.0));
            let EntityLookup::Found { location, .. } = game.server_resident_entity(410) else {
                panic!("admitted mob");
            };
            let epoch = game.native_lifecycle.epoch().unwrap();
            let entity = EntityRef::new(epoch, 410);
            let level = LevelRef::new(epoch, game.dim.key());
            let mut base = CompactEntityReadRow::capture(
                BaseReadSource::Mob(&game.mobs[0]),
                BaseReadGeometry {
                    position: [-0.5, 12.0, 3.0],
                    rotation: [0.0, 0.0],
                    velocity: [0.0; 3],
                    on_ground: false,
                    bounding_box: [[-1.0, 12.0, 2.5], [0.0, 14.0, 3.5]],
                    eye_height: 1.5,
                },
                BaseReadContext {
                    level,
                    logical_server: true,
                    kind: Arc::from("minecraft:pig"),
                    saved: None,
                    added_to_level: Some(true),
                    portal_cooldown: None,
                    leash_holder: None,
                    parts: None,
                    head_yaw: 0.0,
                    spectator: false,
                    invisible: None,
                    client_shift: None,
                },
            );
            // The physical pig has a known name-visibility flag. A publication's
            // unknown must not be replaced by that live resident value.
            assert!(game.uncached_entity_custom_name_visible(entity).is_some());
            base.custom_name_visible = None;
            base.is_sprinting = Some(false);
            let publication = NativeReadPublication {
                capture: EntityReadCapture {
                    tick: NativeTickId::new(1),
                    pass: 2,
                    revision: 0,
                    phase: EntityReadCapturePhase::PassStart,
                    pending_overlay: None,
                },
                base,
                details: NativeReadDetails::Base,
                join_generation: 1,
                target: Target::Mob(410),
                mob_target_facts: None,
            };
            assert_eq!(
                game.entity_residency
                    .borrow_mut()
                    .publish_rows([(410, location, publication)]),
                1
            );
            assert_eq!(game.entity_custom_name_visible(entity), None);
            assert_eq!(game.entity_is_sprinting(entity), Some(false));
            assert_eq!(game.entity_position(entity), Some([-0.5, 12.0, 3.0]));
            assert_eq!(game.entity_block_position(entity), Some([-1, 12, 3]));
            assert_eq!(game.entity_eye_position(entity), Some([-0.5, 13.5, 3.0]));
            assert_eq!(
                game.entity_distance_to_sqr(entity, [-0.5, 12.0, 6.0]),
                Some(9.0)
            );
            // A published subkind absence also must not fall back to a resident
            // living projection, even though the physical pig has health.
            assert_eq!(game.living_health(LivingRef::new(entity)), None);
            let lifted = game.mobs.lift();
            assert_eq!(game.uncached_entity_position(entity), None);
            assert_eq!(game.entity_position(entity), Some([-0.5, 12.0, 3.0]));
            assert_eq!(game.entity_is_sprinting(entity), Some(false));
            assert_eq!(
                game.entity_position(EntityRef::new(NativeWorldEpoch::new(epoch.get() + 1), 410)),
                None
            );
            game.mobs.restore(
                lifted,
                horizon_client::join_seq::RestoreOrder::SurvivorsFirst,
            );
            game.mobs.remove(0);
            assert_eq!(game.entity_position(entity), None);
        });
    }
}

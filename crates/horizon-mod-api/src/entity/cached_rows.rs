//! Owned cached row payloads, materialized only when requested.
//! Field absence denotes unsupported subkind, unknown client data or the
//! documented domain absence, never a foreign live-read Lifted refusal.
use crate::*;

/// Cached DisplayRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct DisplayReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Captured `DisplayRef::kind` value. None means an unsupported subkind or genuinely unknown client data.
    pub kind: Option<DisplayKind>,
    /// Captured `DisplayRef::transformation` value. None means an unsupported subkind or genuinely unknown client data.
    pub transformation: Option<Transformation>,
    /// Captured `DisplayRef::billboard` value. None means an unsupported subkind or genuinely unknown client data.
    pub billboard: Option<Billboard>,
    /// Captured `DisplayRef::brightness` value. None means no brightness override.
    pub brightness: Option<(u8, u8)>,
    /// Captured `DisplayRef::view_range` value. None means an unsupported subkind or genuinely unknown client data.
    pub view_range: Option<f32>,
    /// Captured `DisplayRef::shadow_radius` value. None means an unsupported subkind or genuinely unknown client data.
    pub shadow_radius: Option<f32>,
    /// Captured `DisplayRef::shadow_strength` value. None means an unsupported subkind or genuinely unknown client data.
    pub shadow_strength: Option<f32>,
    /// Captured `DisplayRef::width` value. None means an unsupported subkind or genuinely unknown client data.
    pub width: Option<f32>,
    /// Captured `DisplayRef::height` value. None means an unsupported subkind or genuinely unknown client data.
    pub height: Option<f32>,
    /// Captured `DisplayRef::glow_color_override` value. None means an unsupported subkind or genuinely unknown client data.
    pub glow_color_override: Option<i32>,
    /// Captured `DisplayRef::interpolation_duration` value. None means an unsupported subkind or genuinely unknown client data.
    pub interpolation_duration: Option<i32>,
    /// Captured `DisplayRef::start_interpolation` value. None means an unsupported subkind or genuinely unknown client data.
    pub start_interpolation: Option<i32>,
    /// Captured `DisplayRef::teleport_duration` value. None means an unsupported subkind or genuinely unknown client data.
    pub teleport_duration: Option<i32>,
    /// Captured `DisplayRef::line_width` value. None means an unsupported subkind or genuinely unknown client data.
    pub line_width: Option<i32>,
    /// Captured `DisplayRef::background` value. None means an unsupported subkind or genuinely unknown client data.
    pub background: Option<i32>,
    /// Captured `DisplayRef::text_opacity` value. None means an unsupported subkind or genuinely unknown client data.
    pub text_opacity: Option<i8>,
    /// Captured `DisplayRef::text_shadow` value. None means an unsupported subkind or genuinely unknown client data.
    pub text_shadow: Option<bool>,
    /// Captured `DisplayRef::see_through` value. None means an unsupported subkind or genuinely unknown client data.
    pub see_through: Option<bool>,
    /// Captured `DisplayRef::default_background` value. None means an unsupported subkind or genuinely unknown client data.
    pub default_background: Option<bool>,
    /// Captured `DisplayRef::alignment` value. None means an unsupported subkind or genuinely unknown client data.
    pub alignment: Option<TextAlignment>,
    /// Captured `DisplayRef::block_state` value. None means a non-block display.
    pub block_state: Option<BlockState>,
    /// Captured `DisplayRef::item_context` value. None means a non-item display.
    pub item_context: Option<ItemDisplayContext>,
}

/// Cached DragonRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct DragonReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Captured `DragonRef::phase` value. None means an unsupported subkind or genuinely unknown client data.
    pub phase: Option<DragonPhase>,
    /// Captured `DragonRef::alive_crystals` value. None means an unsupported subkind or genuinely unknown client data.
    pub alive_crystals: Option<i32>,
    /// Captured `DragonRef::previously_killed` value. None means an unsupported subkind or genuinely unknown client data.
    pub previously_killed: Option<bool>,
}

/// Cached EntityRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct EntityReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Generation of the validated owner join; removal does not create a new identity.
    pub join_generation: u64,
    /// Captured `EntityRef::custom_name_visible` value. None means an unsupported subkind or genuinely unknown client data.
    pub custom_name_visible: Option<bool>,
    /// Captured `EntityRef::classification` value. None means an unsupported subkind or genuinely unknown client data.
    pub classification: Option<MobCategory>,
    /// Captured `EntityRef::added_to_level` value. None means an unsupported subkind or genuinely unknown client data.
    pub added_to_level: Option<bool>,
    /// Captured `EntityRef::position` value.
    pub position: [f64; 3],
    /// Captured `EntityRef::block_position` value.
    pub block_position: [i32; 3],
    /// Captured `EntityRef::rotation` value.
    pub rotation: [f32; 2],
    /// Captured `EntityRef::kind` value. None means an unsupported subkind or genuinely unknown client data.
    pub kind: Option<String>,
    /// Captured `EntityRef::level` value.
    pub level: LevelRef,
    /// Captured `EntityRef::permanently_invulnerable` value. None means an unsupported subkind or genuinely unknown client data.
    pub permanently_invulnerable: Option<bool>,
    /// Captured `EntityRef::invulnerable_time` value. None means an unsupported subkind or genuinely unknown client data.
    pub invulnerable_time: Option<i32>,
    /// Captured `EntityRef::damage_cooldown_time` value. None means an unsupported subkind or genuinely unknown client data.
    pub damage_cooldown_time: Option<i32>,
    /// Captured `EntityRef::invulnerable_to_piercing_weapon` value. None means an unsupported subkind or genuinely unknown client data.
    pub invulnerable_to_piercing_weapon: Option<bool>,
    /// Captured `EntityRef::velocity` value.
    pub velocity: [f64; 3],
    /// Captured `EntityRef::on_ground` value.
    pub on_ground: bool,
    /// Captured `EntityRef::bounding_box` value.
    pub bounding_box: [[f64; 3]; 2],
    /// Captured `EntityRef::eye_height` value.
    pub eye_height: f64,
    /// Captured `EntityRef::eye_position` value.
    pub eye_position: [f64; 3],
    /// Captured `EntityRef::view_vector` value.
    pub view_vector: [f64; 3],
    /// Captured `EntityRef::up_vector` value.
    pub up_vector: [f64; 3],
    /// Captured `EntityRef::is_multipart` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_multipart: Option<bool>,
    /// Captured `EntityRef::parts` value. None means an unsupported subkind or genuinely unknown client data.
    pub parts: Option<Vec<EntityRef>>,
    /// Captured `EntityRef::head_yaw` value. None means an unsupported subkind or genuinely unknown client data.
    pub head_yaw: Option<f32>,
    /// Captured `EntityRef::direction` value. None means an unsupported subkind or genuinely unknown client data.
    pub direction: Option<String>,
    /// Captured `EntityRef::dimensions` value.
    pub dimensions: [f64; 2],
    /// Captured `EntityRef::no_physics` value. None means an unsupported subkind or genuinely unknown client data.
    pub no_physics: Option<bool>,
    /// Captured `EntityRef::leash_holder` value. None means an unsupported lead holder or unknown saved-holder data.
    pub leash_holder: Option<LeashHolder>,
    /// Captured `EntityRef::sound_source` value. None means an unsupported subkind or genuinely unknown client data.
    pub sound_source: Option<SoundSource>,
    /// Captured `EntityRef::spectator` value. None means an unsupported subkind or genuinely unknown client data.
    pub spectator: Option<bool>,
    /// Captured `EntityRef::is_shift_key_down` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_shift_key_down: Option<bool>,
    /// Captured `EntityRef::is_crouching` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_crouching: Option<bool>,
    /// Captured `EntityRef::is_sprinting` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_sprinting: Option<bool>,
    /// Captured `EntityRef::is_in_water` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_in_water: Option<bool>,
    /// Captured `EntityRef::is_in_lava` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_in_lava: Option<bool>,
    /// Captured `EntityRef::fire_immune` value. None means an unsupported subkind or genuinely unknown client data.
    pub fire_immune: Option<bool>,
    /// Captured `EntityRef::fire_ticks` value. None means an unsupported subkind or genuinely unknown client data.
    pub fire_ticks: Option<i32>,
    /// Captured `EntityRef::portal_cooldown` value. None means an unsupported subkind or genuinely unknown client data.
    pub portal_cooldown: Option<i32>,
    /// Captured `EntityRef::air_supply` value. None means an unsupported subkind or genuinely unknown client data.
    pub air_supply: Option<i32>,
    /// Captured `EntityRef::max_air_supply` value. None means an unsupported subkind or genuinely unknown client data.
    pub max_air_supply: Option<i32>,
    /// Captured `EntityRef::is_silent` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_silent: Option<bool>,
    /// Captured `EntityRef::glowing` value. None means an unsupported subkind or genuinely unknown client data.
    pub glowing: Option<bool>,
    /// Captured `EntityRef::no_gravity` value. None means an unsupported subkind or genuinely unknown client data.
    pub no_gravity: Option<bool>,
    /// Captured `EntityRef::ticks_frozen` value. None means an unsupported subkind or genuinely unknown client data.
    pub ticks_frozen: Option<i32>,
    /// Captured `EntityRef::tick_count` value. None means an unsupported subkind or genuinely unknown client data.
    pub tick_count: Option<i32>,
    /// Captured `EntityRef::is_invisible` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_invisible: Option<bool>,
    /// Captured `EntityRef::pose` value. None means an unsupported subkind or genuinely unknown client data.
    pub pose: Option<String>,
}

/// Cached FishingHookRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct FishingHookReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Captured `FishingHookRef::player_owner` value. None means an unsupported subkind or genuinely unknown client data.
    pub player_owner: Option<EntityRef>,
    /// Captured `FishingHookRef::hooked_in` value. None means no hooked entity or unknown client data.
    pub hooked_in: Option<EntityRef>,
    /// Captured `FishingHookRef::open_water` value. None means an unsupported subkind or genuinely unknown client data.
    pub open_water: Option<bool>,
}

/// Cached ItemEntityRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct ItemEntityReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Captured `ItemEntityRef::pickup_delay` value. None also represents a stored delay outside i32, as in the scalar read.
    pub pickup_delay: Option<i32>,
    /// Captured `ItemEntityRef::age` value. None means an unsupported subkind or genuinely unknown client data.
    pub age: Option<i32>,
    /// Captured `ItemEntityRef::target` value. None means no target or unknown client data.
    pub target: Option<u128>,
    /// Captured `ItemEntityRef::owner` value. None means no owner, unsupported ownership, or unknown client data.
    pub owner: Option<EntityRef>,
}

/// Cached LivingRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct LivingReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Captured `LivingRef::gliding_ticks` value. None means an unsupported subkind or genuinely unknown client data.
    pub gliding_ticks: Option<i32>,
    /// Captured `LivingRef::is_gliding` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_gliding: Option<bool>,
    /// Captured `LivingRef::is_sleeping` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_sleeping: Option<bool>,
    /// Captured `LivingRef::sleeping_pos` value. None means no sleeping bed.
    pub sleeping_pos: Option<[i32; 3]>,
    /// Captured `LivingRef::is_jumping` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_jumping: Option<bool>,
    /// Captured `LivingRef::is_climbing` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_climbing: Option<bool>,
    /// Captured `LivingRef::baby` value. None means an unsupported subkind or genuinely unknown client data.
    pub baby: Option<bool>,
    /// Captured `LivingRef::health` value. None means an unsupported subkind or genuinely unknown client data.
    pub health: Option<f32>,
    /// Captured `LivingRef::max_health` value. None means an unsupported subkind or genuinely unknown client data.
    pub max_health: Option<f32>,
    /// Captured `LivingRef::alive` value. None means an unsupported subkind or genuinely unknown client data.
    pub alive: Option<bool>,
    /// Captured `LivingRef::kill_credit` value. None means no credited entity.
    pub kill_credit: Option<EntityRef>,
    /// Captured `LivingRef::last_attacker` value. None means no recorded attacker.
    pub last_attacker: Option<EntityRef>,
    /// Captured `LivingRef::is_tamed` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_tamed: Option<bool>,
    /// Captured `LivingRef::owner` value. None means no owner, unsupported ownership, or unknown client data.
    pub owner: Option<u128>,
    /// Captured `LivingRef::item_use_remaining` value. None means an unsupported subkind or genuinely unknown client data.
    pub item_use_remaining: Option<i32>,
    /// Captured `LivingRef::used_item_hand` value. None means an unsupported subkind or genuinely unknown client data.
    pub used_item_hand: Option<Hand>,
    /// Captured `LivingRef::can_use_portal` value. None means an unsupported subkind or genuinely unknown client data.
    pub can_use_portal: Option<bool>,
    /// Captured `LivingRef::can_breathe_underwater` value. None means an unsupported subkind or genuinely unknown client data.
    pub can_breathe_underwater: Option<bool>,
    /// Captured `LivingRef::arrow_count` value. None means an unsupported subkind or genuinely unknown client data.
    pub arrow_count: Option<i32>,
    /// Captured `LivingRef::stinger_count` value. None means an unsupported subkind or genuinely unknown client data.
    pub stinger_count: Option<i32>,
}

/// Cached MerchantRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct MerchantReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Captured `MerchantRef::trading_player` value. None means no trading player.
    pub trading_player: Option<PlayerRef>,
    /// Captured `MerchantRef::profession` value. None means a wandering trader, not a villager profession.
    pub profession: Option<String>,
    /// Captured `MerchantRef::level` value. None means an unsupported subkind or genuinely unknown client data.
    pub level: Option<i32>,
    /// Captured `MerchantRef::villager_type` value. None means a wandering trader has no villager type.
    pub villager_type: Option<String>,
    /// Captured `MerchantRef::xp` value. None means an unsupported subkind or genuinely unknown client data.
    pub xp: Option<i32>,
}

/// Cached MobRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct MobReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Captured `MobRef::is_done` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_done: Option<bool>,
    /// Captured `MobRef::navigation_speed` value. None means an unsupported subkind or genuinely unknown client data.
    pub navigation_speed: Option<f64>,
    /// Captured `MobRef::pathfinding_malus` value. Finite table keyed by PathType; no live attribute lookup.
    pub pathfinding_malus: Vec<(PathType, f32)>,
    /// Captured `MobRef::can_use_portal` value. None means an unsupported subkind or genuinely unknown client data.
    pub can_use_portal: Option<bool>,
    /// Captured `MobRef::target` value. None means no target or unknown client data.
    pub target: Option<LivingRef>,
    /// Captured `MobRef::is_aggressive` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_aggressive: Option<bool>,
    /// Captured `MobRef::persistence_required` value. None means an unsupported subkind or genuinely unknown client data.
    pub persistence_required: Option<bool>,
    /// Captured `MobRef::requires_custom_persistence` value. None means an unsupported subkind or genuinely unknown client data.
    pub requires_custom_persistence: Option<bool>,
    /// Captured `MobRef::no_ai` value. None means an unsupported subkind or genuinely unknown client data.
    pub no_ai: Option<bool>,
    /// Captured `MobRef::restrict_center` value. None means an unsupported subkind or genuinely unknown client data.
    pub restrict_center: Option<[i32; 3]>,
    /// Captured `MobRef::restrict_radius` value. None means an unsupported subkind or genuinely unknown client data.
    pub restrict_radius: Option<i32>,
    /// Captured `MobRef::has_restriction` value. None means an unsupported subkind or genuinely unknown client data.
    pub has_restriction: Option<bool>,
    /// Captured `MobRef::left_handed` value. None means an unsupported subkind or genuinely unknown client data.
    pub left_handed: Option<bool>,
    /// Captured `MobRef::can_pick_up_loot` value. None means an unsupported subkind or genuinely unknown client data.
    pub can_pick_up_loot: Option<bool>,
    /// Captured `MobRef::no_action_time` value. None means an unsupported subkind or genuinely unknown client data.
    pub no_action_time: Option<i32>,
    /// Captured `MobRef::raid` value. None means an unsupported subkind or genuinely unknown client data.
    pub raid: Option<RaidRef>,
}

/// Cached PlayerRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct PlayerReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Captured `PlayerRef::selected_slot` value. None means an unsupported subkind or genuinely unknown client data.
    pub selected_slot: Option<usize>,
    /// Captured `PlayerRef::container_menu` value. None means an unsupported subkind or genuinely unknown client data.
    pub container_menu: Option<MenuRef>,
    /// Captured `PlayerRef::exp_cooldown` value. None means an unsupported subkind or genuinely unknown client data.
    pub exp_cooldown: Option<u32>,
    /// Captured `PlayerRef::last_death_location` value. None means no recorded last death.
    pub last_death_location: Option<GlobalPos>,
    /// Captured `PlayerRef::needs_food` value. None means an unsupported subkind or genuinely unknown client data.
    pub needs_food: Option<bool>,
    /// Captured `PlayerRef::has_enough_food` value. None means an unsupported subkind or genuinely unknown client data.
    pub has_enough_food: Option<bool>,
    /// Captured `PlayerRef::can_eat` value. Index 0 is always=false; index 1 is always=true.
    pub can_eat: [bool; 2],
    /// Captured `PlayerRef::experience` value. None means an unsupported subkind or genuinely unknown client data.
    pub experience: Option<PlayerExperience>,
    /// Captured `PlayerRef::xp_needed_for_next_level` value. None means an unsupported subkind or genuinely unknown client data.
    pub xp_needed_for_next_level: Option<i32>,
    /// Captured `PlayerRef::permission_level` value. None means an unsupported subkind or genuinely unknown client data.
    pub permission_level: Option<u8>,
    /// Captured `PlayerRef::has_infinite_materials` value. None means an unsupported subkind or genuinely unknown client data.
    pub has_infinite_materials: Option<bool>,
    /// Captured `PlayerRef::can_use_game_master_blocks` value. None means an unsupported subkind or genuinely unknown client data.
    pub can_use_game_master_blocks: Option<bool>,
    /// Captured `PlayerRef::previous_game_mode` value. Outer None means unknown client data; inner None means no previous mode.
    pub previous_game_mode: Option<Option<GameMode>>,
    /// Captured `PlayerRef::is_secondary_use_active` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_secondary_use_active: Option<bool>,
    /// Captured `PlayerRef::is_fake` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_fake: Option<bool>,
    /// Captured `PlayerRef::respawn_config` value. Outer None means unknown client data; inner None means no configured respawn.
    pub respawn_config: Option<Option<PlayerRespawnConfig>>,
    /// Captured `PlayerRef::text_filtering` value. None means an unsupported subkind or genuinely unknown client data.
    pub text_filtering: Option<bool>,
    /// Captured `PlayerRef::sleep_ignored` value. None means an unsupported subkind or genuinely unknown client data.
    pub sleep_ignored: Option<bool>,
    /// Captured `PlayerRef::counts_for_sleep` value. None means an unsupported subkind or genuinely unknown client data.
    pub counts_for_sleep: Option<bool>,
    /// Captured `PlayerRef::sleep_timer` value. None means an unsupported subkind or genuinely unknown client data.
    pub sleep_timer: Option<u32>,
    /// Captured `PlayerRef::game_mode` value. None means an unsupported subkind or genuinely unknown client data.
    pub game_mode: Option<GameMode>,
    /// Captured `PlayerRef::is_creative` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_creative: Option<bool>,
    /// Captured `PlayerRef::is_spectator` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_spectator: Option<bool>,
    /// Captured `PlayerRef::may_build` value. None means an unsupported subkind or genuinely unknown client data.
    pub may_build: Option<bool>,
    /// Captured `PlayerRef::food` value. None means an unsupported subkind or genuinely unknown client data.
    pub food: Option<PlayerFood>,
    /// Captured `PlayerRef::abilities` value. None means an unsupported subkind or genuinely unknown client data.
    pub abilities: Option<PlayerAbilities>,
    /// Captured `PlayerRef::block_breaking` value. None means an unsupported subkind or genuinely unknown client data.
    pub block_breaking: Option<BlockBreakingState>,
    /// Captured `PlayerRef::ambient_mood` value. None means an unsupported subkind or genuinely unknown client data.
    pub ambient_mood: Option<f32>,
}

/// Cached ProjectileRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct ProjectileReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Validated entity identity of this cached row.
    pub entity: EntityRef,
    /// Captured `ProjectileRef::owner` value. None means no owner, unsupported ownership, or unknown client data.
    pub owner: Option<EntityRef>,
    /// Captured `ProjectileRef::pickup` value. None means an unsupported subkind or genuinely unknown client data.
    pub pickup: Option<ArrowPickup>,
    /// Captured `ProjectileRef::pierce_level` value. Preserves only PierceLevelError, never Lifted.
    pub pierce_level: Result<i8, PierceLevelError>,
    /// Captured `ProjectileRef::base_damage` value. None means an unsupported subkind or genuinely unknown client data.
    pub base_damage: Option<f64>,
    /// Captured `ProjectileRef::critical` value. None means an unsupported subkind or genuinely unknown client data.
    pub critical: Option<bool>,
}

/// Cached RaidRef fields from one publication, potentially stale.
/// Owned strings and vectors are materialized on request, not cloned every pass.
#[derive(Clone, Debug, Wire)]
pub struct RaidReadRow {
    /// The publication shared by all fields in this row.
    pub capture: EntityReadCapture,
    /// Level-owned raid identity, not an entity owner.
    pub raid: RaidRef,
    /// Captured `RaidRef::center` value. None means an unsupported subkind or genuinely unknown client data.
    pub center: Option<[i32; 3]>,
    /// Captured `RaidRef::omen_level` value. None means an unsupported subkind or genuinely unknown client data.
    pub omen_level: Option<i32>,
    /// Captured `RaidRef::groups_spawned` value. None means an unsupported subkind or genuinely unknown client data.
    pub groups_spawned: Option<i32>,
    /// Captured `RaidRef::num_groups` value. None means an unsupported subkind or genuinely unknown client data.
    pub num_groups: Option<i32>,
    /// Captured `RaidRef::has_bonus_wave` value. None means an unsupported subkind or genuinely unknown client data.
    pub has_bonus_wave: Option<bool>,
    /// Captured `RaidRef::is_started` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_started: Option<bool>,
    /// Captured `RaidRef::is_active` value. None means an unsupported subkind or genuinely unknown client data.
    pub is_active: Option<bool>,
    /// Captured `RaidRef::status` value. None means an unsupported subkind or genuinely unknown client data.
    pub status: Option<RaidStatus>,
    /// Captured `RaidRef::total_health` value. None means an unsupported subkind or genuinely unknown client data.
    pub total_health: Option<f32>,
    /// Captured `RaidRef::living_health` value. None means an unsupported subkind or genuinely unknown client data.
    pub living_health: Option<f32>,
    /// Captured `RaidRef::bar_progress` value. None means an unsupported subkind or genuinely unknown client data.
    pub bar_progress: Option<f32>,
}

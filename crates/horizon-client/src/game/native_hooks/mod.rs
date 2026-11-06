//! Hook points for native mods (docs/native-mods.md, "Mods on the hook points"): empty
//! functions the game calls at fixed places, for horizon-mixins bundles to
//! hook with HEAD. Hooks that answer take an out parameter (`claimed`,
//! `zoom`…) so several mods can hook the same point. A native mod keeps its
//! own state in a module of its bundle; the helpers below read and draw what
//! Horizon's own HUD extras do, and manage a mod's settings.
//!
//! Native hook contracts belong to the game source. Names, parameters and call
//! sites may change; rebuild native mods after game updates.
//! API 399 adds a player's chat message before it is sent (`native_player_chat`).
//!
//! API 398 adds the player's network entity id on `NativeServerPlayer` (`entity_id`).
//!
//! API 397 adds mod config files, loaded and reloaded (`native_register_configs`, `native_config_loading`,
//! `native_config_reloading`, `native_config_unloading`; `native_configs.rs`).
//!
//! API 396 adds a slime or magma cube splitting on death, cancellable (`native_mob_split`).
//!
//! API 395 adds two animals breeding and a baby about to spawn, cancellable (`native_baby_entity_spawn`).
//!
//! API 394 adds experience points a server player is about to gain, changeable or cancelled (`native_player_xp_change`).
//!
//! API 393 adds the problem that stops a player sleeping in a bed, changeable (`native_can_player_sleep`).
//!
//! API 392 adds a living entity about to die, cancellable (`native_living_death`).
//!
//! API 391 adds a mob about to convert into another, cancellable (`native_living_conversion_pre`).
//!
//! API 390 adds a mob converted into another (`native_living_conversion_post`).
//!
//! API 389 adds A village siege found a spawn position, cancellable (`native_village_siege`).
//!
//! API 388 adds Possible natural spawn entries, mutable and cancellable (`native_potential_spawns`).
//!
//! API 387 adds A player tunes a note block (`native_note_block_change`).
//!
//! API 386 adds an entity's data (read and merge, as `/data` does), tags and name through an `EntityRef`.
//!
//! API 385 adds per-stack conditional item model properties (`native_item_model_property`).
//!
//!
//! API 383 adds living_knockback_v2: raw knockback with the attacker's entity id and held items, including armor-stand sweeps.
//!
//! API 382 adds living entity handles (`LivingRef`, `native_living`) and a living entity's equipment and hand items, read and set.
//!
//! API 381 adds a level's blocks and time through a `LevelRef`: block state, block, property, whether a chunk is loaded, height, game time and day time.
//!
//! API 380 adds block textures for client mods: `native_block_sprites` and `native_texture_sprites` say where blocks' and textures' sprites are in the block sprite arrays, and `WorldDrawn::block_sprites` hands a mod the arrays.
//!
//! API 379 adds an entity's velocity (and setting it), ground, box, eye height and position, view vector and distance through an `EntityRef`.
//!
//! API 378 adds validated category declarations for custom recipe types (`native_recipe_book_categories`, `native_recipe_book_category`); display and unlock integration follows separately.
//!
//! API 377 adds a mod's own ingredient kind matching a concrete stack
//! (`native_ingredient_test`, which the ingredient's test calls with the kind,
//! its JSON, the stack and the verdict — the load-time list already named
//! the item).
//!
//! API 376 adds The ammunition a bow or crossbow shooter finds (`native_living_get_projectile`).
//!
//! API 375 adds A player is about to nock a bow (`native_arrow_nock`).
//!
//! API 374 adds Explosion velocity replacement (`native_explosion_knockback`).
//!
//! API 373 adds entity and level handles (`EntityRef`, `LevelRef`, `native_entity`, `native_level`) and an entity's position, block position, rotation, type, UUID and level.
//!
//! API 372 adds a name tag hook (native_render_name_tag).
//!
//! API 371 adds `WorldDrawn::motion`: which mob, player or dropped item each run of the frame's entity quads is.
//!
//! API 370 adds a mod item holding items (`native_item_container`), and the item container helpers in
//! `native_item_container.rs`: `native_item_container_slots`, `native_item_container_insert`, `native_item_container_extract`.
//!
//! API 369 adds an additive first-person hand and item pose (`native_render_hand_pose`).
//!
//! API 368 adds tooltip box colours (`native_tooltip_color`).
//!
//! API 367 adds the screen_opening screen lifecycle observation (`native_screen_opening`) at most once per frame.
//!
//! API 366 adds the screen_init_post screen lifecycle observation (`native_screen_init_post`) at most once per frame.
//!
//! API 365 adds the screen_closing screen lifecycle observation (`native_screen_closing`) at most once per frame.
//!
//! API 364 adds the render hook point `native_hand_drawn(hand)`: the first-person hand is drawn (`HandDrawn`).
//!
//! API 363 adds reading the world camera and block collision boxes (`native_camera`, `native_block_collision`).
//!
//! API 362 adds a per-tick player FOV modifier hook (`native_compute_fov_modifier`).
//!
//! API 361 adds a cancellable item frame item render hook (`native_render_item_in_frame`).
//!
//! API 360 adds Local player movement input before application (`native_movement_input`).
//!
//! API 359 adds a living entity jumping (`native_living_jump`).
//!
//! API 358 adds a living entity healing, with a changeable amount and cancellation (`native_living_heal`).
//!
//! API 357 adds a living entity's fall, with changeable distance and multiplier and cancellation (`native_living_fall`).
//!
//! API 356 adds a living entity drowning, with changeable drowning and damage and a read-only bubble count (`native_living_drown`).
//!
//! API 355 adds a mod's own picture in the HUD atlas (`native_mod_image`).
//!
//! API 354 adds A timed world border size change finished (`native_world_border_bounds_change_finish`).
//!
//! API 353 adds A vault ejects a loot item (`native_vault_loot`).
//!
//! API 352 adds A hopper begins an item transfer attempt (`native_prepare_hopper_transfer`).
//!
//! API 351 adds A guest player changed sneaking (`native_player_toggle_sneak`).
//!
//! API 350 adds A server player released the use-item key (`native_player_stop_using_item`).
//!
//! API 349 adds A server player's recipe book settings changed (`native_player_recipe_book_settings_change`).
//!
//! API 348 adds The server player's selected bow projectile (`native_player_ready_arrow`).
//!
//! API 347 adds A server player's experience level changed (`native_player_level_change`).
//!
//! API 346 adds A server player launched a projectile item (`native_player_launch_projectile`).
//!
//! API 345 adds A guest was disconnected by a server kick (`native_player_kicked`).
//!
//! API 344 adds A guest jumped from the ground (`native_player_jump`).
//!
//! API 343 adds A guest sent player input flags (`native_player_input`).
//!
//! API 342 adds A server player's client finished loading the world (`native_player_client_loaded_world`).
//!
//! API 341 adds A server player selected an item in a bundle (`native_player_bundle_item_select`).
//!
//! API 340 adds A guest's server handled arm swing (`native_player_arm_swing`).
//!
//! API 339 adds A server player earned an advancement criterion (`native_player_advancement_criterion_grant`).
//!
//! API 338 adds a boat or minecart about to be destroyed (`native_vehicle_destroy`).
//!
//! API 337 adds a vehicle taking damage (`native_vehicle_damage`).
//!
//! API 336 adds a boat or minecart created (`native_vehicle_create`).
//!
//! API 335 adds A dropped item entity reached its lifetime and is about to despawn (`native_item_despawn`).
//!
//! API 334 adds A hopper moves an item between containers (`native_inventory_move_item`).
//!
//! API 333 adds A hopper selects a container while searching for items (`native_hopper_inventory_search`).
//!
//! API 332 adds A server explosion is about to begin (`native_explosion_prime`).
//!
//! API 331 adds A mob's leash was removed (`native_entity_unleash`).
//!
//! API 330 adds A mob is about to drop an item (`native_entity_drop_item`).
//!
//! API 329 adds A server player stopped digging before the block broke (`native_block_damage_abort`).
//!
//! API 328 adds A player took an anvil's result (`native_anvil_craft_pre`).
//!
//! API 327 adds A player finished taking an anvil's result (`native_anvil_craft_post`).
//!
//! API 326 adds the title's own screens to the screen hooks, and the instance folder helper (`native_instance_dir`).
//!
//! API 325 adds a dropped item reaching its lifetime (`native_item_expire`).
//!
//! API 324 adds a player saved to the world's store (`native_player_save_to_file`).
//!
//! API 323 adds a player loaded from the world's save (`native_player_load_from_file`).
//!
//! API 322 adds a player's data copied to their new player (`native_player_clone`).
//!
//! API 321 adds A statistic about to be awarded to a server player, cancellable (`native_player_stat_award`).
//!
//! API 320 adds A criterion of a server player's advancement was granted or revoked (`native_player_advancement_progress`).
//!
//! API 319 adds an item frame or painting about to break, cancellable (`native_hanging_break`).
//!
//! API 318 adds A player releases a bow with adjustable charge or cancellation (`native_arrow_loose`).
//!
//! API 317 adds attachment reads that never insert the default (`AttachmentType::has`, `AttachmentType::get_existing`).
//!
//! API 316 adds A player's melee hit on a living target decides whether it is critical (`native_critical_hit`).
//!
//! API 315 adds a player's item breaking (`native_player_destroy_item`).
//!
//! API 314 adds hiding a player from the players below a permission level (`native_hide_player`).
//!
//! API 313 adds drawing a component-bearing item stack and its decorations on a screen (`native_item_stack`).
//!
//! API 312 adds a server player's ender pearl is about to teleport them (`native_ender_pearl_teleport`).
//!
//! API 311 adds a server player who ate chorus fruit is about to try a random spot (`native_chorus_fruit_teleport`).
//!
//! API 310 adds a server player's trade with a villager or wandering trader, with its offer (`native_trade_with_villager`).
//!
//! API 309 adds a shift-click in a mod container's menu, routed by the mod (`native_menu_quick_move`).
//!
//! API 308 adds the time players wake at after sleeping through the night, changeable (`native_sleep_finished_time`).
//!
//! API 307 adds a mod item holding energy (`native_item_energy_buffer`), and the item energy helpers in
//! `native_item_energy.rs`: `native_item_energy`, `native_item_energy_insert`, `native_item_energy_extract`.
//!
//! API 306 adds A world save cycle completed (`native_world_saved`).
//!
//! API 305 adds The dedicated server whitelist was enabled or disabled (`native_whitelist_state_update`).
//!
//! API 304 adds Server weather parameters changing, cancellable (`native_weather_change`).
//!
//! API 303 adds A vault block changes state (`native_vault_change_state`).
//!
//! API 302 adds Player selected a villager trade (`native_trade_select`).
//!
//! API 301 adds The server clock is about to jump, cancellable (`native_time_skip`).
//!
//! API 300 adds Server thunder state changing, cancellable (`native_thunder_change`).
//!
//! API 299 adds A projectile hits a target block (`native_target_hit`).
//!
//! API 298 adds A guest asked the server to complete a command (`native_tab_complete`).
//!
//! API 297 adds A sponge absorbs water (`native_sponge_absorb`).
//!
//! API 296 adds A sculk catalyst begins blooming (`native_sculk_bloom`).
//!
//! API 295 adds A raid is triggered or reaches a terminal state (`native_raid_state_change`).
//!
//! API 294 adds A raid wave that has spawned (`native_raid_spawn_wave`).
//!
//! API 293 adds A projectile was added to the server level (`native_projectile_launch`).
//!
//! API 292 adds Smithing result prepared (`native_prepare_smithing`).
//!
//! API 291 adds Enchanting offers calculated (`native_prepare_item_enchant`).
//!
//! API 290 adds Crafting-grid result prepared (`native_prepare_item_craft`).
//!
//! API 289 adds Grindstone result prepared (`native_prepare_grindstone`).
//!
//! API 288 adds Anvil result prepared (`native_prepare_anvil`).
//!
//! API 287 adds A thrown potion applied its hit effects (`native_potion_splash`).
//!
//! API 286 adds A Nether portal or End arrival platform is created (`native_portal_create`).
//!
//! API 285 adds The server synced changed velocity to a player (`native_player_velocity`).
//!
//! API 284 adds A server player's action removed an entity's leash (`native_player_unleash_entity`).
//!
//! API 283 adds A server player completed a merchant trade (`native_player_trade`).
//!
//! API 282 adds A server player toggled an entity's age lock (`native_player_toggle_entity_age_lock`).
//!
//! API 281 adds Player about to travel through an End gateway, cancellable (`native_player_teleport_end_gateway`).
//!
//! API 280 adds A server player swapped with an equipment slot (`native_player_swap_with_equipment_slot`).
//!
//! API 279 adds A spectator stopped watching an entity (`native_player_stop_spectating_entity`).
//!
//! API 278 adds A server player selected a stonecutter recipe (`native_player_stonecutter_recipe_select`).
//!
//! API 277 adds A server player's respawn point about to change, cancellable (`native_player_spawn_point_set`).
//!
//! API 276 adds A player's shield was disabled by a blocked hit (`native_player_shield_disable`).
//!
//! API 275 adds A player about to shear a mob, cancellable (`native_player_shear_entity`).
//!
//! API 274 adds A player launches with a riptide trident (`native_player_riptide`).
//!
//! API 273 adds A recipe about to be discovered by a server player, cancellable (`native_player_recipe_discovered`).
//!
//! API 272 adds Server player requests recipe book auto-fill (`native_player_recipe_book_click`).
//!
//! API 271 adds A server player about to travel through a portal, cancellable (`native_player_portal`).
//!
//! API 270 adds A server player picked up an arrow (`native_player_pickup_arrow`).
//!
//! API 269 adds A server player's middle-click pick, cancellable (`native_player_pick_item`).
//!
//! API 268 adds A server player opened a sign editor (`native_player_open_sign`).
//!
//! API 267 adds A server player named a mob (`native_player_name_entity`).
//!
//! API 266 adds A server player filled an empty map (`native_player_map_filled`).
//!
//! API 265 adds A server player selected a loom pattern (`native_player_loom_pattern_select`).
//!
//! API 264 adds Server player changed client language (`native_player_locale_change`).
//!
//! API 263 adds A server player's selected hotbar slot, cancellable (`native_player_item_held`).
//!
//! API 262 adds An item cooldown about to start for a server player, cancellable (`native_player_item_cooldown`).
//!
//! API 261 adds XP orb pickup cooldown changed for a server player (`native_player_exp_cooldown_change`).
//!
//! API 260 adds A server player's orb-pickup experience points before they are added (`native_player_exp_change`).
//!
//! API 259 adds A server player used a firework rocket to boost while gliding (`native_player_elytra_boost`).
//!
//! API 258 adds A server player threw an egg (`native_player_egg_throw`).
//!
//! API 257 adds A player book edit or signing packet, cancellable (`native_player_edit_book`).
//!
//! API 256 adds A server player reaches deep sleep (`native_player_deep_sleep`).
//!
//! API 255 adds A server player's client option for their dominant hand changed (`native_player_change_main_hand`).
//!
//! API 254 adds A server player changed a beacon's selected effects (`native_player_change_beacon_effect`).
//!
//! API 253 adds A server player caught a mob in a bucket (`native_player_bucket_entity`).
//!
//! API 252 adds A player left a bed, after their sleeping state changed (`native_player_bed_leave`).
//!
//! API 251 adds A player about to enter a bed, cancellable (`native_player_bed_enter`).
//!
//! API 250 adds A server player's attack strength was reset by a punch (`native_player_attack_cooldown_reset`).
//!
//! API 249 adds A player completed an advancement (`native_player_advancement_done`).
//!
//! API 248 adds Note block about to sound (`native_note_play`).
//!
//! API 247 adds A server game rule changes (`native_world_game_rule_change`).
//!
//! API 246 adds World border center change (`native_world_border_center_changed`).
//!
//! API 245 adds World border bounds change (`native_world_border_bounds_changed`).
//!
//! API 244 adds The world spawn point changes (`native_spawn_change`).
//!
//! API 243 adds A server player took a book from a lectern (`native_player_take_lectern_book`).
//!
//! API 242 adds A server player changed a lectern page (`native_player_lectern_page_change`).
//!
//! API 241 adds An orb repaired a server player's item through Mending (`native_player_item_mend`).
//!
//! API 240 adds A server player changed an item frame (`native_player_item_frame_change`).
//!
//! API 239 adds A server player inserted a book into a lectern (`native_player_insert_lectern_book`).
//!
//! API 238 adds A server player put a plant in a flower pot (`native_player_flower_pot_manipulate`).
//!
//! API 237 adds Farmland moisture changing (`native_moisture_change`).
//!
//! API 236 adds A server lightning bolt is about to be created, cancellable (`native_lightning_strike`).
//!
//! API 235 adds A leaf block decaying (`native_leaves_decay`).
//!
//! API 234 adds A dropped item entity entered the level (`native_item_spawn`).
//!
//! API 233 adds A guest is about to open a block inventory (`native_inventory_block_start`).
//!
//! API 232 adds A hopper is about to pick up a dropped item (`native_hopper_pickup_item`).
//!
//! API 231 adds A vanilla world game event fired (`native_game_event`).
//!
//! API 230 adds A furnace begins cooking an item (`native_furnace_start_smelt`).
//!
//! API 229 adds A furnace finishes smelting an item (`native_furnace_smelt`).
//!
//! API 228 adds A player takes a furnace output (`native_furnace_extract`).
//!
//! API 227 adds A furnace consumes fuel and begins burning (`native_furnace_burn`).
//!
//! API 226 adds A flowing fluid changing level (`native_fluid_level_change`).
//!
//! API 225 adds A firework rocket completed explosion effects (`native_firework_explode`).
//!
//! API 224 adds An experience bottle broke and awarded experience (`native_exp_bottle`).
//!
//! API 223 adds A mob conversion completed (`native_entity_transform`).
//!
//! API 222 adds A guest player's fall flying state changed (`native_entity_toggle_glide`).
//!
//! API 221 adds A server entity completed a same-level teleport command (`native_entity_teleport`).
//!
//! API 220 adds A mob became tamed (`native_entity_tame`).
//!
//! API 219 adds A player fired an arrow from a bow or crossbow (`native_entity_shoot_bow`).
//!
//! API 218 adds A guest player's synchronized pose changed (`native_entity_pose_change`).
//!
//! API 217 adds A server entity begins a portal transfer (`native_entity_portal`).
//!
//! API 216 adds A mob entered love mode (`native_entity_enter_love_mode`).
//!
//! API 215 adds A bee entered a beehive (`native_entity_enter_block`).
//!
//! API 214 adds A mob changed a block (`native_entity_change_block`).
//!
//! API 213 adds Two mobs completed a breeding action (`native_entity_breed`).
//!
//! API 212 adds A mob broke a door (`native_entity_break_door`).
//!
//! API 211 adds Entities left the active world as a chunk unloaded (`native_entities_unload`).
//!
//! API 210 adds Saved entity records finished loading into a chunk (`native_entities_load`).
//!
//! API 209 adds Player applied an enchantment (`native_enchant_item`).
//!
//! API 208 adds The Ender Dragon fight placing its first egg (`native_dragon_egg_form`).
//!
//! API 207 adds Lightning charged a creeper (`native_creeper_power`).
//!
//! API 206 adds A player ignited a creeper (`native_creeper_ignite`).
//!
//! API 205 adds A crafter completes a recipe (`native_crafter_craft`).
//!
//! API 204 adds A player composts an item (`native_compost_item`).
//!
//! API 203 adds Cauldron fill level about to change (`native_cauldron_level_changed`).
//!
//! API 202 adds An item starts cooking on a campfire (`native_campfire_start`).
//!
//! API 201 adds A server system broadcast line, rewritable or cancellable (`native_broadcast_message`).
//!
//! API 200 adds A brewing stand starts a brew (`native_brewing_start`).
//!
//! API 199 adds Brewing stand consumed fuel (`native_brewing_fuel`).
//!
//! API 198 adds Brewing stand finished brewing (`native_brewing_complete`).
//!
//! API 197 adds A dispenser shears an entity (`native_block_shear_entity`).
//!
//! API 196 adds A sculk listener receives a game event (`native_block_receive_game`).
//!
//! API 195 adds A guest checks a block container lock (`native_block_lock_check`).
//!
//! API 194 adds Fire about to be placed (`native_block_ignite`).
//!
//! API 193 adds Precipitation forms ice or snow (`native_block_form`).
//!
//! API 192 adds A dispenser fails to dispense an item (`native_block_failed_dispense`).
//!
//! API 191 adds A block fades (`native_block_fade`).
//!
//! API 190 adds A block is about to drop experience (`native_block_exp`).
//!
//! API 189 adds A dispenser equips armor or equipment (`native_block_dispense_armor`).
//!
//! API 188 adds A block finishes cooking an item (`native_block_cook`).
//!
//! API 187 adds Fire about to burn a block away (`native_block_burn`).
//!
//! API 186 adds A guest's block-breaking progress changes (`native_block_break_progress_update`).
//!
//! API 185 adds Bell about to ring (`native_bell_ring`).
//!
//! API 184 adds A beacon activates or deactivates (`native_beacon_state_change`).
//!
//! API 183 adds An anvil takes damage (`native_anvil_damaged`).
//!
//! API 182 adds a totem of undying saving a player (`native_player_totem`).
//!
//! API 181 adds TNT about to be lit, cancellable (`native_tnt_primed`).
//!
//! API 180 adds a server player placing an item frame, painting or leash knot, cancellable (`native_hanging_place`).
//!
//! API 179 adds a server player emptying a bucket, cancellable (`native_bucket_empty`).
//!
//! API 178 adds a boat or minecart a server player rides having moved (`native_vehicle_moved`).
//!
//! API 177 adds a creative player setting a slot from the creative screen, cancellable (`native_creative_slot`).
//!
//! API 176 adds a player opening one of the game's menus, cancellable (`native_container_opened`).
//!
//! API 175 adds a player taking an item from or putting one on an armor stand, cancellable (`native_armor_stand_manipulate`).
//!
//! API 174 adds a server player's answer to a resource pack (`native_resource_pack_status`).
//!
//! API 173 adds a server player's sprint starting or stopping (`native_player_sprint`).
//!
//! API 172 adds a joining player the mods may refuse (`native_player_login`).
//!
//! API 171 adds the root commands a player's command tree holds (`native_player_commands_sent`).
//!
//! API 170 adds a server player teleported by a command (`native_player_teleported`).
//!
//! API 169 adds a player's food level changing, changeable and cancellable (`native_food_level_changed`).
//!
//! API 168 adds a player swapping their hands, cancellable (`native_player_swap_hands`).
//!
//! API 167 adds lowering the frame rate limit, as Dynamic FPS does (`native_framerate_limit`).
//!
//! API 166 adds a recipe ingredient of a mod's own kind
//! (`native_ingredient`, which the unknown ingredient type in a namespace of
//! own reaches with the kind, its JSON and the item list to fill).
//!
//! API 165 adds the window's physical pixel size (`native_physical_window_size`).
//!
//! API 164 adds a helper that names the block a state belongs to (`native_state_block`).
//!
//! API 163 adds a server mod's line for the event stream and the Discord bridge (`native_server_event`).
//!
//! API 162 adds a player's item losing durability, changeable and cancellable (`native_item_damaged`).
//!
//! API 161 adds a player taking a crafting grid's result (`native_item_crafted`).
//!
//! API 160 adds a player's fishing rod: cast, bite, catch, reel-in, cancellable (`native_player_fish`).
//!
//! API 159 adds the server's difficulty changing (`native_difficulty_changed`).
//!
//! API 158 adds a server player's game mode about to change, cancellable (`native_player_game_mode_changed`).
//!
//! API 157 adds how fire burns a mod block (`native_fire_odds`, `FireBlock.setFlammable`), and a mod block under
//! a campfire that makes it a signal fire (`native_smoke_source`, `CampfireBlock.isSmokeSource`).
//!
//! API 156 adds where a loot roll happens, for a mod's own loot kinds
//! (`NativeLootContext.place`: the origin, the block entity and the level).
//!
//! API 155 adds a server player taking an experience orb (`native_experience_orb_pickup`, asked for through
//! `native_experience_orb_pickup_listened`: cancel it, or change the orb's value and count) and the Mending repair
//! proposed for the item it picked (`native_mending_repair`).
//!
//! API 154 adds a player changing dimension (`native_player_changed_dimension`).
//!
//! API 153 adds a player starting or stopping flight, cancellable (`native_player_toggle_flight`).
//!
//! API 152 adds a player opening and closing a menu (`native_player_container_open`, `native_player_container_close`).
//!
//! API 151 adds models no blockstate names, baked for drawing (`native_standalone_models`,
//! `native_draw_model`).
//!
//! API 150 adds a player finishing a sign, rewritable or cancellable (`native_sign_changed`).
//!
//! API 149 adds a player drawn with a mod's own model, and layers added to mobs and players (`native_player_submit`, `native_mob_layer`, told while a mod sets `native_player_render_listened`).
//!
//! API 148 adds a mob's or an armor stand's equipment slot taking another stack (`native_living_equipment_change`), and whether a mod listens to the mobs' (`native_living_equipment_listened`).
//!
//! API 147 adds a mod item as a fluid container (`native_item_fluid_tank`), and the item fluid helpers in `native_item_fluids.rs`: `native_item_fluid`, `native_item_fluid_fill`, `native_item_fluid_drain`.
//!
//! API 146 adds entities ticking, which a mod can stop (`native_entity_tick_pre`, `native_entity_tick_post`, told while a mod sets `native_entity_tick_listened`).
//!
//! API 145 adds blocks and items declared in code (`native_blocks`, `native_items`).
//!
//! API 144 adds a player's or mob's breathing each tick (`native_living_breathe`, a free function in `horizon_client::breathing`).
//!
//! API 143 adds data map types a native mod registers (`native_data_map_types`), merged from the data packs' and mods' files each time the server's tables are built, read with `native_data_map` and `native_data_map_entries` (`native_data_maps.rs`).
//!
//! API 142 adds an energy buffer on a mod block's block entity (`native_block_entity_energy`), and the energy helpers in `machines/native_energy.rs`: `native_energy`, `native_energy_insert`, `native_energy_extract`, `native_move_energy`.
//!
//! API 141 adds a player starting and stopping tracking an entity (`native_player_start_tracking`, `native_player_stop_tracking`).
//!
//! API 140 adds a mod's own icons on filled maps (`native_map_decorations`).
//!
//! API 139 adds HUD layers around vanilla's own (`native_gui_layer_pre`, `native_gui_layer_post`).
//!
//! API 138 adds a cancellable attack, use and pick-block key hook (`native_interaction_key`).
//!
//! API 137 adds an entity getting on or off a vehicle, which a mod can cancel (`native_entity_mount`).
//!
//! API 136 adds a projectile's impact a mod can cancel (`native_projectile_impact`, heard once a mod sets `native_hears_projectile_impacts`).
//!
//! API 135 adds the client's level made and dropped (`native_client_level_load`, `native_client_level_unload`).
//!
//! API 134 adds a server's levels opening and closing (`native_level_load`, `native_level_unload`).
//!
//! API 133 adds a loot table's function and condition of a mod's
//! own kind (`native_loot_function` and `native_loot_condition`, which the
//! unknown `type` in a namespace of its own reaches with the kind, its JSON,
//! the loot context and the stack or the holds).
//!
//! API 132 adds other players' inventories for server mods, online or from their file (`native_inventory`, `native_move_stack`), and maps a mod draws (`native_send_map`).
//!
//! API 131 adds an item's use tick and release (`native_living_entity_use_item_tick`,
//! `native_on_use_tick`, `native_living_entity_use_item_stop`, `native_release_using`).
//!
//! API 130 adds a content mod menu type's slot rules decided in code (`native_menu_slot_rules`, in
//! `menu.rs`: whether a slot takes a stack, `Slot.mayPlace`, and how many it holds,
//! `Slot.getMaxStackSize`), with mod menu types' clicks on the vanilla menu engine.
//!
//! API 129 adds mod block entities on any template (`native_block_entity`).
//!
//! API 128 adds experience orb merging a mod decides (`native_experience_orb_can_merge`, `native_experience_orb_merge`) and writes (`native_set_experience_orb`).
//!
//! API 127 adds column reads for server mods (`native_surface_heights`, `native_read_columns`).
//!
//! API 126 adds bulk block edits for server mods (`native_edit_blocks`).
//!
//! API 125 adds knockback a mod can change or cancel (`native_living_knockback`).
//!
//! API 124 adds the players with a mod container open (`native_mod_container_viewers`).
//!
//! API 123 adds a screen drag hook (`native_screen_mouse_dragged_pre`).
//!
//! API 122 adds a two-axis gameplay scroll hook (`native_mouse_scrolling`).
//!
//! API 121 adds a complete window keyboard event (`native_input_key`).
//!
//! API 120 adds a server's recipes sent to its Horizon clients: the server's mods name the recipe types
//! (`native_datapack_sync`, `OnDatapackSyncEvent.sendRecipes`) and the client hears they arrived
//! (`native_recipes_received`, `RecipesReceivedEvent`).
//!
//! API 119 adds music for client mods: the track and its volume (`native_music`) and looping layers over it (`native_music_layers`).
//!
//! API 118 adds a screen key release hook (`native_screen_key_released_pre`).
//!
//! API 117 adds a final world fog hook (`native_render_fog`).
//!
//! API 116 adds a client mod's view and its own items (`native_raycast`, `native_view_hit`,
//! `native_claim_items`, `native_poll`).
//!
//! API 115 adds a player respawning (`native_player_respawned`).
//!
//! API 114 adds raw mouse button pre/post hooks (`native_mouse_button_pre`, `native_mouse_button_post`).
//!
//! API 113 adds a computed fog color hook (`native_compute_fog_color`).
//!
//! API 112 adds a detached-camera distance hook (`native_camera_distance`).
//!
//! API 111 adds a native mod's own screen (`native_mod_screen_open`, `native_mod_screen_close`,
//! `native_mod_screen`) and two drawing helpers for the screen hooks (`native_quad`,
//! `native_item_model`).
//!
//! API 110 adds mobs drawn by client mods (`native_mob_submit`, with `NativeMobRender`).
//!
//! API 109 adds a world field-of-view hook (`native_compute_fov`).
//!
//! API 108 adds the server's data tables are built (`native_server_data_loaded`, `TagsUpdatedEvent` with
//! `SERVER_DATA_LOAD`): a world's first open and every `/reload`, told apart by `reload`.
//!
//! API 107 adds biome colours for client mods (`native_biome_colors`, `native_refresh_biome_colors`).
//!
//! API 106 adds data pack files for the tables a native mod contributes (`native_data_pack_files`), on a world's first open and on every `/reload`.
//!
//! API 105 adds a push for the local player and a player's flying speed (`native_push_player`,
//! `native_set_flying_speed`).
//!
//! API 104 adds the open screen hears of a key press (`native_screen_key_pressed_pre`, the
//! `ScreenEvent.KeyPressed.Pre` point, with the screen, vanilla's key name, whether it is an
//! auto-repeat and the modifiers) and of each character it types (`native_screen_char_typed_pre`,
//! `ScreenEvent.CharacterTyped.Pre`, fired after the key has acted, on the screen open then), so a
//! cancelled press goes no further and a cancelled character never reaches a text box. The helpers
//! both paths share (`screen_key_pressed`, `screen_text_typed`, `screen_chars_typed`,
//! `screen_text_hooked`) sit beside them.
//!
//! API 103 adds a mod's own particle types for `native_send_particles`
//! (`ParticleOptions::named`).
//!
//! API 102 adds lead drops to `native_loot_drops`: untying a lead or cutting it with shears, and the lead a mob a player
//! killed while leashed drops on its next tick, with the player named.
//!
//! API 101 adds experience for server mods: reading orbs (`native_experience_orbs`), giving points as an orb does (`native_give_experience`) and giving or taking levels (`native_give_levels`).
//!
//! API 100 adds player-use drops to `native_loot_drops` (`NativeLootDrops::used`): shearing, brushing, carving a
//! pumpkin, picking berries and breaking an item frame, with the player named.
//!
//! API 99 adds a mod's command typed on the dedicated server's console (`native_perform_console_command`).
//!
//! API 98 adds giving a whole stack to any player from a server hook (`native_give_stack`).
//!
//! API 97 adds reading the world's light, weather, time of day, height and item stack sizes from a native mod (light, weather, time of day and height now through `LevelRef`, with `native_max_stack_size`).
//!
//! API 96 adds reading redstone from a server mod's block (`native_neighbor_signal`).
//!
//! API 95 adds any command a player types, cancellable (`native_player_command`).
//!
//! API 94 adds keeping a mod block entity's geometry while it doesn't change
//! (`BlockEntityDraw::keep`).
//!
//! API 93 adds the open screen hears of the wheel before it scrolls (`native_screen_mouse_scrolled_pre`,
//! the `ScreenEvent.MouseScrolled.Pre` point, with the screen, the scroll in x and y as the game's
//! scroll handler scales it, and the modifiers), so a cancelled turn scrolls nothing.
//!
//! API 92 adds the world's clock on a client (`native_clock`).
//!
//! API 91 adds items and block state properties for drawing mod block entities
//! (`native_draw_item`, `native_block_state_property`).
//!
//! API 90 adds using an item over time: its duration and hand pose (`native_use_duration`,
//! `native_use_animation`), `native_can_eat`, and its start and finish
//! (`native_living_entity_use_item_start`, `native_living_entity_use_item_finish`).
//!
//! API 89 adds custom item icon decorations (`native_item_decorations`).
//!
//! API 88 adds a client resource-pack reload completion notification (`native_client_resources_reloaded`).
//!
//! API 87 adds a cancellable first-person hand render hook (`native_render_hand`).
//!
//! API 86 adds a cancellable targeted-block highlight hook (`native_block_highlight`).
//!
//! API 85 adds menus of content mods' own menu types for mod containers (the `menu` of a
//! `ModContainer` may name one).
//!
//! API 84 adds values for a mod's own shader pack (`native_shader_params`).
//!
//! API 83 adds client sounds that wait (`native_play_local_sound`).
//!
//! API 82 adds camera shake for client mods (`native_camera_shake`).
//!
//! API 81 adds falling blocks for server mods (`native_spawn_falling_block`,
//! in `native_falling.rs`).
//!
//! API 80 adds the open screen hears of a mouse button going down or up before it acts
//! (`native_screen_mouse_button_pressed_pre`, `native_screen_mouse_button_released_pre`, the
//! `ScreenEvent.MouseButtonPressed.Pre` and `.MouseButtonReleased.Pre` points, with the screen, the
//! GLFW button and the modifiers), and the helpers the input hooks share (`screen_view_now`,
//! `screen_hooks_open`).
//!
//! API 79 adds items' default components (`native_modify_default_components`, in `horizon-world`'s
//! `item_defaults.rs`): set or remove a component of any item, vanilla's included, as the game reads its defaults.
//!
//! API 78 adds `native_screen_render_post` (`ScreenEvent.Render.Post`: draw over the open
//! screen, given its id, menu type, title, container panel, slot count, mouse
//! and partial tick).
//!
//! API 77 adds a `minecraft:post_attack` enchantment effect of a mod's own kind
//! (`native_enchant_effect`, told the enchantment, the kind, the JSON, the level and
//! the mob hit).
//!
//! API 76 adds a mod block's placement state (`native_state_for_placement`) and the state it is built from
//! (`native_state_with_property`): what a block that faces the player, sits on the clicked face or comes waterlogged needs.
//!
//! API 75 adds changing vanilla's loot tables and drops: `native_loot_table_load` (in
//! `horizon-world`'s `loot.rs`: each table's JSON as it loads) and `native_loot_drops` (a broken
//! block's or a dead mob's items, with what dropped them).
//!
//! API 74 adds a consumable's effect of a mod's own kind
//! (`native_consume_effect`, which the unknown `on_consume_effects` type reaches with
//! the kind and the JSON the file wrote).
//!
//! API 73 adds mod-block entity contacts (`native_block_step_on`, `native_block_entity_inside`, `native_block_fall_on`), including mutable landing damage.
//!
//! API 72 adds bone meal on a mod block (`native_is_valid_bonemeal_target`, `native_is_bonemeal_success`,
//! `native_perform_bonemeal`), and the level's random a world hook draws from (`native_level_random`).
//!
//! API 71 adds drawing mod block entities on the client (`native_block_entity_submit`,
//! vanilla's `BlockEntityRenderer.submit`, culled by `native_block_entity_render_bounding_box`,
//! NeoForge's `getRenderBoundingBox`), with a pose stack (`BlockEntityDraw`) and the
//! helpers `native_draw_quad`, `native_draw_cube` and `native_draw_block`.
//!
//! API 70 adds a mod block's own `canSurvive` (`native_can_survive`), and the blocks a mod plant
//! can stand on (`native_may_place_on`, `VegetationBlock.mayPlaceOn`).
//!
//! API 69 adds a mod's own advancement criteria triggers (`native_trigger_criterion`,
//! in `native_criteria.rs`).
//!
//! API 68 adds stack-dependent image tooltip components, measured with `native_tooltip_component` and rendered with `native_tooltip_component_draw`.
//!
//! API 67 adds dispenser item behaviours (`native_dispense_item`) and `native_dispense_default` for a plain drop.
//!
//! API 66 adds explosions for server mods (`native_explode`, in
//! `native_explosions.rs`).
//!
//! API 65 adds native item component type registration (`native_component_types`) and opaque NBT component helpers.
//!
//! API 64 adds comparator output from a mod's block (`native_has_analog_output`,
//! `native_analog_output`, and the helper `native_update_output_neighbors`).
//!
//! API 63 adds context-free tag lookups for server hooks (`native_server_block_in_tag`, `native_server_item_in_tag`).
//!
//! API 62 adds nicknames for server mods: `native_set_nickname` shows a name in place of a player's in chat and
//! the Tab list (the real one on hover), and `native_text_allowed` asks the server's chat filter, in
//! `native_server.rs`.
//!
//! API 61 adds mods' own sound events in `native_play_sound`: another
//! namespace's event is sent by name, as vanilla sends an unregistered sound.
//!
//! API 60 adds the small bar under an item (`native_item_bar`): `GuiGraphicsExtractor.itemDecorations`
//! hands it vanilla's choice (the damage bar, or a bundle's fullness already applied), to replace or hide.
//!
//! API 59 adds entities for server mods (`native_summon`, `native_move_entity`,
//! `native_set_display`, `native_remove_entity`, in `native_entities.rs`).
//!
//! API 58 adds the renderer's hook points, in `horizon-mod-api`'s
//! `render.rs` (`gfx::native_render`): `native_world_drawn` (draw with the world's colour,
//! depth, matrices and entity geometry before the hand and the HUD) and
//! `native_render_needs` (entity runs and the world's depth without a pack).
//!
//! API 57 adds chunks and players coming and going (`native_chunk_load`,
//! `native_chunk_unload`, `native_player_logged_in`, `native_player_logged_out`).
//!
//! API 56 adds what a content mod's mob effects do: their effect tick
//! (`native_should_apply_effect_tick_this_tick`, `native_apply_effect_tick`)
//! and their being given (`native_on_effect_added`,
//! `native_mob_effect_added`).
//!
//! API 55 adds moving a block's fluids to another block (`native_move_fluids`, in
//! `machines/native_fluids.rs`: what the target refuses goes back into the
//! source, whole thirds kept).
//!
//! API 54 adds fluid tanks on a mod block's block entity
//! (`native_block_entity_fluids`), and the fluid helpers in
//! `machines/native_fluids.rs`: `native_fluid_tanks`,
//! `native_fluid_insert`, `native_fluid_extract`, which also read and
//! change a cauldron.
//!
//! API 53 adds entities leaving a level (`native_entity_leave_level`), for
//! every kind the join hook tells (see API 47).
//!
//! API 52 adds `native_block_entity_sync`: a mod block entity's block and update tag
//! sent to clients again (`Level.sendBlockUpdated`).
//!
//! API 51 adds `native_menu_button_v2`: a mod container's menu button with the clicking player's cursor stack (`clickMenuButton(Player, int)`).
//!
//! API 50 adds damage a mod can change or cancel (`native_living_incoming_damage`).
//!
//! API 49 adds, in `native_server.rs`, a sidebar and Tab list lines for one
//! player (`native_sidebar`, `native_tab_list`), the server's tick times
//! (`native_tick_stats`), players left out of the sleep count
//! (`native_sleep_ignored`), and where players look (`yaw`, `pitch`).
//!
//! API 48 adds a block's items for server mods (`native_container_slots`,
//! `native_extract_item`, `native_move_items`, and `native_dimension` for a
//! machine's tick, in `machines/native_items.rs`).
//!
//! API 47 adds entities joining a level (`native_entity_join_level`); it tells
//! every kind the game keeps in its entity lists (items, orbs, mobs, boats,
//! minecarts, armor stands, TNT, falling blocks, projectiles, rockets,
//! tridents, end crystals, the dragon, cushions, paintings, item frames).
//!
//! API 46 adds bounded world-space rim and colored-light requests
//! (`native_visuals`) and opt-in texture-matched emissive material
//! requests (`native_emissive`).
//!
//! API 45 sends mod block entities to clients (`native_block_entity_update_tag`).
//!
//! API 44 adds the server starting and stopping (`native_server_about_to_start`,
//! `native_server_stopping`).
//!
//! API 43 adds the server level's random (`native_level_random`).
//!
//! API 42 adds the item entities near a block (`native_item_entities`,
//! `native_set_item_entity`).
//!
//! API 41 adds bundles' data files: `horizon_client::native_data!` compiles a file of a
//! bundle's `data/` folder in, and `native_built_data` is the folder its post-build step wrote.
//!
//! API 40 adds mod messages between a mod's server and client halves
//! (`native_net_from_client`, `native_net_from_server`, and
//! `native_net_send_to_player`, `native_net_send_to_server` in
//! `native_net.rs`), and hearing players and mobs: `native_player_died`,
//! `native_mob_spawned`, `native_mob_killed`, `native_item_dropped`,
//! `native_item_picked_up`.
//!
//! API 39 adds item behaviour: a held item's hit on a mob
//! (`native_hurt_enemy`), a carried stack's tick (`native_inventory_tick`) and
//! an equipment slot taking a different item (`native_on_equip`).
//!
//! API 38 adds redstone from a mod's block (`native_block_signal`,
//! `native_is_signal_source`, and the helper `native_update_signal_neighbors`).
//!
//! API 37 adds mobs for server mods (`native_spawn_mob`, `native_mob`,
//! `native_mobs`, `native_teleport_mob`, `native_push_mob`,
//! `native_set_mob_health`, `native_kill_mob`, `native_remove_mob`... in
//! `native_mobs.rs`).
//!
//! API 36 adds recipes of a mod's own types (`native_recipes_of_type`,
//! `native_recipe_ingredient`, `native_recipe_result`).
//!
//! API 35 adds the commands a mod owns on the client (`native_commands`), so
//! that the chat box colours, completes and lists them like the game's own.
//!
//! API 34 adds scheduled block ticks a mod can handle (`native_tick_block`).
//!
//! API 33 adds random ticks a mod can handle (`native_random_tick`), and
//! `native_set_block_property` on the level, which takes `Level.setBlock`'s
//! `flags` as `Game::native_set_block_property_with` does.
//!
//! API 32 adds shape updates a mod can answer (`native_update_shape`), and
//! `native_schedule_block_tick` and `native_schedule_fluid_tick` on the
//! level for the ticks a block's shape update schedules.
//!
//! API 31 adds a player's server tick (`native_player_tick`).
//!
//! API 30 adds left-clicking a block (`native_left_click_block`).
//!
//! API 29 adds right-clicking an entity (`native_interact_entity`).
//!
//! API 28 adds block and item tag reads for server tick hooks (`native_block_in_tag`,
//! `native_item_in_tag`).
//!
//! API 27 adds the player-health HUD rows (`native_health_hud`).
//!
//! API 26 adds the game's shared config folder (`native_config_dir`).
//!
//! API 25 adds server sound playback (`native_play_sound`).
//!
//! API 24 adds a server block setter (`native_set_block`).
//!
//! API 23 adds per-level server tick hooks (`native_level_tick_pre`,
//! `native_level_tick_post`).
//!
//! API 22 adds what a mod needs to keep the world from changing blocks
//! it guards: fluid and fire spreading (`native_block_spread`), pistons
//! (`native_piston_move`), explosions (`native_explosion_detonate`),
//! buckets (`native_fill_bucket`) and a player's attack on an entity
//! (`native_attack_entity`).
//!
//! API 21 adds right clicking a block (`native_right_click_block`), and in
//! `native_server.rs` the acting player (`native_acting_player`), a player's
//! health, the action bar (`native_action_bar`) and commands run as the
//! server (`native_run_command`).
//!
//! API 20 adds buttons, tooltips and item icons on mod screens
//! (`native_screen_click`, `native_screen_tooltip`, `native_menu_button`,
//! and `native_item`, `native_screen_title`, `native_data`, `LevelRef::block`,
//! `LevelRef::block_property`, `native_insert_item`, `native_cursor`).
//!
//! API 19 adds mod-drawn screens for mod blocks (`native_screen`,
//! `native_screen_draw`, and the `native_sprite` helpers), with the mod's own
//! GUI textures.
//!
//! API 18 adds what a machine's tick reaches: a neighbouring mod block entity
//! (`native_with_mod_block_entity`) and its own block's state
//! (`native_set_block_property`).
//!
//! API 17 adds menus for mod containers (the `menu` of a `ModContainer`: vanilla
//! slot grids and furnaces) and has `native_block_entity_tick` take the whole
//! `ModBlockEntity`, with the menu's data slots.
//!
//! API 16 adds item slots for those block entities
//! (`native_block_entity_container`), which hoppers, comparators and
//! breaking the block use as they use vanilla's containers.
//!
//! API 15 adds block entities for a mod's blocks (`native_block_entity`,
//! `native_block_entity_ticks`, `native_block_entity_tick`): made when the
//! block is placed, ticked on the server and saved with the chunk.
//!
//! API 14 adds breaking and placing a block (`native_destroy_block`,
//! `native_place_block`).
//!
//! API 13 adds using an item (`native_use_item_on`, `native_use_item`).
//!
//! API 12 adds item tooltips (`native_tooltip`).
//!
//! API 11 adds server particle emission (`native_send_particles`).
//!
//! API 10 adds server menus (`native_menu_clicked`, `native_menu_closed`,
//! and `native_open_menu`, `native_exchange_items`... in `native_server.rs`).
//!
//! API 9 adds the acting player's plain items (`native_held`,
//! `native_count_items`, `native_take_items`, `native_give_items`... in
//! `native_server.rs`).
//!
//! API 8 adds server commands and players (`native_register_commands`,
//! `native_perform_command`, and the helpers in `native_server.rs`).
//!
//! API 7 adds block-change observations (`native_block_changed`).
//!
//! API 6 adds neighbour-change observations (`native_neighbor_changed`).
//!
//! API 5 adds read-only server block queries (`native_server_block`).
//!
//! API 4 adds authoritative server tick hooks (`native_server_tick`).
//!
//! API 3 adds the world lifecycle hook (`native_world_lifecycle`).
//!
//! API 2 adds the item panel beside container screens and its recipe pages
//! (`native_panel_items`, `native_panel_recipes`, `native_panel_event`),
//! and the game's items and recipes (`native_recipes`,
//! `native_known_recipes`, `native_place_recipe`, `native_give`): what a
//! recipe viewer needs.
//!
//! Native mods are built against an exact game revision, rather than a hook API revision.
//! Bodies have no `cfg` and no macros: horizon-mixins refuses a target with
//! either (`tests::hook_points_stay_hookable`).

/// Generated dispatch entry points, not hand-written hook implementations.
mod fire;

use super::native_data_maps::NativeDataMapType;
use super::{
    Game, GameLevel, LevelDimension, Screen,
    execution_role::{self, ExecutionRole},
    maps::{NativeMapDecoration, NativeMapInfo},
    mod_api_host,
    native_block_entity_draw::BlockEntityDraw,
    native_configs::{NativeConfig, NativeConfigFile},
    native_mob_draw::{NativeMobRender, NativePlayerRender},
    native_panel_recipes::RecipePage,
};
use horizon_assets::item_model::StackView;
use horizon_client::block_contact::Pending;
use horizon_client::creative::Creative;
use horizon_client::{
    atlas_sources::qualified,
    biome::{BiomeColors, SpawnEntry},
    block_entities::Pos,
    chat::ChatSource,
    containers::{
        BlockEntity, MOD_MENUS, ModBlockEntity, ModBlockEntityFields, ModContainer,
        ModEnergyBuffer, ModFluidTanks,
    },
    crafting::{Ingredient, recipe_result},
    extras::Corner,
    extras_hud::Corners,
    fast_hash::FxHashMap,
    first_person::Hand,
    health::{
        Attack, DamageContainer, DamageHooks, DamageLevel, HurtSource, Hurting, Knock, ShieldBlock,
        Victim,
    },
    hud::{HudBatch, rgba},
    items::ItemStack,
    keybinds,
    lan::menu::Slots,
    merchant::MerchantOffer,
    mobs::{Damage, Who},
    mod_list::{self, Installed, KeyMapping, NativePage},
    music::{Music, MusicLayer},
    particles::{ParticleOptions, ParticleType},
    text::Text,
};
use horizon_core::{
    geometry::Direction,
    position::BlockPos,
    random::LegacyRandom,
    ticks::{ScheduleResult, TickPriority},
};
use horizon_world::level::{BonemealCheck, PlaceContext};
use horizon_world::{
    consumable::UseAnimation,
    fluid::FluidKind,
    level::{BlockId, LevelAccess, PlacementContext, Ticks},
    nbt::{Compound, Tag},
    registry::{BlockRegistry, BlockStateId},
};
use mod_api::__private::game::IngredientTest;
use mod_api::GameMode;
pub use mod_api::game::{NativeClock, NativeShake, PhantomSpawnResult, SleepProblem};
use mod_api::{
    self, Answered, BlockPlacement, ClientInputOptions, ClientModifiers, ClientReloadListeners,
    CommandInvocation, EnchantmentInfo, EntityRef, GameRuleActor, InteractionKey, LayerDefinition,
    LivingRef, NativeCamera, NativeCommand, NativeDispensed, NativeHandRender, NativeKeyMapping,
    NativeLifecycleContext, NativeNoteBlockChange, NativeServerPlayer, PathComputationType,
    Placement, PlayerRef, RecipeBookCategory, ScreenId, ScreenOpening, ScreenshotImage,
    ServerReloadListeners, ServerTickPhase, TicketController, ToolUse, WorldPhase,
};
use mod_api::{
    AttachmentStore, BonemealSource, ChunkRef, FluidStack, GameRuleValue, LandingWrite, LevelRef,
    LogicalSide, NativeActiveEffect, NativeBlockClick, NativeBlockUpdateFlags,
    NativeInteractionResult, NativeItemClick, NativeTriState, NeighbourUpdate, Suggestion,
    TooltipElement, UsePhase,
};
use mod_api::{BlockKey, DebugEntry, EffectKey};
use mod_api::{
    CameraType, MobRef, NativeDespawn, NativePermissionLevel, NativeSpawnCheck, NativeSpawnReason,
    Recipe, SoundPlay,
};
pub(crate) use mod_api::{
    ClientPacket, ClientPayload, NativeBlockRim, NativeColoredLight, NativeExplosionStart,
    NativeItemFished, NativeLootDrops, NativeMinimap, NativePanelEvent, NativePortalShape,
    NativeScreen, NativeScreenView, NativeStack, NativeZoom,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use winit::{event::MouseButton, keyboard::KeyCode};

include!(concat!(env!("OUT_DIR"), "/native_hook_modules.rs"));
include!("shared.rs");

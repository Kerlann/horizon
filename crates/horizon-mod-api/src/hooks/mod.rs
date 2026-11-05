//! Declarations of the mod API's hooks, helpers and draw contexts: the catalog.
//!
//! This module is never compiled (`#[cfg(any())]` in `lib.rs`): `horizon-mod-api-gen` reads these
//! files with `syn` and builds the `Hooks` trait, its dispatch, the `Host` traits, the game's entry
//! points and the rustdoc from them. They are in `mod` so that `rustfmt` formats them.
//!
//! Each area file holds up to three traits, each declaration a bodiless `fn name();` with its rustdoc:
//!
//! - `Hooks`: `#[hook(side = "...", contract = "...", ...)]`, one per moment the game tells mods about.
//! - `Helpers`: `#[helper(side = "...", ...)]`, a game function mods reach through the `Host` or a handle.
//! - `DrawContexts`: `#[draw_context(surface = "...", name = "...", prefix = "...")]`, a drawing surface.
//!
//! A hook's arguments:
//!
//! | Argument | Meaning |
//! |---|---|
//! | `side` | `"server"`, `"client"` or `"both"`; required |
//! | `contract` | `"listen"`, `"cancel"`, `"collect"` or `"replace"`; required |
//! | `out` | the event field a cancel, collect or replace hook writes; not for `"listen"` |
//! | `scope`, `key` | a Part method: the kind of Part (`"block"`, `"block_entity"`, `"item"`, `"menu"`) and the hook argument naming what it is attached to |
//! | `answered_by` | `"mod"`: a global replace hook whose output also names the mod that answered |
//! | `host`, `context` | `"read"` or `"level"`; `"registration"`, `"data_load"` or `"loop"` |
//! | `neoforge`, `fabric`, `paper` | the counterpart's name, or a list of them, for the rustdoc and the gap lists |
//! | `cancellable` | whether the NeoForge counterpart event is cancellable (documentation, not the contract) |
//! | `version` | the contract version an injection's `hook_version` names; left out when it is 1 |
//! | `target` | the game function that fires it; left out when it is `Game::native_<name>` in `native_hooks.rs` |
//!
//! A helper's arguments are `side`, `target` (left out when it is `Game::native_<name>`),
//! `client_target` (a both-side helper whose client implementation differs), `receiver` (the handle it
//! is also a method of) and `backend = true`.
//!
//! Every hook needs `///` lines: they are the rustdoc of its trait method, and a hook without them
//! is refused. The generator reads the files in name order, `base.rs` first, and each file in declaration
//! order; that is the order of the generated `HookId`, the dispatch and the WIT, and it comes from
//! the directory, so a new hook never edits an existing line to be placed. A new hook goes in a file
//! of its own (or at the end of the file of its family). The `mod` lines below exist only so that
//! rustfmt formats the files: keep them in the same order, one per file; the generator refuses a
//! list that differs from the directory. The JSON catalog (`native-hooks.json`,
//! `tools/native-hook-fragments/`) is retired and a file there fails the build.
//!
//! The game function a hook names starts as `let _ = (every, parameter);`; `cargo run -p xtask --
//! mod-api` fills in the dispatch call.

#![rustfmt::skip]

mod base;
mod add_client_reload_listeners;
mod add_server_reload_listeners;
mod affect_neighbors_after_removal;
mod animal_tame;
mod anvil_update;
mod armor_hurt;
mod arrow_nock;
mod baby_entity_spawn;
mod block_drops;
mod block_entity_refs;
mod block_entity_view_distance;
mod block_face_sprite;
mod block_factors;
mod block_grow_feature;
mod block_landing;
mod block_render_visibility;
mod block_state_facts;
mod block_state_ids;
mod block_tool_modification;
mod block_use_item_on;
mod block_use_without_item;
mod bonemeal;
mod boss_bar_refs;
mod calculate_player_turn;
mod camera_pose;
mod camera_read;
mod can_continue_sleeping;
mod can_player_sleep;
mod chunk_forced;
mod chunk_refs;
mod chunk_watch;
mod click_menu_button;
mod client_chat;
mod client_chat_received;
mod client_information_updated;
mod client_input_reads;
mod client_packet;
mod client_pause_change;
mod client_pause_read;
mod client_player_change_game_type;
mod client_player_clone;
mod client_player_logging_in;
mod client_player_logging_out;
mod client_services;
mod client_setup;
mod client_stopped;
mod client_stopping;
mod client_tick_post;
mod client_tick_pre;
mod clipboard;
mod clone_item_stack;
mod compute_fov_modifier;
mod connects_to;
mod contents_changed;
mod create_fluid_source;
mod create_spawn_position;
mod crop_grow;
mod crosshair_target;
mod custom_click_action;
mod custom_spawners;
mod display_refs;
mod dragon_refs;
mod draw_animation;
mod draw_measures;
mod enchantment_level_set;
mod enchantments;
mod ender_entity_teleport;
mod enderman_anger;
mod energy_capability;
mod entity_data;
mod entity_fall_and_push;
mod entity_handles;
mod entity_inside;
mod entity_invulnerable;
mod entity_mob_griefing;
mod entity_motion;
mod entity_moves;
mod entity_outline;
mod entity_place;
mod entity_remove;
mod entity_riding;
mod entity_sounds;
mod entity_state;
mod entity_struck_by_lightning;
mod explosion_knockback;
mod explosion_start;
mod farmland_trample;
mod finalize_spawn;
mod fluid_capability;
mod fluid_draw;
mod fluid_flow;
mod fluid_place_block;
mod game_rule_changed;
mod game_shutting_down;
mod get_arm_pose;
mod get_enchantment_level;
mod grindstone_place;
mod grindstone_take;
mod gui_render_post;
mod hand_drawn;
mod host_send_map;
mod ingredient_test;
mod is_pathfindable;
mod item_attribute_modifier;
mod item_capability;
mod item_components;
mod item_container;
mod item_entity_refs;
mod item_fished;
mod item_model_property;
mod item_places;
mod item_smelted;
mod item_stacked_on_other;
mod item_tooltip;
mod item_toss;
mod left_click_empty;
mod level_block_writes;
mod level_craft;
mod level_effects;
mod level_entities_in;
mod level_events;
mod level_item_entities;
mod level_neighbor_updates;
mod level_reads;
mod level_save;
mod level_shapes;
mod level_signals;
mod level_sounds;
mod level_world;
mod living_conversion_post;
mod living_conversion_pre;
mod living_damage_post;
mod living_damage_pre;
mod living_death;
mod living_destroy_block;
mod living_drops;
mod living_effects;
mod living_experience_drop;
mod living_get_projectile;
mod living_health;
mod living_health_writes;
mod living_motion_values;
mod living_refs;
mod living_shield_block;
mod living_use_totem;
mod living_visibility;
mod living_writes;
mod menu_extra_data;
mod menu_refs;
mod menu_slots;
mod merchant_refs;
mod minecart_pass;
mod minecart_refs;
mod mob_despawn;
mod mob_effect_events;
mod mob_goals;
mod mob_navigation;
mod mob_refs;
mod mob_spawn_checks;
mod mob_split;
mod mod_configs;
mod movement_input;
mod native_block_sprites;
mod native_note_block_change;
mod native_potential_spawns;
mod native_village_siege;
mod neighbor_notify;
mod note_block_play;
mod on_item_use_first;
mod permissions_changed;
mod player_abilities_write;
mod player_actions;
mod player_break_speed;
mod player_brewed_potion;
mod player_cooldowns;
mod player_criteria;
mod player_death_location;
mod player_enchant_item;
mod player_flyable_fall;
mod player_food;
mod player_game_mode_write;
mod player_harvest_check;
mod player_item_smithed;
mod player_modes;
mod player_name_format;
mod player_read;
mod player_set_spawn;
mod player_spawn_phantoms;
mod player_stats;
mod player_stays_on_ground_surface;
mod player_tab_list_name_format;
mod player_xp_change;
mod player_xp_give;
mod player_xp_level_change;
mod portal_spawn;
mod potion_brew_post;
mod potion_brew_pre;
mod pre_remove_side_effects;
mod projectile_refs;
mod raid_refs;
mod recipe_book_categories;
mod recipe_property_sets;
mod reload_resources;
mod render_arm;
mod render_frame_post;
mod render_frame_pre;
mod render_gui_post;
mod render_gui_pre;
mod render_item_in_frame;
mod render_name_tag;
mod render_tooltip_gather_components;
mod render_tooltip_pre;
mod right_click_empty;
mod right_click_item;
mod scenes_in_screens;
mod screen_closing;
mod screen_exclusion_areas;
mod screen_hovered_item;
mod screen_init_post;
mod screen_input_post;
mod screen_opening;
mod screen_preedit_effects;
mod screen_render_stages;
mod send_chat;
mod send_packet;
mod server_chat;
mod server_player_entity_id;
mod should_check_weak_power;
mod should_redstone_wire_connect_to;
mod sound_events;
mod sound_refs;
mod spread_players_teleport;
mod structure_template;
mod sweep_attack;
mod tag_fluid_reads;
mod teleport_command;
mod toast_add;
mod tooltip_color;
mod transactions;
mod translate;
mod use_item_on_block;
mod validate_tickets;
mod vanilla_game_event;
mod window_resize;
mod xp_orb_targeting;

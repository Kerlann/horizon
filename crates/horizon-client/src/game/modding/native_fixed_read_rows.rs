//! Resident-source snapshot producers, not cached-row backend accessors.
//! Call only against the owning source lane; these helpers preserve scalar domain absence.
//! The publication pass validates identity, liveness and source/handle correspondence
//! before lending a direct source. Related identities are resolved by its shared index.
use super::{Game, commands::Target, native_server::PlayerRead};
use horizon_core::position::BlockPos;
use mod_api::*;

impl Game<'_> {
    /// Source-lane projection for a mob that may currently be lifted from its list.
    /// Identity and cross-entity credit are resolved by the publication pass;
    /// item use is supplied from its already resolved held-stack projection.
    /// Portal eligibility includes pre-indexed rider flags, supplied by the pass.
    /// Climbing samples the explicitly borrowed source world, including a
    /// parked level, without swapping dimensions or resolving the mob from Game.
    pub(super) fn snapshot_mob_living_read_row(
        &self,
        capture: EntityReadCapture,
        living: LivingRef,
        (mob, world): (&horizon_client::mobs::Mob, &horizon_client::world::World),
        kill_credit: Option<EntityRef>,
        last_attacker: Option<EntityRef>,
        item_use_remaining: i32,
        can_use_portal: bool,
    ) -> Option<LivingReadRow> {
        if mob.removed {
            return None;
        }
        let tamable = mob.is_tamable();
        let using = horizon_client::mobs::spear_use::is_using_item(mob);
        Some(LivingReadRow {
            capture,
            entity: living.entity(),
            gliding_ticks: Some(mob.extra.fall_fly_ticks),
            is_gliding: Some(mob.extra.fall_flying),
            is_sleeping: Some(horizon_client::mobs::villager::sleeping(mob)),
            sleeping_pos: horizon_client::mobs::villager::sleeping_pos(mob),
            is_jumping: Some(false),
            is_climbing: Some(self.native_mob_is_climbing_in(mob, world)),
            baby: Some(mob.extra.age < 0),
            health: Some(mob.health),
            max_health: Some(mob.attrs.max_health),
            alive: Some(mob.health > 0.0),
            kill_credit,
            last_attacker,
            is_tamed: tamable.then_some(mob.extra.tame),
            owner: if tamable {
                mob.extra
                    .owner
                    .map(horizon_client::lan::playerdata::ints_uuid)
            } else {
                None
            },
            item_use_remaining: Some(item_use_remaining),
            used_item_hand: using.then_some(if mob.extra.using_off_hand {
                Hand::Off
            } else {
                Hand::Main
            }),
            can_use_portal: Some(can_use_portal),
            can_breathe_underwater: Some(mob.kind.flags.breathes_water),
            arrow_count: Some(mob.extra.stuck.arrows),
            stinger_count: Some(mob.extra.stuck.stingers),
        })
    }

    pub(super) fn snapshot_dragon_living_read_row(
        &self,
        capture: EntityReadCapture,
        living: LivingRef,
        source: &super::dragon::Dragon,
    ) -> Option<LivingReadRow> {
        Some(LivingReadRow {
            capture,
            entity: living.entity(),
            gliding_ticks: None,
            is_gliding: None,
            is_sleeping: None,
            sleeping_pos: None,
            is_jumping: None,
            is_climbing: None,
            baby: Some(false),
            health: Some(source.health),
            max_health: Some(super::dragon::MAX_HEALTH),
            alive: Some(source.health > 0.0),
            kill_credit: None,
            last_attacker: None,
            is_tamed: None,
            owner: None,
            item_use_remaining: None,
            used_item_hand: None,
            can_use_portal: None,
            can_breathe_underwater: None,
            arrow_count: None,
            stinger_count: None,
        })
    }

    pub(super) fn snapshot_stand_living_read_row(
        &self,
        capture: EntityReadCapture,
        living: LivingRef,
        source: &horizon_client::entities::ArmorStand,
    ) -> LivingReadRow {
        LivingReadRow {
            capture,
            entity: living.entity(),
            gliding_ticks: None,
            is_gliding: None,
            is_sleeping: None,
            sleeping_pos: None,
            is_jumping: None,
            is_climbing: None,
            baby: Some(false),
            health: Some(source.health),
            max_health: Some(20.0),
            alive: Some(source.health > 0.0),
            kill_credit: None,
            last_attacker: None,
            is_tamed: None,
            owner: None,
            item_use_remaining: None,
            used_item_hand: None,
            can_use_portal: None,
            can_breathe_underwater: None,
            arrow_count: None,
            stinger_count: None,
        }
    }

    /// Resident fallback for players and armor stands. Mob and lifted-dragon
    /// passes must use the direct source variants above instead of this lookup path.
    pub(super) fn snapshot_living_read_row(
        &self,
        capture: EntityReadCapture,
        living: LivingRef,
    ) -> Option<LivingReadRow> {
        match self.native_living_target(living)? {
            Target::Player | Target::Guest(_) | Target::Stand(_) => {}
            _ => return None,
        }
        // The scalar getters require &mut solely to insert missing guest combat.
        // A producer must not mutate or invent a missing record.
        let stuck = match self.native_living_target(living)? {
            Target::Player => Some(&self.combat.stuck),
            Target::Guest(conn) => self.guest_combat.get(&conn).map(|c| &c.stuck),
            _ => None,
        };
        Some(LivingReadRow {
            capture,
            entity: living.entity(),
            gliding_ticks: self.uncached_living_gliding_ticks(living),
            is_gliding: self.uncached_living_is_gliding(living),
            is_sleeping: self.uncached_living_is_sleeping(living),
            sleeping_pos: self.uncached_living_sleeping_pos(living),
            is_jumping: self.uncached_living_is_jumping(living),
            is_climbing: self.uncached_living_is_climbing(living),
            baby: self.uncached_living_baby(living),
            health: self.uncached_living_health(living),
            max_health: self.uncached_living_max_health(living),
            alive: self.uncached_living_alive(living),
            kill_credit: self.uncached_living_kill_credit(living),
            last_attacker: self.uncached_living_last_attacker(living),
            is_tamed: self.uncached_living_is_tamed(living),
            owner: self.uncached_living_owner(living),
            item_use_remaining: self.uncached_living_item_use_remaining(living),
            used_item_hand: self.uncached_living_used_item_hand(living),
            can_use_portal: self.uncached_living_can_use_portal(living),
            can_breathe_underwater: self.uncached_living_can_breathe_underwater(living),
            arrow_count: stuck.map(|s| s.arrows),
            stinger_count: stuck.map(|s| s.stingers),
        })
    }

    /// Borrow saved NBT directly. `block_state` is resolved once by the caller's
    /// registry projection, without cloning or reparsing the whole display tag.
    pub(super) fn snapshot_display_read_row(
        &self,
        capture: EntityReadCapture,
        display: DisplayRef,
        kind: NativeDisplayKind,
        tag: &horizon_core::nbt::Tag,
        block_state: Option<BlockState>,
    ) -> Option<DisplayReadRow> {
        use horizon_client::save::{float, number};
        use horizon_core::nbt::Tag;
        let int = |key: &str, default| number(tag.get(key)).map(|n| n as i32).unwrap_or(default);
        let real = |key: &str, default| float(tag.get(key)).map(|n| n as f32).unwrap_or(default);
        let named = |key: &str, name: &str| matches!(tag.get(key), Some(Tag::String(s)) if s.0.iter().copied().eq(name.encode_utf16()));
        let text = kind == NativeDisplayKind::Text;
        let billboard = NativeBillboard::ALL
            .into_iter()
            .find(|b| named("billboard", b.name()))
            .unwrap_or(NativeBillboard::Fixed);
        let brightness = tag.get("brightness").and_then(|b| {
            let block = number(b.get("block")).filter(|v| (0..=15).contains(v))?;
            let sky = number(b.get("sky")).filter(|v| (0..=15).contains(v))?;
            Some((block as u8, sky as u8))
        });
        let (translation, left_rotation, scale, right_rotation) = tag
            .get("transformation")
            .and_then(horizon_client::text_display::transformation)
            .unwrap_or((
                [0.0; 3],
                [0.0, 0.0, 0.0, 1.0],
                [1.0; 3],
                [0.0, 0.0, 0.0, 1.0],
            ));
        Some(DisplayReadRow {
            capture,
            entity: display.entity(),
            kind: Some(kind),
            transformation: Some(NativeTransformation {
                translation,
                left_rotation,
                scale,
                right_rotation,
            }),
            billboard: Some(billboard),
            brightness,
            view_range: Some(real("view_range", 1.0)),
            shadow_radius: Some(real("shadow_radius", 0.0)),
            shadow_strength: Some(real("shadow_strength", 1.0)),
            width: Some(real("width", 0.0)),
            height: Some(real("height", 0.0)),
            glow_color_override: Some(int("glow_color_override", -1)),
            interpolation_duration: Some(int("interpolation_duration", 0)),
            start_interpolation: Some(int("start_interpolation", 0)),
            teleport_duration: Some(int("teleport_duration", 0).clamp(0, 59)),
            line_width: text.then(|| int("line_width", 200)),
            background: text.then(|| int("background", 0x4000_0000)),
            text_opacity: text.then(|| int("text_opacity", -1) as i8),
            text_shadow: text.then(|| int("shadow", 0) != 0),
            see_through: text.then(|| int("see_through", 0) != 0),
            default_background: text.then(|| int("default_background", 0) != 0),
            alignment: text.then(|| {
                if named("alignment", "left") {
                    NativeTextAlignment::Left
                } else if named("alignment", "right") {
                    NativeTextAlignment::Right
                } else {
                    NativeTextAlignment::Center
                }
            }),
            block_state: if kind == NativeDisplayKind::Block {
                block_state
            } else {
                None
            },
            item_context: (kind == NativeDisplayKind::Item).then(|| {
                NativeItemDisplayContext::ALL
                    .into_iter()
                    .find(|c| named("item_display", c.name()))
                    .unwrap_or(NativeItemDisplayContext::None)
            }),
        })
    }

    pub(super) fn snapshot_dragon_read_row(
        &self,
        capture: EntityReadCapture,
        dragon: DragonRef,
        source: &super::dragon::Dragon,
        fight: Option<&super::dragon::Fight>,
    ) -> Option<DragonReadRow> {
        Some(DragonReadRow {
            capture,
            entity: dragon.entity(),
            phase: Some(source.native_phase()),
            alive_crystals: fight.map(|f| f.alive_crystals),
            previously_killed: fight.map(|f| f.previously_killed),
        })
    }

    pub(super) fn snapshot_item_entity_read_row(
        &self,
        capture: EntityReadCapture,
        item: ItemEntityRef,
        source: &horizon_client::entities::ItemEntity,
        owner: Option<EntityRef>,
    ) -> Option<ItemEntityReadRow> {
        let pickup_delay = if source.pickup_delay == i64::from(u32::MAX) {
            Some(i32::from(i16::MAX))
        } else {
            i32::try_from(source.pickup_delay).ok()
        };
        Some(ItemEntityReadRow {
            capture,
            entity: item.entity(),
            pickup_delay,
            age: Some(source.age),
            target: source
                .target
                .map(horizon_client::lan::playerdata::ints_uuid),
            owner,
        })
    }

    /// `source` is None for the separate thrown-trident and rocket lanes.
    /// The caller supplies the already resolved optional owner from the pass index.
    pub(super) fn snapshot_projectile_read_row(
        &self,
        capture: EntityReadCapture,
        projectile: ProjectileRef,
        source: Option<&horizon_client::projectiles::Projectile>,
        owner: Option<EntityRef>,
    ) -> Option<ProjectileReadRow> {
        use horizon_client::{arrows::Pickup, projectiles::ProjectileKind};
        let arrow = source.and_then(|p| p.arrow.as_ref());
        Some(ProjectileReadRow {
            capture,
            entity: projectile.entity(),
            owner,
            pickup: arrow.map(|a| match a.pickup {
                Pickup::Disallowed => NativeArrowPickup::Disallowed,
                Pickup::Allowed => NativeArrowPickup::Allowed,
                Pickup::CreativeOnly => NativeArrowPickup::CreativeOnly,
            }),
            pierce_level: arrow
                .map(|a| a.pierce as i8)
                .ok_or(PierceLevelError::NotArrow),
            base_damage: source.and_then(|p| match p.kind {
                ProjectileKind::Arrow { base_damage, .. } => Some(base_damage),
                _ => None,
            }),
            critical: source
                .and_then(|p| matches!(p.kind, ProjectileKind::Arrow { .. }).then(|| p.critical())),
        })
    }

    pub(super) fn snapshot_fishing_hook_read_row(
        &self,
        capture: EntityReadCapture,
        hook: FishingHookRef,
        source: &super::fishing::Hook,
        player_owner: Option<EntityRef>,
        hooked_in: Option<EntityRef>,
    ) -> Option<FishingHookReadRow> {
        Some(FishingHookReadRow {
            capture,
            entity: hook.entity(),
            player_owner,
            hooked_in,
            open_water: Some(source.open_water()),
        })
    }

    /// `living_health` is accumulated by the pass while visiting raid members,
    /// not by rescanning the mob list once for every raid.
    pub(super) fn snapshot_raid_read_row(
        &self,
        capture: EntityReadCapture,
        raid: RaidRef,
        source: &horizon_client::raid::Raid,
        living_health: f32,
    ) -> Option<RaidReadRow> {
        use horizon_client::raid::Status;
        Some(RaidReadRow {
            capture,
            raid,
            center: Some(source.center),
            omen_level: Some(source.omen_level),
            groups_spawned: Some(source.groups_spawned),
            num_groups: Some(source.num_groups),
            has_bonus_wave: Some(source.omen_level > 1),
            is_started: Some(source.started),
            is_active: Some(source.active),
            status: Some(match source.status {
                Status::Ongoing => NativeRaidStatus::Ongoing,
                Status::Victory => NativeRaidStatus::Victory,
                Status::Loss => NativeRaidStatus::Loss,
                Status::Stopped => NativeRaidStatus::Stopped,
            }),
            total_health: Some(source.total_health),
            living_health: Some(living_health),
            bar_progress: Some(source.bar.progress),
        })
    }

    /// The pool belongs to the publication cache and is reused between passes.
    /// Only the first observation of a dimension allocates; retained identifiers
    /// and materialized rows share the Identifier's existing Arc allocation.
    pub(super) fn snapshot_player_read_row(
        &self,
        capture: EntityReadCapture,
        player: PlayerRef,
        identifiers: &mut NativeReadIdentifiers,
    ) -> Option<NativePlayerReadSnapshot> {
        let read = self.player_read_both(player)?;
        let death = match &read {
            PlayerRead::Acting { last_death, .. } => *last_death,
            PlayerRead::Guest(g) => g.last_death.as_ref(),
        };
        let last_death_location = death.and_then(|(dim, pos)| identifiers.position(dim, *pos));
        let respawn_config = self.player_handle_read(player).and_then(|read| {
            let saved = match read {
                PlayerRead::Acting { .. } => self
                    .respawn_config
                    .as_ref()
                    .map(|c| (c.dim.id(), c.pos, c.yaw, c.forced)),
                PlayerRead::Guest(g) => g
                    .respawn_elsewhere
                    .as_ref()
                    .map(|(d, p, y, f)| (d.as_str(), *p, *y, *f)),
            };
            let Some((dim, pos, yaw, forced)) = saved else {
                return Some(None);
            };
            Some(Some(PlayerRespawnConfig {
                position: identifiers.position(dim, pos)?,
                yaw,
                forced,
            }))
        });
        Some(NativePlayerReadSnapshot(
            PlayerReadRow {
                capture,
                entity: player.entity(),
                selected_slot: self.uncached_player_selected_slot(player),
                container_menu: self.uncached_player_container_menu(player),
                exp_cooldown: self.uncached_player_exp_cooldown(player),
                last_death_location,
                needs_food: self.uncached_player_needs_food(player),
                has_enough_food: self.uncached_player_has_enough_food(player),
                can_eat: [
                    self.uncached_player_can_eat(player, false)?,
                    self.uncached_player_can_eat(player, true)?,
                ],
                experience: self.uncached_player_experience(player),
                xp_needed_for_next_level: self.uncached_player_xp_needed_for_next_level(player),
                permission_level: self.uncached_player_permission_level(player),
                has_infinite_materials: self.uncached_player_has_infinite_materials(player),
                can_use_game_master_blocks: self.uncached_player_can_use_game_master_blocks(player),
                previous_game_mode: self.uncached_player_previous_game_mode(player),
                is_secondary_use_active: self.uncached_player_is_secondary_use_active(player),
                is_fake: self.uncached_player_is_fake(player),
                respawn_config,
                text_filtering: self.uncached_player_text_filtering(player),
                sleep_ignored: self.uncached_player_sleep_ignored(player),
                counts_for_sleep: self.uncached_player_counts_for_sleep(player),
                sleep_timer: self.uncached_player_sleep_timer(player),
                game_mode: self.uncached_player_game_mode(player),
                is_creative: self.uncached_player_is_creative(player),
                is_spectator: self.uncached_player_is_spectator(player),
                may_build: self.uncached_player_may_build(player),
                food: self.uncached_player_food(player),
                abilities: self.uncached_player_abilities(player),
                block_breaking: self.uncached_player_block_breaking(player),
                ambient_mood: self.uncached_player_ambient_mood(player),
            },
            player,
        ))
    }
}

/// Retained player fields have no per-pass owning clones: Identifier is Arc-backed.
/// Keep the row private so owned public materialization stays explicit.
#[derive(Clone)]
pub(super) struct NativePlayerReadSnapshot(pub(super) PlayerReadRow, pub(super) PlayerRef);
impl NativePlayerReadSnapshot {
    pub(super) fn materialize(&self) -> PlayerReadRow {
        self.0.clone()
    }
}

#[derive(Default)]
pub(super) struct NativeReadIdentifiers(std::collections::HashMap<String, Identifier>);
impl NativeReadIdentifiers {
    fn position(&mut self, dimension: &str, [x, y, z]: [i32; 3]) -> Option<GlobalPos> {
        let id = if let Some(id) = self.0.get(dimension) {
            id.clone()
        } else {
            let id = Identifier::parse(dimension).ok()?;
            self.0.insert(dimension.to_owned(), id.clone());
            id
        };
        Some(GlobalPos::new(id, BlockPos::new(x, y, z)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_pool_preserves_unknown_dimensions_and_positions() {
        let mut pool = NativeReadIdentifiers::default();
        let first = pool.position("mod:unloaded", [2, -8, 17]).unwrap();
        let second = pool.position("mod:unloaded", [9, 4, -3]).unwrap();
        assert_eq!(first.dimension().to_string(), "mod:unloaded");
        assert_eq!(first.pos(), BlockPos::new(2, -8, 17));
        assert_eq!(second.pos(), BlockPos::new(9, 4, -3));
        assert_eq!(pool.0.len(), 1);
        assert!(pool.position("invalid space:dimension", [0; 3]).is_none());
        assert_eq!(pool.0.len(), 1);
    }
}

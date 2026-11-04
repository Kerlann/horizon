//! A player's food and experience for native server mods, through a
//! `PlayerRef` (docs/modding/src/how-to/native-mods.md): the `FoodData` and `Player` reads the
//! handle's `food()` doesn't give, the writes, and the experience give an
//! orb hands over (Mending first). Reads return None for a
//! stale handle or a player who isn't online here, as `food()` does. Writes
//! return false for those, for a dead player, and, inside another player's
//! turn, for anyone but that player. What changes reaches the player's client as any
//! change to their food or experience does.
use super::Game;
use horizon_client::entities::Experience;
use mod_api::{NativePlayerExperience, PlayerRef};

impl Game<'_> {
    /// Whether `player` still names the same player on this server.
    fn native_player_valid(&self, player: PlayerRef) -> bool { if let Some(__l) = crate::points_generated::P4068.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.player_handle_read(player).is_some()
    }
    /// Runs `f` with `player`'s state in place, when the handle is current
    /// and they are alive (`native_as_player`).
    fn native_player_write(&mut self, player: PlayerRef, f: impl FnOnce(&mut Self)) -> bool {
        self.native_player_valid(player) && self.native_as_player_handle(player, f)
    }
    /// The acting player's food data, inside `native_player_write`.
    fn native_food_mut(&mut self) -> Option<&mut horizon_world::food::FoodData> { if let Some(__l) = crate::points_generated::P4067.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        self.player.as_mut().map(|p| &mut p.health.food)
    }

    /// `FoodData.needsFood`: the food level is below 20.
    pub(super) fn native_player_needs_food(&self, player: PlayerRef) -> Option<bool> { if let Some(__l) = crate::points_generated::P4066.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(self.native_player_food(player)?.level < 20)
    }
    /// `FoodData.hasEnoughFood`: the food level is above 6, enough to
    /// sprint.
    pub(super) fn native_player_has_enough_food(&self, player: PlayerRef) -> Option<bool> { if let Some(__l) = crate::points_generated::P4065.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(self.native_player_food(player)?.level as f32 > 6.0)
    }
    /// `Player.canEat(always)`: an invulnerable player (creative), `always`
    /// (food that can always be eaten) or a food level below 20.
    pub(super) fn native_player_can_eat(&self, player: PlayerRef, always: bool) -> Option<bool> { let (always,) = match crate::points_generated::P4064.get() { Some(__l) => { let mut __a = (always,); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); __a } None => (always,) };
        let invulnerable = self.native_player_abilities(player)?.invulnerable;
        Some(invulnerable || always || self.native_player_needs_food(player)?)
    }
    /// `FoodData.eat(nutrition, saturationModifier)`: adds `nutrition` to
    /// the food level (0 to 20) and `nutrition * modifier * 2` to the
    /// saturation, which stays at most the food level.
    pub(super) fn native_player_feed(
        &mut self,
        player: PlayerRef,
        nutrition: i32,
        saturation_modifier: f32,
    ) -> bool { let (nutrition,saturation_modifier,) = match crate::points_generated::P4063.get() { Some(__l) => { let mut __a = (nutrition,saturation_modifier,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (nutrition,saturation_modifier,) };
        self.native_player_write(player, |game| {
            if let Some(food) = game.native_food_mut() {
                food.eat(nutrition, saturation_modifier);
            }
        })
    }
    /// `Player.causeFoodExhaustion`: adds exhaustion (at most 40), as
    /// sprinting or jumping does. An invulnerable player (creative) or a
    /// spectator gets none, and the call still returns true.
    pub(super) fn native_player_add_exhaustion(&mut self, player: PlayerRef, amount: f32) -> bool { let (amount,) = match crate::points_generated::P4062.get() { Some(__l) => { let mut __a = (amount,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (amount,) };
        self.native_player_write(player, |game| {
            if let Some(p) = game.player.as_mut() {
                p.cause_food_exhaustion(amount);
            }
        })
    }
    /// `FoodData.setFoodLevel`: sets the food level as it is, unclamped,
    /// as vanilla does.
    pub(super) fn native_player_set_food_level(&mut self, player: PlayerRef, level: i32) -> bool { let (level,) = match crate::points_generated::P4061.get() { Some(__l) => { let mut __a = (level,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (level,) };
        self.native_player_write(player, |game| {
            if let Some(food) = game.native_food_mut() {
                food.food = level;
            }
        })
    }
    /// `FoodData.setSaturation`: sets the saturation as it is, unclamped.
    pub(super) fn native_player_set_saturation(
        &mut self,
        player: PlayerRef,
        saturation: f32,
    ) -> bool { let (saturation,) = match crate::points_generated::P4060.get() { Some(__l) => { let mut __a = (saturation,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (saturation,) };
        self.native_player_write(player, |game| {
            if let Some(food) = game.native_food_mut() {
                food.saturation = saturation;
            }
        })
    }
    /// Sets the exhaustion as it is (Paper's `HumanEntity#setExhaustion`;
    /// vanilla's `FoodData` only adds to it).
    pub(super) fn native_player_set_exhaustion(
        &mut self,
        player: PlayerRef,
        exhaustion: f32,
    ) -> bool { let (exhaustion,) = match crate::points_generated::P4059.get() { Some(__l) => { let mut __a = (exhaustion,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (exhaustion,) };
        self.native_player_write(player, |game| {
            if let Some(food) = game.native_food_mut() {
                food.exhaustion = exhaustion;
            }
        })
    }

    /// `Player.experienceLevel`, `experienceProgress` and `totalExperience`.
    pub(super) fn native_player_experience(
        &self,
        player: PlayerRef,
    ) -> Option<NativePlayerExperience> { if let Some(__l) = crate::points_generated::P4058.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(match self.player_read_both(player)? {
            super::native_server::PlayerRead::Guest(g) => {
                NativePlayerExperience::from(mod_api::__private::built::NativePlayerExperience {
                    level: g.xp_level,
                    progress: g.xp_progress,
                    total: g.xp_total,
                })
            }
            super::native_server::PlayerRead::Acting { .. } => {
                NativePlayerExperience::from(mod_api::__private::built::NativePlayerExperience {
                    level: self.experience.level,
                    progress: self.experience.progress,
                    total: self.experience.total,
                })
            }
        })
    }
    /// `Player.getXpNeededForNextLevel`: the points the current level needs
    /// to fill.
    pub(super) fn native_player_xp_needed_for_next_level(&self, player: PlayerRef) -> Option<i32> { if let Some(__l) = crate::points_generated::P4057.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(Experience::needed(
            self.native_player_experience(player)?.level,
        ))
    }
    /// `ServerPlayer.setExperiencePoints`: sets the progress toward the next
    /// level to `points` of the points it needs, kept below a full bar. The
    /// level and the total are kept.
    pub(super) fn native_player_set_experience_points(
        &mut self,
        player: PlayerRef,
        points: i32,
    ) -> bool { let (points,) = match crate::points_generated::P4056.get() { Some(__l) => { let mut __a = (points,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (points,) };
        self.native_player_write(player, |game| {
            let limit = Experience::needed(game.experience.level) as f32;
            let max = (limit - 1.0) / limit;
            // Mth.clamp; `limit` is at least 7, so nothing here is NaN.
            let progress = points as f32 / limit;
            game.experience.progress = if progress < 0.0 {
                0.0
            } else {
                progress.min(max)
            };
        })
    }
    /// `ServerPlayer.setExperienceLevels`: sets the level, keeping the
    /// progress and the total.
    pub(super) fn native_player_set_experience_levels(
        &mut self,
        player: PlayerRef,
        levels: i32,
    ) -> bool { let (levels,) = match crate::points_generated::P4055.get() { Some(__l) => { let mut __a = (levels,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (levels,) };
        self.native_player_write(player, |game| game.experience.level = levels)
    }
    /// `ExperienceOrb.playerTouch`: Mending repairs the player's damaged
    /// items with `points` first (`ExperienceOrb.repairPlayerItems`), then
    /// only the rest goes to their experience
    /// (`Player.giveExperiencePoints`), with the level-up sound. No orb
    /// pickup sound. False, and nothing given, as the other writes; `points`
    /// above `i32::MAX` is refused before Mending repairs anything.
    pub(super) fn native_player_give_experience_points(
        &mut self,
        player: PlayerRef,
        points: u32,
    ) -> bool { let (points,) = match crate::points_generated::P4054.get() { Some(__l) => { let mut __a = (points,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (points,) };
        // `Player.giveExperiencePoints` takes an int.
        let Ok(points) = i32::try_from(points) else {
            return false;
        };
        self.native_player_write(player, |game| {
            let rest = if points > 0 {
                game.mending(points as u32)
            } else {
                0
            };
            if rest > 0 {
                // `rest` is at most `points`, which fits an int.
                game.give_experience_points(rest as i32);
            }
        })
    }
    /// `Player.giveExperienceLevels`: adds `levels`, or takes them off when
    /// negative, as the anvil and the enchanting table charge with. Below 0
    /// the level, the total and the progress toward the next level are all 0.
    pub(super) fn native_player_give_levels(&mut self, player: PlayerRef, levels: i32) -> bool { let (levels,) = match crate::points_generated::P4053.get() { Some(__l) => { let mut __a = (levels,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (levels,) };
        self.native_player_write(player, |game| game.give_levels(levels))
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::*;
    use horizon_client::{items::ItemStack, motion::PlayerMotion};
    use horizon_core::geometry::Vec3;
    use mod_api::{EntityRef, NativeWorldEpoch};

    /// The acting player with a body, as a hook's turn finds them.
    fn acting_player(game: &mut super::Game<'_>) -> PlayerRef {
        game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
        game.lan_acting_uuid = Some(123);
        game.lan_acting_entity = Some(20);
        game.native_player_ref(123).unwrap()
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn points_repair_a_damaged_mending_item_before_experience() {
        with_game(|game| {
            let player = acting_player(game);
            let mut sword = ItemStack::new("minecraft:diamond_sword", 1);
            sword.damage = 1000;
            sword.enchant("minecraft:mending", 1);
            game.inventory[game.selected] = Some(sword);
            // A damaged Mending item drinks the whole give, whatever the
            // repair rate is: `ExperienceOrb.playerTouch` repairs first.
            assert!(game.native_player_give_experience_points(player, 10));
            let sword = game.inventory[game.selected].as_ref().unwrap();
            assert!(
                sword.damage < 1000,
                "Mending repaired the item before any point became experience"
            );
            assert_eq!(game.experience.total, 0);
            // With nothing left to repair, the give reaches the experience.
            game.inventory[game.selected] = None;
            assert!(game.native_player_give_experience_points(player, 10));
            assert_eq!(game.experience.total, 10);
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn levels_are_added_taken_and_clamped_at_zero() {
        with_game(|game| {
            let player = acting_player(game);
            game.experience.progress = 0.25;
            assert!(game.native_player_give_levels(player, 3));
            assert_eq!(game.experience.level, 3);
            assert_eq!(
                game.experience.progress, 0.25,
                "a level add keeps the progress"
            );
            game.experience.total = 40;
            assert!(game.native_player_give_levels(player, -5));
            assert_eq!(game.experience.level, 0, "below zero clamps to zero");
            assert_eq!(game.experience.total, 0, "the clamp clears the total");
            assert_eq!(
                game.experience.progress, 0.0,
                "the clamp clears the progress"
            );
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn more_points_than_an_int_are_refused_untouched() {
        with_game(|game| {
            let player = acting_player(game);
            // `Player.giveExperiencePoints` takes an int: `i32::MAX` is
            // given, one more is refused.
            assert!(game.native_player_give_experience_points(player, i32::MAX as u32));
            assert_eq!(game.experience.total, i32::MAX);
            let mut sword = ItemStack::new("minecraft:diamond_sword", 1);
            sword.damage = 1000;
            sword.enchant("minecraft:mending", 1);
            game.inventory[game.selected] = Some(sword);
            assert!(!game.native_player_give_experience_points(player, i32::MAX as u32 + 1));
            let sword = game.inventory[game.selected].as_ref().unwrap();
            assert_eq!(sword.damage, 1000, "a refused give repairs nothing");
            assert_eq!(
                game.experience.total,
                i32::MAX,
                "a refused give gives nothing"
            );
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_stale_handle_gives_nothing() {
        with_game(|game| {
            let player = acting_player(game);
            let stale = mod_api::__private::game::player_ref(
                EntityRef::new(NativeWorldEpoch::new(u64::MAX), 7),
                player.uuid(),
            );
            assert!(!game.native_player_give_experience_points(stale, 10));
            assert!(!game.native_player_give_levels(stale, 1));
            assert_eq!(game.experience.total, 0);
            assert_eq!(game.experience.level, 0);
        });
    }
}

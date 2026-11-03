//! A player's food and experience for native server mods, through a
//! `PlayerRef` (docs/native-mods.md): the `FoodData` and `Player` reads the
//! handle's `food()` doesn't give, and the writes. Reads return None for a
//! stale handle or a player who isn't online here, as `food()` does. Writes
//! return false for those, for a dead player, and, inside another player's
//! turn, for anyone but that player. What changes reaches the player's client as any
//! change to their food or experience does.
use super::Game;
use horizon_client::entities::Experience;
use horizon_client::mod_api::{NativePlayerExperience, PlayerRef};

impl Game<'_> {
    /// Whether `player` still names the same player on this server.
    fn native_player_valid(&self, player: PlayerRef) -> bool {
        self.native_player_ref(player.uuid()) == Some(player)
    }
    /// Runs `f` with `player`'s state in place, when the handle is current
    /// and they are alive (`native_as_player`).
    fn native_player_write(&mut self, player: PlayerRef, f: impl FnOnce(&mut Self)) -> bool {
        self.native_player_valid(player) && self.native_as_player(player.uuid(), f)
    }
    /// The acting player's food data, inside `native_player_write`.
    fn native_food_mut(&mut self) -> Option<&mut horizon_world::food::FoodData> {
        self.player.as_mut().map(|p| &mut p.health.food)
    }

    /// `FoodData.needsFood`: the food level is below 20. API @API:player_food@.
    pub(super) fn native_player_needs_food(&self, player: PlayerRef) -> Option<bool> {
        Some(self.native_player_food(player)?.level < 20)
    }
    /// `FoodData.hasEnoughFood`: the food level is above 6, enough to
    /// sprint. API @API:player_food@.
    pub(super) fn native_player_has_enough_food(&self, player: PlayerRef) -> Option<bool> {
        Some(self.native_player_food(player)?.level as f32 > 6.0)
    }
    /// `Player.canEat(always)`: an invulnerable player (creative), `always`
    /// (food that can always be eaten) or a food level below 20. API
    /// @API:player_food@.
    pub(super) fn native_player_can_eat(&self, player: PlayerRef, always: bool) -> Option<bool> {
        let invulnerable = self.native_player_abilities(player)?.invulnerable;
        Some(invulnerable || always || self.native_player_needs_food(player)?)
    }
    /// `FoodData.eat(nutrition, saturationModifier)`: adds `nutrition` to
    /// the food level (0 to 20) and `nutrition * modifier * 2` to the
    /// saturation, which stays at most the food level. API @API:player_food@.
    pub(super) fn native_player_feed(
        &mut self,
        player: PlayerRef,
        nutrition: i32,
        saturation_modifier: f32,
    ) -> bool {
        self.native_player_write(player, |game| {
            if let Some(food) = game.native_food_mut() {
                food.eat(nutrition, saturation_modifier);
            }
        })
    }
    /// `Player.causeFoodExhaustion`: adds exhaustion (at most 40), as
    /// sprinting or jumping does. An invulnerable player (creative) or a
    /// spectator gets none, and the call still returns true. API
    /// @API:player_food@.
    pub(super) fn native_player_add_exhaustion(&mut self, player: PlayerRef, amount: f32) -> bool {
        self.native_player_write(player, |game| {
            if let Some(p) = game.player.as_mut() {
                p.cause_food_exhaustion(amount);
            }
        })
    }
    /// `FoodData.setFoodLevel`: sets the food level as it is, unclamped,
    /// as vanilla does. API @API:player_food@.
    pub(super) fn native_player_set_food_level(&mut self, player: PlayerRef, level: i32) -> bool {
        self.native_player_write(player, |game| {
            if let Some(food) = game.native_food_mut() {
                food.food = level;
            }
        })
    }
    /// `FoodData.setSaturation`: sets the saturation as it is, unclamped.
    /// API @API:player_food@.
    pub(super) fn native_player_set_saturation(
        &mut self,
        player: PlayerRef,
        saturation: f32,
    ) -> bool {
        self.native_player_write(player, |game| {
            if let Some(food) = game.native_food_mut() {
                food.saturation = saturation;
            }
        })
    }
    /// Sets the exhaustion as it is (Paper's `HumanEntity#setExhaustion`;
    /// vanilla's `FoodData` only adds to it). API @API:player_food@.
    pub(super) fn native_player_set_exhaustion(
        &mut self,
        player: PlayerRef,
        exhaustion: f32,
    ) -> bool {
        self.native_player_write(player, |game| {
            if let Some(food) = game.native_food_mut() {
                food.exhaustion = exhaustion;
            }
        })
    }

    /// `Player.experienceLevel`, `experienceProgress` and `totalExperience`.
    /// API @API:player_food@.
    pub(super) fn native_player_experience(
        &self,
        player: PlayerRef,
    ) -> Option<NativePlayerExperience> {
        if !self.native_player_valid(player) {
            return None;
        }
        let uuid = player.uuid();
        // As `native_player_ref` finds them: a guest's record holds their
        // experience except while their state is swapped in.
        let guest = (self.lan_acting_uuid != Some(uuid))
            .then(|| {
                self.lan
                    .iter()
                    .flat_map(|h| h.hub.guests.iter())
                    .find(|g| g.placed && g.uuid == uuid)
            })
            .flatten();
        Some(match guest {
            Some(g) => NativePlayerExperience {
                level: g.xp_level,
                progress: g.xp_progress,
                total: g.xp_total,
            },
            None => NativePlayerExperience {
                level: self.experience.level,
                progress: self.experience.progress,
                total: self.experience.total,
            },
        })
    }
    /// `Player.getXpNeededForNextLevel`: the points the current level needs
    /// to fill. API @API:player_food@.
    pub(super) fn native_player_xp_needed_for_next_level(&self, player: PlayerRef) -> Option<i32> {
        Some(Experience::needed(
            self.native_player_experience(player)?.level,
        ))
    }
    /// `ServerPlayer.setExperiencePoints`: sets the progress toward the next
    /// level to `points` of the points it needs, kept below a full bar. The
    /// level and the total are kept. API @API:player_food@.
    pub(super) fn native_player_set_experience_points(
        &mut self,
        player: PlayerRef,
        points: i32,
    ) -> bool {
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
    /// progress and the total. API @API:player_food@.
    pub(super) fn native_player_set_experience_levels(
        &mut self,
        player: PlayerRef,
        levels: i32,
    ) -> bool {
        self.native_player_write(player, |game| game.experience.level = levels)
    }
}

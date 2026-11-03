//! Events, explosions and lightning in a level for native server mods,
//! through a `LevelRef` (docs/native-mods.md, "Entity and level handles"):
//! `Level.levelEvent`, `Level.gameEvent`, `Level.explode` and a summoned
//! lightning bolt. Like the level's block writes, they act on the level in
//! place, the one the hook runs in; a parked level, a stale handle or a
//! client returns false. Reaching a parked level, as `play_sound` and the
//! reads do through `native_level_world`, is a follow-up.
use super::{Game, native_explosions::NativeBlast};
use horizon_client::mod_api::{LevelRef, NativeExplosionInteraction};
use horizon_client::sculk::Context;

/// A position inside the world's horizontal and vertical bounds.
fn in_bounds(at: [f64; 3]) -> bool {
    at.iter().all(|v| v.is_finite())
        && at[0].abs() < 30_000_000.0
        && at[2].abs() < 30_000_000.0
        && at[1].abs() < 20_000_000.0
}

impl Game<'_> {
    /// Plays level event `id` at `pos` with `data` for the players near it
    /// (`Level.levelEvent(null, id, pos, data)`): `1000`s are sounds, `2000`s
    /// particles, as vanilla's `LevelEvent` numbers them (`2001` breaks a
    /// block with state id `data`). No player is left out, the acting one
    /// included. False for an event that carries a block state (2001, 2014,
    /// 3008) when `data` isn't one.
    pub(super) fn native_level_level_event(
        &mut self,
        level: LevelRef,
        id: i32,
        pos: [i32; 3],
        data: i32,
    ) -> bool {
        if !self.native_level_in_place(level) {
            return false;
        }
        let states = self.res.facts().states.len();
        if matches!(id, 2001 | 2014 | 3008)
            && usize::try_from(data).ok().is_none_or(|d| d >= states)
        {
            return false;
        }
        let sent = self.lan_events.len();
        self.level_event_at(id, pos, data);
        // `levelEvent(null, ...)`: the guests' copies name no excluded
        // player, though `lan_record_events` leaves out the acting one for
        // the events a player's own action plays.
        for event in &mut self.lan_events[sent..] {
            event.3 = None;
        }
        true
    }
    /// Sends game event `event` (`minecraft:block_change`...) from `at`, as
    /// `Level.gameEvent(null, event, at)` does: sculk sensors, shriekers,
    /// wardens and allays in range hear it. False for an event vanilla
    /// doesn't have.
    pub(super) fn native_level_game_event(
        &mut self,
        level: LevelRef,
        event: &str,
        at: [f64; 3],
    ) -> bool {
        let Some(event) = horizon_client::sculk::intern(event) else {
            return false;
        };
        if !self.native_level_in_place(level) || !in_bounds(at) {
            return false;
        }
        self.post_game_event(event, at, Context::default());
        true
    }
    /// Explodes at `center` with no source entity, as `Level.explode(null,
    /// x, y, z, radius, fire, interaction)` does (`native_explode`): `fire`
    /// lights some of the air around, `interaction` says what happens to
    /// blocks: `None`, `Block` or `Tnt` (vanilla's `Mob` and `Trigger` aren't
    /// offered yet). False for a chunk that isn't loaded or a radius outside
    /// `0 < radius <= 32`.
    pub(super) fn native_level_explode(
        &mut self,
        level: LevelRef,
        center: [f64; 3],
        radius: f32,
        fire: bool,
        interaction: NativeExplosionInteraction,
    ) -> bool {
        if !self.native_level_in_place(level) {
            return false;
        }
        let blast = match interaction {
            NativeExplosionInteraction::None => NativeBlast::Keep,
            NativeExplosionInteraction::Block => NativeBlast::Block,
            NativeExplosionInteraction::Tnt => NativeBlast::Tnt,
        };
        self.native_explode(level.dimension(), center, radius, fire, blast)
    }
    /// Strikes lightning at `at`, as summoning a `minecraft:lightning_bolt`
    /// does: it sets fire, hurts and converts what it hits (a pig to a
    /// zombified piglin...) and every player hears the thunder. With
    /// `visual_only` it only flashes and sounds (`setVisualOnly`). The
    /// `lightning_strike` hook hears it first with the cause `mod` and may
    /// cancel it. False for a chunk that isn't loaded or a canceled strike.
    pub(super) fn native_level_strike_lightning(
        &mut self,
        level: LevelRef,
        at: [f64; 3],
        visual_only: bool,
    ) -> bool {
        if !self.native_level_in_place(level) || !in_bounds(at) {
            return false;
        }
        let chunk = ((at[0].floor() as i32) >> 4, (at[2].floor() as i32) >> 4);
        if !self.world.chunks.contains_key(&chunk) {
            return false;
        }
        let block = [
            at[0].floor() as i32,
            at[1].floor() as i32,
            at[2].floor() as i32,
        ];
        if !self.lightning_strike_allowed(None, block, "mod") {
            return false;
        }
        self.spawn_bolt(at, visual_only);
        true
    }
}

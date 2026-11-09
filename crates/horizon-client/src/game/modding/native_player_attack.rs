//! A player's attack strength for native server mods, through a `PlayerRef`:
//! `Player.getAttackStrengthScale(partial)`, the charge `Player.attack` reads
//! at its start (`attack_entity` fires before that, and before the swing
//! resets the ticker). Reads return None for a stale handle or a player who
//! isn't online here; a resting guest reads its saved body.
use super::Game;
use super::{commands::Target, native_live_entity_reads::EntityReadSubject, native_read_dependencies::EntityReadDependency};
use horizon_client::attack_strength::{BASE_ATTACK_SPEED, delay};
use mod_api::PlayerRef;

impl Game<'_> {
    /// `Player.getAttackStrengthScale(partial)` for `player`.
    /// Returns `None` for a non-finite partial tick; finite partials are not restricted.
    pub(in crate::game) fn player_attack_strength_scale(
        &self,
        player: PlayerRef,
        partial: f32,
    ) -> Result<Option<f32>, mod_api::EntityReadError> {
        if !partial.is_finite() || !self.is_logical_server() { return Ok(None); }
        let target=match self.entity_read_dependency::<core::convert::Infallible>(EntityReadSubject::Player(player), &[EntityReadDependency::AttackState, EntityReadDependency::Attributes, EntityReadDependency::Equipment, EntityReadDependency::Effects]) {Some(read) => { let read = read?; if read.location.owner.dimension != self.dim.key() { return Ok(None); } read.target },None=>return Ok(None)};
        Ok((|| {
        // 1.8.9 had no attack cooldown.
        if self.legacy_rules() { return Some(1.0); }
        Some(match target {
            Target::Player => return self.attack_strength_scale_at(partial),
            Target::Guest(conn) => {
                let guest=self.command_guest(conn)?;
                let (ticks, item) = self
                    .lan
                    .as_ref()?
                    .play
                    .get(&guest.conn)
                    .map_or((0, None), |play| (play.attack_ticks(), play.attack_item()));
                let speed = guest_attack_speed(self, guest, item);
                ((ticks as f32 + partial) / delay(speed)).clamp(0.0, 1.0)
            }
            _ => return None,
        })
        })())
    }
}

/// `Game.main_hand_attack_speed` for a guest between turns: its saved body's
/// attack speed base, the main-hand item of its last tick, and its effects'
/// modifiers.
fn guest_attack_speed(
    game: &Game<'_>,
    guest: &horizon_client::lan::guest::Guest,
    item: Option<&str>,
) -> f64 {
    let base = guest
        .motion
        .as_ref()
        .and_then(|player| {
            let key = player.attributes.key("minecraft:attack_speed")?;
            let handle = player.attributes.lookup(Some(&key))?;
            player
                .attributes
                .instance(&handle)
                .ok()
                .map(|value| value.base_value())
        })
        .unwrap_or(BASE_ATTACK_SPEED);
    let value = base
        + item
            .and_then(|id| game.items().get(id))
            .map_or(0.0, |info| f64::from(info.attack_speed));
    guest
        .effects
        .modifiers()
        .into_iter()
        .filter(|modifier| modifier.0 == "minecraft:attack_speed")
        .fold(value, |value, modifier| value * (1.0 + modifier.2))
        .clamp(0.0, 1024.0)
}

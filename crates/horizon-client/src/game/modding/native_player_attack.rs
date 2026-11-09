//! A player's attack strength for native server mods, through a `PlayerRef`:
//! `Player.getAttackStrengthScale(partial)`, the charge `Player.attack` reads
//! at its start (`attack_entity` fires before that, and before the swing
//! resets the ticker). Reads return None for a stale handle or a player who
//! isn't online here; a resting guest reads its actual saved body.
use super::{
    Game, commands::Target, native_base_read_rows::BaseReadSource,
    native_live_entity_reads::EntityReadSubject, native_read_dependencies::EntityReadDependency,
};
use horizon_client::attack_strength::delay;
use mod_api::{EntityReadError, PlayerRef};

impl Game<'_> {
    /// `Player.getAttackStrengthScale(partial)` for `player`.
    /// Returns `None` for a non-finite partial tick; finite partials are not restricted.
    pub(in crate::game) fn player_attack_strength_scale(
        &self,
        player: PlayerRef,
        partial: f32,
    ) -> Result<Option<f32>, EntityReadError> {
        self.read_player_attack_strength_scale(None, player, partial)
    }

    pub(in crate::game) fn read_player_attack_strength_scale(
        &self,
        provider: Option<&dyn super::native_entity_body_provider::EntityBodyProvider>,
        player: PlayerRef,
        partial: f32,
    ) -> Result<Option<f32>, EntityReadError> {
        if !partial.is_finite() || !self.is_logical_server() {
            return Ok(None);
        }
        let Some(admitted) = self.entity_read_admission(
            EntityReadSubject::Player(player),
            &[
                EntityReadDependency::AttackState,
                EntityReadDependency::Attributes,
                EntityReadDependency::Equipment,
                EntityReadDependency::Effects,
            ],
        ) else {
            return Ok(None);
        };
        self.with_entity_read_storage(admitted, provider, |source| {
            let motion = match source.body {
                BaseReadSource::Player(player) => Some(player),
                BaseReadSource::Guest(guest) => guest.motion.as_deref(),
                _ => None,
            }
            .or_else(|| {
                matches!(admitted.target(), Target::Player)
                    .then_some(self.player.as_ref())
                    .flatten()
            })
            .ok_or(EntityReadError::Lifted)?;
            match admitted.target() {
                Target::Player => admitted.project_actual(|| {
                    if self.legacy_rules() {
                        return Some(1.0);
                    }
                    let speed = self.attack_speed_for(
                        Some(motion),
                        self.attack_item.as_deref(),
                        &self.potions.effects,
                    );
                    Some(((self.attack_ticks as f32 + partial) / delay(speed)).clamp(0.0, 1.0))
                }),
                target @ Target::Guest(conn) => {
                    let guest = self
                        .entity_read_guest(source.body, target)
                        .ok_or(EntityReadError::Lifted)?;
                    let host = self.lan.as_ref().ok_or(EntityReadError::Lifted)?;
                    let play = host.play.get(&conn);
                    admitted.project_actual(|| {
                        if self.legacy_rules() {
                            return Some(1.0);
                        }
                        let (ticks, item) = play
                            .map_or((0, None), |play| (play.attack_ticks(), play.attack_item()));
                        let speed = self.attack_speed_for(Some(motion), item, &guest.effects);
                        Some(((ticks as f32 + partial) / delay(speed)).clamp(0.0, 1.0))
                    })
                }
                _ => Ok(None),
            }
        })
        .unwrap_or(Err(EntityReadError::Lifted))
    }
}

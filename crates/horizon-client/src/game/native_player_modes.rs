//! A player's permission level for native server mods, through a
//! `PlayerRef` (docs/modding/src/how-to/native-mods.md): the `Player` reads the handle's game
//! mode reads don't give. They return None for a stale handle or a player
//! who isn't online here.
use super::Game;
use super::native_server::PlayerRead;
use mod_api::{GameMode, PlayerRef};

impl Game<'_> {
    /// The player's permission level, 0 to 4 (`Player.permissions()`,
    /// `PlayerList.getProfilePermissions`): an op's level, the
    /// single-player owner's 4 with commands allowed.
    pub(super) fn native_player_permission_level(&self, player: PlayerRef) -> Option<u8> {
        Some(match self.player_handle_read(player)? {
            PlayerRead::Guest(g) => g.permission,
            PlayerRead::Acting { .. } if self.lan_acting.is_some() => self.ui.registries.permission,
            PlayerRead::Acting { .. } if self.allow_commands() => 4,
            PlayerRead::Acting { .. } => 0,
        })
    }
    /// `Player.hasInfiniteMaterials`: the `instabuild` ability, which
    /// creative mode gives.
    pub(super) fn native_player_has_infinite_materials(&self, player: PlayerRef) -> Option<bool> {
        self.native_player_is_creative(player)
    }
    /// `Player.canUseGameMasterBlocks`: creative and a permission level of
    /// at least 2, as command blocks, structure blocks and jigsaws ask.
    pub(super) fn native_player_can_use_game_master_blocks(
        &self,
        player: PlayerRef,
    ) -> Option<bool> {
        let creative = self.native_player_is_creative(player)?;
        Some(creative && self.native_player_permission_level(player)? >= 2)
    }
    /// `ServerPlayerGameMode.getPreviousGameModeForPlayer`: the mode the
    /// player last left (`previousPlayerGameType`, saved with the player), or
    /// `Some(None)` when they never changed mode. None for a stale or
    /// unavailable player.
    pub(super) fn native_player_previous_game_mode(
        &self,
        player: PlayerRef,
    ) -> Option<Option<GameMode>> {
        let id = match self.player_handle_read(player)? {
            // The acting player's own: a guest's turn swaps theirs in.
            PlayerRead::Acting { .. } => self.previous_mode.map(super::game_mode::mode_id_of),
            PlayerRead::Guest(guest) => guest.previous_game_mode,
        };
        Some(id.and_then(GameMode::from_id))
    }
    /// `Player.isSecondaryUseActive`: whether the player uses items and
    /// blocks as with sneak held, as vanilla reads it from the shift-key
    /// flag (the same read as `EntityRef::is_shift_key_down`). None for a
    /// stale or unavailable player.
    pub(super) fn native_player_is_secondary_use_active(&self, player: PlayerRef) -> Option<bool> {
        self.native_entity_is_shift_key_down(player.entity())
    }
}

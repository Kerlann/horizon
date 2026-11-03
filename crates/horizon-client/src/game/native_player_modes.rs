//! A player's permission level for native server mods, through a
//! `PlayerRef` (docs/native-mods.md): the `Player` reads the handle's game
//! mode reads don't give. They return None for a stale handle or a player
//! who isn't online here.
use super::Game;
use horizon_client::mod_api::PlayerRef;

impl Game<'_> {
    /// The player's permission level, 0 to 4 (`Player.permissions()`,
    /// `PlayerList.getProfilePermissions`): an op's level, the
    /// single-player owner's 4 with commands allowed.
    pub(super) fn native_player_permission_level(&self, player: PlayerRef) -> Option<u8> {
        if self.native_player_ref(player.uuid()) != Some(player) {
            return None;
        }
        let guest = self
            .lan
            .iter()
            .flat_map(|h| h.hub.guests.iter())
            .find(|g| g.placed && g.uuid == player.uuid());
        Some(match guest {
            Some(g) => g.permission,
            // `IntegratedServer.getProfilePermissions` for the owner.
            None if self.allow_commands() => 4,
            None => 0,
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
}

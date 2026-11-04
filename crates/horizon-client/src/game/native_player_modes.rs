//! A player's permission level for native server mods, through a
//! `PlayerRef` (docs/modding/src/how-to/native-mods.md): the `Player` reads the handle's game
//! mode reads don't give. They return None for a stale handle or a player
//! who isn't online here.
use super::Game;
use mod_api::PlayerRef;

impl Game<'_> {
    /// The player's permission level, 0 to 4 (`Player.permissions()`,
    /// `PlayerList.getProfilePermissions`): an op's level, the
    /// single-player owner's 4 with commands allowed.
    pub(super) fn native_player_permission_level(&self, player: PlayerRef) -> Option<u8> { if let Some(__l) = crate::points_generated::P4071.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(match self.player_handle_read(player)? {
            super::native_server::PlayerRead::Guest(g) => g.permission,
            super::native_server::PlayerRead::Acting { .. } if self.lan_acting.is_some() => {
                self.ui.registries.permission
            }
            super::native_server::PlayerRead::Acting { .. } if self.allow_commands() => 4,
            super::native_server::PlayerRead::Acting { .. } => 0,
        })
    }
    /// `Player.hasInfiniteMaterials`: the `instabuild` ability, which
    /// creative mode gives.
    pub(super) fn native_player_has_infinite_materials(&self, player: PlayerRef) -> Option<bool> { if let Some(__l) = crate::points_generated::P4070.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.native_player_is_creative(player)
    }
    /// `Player.canUseGameMasterBlocks`: creative and a permission level of
    /// at least 2, as command blocks, structure blocks and jigsaws ask.
    pub(super) fn native_player_can_use_game_master_blocks(
        &self,
        player: PlayerRef,
    ) -> Option<bool> { if let Some(__l) = crate::points_generated::P4069.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        let creative = self.native_player_is_creative(player)?;
        Some(creative && self.native_player_permission_level(player)? >= 2)
    }
}

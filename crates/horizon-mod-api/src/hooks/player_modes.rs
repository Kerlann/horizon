//! The `player_modes` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    /// The player's operator permission level, 0 through 4; the single-player owner
    /// has 4 with commands allowed. Displaced connected players retain their level.
    /// None for a stale or unavailable player.
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_modes::Game<'_>::native_player_permission_level"
    )]
    fn player_permission_level();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_modes::Game<'_>::native_player_has_infinite_materials"
    )]
    fn player_has_infinite_materials();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_modes::Game<'_>::native_player_can_use_game_master_blocks"
    )]
    fn player_can_use_game_master_blocks();
    /// `ServerPlayerGameMode.getPreviousGameModeForPlayer`: the mode the player last left, `Some(None)` before
    /// any change.
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_modes::Game<'_>::native_player_previous_game_mode"
    )]
    fn player_previous_game_mode();
    /// `Player.isSecondaryUseActive`: sneak-use, from the shift-key flag.
    #[helper(
        side = "both",
        receiver = "player",
        target = "horizon_client::game::native_player_modes::Game<'_>::native_player_is_secondary_use_active"
    )]
    fn player_is_secondary_use_active();
}

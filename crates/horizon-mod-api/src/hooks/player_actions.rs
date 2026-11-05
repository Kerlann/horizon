//! The `player_actions` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    /// Run one action with the persistent fake's own body and inventory, through the same
    /// reach, protection, interaction hooks and Parts as an online player's action.
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_server::Game<'_>::native_player_perform_action"
    )]
    fn player_perform_action();

    /// Run the same player action with temporary survival state, returning the remaining
    /// main-hand stack and other acquired items. Restores the fake's body, inventory, look,
    /// secondary use and action timers on success or refusal; world effects remain.
    /// Refuses real players, stale handles, nested actions and open transactions exactly as
    /// `perform_action` does. Non-finite viewpoint or entity-hit coordinates are refused.
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_server::Game<'_>::native_player_perform_action_scoped"
    )]
    fn player_perform_action_scoped();
}

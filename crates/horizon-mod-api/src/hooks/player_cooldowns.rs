//! The `player_cooldowns` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "both",
        target = "horizon_client::game::native_player_cooldowns::Game<'_>::native_item_cooldown_group"
    )]
    fn item_cooldown_group();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_cooldowns::Game<'_>::native_player_item_cooldowns"
    )]
    fn player_item_cooldowns();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_cooldowns::Game<'_>::native_player_has_item_cooldown"
    )]
    fn player_has_item_cooldown();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_cooldowns::Game<'_>::native_player_item_cooldown_percent"
    )]
    fn player_item_cooldown_percent();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_cooldowns::Game<'_>::native_player_set_item_cooldown"
    )]
    fn player_set_item_cooldown();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_cooldowns::Game<'_>::native_player_remove_item_cooldown"
    )]
    fn player_remove_item_cooldown();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_cooldowns::Game<'_>::native_player_exp_cooldown"
    )]
    fn player_exp_cooldown();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_cooldowns::Game<'_>::native_player_set_exp_cooldown"
    )]
    fn player_set_exp_cooldown();
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_player_cooldowns::Game<'_>::native_player_set_warden_warning_cooldown"
    )]
    fn player_set_warden_warning_cooldown();
}

//! The `menu_refs` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "client",
        receiver = "menu",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_menu_click"
    )]
    fn menu_click();
    #[helper(
        side = "client",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_local_player"
    )]
    fn local_player();
    #[helper(
        side = "both",
        receiver = "player",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_menu_ref"
    )]
    fn player_container_menu();
    #[helper(
        side = "server",
        receiver = "menu",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_menu_container_id"
    )]
    fn menu_container_id();
    #[helper(
        side = "server",
        receiver = "menu",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_menu_kind"
    )]
    fn menu_kind();
    #[helper(
        side = "both",
        receiver = "menu",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_menu_slot_count"
    )]
    fn menu_slot_count();
    #[helper(
        side = "both",
        receiver = "menu",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_menu_slot"
    )]
    fn menu_slot();
    #[helper(
        side = "both",
        receiver = "menu",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_menu_carried"
    )]
    fn menu_carried();
    #[helper(
        side = "client",
        receiver = "menu",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_menu_close"
    )]
    fn menu_close();
    #[helper(
        side = "client",
        receiver = "menu",
        target = "horizon_client::game::native_menu_refs::Game<'_>::native_menu_click_button"
    )]
    fn menu_click_button();
}

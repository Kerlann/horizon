//! The `minecart_refs` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "server",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_ref"
    )]
    fn minecart();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_kind"
    )]
    fn minecart_kind();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_max_speed"
    )]
    fn minecart_max_speed();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_is_on_rails"
    )]
    fn minecart_is_on_rails();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_flipped"
    )]
    fn minecart_flipped();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_set_flipped"
    )]
    fn minecart_set_flipped();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_damage"
    )]
    fn minecart_damage();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_set_damage"
    )]
    fn minecart_set_damage();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_hurt_time"
    )]
    fn minecart_hurt_time();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_set_hurt_time"
    )]
    fn minecart_set_hurt_time();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_custom_display_block"
    )]
    fn minecart_custom_display_block();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_set_custom_display_block"
    )]
    fn minecart_set_custom_display_block();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_display_offset"
    )]
    fn minecart_display_offset();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_set_display_offset"
    )]
    fn minecart_set_display_offset();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_fuel"
    )]
    fn minecart_fuel();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_set_fuel"
    )]
    fn minecart_set_fuel();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_fuse"
    )]
    fn minecart_fuse();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_prime_fuse"
    )]
    fn minecart_prime_fuse();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_hopper_enabled"
    )]
    fn minecart_hopper_enabled();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_set_hopper_enabled"
    )]
    fn minecart_set_hopper_enabled();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_command"
    )]
    fn minecart_command();
    #[helper(
        side = "server",
        receiver = "minecart",
        target = "horizon_client::game::native_minecart_refs::Game<'_>::native_minecart_set_command"
    )]
    fn minecart_set_command();
}

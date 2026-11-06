//! The `living_flags` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "both",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_gliding_ticks"
    )]
    fn living_gliding_ticks();
    #[helper(
        side = "both",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_is_gliding"
    )]
    fn living_is_gliding();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_stop_gliding"
    )]
    fn living_stop_gliding();
    #[helper(
        side = "both",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_is_sleeping"
    )]
    fn living_is_sleeping();
    #[helper(
        side = "both",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_sleeping_pos"
    )]
    fn living_sleeping_pos();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_start_sleeping"
    )]
    fn living_start_sleeping();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_stop_sleeping"
    )]
    fn living_stop_sleeping();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_set_sleeping_pos"
    )]
    fn living_set_sleeping_pos();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_clear_sleeping_pos"
    )]
    fn living_clear_sleeping_pos();
    #[helper(
        side = "both",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_is_jumping"
    )]
    fn living_is_jumping();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_set_is_jumping"
    )]
    fn living_set_is_jumping();
    #[helper(
        side = "both",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_is_climbing"
    )]
    fn living_is_climbing();
    #[helper(
        side = "both",
        receiver = "living",
        target = "horizon_client::game::native_living_flags::Game<'_>::native_living_baby"
    )]
    fn living_baby();
}

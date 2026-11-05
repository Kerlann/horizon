//! The `level_reads` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "both",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_state"
    )]
    fn level_block_state();
    #[helper(
        side = "both",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_block"
    )]
    fn level_block();
    #[helper(
        side = "both",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_block_property"
    )]
    fn level_block_property();
    #[helper(
        side = "server",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_in_place"
    )]
    fn level_in_place();
    #[helper(
        side = "both",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_is_loaded"
    )]
    fn level_is_loaded();
    #[helper(
        side = "server",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_brightness"
    )]
    fn level_brightness();
    /// The level's lowest block y and height (`LevelHeightAccessor.getMinY`, `getHeight`).
    /// Reads any loaded server level or the client's current level; None for a stale
    /// handle, before client login, or for a dimension unavailable on this side.
    #[helper(
        side = "both",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_height"
    )]
    fn level_height();
    #[helper(
        side = "server",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_game_time"
    )]
    fn level_game_time();
    #[helper(
        side = "server",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_day_time"
    )]
    fn level_day_time();
    #[helper(
        side = "both",
        receiver = "level",
        target = "horizon_client::game::native_level_refs::Game<'_>::native_level_fluid_flow"
    )]
    fn level_fluid_flow();
}

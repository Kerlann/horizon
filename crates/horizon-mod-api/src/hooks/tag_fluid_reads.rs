//! The `tag_fluid_reads` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    /// The stored identifier of an engine-issued unresolved fluid handle.
    #[helper(
        side = "both",
        target = "horizon_client::game::native_world_read::Game<'_>::native_unresolved_fluid_id"
    )]
    fn unresolved_fluid_id();
    /// Test the resolved block's numeric membership in the world's merged tag snapshot.
    /// Reload replaces that snapshot without changing key identity. Unknown tags return false;
    /// unavailable authoritative context or block mappings return None. Tag names remain strings.
    #[helper(
        side = "both",
        target = "horizon_client::game::native_world_read::Game<'_>::native_block_in_tag"
    )]
    fn block_in_tag();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_world_read::Game<'_>::native_tag_members"
    )]
    fn tag_members();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_world_read::Game<'_>::native_state_outline"
    )]
    fn state_outline();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_world_read::Game<'_>::native_state_fluid_state"
    )]
    fn state_fluid_state();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_world_read::Game<'_>::native_fluid_in_tag"
    )]
    fn fluid_in_tag();
    #[helper(
        side = "server",
        receiver = "level",
        target = "horizon_client::game::native_world_read::Game<'_>::native_level_fluid_state"
    )]
    fn level_fluid_state();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_world_read::Game<'_>::native_fluid_is_same"
    )]
    fn fluid_is_same();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_world_read::Game<'_>::native_fluid_bucket"
    )]
    fn fluid_bucket();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_world_read::Game<'_>::native_fluid_pickup_sound"
    )]
    fn fluid_pickup_sound();
}

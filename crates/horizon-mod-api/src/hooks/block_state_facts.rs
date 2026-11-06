//! The `block_state_facts` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    /// A registered fluid's state definition, or tank-only for a fluid without world states.
    #[helper(
        side = "both",
        backend = true,
        target = "horizon_client::game::native_block_states::Game<'_>::native_fluid_state_definition"
    )]
    fn fluid_state_definition();
    /// Validate partial fluid-state constraints with indexed errors.
    #[helper(
        side = "both",
        backend = true,
        target = "horizon_client::game::native_block_states::Game<'_>::native_fluid_state_validate"
    )]
    fn fluid_state_validate();
    #[helper(
        side = "both",
        backend = true,
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_state_replaceable"
    )]
    fn block_state_replaceable();
    #[helper(
        side = "both",
        backend = true,
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_state_destroy_speed"
    )]
    fn block_state_destroy_speed();
    #[helper(
        side = "both",
        backend = true,
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_state_sound"
    )]
    fn block_state_sound();
    #[helper(
        side = "both",
        backend = true,
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_state_has_block_entity"
    )]
    fn block_state_has_block_entity();
    #[helper(
        side = "both",
        backend = true,
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_states_of"
    )]
    fn block_states_of();
    #[helper(
        side = "both",
        backend = true,
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_state_definition"
    )]
    fn block_state_definition();
    #[helper(
        side = "both",
        backend = true,
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_state_matches"
    )]
    fn block_state_matches();
}

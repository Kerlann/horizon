//! The `block_state_ids` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "both",
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_state_id"
    )]
    fn block_state_id();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_state_string"
    )]
    fn block_state_string();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_state_default"
    )]
    fn block_state_default();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_block_states::Game<'_>::native_item_stack_in_tag"
    )]
    fn item_stack_in_tag();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_block_states::Game<'_>::native_item_crafting_remainder"
    )]
    fn item_crafting_remainder();
    #[helper(
        side = "both",
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_item"
    )]
    fn block_item();
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
}

//! The `block_state_ids` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "both",
        receiver = "block_key",
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_key_id"
    )]
    fn block_key_id();
    #[helper(
        side = "both",
        receiver = "item_key",
        target = "horizon_client::game::native_block_states::Game<'_>::native_item_key_id"
    )]
    fn item_key_id();
    #[helper(
        side = "both",
        receiver = "effect_key",
        target = "horizon_client::game::native_block_states::Game<'_>::native_effect_key_id"
    )]
    fn effect_key_id();
    #[helper(
        side = "both",
        receiver = "block_entity_type_key",
        target = "horizon_client::game::native_block_states::Game<'_>::native_block_entity_type_key_id"
    )]
    fn block_entity_type_key_id();
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
}

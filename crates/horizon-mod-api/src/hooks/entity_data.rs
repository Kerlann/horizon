//! The `entity_data` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "server",
        receiver = "entity",
        target = "horizon_client::game::native_entity_data::Game<'_>::native_entity_nbt"
    )]
    fn entity_nbt();
    #[helper(
        side = "server",
        receiver = "entity",
        target = "horizon_client::game::native_entity_data::Game<'_>::native_entity_merge_nbt"
    )]
    fn entity_merge_nbt();
    #[helper(
        side = "server",
        receiver = "entity",
        target = "horizon_client::game::native_entity_data::Game<'_>::native_entity_tags"
    )]
    fn entity_tags();
    #[helper(
        side = "server",
        receiver = "entity",
        target = "horizon_client::game::native_entity_data::Game<'_>::native_entity_add_tag"
    )]
    fn entity_add_tag();
    #[helper(
        side = "server",
        receiver = "entity",
        target = "horizon_client::game::native_entity_data::Game<'_>::native_entity_remove_tag"
    )]
    fn entity_remove_tag();
    #[helper(
        side = "both",
        receiver = "entity",
        target = "horizon_client::game::native_entity_data::Game<'_>::native_entity_name"
    )]
    fn entity_name();
}

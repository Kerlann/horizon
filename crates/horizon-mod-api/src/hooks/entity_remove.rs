//! The `entity_remove` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "server",
        receiver = "entity",
        target = "horizon_client::game::native_entity_remove::Game<'_>::native_entity_discard"
    )]
    fn entity_discard();
    #[helper(
        side = "server",
        receiver = "entity",
        target = "horizon_client::game::native_entity_remove::Game<'_>::native_entity_remove"
    )]
    fn entity_remove();
    #[helper(
        side = "server",
        receiver = "entity",
        target = "horizon_client::game::native_entity_remove::Game<'_>::native_entity_spawn_at_location"
    )]
    fn entity_spawn_at_location();
}

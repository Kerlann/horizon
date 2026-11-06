//! The `living_effects` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_effects::Game<'_>::native_living_apply_consume_effect"
    )]
    fn living_apply_consume_effect();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_effects::Game<'_>::native_living_add_effect"
    )]
    fn living_add_effect();
    #[helper(
        side = "both",
        receiver = "living",
        target = "horizon_client::game::native_living_effects::Game<'_>::native_living_effect"
    )]
    fn living_effect();
    #[helper(
        side = "both",
        receiver = "living",
        target = "horizon_client::game::native_living_effects::Game<'_>::native_living_effects"
    )]
    fn living_effects();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_effects::Game<'_>::native_living_remove_effect"
    )]
    fn living_remove_effect();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_effects::Game<'_>::native_living_remove_all_effects"
    )]
    fn living_remove_all_effects();
    #[helper(
        side = "server",
        receiver = "living",
        target = "horizon_client::game::native_living_effects::Game<'_>::native_living_potion_affected"
    )]
    fn living_potion_affected();
    #[helper(
        side = "client",
        receiver = "living",
        target = "horizon_client::game::native_living_effects::Game<'_>::native_living_effect_blend_factor"
    )]
    fn living_effect_blend_factor();
}

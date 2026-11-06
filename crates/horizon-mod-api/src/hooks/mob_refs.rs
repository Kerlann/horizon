//! The `mob_refs` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "both",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_ref"
    )]
    fn mob();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_target"
    )]
    fn mob_target();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_set_target"
    )]
    fn mob_set_target();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_is_aggressive"
    )]
    fn mob_is_aggressive();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_set_aggressive"
    )]
    fn mob_set_aggressive();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_persistence_required"
    )]
    fn mob_persistence_required();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_set_persistence_required"
    )]
    fn mob_set_persistence_required();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_requires_custom_persistence"
    )]
    fn mob_requires_custom_persistence();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_no_ai"
    )]
    fn mob_no_ai();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_set_no_ai"
    )]
    fn mob_set_no_ai();
    #[helper(
        side = "server",
        receiver = "mob",
        target = "horizon_client::game::native_mob_refs::Game<'_>::native_mob_set_baby"
    )]
    fn mob_set_baby();
}

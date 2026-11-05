//! A player's mod-owned advancement triggers, through the scoped criterion resources.

pub trait Helpers {
    /// Fires a mod's advancement trigger for one player, testing each open criterion through
    /// `CriterionTest` in the player's own world before applying its `player` predicate.
    /// Diverges from upstream: the trigger lives on `PlayerRef`, with generated resources
    /// rather than an unscoped Host callback borrowing a private advancement world.
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_criteria::Game<'_>::native_player_trigger_criterion"
    )]
    fn player_trigger_criterion();
}

//! The `can_eat` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0103

pub trait Hooks {
    /// `Player.canEat` for the food `stack`, from `Consumable.canConsume`
    /// when eating starts: `vanilla_can_eat` is vanilla's answer (creative,
    /// `can_always_eat`, or hungry). Answer `Some(false)` to refuse the food,
    /// `Some(true)` to let a full player eat; `can_eat` left `None` keeps
    /// `vanilla_can_eat`. Both sides (the client starts eating as the
    /// server does), so answer from the stack and the player alone.
    #[hook(
        side = "both",
        contract = "replace",
        out = "can_eat",
        host = "read",
        neoforge = "none (vanilla Player.canEat; NeoForge adds no event or extension for it)"
    )]
    fn can_eat();
}

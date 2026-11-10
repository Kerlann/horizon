//! The `level_tick_pre` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0010

pub trait Hooks {
    /// Before an authoritative server level's tick pass. `dimension` is its ID.
    #[hook(side = "server", contract = "listen", neoforge = "LevelTickEvent.Pre")]
    fn level_tick_pre();
}

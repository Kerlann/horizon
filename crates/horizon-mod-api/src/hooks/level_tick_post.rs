//! The `level_tick_post` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0011

pub trait Hooks {
    /// After a successful authoritative server level tick pass.
    #[hook(side = "server", contract = "listen", neoforge = "LevelTickEvent.Post")]
    fn level_tick_post();
}

//! The `loot_condition` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0137

pub trait Hooks {
    /// A loot condition of a mod's own kind: the entry named a `type` in a
    /// namespace outside `minecraft:`. The kind, the JSON, the context and
    /// whether it holds are here (`holds` starts `false`; a mod that claims
    /// the kind sets it). The context is an owned snapshot, captured after the subscription guard.
    #[hook(
        side = "server",
        contract = "collect",
        out = "holds",
        target = "horizon_world::loot::native_loot_condition",
        neoforge = "RegisterEvent"
    )]
    fn loot_condition();
}

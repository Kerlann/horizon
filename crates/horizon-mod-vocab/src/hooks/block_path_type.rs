//! The `block_path_type` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The optional classification of `event.state` at `event.pos`
    /// (`IBlockExtension.getBlockPathType`). Asked before every vanilla rule,
    /// including air and `is_pathfindable`. `Some` replaces the whole static
    /// classification; `None` declines to vanilla. The readonly host reads the
    /// owning level, so the same state may answer differently at different positions.
    /// No mob is supplied: engine callers in NeoForge pass null. Neighbour
    /// and mob-specific adjustments run after this static classification.
    /// Diverges from upstream: new hook.
    #[hook(
        side = "server",
        contract = "replace",
        out = "path_type",
        scope = "block",
        key = "state",
        host = "read",
        neoforge = "IBlockExtension.getBlockPathType"
    )]
    fn block_path_type();
}

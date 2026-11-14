//! The `adjacent_block_path_type` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The `PathType` a block state gives a walk node beside it (`IBlockExtension.getAdjacentBlockPathType`, which `WalkNodeEvaluator.checkNeighbourBlocks` asks of the 24 blocks around an open node, on its level and the levels above and below, leaving out the node's own column). `event.path_type` is the state's own type, after `block_path_type`; `event.adjacent` starts with vanilla's answer for it: `DamagingInNeighbor` beside `Damaging`, `FireInNeighbor` beside `Fire` or `Lava`, `WaterBorder` beside `Water`, `DamageCautious` beside `DamageCautious`, and `None` beside anything else. `None` leaves the node its own type; otherwise the first neighbour with a type gives it to the node. The answer depends on the state and its type alone: the game asks once per state when parts resolve, at world load and on every `/reload`, so path finding reads the stored answer with nothing dispatched, and a part cannot answer from a position, a level, a mob or the node's own type. Diverges from upstream: new hook.
    #[hook(
        side = "server",
        contract = "collect",
        out = "adjacent",
        scope = "block",
        key = "state",
        host = "read",
        neoforge = "IBlockExtension.getAdjacentBlockPathType"
    )]
    fn adjacent_block_path_type();
}

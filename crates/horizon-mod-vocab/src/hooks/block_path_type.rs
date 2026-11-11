//! The `block_path_type` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The `PathType` a block state is to a mob's path finder (`IBlockExtension.getBlockPathType`, which `WalkNodeEvaluator.getPathTypeFromState` asks). `event.path_type` starts with vanilla's answer, after the state's `is_pathfindable` answers, and a part may set any type: `Damaging` makes mobs keep off the block as they do a cactus, `Blocked` makes it a wall. Every walk, fly and amphibious path reads it, and so does every goal that asks the type of a position; the type the block gives a node beside it (`adjacent_block_path_type`) is asked after it. The answer depends on the state alone: the game asks once per state when parts resolve, at world load and on every `/reload`, so path finding reads the stored answer with nothing dispatched, and a part cannot answer from a position, a level, a mob or the time. A mob's own malus for the type still applies. Diverges from upstream: new hook.
    #[hook(
        side = "server",
        contract = "collect",
        out = "path_type",
        scope = "block",
        key = "state",
        host = "read",
        neoforge = "IBlockExtension.getBlockPathType"
    )]
    fn block_path_type();
}

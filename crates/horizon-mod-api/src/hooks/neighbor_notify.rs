//! The `neighbor_notify` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The block at `pos` (`state`) is about to tell its neighbours it
    /// changed (NeoForge's `BlockEvent.NeighborNotifyEvent`): at
    /// `ServerLevel.updateNeighborsAt`, which notifies all six sides
    /// (`skipped_side` is `None`), and `updateNeighborsAtExceptFromFacing`,
    /// which notifies all but `skipped_side`. Set `cancel` and no neighbour
    /// hears, on both paths. Differs from NeoForge 26.3's implementation
    /// (which honours the cancel only when a side is skipped), follows its
    /// documented contract. Asked for every such update, redstone
    /// included: answer fast. Server-side.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "BlockEvent.NeighborNotifyEvent",
        cancellable = true,
        target = "horizon_client::game::native_hooks::neighbor_notify::GameLevel<'_>::native_neighbor_notify"
    )]
    fn neighbor_notify();
}

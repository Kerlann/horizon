//! The `item_damaged` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0246

pub trait Hooks {
    /// The item `item` of a server player is about to lose `amount` points of
    /// durability (`ItemStack.hurtAndBreak`; Bukkit's `PlayerItemDamageEvent`):
    /// the tool that mines or hits, the armour that takes a hit, a shield, a
    /// rod, an elytra. `amount` is what is left after Unbreaking; change it
    /// (0 or less leaves the item whole) or set `cancel` for no wear at all. It
    /// is not heard for a creative player's tools (they take no wear) nor when
    /// Unbreaking already saved the item, nor for the wear of a mob's armour or
    /// of a tool a dispenser uses, nor the host of a LAN world. The player's
    /// state is swapped in, so `host.players().acting_player()`, `PlayerRef::tell` and the
    /// item helpers reach them. Once for each wear, on a server.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (vanilla: ItemStack.hurtAndBreak; PlayerDestroyItemEvent is only the break)"
    )]
    fn item_damaged();
}

//! The `block_entity_refs` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    #[helper(
        side = "both",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_ref"
    )]
    fn block_entity();
    /// Open this loaded block entity's menu. Ok(false) means a non-provider,
    /// cancelled open or unconnected/wrong-level player; a stale block entity
    /// returns BlockEntityError::Stale, never the replacement at its position.
    #[helper(
        side = "server",
        receiver = "player",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_player_open_block_menu"
    )]
    fn player_open_menu();
    #[helper(
        side = "both",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_kind"
    )]
    fn block_entity_kind();
    #[helper(
        side = "both",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_position"
    )]
    fn block_entity_position();
    #[helper(
        side = "both",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_level"
    )]
    fn block_entity_level();
    #[helper(
        side = "both",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_data"
    )]
    fn block_entity_data();
    /// Read the vanilla lock's item predicate as an NBT compound. None means no lock,
    /// or an unavailable block entity. The predicate follows vanilla LockCode semantics.
    #[helper(
        side = "both",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_lock"
    )]
    fn block_entity_lock();
    /// Set the vanilla lock's item predicate, or clear it with None. Returns false for
    /// an unavailable entity or no change. The write joins the current transaction.
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_set_lock"
    )]
    fn block_entity_set_lock();
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_merge_data"
    )]
    fn block_entity_merge_data();
    /// Replace the saved payload, not the block entity's type or position. Ignores
    /// identity fields, joins the transaction and restores the original data on abort.
    /// Changing type goes through the owning block's lifecycle, not this method.
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_replace_data"
    )]
    fn block_entity_replace_data();
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_set_changed"
    )]
    fn block_entity_set_changed();
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_name"
    )]
    fn block_entity_name();
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_set_name"
    )]
    fn block_entity_set_name();
    /// Read the players with this block's menu open (for a chest or barrel, the openers
    /// `ContainerOpenersCounter` counts; for a furnace, hopper or other block menu, its viewers):
    /// connected players in its level, not spectators, whose menu holds it; a double
    /// chest's menu holds both halves. Fake players are left out, as the opener count
    /// behind lids and `mod_container_viewers` leaves them out. Scans only when called.
    /// None for a stale or unloaded entity, or on a client, which sees no menus.
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_viewers"
    )]
    fn block_entity_viewers();
    /// Read a snapshot of a mod block entity's menu data slots, not its item storage.
    /// Reads the server's entity or the client's own copy; None for vanilla/missing,
    /// stale or unloaded entities. Use item_slot for internal items, ITEMS for automation.
    #[helper(
        side = "both",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_data_slots"
    )]
    fn block_entity_data_slots();
    /// Set a declared mod block-entity data slot on the server, without resizing it.
    /// Joins the current transaction, rolls back on abort, and calls setChanged at
    /// commit. False for an invalid/stale/client/parked entity, a missing slot, or
    /// no change. The normal menu broadcast carries changed values to its viewers.
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_set_data_slot"
    )]
    fn block_entity_set_data_slot();
    /// Declare or replace a mod block entity's menu data slots on the server.
    /// Creation and loading start with no data slots; call this with their initial
    /// values to size the vector, instead of assigning a mutable tick payload's
    /// data field. Use `set_data_slot` for later scalar changes without resizing.
    /// Joins the current journal: abort restores the previous vector; a committed
    /// change calls setChanged and the normal menu broadcast sends its values.
    /// Returns Ok(true) for a change, Ok(false) for identical values, or a typed
    /// BlockEntityDataError for an unavailable holder or too many slots (currently
    /// 256, the mod-menu limit). No values are clamped. A removed, replaced or
    /// reloaded holder returns BlockEntityDataError::Stale; reacquire its handle.
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_set_data_slots"
    )]
    fn block_entity_set_data_slots();
    /// Read an internal slot of a configured mod block entity on the server or
    /// the client's own loaded copy. A lazily missing slot is empty. Bounds use
    /// the current container's declared count; saved excess slots are not exposed.
    /// Returns BlockEntityItemError for unavailable holders or invalid indices;
    /// removed, replaced or reloaded holders return Stale.
    #[helper(
        side = "both",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_item_slot"
    )]
    fn block_entity_item_slot();
    /// Writes any slot of any mod block entity the caller holds a handle to;
    /// automation policy (inputs, outputs, faces) is not applied. Vanilla block
    /// entities are `Unavailable`. Use the `ITEMS` capability for external insertion.
    /// Requires the existing transaction: abort restores the before-image, and
    /// Changed runs at root commit. None clears; identical values return Ok(false).
    /// Bounds and registered/component stack limits are checked without clamping.
    /// Refusals return BlockEntityItemError without partial writes or Changed.
    /// A removed, replaced or reloaded holder returns BlockEntityItemError::Stale.
    #[helper(
        side = "server",
        receiver = "block_entity",
        target = "horizon_client::game::native_block_entity_refs::Game<'_>::native_block_entity_set_item_slot"
    )]
    fn block_entity_set_item_slot();
}

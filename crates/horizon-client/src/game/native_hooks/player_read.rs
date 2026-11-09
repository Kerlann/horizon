use super::*;

/// Helpers for native mods, kept as stable as the hook points.
// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// `Inventory.items`: the non-empty main inventory stacks in slot order, including
    /// their components. None for a stale or unavailable player; on clients only the
    /// local player's inventory is available.
    pub(in crate::game) fn player_inventory(&self, player: PlayerRef) -> Result<Option<Vec<ItemStack>>, mod_api::EntityReadError> {
use crate::game::{commands::Target,native_live_entity_reads::EntityReadSubject,native_read_dependencies::EntityReadDependency};

let target=match self.entity_read_dependency::<core::convert::Infallible>(EntityReadSubject::Player(player), &[EntityReadDependency::Inventory]) {Some(target)=>target?,None=>return Ok(None)};
Ok(match target { Target::Player=>Some(self.inventory.iter().flatten().cloned().collect()), Target::Guest(conn)=>self.command_guest(conn).map(|guest|guest.inventory.iter().flatten().cloned().collect()), _=>None })
}
}

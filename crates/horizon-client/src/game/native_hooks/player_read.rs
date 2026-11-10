use super::*;

/// Helpers for native mods, kept as stable as the hook points.
// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// `Inventory.items`: the non-empty main inventory stacks in slot order, including
    /// their components. None for a stale or unavailable player; on clients only the
    /// local player's inventory is available.
    pub(in crate::game) fn player_inventory(
        &self,
        player: PlayerRef,
    ) -> Result<Option<Vec<ItemStack>>, mod_api::EntityReadError> {
        self.read_player_inventory(None, player)
    }

    pub(in crate::game) fn read_player_inventory(
        &self,
        provider: Option<&dyn crate::game::native_entity_body_provider::EntityBodyProvider>,
        player: PlayerRef,
    ) -> Result<Option<Vec<ItemStack>>, mod_api::EntityReadError> {
        use crate::game::{
            native_base_read_rows::BaseReadSource, native_live_entity_reads::EntityReadSubject,
            native_read_dependencies::EntityReadDependency,
        };

        let target = match self.entity_read_admission(
            EntityReadSubject::Player(player),
            &[EntityReadDependency::Inventory],
            provider,
        ) {
            Some(target) => target,
            None => return Ok(None),
        };
        self.with_admitted_entity_source(target, provider, |source| match source.body {
            BaseReadSource::Player(_) => Some(self.inventory.iter().flatten().cloned().collect()),
            BaseReadSource::Guest(guest) => {
                Some(guest.inventory.iter().flatten().cloned().collect())
            }
            _ => None,
        })
    }
}

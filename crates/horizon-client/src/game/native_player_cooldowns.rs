//! A player's item cooldowns for native server mods, through a `PlayerRef`
//! (`Player.getCooldowns()`, vanilla's `ItemCooldowns`). A cooldown group
//! is an item id unless the item's `use_cooldown` component names another
//! (`minecraft:ender_pearl`, a mod's `mymod:wands`...);
//! `item_cooldown_group` gives a stack's. Reads return None for a stale
//! handle or a player who isn't online here; inside another player's turn
//! they still read a resting guest. Writes return false for those, for a
//! dead player, and, inside another player's turn, for anyone but that
//! player.
use super::Game;
use horizon_client::cooldowns::Cooldowns;
use horizon_client::items::ItemStack;
use horizon_client::mod_api::{NativeItemCooldown, PlayerRef};
use std::sync::Arc;

impl Game<'_> {
    /// The cooldowns of `player`, where they are kept now: a guest's own
    /// outside their turn, the game's while theirs is swapped in (as
    /// `native_player_ref` finds the player).
    fn native_cooldowns_of(&self, player: PlayerRef) -> Option<&Cooldowns> {
        if self.native_player_ref(player.uuid()) != Some(player) {
            return None;
        }
        let uuid = player.uuid();
        if self.lan_acting_uuid == Some(uuid) {
            return Some(&self.cooldowns);
        }
        let Some(host) = &self.lan else {
            return Some(&self.cooldowns);
        };
        match host.hub.guests.iter().find(|g| g.placed && g.uuid == uuid) {
            Some(g) => Some(host.play.get(&g.conn).map_or(&EMPTY, |p| &p.cooldowns)),
            None => Some(&self.cooldowns),
        }
    }

    /// `ItemCooldowns.getCooldownGroup(stack)`: the group `stack`'s
    /// cooldown is filed under, the group its `use_cooldown` component names
    /// (its own, else its item's default), else its item id.
    pub(super) fn native_item_cooldown_group(&self, stack: &ItemStack) -> String {
        let default = self
            .items()
            .get(&stack.item)
            .and_then(|info| info.use_cooldown.as_ref());
        horizon_client::cooldowns::group(stack, default).to_string()
    }
    /// Every group the player waits on and its ticks left, ordered by
    /// group.
    pub(super) fn native_player_item_cooldowns(
        &self,
        player: PlayerRef,
    ) -> Option<Vec<NativeItemCooldown>> {
        let mut out: Vec<NativeItemCooldown> = self
            .native_cooldowns_of(player)?
            .remaining()
            .filter(|(_, left)| *left > 0)
            .map(|(group, left)| NativeItemCooldown {
                group: group.to_string(),
                ticks_left: left,
            })
            .collect();
        out.sort_by(|a, b| a.group.cmp(&b.group));
        Some(out)
    }
    /// `ItemCooldowns.isOnCooldown`: whether `group` still has time to run.
    pub(super) fn native_player_has_item_cooldown(
        &self,
        player: PlayerRef,
        group: &str,
    ) -> Option<bool> {
        Some(self.native_cooldowns_of(player)?.is_on_cooldown(group))
    }
    /// `ItemCooldowns.getCooldownPercent(group, 0)`: what is left of the
    /// group's cooldown, from 1 (just started) to 0 (none).
    pub(super) fn native_player_item_cooldown_percent(
        &self,
        player: PlayerRef,
        group: &str,
    ) -> Option<f32> {
        Some(self.native_cooldowns_of(player)?.percent(group, 0.0))
    }
    /// `ItemCooldowns.addCooldown(group, ticks)`: starts (or restarts) the
    /// group's cooldown, and the player's client draws it on every item of the
    /// group (`ServerItemCooldowns.onCooldownStarted`).
    pub(super) fn native_player_set_item_cooldown(
        &mut self,
        player: PlayerRef,
        group: &str,
        ticks: i32,
    ) -> bool {
        let group: Arc<str> = group.into();
        self.native_player_ref(player.uuid()) == Some(player)
            && self.native_as_player(player.uuid(), |game| game.add_cooldown(group, ticks))
    }
    /// `ItemCooldowns.removeCooldown(group)`: ends the group's cooldown now,
    /// on the client too (`ServerItemCooldowns.onCooldownEnded`).
    pub(super) fn native_player_remove_item_cooldown(
        &mut self,
        player: PlayerRef,
        group: &str,
    ) -> bool {
        let group: Arc<str> = group.into();
        self.native_player_ref(player.uuid()) == Some(player)
            && self.native_as_player(player.uuid(), |game| {
                game.cooldowns.remove(&group);
                if game.lan_acting.is_some() {
                    game.guest_cooldowns.push((group, 0));
                }
            })
    }
}

/// What a guest with no play state yet waits on: nothing.
static EMPTY: std::sync::LazyLock<Cooldowns> = std::sync::LazyLock::new(Cooldowns::default);

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use horizon_client::items::ItemStack;
    use horizon_world::nbt::Tag;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_stack_names_its_cooldown_group() {
        with_game(|game| {
            let pearl = ItemStack::new("minecraft:ender_pearl", 1);
            assert_eq!(
                game.native_item_cooldown_group(&pearl),
                "minecraft:ender_pearl"
            );
            let mut wand = ItemStack::new("minecraft:stick", 1);
            wand.data_mut().set_other(
                "minecraft:use_cooldown",
                Some(Tag::compound([
                    ("seconds", Tag::Float(1.0f32.to_bits())),
                    ("cooldown_group", Tag::String("mymod:wands".into())),
                ])),
            );
            assert_eq!(game.native_item_cooldown_group(&wand), "mymod:wands");
            let unknown = ItemStack::new("mymod:not_an_item", 1);
            assert_eq!(
                game.native_item_cooldown_group(&unknown),
                "mymod:not_an_item"
            );
        });
    }
}

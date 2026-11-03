//! The game's item places for native mods (`mod_api::ItemPlace`, arismoko/horizon#41 §7): a
//! player's slots, hands and cursor, entities' slots and item entities. Mods read and write
//! them through `ItemPlace`'s `stack`, `insert` and `extract`, which come here for these places; a block entity's slots,
//! a stack inside an item and a mod's own places go through their handlers.
use super::transactions::After;
use super::{Game, minecarts::CartKind};
use horizon_client::{
    first_person::Hand,
    items::ItemStack,
    mod_api::{ItemPlace, Transaction},
};

/// The player a place belongs to, if a player's.
fn place_player(place: &ItemPlace) -> Option<u128> {
    match place {
        ItemPlace::PlayerSlot { player, .. }
        | ItemPlace::PlayerHand { player, .. }
        | ItemPlace::PlayerCursor { player } => Some(*player),
        _ => None,
    }
}

impl Game<'_> {
    /// The inventory of entity `id` in the level in place, if it has one: a chest or hopper
    /// minecart's (the open menu's copy while one shows it, as `cart_slots`), a chest boat's, or
    /// a chested donkey's, mule's or llama's (`AbstractHorse.inventory`, `getInventoryColumns`
    /// rows of three). With where it stands, for what a split throws.
    pub(super) fn entity_slots(&mut self, id: u32) -> Option<(&mut [Option<ItemStack>], [f64; 3])> {
        let key = (i32::MAX - 15, super::minecarts::CART_KEY_Y, id as i32);
        let at = |p: &horizon_core::geometry::Vec3| [p.x, p.y, p.z];
        let container = |c: &super::minecarts::Minecart| {
            c.id == id && matches!(c.kind, CartKind::Chest | CartKind::Hopper)
        };
        let position = if let Some(c) = self.carts.iter().find(|c| container(c)) {
            at(&c.position)
        } else if let Some(b) = self.boats.iter().find(|b| b.id == id && b.chest) {
            at(&b.position)
        } else {
            let m = self
                .mobs
                .iter_mut()
                .find(|m| m.id == id && m.extra.chested)?;
            let size = super::mount_menu::mount_columns(m) * 3;
            if m.extra.items.len() < size {
                if let Some(level) = self.transactions.last_mut() {
                    level.keep_mob_length(id, m.extra.items.len());
                }
                m.extra.items.resize(size, None);
            }
            return Some((&mut m.extra.items[..size], at(&m.position)));
        };
        if self.carts.iter().any(container) {
            return Some((self.cart_slots(id)?, position));
        }
        if let Some(super::BlockEntity::Chest(open)) = self.world.block_entities.get_mut(&key) {
            return Some((open, position));
        }
        let b = self.boats.iter_mut().find(|b| b.id == id)?;
        Some((&mut b.items, position))
    }
    /// Whether this game simulates a world: a client's copy has no places to write.
    fn places_simulated(&self) -> bool {
        self.serving.is_some() || self.options.dedicated.is_some()
    }
    /// The slot of the player in place that `place` names.
    fn player_place(&mut self, place: &ItemPlace) -> Option<&mut Option<ItemStack>> {
        match place {
            ItemPlace::PlayerSlot { slot, .. } => self.native_carried_slot(*slot),
            ItemPlace::PlayerHand {
                hand: Hand::Main, ..
            } => self.inventory.get_mut(self.selected),
            ItemPlace::PlayerHand {
                hand: Hand::Off, ..
            } => Some(&mut self.offhand),
            ItemPlace::PlayerCursor { .. } => Some(&mut self.carried),
            _ => None,
        }
    }
    /// The stack at a player's or an item entity's place; none for an empty one, a player who
    /// isn't here or is dead, an item entity not in the level in place, any other place
    /// (`ItemPlace::stack` reads those), and on a client.
    pub(super) fn native_game_stack(&mut self, place: &ItemPlace) -> Option<ItemStack> {
        if !self.places_simulated() {
            return None;
        }
        if let ItemPlace::ItemEntity { id } = place {
            return self
                .dropped
                .iter()
                .find(|e| e.id == *id)
                .map(|e| e.stack.clone());
        }
        if let ItemPlace::EntitySlot { entity, slot } = place {
            let (slots, _) = self.entity_slots(*entity)?;
            return slots.get(*slot).cloned().flatten().filter(|s| s.count > 0);
        }
        let mut stack = None;
        self.native_as_player(place_player(place)?, |g| {
            stack = g.player_place(place).and_then(|slot| slot.clone());
        });
        stack.filter(|s| s.count > 0)
    }
    /// `ItemAccess.insert` at a player's place, an item entity's or an entity's slot: up to
    /// `stack.count` of `stack`, into the place where it's empty or holds the same item and
    /// components, up to the item's stack size. The rest goes where vanilla puts an item split
    /// off it: a player's into their inventory, else dropped as `Player.drop(stack, false)` at
    /// the root commit (a creative player's hand takes anything, and gives them one if they have
    /// none, as `ItemUtils.createFilledResult`); an item entity's dropped beside it at the root
    /// commit; an entity slot's into the entity's other slots (`forHandlerIndex`). How much
    /// went: 0 where `native_game_stack` reads nothing for want of the place. In transaction
    /// `tx`, which keeps each place as it was. The player's screen follows with the tick's sync.
    pub(super) fn native_game_insert(
        &mut self,
        place: &ItemPlace,
        stack: ItemStack,
        tx: &Transaction,
    ) -> u32 {
        let _ = tx;
        if !self.places_simulated() || stack.count == 0 {
            return 0;
        }
        let max = self.containers().max_stack(&stack);
        // Puts what of `rest` fits into `slot`.
        let fill = |slot: &mut Option<ItemStack>, rest: &mut ItemStack| {
            let held = slot.as_ref().map_or(0, |held| held.count);
            let room = match slot {
                Some(h) if held > 0 && !h.same_item_same_components(rest) => 0,
                _ => max.saturating_sub(held),
            };
            let put = room.min(rest.count);
            if put > 0 {
                *slot = Some(ItemStack {
                    count: held + put,
                    ..rest.clone()
                });
                rest.count -= put;
            }
        };
        let mut rest = stack.clone();
        if let ItemPlace::ItemEntity { id } = place {
            let Some(e) = self.dropped.iter().find(|e| e.id == *id) else {
                return 0;
            };
            let at = [e.position.x, e.position.y, e.position.z];
            let mut held = Some(e.stack.clone());
            fill(&mut held, &mut rest);
            if let Some(held) = held.filter(|_| rest.count < stack.count) {
                self.journal_set_item_entity(*id, held);
            }
            if rest.count > 0 {
                self.after_commit(After::Spawn(at, rest));
            }
            return stack.count;
        }
        if let ItemPlace::EntitySlot { entity, slot } = place {
            if !self
                .entity_slots(*entity)
                .is_some_and(|(slots, _)| *slot < slots.len())
            {
                return 0;
            }
            self.journal_entity(*entity);
            let Some((slots, _)) = self.entity_slots(*entity) else {
                return 0;
            };
            fill(&mut slots[*slot], &mut rest);
            for (i, held) in slots.iter_mut().enumerate() {
                if i != *slot {
                    fill(held, &mut rest);
                }
            }
            return stack.count - rest.count;
        }
        let Some(player) = place_player(place) else {
            return 0;
        };
        let mut went = 0;
        self.native_as_player(player, |g| {
            if g.player_place(place).is_none() {
                return;
            }
            went = stack.count;
            g.journal_player(player);
            if g.creative && matches!(place, ItemPlace::PlayerHand { .. }) {
                let have = g.inventory.iter().flatten();
                if !have.clone().any(|h| h.same_item_same_components(&stack)) {
                    g.give(ItemStack { count: 1, ..stack });
                }
                return;
            }
            if let Some(slot) = g.player_place(place) {
                fill(slot, &mut rest);
            }
            if let Some(rest) = (rest.count > 0).then(|| g.give(rest)).flatten() {
                g.drop_after_commit(player, rest);
            }
        });
        went
    }
    /// `ItemAccess.extract` at those places: up to `stack.count` of the item and components of
    /// `stack`, how many came out. An item entity emptied stays, empty, until the root commit
    /// removes it. A creative player's hand gives without losing any (`forInfiniteMaterials`).
    pub(super) fn native_game_extract(
        &mut self,
        place: &ItemPlace,
        stack: ItemStack,
        tx: &Transaction,
    ) -> u32 {
        let _ = tx;
        if !self.places_simulated() {
            return 0;
        }
        // How many of `stack` `held` gives.
        let taken = |held: Option<&ItemStack>| {
            held.filter(|held| held.same_item_same_components(&stack))
                .map_or(0, |held| held.count.min(stack.count))
        };
        let left = |held: &ItemStack, took: u32| ItemStack {
            count: held.count - took,
            ..held.clone()
        };
        if let ItemPlace::ItemEntity { id } = place {
            let Some(e) = self.dropped.iter().find(|e| e.id == *id) else {
                return 0;
            };
            let took = taken(Some(&e.stack));
            let kept = left(&e.stack, took);
            if took > 0 {
                self.journal_set_item_entity(*id, kept);
            }
            return took;
        }
        if let ItemPlace::EntitySlot { entity, slot } = place {
            let Some((slots, _)) = self.entity_slots(*entity) else {
                return 0;
            };
            let took = taken(slots.get(*slot).and_then(Option::as_ref));
            if took == 0 {
                return 0;
            }
            self.journal_entity(*entity);
            let Some((slots, _)) = self.entity_slots(*entity) else {
                return 0;
            };
            let kept = slots[*slot].as_ref().map(|held| left(held, took));
            slots[*slot] = kept.filter(|kept| kept.count > 0);
            return took;
        }
        let Some(player) = place_player(place) else {
            return 0;
        };
        let mut took = 0;
        self.native_as_player(player, |g| {
            let creative = g.creative && matches!(place, ItemPlace::PlayerHand { .. });
            let Some(slot) = g.player_place(place) else {
                return;
            };
            took = taken(slot.as_ref());
            if took == 0 || creative {
                return;
            }
            g.journal_player(player);
            if let Some(slot) = g.player_place(place) {
                let kept = slot.as_ref().map(|held| left(held, took));
                *slot = kept.filter(|kept| kept.count > 0);
            }
        });
        took
    }
    /// Drops `stack` at the middle of the block at `pos`, as a block entity's handler gives up
    /// what it can't put back.
    pub(super) fn native_drop_at(&mut self, pos: [i32; 3], stack: ItemStack) {
        if self.places_simulated() && stack.count > 0 {
            // `Block.popResource`'s default pickup delay, at the root commit.
            self.after_commit(After::Drop(pos, stack));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use horizon_client::{
        items::ItemStack,
        lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
        mod_api::{Host, ItemPlace, ItemResource},
    };
    use horizon_protocol::packets::configuration::ClientInformation;
    use std::sync::{Arc, atomic::AtomicI32, mpsc};

    #[test]
    #[ignore = "needs prepared resources"]
    fn a_guests_place_is_written_as_theirs_and_an_abort_puts_it_back() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let settings = super::super::lan::LanSettings {
                port: 0,
                guest_commands: false,
                force_game_mode: false,
            };
            let net = NetConfig {
                announce: false,
                ..NetConfig::lan(0, "PlaceTest".into(), 2)
            };
            let config = HubConfig {
                default_game_mode: 0,
                guest_commands: false,
                force_game_mode: false,
                hardcore: false,
                view_distance: 10,
                simulation_distance: 10,
                dimension: "minecraft:overworld".into(),
                allow_flight: false,
                seed: 0,
                max_players: 2,
                integrated: false,
                levels: Default::default(),
            };
            game.lan_host(settings, net, config).unwrap();
            let (tx, _rx) = mpsc::channel();
            let mut guest = Guest::new(
                Joined {
                    conn: 1,
                    uuid: 123,
                    name: "guest".into(),
                    properties: Vec::new(),
                    info: ClientInformation::new(8),
                    brand: String::new(),
                    sender: PlayerSender::new(tx, Arc::default()),
                    latency: Arc::new(AtomicI32::new(0)),
                    address: "127.0.0.1:1".parse().unwrap(),
                },
                20,
                [0.0; 3],
                8,
            );
            guest.placed = true;
            guest.inventory[0] = Some(ItemStack::new("minecraft:bucket", 3));
            game.lan.as_mut().unwrap().hub.guests.push(guest);
            let host_before = game.inventory.clone();
            let guest_inventory =
                |game: &super::Game<'_>| game.lan.as_ref().unwrap().hub.guests[0].inventory.clone();
            let before = guest_inventory(game);
            let slot = ItemPlace::PlayerSlot {
                player: 123,
                slot: 0,
            };
            let water = ItemResource::of(&ItemStack::new("minecraft:water_bucket", 1));

            // Aborted, the guest's inventory is as it was, and the host's was never touched.
            let host: &mut dyn Host = game;
            let _ = host.transaction(|host, tx| {
                assert_eq!(slot.exchange(host, tx, &water, 1), 1);
                assert_eq!(slot.amount(host), 2);
                Err::<(), ()>(())
            });
            assert_eq!(guest_inventory(game), before);
            assert_eq!(game.inventory, host_before);

            // Committed, the guest keeps two buckets and gets the filled one.
            let host: &mut dyn Host = game;
            let _ = host.transaction(|host, tx| Ok::<_, ()>(slot.exchange(host, tx, &water, 1)));
            let after = guest_inventory(game);
            assert_eq!(after[0], Some(ItemStack::new("minecraft:bucket", 2)));
            let filled = after.iter().flatten();
            assert_eq!(
                filled
                    .filter(|s| &*s.item == "minecraft:water_bucket")
                    .count(),
                1
            );
            assert_eq!(game.inventory, host_before);
        });
    }
}

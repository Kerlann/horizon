//! Read-only handles to supported server menus.
use super::Game;
use horizon_client::{
    items::ItemStack,
    lan::{
        guest::Guest,
        menu::{SlotBacking, VEHICLE_KEY_Y},
    },
    mod_api::{LevelRef, MenuRef, PlayerRef},
};

/// A supported menu slot's underlying storage, for the item-place journal.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuBacking {
    Block {
        level: LevelRef,
        pos: [i32; 3],
        slot: usize,
    },
    Entity {
        level: LevelRef,
        id: u32,
        slot: usize,
    },
    Inventory(usize),
    Armor(usize),
    Offhand,
    CraftGrid(usize),
    CraftResult,
    Ender(usize),
    /// A native menu's transient slots, not a mod block entity's slots.
    Native(usize),
}

impl Game<'_> {
    fn native_menu_guest(&self, menu: MenuRef) -> Option<&Guest> {
        if self.lan_acting.is_some() {
            return None;
        }
        (self.native_player_ref(menu.player.uuid())? == menu.player).then_some(())?;
        let host = self.lan.as_ref()?;
        let guest = host
            .hub
            .guests
            .iter()
            .find(|g| g.placed && g.uuid == menu.player.uuid())?;
        (guest.menu_generation == menu.generation).then_some(())?;
        if let Some(open) = &guest.menu {
            (open.window == menu.window && open.hidden.is_empty()).then_some(guest)
        } else {
            let play = host.play.get(&guest.conn);
            if menu.window != 0
                || play.is_some_and(|p| {
                    p.merchant.is_some()
                        || p.crafting.table.is_some()
                        || p.station.is_some()
                        || p.lectern.is_some()
                        || p.mount_window.is_some()
                })
            {
                return None;
            }
            Some(guest)
        }
    }

    /// A player's current supported menu. Refuses crafting tables, merchants,
    /// workstations, lecterns, mounts and menus with hidden slots; also refuses
    /// clients, stale players and calls during a player swap. API @API:menu_refs@.
    pub(super) fn native_menu_ref(&self, player: PlayerRef) -> Option<MenuRef> {
        let host = self.lan.as_ref()?;
        let guest = host
            .hub
            .guests
            .iter()
            .find(|g| g.placed && g.uuid == player.uuid())?;
        let menu = MenuRef {
            player,
            window: guest.menu.as_ref().map_or(0, |m| m.window),
            generation: guest.menu_generation,
        };
        self.native_menu_guest(menu)?;
        Some(menu)
    }

    /// Container id (0 for the player's inventory menu). API @API:menu_refs@.
    pub(super) fn native_menu_container_id(&self, menu: MenuRef) -> Option<i32> {
        self.native_menu_guest(menu)?;
        Some(menu.window)
    }

    /// Registry menu type; the inventory menu has no MenuType in vanilla.
    /// The outer None means an unavailable menu. API @API:menu_refs@.
    pub(super) fn native_menu_kind(&self, menu: MenuRef) -> Option<Option<String>> {
        let guest = self.native_menu_guest(menu)?;
        match &guest.menu {
            Some(m) => Some(Some(
                self.lan
                    .as_ref()?
                    .hub
                    .registry_name("minecraft:menu", m.kind)?
                    .to_owned(),
            )),
            None => Some(None),
        }
    }

    /// Number of slots in AbstractContainerMenu.slots. API @API:menu_refs@.
    pub(super) fn native_menu_slot_count(&self, menu: MenuRef) -> Option<usize> {
        let guest = self.native_menu_guest(menu)?;
        Some(guest.menu.as_ref().map_or(46, |m| m.size + 36))
    }

    /// Map a menu slot to its actual backing storage for item-place writes.
    pub(super) fn native_menu_backing(&self, menu: MenuRef, slot: usize) -> Option<MenuBacking> {
        let guest = self.native_menu_guest(menu)?;
        let Some(open) = &guest.menu else {
            return match slot {
                0 => Some(MenuBacking::CraftResult),
                1..=4 => Some(MenuBacking::CraftGrid(slot - 1)),
                5..=8 => Some(MenuBacking::Armor(slot - 5)),
                9..=35 => Some(MenuBacking::Inventory(slot)),
                36..=44 => Some(MenuBacking::Inventory(slot - 36)),
                45 => Some(MenuBacking::Offhand),
                _ => None,
            };
        };
        if slot >= open.size {
            return match slot - open.size {
                n @ 0..=26 => Some(MenuBacking::Inventory(n + 9)),
                n @ 27..=35 => Some(MenuBacking::Inventory(n - 27)),
                _ => None,
            };
        }
        let level = self.native_level(&guest.dimension)?;
        let world = self.native_level_world(level)?;
        Some(match open.slots.backing(world, slot)? {
            SlotBacking::Block(pos, n) if pos.1 == VEHICLE_KEY_Y => MenuBacking::Entity {
                level,
                id: pos.2 as u32,
                slot: n,
            },
            SlotBacking::Block(pos, n) => MenuBacking::Block {
                level,
                pos: [pos.0, pos.1, pos.2],
                slot: n,
            },
            SlotBacking::Ender(n) => MenuBacking::Ender(n),
            SlotBacking::Native(n) => MenuBacking::Native(n),
        })
    }

    /// Full stack at a menu index. Outer None means unavailable or out of
    /// range; Some(None) means an empty slot. API @API:menu_refs@.
    pub(super) fn native_menu_slot(&self, menu: MenuRef, slot: usize) -> Option<Option<ItemStack>> {
        let backing = self.native_menu_backing(menu, slot)?;
        let guest = self.native_menu_guest(menu)?;
        Some(match backing {
            MenuBacking::Block { level, .. } | MenuBacking::Entity { level, .. } => guest
                .menu
                .as_ref()?
                .slots
                .get(self.native_level_world(level)?, guest, slot),
            MenuBacking::Inventory(n) => guest.inventory.get(n)?.clone(),
            MenuBacking::Armor(n) => guest.armor.get(n)?.clone(),
            MenuBacking::Offhand => guest.offhand.clone(),
            MenuBacking::CraftGrid(n) => guest.craft_grid.get(n)?.clone(),
            MenuBacking::CraftResult => guest.craft_result.clone(),
            MenuBacking::Ender(n) => guest.ender_items.get(n)?.clone(),
            MenuBacking::Native(n) => guest.native_menu.get(n)?.clone(),
        })
    }

    /// Carried (cursor) stack; outer None means unavailable. API @API:menu_refs@.
    pub(super) fn native_menu_carried(&self, menu: MenuRef) -> Option<Option<ItemStack>> {
        Some(self.native_menu_guest(menu)?.carried.clone())
    }
}
#[cfg(test)]
mod tests {
    use super::super::{lan::LanSettings, native_entity_refs::tests::with_game};
    use super::*;
    use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
    use horizon_protocol::packets::configuration::ClientInformation;
    use std::sync::{
        Arc,
        atomic::{AtomicI32, AtomicUsize},
    };

    fn guest() -> Guest {
        let (tx, _) = std::sync::mpsc::channel();
        let mut g = Guest::new(
            Joined {
                conn: 1,
                uuid: 123,
                name: "guest".into(),
                properties: Vec::new(),
                info: ClientInformation::new(8),
                brand: String::new(),
                sender: PlayerSender::new(tx, Arc::new(AtomicUsize::new(0))),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:1".parse().unwrap(),
            },
            1,
            [0.0; 3],
            10,
        );
        g.placed = true;
        g
    }

    #[test]
    #[ignore = "needs prepared resources"]
    fn menus_read_storage_and_invalidate_on_replacement_wrap_and_reconnect() {
        with_game(|game| {
            let config = HubConfig {
                default_game_mode: 0,
                guest_commands: false,
                force_game_mode: false,
                hardcore: false,
                view_distance: 4,
                simulation_distance: 4,
                dimension: "minecraft:overworld".into(),
                allow_flight: true,
                seed: 0,
                max_players: 10,
                integrated: true,
                levels: game.level_shapes,
            };
            let mut net = NetConfig::lan(0, "test".into(), 10);
            net.listen = false;
            game.lan_host(
                LanSettings {
                    port: 0,
                    guest_commands: false,
                    force_game_mode: false,
                },
                net,
                config,
            )
            .unwrap();
            game.lan.as_mut().unwrap().hub.guests.push(guest());
            let player = game.native_player_ref(123).unwrap();
            let inventory = game.native_menu_ref(player).unwrap();
            assert_eq!(inventory.container_id(game), Some(0));
            assert_eq!(inventory.kind(game), Some(None));
            assert_eq!(inventory.slot_count(game), Some(46));
            assert_eq!(inventory.slot(game, 0), Some(None));
            assert_eq!(
                game.native_menu_backing(inventory, 36),
                Some(MenuBacking::Inventory(0))
            );
            assert_eq!(
                game.native_menu_backing(inventory, 45),
                Some(MenuBacking::Offhand)
            );
            assert_eq!(inventory.slot(game, 46), None);
            let stone = ItemStack::new("minecraft:stone", 4);
            let host = game.lan.as_mut().unwrap();
            host.hub.guests[0].inventory[9] = Some(stone.clone());
            host.hub.guests[0].carried = Some(stone.clone());
            let title = horizon_world::nbt::Tag::String("test".into());
            let window = host
                .hub
                .open_native_menu(1, 1, title.clone(), vec![Some(stone.clone())])
                .unwrap();
            assert_eq!(inventory.slot_count(game), None);
            let menu = game.native_menu_ref(player).unwrap();
            assert_eq!(menu.container_id(game), Some(window));
            assert_eq!(menu.kind(game), Some(Some("minecraft:generic_9x1".into())));
            assert_eq!(menu.slot_count(game), Some(45));
            assert_eq!(menu.slot(game, 0), Some(Some(stone.clone())));
            assert_eq!(menu.slot(game, 1), Some(None));
            assert_eq!(menu.slot(game, 9), Some(Some(stone.clone())));
            assert_eq!(menu.carried(game), Some(Some(stone)));
            assert_eq!(
                game.native_menu_backing(menu, 0),
                Some(MenuBacking::Native(0))
            );
            assert_eq!(
                game.native_menu_backing(menu, 9),
                Some(MenuBacking::Inventory(9))
            );
            assert_eq!(
                game.native_menu_backing(menu, 36),
                Some(MenuBacking::Inventory(0))
            );
            assert_eq!(game.native_menu_backing(menu, 45), None);
            let host = game.lan.as_mut().unwrap();
            for _ in 0..100 {
                host.hub
                    .open_native_menu(1, 1, title.clone(), vec![])
                    .unwrap();
            }
            assert_eq!(host.hub.guests[0].menu.as_ref().unwrap().window, window);
            assert_eq!(menu.slot_count(game), None);
            let newer = game.native_menu_ref(player).unwrap();
            let host = game.lan.as_mut().unwrap();
            host.hub.close_container(1);
            assert_eq!(newer.slot_count(game), None);
            let inventory = game.native_menu_ref(player).unwrap();
            let host = game.lan.as_mut().unwrap();
            host.play.entry(1).or_default().crafting.table = Some((
                7,
                0,
                Box::new(horizon_client::menu::Menu::new(
                    horizon_client::menu::Kind::Crafting,
                )),
                [0; 3],
            ));
            assert_eq!(game.native_menu_ref(player), None);
            assert_eq!(inventory.slot_count(game), None);
            let host = game.lan.as_mut().unwrap();
            host.play.clear();
            host.hub.guests[0] = guest();
            assert_eq!(inventory.slot_count(game), None);

            use horizon_client::{containers::BlockEntity, lan::menu::Slots};
            let a = (1, 2, 3);
            let b = (2, 2, 3);
            let stone = ItemStack::new("minecraft:stone", 3);
            game.world
                .block_entities
                .insert(a, BlockEntity::Chest(vec![None; 2]));
            game.world
                .block_entities
                .insert(b, BlockEntity::Chest(vec![Some(stone.clone()); 3]));
            game.lan.as_mut().unwrap().hub.open_container(
                1,
                "minecraft:generic_9x1",
                title.clone(),
                a,
                Slots::Double(a, b),
                vec![None; 5],
            );
            let menu = game.native_menu_ref(player).unwrap();
            assert_eq!(
                game.native_menu_backing(menu, 2),
                Some(MenuBacking::Block {
                    level: game.native_level("minecraft:overworld").unwrap(),
                    pos: [b.0, b.1, b.2],
                    slot: 0
                })
            );
            assert_eq!(menu.slot(game, 2), Some(Some(stone.clone())));
            let vehicle = (i32::MAX - 15, VEHICLE_KEY_Y, 42);
            game.world
                .block_entities
                .insert(vehicle, BlockEntity::Chest(vec![Some(stone.clone())]));
            game.lan.as_mut().unwrap().hub.open_container(
                1,
                "minecraft:generic_9x1",
                title,
                vehicle,
                Slots::Block(vehicle),
                vec![None],
            );
            let menu = game.native_menu_ref(player).unwrap();
            assert_eq!(
                game.native_menu_backing(menu, 0),
                Some(MenuBacking::Entity {
                    level: game.native_level("minecraft:overworld").unwrap(),
                    id: 42,
                    slot: 0
                })
            );
            assert_eq!(menu.slot(game, 0), Some(Some(stone)));

            // Identical coordinates in different dimensions must not alias.
            let gold = ItemStack::new("minecraft:gold_ingot", 2);
            let emerald = ItemStack::new("minecraft:emerald", 3);
            game.world
                .block_entities
                .insert(a, BlockEntity::Chest(vec![Some(gold.clone())]));
            let title = horizon_world::nbt::Tag::String("dimension".into());
            game.lan.as_mut().unwrap().hub.open_container(
                1,
                "minecraft:generic_9x1",
                title.clone(),
                a,
                Slots::Block(a),
                vec![Some(gold.clone())],
            );
            let overworld_menu = game.native_menu_ref(player).unwrap();
            let overworld = game.native_level("minecraft:overworld").unwrap();
            assert!(
                game.enter_command_level(super::super::Dim::Nether)
                    .is_some()
            );
            game.world
                .block_entities
                .insert(a, BlockEntity::Chest(vec![Some(emerald.clone())]));
            let mut nether_guest = guest();
            nether_guest.conn = 2;
            nether_guest.uuid = 456;
            nether_guest.entity_id = 2;
            nether_guest.dimension = "minecraft:the_nether".into();
            game.lan.as_mut().unwrap().hub.guests.push(nether_guest);
            game.lan.as_mut().unwrap().hub.open_container(
                2,
                "minecraft:generic_9x1",
                title,
                a,
                Slots::Block(a),
                vec![Some(emerald.clone())],
            );
            let other_player = game.native_player_ref(456).unwrap();
            let nether_menu = game.native_menu_ref(other_player).unwrap();
            let nether = game.native_level("minecraft:the_nether").unwrap();
            assert_eq!(overworld_menu.slot(game, 0), Some(Some(gold)));
            assert_eq!(
                game.native_menu_backing(overworld_menu, 0),
                Some(MenuBacking::Block {
                    level: overworld,
                    pos: [a.0, a.1, a.2],
                    slot: 0
                })
            );
            assert!(
                game.enter_command_level(super::super::Dim::Overworld)
                    .is_some()
            );
            assert_eq!(nether_menu.slot(game, 0), Some(Some(emerald)));
            assert_eq!(
                game.native_menu_backing(nether_menu, 0),
                Some(MenuBacking::Block {
                    level: nether,
                    pos: [a.0, a.1, a.2],
                    slot: 0
                })
            );
        });
    }
}

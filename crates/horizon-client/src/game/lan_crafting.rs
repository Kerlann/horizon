//! Crafting for LAN guests: the inventory's 2x2 grid (window 0) and the
//! crafting table (`minecraft:crafting`) run the host's menu engine
//! (`horizon_client::menu`, `Kind::Inventory` / `Kind::Crafting`) over the
//! guest's inventory, with vanilla's packets: OpenScreen, container clicks
//! and their corrections, and the grid back into the inventory when the menu
//! closes (`AbstractContainerMenu.clearContainer`).
use super::Game;
use horizon_client::{
    containers::BlockEntity,
    furnace::FurnaceCooking,
    items::ItemStack,
    lan::{guest::Guest, menu::Slots},
    menu::{Env, Input, Kind, Menu},
};
use horizon_protocol::packets::play::{self, Clientbound as C};

/// `AbstractContainerMenu.dropOrPlaceInInventory`: actual network disconnects
/// drop inputs; a live close still returns them to the inventory.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuRemoval {
    Live,
    Disconnected,
}

/// The guest's menus the game runs: kept between clicks, so a drag (quick
/// craft) spans them as in vanilla.
#[derive(Default)]
pub(super) struct GuestCrafting {
    /// The inventory menu; its grid lives in the hub's guest.
    inventory: Option<Box<Menu>>,
    /// The open crafting table: its window, state id, menu and block.
    pub(super) table: Option<(i32, i32, Box<Menu>, [i32; 3])>,
    /// The open furnace's or brewing stand's window and menu (`AbstractFurnaceMenu`,
    /// `BrewingStandMenu`); its slots live in the block entity and the hub's
    /// container menu keeps the client in sync.
    pub(super) machine: Option<(i32, Box<Menu>)>,
}

/// ServerboundContainerClickPacket's input.
pub(super) fn input(id: i32) -> Input {
    match id {
        0 => Input::Pickup,
        1 => Input::QuickMove,
        2 => Input::Swap,
        3 => Input::Clone,
        4 => Input::Throw,
        5 => Input::QuickCraft,
        _ => Input::PickupAll,
    }
}

/// The armor's player-container slots, head to feet (`Inventory` 39..36).
const ARMOR: [usize; 4] = [39, 38, 37, 36];
const OFFHAND: usize = 40;

/// The guest's inventory, armor, off hand, selection and carried stack into `menu`.
pub(super) fn load_player(menu: &mut Menu, g: &Guest) {
    for i in 0..36 {
        menu.set_player_item(i, g.inventory[i].clone());
    }
    for (k, slot) in ARMOR.into_iter().enumerate() {
        menu.set_player_item(slot, g.armor[k].clone());
    }
    menu.set_player_item(OFFHAND, g.offhand.clone());
    menu.selected = g.selected;
    menu.creative = g.creative();
    menu.levels = g.xp_level;
    menu.carried = g.carried.clone();
}

/// The vanilla menu engine's kind for a guest's menu over block entity `entity`, with the
/// container's slots: a furnace's, a brewing stand's, a crafter's, or a content mod's menu type
/// with its slots' rules. None for the others, whose clicks the hub runs.
pub(super) fn machine_menu(
    entity: &BlockEntity,
    mod_type: Option<&'static horizon_client::containers::ModMenuType>,
) -> Option<(Kind, Vec<Option<ItemStack>>)> {
    Some(match entity {
        BlockEntity::Furnace(f) => {
            let recipe = match f.kind.recipe_type() {
                "minecraft:blasting" => "blasting",
                "minecraft:smoking" => "smoking",
                _ => "smelting",
            };
            (Kind::Furnace(recipe), f.items.to_vec())
        }
        BlockEntity::Brewing(b) => (Kind::Brewing, b.items.to_vec()),
        BlockEntity::Crafter(c) => (Kind::CrafterBlock, c.items.to_vec()),
        BlockEntity::Mod(m) => {
            let t = mod_type?;
            let mut items = m.items.clone();
            items.resize(t.slots, None);
            (Kind::Mod(t), items)
        }
        _ => return None,
    })
}

/// `load_player` back.
fn store_player(menu: &mut Menu, g: &mut Guest) {
    for i in 0..36 {
        g.inventory[i] = menu.player_item(i).cloned();
    }
    for (k, slot) in ARMOR.into_iter().enumerate() {
        g.armor[k] = menu.player_item(slot).cloned();
    }
    g.offhand = menu.player_item(OFFHAND).cloned();
    g.carried = menu.carried.take();
}

/// Inventory.placeItemBackInInventory for each stack: what doesn't fit is
/// left for Player.drop (creative players keep it too).
fn place_back(menu: &mut Menu, env: &Env, stacks: Vec<ItemStack>) {
    menu.creative = false;
    for mut stack in stacks {
        menu.give(env, &mut stack);
        if stack.count > 0 {
            menu.dropped.push(stack);
        }
    }
}

impl Game<'_> {
    /// `LocalPlayer.doLimitedCrafting`: whether limited crafting is active.
    pub fn limited_crafting(&self) -> bool {
        self.remote.as_ref().map_or_else(
            || self.ui.rules.bool("limited_crafting"),
            |r| r.do_limited_crafting,
        )
    }

    pub(super) fn menu_env(&self) -> Env<'_> {
        Env {
            player: self
                .lan_acting_uuid
                .or_else(|| self.remote.as_ref().map(|r| r.profile())),
            player_entity_id: Some(self.own_entity_id()),
            loop_context: &self.loop_context,
            items: self.items(),
            recipes: self.recipes(),
            brewing: &self.stations().brewing,
            banners: &self.stations().banners,
            station_recipes: &self.stations().recipes,
            enchantments: &self.stations().enchantments,
            names: &self.pack.names,
            trade: None,
            maps: None,
            limited_crafting: self.limited_crafting(),
            known_recipes: Some(&self.level.recipe_book),
        }
    }

    /// Runs `op` on the guest's menu with their inventory copied in and back.
    pub(super) fn guest_menu_op<R>(
        &mut self,
        gi: usize,
        menu: &mut Menu,
        op: impl FnOnce(&mut Menu, &Env) -> R,
    ) -> Option<R> {
        // The cartography table reads and makes the host level's maps.
        let held = if menu.kind() == Kind::Cartography {
            let g = &self.lan.as_ref()?.hub.guests[gi];
            Some(
                (0..menu.slot_count())
                    .filter_map(|s| menu.slot(s).cloned())
                    .chain(g.inventory.iter().flatten().cloned())
                    .chain(g.carried.clone())
                    .collect::<Vec<ItemStack>>(),
            )
        } else {
            None
        };
        let maps = held.as_ref().map(|held| self.level_maps(held));
        let host = self.lan.as_ref()?;
        let env = Env {
            player: Some(host.hub.guests[gi].uuid),
            player_entity_id: Some(host.hub.guests[gi].entity_id),
            loop_context: &self.loop_context,
            maps: maps.as_ref().map(|m| m as &dyn horizon_client::menu::Maps),
            limited_crafting: host.hub.limited_crafting,
            known_recipes: Some(&host.hub.guests[gi].recipe_book),
            ..self.menu_env()
        };
        load_player(menu, &host.hub.guests[gi]);
        menu.brewed_potion_hook = !mod_api::__private::mods().is_empty();
        menu.item_smelted_hook = menu.brewed_potion_hook;
        let result = op(menu, &env);
        if let Some(maps) = maps {
            self.register_made_maps(maps);
        }
        // A taken anvil result waits for AnvilCraftEvent.Pre: what the click
        // dropped waits with it (lan_station_op throws it or refuses the take).
        let dropped = if menu.anvil_craft.is_some() {
            Vec::new()
        } else {
            std::mem::take(&mut menu.dropped)
        };
        let equipped = std::mem::take(&mut menu.equipped);
        let brewed = std::mem::take(&mut menu.brewed);
        let brewed_potion_takes = std::mem::take(&mut menu.brewed_potion_takes);
        let crafted = std::mem::take(&mut menu.crafted);
        let item_crafted = std::mem::take(&mut menu.item_crafted);
        menu.drops.clear();
        let host = self.lan.as_mut()?;
        store_player(menu, &mut host.hub.guests[gi]);
        let guest = &host.hub.guests[gi];
        let (conn, uuid, entity_id) = (guest.conn, guest.uuid, guest.entity_id);
        if !brewed.is_empty() || !brewed_potion_takes.is_empty() {
            self.as_guest(conn, |g| {
                for stack in &brewed_potion_takes {
                    g.native_player_brewed_potion(uuid, entity_id, stack);
                }
                for stack in &brewed {
                    g.trigger_brewed_potion(stack);
                }
            });
        }
        if !crafted.is_empty() {
            self.as_guest(conn, |g| {
                for (recipe, ingredients) in &crafted {
                    g.trigger_recipe_crafted(recipe, ingredients);
                }
            });
        }
        if !item_crafted.is_empty() {
            self.as_guest(conn, |g| {
                let player = g.native_acting_id();
                for (recipe, stack) in &item_crafted {
                    g.native_item_crafted(player, recipe, stack);
                }
            });
        }
        if !equipped.is_empty() {
            self.as_guest(conn, |g| {
                for (slot, stack) in equipped {
                    g.player_equipped(slot, stack.as_ref());
                }
            });
        }
        self.drop_guest_items(gi, dropped);
        Some(result)
    }

    /// `InventoryMenu.slotsChanged` for guest `uuid`'s crafting grid written outside a click
    /// (a mod's menu slot write, at the root commit): the result made again from the grid, with
    /// `native_prepare_item_craft` as a click has it, kept by the guest and their menu. The
    /// client hears it with the tick's inventory sync.
    pub(super) fn guest_craft_result(&mut self, conn: u32) {
        let Some(g) = self
            .lan
            .as_ref()
            .and_then(|host| host.hub.guest(conn).filter(|g| g.placed))
        else {
            return;
        };
        let uuid = g.uuid;
        let mut result = self
            .recipes()
            .find(&self.loop_context, &g.craft_grid, 2)
            .map(|(_, stack)| stack);
        if mod_api::__private::listened(mod_api::HookId::PrepareItemCraft) {
            let name = g.name.clone();
            self.native_prepare_item_craft(uuid, &name, None, &mut result);
        }
        let Some(host) = self.lan.as_mut() else {
            return;
        };
        let Some(g) = host.hub.guest_mut(conn) else {
            return;
        };
        g.craft_result.clone_from(&result);
        let menu = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.crafting.inventory.take());
        if let Some(mut menu) = menu {
            menu.set_slot(&self.menu_env(), 0, result);
            if let Some(play) = self.lan.as_mut().and_then(|host| host.play.get_mut(&conn)) {
                play.crafting.inventory = Some(menu);
            }
        }
    }

    /// A click on the guest's inventory menu (window 0): `InventoryMenu`,
    /// crafting grid included. False for other windows.
    pub(super) fn lan_inventory_click(&mut self, conn: u32, p: &play::ContainerClick) -> bool {
        if p.container != 0 {
            return false;
        }
        let Some(host) = self.lan.as_mut() else {
            return true;
        };
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return true;
        };
        if host.hub.guests[gi].spectator() {
            return true;
        }
        let crafting = host.play.entry(conn).or_default();
        let mut menu = crafting
            .crafting
            .inventory
            .take()
            .unwrap_or_else(|| Box::new(Menu::new(Kind::Inventory)));
        let grid = host.hub.guests[gi].craft_grid.clone();
        let Some(changed) = self.guest_menu_op(gi, &mut menu, |menu, env| {
            for (k, s) in grid.into_iter().enumerate() {
                menu.set_slot(env, 1 + k, s);
            }
            let before: Vec<_> = (0..menu.slot_count())
                .map(|s| menu.slot(s).cloned())
                .collect();
            let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
            (0..menu.slot_count())
                .filter(|&s| menu.slot(s) != before[s].as_ref())
                .collect::<Vec<_>>()
        }) else {
            return true;
        };
        let Some(host) = self.lan.as_ref() else {
            return true;
        };
        let guest = &host.hub.guests[gi];
        let (uuid, name) = (guest.uuid, guest.name.clone());
        let mut result = menu.slot(0).cloned();
        self.native_prepare_item_craft(uuid, &name, None, &mut result);
        let env = self.menu_env();
        menu.set_slot(&env, 0, result);
        let Some(host) = self.lan.as_mut() else {
            return true;
        };
        let g = &mut host.hub.guests[gi];
        for k in 0..4 {
            g.craft_grid[k] = menu.slot(1 + k).cloned();
        }
        g.craft_result = menu.slot(0).cloned();
        host.play.entry(conn).or_default().crafting.inventory = Some(menu);
        host.hub.inventory_clicked(conn, p, &self.world, &changed);
        true
    }

    /// A click on the guest's open furnace, blast furnace, smoker or brewing stand:
    /// the host's `AbstractFurnaceMenu` (fuel slot, take-only result, quick moves by
    /// recipe and fuel), `BrewingStandMenu` (one bottle per slot, ingredient and
    /// blaze powder slots) or a content mod's menu type (`Kind::Mod`) over the
    /// block entity's slots. False for other windows.
    pub(super) fn lan_machine_click(&mut self, conn: u32, p: &play::ContainerClick) -> bool {
        let Some(host) = self.lan.as_mut() else {
            return false;
        };
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return false;
        };
        let Some((pos, mod_type, hidden)) = host.hub.guests[gi]
            .menu
            .as_ref()
            .filter(|m| p.container != 0 && m.window == p.container)
            .and_then(|m| match m.slots {
                Slots::Block(pos) => Some((pos, m.mod_type, m.hidden.clone())),
                _ => None,
            })
        else {
            return false;
        };
        let Some((kind, items)) = self
            .world
            .block_entities
            .get(&pos)
            .and_then(|entity| machine_menu(entity, mod_type))
        else {
            return false;
        };
        let furnace = matches!(kind, Kind::Furnace(_));
        // CrafterBlockEntity.containerData: the disabled slots, then triggered.
        let crafter_data = match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Crafter(c)) => Some((c.disabled, c.triggered)),
            _ => None,
        };
        if host.hub.guests[gi].spectator() {
            // handleContainerClick: nothing changes, and sendAllDataToRemote
            // takes back what the client predicted.
            host.hub.resend_menu(conn, p.container, &self.world);
            return true;
        }
        let mut menu = host
            .play
            .entry(conn)
            .or_default()
            .crafting
            .machine
            .take()
            .filter(|(window, m)| *window == p.container && m.kind() == kind)
            .map_or_else(|| Box::new(Menu::new(kind)), |(_, m)| m);
        for &slot in &hidden {
            menu.hide_slot(slot);
        }
        let Some(changed) = self.guest_menu_op(gi, &mut menu, |menu, env| {
            for (k, s) in items.iter().enumerate() {
                menu.set_slot(env, k, s.clone());
            }
            // After the slots: setting a disabled slot (CrafterBlockEntity.setItem) enables it.
            if let Some((disabled, triggered)) = crafter_data {
                for (k, d) in disabled.iter().enumerate() {
                    menu.data[k] = i32::from(*d);
                }
                menu.data[9] = i32::from(triggered);
            }
            let before: Vec<_> = (0..menu.slot_count())
                .map(|s| menu.slot(s).cloned())
                .collect();
            let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
            (0..menu.slot_count())
                .filter(|&s| menu.slot(s) != before[s].as_ref())
                .collect::<Vec<_>>()
        }) else {
            return true;
        };
        match self.world.block_entities.get_mut(&pos) {
            Some(BlockEntity::Furnace(f)) => {
                for k in 0..3 {
                    f.items[k] = menu.slot(k).cloned();
                }
                // AbstractFurnaceBlockEntity.setItem: only a new input restarts cooking.
                f.input_clicked(&self.loop_context, world_recipes!(self), items[0].clone());
            }
            // BrewingStandBlockEntity.serverTick notices a new ingredient itself.
            Some(BlockEntity::Brewing(b)) => {
                for k in 0..5 {
                    b.items[k] = menu.slot(k).cloned();
                }
            }
            // The mod's items past its menu's slots stay as they were.
            Some(BlockEntity::Mod(m)) => {
                let n = items.len();
                if m.items.len() < n {
                    m.items.resize(n, None);
                }
                for (k, slot) in m.items.iter_mut().take(n).enumerate() {
                    *slot = menu.slot(k).cloned();
                }
            }
            // CrafterBlockEntity.setItem: filling a disabled slot enables it.
            Some(BlockEntity::Crafter(c)) => {
                for k in 0..9 {
                    c.items[k] = menu.slot(k).cloned();
                    c.disabled[k] = menu.data[k] == 1;
                }
            }
            _ => {}
        }
        // FurnaceResultSlot.checkTakeAchievements, each as the click made it.
        let Some(guest) = self.lan.as_ref().map(|host| &host.hub.guests[gi]) else {
            return true;
        };
        let (uuid, entity_id, at) = (guest.uuid, guest.entity_id, guest.position);
        for (stack, removed) in std::mem::take(&mut menu.item_smelted) {
            self.native_item_smelted(uuid, entity_id, stack.as_ref(), removed);
        }
        // The guest took from the output.
        let count = |s: Option<&ItemStack>| s.map_or(0, |s| s.count);
        if furnace && count(menu.slot(2)) < count(items[2].as_ref()) {
            let output = items[2].as_ref();
            if let Some(output) = output
                && mod_api::__private::listened(mod_api::HookId::FurnaceExtract)
                && let Some(name) = self
                    .lan
                    .as_ref()
                    .map(|host| host.hub.guests[gi].name.clone())
            {
                let taken = output.count - count(menu.slot(2));
                self.native_furnace_extract(
                    uuid,
                    &name,
                    [pos.0, pos.1, pos.2],
                    &output.item,
                    taken,
                );
            }
            self.award_furnace_recipes(pos, at);
        }
        // Slot.setChanged on a changed container slot: BlockEntity.setChanged,
        // so the chunk is saved and comparators read the block again.
        if changed.iter().any(|&s| s < items.len()) {
            self.container_changed(pos);
        }
        if let Some(host) = self.lan.as_mut() {
            host.play.entry(conn).or_default().crafting.machine = Some((p.container, menu));
            host.hub.inventory_clicked(conn, p, &self.world, &changed);
        }
        true
    }

    /// `ServerGamePacketListenerImpl.handleContainerSlotStateChanged`: a
    /// guest's crafter slot toggle (`CrafterBlockEntity.setSlotState`) in
    /// their open crafter window.
    pub(super) fn lan_crafter_slot_state(
        &mut self,
        conn: u32,
        window: i32,
        slot: i32,
        enabled: bool,
    ) {
        let Some(host) = self.lan.as_ref() else {
            return;
        };
        let Some(pos) = host
            .hub
            .guest(conn)
            .filter(|g| !g.spectator())
            .and_then(|g| g.menu.as_ref())
            .filter(|m| window != 0 && m.window == window)
            .and_then(|m| match m.slots {
                Slots::Block(pos) => Some(pos),
                _ => None,
            })
        else {
            return;
        };
        if let (Some(BlockEntity::Crafter(c)), Ok(slot)) = (
            self.world.block_entities.get_mut(&pos),
            usize::try_from(slot),
        ) {
            c.set_slot_state(slot, enabled);
            self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        }
    }

    /// ServerGamePacketListenerImpl.handlePlaceRecipe: the guest's recipe
    /// book places a known recipe in their inventory grid or crafting table
    /// (`ServerPlaceRecipe`), and answers with a ghost recipe when the items
    /// are missing.
    pub(super) fn lan_place_recipe(
        &mut self,
        conn: u32,
        p: &horizon_protocol::packets::recipes::PlaceRecipe,
    ) {
        use horizon_client::menu::place::PostPlace;
        let Some(host) = self.lan.as_ref() else {
            return;
        };
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        let g = &host.hub.guests[gi];
        if g.spectator() {
            return;
        }
        // The server menu uses the world's active data-pack tables, the same
        // tables used by the guest menu below. The base resources can have a
        // different display registry or placement for an overridden recipe.
        let Some(id) = world_stations!(self)
            .recipe_index
            .recipe_of_display(p.recipe)
            .map(str::to_owned)
        else {
            return;
        };
        // An unknown or impossible recipe is refused outright.
        if !g.recipe_book.knows(&id)
            || world_recipes!(self)
                .placement(&id)
                .is_none_or(|placement| placement.impossible())
        {
            return;
        }
        let (player, name, creative) = (g.uuid, g.name.clone(), g.creative());
        self.native_player_recipe_book_click(player, &name, &id, p.use_max);
        let Some(host) = self.lan.as_mut() else {
            return;
        };
        let placed = if p.container == 0 {
            let crafting = host.play.entry(conn).or_default();
            let mut menu = crafting
                .crafting
                .inventory
                .take()
                .unwrap_or_else(|| Box::new(Menu::new(Kind::Inventory)));
            let grid = host.hub.guests[gi].craft_grid.clone();
            let Some(placed) = self.guest_menu_op(gi, &mut menu, |menu, env| {
                for (k, s) in grid.into_iter().enumerate() {
                    menu.set_slot(env, 1 + k, s);
                }
                menu.place_recipe(env, &id, p.use_max, creative, None)
            }) else {
                return;
            };
            let Some(host) = self.lan.as_mut() else {
                return;
            };
            let g = &mut host.hub.guests[gi];
            for k in 0..4 {
                g.craft_grid[k] = menu.slot(1 + k).cloned();
            }
            g.craft_result = menu.slot(0).cloned();
            host.play.entry(conn).or_default().crafting.inventory = Some(menu);
            host.hub.send_inventory(gi);
            placed
        } else {
            let Some((window, state, mut menu, pos)) = host
                .play
                .get_mut(&conn)
                .and_then(|play| play.crafting.table.take_if(|(w, ..)| *w == p.container))
            else {
                return;
            };
            let Some(placed) = self.guest_menu_op(gi, &mut menu, |menu, env| {
                menu.place_recipe(env, &id, p.use_max, creative, None)
            }) else {
                return;
            };
            let Some(host) = self.lan.as_mut() else {
                return;
            };
            host.play.entry(conn).or_default().crafting.table = Some((window, state, menu, pos));
            self.lan_send_table_content(conn);
            placed
        };
        if placed == PostPlace::Ghost
            && let Some(host) = self.lan.as_mut()
        {
            host.hub.send_ghost_recipe(conn, p.container, &id);
        }
    }

    /// The guest opened a crafting table (`CraftingTableBlock.getMenuProvider`).
    pub(super) fn lan_open_crafting(&mut self, conn: u32, pos: [i32; 3]) {
        if !self.guest_may_open(conn, "minecraft:crafting", Some(pos)) {
            return;
        }
        // ServerPlayer.openMenu closes the menu that was open.
        self.guest_close_all(conn);
        let Some(host) = self.lan.as_mut() else {
            return;
        };
        let title = horizon_protocol::text::translatable("container.crafting", &[]);
        let Some(window) = host.hub.open_window(conn, "minecraft:crafting", title) else {
            return;
        };
        let menu = Box::new(Menu::new(Kind::Crafting));
        host.play.entry(conn).or_default().crafting.table = Some((window, 0, menu, pos));
        self.lan_send_table_content(conn);
    }

    /// A click on the guest's crafting table window; false for other windows.
    pub(super) fn lan_crafting_click(&mut self, conn: u32, p: &play::ContainerClick) -> bool {
        let Some(host) = self.lan.as_mut() else {
            return false;
        };
        let Some(play) = host.play.get_mut(&conn) else {
            return false;
        };
        let Some((window, state, mut menu, pos)) = play
            .crafting
            .table
            .take_if(|(window, ..)| *window == p.container)
        else {
            return false;
        };
        // handleContainerClick: broadcastFullState only for a click on an old
        // state, else only what the client got wrong. The whole menu after every
        // click would undo the clicks it has predicted since.
        let mut agreed = false;
        // A table that is no longer valid takes no clicks (handleContainerClick).
        let gi = host.hub.guests.iter().position(|g| g.conn == conn);
        let Some(host) = self.lan.as_ref() else {
            return false;
        };
        if let Some(gi) = gi
            && !host.hub.guests[gi].spectator()
            && self.lan_table_valid(&host.hub.guests[gi], pos)
        {
            let Some(mut changed) = self.guest_menu_op(gi, &mut menu, |menu, env| {
                let before: Vec<_> = (0..menu.slot_count())
                    .map(|s| menu.slot(s).cloned())
                    .collect();
                let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), input(p.input));
                (0..menu.slot_count())
                    .filter(|&s| menu.slot(s) != before[s].as_ref())
                    .collect::<Vec<_>>()
            }) else {
                return true;
            };
            let Some(guest) = self.lan.as_ref().map(|host| &host.hub.guests[gi]) else {
                return true;
            };
            let (uuid, name) = (guest.uuid, guest.name.clone());
            let mut result = menu.slot(0).cloned();
            let vanilla = result.clone();
            self.native_prepare_item_craft(uuid, &name, Some(pos), &mut result);
            if result != vanilla && !changed.contains(&0) {
                changed.push(0);
            }
            let env = self.menu_env();
            menu.set_slot(&env, 0, result);
            let Some(host) = self.lan.as_mut() else {
                return true;
            };
            agreed = p.state == state
                && host.hub.click_agreed(
                    gi,
                    p,
                    menu.slot_count(),
                    &|s| menu.slot(s).cloned(),
                    &changed,
                );
            if agreed {
                // The inventory menu's sync isn't to send what the click settled.
                let g = &mut host.hub.guests[gi];
                for &s in changed.iter().filter(|&&s| s >= 10) {
                    g.menu_sent[s - 1] = menu.slot(s).cloned();
                }
            }
        }
        let Some(host) = self.lan.as_mut() else {
            return true;
        };
        host.play.entry(conn).or_default().crafting.table = Some((window, state, menu, pos));
        if !agreed {
            self.lan_send_table_content(conn);
        }
        true
    }

    /// `CraftingMenu.stillValid`: the block is still a crafting table and the
    /// guest is within reach of it.
    pub(super) fn lan_table_valid(&self, g: &Guest, pos: [i32; 3]) -> bool {
        let name = &self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]))
            .name;
        &**name == "minecraft:crafting_table" && g.within_block_range(pos, 4.0)
    }

    /// `ServerPlayer.tick`'s `stillValid`: a guest's crafting table closes
    /// once it is gone or out of reach, giving its grid back.
    pub(super) fn lan_tables_tick(&mut self) {
        let Some(host) = self.lan.as_ref() else {
            return;
        };
        let invalid: Vec<(u32, i32)> = host
            .play
            .iter()
            .filter_map(|(c, p)| p.crafting.table.as_ref().map(|t| (*c, t.0, t.3)))
            .filter(|&(c, _, pos)| {
                host.hub
                    .guests
                    .iter()
                    .find(|g| g.conn == c)
                    .is_some_and(|g| host.hub.here(g) && !self.lan_table_valid(g, pos))
            })
            .map(|(c, window, _)| (c, window))
            .collect();
        for (conn, window) in invalid {
            self.lan_crafting_closed(conn, Some(window));
            let Some(host) = self.lan.as_mut() else {
                return;
            };
            host.hub.send_to(
                conn,
                &C::ContainerClose(play::ContainerClose { container: window }),
            );
            if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
                self.guest_carried_back(gi);
            }
        }
    }

    /// The whole crafting table menu for the client (`sendAllDataToRemote`).
    fn lan_send_table_content(&mut self, conn: u32) {
        let Some(host) = self.lan.as_mut() else {
            return;
        };
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return;
        };
        let Some((window, state, menu, _)) = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.crafting.table.as_mut())
        else {
            return;
        };
        *state = (*state + 1) & 0x7FFF;
        let (window, state) = (*window, *state);
        let mut items: Vec<Option<ItemStack>> = (0..10).map(|s| menu.slot(s).cloned()).collect();
        let g = &host.hub.guests[gi];
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let items = items.iter().map(|s| host.hub.stack(s)).collect();
        let carried = host.hub.stack(&carried);
        host.hub.send_to(
            conn,
            &C::ContainerSetContent(play::ContainerSetContent {
                container: window,
                state,
                items,
                carried,
            }),
        );
        host.hub.guests[gi].inventory_told();
    }

    /// `AbstractContainerMenu.removed` of a crafting menu: the closed window
    /// (`window`; None closes the table whichever it is, and 0 the inventory
    /// menu) gives its grid back to the inventory, what doesn't fit is
    /// dropped. False when no such window is open.
    /// The open crafting table's own slots (result and grid) before the
    /// player's inventory, if the guest has one open.
    pub(super) fn lan_crafting_table_own_slots(&self, conn: u32) -> Option<usize> {
        let (_, _, menu, _) = self
            .lan
            .as_ref()?
            .play
            .get(&conn)?
            .crafting
            .table
            .as_ref()?;
        Some(menu.slot_count() - 36)
    }

    pub(super) fn lan_crafting_closed(&mut self, conn: u32, window: Option<i32>) -> bool {
        self.lan_crafting_removed(conn, window, MenuRemoval::Live)
    }

    /// Remove only input slots; disconnected players drop their inputs.
    pub(super) fn lan_crafting_removed(
        &mut self,
        conn: u32,
        window: Option<i32>,
        removal: MenuRemoval,
    ) -> bool {
        let Some(host) = self.lan.as_mut() else {
            return false;
        };
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else {
            return false;
        };
        let crafting = &mut host.play.entry(conn).or_default().crafting;
        let grid: Vec<ItemStack> = if window == Some(0) {
            crafting.inventory = None;
            let g = &mut host.hub.guests[gi];
            g.craft_result = None;
            g.craft_grid.iter_mut().filter_map(Option::take).collect()
        } else {
            match crafting
                .table
                .take_if(|(w, ..)| window.is_none_or(|window| *w == window))
            {
                Some((_, _, menu, _)) => (1..10).filter_map(|s| menu.slot(s).cloned()).collect(),
                None => return false,
            }
        };
        if !grid.is_empty() {
            match removal {
                MenuRemoval::Live => self.lan_place_back(gi, grid),
                MenuRemoval::Disconnected => self.drop_guest_items(gi, grid),
            }
        }
        true
    }

    /// `Inventory.placeItemBackInInventory` of `stacks` for guest `gi`, what
    /// doesn't fit dropped (`clearContainer`).
    pub(super) fn lan_place_back(&mut self, gi: usize, stacks: Vec<ItemStack>) {
        let mut menu = Menu::new(Kind::Inventory);
        self.guest_menu_op(gi, &mut menu, |menu, env| place_back(menu, env, stacks));
    }

    /// A guest left: what their crafting grids and workstation held goes back into their
    /// inventory before it is saved (dropped where they stood if full).
    pub(super) fn lan_crafting_left(&mut self, g: &mut Guest) {
        // The guest's play state stays: their statistics are awarded and saved
        // after this (`HostEvent::Left`), which removes it.
        let Some(host) = self.lan.as_mut() else {
            return;
        };
        let play = host.play.get_mut(&g.conn);
        let mut grid: Vec<ItemStack> = g.craft_grid.iter_mut().filter_map(Option::take).collect();
        g.craft_result = None;
        if let Some(play) = play {
            if let Some((_, _, menu, _)) = play.crafting.table.take() {
                grid.extend((1..10).filter_map(|s| menu.slot(s).cloned()));
            }
            // A workstation's inputs too.
            if let Some(station) = play.station.take() {
                grid.extend(super::lan_stations::station_inputs(&station));
            }
        }
        if grid.is_empty() {
            return;
        }
        let mut menu = Menu::new(Kind::Inventory);
        load_player(&mut menu, g);
        place_back(&mut menu, &self.menu_env(), grid);
        let dropped = std::mem::take(&mut menu.dropped);
        store_player(&mut menu, g);
        for stack in dropped {
            self.spawn_item(stack, g.position, [0.0, 0.1, 0.0], 40);
        }
    }
}

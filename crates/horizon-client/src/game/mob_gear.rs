//! Enchanting what mobs spawn with (`Mob.populateDefaultEquipmentEnchantments`
//! through the `mob_spawn_equipment` provider) and the raid buffs
//! (`Raider.applyRaidBuffs`).
use super::Game;
use horizon_client::{
    mobs::{
        Species,
        drops::{self, SLOTS},
        finalize::equip_stack,
        gear::{self, ARMOR, MAIN},
        spawner::equipment_table,
    },
    raid::{self, RaiderType},
};
use horizon_core::random::RandomSource;
use horizon_world::{loot::EntityLootParams, nbt::Tag};

/// `EnchantmentsByCostWithDifficulty` of `mob_spawn_equipment`.
const MIN_COST: i32 = 5;
const MAX_COST_SPAN: f32 = 17.0;

/// The mobs that call `populateDefaultEquipmentEnchantments` when they spawn.
fn enchants_spawn_gear(s: Species) -> bool {
    matches!(
        s,
        Species::Zombie
            | Species::Husk
            | Species::Drowned
            | Species::ZombieVillager
            | Species::ZombifiedPiglin
            | Species::Skeleton
            | Species::Stray
            | Species::Bogged
            | Species::Parched
            | Species::Pillager
            | Species::Vindicator
            | Species::Piglin
            | Species::Vex
    )
}
/// The item a mob holds or wears in an equipment slot.
fn slot_item(m: &horizon_client::mobs::Mob, slot: usize) -> Option<std::sync::Arc<str>> {
    use horizon_items::equipment as eq;
    let slot = match slot {
        gear::MAIN => eq::MAINHAND,
        gear::OFF => eq::OFFHAND,
        gear::HEAD => eq::HEAD,
        gear::CHEST => eq::CHEST,
        gear::LEGS => eq::LEGS,
        _ => eq::FEET,
    };
    horizon_client::mobs::finalize::item(m, slot)
}

impl Game<'_> {
    /// `Mob.equip(EquipmentTable)` for a spawner's `equipment`: the loot table
    /// (the mob as `this`, its random sequence) is rolled and each stack goes
    /// in the slot `EquipmentUser.resolveSlot` picks, with the table's drop
    /// chance for that slot.
    pub(super) fn equip_from_table(&mut self, id: u32, table: &Tag) { let (id,) = match crate::points_generated::P2772.get() { Some(__l) => { let mut __a = (id,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (id,) };
        let Some((name, chances)) = equipment_table(table) else {
            return;
        };
        let Some(loot) = self.server_loot.get() else {
            return;
        };
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        if !loot.has_table(&name) {
            return;
        }
        let params = EntityLootParams {
            this: drops::loot_entity(m, None),
            ..EntityLootParams::default()
        };
        let seed = self.level.seed;
        let stacks = match loot.random_sequence(&name) {
            Some(sequence) => {
                let random = self.rand_sequences.get(sequence, seed);
                loot.roll_entity(&name, random, &params)
            }
            None => loot.roll_entity(&name, &mut self.world.random, &params),
        };
        let mut inserted: Vec<usize> = Vec::new();
        for stack in stacks.iter().filter(|s| !s.is_empty()) {
            // `resolveSlot`: an `equippable` item goes in its own slot, any
            // other in the main hand, each slot once.
            let slot = match self
                .res
                .items()
                .get(&stack.item)
                .and_then(|i| i.equip_slot.as_deref())
            {
                Some(name) => SLOTS.iter().position(|s| *s == name),
                None => Some(0),
            };
            let Some(slot) = slot.filter(|s| !inserted.contains(s)) else {
                continue;
            };
            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                equip_stack(m, slot, stack, chances[slot]);
            }
            inserted.push(slot);
        }
    }
    /// `Mob.populateDefaultEquipmentEnchantments`: the weapon one time in
    /// four and each piece of armour one time in two, scaled by the local
    /// difficulty, enchanted as if at an enchanting table of cost 5 to 22.
    pub(super) fn enchant_spawn_equipment(&mut self, id: u32) { let (id,) = match crate::points_generated::P2771.get() { Some(__l) => { let mut __a = (id,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (id,) };
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let species = m.species();
        if !enchants_spawn_gear(species) {
            return;
        }
        let special = self
            .local_difficulty([m.position.x, m.position.y, m.position.z])
            .special();
        self.enchant_slot(id, MAIN, 0.25, special);
        for slot in ARMOR {
            self.enchant_slot(id, slot, 0.5, special);
        }
        // Pillager.enchantSpawnedWeapon: one crossbow in 300 pierces.
        if species == Species::Pillager && self.world.random.next_int_bound(300).unwrap_or(1) == 0 {
            self.enchant_single(id, MAIN, "minecraft:piercing", 1, "minecraft:crossbow");
        }
    }
    /// `Zombie` / `AbstractSkeleton` halloween pumpkin after spawn gear.
    pub(super) fn halloween_spawn_head(&mut self, id: u32) { let (id,) = match crate::points_generated::P2770.get() { Some(__l) => { let mut __a = (id,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (id,) };
        let halloween = horizon_client::mobs::finalize::is_halloween();
        let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
            return;
        };
        let mut random = self.mobs[i].random.clone();
        horizon_client::mobs::finalize::halloween_head(&mut self.mobs[i], &mut random, halloween);
        self.mobs[i].random = random;
    }
    fn enchant_slot(&mut self, id: u32, slot: usize, chance: f32, special: f32) { let (id,slot,chance,special,) = match crate::points_generated::P2769.get() { Some(__l) => { let mut __a = (id,slot,chance,special,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (id,slot,chance,special,) };
        let Some(item) = self
            .mobs
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| slot_item(m, slot))
        else {
            return;
        };
        if self.world.random.next_float() >= chance * special {
            return;
        }
        self.enchant_item(id, slot, &item, special);
    }
    /// `EnchantmentsByCostWithDifficulty.enchant` on the item in a slot.
    pub(super) fn enchant_item(&mut self, id: u32, slot: usize, item: &str, special: f32) { let (id,slot,special,) = match crate::points_generated::P2768.get() { Some(__l) => { let mut __a = (id,slot,special,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (id,slot,special,) };
        let random = &mut self.world.random;
        let top = MIN_COST + (special * MAX_COST_SPAN) as i32;
        let cost = random
            .next_int_between_inclusive(MIN_COST, top)
            .unwrap_or(MIN_COST);
        let e = &world_stations!(self).enchantments;
        let pool = e.tag("minecraft:on_mob_spawn_equipment");
        let picked = e.select(world_items!(self), random, item, cost, pool);
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            for (enchantment, level) in picked {
                gear::upgrade(m, slot, &enchantment, level);
            }
        }
    }
    /// A `SingleEnchantment` provider on the item in `slot`, when it is `item`.
    fn enchant_single(&mut self, id: u32, slot: usize, enchantment: &str, level: u32, item: &str) { let (id,slot,level,) = match crate::points_generated::P2767.get() { Some(__l) => { let mut __a = (id,slot,level,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (id,slot,level,) };
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id)
            && slot_item(m, slot).as_deref() == Some(item)
        {
            gear::upgrade(m, slot, enchantment, level);
        }
    }

    /// `Pillager` / `Vindicator.applyRaidBuffs`: Quick Charge on a fresh
    /// crossbow after waves 3 and 5, Sharpness on a fresh axe.
    pub(super) fn apply_raid_buffs(&mut self, id: u32, kind: RaiderType, wave: i32, odds: f32) { let (id,wave,odds,) = match crate::points_generated::P2766.get() { Some(__l) => { let mut __a = (id,wave,odds,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (id,wave,odds,) };
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        let enchant = m.random.next_float() <= odds;
        match kind {
            RaiderType::Pillager if enchant => {
                let level = if wave > raid::num_groups(2) {
                    2
                } else if wave > raid::num_groups(1) {
                    1
                } else {
                    0
                };
                if level > 0 {
                    horizon_client::mobs::finalize::set_item(
                        m,
                        horizon_client::mobs::finalize::MAINHAND,
                        "minecraft:crossbow",
                    );
                    gear::upgrade(m, MAIN, "minecraft:quick_charge", level);
                }
            }
            RaiderType::Vindicator => {
                horizon_client::mobs::finalize::set_item(
                    m,
                    horizon_client::mobs::finalize::MAINHAND,
                    "minecraft:iron_axe",
                );
                if enchant {
                    let level = if wave > raid::num_groups(2) { 2 } else { 1 };
                    gear::upgrade(m, MAIN, "minecraft:sharpness", level);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_vanilla_mobs_enchant_their_spawn_gear() {
        assert!(enchants_spawn_gear(Species::Skeleton));
        assert!(enchants_spawn_gear(Species::Pillager));
        assert!(!enchants_spawn_gear(Species::WitherSkeleton));
        assert!(!enchants_spawn_gear(Species::PiglinBrute));
        assert!(!enchants_spawn_gear(Species::Villager));
    }
}

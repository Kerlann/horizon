//! Living entities for native server mods, through a `LivingRef`
//! (docs/native-mods.md, "Entity and level handles", #7387): the equipment
//! of mobs, players and armor stands. Reads return `None`, writes false, for
//! a stale handle or on a client.
use super::Game;
use super::commands::{Target, items::Owner};
use horizon_client::first_person::Hand;
use horizon_client::items::ItemStack;
use horizon_client::mod_api::{EntityRef, EquipmentSlot, LivingRef};

/// A player's armour slots (`armor`, head first) by equipment slot.
fn armor_index(slot: EquipmentSlot) -> Option<usize> {
    match slot {
        EquipmentSlot::Head => Some(0),
        EquipmentSlot::Chest => Some(1),
        EquipmentSlot::Legs => Some(2),
        EquipmentSlot::Feet => Some(3),
        _ => None,
    }
}

/// A player's stack in `slot`, from its inventory, armour and off hand.
fn player_slot(
    inventory: &[Option<ItemStack>],
    selected: usize,
    armor: &[Option<ItemStack>; 4],
    offhand: &Option<ItemStack>,
    slot: EquipmentSlot,
) -> Option<ItemStack> {
    match slot {
        EquipmentSlot::MainHand => inventory.get(selected).cloned().flatten(),
        EquipmentSlot::OffHand => offhand.clone(),
        _ => armor[armor_index(slot)?].clone(),
    }
}

/// The `/item` slot name of an equipment slot.
fn slot_name(slot: EquipmentSlot) -> &'static str {
    match slot {
        EquipmentSlot::MainHand => "weapon.mainhand",
        EquipmentSlot::OffHand => "weapon.offhand",
        EquipmentSlot::Head => "armor.head",
        EquipmentSlot::Chest => "armor.chest",
        EquipmentSlot::Legs => "armor.legs",
        EquipmentSlot::Feet => "armor.feet",
        EquipmentSlot::Body => "armor.body",
        EquipmentSlot::Saddle => "saddle",
    }
}

fn available_living_target(target: Target, swapping: bool) -> Option<Target> {
    match target {
        Target::Guest(_) if swapping => None,
        t @ (Target::Player
        | Target::Guest(_)
        | Target::Mob(_)
        | Target::Stand(_)
        | Target::Dragon(_)) => Some(t),
        _ => None,
    }
}

fn supports_equipment_write(target: Target) -> bool {
    !matches!(target, Target::Mob(_))
}

fn equipment_stack(stack: Option<ItemStack>) -> Option<ItemStack> {
    stack.filter(|s| s.count > 0 && &*s.item != "minecraft:air")
}

impl Game<'_> {
    /// What a living handle names now: a player, a mob, an armor stand or the dragon.
    fn native_living_target(&self, living: LivingRef) -> Option<Target> {
        available_living_target(
            self.native_entity_target(living.entity())?,
            self.lan_acting_uuid.is_some(),
        )
    }
    /// The entity as a living entity (vanilla's `LivingEntity`: a mob, a
    /// player, an armor stand or the dragon), or None for another kind or a
    /// stale handle. API @API:living_refs@.
    pub(super) fn native_living(&self, entity: EntityRef) -> Option<LivingRef> {
        let living = LivingRef::new(entity);
        self.native_living_target(living).map(|_| living)
    }
    /// The stack in equipment slot `slot` (`LivingEntity.getItemBySlot`):
    /// None when it is empty, the entity has no such slot (a player has no
    /// body or saddle slot), or the handle is stale. API @API:living_refs@.
    pub(super) fn native_living_equipment(
        &self,
        living: LivingRef,
        slot: EquipmentSlot,
    ) -> Option<ItemStack> {
        match self.native_living_target(living)? {
            Target::Player => player_slot(
                &self.inventory,
                self.selected,
                &self.armor,
                &self.offhand,
                slot,
            ),
            Target::Guest(conn) => {
                let g = self.command_guest(conn)?;
                player_slot(&g.inventory, g.selected, &g.armor, &g.offhand, slot)
            }
            Target::Mob(id) => {
                self.mobs.iter().find(|m| m.id == id)?.equipment_stacks()[slot as usize].clone()
            }
            Target::Stand(id) => self
                .armor_stands
                .iter()
                .find(|a| a.id == id)?
                .equipment
                .get(slot as usize)?
                .clone(),
            _ => None,
        }
    }
    /// The stack in hand `hand` (`LivingEntity.getItemInHand`). API
    /// @API:living_refs@.
    pub(super) fn native_living_hand_item(
        &self,
        living: LivingRef,
        hand: Hand,
    ) -> Option<ItemStack> {
        let slot = match hand {
            Hand::Main => EquipmentSlot::MainHand,
            Hand::Off => EquipmentSlot::OffHand,
        };
        self.native_living_equipment(living, slot)
    }
    /// Puts `stack` (None: empties it) in equipment slot `slot`
    /// (`LivingEntity.setItemSlot`), as `/item replace entity` does. False
    /// when the entity has no such slot or is a mob. Mob equipment writes
    /// are not supported until full equipment stacks can be stored.
    /// API @API:living_refs@.
    pub(super) fn native_living_set_equipment(
        &mut self,
        living: LivingRef,
        slot: EquipmentSlot,
        stack: Option<ItemStack>,
    ) -> bool {
        let Some(t) = self.native_living_target(living) else {
            return false;
        };
        if !supports_equipment_write(t) {
            return false;
        }
        let stack = equipment_stack(stack);
        match self
            .slots_named(&Owner::Entity(t), slot_name(slot))
            .as_slice()
        {
            [s] => self.slot_set(s, stack),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displaced_guests_are_unavailable_during_a_swap() {
        assert_eq!(available_living_target(Target::Guest(2), true), None);
        assert_eq!(
            available_living_target(Target::Guest(2), false),
            Some(Target::Guest(2))
        );
        assert_eq!(
            available_living_target(Target::Player, true),
            Some(Target::Player)
        );
    }

    #[test]
    fn mob_equipment_writes_are_refused() {
        assert!(!supports_equipment_write(Target::Mob(1)));
        assert!(supports_equipment_write(Target::Player));
        assert!(supports_equipment_write(Target::Stand(2)));
    }

    #[test]
    fn air_and_zero_count_are_empty_on_write() {
        assert!(equipment_stack(Some(ItemStack::new("minecraft:air", 1))).is_none());
        assert!(equipment_stack(Some(ItemStack::new("minecraft:stone", 0))).is_none());
        assert!(equipment_stack(Some(ItemStack::new("minecraft:stone", 1))).is_some());
    }

    #[test]
    fn a_players_slots_come_from_its_inventory_armour_and_off_hand() {
        let stack = |id: &str| Some(ItemStack::new(id, 1));
        let inventory = vec![stack("minecraft:stone"), stack("minecraft:dirt")];
        let armor = [
            stack("minecraft:iron_helmet"),
            None,
            None,
            stack("minecraft:iron_boots"),
        ];
        let offhand = stack("minecraft:shield");
        let at = |slot| player_slot(&inventory, 1, &armor, &offhand, slot).map(|s| s.item);
        assert_eq!(
            at(EquipmentSlot::MainHand).as_deref(),
            Some("minecraft:dirt")
        );
        assert_eq!(
            at(EquipmentSlot::OffHand).as_deref(),
            Some("minecraft:shield")
        );
        assert_eq!(
            at(EquipmentSlot::Head).as_deref(),
            Some("minecraft:iron_helmet")
        );
        assert_eq!(
            at(EquipmentSlot::Feet).as_deref(),
            Some("minecraft:iron_boots")
        );
        assert_eq!(at(EquipmentSlot::Chest), None);
        assert_eq!(at(EquipmentSlot::Body), None);
    }

    #[test]
    fn every_equipment_slot_has_its_own_item_command_name() {
        let names = EquipmentSlot::ALL.map(slot_name);
        for (i, n) in names.iter().enumerate() {
            assert!(!names[..i].contains(n), "{n} named twice");
        }
    }
}

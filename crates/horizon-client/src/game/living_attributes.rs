//! The item modifiers of the live attribute maps of mobs, armor stands and the
//! dragon (`LivingEntity.collectEquipmentChanges`). The map and the NBT are in
//! `horizon_client::living_attributes`.
use super::Game;

impl Game<'_> {
    /// `LivingEntity.tick`'s `collectEquipmentChanges` for what the server
    /// simulates besides players: each slot whose item is not the one the map
    /// holds swaps the old item's attribute modifiers for the new one's. An
    /// entity that changed nothing costs a compare of its eight slots, and
    /// one that wears nothing and never wore anything is skipped outright.
    pub(super) fn tick_equipment_attributes(&mut self) { if let Some(__l) = crate::points_generated::P2551.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        if self.remote.is_some() || self.options.execution_role.is_client() {
            return;
        }
        let catalog = self.motion.attributes.clone();
        let items = world_items!(self);
        for m in self.mobs.iter_mut().filter(|m| !m.removed) {
            if m.extra.equipment.is_empty() && !m.extra.live.has_worn() {
                continue;
            }
            // The map is taken out so the seed can read the mob.
            let mut live = std::mem::take(&mut m.extra.live);
            let changed = live.collect_equipment_changes(
                &catalog,
                &m.kind.id,
                &m.extra.equipment,
                items,
                || {
                    horizon_client::entity_nbt::attribute_entries(m)
                        .into_iter()
                        .map(|(_, e)| e)
                        .collect()
                },
            );
            warn_failed(m.id, &m.kind.id, changed);
            m.extra.live = live;
        }
        for a in self.armor_stands.iter_mut() {
            if a.equipment.is_empty() && !a.live.has_worn() {
                continue;
            }
            let changed = a.live.collect_equipment_changes(
                &catalog,
                "minecraft:armor_stand",
                &a.equipment,
                items,
                Vec::new,
            );
            warn_failed(a.id, "minecraft:armor_stand", changed);
        }
        if let Some(d) = self.dragon.as_mut()
            && !(d.equipment.is_empty() && !d.live.has_worn())
        {
            let changed = d.live.collect_equipment_changes(
                &catalog,
                "minecraft:ender_dragon",
                &d.equipment,
                items,
                Vec::new,
            );
            warn_failed(d.id, "minecraft:ender_dragon", changed);
        }
    }
}

/// An item modifier the map refused (a bad modifier id) is logged, naming the
/// entity, instead of being lost.
fn warn_failed(id: u32, kind: &str, result: horizon_client::Result<()>) {
    if let Err(error) = result {
        log::warn!("equipment attributes of {kind} {id}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::native_entity_refs::tests::with_game;
    use horizon_client::{entities::ArmorStand, items::ItemStack, mobs::Mob};
    use horizon_core::geometry::Vec3;
    use horizon_items::equipment::{CHEST, HEAD, MAINHAND};

    fn stack(item: &str) -> Option<ItemStack> {
        Some(ItemStack::new(item, 1))
    }

    /// A zombie, an armor stand and the dragon, ids 12, 13 and 14.
    fn spawn(game: &mut Game<'_>) {
        let zombie = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        game.mobs.push(Mob::new(12, zombie, Vec3::ZERO, 0.0));
        game.armor_stands.push(ArmorStand::new(13, Vec3::ZERO, 0.0));
        let mut dragon = crate::game::dragon::Dragon::new(Vec3::ZERO, 0.0, 0);
        dragon.id = 14;
        game.dragon = Some(dragon);
    }

    fn read(game: &Game<'_>, id: u32, name: &str) -> (Option<f64>, Option<f64>) {
        let living = game.native_living(game.native_entity(id).unwrap()).unwrap();
        (
            living.attribute_value(game, name),
            living.attribute_base(game, name),
        )
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_zombie_reads_its_defaults() {
        with_game(|game| {
            spawn(game);
            assert_eq!(
                read(game, 12, "minecraft:max_health"),
                (Some(20.0), Some(20.0))
            );
            assert_eq!(
                read(game, 12, "minecraft:attack_damage"),
                (Some(3.0), Some(3.0))
            );
            // Not an attribute of an armor stand.
            assert_eq!(read(game, 13, "minecraft:attack_damage"), (None, None));
        });
    }

    /// A read of a mob that wears nothing builds its map once and keeps it,
    /// so the next reads allocate nothing.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_read_builds_the_map_once() {
        with_game(|game| {
            spawn(game);
            assert!(game.mobs[0].extra.live.built().is_none());
            read(game, 12, "minecraft:max_health");
            let first: *const _ = game.mobs[0].extra.live.built().unwrap();
            read(game, 12, "minecraft:attack_damage");
            read(game, 12, "minecraft:max_health");
            let again: *const _ = game.mobs[0].extra.live.built().unwrap();
            assert!(std::ptr::eq(first, again));
            // A changed base drops it; the next read builds it from the new base.
            game.mobs[0].extra.live.invalidate();
            assert!(game.mobs[0].extra.live.built().is_none());
            assert_eq!(
                read(game, 12, "minecraft:max_health"),
                (Some(20.0), Some(20.0))
            );
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_held_sword_raises_the_attack_damage_until_it_is_put_away() {
        with_game(|game| {
            spawn(game);
            let damage = "minecraft:attack_damage";
            game.mobs[0].set_equipment_stack(MAINHAND, stack("minecraft:diamond_sword"));
            game.tick_equipment_attributes();
            // +6 from the sword's `base_attack_damage` modifier; the base stays.
            assert_eq!(read(game, 12, damage), (Some(9.0), Some(3.0)));
            // The same item again changes nothing (no duplicate modifier).
            game.tick_equipment_attributes();
            assert_eq!(read(game, 12, damage), (Some(9.0), Some(3.0)));
            // Another sword takes the first one's modifier off before its own.
            game.mobs[0].set_equipment_stack(MAINHAND, stack("minecraft:netherite_sword"));
            game.tick_equipment_attributes();
            assert_eq!(read(game, 12, damage), (Some(10.0), Some(3.0)));
            game.mobs[0].set_equipment_stack(MAINHAND, None);
            game.tick_equipment_attributes();
            assert_eq!(read(game, 12, damage), (Some(3.0), Some(3.0)));
            // A sword on the head is in no slot its modifiers name.
            game.mobs[0].set_equipment_stack(HEAD, stack("minecraft:diamond_sword"));
            game.tick_equipment_attributes();
            assert_eq!(read(game, 12, damage), (Some(3.0), Some(3.0)));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_stand_and_the_dragon_wear_armor_modifiers() {
        with_game(|game| {
            spawn(game);
            let armor = "minecraft:armor";
            game.armor_stands[0]
                .equipment
                .set(HEAD, stack("minecraft:diamond_helmet"));
            game.dragon
                .as_mut()
                .unwrap()
                .equipment
                .set(CHEST, stack("minecraft:diamond_chestplate"));
            game.tick_equipment_attributes();
            assert_eq!(read(game, 13, armor), (Some(3.0), Some(0.0)));
            assert_eq!(read(game, 14, armor), (Some(8.0), Some(0.0)));
            game.armor_stands[0].equipment.set(HEAD, None);
            game.tick_equipment_attributes();
            assert_eq!(read(game, 13, armor), (Some(0.0), Some(0.0)));
            assert_eq!(read(game, 14, armor), (Some(8.0), Some(0.0)));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_stale_handle_reads_none() {
        with_game(|game| {
            spawn(game);
            let living = game.native_living(game.native_entity(12).unwrap()).unwrap();
            assert_eq!(
                living.attribute_value(game, "minecraft:max_health"),
                Some(20.0)
            );
            game.mobs.clear();
            assert_eq!(living.attribute_value(game, "minecraft:max_health"), None);
            assert_eq!(living.attribute_base(game, "minecraft:max_health"), None);
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_saved_base_and_modifier_are_read_and_saved_again() {
        use horizon_world::nbt::Tag;
        with_game(|game| {
            spawn(game);
            let saved = Tag::List(vec![Tag::compound([
                ("id", Tag::String("minecraft:max_health".into())),
                ("base", Tag::Double(40.0f64.to_bits())),
                (
                    "modifiers",
                    Tag::List(vec![Tag::compound([
                        ("id", Tag::String("test:bonus".into())),
                        ("amount", Tag::Double(0.5f64.to_bits())),
                        ("operation", Tag::String("add_multiplied_base".into())),
                    ])]),
                ),
            ])]);
            game.armor_stands[0].live.load(Some(&saved), |_| true);
            assert_eq!(
                read(game, 13, "minecraft:max_health"),
                (Some(60.0), Some(40.0))
            );
            let mut list = Vec::new();
            game.armor_stands[0].live.save_into(&mut list);
            assert_eq!(Tag::List(list), saved);
        });
    }
}

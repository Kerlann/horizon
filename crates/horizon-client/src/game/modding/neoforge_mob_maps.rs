//! NeoForge's built-in mob, villager and game event data maps
//! ([`horizon_client::neoforge_mob_maps`]): the world's data packs' and mods'
//! files merged over vanilla's tables on every server data load, as a mod's own
//! data maps are, and installed for the server's mobs and sculk sensors.
use super::Game;
use super::native_data_maps::merge_builtin;
pub(in crate::game) use horizon_client::neoforge_mob_maps::vibration_frequency;
use horizon_client::neoforge_mob_maps::{self, MAPS, NeoForgeMobMaps};
use std::collections::{HashMap, HashSet};

/// The maps of a world's tables' files, `tags` the tags of a registry folder;
/// a map no pack has a file for stays vanilla's.
fn load<'a>(
    overlay: &horizon_world::loot::PackOverlay<'_>,
    tags: impl Fn(&str) -> Option<&'a HashMap<String, HashSet<String>>>,
) -> NeoForgeMobMaps {
    let none = HashMap::new();
    let mut maps = NeoForgeMobMaps::default();
    for (id, dir) in MAPS {
        let base = || neoforge_mob_maps::vanilla_layer(id);
        let tags = tags(dir).unwrap_or(&none);
        if let Some(values) = merge_builtin(id, dir, base, overlay, tags) {
            maps.set(id, &values);
        }
    }
    maps
}

impl Game<'_> {
    /// Install the open world's tables' maps, replacing the last load's:
    /// vanilla's tables for a world without tables.
    pub(in crate::game) fn install_mob_maps(&self) {
        let maps = self
            .world_tables
            .as_ref()
            .map_or_else(NeoForgeMobMaps::default, |tables| {
                let overlay = horizon_world::loot::PackOverlay {
                    conditions: &tables.conditions,
                    files: &tables.files,
                };
                load(&overlay, |dir| match dir {
                    "item" => Some(&tables.items.item_tags),
                    dir => tables.pack_tags.get(dir),
                })
            });
        neoforge_mob_maps::install(maps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::mobs::Species;
    use serde_json::json;

    #[test]
    fn pack_files_change_the_maps_they_name() {
        let file = |value: serde_json::Value| vec![serde_json::to_vec(&value).unwrap()];
        let files = vec![
            (
                "data_maps/neoforge:entity_type/acceptable_villager_distances".to_owned(),
                file(json!({
                    "values": {"minecraft:creeper": {"distance": 8.0}},
                    "remove": ["minecraft:pillager"]
                })),
            ),
            (
                "data_maps/neoforge:item/villager_compostables".to_owned(),
                file(json!({"values": {"#test:seeds": {"can_villager_compost": true}}})),
            ),
        ];
        let overlay = horizon_world::loot::PackOverlay {
            files: &files,
            ..horizon_world::loot::PackOverlay::NONE
        };
        let item_tags = HashMap::from([(
            "test:seeds".to_owned(),
            HashSet::from(["test:rice".to_owned()]),
        )]);
        let maps = load(&overlay, |dir| (dir == "item").then_some(&item_tags));
        assert_eq!(maps.villager_distance(Species::Creeper), Some(8.0));
        assert_eq!(maps.villager_distance(Species::Pillager), None);
        assert_eq!(maps.villager_distance(Species::Zombie), Some(8.0));
        assert!(maps.villager_compostable("test:rice"));
        assert!(maps.villager_compostable("minecraft:wheat_seeds"));
        // Maps no file names keep vanilla's tables.
        assert_eq!(maps.vibration_frequency("minecraft:step"), 1);
        assert!(maps.parrot_sounds().is_none());
    }
}

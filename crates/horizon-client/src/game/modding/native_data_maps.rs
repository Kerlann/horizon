//! Data maps for native server mods (docs/modding/src/how-to/native-mods.md): values a data pack
//! attaches to the entries of a registry, under the types a mod registered
//! (`native_data_map_types`). Vanilla has no data maps; the contract is
//! NeoForge's `DataMapType`: files at
//! `data/<map namespace>/data_maps/<registry path>/<map path>.json` hold
//! `replace`, `values` (entry ids and `#tags` to values) and `remove`, and a
//! newer value overrides an older one for the same entry. The files are the
//! world's data packs' and what mods contribute through
//! `native_data_pack_files` (key `data_maps/<ns>:<registry path>/<map path>`),
//! merged each time the server's tables are built: a world's first open and
//! every `/reload`.
use super::Game;
#[cfg(test)]
use super::data_map_files::apply_file;
use super::data_map_files::apply_files;
use super::data_map_json::to_tag;
use horizon_core::nbt::Tag;
pub use mod_api::game::DataMapType;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The registries data maps can be attached to so far, with their folder
/// (the registry's path: `minecraft` registries have no namespace folder).
const REGISTRIES: [(&str, &str); 2] = [("minecraft:item", "item"), ("minecraft:block", "block")];

/// The registries `types` are for that have data maps, in `REGISTRIES` order.
pub(crate) fn registries_of(types: &[DataMapType]) -> Vec<&'static str> {
    let mut registries: Vec<_> = REGISTRIES
        .iter()
        .map(|(registry, _)| *registry)
        .filter(|registry| types.iter().any(|ty| ty.registry == *registry))
        .collect();
    for definition in horizon_client::neoforge_mob_maps::MAPS {
        if !registries.contains(&definition.registry) {
            registries.push(definition.registry);
        }
    }
    registries
}

/// The merged values of every registered type: registry, then type id, to the
/// value of each entry that has one.
pub(crate) enum MapValues {
    Json(BTreeMap<String, Value>),
    /// Builtin codecs own decoded values, including f32 infinities that JSON
    /// cannot represent. Encode once, not by reparsing on every host read.
    Decoded(BTreeMap<String, Tag>),
}
#[derive(Default)]
pub(crate) struct DataMaps {
    maps: HashMap<&'static str, HashMap<String, MapValues>>,
    pub(super) mob_maps: std::sync::Arc<horizon_client::neoforge_mob_maps::NeoForgeMobMaps>,
}

impl DataMaps {
    /// The types' values from `files` (the server's tables' pack files, in
    /// pack order, mods' below the packs'), tags resolved against the
    /// loaded `item_tags` and `block_tags`. A type with an unsupported
    /// registry or a bad id is left out with a warning; a file that isn't a
    /// data map is skipped alone.
    pub(crate) fn load(
        types: &[DataMapType],
        overlay: &horizon_world::loot::PackOverlay<'_>,
        item_tags: &HashMap<String, HashSet<String>>,
        block_tags: &HashMap<String, HashSet<String>>,
        builtin: super::neoforge_mob_maps::Loaded,
    ) -> Self {
        let mut maps: HashMap<&'static str, HashMap<String, MapValues>> = HashMap::new();
        for (definition, values) in builtin.values {
            maps.entry(definition.registry)
                .or_default()
                .insert(definition.id.to_owned(), values);
        }
        for ty in types {
            let Some((registry, dir)) = REGISTRIES.iter().find(|(r, _)| *r == ty.registry) else {
                log::warn!(
                    "Horizon: data map {} is for {}, which has no data maps yet: left out",
                    ty.id,
                    ty.registry
                );
                continue;
            };
            let valid = |s: &str| {
                !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-./".contains(c))
            };
            let ok = ty
                .id
                .split_once(':')
                .is_some_and(|(ns, path)| ns != "minecraft" && valid(ns) && valid(path));
            if !ok {
                log::warn!(
                    "Horizon: data map type {} is not a mod's id: left out",
                    ty.id
                );
                continue;
            }
            let registered = maps.entry(registry).or_default();
            if registered.contains_key(&ty.id) {
                log::warn!(
                    "Horizon: data map {} registered twice: the first is kept",
                    ty.id
                );
                continue;
            }
            let tags = if *dir == "item" {
                item_tags
            } else {
                block_tags
            };
            let mut map = BTreeMap::new();
            apply_files(&mut map, dir, &ty.id, overlay, tags);
            registered.insert(ty.id.clone(), MapValues::Json(map));
        }
        Self {
            maps,
            mob_maps: builtin.snapshot,
        }
    }

    /// JSON-backed entries for engine consumers of mod-defined block maps.
    /// Builtins have their own codecs and are read through `value` as decoded NBT.
    pub(crate) fn json_values(
        &self,
        registry: &str,
        type_id: &str,
    ) -> impl Iterator<Item = (&str, &Value)> {
        self.maps
            .get(registry)
            .and_then(|maps| maps.get(type_id))
            .and_then(|values| match values {
                MapValues::Json(values) => Some(values),
                MapValues::Decoded(_) => None,
            })
            .into_iter()
            .flatten()
            .map(|(entry, value)| (entry.as_str(), value))
    }

    /// `entry`'s NBT value: canonical codec records for builtins, JSON-derived
    /// NBT for mod-defined maps. None for missing entries; unregistered types
    /// are refused. Builtin infinity remains NBT Float, never JSON null.
    fn value(
        &self,
        registry: &str,
        type_id: &str,
        entry: &str,
    ) -> Result<Option<Tag>, mod_api::DataMapError> {
        let map = self
            .maps
            .get(registry)
            .and_then(|maps| maps.get(type_id))
            .ok_or(mod_api::DataMapError::UnknownType)?;
        match map {
            MapValues::Json(values) => values.get(entry).map(to_tag).transpose(),
            MapValues::Decoded(values) => Ok(values.get(entry).cloned()),
        }
    }
}

impl Game<'_> {
    /// Tell the mods the data maps of every registry with a type were merged
    /// (`DataMapsUpdatedEvent`), once the server's tables hold them.
    pub(in crate::game) fn announce_data_maps_updated(
        &mut self,
        cause: mod_api::DataMapsUpdateCause,
    ) {
        for registry in self.data_map_registries.clone() {
            self.native_data_maps_updated(registry, cause);
        }
    }
}

impl Game<'_> {
    /// The server's merged data maps, none unless this is a server whose
    /// tables were built from packs and mods' files.
    fn loaded_data_maps(&self) -> Result<&DataMaps, mod_api::DataMapError> {
        if !self.options.execution_role.is_logical_server() {
            return Err(mod_api::DataMapError::Unavailable);
        }
        self.world_tables
            .as_ref()
            .map(|tables| &tables.data_maps)
            .ok_or(mod_api::DataMapError::Unavailable)
    }
    /// Item `item`'s value in data map `map`, before the map decodes it: the merged JSON the
    /// server's data packs and mods' files gave it, as NBT. `Ok(None)` where it has none.
    pub(in crate::game) fn item_data_map_value(
        &self,
        item: mod_api::ItemKey,
        map: &str,
    ) -> Result<Option<Tag>, mod_api::DataMapError> {
        let maps = self.loaded_data_maps()?;
        let id = mod_api::__private::item_key_id(item).ok_or(mod_api::DataMapError::Unavailable)?;
        maps.value("minecraft:item", map, id)
    }
    /// Block `block`'s value in data map `map`, as `item_data_map_value` answers for an item.
    pub(in crate::game) fn block_data_map_value(
        &self,
        block: mod_api::BlockKey,
        map: &str,
    ) -> Result<Option<Tag>, mod_api::DataMapError> {
        let maps = self.loaded_data_maps()?;
        let id =
            mod_api::__private::block_key_id(block).ok_or(mod_api::DataMapError::Unavailable)?;
        maps.value("minecraft:block", map, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn load_types(
        types: &[DataMapType],
        overlay: &horizon_world::loot::PackOverlay<'_>,
        items: &HashMap<String, HashSet<String>>,
        blocks: &HashMap<String, HashSet<String>>,
    ) -> DataMaps {
        let builtin = super::super::neoforge_mob_maps::load(
            overlay,
            |directory| match directory {
                "item" => Some(items),
                "block" => Some(blocks),
                _ => None,
            },
            |_, _| true,
        )
        .unwrap();
        DataMaps::load(types, overlay, items, blocks, builtin)
    }

    #[test]
    fn data_map_read_refuses_lossy_payloads_instead_of_successful_substitutes() {
        for value in [
            json!({"nested": [null]}),
            json!(9223372036854775809_u64),
            json!(u64::MAX),
        ] {
            let maps = DataMaps {
                maps: HashMap::from([(
                    "minecraft:item",
                    HashMap::from([(
                        "test:map".to_owned(),
                        MapValues::Json(BTreeMap::from([("minecraft:stone".to_owned(), value)])),
                    )]),
                )]),
                ..Default::default()
            };
            assert_eq!(
                maps.value("minecraft:item", "test:map", "minecraft:stone"),
                Err(mod_api::DataMapError::Undecodable)
            );
            assert_eq!(
                maps.value("minecraft:item", "test:map", "minecraft:air"),
                Ok(None)
            );
        }
    }

    fn tags(list: &[(&str, &[&str])]) -> HashMap<String, HashSet<String>> {
        list.iter()
            .map(|(tag, members)| {
                (
                    (*tag).to_owned(),
                    members.iter().map(|m| (*m).to_owned()).collect(),
                )
            })
            .collect()
    }

    fn ty(id: &str, registry: &str) -> DataMapType {
        DataMapType {
            id: id.to_owned(),
            registry: registry.to_owned(),
        }
    }

    fn files(list: &[(&str, &[Value])]) -> Vec<(String, Vec<Vec<u8>>)> {
        list.iter()
            .map(|(key, versions)| {
                (
                    (*key).to_owned(),
                    versions
                        .iter()
                        .map(|v| v.to_string().into_bytes())
                        .collect(),
                )
            })
            .collect()
    }

    fn load(files: &[(String, Vec<Vec<u8>>)]) -> DataMaps {
        let items = tags(&[(
            "minecraft:logs",
            &["minecraft:oak_log", "minecraft:birch_log"],
        )]);
        load_types(
            &[ty("mymod:fuel", "minecraft:item")],
            &horizon_world::loot::PackOverlay {
                files,
                ..horizon_world::loot::PackOverlay::NONE
            },
            &items,
            &HashMap::new(),
        )
    }

    fn get<'a>(maps: &'a DataMaps, entry: &str) -> Option<&'a Value> {
        let MapValues::Json(values) = &maps.maps["minecraft:item"]["mymod:fuel"] else {
            panic!("mod-defined map uses JSON");
        };
        values.get(entry)
    }

    #[test]
    fn a_value_reads_as_nbt_and_a_map_no_mod_registered_is_refused() {
        let f = files(&[(
            "data_maps/mymod:item/fuel",
            &[json!({"values": {
                "minecraft:coal": 1600,
                "minecraft:stick": {"burn": 3_000_000_000_i64}
            }})],
        )]);
        let maps = load(&f);
        let value =
            |registry: &str, type_id: &str, entry: &str| maps.value(registry, type_id, entry);
        assert_eq!(
            value("minecraft:item", "mymod:fuel", "minecraft:coal"),
            Ok(Some(Tag::Int(1600)))
        );
        assert_eq!(
            value("minecraft:item", "mymod:fuel", "minecraft:stick"),
            Ok(Some(Tag::Compound(
                [("burn".into(), Tag::Long(3_000_000_000))]
                    .into_iter()
                    .collect()
            )))
        );
        assert_eq!(
            value("minecraft:item", "mymod:fuel", "minecraft:stone"),
            Ok(None)
        );
        assert_eq!(
            value("minecraft:block", "mymod:fuel", "minecraft:coal"),
            Err(mod_api::DataMapError::UnknownType)
        );
        assert_eq!(
            value("minecraft:item", "mymod:other", "minecraft:coal"),
            Err(mod_api::DataMapError::UnknownType)
        );
    }

    #[test]
    fn registries_with_a_type_are_listed_once_in_registry_order() {
        let types = [
            ty("mymod:b", "minecraft:block"),
            ty("mymod:a", "minecraft:item"),
            ty("mymod:c", "minecraft:block"),
            ty("mymod:d", "minecraft:biome"),
        ];
        assert_eq!(
            registries_of(&types),
            [
                "minecraft:item",
                "minecraft:block",
                "minecraft:entity_type",
                "minecraft:worldgen/biome",
                "minecraft:villager_profession",
                "minecraft:game_event"
            ]
        );
        let builtins = [
            "minecraft:entity_type",
            "minecraft:worldgen/biome",
            "minecraft:villager_profession",
            "minecraft:game_event",
            "minecraft:item",
        ];
        assert_eq!(registries_of(&[ty("mymod:d", "minecraft:biome")]), builtins);
        assert_eq!(registries_of(&[]), builtins);
    }

    #[test]
    fn builtin_values_are_available_without_mod_registration_and_replace_as_one_generation() {
        let f = files(&[(
            "data_maps/neoforge:item/villager_compostables",
            &[
                json!({"replace": true, "values": {"test:rice": {"can_villager_compost": true}, "test:grain": false}}),
            ],
        )]);
        let maps = load_types(
            &[],
            &horizon_world::loot::PackOverlay {
                files: &f,
                ..horizon_world::loot::PackOverlay::NONE
            },
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(
            maps.value(
                "minecraft:item",
                "neoforge:villager_compostables",
                "test:grain"
            ),
            Ok(Some(Tag::compound([(
                "can_villager_compost",
                Tag::Byte(0)
            )])))
        );
        assert_eq!(
            maps.value(
                "minecraft:item",
                "neoforge:villager_compostables",
                "minecraft:wheat_seeds"
            ),
            Ok(None)
        );
        assert!(maps.mob_maps.villager_compostable("test:rice"));
        assert!(!maps.mob_maps.villager_compostable("test:grain"));
        assert!(!maps.mob_maps.villager_compostable("minecraft:wheat_seeds"));
        let next = load_types(
            &[],
            &horizon_world::loot::PackOverlay::NONE,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(
            next.value(
                "minecraft:item",
                "neoforge:villager_compostables",
                "minecraft:wheat_seeds"
            )
            .unwrap()
            .is_some()
        );
        assert!(next.mob_maps.villager_compostable("minecraft:wheat_seeds"));
        assert!(!next.mob_maps.villager_compostable("test:rice"));
        assert!(maps.mob_maps.villager_compostable("test:rice"));
    }

    #[test]
    fn values_attach_to_entries_and_to_every_member_of_a_tag() {
        let f = files(&[(
            "data_maps/mymod:item/fuel",
            &[json!({"values": {
                "minecraft:coal": {"burn": 1600},
                "stick": 100,
                "#minecraft:logs": {"burn": 300}
            }})],
        )]);
        let maps = load(&f);
        assert_eq!(get(&maps, "minecraft:coal"), Some(&json!({"burn": 1600})));
        // An id without a namespace is a `minecraft` one.
        assert_eq!(get(&maps, "minecraft:stick"), Some(&json!(100)));
        assert_eq!(get(&maps, "minecraft:oak_log"), Some(&json!({"burn": 300})));
        assert_eq!(
            get(&maps, "minecraft:birch_log"),
            Some(&json!({"burn": 300}))
        );
        assert_eq!(get(&maps, "minecraft:stone"), None);
    }

    #[test]
    fn a_newer_file_overrides_and_replace_clears_what_came_before() {
        let f = files(&[(
            "data_maps/mymod:item/fuel",
            &[
                json!({"values": {"minecraft:coal": 1, "minecraft:stick": 2}}),
                json!({"values": {"minecraft:coal": 10}}),
            ],
        )]);
        let maps = load(&f);
        assert_eq!(get(&maps, "minecraft:coal"), Some(&json!(10)));
        assert_eq!(get(&maps, "minecraft:stick"), Some(&json!(2)));
        let f = files(&[(
            "data_maps/mymod:item/fuel",
            &[
                json!({"values": {"minecraft:coal": 1, "minecraft:stick": 2}}),
                json!({"replace": true, "values": {"minecraft:coal": 10}}),
            ],
        )]);
        let maps = load(&f);
        assert_eq!(get(&maps, "minecraft:coal"), Some(&json!(10)));
        assert_eq!(get(&maps, "minecraft:stick"), None);
    }

    #[test]
    fn remove_takes_entries_and_tags_away_after_the_files_own_values() {
        let f = files(&[(
            "data_maps/mymod:item/fuel",
            &[
                json!({"values": {"#minecraft:logs": 5, "minecraft:coal": 1}}),
                // The file's own value for coal is attached, then removed.
                json!({"values": {"minecraft:coal": 9}, "remove": ["minecraft:coal", "#minecraft:logs"]}),
            ],
        )]);
        let maps = load(&f);
        assert_eq!(get(&maps, "minecraft:coal"), None);
        assert_eq!(get(&maps, "minecraft:oak_log"), None);
    }

    #[test]
    fn the_wrapper_form_gives_its_value_and_a_plain_object_is_a_value() {
        let f = files(&[(
            "data_maps/mymod:item/fuel",
            &[json!({"values": {
                "minecraft:coal": {"value": {"burn": 3}, "replace": true},
                "minecraft:stick": {"value": 1, "other": 2}
            }})],
        )]);
        let maps = load(&f);
        assert_eq!(get(&maps, "minecraft:coal"), Some(&json!({"burn": 3})));
        // More than `value` and `replace`: the object is the data.
        assert_eq!(
            get(&maps, "minecraft:stick"),
            Some(&json!({"value": 1, "other": 2}))
        );
    }

    #[test]
    fn a_bad_file_is_skipped_alone_and_bad_types_are_left_out() {
        let mut f = files(&[(
            "data_maps/mymod:item/fuel",
            &[json!({"values": {"minecraft:coal": 1}})],
        )]);
        f[0].1.insert(0, b"{nope".to_vec());
        f[0].1
            .push(json!({"nothing": true}).to_string().into_bytes());
        let maps = load(&f);
        assert_eq!(get(&maps, "minecraft:coal"), Some(&json!(1)));
        let maps = load_types(
            &[
                ty("minecraft:fuel", "minecraft:item"),
                ty("mymod:x", "minecraft:biome"),
                ty("nonamespace", "minecraft:item"),
                ty("mymod:a", "minecraft:block"),
                ty("mymod:a", "minecraft:block"),
            ],
            &horizon_world::loot::PackOverlay::NONE,
            &HashMap::new(),
            &HashMap::new(),
        );
        // The one good mod type is registered once, alongside the builtins.
        assert_eq!(maps.maps.len(), 6);
        let blocks = &maps.maps["minecraft:block"];
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks["mymod:a"], MapValues::Json(values) if values.is_empty()));
    }

    #[test]
    fn conditional_layers_are_skipped_before_replace_and_remove() {
        let f = files(&[(
            "data_maps/mymod:item/fuel",
            &[
                json!({"values":{"minecraft:coal":1}}),
                json!({"neoforge:conditions":[{"type":"unsupported"}],"replace":true}),
                json!({"neoforge:conditions":[{"type":"neoforge:mod_loaded","modid":"provider"}],"remove":["minecraft:coal"],"values":{"minecraft:stick":2}}),
            ],
        )]);
        let off = load(&f);
        assert_eq!(get(&off, "minecraft:coal"), Some(&json!(1)));
        assert!(get(&off, "minecraft:stick").is_none());
        let enabled = horizon_world::conditions::LoadConditions::new(["provider"]);
        let on = load_types(
            &[ty("mymod:fuel", "minecraft:item")],
            &horizon_world::loot::PackOverlay {
                files: &f,
                conditions: &enabled,
            },
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(get(&on, "minecraft:coal").is_none());
        assert_eq!(get(&on, "minecraft:stick"), Some(&json!(2)));
    }

    #[test]
    fn block_maps_read_the_block_tags() {
        let blocks = tags(&[("minecraft:logs", &["minecraft:oak_log"])]);
        let f = files(&[(
            "data_maps/mymod:block/hardness",
            &[json!({"values": {"#minecraft:logs": 2}})],
        )]);
        let maps = load_types(
            &[ty("mymod:hardness", "minecraft:block")],
            &horizon_world::loot::PackOverlay {
                files: &f,
                ..horizon_world::loot::PackOverlay::NONE
            },
            &HashMap::new(),
            &blocks,
        );
        let MapValues::Json(map) = &maps.maps["minecraft:block"]["mymod:hardness"] else {
            panic!("block map uses JSON");
        };
        assert_eq!(map.get("minecraft:oak_log"), Some(&json!(2)));
    }
}

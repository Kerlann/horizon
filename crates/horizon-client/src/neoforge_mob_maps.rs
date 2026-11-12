//! NeoForge's built-in mob, villager and game event data maps
//! (`NeoForgeDataMaps`): what a world's data packs make of vanilla's tables of
//! villager scare distances, parrot imitations, biome villager types, hero
//! gifts, vibration frequencies and the items farmers compost. NeoForge ships
//! vanilla's tables as these maps' data: [`vanilla_layer`] is that lowest
//! layer, which the server's data load merges the packs' files over. A map no
//! pack has a file for stays none, and its readers keep vanilla's table.
//!
//! Each server data load (a world's open, `/reload`) installs one set
//! ([`install`]), read by the server's mobs and sculk sensors ([`current`]).
use crate::mobs::{Species, parrot, render::VILLAGER_TYPES, species};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, LazyLock, RwLock};

/// The maps, by id, with the folder of their registry
/// (`data/<ns>/data_maps/<folder>/<path>.json`).
pub const MAPS: [(&str, &str); 6] = [
    ("neoforge:acceptable_villager_distances", "entity_type"),
    ("neoforge:parrot_imitations", "entity_type"),
    ("neoforge:villager_types", "worldgen/biome"),
    ("neoforge:raid_hero_gifts", "villager_profession"),
    ("neoforge:vibration_frequencies", "game_event"),
    ("neoforge:villager_compostables", "item"),
];

/// The professions with a hero gift (`GiveGiftToHero.GIFTS`).
const GIFT_PROFESSIONS: [&str; 13] = [
    "armorer",
    "butcher",
    "cartographer",
    "cleric",
    "farmer",
    "fisherman",
    "fletcher",
    "leatherworker",
    "librarian",
    "mason",
    "shepherd",
    "toolsmith",
    "weaponsmith",
];

/// `WorkAtComposter.COMPOSTABLE_ITEMS`.
const COMPOSTABLES: [&str; 2] = ["minecraft:wheat_seeds", "minecraft:beetroot_seeds"];

/// `SoundEvents.EMPTY`'s id: the happy ghast's silent imitation.
const EMPTY_SOUND: &str = "minecraft:intentionally_empty";

/// `VillagerHostilesSensor.ACCEPTABLE_DISTANCE_FROM_HOSTILES`.
pub fn vanilla_villager_distance(s: Species) -> Option<f32> {
    Some(match s {
        Species::Drowned
        | Species::Husk
        | Species::Vex
        | Species::Zombie
        | Species::ZombieVillager => 8.0,
        Species::Evoker | Species::Illusioner | Species::Ravager => 12.0,
        Species::Pillager => 15.0,
        Species::Vindicator | Species::Zoglin => 10.0,
        _ => return None,
    })
}

/// The maps a server data load gave; a none map keeps vanilla's table.
#[derive(Debug, Default)]
pub struct NeoForgeMobMaps {
    /// `neoforge:acceptable_villager_distances`, by `Species as usize`.
    villager_distances: Option<Vec<Option<f32>>>,
    /// `neoforge:parrot_imitations`.
    parrot_imitations: Option<ParrotImitations>,
    /// `neoforge:villager_types`: biome id to index into `VILLAGER_TYPES`.
    villager_types: Option<HashMap<String, u8>>,
    /// `neoforge:raid_hero_gifts`: profession id to loot table.
    hero_gifts: Option<HashMap<String, String>>,
    /// `neoforge:vibration_frequencies`: game event id to frequency.
    vibration_frequencies: Option<HashMap<String, i32>>,
    /// `neoforge:villager_compostables`: the items farmers compost.
    villager_compostables: Option<HashSet<String>>,
}

#[derive(Debug, Default)]
struct ParrotImitations {
    /// By `Species as usize`: the sound (`""` silent).
    by_species: Vec<Option<&'static str>>,
    /// Every entry's sound, in id order: what `getAmbient`'s rare draw picks among.
    sounds: Vec<&'static str>,
}

/// A value's `name` field, or the bare value NeoForge's codecs also take.
fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.get(name).unwrap_or(value)
}

/// `minecraft:` for an id written without a namespace.
fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// A sound event id as the mobs' sounds name it (vanilla's without `minecraft:`;
/// [`EMPTY_SOUND`] silent), interned.
fn mob_sound(id: &str) -> &'static str {
    if id == EMPTY_SOUND {
        return "";
    }
    crate::mobs::entity_sounds::intern_full_id(id.strip_prefix("minecraft:").unwrap_or(id))
}

/// Each vanilla species' value in an entity type map, by `Species as usize`.
fn by_species<T: Copy>(
    values: &BTreeMap<String, Value>,
    decode: impl Fn(&Value) -> Option<T>,
) -> Vec<Option<T>> {
    let defs = species::ALL;
    let len = defs
        .iter()
        .map(|d| d.species as usize + 1)
        .max()
        .unwrap_or(0);
    let mut table = vec![None; len];
    for d in defs {
        table[d.species as usize] = values.get(species::vanilla_id(d.species)).and_then(&decode);
    }
    table
}

/// Every vanilla species with its entity type id.
fn vanilla_species() -> impl Iterator<Item = (Species, String)> {
    species::ALL
        .iter()
        .map(|d| (d.species, species::vanilla_id(d.species).to_owned()))
}

/// Vanilla's table as map `id`'s values: the layer the packs' files merge over.
pub fn vanilla_layer(id: &str) -> BTreeMap<String, Value> {
    match id {
        "neoforge:acceptable_villager_distances" => vanilla_species()
            .filter_map(|(s, id)| Some((id, json!({ "distance": vanilla_villager_distance(s)? }))))
            .collect(),
        "neoforge:parrot_imitations" => vanilla_species()
            .filter_map(|(s, id)| {
                let sound = parrot::imitation(s)?;
                let sound = if sound.is_empty() {
                    EMPTY_SOUND.to_owned()
                } else {
                    full_id(sound)
                };
                Some((id, json!({ "sound": sound })))
            })
            .collect(),
        "neoforge:villager_types" => crate::mobs::finalize::BIOME_VILLAGER_TYPES
            .iter()
            .map(|(biome, t)| {
                let kind = VILLAGER_TYPES[*t as usize];
                (full_id(biome), json!({ "villager_type": full_id(kind) }))
            })
            .collect(),
        "neoforge:raid_hero_gifts" => GIFT_PROFESSIONS
            .iter()
            .map(|p| {
                let table = format!("minecraft:gameplay/hero_of_the_village/{p}_gift");
                (full_id(p), json!({ "loot_table": table }))
            })
            .collect(),
        "neoforge:vibration_frequencies" => crate::sculk::EVENTS
            .iter()
            .filter(|(_, f)| *f > 0)
            .map(|(event, f)| ((*event).to_owned(), *f))
            .chain(
                crate::sculk::RESONANCE
                    .iter()
                    .enumerate()
                    .map(|(i, event)| ((*event).to_owned(), i as i32 + 1)),
            )
            .map(|(event, f)| (event, json!({ "frequency": f })))
            .collect(),
        "neoforge:villager_compostables" => COMPOSTABLES
            .iter()
            .map(|item| ((*item).to_owned(), json!({ "can_villager_compost": true })))
            .collect(),
        _ => BTreeMap::new(),
    }
}

/// The entries of `values` `decode` reads, a bad one left out with a warning.
fn decoded<T>(
    map: &str,
    values: &BTreeMap<String, Value>,
    decode: impl Fn(&Value) -> Option<T>,
) -> impl Iterator<Item = (String, T)> {
    values.iter().filter_map(move |(entry, value)| {
        let decoded = decode(value);
        if decoded.is_none() {
            log::warn!("Horizon: {map} value of {entry} is not one, left out: {value}");
        }
        Some((entry.clone(), decoded?))
    })
}

impl NeoForgeMobMaps {
    /// Map `id`'s merged values (from [`MAPS`]); an unknown id is ignored.
    pub fn set(&mut self, id: &str, values: &BTreeMap<String, Value>) {
        match id {
            "neoforge:acceptable_villager_distances" => {
                self.villager_distances = Some(by_species(values, |v| {
                    field(v, "distance").as_f64().map(|d| d as f32)
                }));
            }
            "neoforge:parrot_imitations" => {
                let sound = |v: &Value| field(v, "sound").as_str().map(mob_sound);
                self.parrot_imitations = Some(ParrotImitations {
                    by_species: by_species(values, sound),
                    sounds: decoded(id, values, sound).map(|(_, s)| s).collect(),
                });
            }
            "neoforge:villager_types" => {
                let kind = |v: &Value| {
                    let name = field(v, "villager_type").as_str()?;
                    let name = name.strip_prefix("minecraft:").unwrap_or(name);
                    // Horizon's villagers know vanilla's seven types.
                    VILLAGER_TYPES
                        .iter()
                        .position(|t| *t == name)
                        .map(|i| i as u8)
                };
                self.villager_types = Some(decoded(id, values, kind).collect());
            }
            "neoforge:raid_hero_gifts" => {
                let table = |v: &Value| field(v, "loot_table").as_str().map(full_id);
                self.hero_gifts = Some(decoded(id, values, table).collect());
            }
            "neoforge:vibration_frequencies" => {
                let frequency = |v: &Value| {
                    let f = field(v, "frequency").as_i64()?;
                    (1..=15).contains(&f).then_some(f as i32)
                };
                self.vibration_frequencies = Some(decoded(id, values, frequency).collect());
            }
            "neoforge:villager_compostables" => {
                let compost = |v: &Value| field(v, "can_villager_compost").as_bool();
                self.villager_compostables = Some(
                    decoded(id, values, compost)
                        .filter_map(|(item, yes)| yes.then_some(item))
                        .collect(),
                );
            }
            _ => {}
        }
    }

    /// How close a hostile of species `s` may come before a villager panics
    /// (`VillagerHostilesSensor.isClose`); none for a species that isn't one.
    pub fn villager_distance(&self, s: Species) -> Option<f32> {
        match &self.villager_distances {
            Some(table) => table.get(s as usize).copied().flatten(),
            None => vanilla_villager_distance(s),
        }
    }

    /// The sound a parrot imitates species `s` with (`""` silent); none for
    /// a species it doesn't imitate.
    pub fn parrot_imitation(&self, s: Species) -> Option<&'static str> {
        match &self.parrot_imitations {
            Some(p) => p.by_species.get(s as usize).copied().flatten(),
            None => parrot::imitation(s),
        }
    }

    /// Every imitation a parrot's rare ambient draw picks among, in id order;
    /// none for vanilla's table.
    pub fn parrot_sounds(&self) -> Option<&[&'static str]> {
        self.parrot_imitations.as_ref().map(|p| p.sounds.as_slice())
    }

    /// `VillagerType.byBiome`: the index into `VILLAGER_TYPES` of a biome's
    /// villagers, plains where the map names none.
    pub fn villager_type(&self, biome: &str) -> u8 {
        match &self.villager_types {
            Some(types) => types.get(&full_id(biome)).copied().unwrap_or(0),
            None => crate::mobs::finalize::villager_type(biome),
        }
    }

    /// The hero gift loot table of profession `id` (`GiveGiftToHero`), the
    /// unemployed gift where the map names none; none for vanilla's table.
    pub fn hero_gift(&self, profession: &str) -> Option<String> {
        let gifts = self.hero_gifts.as_ref()?;
        Some(
            gifts.get(&full_id(profession)).cloned().unwrap_or_else(|| {
                "minecraft:gameplay/hero_of_the_village/unemployed_gift".to_owned()
            }),
        )
    }

    /// `VibrationSystem.getGameEventFrequency`: 0 for an event that makes none.
    pub fn vibration_frequency(&self, event: &str) -> i32 {
        match &self.vibration_frequencies {
            Some(frequencies) => frequencies.get(event).copied().unwrap_or(0),
            None => crate::sculk::frequency(event),
        }
    }

    /// Whether a farmer composts `item` (`WorkAtComposter.compostItems`).
    pub fn villager_compostable(&self, item: &str) -> bool {
        match &self.villager_compostables {
            Some(items) => items.contains(item),
            None => COMPOSTABLES.contains(&item),
        }
    }
}

static CURRENT: LazyLock<RwLock<Arc<NeoForgeMobMaps>>> = LazyLock::new(Default::default);

/// The maps the last server data load installed: vanilla's tables before one.
pub fn current() -> Arc<NeoForgeMobMaps> {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Replace the installed maps with a server data load's.
pub fn install(maps: NeoForgeMobMaps) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(maps);
}

/// [`NeoForgeMobMaps::vibration_frequency`] of the installed maps.
pub fn vibration_frequency(event: &str) -> i32 {
    current().vibration_frequency(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vanilla's layer, with `edit` applied, as map `id`.
    fn edited(id: &str, edit: impl FnOnce(&mut BTreeMap<String, Value>)) -> NeoForgeMobMaps {
        let mut values = vanilla_layer(id);
        edit(&mut values);
        let mut maps = NeoForgeMobMaps::default();
        maps.set(id, &values);
        maps
    }

    #[test]
    fn the_vanilla_layer_reads_back_as_vanilla_tables() {
        let mut maps = NeoForgeMobMaps::default();
        for (id, _) in MAPS {
            maps.set(id, &vanilla_layer(id));
        }
        let vanilla = NeoForgeMobMaps::default();
        for d in species::ALL {
            let s = d.species;
            assert_eq!(maps.villager_distance(s), vanilla.villager_distance(s));
            assert_eq!(maps.parrot_imitation(s), vanilla.parrot_imitation(s));
        }
        for biome in ["minecraft:badlands", "minecraft:grove", "minecraft:plains"] {
            assert_eq!(maps.villager_type(biome), vanilla.villager_type(biome));
        }
        assert_eq!(
            maps.hero_gift("minecraft:cleric").as_deref(),
            Some("minecraft:gameplay/hero_of_the_village/cleric_gift")
        );
        assert_eq!(
            maps.hero_gift("minecraft:nitwit").as_deref(),
            Some("minecraft:gameplay/hero_of_the_village/unemployed_gift")
        );
        for event in [
            "minecraft:step",
            "minecraft:explode",
            "minecraft:resonate_7",
            "minecraft:shriek",
        ] {
            assert_eq!(
                maps.vibration_frequency(event),
                vanilla.vibration_frequency(event)
            );
        }
        assert!(maps.villager_compostable("minecraft:wheat_seeds"));
        assert!(!maps.villager_compostable("minecraft:wheat"));
    }

    #[test]
    fn pack_values_override_add_and_remove_entries() {
        let maps = edited("neoforge:acceptable_villager_distances", |v| {
            v.insert("minecraft:creeper".into(), json!({ "distance": 8.0 }));
            v.remove("minecraft:pillager");
        });
        assert_eq!(maps.villager_distance(Species::Creeper), Some(8.0));
        assert_eq!(maps.villager_distance(Species::Pillager), None);
        assert_eq!(maps.villager_distance(Species::Zombie), Some(8.0));

        let maps = edited("neoforge:parrot_imitations", |v| {
            v.insert(
                "minecraft:cow".into(),
                json!("minecraft:entity.cow.ambient"),
            );
            v.insert("minecraft:blaze".into(), json!({ "sound": EMPTY_SOUND }));
        });
        assert_eq!(
            maps.parrot_imitation(Species::Cow),
            Some("entity.cow.ambient")
        );
        assert_eq!(maps.parrot_imitation(Species::Blaze), Some(""));
        assert!(
            maps.parrot_sounds()
                .unwrap()
                .contains(&"entity.cow.ambient")
        );

        let maps = edited("neoforge:villager_types", |v| {
            v.insert(
                "minecraft:plains".into(),
                json!({ "villager_type": "minecraft:snow" }),
            );
            v.insert("test:moon".into(), json!({ "villager_type": "test:moon" }));
        });
        assert_eq!(maps.villager_type("minecraft:plains"), 4);
        assert_eq!(maps.villager_type("test:moon"), 0);

        let maps = edited("neoforge:vibration_frequencies", |v| {
            v.insert("minecraft:step".into(), json!({ "frequency": 9 }));
            v.insert("test:boom".into(), json!(16));
        });
        assert_eq!(maps.vibration_frequency("minecraft:step"), 9);
        assert_eq!(maps.vibration_frequency("test:boom"), 0);

        let maps = edited("neoforge:villager_compostables", |v| {
            v.insert(
                "minecraft:wheat".into(),
                json!({ "can_villager_compost": true }),
            );
            v.insert("minecraft:wheat_seeds".into(), json!(false));
        });
        assert!(maps.villager_compostable("minecraft:wheat"));
        assert!(!maps.villager_compostable("minecraft:wheat_seeds"));
    }
}

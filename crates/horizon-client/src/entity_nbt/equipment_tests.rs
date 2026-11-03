//! The mob `equipment` compound is saved as `ar/main` saved it. The `.nbt`
//! files in `tests/data/mob_equipment` were written by that codec (set
//! `HORIZON_WRITE_FIXTURES=1` and run the ignored test to regenerate them);
//! each must load and save back to the same bytes.
use super::*;
use crate::mobs::Species;
use horizon_world::nbt::{Limits, NamedTag, parse_snbt, read_named, write_named};

const UUID: [i32; 4] = [1, 2, 3, 4];

/// (file, mob id, species, the SNBT a save of that mob holds).
const CASES: [(&str, &str, Species, &str); 3] = [
    (
        "zombie_armor",
        "minecraft:zombie",
        Species::Zombie,
        r#"{id:"minecraft:zombie",Pos:[0.5d,64.0d,0.5d],equipment:{mainhand:{id:"minecraft:iron_sword",count:1},head:{id:"minecraft:iron_helmet",count:1,components:{"minecraft:enchantments":{"minecraft:protection":2}}},chest:{id:"minecraft:leather_chestplate",count:1,components:{"minecraft:dyed_color":16711680}},legs:{id:"minecraft:iron_leggings",count:1,components:{"minecraft:damage":7}},feet:{id:"minecraft:golden_boots",count:1}}}"#,
    ),
    (
        "horse_body_saddle",
        "minecraft:horse",
        Species::Horse,
        r#"{id:"minecraft:horse",Pos:[0.5d,64.0d,0.5d],Tame:1b,Variant:259,Temper:20,equipment:{body:{id:"minecraft:iron_horse_armor",count:1,components:{"minecraft:damage":5}},saddle:{id:"minecraft:saddle",count:1}}}"#,
    ),
    (
        "llama_carpet",
        "minecraft:llama",
        Species::Llama,
        r#"{id:"minecraft:llama",Pos:[0.5d,64.0d,0.5d],Tame:1b,Strength:3,Variant:1,equipment:{body:{id:"minecraft:red_carpet",count:1}}}"#,
    ),
];

fn types(id: &str, species: Species) -> HashMap<String, Arc<MobType>> {
    let mut kind = crate::mobs::sensor_scan_tests::kind();
    let ty = Arc::make_mut(&mut kind);
    ty.id = id.into();
    ty.species = species;
    [(id.to_string(), kind)].into()
}

/// Loads `tag` as a mob and saves it back over itself, as a chunk save does.
fn round_trip(tag: &Tag, id: &str, species: Species) -> Tag {
    let m = mob_from_nbt(tag, &types(id, species), 1, None).unwrap();
    mob_to_nbt(&m, UUID, Some(tag), None)
}

fn bytes(tag: &Tag) -> Vec<u8> {
    let root = NamedTag {
        name: "".into(),
        tag: tag.clone(),
    };
    write_named(&root, Limits::default()).unwrap()
}

fn path(name: &str) -> std::path::PathBuf {
    // This module is compiled into horizon-client-lib, beside horizon-client.
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../horizon-client/tests/data/mob_equipment/{name}.nbt"
    ))
}

#[test]
#[ignore = "writes the fixtures; run with HORIZON_WRITE_FIXTURES=1 on the codec to record"]
fn write_fixtures() {
    assert!(std::env::var_os("HORIZON_WRITE_FIXTURES").is_some());
    for (name, id, species, snbt) in CASES {
        let input = parse_snbt(snbt).unwrap().0;
        std::fs::write(path(name), bytes(&round_trip(&input, id, species))).unwrap();
    }
}

#[test]
fn saved_mob_equipment_loads_and_saves_byte_identically() {
    for (name, id, species, _) in CASES {
        let file = std::fs::read(path(name)).unwrap();
        let fixture = read_named(&file, Limits::default()).unwrap().0.tag;
        assert!(fixture.get("equipment").is_some(), "{name} has equipment");
        let saved = round_trip(&fixture, id, species);
        assert_eq!(bytes(&saved), file, "{name} saves back unchanged");
    }
}

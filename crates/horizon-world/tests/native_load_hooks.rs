use horizon_world::{
    item_defaults, loot,
    native_load_hooks::{self, NativeLoadHooks},
};
use serde_json::{Map, Value, json};

#[test]
fn load_hooks_are_noops_before_install_and_use_first_callbacks_after() {
    let mut table = json!([]);
    let mut components = Map::new();
    loot::native_loot_table_load("table", &mut table);
    item_defaults::native_modify_default_components("item", &mut components);
    assert_eq!(table, json!([]));
    assert!(components.is_empty());

    let hooks = || NativeLoadHooks {
        loot_table_load: |id, table| table.as_array_mut().unwrap().push(id.into()),
        modify_default_components: |item, components| {
            components.insert("item".into(), Value::String(item.into()));
        },
        loot_function: |_, _, _, stack| stack.count += 1,
        loot_condition: |kind, _, _, holds| *holds = kind == "mymod:held",
    };
    assert!(native_load_hooks::install(hooks()).is_ok());
    assert!(
        native_load_hooks::install(NativeLoadHooks {
            loot_table_load: |_, table| *table = json!("second"),
            modify_default_components: |_, components| components.clear(),
            loot_function: |_, _, _, stack| stack.count = 0,
            loot_condition: |_, _, _, holds| *holds = false,
        })
        .is_err()
    );
    loot::native_loot_table_load("table", &mut table);
    item_defaults::native_modify_default_components("item", &mut components);
    assert_eq!(table, json!(["table"]));
    assert_eq!(components["item"], json!("item"));

    // Client/shared loads keep raw tables; each server load gets exactly one
    // callback per table, even when the raw map is already cached.
    let root = std::env::temp_dir().join(format!("horizon-load-hooks-{}", std::process::id()));
    let dir = root.join("data/minecraft/loot_table");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(root.join("items.json"), "{}").unwrap();
    std::fs::write(dir.join("test.json"), "[]").unwrap();
    let raw = loot::shared_tables(&root, &loot::PackOverlay::NONE).unwrap();
    assert_eq!(raw["minecraft:test"], json!([]));
    for _ in 0..2 {
        let mut server = loot::LootData::load(&root, &loot::PackOverlay::NONE).unwrap();
        server.apply_native_table_loads();
        assert_eq!(server.tables()["minecraft:test"], json!(["minecraft:test"]));
        assert_eq!(raw["minecraft:test"], json!([]));
    }
    std::fs::remove_dir_all(&root).unwrap();

    let ctx = loot::NativeLootContext {
        entity: None,
        damage: None,
        player: None,
        tool: None,
        block: None,
        luck: 0.0,
        biome: None,
        seed: 0,
        place: None,
    };
    for (kind, expected) in [("mymod:held", true), ("mymod:other", false)] {
        let mut holds = !expected;
        loot::native_loot_condition(kind, &json!({}), &ctx, &mut holds);
        assert_eq!(holds, expected, "{kind}");
    }
}

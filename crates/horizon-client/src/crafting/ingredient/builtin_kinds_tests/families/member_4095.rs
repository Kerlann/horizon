use super::*;

#[test]
fn glider_component_recipe_normalizes_maps_and_matches_borrowed_values() {
    let mut items = Items::empty();
    items.component_defaults = std::sync::Arc::new(std::collections::HashMap::from([(
        "minecraft:elytra".into(),
        crate::json_nbt::json_to_compound(
            serde_json::json!({"minecraft:glider":{"ignored_default":false}})
                .as_object()
                .unwrap(),
        )
        .unwrap(),
    )]));
    let source = r#"{"type":"neoforge:components","items":"minecraft:elytra","components":{"minecraft:glider":{"ignored_patch":[1,2]}},"strict":true}"#;
    let ingredient = parse(source, &items);
    let load = load();
    let context = load.loop_context(LoopSide::Server, []);
    let root = tempfile::tempdir().unwrap();
    let recipes = crate::crafting::Recipes::from_sources(
        root.path(),
        &items,
        &horizon_world::loot::PackOverlay::NONE,
        vec![(
            "fixture:glider".into(),
            serde_json::json!({
                "type":"minecraft:crafting_shapeless",
                "ingredients":[serde_json::from_str::<Value>(source).unwrap()],
                "result":{"id":"minecraft:diamond"}
            }),
        )],
        &load,
    )
    .unwrap();
    assert_eq!(recipes.crafting.len(), 1);
    let default = ItemStack::new("minecraft:elytra", 1);
    let mut explicit = default.clone();
    explicit.data_mut().set_other(
        "minecraft:glider",
        Some(Tag::compound([(
            "ignored_candidate",
            Tag::String("anything".into()),
        )])),
    );
    let mut removed = default.clone();
    (_, removed.extra) = crate::item_data::decode_components(Some(&Tag::compound([(
        "!minecraft:glider",
        Tag::Int(9),
    )])));
    let mut malformed = default.clone();
    malformed
        .data_mut()
        .set_other("minecraft:glider", Some(Tag::Byte(1)));
    for stack in [&default, &explicit] {
        assert!(recipes.recipe_matches(&context, "fixture:glider", &[Some(stack.clone())], 1));
    }
    for stack in [&removed, &malformed] {
        assert!(!recipes.recipe_matches(&context, "fixture:glider", &[Some(stack.clone())], 1));
    }
    let absent = parse(
        r#"{"type":"neoforge:components","items":"minecraft:elytra","components":{"!minecraft:glider":123}}"#,
        &items,
    );
    assert!(!absent.test_in(&context, &default));
    assert!(!absent.test_in(&context, &explicit));
    assert!(absent.test_in(&context, &removed));
    let without_default = parse(source, &Items::empty());
    assert!(!without_default.test_in(&context, &default));
    assert!(without_default.test_in(&context, &explicit));
    assert_eq!(
        ingredient.shown.as_ref().unwrap()[0]
            .data()
            .unwrap()
            .other("minecraft:glider"),
        Some(&Tag::Compound(Default::default()))
    );
    let (_, allocations) = horizon_threads::allocation_probe::measure(|| {
        for _ in 0..100 {
            assert!(std::hint::black_box(ingredient.test_in(&context, &default)));
            assert!(std::hint::black_box(
                ingredient.test_in(&context, &explicit)
            ));
            assert!(!std::hint::black_box(
                ingredient.test_in(&context, &removed)
            ));
            assert!(!std::hint::black_box(
                ingredient.test_in(&context, &malformed)
            ));
        }
    });
    assert_eq!(allocations.calls, 0, "{allocations:?}");
    assert_eq!(allocations.bytes, 0);
}

#[test]
fn glider_component_malformed_payloads_name_component_and_omit_recipe() {
    let items = Items::empty();
    let root = tempfile::tempdir().unwrap();
    let load = load();
    for payload in [
        serde_json::json!(true),
        serde_json::json!(0),
        serde_json::json!("unit"),
        serde_json::json!([]),
    ] {
        let patch = crate::json_nbt::json_to_compound(
            serde_json::json!({"minecraft:glider":payload})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        let rule = IngredientRule::Components {
            patch,
            removed: vec![],
            strict: false,
        };
        let error = components::validate(&rule, &items).unwrap_err();
        assert!(error.contains("component minecraft:glider:"), "{error}");
        let recipe = serde_json::json!({
            "type":"minecraft:crafting_shapeless",
            "ingredients":[{
                "type":"neoforge:components",
                "items":"minecraft:elytra",
                "components":{"minecraft:glider":payload}
            }],
            "result":{"id":"minecraft:diamond"}
        });
        let recipes = crate::crafting::Recipes::from_sources(
            root.path(),
            &items,
            &horizon_world::loot::PackOverlay::NONE,
            vec![("fixture:invalid_glider".into(), recipe)],
            &load,
        )
        .unwrap();
        assert!(recipes.crafting.is_empty(), "{payload}");
    }
}

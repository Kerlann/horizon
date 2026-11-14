use super::*;

#[test]
fn tooltip_style_recipe_decodes_identifiers_and_matches_borrowed_values() {
    let mut items = Items::empty();
    items.component_defaults = std::sync::Arc::new(std::collections::HashMap::from([(
        "minecraft:stone".into(),
        crate::json_nbt::json_to_compound(
            serde_json::json!({"minecraft:tooltip_style":":style"})
                .as_object()
                .unwrap(),
        )
        .unwrap(),
    )]));
    let load = load();
    let context = load.loop_context(LoopSide::Server, []);
    let root = tempfile::tempdir().unwrap();
    for (value, valid) in [
        (serde_json::json!("style"), true),
        (serde_json::json!(":style"), true),
        (serde_json::json!("fixture:"), true),
        (serde_json::json!(""), true),
        (serde_json::json!(":"), true),
        (serde_json::json!("UPPER"), false),
        (serde_json::json!("..:style"), false),
        (serde_json::json!("a:b:c"), false),
        (serde_json::json!(3), false),
    ] {
        let source = serde_json::json!({
            "type":"minecraft:crafting_shapeless",
            "ingredients":[{"type":"neoforge:components","items":"minecraft:stone",
                "components":{"minecraft:tooltip_style":value}}],
            "result":{"id":"minecraft:diamond"}
        });
        let recipes = crate::crafting::Recipes::from_sources(
            root.path(),
            &items,
            &horizon_world::loot::PackOverlay::NONE,
            vec![("fixture:tooltip_style".into(), source)],
            &load,
        )
        .unwrap();
        assert_eq!(recipes.crafting.len(), usize::from(valid), "{value}");
        if !valid {
            continue;
        }
        let crate::crafting::Recipe::Shapeless { ingredients, .. } = &recipes.crafting[0].1 else {
            panic!("expected shapeless recipe");
        };
        let ingredient = &ingredients[0];
        let mut stack = ItemStack::new("minecraft:stone", 1);
        assert_eq!(
            ingredient.test_in(&context, &stack),
            value == "style" || value == ":style"
        );
        stack.data_mut().set_other(
            "minecraft:tooltip_style",
            Some(crate::crafting::json_tag(&value)),
        );
        let (_, allocations) = horizon_threads::allocation_probe::measure(|| {
            assert!(ingredient.test_in(&context, &stack));
        });
        assert_eq!(allocations.calls, 0);
        assert_eq!(allocations.bytes, 0);
        stack.data_mut().set_other(
            "minecraft:tooltip_style",
            Some(Tag::String("fixture:different".into())),
        );
        assert!(!ingredient.test_in(&context, &stack));
        stack.data_mut().set_other("minecraft:tooltip_style", None);
        stack.data_mut().set_other(
            "!minecraft:tooltip_style",
            Some(Tag::Compound(Default::default())),
        );
        assert!(!ingredient.test_in(&context, &stack));
    }
}

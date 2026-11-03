use super::*;

fn base(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "horizon-block-entity-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("assets/minecraft/blockstates")).unwrap();
    for (file, text) in [
        (
            "facts.json",
            r#"{"states":[["minecraft:stone"]],"defaults":{"minecraft:stone":0}}"#,
        ),
        (
            "gameplay-facts.json",
            r#"{"states":[{"item":"minecraft:stone"}]}"#,
        ),
        (
            "registries.json",
            r#"{"minecraft:item":{"entries":{}},"minecraft:block":{"entries":{}},
                "minecraft:block_entity_type":{"entries":{"minecraft:sign":{"protocol_id":8}}}}"#,
        ),
        ("items.json", "{}"),
        ("assets/minecraft/blockstates/stone.json", "{}"),
        ("horizon-resources.json", "{}"),
    ] {
        fs::write(base.join(file), text).unwrap();
    }
    base
}

fn kind(id: &str, blocks: &[&str]) -> BlockEntityType {
    BlockEntityType {
        id: id.into(),
        blocks: blocks.iter().map(|b| (*b).into()).collect(),
    }
}

fn declared_mod() -> ContentMod {
    let mut content = Content::declared("test");
    content
        .declare(
            &["test:a", "test:b"].map(|id| Block {
                id: id.into(),
                template: "minecraft:stone".into(),
                ..Block::default()
            }),
            &[],
        )
        .unwrap();
    ContentMod {
        id: "test".into(),
        content,
    }
}

#[test]
fn validates_declared_type_ids_and_exclusive_known_mod_block_membership() {
    let base = base("validation");
    let mut mods = [declared_mod()];
    for (types, error) in [
        (
            vec![
                kind("test:machine", &["test:a"]),
                kind("test:machine", &["test:b"]),
            ],
            "block entity type test:machine is already declared",
        ),
        (
            vec![kind("minecraft:sign", &["test:a"])],
            "block entity type minecraft:sign is already declared",
        ),
        (
            vec![kind("test:machine", &["test:a", "test:a"])],
            "block test:a belongs to both block entity types test:machine and test:machine",
        ),
        (
            vec![kind("test:one", &["test:a"]), kind("test:two", &["test:a"])],
            "block test:a belongs to both block entity types test:one and test:two",
        ),
        (
            vec![kind("test:machine", &["missing:block"])],
            "block entity type test:machine: missing:block is not a block",
        ),
        (
            vec![kind("test:machine", &["minecraft:stone"])],
            "mod test: block entity type test:machine cannot own vanilla block minecraft:stone",
        ),
    ] {
        mods[0].content.block_entity_types = types;
        assert_eq!(Plan::new(&base, &mods).err().unwrap().to_string(), error);
    }
    mods[0].content.block_entity_types = vec![kind("test:machine", &["test:b", "test:a"])];
    let plan = Plan::new(&base, &mods).unwrap();
    assert_eq!(
        serde_json::to_string(&plan.block_entity_membership).unwrap(),
        r#"{"test:a":"test:machine","test:b":"test:machine"}"#
    );
    drop(plan);
    mods[0].content.block_entity_types.clear();
    let plan = Plan::new(&base, &mods).unwrap();
    let mut registry = read_json(&base.join("registries.json")).unwrap();
    plan.json("registries.json", &mut registry).unwrap();
    let registry: serde_json::Value = serde_json::to_value(registry).unwrap();
    assert_eq!(
        registry["minecraft:block_entity_type"]["entries"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn code_declared_types_build_a_root_and_membership_changes_invalidate_its_cache() {
    let base = base("cache");
    let mut mods = [ContentMod {
        id: "test".into(),
        content: Content::declared("test"),
    }];
    let cache = base.with_extension("cache");
    assert_eq!(resource_root(&base, &mods, &cache).unwrap(), base);
    mods[0].content.block_entity_types = vec![kind("test:machine", &[])];
    let root = resource_root(&base, &mods, &cache).unwrap();
    assert_ne!(root, base);
    assert!(root.join(crate::content_identity::MARKER).exists());
    assert_eq!(
        fs::read_to_string(root.join(BLOCK_ENTITY_TYPES)).unwrap(),
        "{}"
    );
    mods[0].content.blocks = declared_mod().content.blocks;
    mods[0].content.block_entity_types[0].blocks = vec!["test:a".into()];
    resource_root(&base, &mods, &cache).unwrap();
    let stamp = fs::read(root.join(STAMP)).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(BLOCK_ENTITY_TYPES)).unwrap(),
        r#"{"test:a":"test:machine"}"#
    );
    // Same type and wire ID, different valid blocks: the overlay must not be reused.
    mods[0].content.block_entity_types[0].blocks = vec!["test:b".into()];
    resource_root(&base, &mods, &cache).unwrap();
    assert_ne!(fs::read(root.join(STAMP)).unwrap(), stamp);
    assert_eq!(
        fs::read_to_string(root.join(BLOCK_ENTITY_TYPES)).unwrap(),
        r#"{"test:b":"test:machine"}"#
    );
    let registry =
        horizon_protocol::registry::Builtin::load(&root.join("registries.json")).unwrap();
    assert_eq!(
        registry
            .get("minecraft:block_entity_type")
            .unwrap()
            .id("test:machine"),
        Some(9)
    );
    fs::remove_dir_all(cache).unwrap();
    fs::remove_dir_all(base).unwrap();
}

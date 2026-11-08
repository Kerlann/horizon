//! Real wasm capability providers within and across dependency groups.
use mod_api::__private::game::{BackendRefOf, Fields};
use mod_api::{
    Backend, BackendRef, BlockEntityRef, BlockEntityTypeKey, BoolGameRuleKey, Host, LevelRef,
    NativeWorldEpoch, ServerChatEvent, ServerHost, Text,
};

struct World;

fn overworld() -> LevelRef {
    LevelRef::new(NativeWorldEpoch::new(1), mod_api::DimensionKey::OVERWORLD)
}

impl mod_api::prelude::LevelsBackend for World {
    fn level_in_place(&self, level: LevelRef) -> bool {
        level == overworld()
    }
    fn level_game_rule_bool(&self, level: LevelRef, key: BoolGameRuleKey) -> Option<bool> {
        (level == overworld() && mod_api::__private::game_rule_bool_slot(key).is_some())
            .then_some(true)
    }
}
impl mod_api::prelude::BlockEntitiesBackend for World {
    fn block_entity_kind(&self, entity: BlockEntityRef) -> Option<BlockEntityTypeKey> {
        (entity.fields().position == [0, 64, 0])
            .then(|| mod_api::__private::block_entity_type_key("test:counter_box"))
            .flatten()
    }
}
impl Backend for World {}
mod_api::impl_backend_areas!(World, except(block_entities, levels));
impl mod_api::prelude::EntitiesHost for World {
    fn level(&self, dimension: &str) -> Option<LevelRef> {
        (dimension == "minecraft:overworld").then(overworld)
    }
}
impl mod_api::prelude::BlockEntitiesHost for World {
    fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
        Some(mod_api::__private::game::block_entity_ref(
            level, position, 1,
        ))
    }
}
impl Host for World {
    fn backend(&self) -> Option<BackendRef<'_>> {
        Some(BackendRef::of(self))
    }
}
mod_api::impl_host_areas!(World, except(block_entities, entities));
impl ServerHost for World {}
mod_api::impl_server_host_areas!(World);

#[test]
fn capability_is_answered_only_within_its_group() {
    let (Ok(folder), Ok(std)) = (
        std::env::var("HORIZON_WASM_GROUPS"),
        std::env::var("HORIZON_WASM_STD"),
    ) else {
        eprintln!("skipping: group fixtures and shared std are not set");
        return;
    };
    let mut infos = Vec::new();
    let packages = ["counter_lib", "alpha", "beta", "gamma"].map(|id| {
        let path = std::path::Path::new(&folder).join(format!("{id}/target/horizon-mod/{id}.hmod"));
        let manifest = horizon_mod_package::Package::read(&path).unwrap().manifest;
        infos.push(mod_api::PackageInfo::new(
            manifest.id.clone(),
            manifest.version.clone(),
            mod_api::PackageKind::Code,
            mod_api::Side::Both,
            vec![manifest.id.clone()],
            manifest.dependencies.keys().cloned().collect(),
        ));
        horizon_mod_wasm::WasmPackage {
            manifest,
            module: horizon_mod_package::Package::read_wasm(&path).unwrap(),
        }
    });
    let loaded = horizon_mod_wasm::load(&std::fs::read(std).unwrap(), packages.into());
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    loaded.initialize(&infos).unwrap();
    mod_api::testing::with_mods(loaded.mods.into(), || {
        mod_api::__private::install_parts(
            mod_api::__private::mods(),
            &mod_api::__private::part_tests::rule_schema(),
        )
        .unwrap();
        mod_api::__private::mark_game_thread();
        let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        let types = horizon_client_core::block_entity_types::BlockEntityTypes::new(
            Default::default(),
            vec!["test:counter_box".into()],
        );
        mod_api::__private::resolve_block_entity_types(types.names(), |id| types.key(id)).unwrap();
        let mut answers: Vec<String> = mod_api::__private::mods()
            .iter()
            .map(|entry| {
                let mut message = Text::literal("");
                let mut replies = Vec::new();
                let mut event = ServerChatEvent {
                    player_id: 1,
                    player: 1,
                    username: "tester",
                    raw_text: "",
                    message: &mut message,
                    replies: &mut replies,
                };
                entry.fields().hooks.server_chat(&mut World, &mut event);
                let mod_api::text::TextContent::Literal(answer) = message.content else {
                    panic!("expected literal reply")
                };
                answer
            })
            .filter(|answer| !answer.is_empty())
            .collect();
        answers.sort();
        assert_eq!(answers, ["alpha: 7 true", "beta: 7", "gamma: none"]);
    });
}

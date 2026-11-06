//! A capability of a library mod's own handler type across wasm groups: two mods depending on
//! the library share one group, so the one answering it is reached by the other; a mod of
//! another group asking a capability of the same name gets none. CI builds the mods in
//! `tests/groups/` first and points `HORIZON_WASM_GROUPS` at that folder and `HORIZON_WASM_STD`
//! at the std member; the test is ignored elsewhere.
use mod_api::__private::game::BackendRefOf;
use mod_api::__private::game::Fields;
use mod_api::{
    Backend, BackendRef, BlockEntityRef, BlockEntityTypeKey, BoolGameRuleKey, Host, LevelRef,
    NativeWorldEpoch, ServerChatEvent, ServerHost, Text,
};

/// One level with a `test:counter_box` block entity at its origin column.
struct World;

fn overworld() -> LevelRef {
    LevelRef::new(NativeWorldEpoch::new(1), "minecraft:overworld")
}

impl Backend for World {
    fn level_in_place(&self, level: LevelRef) -> bool {
        level == overworld()
    }
    fn level_game_rule_bool(&self, level: LevelRef, key: BoolGameRuleKey) -> Option<bool> {
        (level == overworld() && mod_api::__private::game_rule_bool_slot(key).is_some())
            .then_some(true)
    }
    fn block_entity_kind(&self, entity: BlockEntityRef) -> Option<BlockEntityTypeKey> {
        (entity.fields().position == [0, 64, 0])
            .then(|| mod_api::__private::block_entity_type_key("test:counter_box"))
            .flatten()
    }
}

impl Host for World {
    fn backend(&self) -> Option<BackendRef<'_>> {
        Some(BackendRef::of(self))
    }
    fn level(&self, dimension: &str) -> Option<LevelRef> {
        (dimension == "minecraft:overworld").then(overworld)
    }
    fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
        Some(mod_api::__private::game::block_entity_ref(
            level, position, 1,
        ))
    }
}

impl ServerHost for World {}

/// The built group mods, and the std member.
fn built() -> ([horizon_mod_wasm::WasmPackage; 4], Vec<u8>) {
    let path = |var: &str| std::path::PathBuf::from(std::env::var(var).expect(var));
    let folder = path("HORIZON_WASM_GROUPS");
    let std_member = std::fs::read(path("HORIZON_WASM_STD")).unwrap();
    let packages = ["counter_lib", "alpha", "beta", "gamma"].map(|id| {
        let package = folder.join(format!("{id}/target/horizon-mod/{id}.hmod"));
        horizon_mod_wasm::WasmPackage {
            manifest: horizon_mods::Package::read(&package).unwrap().manifest,
            module: horizon_mods::Package::read_wasm(&package).unwrap(),
        }
    });
    (packages, std_member)
}

#[test]
#[ignore = "needs the group mods built to mod.wasm (CI's wasm-mods job)"]
fn a_library_refused_over_a_point_takes_its_dependents_with_it() {
    let (mut packages, std_member) = built();
    // The library lists a point this game doesn't have.
    let wasm = packages[0].manifest.wasm.as_ref().unwrap();
    packages[0].manifest = horizon_mods::manifest::Manifest::parse(&format!(
        "id = \"counter_lib\"\nname = \"Counter library\"\nversion = \"1.0.0\"\napi = \"0.2\"\n\
         capabilities = [\"registries\"]\n\n[wasm]\nsha512 = \"{}\"\ntoolchain = \"{}\"\n\n\
         [[native.injection]]\nid = \"gone\"\ntarget = \"horizon_client::game::Game<'_>::gone\"\n\
         kind = \"HEAD\"\ntrait_method = \"gone\"\n",
        wasm.sha512(),
        wasm.toolchain()
    ))
    .unwrap();
    let loaded = horizon_mod_wasm::load(
        &horizon_mod_wasm::RuntimeConfig::default(),
        &std_member,
        packages.into(),
    );
    let mut refused: Vec<(&str, String)> = loaded
        .refused
        .iter()
        .map(|(id, why)| {
            let why = match why {
                horizon_mod_wasm::Refusal::NoSuchPoint { .. } => "no such point".to_owned(),
                horizon_mod_wasm::Refusal::DependencyRefused { dependency } => {
                    format!("needs {dependency}")
                }
                why => why.to_string(),
            };
            (id.as_str(), why)
        })
        .collect();
    refused.sort();
    assert_eq!(
        refused,
        [
            ("alpha", "needs counter_lib".to_owned()),
            ("beta", "needs counter_lib".to_owned()),
            ("counter_lib", "no such point".to_owned()),
        ]
    );
    // Only gamma, of the other group, is installed, and nothing listens at points.
    assert_eq!(loaded.mods.len(), 1);
    assert!(loaded.listening.is_empty());
}

#[test]
#[ignore = "needs the group mods built to mod.wasm (CI's wasm-mods job)"]
fn a_library_capability_is_answered_in_its_group_and_nowhere_else() {
    let (packages, std_member) = built();
    let loaded = horizon_mod_wasm::load(
        &horizon_mod_wasm::RuntimeConfig::default(),
        &std_member,
        packages.into(),
    );
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    mod_api::__private::install_parts(&loaded.mods).unwrap();
    assert!(
        mod_api::__private::game_rule_registry()
            .categories()
            .contains(&"alpha:machines".to_owned())
    );
    mod_api::__private::resolve_block_entity_types(
        &horizon_client_core::block_entity_types::BlockEntityTypes::new(
            Default::default(),
            vec!["test:counter_box".into()],
        ),
    )
    .unwrap();
    let mut answers: Vec<String> = loaded
        .mods
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
            message.plain()
        })
        .filter(|answer| !answer.is_empty())
        .collect();
    answers.sort();
    assert_eq!(answers, ["alpha: 7 true", "beta: 7", "gamma: none"]);
}

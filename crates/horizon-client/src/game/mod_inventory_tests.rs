use super::*;
use horizon_mods::{enabled::EnabledMods, selection::SelectedPackage};
use mod_api::{Hooks, ModEntry, testing::with_mods};
use std::sync::atomic::{AtomicBool, Ordering};

struct EmptyCode;
#[mod_api::__subscriptions]
impl Hooks for EmptyCode {}

fn selected(fixture: &Fixture, id: &str, code: bool, side: &str) -> SelectedPackage {
    let module = b"\0asm\x01\0\0\0";
    let wasm = if code {
        format!(
            "\n[wasm]\nsha512='{}'\ntoolchain='fixture'\n",
            horizon_mods::manifest::sha512_of(module)
        )
    } else {
        String::new()
    };
    let manifest = format!(
        "id='{id}'\nname='{id}'\nversion='1'\napi='0.2'\nside='{side}'\nnamespaces=['shared_space']\ncapabilities=['registries']\n{wasm}"
    );
    let files: Vec<(&str, &[u8])> = if code {
        vec![
            ("mod.wasm", module.as_slice()),
            (
                "Cargo.toml",
                b"[package]\nname='inventory_fixture'\nversion='1.0.0'\n".as_slice(),
            ),
            ("src/lib.rs", b"pub struct Mod;".as_slice()),
        ]
    } else {
        Vec::new()
    };
    let path = fixture.0.join(format!("{id}.hmod"));
    std::fs::write(
        &path,
        horizon_mods::Package::write_files(&manifest, &files).unwrap(),
    )
    .unwrap();
    SelectedPackage {
        package: horizon_mods::Package::read(&path).unwrap(),
        path,
    }
}

#[test]
fn admission_uses_active_native_and_wasm_entries_and_accepted_content_not_startup_metadata() {
    let fixture = Fixture::new("inventory-admission");
    hmod(&fixture.0, "machine_sample");
    let sources = ModsState {
        native: Arc::new(fixture.runtime()),
        selected: vec![
            selected(&fixture, "wasm_mod", true, "both"),
            selected(&fixture, "client_code", true, "client"),
            selected(&fixture, "refused_code", true, "both"),
            selected(&fixture, "plain", false, "both"),
            selected(&fixture, "refused_content", false, "both"),
        ]
        .into(),
        ..Default::default()
    };
    with_mods(
        vec![
            ModEntry::new("machine_sample", Box::new(EmptyCode)),
            ModEntry::new("wasm_mod", Box::new(EmptyCode)),
            ModEntry::new("client_code", Box::new(EmptyCode)),
        ]
        .into(),
        || {
            mod_api::__private::with_test_packages(&["refused_code", "shared_space"], || {
                assert!(
                    mod_api::__private::loaded_mods()
                        .get("refused_code")
                        .is_some()
                );
                let mut startup = sources.clone();
                let disabled =
                    std::collections::BTreeSet::from(["machine_sample".into(), "wasm_mod".into()]);
                startup.capture_code_switches(&disabled);
                assert!(mod_api::__private::mod_index_on(
                    mod_api::__private::mod_index("machine_sample").unwrap()
                ));
                assert_eq!(
                    startup
                        .enabled_inventory(|id| id == "plain")
                        .server_ids()
                        .collect::<Vec<_>>(),
                    ["plain"]
                );
                startup.apply_code_switches(&disabled);
                assert!(mod_api::__private::set_mod_enabled("machine_sample", true));
                assert!(mod_api::__private::set_mod_enabled("wasm_mod", true));
                assert_eq!(
                    startup
                        .enabled_inventory(|id| id == "plain")
                        .server_ids()
                        .collect::<Vec<_>>(),
                    ["machine_sample", "plain", "wasm_mod"]
                );
                let initial = sources.enabled_inventory(|id| id == "plain");
                assert_eq!(
                    initial.client_ids().collect::<Vec<_>>(),
                    ["client_code", "machine_sample", "plain", "wasm_mod"]
                );
                assert_eq!(
                    initial.server_ids().collect::<Vec<_>>(),
                    ["machine_sample", "plain", "wasm_mod"]
                );
                assert!(mod_api::__private::set_mod_enabled("machine_sample", false));
                assert!(mod_api::__private::set_mod_enabled("wasm_mod", false));
                let paused = sources.enabled_inventory(|id| id == "plain");
                assert_eq!(paused.server_ids().collect::<Vec<_>>(), ["plain"]);
                assert_eq!(
                    paused.client_ids().collect::<Vec<_>>(),
                    ["client_code", "plain"]
                );
                assert_eq!(
                    initial.server_ids().collect::<Vec<_>>(),
                    ["machine_sample", "plain", "wasm_mod"]
                );
            });
        },
    );
}

struct Guard(Arc<AtomicBool>);
impl mod_api::ServerReloadListener for Guard {
    fn prepare(
        &self,
        _: &mod_api::ServerReloadContext,
    ) -> std::result::Result<Box<dyn mod_api::ReloadApply>, mod_api::ReloadResourceError> {
        if self.0.load(Ordering::Relaxed) {
            return Err(mod_api::ReloadResourceError::new(
                mod_api::Identifier::parse("reload_guard:guard").unwrap(),
                std::io::Error::other("inventory fixture rejected the new generation"),
            ));
        }
        Ok(<dyn mod_api::ReloadApply>::new(|| {}))
    }
}
#[mod_api::__subscriptions]
impl Hooks for Guard {
    fn add_server_reload_listeners(
        &self,
        _: &dyn mod_api::Host,
        event: &mut mod_api::AddServerReloadListenersEvent<'_>,
    ) {
        event
            .listeners
            .add_listener(
                mod_api::Identifier::parse("reload_guard:guard").unwrap(),
                mod_api::FileToIdConverter::json("inventory_test").unwrap(),
                Arc::new(Guard(self.0.clone())),
            )
            .unwrap();
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn code_only_reload_publishes_one_new_inventory_after_prepare_and_preserves_old_readers() {
    let fixture = Fixture::new("inventory-reload");
    hmod(&fixture.0, "machine_sample");
    let sources = ModsState {
        native: Arc::new(fixture.runtime()),
        ..Default::default()
    };
    let reject = Arc::new(AtomicBool::new(false));
    with_mods(
        vec![
            ModEntry::new("machine_sample", Box::new(EmptyCode)),
            ModEntry::new("reload_guard", Box::new(Guard(reject.clone()))),
        ]
        .into(),
        || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let original = game.options.resources.clone();
                let initial = content_overlay(&original, &sources, &fixture.0.join("jars"))
                    .unwrap()
                    .root;
                game.options.resources = initial.prepared().to_owned();
                game.pack.data_source = initial.data().clone();
                game.pack.data_changes = initial.data_changes().clone();
                game.mods = sources.clone();
                game.world_root = fixture.0.join("world");
                let pack = game.world_root.join("datapacks/inventory");
                let recipes = pack.join("data/inventorytest/recipe");
                std::fs::create_dir_all(&recipes).unwrap();
                std::fs::write(pack.join("pack.mcmeta"), serde_json::to_vec(&serde_json::json!({
                "pack": {"description":"inventory fixture", "min_format":horizon_core::version::DATA_PACK_FORMAT, "max_format":horizon_core::version::DATA_PACK_FORMAT}
            })).unwrap()).unwrap();
                std::fs::write(recipes.join("requires_code.json"), br#"{"type":"inventorytest:probe","neoforge:conditions":[{"type":"neoforge:mod_loaded","modid":"machine_sample"}]}"#).unwrap();
                game.reload_datapacks(true, &[]).unwrap();
                let read = |game: &Game<'_>| {
                    crate::game::lan::server_sources(
                        &game.options.resources,
                        &game.res.core.item_data,
                        &game.pack_files(),
                    )
                    .unwrap()
                };
                let has_recipe = |sources: &horizon_client::crafting::ResolvedSources| {
                    sources
                        .recipes
                        .iter()
                        .any(|recipe| recipe.name == "inventorytest:requires_code")
                };
                let old = read(game);
                assert!(has_recipe(&old));
                let old_root = game.options.resources.clone();
                let old_data = game.pack.data_source.clone();
                assert!(mod_api::__private::set_mod_enabled("machine_sample", false));
                reject.store(true, Ordering::Relaxed);
                assert!(game.reload_datapacks(true, &[]).is_err());
                assert_eq!(game.options.resources, old_root);
                assert!(game.pack.data_source.same_generation(&old_data));
                assert_eq!(
                    EnabledMods::read(&old_root)
                        .unwrap()
                        .server_ids()
                        .collect::<Vec<_>>(),
                    ["machine_sample"]
                );
                reject.store(false, Ordering::Relaxed);
                game.reload_datapacks(true, &[]).unwrap();
                assert_eq!(game.options.resources, original);
                assert!(!has_recipe(&read(game)));
                assert!(has_recipe(&old));
                assert!(mod_api::__private::set_mod_enabled("machine_sample", true));
                game.reload_datapacks(true, &[]).unwrap();
                assert!(has_recipe(&read(game)));
                assert_eq!(game.options.resources, old_root);
            });
        },
    );
}

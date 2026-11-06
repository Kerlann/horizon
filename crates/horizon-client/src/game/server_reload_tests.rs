use super::*;
use crate::game::{native_entity_refs::tests::with_game, world_loading::WorldRequest};
use mod_api::{
    self, AddServerReloadListenersEvent, Hooks, Host, ListenerKey, ModEntry, ReloadApply,
    ReloadResourceError, ServerReloadListener,
};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

struct DataListener {
    live: Arc<Mutex<u64>>,
    reject: Arc<AtomicBool>,
}
impl ServerReloadListener for DataListener {
    fn prepare(
        &self,
        context: &ServerReloadContext,
    ) -> Result<Box<dyn ReloadApply>, ReloadResourceError> {
        let id = Identifier::parse("reload_test:value").unwrap();
        assert!(context.list().any(|entry| entry == &id));
        // Arbitrary IDs only perform map lookups, never filesystem reads.
        assert!(
            context
                .read(&Identifier::parse("reload_test:../private").unwrap())
                .is_none()
        );
        let mut value = 0;
        for id in context.list() {
            value += serde_json::from_slice::<u64>(context.read(id).unwrap())
                .map_err(|error| ReloadResourceError::new(id.clone(), error))?;
        }
        let live = self.live.clone();
        Ok(<dyn ReloadApply>::new(move || {
            *live.lock().unwrap() = value
        }))
    }
}
struct CheckListener(Arc<AtomicBool>);
impl ServerReloadListener for CheckListener {
    fn prepare(
        &self,
        _: &ServerReloadContext,
    ) -> Result<Box<dyn ReloadApply>, ReloadResourceError> {
        if self.0.load(Ordering::Relaxed) {
            return Err(ReloadResourceError::new(
                Identifier::parse("reload_test:value").unwrap(),
                std::io::Error::other("rejected new machine data"),
            ));
        }
        Ok(<dyn ReloadApply>::new(|| {}))
    }
}
#[mod_api::__subscriptions]
impl Hooks for DataListener {
    fn add_server_reload_listeners(
        &self,
        _host: &dyn Host,
        event: &mut AddServerReloadListenersEvent<'_>,
    ) {
        let id = |text| Identifier::parse(text).unwrap();
        event
            .listeners
            .add_listener(
                id("reload_test:data"),
                FileToIdConverter::json("machines").unwrap(),
                Arc::new(Self {
                    live: self.live.clone(),
                    reject: self.reject.clone(),
                }),
            )
            .unwrap();
        event
            .listeners
            .add_listener(
                id("reload_test:check"),
                FileToIdConverter::json("machines").unwrap(),
                Arc::new(CheckListener(self.reject.clone())),
            )
            .unwrap();
        event
            .listeners
            .add_dependency(id("reload_test:data"), id("reload_test:check"))
            .unwrap();
    }
}
fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}
fn request(root: &Path) -> WorldRequest {
    WorldRequest {
        save_root: root.to_owned(),
        folder: "test".into(),
        name: Some("test".into()),
        seed: Some(1),
        creative: true,
        create: None,
    }
}

#[test]
#[ignore = "needs prepared resources"]
fn missing_original_at_reload_preserves_the_published_data_and_reports_its_owners() {
    const MODE: &str = "HORIZON_JAR_RELOAD_CHILD";
    let root = if let Some(root) = std::env::var_os(MODE) {
        std::path::PathBuf::from(root)
    } else {
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("server-reload-jar-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--ignored", "--exact", "game::server_reload::tests::missing_original_at_reload_preserves_the_published_data_and_reports_its_owners", "--nocapture"])
            .env(MODE, &root).env("HORIZON_NO_AUDIO", "1").env("HORIZON_LAUNCHER_NO_SELF_UPDATE", "1");
        for key in [
            "HORIZON_HOME",
            "HORIZON_LAUNCHER_HOME",
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
            "HORIZON_LAUNCHER_TRASH",
            "HORIZON_LAUNCHER_CLIPBOARD",
            "HORIZON_LAUNCHER_DESKTOP",
        ] {
            command.env(key, root.join(key));
        }
        let status = command.status().unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(status.success());
        return;
    };
    let jar = horizon_mods::manifest::JarRef::new("original".into(), "1".into(), "a".repeat(128))
        .unwrap();
    let jar_file = jar.cache_path(&horizon_client::logging::game_home().join("jars"));
    std::fs::create_dir_all(jar_file.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&jar_file).unwrap());
    zip.start_file(
        "data/reload_test/function/keep.mcfunction",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    std::io::Write::write_all(&mut zip, b"say original").unwrap();
    zip.finish().unwrap();
    let original = std::fs::read(&jar_file).unwrap();
    let manifest = format!(
        "id='reload_test'\nname='Reload Test'\nversion='1'\napi='0.2'\ncapabilities=['registries']\n[[jar]]\nid='original'\nversion='1'\nsha512='{}'\n",
        jar.sha512()
    );
    let package = root.join("reload_test.hmod");
    write(
        &package,
        horizon_mods::Package::write_files(&manifest, &[]).unwrap(),
    );
    let package_manifest = horizon_mods::Package::read(&package).unwrap().manifest;
    let content = horizon_mods::Content::read(&package, &package_manifest).unwrap();
    let resources = root.join("resources");
    let base = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    horizon_client::mod_content::build(
        &base,
        jar_file.parent().unwrap(),
        &[horizon_client::mod_content::ContentMod {
            id: "reload_test".into(),
            content,
            jars: vec![jar.clone()],
        }],
        &resources,
    )
    .unwrap();
    let world = root.join("world");
    let function = world.join("datapacks/test/data/reload_test/function/keep.mcfunction");
    write(&world.join("datapacks/test/pack.mcmeta"), "{}");
    write(&function, "say old");
    write(
        &world.join("datapacks.json"),
        r#"{"enabled":["file/test"],"disabled":[]}"#,
    );
    with_game(|game| {
        game.options.resources = resources.clone();
        game.load_world(request(&world)).unwrap();
        let previous = game.fns.store.functions["reload_test:keep"].clone();
        let enabled = game.fns.enabled().to_vec();
        write(&function, "say new");
        std::fs::remove_file(&jar_file).unwrap();
        let error = game
            .reload_datapacks(true, &game.level.enabled_features.clone())
            .err()
            .unwrap();
        let ReloadStageError::OriginalJars(horizon_mods::manifest::JarInputsError::Missing(
            missing,
        )) = error
        else {
            panic!("expected typed missing original, got {error:?}");
        };
        assert_eq!(missing.owners, ["reload_test"]);
        assert_eq!(missing.jar, jar);
        assert_eq!(missing.path, jar_file);
        let reported = game.cmd_reload().unwrap_err().message.plain();
        assert!(reported.contains("reload_test: original 1:"), "{reported}");
        assert!(reported.contains(jar.sha512()), "{reported}");
        assert_eq!(game.options.resources, resources);
        assert_eq!(game.fns.store.functions["reload_test:keep"], previous);
        assert_eq!(game.fns.enabled(), enabled);
        write(&jar_file, &original);
        assert!(game.run_command("reload"));
        assert_ne!(game.fns.store.functions["reload_test:keep"], previous);
    });
}

#[test]
#[ignore = "needs prepared resources"]
fn server_reload_delivers_mod_files_overrides_and_preserves_all_data_on_failure() {
    let root = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("server-reload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let manifest = "id = \"reload_test\"\nname = \"Reload Test\"\nversion = \"0.1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n";
    let package = root.join("reload_test.hmod");
    write(
        &package,
        horizon_mods::Package::write_files(
            manifest,
            &[("data/reload_test/machines/value.json", b"1")],
        )
        .unwrap(),
    );
    let package_manifest = horizon_mods::Package::read(&package).unwrap().manifest;
    let content = horizon_mods::Content::read(&package, &package_manifest).unwrap();
    let resources = root.join("resources");
    let base = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    horizon_client::mod_content::build(
        &base,
        Path::new(""),
        &[horizon_client::mod_content::ContentMod {
            id: "reload_test".into(),
            content,
            jars: Vec::new(),
        }],
        &resources,
    )
    .unwrap();
    let world = root.join("world");
    let pack = world.join("datapacks/test");
    write(&pack.join("pack.mcmeta"), "{}");
    let data = pack.join("data/reload_test/machines/value.json");
    write(&data, "2");
    // Other namespaces and subdirectories join the same directory listener.
    write(&pack.join("data/other/machines/heavy/extra.json"), "5");
    write(&pack.join("data/other/machines/ignored.txt"), "not json");
    let function = pack.join("data/reload_test/function/keep.mcfunction");
    write(&function, "say old");
    write(
        &world.join("datapacks.json"),
        r#"{"enabled":["file/test"],"disabled":[]}"#,
    );
    let live = Arc::new(Mutex::new(0));
    let reject = Arc::new(AtomicBool::new(false));
    let mods = vec![ModEntry::new(
        "reload_test",
        Box::new(DataListener {
            live: live.clone(),
            reject: reject.clone(),
        }),
    )]
    .into();
    mod_api::testing::with_mods(mods, || {
        with_game(|game| {
            game.options.resources = resources;
            game.load_world(request(&world)).unwrap();
            assert_eq!(
                *live.lock().unwrap(),
                7,
                "override 2 plus other namespace 5"
            );
            assert!(game.run_command("datapack disable \"file/test\""));
            assert_eq!(
                *live.lock().unwrap(),
                1,
                "the mod's packaged file is the fallback"
            );
            assert!(game.run_command("datapack enable \"file/test\""));
            assert_eq!(*live.lock().unwrap(), 7);
            write(&data, "3");
            assert!(game.run_command("reload"));
            assert_eq!(*live.lock().unwrap(), 8);
            let old_function = game.fns.store.functions["reload_test:keep"].clone();
            let old_selection = std::fs::read(world.join("datapacks.json")).unwrap();
            let old_enabled = game.fns.enabled().to_vec();
            let old_vanilla_disabled = game.cmds.vanilla_disabled;
            write(&data, "4");
            write(&function, "say new");
            reject.store(true, Ordering::Relaxed);
            assert!(!game.run_command("reload"));
            assert_eq!(
                *live.lock().unwrap(),
                8,
                "earlier successful prepare must not publish"
            );
            assert_eq!(game.fns.store.functions["reload_test:keep"], old_function);
            let error = game
                .reload_datapacks(true, &game.level.enabled_features.clone())
                .err()
                .unwrap()
                .to_string();
            assert!(
                error.contains("reload listener reload_test:check: reload_test:value"),
                "{error}"
            );
            assert!(!game.run_command("datapack disable \"file/test\""));
            assert_eq!(
                std::fs::read(world.join("datapacks.json")).unwrap(),
                old_selection
            );
            assert_eq!(
                game.fns.enabled(),
                old_enabled,
                "the refused change publishes nothing"
            );
            assert_eq!(
                game.cmds.vanilla_disabled, old_vanilla_disabled,
                "the refused change flips no command switch"
            );
            assert!(game.fns.store.functions.contains_key("reload_test:keep"));
            assert_eq!(*live.lock().unwrap(), 8);
            let previous_root = game.options.resources.clone();
            let previous_applied = game.mods.applied.clone();
            let previous_enabled = game.settings.mods.enabled.clone();
            let previous_selected = game.mods.selected.clone();
            let previous_refused = game.mods.refused.clone();
            let previous_switched = game.mods.data_switched;
            let sources = game
                .mods
                .for_reload(&root, ["reload_test".to_owned()].into());
            let candidate_root = root.join("candidate-resources");
            std::fs::create_dir_all(&candidate_root).unwrap();
            let error = game.reload_mod_data(candidate_root, &sources).unwrap_err();
            assert!(error.to_string().contains("rejected new machine data"));
            assert_eq!(game.options.resources, previous_root);
            assert_eq!(game.mods.applied, previous_applied);
            assert_eq!(game.settings.mods.enabled, previous_enabled);
            assert!(Arc::ptr_eq(&game.mods.selected, &previous_selected));
            assert!(Arc::ptr_eq(&game.mods.refused, &previous_refused));
            assert_eq!(game.mods.data_switched, previous_switched);
            assert_eq!(game.fns.store.functions["reload_test:keep"], old_function);
            assert_eq!(*live.lock().unwrap(), 8);
            reject.store(false, Ordering::Relaxed);
            write(&data, "not JSON");
            let error = game
                .reload_datapacks(true, &game.level.enabled_features.clone())
                .err()
                .unwrap()
                .to_string();
            assert!(
                error.contains("reload listener reload_test:data: reload_test:value"),
                "{error}"
            );
            assert_eq!(*live.lock().unwrap(), 8);
            assert_eq!(game.fns.store.functions["reload_test:keep"], old_function);
            // Retry the same content selection after fixing its data.
            write(&data, "4");
            game.reload_mod_data(previous_root.clone(), &sources)
                .unwrap();
            assert_eq!(game.options.resources, previous_root);
            assert_eq!(game.mods.applied, sources.applied);
            assert_eq!(game.settings.mods.enabled, sources.applied.clone().unwrap());
            assert!(Arc::ptr_eq(&game.mods.selected, &sources.selected));
            assert!(Arc::ptr_eq(&game.mods.refused, &sources.refused));
            assert!(game.mods.data_switched);
            assert_eq!(*live.lock().unwrap(), 9);
            assert_ne!(game.fns.store.functions["reload_test:keep"], old_function);
        })
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "needs prepared resources"]
fn server_reload_skips_unparsable_files_and_delivers_the_rest() {
    // Vanilla's `SimpleJsonResourceReloadListener.scanDirectory` logs and skips one bad
    // file; only a listener's own error aborts the reload. This listener follows it.
    struct ParsingListener(Arc<Mutex<Vec<(String, String)>>>);
    impl ServerReloadListener for ParsingListener {
        fn prepare(
            &self,
            context: &ServerReloadContext,
        ) -> Result<Box<dyn ReloadApply>, ReloadResourceError> {
            let mut parsed = Vec::new();
            for id in context.list() {
                match serde_json::from_slice::<String>(context.read(id).unwrap()) {
                    Ok(value) => parsed.push((id.to_string(), value)),
                    Err(error) => log::warn!("Couldn't parse data file '{id}': {error}"),
                }
            }
            let live = self.0.clone();
            Ok(<dyn ReloadApply>::new(move || {
                *live.lock().unwrap() = parsed
            }))
        }
    }
    struct SkipHooks(Arc<Mutex<Vec<(String, String)>>>);
    #[mod_api::__subscriptions]
    impl Hooks for SkipHooks {
        fn add_server_reload_listeners(
            &self,
            _host: &dyn Host,
            event: &mut AddServerReloadListenersEvent<'_>,
        ) {
            event
                .listeners
                .add_listener(
                    Identifier::parse("reload_test:data").unwrap(),
                    FileToIdConverter::json("machines").unwrap(),
                    Arc::new(ParsingListener(self.0.clone())),
                )
                .unwrap();
        }
    }
    let root = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("server-reload-skip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let manifest = "id = \"reload_test\"\nname = \"Reload Test\"\nversion = \"0.1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n";
    let package = root.join("reload_test.hmod");
    write(
        &package,
        horizon_mods::Package::write_files(
            manifest,
            &[("data/reload_test/machines/bar.json", b"\"good\"")],
        )
        .unwrap(),
    );
    let package_manifest = horizon_mods::Package::read(&package).unwrap().manifest;
    let content = horizon_mods::Content::read(&package, &package_manifest).unwrap();
    let resources = root.join("resources");
    let base = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    horizon_client::mod_content::build(
        &base,
        Path::new(""),
        &[horizon_client::mod_content::ContentMod {
            id: "reload_test".into(),
            content,
            jars: Vec::new(),
        }],
        &resources,
    )
    .unwrap();
    let world = root.join("world");
    let pack = world.join("datapacks/test");
    write(&pack.join("pack.mcmeta"), "{}");
    let malformed = pack.join("data/reload_test/machines/foo.json");
    write(&malformed, "{bad json");
    write(
        &pack.join("data/reload_test/machines/bar.json"),
        "\"better\"",
    );
    write(
        &world.join("datapacks.json"),
        r#"{"enabled":["file/test"],"disabled":[]}"#,
    );
    let live = Arc::new(Mutex::new(Vec::new()));
    mod_api::testing::with_mods(
        vec![ModEntry::new(
            "reload_test",
            Box::new(SkipHooks(live.clone())),
        )]
        .into(),
        || {
            with_game(|game| {
                game.options.resources = resources;
                game.load_world(request(&world)).unwrap();
                assert_eq!(
                    *live.lock().unwrap(),
                    vec![("reload_test:bar".into(), "better".into())],
                    "the malformed file is skipped and the override wins"
                );
                write(&malformed, "\"fixed\"");
                assert!(game.run_command("reload"));
                assert_eq!(
                    *live.lock().unwrap(),
                    vec![
                        ("reload_test:bar".into(), "better".into()),
                        ("reload_test:foo".into(), "fixed".into()),
                    ],
                    "a repaired file joins the next reload"
                );
                write(&malformed, "still not json");
                assert!(
                    game.run_command("reload"),
                    "a skipped file never fails the reload"
                );
                assert_eq!(
                    *live.lock().unwrap(),
                    vec![("reload_test:bar".into(), "better".into())],
                    "the bad file is skipped again"
                );
            })
        },
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "needs prepared resources"]
fn server_reload_registration_failure_stops_world_open_and_quiet_mods_never_listen() {
    struct Quiet;
    #[mod_api::__subscriptions]
    impl Hooks for Quiet {}
    struct Broken;
    #[mod_api::__subscriptions]
    impl Hooks for Broken {
        fn add_server_reload_listeners(
            &self,
            _host: &dyn Host,
            event: &mut AddServerReloadListenersEvent<'_>,
        ) {
            let _ = event.listeners.add_dependency(
                Identifier::parse("mistyped:listener").unwrap(),
                VanillaServerListeners::functions(),
            );
        }
    }
    let root = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("server-reload-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    mod_api::testing::with_mods(vec![ModEntry::new("quiet", Box::new(Quiet))].into(), || {
        with_game(|game| {
            // A mod that doesn't implement the hook is never subscribed, so
            // world open and `/reload` skip the registration graph.
            assert!(!mod_api::__private::listened(
                mod_api::HookId::AddServerReloadListeners
            ));
            game.load_world(request(&root)).unwrap();
            assert!(!mod_api::__private::listened(
                mod_api::HookId::AddServerReloadListeners
            ));
            assert!(game.run_command("reload"));
            assert!(!mod_api::__private::listened(
                mod_api::HookId::AddServerReloadListeners
            ));
        })
    });
    std::fs::remove_dir_all(&root).unwrap();
    mod_api::testing::with_mods(
        vec![ModEntry::new("broken_mod", Box::new(Broken))].into(),
        || {
            with_game(|game| {
                let error = game.load_world(request(&root)).unwrap_err().to_string();
                assert!(
                    error.contains("unknown reload listener mistyped:listener"),
                    "{error}"
                );
                assert!(game.fns.store.functions.is_empty());
            })
        },
    );
    std::fs::remove_dir_all(root).unwrap();
}

struct RetainedData {
    live: Arc<Mutex<u64>>,
    reject: Arc<AtomicBool>,
}
impl ServerReloadListener for RetainedData {
    fn prepare(
        &self,
        context: &ServerReloadContext,
    ) -> Result<Box<dyn ReloadApply>, ReloadResourceError> {
        let mut value = 0;
        for id in context.list() {
            value += serde_json::from_slice::<u64>(context.read(id).unwrap())
                .map_err(|error| ReloadResourceError::new(id.clone(), error))?;
        }
        if self.reject.load(Ordering::Relaxed) {
            return Err(ReloadResourceError::new(
                Identifier::parse("reload_test:value").unwrap(),
                std::io::Error::other("rejected new machine data"),
            ));
        }
        let live = self.live.clone();
        Ok(<dyn ReloadApply>::new(move || {
            *live.lock().unwrap() = value
        }))
    }
}
#[mod_api::__subscriptions]
impl Hooks for RetainedData {
    fn add_server_reload_listeners(
        &self,
        _host: &dyn Host,
        event: &mut AddServerReloadListenersEvent<'_>,
    ) {
        let key = ListenerKey::<Self>::new(Identifier::parse("reload_test:kept").unwrap());
        event
            .listeners
            .add_retained_listener(
                &key,
                FileToIdConverter::json("machines").unwrap(),
                Arc::new(Self {
                    live: self.live.clone(),
                    reject: self.reject.clone(),
                }),
            )
            .unwrap();
    }
}

#[test]
#[ignore = "needs prepared resources"]
fn server_reload_retains_listeners_for_other_hooks() {
    let root = std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("server-reload-retained-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let manifest = "id = \"reload_test\"\nname = \"Reload Test\"\nversion = \"0.1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n";
    let package = root.join("reload_test.hmod");
    write(
        &package,
        horizon_mods::Package::write_files(
            manifest,
            &[("data/reload_test/machines/value.json", b"1")],
        )
        .unwrap(),
    );
    let package_manifest = horizon_mods::Package::read(&package).unwrap().manifest;
    let content = horizon_mods::Content::read(&package, &package_manifest).unwrap();
    let resources = root.join("resources");
    let base = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    horizon_client::mod_content::build(
        &base,
        Path::new(""),
        &[horizon_client::mod_content::ContentMod {
            id: "reload_test".into(),
            content,
            jars: Vec::new(),
        }],
        &resources,
    )
    .unwrap();
    let world = root.join("world");
    let pack = world.join("datapacks/test");
    write(&pack.join("pack.mcmeta"), "{}");
    let data = pack.join("data/reload_test/machines/value.json");
    write(&data, "2");
    write(
        &world.join("datapacks.json"),
        r#"{"enabled":["file/test"],"disabled":[]}"#,
    );
    let live = Arc::new(Mutex::new(0));
    let reject = Arc::new(AtomicBool::new(false));
    let mods = vec![ModEntry::new(
        "reload_test",
        Box::new(RetainedData {
            live: live.clone(),
            reject: reject.clone(),
        }),
    )]
    .into();
    let key = ListenerKey::<RetainedData>::new(Identifier::parse("reload_test:kept").unwrap());
    let wrong_type =
        ListenerKey::<CheckListener>::new(Identifier::parse("reload_test:kept").unwrap());
    mod_api::testing::with_mods(mods, || {
        with_game(|game| {
            assert!(game.native_reload_resources().listener(&key).is_none());
            game.options.resources = resources;
            game.load_world(request(&world)).unwrap();
            let kept = game.native_reload_resources().listener(&key).unwrap();
            assert_eq!(*kept.live.lock().unwrap(), 2);
            assert!(Arc::ptr_eq(
                &kept,
                &game.native_reload_resources().listener(&key).unwrap()
            ));
            assert!(
                game.native_reload_resources()
                    .listener(&wrong_type)
                    .is_none()
            );
            let before = Arc::as_ptr(&kept);
            write(&data, "3");
            assert!(game.run_command("reload"));
            let kept = game.native_reload_resources().listener(&key).unwrap();
            assert_eq!(*kept.live.lock().unwrap(), 3);
            assert_ne!(
                Arc::as_ptr(&kept),
                before,
                "each reload retains fresh objects"
            );
            write(&data, "4");
            reject.store(true, Ordering::Relaxed);
            assert!(!game.run_command("reload"));
            let kept = game.native_reload_resources().listener(&key).unwrap();
            assert_eq!(
                *kept.live.lock().unwrap(),
                3,
                "a failed reload keeps the last successful store"
            );
        })
    });
    std::fs::remove_dir_all(root).unwrap();
}

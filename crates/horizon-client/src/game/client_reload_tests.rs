use super::*;
use mod_api::{
    ClientReloadContext, ClientReloadListener, ClientReloadListeners, FileToIdConverter,
    Identifier, ReloadApply, ReloadResourceError,
};
use std::{path::PathBuf, sync::Arc};

fn id(value: &str) -> Identifier {
    Identifier::parse(value).unwrap()
}
fn hats() -> FileToIdConverter {
    FileToIdConverter::json("hats").unwrap()
}

/// A listener recording the context it prepared and applying to a shared log.
struct HatListener {
    seen: std::sync::Mutex<Vec<String>>,
    applied: Arc<std::sync::Mutex<Vec<String>>>,
}
impl ClientReloadListener for HatListener {
    fn prepare(
        &self,
        context: &ClientReloadContext,
    ) -> Result<Box<dyn ReloadApply>, ReloadResourceError> {
        let seen: Vec<String> = context
            .list()
            .filter_map(|id| {
                context
                    .read(id)
                    .map(|bytes| format!("{}={}", id, String::from_utf8_lossy(bytes)))
            })
            .collect();
        self.seen.lock().unwrap().extend(seen);
        let applied = self.applied.clone();
        Ok(<dyn ReloadApply>::new(move || {
            applied.lock().unwrap().push("hats".into());
        }))
    }
}

/// A temp root with one asset under `assets/test_mod/hats/`; each test gets
/// its own, so parallel runs write to different folders.
fn root_with_asset(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "horizon-client-reload-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let asset = dir.join("assets/test_mod/hats/top.json");
    std::fs::create_dir_all(asset.parent().unwrap()).unwrap();
    std::fs::write(asset, bytes).unwrap();
    dir
}

#[test]
fn stage_reads_the_merged_assets_and_apply_publishes_after_all_prepares() {
    let root = root_with_asset("merged", b"\"top\"");
    let applied = Arc::new(std::sync::Mutex::new(Vec::new()));
    let listener = Arc::new(HatListener {
        seen: std::sync::Mutex::new(Vec::new()),
        applied: applied.clone(),
    });
    let mut listeners = <mod_api::ClientListenerRegistry as mod_api::__private::game::ClientReloadListenersOrder>::new();
    listeners
        .add_listener(id("test_mod:hats"), hats(), listener.clone())
        .unwrap();
    let prepared = stage_client_reload(&root, listeners).unwrap();
    assert_eq!(
        listener.seen.lock().unwrap().as_slice(),
        ["test_mod:top=\"top\""]
    );
    assert!(applied.lock().unwrap().is_empty(), "nothing published yet");
    apply_client_reload(prepared);
    assert_eq!(applied.lock().unwrap().as_slice(), ["hats"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_listener_error_keeps_every_previous_state() {
    struct Failing;
    #[derive(Debug)]
    struct NotAHat;
    impl std::fmt::Display for NotAHat {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "not a hat")
        }
    }
    impl std::error::Error for NotAHat {}
    impl ClientReloadListener for Failing {
        fn prepare(
            &self,
            _: &ClientReloadContext,
        ) -> Result<Box<dyn ReloadApply>, ReloadResourceError> {
            Err(ReloadResourceError::new(id("test_mod:broken"), NotAHat))
        }
    }
    let root = root_with_asset("failing", b"{}");
    let mut listeners = <mod_api::ClientListenerRegistry as mod_api::__private::game::ClientReloadListenersOrder>::new();
    listeners
        .add_listener(id("test_mod:failing"), hats(), Arc::new(Failing))
        .unwrap();
    let Err(error) = stage_client_reload(&root, listeners) else {
        panic!("a failing listener stops the whole reload")
    };
    assert_eq!(
        error.to_string(),
        "reload listener test_mod:failing: test_mod:broken: not a hat"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_converted_file_outside_the_pack_is_absent_not_empty() {
    let root = root_with_asset("absent", b"{}");
    let listener = Arc::new(HatListener {
        seen: std::sync::Mutex::new(Vec::new()),
        applied: Arc::new(std::sync::Mutex::new(Vec::new())),
    });
    let mut listeners = <mod_api::ClientListenerRegistry as mod_api::__private::game::ClientReloadListenersOrder>::new();
    listeners
        .add_listener(id("test_mod:hats"), hats(), listener.clone())
        .unwrap();
    stage_client_reload(&root, listeners).unwrap();
    assert_eq!(
        listener.seen.lock().unwrap().as_slice(),
        ["test_mod:top={}"],
        "the staged root holds only the merged asset"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The initial load runs the client reload listeners: the load itself
/// registers and stages them against the real merged root (here the direct
/// `--world` path; the title screen's loader thread runs the same code), and
/// the applies it delivers run once the game exists — before any F3+T
/// reload.
#[test]
#[ignore = "needs prepared resources"]
fn initial_load_stages_and_applies_client_reload_listeners() {
    use crate::game::{Progress, load_resources, native_entity_refs::tests::with_game};
    use mod_api::testing::with_mods;
    use mod_api::{self, ModEntry};
    use mod_api::{AddClientReloadListenersEvent, ClientHost, Hooks};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// Counts the real root's own `post_effect` assets, like a mod reading
    /// vanilla files on the first load.
    struct StartupProbe {
        applied: Arc<AtomicBool>,
        count: Arc<AtomicUsize>,
    }
    impl ClientReloadListener for StartupProbe {
        fn prepare(
            &self,
            context: &ClientReloadContext,
        ) -> Result<Box<dyn ReloadApply>, ReloadResourceError> {
            let seen = context
                .list()
                .filter(|id| id.namespace() == "minecraft")
                .count();
            let applied = self.applied.clone();
            let count = self.count.clone();
            Ok(<dyn ReloadApply>::new(move || {
                count.fetch_max(seen, Ordering::SeqCst);
                applied.store(true, Ordering::SeqCst);
            }))
        }
    }
    #[mod_api::__subscriptions]
    impl Hooks for StartupProbe {
        fn add_client_reload_listeners(
            &self,
            _host: &dyn ClientHost,
            event: &mut AddClientReloadListenersEvent<'_>,
        ) {
            event
                .listeners
                .add_listener(
                    id("client_test:startup"),
                    FileToIdConverter::json("post_effect").unwrap(),
                    Arc::new(StartupProbe {
                        applied: self.applied.clone(),
                        count: self.count.clone(),
                    }),
                )
                .unwrap();
        }
    }

    let applied = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicUsize::new(0));
    let mods = vec![ModEntry::new(
        "client_test",
        Box::new(StartupProbe {
            applied: applied.clone(),
            count: count.clone(),
        }),
    )]
    .into();
    with_mods(mods, || {
        with_game(|game| {
            let root = crate::game::packs::startup_root(
                &game.options.resources,
                &game.settings.resource_packs,
                &[],
                &game.options.mods,
            )
            .or_else(|error| {
                log::warn!("Horizon: resource packs not applied: {error}");
                crate::game::mods::content_root(&game.options.resources, &game.options.mods)
            })
            .unwrap();
            let mut loaded = load_resources(
                root,
                None,
                &game.options.resources,
                &game.settings,
                std::time::Instant::now(),
                false,
                crate::game::execution_role::ExecutionRole::Client,
                &Progress::new(),
                &mut |_, _| {},
            )
            .expect("initial load");
            assert!(
                !loaded.client_applies.is_empty(),
                "the load itself registered and staged the listener"
            );
            assert!(
                !applied.load(Ordering::SeqCst),
                "nothing applied before handoff"
            );
            // The same call `start_game` makes once the game owns the assets.
            apply_client_reload(std::mem::take(&mut loaded.client_applies));
            assert!(applied.load(Ordering::SeqCst), "the first load applies");
            assert!(
                count.load(Ordering::SeqCst) >= 5,
                "the initial stage read the real root's own assets"
            );
        });
    });
}

/// A listener registered through the real dispatch, staged against a real
/// merged root (the game's own resources with a dev mod and a resource pack).
#[test]
#[ignore = "needs prepared resources"]
fn client_reload_reads_real_assets_with_pack_overrides() {
    use crate::game::native_entity_refs::tests::with_game;
    use mod_api::testing::with_mods;
    use mod_api::{self, ModEntry};
    use mod_api::{AddClientReloadListenersEvent, ClientHost, Hooks};
    use std::{
        collections::BTreeSet,
        sync::Mutex,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    let dir = std::env::temp_dir().join(format!("client-reload-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mod_dir = dir.join("mod");
    let pack_dir = dir.join("packs/override");
    let asset = "assets/client_test/hats/top.json";
    for root in [&mod_dir, &pack_dir] {
        std::fs::create_dir_all(root.join(asset).parent().unwrap()).unwrap();
    }
    std::fs::write(
        mod_dir.join("horizon-mod.toml"),
        "id = \"client_test\"\nname = \"Client test\"\nversion = \"1\"\napi = \"0.2\"\ncapabilities = [\"registries\"]\n",
    )
    .unwrap();
    std::fs::write(mod_dir.join(asset), b"mod").unwrap();
    std::fs::write(pack_dir.join(asset), b"pack").unwrap();
    std::fs::write(
        pack_dir.join("pack.mcmeta"),
        br#"{"pack":{"description":"test","pack_format":97}}"#,
    )
    .unwrap();

    struct HatListener {
        live: Arc<Mutex<Vec<String>>>,
        reject: Arc<AtomicBool>,
        vanilla: Arc<AtomicUsize>,
    }
    impl ClientReloadListener for HatListener {
        fn prepare(
            &self,
            context: &ClientReloadContext,
        ) -> Result<Box<dyn ReloadApply>, ReloadResourceError> {
            if self.reject.load(Ordering::Relaxed) {
                return Err(ReloadResourceError::new(
                    id("client_test:top"),
                    std::io::Error::other("rejected new hats"),
                ));
            }
            let top = id("client_test:top");
            let value = context
                .read(&top)
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .unwrap_or_default();
            let live = self.live.clone();
            Ok(<dyn ReloadApply>::new(move || {
                live.lock().unwrap().push(value)
            }))
        }
    }
    #[mod_api::__subscriptions]
    impl Hooks for HatListener {
        fn add_client_reload_listeners(
            &self,
            _host: &dyn ClientHost,
            event: &mut AddClientReloadListenersEvent<'_>,
        ) {
            event
                .listeners
                .add_listener(
                    id("client_test:hats"),
                    hats(),
                    Arc::new(HatListener {
                        live: self.live.clone(),
                        reject: self.reject.clone(),
                        vanilla: self.vanilla.clone(),
                    }),
                )
                .unwrap();
            // The real root's own assets, through a directory vanilla ships.
            event
                .listeners
                .add_listener(
                    id("client_test:vanilla"),
                    FileToIdConverter::json("post_effect").unwrap(),
                    Arc::new(VanillaProbe(self.vanilla.clone())),
                )
                .unwrap();
        }
    }

    /// Counts vanilla's own `post_effect` files in the same merged view.
    struct VanillaProbe(Arc<AtomicUsize>);
    impl ClientReloadListener for VanillaProbe {
        fn prepare(
            &self,
            context: &ClientReloadContext,
        ) -> Result<Box<dyn ReloadApply>, ReloadResourceError> {
            let count = context
                .list()
                .filter(|id| id.namespace() == "minecraft")
                .count();
            let seen = self.0.clone();
            Ok(<dyn ReloadApply>::new(move || {
                seen.fetch_max(count, Ordering::SeqCst);
            }))
        }
    }

    let live = Arc::new(Mutex::new(Vec::new()));
    let reject = Arc::new(AtomicBool::new(false));
    let vanilla = Arc::new(AtomicUsize::new(0));
    let mods = vec![ModEntry::new(
        "client_test",
        Box::new(HatListener {
            live: live.clone(),
            reject: reject.clone(),
            vanilla: vanilla.clone(),
        }),
    )]
    .into();
    with_mods(mods, || {
        with_game(|game| {
            let resources = game.options.resources.clone();
            let enabled = BTreeSet::new();
            let mut sources = crate::game::mods::ModsState {
                dev: vec![mod_dir.clone()],
                ..Default::default()
            };
            sources.select(&std::env::current_dir().unwrap(), &enabled);
            // The merged root without `startup_root`'s process-wide mod
            // registrations, which a test must not freeze for the others
            // (`brewing`'s mod effects among them).
            let merged = |packs: &[horizon_client::packs::Pack]| {
                let base = crate::game::mods::content_overlay(
                    &resources,
                    &sources,
                    &horizon_client::logging::game_home().join("jars"),
                )
                .unwrap()
                .root;
                horizon_client::packs::resource_root(&base, packs, &crate::game::packs::cache_dir())
                    .unwrap()
            };
            // The mod's packaged asset over nothing, with the real root's own
            // assets in the same view.
            let root = merged(&[]);
            let prepared =
                stage_client_reload(&root, game.register_client_reload_listeners()).unwrap();
            assert!(live.lock().unwrap().is_empty(), "nothing published yet");
            apply_client_reload(prepared);
            assert_eq!(live.lock().unwrap().as_slice(), ["mod"]);
            assert!(
                vanilla.load(Ordering::SeqCst) >= 5,
                "the real root's own post_effect assets join the view"
            );
            // A selected resource pack overrides the mod's packaged asset.
            let packs = horizon_client::packs::folder(
                &dir.join("packs"),
                horizon_client::packs::RESOURCE_FORMAT,
            );
            assert_eq!(packs.len(), 1);
            let root = merged(&packs);
            let prepared =
                stage_client_reload(&root, game.register_client_reload_listeners()).unwrap();
            apply_client_reload(prepared);
            assert_eq!(live.lock().unwrap().as_slice(), ["mod", "pack"]);
            // A rejected prepare aborts the reload and keeps the previous state.
            reject.store(true, Ordering::Relaxed);
            let root = merged(&[]);
            let Err(error) = stage_client_reload(&root, game.register_client_reload_listeners())
            else {
                panic!("a failing listener stops the whole reload")
            };
            assert!(error.to_string().contains("client_test:hats"));
            assert_eq!(
                live.lock().unwrap().as_slice(),
                ["mod", "pack"],
                "the refused reload publishes nothing"
            );
        });
    });
    std::fs::remove_dir_all(dir).unwrap();
}

/// `client_setup` reaches every listener with the game's client host, one call per
/// firing; `run` fires it once per process, so a listener sees one start.
#[test]
#[ignore = "needs prepared resources"]
fn client_setup_reaches_each_listener_once_per_firing() {
    use crate::game::native_entity_refs::tests::with_game;
    use mod_api::testing::with_mods;
    use mod_api::{ClientHost, Hooks, ModEntry};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Setup(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for Setup {
        fn client_setup(&self, _host: &mut dyn ClientHost, _event: &mut mod_api::ClientSetupEvent) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let (first, second) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mods = vec![
        ModEntry::new("setup_a", Box::new(Setup(first.clone()))),
        ModEntry::new("setup_b", Box::new(Setup(second.clone()))),
    ]
    .into();
    with_mods(mods, || {
        with_game(|game| {
            game.native_client_setup();
        });
    });
    assert_eq!(
        (first.load(Ordering::SeqCst), second.load(Ordering::SeqCst)),
        (1, 1)
    );
}

/// The process's client start is a client that is not a splitscreen guest: a dedicated
/// server has no client, and a guest's `run` is another local player of the same process.
#[test]
#[ignore = "needs prepared resources"]
fn only_the_first_local_client_is_the_client_start() {
    use crate::game::execution_role::ExecutionRole;
    use crate::game::native_entity_refs::tests::with_game;

    with_game(|game| {
        game.options.execution_role = ExecutionRole::Client;
        game.options.split_guest = false;
        assert!(game.options.is_client_start());
        game.options.split_guest = true;
        assert!(!game.options.is_client_start());
        game.options.split_guest = false;
        game.options.execution_role = ExecutionRole::DedicatedServer;
        assert!(!game.options.is_client_start());
    });
}

/// A world's moments reach a listener once each and in order: the server's start
/// (`server_about_to_start`, then `server_started` where `WorldOpenReady` was, only once
/// per open and not for a failed one), its stop (`server_stopping`, then the final close),
/// and the client's own close (`client_stopping`).
#[test]
#[ignore = "needs prepared resources"]
fn the_lifecycle_moments_fire_once_each_in_order() {
    use crate::game::native_entity_refs::tests::with_game;
    use mod_api::testing::with_mods;
    use mod_api::{ClientHost, Hooks, ModEntry, ServerHost};
    use std::sync::Mutex;

    struct Log(Arc<Mutex<Vec<String>>>);
    #[mod_api::__subscriptions]
    impl Hooks for Log {
        fn server_about_to_start(
            &self,
            _host: &mut dyn ServerHost,
            e: &mut mod_api::ServerAboutToStartEvent,
        ) {
            self.0
                .lock()
                .unwrap()
                .push(format!("about_to_start dedicated={}", e.dedicated));
        }
        fn server_starting(
            &self,
            _host: &mut dyn ServerHost,
            e: &mut mod_api::ServerStartingEvent,
        ) {
            self.0
                .lock()
                .unwrap()
                .push(format!("starting dedicated={}", e.dedicated));
        }
        fn server_stopped(&self, _host: &mut dyn ServerHost, e: &mut mod_api::ServerStoppedEvent) {
            self.0
                .lock()
                .unwrap()
                .push(format!("stopped dedicated={}", e.dedicated));
        }
        fn server_started(&self, _host: &mut dyn ServerHost, e: &mut mod_api::ServerStartedEvent) {
            self.0
                .lock()
                .unwrap()
                .push(format!("started dedicated={}", e.dedicated));
        }
        fn server_stopping(
            &self,
            _host: &mut dyn ServerHost,
            e: &mut mod_api::ServerStoppingEvent,
        ) {
            self.0
                .lock()
                .unwrap()
                .push(format!("stopping dedicated={}", e.dedicated));
        }
        fn world_lifecycle(
            &self,
            _host: &mut dyn ServerHost,
            event: &mut mod_api::WorldLifecycleEvent<'_>,
        ) {
            self.0.lock().unwrap().push(format!("{:?}", event.phase));
        }
        fn client_stopping(
            &self,
            _host: &mut dyn ClientHost,
            _event: &mut mod_api::ClientStoppingEvent,
        ) {
            self.0.lock().unwrap().push("client_stopping".into());
        }
        fn client_stopped(
            &self,
            _host: &mut dyn ClientHost,
            _event: &mut mod_api::ClientStoppedEvent,
        ) {
            self.0.lock().unwrap().push("client_stopped".into());
        }
        fn game_shutting_down(
            &self,
            host: mod_api::HostSide<'_>,
            _event: &mut mod_api::GameShuttingDownEvent,
        ) {
            let side = match host {
                mod_api::HostSide::Client(_) => "client",
                mod_api::HostSide::Server(_) => "server",
            };
            self.0.lock().unwrap().push(format!("shutting_down {side}"));
        }
    }

    let log = Arc::new(Mutex::new(Vec::new()));
    let mods = vec![ModEntry::new("log", Box::new(Log(log.clone())))].into();
    with_mods(mods, || {
        with_game(|game| {
            game.options.execution_role =
                crate::game::execution_role::ExecutionRole::DedicatedServer;
            game.native_lifecycle_world_closed();
            log.lock().unwrap().clear();
            game.native_server_about_to_start(true);
            game.native_server_starting(true);
            game.native_lifecycle_open_started();
            game.native_lifecycle_world_ready();
            game.native_lifecycle_world_ready();
            game.native_lifecycle.begin_close();
            game.native_server_stopping(true);
            game.native_lifecycle_world_closed();
            game.native_lifecycle_world_closed();
            game.native_server_stopped(true);
            game.native_game_shutting_down();
            game.options.execution_role = crate::game::execution_role::ExecutionRole::Client;
            game.native_client_stopping();
            game.native_client_stopped();
            game.native_game_shutting_down();
        });
    });
    assert_eq!(
        *log.lock().unwrap(),
        [
            "about_to_start dedicated=true",
            "starting dedicated=true",
            "started dedicated=true",
            "stopping dedicated=true",
            "WorldCloseFinal",
            "stopped dedicated=true",
            "shutting_down server",
            "client_stopping",
            "client_stopped",
            "shutting_down client",
        ]
    );
}

/// `get_arm_pose`'s answer is the pose of that hand's stack; a hand it does not answer for,
/// or no listener at all, leaves the item's own (declared) pose, which a vanilla stick lacks.
#[test]
#[ignore = "needs prepared resources"]
fn an_arm_pose_answer_replaces_the_pose_the_item_gives() {
    use crate::game::native_entity_refs::tests::with_game;
    use mod_api::testing::with_mods;
    use mod_api::{ArmPose, ClientHost, GetArmPoseEvent, Hand, Hooks, ModEntry};

    struct Horn;
    #[mod_api::__subscriptions]
    impl Hooks for Horn {
        fn get_arm_pose(
            &self,
            _: &dyn ClientHost,
            event: &mut GetArmPoseEvent<'_>,
        ) -> Option<ArmPose> {
            (event.hand == Hand::Main).then_some(ArmPose::TootHorn)
        }
    }

    let stick = horizon_client::items::ItemStack::new("minecraft:stick", 1);
    let stacks = [Some(&stick), Some(&stick)];
    with_game(|game| {
        assert_eq!(game.held_arm_poses(1, stacks), [None, None]);
        let mods = vec![ModEntry::new("horn", Box::new(Horn))].into();
        with_mods(mods, || {
            assert_eq!(
                game.held_arm_poses(1, stacks),
                [Some(ArmPose::TootHorn), None]
            );
        });
    });
}

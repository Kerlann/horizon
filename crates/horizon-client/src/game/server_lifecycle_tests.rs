use crate::game::{execution_role::ExecutionRole, native_entity_refs::tests::with_game};
use mod_api::{Hooks, ModEntry, ServerHost, testing::with_mods};
use std::sync::{Arc, Mutex};

/// Exercise the real stop/save paths, not just direct calls to hook dispatch.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn every_started_server_stops_and_saves_even_when_startup_or_ticks_fail() {
    struct Log(
        Arc<Mutex<Vec<&'static str>>>,
        Arc<std::sync::atomic::AtomicBool>,
    );
    #[mod_api::__subscriptions]
    impl Hooks for Log {
        fn server_starting(&self, _: &mut dyn ServerHost, _: &mut mod_api::ServerStartingEvent) {
            self.0.lock().unwrap().push("starting");
        }
        fn server_stopping(&self, _: &mut dyn ServerHost, _: &mut mod_api::ServerStoppingEvent) {
            self.0.lock().unwrap().push("stopping");
            if self.1.load(std::sync::atomic::Ordering::Relaxed) {
                panic!("stopping listener failed");
            }
        }
        fn world_saved(&self, _: &mut dyn ServerHost, _: &mut mod_api::WorldSavedEvent) {
            self.0.lock().unwrap().push("saved");
        }
        fn server_stopped(&self, _: &mut dyn ServerHost, _: &mut mod_api::ServerStoppedEvent) {
            self.0.lock().unwrap().push("stopped");
        }
    }
    let log = Arc::new(Mutex::new(Vec::new()));
    let panic_stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));
    with_mods(
        vec![ModEntry::new(
            "log",
            Box::new(Log(log.clone(), panic_stopping.clone())),
        )]
        .into(),
        || {
            for dedicated in [false, true] {
                for case in [
                    "normal",
                    "startup-error",
                    "tick-error",
                    "panic",
                    "save-error",
                    "unopened",
                    "stopping-panic",
                    "before-start",
                ] {
                    log.lock().unwrap().clear();
                    panic_stopping.store(
                        case == "stopping-panic",
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    with_game(|game| {
                        game.options.execution_role = if dedicated {
                            ExecutionRole::DedicatedServer
                        } else {
                            ExecutionRole::IntegratedServer
                        };
                        let root = std::env::temp_dir().join(format!(
                            "server-lifecycle-{}-{dedicated}-{case}",
                            std::process::id()
                        ));
                        std::fs::create_dir_all(&root).unwrap();
                        game.world_root = root.clone();
                        game.save = Arc::new(horizon_client::save::SaveDir::new(
                            root.clone(),
                            horizon_client::save::LevelShape::OVERWORLD,
                        ));
                        game.level.name = "lifecycle final save".into();
                        if case != "before-start" {
                            game.session_lock =
                                Some(horizon_client::save::SessionLock::acquire(&root).unwrap());
                            game.server_world_adopted = case != "unopened";
                        }
                        if case == "unopened" {
                            // A failed open must not overwrite an existing, unreadable save.
                            std::fs::write(root.join("level.dat"), b"unadopted save").unwrap();
                        }
                        let result = game.run_server_lifecycle(dedicated, |game, began| {
                            if case == "before-start" {
                                return Err("before startup".into());
                            }
                            *began = true;
                            game.native_server_about_to_start(dedicated);
                            game.native_server_starting(dedicated);
                            game.native_lifecycle_open_started();
                            if matches!(case, "startup-error" | "unopened") {
                                return Err("failed init".into());
                            }
                            game.native_lifecycle_world_ready();
                            match case {
                                "tick-error" => Err("failed tick".into()),
                                "panic" => panic!("failed tick"),
                                "save-error" => {
                                    let file = root.join("not-a-directory");
                                    std::fs::write(&file, b"occupied").unwrap();
                                    game.world_root = file;
                                    Ok(())
                                }
                                _ => Ok(()),
                            }
                        });
                        assert_eq!(result.is_ok(), case == "normal", "{dedicated} {case}");
                        let expected: &[&str] = match case {
                            "before-start" => &[],
                            "unopened" | "save-error" => &["starting", "stopping", "stopped"],
                            _ => &["starting", "stopping", "saved", "stopped"],
                        };
                        assert_eq!(*log.lock().unwrap(), expected, "{dedicated} {case}");
                        assert!(game.session_lock.is_none(), "storage released after {case}");
                        if case == "unopened" {
                            assert_eq!(
                                std::fs::read(root.join("level.dat")).unwrap(),
                                b"unadopted save"
                            );
                        } else if !matches!(case, "save-error" | "before-start") {
                            let saved = horizon_client::save::Level::read(&root).unwrap();
                            assert_eq!(saved.name, "lifecycle final save");
                        }
                        std::fs::remove_dir_all(root).unwrap();
                    });
                }
            }
        },
    );
}

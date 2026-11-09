//! Two real shared-memory guest packages against the authoritative Game backend.
//! Opt in with the existing mod-testing feature and the explicitly built guest.
#![cfg(feature = "testing")]
use super::native_entity_command_lifecycle_tests::with_fixture;
use horizon_mod_package::manifest::Package;
use mod_api::*;
struct Observer;
#[mod_api::__subscriptions]
impl Hooks for Observer {
    fn entity_tick_pre(&self, host: &mut dyn ServerHost, _: &mut EntityTickPreEvent<'_>) -> Flow {
        let epoch = host.entity(71).unwrap().epoch();
        for sequence in 0..6 {
            assert_eq!(
                CommandId { epoch, sequence }.query(host),
                Err(CommandQueryError::UnknownOrExpired)
            );
        }
        Flow::Continue
    }
    fn entity_command_completed(
        &self,
        _: &mut dyn ServerHost,
        _: &mut EntityCommandCompletedEvent<'_>,
    ) {
        panic!("wasm issuer terminal broadcast to native observer");
    }
}
#[test]
#[ignore = "needs prepared resources plus HORIZON_SILENT_WASM and HORIZON_WASM_STD"]
fn silent_real_wasm_packages_keep_receipts_queries_and_terminal_events_isolated() {
    let fixture = std::env::var("HORIZON_SILENT_WASM")
        .expect("build tests/entity-silent-wasm with cargo horizon-mod build --wasm");
    let std_member =
        std::fs::read(std::env::var("HORIZON_WASM_STD").expect("build the pinned wasm-std member"))
            .unwrap();
    let module = Package::read_wasm(std::path::Path::new(&fixture)).unwrap();
    let original = Package::read(std::path::Path::new(&fixture))
        .unwrap()
        .manifest;
    let mut packages = Vec::new();
    let mut inventory = Vec::new();
    for id in ["silent_wasm_a", "silent_wasm_b"] {
        let mut manifest = original.clone();
        manifest.id = id.into();
        inventory.push(PackageInfo::new(
            id.into(),
            manifest.version.clone(),
            PackageKind::Code,
            Side::Both,
            vec![id.into()],
            Vec::new(),
        ));
        packages.push(horizon_mod_wasm::WasmPackage {
            manifest,
            module: module.clone(),
        });
    }
    let loaded = horizon_mod_wasm::load(&std_member, packages);
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    assert_eq!(loaded.mods.len(), 2);
    loaded
        .initialize(&inventory)
        .expect("actual guest registration");
    let mut mods = loaded.mods;
    mods.push(ModEntry::new("silent-wasm-observer", Box::new(Observer)));
    with_fixture(|game, _| {
        mod_api::testing::with_mods(mods.into(), || {
            game.tick_creature_entities().unwrap();
            let target = game.mobs.iter().find(|mob| mob.id == 72).unwrap();
            assert!(target.silent);
            assert!(
                target.no_gravity,
                "first guest reached B Pre after validating every own completion"
            );
            assert!(
                target.glowing,
                "second guest reached B Pre after validating every own completion"
            );
            let epoch = game.native_lifecycle.epoch().unwrap();
            for (issuer, ids) in [("silent_wasm_a", [0, 1, 4]), ("silent_wasm_b", [2, 3, 5])] {
                let _scope = mod_api::__private::enter(issuer, "test query");
                for sequence in 0..6 {
                    let status = CommandId { epoch, sequence }.query(game);
                    if ids.contains(&sequence) {
                        assert!(matches!(
                            status,
                            Ok(EntityCommandStatus::Completed(
                                EntityWriteCompletion::EntitySetSilent(CommandCompleted {
                                    outcome: Ok(true),
                                    ..
                                })
                            ))
                        ));
                    } else {
                        assert_eq!(status, Err(CommandQueryError::UnknownOrExpired));
                    }
                }
            }
        });
    });
}

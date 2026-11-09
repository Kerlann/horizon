//! Domain-level invalidation, compound issuer and transaction behavior.
use super::{Game, native_entity_commands::EntityWriterScope};
use horizon_client::{chunk::Chunk, mobs::Mob};
use horizon_core::geometry::Vec3;
use mod_api::*;
use std::sync::{Arc, Mutex};
type Events = Arc<Mutex<Vec<(String, EntityWriteCompletion)>>>;
struct Runtime(Events);
#[mod_api::__subscriptions]
impl Hooks for Runtime {
    fn entity_command_completed(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityCommandCompletedEvent<'_>,
    ) {
        assert_eq!(mod_api::crash::current_native_mod(), Some("silent-runtime"));
        let package = mod_api::crash::current_converted_call()
            .map_or_else(|| "native".to_owned(), |call| call.package.into_owned());
        let EntityWriteCompletion::EntitySetSilent(done) = event.completion;
        assert_eq!(
            done.id.query(host),
            Ok(EntityCommandStatus::Completed(event.completion.clone()))
        );
        self.0
            .lock()
            .unwrap()
            .push((package, event.completion.clone()));
    }
}
pub(super) fn with_fixture(f: impl FnOnce(&mut Game<'_>, &Events)) {
    let events = Events::default();
    mod_api::testing::with_mods(
        vec![ModEntry::new(
            "silent-runtime",
            Box::new(Runtime(events.clone())),
        )]
        .into(),
        || {
            super::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                for id in [71, 72] {
                    game.mobs.push(Mob::new(
                        id,
                        kind.clone(),
                        Vec3::new(f64::from(id - 70), 80.0, 0.5),
                        0.0,
                    ));
                }
                f(game, &events);
            });
        },
    );
}
pub(super) fn writer(game: &Game<'_>) -> EntityWriterScope {
    let (location, _) = game
        .entity_residency
        .borrow_mut()
        .server_admitted_location(71, |at| game.locate_server_resident_owner(at))
        .unwrap();
    assert!(!game.entity_read_actor_owns(71, location));
    let scope = game
        .begin_entity_writer(horizon_client::join_seq::residency::write_queue::Target {
            runtime_id: 71,
            location,
        })
        .unwrap();
    assert!(game.entity_read_actor_owns(71, location));
    scope
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn silent_converted_package_issuers_do_not_collapse_into_native_runtime() {
    with_fixture(|game, events| {
        let _native = mod_api::__private::enter("silent-runtime", "test");
        let packages = mod_api::crash::ConvertedPackages::new("silent-runtime", ["a", "b"]);
        let scope = writer(game);
        let target = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
        let mut ids = Vec::new();
        for package in ["a", "b"] {
            let _package = packages
                .package(package)
                .unwrap()
                .enter("pre", "test")
                .unwrap();
            if let Some(id) = ids.first() {
                assert_eq!(
                    game.entity_command_query(*id),
                    Err(CommandQueryError::UnknownOrExpired)
                );
            }
            let WriteReceipt::Pending(id) = game.entity_set_silent(target, true) else {
                panic!("foreign write did not queue");
            };
            ids.push(id);
        }
        // The native runtime's own scope is not either converted package.
        assert_eq!(
            game.entity_command_query(ids[0]),
            Err(CommandQueryError::UnknownOrExpired)
        );
        game.drain_entity_commands(scope.frame.turn);
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .map(|(package, _)| package.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        for (index, package) in ["a", "b"].into_iter().enumerate() {
            let _package = packages
                .package(package)
                .unwrap()
                .enter("query", "test")
                .unwrap();
            assert!(matches!(
                game.entity_command_query(ids[index]),
                Ok(EntityCommandStatus::Completed(_))
            ));
            assert_eq!(
                game.entity_command_query(ids[1 - index]),
                Err(CommandQueryError::UnknownOrExpired)
            );
        }
    });
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn silent_item_transaction_refuses_only_enqueue_before_allocating_id() {
    with_fixture(|game, events| {
        let _native = mod_api::__private::enter("silent-runtime", "test");
        let epoch = game.native_lifecycle.epoch().unwrap();
        let own = EntityRef::new(epoch, 71);
        let other = EntityRef::new(epoch, 72);
        let scope = writer(game);
        let result: Result<(), ()> = game.transaction(|host, _| {
            assert_eq!(own.set_silent(host, true), WriteReceipt::Applied(true));
            assert_eq!(
                other.set_silent(host, true),
                WriteReceipt::Refused(EntityWriteError::TransactionActive)
            );
            Ok(()) // Abort: direct/OWN entity writes are not item rollback.
        });
        assert_eq!(result, Ok(()));
        assert!(game.mobs.iter().find(|mob| mob.id == 71).unwrap().silent);
        assert!(!game.mobs.iter().find(|mob| mob.id == 72).unwrap().silent);
        assert!(events.lock().unwrap().is_empty());
        assert!(
            game.entity_cached_row(other)
                .is_none_or(|row| row.capture.pending_overlay.is_none())
        );
        let WriteReceipt::Pending(id) = game.entity_set_silent(other, true) else {
            panic!("retry after transaction did not queue");
        };
        assert_eq!(id.sequence, 0, "refusal allocated no id");
        game.drain_entity_commands(scope.frame.turn);
        assert_eq!(events.lock().unwrap().len(), 1);
    });
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn silent_apply_rejects_removed_or_reused_generation_and_preserves_overlay_on_capture() {
    for replace in [false, true] {
        with_fixture(|game, events| {
            let _native = mod_api::__private::enter("silent-runtime", "test");
            let scope = writer(game);
            let other = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
            let WriteReceipt::Pending(id) = game.entity_set_silent(other, true) else {
                panic!("foreign write did not queue");
            };
            let (location, _) = game
                .entity_residency
                .borrow_mut()
                .server_admitted_location(72, |at| game.locate_server_resident_owner(at))
                .unwrap();
            let target = horizon_client::join_seq::residency::write_queue::Target {
                runtime_id: 72,
                location,
            };
            assert!(game.publish_silent_target(target, EntityReadCapturePhase::BeforeCallback));
            game.with_entity_read_publication(other, |row| {
                use super::native_read_dependencies::EntityReadDependency;
                assert_eq!(
                    row.pending_dependency(&[EntityReadDependency::Nbt]),
                    Some(id)
                );
                assert_eq!(row.pending_dependency(&[]), None);
                assert_eq!(row.capture.pending_overlay, Some(id));
                assert_eq!(row.base.is_silent, Some(true));
                let super::native_entity_read_cache::NativeReadDetails::Mob { living, mob, .. } =
                    &row.details
                else {
                    panic!("real mob family not captured");
                };
                assert_eq!(living.capture.pending_overlay, Some(id));
                assert_eq!(mob.capture.pending_overlay, Some(id));
                Some(())
            })
            .unwrap();
            game.mobs.retain(|mob| mob.id != 72);
            if replace {
                let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                game.mobs
                    .push(Mob::new(72, kind, Vec3::new(4.0, 80.0, 0.5), 0.0));
            }
            game.drain_entity_commands(scope.frame.turn);
            let expected = if replace {
                EntityWriteError::GenerationChanged
            } else {
                EntityWriteError::Removed
            };
            let completion = EntityWriteCompletion::EntitySetSilent(CommandCompleted {
                id,
                outcome: Err(expected),
            });
            assert_eq!(
                game.entity_command_query(id),
                Ok(EntityCommandStatus::Completed(completion.clone()))
            );
            assert_eq!(events.lock().unwrap()[0].1, completion);
            assert!(game.mobs.iter().all(|mob| !mob.silent));
        });
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn silent_parked_mob_applies_without_selecting_its_dimension() {
    with_fixture(|game, events| {
        game.mobs.retain(|mob| mob.id != 72);
        game.enter_level(super::Dim::Nether).unwrap();
        let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
        game.mobs
            .push(Mob::new(72, kind, Vec3::new(2.0, 80.0, 0.5), 0.0));
        game.enter_level(super::Dim::Overworld).unwrap();
        let _native = mod_api::__private::enter("silent-runtime", "test");
        let scope = writer(game);
        let other = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
        let dimension = game.dim.key();
        let WriteReceipt::Pending(id) = game.entity_set_silent(other, true) else {
            panic!("parked write did not queue");
        };
        assert_eq!(game.entity_is_silent(other), Some(true));
        game.drain_entity_commands(scope.frame.turn);
        assert_eq!(game.dim.key(), dimension);
        assert!(
            game.levels
                .parked_of("minecraft:the_nether")
                .unwrap()
                .mobs
                .iter()
                .find(|mob| mob.id == 72)
                .unwrap()
                .silent
        );
        assert_eq!(
            game.entity_cached_row(other)
                .unwrap()
                .capture
                .pending_overlay,
            None
        );
        assert_eq!(
            events.lock().unwrap()[0].1,
            EntityWriteCompletion::EntitySetSilent(CommandCompleted {
                id,
                outcome: Ok(true)
            })
        );
    });
}

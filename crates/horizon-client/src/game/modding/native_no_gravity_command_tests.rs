//! Actual stored scalar writes and their shared FIFO/issuer completion boundary.
use super::Game;
use horizon_client::{chunk::Chunk, mobs::Mob};
use horizon_core::geometry::Vec3;
use mod_api::*;
use std::sync::{Arc, Mutex};

type Events = Arc<Mutex<Vec<EntityWriteCompletion>>>;
struct Monitor(Events);
#[mod_api::__subscriptions]
impl Hooks for Monitor {
    fn entity_tick_listened(&self, _: &mut dyn ServerHost, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }
    fn entity_command_completed(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityCommandCompletedEvent<'_>,
    ) {
        let id = match event.completion {
            EntityWriteCompletion::EntitySetSilent(done)
            | EntityWriteCompletion::EntitySetNoGravity(done) => done.id,
        };
        assert_eq!(
            id.query(host),
            Ok(EntityCommandStatus::Completed(event.completion.clone()))
        );
        self.0.lock().unwrap().push(event.completion.clone());
    }
}
fn fixture(f: impl FnOnce(&mut Game<'_>, &Events)) {
    let events = Events::default();
    mod_api::testing::with_mods(
        vec![ModEntry::new(
            "gravity-writer",
            Box::new(Monitor(events.clone())),
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

struct TickWriter(Arc<Mutex<Vec<&'static str>>>);
#[mod_api::__subscriptions]
impl Hooks for TickWriter {
    fn entity_tick_listened(&self, _: &mut dyn ServerHost, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }
    fn entity_tick_pre(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityTickPreEvent<'_>,
    ) -> Flow {
        self.0
            .lock()
            .unwrap()
            .push(if event.entity == 71 { "pre A" } else { "pre B" });
        let actor = host.entity(event.entity).unwrap();
        if event.entity == 71 {
            assert_eq!(
                actor.set_no_gravity(host, true),
                WriteReceipt::Applied(true)
            );
            assert_eq!(actor.no_gravity(host), Some(true));
            let other = host.entity(72).unwrap();
            assert!(matches!(
                other.set_no_gravity(host, true),
                WriteReceipt::Pending(_)
            ));
        } else {
            assert_eq!(
                *self.0.lock().unwrap(),
                ["pre A", "post A", "complete", "pre B"]
            );
            assert_eq!(actor.no_gravity(host), Some(true));
        }
        Flow::Continue
    }
    fn entity_tick_post(&self, _: &mut dyn ServerHost, event: &mut EntityTickPostEvent<'_>) {
        self.0.lock().unwrap().push(if event.entity == 71 {
            "post A"
        } else {
            "post B"
        });
    }
    fn entity_command_completed(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityCommandCompletedEvent<'_>,
    ) {
        let EntityWriteCompletion::EntitySetNoGravity(done) = event.completion else {
            panic!("unexpected operation")
        };
        assert_eq!(done.outcome, Ok(true));
        assert_eq!(
            done.id.query(host),
            Ok(EntityCommandStatus::Completed(event.completion.clone()))
        );
        self.0.lock().unwrap().push("complete");
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn gravity_actual_actor_loop_applies_after_writer_and_before_next_actor() {
    let order = Arc::new(Mutex::new(Vec::new()));
    mod_api::testing::with_mods(
        vec![ModEntry::new(
            "gravity-tick-writer",
            Box::new(TickWriter(order.clone())),
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
                    let mut mob = Mob::new(
                        id,
                        kind.clone(),
                        Vec3::new(f64::from(id - 70), 80.0, 0.5),
                        0.0,
                    );
                    mob.no_ai = true;
                    game.mobs.push(mob);
                }
                game.tick_creature_entities().unwrap();
                assert_eq!(
                    *order.lock().unwrap(),
                    ["pre A", "post A", "complete", "pre B", "post B"]
                );
                assert!(game.mobs.iter().all(|m| m.no_gravity));
            });
        },
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn gravity_direct_own_transaction_and_ordered_foreign_patches_share_the_real_fifo() {
    fixture(|game, events| {
        let _issuer = mod_api::__private::enter("gravity-writer", "test");
        let epoch = game.native_lifecycle.epoch().unwrap();
        let own = EntityRef::new(epoch, 71);
        let other = EntityRef::new(epoch, 72);
        assert_eq!(
            other.set_no_gravity(game, true),
            WriteReceipt::Applied(true)
        );
        assert!(game.mobs.iter().find(|m| m.id == 72).unwrap().no_gravity);
        assert_eq!(
            other.set_no_gravity(game, false),
            WriteReceipt::Applied(true)
        );
        let scope = super::native_entity_command_lifecycle_tests::writer(game);
        let result: Result<(), ()> = game.transaction(|host, _| {
            assert_eq!(own.set_no_gravity(host, true), WriteReceipt::Applied(true));
            assert_eq!(
                other.set_no_gravity(host, true),
                WriteReceipt::Refused(EntityWriteError::TransactionActive)
            );
            Ok(())
        });
        assert_eq!(result, Ok(()));
        assert!(game.mobs.iter().find(|m| m.id == 71).unwrap().no_gravity);
        assert!(events.lock().unwrap().is_empty());
        let WriteReceipt::Pending(first) = other.set_no_gravity(game, true) else {
            panic!("foreign resident did not queue")
        };
        assert_eq!(
            first.sequence, 0,
            "transaction refusal allocated no command id"
        );
        let WriteReceipt::Pending(second) = other.set_silent(game, true) else {
            panic!("silent did not queue")
        };
        let WriteReceipt::Pending(third) = other.set_no_gravity(game, false) else {
            panic!("second gravity write did not queue")
        };
        assert_eq!([first.sequence, second.sequence, third.sequence], [0, 1, 2]);
        assert!(
            !game.mobs.iter().find(|m| m.id == 72).unwrap().no_gravity,
            "acceptance did not write canonical storage"
        );
        assert!(!game.mobs.iter().find(|m| m.id == 72).unwrap().silent);
        assert_eq!(other.no_gravity(game), Some(false));
        assert_eq!(other.is_silent(game), Some(true));
        let (at, _) = game
            .resident_entities
            .table
            .borrow_mut()
            .server_admitted_location(72, |at| game.locate_server_resident_owner(at))
            .unwrap();
        assert!(game.publish_no_gravity_target(
            horizon_client::join_seq::residency::write_queue::Target {
                runtime_id: 72,
                location: at
            },
            EntityReadCapturePhase::BeforeCallback
        ));
        assert_eq!(
            other.no_gravity(game),
            Some(false),
            "capture rebased both ordered fields"
        );
        assert_eq!(other.is_silent(game), Some(true));
        game.with_entity_read_publication(other, |row| {
            assert_eq!(
                row.pending_dependency(&[
                    super::native_read_dependencies::EntityReadDependency::Nbt
                ]),
                Some(third)
            );
            assert_eq!(row.capture.pending_overlay, Some(third));
            let super::native_entity_read_cache::NativeReadDetails::Mob { living, mob, .. } =
                &row.details
            else {
                panic!("missing actual mob projection")
            };
            assert_eq!(living.capture.pending_overlay, Some(third));
            assert_eq!(mob.capture.pending_overlay, Some(third));
            Some(())
        })
        .unwrap();
        game.drain_entity_commands(scope.frame.turn);
        assert!(!game.mobs.iter().find(|m| m.id == 72).unwrap().no_gravity);
        assert!(game.mobs.iter().find(|m| m.id == 72).unwrap().silent);
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                EntityWriteCompletion::EntitySetNoGravity(CommandCompleted {
                    id: first,
                    outcome: Ok(true)
                }),
                EntityWriteCompletion::EntitySetSilent(CommandCompleted {
                    id: second,
                    outcome: Ok(true)
                }),
                EntityWriteCompletion::EntitySetNoGravity(CommandCompleted {
                    id: third,
                    outcome: Ok(true)
                }),
            ]
        );
        let final_row = game.entity_cached_row(other).unwrap();
        assert_eq!(final_row.capture.pending_overlay, None);
        assert_eq!(
            final_row.capture.phase,
            EntityReadCapturePhase::AfterAppliedWrite
        );
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn gravity_all_ten_real_source_families_apply_active_and_parked_without_swaps() {
    fixture(|game, events| {
        game.enter_level(super::Dim::Nether).unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let epoch = game.native_lifecycle.epoch().unwrap();
        let mut targets = Vec::new();
        for (n, (kind, data)) in [
            ("pig", "NoAI:1b"),
            ("item", "Item:{id:\"minecraft:stone\",count:1}"),
            ("experience_orb", "Value:1s"),
            ("armor_stand", "Marker:0b"),
            ("oak_boat", ""),
            ("minecart", ""),
            ("snowball", ""),
            ("tnt", "fuse:1000s"),
            ("trident", ""),
            (
                "falling_block",
                "BlockState:{Name:\"minecraft:stone\"},Time:1",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let tail = if data.is_empty() {
                String::new()
            } else {
                format!(",{data}")
            };
            assert!(
                game.run_command(&format!(
                    "summon {kind} 4.5 100 4.5 {{UUID:[I;731,0,0,{n}]{tail}}}"
                )),
                "summon {kind}"
            );
            let uuid = [731, 0, 0, n as i32];
            let id = *game
                .persist
                .uuids
                .iter()
                .find(|(_, value)| **value == uuid)
                .unwrap()
                .0;
            let entity = EntityRef::new(epoch, id);
            assert_eq!(
                entity.set_no_gravity(game, true),
                WriteReceipt::Applied(true),
                "active {kind}"
            );
            assert_eq!(entity.no_gravity(game), Some(true), "active read {kind}");
            assert_eq!(
                entity.set_no_gravity(game, false),
                WriteReceipt::Applied(true)
            );
            assert_eq!(entity.no_gravity(game), Some(false), "active reset {kind}");
            targets.push(entity);
        }
        game.enter_level(super::Dim::Overworld).unwrap();
        let _issuer = mod_api::__private::enter("gravity-writer", "test");
        let scope = super::native_entity_command_lifecycle_tests::writer(game);
        let dim = game.dim.key();
        for entity in &targets {
            let WriteReceipt::Pending(_) = entity.set_no_gravity(game, true) else {
                panic!("parked foreign did not queue")
            };
            assert_eq!(entity.no_gravity(game), Some(true));
        }
        assert_eq!(game.dim.key(), dim);
        game.drain_entity_commands(scope.frame.turn);
        assert_eq!(
            game.dim.key(),
            dim,
            "apply did not select the parked dimension"
        );
        assert_eq!(events.lock().unwrap().len(), 10);
        for entity in targets {
            let (at, _) = game
                .resident_entities
                .table
                .borrow_mut()
                .server_admitted_location(entity.id(), |at| game.locate_server_resident_owner(at))
                .unwrap();
            let parked = game.levels.parked_of("minecraft:the_nether").unwrap();
            let raw = parked.resident_read_source(at).unwrap();
            assert_eq!(raw.id, entity.id());
            assert_eq!(
                raw.source.no_gravity(),
                Some(true),
                "actual parked storage changed"
            );
            assert_eq!(
                game.entity_cached_row(entity)
                    .unwrap()
                    .capture
                    .pending_overlay,
                None
            );
        }
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn gravity_direct_taken_source_refuses_instead_of_claiming_application() {
    fixture(|game, events| {
        let other = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
        let (at, _) = game
            .resident_entities
            .table
            .borrow_mut()
            .server_admitted_location(72, |at| game.locate_server_resident_owner(at))
            .unwrap();
        let slot = game.mobs.locate(at.join).unwrap();
        let held = game.mobs.extract(at.join).unwrap();
        assert_eq!(
            other.set_no_gravity(game, true),
            WriteReceipt::Refused(EntityWriteError::OutsideScope)
        );
        assert!(!held.no_gravity);
        assert!(events.lock().unwrap().is_empty());
        game.mobs.put_back(slot, held);
        assert_eq!(
            other.set_no_gravity(game, true),
            WriteReceipt::Applied(true)
        );
        assert!(game.mobs.iter().find(|m| m.id == 72).unwrap().no_gravity);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn gravity_missing_backing_refuses_without_a_command_or_overlay() {
    fixture(|game, events| {
        game.build_registries();
        game.set_allow_commands(true);
        assert!(game.run_command("summon item_frame 4.5 80 4.5 {UUID:[I;732,0,0,1]}"));
        let id = *game
            .persist
            .uuids
            .iter()
            .find(|(_, uuid)| **uuid == [732, 0, 0, 1])
            .unwrap()
            .0;
        let epoch = game.native_lifecycle.epoch().unwrap();
        let frame = EntityRef::new(epoch, id);
        let _issuer = mod_api::__private::enter("gravity-writer", "test");
        let scope = super::native_entity_command_lifecycle_tests::writer(game);
        assert_eq!(
            frame.set_no_gravity(game, true),
            WriteReceipt::Refused(EntityWriteError::Unsupported)
        );
        assert!(events.lock().unwrap().is_empty());
        assert!(
            game.entity_cached_row(frame)
                .is_none_or(|row| row.capture.pending_overlay.is_none())
        );
        let other = EntityRef::new(epoch, 72);
        let WriteReceipt::Pending(id) = other.set_no_gravity(game, true) else {
            panic!("supported write refused")
        };
        assert_eq!(id.sequence, 0);
        game.drain_entity_commands(scope.frame.turn);
        assert_eq!(events.lock().unwrap().len(), 1);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn gravity_transfer_does_not_retarget_the_accepted_source_address() {
    fixture(|game, events| {
        game.build_registries();
        let uuid = game.uuid_for(72);
        let _issuer = mod_api::__private::enter("gravity-writer", "test");
        let scope = super::native_entity_command_lifecycle_tests::writer(game);
        let other = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
        let WriteReceipt::Pending(id) = other.set_no_gravity(game, true) else {
            panic!("foreign did not queue")
        };
        assert!(game.entity_teleport_to_level(
            other,
            "minecraft:the_nether",
            [4.0, 80.0, 0.5],
            None
        ));
        game.run_level_teleports();
        let destination = game
            .levels
            .parked_of("minecraft:the_nether")
            .unwrap()
            .saved_entity_uuids()
            .find(|(_, saved)| *saved == uuid)
            .unwrap()
            .0;
        assert_ne!(
            destination, 72,
            "canonical transfer reloaded a fresh runtime identity"
        );
        let (at, _) = game
            .resident_entities
            .table
            .borrow_mut()
            .server_admitted_location(destination, |at| game.locate_server_resident_owner(at))
            .unwrap();
        assert_eq!(at.owner.dimension, DimensionKey::NETHER);
        game.drain_entity_commands(scope.frame.turn);
        assert_eq!(
            *events.lock().unwrap(),
            vec![EntityWriteCompletion::EntitySetNoGravity(
                CommandCompleted {
                    id,
                    outcome: Err(EntityWriteError::Removed)
                }
            )]
        );
        let parked = game.levels.parked_of("minecraft:the_nether").unwrap();
        assert_eq!(
            parked.resident_read_source(at).unwrap().source.no_gravity(),
            Some(false)
        );
    });
}

#[test]
#[ignore = "needs prepared resources and --features system-allocator"]
fn gravity_warm_converted_acceptance_apply_event_and_query_do_not_allocate() {
    fixture(|game, events| {
        let package = format!("converted:{}", "gravity-package".repeat(64));
        let packages =
            mod_api::content::crash::ConvertedPackages::new("gravity-writer", [package.clone()]);
        let _issuer = mod_api::__private::enter("gravity-writer", "allocation test");
        let _package = packages
            .package(&package)
            .unwrap()
            .enter("pre", "test")
            .unwrap();
        let other = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
        let warm = super::native_entity_command_lifecycle_tests::writer(game);
        for _ in 0..64 {
            assert!(matches!(
                other.set_no_gravity(game, true),
                WriteReceipt::Pending(_)
            ));
        }
        game.drain_entity_commands(warm.frame.turn);
        events.lock().unwrap().clear();
        for _ in 0..2 {
            game.entity_read_state
                .borrow_mut()
                .commands
                .as_mut()
                .unwrap()
                .queue
                .end_tick();
        }
        drop(warm);
        let writer = super::native_entity_command_lifecycle_tests::writer(game);
        let accepted = allocation_counter::measure(|| {
            for _ in 0..64 {
                let WriteReceipt::Pending(id) = other.set_no_gravity(game, false) else {
                    panic!("warm scalar refused")
                };
                assert_eq!(id.query(game), Ok(EntityCommandStatus::Pending));
            }
        });
        assert_eq!(accepted.count_total, 0, "warm accept/query: {accepted:?}");
        let delivered =
            allocation_counter::measure(|| game.drain_entity_commands(writer.frame.turn));
        assert_eq!(
            delivered.count_total, 0,
            "warm apply/event/query: {delivered:?}"
        );
        assert_eq!(events.lock().unwrap().len(), 64);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn gravity_taken_target_accepts_only_projection_then_revalidates_removal_or_reuse() {
    for replace in [false, true] {
        fixture(|game, events| {
            let _issuer = mod_api::__private::enter("gravity-writer", "test");
            let scope = super::native_entity_command_lifecycle_tests::writer(game);
            let other = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
            let (at, _) = game
                .resident_entities
                .table
                .borrow_mut()
                .server_admitted_location(72, |at| game.locate_server_resident_owner(at))
                .unwrap();
            assert!(game.publish_no_gravity_target(
                horizon_client::join_seq::residency::write_queue::Target {
                    runtime_id: 72,
                    location: at
                },
                EntityReadCapturePhase::BeforeCallback
            ));
            let slot = game.mobs.locate(at.join).unwrap();
            let held = game.mobs.extract(at.join).unwrap();
            let WriteReceipt::Pending(id) = other.set_no_gravity(game, true) else {
                panic!("retained Taken projection did not queue")
            };
            assert!(!held.no_gravity);
            assert_eq!(other.no_gravity(game), Some(true));
            game.mobs.put_back(slot, held);
            game.mobs.retain(|m| m.id != 72);
            if replace {
                let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                game.mobs
                    .push(Mob::new(72, kind, Vec3::new(4.0, 80.0, 0.5), 0.0));
            }
            game.drain_entity_commands(scope.frame.turn);
            let error = if replace {
                EntityWriteError::GenerationChanged
            } else {
                EntityWriteError::Removed
            };
            assert_eq!(
                *events.lock().unwrap(),
                vec![EntityWriteCompletion::EntitySetNoGravity(
                    CommandCompleted {
                        id,
                        outcome: Err(error)
                    }
                )]
            );
            assert!(game.mobs.iter().all(|m| !m.no_gravity));
        });
    }
}

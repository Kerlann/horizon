//! Safe allocator instrumentation, only with mimalloc disabled by the existing
//! system-allocator feature. No unsafe test allocator or synthetic command event.
#![cfg(all(feature = "system-allocator", not(feature = "alloc-census")))]
use super::native_entity_command_lifecycle_tests::with_fixture;
use mod_api::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Runtime {
    package: Arc<str>,
    completed: Arc<AtomicUsize>,
}
#[mod_api::__subscriptions]
impl Hooks for Runtime {
    fn entity_command_completed(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityCommandCompletedEvent<'_>,
    ) {
        mod_api::__private::with_current_converted_package(|package| {
            assert_eq!(package, Some(self.package.as_ref()))
        });
        let EntityWriteCompletion::EntitySetSilent(done) = event.completion;
        assert_eq!(
            done.id.query(host),
            Ok(EntityCommandStatus::Completed(event.completion.clone()))
        );
        self.completed.fetch_add(1, Ordering::Relaxed);
    }
}
#[test]
#[ignore = "needs prepared resources and --features system-allocator"]
fn silent_warm_converted_acceptance_and_queries_allocate_no_package_text() {
    with_fixture(|game, _| {
        let package: Arc<str> = Arc::from(format!("converted:{}", "owned-package-name".repeat(64)));
        let completed = Arc::new(AtomicUsize::new(0));
        mod_api::testing::with_mods(
            vec![ModEntry::new(
                "alloc-runtime",
                Box::new(Runtime {
                    package: package.clone(),
                    completed: completed.clone(),
                }),
            )]
            .into(),
            || {
                let inventory =
                    mod_api::crash::ConvertedPackages::new("alloc-runtime", [package.to_string()]);
                let _native = mod_api::__private::enter("alloc-runtime", "test");
                let _package = inventory
                    .package(&package)
                    .unwrap()
                    .enter("pre", "allocation test")
                    .unwrap();
                let epoch = game.native_lifecycle.epoch().unwrap();
                let other = EntityRef::new(epoch, 72);
                let (location, _) = game
                    .entity_residency
                    .borrow_mut()
                    .server_admitted_location(71, |at| game.locate_server_resident_owner(at))
                    .unwrap();
                let actor = horizon_client::join_seq::residency::write_queue::Target {
                    runtime_id: 71,
                    location,
                };
                // Warm the issuer intern table, FIFO/record capacity and row overlay.
                let warm = game.begin_entity_writer(actor).unwrap();
                for _ in 0..64 {
                    assert!(matches!(
                        game.entity_set_silent(other, true),
                        WriteReceipt::Pending(_)
                    ));
                }
                game.drain_entity_commands(warm.frame.turn);
                game.entity_read_state
                    .borrow_mut()
                    .commands
                    .as_mut()
                    .unwrap()
                    .queue
                    .end_tick();
                game.entity_read_state
                    .borrow_mut()
                    .commands
                    .as_mut()
                    .unwrap()
                    .queue
                    .end_tick();
                drop(warm);
                let writer = game.begin_entity_writer(actor).unwrap();
                let accepted = allocation_counter::measure(|| {
                    for _ in 0..64 {
                        let WriteReceipt::Pending(id) = game.entity_set_silent(other, false) else {
                            panic!("warm write refused");
                        };
                        assert_eq!(id.query(game), Ok(EntityCommandStatus::Pending));
                    }
                });
                assert_eq!(
                    accepted.count_total, 0,
                    "warm converted accept/query: {accepted:?}"
                );
                let delivery =
                    allocation_counter::measure(|| game.drain_entity_commands(writer.frame.turn));
                assert_eq!(
                    delivery.count_total, 0,
                    "warm actual apply/event/query: {delivery:?}"
                );
                assert_eq!(completed.load(Ordering::Relaxed), 128);
            },
        );
    });
}

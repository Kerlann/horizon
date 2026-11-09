//! An original actor panic and multiple notification panics cannot strand FIFO work.
use super::native_entity_command_lifecycle_tests::with_fixture;
use mod_api::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
struct Panics {
    ids: Arc<Mutex<Vec<CommandId>>>,
    completed: Arc<AtomicUsize>,
}
#[mod_api::__subscriptions]
impl Hooks for Panics {
    fn entity_tick_listened(&self, _: &mut dyn ServerHost, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }
    fn entity_tick_pre(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityTickPreEvent<'_>,
    ) -> Flow {
        if event.entity == 71 {
            let target = host.entity(72).unwrap();
            for value in [true, false] {
                let WriteReceipt::Pending(id) = target.set_silent(host, value) else {
                    panic!("write did not queue");
                };
                self.ids.lock().unwrap().push(id);
            }
            panic!("original actor panic");
        }
        Flow::Continue
    }
    fn entity_command_completed(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityCommandCompletedEvent<'_>,
    ) {
        let EntityWriteCompletion::EntitySetSilent(done) = event.completion;
        assert_eq!(done.outcome, Ok(true));
        assert_eq!(
            done.id.query(host),
            Ok(EntityCommandStatus::Completed(event.completion.clone()))
        );
        let previous = self.completed.fetch_add(1, Ordering::Relaxed);
        if previous == 0 {
            panic!("first completion panic");
        }
        panic!("second completion panic");
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn silent_actor_and_completion_panics_preserve_original_and_finish_accepted_work() {
    with_fixture(|game, _| {
        let ids = Arc::new(Mutex::new(Vec::new()));
        let completed = Arc::new(AtomicUsize::new(0));
        mod_api::testing::with_mods(
            vec![ModEntry::new(
                "panics",
                Box::new(Panics {
                    ids: ids.clone(),
                    completed: completed.clone(),
                }),
            )]
            .into(),
            || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    game.tick_creature_entities().unwrap()
                }));
                let payload = result.expect_err("actual actor callback must unwind");
                assert_eq!(
                    payload.downcast_ref::<&str>(),
                    Some(&"original actor panic")
                );
                assert_eq!(
                    completed.load(Ordering::Relaxed),
                    2,
                    "every accepted terminal was attempted despite notification panic"
                );
                assert!(!game.mobs.iter().find(|mob| mob.id == 72).unwrap().silent);
                assert!(game.active_entity_writer().is_none());
                let _native = mod_api::__private::enter("panics", "test query");
                for id in ids.lock().unwrap().iter() {
                    assert!(matches!(
                        id.query(game),
                        Ok(EntityCommandStatus::Completed(
                            EntityWriteCompletion::EntitySetSilent(CommandCompleted {
                                outcome: Ok(true),
                                ..
                            })
                        ))
                    ));
                }
                let other = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
                assert_eq!(
                    game.entity_cached_row(other)
                        .unwrap()
                        .capture
                        .pending_overlay,
                    None
                );
            },
        );
    });
}

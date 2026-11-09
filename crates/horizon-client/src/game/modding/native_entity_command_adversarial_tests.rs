//! Adversarial callbacks exercise real dispatch, not synthetic completions.
use super::native_entity_command_lifecycle_tests::with_fixture;
use mod_api::*;
use std::sync::{Arc, Mutex};
struct Nested(bool);
#[mod_api::__subscriptions]
impl Hooks for Nested {
    fn entity_tick_pre(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityTickPreEvent<'_>,
    ) -> Flow {
        assert_eq!(event.entity, 72);
        let actor = host.entity(72).unwrap();
        assert_eq!(actor.set_silent(host, true), WriteReceipt::Applied(true));
        assert_eq!(
            actor.is_silent(host),
            Some(true),
            "explicit OWN reads actual source"
        );
        let row = actor.cached_row(host).unwrap();
        assert_eq!(
            row.is_silent,
            Some(false),
            "explicit cache still projects earlier accepted work"
        );
        assert!(row.capture.pending_overlay.is_some());
        assert_eq!(
            host.entity(71).unwrap().set_silent(host, true),
            WriteReceipt::Pending(CommandId {
                epoch: actor.epoch(),
                sequence: 1
            })
        );
        assert!(!self.0, "nested callback panic");
        Flow::Continue
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn silent_nested_callback_own_actor_keeps_turn_and_restores_on_panic() {
    for panic in [false, true] {
        with_fixture(|game, _| {
            let (a, _) = game
                .entity_residency
                .borrow_mut()
                .server_admitted_location(71, |at| game.locate_server_resident_owner(at))
                .unwrap();
            let outer = game
                .begin_entity_writer(horizon_client::join_seq::residency::write_queue::Target {
                    runtime_id: 71,
                    location: a,
                })
                .unwrap();
            let _native = mod_api::__private::enter("silent-runtime", "test");
            let other = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
            let WriteReceipt::Pending(id) = game.entity_set_silent(other, false) else {
                panic!("initial foreign write did not queue");
            };
            let result = mod_api::testing::with_mods(
                vec![ModEntry::new("nested", Box::new(Nested(panic)))].into(),
                || {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        game.native_entity_tick_pre(
                            "minecraft:overworld",
                            72,
                            "minecraft:pig",
                            &mut false,
                        );
                    }))
                },
            );
            assert_eq!(result.is_err(), panic);
            let restored = game.active_entity_writer().unwrap();
            assert_eq!(restored.turn, outer.frame.turn);
            assert_eq!(restored.actor, outer.frame.actor);
            assert_eq!(
                game.entity_is_silent(other),
                Some(false),
                "foreign cache priority restored"
            );
            game.drain_entity_commands(outer.frame.turn);
            assert!(matches!(
                game.entity_command_query(id),
                Ok(EntityCommandStatus::Completed(_))
            ));
            assert!(!game.mobs.iter().find(|mob| mob.id == 72).unwrap().silent);
        });
    }
}
#[derive(Default)]
struct BudgetState {
    ids: Vec<CommandId>,
    completed: usize,
    refused: bool,
    b_pre: bool,
}
struct Budget(Arc<Mutex<BudgetState>>);
#[mod_api::__subscriptions]
impl Hooks for Budget {
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
            let WriteReceipt::Pending(id) = target.set_silent(host, true) else {
                panic!("initial write refused");
            };
            self.0.lock().unwrap().ids.push(id);
        } else {
            let mut state = self.0.lock().unwrap();
            assert!(state.refused);
            assert_eq!(
                state.completed,
                super::native_entity_commands::MAX_ENTITY_COMMANDS_PER_TURN
            );
            state.b_pre = true;
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
        let target = host.entity(72).unwrap();
        let mut state = self.0.lock().unwrap();
        state.completed += 1;
        match target.set_silent(host, true) {
            WriteReceipt::Pending(id) => {
                assert_eq!(id.sequence as usize, state.completed);
                state.ids.push(id);
            }
            WriteReceipt::Refused(EntityWriteError::WorkLimit) => {
                state.refused = true;
                assert_eq!(
                    target.cached_row(host).unwrap().capture.pending_overlay,
                    None
                );
            }
            other => panic!("unexpected receipt: {other:?}"),
        }
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn silent_completion_append_has_cumulative_budget_and_reaches_next_actor() {
    with_fixture(|game, _| {
        let state = Arc::new(Mutex::new(BudgetState::default()));
        mod_api::testing::with_mods(
            vec![ModEntry::new("budget", Box::new(Budget(state.clone())))].into(),
            || {
                game.tick_creature_entities().unwrap();
                let state = state.lock().unwrap();
                assert!(state.b_pre);
                assert!(state.refused);
                assert_eq!(
                    state.ids.len(),
                    super::native_entity_commands::MAX_ENTITY_COMMANDS_PER_TURN
                );
                assert_eq!(
                    state.completed,
                    state.ids.len(),
                    "no refused-request completion"
                );
                assert_eq!(
                    state.ids.last().unwrap().sequence as usize + 1,
                    state.ids.len()
                );
            },
        );
    });
}

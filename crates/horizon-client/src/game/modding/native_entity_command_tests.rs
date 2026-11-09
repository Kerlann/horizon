//! Actual domain assertions at real Pre/actor/Post/store-back boundaries.
use horizon_client::{chunk::Chunk, mobs::Mob};
use horizon_core::geometry::Vec3;
use mod_api::*;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Observed {
    ids: Vec<CommandId>,
    order: Vec<&'static str>,
    terminals: Vec<EntityWriteCompletion>,
}
struct Writer(Arc<Mutex<Observed>>);
#[mod_api::__subscriptions]
impl Hooks for Writer {
    fn entity_tick_listened(&self, _: &mut dyn ServerHost, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }
    fn entity_tick_pre(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityTickPreEvent<'_>,
    ) -> Flow {
        let actor = host.entity(event.entity).unwrap();
        assert_eq!(actor.set_silent(host, true), WriteReceipt::Applied(true));
        self.0
            .lock()
            .unwrap()
            .order
            .push(if event.entity == 71 { "pre A" } else { "pre B" });
        let other = host.entity(72).unwrap();
        if event.entity == 71 {
            let mut ids = Vec::new();
            for silent in [true, false] {
                let WriteReceipt::Pending(id) = other.set_silent(host, silent) else {
                    panic!("foreign resident did not queue");
                };
                assert_eq!(id.query(host), Ok(EntityCommandStatus::Pending));
                assert_eq!(other.is_silent(host), Some(silent));
                ids.push(id);
            }
            self.0.lock().unwrap().ids = ids;
        } else {
            // All three writes and terminal callbacks ran before B's Pre.
            assert_eq!(self.0.lock().unwrap().terminals.len(), 3);
            assert_eq!(other.is_silent(host), Some(true));
        }
        Flow::Continue
    }
    fn entity_tick_post(&self, _: &mut dyn ServerHost, event: &mut EntityTickPostEvent<'_>) {
        self.0.lock().unwrap().order.push(if event.entity == 71 {
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
        let EntityWriteCompletion::EntitySetSilent(done) = event.completion;
        assert_eq!(done.outcome, Ok(true));
        assert_eq!(
            done.id.query(host),
            Ok(EntityCommandStatus::Completed(event.completion.clone()))
        );
        let count = self.0.lock().unwrap().terminals.len();
        let other = host.entity(72).unwrap();
        assert_eq!(other.is_silent(host), Some(count != 0));
        if count == 0 {
            let WriteReceipt::Pending(id) = other.set_silent(host, true) else {
                panic!("completion did not append to enclosing writer");
            };
            assert_eq!(id.query(host), Ok(EntityCommandStatus::Pending));
            // No recursive delivery while this callback is still executing.
            assert!(self.0.lock().unwrap().terminals.is_empty());
            self.0.lock().unwrap().ids.push(id);
        }
        let mut state = self.0.lock().unwrap();
        state.order.push("complete");
        state.terminals.push(event.completion.clone());
    }
}
struct Observer(Arc<Mutex<Observed>>);
#[mod_api::__subscriptions]
impl Hooks for Observer {
    fn entity_tick_pre(&self, host: &mut dyn ServerHost, _: &mut EntityTickPreEvent<'_>) -> Flow {
        for id in &self.0.lock().unwrap().ids {
            assert_eq!(id.query(host), Err(CommandQueryError::UnknownOrExpired));
        }
        Flow::Continue
    }
    fn entity_command_completed(
        &self,
        _: &mut dyn ServerHost,
        _: &mut EntityCommandCompletedEvent<'_>,
    ) {
        panic!("another issuer received completion");
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn real_silent_fifo_overlay_nested_completion_and_issuer_lifecycle() {
    let observed = Arc::new(Mutex::new(Observed::default()));
    mod_api::testing::with_mods(
        vec![
            ModEntry::new("silent-writer", Box::new(Writer(observed.clone()))),
            ModEntry::new("silent-observer", Box::new(Observer(observed.clone()))),
        ]
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
                let state = observed.lock().unwrap();
                assert_eq!(
                    state.order,
                    [
                        "pre A", "post A", "complete", "complete", "complete", "pre B", "post B"
                    ]
                );
                for (id, terminal) in state.ids.iter().zip(&state.terminals) {
                    let _issuer = mod_api::__private::enter("silent-writer", "test");
                    assert_eq!(
                        id.query(game),
                        Ok(EntityCommandStatus::Completed(terminal.clone()))
                    );
                }
                assert!(game.mobs.iter().all(|mob| mob.silent));
                let id = state.ids[0];
                let _issuer = mod_api::__private::enter("silent-writer", "test");
                game.entity_read_state
                    .borrow_mut()
                    .commands
                    .as_mut()
                    .unwrap()
                    .queue
                    .end_tick();
                assert!(matches!(
                    id.query(game),
                    Ok(EntityCommandStatus::Completed(_))
                ));
                game.entity_read_state
                    .borrow_mut()
                    .commands
                    .as_mut()
                    .unwrap()
                    .queue
                    .end_tick();
                assert_eq!(id.query(game), Err(CommandQueryError::UnknownOrExpired));
                let stale = CommandId {
                    epoch: NativeWorldEpoch::new(u64::MAX),
                    sequence: id.sequence,
                };
                assert_eq!(stale.query(game), Err(CommandQueryError::StaleEpoch));
            });
        },
    );
}

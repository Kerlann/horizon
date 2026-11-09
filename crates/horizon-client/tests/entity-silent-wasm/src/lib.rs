use mod_api::*;
use std::sync::Mutex;
#[derive(Default)]
struct State {
    ids: Vec<CommandId>,
    completed: usize,
}
#[derive(Default)]
struct Mod {
    state: Mutex<State>,
}
#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn entity_tick_listened(&self, _: &mut dyn ServerHost, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }
    fn entity_tick_pre(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityTickPreEvent<'_>,
    ) -> Flow {
        let own = host.entity(event.entity).unwrap();
        assert_eq!(own.set_silent(host, true), WriteReceipt::Applied(true));
        let other = host.entity(72).unwrap();
        let mut state = self.state.lock().unwrap();
        if event.entity == 71 {
            for value in [true, false] {
                let WriteReceipt::Pending(id) = other.set_silent(host, value) else {
                    panic!("foreign write was not accepted");
                };
                assert_eq!(id.query(host), Ok(EntityCommandStatus::Pending));
                assert_eq!(other.is_silent(host), Some(value));
                state.ids.push(id);
            }
        } else {
            assert_eq!(state.completed, 3, "all own completions precede B Pre");
            assert_eq!(other.is_silent(host), Some(true));
            for id in &state.ids {
                assert!(matches!(
                    id.query(host),
                    Ok(EntityCommandStatus::Completed(_))
                ));
            }
            // Independent physical markers prove that both guest copies made
            // it past every assertion, including their last completion query.
            match state.ids[0].sequence {
                0 => assert!(other.set_no_gravity(host, true)),
                2 => assert!(other.set_glowing(host, true)),
                _ => panic!("unexpected independent package command order"),
            }
        }
        Flow::Continue
    }
    fn entity_command_completed(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityCommandCompletedEvent<'_>,
    ) {
        let EntityWriteCompletion::EntitySetSilent(done) = event.completion;
        let mut state = self.state.lock().unwrap();
        assert!(
            state.ids.contains(&done.id),
            "a different wasm package's terminal leaked"
        );
        assert_eq!(done.outcome, Ok(true));
        assert_eq!(
            done.id.query(host),
            Ok(EntityCommandStatus::Completed(event.completion.clone()))
        );
        if state.completed == 0 {
            let other = host.entity(72).unwrap();
            let WriteReceipt::Pending(id) = other.set_silent(host, true) else {
                panic!("nested completion write did not append");
            };
            assert_eq!(id.query(host), Ok(EntityCommandStatus::Pending));
            state.ids.push(id);
        }
        state.completed += 1;
    }
}

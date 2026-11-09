//! Actual world failure retires pending and terminal command state.
use super::native_entity_command_lifecycle_tests::{with_fixture, writer};
use mod_api::*;
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn silent_world_failure_invalidates_pending_and_terminal_without_old_scope_resurrection() {
    with_fixture(|game, events| {
        // with_game stops during Opening; a server-loop failure retires an
        // actually Ready epoch, whereas open failures have their own boundary.
        game.native_lifecycle.world_open_ready().unwrap();
        let _native = mod_api::__private::enter("silent-runtime", "test");
        let scope = writer(game);
        let other = EntityRef::new(game.native_lifecycle.epoch().unwrap(), 72);
        let WriteReceipt::Pending(terminal) = game.entity_set_silent(other, false) else {
            panic!("write did not queue");
        };
        game.drain_entity_commands(scope.frame.turn);
        let WriteReceipt::Pending(pending) = game.entity_set_silent(other, true) else {
            panic!("second write did not queue");
        };
        assert_eq!(events.lock().unwrap().len(), 1);
        game.native_lifecycle_close_failed("fixture server-loop failure");
        assert_eq!(
            game.entity_command_query(terminal),
            Err(CommandQueryError::StaleEpoch)
        );
        assert_eq!(
            game.entity_command_query(pending),
            Err(CommandQueryError::StaleEpoch)
        );
        assert!(game.entity_read_state.borrow().commands.is_none());
        assert!(game.active_entity_writer().is_none());
        drop(scope);
        assert!(game.active_entity_writer().is_none());
        assert_eq!(
            events.lock().unwrap().len(),
            1,
            "retired pending work has no fabricated completion"
        );
        assert!(!game.mobs.iter().find(|mob| mob.id == 72).unwrap().silent);
        game.native_lifecycle
            .begin_open(NativeAuthority::DedicatedServer);
        game.native_lifecycle.world_open_ready().unwrap();
        assert_ne!(game.native_lifecycle.epoch(), Some(other.epoch()));
        assert_eq!(
            game.entity_command_query(pending),
            Err(CommandQueryError::StaleEpoch)
        );
    });
}

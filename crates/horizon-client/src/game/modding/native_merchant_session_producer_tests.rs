use super::{native_guest_turn_tests::with_guests, trading::MerchantState};
use horizon_client::merchant::Session;

fn state(id: u32, xp: i32) -> MerchantState {
    let mut session = Session::new(Vec::new(), true);
    session.xp = xp;
    session.level = 2;
    MerchantState {
        session: Box::new(session),
        scroll: 0,
        dragging: false,
        focused: None,
        mob: Some(id),
        name: None,
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn canonical_no_r_session_open_change_close_removal_and_loans_keep_nominations() {
    with_guests(|game| {
        super::native_mob_backing_tests::prepare(game);
        super::native_merchant_refs::tests::spawn(game, 40, "minecraft:villager");
        super::native_merchant_refs::tests::spawn(game, 50, "minecraft:villager");
        assert!(
            game.begin_resident_read_pass(horizon_client::join_seq::residency::Kind::Boat)
                .is_none()
        );
        assert_eq!(game.merchant_customer_connections(40), Some(vec![]));
        game.lan_open_merchant(13, state(40, 17));
        assert_eq!(game.merchant_customer_connections(40), Some(vec![13]));
        assert_eq!(game.lan_trade_session(13).unwrap().xp, 17);
        let window = game.lan.as_ref().unwrap().play[&13]
            .merchant
            .as_ref()
            .unwrap()
            .window;
        assert!(!game.lan_merchant_closed(13, Some(window + 1)));
        assert_eq!(
            game.merchant_customer_connections(40),
            Some(vec![13]),
            "refused close did not change topology"
        );

        game.as_guest(13, |selected| {
            assert!(
                selected.lan_trade_session(13).is_none(),
                "actual GuestPlay is loaned"
            );
            assert_eq!(selected.merchant_customer_connections(40), Some(vec![13]));
            selected
                .as_guest(12, |nested| {
                    assert_eq!(nested.merchant_customer_connections(40), Some(vec![13]));
                    assert!(nested.lan_trade_session(13).is_none());
                })
                .unwrap();
        })
        .unwrap();
        assert_eq!(game.lan_trade_session(13).unwrap().xp, 17);
        {
            let loan = game.begin_merchant_session_loan(13).unwrap();
            let held = game
                .lan
                .as_mut()
                .unwrap()
                .play
                .get_mut(&13)
                .unwrap()
                .merchant
                .take()
                .unwrap();
            assert_eq!(game.merchant_customer_connections(40), Some(vec![13]));
            assert!(
                game.lan_trade_session(13).is_none(),
                "nomination is not a session payload"
            );
            game.entity_read_state.borrow_mut().mob_relations.clear();
            assert_eq!(
                game.merchant_customer_connections(40),
                None,
                "an unprepared session loan is not no customer"
            );
            game.lan
                .as_mut()
                .unwrap()
                .play
                .get_mut(&13)
                .unwrap()
                .merchant = Some(held);
            drop(loan);
        }
        game.entity_read_state.borrow_mut().mob_relations.clear();
        assert_eq!(game.merchant_customer_connections(40), Some(vec![13]));
        super::native_mob_read_context::record_backing_work(|work| *work = Default::default());
        game.lan_open_merchant(13, state(50, 29));
        assert_eq!(game.merchant_customer_connections(40), Some(vec![]));
        assert_eq!(game.merchant_customer_connections(50), Some(vec![13]));
        assert_eq!(game.lan_trade_session(13).unwrap().xp, 29);
        assert!(game.lan_merchant_closed(13, None));
        assert_eq!(game.merchant_customer_connections(50), Some(vec![]));
        assert!(game.lan_trade_session(13).is_none());
        assert!(!game.lan_merchant_closed(13, None));
        game.lan_open_merchant(13, state(40, 31));
        game.guest_disconnected(13);
        assert_eq!(game.merchant_customer_connections(40), Some(vec![]));
        assert!(game.lan_trade_session(13).is_none());
        game.lan_open_merchant(13, state(40, 37));
        let host = game.lan.as_mut().unwrap();
        let slot = host.hub.guests.connection_index(13).unwrap();
        let _removed = host.hub.guests.remove(slot);
        assert!(
            !game.lan_merchant_closed(13, None),
            "no live guest remains to close a window"
        );
        assert!(game.lan_trade_session(13).is_none());
        assert_eq!(
            game.merchant_customer_connections(40),
            Some(vec![]),
            "permanent source removal retires the nomination even on a refused window operation"
        );
        assert_eq!(
            super::native_mob_read_context::backing_work().population_visits,
            0,
            "session changes only update their affected edge"
        );
    });
}

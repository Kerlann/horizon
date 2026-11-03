//! Real-game regressions for native handles used during a guest's turn.

use super::{lan::LanSettings, native_entity_refs::tests::with_game};
use horizon_client::{
    lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig},
    mobs::Mob,
};
use horizon_core::geometry::Vec3;
use horizon_protocol::packets::configuration::ClientInformation;
use std::sync::{
    Arc,
    atomic::{AtomicI32, AtomicUsize},
};

pub(in crate::game) fn with_guests(test: impl FnOnce(&mut super::Game<'_>)) {
    with_game(|game| {
        let settings = LanSettings {
            port: 0,
            guest_commands: false,
            force_game_mode: false,
        };
        let net = NetConfig {
            listen: false,
            announce: false,
            lod_distance: 0,
            ..NetConfig::lan(0, String::new(), 2)
        };
        let config = HubConfig {
            default_game_mode: 0,
            guest_commands: false,
            force_game_mode: false,
            hardcore: false,
            view_distance: 2,
            simulation_distance: 2,
            dimension: game.dim.id().to_owned(),
            allow_flight: true,
            seed: 0,
            max_players: 2,
            integrated: true,
            levels: game.level_shapes,
        };
        game.lan_host(settings, net, config).unwrap();
        for id in [12, 13] {
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut guest = Guest::new(
                Joined {
                    conn: id,
                    uuid: u128::from(id),
                    name: format!("guest{id}"),
                    properties: Vec::new(),
                    info: ClientInformation::new(8),
                    brand: String::new(),
                    sender: PlayerSender::new(tx, Arc::new(AtomicUsize::new(0))),
                    latency: Arc::new(AtomicI32::new(0)),
                    address: "127.0.0.1:1".parse().unwrap(),
                },
                id as i32,
                [3.0, 64.0, 5.0],
                10,
            );
            guest.placed = true;
            game.lan.as_mut().unwrap().hub.guests.push(guest);
        }
        test(game);
    });
}

/// Before #228, the host was absent during a guest turn and B and the
/// online list were invisible to native handles.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn another_guest_and_online_players_are_visible_during_a_turn() {
    with_guests(|game| {
        let rider = game.native_entity(13).unwrap();
        game.as_guest(12, |game| {
            assert_eq!(rider.position(game), Some([3.0, 64.0, 5.0]));
            let online = game.native_server_players();
            assert_eq!(online.iter().map(|p| p.uuid).collect::<Vec<_>>(), [12, 13]);
            assert_eq!(online[1].position, [3.0, 64.0, 5.0]);
        })
        .unwrap();
    });
}

/// B's live menu and motion stay available while A acts, but displaced
/// menus and an outer turn's motion must not use the swapped-out records.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn another_guests_menu_and_push_work_during_a_turn() {
    use horizon_client::items::ItemStack;

    with_guests(|game| {
        for conn in [12, 13] {
            game.as_guest(conn, |game| {
                game.player.as_mut().unwrap().velocity = Vec3::ZERO;
            })
            .unwrap();
        }
        let a = game.native_player_ref(12).unwrap();
        let b = game.native_player_ref(13).unwrap();
        let a_menu = game.native_menu_ref(a).unwrap();
        let b_menu = game.native_menu_ref(b).unwrap();
        let stone = ItemStack::new("minecraft:stone", 4);
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .guest_mut(13)
            .unwrap()
            .inventory[0] = Some(stone.clone());
        game.as_guest(12, |game| {
            assert!(game.native_menu_ref(a).is_none());
            assert_eq!(a_menu.slot_count(game), None);
            assert_eq!(game.native_menu_ref(b), Some(b_menu));
            assert_eq!(b_menu.slot_count(game), Some(46));
            assert_eq!(b_menu.slot(game, 36), Some(Some(stone.clone())));
            assert!(game.native_entity_set_rotation(b.entity(), 90.0, 10.0));
            assert!(game.native_entity_set_head_yaw(b.entity(), 45.0));
            assert_eq!(game.native_entity_head_yaw(b.entity()), Some(45.0));
            assert!(game.native_entity_push(b.entity(), [0.25, 0.5, -0.25]));
            assert!(game.native_entity_push(a.entity(), [0.5, 0.0, 0.0]));
            game.as_guest(13, |game| {
                assert_eq!(
                    game.player.as_ref().unwrap().velocity,
                    Vec3::new(0.25, 0.5, -0.25)
                );
                assert!(game.native_menu_ref(a).is_none());
                assert!(game.native_menu_ref(b).is_none());
                assert_eq!(b_menu.slot_count(game), None);
                assert!(!game.native_entity_push(a.entity(), [1.0, 0.0, 0.0]));
            })
            .unwrap();
            assert_eq!(
                game.player.as_ref().unwrap().velocity,
                Vec3::new(0.5, 0.0, 0.0)
            );
        })
        .unwrap();
        assert_eq!(b_menu.slot(game, 36), Some(Some(stone)));
        assert_eq!(
            game.lan
                .as_ref()
                .unwrap()
                .hub
                .guest(13)
                .unwrap()
                .motion
                .as_ref()
                .unwrap()
                .velocity,
            Vec3::new(0.25, 0.5, -0.25)
        );
    });
}

/// The body/saddle store and existing armor/offhand move with nested turns.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn nested_guest_turns_restore_each_equipment_store() {
    use horizon_client::items::ItemStack;
    use horizon_items::equipment::{BODY, SADDLE};

    with_guests(|game| {
        let stack = |id| Some(ItemStack::new(id, 1));
        game.armor[0] = stack("minecraft:iron_helmet");
        game.offhand = stack("minecraft:torch");
        game.equipment[BODY] = stack("minecraft:iron_horse_armor");
        for (conn, body, saddle) in [
            (12, "minecraft:golden_horse_armor", "minecraft:saddle"),
            (
                13,
                "minecraft:diamond_horse_armor",
                "minecraft:leather_horse_armor",
            ),
        ] {
            let guest = game.lan.as_mut().unwrap().hub.guest_mut(conn).unwrap();
            guest.equipment[BODY] = stack(body);
            guest.equipment[SADDLE] = stack(saddle);
        }
        game.as_guest(12, |game| {
            assert_eq!(game.equipment[BODY], stack("minecraft:golden_horse_armor"));
            game.as_guest(13, |game| {
                assert_eq!(game.equipment[BODY], stack("minecraft:diamond_horse_armor"));
                game.equipment[SADDLE] = stack("minecraft:saddle");
            })
            .unwrap();
            assert_eq!(game.equipment[BODY], stack("minecraft:golden_horse_armor"));
            assert_eq!(game.equipment[SADDLE], stack("minecraft:saddle"));
        })
        .unwrap();
        assert_eq!(game.armor[0], stack("minecraft:iron_helmet"));
        assert_eq!(game.offhand, stack("minecraft:torch"));
        assert_eq!(game.equipment[BODY], stack("minecraft:iron_horse_armor"));
        assert_eq!(
            game.lan.as_ref().unwrap().hub.guest(13).unwrap().equipment[SADDLE],
            stack("minecraft:saddle")
        );
    });
}

/// A merchant's live session belongs to B even while A is acting.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn another_guests_live_merchant_session_is_visible_during_a_turn() {
    use super::{
        native_merchant_refs::tests::{offer, spawn},
        trading::MerchantState,
    };
    use horizon_client::merchant::{Session, Trader};

    with_guests(|game| {
        let villager = spawn(game, 40, "minecraft:villager");
        let old = offer("minecraft:wheat", "minecraft:emerald");
        let mut live = offer("minecraft:coal", "minecraft:diamond");
        // Opening the screen recomputes the special price for this guest.
        live.special_price_diff = 0;
        game.mobs[0].extra.trade = Some(Box::new(Trader {
            offers: vec![old],
            ..Trader::default()
        }));
        game.mobs[0].extra.flag2 = true;
        let mut session = Session::new(vec![live.clone()], true);
        session.xp = 17;
        session.level = 2;
        game.lan_open_merchant(
            13,
            MerchantState {
                session: Box::new(session),
                scroll: 0,
                dragging: false,
                focused: None,
                mob: Some(40),
                name: None,
            },
        );
        game.as_guest(12, |game| {
            assert_eq!(villager.offers(game), Some(vec![live]));
            assert_eq!(villager.xp(game), Some(17));
            assert_eq!(villager.level(game), Some(2));
            assert_eq!(villager.trading_player(game).map(|p| p.uuid()), Some(13));
        })
        .unwrap();
    });
}

/// Requires #235's passenger walk as well as #228's host visibility.
#[test]
#[ignore = "needs #235's passenger walk"]
fn another_guest_follows_a_vehicle_teleported_during_a_turn() {
    with_guests(|game| {
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .guest_mut(13)
            .unwrap()
            .vehicle = Some(42);
        let horse = game.res.mob_types().get("minecraft:horse").unwrap().clone();
        game.mobs
            .push(Mob::new(42, horse, Vec3::new(3.0, 64.0, 5.0), 0.0));
        // Put B in the horse's seat using the same placement as a normal turn.
        game.as_guest(13, |game| game.position_rider()).unwrap();
        let vehicle = game.native_entity(42).unwrap();
        let rider = game.native_entity(13).unwrap();
        let before = [3.0, 64.0 + (1.44375 - 0.6), 5.0];
        let after = [10.0, 70.0 + (1.44375 - 0.6), -4.0];
        game.as_guest(12, |game| {
            assert_eq!(rider.position(game), Some(before));
            assert!(vehicle.teleport_to(game, [10.0, 70.0, -4.0], None));
            assert_eq!(vehicle.position(game), Some([10.0, 70.0, -4.0]));
            assert_eq!(rider.position(game), Some(after));
            assert_eq!(
                game.lan.as_ref().unwrap().hub.guest(13).unwrap().vehicle,
                Some(42)
            );
        })
        .unwrap();
    });
}

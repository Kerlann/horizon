use horizon_client::motion::PlayerMotion;
use horizon_core::geometry::Vec3;

#[test]
#[ignore = "requires prepared resources"]
fn guest_replay_synchronizes_worn_boots() {
    mod_platform::testing::with_mods(Default::default(), || {
        crate::game::native_guest_turn_tests::with_guests(|game| {
            let mut config = horizon_anticheat::Config::default();
            config.log.clear();
            config.strikes.clear();
            config.prediction.enabled = true;
            game.lan.as_mut().unwrap().hub.set_anticheat(config);
            assert!(game.lan.as_ref().unwrap().hub.recording());
            let body = PlayerMotion::new(game.motion, Vec3::new(3.0, 64.0, 5.0)).unwrap();
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .update_guest(12, |guest| {
                    guest.motion = Some(Box::new(body));
                })
                .unwrap();
            for (item, count, expected) in [
                ("minecraft:leather_boots", 1, true),
                ("minecraft:iron_boots", 1, false),
                ("minecraft:leather_boots", 0, false),
            ] {
                game.lan
                    .as_mut()
                    .unwrap()
                    .hub
                    .update_guest(12, |guest| {
                        guest.armor[3] = Some(crate::game::ItemStack::new(item, count));
                        guest.motion.as_deref_mut().unwrap().leather_boots = !expected;
                    })
                    .unwrap();
                game.guest_replay(12);
                let actual = game
                    .lan
                    .as_ref()
                    .unwrap()
                    .hub
                    .guest(12)
                    .unwrap()
                    .motion
                    .as_deref()
                    .unwrap()
                    .leather_boots;
                assert_eq!(actual, expected, "{item}, count {count}");
                println!("{item}, count {count}: motion.leather_boots={actual}");
            }
        });
    });
}

//! Candidate completeness without an R subscription or publication.
use crate::game::{commands::Target, native_guest_turn_tests::with_guests};
use horizon_client::{join_seq::residency::table::EntityLookup, mobs::Mob};
use horizon_core::geometry::Vec3;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn no_r_roster_loans_and_canonical_relationship_writes_keep_candidates_complete() {
    with_guests(|game| {
        crate::game::native_mob_backing_tests::prepare(game);
        let level = mod_api::LevelRef::new(game.native_lifecycle.epoch().unwrap(), game.dim.key());
        let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
        for id in [40, 41, 42, 50] {
            game.mobs
                .push(Mob::new(id, kind.clone(), Vec3::new(3.0, 64.0, 5.0), 0.0));
            game.uuid_for(id);
        }
        assert!(game.ride_mount(Target::Mob(41), Target::Mob(40)).is_ok());
        assert!(
            game.begin_resident_read_pass(horizon_client::join_seq::residency::Kind::Boat)
                .is_none(),
            "no R subscription"
        );
        let candidates = |g: &crate::game::Game<'_>, vehicle| {
            g.with_prepared_mob_relations(|index| {
                index
                    .mob_rider_candidates(vehicle)
                    .map(|riders| riders.collect::<Vec<_>>())
            })
            .flatten()
        };
        assert_eq!(candidates(game, 40), Some(vec![41]));
        let mut roster = game.mobs.lift();
        assert_eq!(game.server_resident_entity(42), EntityLookup::Taken);
        crate::game::native_mob_read_context::record_backing_work(|work| {
            *work = Default::default()
        });
        assert_eq!(
            candidates(game, 40),
            Some(vec![41]),
            "unrelated Taken source is not inspected"
        );
        assert_eq!(candidates(game, 50), Some(vec![]));
        assert_eq!(
            crate::game::native_mob_read_context::backing_work().population_visits,
            0
        );
        game.mobs.restore(
            roster,
            horizon_client::join_seq::RestoreOrder::SurvivorsFirst,
        );
        game.dismount_mob(41);
        assert!(game.ride_mount(Target::Mob(41), Target::Mob(50)).is_ok());
        roster = game.mobs.lift();
        assert_eq!(candidates(game, 40), Some(vec![]));
        assert_eq!(candidates(game, 50), Some(vec![41]));
        game.mobs.restore(
            roster,
            horizon_client::join_seq::RestoreOrder::SurvivorsFirst,
        );
        assert_eq!(
            crate::game::native_mob_read_context::backing_work().population_visits,
            0,
            "small writes never rebuild the roster"
        );

        game.as_guest(12, |outer| {
            assert!(outer.start_riding_target(Target::Player, Target::Mob(40), true));
            let riders = outer.mob_guest_rider_connections(level, 40).unwrap();
            assert_eq!(riders, vec![12]);
            outer
                .as_guest(13, |inner| {
                    let riders = inner.mob_guest_rider_connections(level, 40).unwrap();
                    assert_eq!(
                        riders,
                        vec![12],
                        "outer selected relationship survives its real body loan"
                    );
                })
                .unwrap();
        })
        .unwrap();
        assert_eq!(game.mob_guest_rider_connections(level, 40), Some(vec![12]));
        game.as_guest(12, |g| g.stop_riding()).unwrap();
        assert_eq!(game.mob_guest_rider_connections(level, 40), Some(vec![]));
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .update_guest(13, |guest| guest.vehicle = Some(50));
        assert_eq!(
            game.mob_guest_rider_connections(level, 50),
            Some(vec![13]),
            "source-owned changes outside a turn do not need AI/R refresh"
        );
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .update_guest(13, |guest| guest.vehicle = None);
        assert_eq!(game.mob_guest_rider_connections(level, 50), Some(vec![]));
        game.entity_read_state.borrow_mut().mob_relations.clear();
        let held = game.mobs.lift();
        assert_eq!(
            candidates(game, 40),
            None,
            "an unprepared loan is not empty passengers"
        );
        game.mobs
            .restore(held, horizon_client::join_seq::RestoreOrder::SurvivorsFirst);
    });
}

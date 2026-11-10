use super::native_guest_turn_tests::with_guests;
use super::native_mob_backing_tests::{add, prepare};
use super::native_mob_read_context::{BackingWork, backing_work};
use horizon_client::{
    mobs::{TeamLookup, Who},
    save::uuid_text,
};
use horizon_core::geometry::Vec3;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn repeated_canonical_member_and_owner_writes_do_not_revisit_population() {
    let mut results = Vec::new();
    for population in [128, 8192] {
        with_guests(|game| {
            prepare(game);
            game.native_lifecycle.world_open_ready().unwrap();
            game.native_lifecycle.server_tick_start(true).unwrap();
            for id in 71..71 + population {
                add(
                    game,
                    id,
                    "minecraft:wolf",
                    Vec3::new(f64::from(id), 70.0, 1.0),
                    true,
                );
            }
            game.mobs[1].extra.tame = true;
            game.mobs[1].extra.owner = Some([4, 0, 0, 73]);
            for name in ["red", "blue", "green"] {
                game.score.board.add_team(name);
            }
            let member = uuid_text([4, 0, 0, 73]);
            game.score.board.add_to_team(&member, "blue");
            game.score
                .board
                .add_to_team(&uuid_text([4, 0, 0, 74]), "green");
            game.prepare_ai_mob_relations(&game.mobs);
            macro_rules! team {
                () => {
                    TeamLookup::team(game, &game.mobs[0], Who::Mob(72), None)
                        .map(|t| t.name.as_str())
                };
            }
            assert_eq!(team!(), Some("blue"));
            let before = backing_work();
            assert!(
                before.population_visits >= population as usize,
                "the actual admitted source population was exercised"
            );
            let start = std::time::Instant::now();
            let mut owner = 73;
            for iteration in 0..1000 {
                let name = if iteration % 2 == 0 { "red" } else { "blue" };
                let command = format!("team join {name} {member}");
                assert!(game.as_console(|g| g.run_command(&command)));
                assert_eq!(team!(), Some(if owner == 73 { name } else { "green" }));
                owner = if iteration % 2 == 0 { 74 } else { 73 };
                let command = format!(
                    "data merge entity {} {{Owner:[I;4,0,0,{owner}]}}",
                    uuid_text([4, 0, 0, 72])
                );
                assert!(game.as_console(|g| g.run_command(&command)));
                assert_eq!(team!(), Some(if owner == 73 { name } else { "green" }));
            }
            let elapsed = start.elapsed();
            let after = backing_work();
            let work = BackingWork {
                population_visits: after.population_visits - before.population_visits,
                source_lookups: after.source_lookups - before.source_lookups,
                owner_changes: after.owner_changes - before.owner_changes,
                player_projections: after.player_projections - before.player_projections,
            };
            assert_eq!(
                work.population_visits, 0,
                "no snapshot/index population refresh"
            );
            assert_eq!(work.owner_changes, 1000, "actual changed ownership edges");
            assert!(
                work.source_lookups >= 4000,
                "outcomes used actual canonical source validation"
            );
            eprintln!(
                "{population} actual sources: 1000 member + 1000 owner commands: {elapsed:?}; {work:?}"
            );
            results.push(work);
        });
    }
    assert_eq!(
        results[0], results[1],
        "backing work is independent of population"
    );
}

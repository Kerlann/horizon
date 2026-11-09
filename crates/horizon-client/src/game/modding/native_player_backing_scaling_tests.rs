use super::native_guest_turn_tests::with_guests;
use super::native_mob_backing::{PlayerBackingIndex, PlayerBackingLoans};
use super::native_mob_backing_tests::prepare;
use super::native_mob_read_context::backing_work;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn changed_player_commands_project_one_guest_not_the_online_population() {
    let mut totals = Vec::new();
    for population in [128, 8192] {
        with_guests(|game| {
            prepare(game);
            game.native_lifecycle.world_open_ready().unwrap();
            game.native_lifecycle.server_tick_start(true).unwrap();
            for conn in 100..100 + population - 2 {
                let mut guest = crate::game::native_item_places::tests::guest(conn);
                guest.loaded = true;
                game.lan.as_mut().unwrap().hub.guests.push(guest);
            }
            game.prepare_ai_mob_relations(&game.mobs);
            let mobs = game.mobs.lift();
            let mut guests = game.lan_guest_views();
            assert_eq!(guests.len(), population as usize);
            let untouched = guests
                .iter()
                .find(|(conn, _)| *conn == 13)
                .unwrap()
                .1
                .clone();
            let mut guest_ids: Vec<_> = game
                .lan
                .as_ref()
                .unwrap()
                .hub
                .guests
                .iter()
                .map(|g| (g.conn, g.entity_id as u32))
                .collect();
            let mut index = PlayerBackingIndex::new(&guests, &guest_ids);
            let mut local = None;
            let mut spare = guests.first().map(|(_, view)| view.clone());
            let mut nearest =
                crate::game::natural_spawning::NearestPlayers::new(&game.level_players());
            game.entity_read_state.borrow_mut().mob_ai_level = Some(game.dim.key());
            macro_rules! refresh {
                () => {
                    game.refresh_ai_player_backing(
                        &mut index,
                        PlayerBackingLoans {
                            mobs: &mobs,
                            local: &mut local,
                            spare: &mut spare,
                            guests: &mut guests,
                            guest_ids: &mut guest_ids,
                            nearest: &mut nearest,
                        },
                    )
                };
            }
            let before = backing_work();
            for iteration in 0..100 {
                let mode = if iteration % 2 == 0 {
                    "spectator"
                } else {
                    "adventure"
                };
                let command = format!("gamemode {mode} guest12");
                let work = backing_work();
                assert!(game.as_console(|g| g.run_command(&command)));
                assert_eq!(refresh!(), 1);
                let projected = backing_work().player_projections - work.player_projections;
                assert!(
                    (3..=8).contains(&projected),
                    "{population}: {projected} actual projections"
                );
                assert_eq!(
                    guests
                        .iter()
                        .find(|(conn, _)| *conn == 12)
                        .unwrap()
                        .1
                        .creative,
                    iteration % 2 == 0
                );
                let _ = game.as_console(|g| g.run_command(&command));
                assert_eq!(refresh!(), 0, "same canonical mode does not invalidate");
            }
            let after = backing_work();
            assert_eq!(after.population_visits, before.population_visits);
            assert_eq!(
                guests.iter().find(|(conn, _)| *conn == 13).unwrap().1,
                untouched
            );
            let projections = after.player_projections - before.player_projections;
            eprintln!(
                "{population} real online guests: 100 changed + 100 no-op mode commands: {projections} player projections"
            );
            totals.push(projections);
            game.entity_read_state.borrow_mut().mob_ai_level = None;
            game.mobs
                .restore(mobs, horizon_client::join_seq::RestoreOrder::SurvivorsFirst);
        });
    }
    assert_eq!(totals[0], totals[1]);
}

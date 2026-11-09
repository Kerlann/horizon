use super::native_guest_turn_tests::with_guests;
use super::native_mob_backing_tests::{add, prepare};
use horizon_client::{
    mobs::{TeamLookup, Who},
    save::uuid_text,
};
use horizon_core::geometry::Vec3;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn terminal_owners_require_current_sources_retain_loans_and_see_arrivals() {
    with_guests(|game| {
        prepare(game);
        game.native_lifecycle.world_open_ready().unwrap();
        game.native_lifecycle.server_tick_start(true).unwrap();
        for id in 71..=73 {
            add(
                game,
                id,
                "minecraft:wolf",
                Vec3::new(f64::from(id), 70.0, 1.0),
                true,
            );
        }
        game.mobs[0].extra.tame = true;
        let live = [4, 0, 0, 72];
        let removed = [4, 0, 0, 73];
        let stale = [4, 0, 0, 777];
        game.set_saved_entity_uuid(777, stale);
        let later = [4, 0, 0, 779];
        game.set_saved_entity_uuid(778, later);
        game.mobs[2].removed = true;
        game.score.board.add_team("red");
        game.score.board.add_team("blue");
        for uuid in [live, removed, stale, later] {
            game.score.board.add_to_team(&uuid_text(uuid), "red");
        }
        game.score.board.add_to_team("guest13", "blue");
        macro_rules! team {
            () => {
                TeamLookup::team(game, &game.mobs[0], Who::Mob(71), None).map(|t| t.name.as_str())
            };
        }
        for owner in [removed, stale] {
            game.mobs[0].extra.owner = Some(owner);
            assert_eq!(team!(), None, "a saved key is not admission: {owner:?}");
        }
        game.mobs[0].extra.owner = Some(live);
        assert_eq!(team!(), Some("red"));
        let held = game.mobs.lift();
        assert_eq!(
            TeamLookup::team(game, &held[0], Who::Mob(71), None).map(|t| t.name.as_str()),
            Some("red"),
            "actual admitted loans retain identity without R"
        );
        game.mobs
            .restore(held, horizon_client::join_seq::RestoreOrder::SurvivorsFirst);
        game.mobs.retain(|m| m.id != 72);
        assert!(game.persist.uuids.contains_key(&72));
        assert_eq!(team!(), None, "physical removal defeats a stale saved key");
        game.enter_level(crate::game::Dim::Nether).unwrap();
        add(game, 74, "minecraft:wolf", Vec3::new(2.0, 70.0, 1.0), true);
        game.score
            .board
            .add_to_team(&uuid_text([4, 0, 0, 74]), "red");
        game.enter_level(crate::game::Dim::Overworld).unwrap();
        game.mobs[0].extra.owner = Some([4, 0, 0, 74]);
        assert_eq!(
            team!(),
            None,
            "a foreign parked source is not a terminal owner"
        );
        let unbound = [4, 0, 0, 778];
        game.score.board.add_to_team(&uuid_text(unbound), "red");
        game.mobs[0].extra.owner = Some(unbound);
        assert_eq!(game.set_saved_entity_uuid(777, unbound), Some(stale));
        assert_eq!(
            team!(),
            None,
            "a map write cannot admit a nonexistent source"
        );
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .update_guest(13, |g| g.dimension = "minecraft:the_nether".into());
        game.mobs[0].extra.owner = Some([0, 0, 0, 13]);
        assert_eq!(
            team!(),
            Some("blue"),
            "global online profiles win across dimensions"
        );
        add(game, 75, "minecraft:wolf", Vec3::new(2.0, 70.0, 1.0), true);
        let last = game.mobs.len() - 1;
        game.mobs[last].extra.tame = true;
        game.mobs[last].extra.owner = Some([0, 0, 0, 13]);
        game.record_ai_mob_owner_id(75);
        game.score
            .board
            .add_to_team(&uuid_text([4, 0, 0, 75]), "red");
        game.mobs[0].extra.owner = Some([4, 0, 0, 75]);
        assert_eq!(
            team!(),
            Some("blue"),
            "profile precedence applies at an intermediate hop"
        );
        assert!(game.score.board.remove_from_team("guest13"));
        assert_eq!(
            team!(),
            None,
            "a real online profile without a team still wins"
        );
        game.mobs[0].extra.owner = Some(later);
        assert_eq!(team!(), None, "the early saved key is not yet a source");
        let kind = game.res.mob_types().get("minecraft:wolf").unwrap().clone();
        game.mobs
            .push(horizon_client::mobs::Mob::new(778, kind, Vec3::ZERO, 0.0));
        assert_eq!(
            team!(),
            Some("red"),
            "a newly admitted source can use its unchanged saved key"
        );
        game.mobs[0].extra.owner = Some([0, 0, 0, 14]);
        assert_eq!(team!(), None);
        let tick = game.native_lifecycle.capture_tick();
        game.score.board.add_to_team("guest14", "red");
        let mut arriving = crate::game::native_item_places::tests::guest(14);
        arriving.uuid = 14;
        arriving.loaded = true;
        game.lan.as_mut().unwrap().hub.guests.push(arriving);
        assert_eq!(game.native_lifecycle.capture_tick(), tick);
        assert_eq!(
            team!(),
            Some("red"),
            "negative lookup must not hide a same-tick arrival"
        );
        for id in 900..970 {
            add(game, id, "minecraft:wolf", Vec3::ZERO, true);
            let last = game.mobs.len() - 1;
            game.mobs[last].extra.tame = true;
            game.mobs[last].extra.owner = Some(if id == 969 {
                [0, 0, 0, 14]
            } else {
                [4, 0, 0, id as i32 + 1]
            });
            game.record_ai_mob_owner_id(id);
        }
        game.mobs[0].extra.owner = Some([4, 0, 0, 900]);
        assert_eq!(
            team!(),
            Some("red"),
            "long valid chains reach the online profile"
        );
        let last = game.mobs.len() - 1;
        game.mobs[last].extra.owner = Some([4, 0, 0, 900]);
        game.record_ai_mob_owner_id(969);
        assert_eq!(
            team!(),
            None,
            "cycles terminate without a population-sized scan"
        );
    });
}

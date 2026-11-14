//! The issue #3977 merge contract through a real `Game`: `3 x 2` and `5 x 1` become
//! `11 x 1`, which saves unchanged and pays 11 once on pickup.
use crate::game::native_entity_refs::tests::with_game;
use crate::game::native_item_places::tests::{guest, open_to_lan};
use horizon_client::entities::ExperienceOrb;
use horizon_client::entity_nbt::{orb_to_nbt, orbs_from_nbt};
use horizon_core::geometry::Vec3;
use mod_api::{ExperienceOrbCanMergeEvent, ExperienceOrbMergeEvent, Flow, Hooks, ServerHost};

fn orb(id: u32, x: f64, value: i32, count: u32) -> ExperienceOrb {
    let at = Vec3::new(x, 70.0, 0.5);
    ExperienceOrb {
        attachment_lifetime: Default::default(),
        fall_distance: 0.0,
        rotation: [0.0, 0.0],
        join: Default::default(),
        id,
        position: at,
        previous: at,
        velocity: Vec3::ZERO,
        value,
        age: 0,
        on_ground: true,
        count,
        ticks: 0,
        health: 5,
        air: 300,
        no_gravity: true,
        following: None,
    }
}

fn open(game: &mut crate::game::Game<'_>) {
    open_to_lan(game);
    let mut g = guest(1);
    g.loaded = true;
    g.position = [0.5, 70.0, 0.5];
    game.lan.as_mut().unwrap().hub.guests.push(g);
}

/// Forced merge of different values, then the exact total written back.
struct Normalize;
#[mod_api::__subscriptions]
impl Hooks for Normalize {
    fn experience_orb_can_merge(
        &self,
        _: &mut dyn ServerHost,
        event: &mut ExperienceOrbCanMergeEvent<'_>,
    ) {
        event.can_merge = true;
    }
    fn experience_orb_merge(
        &self,
        host: &mut dyn ServerHost,
        event: &mut ExperienceOrbMergeEvent<'_>,
    ) -> Flow {
        if (event.into_value, event.into_count, event.from_value, event.from_count) == (3, 2, 5, 1) {
            assert!(host.set_experience_orb(event.dimension, event.into, Some(11), Some(1), None));
        }
        Flow::Continue
    }
}

/// The merge hook says no; both orbs stay.
struct Cancel;
#[mod_api::__subscriptions]
impl Hooks for Cancel {
    fn experience_orb_can_merge(
        &self,
        _: &mut dyn ServerHost,
        event: &mut ExperienceOrbCanMergeEvent<'_>,
    ) {
        event.can_merge = true;
    }
    fn experience_orb_merge(
        &self,
        _: &mut dyn ServerHost,
        _: &mut ExperienceOrbMergeEvent<'_>,
    ) -> Flow {
        Flow::Cancel
    }
}

/// Writes the total, then changes the source so the decision no longer applies.
struct Stale;
#[mod_api::__subscriptions]
impl Hooks for Stale {
    fn experience_orb_can_merge(
        &self,
        _: &mut dyn ServerHost,
        event: &mut ExperienceOrbCanMergeEvent<'_>,
    ) {
        event.can_merge = true;
    }
    fn experience_orb_merge(
        &self,
        host: &mut dyn ServerHost,
        event: &mut ExperienceOrbMergeEvent<'_>,
    ) -> Flow {
        assert!(host.set_experience_orb(event.dimension, event.into, Some(11), Some(1), None));
        assert!(host.set_experience_orb(event.dimension, event.from, Some(6), None, None));
        Flow::Continue
    }
}

fn mods(hooks: impl Hooks + 'static) -> mod_api::Mods {
    vec![mod_api::ModEntry::new("orb-contract", Box::new(hooks))].into()
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_resident_merge_of_three_times_two_and_five_saves_and_pays_eleven() {
    let mut paid = 0;
    mod_platform::testing::with_mods(mods(Normalize), || {
        with_game(|game| {
            open(game);
            game.orbs.push(orb(1, 0.5, 3, 2));
            game.orbs.push(orb(41, 0.7, 5, 1));
            game.merge_orb(0);
            assert_eq!(game.orbs.len(), 1);
            assert_eq!((game.orbs[0].value, game.orbs[0].count), (11, 1));
            let saved = orb_to_nbt(&game.orbs[0], [0; 4]);
            let back = orbs_from_nbt(&saved);
            assert_eq!((back[0].value, back[0].count), (11, 1));
            game.guest_orbs(0);
            assert!(game.orbs.is_empty());
            paid = game.lan.as_ref().unwrap().hub.guests[0].xp_total;
        });
    });
    assert_eq!(paid, 11);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_spawning_merge_of_three_times_two_and_five_saves_and_pays_eleven() {
    let mut paid = 0;
    mod_platform::testing::with_mods(mods(Normalize), || {
        with_game(|game| {
            open(game);
            game.orbs.push(orb(41, 0.5, 3, 2));
            game.award_orb([0.5, 70.0, 0.5], 5, 1);
            assert_eq!(game.orbs.len(), 1);
            assert_eq!((game.orbs[0].value, game.orbs[0].count), (11, 1));
            let back = orbs_from_nbt(&orb_to_nbt(&game.orbs[0], [0; 4]));
            assert_eq!((back[0].value, back[0].count), (11, 1));
            game.guest_orbs(0);
            assert!(game.orbs.is_empty());
            paid = game.lan.as_ref().unwrap().hub.guests[0].xp_total;
        });
    });
    assert_eq!(paid, 11);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_canceled_merge_keeps_both_orbs() {
    mod_platform::testing::with_mods(mods(Cancel), || {
        with_game(|game| {
            game.orbs.push(orb(1, 0.5, 3, 2));
            game.orbs.push(orb(41, 0.7, 5, 1));
            game.merge_orb(0);
            let left: Vec<_> = game.orbs.iter().map(|o| (o.id, o.count)).collect();
            assert_eq!(left, [(1, 2), (41, 1)]);
        });
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_changed_source_refuses_the_merge_and_puts_the_written_orb_back() {
    mod_platform::testing::with_mods(mods(Stale), || {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            game.orbs.push(orb(1, 0.5, 3, 2));
            game.orbs.push(orb(41, 0.7, 5, 1));
            game.merge_orb(0);
            let left: Vec<_> = game.orbs.iter().map(|o| (o.value, o.count)).collect();
            assert_eq!(left, [(3, 2), (6, 1)]);
        });
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_count_past_what_a_save_holds_refuses_the_merge() {
    with_game(|game| {
        game.orbs.push(orb(1, 0.5, 3, i32::MAX as u32));
        game.orbs.push(orb(41, 0.7, 3, 1));
        game.merge_orb(0);
        assert_eq!(game.orbs.len(), 2);
        assert_eq!(game.orbs[0].count, i32::MAX as u32);
    });
}

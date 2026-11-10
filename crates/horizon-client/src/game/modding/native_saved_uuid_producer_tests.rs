use super::native_mob_read_context::NativeMobRelations;

#[test]
fn partial_uuid_deltas_do_not_certify_an_unseeded_store() {
    let mut index = NativeMobRelations::default();
    let uuid = [0, 0, 0, 99];
    assert_eq!(index.saved_uuid_candidates(uuid), None);
    index.update_saved_uuid(40, None, Some(uuid));
    assert_eq!(index.saved_uuid_candidates(uuid), None);
    index.clear();
    assert_eq!(index.saved_uuid_candidates(uuid), None);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn actual_saved_store_seed_duplicates_changes_removals_and_missing_owner_are_explicit() {
    super::native_guest_turn_tests::with_guests(|game| {
        super::native_mob_backing_tests::prepare(game);
        super::native_merchant_refs::tests::spawn(game, 40, "minecraft:villager");
        let epoch = game.native_lifecycle.epoch().unwrap();
        let level = mod_api::LevelRef::new(epoch, game.native_level_key());
        let uuid = [0, 0, 0, 99];
        let next = [0, 0, 0, 100];
        game.set_saved_entity_uuid(40, uuid);
        game.set_saved_entity_uuid(999, uuid); // Loose saved key, not an admitted identity.
        game.with_prepared_mob_relations(|index| {
            let mut candidates = index.saved_uuid_candidates(uuid).unwrap();
            candidates.sort_unstable();
            assert_eq!(candidates, vec![40, 999]);
            assert_eq!(index.saved_uuid_candidates(next), Some(vec![]));
        })
        .unwrap();
        assert_eq!(
            game.current_admitted_saved_id(uuid),
            Some(40),
            "loose duplicate cannot hide the valid source"
        );
        super::native_mob_read_context::record_backing_work(|work| *work = Default::default());
        game.set_saved_entity_uuid(999, next);
        game.set_saved_entity_uuid(999, next); // Real no-op assignment.
        game.with_mob_read_relations(level, |index| {
            assert_eq!(index.saved_uuid_candidates(uuid), Some(vec![40]));
            assert_eq!(index.saved_uuid_candidates(next), Some(vec![999]));
        })
        .unwrap();
        game.remove_saved_entity_uuid(999);
        game.with_mob_read_relations(level, |index| {
            assert_eq!(index.saved_uuid_candidates(uuid), Some(vec![40]));
            assert_eq!(index.saved_uuid_candidates(next), Some(vec![]));
        })
        .unwrap();
        assert_eq!(
            super::native_mob_read_context::backing_work().population_visits,
            0
        );
        let mut index = NativeMobRelations::default();
        let missing = mod_api::LevelRef::new(epoch, mod_api::DimensionKey::END);
        assert!(game.levels.parked_of(missing.dimension_id()).is_none());
        game.rebuild_mob_read_relations(missing, &mut index, true);
        assert_eq!(
            index.saved_uuid_candidates(uuid),
            None,
            "missing saved store is not an empty store"
        );
        game.rebuild_mob_read_relations(level, &mut index, true);
        assert_eq!(index.saved_uuid_candidates(uuid), Some(vec![40]));
        index.clear();
        assert_eq!(index.saved_uuid_candidates(uuid), None);
    });
}

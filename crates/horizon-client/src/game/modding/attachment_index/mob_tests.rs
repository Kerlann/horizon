use super::*;
use crate::game::native_entity_refs::tests::with_game;
use horizon_core::nbt::Tag;
use mod_api::{AttachmentQueries, AttachmentQueriesMut, AttachmentType};

static VALUE: AttachmentType<i32> = AttachmentType {
    namespace: "test",
    name: "mana",
    default: || 0,
    codec: None,
    copy_on_death: false,
    sync: false,
};

fn mob(uuid: [i32; 4]) -> Tag {
    Tag::compound([
        ("id", Tag::String("minecraft:cow".into())),
        ("UUID", Tag::IntArray(uuid.to_vec())),
        (
            "Pos",
            Tag::List(vec![
                Tag::Double(1.0f64.to_bits()),
                Tag::Double(70.0f64.to_bits()),
                Tag::Double(3.0f64.to_bits()),
            ]),
        ),
        (
            horizon_mod_runtime::attachments::ATTACHMENTS_KEY,
            Tag::compound([(
                "test",
                Tag::compound([(
                    "mana",
                    Tag::compound([("value", Tag::Compound(Default::default()))]),
                )]),
            )]),
        ),
    ])
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn saved_mob_binding_survives_store_back_and_retires_at_sweep() {
    with_game(|game| {
        game.options.dedicated = Some(Vec::new());
        game.native_level_opened(Dim::Overworld);
        let uuid = [17, 18, 19, 20];
        let id = game.load_mob(mob(uuid), None).unwrap();
        // Check residency before a typed read decodes the saved entry.
        let index = game.attachment_indexes[&Dim::Overworld].clone();
        let key = ("test".into(), "mana".into());
        assert_eq!(index.lock().unwrap().snapshot(&key).len(), 1);
        VALUE.set(game, Holder::Entity(id), 7).unwrap();
        let old = game.query_mut(&VALUE).unwrap();
        let row = old.iter().next().unwrap().unwrap();
        let work = index.lock().unwrap().work_count();
        let replacement = game.mobs[0].clone();
        horizon_client::join_seq::store_back(&mut game.mobs[0], replacement);
        assert!(game.mobs[0].attachment_lifetime.is_bound());
        let after = index.lock().unwrap().work_count();
        assert_eq!(after, work);
        assert_eq!(index.lock().unwrap().snapshot(&key).len(), 1);
        assert_eq!(VALUE.get(game, Holder::Entity(id)), Ok(7));
        assert_eq!(
            game.query(&VALUE)
                .unwrap()
                .iter()
                .next()
                .unwrap()
                .unwrap()
                .value,
            &7
        );
        let entity = game.entity(id).unwrap();
        assert!(game.entity_discard(entity));
        assert_eq!(index.lock().unwrap().snapshot(&key).len(), 1);
        game.native_removals_flush();
        game.world
            .chunks
            .insert((0, 0), Arc::new(horizon_client::chunk::Chunk::empty(0, 0)));
        game.tickets.update(None);
        game.tick_creature_entities().unwrap();
        assert!(game.mobs.is_empty());
        assert!(index.lock().unwrap().snapshot(&key).is_empty());
        assert_eq!(
            row.set(game, 9),
            Err(mod_api::AttachmentRowError::StaleHolder)
        );
        assert_eq!(*row.value(), 7);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn saved_mob_binding_tracks_serialized_transfer_and_parked_levels() {
    with_game(|game| {
        game.options.dedicated = Some(Vec::new());
        game.native_level_opened(Dim::Overworld);
        let uuid = [21, 22, 23, 24];
        let id = game.load_mob(mob(uuid), None).unwrap();
        let source = game.attachment_indexes[&Dim::Overworld].clone();
        let key = ("test".into(), "mana".into());
        assert_eq!(source.lock().unwrap().snapshot(&key).len(), 1);
        VALUE.set(game, Holder::Entity(id), 11).unwrap();
        let batch = game.query_mut(&VALUE).unwrap();
        let row = batch.iter().next().unwrap().unwrap();
        game.enter_level(Dim::Nether).unwrap();
        assert_eq!(source.lock().unwrap().snapshot(&key).len(), 1);
        game.enter_level(Dim::Overworld).unwrap();
        assert_eq!(VALUE.get(game, Holder::Entity(id)), Ok(11));
        // Serialized portals discard the source owner before destination load.
        game.mobs.retain(|m| m.id != id);
        game.detach_entity(id);
        assert!(source.lock().unwrap().snapshot(&key).is_empty());
        game.enter_level(Dim::Nether).unwrap();
        let moved = game.load_mob(mob(uuid), None).unwrap();
        assert_ne!(moved, id); // This route allocates; retained-id vehicles have their own arrival seam.
        assert_eq!(
            game.attachment_indexes[&Dim::Nether]
                .lock()
                .unwrap()
                .snapshot(&key)
                .len(),
            1
        );
        assert_eq!(VALUE.get(game, Holder::Entity(moved)), Ok(11));
        assert_eq!(
            row.set(game, 12),
            Err(mod_api::AttachmentRowError::StaleHolder)
        );
        assert_eq!(*row.value(), 11);
        game.mobs.clear();
        assert!(
            game.attachment_indexes[&Dim::Nether]
                .lock()
                .unwrap()
                .snapshot(&key)
                .is_empty()
        );
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn saved_mob_binding_rebinds_retained_id_with_a_new_generation() {
    with_game(|game| {
        game.options.dedicated = Some(Vec::new());
        game.native_level_opened(Dim::Overworld);
        let id = game.load_mob(mob([25, 26, 27, 28]), None).unwrap();
        VALUE.set(game, Holder::Entity(id), 13).unwrap();
        let batch = game.query_mut(&VALUE).unwrap();
        let row = batch.iter().next().unwrap().unwrap();
        let holder = game.entity(id).unwrap();
        let source = game.attachment_indexes[&Dim::Overworld].clone();
        let key = ("test".into(), "mana".into());
        // The retained-owner route retires before carrying the value to arrival.
        let mut owner = game.mobs.remove(0);
        owner.attachment_lifetime.retire();
        assert!(source.lock().unwrap().snapshot(&key).is_empty());
        game.enter_level(Dim::Nether).unwrap();
        game.rebind_transferred_entity(Dim::Overworld, id, &mut owner);
        game.mobs.push(owner);
        let current = game.attachment_indexes[&Dim::Nether]
            .lock()
            .unwrap()
            .snapshot(&key);
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].holder, AttachmentHolder::Entity(holder));
        assert_eq!(current[0].entry.read(&VALUE), Ok(&13));
        assert_eq!(
            row.set(game, 14),
            Err(mod_api::AttachmentRowError::StaleHolder)
        );
        assert_eq!(*row.value(), 13);
        game.mobs.clear();
        assert!(
            game.attachment_indexes[&Dim::Nether]
                .lock()
                .unwrap()
                .snapshot(&key)
                .is_empty()
        );
    });
}

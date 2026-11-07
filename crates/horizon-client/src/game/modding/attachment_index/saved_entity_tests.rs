use super::*;
use crate::game::native_entity_refs::tests::with_game;
use horizon_core::nbt::Tag;
use mod_api::{AttachmentQueriesMut, AttachmentType};

static VALUE: AttachmentType<i32> = AttachmentType {
    namespace: "saved_entity",
    name: "value",
    default: || 0,
    codec: None,
    copy_on_death: false,
    sync: false,
};

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn saved_entity_loaders_publish_and_retire_the_authoritative_owner() {
    with_game(|game| {
        game.options.dedicated = Some(Vec::new());
        game.native_level_opened(Dim::Overworld);
        for (n, kind) in [
            "item",
            "experience_orb",
            "armor_stand",
            "item_frame",
            "oak_boat",
            "minecart",
            "arrow",
            "small_fireball",
            "snowball",
        ]
        .into_iter()
        .enumerate()
        {
            let uuid = [31, 32, 33, n as i32];
            let tag = Tag::compound([
                (
                    "id",
                    Tag::String(format!("minecraft:{kind}").as_str().into()),
                ),
                ("UUID", Tag::IntArray(uuid.to_vec())),
                (
                    "Pos",
                    Tag::List(
                        [1.0f64, 70.0, 3.0]
                            .map(|n| Tag::Double(n.to_bits()))
                            .to_vec(),
                    ),
                ),
                ("block_pos", Tag::IntArray(vec![1, 70, 3])),
                (
                    "Item",
                    Tag::compound([
                        ("id", Tag::String("minecraft:stone".into())),
                        ("count", Tag::Int(1)),
                    ]),
                ),
                (
                    horizon_mod_runtime::attachments::ATTACHMENTS_KEY,
                    Tag::compound([(
                        "saved_entity",
                        Tag::compound([(
                            "value",
                            Tag::compound([("value", Tag::Compound(Default::default()))]),
                        )]),
                    )]),
                ),
            ]);
            match kind {
                "item" => game.load_item(tag, Some(uuid)),
                "experience_orb" => game.load_orb(tag, Some(uuid)),
                "armor_stand" => game.load_armor_stand(tag, Some(uuid)),
                "item_frame" => game.load_item_frame(tag, Some(uuid)),
                "oak_boat" => game.load_boat(tag, Some(uuid), "minecraft:oak_boat"),
                "minecart" => game.load_cart(tag, Some(uuid), "minecraft:minecart"),
                "arrow" => game.load_arrow(tag, Some(uuid), None),
                "small_fireball" => game.load_fireball(tag, Some(uuid), None),
                "snowball" => game.load_throwable(tag, Some(uuid), None),
                _ => unreachable!(),
            }
            let key = ("saved_entity".into(), "value".into());
            let index = game.attachment_indexes[&Dim::Overworld].clone();
            let saved = index.lock().unwrap().snapshot(&key);
            assert_eq!(saved.len(), 1, "{kind} before typed access");
            let AttachmentHolder::Entity(entity) = saved[0].holder else {
                panic!("not entity");
            };
            assert_ne!(entity.id(), 0, "{kind}");
            VALUE.set(game, Holder::Entity(entity.id()), 19).unwrap();
            let old = game.query_mut(&VALUE).unwrap();
            let row = old.iter().next().unwrap().unwrap();
            let work = index.lock().unwrap().work_count();
            // Exercise the clone-replay boundary used by vehicle/projectile ticks.
            match kind {
                "oak_boat" => {
                    let copy = game.boats[0].clone();
                    assert!(game.boats.store_back(0, copy));
                }
                "minecart" => {
                    let copy = game.carts[0].clone();
                    assert!(game.carts.store_back(0, copy));
                }
                "arrow" | "small_fireball" | "snowball" => {
                    let copy = game.creatures.projectiles[0].clone();
                    assert!(game.creatures.projectiles.store_back(0, copy));
                }
                _ => (),
            }
            assert_eq!(
                index.lock().unwrap().work_count(),
                work,
                "{kind} store_back"
            );
            assert_eq!(
                VALUE.get(game, Holder::Entity(entity.id())),
                Ok(19),
                "{kind}"
            );
            game.enter_level(Dim::Nether).unwrap();
            assert_eq!(
                index.lock().unwrap().snapshot(&key).len(),
                1,
                "{kind} parked"
            );
            game.enter_level(Dim::Overworld).unwrap();
            assert!(game.entity_discard(entity), "{kind}");
            game.native_removals_flush();
            assert!(
                index.lock().unwrap().snapshot(&key).is_empty(),
                "{kind} sweep"
            );
            assert_eq!(
                row.set(game, 20),
                Err(mod_api::AttachmentRowError::StaleHolder),
                "{kind}"
            );
            assert_eq!(*row.value(), 19);
        }
    });
}

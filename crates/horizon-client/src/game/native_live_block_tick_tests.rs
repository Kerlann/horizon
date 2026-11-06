//! Live tick reads, ordinary nested rollback, and removal-time contents.
use super::native_live_holder_tests::{mod_holder, with_parts};
use horizon_client::containers::ModContainer;
use mod_api::{
    AttachmentType, BlockEntityPart, BlockEntityTickEvent, BlockKey, BlockPart, Holder, Hooks,
    HostTransaction, ItemStack, LevelRandomHost, NativeBlockUpdateFlags, PreRemoveSideEffectsEvent,
    Registrar, RegistrationContext, ServerHost,
};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

const MARKER: AttachmentType<u32> = AttachmentType {
    namespace: "live_bench",
    name: "tick",
    default: || 0,
    codec: None,
    copy_on_death: false,
    sync: false,
};
struct ProbeMod(bool, Arc<AtomicU32>);
#[mod_api::__subscriptions]
impl Hooks for ProbeMod {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        let replacement = r.block_key("minecraft:granite");
        let kind = r.block_entity_type("ticker").blocks(&[]).build();
        r.block_entity_part(
            &kind,
            ProbePart {
                remove: self.0,
                replacement,
                asked: self.1.clone(),
            },
        )
        .unwrap();
    }
}
struct ProbePart {
    remove: bool,
    replacement: BlockKey,
    asked: Arc<AtomicU32>,
}
#[mod_api::horizon_part]
impl BlockPart for ProbePart {}
#[mod_api::horizon_part]
impl BlockEntityPart for ProbePart {
    fn block_entity_tick(&self, host: &mut dyn ServerHost, event: &mut BlockEntityTickEvent) {
        let entity = event.entity;
        let level = entity.level(host).unwrap();
        assert_eq!(host.block_entity(level, event.pos), Some(entity));
        assert_eq!(entity.data_slots(host), Some(vec![0]));
        let diamonds = ItemStack::new("minecraft:diamond", 3);
        assert_eq!(entity.item_slot(host, 0), Ok(Some(diamonds.clone())));
        let holder = Holder::BlockEntity(event.pos);
        let aborted: Result<(), ()> = host.transaction(|host, _| {
            assert!(entity.set_data_slot(host, 0, 1));
            let child: Result<(), ()> = host.transaction(|host, tx| {
                assert!(entity.set_data_slot(host, 0, 2));
                assert_eq!(
                    entity.set_item_slot(host, tx, 0, Some(ItemStack::new("minecraft:diamond", 4))),
                    Ok(true)
                );
                MARKER.set(host, holder, 55).unwrap();
                Ok(())
            });
            child.unwrap();
            assert_eq!(entity.data_slots(host), Some(vec![2]));
            Err(())
        });
        assert_eq!(aborted, Err(()));
        assert_eq!(entity.data_slots(host), Some(vec![0]));
        assert_eq!(entity.item_slot(host, 0), Ok(Some(diamonds)));
        assert_eq!(MARKER.get_existing(host, holder), Ok(None));
        let committed: Result<(), ()> = host.transaction(|host, tx| {
            assert!(entity.set_data_slot(host, 0, 7));
            assert_eq!(
                entity.set_item_slot(host, tx, 0, Some(ItemStack::new("minecraft:diamond", 5))),
                Ok(true)
            );
            MARKER.set(host, holder, 55).unwrap();
            Ok(())
        });
        committed.unwrap();
        if self.remove {
            let state = host.block_state_default(self.replacement).unwrap();
            assert!(level.set_block(host, event.pos, state, NativeBlockUpdateFlags::KNOWN_SHAPE));
            assert_eq!(entity.data_slots(host), None);
        }
    }
    fn pre_remove_side_effects(
        &self,
        host: &mut dyn LevelRandomHost,
        event: &mut PreRemoveSideEffectsEvent<'_>,
    ) {
        self.asked.fetch_add(1, Ordering::Relaxed);
        let pos = event.pos;
        assert_eq!(
            MARKER.get_existing(host, Holder::BlockEntity([pos.x, pos.y, pos.z])),
            Ok(Some(55))
        );
        event.drops.push(ItemStack::new("minecraft:gold_ingot", 55));
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn own_tick_reads_writes_and_removal_reach_live_storage() {
    for remove in [false, true] {
        let asked = Arc::new(AtomicU32::new(0));
        with_parts(Box::new(ProbeMod(remove, asked.clone())), |root| {
            super::native_entity_refs::tests::with_game_at(root, |game| {
                let entity = mod_holder(game);
                let state = game.world.block(1, 64, 1).0;
                game.world.block_entities.set_mod_containers(
                    [(
                        state,
                        Arc::new(ModContainer {
                            slots: 1,
                            faces: std::array::from_fn(|_| vec![0]),
                            inputs: vec![0],
                            outputs: vec![0],
                            menu: None,
                        }),
                    )]
                    .into_iter()
                    .collect(),
                );
                assert_eq!(entity.set_data_slots(game, vec![0]), Ok(true));
                let initialized: Result<(), ()> =
                    (game as &mut dyn ServerHost).transaction(|host, tx| {
                        assert_eq!(
                            entity.set_item_slot(
                                host,
                                tx,
                                0,
                                Some(ItemStack::new("minecraft:diamond", 3))
                            ),
                            Ok(true)
                        );
                        Ok(())
                    });
                initialized.unwrap();
                game.tick_mod_block_entity((1, 64, 1));
                assert!(game.transactions.is_empty());
                if remove {
                    assert!(!game.world.block_entities.contains_key(&(1, 64, 1)));
                    assert_eq!(entity.data_slots(game), None);
                    let count = |id: &str| {
                        game.dropped
                            .iter()
                            .filter(|item| &*item.stack.item == id)
                            .map(|item| item.stack.count)
                            .sum::<u32>()
                    };
                    assert_eq!(
                        count("minecraft:diamond"),
                        5,
                        "drop committed current slots, not the pre-tick 3"
                    );
                    assert_eq!(count("minecraft:gold_ingot"), 55);
                    game.tick_mod_block_entity((1, 64, 1));
                    assert_eq!(
                        asked.load(Ordering::Relaxed),
                        1,
                        "no put-back or second removal"
                    );
                } else {
                    assert_eq!(entity.data_slots(game), Some(vec![7]));
                    assert_eq!(
                        entity.item_slot(game, 0),
                        Ok(Some(ItemStack::new("minecraft:diamond", 5)))
                    );
                    assert_eq!(asked.load(Ordering::Relaxed), 0);
                }
            });
        });
    }
}

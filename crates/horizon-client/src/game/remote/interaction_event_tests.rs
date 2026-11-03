//! Behavioural proofs of the broadcast events around scoped interaction Parts.
use super::super::{
    Game, Hit, execution_role::ExecutionRole, native_entity_refs::tests::with_game,
};
use crate::mod_api::{
    self, BlockPart, Flow, Hooks, HostSide, ItemPart, LogicalSide,
    NativeInteractionResult as Answer, NativeTriState, Registrar, RightClickBlockEvent,
    RightClickItemEvent, ServerHost, UseItemEvent, UseItemOnBlockEvent, UseItemOnEvent, UseOnEvent,
    UsePhase,
};
use horizon_client::{chunk::Chunk, first_person::Hand, items::ItemStack, motion::PlayerMotion};
use horizon_core::geometry::Vec3;
use horizon_protocol::connection::Pipe;
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Phase(UsePhase, LogicalSide),
    BlockPart,
    UseOnPart,
    Air(LogicalSide, Hand),
    ItemPart,
}

#[derive(Default)]
struct Controls {
    steps: Vec<Step>,
    block_answer: Option<Answer>,
    item_on_answer: Option<Answer>,
    air_answer: Option<Answer>,
    expected_hand: Option<Hand>,
    use_block: NativeTriState,
    use_item: NativeTriState,
}

#[derive(Clone, Default)]
struct Observer(Arc<Mutex<Controls>>);

fn side(host: HostSide<'_>) -> LogicalSide {
    match host {
        HostSide::Server(_) => LogicalSide::Server,
        HostSide::Client(_) => LogicalSide::Client,
    }
}

#[mod_api::__subscriptions]
impl Hooks for Observer {
    fn register(&self, r: &mut Registrar) {
        for block in ["minecraft:lever", "minecraft:stone"] {
            r.block_part(block, self.clone());
        }
        for item in ["minecraft:stick", "minecraft:shield"] {
            r.item_part(item, self.clone());
        }
    }
    fn right_click_block(&self, _: HostSide<'_>, event: &mut RightClickBlockEvent<'_>) -> Flow {
        let state = self.0.lock().unwrap();
        *event.use_block = state.use_block;
        *event.use_item = state.use_item;
        Flow::Continue
    }
    fn use_item_on_block(&self, host: HostSide<'_>, event: &mut UseItemOnBlockEvent<'_>) -> Flow {
        assert_eq!(side(host), event.click.side);
        assert_eq!(event.click.pos, [0, 64, 0]);
        assert_eq!(event.click.hit.face, [0, 1, 0]);
        assert_eq!(*event.cancellation_result, Answer::TryWithEmptyHand);
        let mut state = self.0.lock().unwrap();
        assert_eq!(event.click.hand, state.expected_hand.unwrap_or(Hand::Main));
        state.steps.push(Step::Phase(event.phase, event.click.side));
        let answer = match event.phase {
            UsePhase::Block => state.block_answer,
            UsePhase::ItemAfterBlock => state.item_on_answer,
            UsePhase::ItemBeforeBlock => panic!("onItemUseFirst has no firing point yet"),
        };
        if let Some(answer) = answer {
            *event.cancellation_result = answer;
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
    fn right_click_item(&self, host: HostSide<'_>, event: &mut RightClickItemEvent<'_>) -> Flow {
        assert_eq!(side(host), event.click.side);
        assert_eq!(*event.cancellation_result, Answer::Pass);
        let mut state = self.0.lock().unwrap();
        state
            .steps
            .push(Step::Air(event.click.side, event.click.hand));
        if let Some(answer) = state.air_answer {
            *event.cancellation_result = answer;
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
}
impl BlockPart for Observer {
    fn use_item_on(&self, _: &mut dyn ServerHost, _: &mut UseItemOnEvent<'_>) -> Option<Answer> {
        let mut state = self.0.lock().unwrap();
        state.steps.push(Step::BlockPart);
        None
    }
}
impl ItemPart for Observer {
    fn use_on(&self, _: &mut dyn ServerHost, _: &mut UseOnEvent<'_>) -> Flow {
        self.0.lock().unwrap().steps.push(Step::UseOnPart);
        Flow::Cancel
    }
    fn use_item(&self, _: &mut dyn ServerHost, _: &mut UseItemEvent<'_>) -> Flow {
        self.0.lock().unwrap().steps.push(Step::ItemPart);
        Flow::Cancel
    }
}

// The same real Game runs either its server path or its normal local-client
// connection/prediction path, as the existing remote menu fixture does.
fn with_interaction_game(client: bool, test: impl FnOnce(&mut Game<'_>)) {
    with_game(|game| {
        let _peer = if client {
            game.options.execution_role = ExecutionRole::Client;
            let (client, server) = Pipe::pair();
            game.connect_local(client, "InteractionTest").unwrap();
            Some(server)
        } else {
            game.options.dedicated = Some(Vec::new());
            None
        };
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 3.5)).unwrap());
        game.set_mode_id(1);
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.inventory[game.selected] = Some(ItemStack::new("minecraft:stick", 1));
        test(game);
    });
}

fn hit() -> Hit {
    Hit {
        pos: [0, 64, 0],
        face: [0, 1, 0],
        point: [0.5, 65.0, 0.5],
        inside: false,
    }
}

fn use_block(game: &mut Game<'_>, client: bool) -> bool {
    if client {
        game.remote_use_item_on(Hand::Main)
    } else {
        game.guest_use = Some(super::super::GuestUse::OnBlock(Hand::Main));
        game.use_hand(Some(&hit())).unwrap()
    }
}

#[test]
#[ignore = "requires prepared game resources"]
fn phase_events_replace_the_behaviour_before_its_part_on_both_sides() {
    let observer = Observer::default();
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "phase_observer",
        Box::new(observer.clone()),
    )]
    .into();
    mod_api::install_parts(&mods).unwrap();
    mod_api::testing::with_mods(mods, || {
        for client in [false, true] {
            with_interaction_game(client, |game| {
                let side = if client {
                    LogicalSide::Client
                } else {
                    LogicalSide::Server
                };
                let registry = game.pack.assets.registry;
                let stone = registry.default_state("minecraft:stone").unwrap();
                let lever = game
                    .with_props(
                        registry.default_state("minecraft:lever").unwrap(),
                        &[("face", "floor")],
                    )
                    .unwrap();
                game.world.set(0, 63, 0, stone).unwrap();
                game.world.set(0, 64, 0, lever).unwrap();
                game.forced_hit = Some(hit());
                *observer.0.lock().unwrap() = Controls::default();
                assert!(use_block(game, client));
                assert_ne!(game.world.block(0, 64, 0), lever);
                let mut expected = vec![Step::Phase(UsePhase::Block, side)];
                if !client {
                    expected.push(Step::BlockPart);
                }
                assert_eq!(observer.0.lock().unwrap().steps, expected);

                for answer in [Answer::Success, Answer::SuccessServer, Answer::Consume] {
                    game.world.set(0, 64, 0, lever).unwrap();
                    *observer.0.lock().unwrap() = Controls {
                        block_answer: Some(answer),
                        ..Controls::default()
                    };
                    let swings = game.swings_started;
                    assert!(use_block(game, client));
                    assert_eq!(game.world.block(0, 64, 0), lever);
                    assert_eq!(
                        observer.0.lock().unwrap().steps,
                        [Step::Phase(UsePhase::Block, side)]
                    );
                    let swings = game.swings_started - swings;
                    assert_eq!(
                        swings,
                        u32::from(
                            answer == Answer::Success
                                || (!client && answer == Answer::SuccessServer)
                        )
                    );
                }

                // TRY skips the item method/Part, but still delegates the
                // block's empty method on the main hand, on both sides.
                *observer.0.lock().unwrap() = Controls {
                    block_answer: Some(Answer::TryWithEmptyHand),
                    ..Controls::default()
                };
                assert!(use_block(game, client));
                assert_ne!(game.world.block(0, 64, 0), lever);
                assert_eq!(
                    observer.0.lock().unwrap().steps,
                    [Step::Phase(UsePhase::Block, side)]
                );

                for answer in [Answer::Pass, Answer::Fail] {
                    game.world.set(0, 64, 0, lever).unwrap();
                    *observer.0.lock().unwrap() = Controls {
                        block_answer: Some(answer),
                        item_on_answer: Some(Answer::Consume),
                        ..Controls::default()
                    };
                    assert!(use_block(game, client));
                    assert_eq!(game.world.block(0, 64, 0), lever);
                    assert_eq!(
                        observer.0.lock().unwrap().steps,
                        [
                            Step::Phase(UsePhase::Block, side),
                            Step::Phase(UsePhase::ItemAfterBlock, side),
                        ]
                    );
                }

                // A passing block gets the item phase, before the item Part.
                game.world.set(0, 64, 0, stone).unwrap();
                *observer.0.lock().unwrap() = Controls::default();
                let _ = use_block(game, client);
                let mut expected = vec![Step::Phase(UsePhase::Block, side)];
                if !client {
                    expected.push(Step::BlockPart);
                }
                expected.push(Step::Phase(UsePhase::ItemAfterBlock, side));
                if !client {
                    expected.push(Step::UseOnPart);
                }
                assert_eq!(observer.0.lock().unwrap().steps, expected);

                for answer in [
                    Answer::Success,
                    Answer::SuccessServer,
                    Answer::Consume,
                    Answer::Pass,
                    Answer::Fail,
                    Answer::TryWithEmptyHand,
                ] {
                    *observer.0.lock().unwrap() = Controls {
                        item_on_answer: Some(answer),
                        ..Controls::default()
                    };
                    let swings = game.swings_started;
                    assert_eq!(
                        use_block(game, client),
                        answer.consumes_action() || answer == Answer::Fail
                    );
                    let mut expected = vec![Step::Phase(UsePhase::Block, side)];
                    if !client {
                        expected.push(Step::BlockPart);
                    }
                    expected.push(Step::Phase(UsePhase::ItemAfterBlock, side));
                    assert_eq!(observer.0.lock().unwrap().steps, expected);
                    assert_eq!(
                        game.swings_started - swings,
                        u32::from(
                            answer == Answer::Success
                                || (!client && answer == Answer::SuccessServer)
                        )
                    );
                }

                // The phase event is later than RightClickBlock's choices.
                *observer.0.lock().unwrap() = Controls {
                    use_block: NativeTriState::False,
                    use_item: NativeTriState::False,
                    ..Controls::default()
                };
                assert!(!use_block(game, client));
                assert!(observer.0.lock().unwrap().steps.is_empty());

                game.offhand = Some(ItemStack::new("minecraft:stick", 1));
                *observer.0.lock().unwrap() = Controls {
                    block_answer: Some(Answer::Success),
                    expected_hand: Some(Hand::Off),
                    ..Controls::default()
                };
                let used = if client {
                    game.remote_use_item_on(Hand::Off)
                } else {
                    game.guest_use = Some(super::super::GuestUse::OnBlock(Hand::Off));
                    game.with_hand(Hand::Off, |g| g.use_hand(Some(&hit())).unwrap())
                };
                assert!(used);
                assert_eq!(
                    observer.0.lock().unwrap().steps,
                    [Step::Phase(UsePhase::Block, side)]
                );
            });
        }
    });
}

#[test]
#[ignore = "requires prepared game resources"]
fn air_event_skips_the_item_and_non_success_client_answers_try_the_other_hand() {
    let observer = Observer::default();
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "air_observer",
        Box::new(observer.clone()),
    )]
    .into();
    mod_api::install_parts(&mods).unwrap();
    mod_api::testing::with_mods(mods, || {
        for client in [false, true] {
            with_interaction_game(client, |game| {
                let side = if client {
                    LogicalSide::Client
                } else {
                    LogicalSide::Server
                };
                for answer in [
                    Answer::Success,
                    Answer::SuccessServer,
                    Answer::Consume,
                    Answer::Pass,
                    Answer::Fail,
                    Answer::TryWithEmptyHand,
                ] {
                    *observer.0.lock().unwrap() = Controls {
                        air_answer: Some(answer),
                        ..Controls::default()
                    };
                    let used = if client {
                        game.remote_use_item(Hand::Main)
                    } else {
                        game.use_hand(None).unwrap()
                    };
                    assert_eq!(
                        used,
                        if client {
                            answer == Answer::Success
                        } else {
                            answer.consumes_action()
                        }
                    );
                    assert_eq!(
                        observer.0.lock().unwrap().steps,
                        [Step::Air(side, Hand::Main)]
                    );
                }
                if client {
                    *observer.0.lock().unwrap() = Controls {
                        air_answer: Some(Answer::Consume),
                        ..Controls::default()
                    };
                    game.offhand = Some(ItemStack::new("minecraft:shield", 1));
                    game.remote_use_hands();
                    assert_eq!(
                        observer.0.lock().unwrap().steps,
                        [Step::Air(side, Hand::Main), Step::Air(side, Hand::Off)]
                    );
                    assert!(game.using.is_none());
                } else {
                    *observer.0.lock().unwrap() = Controls::default();
                    assert!(game.use_hand(None).unwrap());
                    assert_eq!(
                        observer.0.lock().unwrap().steps,
                        [Step::Air(side, Hand::Main), Step::ItemPart]
                    );
                }
                observer.0.lock().unwrap().steps.clear();
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                game.world.set(0, 64, 0, stone).unwrap();
                game.forced_hit = Some(hit());
                // Even a passing block remains a block target for Item.use.
                assert!(
                    game.right_click_item(Hand::Main, &ItemStack::new("minecraft:stick", 1))
                        .is_none()
                );
                assert!(observer.0.lock().unwrap().steps.is_empty());
                game.forced_hit = None;
                let kind = game
                    .res
                    .mob_types()
                    .get("minecraft:zombie")
                    .unwrap()
                    .clone();
                game.mobs.push(horizon_client::mobs::Mob::new(
                    42,
                    kind,
                    Vec3::new(0.5, 64.0, 5.5),
                    0.0,
                ));
                assert!(game.entity_targeted());
                assert!(
                    game.right_click_item(Hand::Main, &ItemStack::new("minecraft:stick", 1))
                        .is_none()
                );
                assert!(observer.0.lock().unwrap().steps.is_empty());
            });
        }
    });
}

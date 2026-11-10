//! Actual guest pickup hooks mutate canonical owners, never copied bodies.
#![cfg(test)]
use crate::game::{Game, entity::taken_pass_tests::SlotIdentities};
use mod_api::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug)]
enum Action {
    Remove,
    Cancel,
    Change,
    Panic,
}
struct Mutate {
    action: Action,
    orb: bool,
    fired: Arc<AtomicBool>,
}
impl Mutate {
    fn change(&self, host: &mut dyn ServerHost) {
        if self.fired.swap(true, Ordering::SeqCst) {
            return;
        }
        let tag = if matches!(self.action, Action::Remove) {
            "pickup_original"
        } else {
            "pickup_earlier"
        };
        assert!(host.commands().run_command(&format!("kill @e[tag={tag}]")));
        // A newborn occupies an old ordinal, but never the original join.
        let command = if self.orb {
            "summon experience_orb 3 64 5 {Tags:[pickup_newborn],Value:19s,Count:4}"
        } else {
            "summon item 3 64 5 {Tags:[pickup_newborn],Item:{id:\"minecraft:diamond\",count:19}}"
        };
        assert!(host.commands().run_command(command));
        if matches!(self.action, Action::Panic) {
            std::panic::panic_any(8801_u32);
        }
    }
}
#[mod_api::__subscriptions]
impl Hooks for Mutate {
    fn item_picked_up(&self, host: &mut dyn ServerHost, _: &mut ItemPickedUpEvent<'_>) -> Flow {
        self.change(host);
        if matches!(self.action, Action::Cancel) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
    fn pickup_xp(&self, host: &mut dyn ServerHost, event: &mut PickupXpEvent<'_>) -> Flow {
        self.change(host);
        event.value = 11;
        event.count = 3;
        if matches!(self.action, Action::Cancel) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
}

fn prepare(game: &mut Game<'_>, orb: bool) -> (u32, u32) {
    game.options.dedicated = Some(Vec::new());
    game.build_registries();
    game.set_allow_commands(true);
    for x in [0, 1] {
        game.world
            .chunks
            .insert((x, 0), Arc::new(horizon_client::chunk::Chunk::empty(x, 0)));
    }
    let kind = if orb { "experience_orb" } else { "item" };
    let payload = if orb {
        "Value:7s,Count:2"
    } else {
        "Item:{id:\"minecraft:stone\",count:7},PickupDelay:0s"
    };
    for (name, x, n) in [("earlier", 20, 1), ("original", 3, 2)] {
        assert!(game.run_command(&format!(
            "summon {kind} {x} 64 5 {{Tags:[pickup_{name}],UUID:[I;771,0,0,{n}],{payload}}}"
        )));
    }
    let id = |n| {
        *game
            .persist
            .uuids
            .iter()
            .find(|(_, uuid)| **uuid == [771, 0, 0, n])
            .unwrap()
            .0
    };
    (id(1), id(2))
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn guest_item_hook_changes_keep_exact_owner_newborn_order_and_panic_lifetime() {
    for action in [
        Action::Remove,
        Action::Cancel,
        Action::Change,
        Action::Panic,
    ] {
        crate::game::native_guest_turn_tests::with_guests(|game| {
            let (earlier, original) = prepare(game, false);
            let epoch = game.native_lifecycle.epoch().unwrap();
            let mut identities = SlotIdentities::default();
            game.dropped
                .for_each_mut(|item| identities.observe_one(item, epoch, item.id));
            let join = game.dropped[1].join;
            let fired = Arc::new(AtomicBool::new(false));
            let mods = vec![ModEntry::new(
                "guest-pickup-mutation",
                Box::new(Mutate {
                    action,
                    orb: false,
                    fired: fired.clone(),
                }),
            )]
            .into();
            let result = testing::with_mods(mods, || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| game.guest_pickups(0)))
            });
            assert!(fired.load(Ordering::SeqCst), "actual Item hook {action:?}");
            assert_eq!(result.is_err(), matches!(action, Action::Panic));
            if let Err(panic) = result {
                assert_eq!(panic.downcast_ref::<u32>(), Some(&8801));
            }
            assert_eq!(
                game.dropped.iter().any(|item| item.id == earlier),
                matches!(action, Action::Remove)
            );
            let newborn = game
                .dropped
                .iter()
                .find(|item| item.stack.item.as_ref() == "minecraft:diamond")
                .unwrap();
            assert_eq!(newborn.stack.count, 19);
            assert_ne!(newborn.join, join);
            let index = game.dropped.locate(join);
            if matches!(action, Action::Cancel | Action::Panic) {
                let index = index.expect("surviving original must be canonical after callback");
                game.dropped
                    .update(index, |item| identities.observe_one(item, epoch, original));
                assert_eq!(game.dropped[index].stack.count, 7);
                assert_eq!(
                    game.dropped[index].pickup_delay,
                    if matches!(action, Action::Cancel) {
                        10
                    } else {
                        0
                    }
                );
                assert_eq!(game.dropped[0].id, original);
            } else {
                assert!(
                    index.is_none(),
                    "consumed or removed original cannot return"
                );
            }
            let picked = game
                .as_guest(12, |game| {
                    game.inventory
                        .iter()
                        .flatten()
                        .filter(|stack| stack.item.as_ref() == "minecraft:stone")
                        .map(|stack| stack.count)
                        .sum::<u32>()
                })
                .unwrap();
            assert_eq!(
                picked,
                if matches!(action, Action::Change) {
                    7
                } else {
                    0
                }
            );
        });
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn guest_orb_hook_changes_keep_real_count_value_newborn_and_panic_lifetime() {
    for action in [
        Action::Remove,
        Action::Cancel,
        Action::Change,
        Action::Panic,
    ] {
        crate::game::native_guest_turn_tests::with_guests(|game| {
            let (earlier, original) = prepare(game, true);
            let epoch = game.native_lifecycle.epoch().unwrap();
            let mut identities = SlotIdentities::default();
            game.orbs
                .for_each_mut(|orb| identities.observe_one(orb, epoch, orb.id));
            let join = game.orbs[1].join;
            let fired = Arc::new(AtomicBool::new(false));
            let mods = vec![ModEntry::new(
                "guest-orb-mutation",
                Box::new(Mutate {
                    action,
                    orb: true,
                    fired: fired.clone(),
                }),
            )]
            .into();
            let result = testing::with_mods(mods, || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| game.guest_orbs(0)))
            });
            assert!(fired.load(Ordering::SeqCst), "actual XP hook {action:?}");
            assert_eq!(result.is_err(), matches!(action, Action::Panic));
            if let Err(panic) = result {
                assert_eq!(panic.downcast_ref::<u32>(), Some(&8801));
            }
            assert_eq!(
                game.orbs.iter().any(|orb| orb.id == earlier),
                matches!(action, Action::Remove)
            );
            let newborn = game.orbs.iter().find(|orb| orb.value == 19).unwrap();
            assert_eq!(newborn.count, 4);
            assert_ne!(newborn.join, join);
            let index = game.orbs.locate(join);
            if matches!(action, Action::Remove) {
                assert!(index.is_none());
            } else {
                let index = index.expect("original orb survives once or cancels");
                game.orbs
                    .update(index, |orb| identities.observe_one(orb, epoch, original));
                assert_eq!(
                    game.orbs[index].value,
                    if matches!(action, Action::Change) {
                        11
                    } else {
                        7
                    }
                );
                assert_eq!(game.orbs[index].count, 2);
            }
            let host = game.lan.as_ref().unwrap();
            assert_eq!(
                host.hub.guests[0].xp_total,
                if matches!(action, Action::Change) {
                    11
                } else {
                    0
                }
            );
            assert_eq!(
                host.play.get(&12).map_or(0, |play| play.xp_delay),
                if matches!(action, Action::Change) {
                    2
                } else {
                    0
                }
            );
        });
    }
}

struct MarkNominee {
    target: u32,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}
#[mod_api::__subscriptions]
impl Hooks for MarkNominee {
    fn item_picked_up(&self, host: &mut dyn ServerHost, _: &mut ItemPickedUpEvent<'_>) -> Flow {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let target = host.entity(self.target).unwrap();
            assert!(target.remove(host, RemovalReason::Discarded));
        }
        Flow::Cancel
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn guest_pickup_does_not_callback_an_already_removed_nomination() {
    crate::game::native_guest_turn_tests::with_guests(|game| {
        let (earlier, original) = prepare(game, false);
        let at = game.dropped[1].position;
        game.dropped.update(0, |item| item.position = at);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mods = vec![ModEntry::new(
            "mark-later-nominee",
            Box::new(MarkNominee {
                target: earlier,
                calls: calls.clone(),
            }),
        )]
        .into();
        testing::with_mods(mods, || game.guest_pickups(0));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "removed nominee must not get a second callback"
        );
        assert!(game.dropped.iter().any(|item| item.id == earlier));
        game.native_removals_flush();
        assert!(!game.dropped.iter().any(|item| item.id == earlier));
        assert_eq!(game.dropped[0].id, original);
        assert_eq!(game.dropped[0].pickup_delay, 10);
    });
}

struct NoopXp;
#[mod_api::__subscriptions]
impl Hooks for NoopXp {
    fn pickup_xp(&self, _: &mut dyn ServerHost, _: &mut PickupXpEvent<'_>) -> Flow {
        Flow::Continue
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn guest_orb_noop_hook_preserves_selection_pitch_rng_count_and_xp() {
    for listened in [false, true] {
        crate::game::native_guest_turn_tests::with_guests(|game| {
            let (_, original) = prepare(game, true);
            let mut expected = game.world.random.clone();
            expected.next_int_bound(1).unwrap();
            expected.next_float();
            expected.next_float();
            if listened {
                let mods = vec![ModEntry::new("noop-xp", Box::new(NoopXp))].into();
                testing::with_mods(mods, || game.guest_orbs(0));
            } else {
                game.guest_orbs(0);
            }
            assert_eq!(game.world.random.next_long(), expected.next_long());
            let original = game.orbs.iter().find(|orb| orb.id == original).unwrap();
            assert_eq!((original.value, original.count), (7, 1));
            assert_eq!(game.lan.as_ref().unwrap().hub.guests[0].xp_total, 7);
        });
    }
}

struct CriterionShift(Arc<AtomicBool>);
#[mod_api::__subscriptions]
impl Hooks for CriterionShift {
    fn player_advancement_progress(
        &self,
        host: &mut dyn ServerHost,
        _: &mut PlayerAdvancementProgressEvent<'_>,
    ) {
        if !self.0.swap(true, Ordering::SeqCst) {
            assert!(host.commands().run_command("kill @e[tag=pickup_earlier]"));
            assert!(host.commands().run_command("summon item 3 64 5 {Tags:[pickup_newborn],Item:{id:\"minecraft:diamond\",count:19}}"));
        }
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn guest_partial_item_criterion_preserves_amount_after_actual_ordinal_replacement() {
    crate::game::native_guest_turn_tests::with_guests(|game| {
        let (_, original) = prepare(game, false);
        let epoch = game.native_lifecycle.epoch().unwrap();
        let mut identities = SlotIdentities::default();
        game.dropped
            .for_each_mut(|item| identities.observe_one(item, epoch, item.id));
        let join = game.dropped[1].join;
        game.dropped.update(1, |item| {
            item.thrower = Some(horizon_client::lan::playerdata::uuid_ints(12))
        });
        game.as_guest(12, |game| {
            game.inventory
                .fill(Some(horizon_client::items::ItemStack::new(
                    "minecraft:stone",
                    64,
                )));
            game.offhand = Some(horizon_client::items::ItemStack::new("minecraft:stone", 64));
            game.inventory[0] = Some(horizon_client::items::ItemStack::new("minecraft:stone", 62));
        })
        .unwrap();
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
            .join("guest-partial-criterion");
        std::fs::create_dir_all(root.join("data/test/advancement")).unwrap();
        std::fs::write(
            root.join("data/test/advancement/pickup.json"),
            br#"{"criteria":{"take":{"trigger":"minecraft:thrown_item_picked_up_by_player"}}}"#,
        )
        .unwrap();
        game.adv.tree = Arc::new(horizon_client::advancements::Tree::load(&root.join("data")));
        game.adv.loaded = true;
        let (tx, rx) = std::sync::mpsc::channel();
        game.lan.as_mut().unwrap().hub.guests.update(0, |guest| {
            guest.loaded = true;
            guest.sender = horizon_client::lan::PlayerSender::new(
                tx,
                Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            );
        });
        let fired = Arc::new(AtomicBool::new(false));
        let mods = vec![ModEntry::new(
            "post-pickup-shift",
            Box::new(CriterionShift(fired.clone())),
        )]
        .into();
        testing::with_mods(mods, || game.guest_pickups(0));
        assert!(
            fired.load(Ordering::SeqCst),
            "actual post-pickup criterion must run"
        );
        let index = game.dropped.locate(join).unwrap();
        game.dropped
            .update(index, |item| identities.observe_one(item, epoch, original));
        assert_eq!(index, 0);
        assert_eq!(game.dropped[index].stack.count, 5);
        assert_eq!(
            game.dropped[1].stack.count, 19,
            "newborn at old ordinal must not be consumed"
        );
        assert_eq!(
            game.as_guest(12, |game| game.inventory[0].as_ref().unwrap().count),
            Some(64)
        );
        let amounts = rx
            .try_iter()
            .flat_map(|out| match out {
                horizon_client::lan::Out::Packet(body) => vec![body],
                horizon_client::lan::Out::Packets(bodies) => bodies,
                horizon_client::lan::Out::Disconnect(_) => panic!("unexpected disconnect"),
            })
            .filter_map(|body| {
                let mut reader = horizon_protocol::buf::Reader::new(&body);
                let id = reader.varint().unwrap();
                match horizon_protocol::packets::play::Clientbound::decode(id, reader.rest())
                    .unwrap()
                {
                    horizon_protocol::packets::play::Clientbound::TakeItemEntity(packet)
                        if packet.item == original as i32 =>
                    {
                        Some(packet.amount)
                    }
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(amounts, [2]);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn marked_vehicle_actor_refusal_preserves_inventory_for_normal_terminal_sweep() {
    use crate::game::entity_actor_loan::{EntityActor, EntityActorLoan};
    fn probe<T: EntityActor<false>>(
        game: &mut Game<'_>,
        id: u32,
        join: horizon_client::join_seq::JoinSeq,
        before: bool,
    ) {
        let handle = game.entity(id).unwrap();
        if before {
            assert!(game.entity_remove(handle, RemovalReason::Discarded));
            assert!(EntityActorLoan::<T, false>::extract(game, join).is_none());
        } else {
            let mut actor = EntityActorLoan::<T, false>::extract(game, join).unwrap();
            assert!(
                actor
                    .with_game(game, |game| game
                        .entity_remove(handle, RemovalReason::Discarded))
                    .is_none()
            );
        }
    }
    for (kind, boat) in [("oak_chest_boat", true), ("chest_minecart", false)] {
        for before in [true, false] {
            crate::game::native_guest_turn_tests::with_guests(|game| {
                prepare(game, false);
                assert!(game.run_command(&format!("summon {kind} 3 64 5 {{UUID:[I;771,0,0,3],Items:[{{Slot:0b,id:\"minecraft:diamond\",count:4}}]}}")));
                let id = *game
                    .persist
                    .uuids
                    .iter()
                    .find(|(_, uuid)| **uuid == [771, 0, 0, 3])
                    .unwrap()
                    .0;
                if boat {
                    let join = game.boats[0].join;
                    probe::<crate::game::boats::Boat>(game, id, join, before);
                    assert_eq!(game.boats[0].items[0].as_ref().unwrap().count, 4);
                } else {
                    let join = game.carts[0].join;
                    probe::<crate::game::minecarts::Minecart>(game, id, join, before);
                    assert_eq!(game.carts[0].items[0].as_ref().unwrap().count, 4);
                }
                assert!(
                    !game
                        .dropped
                        .iter()
                        .any(|item| item.stack.item.as_ref() == "minecraft:diamond")
                );
                game.native_removals_flush();
                assert!(!game.boats.iter().any(|item| item.id == id));
                assert!(!game.carts.iter().any(|item| item.id == id));
                let spilled = game
                    .dropped
                    .iter()
                    .filter(|item| item.stack.item.as_ref() == "minecraft:diamond")
                    .map(|item| item.stack.count)
                    .sum::<u32>();
                assert_eq!(spilled, 4, "{kind}, removed before extraction: {before}");
            });
        }
    }
}

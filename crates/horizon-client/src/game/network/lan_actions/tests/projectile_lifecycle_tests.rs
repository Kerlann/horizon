//! Production residency regressions for projectile ownership.
#![cfg(test)]
use crate::game::Game;
use horizon_client::{
    motion::PlayerMotion,
    projectiles::{Projectile, ProjectileKind},
};
use horizon_core::geometry::Vec3;
use horizon_mod_runtime::attachments::{
    AttachmentType, Attachments, HasAttachmentLifetime, Holder, index::AttachmentIndex,
};
use std::sync::{Arc, Mutex};

fn arrow(id: u32, at: [f64; 3]) -> Projectile {
    let mut p = Projectile::new(
        ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        },
        Vec3::new(at[0], at[1], at[2]),
        None,
    );
    p.id = id;
    p
}

static VALUE: AttachmentType<i32> = AttachmentType {
    namespace: "test",
    name: "projectile_restore",
    default: || 0,
    codec: None,
    copy_on_death: false,
    sync: false,
};

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn projectile_restore_entries_keep_the_bound_arrow() {
    for path in 0..3 {
        let exercise = |game: &mut Game<'_>| {
            let at = [3.0, 64.0, 5.0];
            game.world
                .chunks
                .insert((0, 0), Arc::new(horizon_client::chunk::Chunk::empty(0, 0)));
            game.block_ticks
                .add_empty_chunk(horizon_core::position::ChunkPos::new(0, 0))
                .unwrap();
            game.creatures.projectiles.push(arrow(71, at));
            game.creatures
                .projectiles
                .update(0, |p| p.velocity = Vec3::new(0.3, 0.0, 0.0));
            let index = Arc::new(Mutex::new(AttachmentIndex::default()));
            let mut values = Attachments::default();
            VALUE.set(&mut values, Holder::Entity(71), 10).unwrap();
            let holder = mod_api::EntityRef::new(game.native_lifecycle.epoch().unwrap(), 71);
            let original = game.creatures.projectiles.update(0, |p| {
                p.attachment_lifetime().bind(&mut values, &index, holder);
                p.attachment_lifetime().registration().unwrap()
            });
            let key = ("test".into(), "projectile_restore".into());
            let before = index.lock().unwrap().snapshot(&key);
            assert_eq!(before.len(), 1);
            if path == 0 {
                game.tick_remote_projectiles(|_, at| at);
                assert!(game.creatures.projectiles[0].position.x > at[0]);
            } else {
                // One takeable unbound arrow exercises the pickup branch while
                // the bound, disallowed arrow must survive the same retain.
                let mut picked = arrow(72, at);
                let state = picked.arrow.as_mut().unwrap();
                state.in_ground = true;
                state.pickup = horizon_client::arrows::Pickup::Allowed;
                for id in [73, 74] {
                    game.creatures.projectiles.push(arrow(id, at));
                }
                game.creatures.projectiles.push(picked);
                if path == 1 {
                    crate::game::entity::taken_pass_tests::assert_projectile_spawn_survives(
                        game,
                        |game| {
                            game.guest_arrows(0);
                        },
                    );
                } else {
                    game.player = Some(
                        PlayerMotion::new(game.motion, Vec3::new(at[0], at[1], at[2])).unwrap(),
                    );
                    crate::game::entity::taken_pass_tests::assert_projectile_spawn_survives(
                        game,
                        |game| {
                            game.touch_arrows();
                        },
                    );
                }
                assert_eq!(
                    game.creatures
                        .projectiles
                        .iter()
                        .map(|p| p.id)
                        .collect::<Vec<_>>(),
                    [71, 73, 74]
                );
            }
            let current = game
                .creatures
                .projectiles
                .update(0, |p| p.attachment_lifetime().registration().unwrap());
            assert!(original.same_registration(&current), "restore path {path}");
            assert_eq!(
                index.lock().unwrap().snapshot(&key).len(),
                1,
                "restore path {path}"
            );
            game.creatures.projectiles.clear();
            assert!(index.lock().unwrap().snapshot(&key).is_empty());
            assert_eq!(before[0].entry.read(&VALUE), Ok(&10));
        };
        if path == 0 {
            crate::game::native_entity_refs::tests::with_client_game(exercise);
        } else {
            crate::game::native_guest_turn_tests::with_guests(exercise);
        }
    }
}

struct CanonicalPickupObserver {
    panic: bool,
    seen: Arc<std::sync::atomic::AtomicUsize>,
}
#[mod_api::__subscriptions]
impl mod_api::Hooks for CanonicalPickupObserver {
    fn player_pickup_arrow(
        &self,
        host: &mut dyn mod_api::ServerHost,
        event: &mut mod_api::PlayerPickupArrowEvent<'_>,
    ) {
        assert_eq!(event.entity_id, 7102);
        assert_eq!(event.item, "minecraft:arrow");
        assert!(
            host.entity(7102).is_none(),
            "pickup notice follows actual retirement"
        );
        assert_eq!(
            host.entity(7101).unwrap().position(host),
            Some([3.0, 64.0, 5.0])
        );
        assert_eq!(
            host.entity(7103).unwrap().position(host),
            Some([3.0, 64.0, 5.0])
        );
        self.seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.panic {
            panic!("primary guest pickup notice panic");
        }
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn guest_arrow_pickup_keeps_actual_owner_and_retires_before_notice_even_on_panic() {
    fn at_boundary(game: &mut Game<'_>) {
        // This runs at the actual initial schedule boundary, before inventory
        // borrowing. A lifted owner must not satisfy it.
        assert!(matches!(
            game.server_resident_entity(7101),
            horizon_client::join_seq::residency::table::EntityLookup::Found { .. }
        ));
        let header = <dyn mod_api::Host>::entity(game, 7101).unwrap();
        assert_eq!(header.position(game), Some([3.0, 64.0, 5.0]));
        let original = game
            .creatures
            .projectiles
            .iter()
            .position(|p| p.id == 7104)
            .unwrap();
        game.creatures.projectiles.remove(original);
        assert!(<dyn mod_api::Host>::entity(game, 7104).is_none());
        // Keep the population length unchanged: a compact ordinal budget would
        // now admit the newborn in the removed original's position.
        let mut newborn = arrow(7103, [3.0, 64.0, 5.0]);
        let state = newborn.arrow.as_mut().unwrap();
        state.in_ground = true;
        state.pickup = horizon_client::arrows::Pickup::Allowed;
        game.creatures.projectiles.push(newborn);
    }
    for panic in [false, true] {
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        mod_api::testing::with_mods(
            vec![mod_api::ModEntry::new(
                "guest_pickup_observer",
                Box::new(CanonicalPickupObserver {
                    panic,
                    seen: seen.clone(),
                }),
            )]
            .into(),
            || {
                crate::game::native_guest_turn_tests::with_guests(|game| {
                    game.options.dedicated = Some(Vec::new());
                    game.creatures
                        .projectiles
                        .push(arrow(7101, [3.0, 64.0, 5.0]));
                    let mut picked = arrow(7102, [3.0, 64.0, 5.0]);
                    picked.arrow.as_mut().unwrap().in_ground = true;
                    picked.arrow.as_mut().unwrap().pickup = horizon_client::arrows::Pickup::Allowed;
                    game.creatures.projectiles.push(picked);
                    let mut removed_original = arrow(7104, [3.0, 64.0, 5.0]);
                    removed_original.arrow.as_mut().unwrap().in_ground = true;
                    removed_original.arrow.as_mut().unwrap().pickup =
                        horizon_client::arrows::Pickup::Allowed;
                    game.creatures.projectiles.push(removed_original);
                    game.native_joins_skip();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::game::entity::taken_pass_tests::with_spawn(at_boundary, || {
                            game.guest_arrows(0)
                        });
                    }));
                    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
                    if panic {
                        assert_eq!(
                            result.unwrap_err().downcast_ref::<&str>(),
                            Some(&"primary guest pickup notice panic")
                        );
                    } else {
                        result.unwrap();
                    }
                    assert_eq!(
                        game.creatures
                            .projectiles
                            .iter()
                            .map(|p| p.id)
                            .collect::<Vec<_>>(),
                        [7101, 7103]
                    );
                    assert!(matches!(
                        game.server_resident_entity(7102),
                        horizon_client::join_seq::residency::table::EntityLookup::Missing
                    ));
                    assert_eq!(game.creatures.projectiles[1].age, 0);
                    let guest = &game.lan.as_ref().unwrap().hub.guests[0];
                    assert_eq!(
                        guest
                            .inventory
                            .iter()
                            .flatten()
                            .filter(|s| s.item.as_ref() == "minecraft:arrow")
                            .map(|s| s.count)
                            .sum::<u32>(),
                        1
                    );
                });
            },
        );
    }
}

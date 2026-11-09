use super::*;
use crate::game::native_entity_refs::tests::with_game;
use horizon_client::mobs::part_goals::GoalHost;
use horizon_core::geometry::Vec3;
use mod_api::prelude::PlayersHost;

struct OneMob<'a>(&'a Mob);
impl mobs::LiveMobs for OneMob<'_> {
    fn find(&self, id: u32) -> Option<&Mob> {
        (self.0.id == id).then_some(self.0)
    }

    fn for_each(&self, visitor: &mut dyn FnMut(&Mob)) {
        visitor(self.0);
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn disjoint_live_loan_is_foreign_and_not_a_snapshot_fallback() {
    with_game(|game| {
        let kind = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        let mut actor = Mob::new(9001, kind.clone(), Vec3::ZERO, 0.0);
        let mut other = Mob::new(9002, kind.clone(), Vec3::ZERO, 0.0);
        let stale = Mob::new(9003, kind, Vec3::ZERO, 0.0);
        let snapshot = [
            crate::game::creatures::seen(&other, game.level.player_uuid),
            crate::game::creatures::seen(&stale, game.level.player_uuid),
        ];
        other.yaw = 37.0;
        other.head_pitch = -12.0;
        other.attrs.max_health = 73.0;
        let live = OneMob(&other);
        let mut world = game.mob_world_base(&snapshot);
        world.goal_host = Some(game);
        world.live_mobs = Some(&live);
        let epoch = game.native_lifecycle.epoch().unwrap();
        let entity = EntityRef::new(epoch, other.id);
        let context = GoalCx {
            m: &mut actor,
            w: &world,
            host: game,
        };
        assert_eq!(entity.rotation(&context), None);
        assert_eq!(LivingRef::new(entity).max_health(&context), None);
        assert!(context.lookup_entity(other.id).is_none());
        assert!(context.lookup_entity(stale.id).is_none());
        assert!(context.lookup_entity(9999).is_none());
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn scoped_host_resolves_other_real_players_from_the_game() {
    use horizon_client::lan::{Joined, PlayerSender, guest::Guest};
    use horizon_protocol::packets::configuration::ClientInformation;
    use std::sync::{
        Arc,
        atomic::{AtomicI32, AtomicUsize},
    };

    with_game(|game| {
        crate::game::native_item_places::tests::open_to_lan(game);
        for id in [12, 13] {
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut guest = Guest::new(
                Joined {
                    conn: id,
                    uuid: u128::from(id),
                    name: format!("guest{id}"),
                    properties: Vec::new(),
                    info: ClientInformation::new(8),
                    brand: String::new(),
                    sender: PlayerSender::new(tx, Arc::new(AtomicUsize::new(0))),
                    latency: Arc::new(AtomicI32::new(0)),
                    address: "127.0.0.1:1".parse().unwrap(),
                    expected: false,
                },
                id as i32,
                [f64::from(id), 64.0, 5.0],
                10,
            );
            guest.placed = true;
            game.lan.as_mut().unwrap().hub.guests.push(guest);
        }
        let kind = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        let mut actor = Mob::new(9001, kind, Vec3::ZERO, 0.0);
        let mut world = game.mob_world_base(&[]);
        world.goal_host = Some(game);
        // Deliberately select only one guest: the other remains a real live
        // player and must resolve through the underlying game host.
        world.player_identity = Some(([0, 0, 0, 12], 12));
        let context = GoalCx {
            m: &mut actor,
            w: &world,
            host: game,
        };
        let selected = context.player(12).expect("selected guest is online");
        assert_eq!(selected.selected_slot(&context), Some(0));
        let player = context.player(13).expect("other guest is online");
        assert_eq!(player.entity().id(), 13);
        assert_eq!(context.lookup_entity(13), Some(player.entity()));
        assert!(context.lookup_living(player.entity()).is_some());
        assert_eq!(
            player.entity().position(&context),
            player.entity().position(game)
        );
        assert!(context.player(999).is_none());
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn explicit_own_loan_reads_applied_navigation_and_rejects_player_forgery() {
    use horizon_client::mobs::path::{Path, PathNode};
    with_game(|game| {
        let kind = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        let mut actor = Mob::new(9001, kind, Vec3::ZERO, 0.0);
        actor.ai.nav.path = Some(Path {
            nodes: vec![PathNode {
                x: 1,
                y: 0,
                z: 0,
                kind: mod_api::PathType::Walkable,
            }],
            next: 0,
            target: [1, 0, 0],
            dist_to_target: 0.0,
            reached: true,
        });
        let mut world = game.mob_world_base(&[]);
        world.goal_host = Some(game);
        let mut context = GoalCx {
            m: &mut actor,
            w: &world,
            host: game,
        };
        let own = context.mob_handle();
        assert_eq!(own.is_done(&context), Some(false));
        GoalCxBackend::stop_navigation(&mut context);
        assert_eq!(own.is_done(&context), Some(true));
        assert_eq!(own.entity().position(&context), Some([0.0; 3]));
        let forged = mod_api::__private::game::player_ref(own.entity(), 13);
        assert_eq!(forged.selected_slot(&context), None);
        assert_eq!(forged.food(&context), None);
        let stale = EntityRef::new(
            mod_api::NativeWorldEpoch::new(own.entity().epoch().get() + 1),
            own.entity().id(),
        );
        assert_eq!(stale.position(&context), None);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn foreign_lifted_read_observes_shared_publication_updates_not_seen_or_live_views() {
    use crate::game::{
        commands::Target,
        native_base_read_rows::{
            BaseReadContext, BaseReadGeometry, BaseReadSource, CompactEntityReadRow,
        },
        native_entity_read_cache::{NativeReadDetails, NativeReadPublication},
    };
    use horizon_client::join_seq::{RestoreOrder, residency::table::EntityLookup};
    use std::sync::Arc;
    with_game(|game| {
        game.reset_entity_residency();
        let kind = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        let mut actor = Mob::new(9001, kind.clone(), Vec3::ZERO, 0.0);
        let disjoint = Mob::new(9002, kind.clone(), Vec3::new(99.0, 0.0, 0.0), 0.0);
        game.mobs.push(Mob::new(9002, kind, Vec3::ZERO, 0.0));
        let snapshot = [crate::game::creatures::seen(
            &game.mobs[0],
            game.level.player_uuid,
        )];
        let EntityLookup::Found { location, .. } = game.server_resident_entity(9002) else {
            panic!("admitted foreign source");
        };
        let epoch = game.native_lifecycle.epoch().unwrap();
        let entity = EntityRef::new(epoch, 9002);
        let base = CompactEntityReadRow::capture(
            BaseReadSource::Mob(&game.mobs[0]),
            BaseReadGeometry {
                position: [5.0, 2.0, 3.0],
                rotation: [20.0, 10.0],
                velocity: [0.0; 3],
                on_ground: false,
                bounding_box: [[4.5, 2.0, 2.5], [5.5, 4.0, 3.5]],
                dimensions: [1.0, 2.0],
                eye_height: 1.5,
            },
            BaseReadContext {
                level: mod_api::LevelRef::new(epoch, game.dim.key()),
                logical_server: true,
                kind: Arc::from("minecraft:zombie"),
                saved: None,
                added_to_level: Some(true),
                portal_cooldown: None,
                leash_holder: None,
                parts: None,
                head_yaw: 0.0,
                spectator: false,
                invisible: None,
                client_shift: None,
            },
        );
        let publication = NativeReadPublication {
            silent_overlay: Default::default(),
            capture: mod_api::EntityReadCapture {
                tick: mod_api::NativeTickId::new(1),
                pass: 1,
                revision: 0,
                phase: mod_api::EntityReadCapturePhase::BeforeCallback,
                pending_overlay: None,
            },
            base,
            details: NativeReadDetails::Base,
            join_generation: 1,
            target: Target::Mob(9002),
            mob_target_facts: None,
        };
        assert_eq!(
            game.entity_residency
                .borrow_mut()
                .publish_rows([(9002, location, publication)]),
            1
        );
        let lifted = game.mobs.lift();
        {
            let live = OneMob(&disjoint);
            let mut world = game.mob_world_base(&snapshot);
            world.goal_host = Some(game);
            world.live_mobs = Some(&live);
            let context = GoalCx {
                m: &mut actor,
                w: &world,
                host: game,
            };
            assert_eq!(entity.position(&context), Some([5.0, 2.0, 3.0]));
            game.entity_residency
                .borrow_mut()
                .edit_cached_row(9002, location, |row| {
                    row.base.position = [7.0, 2.0, 3.0];
                    row.capture.revision += 1;
                });
            assert_eq!(entity.position(&context), Some([7.0, 2.0, 3.0]));
            // The legacy heavy scalar is unsupported while its foreign source
            // is lifted; the disjoint Live loan must not masquerade as OWN.
            assert_eq!(entity.tags(&context), None);
        }
        game.mobs.restore(lifted, RestoreOrder::SurvivorsFirst);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn acting_guest_reads_are_shared_host_reads_not_selected_world_player_reads() {
    use crate::game::native_living_flags::tests::{guest, guest_server};
    with_game(|game| {
        guest_server(game, false, true);
        let _receivers = [guest(game, 12).1, guest(game, 13).1];
        assert!(game.native_as_player(13, |game| {
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            let mut actor = Mob::new(9001, kind, Vec3::ZERO, 0.0);
            let mut world = game.mob_world_base(&[]);
            world.goal_host = Some(game);
            world.player_identity = Some(([0, 0, 0, 12], 12));
            let context = GoalCx {
                m: &mut actor,
                w: &world,
                host: game,
            };
            let acting = context.player(13).expect("actual acting guest");
            assert_eq!(acting.entity().id(), 13);
            assert_eq!(acting.selected_slot(&context), Some(0));
            assert_eq!(acting.food(&context), acting.food(game));
            assert_eq!(
                acting.entity().position(&context),
                acting.entity().position(game)
            );
            assert_eq!(context.player(12), game.player(12));
        }));
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn actual_goal_callback_refreshes_own_rows_only_at_callback_and_apply_boundaries() {
    struct Stopper;
    impl mod_api::Goal for Stopper {
        fn flags(&self) -> mod_api::GoalFlags {
            mod_api::GoalFlags::MOVE
        }
        fn can_use(&mut self, cx: &mut mod_api::GoalContext<'_>) -> bool {
            let own = cx.mob();
            let before = own.cached_row(cx.host()).expect("callback publication");
            assert_eq!(
                before.capture.phase,
                mod_api::EntityReadCapturePhase::BeforeCallback
            );
            assert_eq!(before.is_done, Some(false));
            assert_eq!(own.cached_row(cx.host()).unwrap().capture, before.capture);
            cx.look_at([f64::NAN, 0.0, 0.0]);
            assert_eq!(own.cached_row(cx.host()).unwrap().capture, before.capture);
            cx.stop_navigation();
            let after = own.cached_row(cx.host()).expect("applied publication");
            assert_eq!(
                after.capture.phase,
                mod_api::EntityReadCapturePhase::AfterAppliedWrite
            );
            assert_eq!(after.is_done, Some(true));
            assert_eq!(own.is_done(cx.host()), Some(true));
            assert_eq!(own.cached_row(cx.host()).unwrap().capture, after.capture);
            true
        }
    }
    with_game(|game| {
        let kind = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        let mut actor = Mob::new(9010, kind, Vec3::ZERO, 0.0);
        actor.ai.nav.path = Some(horizon_client::mobs::path::Path {
            nodes: vec![horizon_client::mobs::path::PathNode {
                x: 1,
                y: 0,
                z: 0,
                kind: mod_api::PathType::Walkable,
            }],
            next: 0,
            target: [1, 0, 0],
            dist_to_target: 0.0,
            reached: true,
        });
        game.mobs.push(actor);
        let mut lifted = game.mobs.lift();
        let mut goal: Box<dyn mod_api::Goal> = Box::new(Stopper);
        let mut world = game.mob_world_base(&[]);
        world.goal_host = Some(game);
        assert_eq!(
            game.run_goal(
                &mut goal,
                &mut lifted[0],
                &world,
                GoalOp::CanUse,
                "fixture",
                "stopper"
            ),
            Some(true)
        );
        game.mobs.restore(
            lifted,
            horizon_client::join_seq::RestoreOrder::SurvivorsFirst,
        );
    });
}

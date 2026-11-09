//! One-source scopes for actual actor loans. Whole-list lifts use the owner
//! pass helper instead; a per-actor loan never resnapshots its population.
use super::{
    Game, native_base_read_rows::BaseReadSource,
    native_resident_read_publication::resident_source_identity,
};
use horizon_client::join_seq::residency::{Kind, Owner, read_pass::ReadScope, table::Location};
use mod_api::EntityReadCapturePhase;

impl Game<'_> {
    /// The caller has restored its owner and world before invoking this seam.
    /// Accepted work finishes even after a body panic. Completion faults never
    /// replace that original payload; the caller can refresh outputs before
    /// resuming the selected panic.
    pub(super) fn finish_resident_actor_turn<R>(
        &mut self,
        writer: Option<&super::native_entity_commands::EntityWriterScope>,
        body: std::thread::Result<R>,
    ) -> std::thread::Result<R> {
        let completion = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(writer) = writer {
                self.publish_resident_read_location(
                    writer.frame.actor.location,
                    EntityReadCapturePhase::AfterAppliedWrite,
                );
                self.drain_entity_commands(writer.frame.turn);
            }
        }));
        match body {
            Err(original) => {
                if completion.is_err() {
                    log::error!("entity completion also panicked while finishing an actor panic");
                }
                Err(original)
            }
            Ok(value) => completion.map(|()| value),
        }
    }

    pub(super) fn begin_resident_actor_writer(
        &self,
        source: BaseReadSource<'_>,
    ) -> Option<super::native_entity_commands::EntityWriterScope> {
        let (kind, id, join) = resident_source_identity(source)?;
        if kind == Kind::Mob || id == 0 {
            return None;
        }
        self.begin_entity_writer(horizon_client::join_seq::residency::write_queue::Target {
            runtime_id: id,
            location: Location {
                owner: Owner {
                    dimension: self.dim.key(),
                    kind,
                },
                join,
            },
        })
    }

    pub(super) fn begin_resident_actor_read_pass(
        &self,
        source: BaseReadSource<'_>,
    ) -> Option<ReadScope> {
        if !self.is_logical_server() || !mod_api::__private::entity_reads_listened() {
            return None;
        }
        let (kind, _, join) = resident_source_identity(source)?;
        if kind == Kind::Mob {
            return None;
        }
        let owner = Owner {
            dimension: self.dim.key(),
            kind,
        };
        let tick = self.native_lifecycle.capture_tick()?;
        let clock = self.entity_read_state.borrow().passes.clone();
        let scope = match ReadScope::begin(clock, true, owner, tick) {
            Ok(scope) => scope?,
            Err(error) => {
                log::error!("cannot open resident actor read pass: {error:?}");
                return None;
            }
        };
        self.publish_lent_resident_reads(
            Location { owner, join },
            source,
            EntityReadCapturePhase::PassStart,
        )
        .then_some(scope)
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::*;
    use horizon_client::mobs::spawner::SpawnerExt;
    use mod_api::{EntityRef, HookId, Hooks, ModEntry};
    struct VehicleObserver {
        mode: u8,
        terminals: std::sync::Arc<std::sync::Mutex<usize>>,
        events: std::sync::Arc<std::sync::Mutex<Vec<(bool, u32)>>>,
        before_second: std::sync::Arc<std::sync::Mutex<Vec<Option<mod_api::EntityReadRow>>>>,
    }
    #[mod_api::__subscriptions]
    impl Hooks for VehicleObserver {
        fn entity_tick_listened(
            &self,
            _: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::EntityTickListenedEvent,
        ) {
            event.listened = true;
        }
        fn entity_tick_pre(
            &self,
            host: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::EntityTickPreEvent<'_>,
        ) -> mod_api::Flow {
            self.events.lock().unwrap().push((true, event.entity));
            if event.entity == 72 {
                assert_eq!(*self.terminals.lock().unwrap(), 2);
                self.before_second
                    .lock()
                    .unwrap()
                    .push(host.entity(71).and_then(|actor| actor.cached_row(host)));
            }
            if event.entity == 71 {
                let other = host.entity(73).unwrap();
                for silent in [true, false] {
                    assert!(matches!(
                        other.set_silent(host, silent),
                        mod_api::WriteReceipt::Pending(_)
                    ));
                }
                if self.mode == 3 {
                    panic!("vehicle actor panic");
                }
                if self.mode == 1 {
                    return mod_api::Flow::Cancel;
                }
                if self.mode == 2 {
                    assert!(host.entity(71).unwrap().discard(host));
                }
            }
            mod_api::Flow::Continue
        }
        fn entity_command_completed(
            &self,
            _: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::EntityCommandCompletedEvent<'_>,
        ) {
            let mod_api::EntityWriteCompletion::EntitySetSilent(done) = event.completion;
            assert_eq!(done.outcome, Ok(true));
            *self.terminals.lock().unwrap() += 1;
            if self.mode == 3 {
                panic!("secondary completion panic");
            }
        }
        fn entity_tick_post(
            &self,
            _: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::EntityTickPostEvent<'_>,
        ) {
            self.events.lock().unwrap().push((false, event.entity));
        }
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn vehicle_next_pre_reads_applied_prior_actor_and_respects_cancel_and_removal() {
        use horizon_core::geometry::Vec3;
        for cart in [false, true] {
            for mode in 0..4 {
                let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                let terminals = std::sync::Arc::new(std::sync::Mutex::new(0));
                let before_second = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                mod_api::testing::with_mods(
                    vec![ModEntry::new(
                        "vehicle-observer",
                        Box::new(VehicleObserver {
                            mode,
                            terminals: terminals.clone(),
                            events: events.clone(),
                            before_second: before_second.clone(),
                        }),
                    )]
                    .into(),
                    || {
                        with_game(|game| {
                            game.options.dedicated = Some(Vec::new());
                            game.world.chunks.insert(
                                (0, 0),
                                std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
                            );
                            game.tickets.update(None);
                            for (id, x) in [(71, 1.0), (72, 8.0)] {
                                if cart {
                                    let mut actor = super::super::minecarts::Minecart::new(
                                        id,
                                        super::super::minecarts::CartKind::Rideable,
                                        Vec3::new(x, 80.0, 0.5),
                                    );
                                    actor.velocity.x = 0.25;
                                    game.carts.push(actor);
                                } else {
                                    let mut actor = super::super::boats::Boat::new(
                                        id,
                                        "oak".into(),
                                        Vec3::new(x, 80.0, 0.5),
                                        0.0,
                                    );
                                    actor.velocity.x = 0.25;
                                    game.boats.push(actor);
                                }
                            }
                            let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                            game.mobs.push(horizon_client::mobs::Mob::new(
                                73,
                                kind,
                                Vec3::new(12.0, 80.0, 0.5),
                                0.0,
                            ));
                            game.native_joins_skip();
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    if cart {
                                        game.tick_carts();
                                    } else {
                                        game.tick_boats();
                                    }
                                }));
                            assert_eq!(*terminals.lock().unwrap(), 2);
                            assert!(!game.mobs[0].silent);
                            if mode == 3 {
                                let payload = result.expect_err("actor panic was swallowed");
                                assert_eq!(
                                    payload.downcast_ref::<&str>(),
                                    Some(&"vehicle actor panic")
                                );
                                assert_eq!(*events.lock().unwrap(), vec![(true, 71)]);
                                assert!(game.held_out.is_empty());
                                return;
                            }
                            result.unwrap();
                            // Vehicle discard is logical/deferred in this engine;
                            // preserve its existing Post rather than silently
                            // changing storage or tick semantics in this layer.
                            let expected = if mode != 1 {
                                vec![(true, 71), (false, 71), (true, 72), (false, 72)]
                            } else {
                                vec![(true, 71), (true, 72), (false, 72)]
                            };
                            assert_eq!(
                                *events.lock().unwrap(),
                                expected,
                                "cart={cart}, mode={mode}"
                            );
                            let rows = before_second.lock().unwrap();
                            assert_eq!(rows.len(), 1);
                            if mode == 2 {
                                assert!(rows[0].is_none());
                            } else {
                                let row = rows[0]
                                    .as_ref()
                                    .expect("first actor publication before second actor Pre");
                                if mode == 0 {
                                    assert!(
                                        row.position[0] > 1.0,
                                        "cart={cart}: {}",
                                        row.position[0]
                                    );
                                } else {
                                    assert_eq!(row.position[0], 1.0);
                                    assert_ne!(
                                        row.capture.phase,
                                        EntityReadCapturePhase::AfterEntityTick
                                    );
                                }
                            }
                        })
                    },
                );
            }
        }
    }

    struct Reader;
    impl Hooks for Reader {
        fn subscribed(&self) -> &'static [Option<HookId>] {
            &[Some(HookId::ClientTickPre)]
        }
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn actor_scope_snapshots_only_actual_source_and_lends_no_game_borrow() {
        mod_api::testing::with_mods(
            vec![ModEntry::new("reader", Box::new(Reader))].into(),
            || {
                with_game(|game| {
                    for x in [1.0, 4.0] {
                        game.summon_with_nbt(
                            "minecraft:item",
                            [x, 70.0, 1.0],
                            "{Item:{id:\"minecraft:stone\",count:1}}",
                        )
                        .unwrap();
                    }
                    game.native_joins_skip();
                    let epoch = game.native_lifecycle.epoch().unwrap();
                    let ids = [game.dropped[0].id, game.dropped[1].id];
                    let scope = game
                        .begin_resident_actor_read_pass(BaseReadSource::Item(&game.dropped[0]))
                        .unwrap();
                    assert!(
                        game.entity_cached_row(EntityRef::new(epoch, ids[1]))
                            .is_none()
                    );
                    assert_eq!(
                        game.entity_cached_row(EntityRef::new(epoch, ids[0]))
                            .unwrap()
                            .capture
                            .phase,
                        EntityReadCapturePhase::PassStart
                    );
                    let mut taken = game.dropped.lift();
                    taken[0].position.x = 9.0;
                    game.publish_resident_actor_reads(
                        BaseReadSource::Item(&taken[0]),
                        EntityReadCapturePhase::AfterEntityTick,
                    );
                    assert_eq!(
                        game.entity_cached_row(EntityRef::new(epoch, ids[0]))
                            .unwrap()
                            .position,
                        [9.0, 70.0, 1.0]
                    );
                    assert!(
                        game.entity_cached_row(EntityRef::new(epoch, ids[1]))
                            .is_none()
                    );
                    game.dropped.restore(
                        taken,
                        horizon_client::join_seq::RestoreOrder::SurvivorsFirst,
                    );
                    drop(scope);
                })
            },
        );
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn vehicle_body_unwind_keeps_progress_and_clears_only_its_hold() {
        use horizon_core::geometry::Vec3;
        mod_api::testing::with_mods(
            vec![ModEntry::new("reader", Box::new(Reader))].into(),
            || {
                with_game(|game| {
                    game.boats.push(super::super::boats::Boat::new(
                        71,
                        "oak".into(),
                        Vec3::new(1.0, 80.0, 0.5),
                        0.0,
                    ));
                    game.carts.push(super::super::minecarts::Minecart::new(
                        72,
                        super::super::minecarts::CartKind::Rideable,
                        Vec3::new(2.0, 80.0, 0.5),
                    ));
                    game.native_joins_skip();
                    let joins = (game.boats[0].join, game.carts[0].join);
                    game.held_out.push(999);
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        game.with_boat_body(0, |_, body| {
                            body.position.x = 7.0;
                            panic!("boat physics panic");
                        })
                    }));
                    assert!(result.is_err());
                    assert_eq!(game.boats[0].position.x, 7.0);
                    assert_eq!(game.boats[0].join, joins.0);
                    assert_eq!(game.held_out, [999]);
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        game.with_cart_body(0, |_, body| {
                            body.position.x = 9.0;
                            panic!("cart physics panic");
                        })
                    }));
                    assert!(result.is_err());
                    assert_eq!(game.carts[0].position.x, 9.0);
                    assert_eq!(game.carts[0].join, joins.1);
                    assert_eq!(game.held_out, [999]);
                    game.held_out.clear();
                })
            },
        );
    }
    struct SpawnerCartObserver {
        panic: bool,
        planning_panic: bool,
        cart: std::sync::Arc<std::sync::OnceLock<mod_api::MinecartRef>>,
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[mod_api::__subscriptions]
    impl Hooks for SpawnerCartObserver {
        fn finalize_spawn(
            &self,
            host: &dyn mod_api::Host,
            _: &mut mod_api::FinalizeSpawnEvent<'_>,
        ) -> mod_api::Flow {
            let cart = *self.cart.get().unwrap();
            assert_eq!(cart.hurt_time(host), Some(8));
            assert_eq!(cart.damage(host), Some(6.0));
            if self.planning_panic {
                self.calls
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                panic!("spawner planning panic");
            }
            mod_api::Flow::Continue
        }
        fn mob_spawned(
            &self,
            host: &mut dyn mod_api::ServerHost,
            _: &mut mod_api::MobSpawnedEvent<'_>,
        ) {
            let cart = host.entity(71).expect("the spawning cart remains admitted");
            assert!(
                cart.position(host).unwrap()[0] > 8.5,
                "planning movement is canonical before the join callback"
            );
            assert!(cart.set_velocity(host, [0.3, 0.2, 0.1]));
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if self.panic {
                panic!("spawner join panic");
            }
        }
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn spawner_callbacks_restore_cart_rng_and_listener_writes_on_unwind() {
        use horizon_core::geometry::Vec3;
        use horizon_core::random::{LegacyRandom, RandomSource};
        for (panic, planning_panic, ridden) in [
            (false, false, false),
            (true, false, false),
            (true, true, false),
            (false, false, true),
            (true, false, true),
            (true, true, true),
        ] {
            let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let cart_handle = std::sync::Arc::new(std::sync::OnceLock::new());
            mod_api::testing::with_mods(
                vec![ModEntry::new(
                    "spawner-cart-observer",
                    Box::new(SpawnerCartObserver {
                        panic,
                        planning_panic,
                        cart: cart_handle.clone(),
                        calls: calls.clone(),
                    }),
                )]
                .into(),
                || {
                    with_game(|game| {
                        game.lan_acting_fake = false;
                        game.options.dedicated = Some(Vec::new());
                        game.world.chunks.insert(
                            (0, 0),
                            std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
                        );
                        game.tickets.update(None);
                        game.player = Some(
                            horizon_client::motion::PlayerMotion::new(
                                game.motion,
                                Vec3::new(12.5, 65.0, 12.5),
                            )
                            .unwrap(),
                        );
                        let tag = horizon_core::nbt::parse_snbt("{Delay:0s,SpawnCount:1s,SpawnRange:0s,SpawnData:{entity:{id:\"minecraft:zombie\"}}}").unwrap().0;
                        let mut cart = super::super::minecarts::Minecart::new(
                            71,
                            super::super::minecarts::CartKind::Spawner,
                            Vec3::new(8.5, 64.5, 8.5),
                        );
                        cart.velocity.x = 0.25;
                        cart.hurt_time = 9;
                        cart.damage = 7.0;
                        cart.spawner =
                            Some(Box::new(horizon_client::mobs::spawner::Spawner::load(&tag)));
                        cart.spawner
                            .as_mut()
                            .unwrap()
                            .next_data_mut()
                            .unwrap()
                            .custom_spawn_rules =
                            Some(horizon_client::mobs::spawner::CustomSpawnRules {
                                block: (0, 15),
                                sky: (0, 15),
                            });
                        game.ui.difficulty = 2;
                        game.world.health_rules.difficulty =
                            horizon_world::food::Difficulty::Normal;
                        game.carts.push(cart);
                        game.native_joins_skip();
                        cart_handle
                            .set(game.minecart(game.entity(71).unwrap()).unwrap())
                            .unwrap();
                        game.world.random = LegacyRandom::new(871_931);
                        let unprogressed = game.world.random.clone().next_long();
                        if ridden {
                            game.riding = Some(71);
                        }
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            if ridden {
                                assert!(
                                    game.tick_cart_riding(horizon_client::sprint::Keys::default())
                                );
                            } else {
                                game.tick_carts();
                            }
                        }));
                        assert_eq!(
                            calls.load(std::sync::atomic::Ordering::Relaxed),
                            1,
                            "cart ticks {}, spawner delay {}, spawned mobs {}",
                            game.carts[0].tick_count,
                            game.carts[0].spawner.as_ref().unwrap().spawn_delay,
                            game.mobs.len()
                        );
                        assert_eq!(result.is_err(), panic);
                        if planning_panic {
                            let progressed = game.world.random.next_long();
                            assert_ne!(
                                progressed,
                                LegacyRandom::new(0).next_long(),
                                "the level must not retain the temporary seed-zero carrier"
                            );
                            assert_ne!(
                                progressed, unprogressed,
                                "planning's actual random draws must survive unwind"
                            );
                            assert!(game.mobs.is_empty());
                            assert_eq!(game.carts[0].spawner.as_ref().unwrap().spawn_delay, 0);
                        } else {
                            assert_eq!(game.carts[0].velocity, Vec3::new(0.3, 0.2, 0.1));
                            assert!(game.carts[0].spawner.as_ref().unwrap().spawn_delay >= 200);
                        }
                        assert!(game.held_out.is_empty());
                    });
                },
            );
        }
    }
}

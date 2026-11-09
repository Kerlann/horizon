//! One-source scopes for actual actor loans. Whole-list lifts use the owner
//! pass helper instead; a per-actor loan never resnapshots its population.
use super::{
    Game, native_base_read_rows::BaseReadSource,
    native_resident_read_publication::resident_source_identity,
};
use horizon_client::join_seq::residency::{Kind, Owner, read_pass::ReadScope, table::Location};
use mod_api::EntityReadCapturePhase;

impl Game<'_> {
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
    use mod_api::{EntityRef, HookId, Hooks, ModEntry};
    struct VehicleObserver {
        mode: u8,
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
                self.before_second
                    .lock()
                    .unwrap()
                    .push(host.entity(71).and_then(|actor| actor.cached_row(host)));
            }
            if event.entity == 71 {
                if self.mode == 1 {
                    return mod_api::Flow::Cancel;
                }
                if self.mode == 2 {
                    assert!(host.entity(71).unwrap().discard(host));
                }
            }
            mod_api::Flow::Continue
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
            for mode in 0..3 {
                let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                let before_second = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                mod_api::testing::with_mods(
                    vec![ModEntry::new(
                        "vehicle-observer",
                        Box::new(VehicleObserver {
                            mode,
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
                            game.native_joins_skip();
                            if cart {
                                game.tick_carts();
                            } else {
                                game.tick_boats();
                            }
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
}

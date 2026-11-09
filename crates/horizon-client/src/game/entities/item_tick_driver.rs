//! Canonical per-item tick boundaries. Internal movement/expiry callbacks are
//! still owned by the shared physics kernel; their staged handoff is separate.
use super::{Game, item_tick::ItemPhysicsPass, native_base_read_rows::BaseReadSource};
use horizon_client::{
    entities::ObstacleGrid,
    fast_hash::{FxHashMap, FxHashSet},
    join_seq::{
        JoinSeq, RestoreOrder,
        physics_mutations::{Mutation, Watch},
    },
};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

struct ColliderChanges {
    mobs: Watch,
    boats: Watch,
    mob_order: FxHashMap<JoinSeq, u128>,
    boat_order: FxHashMap<JoinSeq, u128>,
    boat_ids: FxHashMap<u32, JoinSeq>,
    events: Vec<Mutation>,
    dirty: FxHashSet<JoinSeq>,
}
impl ColliderChanges {
    fn new(game: &mut Game<'_>) -> (Self, ObstacleGrid) {
        let mobs = game.mobs.watch_physics_mutations();
        let boats = game.boats.watch_physics_mutations();
        let mob_order: FxHashMap<_, _> = game
            .mobs
            .iter()
            .enumerate()
            .map(|(order, mob)| (mob.join, order as u128))
            .collect();
        let mut boat_ids = FxHashMap::default();
        let boat_order: FxHashMap<_, _> = game
            .boats
            .iter()
            .enumerate()
            .map(|(order, boat)| {
                boat_ids.insert(boat.id, boat.join);
                (boat.join, (1_u128 << 64) + order as u128)
            })
            .collect();
        let grid = ObstacleGrid::with_keys(
            game.mobs
                .iter()
                .filter(|mob| super::ghasts::stops_movers(mob))
                .map(|mob| (mob.join, mob_order[&mob.join], mob.bounds()))
                .chain(
                    game.boats
                        .iter()
                        .map(|boat| (boat.join, boat_order[&boat.join], boat.bounds())),
                ),
        );
        (
            Self {
                mobs,
                boats,
                mob_order,
                boat_order,
                boat_ids,
                events: Vec::new(),
                dirty: FxHashSet::default(),
            },
            grid,
        )
    }
    fn remove_boat(&self, id: u32, grid: &mut ObstacleGrid) {
        if let Some(&join) = self.boat_ids.get(&id) {
            grid.refresh_key(join, self.boat_order[&join], None);
        }
    }
    fn refresh(&mut self, game: &Game<'_>, grid: &mut ObstacleGrid) {
        self.mobs.drain(&mut self.events);
        self.dirty
            .extend(self.events.iter().map(|event| event.join));
        for join in self.dirty.drain() {
            let next = self.mob_order.len() as u128;
            let order = *self.mob_order.entry(join).or_insert(next);
            let bounds = game
                .mobs
                .locate(join)
                .map(|at| &game.mobs[at])
                .filter(|mob| super::ghasts::stops_movers(mob))
                .map(|mob| mob.bounds());
            grid.refresh_key(join, order, bounds);
        }
        self.boats.drain(&mut self.events);
        self.dirty
            .extend(self.events.iter().map(|event| event.join));
        for join in self.dirty.drain() {
            let next = (1_u128 << 64) + self.boat_order.len() as u128;
            let order = *self.boat_order.entry(join).or_insert(next);
            let bounds = game.boats.locate(join).map(|at| {
                let boat = &game.boats[at];
                self.boat_ids.insert(boat.id, join);
                (!game
                    .persist
                    .removals
                    .contains(&super::commands::Target::Boat(boat.id)))
                .then(|| boat.bounds())
            });
            grid.refresh_key(join, order, bounds.flatten());
        }
    }
}

fn refresh_items(
    game: &Game<'_>,
    pass: &mut ItemPhysicsPass,
    watch: &Watch,
    events: &mut Vec<Mutation>,
    dirty: &mut FxHashSet<JoinSeq>,
    actor: JoinSeq,
) {
    watch.drain(events);
    dirty.extend(events.iter().map(|event| event.join));
    for join in dirty.drain() {
        let source = game
            .dropped
            .locate(join)
            .map(|at| &game.dropped[at])
            .filter(|item| {
                !game
                    .persist
                    .removals
                    .contains(&super::commands::Target::Item(item.id))
            });
        pass.index.refresh(join, source);
        if join != actor
            && let Some(item) = source
        {
            game.publish_lent_resident_reads(
                horizon_client::join_seq::residency::table::Location {
                    owner: horizon_client::join_seq::residency::Owner {
                        dimension: game.dim.key(),
                        kind: horizon_client::join_seq::residency::Kind::Item,
                    },
                    join,
                },
                BaseReadSource::Item(item),
                mod_api::EntityReadCapturePhase::AfterAppliedWrite,
            );
        }
    }
}

impl Game<'_> {
    pub(super) fn tick_dropped_items_listened(
        &mut self,
        round: &mut super::native_ticks::TickRound,
    ) {
        // Both owner publication and the stable schedule precede the first Pre.
        let _reads = self.begin_resident_read_pass(horizon_client::join_seq::residency::Kind::Item);
        let watch = self.dropped.watch_physics_mutations();
        let (mut colliders, grid) = ColliderChanges::new(self);
        let items = self.dropped.lift();
        let schedule: Vec<_> = items.iter().map(|item| item.join).collect();
        let mut pass = ItemPhysicsPass::new(items, grid, self.remote.is_none(), true);
        self.dropped.restore(
            std::mem::take(&mut pass.items),
            RestoreOrder::SurvivorsFirst,
        );
        let mut removal_cursor = 0;
        let mut events = Vec::new();
        let mut dirty = FxHashSet::default();
        for join in schedule {
            let Some(at) = self.dropped.locate(join) else {
                continue;
            };
            let item = &self.dropped[at];
            let ticking = self.entity_ticking_area();
            if (pass.merging && self.joined_during_entity_pass(item.id))
                || !ticking(
                    item.position.x.floor() as i32,
                    item.position.z.floor() as i32,
                )
            {
                continue;
            }
            let id = item.id;
            if self
                .persist
                .removals
                .contains(&super::commands::Target::Item(id))
            {
                self.dropped.remove(at);
                pass.index.refresh(join, None);
                continue;
            }
            let writer = self.begin_resident_actor_writer(BaseReadSource::Item(item));
            let mut lifted = false;
            let body = catch_unwind(AssertUnwindSafe(|| {
                if !self.native_tick_pre_one(round, id, "minecraft:item") {
                    return;
                }
                refresh_items(self, &mut pass, &watch, &mut events, &mut dirty, join);
                colliders.refresh(self, &mut pass.obstacles);
                for target in
                    &self.persist.removals[removal_cursor.min(self.persist.removals.len())..]
                {
                    match *target {
                        super::commands::Target::Item(id) => pass.index.remove_id(id),
                        super::commands::Target::Boat(id) => {
                            colliders.remove_boat(id, &mut pass.obstacles)
                        }
                        _ => {}
                    }
                }
                removal_cursor = self.persist.removals.len();
                let Some(at) = self.dropped.locate(join) else {
                    return;
                };
                if self
                    .persist
                    .removals
                    .contains(&super::commands::Target::Item(id))
                {
                    self.dropped.remove(at);
                    return;
                }
                pass.items = self.dropped.lift();
                lifted = true;
                self.tick_dropped_item_body(&mut pass, at);
                self.dropped.restore(
                    std::mem::take(&mut pass.items),
                    RestoreOrder::SurvivorsFirst,
                );
                lifted = false;
                // Only actual retirements touch storage; no population retain.
                for retired in pass.index.take_retired() {
                    if let Some(at) = self.dropped.locate(retired) {
                        self.dropped.remove(at);
                    }
                }
                refresh_items(self, &mut pass, &watch, &mut events, &mut dirty, join);
                self.native_tick_post_one(round, id, "minecraft:item");
            }));
            if lifted {
                self.dropped.restore(
                    std::mem::take(&mut pass.items),
                    RestoreOrder::SurvivorsFirst,
                );
            }
            for retired in pass.index.take_retired() {
                if let Some(at) = self.dropped.locate(retired) {
                    self.dropped.remove(at);
                }
            }
            let result = self.finish_resident_actor_turn(writer.as_ref(), body);
            drop(writer);
            refresh_items(self, &mut pass, &watch, &mut events, &mut dirty, join);
            colliders.refresh(self, &mut pass.obstacles);
            if let Err(panic) = result {
                resume_unwind(panic);
            }
        }
        for pos in pass.bubble_tops {
            self.bubble_column_particles(pos);
        }
        let items = self.dropped.lift();
        self.finish_dropped_item_pickups(items, pass.destroyed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, OnceLock};
    struct Observer {
        mode: u8,
        ids: Arc<OnceLock<[u32; 3]>>,
        order: Arc<Mutex<Vec<&'static str>>>,
    }
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for Observer {
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
            let [a, _, mob] = *self.ids.get().unwrap();
            let actor = host.entity(event.entity).unwrap();
            assert_eq!(actor.position(host).unwrap()[1], 80.0);
            self.order
                .lock()
                .unwrap()
                .push(if event.entity == a { "pre A" } else { "pre B" });
            if event.entity == a {
                assert!(matches!(
                    host.entity(mob).unwrap().set_silent(host, true),
                    mod_api::WriteReceipt::Pending(_)
                ));
                if self.mode == 1 {
                    return mod_api::Flow::Cancel;
                }
                if self.mode == 2 {
                    assert!(actor.discard(host));
                }
                if self.mode == 3 {
                    panic!("primary item Pre panic");
                }
            } else {
                assert_eq!(
                    self.order
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|&&name| name == "done")
                        .count(),
                    1
                );
                assert_eq!(host.entity(mob).unwrap().is_silent(host), Some(true));
            }
            mod_api::Flow::Continue
        }
        fn entity_tick_post(
            &self,
            host: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::EntityTickPostEvent<'_>,
        ) {
            let [a, _, _] = *self.ids.get().unwrap();
            let actor = host.entity(event.entity).unwrap();
            assert!((actor.position(host).unwrap()[1] - 79.96).abs() < 1e-12);
            self.order.lock().unwrap().push(if event.entity == a {
                "post A"
            } else {
                "post B"
            });
        }
        fn entity_command_completed(
            &self,
            _: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::EntityCommandCompletedEvent<'_>,
        ) {
            let mod_api::EntityWriteCompletion::EntitySetSilent(done) = event.completion;
            assert_eq!(done.outcome, Ok(true));
            self.order.lock().unwrap().push("done");
            if self.mode == 3 {
                panic!("secondary item completion panic");
            }
        }
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn item_turns_restore_canonical_pre_post_and_finish_fifo_before_next_actor() {
        for mode in 0..4 {
            let ids = Arc::new(OnceLock::new());
            let order = Arc::new(Mutex::new(Vec::new()));
            mod_api::testing::with_mods(
                vec![mod_api::ModEntry::new(
                    "item_turn_observer",
                    Box::new(Observer {
                        mode,
                        ids: ids.clone(),
                        order: order.clone(),
                    }),
                )]
                .into(),
                || {
                    crate::game::native_entity_refs::tests::with_game(|game| {
                        game.options.dedicated = Some(Vec::new());
                        game.player = None;
                        game.world
                            .chunks
                            .insert((0, 0), Arc::new(horizon_client::chunk::Chunk::empty(0, 0)));
                        game.tickets.update(None);
                        for x in [8.5, 12.5] {
                            game.summon_with_nbt("minecraft:item",[x,80.0,8.5],"{Item:{id:\"minecraft:sand\",count:1},PickupDelay:0s,Motion:[0.0d,0.0d,0.0d]}").unwrap();
                        }
                        game.summon_with_nbt("minecraft:zombie", [4.5, 80.0, 8.5], "{}")
                            .unwrap();
                        game.native_joins_skip();
                        ids.set([game.dropped[0].id, game.dropped[1].id, game.mobs[0].id])
                            .unwrap();
                        game.ticked_entities_through = u32::MAX;
                        let result = catch_unwind(AssertUnwindSafe(|| game.tick_items()));
                        assert!(game.mobs[0].silent);
                        if mode == 3 {
                            let panic = result.unwrap_err();
                            assert_eq!(
                                panic.downcast_ref::<&str>(),
                                Some(&"primary item Pre panic")
                            );
                            assert_eq!(game.dropped.len(), 2);
                            assert!(game.dropped.iter().all(|item| item.position.y == 80.0));
                        } else {
                            result.unwrap();
                            assert_eq!(game.dropped.len(), if mode == 2 { 1 } else { 2 });
                        }
                    });
                },
            );
            let expected: &[&str] = match mode {
                0 => &["pre A", "post A", "done", "pre B", "post B"],
                1 | 2 => &["pre A", "done", "pre B", "post B"],
                _ => &["pre A", "done"],
            };
            assert_eq!(&*order.lock().unwrap(), expected);
        }
    }
}

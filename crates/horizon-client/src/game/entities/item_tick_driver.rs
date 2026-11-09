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
    index: &mut super::item_merge_index::ItemMergeIndex,
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
        index.refresh(join, source);
        if join != actor
            && let Some(item) = source
        {
            game.publish_lent_resident_reads(
                horizon_client::join_seq::residency::table::Location {
                    source: horizon_client::join_seq::residency::Source::Ordinary,
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

pub(super) struct ItemChanges {
    watch: Watch,
    colliders: ColliderChanges,
    events: Vec<Mutation>,
    dirty: FxHashSet<JoinSeq>,
    removal_cursor: usize,
}
impl ItemChanges {
    fn new(game: &mut Game<'_>) -> (Self, ObstacleGrid) {
        let watch = game.dropped.watch_physics_mutations();
        let (colliders, grid) = ColliderChanges::new(game);
        (
            Self {
                watch,
                colliders,
                events: Vec::new(),
                dirty: FxHashSet::default(),
                removal_cursor: 0,
            },
            grid,
        )
    }
    pub(super) fn refresh(
        &mut self,
        game: &Game<'_>,
        index: &mut super::item_merge_index::ItemMergeIndex,
        grid: &mut ObstacleGrid,
        actor: JoinSeq,
    ) {
        refresh_items(
            game,
            index,
            &self.watch,
            &mut self.events,
            &mut self.dirty,
            actor,
        );
        self.colliders.refresh(game, grid);
        if self.removal_cursor > game.persist.removals.len() {
            self.removal_cursor = 0;
        }
        for target in &game.persist.removals[self.removal_cursor..] {
            match *target {
                super::commands::Target::Item(id) => index.remove_id(id),
                super::commands::Target::Boat(id) => self.colliders.remove_boat(id, grid),
                _ => {}
            }
        }
        self.removal_cursor = game.persist.removals.len();
    }
}

impl Game<'_> {
    /// The whole actual owner is resident for the callback. No body reference
    /// or resident element position crosses it; the caller resolves its join again.
    pub(super) fn with_canonical_item_callback<R>(
        &mut self,
        items: &mut horizon_client::join_seq::JoinList<horizon_client::entities::ItemEntity>,
        index: &mut super::item_merge_index::ItemMergeIndex,
        obstacles: &mut ObstacleGrid,
        changes: &mut Option<ItemChanges>,
        actor: JoinSeq,
        callback: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let Some(changes) = changes else {
            return callback(self);
        };
        self.dropped
            .restore(std::mem::take(items), RestoreOrder::SurvivorsFirst);
        if let Some(writer) = self.active_entity_writer() {
            self.publish_resident_read_location(
                writer.actor.location,
                mod_api::EntityReadCapturePhase::BeforeCallback,
            );
        }
        let result = catch_unwind(AssertUnwindSafe(|| callback(self)));
        let completion = catch_unwind(AssertUnwindSafe(|| {
            changes.refresh(self, index, obstacles, actor)
        }));
        *items = self.dropped.lift();
        match result {
            Err(primary) => {
                if completion.is_err() {
                    log::error!("item index completion also panicked during callback unwind");
                }
                resume_unwind(primary)
            }
            Ok(value) => {
                if let Err(panic) = completion {
                    resume_unwind(panic);
                }
                value
            }
        }
    }
    pub(super) fn tick_dropped_items_listened(
        &mut self,
        round: &mut super::native_ticks::TickRound,
    ) {
        // Both owner publication and the stable schedule precede the first Pre.
        let _reads = self.begin_resident_read_pass(horizon_client::join_seq::residency::Kind::Item);
        let (changes, grid) = ItemChanges::new(self);
        let items = self.dropped.lift();
        let schedule: Vec<_> = items.iter().map(|item| item.join).collect();
        let mut pass = ItemPhysicsPass::new(items, grid, self.remote.is_none(), true);
        pass.changes = Some(changes);
        self.dropped.restore(
            std::mem::take(&mut pass.items),
            RestoreOrder::SurvivorsFirst,
        );
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
                pass.refresh(self, join);
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
                pass.refresh(self, join);
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
            pass.refresh(self, join);
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
    struct ExpiryObserver {
        panic: bool,
        mob: Arc<OnceLock<u32>>,
        completed: Arc<std::sync::atomic::AtomicUsize>,
    }
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for ExpiryObserver {
        fn item_expire(
            &self,
            host: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::ItemExpireEvent<'_>,
        ) {
            let actor = host
                .entity(event.entity)
                .expect("actual item is canonical at expiry");
            assert!((actor.position(host).unwrap()[1] - 79.96).abs() < 1e-12);
            assert_eq!(
                actor.cached_row(host).unwrap().capture.phase,
                mod_api::EntityReadCapturePhase::BeforeCallback
            );
            assert!(matches!(
                host.entity(*self.mob.get().unwrap())
                    .unwrap()
                    .set_silent(host, true),
                mod_api::WriteReceipt::Pending(_)
            ));
            if self.panic {
                panic!("item expiry panic");
            }
            event.extra_life = 20;
        }
        fn entity_command_completed(
            &self,
            _: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::EntityCommandCompletedEvent<'_>,
        ) {
            let mod_api::EntityWriteCompletion::EntitySetSilent(done) = event.completion;
            assert_eq!(done.outcome, Ok(true));
            self.completed
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn item_expiry_sees_actual_progress_and_unwind_restores_it_before_fifo() {
        for panic in [false, true] {
            let mob = Arc::new(OnceLock::new());
            let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            mod_api::testing::with_mods(
                vec![mod_api::ModEntry::new(
                    "expiry_observer",
                    Box::new(ExpiryObserver {
                        panic,
                        mob: mob.clone(),
                        completed: completed.clone(),
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
                        game.summon_with_nbt("minecraft:item",[8.5,80.0,8.5],"{Item:{id:\"minecraft:sand\",count:1},Age:5999s,PickupDelay:0s,Motion:[0.0d,0.0d,0.0d]}").unwrap();
                        game.summon_with_nbt("minecraft:pig", [4.5, 80.0, 8.5], "{}")
                            .unwrap();
                        game.native_joins_skip();
                        mob.set(game.mobs[0].id).unwrap();
                        game.ticked_entities_through = u32::MAX;
                        let result = catch_unwind(AssertUnwindSafe(|| game.tick_items()));
                        if panic {
                            assert_eq!(
                                result.unwrap_err().downcast_ref::<&str>(),
                                Some(&"item expiry panic")
                            );
                        } else {
                            result.unwrap();
                        }
                        assert_eq!(completed.load(std::sync::atomic::Ordering::Relaxed), 1);
                        assert!(game.mobs[0].silent);
                        assert_eq!(game.dropped.len(), 1);
                        assert!((game.dropped[0].position.y - 79.96).abs() < 1e-12);
                        assert_eq!(game.dropped[0].age, if panic { 6000 } else { 5980 });
                    });
                },
            );
        }
    }
    struct BirthOnContact(Arc<std::sync::atomic::AtomicUsize>);
    #[mod_api::horizon_part]
    impl mod_api::BlockPart for BirthOnContact {
        fn item_after_vertical_collision(
            &self,
            host: mod_api::HostSide,
            event: &mut mod_api::ItemAfterVerticalCollisionEvent<'_>,
        ) -> mod_api::Flow {
            let mod_api::HostSide::Server(host) = host else {
                panic!("expected server contact");
            };
            let actor = host
                .entity(event.entity)
                .expect("contact owner is canonical");
            assert_eq!(actor.position(host).unwrap()[1], 65.0);
            assert!(
                actor
                    .spawn_at_location(host, event.stack.as_ref().unwrap().clone(), 0.0)
                    .unwrap()
                    .is_some()
            );
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            mod_api::Flow::Continue
        }
    }
    struct BirthMod(Arc<std::sync::atomic::AtomicUsize>);
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for BirthMod {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            let stone = r.block_key("minecraft:stone");
            r.block_part(stone, BirthOnContact(self.0.clone()));
        }
    }
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn contact_birth_is_a_merge_neighbor_but_not_another_tick_this_pass() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "contact_birth",
            Box::new(BirthMod(calls.clone())),
        )]
        .into();
        crate::game::api_values::install_parts(&mods).unwrap();
        mod_api::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                game.player = None;
                game.build_registries();
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(horizon_client::chunk::Chunk::empty(0, 0)));
                game.tickets.update(None);
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                game.world.set(8, 64, 8, stone).unwrap();
                game.summon_with_nbt("minecraft:item",[8.5,65.01,8.5],"{Item:{id:\"minecraft:sand\",count:2},Age:10s,PickupDelay:0s,Motion:[0.0d,-0.2d,0.0d]}").unwrap();
                game.dropped[0].ticks = 39;
                game.native_joins_skip();
                let original = game.dropped[0].join;
                game.ticked_entities_through = u32::MAX;
                game.tick_items();
                assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
                assert_eq!(game.dropped.len(), 1);
                let newborn = &game.dropped[0];
                assert_ne!(newborn.join, original);
                assert_eq!(newborn.stack.count, 4);
                assert_eq!(newborn.age, 0);
                assert_eq!(newborn.position.y, 65.0);
            });
        });
    }
}

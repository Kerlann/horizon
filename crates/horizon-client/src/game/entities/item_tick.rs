//! Shared item physics arithmetic. Callback-driven orchestration and the
//! compact unlistened pass use one body, with pass-owned scratch storage.
use super::{Game, ItemStack};
use horizon_client::{
    entities::{ItemCells, ItemEntity, ObstacleGrid},
    join_seq::JoinList,
};
use horizon_core::random::RandomSource;
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
};

pub(super) struct ItemPhysicsPass {
    pub(super) items: JoinList<ItemEntity>,
    pub(super) removed: Vec<bool>,
    pub(super) cells: ItemCells,
    pub(super) obstacles: ObstacleGrid,
    pub(super) nearby: Vec<Aabb>,
    pub(super) bubble_tops: Vec<BlockPos>,
    pub(super) destroyed: Vec<(ItemStack, Vec3)>,
    pub(super) merging: bool,
    pub(super) capture_reads: bool,
}
impl ItemPhysicsPass {
    pub(super) fn new(
        items: JoinList<ItemEntity>,
        collisions: Vec<Aabb>,
        merging: bool,
        capture_reads: bool,
    ) -> Self {
        Self {
            removed: vec![false; items.len()],
            cells: ItemCells::new(&items),
            items,
            obstacles: ObstacleGrid::new(collisions),
            nearby: Vec::new(),
            bubble_tops: Vec::new(),
            destroyed: Vec::new(),
            merging,
            capture_reads,
        }
    }
}
impl Game<'_> {
    pub(super) fn tick_dropped_item_body(&mut self, pass: &mut ItemPhysicsPass, i: usize) {
        let ItemPhysicsPass {
            items,
            removed,
            cells,
            obstacles,
            nearby,
            bubble_tops,
            destroyed,
            merging,
            capture_reads,
        } = pass;
        let (merging, capture_reads) = (*merging, *capture_reads);
        // Entity.baseTick: the entity-level invulnerability window of
        // the saved data counts down (#916) — only for an item that
        // really ticks in this pass.
        self.tick_saved_invulnerable_time(items[i].id);
        let item = &mut items[i];
        // `getEntityCollisions(entity, bounds.expandTowards(movement))`: the boxes around
        // where the item can reach this tick (a block of slack for the next velocity).
        let reach = Vec3::new(
            item.velocity.x.abs() + 1.0,
            item.velocity.y.abs() + 1.0,
            item.velocity.z.abs() + 1.0,
        );
        obstacles.near(item.bounds().inflate(reach), nearby);
        let no_physics = merging
            && item.inside_block(
                &self.world,
                self.collision,
                nearby,
                Some(self.border.geometry()),
            );
        let escape_speed = no_physics.then(|| item.random.next_float() * 0.2 + 0.1);
        let mut entered = Vec::new();
        let (alive, discarded) =
            self.tick_item_with_block_contacts(item, escape_speed, nearby, false, &mut |splash| {
                entered.push(splash)
            });
        // Entity.doWaterSplashEffect: the server plays the sound (broadcast
        // to clients). Its addParticle is empty.
        for splash in entered {
            self.apply_item_splash(splash);
        }
        if discarded {
            removed[i] = true;
            cells.remove(i, item.previous);
            return;
        }
        if merging && !no_physics {
            self.item_landing_event(item.id, item.previous, item.position, item.on_ground);
            self.entity_powder_snow_particles(
                item.previous,
                item.position,
                item.bounds(),
                item.velocity,
                false,
            );
        }
        cells.moved(i, item.previous, item.position);
        // Entity.handleOnAboveBubbleColumn: the server sends the particles of
        // the surface of a bubble column the item is at the top of.
        if merging {
            bubble_tops.extend(horizon_client::motion::bubble_column_surfaces(
                &self.world,
                self.pack.assets.registry,
                self.res.facts(),
                self.collision,
                item.bounds(),
            ));
        }
        // ItemEntity.tick: merge every 2 ticks while moving between
        // blocks, every 40 otherwise, before the lifetime check.
        let block = |v: Vec3| (v.x.floor(), v.y.floor(), v.z.floor());
        let rate = if block(item.previous) == block(item.position) {
            40
        } else {
            2
        };
        if merging
            && (alive || item.age == 6000 && item.position.y > -128.0)
            && item.ticks.is_multiple_of(rate)
        {
            let max_stack = |stack: &ItemStack| self.stack_max_size(stack).max(1);
            horizon_client::entities::merge_with_neighbours(items, removed, cells, i, &max_stack);
        }
        let item = &mut items[i];
        let mut alive = alive || (item.age < 6000 && item.position.y > -128.0);
        if merging && !removed[i] && item.age >= 6000 && item.position.y > -128.0 {
            let mut extra_life = 0;
            self.native_item_expire(item.id, &item.stack, &mut extra_life);
            if extra_life > 0 {
                item.age = 6000 - extra_life.min(32766);
                alive = true;
            } else if self.serving.is_some() || self.options.dedicated.is_some() {
                self.native_item_despawn(
                    item.id,
                    &item.stack.item,
                    item.stack.count as i32,
                    [item.position.x, item.position.y, item.position.z],
                );
            }
        }
        if !alive || removed[i] {
            removed[i] = true;
            cells.remove(i, item.position);
            return;
        }
        let item_id = item.id;
        let saved = [
            "minecraft:on_fire",
            "minecraft:lava",
            "minecraft:in_fire",
            "minecraft:cactus",
        ]
        .map(|damage| self.saved_refuses_damage(item_id, damage, false));
        let tags = world_items!(self);
        let registry = self.pack.assets.registry;
        let block_registries = &self.pack.block_registries;
        let hooks = &self.damage_hooks;
        let environment = item.environment_tick_with_hook(
            &mut self.world,
            self.res.facts(),
            registry,
            tags,
            |world, damage| {
                let vanilla = match damage {
                    "minecraft:on_fire" => saved[0],
                    "minecraft:lava" => saved[1],
                    "minecraft:in_fire" => saved[2],
                    "minecraft:cactus" => saved[3],
                    _ => false,
                };
                let source = horizon_client::health::HurtSource {
                    damage_type: damage,
                    causing: 0,
                    direct: 0,
                    position: None,
                };
                horizon_client::health::DamageLevel::of(
                    world,
                    registry,
                    Some(tags),
                    Some(block_registries),
                    hooks,
                )
                .invulnerability_check(
                    horizon_client::health::Hurting::Entity(
                        item_id as i32,
                        "minecraft:item",
                        source,
                    ),
                    vanilla,
                )
            },
        );
        if environment.should_play_lava_hurt_sound(item.ticks) {
            let pitch = 2.0 + self.world.random.next_float() * 0.4;
            self.audio.play(
                "minecraft:entity.generic.burn",
                Some([item.position.x, item.position.y, item.position.z]),
                0.4,
                pitch,
            );
        }
        if !environment.alive {
            removed[i] = true;
            cells.remove(i, item.position);
            for stack in environment.spilled {
                destroyed.push((stack, item.position));
            }
        }
        if capture_reads && !removed[i] {
            self.publish_lent_resident_reads(
                horizon_client::join_seq::residency::table::Location {
                    owner: horizon_client::join_seq::residency::Owner {
                        dimension: self.dim.key(),
                        kind: horizon_client::join_seq::residency::Kind::Item,
                    },
                    join: item.join,
                },
                super::native_base_read_rows::BaseReadSource::Item(item),
                mod_api::EntityReadCapturePhase::AfterEntityTick,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn compact_item_pass_keeps_vanilla_merge_age_and_motion() {
        mod_api::testing::with_mods(Vec::new().into(), || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                game.player = None;
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
                );
                game.tickets.update(None);
                for (x, count, age) in [(8.5, 3, 39), (8.6, 5, 17), (12.5, 2, 39)] {
                    game.summon_with_nbt("minecraft:item",[x,80.0,8.5],&format!("{{Item:{{id:\"minecraft:sand\",count:{count}}},Age:{age}s,PickupDelay:0s,Motion:[0.0d,0.0d,0.0d]}}")).unwrap();
                }
                for item in &mut game.dropped {
                    item.ticks = 39;
                }
                game.ticked_entities_through = u32::MAX;
                game.tick_items();
                assert_eq!(game.dropped.len(), 2);
                assert_eq!(
                    game.dropped
                        .iter()
                        .map(|item| (item.stack.count, item.age))
                        .collect::<Vec<_>>(),
                    [(8, 18), (2, 40)]
                );
                for item in &game.dropped {
                    assert!((item.position.y - 79.96).abs() < 1e-12);
                    // Vanilla widens the 0.98F air drag before multiplying.
                    assert!((item.velocity.y + 0.03920000076293945).abs() < 1e-12);
                }
            });
        });
    }
}

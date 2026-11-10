use super::*;

impl Game<'_> {
    /// Dropped items touching a guest go into their inventory
    /// (`ItemEntity.playerTouch`, then `ServerPlayer.onItemPickup`).
    pub(super) fn guest_pickups(&mut self, gi: usize) {
        let Some(host) = self.lan.as_ref() else {
            return;
        };
        let (conn, mode, position, id, uuid) = {
            let g = &host.hub.guests[gi];
            (
                g.conn,
                g.game_mode,
                g.position,
                g.entity_id,
                horizon_client::lan::playerdata::uuid_ints(g.uuid),
            )
        };
        if mode == SPECTATOR || self.dropped.is_empty() {
            return;
        }
        // The test below takes items whose position is less than 1.5 from
        // the player's on x and z: their boxes reach these columns.
        if let Some(columns) = self.pickup_columns.as_mut()
            && !columns.may_reach_area(
                &self.dropped,
                (
                    (position[0] - 1.5).floor() as i32,
                    (position[0] + 1.5).floor() as i32,
                ),
                (
                    (position[2] - 1.5).floor() as i32,
                    (position[2] + 1.5).floor() as i32,
                ),
            )
        {
            return;
        }
        let near: Vec<_> = self
            .dropped
            .iter()
            .filter(|d| {
                !self
                    .persist
                    .removals
                    .contains(&super::super::commands::Target::Item(d.id))
                    && d.pickup_delay == 0
                    && d.target.is_none_or(|t| t == uuid)
                    && (d.position.x - position[0]).abs() < 1.5
                    && (d.position.z - position[2]).abs() < 1.5
                    && d.position.y > position[1] - 0.6
                    && d.position.y < position[1] + 2.4
            })
            .map(|d| (d.id, d.join))
            .collect();
        let mut taken = Vec::new();
        for (item_id, item_join) in near.into_iter().rev() {
            let Some(mut actor) = crate::game::entity_actor_loan::EntityActorLoan::<
                horizon_client::entities::ItemEntity,
            >::extract(self, item_join) else {
                continue;
            };
            let stack = actor.body_mut().unwrap().stack.clone();
            let Some(Some(refused)) = actor.with_game(self, |game| {
                game.as_guest(conn, |game| game.native_pickup_refused(&stack))
            }) else {
                actor.restore(self);
                continue;
            };
            if refused {
                actor.body_mut().unwrap().pickup_delay = 10;
                actor.restore(self);
                actor.publish(self, mod_api::EntityReadCapturePhase::AfterAppliedWrite);
                continue;
            }
            let item = actor.body_mut().unwrap();
            let (stack, thrower) = (item.stack.clone(), item.thrower);
            let before = stack.count;
            let Some(Some(rest)) = actor.with_game(self, |game| {
                game.as_guest(conn, |game| game.add_stack_to_inventory(stack))
            }) else {
                actor.restore(self);
                continue;
            };
            match rest {
                None => {
                    let join = actor.join();
                    let mut consumed = None;
                    actor.with_game(self, |game| {
                        if let Some(index) = game.dropped.locate(join) {
                            consumed = Some(game.remove_dropped(index).stack);
                        }
                    });
                    if let Some(stack) = consumed {
                        if let Some(thrower) = thrower {
                            self.as_guest(conn, |game| {
                                game.trigger_thrown_picked_up_by_player(&stack, thrower)
                            });
                        }
                        taken.push((item_id, before));
                    }
                }
                Some(rest) if rest.count < before => {
                    let amount = before - rest.count;
                    actor.body_mut().unwrap().stack = rest;
                    if let Some(thrower) = thrower {
                        let left = actor.body_mut().unwrap().stack.clone();
                        actor.with_game(self, |game| {
                            game.as_guest(conn, |game| {
                                game.trigger_thrown_picked_up_by_player(&left, thrower)
                            })
                        });
                    }
                    // A post-pickup criterion may remove the actor; it cannot
                    // change the amount already inserted into the inventory.
                    taken.push((item_id, amount));
                }
                Some(_) => {}
            }
            actor.restore(self);
            actor.publish(self, mod_api::EntityReadCapturePhase::AfterAppliedWrite);
        }
        // Entity.take: the clients play the pickup sound themselves
        // (handleTakeItemEntity).
        if let Some(host) = self.lan.as_ref() {
            for (item, count) in taken {
                host.hub.take_item(item as i32, id, count as i32, position);
            }
        }
    }

    /// `Player.aiStep` touching arrows in the guest's box grown by (1, 0.5, 1):
    /// `AbstractArrow.playerTouch` for a guest, as `touch_arrows` does for the
    /// host's player. The guest's own client plays the pickup sound
    /// (`handleTakeItemEntity`).
    pub(super) fn guest_arrows(&mut self, gi: usize) {
        use horizon_client::arrows::Touch;
        let Some(host) = self.lan.as_ref() else {
            return;
        };
        let (conn, id, at, creative, unable, dims, player, name) = {
            let g = &host.hub.guests[gi];
            (
                g.conn,
                g.entity_id,
                g.position,
                g.creative(),
                g.spectator() || g.dead || g.health <= 0.0,
                g.dimensions(),
                g.uuid,
                g.name.clone(),
            )
        };
        if unable || self.creatures.projectiles.is_empty() {
            return;
        }
        let w = f64::from(dims.width) / 2.0;
        let reach = Aabb::new(
            Vec3::new(at[0] - w, at[1], at[2] - w),
            Vec3::new(at[0] + w, at[1] + f64::from(dims.height), at[2] + w),
        )
        .inflate(Vec3::new(1.0, 0.5, 1.0));
        // Keep every actual body canonical while the guest's inventory is
        // borrowed. Subscribed callbacks use exact initial joins; without
        // callbacks a compact cursor needs no new schedule allocation.
        let initial_len = self.creatures.projectiles.len();
        let schedule = mod_api::__private::entity_reads_listened().then(|| {
            self.creatures
                .projectiles
                .iter()
                .map(|shot| shot.join)
                .collect::<Vec<_>>()
        });
        // Existing batch-boundary fixture injection, without lending storage.
        #[cfg(test)]
        crate::game::entity::taken_pass_tests::after_take(self);
        let mut picked = Vec::new();
        let mut cursor = 0;
        for ordinal in 0..initial_len {
            let index = match &schedule {
                Some(joins) => self.creatures.projectiles.locate(joins[ordinal]),
                None => (cursor < self.creatures.projectiles.len()).then_some(cursor),
            };
            let Some(index) = index else { continue };
            cursor = index + 1;
            let shot = &self.creatures.projectiles[index];
            let join = shot.join;
            let Some(a) = shot.arrow.as_ref().filter(|a| !a.removed) else {
                continue;
            };
            let p = shot.position;
            let bounds = Aabb::new(
                Vec3::new(p.x - 0.25, p.y, p.z - 0.25),
                Vec3::new(p.x + 0.25, p.y + 0.5, p.z + 0.25),
            );
            if !bounds.intersects(reach) {
                continue;
            }
            let touch = a.player_touch(creative);
            let taken = match touch {
                Touch::Ignore => false,
                Touch::Take => true,
                Touch::Give(stack) => {
                    self.as_guest(conn, |g| g.add_stack_to_inventory(stack))
                        .flatten()
                        .is_none()
                        || creative
                }
            };
            if taken && let Some(index) = self.creatures.projectiles.locate(join) {
                cursor = index;
                let shot = self.creatures.projectiles.remove(index);
                if let Some(arrow) = shot.arrow {
                    picked.push((shot.id, arrow.pickup_item.item));
                }
            }
        }
        // Keep the existing after-pass notification moment. Only operation
        // arguments survive removal, never cloned authoritative projectile bodies.
        for (shot, item) in picked {
            self.native_player_pickup_arrow(player, &name, shot, &item);
            if let Some(host) = self.lan.as_ref() {
                host.hub.take_item(shot as i32, id, 1, at);
            }
        }
    }

    /// `Player.tick`'s takeXpDelay and `Player.aiStep`'s touch of the
    /// experience orbs in the guest's box grown by 1, 0.5, 1: one of them at
    /// random, at most every 2 ticks (`ExperienceOrb.playerTouch`), repairs
    /// with Mending and gives the rest as points. Everyone near hears it.
    pub(super) fn guest_orbs(&mut self, gi: usize) {
        let Some(host) = self.lan.as_mut() else {
            return;
        };
        let g = &host.hub.guests[gi];
        let own = g.bounds();
        let vehicle = g.vehicle;
        let (conn, id, at, player, name) =
            (g.conn, g.entity_id, g.position, g.uuid, g.name.clone());
        let unable = g.spectator() || g.dead || g.health <= 0.0;
        let play = host.play.entry(conn).or_default();
        let cooldown = if play.xp_delay > 0 {
            play.xp_delay -= 1;
            Some(play.xp_delay)
        } else {
            None
        };
        let delayed = play.xp_delay > 0;
        if let Some(cooldown) = cooldown {
            self.native_player_exp_cooldown_change(player, &name, cooldown);
        }
        if unable || delayed {
            return;
        }
        let area = match vehicle.and_then(|v| self.guest_target_box(gi, v as i32)) {
            Some(vehicle) => own.union_bounds(vehicle).inflate(Vec3::new(1.0, 0.0, 1.0)),
            None => own.inflate(Vec3::new(1.0, 0.5, 1.0)),
        };
        let (min, max) = (
            [
                area.min_position().x,
                area.min_position().y,
                area.min_position().z,
            ],
            [
                area.max_position().x,
                area.max_position().y,
                area.max_position().z,
            ],
        );
        let touching: Vec<_> = self
            .orbs
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                !self
                    .persist
                    .removals
                    .contains(&super::super::commands::Target::Orb(o.id))
                    && o.position.x + 0.25 > min[0]
                    && o.position.x - 0.25 < max[0]
                    && o.position.y + 0.5 > min[1]
                    && o.position.y < max[1]
                    && o.position.z + 0.25 > min[2]
                    && o.position.z - 0.25 < max[2]
            })
            .map(|(_, orb)| orb.join)
            .collect();
        if touching.is_empty() {
            return;
        }
        // Util.getRandom(orbs, random).
        let pick = self
            .world
            .random
            .next_int_bound(touching.len() as i32)
            .unwrap_or(0) as usize;
        // A merged orb gives its value once per count and goes at zero.
        let join = touching[pick];
        let Some(mut actor) = crate::game::entity_actor_loan::EntityActorLoan::<
            horizon_client::entities::ExperienceOrb,
        >::extract(self, join) else {
            return;
        };
        let orb = actor.body_mut().unwrap();
        let (orb_id, mut value, mut count) = (orb.id, orb.value, orb.count);
        let mut canceled = false;
        // Swapping the guest in costs: only when a mod hears the pickup.
        let listened = (self.serving.is_some() || self.options.dedicated.is_some())
            && mod_api::__private::listened(mod_api::HookId::PickupXp);
        if listened {
            let dimension = self.dim.id();
            let called = actor.with_game(self, |game| {
                game.as_guest(conn, |game| {
                    let player = game.native_acting_id();
                    game.native_pickup_xp(
                        player,
                        dimension,
                        orb_id,
                        &mut value,
                        &mut count,
                        &mut canceled,
                    );
                })
            });
            if !matches!(called, Some(Some(()))) || canceled {
                actor.restore(self);
                return;
            }
            let orb = actor.body_mut().unwrap();
            orb.value = value;
            orb.count = count.max(1);
        }
        let orb = actor.body_mut().unwrap();
        let (value, position) = (orb.value, orb.position);
        if orb.count <= 1 {
            let join = actor.join();
            actor.with_game(self, |game| {
                if let Some(index) = game.orbs.locate(join) {
                    game.remove_orb(index);
                }
            });
        } else {
            orb.count -= 1;
            actor.restore(self);
            actor.publish(self, mod_api::EntityReadCapturePhase::AfterAppliedWrite);
        }
        if let Some(host) = self.lan.as_mut() {
            host.play.entry(conn).or_default().xp_delay = 2;
        }
        self.native_player_exp_cooldown_change(player, &name, 2);
        if let Some(host) = self.lan.as_ref() {
            host.hub.take_item(orb_id as i32, id, 1, at);
        }
        // ClientPacketListener.handleTakeItemEntity's sound, at the orb.
        let pitch = (self.world.random.next_float() - self.world.random.next_float()) * 0.35 + 0.9;
        let p = position;
        self.audio.play(
            "minecraft:entity.experience_orb.pickup",
            Some([p.x, p.y, p.z]),
            0.1,
            pitch,
        );
        let value = value.max(0) as u32;
        self.as_guest(conn, |game| {
            let rest = game.mending(value);
            if rest > 0 {
                game.give_experience_points(rest.min(i32::MAX as u32) as i32);
            }
        });
    }
}

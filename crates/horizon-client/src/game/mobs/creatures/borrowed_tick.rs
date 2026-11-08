//! The borrowed mob-tick region; ownership and restoration stay in the caller.
use super::*;

/// Stack-only borrows of the caller's existing pass and bookkeeping.
pub(super) struct BorrowedMobTick<'a, 'p, F> {
    pub(super) mobs: &'a mut horizon_client::join_seq::JoinList<Mob>,
    pub(super) snapshot: &'a mut Vec<Seen>,
    pub(super) events: &'a mut Vec<MobEvent>,
    pub(super) dead: &'a mut Vec<Mob>,
    pub(super) corpses: &'a mut Vec<Mob>,
    pub(super) died: &'a mut horizon_client::fast_hash::FxHashSet<u32>,
    pub(super) ticked: &'a mut horizon_client::fast_hash::FxHashSet<u32>,
    pub(super) guests: &'a mut super::super::lan_mobs::GuestViews,
    pub(super) spare: &'a mut Option<PlayerView>,
    pub(super) bindings: &'a mut horizon_client::fast_hash::FxHashMap<u32, u32>,
    pub(super) guest_ids: &'a [(u32, u32)],
    pub(super) guest_events: &'a mut Vec<(u32, u32, MobEvent)>,
    pub(super) player_credit:
        &'a mut std::collections::HashMap<u32, super::super::combat::PlayerCredit>,
    pub(super) path_writes: &'a [((i32, i32, i32), u32, u32)],
    pub(super) gone: &'a mut Vec<(u32, Vec3, f64, f64, Option<Mob>)>,
    pub(super) lent: &'a mut horizon_client::health::LentLevel,
    pub(super) laps: &'a mut horizon_client::spans::Laps,
    pub(super) uuid: Option<[i32; 4]>,
    pub(super) level_id: &'static str,
    pub(super) ticking: &'a F,
    pub(super) nearest: &'a super::super::natural_spawning::NearestPlayers<'p>,
    pub(super) round: &'a super::super::native_ticks::TickRound,
}

impl Game<'_> {
    pub(super) fn tick_borrowed_creatures<F: Fn(i32, i32) -> bool>(
        &mut self,
        borrowed: BorrowedMobTick<'_, '_, F>,
    ) {
        let BorrowedMobTick {
            mobs,
            snapshot,
            events,
            dead,
            corpses,
            died,
            ticked,
            guests,
            spare,
            bindings,
            guest_ids,
            guest_events,
            player_credit,
            path_writes,
            gone,
            lent,
            laps,
            uuid,
            level_id,
            ticking,
            nearest,
            round,
        } = borrowed;
        let mut pushes = horizon_client::mobs::PendingPushes::default();
        let mut keep = Vec::with_capacity(mobs.len());
        let mut grid =
            horizon_client::mobs::seen_grid::SeenGrid::new(snapshot.iter().map(|s| s.bounds));
        // All but the snapshot is the same for every mob of the tick, so
        // build it once: the player view and the teams scan every UUID.
        self.leash_boats = self
            .boats
            .iter()
            .map(super::super::boat_leads::boat_state)
            .collect();
        let mut base = self.mob_world_base(&[]);
        let mut level = horizon_client::health::DamageLevel::lent(
            &self.world,
            self.pack.assets.registry,
            lent,
            Some(world_items!(self)),
            Some(&self.pack.block_registries),
            &self.damage_hooks,
        );
        let path_changes = horizon_client::mobs::collision_changes(&base, path_writes);
        // Each mob keeps its snapshot slot all tick; the first slot of an
        // id answers, as a scan would.
        let mut index = horizon_client::fast_hash::FxHashMap::<u32, u32>::default();
        let mut villager_slots = Vec::new();
        let mut solid_slots = Vec::new();
        for (i, s) in snapshot.iter().enumerate() {
            index.entry(s.id).or_insert(i as u32);
            if s.species == horizon_client::mobs::Species::Villager {
                villager_slots.push(i as u32);
            }
            if s.may_block_mobs() {
                solid_slots.push(i as u32);
            }
        }
        laps.lap("creature_prepare");
        let mut cached_difficulty: Option<((i32, i32), spawning::LocalDifficulty)> = None;
        for i in 0..mobs.len() {
            let m = &mut mobs[i];
            let (bx, bz) = (m.position.x.floor() as i32, m.position.z.floor() as i32);
            let seen_guest = if guests.is_empty() {
                None
            } else {
                let bound = bindings.get(&m.id).copied();
                let conn =
                    super::super::lan_mobs::player_for(m, base.player.as_ref(), guests, bound);
                match conn {
                    Some(conn) => {
                        bindings.insert(m.id, conn);
                    }
                    None => {
                        bindings.remove(&m.id);
                    }
                }
                conn
            };
            if !base.world.loaded(bx, bz) {
                keep.push(!m.hostile() || m.persistent);
                continue;
            }
            let nearest_player = nearest
                .nearest(m.position.x, m.position.y, m.position.z)
                .map(|p| p.position);
            // `MobDespawnEvent`, before `checkDespawn`'s own draws; one branch
            // with no listener. Cleared below: the tick may not reach the check.
            // The wither overrides `checkDespawn` without `super`, so the
            // event, patched into `Mob.checkDespawn`, never fires for it;
            // a mannequin is no `Mob` and never checks despawning.
            if mod_api::__private::listened(mod_api::HookId::MobDespawn)
                && !matches!(
                    m.species(),
                    horizon_client::mobs::Species::Wither
                        | horizon_client::mobs::Species::Mannequin
                )
                && let Some(epoch) = self.native_lifecycle.epoch()
            {
                self.native_mob_despawn(
                    self.dim.id(),
                    mod_api::MobRef::new(mod_api::EntityRef::new(epoch, m.id)),
                    &m.kind.id,
                    [m.position.x, m.position.y, m.position.z],
                    &mut m.despawn,
                );
            }
            // ServerLevel.tick: every loaded entity checks despawning, but
            // only those in entity-ticking chunks (within the simulation
            // distance) tick. The despawn check reads the nearest player
            // and the difficulty, not the player the mob sees.
            if !ticking(bx, bz) {
                let w = MobWorld {
                    others: snapshot,
                    others_grid: Some(&grid),
                    others_index: Some(&index),
                    others_villagers: Some(&villager_slots),
                    others_solid: Some(&solid_slots),
                    nearest_player,
                    player: None,
                    teams: std::sync::Arc::clone(&base.teams),
                    ..base
                };
                let kept = horizon_client::mobs::ai::despawn_check(m, &w);
                m.despawn = mod_api::NativeDespawn::default();
                if !kept {
                    died.remove(&m.id);
                    snapshot[i].alive = false;
                }
                keep.push(kept);
                continue;
            }
            let controlling_player_movement = horizon_client::mobs::equine::controlled(m)
                .then(|| {
                    base.player
                        .as_ref()
                        .filter(|p| p.riding_vehicle == Some(m.id))
                        .map(|p| p.velocity)
                        .or_else(|| {
                            guests
                                .iter()
                                .find(|(_, p)| p.riding_vehicle == Some(m.id))
                                .map(|(_, p)| p.velocity)
                        })
                })
                .flatten();
            // A mod canceled its tick: it stays as it is.
            if round.canceled(m.id) {
                m.despawn = mod_api::NativeDespawn::default();
                keep.push(true);
                continue;
            }
            // getCurrentDifficultyAt(blockPosition()): the mob's own chunk,
            // not the local player's (a server thread has no player).
            let chunk_key = (bx >> 4, bz >> 4);
            let difficulty = match cached_difficulty {
                Some((key, difficulty)) if key == chunk_key => difficulty,
                _ => {
                    let difficulty =
                        self.local_difficulty([m.position.x, m.position.y, m.position.z]);
                    cached_difficulty = Some((chunk_key, difficulty));
                    difficulty
                }
            };
            horizon_client::mobs::apply_mob_pushes(m, &mut pushes);
            horizon_client::mobs::blocks_changed(m, &path_changes);
            let from = events.len();
            // The guest's view is lent to the mob for its tick, not copied:
            // the tick only reads it (`&MobWorld`), and it goes back to the
            // guest's slot straight after. Its identity is captured with
            // it, once: the player a goal resolves is the one the tick
            // sees — a guest on a server, the game's own otherwise (#1077).
            let lent = seen_guest.and_then(|conn| guests.iter().position(|g| g.0 == conn));
            let active_player = match (seen_guest, lent) {
                (None, _) => base.player.take(),
                (Some(_), Some(k)) => spare
                    .take()
                    .map(|stand_in| std::mem::replace(&mut guests[k].1, stand_in)),
                (Some(_), None) => None,
            };
            let active_identity = match seen_guest {
                None => base.player_identity,
                Some(conn) => self.guest_identity(conn),
            };
            let mut w = MobWorld {
                others: snapshot,
                others_grid: Some(&grid),
                others_index: Some(&index),
                others_villagers: Some(&villager_slots),
                others_solid: Some(&solid_slots),
                difficulty,
                nearest_player,
                player: active_player,
                player_identity: active_identity,
                teams: std::sync::Arc::clone(&base.teams),
                ..base
            };
            w.controlling_player_movement = controlling_player_movement;
            let credit_expiring = m.ai.last_hurt_by_player == 1;
            ticked.insert(m.id);
            // `horizon-report`'s cost by mob type (tick_profile.rs).
            let sampled = super::super::tick_profile::start().map(|t| (t, mobs[i].kind.clone()));
            let kept = horizon_client::mobs::riders::tick(mobs, i, &w, &mut level, events);
            if let Some((started, kind)) = sampled {
                super::super::tick_profile::add(level_id, &kind, started);
            }
            let m = &mut mobs[i];
            m.despawn = mod_api::NativeDespawn::default();
            if credit_expiring && m.ai.last_hurt_by_player == 0 {
                player_credit.remove(&m.id);
            }
            if seen_guest.is_none() {
                base.player = w.player;
            } else if let Some(k) = lent {
                *spare = w.player.map(|v| std::mem::replace(&mut guests[k].1, v));
            }
            // A killed mob's body is removed: its effects that act on
            // death run (`LivingEntity.remove` with KILLED).
            if !kept && !m.alive() && !m.removed && !m.extra.effects.is_empty() {
                corpses.push(m.clone());
            }
            horizon_client::mobs::take_mob_pushes(events, from, &mut pushes);
            // LivingEntity.tickDeath: after twenty ticks the body is removed.
            if !kept && !m.removed && !m.alive() && m.death_time >= 20 {
                let (width, height) = (f64::from(m.width()), f64::from(m.height()));
                // 🤖 Agent: `AbstractCubeMob.remove` splits a grown cube into
                // its smaller halves when it is removed, twenty ticks after
                // the death. The parent leaves the list before the split
                // runs, so its size, position, kind and persistence are
                // captured here, as they are at the removal.
                let split = if matches!(
                    m.species(),
                    Species::Slime | Species::MagmaCube | Species::SulfurCube
                ) && m.extra.size > 1
                {
                    Some(m.clone())
                } else {
                    None
                };
                gone.push((m.id, m.position, width, height, split));
            }
            if let Some(conn) = seen_guest {
                // ShulkerBullet.finalTarget is the guest's entity: the
                // bullet homes on it while the projectiles tick, where
                // `Who::Player` is the host's own player.
                if let Some(&(_, id)) = guest_ids.iter().find(|g| g.0 == conn) {
                    for e in &mut events[from..] {
                        if let MobEvent::Projectile(p) = e
                            && let ProjectileKind::ShulkerBullet { target, .. } = &mut p.kind
                            && *target == Some(Who::Player)
                        {
                            *target = Some(Who::Mob(id));
                        }
                    }
                }
                let mut k = from;
                while k < events.len() {
                    if super::super::lan_mobs::acts_on_player(&events[k]) {
                        guest_events.push((conn, m.id, events.remove(k)));
                    } else {
                        k += 1;
                    }
                }
            }
            // Killed during its own tick (drowning, burning, falling...).
            if !m.alive() && !m.removed && died.insert(m.id) {
                dead.push(m.clone());
            }
            if !kept {
                died.remove(&m.id);
            }
            keep.push(kept);
            // ServerLevel ticks entities one after another: the ones
            // after this mob see it as it is now.
            snapshot[i] = seen(m, uuid);
            snapshot[i].alive &= kept;
            grid.update(i, snapshot[i].bounds);
            horizon_client::mobs::apply_brain_events(mobs, &mut level, events, from);
        }
        laps.lap("creature_loop");
        let mut flags = keep.into_iter();
        mobs.retain(|_| flags.next().unwrap_or(true));
        for m in mobs {
            horizon_client::mobs::apply_mob_pushes(m, &mut pushes);
        }
    }
}

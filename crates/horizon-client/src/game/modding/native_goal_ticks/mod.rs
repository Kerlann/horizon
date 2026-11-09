//! A goal reads its explicit actor loan live. Every foreign handle and narrowing
//! delegates to the real shared Host, including when another source is lifted.
//! Publications are refreshed at actual callback/apply boundaries, never relabelled
//! by cached-row access. No mod-only foreign snapshot resolution lives here.
use super::Game;
use crate::mobs::{
    self, Mob, MobWorld, Who,
    part_goals::{BehaviorOp, GoalOp},
};
use mod_api::{
    BehaviorCxBackend, EntityRef, GoalCxBackend, Host, LegacyRandom, LivingRef, MobRef,
    crash::{NativeModCall, enter_native_mod},
};

mod cached_reads;
mod live_entity;
mod live_living;
mod live_merchant;
mod live_mob;
#[cfg(test)]
mod live_read_tests;
mod live_support;
mod live_vehicle;
#[cfg(test)]
mod redirect_tests;

/// The explicit actor loan, its engine world-query context, and the real shared
/// Host. Engine AI facts in MobWorld do not form a second mod-visible reader.
pub(super) struct GoalCx<'a, 'w> {
    m: &'a mut Mob,
    w: &'a MobWorld<'w>,
    host: &'a dyn Host,
}

impl GoalCx<'_, '_> {
    fn publish_applied_write(&self) {
        if let Some(host) = self.w.goal_host {
            host.refresh_mob_read_publication(
                self.m,
                self.w,
                mod_api::EntityReadCapturePhase::AfterAppliedWrite,
            );
        }
    }

    /// OWN is only the explicit actor loan, never a disjoint live view.
    pub(super) fn is_own_entity(&self, entity: EntityRef) -> bool {
        !self.m.removed
            && self.w.goal_host.and_then(|host| host.epoch()) == Some(entity.epoch())
            && entity.id() == self.m.id
    }

    fn mob_handle(&self) -> MobRef {
        let epoch = self
            .w
            .goal_host
            .and_then(|host| host.epoch())
            .expect("a goal context is built only with a world host");
        MobRef::new(EntityRef::new(epoch, self.m.id))
    }

    pub(crate) fn lookup_entity(&self, id: u32) -> Option<EntityRef> {
        if id == self.m.id && !self.m.removed {
            Some(self.mob_handle().entity())
        } else {
            self.host.entity(id)
        }
    }
    pub(crate) fn lookup_living(&self, entity: EntityRef) -> Option<LivingRef> {
        if self.is_own_entity(entity) {
            Some(LivingRef::new(entity))
        } else {
            self.host.living(entity)
        }
    }
    pub(crate) fn lookup_mob(&self, entity: EntityRef) -> Option<MobRef> {
        if self.is_own_entity(entity) {
            Some(MobRef::new(entity))
        } else {
            self.host.mob(entity)
        }
    }
    fn position(&self, entity: EntityRef) -> Option<horizon_core::geometry::Vec3> {
        let p = if self.is_own_entity(entity) {
            return Some(self.m.position);
        } else {
            entity.position(self.host)?
        };
        Some(horizon_core::geometry::Vec3::new(p[0], p[1], p[2]))
    }
    fn r_entity_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        let m = self.entity_mob(entity)?;
        Some([m.position.x, m.position.y, m.position.z])
    }
    fn r_entity_block_position(&self, entity: EntityRef) -> Option<[i32; 3]> {
        Some(self.r_entity_position(entity)?.map(|v| v.floor() as i32))
    }
    fn r_entity_velocity(&self, entity: EntityRef) -> Option<[f64; 3]> {
        let m = self.entity_mob(entity)?;
        Some([m.velocity.x, m.velocity.y, m.velocity.z])
    }
    fn r_entity_rotation(&self, entity: EntityRef) -> Option<[f32; 2]> {
        let m = self.entity_mob(entity)?;
        Some([m.yaw, m.head_pitch])
    }
    fn r_entity_head_yaw(&self, entity: EntityRef) -> Option<f32> {
        Some(self.entity_mob(entity)?.head_yaw)
    }
    fn r_entity_bounding_box(&self, entity: EntityRef) -> Option<[[f64; 3]; 2]> {
        let b = self.entity_mob(entity)?.bounds();
        let lo = b.min_position();
        let hi = b.max_position();
        Some([[lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]])
    }
    fn r_entity_eye_height(&self, entity: EntityRef) -> Option<f64> {
        let m = self.entity_mob(entity)?;
        Some(m.eye_y() - m.position.y)
    }
    fn r_entity_eye_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        let m = self.entity_mob(entity)?;
        Some([m.position.x, m.eye_y(), m.position.z])
    }
    fn r_entity_distance_to_sqr(&self, entity: EntityRef, pos: [f64; 3]) -> Option<f64> {
        let p = self.r_entity_position(entity)?;
        let [x, y, z] = std::array::from_fn(|i| p[i] - pos[i]);
        Some(x * x + y * y + z * z)
    }
    fn r_entity_on_ground(&self, entity: EntityRef) -> Option<bool> {
        Some(self.entity_mob(entity)?.on_ground)
    }
    fn r_entity_kind(&self, entity: EntityRef) -> Option<String> {
        Some(self.entity_mob(entity)?.kind.id.clone())
    }
    fn r_living_health(&self, living: LivingRef) -> Option<f32> {
        Some(self.entity_mob(living.entity())?.health)
    }
    fn r_living_max_health(&self, living: LivingRef) -> Option<f32> {
        Some(self.entity_mob(living.entity())?.attrs.max_health)
    }
    fn r_living_alive(&self, living: LivingRef) -> Option<bool> {
        Some(self.entity_mob(living.entity())?.alive())
    }
}

impl GoalCxBackend for GoalCx<'_, '_> {
    fn host(&self) -> &dyn Host {
        self
    }

    fn mob(&self) -> MobRef {
        self.mob_handle()
    }

    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.m.random
    }

    fn move_to(&mut self, at: [f64; 3], speed: f64) -> bool {
        // The navigation handle's own boundary: numbers the pathfinder does
        // not take refuse here as they refuse there.
        if !super::native_mob_navigation::navigation_move_accepts(at, speed) {
            return false;
        }
        let applied = self.m.nav_move_to(self.w, at, speed);
        if applied {
            self.publish_applied_write();
        }
        applied
    }

    fn stop_navigation(&mut self) {
        self.m.nav_stop();
        self.publish_applied_write();
    }

    fn look_at(&mut self, at: [f64; 3]) {
        // The look control's boundary, as the navigation handle takes it.
        if super::native_mob_navigation::navigation_look_accepts(at) {
            self.m.look_at(at);
            self.publish_applied_write();
        }
    }

    fn set_target(&mut self, target: Option<LivingRef>) -> bool {
        // The write handle's own setter refusals, the one rule it asks too
        // (#1175): the brain-backed species and the three whose setter side
        // effects are not modeled.
        if self.m.refuses_set_target() {
            return false;
        }
        let who = match target {
            None => None,
            Some(target) => {
                let Some(who) = self.w.goal_host.and_then(|host| {
                    host.resolve_lent_mob_goal_target(self.m, self.w, target, true)
                }) else {
                    return false;
                };
                Some(who)
            }
        };
        let applied = matches!(
            self.m.set_ai_target(self.w, who),
            mobs::TargetChange::Set(_)
        );
        if applied {
            self.publish_applied_write();
        }
        applied
    }

    fn jump(&mut self) {
        // `JumpControl.jump`: the wish the control consumes on the mob's next
        // own tick (it assigns `jumping` from `jump` there), as the mob jump
        // handle writes it.
        self.m.ai.jump = true;
        self.publish_applied_write();
    }
}

impl BehaviorCxBackend for GoalCx<'_, '_> {
    fn host(&self) -> &dyn Host {
        self
    }

    fn mob(&self) -> MobRef {
        self.mob_handle()
    }

    fn walk_target(&self) -> Option<mod_api::BrainWalkTarget> {
        mobs::mod_behaviors::walk_target(self.m, self.w)
    }

    fn look_target(&self) -> Option<[f64; 3]> {
        mobs::mod_behaviors::look_target(self.m, self.w)
    }

    fn attack_target(&self) -> Option<LivingRef> {
        // The brain's own target read through the engine's path, the handle
        // resolved in the tick's view.
        let who = mobs::ai::brain_target(self.m)??;
        let host = self.w.goal_host?;
        let epoch = host.epoch()?;
        let id = match who {
            Who::Mob(id) => id,
            Who::Player => self.w.selected_player_identity()?.1,
        };
        let entity = EntityRef::new(epoch, id);
        self.lookup_living(entity)
    }

    fn set_brain_target(&mut self, target: Option<LivingRef>) -> bool {
        // The write handle's own setter rules, over the tick's view: the mob
        // itself, the player it sees, or another live/snapshot mob.
        let who = match target {
            None => None,
            Some(target) => {
                let Some(who) = self.w.goal_host.and_then(|host| {
                    host.resolve_lent_mob_goal_target(self.m, self.w, target, false)
                }) else {
                    return false;
                };
                Some(who)
            }
        };
        let applied = mobs::ai::set_brain_target(self.m, who);
        if applied {
            self.publish_applied_write();
        }
        applied
    }

    fn set_walk_target(&mut self, at: [i32; 3], speed: f64, close: i32) {
        let applies = mobs::mod_behaviors::has_brain(self.m.species());
        mobs::mod_behaviors::set_walk_target(self.m, at, speed, close);
        if applies {
            self.publish_applied_write();
        }
    }
}

impl mobs::part_goals::GoalHost for Game<'_> {
    fn refresh_mob_read_publication(
        &self,
        mob: &Mob,
        world: &MobWorld<'_>,
        phase: mod_api::EntityReadCapturePhase,
    ) -> bool {
        self.publish_lent_mob_reads(mob, world, phase)
    }
    fn resolve_lent_mob_goal_target(
        &self,
        actor: &Mob,
        world: &MobWorld<'_>,
        target: LivingRef,
        validate: bool,
    ) -> Option<Who> {
        Game::resolve_lent_mob_goal_target(self, actor, world, target, validate)
    }

    fn epoch(&self) -> Option<mod_api::NativeWorldEpoch> {
        self.native_lifecycle.epoch()
    }

    fn mob_item_use_remaining_lent(&self, mob: &Mob) -> Option<i32> {
        (!mob.removed).then(|| self.raw_mob_item_use_remaining(mob))
    }
    fn merchant_is_live(&self, mob: &Mob) -> bool {
        self.is_logical_server() && !mob.removed
    }

    fn merchant_offers_lent(&self, mob: &Mob) -> Option<Vec<mod_api::MerchantOffer>> {
        use super::api_values::TryIntoApi;
        self.merchant_read_lent(mob, |offers, _, _| offers.try_into_api().ok())
    }

    fn merchant_trading_player_lent(&self, mob: &Mob) -> Option<mod_api::PlayerRef> {
        let conn = self.lan_trade_conn(mob.id)?;
        self.player(self.lan_trade_player(conn)?)
    }

    fn merchant_trade_state_lent(&self, mob: &Mob) -> (i32, i32) {
        self.merchant_read_lent(mob, |_, xp, level| (xp, level))
    }

    fn mob_passengers_lent(&self, mob: &Mob) -> Option<Vec<EntityRef>> {
        self.lent_mob_passengers(mob)
    }

    fn mob_dismount_location(
        &self,
        vehicle: Option<&Mob>,
        vehicle_id: u32,
        rider: Option<&Mob>,
        passenger: EntityRef,
    ) -> Option<[f64; 3]> {
        self.scoped_mob_dismount_location(vehicle, vehicle_id, rider, passenger)
    }

    fn mob_player_controls(&self, mob: &Mob, player: u32) -> bool {
        let Some(epoch) = self.native_lifecycle.epoch() else {
            return false;
        };
        let Some(target) = self.native_entity_target(EntityRef::new(epoch, player)) else {
            return false;
        };
        matches!(
            target,
            super::commands::Target::Player | super::commands::Target::Guest(_)
        ) && self.native_player_controls(mob, target)
    }

    fn mob_tags(&self, mob: &Mob) -> Option<Vec<String>> {
        if self.remote.is_some() {
            return None;
        }
        Some(self.tags_of(super::commands::Target::Mob(mob.id)))
    }

    fn mob_name(&self, mob: &Mob, actor: &Mob, world: &MobWorld<'_>) -> String {
        let name = match self.mob_team_scoped(mob, actor, world) {
            Some(team) => team.formatted_name(mob.command_name()),
            None => mob.command_name(),
        };
        name.plain()
    }

    fn mob_added_to_level(&self, mob: &Mob) -> bool {
        self.persist
            .live
            .binary_search_by_key(&mob.id, |entry| entry.0)
            .is_ok()
    }

    fn mob_portal_cooldown(&self, mob: &Mob) -> i32 {
        self.portals.entities.get(&mob.id).map_or(0, |p| p.cooldown)
    }

    fn mob_invulnerable_to_damage(&self, mob: &Mob, damage_type: &str) -> bool {
        let source = mobs::Damage::of(Self::damage_kind(&self.motion.health.damage, damage_type));
        mob.refuses_damage(&source)
    }

    fn mob_can_use_portal(&self, mob: &Mob) -> bool {
        self.mob_portal_eligible(mob)
    }

    fn mob_is_climbing(&self, mob: &Mob) -> bool {
        self.native_mob_is_climbing(mob)
    }

    fn mob_player_credit(&self, mob: &Mob) -> Option<Option<u32>> {
        let credit = *self.last_hurt_by_player.get(&mob.id)?;
        Some(
            self.resolve_player_credit(credit)
                .and_then(|conn| self.player_entity_id_by_connection(conn))
                .and_then(|id| u32::try_from(id).ok()),
        )
    }

    fn mob_target(&self, mob: &Mob, actor: &Mob, world: &MobWorld<'_>) -> Option<LivingRef> {
        self.lent_mob_target(mob, actor, world)
    }

    fn mob_on_block(&self, mob: &Mob) -> [i32; 3] {
        let support = mob
            .on_ground
            .then(|| self.supporting_block(mob.position, mob.bounds()))
            .flatten();
        super::native_entity_motion::on_block_position(support, mob.position)
    }

    fn entities_team_damage_allowed(
        &self,
        entity: EntityRef,
        entity_mob: Option<&Mob>,
        source: EntityRef,
        source_mob: Option<&Mob>,
        actor: &Mob,
        world: &MobWorld<'_>,
    ) -> Option<bool> {
        let own = match entity_mob {
            Some(mob) => self.mob_team_scoped(mob, actor, world),
            None => self.native_target_team(self.native_entity_target(entity)?)?,
        };
        let other = match source_mob {
            Some(mob) => self.mob_team_scoped(mob, actor, world),
            None => self.native_target_team(self.native_entity_target(source)?)?,
        };
        Some(horizon_client::scoreboard::teams_allow_damage(own, other))
    }

    fn run_goal(
        &self,
        goal: &mut Box<dyn mod_api::Goal>,
        m: &mut Mob,
        w: &MobWorld<'_>,
        op: GoalOp,
        mod_id: &'static str,
        part: &'static str,
    ) -> Option<bool> {
        let call = match op {
            GoalOp::CanUse => "goal_can_use",
            GoalOp::CanContinue => "goal_can_continue",
            GoalOp::Start => "goal_start",
            GoalOp::Stop => "goal_stop",
            GoalOp::Tick => "goal_tick",
        };
        self.publish_lent_mob_reads(m, w, mod_api::EntityReadCapturePhase::BeforeCallback);
        let mut gcx = GoalCx { m, w, host: self };
        let mut context = mod_api::GoalContext::new(&mut gcx);
        let _guard = enter_native_mod(NativeModCall {
            id: mod_id,
            hook: call,
            part: Some(part),
        });
        Some(match op {
            GoalOp::CanUse => goal.can_use(&mut context),
            GoalOp::CanContinue => goal.can_continue(&mut context),
            GoalOp::Start => {
                goal.start(&mut context);
                true
            }
            GoalOp::Stop => {
                goal.stop(&mut context);
                true
            }
            GoalOp::Tick => {
                goal.tick(&mut context);
                true
            }
        })
    }

    fn run_behavior(
        &self,
        behavior: &mut Box<dyn mod_api::Behavior>,
        m: &mut Mob,
        w: &MobWorld<'_>,
        op: BehaviorOp,
        mod_id: &'static str,
        part: &'static str,
    ) -> Option<bool> {
        let call = match op {
            BehaviorOp::CheckExtraStartConditions => "behavior_check_extra_start_conditions",
            BehaviorOp::Start => "behavior_start",
            BehaviorOp::CanStillUse => "behavior_can_still_use",
            BehaviorOp::Tick => "behavior_tick",
            BehaviorOp::Stop => "behavior_stop",
        };
        // The same view a goal runs on (#824): one boundary, the brain's
        // memories typed on top.
        self.publish_lent_mob_reads(m, w, mod_api::EntityReadCapturePhase::BeforeCallback);
        let mut gcx = GoalCx { m, w, host: self };
        let mut context = mod_api::BehaviorCx::new(&mut gcx);
        let _guard = enter_native_mod(NativeModCall {
            id: mod_id,
            hook: call,
            part: Some(part),
        });
        Some(match op {
            BehaviorOp::CheckExtraStartConditions => {
                behavior.check_extra_start_conditions(&mut context)
            }
            BehaviorOp::Start => {
                behavior.start(&mut context);
                true
            }
            BehaviorOp::CanStillUse => behavior.can_still_use(&mut context),
            BehaviorOp::Tick => {
                behavior.tick(&mut context);
                true
            }
            BehaviorOp::Stop => {
                behavior.stop(&mut context);
                true
            }
        })
    }
}

impl Game<'_> {
    /// Dispatch a target change against the mob tick's scoped read view. The
    /// game has lent its mob list to the tick, so using `Game` itself as the
    /// host here would make the actor and other mob handles unresolvable.
    pub(super) fn native_mob_change_target_in_tick(
        &self,
        mob: &mut Mob,
        world: &MobWorld<'_>,
        is_brain: bool,
        target: Option<Who>,
    ) -> mobs::TargetChange {
        let vanilla = mobs::TargetChange::Set(target);
        if !mod_api::__private::listened(mod_api::HookId::LivingChangeTarget) {
            return vanilla;
        }
        let Some(epoch) = self.native_lifecycle.epoch() else {
            return vanilla;
        };
        let original = match target {
            None => None,
            Some(Who::Mob(id)) => Some(LivingRef::new(EntityRef::new(epoch, id))),
            Some(Who::Player) => {
                let Some((_, id)) = world.selected_player_identity() else {
                    return vanilla;
                };
                Some(LivingRef::new(EntityRef::new(epoch, id)))
            }
        };
        let entity = MobRef::new(EntityRef::new(epoch, mob.id));
        self.publish_lent_mob_reads(mob, world, mod_api::EntityReadCapturePhase::BeforeCallback);
        let context = GoalCx {
            m: mob,
            w: world,
            host: self,
        };
        let mut new_target = original;
        let mut cancel = false;
        super::native_hooks::NativeHookHost { host: &context }.native_living_change_target(
            mod_api::__private::LivingChangeTargetCall {
                mob: entity,
                original_target: original,
                new_target: &mut new_target,
                is_brain,
                cancel: &mut cancel,
            },
        );
        if cancel {
            return mobs::TargetChange::Keep;
        }
        if new_target == original {
            return vanilla;
        }
        let Some(target) = new_target else {
            return mobs::TargetChange::Set(None);
        };
        self.resolve_lent_mob_goal_target(context.m, world, target, false)
            .map_or(mobs::TargetChange::Keep, |who| {
                mobs::TargetChange::Set(Some(who))
            })
    }
}

pub(super) mod generated {
    // OWN reads use direct loan adapters; foreign reads use the real Host.
    include!(concat!(env!("OUT_DIR"), "/mod_api_goal_view_generated.rs"));
}

#[cfg(test)]
#[path = "native_goal_ticks_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "native_behavior_tests.rs"]
mod behavior_tests;

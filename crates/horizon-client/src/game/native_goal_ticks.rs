//! The mod goal context of a mob's tick (arismoko/horizon#1077): the reads a
//! goal makes through `GoalContext::host` answer the ticking mob itself, the
//! player the tick sees and the tick's snapshot of the other entities — the
//! entity lookups and snapshot-backed reads are the view's own (generated
//! below), unavailable entity fields answer their typed refusal, and intact
//! non-entity reads (levels, block entities, tags, recipes, settings) forward to the game
//! host the view carries. The writes a goal makes apply to the mob in hand
//! through the engine's own paths (`nav_move_to`, `ai.target`), and the write
//! handles a `ServerHost` takes do not exist here.
use super::Game;
use crate::mobs::{self, Mob, MobWorld, Seen, TargetInfo, Who, part_goals::GoalOp};
use horizon_core::geometry::Axis;
use mod_api::{
    EntityRef, GoalCxBackend, Host, LegacyRandom, LivingRef, MobRef, PlayerRef,
    crash::{NativeModCall, enter_native_mod},
};

/// What a mod goal runs with: the mob in hand, the tick's `MobWorld` (the
/// [`mobs::part_goals::GoalHost`], the snapshot and the player view ride on
/// it) and the game host behind the tick, for the reads that are the level's
/// own. The writes apply to the mob itself.
pub(super) struct GoalCx<'a, 'w> {
    m: &'a mut Mob,
    w: &'a MobWorld<'w>,
    host: &'a dyn Host,
    backend: &'a dyn mod_api::Backend,
}

/// What a handle names in the goal view: the ticking mob itself, the player
/// the tick sees, another mob in the snapshot, or the separately simulated
/// dragon. The ticking mob's own id never consults the taken list — the
/// exception that makes any mob handle resolve in its own tick (#29's limit).
enum Resolved<'a> {
    Own,
    Player,
    Seen(&'a Seen),
    Dragon(TargetInfo),
}

impl GoalCx<'_, '_> {
    /// Resolve `entity` in the tick's view, or None for a stale or foreign
    /// handle or an entity the snapshot does not carry. The selected real
    /// player's identity travels with its view, including guest turns.
    fn resolve(&self, entity: EntityRef) -> Option<Resolved<'_>> {
        let host = self.w.goal_host?;
        if host.epoch() != Some(entity.epoch()) {
            return None;
        }
        let id = entity.id();
        if id == self.m.id {
            return Some(Resolved::Own);
        }
        if self
            .w
            .player
            .as_ref()
            .is_some_and(|p| p.entity_id == Some(id))
        {
            return Some(Resolved::Player);
        }
        if let Some((dragon, info)) = self.w.dragon
            && dragon == id
        {
            return Some(Resolved::Dragon(info));
        }
        let others = self.w.others;
        let seen = match self.w.others_index {
            Some(index) => index.get(&id).map(|&slot| &others[slot as usize]),
            None => others.iter().find(|s| s.id == id),
        }?;
        Some(Resolved::Seen(seen))
    }

    // --- the entity lookups the generated Host impl forwards to ----------

    /// `Host::entity`: the id resolves in the tick's view — the ticking mob,
    /// the player it sees, a mob of the snapshot, the dragon — or none.
    pub(crate) fn lookup_entity(&self, id: u32) -> Option<EntityRef> {
        let host = self.w.goal_host?;
        let epoch = host.epoch()?;
        match self.resolve(EntityRef::new(epoch, id))? {
            Resolved::Own | Resolved::Player | Resolved::Seen(_) | Resolved::Dragon(_) => {
                Some(EntityRef::new(epoch, id))
            }
        }
    }

    /// `Host::living`: the handle resolves in the tick's view.
    pub(crate) fn lookup_living(&self, entity: EntityRef) -> Option<LivingRef> {
        self.resolve(entity).map(|_| LivingRef::new(entity))
    }

    /// `Host::mob`: the handle resolves in the tick's view.
    pub(crate) fn lookup_mob(&self, entity: EntityRef) -> Option<MobRef> {
        match self.resolve(entity)? {
            Resolved::Player => None,
            _ => Some(MobRef::new(entity)),
        }
    }

    /// `Host::player`: the player the tick sees, by uuid.
    pub(crate) fn lookup_player(&self, uuid: u128) -> Option<PlayerRef> {
        let host = self.w.goal_host?;
        let player = self.w.player.as_ref()?;
        let id = player.entity_id?;
        let epoch = host.epoch()?;
        (uuid == horizon_client::lan::playerdata::ints_uuid(player.uuid))
            .then(|| mod_api::__private::game::player_ref(EntityRef::new(epoch, id), uuid))
    }

    /// `Host::item_entity`: no item entity is in the tick's view.
    pub(crate) fn lookup_item_entity(&self, entity: EntityRef) -> Option<mod_api::ItemEntityRef> {
        let _ = entity;
        None
    }

    /// `Host::block_entity`: block entities live in the world's chunks, which
    /// the tick does not take out — the game host answers for them.
    pub(crate) fn lookup_block_entity(
        &self,
        level: mod_api::LevelRef,
        pos: [i32; 3],
    ) -> Option<mod_api::BlockEntityRef> {
        self.host.block_entity(level, pos)
    }

    /// The entity's position, or None where unresolvable.
    fn position(&self, entity: EntityRef) -> Option<horizon_core::geometry::Vec3> {
        match self.resolve(entity)? {
            Resolved::Own => Some(self.m.position),
            Resolved::Player => self.w.player.as_ref().map(|p| p.position),
            Resolved::Seen(seen) => Some(seen.position),
            Resolved::Dragon(info) => Some(info.position),
        }
    }

    // --- the snapshot-backed reads the generated view forwards to ----------

    fn r_entity_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        self.position(entity).map(|p| [p.x, p.y, p.z])
    }

    fn r_entity_block_position(&self, entity: EntityRef) -> Option<[i32; 3]> {
        self.position(entity)
            .map(|p| [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32])
    }

    fn r_entity_velocity(&self, entity: EntityRef) -> Option<[f64; 3]> {
        match self.resolve(entity)? {
            Resolved::Own => Some(self.m.velocity),
            Resolved::Player => self.w.player.as_ref().map(|p| p.velocity),
            Resolved::Seen(seen) => Some(seen.velocity),
            Resolved::Dragon(_) => None,
        }
        .map(|v| [v.x, v.y, v.z])
    }

    fn r_entity_rotation(&self, entity: EntityRef) -> Option<[f32; 2]> {
        match self.resolve(entity)? {
            // `getYRot` and `getXRot`: the snapshot carries no pitch and a
            // player's view no pitch either, so only the mob in hand answers.
            Resolved::Own => Some([self.m.yaw, self.m.head_pitch]),
            _ => None,
        }
    }

    fn r_entity_head_yaw(&self, entity: EntityRef) -> Option<f32> {
        match self.resolve(entity)? {
            Resolved::Own => Some(self.m.head_yaw),
            Resolved::Player => self.w.player.as_ref().map(|p| p.yaw),
            // `Seen::yaw` is the body's, not the head's.
            Resolved::Seen(_) | Resolved::Dragon(_) => None,
        }
    }

    fn r_entity_bounding_box(&self, entity: EntityRef) -> Option<[[f64; 3]; 2]> {
        let bounds = match self.resolve(entity)? {
            Resolved::Own => self.m.bounds(),
            Resolved::Player => self.w.player.as_ref().map(|p| p.bounds)?,
            Resolved::Seen(seen) => seen.bounds,
            Resolved::Dragon(info) => info.bounds,
        };
        let axes = [Axis::X, Axis::Y, Axis::Z];
        Some([axes.map(|a| bounds.min(a)), axes.map(|a| bounds.max(a))])
    }

    fn r_entity_eye_height(&self, entity: EntityRef) -> Option<f64> {
        match self.resolve(entity)? {
            Resolved::Own => Some(self.m.eye_y() - self.m.position.y),
            Resolved::Player => self.w.player.as_ref().map(|p| p.eye[1] - p.position.y),
            Resolved::Seen(seen) => Some(seen.eye_y - seen.position.y),
            Resolved::Dragon(info) => Some(info.eye_y - info.position.y),
        }
    }

    fn r_entity_eye_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        match self.resolve(entity)? {
            Resolved::Own => {
                let y = self.m.eye_y();
                Some([self.m.position.x, y, self.m.position.z])
            }
            Resolved::Player => self.w.player.as_ref().map(|p| p.eye),
            Resolved::Seen(seen) => Some([seen.position.x, seen.eye_y, seen.position.z]),
            Resolved::Dragon(info) => Some([info.position.x, info.eye_y, info.position.z]),
        }
    }

    fn r_entity_distance_to_sqr(&self, entity: EntityRef, pos: [f64; 3]) -> Option<f64> {
        self.position(entity).map(|p| {
            (p.x - pos[0]).mul_add(p.x - pos[0], 0.0)
                + (p.y - pos[1]).mul_add(p.y - pos[1], 0.0)
                + (p.z - pos[2]).mul_add(p.z - pos[2], 0.0)
        })
    }

    fn r_entity_on_ground(&self, entity: EntityRef) -> Option<bool> {
        match self.resolve(entity)? {
            Resolved::Own => Some(self.m.on_ground),
            // Not carried by the snapshot or a player's view.
            _ => None,
        }
    }

    fn r_entity_kind(&self, entity: EntityRef) -> Option<String> {
        match self.resolve(entity)? {
            // The snapshot carries the species, not its type id.
            Resolved::Own => Some(self.m.kind.id.clone()),
            _ => None,
        }
    }

    fn r_living_health(&self, living: LivingRef) -> Option<f32> {
        match self.resolve(living.entity())? {
            Resolved::Own => Some(self.m.health),
            Resolved::Seen(seen) => Some(seen.health),
            // A player's view carries no health.
            _ => None,
        }
    }

    fn r_living_max_health(&self, living: LivingRef) -> Option<f32> {
        match self.resolve(living.entity())? {
            Resolved::Own => Some(self.m.attrs.max_health),
            // Not carried by the snapshot.
            _ => None,
        }
    }

    fn r_living_alive(&self, living: LivingRef) -> Option<bool> {
        match self.resolve(living.entity())? {
            Resolved::Own => Some(self.m.alive()),
            Resolved::Player => self.w.player.as_ref().map(|p| p.alive),
            Resolved::Seen(seen) => Some(seen.alive),
            Resolved::Dragon(info) => Some(info.alive),
        }
    }
}

impl GoalCxBackend for GoalCx<'_, '_> {
    fn host(&self) -> &dyn Host {
        self
    }

    fn mob(&self) -> MobRef {
        let host = self
            .w
            .goal_host
            .expect("a goal context is built only where a goal host is");
        let epoch = host.epoch().expect("a goal host is built only in a world");
        MobRef::new(EntityRef::new(epoch, self.m.id))
    }

    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.m.random
    }

    fn move_to(&mut self, at: [f64; 3], speed: f64) -> bool {
        speed.is_finite()
            && super::native_mob_navigation::safe_control_position(at)
            && self.m.nav_move_to(self.w, at, speed)
    }

    fn stop_navigation(&mut self) {
        self.m.nav_stop();
    }

    fn look_at(&mut self, at: [f64; 3]) {
        if super::native_mob_navigation::safe_control_position(at) {
            self.m.look_at(at);
        }
    }

    fn set_target(&mut self, target: Option<LivingRef>) -> bool {
        // The write handle's rules, as far as the tick sees them: the
        // brain-backed species and the three whose setter side effects are
        // not modeled refuse, an unresolvable target refuses.
        if !super::native_mob_refs::target_setter_available(self.m) {
            return false;
        }
        let who = match target {
            None => None,
            Some(living) => match self.resolve(living.entity()) {
                Some(Resolved::Own) => {
                    if !super::native_mob_refs::can_target_mob(self.m, (&*self.m).into(), self.w) {
                        return false;
                    }
                    Some(Who::Mob(self.m.id))
                }
                Some(Resolved::Player) => {
                    let player = self.w.player.as_ref().expect("resolved player");
                    let facts = super::native_mob_refs::PlayerFacts {
                        mode: if player.spectator {
                            3
                        } else if player.creative {
                            1
                        } else {
                            0
                        },
                        alive: player.alive,
                        invulnerable: player.invulnerable,
                        uuid: player.uuid,
                    };
                    if !super::native_mob_refs::can_target_player(
                        self.m,
                        facts,
                        self.w.difficulty.base.into(),
                    ) {
                        return false;
                    }
                    Some(Who::Player)
                }
                Some(Resolved::Seen(seen)) => {
                    if !super::native_mob_refs::can_target_mob(self.m, seen.into(), self.w) {
                        return false;
                    }
                    Some(Who::Mob(seen.id))
                }
                Some(Resolved::Dragon(_)) | None => return false,
            },
        };
        self.m.ai.target = who;
        true
    }

    fn jump(&mut self) {
        self.m.ai.jump = true;
    }
}

impl mobs::part_goals::GoalHost for Game<'_> {
    fn epoch(&self) -> Option<mod_api::NativeWorldEpoch> {
        self.native_lifecycle.epoch()
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
        let mut gcx = GoalCx {
            m,
            w,
            host: self,
            backend: self,
        };
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
}

pub(super) mod generated {
    // The mob tick's read view (#1077), as the game generates it: the
    // Entity reads use the snapshot or refuse unavailable fields; intact
    // non-entity reads forward to the game backend.
    include!(concat!(env!("OUT_DIR"), "/mod_api_goal_view_generated.rs"));
}

#[cfg(test)]
#[path = "native_goal_ticks_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "native_goal_ticks_regressions.rs"]
mod regressions;
